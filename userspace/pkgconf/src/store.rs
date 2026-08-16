//! Finding `.pc` files on disk and resolving a dependency graph over them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::pcfile::{self, Dep, ParseError, PcFile};
use crate::version;

/// The directories searched when `PKG_CONFIG_LIBDIR` is unset.  These are the
/// two locations the whole ecosystem agrees on; a package that installs
/// somewhere else is expected to extend `PKG_CONFIG_PATH`.
pub const DEFAULT_SEARCH_DIRS: &[&str] = &["/usr/lib/pkgconfig", "/usr/share/pkgconfig"];

/// The separator between entries of `PKG_CONFIG_PATH`.  SlateOS is a
/// forward-slash, colon-separated world like every other Unix; the Windows
/// arm exists only so the host-side test build behaves sanely.
#[cfg(windows)]
pub const PATH_SEP: char = ';';
#[cfg(not(windows))]
pub const PATH_SEP: char = ':';

/// The package name every reference implementation answers for without
/// touching the filesystem.
pub const VIRTUAL_PKG_CONFIG: &str = "pkg-config";

/// Build the virtual `pkg-config` package.
///
/// `pkg-config --variable=pc_path pkg-config` is how autotools, `CMake` and
/// Meson discover where to *install* a `.pc` file, and
/// `--variable=pc_system_libdirs` is how they decide which `-L` flags are
/// redundant.  Both have to be answerable on a system where no `.pc` file is
/// installed yet — the bootstrap case — so they cannot come from disk.
///
/// The values are the compile-time defaults on purpose, not the search path
/// this invocation is actually using.  A cross build sets `PKG_CONFIG_LIBDIR`
/// to the target sysroot; if `pc_path` tracked that, every package built
/// inside such an environment would install its own `.pc` file into the
/// sysroot and disappear from the host's view of the system.
///
/// It shadows a real `pkg-config.pc` found on the search path, matching
/// upstream pkgconf, which consults its builtin table before searching.
fn virtual_pkg_config() -> PcFile {
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    vars.insert("pc_path".to_string(), join_dirs(DEFAULT_SEARCH_DIRS));
    vars.insert(
        "pc_system_includedirs".to_string(),
        join_dirs(crate::flags::SYSTEM_INCLUDE_DIRS),
    );
    vars.insert(
        "pc_system_libdirs".to_string(),
        join_dirs(crate::flags::SYSTEM_LIB_DIRS),
    );
    PcFile {
        // Deliberately empty: there is no file, so `${pcfiledir}` must not
        // resolve for this package rather than resolving to something
        // plausible-looking like the current directory.
        path: PathBuf::new(),
        key: VIRTUAL_PKG_CONFIG.to_string(),
        name: VIRTUAL_PKG_CONFIG.to_string(),
        description: "virtual package defining pkg-config API version supported".to_string(),
        version: crate::PKGCONF_VERSION.to_string(),
        vars,
        ..PcFile::default()
    }
}

fn join_dirs(dirs: &[&str]) -> String {
    dirs.join(&PATH_SEP.to_string())
}

/// Why a package could not be produced.
#[derive(Clone, Debug)]
pub enum LookupError {
    /// No `<name>.pc` anywhere on the search path.
    NotFound { name: String },
    /// Found, but unreadable.
    Unreadable { path: PathBuf, reason: String },
    /// Found and readable, but not valid.
    Invalid { path: PathBuf, error: ParseError },
    /// Found, but the wrong version for the constraint that pulled it in.
    VersionMismatch {
        dep: Dep,
        have: String,
        required_by: Option<String>,
    },
    /// A `Conflicts:` clause matched an installed package.
    Conflict {
        package: String,
        dep: Dep,
        have: String,
    },
}

