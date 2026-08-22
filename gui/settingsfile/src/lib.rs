//! Where a user's configuration lives, and how it gets there.
//!
//! Every settings panel in the desktop edits state that has to outlive the
//! session, and none of them had anywhere to put it. This crate is the one
//! place that answers "which file?" and "how do I write it without losing the
//! user's copy if the power goes out mid-save?", so that twenty settings
//! panels do not answer it twenty different ways.
//!
//! # Why this is a crate and not a module of `appearance`
//!
//! It began as `appearance::config`, because appearance settings were the
//! first thing here that needed saving. Nothing in it is about appearance:
//! `appearance.yaml` and `input.yaml` are stored by the same rules, and so
//! will every settings group added after them. Leaving it where it started
//! would have meant the *input* settings model depending on the *appearance*
//! model — and through it on the widget toolkit, for a `Color` type it never
//! names — purely to learn where `$XDG_CONFIG_HOME` is. A settings group that
//! carries no colours should not compile a widget library to find its own
//! file.
//!
//! `appearance` re-exports this crate as `appearance::config`, so the original
//! path still resolves and no caller had to change.
//!
//! # Layout
//!
//! `$XDG_CONFIG_HOME/slateos/<name>.yaml`, falling back to
//! `$HOME/.config/slateos/<name>.yaml`. Both spellings are honoured because
//! the rest of the tree already assumes an XDG-shaped home (`~/.cache`,
//! `~/.local/share/Trash`), and a user who has set `XDG_CONFIG_HOME` has said
//! plainly where they want their configuration.
//!
//! With neither variable set there is no user to have settings, so
//! [`store`] reports it rather than inventing a location — writing a user's
//! personal preferences into a system-wide path because `$HOME` was missing
//! would be worse than not writing them at all.
//!
//! # Durability
//!
//! [`store`] writes a temporary file beside the target and renames it over
//! the original. A rename within a directory is atomic, so a reader — or a
//! crash — sees either the whole old file or the whole new one, never the
//! truncated middle of a save. Writing in place is how a settings dialog
//! turns a power cut into an empty config file.

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use yamldoc::Document;

/// Give an enum a spelling in the configuration file.
///
/// These names are deliberately **not** the enum's `label`. A label is what the
/// user reads on screen — "Extra Large (96px)", "Flat (no acceleration)" — and
/// it changes when the wording is improved or a preset is retuned. A config
/// spelling is part of the file format: change it and every existing user's
/// saved choice silently reverts to the default the next time the desktop
/// starts. Keeping them separate means the UI text is free to move.
///
/// It lives here rather than in one settings crate because the second settings
/// crate wanted it too, and a macro copied between two crates is two file
/// formats waiting to disagree about whether a name is `right_handed` or
/// `right-handed`.
///
/// ```
/// # use settingsfile::yaml_enum;
/// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// enum Hand { Left, Right }
/// yaml_enum!(Hand { Left => "left", Right => "right" });
/// assert_eq!(Hand::Left.yaml_name(), "left");
/// assert_eq!(Hand::from_yaml_name("right"), Some(Hand::Right));
/// assert_eq!(Hand::from_yaml_name("sideways"), None);
/// ```
#[macro_export]
macro_rules! yaml_enum {
    ($ty:ty { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl $ty {
            /// This value's spelling in the configuration file.
            #[must_use]
            pub fn yaml_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// The value a configuration file spelling names.
            ///
            /// `None` for a spelling this build does not know, which is how a
            /// file written by a newer desktop degrades to the default rather
            /// than refusing to load.
            #[must_use]
            pub fn from_yaml_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

/// The directory holding this user's SlateOS configuration.
///
/// `None` when the environment names no home, which is the case in an early
/// boot context or a stripped service environment.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg).join("slateos"));
    }
    let home = env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home).join(".config").join("slateos"))
}

/// The file a named settings group lives in.
#[must_use]
pub fn path_for(name: &str) -> Option<PathBuf> {
    let mut path = config_dir()?;
    path.push(name);
    path.set_extension("yaml");
    Some(path)
}

