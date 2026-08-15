#![deny(clippy::all, clippy::pedantic)]
// `run` is a dispatcher over ~20 mutually exclusive output modes.  Splitting it
// would scatter the interactions between them (which modes suppress errors,
// which imply a dependency walk) across several functions without making any
// one of them clearer.
#![allow(clippy::too_many_lines)]

//! pkgconf — SlateOS package configuration tool.
//!
//! Answers "what flags do I need to compile and link against library X?" by
//! reading the `.pc` files X installed, following its declared dependencies,
//! and emitting a de-duplicated command line.  This is the interface every
//! autotools/CMake/Meson build on the system uses to find libraries, so its
//! output has to be right and its failure modes have to be legible.
//!
//! Two personalities, selected from `argv[0]`:
//!
//! * `pkgconf` / `pkg-config` — the package query tool.  The two are identical;
//!   `pkg-config` exists because build systems hard-code that name.
//! * `bomtool` — prints a name/version/URL manifest for a dependency closure.
//!   Deliberately *not* SPDX: emitting a document that claims to be SPDX
//!   without the licence data to fill it in would be worse than emitting none.
//!
//! ## Relationship to the reference implementations
//!
//! Behaviour follows pkg-config 0.29 / pkgconf 2.x closely enough that build
//! scripts written against them work unchanged: the same search-path rules and
//! environment variables, the same `rpmvercmp` version ordering, the same
//! `No package 'x' found` diagnostic that configure scripts grep for, and the
//! same rule that `Requires.private` contributes to `--cflags` always but to
//! `--libs` only under `--static`.
//!
//! One deliberate divergence, in [`flags::dedup`]: duplicate flags are removed
//! globally rather than only when adjacent.  See that function for why, and
//! why it cannot break a link that would otherwise have worked.

mod flags;
mod pcfile;
mod store;
mod version;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;

use flags::{Flag, FlagKind};
use pcfile::{Dep, PcFile};
use store::{LookupError, Store};
use version::CmpOp;

/// Reported by `--version`, and compared against by
/// `--atleast-pkgconfig-version`, which `PKG_PROG_PKG_CONFIG` calls.
const PKGCONF_VERSION: &str = "2.1.0";

const USAGE: &str = "\
usage: pkgconf [OPTIONS] [PACKAGES...]

Query the compiler and linker flags a package needs.  A PACKAGE is a name
resolved to <name>.pc on the search path, optionally followed by a version
constraint (e.g. 'zlib >= 1.2'), or a literal path to a .pc file.

Output selection:
  --cflags                 compiler flags (-I and everything else)
  --cflags-only-I          only -I flags
  --cflags-only-other      compiler flags other than -I
  --libs                   linker flags (-L, -l and everything else)
  --libs-only-l            only -l flags
  --libs-only-L            only -L flags
  --libs-only-other        linker flags other than -L and -l
  --static                 flags for static linking (adds Libs.private)
  --modversion             the version of each named package
  --print-provides         'name = version' for each named package
  --print-requires         each named package's Requires
  --print-requires-private each named package's Requires.private
  --variable=NAME          the value of a .pc variable
  --print-variables        every variable a package defines
  --list-all               every package on the search path

Existence and version checks (silent; the exit status is the answer):
  --exists                 0 if every named package resolves
  --atleast-version=V      0 if every named package is at least V
  --exact-version=V        0 if every named package is exactly V
  --max-version=V          0 if every named package is at most V
  --atleast-pkgconfig-version=V   0 if this tool is at least V
  --validate               parse the named .pc files without resolving them

Behaviour:
  --define-variable=N=V    override a .pc variable before expansion
  --with-path=DIR         search DIR as well
  --keep-system-cflags     do not drop -I/usr/include
  --keep-system-libs       do not drop -L/usr/lib
  --print-errors           report errors even in check mode
  --silence-errors         report no errors
  --errors-to-stdout       send error text to stdout
  --version                this tool's version
  -h, --help               this message

Environment:
  PKG_CONFIG_PATH          extra search directories, searched first
  PKG_CONFIG_LIBDIR        replaces the default search directories
  PKG_CONFIG_SYSROOT_DIR   prefixed onto -I and -L paths
  PKG_CONFIG_TOP_BUILD_DIR value of the ${pc_top_builddir} variable
  PKG_CONFIG_ALLOW_SYSTEM_CFLAGS  same as --keep-system-cflags
  PKG_CONFIG_ALLOW_SYSTEM_LIBS    same as --keep-system-libs
";

