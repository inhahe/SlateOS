//! Splitting, classifying, filtering and de-duplicating compiler/linker flags.
//!
//! `Cflags:` and `Libs:` in a `.pc` file are *shell* fragments, not
//! whitespace-separated tokens: a path with a space in it is legitimately
//! written `-I"/opt/my lib/include"`, and the reference tools honour that.  So
//! the first job here is a small POSIX-ish word splitter.
//!
//! The second job is order and duplication.  Flags are gathered from a
//! dependency graph, so the same `-lm` or `-I/usr/include/foo` routinely
//! arrives from several packages, and naively concatenating produces long
//! redundant command lines — and, worse, static-link order errors.  See
//! [`dedup`] for the rule and why it differs from pkg-config's.

/// How a flag participates in de-duplication and in the `--*-only-*` filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagKind {
    /// `-I<path>` — a header search path.
    IncludePath,
    /// `-L<path>` — a library search path.
    LibPath,
    /// `-l<name>` — a library to link.
    LibName,
    /// Anything else: `-D`, `-pthread`, `-Wl,...`, a bare object file, ...
    Other,
}

/// A single flag, already joined into one argument (`-I/usr/include`, never
/// the two-token form `-I` `/usr/include`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flag {
    pub kind: FlagKind,
    pub text: String,
}

impl Flag {
    /// The path or library name carried by the flag, i.e. the text after the
    /// two-character prefix.  Empty for [`FlagKind::Other`].
    #[must_use]
    pub fn payload(&self) -> &str {
        match self.kind {
            FlagKind::IncludePath | FlagKind::LibPath | FlagKind::LibName => {
                self.text.get(2..).unwrap_or("")
            }
            FlagKind::Other => "",
        }
    }
}

/// Split a shell fragment into words, honouring `'...'`, `"..."` and `\`.
///
/// Deliberately *not* a full shell parser: there is no expansion of any kind
/// here (`$VAR` substitution in a `.pc` file happens earlier, on the raw text,
/// and uses `${}` syntax rather than the shell's).  Backslash and quotes are
/// handled because they are the only shell syntax that changes where the word
/// boundaries fall, which is all this function decides.
#[must_use]
pub fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have_word = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if have_word {
                    out.push(core::mem::take(&mut cur));
                    have_word = false;
                }
            }
            '\'' => {
                // Single quotes are literal all the way to the closing quote;
                // a backslash inside them is an ordinary character.
                have_word = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    cur.push(c);
                }
            }
            '"' => {
                have_word = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        // Inside double quotes a backslash only escapes these
                        // four characters; before anything else it stays.
                        '\\' => match chars.peek() {
                            Some(&n @ ('"' | '\\' | '$' | '`')) => {
                                cur.push(n);
                                chars.next();
                            }
                            _ => cur.push('\\'),
                        },
                        c => cur.push(c),
                    }
                }
            }
            '\\' => {
                have_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c => {
                have_word = true;
                cur.push(c);
            }
        }
    }
    if have_word {
        out.push(cur);
    }
    out
}

/// Turn a word list into flags, joining the two-token forms (`-I` `/x`) that a
/// `.pc` file is allowed to use into the single-token form everyone expects.
#[must_use]
pub fn parse_flags(words: &[String]) -> Vec<Flag> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let Some(w) = words.get(i) else { break };
        let kind = match w.get(..2) {
            Some("-I") => FlagKind::IncludePath,
            Some("-L") => FlagKind::LibPath,
            Some("-l") => FlagKind::LibName,
            _ => FlagKind::Other,
        };
        if kind == FlagKind::Other {
            out.push(Flag {
                kind,
                text: w.clone(),
            });
            i += 1;
            continue;
        }
        if w.len() == 2 {
            // Detached argument: `-I /usr/include`.  Join it, because every
            // consumer of our output (and every dedup rule below) assumes a
            // flag is one argument.
            if let Some(next) = words.get(i + 1) {
                out.push(Flag {
                    kind,
                    text: format!("{w}{next}"),
                });
                i += 2;
                continue;
            }
            // Trailing bare `-I` with nothing after it: pass it through rather
            // than silently dropping it, so the error surfaces at the compiler.
            out.push(Flag {
                kind: FlagKind::Other,
                text: w.clone(),
            });
            i += 1;
            continue;
        }
        out.push(Flag {
            kind,
            text: w.clone(),
        });
        i += 1;
    }
    out
}

/// Convenience: split and classify in one step.
#[must_use]
pub fn parse_fragment(s: &str) -> Vec<Flag> {
    parse_flags(&shell_split(s))
}