/// Read a settings group.
///
/// A file that does not exist, or cannot be read, yields an empty document —
/// which every settings reader turns into its defaults. That is deliberately
/// not an error: "the user has never opened this panel" is the ordinary case
/// on a fresh install, not a failure to report.
///
/// A file that exists but holds something unreadable is *also* returned as
/// itself rather than discarded, so that saving a single changed setting
/// splices into the user's file instead of replacing it wholesale.
#[must_use]
pub fn load(name: &str) -> Document {
    let Some(path) = path_for(name) else {
        return Document::new();
    };
    match fs::read_to_string(&path) {
        Ok(text) => Document::parse(&text),
        Err(_) => Document::new(),
    }
}

/// Write a settings group, atomically.
///
/// # Errors
///
/// If there is no configuration directory to write to, if it cannot be
/// created, or if the write or the rename fails.
pub fn store(name: &str, doc: &Document) -> io::Result<()> {
    let path = path_for(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no configuration directory: neither XDG_CONFIG_HOME nor HOME is set",
        )
    })?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Named for the target rather than randomly, so a crash between the write
    // and the rename leaves one identifiable piece of litter that the next
    // save overwrites — not an unbounded pile of temporaries.
    let mut temp = path.clone();
    let mut file_name = path.file_name().unwrap_or_default().to_os_string();
    file_name.push(".new");
    temp.set_file_name(file_name);

    fs::write(&temp, doc.to_text())?;
    match fs::rename(&temp, &path) {
        Ok(()) => Ok(()),
        Err(err) => {
            // The temporary is not the user's file, so failing to clean it up
            // is not worth masking the error that actually matters.
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

// Test support, so panicking on a broken precondition is the right behaviour:
// a test whose sandbox cannot be created must fail loudly rather than quietly
// fall through to the developer's real configuration directory. The module is
// not `#[cfg(test)]` because dependent crates' tests need it.
#[allow(clippy::expect_used)]
pub mod testing {
    //! Test support: run against a private, throwaway configuration directory.
    //!
    //! Anything that exercises [`load`](super::load) or
    //! [`store`](super::store) for real needs somewhere to put the file that
    //! is not the developer's own `~/.config/slateos`. That takes a scratch
    //! directory, an environment variable, and — because the environment is
    //! process-global while tests run in parallel — a lock. It lives here
    //! rather than in each crate's test module because every settings panel
    //! that gains persistence will want it, and three copies of a
    //! `set_var`/restore dance is three chances to leave `$HOME` pointing at a
    //! deleted directory for the rest of the run.

    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// The environment is process-global, so callers take turns rather than
    /// racing each other over `XDG_CONFIG_HOME`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run `body` with the configuration directory pointed at a fresh empty
    /// directory, which is removed afterwards. The directory is passed in so
    /// the body can inspect what was written.
    ///
    /// # Panics
    ///
    /// If the scratch directory cannot be created.
    pub fn with_scratch_config<T>(tag: &str, body: impl FnOnce(&Path) -> T) -> T {
        // A poisoned lock means some other test panicked while holding it;
        // the environment was still restored by the guard below, so there is
        // nothing to recover and no reason to fail this test too.
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let mut root = env::temp_dir();
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        root.push(format!("slateos-scratch-{tag}-{id}"));
        fs::create_dir_all(&root).expect("create scratch config dir");

        let old_xdg = env::var_os("XDG_CONFIG_HOME");
        let old_home = env::var_os("HOME");
        // SAFETY: the lock above makes this the only thread touching the
        // environment for the duration, which is what `set_var` requires.
        unsafe {
            env::set_var("XDG_CONFIG_HOME", &root);
            // Removed as well as overridden: `config_dir` prefers XDG, but a
            // test that clears XDG itself should not fall through to the
            // developer's real home.
            env::remove_var("HOME");
        }

        let out = body(&root);

        // SAFETY: as above.
        unsafe {
            match old_xdg {
                Some(v) => env::set_var("XDG_CONFIG_HOME", v),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
            match old_home {
                Some(v) => env::set_var("HOME", v),
                None => env::remove_var("HOME"),
            }
        }
        // Best effort: a scratch directory left behind in the system temp
        // directory is litter, not a failure worth masking the test result.
        let _ = fs::remove_dir_all(&root);
        drop(guard);
        out
    }

    /// The file a settings group would be written to inside the scratch
    /// directory `root`.
    #[must_use]
    pub fn scratch_path(root: &Path, name: &str) -> PathBuf {
        let mut path = root.join("slateos").join(name);
        path.set_extension("yaml");
        path
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// `env::set_var` is process-global, so these tests take turns rather than
    /// racing each other over `HOME`.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `body` with the config-directory variables set as given, restoring
    /// whatever the process had before.
    fn with_env<T>(xdg: Option<&str>, home: Option<&str>, body: impl FnOnce() -> T) -> T {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old_xdg = env::var_os("XDG_CONFIG_HOME");
        let old_home = env::var_os("HOME");
        // SAFETY: the lock above makes this the only thread touching the
        // environment for the duration, which is what `set_var` requires.
        unsafe {
            match xdg {
                Some(v) => env::set_var("XDG_CONFIG_HOME", v),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
            match home {
                Some(v) => env::set_var("HOME", v),
                None => env::remove_var("HOME"),
            }
        }
        let out = body();
        // SAFETY: as above.
        unsafe {
            match old_xdg {
                Some(v) => env::set_var("XDG_CONFIG_HOME", v),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
            match old_home {
                Some(v) => env::set_var("HOME", v),
                None => env::remove_var("HOME"),
            }
        }
        drop(guard);
        out
    }

    /// A scratch directory that removes itself.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = env::temp_dir();
            let id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            path.push(format!("slateos-cfg-{tag}-{id}"));
            fs::create_dir_all(&path).expect("create scratch dir");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn xdg_config_home_wins_over_home() {
        let dir = with_env(Some("/x"), Some("/h"), config_dir);
        assert_eq!(dir, Some(PathBuf::from("/x").join("slateos")));
    }

    #[test]
    fn home_is_the_fallback() {
        let dir = with_env(None, Some("/h"), config_dir);
        assert_eq!(
            dir,
            Some(PathBuf::from("/h").join(".config").join("slateos"))
        );
    }

    #[test]
    fn an_empty_variable_is_not_a_location() {
        assert_eq!(with_env(Some(""), None, config_dir), None);
        assert_eq!(with_env(None, Some(""), config_dir), None);
        assert_eq!(with_env(None, None, config_dir), None);
    }

    #[test]
    fn a_name_becomes_a_yaml_file() {
        let path = with_env(Some("/x"), None, || path_for("appearance"));
        assert_eq!(
            path,
            Some(PathBuf::from("/x").join("slateos").join("appearance.yaml"))
        );
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_document() {
        let temp = TempDir::new("missing");
        let doc = with_env(Some(temp.0.to_str().unwrap()), None, || {
            load("nothing-here")
        });
        assert!(doc.is_empty());
    }

    #[test]
    fn with_no_home_a_store_reports_it_instead_of_guessing() {
        let mut doc = Document::new();
        doc.set_i64(&["a"], 1);
        let err = with_env(Some(""), Some(""), || store("appearance", &doc))
            .expect_err("storing with no home should fail");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_stored_document_reloads_as_itself() {
        let temp = TempDir::new("roundtrip");
        let text = "# hand-written\nfonts:\n  size: 13  # points\n";
        let doc = Document::parse(text);
        with_env(Some(temp.0.to_str().unwrap()), None, || {
            store("appearance", &doc).expect("store");
            // The comment and the layout have to come back, not just the value.
            assert_eq!(load("appearance").to_text(), text);
            // And the file really is where `path_for` says it is.
            let path = path_for("appearance").expect("path");
            assert_eq!(fs::read_to_string(&path).expect("read"), text);
        });
    }

    #[test]
    fn a_second_store_leaves_no_temporary_behind() {
        let temp = TempDir::new("clean");
        with_env(Some(temp.0.to_str().unwrap()), None, || {
            let mut doc = Document::parse("a: 1\n");
            store("appearance", &doc).expect("first store");
            doc.set_i64(&["a"], 2);
            store("appearance", &doc).expect("second store");
            assert_eq!(load("appearance").get_i64(&["a"]), Some(2));
            let dir = config_dir().expect("dir");
            let names: Vec<_> = fs::read_dir(&dir)
                .expect("read dir")
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names, vec!["appearance.yaml".to_owned()]);
        });
    }

    #[test]
    fn store_creates_the_directory() {
        let temp = TempDir::new("mkdir");
        let nested = temp.0.join("deep").join("deeper");
        with_env(Some(nested.to_str().unwrap()), None, || {
            store("appearance", &Document::parse("a: 1\n")).expect("store");
            assert_eq!(load("appearance").get_i64(&["a"]), Some(1));
        });
    }
}