/// Everything the command line asked for.
#[derive(Default, Debug)]
struct Options {
    help: bool,
    show_version: bool,
    list_all: bool,
    modversion: bool,
    exists: bool,
    validate: bool,
    print_variables: bool,
    print_requires: bool,
    print_requires_private: bool,
    print_provides: bool,
    variable: Option<String>,
    cflags: bool,
    cflags_only_i: bool,
    cflags_only_other: bool,
    libs: bool,
    libs_only_l: bool,
    libs_only_big_l: bool,
    libs_only_other: bool,
    static_mode: bool,
    /// `None` = not specified; the default depends on whether this is a
    /// check-only invocation.
    print_errors: Option<bool>,
    errors_to_stdout: bool,
    keep_system_cflags: bool,
    keep_system_libs: bool,
    version_checks: Vec<(CmpOp, String)>,
    atleast_pkgconfig_version: Option<String>,
    defines: BTreeMap<String, String>,
    with_paths: Vec<String>,
    /// Non-option words, later joined and parsed as a package list.
    words: Vec<String>,
    bom: bool,
}

impl Options {
    /// True when the invocation is purely a yes/no question, in which case
    /// errors are silent by default — a `configure` script probing for an
    /// optional library must not spray diagnostics for the ones it lacks.
    fn is_check_only(&self) -> bool {
        self.exists || !self.version_checks.is_empty() || self.atleast_pkgconfig_version.is_some()
    }

    fn wants_cflags(&self) -> bool {
        self.cflags || self.cflags_only_i || self.cflags_only_other
    }

    fn wants_libs(&self) -> bool {
        self.libs || self.libs_only_l || self.libs_only_big_l || self.libs_only_other
    }
}

/// Pull the value of an option that takes one, accepting both `--opt=value`
/// and `--opt value`.
fn take_value(
    arg: &str,
    name: &str,
    it: &mut std::vec::IntoIter<String>,
) -> Result<String, String> {
    if let Some(rest) = arg.strip_prefix(&format!("{name}=")) {
        return Ok(rest.to_string());
    }
    it.next()
        .ok_or_else(|| format!("Option '{name}' requires an argument"))
}

/// Parse the command line.
///
/// # Errors
///
/// A human-readable usage message for an unknown option or a missing operand.
fn parse_args(argv: Vec<String>) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = argv.into_iter();
    // Everything after `--` is a package name, even if it looks like an option.
    let mut literal = false;

    while let Some(arg) = it.next() {
        if literal || !arg.starts_with('-') || arg == "-" {
            o.words.push(arg);
            continue;
        }
        let head = arg.split('=').next().unwrap_or(&arg).to_string();
        match head.as_str() {
            "--" => literal = true,
            "-h" | "--help" => o.help = true,
            "--version" => o.show_version = true,
            "--list-all" => o.list_all = true,
            "--modversion" => o.modversion = true,
            "--exists" => o.exists = true,
            "--validate" => o.validate = true,
            "--print-variables" => o.print_variables = true,
            "--print-requires" => o.print_requires = true,
            "--print-requires-private" => o.print_requires_private = true,
            "--print-provides" => o.print_provides = true,
            "--cflags" => o.cflags = true,
            "--cflags-only-I" => o.cflags_only_i = true,
            "--cflags-only-other" => o.cflags_only_other = true,
            "--libs" => o.libs = true,
            "--libs-only-l" => o.libs_only_l = true,
            "--libs-only-L" => o.libs_only_big_l = true,
            "--libs-only-other" => o.libs_only_other = true,
            "--static" => o.static_mode = true,
            "--print-errors" => o.print_errors = Some(true),
            "--silence-errors" => o.print_errors = Some(false),
            "--errors-to-stdout" => o.errors_to_stdout = true,
            "--keep-system-cflags" => o.keep_system_cflags = true,
            "--keep-system-libs" => o.keep_system_libs = true,
            "--variable" => o.variable = Some(take_value(&arg, "--variable", &mut it)?),
            "--atleast-version" => {
                o.version_checks
                    .push((CmpOp::Ge, take_value(&arg, "--atleast-version", &mut it)?));
            }
            "--exact-version" => {
                o.version_checks
                    .push((CmpOp::Eq, take_value(&arg, "--exact-version", &mut it)?));
            }
            "--max-version" => {
                o.version_checks
                    .push((CmpOp::Le, take_value(&arg, "--max-version", &mut it)?));
            }
            "--atleast-pkgconfig-version" => {
                o.atleast_pkgconfig_version =
                    Some(take_value(&arg, "--atleast-pkgconfig-version", &mut it)?);
            }
            "--define-variable" => {
                let v = take_value(&arg, "--define-variable", &mut it)?;
                let (name, value) = v.split_once('=').ok_or_else(|| {
                    format!("--define-variable argument '{v}' is not of the form NAME=VALUE")
                })?;
                o.defines.insert(name.to_string(), value.to_string());
            }
            "--with-path" => o.with_paths.push(take_value(&arg, "--with-path", &mut it)?),
            // Accepted and ignored: these affect only how the reference tools
            // cache or report, and silently accepting them keeps third-party
            // build scripts working rather than failing on an unknown option.
            // The `bomtool` personality's output mode.  It has a command-line
            // spelling so that the personality dispatch in `main` is a single
            // argv insertion rather than a second code path through `run`.
            "--bom" => o.bom = true,
            "--uninstalled" | "--debug" | "--short-errors" | "--dont-define-prefix" => {}
            other => return Err(format!("Unknown option '{other}'")),
        }
    }
    Ok(o)
}

