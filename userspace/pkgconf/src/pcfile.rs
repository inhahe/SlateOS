//! Parsing a single `.pc` file.
//!
//! The format is two kinds of line interleaved:
//!
//! ```text
//! prefix=/usr                       # a variable assignment
//! libdir=${prefix}/lib              # ... which may reference earlier ones
//!
//! Name: zlib                        # a keyword field
//! Version: 1.3.1
//! Requires.private: libfoo >= 2.0
//! Libs: -L${libdir} -lz
//! ```
//!
//! Two details drive the whole design of this module:
//!
//! * **Substitution happens at parse time, in file order.** A `${var}` is
//!   resolved against the variables defined *above* it, so forward references
//!   do not work.  That is the reference behaviour and `.pc` files in the wild
//!   depend on it (they redefine `prefix` above the fields that use it).
//! * **Variables are the extension point.** `--define-variable=prefix=/opt`
//!   has to beat the file's own `prefix=`, which is how a relocated package is
//!   queried, so overrides are consulted before the file's own bindings and an
//!   overridden assignment is skipped rather than recorded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::version::CmpOp;

/// One entry of a `Requires:` / `Requires.private:` / `Conflicts:` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    pub name: String,
    /// `None` means "any version".
    pub constraint: Option<(CmpOp, String)>,
}

impl Dep {
    /// Render back to `.pc` syntax, for `--print-requires` and error text.
    #[must_use]
    pub fn display(&self) -> String {
        match &self.constraint {
            Some((op, v)) => format!("{} {} {}", self.name, op.as_str(), v),
            None => self.name.clone(),
        }
    }
}

/// A parsed `.pc` file.  All strings have already had `${...}` substituted.
#[derive(Clone, Debug, Default)]
pub struct PcFile {
    /// Where it was loaded from; `--validate` and error messages quote this.
    pub path: PathBuf,
    /// The lookup key — the file stem, *not* the `Name:` field.  pkg-config
    /// resolves `Requires: foo` by opening `foo.pc`; `Name:` is a human label
    /// and is routinely different (`Name: zlib` in `zlib.pc`, but
    /// `Name: GLib` in `glib-2.0.pc`).
    pub key: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    pub requires: Vec<Dep>,
    pub requires_private: Vec<Dep>,
    pub conflicts: Vec<Dep>,
    pub cflags: String,
    pub libs: String,
    pub libs_private: String,
    /// Variables in definition order, so `--print-variables` is stable and
    /// reproducible rather than hash-ordered.
    pub vars: BTreeMap<String, String>,
}

/// Everything that can go wrong reading a `.pc` file.  Kept as data rather
/// than a pre-formatted string so callers can decide whether to print it
/// (`--print-errors`) and where (`--errors-to-stdout`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// A line was neither blank, a comment, an assignment, nor `Key: value`.
    Malformed { line: usize, text: String },
    /// The same variable was assigned twice.  The reference tools treat this
    /// as fatal because the second value silently winning is a common and
    /// very confusing packaging bug.
    DuplicateVariable { line: usize, name: String },
    /// A `Requires:` entry had an operator with no version after it.
    BadRequires { field: String, text: String },
}

impl ParseError {
    #[must_use]
    pub fn message(&self, path: &Path) -> String {
        let p = path.display();
        match self {
            Self::Malformed { line, text } => {
                format!("{p}:{line}: parse error: couldn't parse line '{text}'")
            }
            Self::DuplicateVariable { line, name } => {
                format!("{p}:{line}: duplicate definition of variable '{name}'")
            }
            Self::BadRequires { field, text } => {
                format!("{p}: could not parse {field} field: '{text}'")
            }
        }
    }
}

/// Guard against `a=${b}` / `b=${a}`.  Substitution is iterative rather than
/// recursive (values are expanded when assigned, so a cycle can only form
/// through the caller-supplied override map), but the bound keeps a
/// pathological input from looping.
const MAX_SUBST_DEPTH: usize = 64;