impl LookupError {
    /// The multi-line text the reference tools print for this failure.  The
    /// "Perhaps you should add the directory" paragraph is worth reproducing
    /// verbatim: it is the single most useful diagnostic pkg-config emits, and
    /// build scripts in the wild grep for the final `No package '...' found`.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotFound { name } => format!(
                "Package {name} was not found in the pkg-config search path.\n\
                 Perhaps you should add the directory containing `{name}.pc'\n\
                 to the PKG_CONFIG_PATH environment variable\n\
                 No package '{name}' found"
            ),
            Self::Unreadable { path, reason } => {
                format!("Failed to open '{}': {reason}", path.display())
            }
            Self::Invalid { path, error } => error.message(path),
            Self::VersionMismatch {
                dep,
                have,
                required_by,
            } => {
                let (op, want) = match &dep.constraint {
                    Some((op, want)) => (op.as_str(), want.as_str()),
                    None => ("", ""),
                };
                let who = match required_by {
                    Some(p) => format!("Package '{p}' requires"),
                    None => "Requested".to_string(),
                };
                format!(
                    "{who} '{} {op} {want}' but version of {} is {have}",
                    dep.name, dep.name
                )
            }
            Self::Conflict { package, dep, have } => format!(
                "Version mismatch: '{package}' conflicts with '{}', found {have}",
                dep.display()
            ),
        }
    }
}

/// A search path plus a cache of everything loaded from it.
pub struct Store {
    dirs: Vec<PathBuf>,
    overrides: BTreeMap<String, String>,
    cache: BTreeMap<String, Rc<PcFile>>,
    /// Undefined-variable warnings accumulated while loading, in encounter
    /// order.  Reported only when errors are being printed.
    pub warnings: Vec<String>,
}