/// Which flags survive the `--*-only-*` filters.
fn filter_cflags(o: &Options, all: Vec<Flag>) -> Vec<Flag> {
    if o.cflags {
        return all;
    }
    all.into_iter()
        .filter(|f| {
            (o.cflags_only_i && f.kind == FlagKind::IncludePath)
                || (o.cflags_only_other && f.kind != FlagKind::IncludePath)
        })
        .collect()
}

fn filter_libs(o: &Options, all: Vec<Flag>) -> Vec<Flag> {
    if o.libs {
        return all;
    }
    all.into_iter()
        .filter(|f| {
            (o.libs_only_l && f.kind == FlagKind::LibName)
                || (o.libs_only_big_l && f.kind == FlagKind::LibPath)
                || (o.libs_only_other
                    && f.kind != FlagKind::LibName
                    && f.kind != FlagKind::LibPath)
        })
        .collect()
}

/// Where diagnostics go, and whether they are emitted at all.
struct Reporter {
    print: bool,
    to_stdout: bool,
}

impl Reporter {
    fn emit(&self, msg: &str, out: &mut String, err: &mut String) {
        if !self.print {
            return;
        }
        let sink = if self.to_stdout { out } else { err };
        sink.push_str(msg);
        if !msg.ends_with('\n') {
            sink.push('\n');
        }
    }
}