/// Expand `${name}` references in `text`.
///
/// `$$` is a literal `$`.  An undefined variable expands to the empty string
/// and is reported through `missing` so the caller can warn — matching the
/// reference tools, which warn but carry on, because erroring out here would
/// break the many `.pc` files that reference an optional variable.
fn substitute(
    text: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
    missing: &mut Vec<String>,
) -> String {
    let mut cur = text.to_string();
    for _ in 0..MAX_SUBST_DEPTH {
        let mut out = String::with_capacity(cur.len());
        let mut chars = cur.chars().peekable();
        let mut expanded_any = false;
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            match chars.peek() {
                Some('$') => {
                    chars.next();
                    out.push('$');
                }
                Some('{') => {
                    chars.next();
                    let mut name = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == '}' {
                            closed = true;
                            break;
                        }
                        name.push(c);
                    }
                    if !closed {
                        // Unterminated `${`: emit it literally rather than
                        // swallowing the rest of the line.
                        out.push_str("${");
                        out.push_str(&name);
                        continue;
                    }
                    match lookup(&name) {
                        Some(v) => {
                            out.push_str(&v);
                            expanded_any = true;
                        }
                        None => {
                            if !missing.contains(&name) {
                                missing.push(name);
                            }
                        }
                    }
                }
                // A bare `$` not followed by `{` is literal.
                _ => out.push('$'),
            }
        }
        cur = out;
        if !expanded_any {
            break;
        }
    }
    cur
}

/// Parse a `Requires:`-style list: entries separated by commas and/or
/// whitespace, each `name` or `name op version`.
fn parse_deps(field: &str, text: &str) -> Result<Vec<Dep>, ParseError> {
    let mut tokens: Vec<&str> = Vec::new();
    for chunk in text.split(',') {
        tokens.extend(chunk.split_whitespace());
    }

    let mut out: Vec<Dep> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let Some(&name) = tokens.get(i) else { break };
        i += 1;
        // An operator may be glued to the name or the version (`foo>=1.2`),
        // but the overwhelmingly common form is space-separated, and the
        // reference parser only accepts the separated form.  Accept both:
        // a glued form is unambiguous and rejecting it helps nobody.
        let mut dep = Dep {
            name: name.to_string(),
            constraint: None,
        };
        if let Some(op) = tokens.get(i).and_then(|t| CmpOp::parse(t)) {
            i += 1;
            let Some(&ver) = tokens.get(i) else {
                return Err(ParseError::BadRequires {
                    field: field.to_string(),
                    text: text.to_string(),
                });
            };
            i += 1;
            dep.constraint = Some((op, ver.to_string()));
        } else if let Some((n, rest)) = split_glued_operator(name) {
            let (op, ver) = rest;
            dep.name = n;
            dep.constraint = Some((op, ver));
        }
        if dep.name.is_empty() {
            return Err(ParseError::BadRequires {
                field: field.to_string(),
                text: text.to_string(),
            });
        }
        out.push(dep);
    }
    Ok(out)
}

/// Recognise `foo>=1.2` written without spaces.
fn split_glued_operator(tok: &str) -> Option<(String, (CmpOp, String))> {
    // Longest operators first so `>=` is not read as `>`.
    for op_text in ["<=", ">=", "!=", "==", "<", ">", "="] {
        if let Some(pos) = tok.find(op_text) {
            if pos == 0 {
                continue;
            }
            let name = tok.get(..pos)?;
            let ver = tok.get(pos + op_text.len()..)?;
            if ver.is_empty() {
                continue;
            }
            let op = CmpOp::parse(op_text)?;
            return Some((name.to_string(), (op, ver.to_string())));
        }
    }
    None
}