impl Store {
    #[must_use]
    pub fn new(dirs: Vec<PathBuf>, overrides: BTreeMap<String, String>) -> Self {
        Self {
            dirs,
            overrides,
            cache: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    /// Build the search path from the environment, in priority order:
    /// `PKG_CONFIG_PATH`, then `PKG_CONFIG_LIBDIR` (or the built-in defaults
    /// if it is unset), then any `--with-path=` directories.
    ///
    /// `PKG_CONFIG_LIBDIR` *replaces* the defaults rather than adding to them,
    /// which is what makes it usable for cross builds: it is the only way to
    /// stop the host's own `.pc` files leaking into a target query.
    #[must_use]
    pub fn search_dirs_from_env(
        extra: &[String],
        get: &dyn Fn(&str) -> Option<String>,
    ) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let push_list = |s: &str, dirs: &mut Vec<PathBuf>| {
            for part in s.split(PATH_SEP) {
                if !part.is_empty() {
                    dirs.push(PathBuf::from(part));
                }
            }
        };
        if let Some(p) = get("PKG_CONFIG_PATH") {
            push_list(&p, &mut dirs);
        }
        match get("PKG_CONFIG_LIBDIR") {
            Some(p) => push_list(&p, &mut dirs),
            None => {
                for d in DEFAULT_SEARCH_DIRS {
                    dirs.push(PathBuf::from(*d));
                }
            }
        }
        for e in extra {
            dirs.push(PathBuf::from(e));
        }
        dirs
    }

    /// Locate `<name>.pc`, honouring the search-path order.
    fn locate(&self, name: &str) -> Option<PathBuf> {
        // A literal path to a .pc file is accepted in place of a name; this is
        // how a build tree queries an uninstalled package.
        //
        // The extension test is case-*sensitive* on purpose, so `foo.PC` is a
        // package name rather than a path: `design.txt` mandates a
        // case-sensitive filesystem, and both reference tools compare the
        // literal `.pc`. Going through `Path::extension` rather than
        // `ends_with` also stops a bare `.pc` — a legal dotfile name — being
        // read as a zero-length package name.
        let p = Path::new(name);
        if p.extension().is_some_and(|e| e == "pc") && p.is_file() {
            return Some(p.to_path_buf());
        }
        for dir in &self.dirs {
            let candidate = dir.join(format!("{name}.pc"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    /// Load a package by name, from cache if already read.
    ///
    /// # Errors
    ///
    /// [`LookupError::NotFound`] if no such `.pc` file exists,
    /// [`LookupError::Unreadable`] on an I/O failure, or
    /// [`LookupError::Invalid`] if the file does not parse.
    pub fn load(&mut self, name: &str) -> Result<Rc<PcFile>, LookupError> {
        if let Some(p) = self.cache.get(name) {
            return Ok(Rc::clone(p));
        }
        if name == VIRTUAL_PKG_CONFIG {
            let rc = Rc::new(virtual_pkg_config());
            self.cache.insert(name.to_string(), Rc::clone(&rc));
            return Ok(rc);
        }
        let path = self.locate(name).ok_or_else(|| LookupError::NotFound {
            name: name.to_string(),
        })?;
        let text = std::fs::read_to_string(&path).map_err(|e| LookupError::Unreadable {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        let key = Path::new(name)
            .file_stem()
            .map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned());
        let parsed = pcfile::parse(&key, &path, &text, &self.overrides).map_err(|error| {
            LookupError::Invalid {
                path: path.clone(),
                error,
            }
        })?;
        for v in parsed.undefined_vars {
            self.warnings.push(format!(
                "Variable '{v}' not defined in '{}'",
                path.display()
            ));
        }
        let rc = Rc::new(parsed.pkg);
        self.cache.insert(name.to_string(), Rc::clone(&rc));
        Ok(rc)
    }

    /// Every package on the search path, keyed by name.  Earlier directories
    /// win, matching lookup order, so `--list-all` describes the packages that
    /// would actually be selected.  Unparseable files are skipped rather than
    /// aborting the listing — one broken `.pc` installed by a third party
    /// should not make `--list-all` useless.
    #[must_use]
    pub fn list_all(&mut self) -> Vec<Rc<PcFile>> {
        let mut names: Vec<String> = Vec::new();
        for dir in self.dirs.clone() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut in_dir: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e == "pc")
                    && let Some(stem) = p.file_stem()
                {
                    in_dir.push(stem.to_string_lossy().into_owned());
                }
            }
            in_dir.sort();
            for n in in_dir {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        names.sort();
        names.iter().filter_map(|n| self.load(n).ok()).collect()
    }

    /// Check a dependency's version constraint against a loaded package.
    fn check_version(
        dep: &Dep,
        pkg: &PcFile,
        required_by: Option<&str>,
    ) -> Result<(), LookupError> {
        let Some((op, want)) = &dep.constraint else {
            return Ok(());
        };
        if op.satisfied_by(version::compare(&pkg.version, want)) {
            return Ok(());
        }
        Err(LookupError::VersionMismatch {
            dep: dep.clone(),
            have: pkg.version.clone(),
            required_by: required_by.map(ToString::to_string),
        })
    }

    /// Resolve `roots` and everything they require into link order.
    ///
    /// The returned list is **dependents before dependencies**: if A requires
    /// B, A comes first, which is the order a static linker needs.  Where two
    /// packages both require a third, the third is emitted after both — a
    /// plain depth-first pre-order would get that wrong, so this is a real
    /// topological sort (reverse post-order over reversed roots, which also
    /// preserves the command-line order of independent roots).
    ///
    /// `include_private` follows `Requires.private` as well.  Callers pass
    /// `true` when computing cflags (a private dependency's *headers* are
    /// still needed to compile against the public one) and only under
    /// `--static` when computing libs.
    ///
    /// # Errors
    ///
    /// The first [`LookupError`] hit while loading or version-checking.
    pub fn resolve(
        &mut self,
        roots: &[Dep],
        include_private: bool,
    ) -> Result<Vec<Rc<PcFile>>, LookupError> {
        let mut post: Vec<Rc<PcFile>> = Vec::new();
        let mut done: Vec<String> = Vec::new();
        let mut on_stack: Vec<String> = Vec::new();

        for dep in roots.iter().rev() {
            self.visit(
                dep,
                None,
                include_private,
                &mut post,
                &mut done,
                &mut on_stack,
            )?;
        }
        post.reverse();
        Ok(post)
    }

    fn visit(
        &mut self,
        dep: &Dep,
        required_by: Option<&str>,
        include_private: bool,
        post: &mut Vec<Rc<PcFile>>,
        done: &mut Vec<String>,
        on_stack: &mut Vec<String>,
    ) -> Result<(), LookupError> {
        let pkg = self.load(&dep.name)?;
        Self::check_version(dep, &pkg, required_by)?;

        if done.contains(&dep.name) {
            return Ok(());
        }
        if on_stack.contains(&dep.name) {
            // A dependency cycle.  The reference tools loop forever here; we
            // simply stop descending.  The package is still emitted exactly
            // once, by the frame that first entered it.
            return Ok(());
        }
        on_stack.push(dep.name.clone());

        let mut children: Vec<Dep> = pkg.requires.clone();
        if include_private {
            children.extend(pkg.requires_private.iter().cloned());
        }
        // Children are visited in reverse so that the final `post.reverse()`
        // puts them back into declaration order.  Visiting them forwards would
        // emit sibling dependencies backwards, which is harmless for `-I` but
        // reverses link order for `-l`.
        for child in children.iter().rev() {
            self.visit(child, Some(&pkg.key), include_private, post, done, on_stack)?;
        }

        // Conflicts are checked after the subtree so that a conflicting
        // package pulled in transitively is caught too.
        for c in &pkg.conflicts {
            if let Ok(other) = self.load(&c.name) {
                let matches = match &c.constraint {
                    Some((op, want)) => op.satisfied_by(version::compare(&other.version, want)),
                    None => true,
                };
                if matches {
                    return Err(LookupError::Conflict {
                        package: pkg.key.clone(),
                        dep: c.clone(),
                        have: other.version.clone(),
                    });
                }
            }
        }

        on_stack.retain(|n| n != &dep.name);
        done.push(dep.name.clone());
        post.push(pkg);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SEARCH_DIRS, LookupError, PATH_SEP, Store};
    use crate::pcfile::Dep;
    use crate::version::CmpOp;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A scratch directory populated with `.pc` files.  Removed on drop so a
    /// failing test cannot leave litter behind for the next run to trip over.
    struct Scratch {
        dir: PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Self {
            let mut dir = std::env::temp_dir();
            dir.push(format!("slateos-pkgconf-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create scratch dir");
            Self { dir }
        }

        fn write(&self, name: &str, body: &str) {
            std::fs::write(self.dir.join(format!("{name}.pc")), body).expect("write .pc");
        }

        fn store(&self) -> Store {
            Store::new(vec![self.dir.clone()], BTreeMap::new())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn dep(name: &str) -> Dep {
        Dep {
            name: name.to_string(),
            constraint: None,
        }
    }

    fn dep_v(name: &str, op: CmpOp, v: &str) -> Dep {
        Dep {
            name: name.to_string(),
            constraint: Some((op, v.to_string())),
        }
    }

    fn keys(pkgs: &[std::rc::Rc<crate::pcfile::PcFile>]) -> Vec<String> {
        pkgs.iter().map(|p| p.key.clone()).collect()
    }

    #[test]
    fn default_search_path_is_used_when_libdir_is_unset() {
        let dirs = Store::search_dirs_from_env(&[], &|_| None);
        assert_eq!(
            dirs,
            DEFAULT_SEARCH_DIRS
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn pkg_config_libdir_replaces_the_defaults() {
        let dirs = Store::search_dirs_from_env(&[], &|k| {
            (k == "PKG_CONFIG_LIBDIR").then(|| "/target/lib/pkgconfig".to_string())
        });
        assert_eq!(dirs, vec![PathBuf::from("/target/lib/pkgconfig")]);
    }

    #[test]
    fn pkg_config_path_comes_first_and_splits_on_the_separator() {
        let value = format!("/a{PATH_SEP}/b");
        let dirs =
            Store::search_dirs_from_env(&[], &|k| (k == "PKG_CONFIG_PATH").then(|| value.clone()));
        assert_eq!(dirs[0], PathBuf::from("/a"));
        assert_eq!(dirs[1], PathBuf::from("/b"));
        assert_eq!(dirs[2], PathBuf::from(DEFAULT_SEARCH_DIRS[0]));
    }

    #[test]
    fn with_path_directories_are_appended_last() {
        let dirs = Store::search_dirs_from_env(&["/extra".to_string()], &|_| None);
        assert_eq!(dirs.last(), Some(&PathBuf::from("/extra")));
    }

    #[test]
    fn empty_path_entries_are_ignored() {
        let value = format!("{PATH_SEP}/a{PATH_SEP}{PATH_SEP}");
        let dirs = Store::search_dirs_from_env(&[], &|k| {
            (k == "PKG_CONFIG_LIBDIR").then(|| value.clone())
        });
        assert_eq!(dirs, vec![PathBuf::from("/a")]);
    }

    #[test]
    fn a_missing_package_reports_the_search_path_hint() {
        let s = Scratch::new("missing");
        let err = s.store().load("nope").expect_err("should not exist");
        let msg = err.message();
        assert!(msg.contains("No package 'nope' found"), "{msg}");
        assert!(msg.contains("PKG_CONFIG_PATH"), "{msg}");
    }

    #[test]
    fn a_package_is_found_and_parsed() {
        let s = Scratch::new("found");
        s.write(
            "zlib",
            "prefix=/usr\nName: zlib\nVersion: 1.3.1\nLibs: -lz\n",
        );
        let pkg = s.store().load("zlib").expect("load");
        assert_eq!(pkg.version, "1.3.1");
        assert_eq!(pkg.libs, "-lz");
    }

    #[test]
    fn an_unparseable_package_reports_where() {
        let s = Scratch::new("bad");
        s.write("bad", "Name: bad\nnonsense line\n");
        let err = s.store().load("bad").expect_err("should not parse");
        assert!(err.message().contains("bad.pc"), "{}", err.message());
    }

    #[test]
    fn requires_are_resolved_transitively() {
        let s = Scratch::new("trans");
        s.write("a", "Name: a\nVersion: 1\nRequires: b\nLibs: -la\n");
        s.write("b", "Name: b\nVersion: 1\nRequires: c\nLibs: -lb\n");
        s.write("c", "Name: c\nVersion: 1\nLibs: -lc\n");
        let order = s.store().resolve(&[dep("a")], false).expect("resolve");
        assert_eq!(keys(&order), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_shared_dependency_is_emitted_after_both_users() {
        // A -> {B, C}, C -> B.  Pre-order DFS would give A B C, putting B
        // before C even though C needs it; the topological sort gives A C B.
        let s = Scratch::new("diamond");
        s.write("a", "Name: a\nVersion: 1\nRequires: b, c\nLibs: -la\n");
        s.write("b", "Name: b\nVersion: 1\nLibs: -lb\n");
        s.write("c", "Name: c\nVersion: 1\nRequires: b\nLibs: -lc\n");
        let order = s.store().resolve(&[dep("a")], false).expect("resolve");
        assert_eq!(keys(&order), vec!["a", "c", "b"]);
    }

    #[test]
    fn independent_roots_keep_command_line_order() {
        let s = Scratch::new("roots");
        s.write("x", "Name: x\nVersion: 1\nLibs: -lx\n");
        s.write("y", "Name: y\nVersion: 1\nLibs: -ly\n");
        let order = s
            .store()
            .resolve(&[dep("x"), dep("y")], false)
            .expect("resolve");
        assert_eq!(keys(&order), vec!["x", "y"]);
    }

    #[test]
    fn private_requires_are_followed_only_when_asked() {
        let s = Scratch::new("private");
        s.write("a", "Name: a\nVersion: 1\nRequires.private: p\nLibs: -la\n");
        s.write("p", "Name: p\nVersion: 1\nLibs: -lp\n");
        let public = s.store().resolve(&[dep("a")], false).expect("resolve");
        assert_eq!(keys(&public), vec!["a"]);
        let private = s.store().resolve(&[dep("a")], true).expect("resolve");
        assert_eq!(keys(&private), vec!["a", "p"]);
    }

    #[test]
    fn a_version_constraint_is_enforced() {
        let s = Scratch::new("ver");
        s.write("a", "Name: a\nVersion: 1.2.3\n");
        let mut store = s.store();
        assert!(
            store
                .resolve(&[dep_v("a", CmpOp::Ge, "1.2")], false)
                .is_ok()
        );
        let err = store
            .resolve(&[dep_v("a", CmpOp::Ge, "2.0")], false)
            .expect_err("1.2.3 is not >= 2.0");
        assert!(matches!(err, LookupError::VersionMismatch { .. }));
        assert!(err.message().contains("1.2.3"), "{}", err.message());
    }

    #[test]
    fn a_transitive_version_constraint_names_the_requiring_package() {
        let s = Scratch::new("transver");
        s.write("a", "Name: a\nVersion: 1\nRequires: b >= 9.0\n");
        s.write("b", "Name: b\nVersion: 1.0\n");
        let err = s
            .store()
            .resolve(&[dep("a")], false)
            .expect_err("b is too old");
        let msg = err.message();
        assert!(msg.contains("Package 'a' requires"), "{msg}");
    }

    #[test]
    fn a_dependency_cycle_terminates() {
        let s = Scratch::new("cycle");
        s.write("a", "Name: a\nVersion: 1\nRequires: b\nLibs: -la\n");
        s.write("b", "Name: b\nVersion: 1\nRequires: a\nLibs: -lb\n");
        let order = s.store().resolve(&[dep("a")], false).expect("resolve");
        assert_eq!(order.len(), 2);
        assert_eq!(keys(&order)[0], "a");
    }

    #[test]
    fn a_matching_conflicts_clause_is_an_error() {
        let s = Scratch::new("conflict");
        s.write("a", "Name: a\nVersion: 1\nConflicts: b < 2.0\n");
        s.write("b", "Name: b\nVersion: 1.0\n");
        let err = s
            .store()
            .resolve(&[dep("a")], false)
            .expect_err("b 1.0 conflicts");
        assert!(matches!(err, LookupError::Conflict { .. }));
    }

    #[test]
    fn a_non_matching_conflicts_clause_is_fine() {
        let s = Scratch::new("noconflict");
        s.write("a", "Name: a\nVersion: 1\nConflicts: b < 2.0\n");
        s.write("b", "Name: b\nVersion: 3.0\n");
        assert!(s.store().resolve(&[dep("a")], false).is_ok());
    }

    #[test]
    fn list_all_returns_every_package_sorted() {
        let s = Scratch::new("listall");
        s.write("zlib", "Name: zlib\nVersion: 1\n");
        s.write("acme", "Name: acme\nVersion: 2\n");
        let all = s.store().list_all();
        assert_eq!(keys(&all), vec!["acme", "zlib"]);
    }

    #[test]
    fn list_all_skips_unparseable_files_rather_than_giving_up() {
        let s = Scratch::new("listbad");
        s.write("good", "Name: good\nVersion: 1\n");
        s.write("broken", "Name: broken\nnot a line\n");
        let all = s.store().list_all();
        assert_eq!(keys(&all), vec!["good"]);
    }

    #[test]
    fn an_earlier_search_directory_wins() {
        let first = Scratch::new("prio1");
        let second = Scratch::new("prio2");
        first.write("dup", "Name: dup\nVersion: 1\n");
        second.write("dup", "Name: dup\nVersion: 2\n");
        let mut store = Store::new(vec![first.dir.clone(), second.dir.clone()], BTreeMap::new());
        assert_eq!(store.load("dup").expect("load").version, "1");
    }

    #[test]
    fn a_literal_pc_path_can_stand_in_for_a_name() {
        let s = Scratch::new("literal");
        s.write("uninst", "Name: uninst\nVersion: 7\n");
        let path = s.dir.join("uninst.pc");
        // An empty search path: only the literal-path branch can find this.
        let mut store = Store::new(Vec::new(), BTreeMap::new());
        let pkg = store
            .load(&path.to_string_lossy())
            .expect("literal path should load");
        assert_eq!(pkg.version, "7");
        assert_eq!(pkg.key, "uninst");
    }

    #[test]
    fn a_name_ending_in_pc_that_is_not_a_file_is_still_searched_for() {
        // `libfoo.pc` on the command line with no such file in the current
        // directory must fall through to the search path, not fail outright.
        let s = Scratch::new("literalfall");
        s.write("odd.pc", "Name: odd.pc\nVersion: 3\n");
        let mut store = Store::new(vec![s.dir.clone()], BTreeMap::new());
        assert_eq!(store.load("odd.pc").expect("via search path").version, "3");
    }

    #[test]
    fn the_virtual_pkg_config_package_shadows_a_real_one_on_the_path() {
        // Upstream pkgconf consults its builtin table before touching the
        // filesystem, and build systems test against that: a stray
        // pkg-config.pc must not be able to redefine pc_path underneath them.
        let s = Scratch::new("virtualshadow");
        s.write(
            "pkg-config",
            "Name: impostor\nVersion: 0.1\npc_path=/wrong\n",
        );
        let mut store = Store::new(vec![s.dir.clone()], BTreeMap::new());
        let pkg = store.load("pkg-config").expect("builtin always resolves");
        assert_eq!(pkg.version, crate::PKGCONF_VERSION);
        assert_eq!(
            pkg.var("pc_path").as_deref(),
            Some(DEFAULT_SEARCH_DIRS.join(&PATH_SEP.to_string()).as_str())
        );
    }

    #[test]
    fn undefined_variables_become_warnings_not_failures() {
        let s = Scratch::new("warn");
        s.write("w", "Name: w\nVersion: 1\nCflags: -I${nope}\n");
        let mut store = s.store();
        store.load("w").expect("load");
        assert_eq!(store.warnings.len(), 1);
        assert!(store.warnings[0].contains("nope"), "{:?}", store.warnings);
    }
}