/// Remove redundant flags while preserving a usable static link order.
///
/// The input is in dependency order — a package appears before everything it
/// requires — so:
///
/// * for `-I`, `-L` and everything else, the **first** occurrence is kept.
///   Search-path order is "first match wins", so keeping the earliest
///   occurrence preserves the semantics exactly.
/// * for `-l`, the **last** occurrence is kept.  A static linker resolves
///   left to right, so a library must appear *after* every library that needs
///   it; dropping the later copy of a shared dependency (say `-lm`, required
///   by two packages) would leave it too early to satisfy the second one.
///
/// This differs from pkg-config 0.29, which only collapses *adjacent*
/// duplicates and therefore leaves long command lines with repeated flags. It
/// differs in output text, never in meaning: any link that succeeds with
/// pkg-config's output succeeds with this one.
#[must_use]
pub fn dedup(flags: Vec<Flag>) -> Vec<Flag> {
    // Pass 1, right to left: mark the last occurrence of each `-l`.
    let mut keep = vec![true; flags.len()];
    let mut seen_libs: Vec<&str> = Vec::new();
    for (idx, f) in flags.iter().enumerate().rev() {
        if f.kind == FlagKind::LibName {
            if seen_libs.contains(&f.text.as_str()) {
                if let Some(slot) = keep.get_mut(idx) {
                    *slot = false;
                }
            } else {
                seen_libs.push(f.text.as_str());
            }
        }
    }
    // Pass 2, left to right: mark the first occurrence of everything else.
    let mut seen_other: Vec<&str> = Vec::new();
    for (idx, f) in flags.iter().enumerate() {
        if f.kind != FlagKind::LibName {
            if seen_other.contains(&f.text.as_str()) {
                if let Some(slot) = keep.get_mut(idx) {
                    *slot = false;
                }
            } else {
                seen_other.push(f.text.as_str());
            }
        }
    }
    flags
        .into_iter()
        .zip(keep)
        .filter_map(|(f, k)| if k { Some(f) } else { None })
        .collect()
}

/// Directories the compiler and linker already search, and which therefore
/// must not be added explicitly: doing so can reorder the search path and pull
/// in a system header ahead of a package's own.
const SYSTEM_INCLUDE_DIRS: &[&str] = &["/usr/include"];
const SYSTEM_LIB_DIRS: &[&str] = &["/usr/lib", "/lib", "/usr/lib64", "/lib64"];

/// Drop `-I` flags naming a directory the compiler searches anyway.
#[must_use]
pub fn strip_system_includes(flags: Vec<Flag>) -> Vec<Flag> {
    flags
        .into_iter()
        .filter(|f| {
            f.kind != FlagKind::IncludePath || !SYSTEM_INCLUDE_DIRS.contains(&f.payload())
        })
        .collect()
}

/// Drop `-L` flags naming a directory the linker searches anyway.
#[must_use]
pub fn strip_system_libdirs(flags: Vec<Flag>) -> Vec<Flag> {
    flags
        .into_iter()
        .filter(|f| f.kind != FlagKind::LibPath || !SYSTEM_LIB_DIRS.contains(&f.payload()))
        .collect()
}

/// Prefix `-I`/`-L` paths with a sysroot, for cross builds.
///
/// Only absolute paths are rewritten — a relative path is relative to the
/// build tree, not to the target root — and a path already under the sysroot
/// is left alone so that repeated application is a no-op.
#[must_use]
pub fn apply_sysroot(flags: Vec<Flag>, sysroot: &str) -> Vec<Flag> {
    if sysroot.is_empty() || sysroot == "/" {
        return flags;
    }
    let root = sysroot.trim_end_matches('/');
    flags
        .into_iter()
        .map(|f| match f.kind {
            FlagKind::IncludePath | FlagKind::LibPath => {
                let payload = f.payload();
                if !payload.starts_with('/') || payload.starts_with(&format!("{root}/")) {
                    return f;
                }
                let prefix = f.text.get(..2).unwrap_or("");
                Flag {
                    kind: f.kind,
                    text: format!("{prefix}{root}{payload}"),
                }
            }
            _ => f,
        })
        .collect()
}