/// Parse a package list given on the *command line*.
///
/// The same grammar as `Requires:`, because the command line uses it: both
/// `pkgconf --exists 'zlib >= 1.2'` (one quoted argument) and
/// `pkgconf --exists zlib '>=' 1.2` (three) reach here as one joined string.
///
/// # Errors
///
/// A ready-to-print message if a constraint is missing its version.
pub fn parse_dep_list(text: &str) -> Result<Vec<Dep>, String> {
    parse_deps("package list", text)
        .map_err(|_| format!("Ignoring incomplete version constraint in package list '{text}'"))
}

/// The result of parsing: the package plus any non-fatal complaints.
#[derive(Debug)]
pub struct Parsed {
    pub pkg: PcFile,
    /// Names of `${...}` references that had no binding.
    pub undefined_vars: Vec<String>,
}

/// Parse the contents of a `.pc` file.
///
/// `key` is the lookup name (the file stem).  `path` is used for `pcfiledir`
/// and for error messages.  `overrides` are `--define-variable` bindings plus
/// the built-in `pc_sysrootdir`/`pc_top_builddir`; they win over the file's
/// own assignments.
///
/// # Errors
///
/// Returns the first [`ParseError`] encountered.  Parsing stops there, as the
/// remainder of a malformed file cannot be trusted.
pub fn parse(
    key: &str,
    path: &Path,
    text: &str,
    overrides: &BTreeMap<String, String>,
) -> Result<Parsed, ParseError> {
    let mut pkg = PcFile {
        path: path.to_path_buf(),
        key: key.to_string(),
        ..PcFile::default()
    };

    // `pcfiledir` lets a relocatable package express its own prefix as
    // `${pcfiledir}/../..`; it is a built-in and cannot be assigned.
    let pcfiledir = pcfiledir_of(path);

    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut undefined: Vec<String> = Vec::new();

    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Decide assignment vs. field by which delimiter comes first: a value
        // may legitimately contain the other character (`Libs: -Wl,-rpath=/x`
        // has an `=`, and `prefix=/opt/a:b` has a `:`).
        let eq = line.find('=');
        let colon = line.find(':');
        let is_assignment = match (eq, colon) {
            (Some(e), Some(c)) => e < c,
            (Some(_), None) => true,
            _ => false,
        };

        if is_assignment {
            let Some(e) = eq else { continue };
            let name = line.get(..e).unwrap_or("").trim().to_string();
            let value = line.get(e + 1..).unwrap_or("").trim();
            if name.is_empty() {
                return Err(ParseError::Malformed {
                    line: line_no,
                    text: raw.to_string(),
                });
            }
            if overrides.contains_key(&name) {
                // A --define-variable beats the file; skip the assignment
                // entirely so the override is what later lines see too.
                continue;
            }
            if vars.contains_key(&name) {
                return Err(ParseError::DuplicateVariable {
                    line: line_no,
                    name,
                });
            }
            let expanded = substitute(
                value,
                &|n: &str| resolve(n, overrides, &vars, &pcfiledir),
                &mut undefined,
            );
            vars.insert(name, expanded);
            continue;
        }

        let Some(c) = colon else {
            return Err(ParseError::Malformed {
                line: line_no,
                text: raw.to_string(),
            });
        };
        let key_text = line.get(..c).unwrap_or("").trim();
        let value_raw = line.get(c + 1..).unwrap_or("").trim();
        let value = substitute(
            value_raw,
            &|n: &str| resolve(n, overrides, &vars, &pcfiledir),
            &mut undefined,
        );

        match key_text {
            "Name" => pkg.name = value,
            "Description" => pkg.description = value,
            "URL" => pkg.url = value,
            "Version" => pkg.version = value,
            "Requires" => pkg.requires = parse_deps("Requires", &value)?,
            "Requires.private" => {
                pkg.requires_private = parse_deps("Requires.private", &value)?;
            }
            "Conflicts" => pkg.conflicts = parse_deps("Conflicts", &value)?,
            // Both spellings occur in the wild; the reference parser accepts
            // each, so a file using `CFlags:` must not silently lose its flags.
            "Cflags" | "CFlags" => pkg.cflags = value,
            "Libs" => pkg.libs = value,
            "Libs.private" => pkg.libs_private = value,
            // Unknown keywords are ignored rather than fatal: pkgconf-only
            // extensions (`Provides:`, `Copyright:`) appear in real files and
            // rejecting them would make those packages unusable.
            _ => {}
        }
    }

    pkg.vars = vars;
    Ok(Parsed {
        pkg,
        undefined_vars: undefined,
    })
}