/// Run the tool with an injected environment and captured output, so the whole
/// command line surface is testable without a process.
fn run(
    argv: Vec<String>,
    env: &dyn Fn(&str) -> Option<String>,
    out: &mut String,
    err: &mut String,
) -> i32 {
    let o = match parse_args(argv) {
        Ok(o) => o,
        Err(msg) => {
            err.push_str(&msg);
            err.push('\n');
            return 1;
        }
    };

    if o.help {
        out.push_str(USAGE);
        return 0;
    }
    if o.show_version {
        out.push_str(PKGCONF_VERSION);
        out.push('\n');
        return 0;
    }
    if let Some(want) = &o.atleast_pkgconfig_version {
        return i32::from(!CmpOp::Ge.satisfied_by(version::compare(PKGCONF_VERSION, want)));
    }

    let reporter = Reporter {
        print: o.print_errors.unwrap_or(!o.is_check_only()),
        to_stdout: o.errors_to_stdout,
    };

    // Built-in variables that come from the environment rather than the file.
    let sysroot = env("PKG_CONFIG_SYSROOT_DIR").unwrap_or_default();
    let mut overrides = o.defines.clone();
    overrides
        .entry("pc_sysrootdir".to_string())
        .or_insert_with(|| {
            if sysroot.is_empty() {
                "/".to_string()
            } else {
                sysroot.clone()
            }
        });
    overrides
        .entry("pc_top_builddir".to_string())
        .or_insert_with(|| {
            env("PKG_CONFIG_TOP_BUILD_DIR").unwrap_or_else(|| "$(top_builddir)".to_string())
        });

    let dirs: Vec<PathBuf> = Store::search_dirs_from_env(&o.with_paths, env);
    let mut store = Store::new(dirs, overrides);

    if o.list_all {
        for pkg in store.list_all() {
            out.push_str(&format!(
                "{:<24} {} - {}\n",
                pkg.key, pkg.name, pkg.description
            ));
        }
        return 0;
    }

    // Package specs may be split across argv entries (`foo >= 1.2` is three
    // words) or quoted into one; joining and re-splitting handles both.
    let joined = o.words.join(" ");
    let roots: Vec<Dep> = match pcfile::parse_dep_list(&joined) {
        Ok(d) => d,
        Err(msg) => {
            reporter.emit(&msg, out, err);
            return 1;
        }
    };
    if roots.is_empty() {
        reporter.emit("Must specify package names on the command line", out, err);
        return 1;
    }

    // --validate deliberately stops at parsing: it answers "is this .pc file
    // well-formed", which must be answerable before its dependencies exist.
    if o.validate {
        let mut rc = 0;
        for dep in &roots {
            match store.load(&dep.name) {
                Ok(_) => {}
                Err(e) => {
                    reporter.emit(&e.message(), out, err);
                    rc = 1;
                }
            }
        }
        for w in &store.warnings {
            reporter.emit(w, out, err);
        }
        return rc;
    }

    // Load the requested packages themselves, then the full closure.  Doing
    // the roots first means a bad *root* is reported as such rather than as a
    // failure somewhere in a dependency walk.
    let mut requested: Vec<Rc<PcFile>> = Vec::new();
    for dep in &roots {
        match store.load(&dep.name) {
            Ok(p) => requested.push(p),
            Err(e) => {
                reporter.emit(&e.message(), out, err);
                return 1;
            }
        }
    }

    // Command-line version constraints (`--atleast-version` and friends) apply
    // to every named package.
    for (op, want) in &o.version_checks {
        for pkg in &requested {
            if !op.satisfied_by(version::compare(&pkg.version, want)) {
                reporter.emit(
                    &LookupError::VersionMismatch {
                        dep: Dep {
                            name: pkg.key.clone(),
                            constraint: Some((*op, want.clone())),
                        },
                        have: pkg.version.clone(),
                        required_by: None,
                    }
                    .message(),
                    out,
                    err,
                );
                return 1;
            }
        }
    }

    // The closure is walked even for --exists: a package whose Requires cannot
    // be satisfied is not usable, and reporting it as present would only move
    // the failure to the compiler.
    let closure = match store.resolve(&roots, true) {
        Ok(c) => c,
        Err(e) => {
            reporter.emit(&e.message(), out, err);
            return 1;
        }
    };

    for w in &store.warnings {
        reporter.emit(w, out, err);
    }

    if o.bom {
        for pkg in &closure {
            let url = if pkg.url.is_empty() { "-" } else { &pkg.url };
            out.push_str(&format!("{}\t{}\t{}\n", pkg.key, pkg.version, url));
        }
        return 0;
    }

    if o.exists {
        return 0;
    }

    if o.modversion {
        for pkg in &requested {
            out.push_str(&pkg.version);
            out.push('\n');
        }
    }
    if o.print_provides {
        for pkg in &requested {
            out.push_str(&format!("{} = {}\n", pkg.key, pkg.version));
        }
    }
    if o.print_requires {
        for pkg in &requested {
            for d in &pkg.requires {
                out.push_str(&d.display());
                out.push('\n');
            }
        }
    }
    if o.print_requires_private {
        for pkg in &requested {
            for d in &pkg.requires_private {
                out.push_str(&d.display());
                out.push('\n');
            }
        }
    }
    if let Some(name) = &o.variable {
        for pkg in &requested {
            out.push_str(pkg.var(name).unwrap_or(""));
            out.push('\n');
        }
    }
    if o.print_variables {
        for pkg in &requested {
            for name in pkg.vars.keys() {
                out.push_str(name);
                out.push('\n');
            }
        }
    }

    let mut line: Vec<Flag> = Vec::new();
    if o.wants_cflags() {
        // Requires.private always contributes cflags: compiling against a
        // public header that includes a private dependency's header needs that
        // dependency's -I, whether or not we link statically.
        let mut cf: Vec<Flag> = Vec::new();
        for pkg in &closure {
            cf.extend(flags::parse_fragment(&pkg.cflags));
        }
        cf = flags::dedup(cf);
        if !(o.keep_system_cflags || env("PKG_CONFIG_ALLOW_SYSTEM_CFLAGS").is_some()) {
            cf = flags::strip_system_includes(cf);
        }
        cf = flags::apply_sysroot(cf, &sysroot);
        line.extend(filter_cflags(&o, cf));
    }
    if o.wants_libs() {
        // Libs.private, by contrast, is only correct under --static: for a
        // shared link the dynamic linker pulls those in itself, and naming
        // them here over-links every consumer.
        let lib_closure = if o.static_mode {
            closure.clone()
        } else {
            match store.resolve(&roots, false) {
                Ok(c) => c,
                Err(e) => {
                    reporter.emit(&e.message(), out, err);
                    return 1;
                }
            }
        };
        let mut lf: Vec<Flag> = Vec::new();
        for pkg in &lib_closure {
            lf.extend(flags::parse_fragment(&pkg.libs));
            if o.static_mode {
                lf.extend(flags::parse_fragment(&pkg.libs_private));
            }
        }
        lf = flags::dedup(lf);
        if !(o.keep_system_libs || env("PKG_CONFIG_ALLOW_SYSTEM_LIBS").is_some()) {
            lf = flags::strip_system_libdirs(lf);
        }
        lf = flags::apply_sysroot(lf, &sysroot);
        line.extend(filter_libs(&o, lf));
    }
    if o.wants_cflags() || o.wants_libs() {
        out.push_str(&flags::render(&line));
        out.push('\n');
    }

    0
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();

    // argv[0] selects the personality.  Strip the directory and any `.exe`
    // that a host-side build leaves on it.
    let personality = {
        let full = argv.first().map_or("pkgconf", String::as_str);
        let base = full.rsplit(['/', '\\']).next().unwrap_or(full);
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };

    let mut opts: Vec<String> = argv.into_iter().skip(1).collect();
    if personality == "bomtool" {
        opts.insert(0, "--bom".to_string());
    }

    let mut out = String::new();
    let mut err = String::new();
    let env = |k: &str| std::env::var(k).ok();
    let code = run(opts, &env, &mut out, &mut err);

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    // A closed stdout (`pkgconf --list-all | head`) is not an error worth a
    // diagnostic — the exit status still reports it.
    let _ = lock.write_all(out.as_bytes());
    let _ = lock.flush();
    if !err.is_empty() {
        let _ = std::io::stderr().write_all(err.as_bytes());
    }
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{parse_args, run};
    use std::path::PathBuf;

    /// A scratch directory of `.pc` files plus a fixed environment, so every
    /// test below exercises the real command-line surface end to end.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let mut dir = std::env::temp_dir();
            dir.push(format!("slateos-pkgconf-cli-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create fixture dir");
            Self { dir }
        }

        fn write(&self, name: &str, body: &str) {
            std::fs::write(self.dir.join(format!("{name}.pc")), body).expect("write .pc");
        }

        /// Run with the fixture as the *only* search directory, so nothing the
        /// host machine happens to have installed can affect the result.
        fn run(&self, args: &[&str]) -> (i32, String, String) {
            self.run_env(args, &[])
        }

        fn run_env(&self, args: &[&str], extra: &[(&str, &str)]) -> (i32, String, String) {
            let libdir = self.dir.to_string_lossy().into_owned();
            let extra: Vec<(String, String)> = extra
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect();
            let env = move |k: &str| -> Option<String> {
                if k == "PKG_CONFIG_LIBDIR" {
                    return Some(libdir.clone());
                }
                extra
                    .iter()
                    .find(|(ek, _)| ek == k)
                    .map(|(_, v)| v.clone())
            };
            let mut out = String::new();
            let mut err = String::new();
            let code = run(
                args.iter().map(|s| (*s).to_string()).collect(),
                &env,
                &mut out,
                &mut err,
            );
            (code, out, err)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const ZLIB: &str = "\
prefix=/usr
libdir=${prefix}/lib
includedir=${prefix}/include
Name: zlib
Description: zlib compression library
Version: 1.3.1
URL: https://zlib.net
Libs: -L${libdir} -lz
Cflags: -I${includedir}/zlib
";

    fn png_fixture(tag: &str) -> Fixture {
        let f = Fixture::new(tag);
        f.write("zlib", ZLIB);
        f.write(
            "libpng",
            "\
prefix=/usr
Name: libpng
Description: PNG library
Version: 1.6.40
Requires: zlib >= 1.2
Libs: -L${prefix}/lib -lpng16
Libs.private: -lm
Cflags: -I${prefix}/include/libpng16
",
        );
        f
    }

    // ── argument parsing ────────────────────────────────────────────────

    #[test]
    fn option_values_accept_both_spellings() {
        let a = parse_args(vec!["--variable=prefix".into()]).expect("parse");
        let b = parse_args(vec!["--variable".into(), "prefix".into()]).expect("parse");
        assert_eq!(a.variable.as_deref(), Some("prefix"));
        assert_eq!(b.variable.as_deref(), Some("prefix"));
    }

    #[test]
    fn a_missing_option_argument_is_an_error() {
        assert!(parse_args(vec!["--variable".into()]).is_err());
    }

    #[test]
    fn an_unknown_option_is_an_error() {
        let e = parse_args(vec!["--frobnicate".into()]).expect_err("unknown");
        assert!(e.contains("--frobnicate"), "{e}");
    }

    #[test]
    fn double_dash_makes_the_rest_package_names() {
        let o = parse_args(vec!["--".into(), "--weird-name".into()]).expect("parse");
        assert_eq!(o.words, vec!["--weird-name".to_string()]);
    }

    #[test]
    fn define_variable_requires_name_equals_value() {
        assert!(parse_args(vec!["--define-variable=prefix".into()]).is_err());
        let o = parse_args(vec!["--define-variable=prefix=/opt".into()]).expect("parse");
        assert_eq!(o.defines.get("prefix").map(String::as_str), Some("/opt"));
    }

    #[test]
    fn harmless_reference_options_are_accepted_and_ignored() {
        let o = parse_args(vec!["--uninstalled".into(), "--short-errors".into()])
            .expect("should be accepted");
        assert!(o.words.is_empty());
    }

    // ── basic queries ───────────────────────────────────────────────────

    #[test]
    fn help_and_version_succeed() {
        let f = Fixture::new("help");
        let (code, out, _) = f.run(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("--cflags"), "{out}");

        let (code, out, _) = f.run(&["--version"]);
        assert_eq!(code, 0);
        assert_eq!(out.trim(), super::PKGCONF_VERSION);
    }

    #[test]
    fn no_package_names_is_an_error() {
        let f = Fixture::new("nonames");
        let (code, _, err) = f.run(&["--cflags"]);
        assert_eq!(code, 1);
        assert!(err.contains("Must specify package names"), "{err}");
    }

    #[test]
    fn cflags_and_libs_come_from_the_pc_file() {
        let f = Fixture::new("basic");
        f.write("zlib", ZLIB);
        let (code, out, _) = f.run(&["--cflags", "zlib"]);
        assert_eq!(code, 0);
        assert_eq!(out.trim(), "-I/usr/include/zlib");

        let (code, out, _) = f.run(&["--libs", "zlib"]);
        assert_eq!(code, 0);
        // -L/usr/lib is a default linker directory and is dropped.
        assert_eq!(out.trim(), "-lz");
    }

    #[test]
    fn cflags_and_libs_together_are_one_line() {
        let f = Fixture::new("oneline");
        f.write("zlib", ZLIB);
        let (_, out, _) = f.run(&["--cflags", "--libs", "zlib"]);
        assert_eq!(out, "-I/usr/include/zlib -lz\n");
    }

    #[test]
    fn a_missing_package_fails_with_the_standard_diagnostic() {
        let f = Fixture::new("missing");
        let (code, _, err) = f.run(&["--cflags", "nope"]);
        assert_eq!(code, 1);
        assert!(err.contains("No package 'nope' found"), "{err}");
    }

    #[test]
    fn exists_is_silent_in_both_directions() {
        let f = Fixture::new("exists");
        f.write("zlib", ZLIB);
        let (code, out, err) = f.run(&["--exists", "zlib"]);
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
        let (code, out, err) = f.run(&["--exists", "nope"]);
        assert_eq!(code, 1);
        assert!(out.is_empty() && err.is_empty(), "out={out:?} err={err:?}");
    }

    #[test]
    fn print_errors_overrides_check_mode_silence() {
        let f = Fixture::new("printerrors");
        let (code, _, err) = f.run(&["--exists", "--print-errors", "nope"]);
        assert_eq!(code, 1);
        assert!(err.contains("No package 'nope' found"), "{err}");
    }

    #[test]
    fn silence_errors_suppresses_a_normal_failure() {
        let f = Fixture::new("silence");
        let (code, out, err) = f.run(&["--cflags", "--silence-errors", "nope"]);
        assert_eq!(code, 1);
        assert!(out.is_empty() && err.is_empty(), "out={out:?} err={err:?}");
    }

    #[test]
    fn errors_to_stdout_redirects_the_diagnostic() {
        let f = Fixture::new("errstdout");
        let (code, out, err) = f.run(&["--cflags", "--errors-to-stdout", "nope"]);
        assert_eq!(code, 1);
        assert!(out.contains("No package 'nope' found"), "{out}");
        assert!(err.is_empty(), "{err}");
    }

    #[test]
    fn modversion_and_provides() {
        let f = Fixture::new("modver");
        f.write("zlib", ZLIB);
        let (_, out, _) = f.run(&["--modversion", "zlib"]);
        assert_eq!(out, "1.3.1\n");
        let (_, out, _) = f.run(&["--print-provides", "zlib"]);
        assert_eq!(out, "zlib = 1.3.1\n");
    }

    // ── version constraints ─────────────────────────────────────────────

    #[test]
    fn atleast_version_compares_numerically() {
        let f = Fixture::new("atleast");
        f.write("zlib", ZLIB);
        assert_eq!(f.run(&["--atleast-version=1.2", "zlib"]).0, 0);
        assert_eq!(f.run(&["--atleast-version=1.3.1", "zlib"]).0, 0);
        assert_eq!(f.run(&["--atleast-version=1.4", "zlib"]).0, 1);
        // 1.3.1 vs 1.10: the numeric segment rule, not string ordering.
        assert_eq!(f.run(&["--atleast-version=1.10", "zlib"]).0, 1);
    }

    #[test]
    fn exact_and_max_version() {
        let f = Fixture::new("exactmax");
        f.write("zlib", ZLIB);
        assert_eq!(f.run(&["--exact-version=1.3.1", "zlib"]).0, 0);
        assert_eq!(f.run(&["--exact-version=1.3", "zlib"]).0, 1);
        assert_eq!(f.run(&["--max-version=2.0", "zlib"]).0, 0);
        assert_eq!(f.run(&["--max-version=1.0", "zlib"]).0, 1);
    }

    #[test]
    fn an_inline_constraint_on_the_command_line_is_honoured() {
        let f = Fixture::new("inline");
        f.write("zlib", ZLIB);
        assert_eq!(f.run(&["--exists", "zlib >= 1.2"]).0, 0);
        assert_eq!(f.run(&["--exists", "zlib", ">=", "1.2"]).0, 0);
        assert_eq!(f.run(&["--exists", "zlib >= 9.0"]).0, 1);
    }

    #[test]
    fn atleast_pkgconfig_version_answers_about_this_tool() {
        let f = Fixture::new("selfver");
        assert_eq!(f.run(&["--atleast-pkgconfig-version=0.9.0"]).0, 0);
        assert_eq!(f.run(&["--atleast-pkgconfig-version=99.0"]).0, 1);
    }

    // ── dependency resolution ───────────────────────────────────────────

    #[test]
    fn requires_contribute_flags_in_link_order() {
        let f = png_fixture("deps");
        let (code, out, _) = f.run(&["--libs", "libpng"]);
        assert_eq!(code, 0);
        // libpng before zlib: a static linker resolves left to right.
        assert_eq!(out.trim(), "-lpng16 -lz");

        let (_, out, _) = f.run(&["--cflags", "libpng"]);
        assert_eq!(out.trim(), "-I/usr/include/libpng16 -I/usr/include/zlib");
    }

    #[test]
    fn an_unsatisfied_transitive_constraint_fails_and_names_the_requirer() {
        let f = Fixture::new("transfail");
        f.write("a", "Name: a\nVersion: 1\nRequires: b >= 9.0\n");
        f.write("b", "Name: b\nVersion: 1.0\n");
        let (code, _, err) = f.run(&["--cflags", "a"]);
        assert_eq!(code, 1);
        assert!(err.contains("Package 'a' requires"), "{err}");
    }

    #[test]
    fn libs_private_appears_only_under_static() {
        let f = png_fixture("static");
        let (_, out, _) = f.run(&["--libs", "libpng"]);
        assert!(!out.contains("-lm"), "{out}");
        let (_, out, _) = f.run(&["--libs", "--static", "libpng"]);
        assert!(out.contains("-lm"), "{out}");
    }

    #[test]
    fn requires_private_always_contributes_cflags_but_libs_only_under_static() {
        let f = Fixture::new("reqpriv");
        f.write(
            "app",
            "Name: app\nVersion: 1\nRequires.private: helper\nLibs: -lapp\nCflags: -I/app\n",
        );
        f.write(
            "helper",
            "Name: helper\nVersion: 1\nLibs: -lhelper\nCflags: -I/helper\n",
        );
        let (_, out, _) = f.run(&["--cflags", "app"]);
        assert_eq!(out.trim(), "-I/app -I/helper");
        let (_, out, _) = f.run(&["--libs", "app"]);
        assert_eq!(out.trim(), "-lapp");
        let (_, out, _) = f.run(&["--libs", "--static", "app"]);
        assert_eq!(out.trim(), "-lapp -lhelper");
    }

    #[test]
    fn duplicate_flags_across_packages_collapse() {
        let f = Fixture::new("dupflags");
        f.write("top", "Name: top\nVersion: 1\nRequires: l, r\nCflags: -I/common\n");
        f.write("l", "Name: l\nVersion: 1\nCflags: -I/common\nLibs: -ll -lm\n");
        f.write("r", "Name: r\nVersion: 1\nCflags: -I/common\nLibs: -lr -lm\n");
        let (_, out, _) = f.run(&["--cflags", "top"]);
        assert_eq!(out.trim(), "-I/common");
        let (_, out, _) = f.run(&["--libs", "top"]);
        // -lm survives once, at its *last* position, so both -ll and -lr still
        // precede it and a static link resolves.
        assert_eq!(out.trim(), "-ll -lr -lm");
    }

    // ── filters ─────────────────────────────────────────────────────────

    #[test]
    fn only_filters_select_flag_classes() {
        let f = Fixture::new("filters");
        f.write(
            "x",
            "Name: x\nVersion: 1\nCflags: -I/inc -DFOO\nLibs: -L/opt/lib -lx -pthread\n",
        );
        assert_eq!(f.run(&["--cflags-only-I", "x"]).1.trim(), "-I/inc");
        assert_eq!(f.run(&["--cflags-only-other", "x"]).1.trim(), "-DFOO");
        assert_eq!(f.run(&["--libs-only-l", "x"]).1.trim(), "-lx");
        assert_eq!(f.run(&["--libs-only-L", "x"]).1.trim(), "-L/opt/lib");
        assert_eq!(f.run(&["--libs-only-other", "x"]).1.trim(), "-pthread");
    }

    // ── variables ───────────────────────────────────────────────────────

    #[test]
    fn variable_lookup_and_listing() {
        let f = Fixture::new("vars");
        f.write("zlib", ZLIB);
        assert_eq!(f.run(&["--variable=prefix", "zlib"]).1, "/usr\n");
        assert_eq!(f.run(&["--variable=libdir", "zlib"]).1, "/usr/lib\n");
        // An undefined variable is an empty line, not an error.
        let (code, out, err) = f.run(&["--variable=nope", "zlib"]);
        assert_eq!((code, out.as_str(), err.as_str()), (0, "\n", ""));

        let (_, out, _) = f.run(&["--print-variables", "zlib"]);
        let names: Vec<&str> = out.lines().collect();
        assert_eq!(names, vec!["includedir", "libdir", "prefix"]);
    }

    #[test]
    fn define_variable_relocates_a_package() {
        let f = Fixture::new("reloc");
        f.write("zlib", ZLIB);
        let (_, out, _) = f.run(&["--define-variable=prefix=/opt", "--libs", "zlib"]);
        assert_eq!(out.trim(), "-L/opt/lib -lz");
        let (_, out, _) = f.run(&["--define-variable=prefix=/opt", "--cflags", "zlib"]);
        assert_eq!(out.trim(), "-I/opt/include/zlib");
    }

    // ── environment ─────────────────────────────────────────────────────

    #[test]
    fn sysroot_prefixes_search_paths() {
        let f = Fixture::new("sysroot");
        f.write("zlib", ZLIB);
        let (_, out, _) = f.run_env(
            &["--cflags", "--libs", "--keep-system-libs", "zlib"],
            &[("PKG_CONFIG_SYSROOT_DIR", "/tgt")],
        );
        assert_eq!(out.trim(), "-I/tgt/usr/include/zlib -L/tgt/usr/lib -lz");
    }

    #[test]
    fn system_directories_can_be_kept() {
        let f = Fixture::new("keepsys");
        f.write("s", "Name: s\nVersion: 1\nCflags: -I/usr/include\nLibs: -L/usr/lib -ls\n");
        assert_eq!(f.run(&["--cflags", "s"]).1.trim(), "");
        assert_eq!(f.run(&["--cflags", "--keep-system-cflags", "s"]).1.trim(), "-I/usr/include");
        assert_eq!(f.run(&["--libs", "s"]).1.trim(), "-ls");
        assert_eq!(
            f.run(&["--libs", "--keep-system-libs", "s"]).1.trim(),
            "-L/usr/lib -ls"
        );
    }

    #[test]
    fn allow_system_env_vars_have_the_same_effect_as_the_flags() {
        let f = Fixture::new("allowsysenv");
        f.write("s", "Name: s\nVersion: 1\nCflags: -I/usr/include\nLibs: -L/usr/lib -ls\n");
        let (_, out, _) = f.run_env(
            &["--cflags", "s"],
            &[("PKG_CONFIG_ALLOW_SYSTEM_CFLAGS", "1")],
        );
        assert_eq!(out.trim(), "-I/usr/include");
        let (_, out, _) = f.run_env(&["--libs", "s"], &[("PKG_CONFIG_ALLOW_SYSTEM_LIBS", "1")]);
        assert_eq!(out.trim(), "-L/usr/lib -ls");
    }

    #[test]
    fn pc_sysrootdir_is_available_as_a_variable() {
        let f = Fixture::new("pcsysroot");
        f.write("v", "Name: v\nVersion: 1\nCflags: -I${pc_sysrootdir}/inc\n");
        let (_, out, _) = f.run_env(&["--cflags", "v"], &[("PKG_CONFIG_SYSROOT_DIR", "/tgt")]);
        // The variable expands, and the sysroot prefix is not applied twice.
        assert_eq!(out.trim(), "-I/tgt/inc");
    }

    // ── listing and validation ──────────────────────────────────────────

    #[test]
    fn list_all_shows_installed_packages() {
        let f = png_fixture("list");
        let (code, out, _) = f.run(&["--list-all"]);
        assert_eq!(code, 0);
        let names: Vec<&str> = out
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        assert_eq!(names, vec!["libpng", "zlib"]);
    }

    #[test]
    fn validate_accepts_a_good_file_and_rejects_a_bad_one() {
        let f = Fixture::new("validate");
        f.write("good", "Name: good\nVersion: 1\n");
        f.write("bad", "Name: bad\ngarbage here\n");
        assert_eq!(f.run(&["--validate", "good"]).0, 0);
        let (code, _, err) = f.run(&["--validate", "bad"]);
        assert_eq!(code, 1);
        assert!(err.contains("bad.pc"), "{err}");
    }

    #[test]
    fn validate_does_not_require_dependencies_to_exist() {
        // The point of --validate is to check a file in isolation, e.g. in a
        // package build before its dependencies are installed.
        let f = Fixture::new("validatedeps");
        f.write("lonely", "Name: lonely\nVersion: 1\nRequires: absent\n");
        assert_eq!(f.run(&["--validate", "lonely"]).0, 0);
        assert_eq!(f.run(&["--exists", "lonely"]).0, 1);
    }

    #[test]
    fn print_requires_lists_declared_dependencies() {
        let f = png_fixture("printreq");
        let (_, out, _) = f.run(&["--print-requires", "libpng"]);
        assert_eq!(out, "zlib >= 1.2\n");
        let (_, out, _) = f.run(&["--print-requires-private", "libpng"]);
        assert_eq!(out, "");
    }

    #[test]
    fn several_packages_are_queried_in_command_line_order() {
        let f = png_fixture("multi");
        let (_, out, _) = f.run(&["--modversion", "libpng", "zlib"]);
        assert_eq!(out, "1.6.40\n1.3.1\n");
    }
}