/// Render flags as a single command-line string.
#[must_use]
pub fn render(flags: &[Flag]) -> String {
    flags
        .iter()
        .map(|f| f.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        apply_sysroot, dedup, parse_fragment, render, shell_split, strip_system_includes,
        strip_system_libdirs, Flag, FlagKind,
    };

    fn texts(flags: &[Flag]) -> Vec<&str> {
        flags.iter().map(|f| f.text.as_str()).collect()
    }

    #[test]
    fn plain_words_split_on_whitespace() {
        assert_eq!(shell_split("-lz -lm"), vec!["-lz", "-lm"]);
        assert_eq!(shell_split("  -lz\t\n-lm  "), vec!["-lz", "-lm"]);
        assert!(shell_split("   ").is_empty());
        assert!(shell_split("").is_empty());
    }

    #[test]
    fn quotes_protect_embedded_spaces() {
        assert_eq!(
            shell_split(r#"-I"/opt/my lib/include" -lz"#),
            vec!["-I/opt/my lib/include", "-lz"]
        );
        assert_eq!(
            shell_split("-I'/opt/my lib/include'"),
            vec!["-I/opt/my lib/include"]
        );
    }

    #[test]
    fn backslash_escapes_a_space() {
        assert_eq!(shell_split(r"-I/opt/my\ lib"), vec!["-I/opt/my lib"]);
    }

    #[test]
    fn single_quotes_are_fully_literal() {
        assert_eq!(shell_split(r"'a\b'"), vec![r"a\b"]);
    }

    #[test]
    fn double_quotes_escape_only_four_characters() {
        assert_eq!(shell_split(r#""a\"b""#), vec![r#"a"b"#]);
        assert_eq!(shell_split(r#""a\nb""#), vec![r"a\nb"]);
        assert_eq!(shell_split(r#""a\$b""#), vec!["a$b"]);
    }

    #[test]
    fn an_empty_quoted_word_is_still_a_word() {
        assert_eq!(shell_split(r#"-DX="" -lz"#), vec!["-DX=", "-lz"]);
    }

    #[test]
    fn flags_are_classified() {
        let f = parse_fragment("-I/usr/include/foo -L/opt/lib -lfoo -pthread");
        assert_eq!(
            f.iter().map(|f| f.kind).collect::<Vec<_>>(),
            vec![
                FlagKind::IncludePath,
                FlagKind::LibPath,
                FlagKind::LibName,
                FlagKind::Other
            ]
        );
        assert_eq!(f[0].payload(), "/usr/include/foo");
        assert_eq!(f[2].payload(), "foo");
        assert_eq!(f[3].payload(), "");
    }

    #[test]
    fn detached_flag_arguments_are_joined() {
        let f = parse_fragment("-I /usr/include/foo -L /opt/lib -l foo");
        assert_eq!(texts(&f), vec!["-I/usr/include/foo", "-L/opt/lib", "-lfoo"]);
    }

    #[test]
    fn a_trailing_bare_dash_i_is_passed_through_not_dropped() {
        let f = parse_fragment("-lz -I");
        assert_eq!(texts(&f), vec!["-lz", "-I"]);
        assert_eq!(f[1].kind, FlagKind::Other);
    }

    #[test]
    fn dedup_keeps_the_first_include_path() {
        let f = parse_fragment("-I/a -I/b -I/a -I/c");
        assert_eq!(texts(&dedup(f)), vec!["-I/a", "-I/b", "-I/c"]);
    }

    #[test]
    fn dedup_keeps_the_last_library_so_static_link_order_survives() {
        // Package A needs -lm, package B (linked after A) also needs it.
        // Keeping the *first* -lm would place it before -lb and break the
        // link; keeping the last is correct.
        let f = parse_fragment("-la -lm -lb -lm");
        assert_eq!(texts(&dedup(f)), vec!["-la", "-lb", "-lm"]);
    }

    #[test]
    fn dedup_leaves_distinct_flags_alone() {
        let f = parse_fragment("-la -lb -lc");
        assert_eq!(texts(&dedup(f)), vec!["-la", "-lb", "-lc"]);
    }

    #[test]
    fn dedup_of_other_flags_keeps_first() {
        let f = parse_fragment("-pthread -DFOO -pthread");
        assert_eq!(texts(&dedup(f)), vec!["-pthread", "-DFOO"]);
    }

    #[test]
    fn system_directories_are_stripped() {
        let f = parse_fragment("-I/usr/include -I/usr/include/foo");
        assert_eq!(texts(&strip_system_includes(f)), vec!["-I/usr/include/foo"]);

        let f = parse_fragment("-L/usr/lib -L/lib64 -L/opt/lib");
        assert_eq!(texts(&strip_system_libdirs(f)), vec!["-L/opt/lib"]);
    }

    #[test]
    fn stripping_system_dirs_does_not_touch_libraries() {
        let f = parse_fragment("-L/usr/lib -lusr");
        assert_eq!(texts(&strip_system_libdirs(f)), vec!["-lusr"]);
    }

    #[test]
    fn sysroot_prefixes_absolute_search_paths_only() {
        let f = parse_fragment("-I/usr/include/foo -L/usr/lib -lfoo -Irelative");
        let f = apply_sysroot(f, "/sysroot");
        assert_eq!(
            texts(&f),
            vec![
                "-I/sysroot/usr/include/foo",
                "-L/sysroot/usr/lib",
                "-lfoo",
                "-Irelative"
            ]
        );
    }

    #[test]
    fn sysroot_application_is_idempotent() {
        let once = apply_sysroot(parse_fragment("-I/usr/include"), "/sysroot");
        let twice = apply_sysroot(once.clone(), "/sysroot");
        assert_eq!(texts(&once), texts(&twice));
    }

    #[test]
    fn an_empty_or_root_sysroot_changes_nothing() {
        let f = parse_fragment("-I/usr/include");
        assert_eq!(texts(&apply_sysroot(f.clone(), "")), vec!["-I/usr/include"]);
        assert_eq!(texts(&apply_sysroot(f, "/")), vec!["-I/usr/include"]);
    }

    #[test]
    fn rendering_joins_with_single_spaces() {
        assert_eq!(render(&parse_fragment("-lz   -lm")), "-lz -lm");
        assert_eq!(render(&[]), "");
    }
}