/// The directory holding a `.pc` file, as `${pcfiledir}` sees it.
///
/// Backslashes are folded to `/` because the value is substituted into
/// `-I`/`-L` flags that a compiler on SlateOS reads, and SlateOS paths use
/// forward slashes; on a Windows *host* — which is where this crate's tests
/// run — `Path::parent` would otherwise hand back `C:\x\y` and produce flags
/// no target compiler could parse.
fn pcfiledir_of(path: &Path) -> String {
    path.parent().map_or_else(
        || ".".to_string(),
        |p| p.to_string_lossy().replace('\\', "/"),
    )
}

/// Variable lookup order: command-line overrides, then the file's own
/// bindings, then the built-in `pcfiledir`.
fn resolve(
    name: &str,
    overrides: &BTreeMap<String, String>,
    vars: &BTreeMap<String, String>,
    pcfiledir: &str,
) -> Option<String> {
    if let Some(v) = overrides.get(name) {
        return Some(v.clone());
    }
    if let Some(v) = vars.get(name) {
        return Some(v.clone());
    }
    if name == "pcfiledir" {
        return Some(pcfiledir.to_string());
    }
    None
}

impl PcFile {
    /// Look up a variable for `--variable=NAME`.
    ///
    /// `pcfiledir` answers here as well as inside `${...}` substitution.  It
    /// has to: a relocatable package writes `prefix=${pcfiledir}/../..`, and
    /// the build system that consumes it then asks for `pcfiledir` directly to
    /// compute its own install paths.  Resolving it during parsing but not
    /// here would make `${pcfiledir}` work and `--variable=pcfiledir` return
    /// the empty string, which is the kind of split that only shows up in
    /// somebody else's build failure.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<String> {
        if let Some(v) = self.vars.get(name) {
            return Some(v.clone());
        }
        // A virtual package has no file, so it has no `pcfiledir`; answering
        // "." there would hand a relocatable-path expression a value that
        // silently means "wherever the build happened to be run from".
        if name == "pcfiledir" && !self.path.as_os_str().is_empty() {
            return Some(pcfiledir_of(&self.path));
        }
        None
    }

    /// Every name `--variable=` would resolve, in the order
    /// `--print-variables` prints them.
    ///
    /// pkgconf lists `pcfiledir`; pkg-config 0.29 does not.  We follow pkgconf,
    /// because a name that `--variable=` answers and `--print-variables` omits
    /// is a worse lie than either behaviour on its own.  A file that assigns
    /// `pcfiledir` itself is listed once, from its own bindings.
    #[must_use]
    pub fn var_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.vars.keys().map(String::as_str).collect();
        if !self.vars.contains_key("pcfiledir") && !self.path.as_os_str().is_empty() {
            names.push("pcfiledir");
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseError, parse};
    use crate::version::CmpOp;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn parse_ok(text: &str) -> super::PcFile {
        parse(
            "test",
            Path::new("/usr/lib/pkgconfig/test.pc"),
            text,
            &BTreeMap::new(),
        )
        .expect("parse should succeed")
        .pkg
    }

    const ZLIB: &str = "\
prefix=/usr
exec_prefix=${prefix}
libdir=${exec_prefix}/lib
includedir=${prefix}/include

Name: zlib
Description: zlib compression library
Version: 1.3.1
Libs: -L${libdir} -lz
Cflags: -I${includedir}
";

    #[test]
    fn fields_and_variables_are_read() {
        let p = parse_ok(ZLIB);
        assert_eq!(p.name, "zlib");
        assert_eq!(p.version, "1.3.1");
        assert_eq!(p.description, "zlib compression library");
        assert_eq!(p.var("prefix").as_deref(), Some("/usr"));
    }

    #[test]
    fn variables_expand_transitively_in_file_order() {
        let p = parse_ok(ZLIB);
        assert_eq!(p.var("libdir").as_deref(), Some("/usr/lib"));
        assert_eq!(p.libs, "-L/usr/lib -lz");
        assert_eq!(p.cflags, "-I/usr/include");
    }

    #[test]
    fn forward_references_do_not_resolve() {
        // Matches the reference tools: substitution is at assignment time.
        let p = parse_ok("a=${b}\nb=2\nName: t\nVersion: 1\n");
        assert_eq!(p.var("a").as_deref(), Some(""));
        assert_eq!(p.var("b").as_deref(), Some("2"));
    }

    #[test]
    fn an_undefined_variable_expands_empty_and_is_reported() {
        let parsed = parse(
            "test",
            Path::new("/x/test.pc"),
            "Name: t\nVersion: 1\nCflags: -I${nope}/include\n",
            &BTreeMap::new(),
        )
        .expect("parse");
        assert_eq!(parsed.pkg.cflags, "-I/include");
        assert_eq!(parsed.undefined_vars, vec!["nope".to_string()]);
    }

    #[test]
    fn dollar_dollar_is_a_literal_dollar() {
        let p = parse_ok("Name: t\nVersion: 1\nCflags: -DX=$$HOME\n");
        assert_eq!(p.cflags, "-DX=$HOME");
    }

    #[test]
    fn a_bare_dollar_is_literal() {
        let p = parse_ok("Name: t\nVersion: 1\nLibs: -Wl,-rpath,$ORIGIN\n");
        assert_eq!(p.libs, "-Wl,-rpath,$ORIGIN");
    }

    #[test]
    fn an_unterminated_brace_is_emitted_literally() {
        let p = parse_ok("Name: t\nVersion: 1\nCflags: -I${oops\n");
        assert_eq!(p.cflags, "-I${oops");
    }

    #[test]
    fn pcfiledir_is_built_in() {
        let p = parse_ok("prefix=${pcfiledir}/../..\nName: t\nVersion: 1\n");
        assert_eq!(p.var("prefix").as_deref(), Some("/usr/lib/pkgconfig/../.."));
    }

    #[test]
    fn overrides_beat_the_files_own_assignment() {
        let mut ov = BTreeMap::new();
        ov.insert("prefix".to_string(), "/opt".to_string());
        let p = parse("test", Path::new("/x/test.pc"), ZLIB, &ov)
            .expect("parse")
            .pkg;
        // The file's `prefix=/usr` line is skipped, so everything downstream
        // of it relocates too.
        assert_eq!(p.libs, "-L/opt/lib -lz");
        assert_eq!(p.cflags, "-I/opt/include");
    }

    #[test]
    fn a_duplicate_variable_is_an_error() {
        let err = parse(
            "test",
            Path::new("/x/test.pc"),
            "prefix=/usr\nprefix=/opt\nName: t\n",
            &BTreeMap::new(),
        )
        .expect_err("duplicate should fail");
        assert_eq!(
            err,
            ParseError::DuplicateVariable {
                line: 2,
                name: "prefix".to_string()
            }
        );
    }

    #[test]
    fn a_duplicate_that_is_overridden_is_not_an_error() {
        // With --define-variable=prefix=..., both assignments are skipped, so
        // the file no longer trips the duplicate check.
        let mut ov = BTreeMap::new();
        ov.insert("prefix".to_string(), "/opt".to_string());
        let p = parse(
            "test",
            Path::new("/x/test.pc"),
            "prefix=/usr\nprefix=/other\nName: t\nVersion: 1\n",
            &ov,
        )
        .expect("parse")
        .pkg;
        assert_eq!(p.name, "t");
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let p = parse_ok("# a comment\n\n   \nName: t\nVersion: 1\n");
        assert_eq!(p.name, "t");
    }

    #[test]
    fn both_cflags_spellings_are_accepted() {
        assert_eq!(parse_ok("Name: t\nCFlags: -DX\n").cflags, "-DX");
        assert_eq!(parse_ok("Name: t\nCflags: -DY\n").cflags, "-DY");
    }

    #[test]
    fn unknown_keywords_are_ignored_not_fatal() {
        let p = parse_ok("Name: t\nProvides: t = 1\nCopyright: nobody\nVersion: 1\n");
        assert_eq!(p.version, "1");
    }

    #[test]
    fn a_value_containing_an_equals_sign_is_still_a_field() {
        let p = parse_ok("Name: t\nLibs: -Wl,-rpath=/opt/lib -lz\n");
        assert_eq!(p.libs, "-Wl,-rpath=/opt/lib -lz");
    }

    #[test]
    fn a_value_containing_a_colon_is_still_an_assignment() {
        let p = parse_ok("searchpath=/a:/b\nName: t\n");
        assert_eq!(p.var("searchpath").as_deref(), Some("/a:/b"));
    }

    #[test]
    fn requires_accepts_commas_and_whitespace() {
        let p = parse_ok("Name: t\nRequires: a, b >= 2.0 c\n");
        assert_eq!(p.requires.len(), 3);
        assert_eq!(p.requires[0].name, "a");
        assert_eq!(p.requires[0].constraint, None);
        assert_eq!(p.requires[1].name, "b");
        assert_eq!(
            p.requires[1].constraint,
            Some((CmpOp::Ge, "2.0".to_string()))
        );
        assert_eq!(p.requires[2].name, "c");
    }

    #[test]
    fn requires_accepts_a_glued_operator() {
        let p = parse_ok("Name: t\nRequires: b>=2.0\n");
        assert_eq!(p.requires[0].name, "b");
        assert_eq!(
            p.requires[0].constraint,
            Some((CmpOp::Ge, "2.0".to_string()))
        );
    }

    #[test]
    fn a_dangling_operator_in_requires_is_an_error() {
        let err = parse(
            "test",
            Path::new("/x/test.pc"),
            "Name: t\nRequires: b >=\n",
            &BTreeMap::new(),
        )
        .expect_err("dangling operator should fail");
        assert!(matches!(err, ParseError::BadRequires { .. }));
    }

    #[test]
    fn requires_private_and_conflicts_parse_the_same_way() {
        let p = parse_ok("Name: t\nRequires.private: x >= 1\nConflicts: y < 2\n");
        assert_eq!(p.requires_private[0].name, "x");
        assert_eq!(p.conflicts[0].name, "y");
        assert_eq!(
            p.conflicts[0].constraint,
            Some((CmpOp::Lt, "2".to_string()))
        );
    }

    #[test]
    fn dep_display_round_trips() {
        let p = parse_ok("Name: t\nRequires: a, b >= 2.0\n");
        assert_eq!(p.requires[0].display(), "a");
        assert_eq!(p.requires[1].display(), "b >= 2.0");
    }

    #[test]
    fn a_line_that_is_neither_is_a_parse_error() {
        let err = parse(
            "test",
            Path::new("/x/test.pc"),
            "Name: t\nthis is nonsense\n",
            &BTreeMap::new(),
        )
        .expect_err("nonsense should fail");
        assert!(matches!(err, ParseError::Malformed { line: 2, .. }));
    }

    #[test]
    fn error_messages_name_the_file_and_line() {
        let err = ParseError::Malformed {
            line: 7,
            text: "junk".to_string(),
        };
        let msg = err.message(Path::new("/usr/lib/pkgconfig/t.pc"));
        assert!(msg.contains("t.pc"), "{msg}");
        assert!(msg.contains(":7:"), "{msg}");
    }
}
