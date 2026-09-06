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

/// Notices when a settings group's file has changed underneath a running
/// process.
///
/// # The problem this exists for
///
/// Two processes share every settings file: the panel that writes it and the
/// program the setting is *about*. Change the accent colour in Settings and
/// the running desktop keeps the old one, because nothing tells it to look
/// again -- the two agree only across a restart. That is the same class of bug
/// as a stale cache, and it is the last open half of both
/// `TD-APPEARANCE-SETTINGS-ARE-NEVER-WRITTEN-TO-DISK` and
/// `TD-THREE-INDEPENDENT-APPEARANCE-MODELS`.
///
/// # Why it compares contents rather than a timestamp
///
/// The obvious watcher stats the file and compares the modification time. It
/// is wrong here in both directions:
///
/// - **It misses changes.** Timestamps are coarse -- one second on some
///   filesystems -- so two saves inside one tick are one timestamp. A user
///   dragging a colour slider produces exactly that.
/// - **It invents changes.** [`store`] writes a temporary and renames it over
///   the target, so every save lands a new inode with a new timestamp even
///   when the bytes are identical -- which is what saving a panel you did not
///   change does. A timestamp watcher would repaint the desktop each time.
///
/// Comparing the bytes has neither failure, and costs less than the
/// alternative rather than more: a settings file is a few kilobytes, so this
/// is one small read per look, where stat-then-read is two syscalls in the
/// case that matters. Should these files ever grow to where that is not true,
/// the cheap gate to add is a stat *in front of* the comparison, not in place
/// of it.
///
/// # Why there is no clock in it
///
/// [`poll`](Self::poll) looks exactly once and says what it found. It does not
/// know how often it is called and has no opinion about it, for the reason
/// `net80211::assoc`'s step function has no clock either: deciding *how often*
/// to look is a policy of whoever holds a timer, and a poller that rate-limits
/// itself cannot be driven at full speed by a test.
///
/// # What this is not
///
/// A poller, not a subscription. `design.txt` calls filesystem change
/// notification "kernel-level, essential", and when that exists this type is
/// where it plugs in -- `poll` keeps its signature and stops reading the file
/// on a caller that has been told nothing happened. Until the kernel can
/// deliver that, a process that wants to notice a change has to look, and the
/// alternative to looking cheaply is not looking at all. See `todo.txt`.
#[derive(Debug, Clone)]
pub struct Watcher {
    /// The settings group, as passed to [`load`] and [`store`].
    name: String,
    /// What the file held when it was last looked at.
    seen: Seen,
}

/// What a [`Watcher`] found the last time it looked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    /// Not looked yet, so the next look reports whatever is there.
    ///
    /// This is what lets a caller use the watcher for its *first* read as well
    /// as its later ones. A watcher constructed already-primed would leave the
    /// caller to load the file separately, and a save landing between that
    /// load and the priming would be invisible forever after -- a race that
    /// only shows up on a machine where something writes settings at startup.
    Unread,
    /// The file's exact contents.
    Contents(String),
    /// There was no readable file.
    ///
    /// Deliberately the same state for "not there" and "there but unreadable",
    /// because [`load`] already makes no distinction: both yield an empty
    /// document, which every reader turns into its defaults. A watcher that
    /// disagreed with `load` about that would make a program's startup state
    /// differ from the state it reloads into, which is precisely the kind of
    /// disagreement this type exists to remove.
    Absent,
}

impl Watcher {
    /// Watch a settings group, having seen nothing yet.
    ///
    /// The first [`poll`](Self::poll) therefore reports the current contents,
    /// so a caller can use one watcher for both its initial load and its
    /// reloads instead of reading the file two different ways.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            seen: Seen::Unread,
        }
    }

    /// The settings group being watched.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Look once. `Some(document)` if the file differs from the last look.
    ///
    /// Returns `None` when nothing has changed, which is the answer almost
    /// every time and is what makes it cheap to call often: the caller does no
    /// parsing, no re-derivation and no repaint.
    ///
    /// A file that is deleted reports `Some` too, holding an empty document --
    /// the settings have genuinely changed, back to their defaults, and a
    /// caller that ignored it would keep showing preferences the user has
    /// removed.
    pub fn poll(&mut self) -> Option<Document> {
        let found = match path_for(&self.name).map(fs::read_to_string) {
            Some(Ok(text)) => Seen::Contents(text),
            // No configuration directory, no file, or a file this process
            // cannot read -- all three are "nothing to read", as in `load`.
            None | Some(Err(_)) => Seen::Absent,
        };
        if found == self.seen {
            return None;
        }
        let doc = match &found {
            Seen::Contents(text) => Document::parse(text),
            Seen::Unread | Seen::Absent => Document::new(),
        };
        self.seen = found;
        Some(doc)
    }
}

// Test support, so panicking on a broken precondition is the right behaviour:
// a test whose sandbox cannot be created must fail loudly rather than quietly
// fall through to the developer's real configuration directory.
//
// The module cannot be `#[cfg(test)]` because dependent crates' *own* tests
// need it, and a `#[cfg(test)]` item is compiled only when its own crate is
// under test. It is instead behind a default-off feature, which a dependent
// turns on in its `[dev-dependencies]` — the same shape `safeio`'s `audit`
// counters use. The reason it is not simply unconditional: it needs
// `scratchdir`, whose whole point is to be a `[dev-dependencies]` entry that
// never reaches a target build, and an unconditional `pub mod testing` would
// have dragged it into the shipped compositor.
#[cfg(feature = "testing")]
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

    use scratchdir::ScratchDir;
    use std::env;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// The environment is process-global, so callers take turns rather than
    /// racing each other over `XDG_CONFIG_HOME`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Restores `XDG_CONFIG_HOME` and `HOME` to what the process had, on every
    /// exit path.
    ///
    /// This used to be straight-line code after the call to `body`, which meant
    /// a body that panicked — i.e. any failing assertion, which is the normal
    /// way a test ends badly — skipped the restore entirely and left
    /// `XDG_CONFIG_HOME` pointing at a directory that was about to be deleted.
    /// Every later test in the process then read its settings from a path that
    /// does not exist. That is precisely the failure this module's own doc
    /// comment says it exists to prevent, so the restore has to be a `Drop`.
    struct EnvRestore {
        xdg: Option<OsString>,
        home: Option<OsString>,
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            // SAFETY: `ENV_LOCK` is held for as long as this guard lives, so
            // this is the only thread touching the environment — which is what
            // `set_var`/`remove_var` require.
            unsafe {
                match self.xdg.take() {
                    Some(v) => env::set_var("XDG_CONFIG_HOME", v),
                    None => env::remove_var("XDG_CONFIG_HOME"),
                }
                match self.home.take() {
                    Some(v) => env::set_var("HOME", v),
                    None => env::remove_var("HOME"),
                }
            }
        }
    }

    /// Run `body` with the configuration directory pointed at a fresh empty
    /// directory, which is removed afterwards. The directory is passed in so
    /// the body can inspect what was written.
    ///
    /// # Panics
    ///
    /// If the scratch directory cannot be created.
    pub fn with_scratch_config<T>(tag: &str, body: impl FnOnce(&Path) -> T) -> T {
        // A poisoned lock means some other test panicked while holding it; the
        // environment was restored by `EnvRestore` on the way out of that
        // panic, so there is nothing to recover and no reason to fail this
        // test too.
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // Named from the process id and a per-process counter rather than from
        // the clock. The clock tag this replaces was not unique: `cargo test`
        // runs a binary's tests as threads of one process, and the clock a
        // thread reads is only refreshed on a timer interrupt, so every caller
        // that arrived within the same tick got the same directory. (The
        // `ENV_LOCK` above serialises callers *within* one process, but not
        // two test binaries running at once, which cargo does routinely.)
        let root = ScratchDir::new(&format!("slateos-scratch-{tag}"));

        let restore = EnvRestore {
            xdg: env::var_os("XDG_CONFIG_HOME"),
            home: env::var_os("HOME"),
        };
        // SAFETY: the lock above makes this the only thread touching the
        // environment for the duration, which is what `set_var` requires.
        unsafe {
            env::set_var("XDG_CONFIG_HOME", root.dir());
            // Removed as well as overridden: `config_dir` prefers XDG, but a
            // test that clears XDG itself should not fall through to the
            // developer's real home.
            env::remove_var("HOME");
        }

        let out = body(root.dir());

        // Explicit, so the order is stated rather than inherited from the
        // declaration order: environment first (while the directory it names
        // still exists), then the directory, then the lock.
        drop(restore);
        drop(root);
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
    use scratchdir::ScratchDir;

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

    // -- Watcher --

    /// Run `body` against a private configuration directory, with a `write`
    /// that puts a settings file into it.
    fn with_config_dir<T>(tag: &str, body: impl FnOnce(&dyn Fn(&str, &str)) -> T) -> T {
        let dir = ScratchDir::new(tag);
        let root = dir.path("cfg");
        let root_str = root.to_str().expect("scratch path is UTF-8").to_string();
        // Asks `path_for` where the file goes rather than rebuilding the path,
        // because a fixture that computes its own answer can disagree with the
        // code -- and the first version of this helper did exactly that,
        // dropping the `slateos` component and writing files the watcher was
        // never going to look at. Two of the tests still passed, on an absent
        // file behaving like an absent file.
        let write = |name: &str, text: &str| {
            let path = path_for(name).expect("a config path, the env being set");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create config dir");
            }
            fs::write(&path, text).expect("write settings file");
        };
        with_env(Some(&root_str), None, || body(&write))
    }

    #[test]
    fn the_first_look_reports_what_is_there() {
        with_config_dir("watch-first", |write| {
            write("appearance", "theme_mode: light\n");
            let mut w = Watcher::new("appearance");
            let doc = w
                .poll()
                .expect("the first look reports, having seen nothing");
            assert!(doc.to_text().contains("light"));
        });
    }

    #[test]
    fn the_first_look_reports_an_absent_file_too() {
        // Not a change *from* anything, but the caller has still learned the
        // current state -- which is the whole point of starting unread.
        with_config_dir("watch-absent", |_write| {
            let mut w = Watcher::new("appearance");
            let doc = w.poll().expect("an absent file is still a first answer");
            assert_eq!(doc.to_text(), Document::new().to_text());
            assert!(w.poll().is_none(), "and it does not repeat");
        });
    }

    #[test]
    fn an_unchanged_file_reports_nothing() {
        with_config_dir("watch-quiet", |write| {
            write("appearance", "theme_mode: light\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());
            assert!(w.poll().is_none());
            assert!(w.poll().is_none(), "and stays quiet");
        });
    }

    #[test]
    fn a_changed_file_reports_once() {
        with_config_dir("watch-change", |write| {
            write("appearance", "theme_mode: light\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());

            write("appearance", "theme_mode: dark\n");
            let doc = w.poll().expect("the change is reported");
            assert!(doc.to_text().contains("dark"));
            assert!(w.poll().is_none(), "and not reported twice");
        });
    }

    #[test]
    fn two_changes_inside_one_timestamp_are_both_seen() {
        // The first thing a modification-time watcher gets wrong. Timestamps
        // are coarse -- a second, on some filesystems -- so two writes in one
        // tick share one, and the second is invisible. Dragging a colour
        // slider produces exactly this. These writes are back to back, so if
        // the clock has any granularity at all they land inside it.
        with_config_dir("watch-fast", |write| {
            write("appearance", "accent: blue\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());

            write("appearance", "accent: red\n");
            write("appearance", "accent: green\n");
            let doc = w.poll().expect("the newest contents are seen");
            assert!(doc.to_text().contains("green"), "{}", doc.to_text());
        });
    }

    #[test]
    fn rewriting_the_same_bytes_is_not_a_change() {
        // The second thing a modification-time watcher gets wrong, and the
        // more damaging one here: `store` renames a temporary over the target,
        // so *every* save lands a new inode with a new timestamp -- including
        // saving a panel nothing was changed in. A timestamp watcher would
        // repaint the desktop each time somebody opened Settings and closed
        // it.
        with_config_dir("watch-rewrite", |write| {
            write("appearance", "theme_mode: dark\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());

            write("appearance", "theme_mode: dark\n");
            assert!(w.poll().is_none(), "identical bytes are not a change");
        });
    }

    #[test]
    fn a_real_store_is_seen_through_the_rename() {
        // The path that actually matters: not a test writing bytes, but
        // `store`'s write-a-temporary-and-rename. The watcher must follow the
        // target across the replacement, and must not be fooled by the `.new`
        // temporary sitting beside it.
        with_config_dir("watch-store", |write| {
            write("appearance", "theme_mode: dark\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());

            let mut doc = Document::parse("theme_mode: light\n");
            store("appearance", &doc).expect("store");
            let seen = w.poll().expect("a real save is seen");
            assert!(seen.to_text().contains("light"));

            // And the same document again is not a second change, even though
            // the rename has replaced the file underneath.
            doc = Document::parse(&seen.to_text());
            store("appearance", &doc).expect("store again");
            assert!(w.poll().is_none(), "an identical re-save is not a change");
        });
    }

    #[test]
    fn deleting_the_file_reports_the_defaults() {
        // The settings really have changed -- back to the defaults -- and a
        // caller that ignored it would keep showing preferences the user has
        // removed.
        with_config_dir("watch-delete", |write| {
            write("appearance", "theme_mode: light\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());

            let path = path_for("appearance").expect("a path");
            fs::remove_file(&path).expect("remove");
            let doc = w.poll().expect("deletion is a change");
            assert_eq!(doc.to_text(), Document::new().to_text());
            assert!(w.poll().is_none(), "and only reported once");
        });
    }

    #[test]
    fn a_file_that_comes_back_is_a_change_again() {
        with_config_dir("watch-return", |write| {
            write("appearance", "theme_mode: light\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());

            let path = path_for("appearance").expect("a path");
            fs::remove_file(&path).expect("remove");
            assert!(w.poll().is_some());

            write("appearance", "theme_mode: light\n");
            let doc = w.poll().expect("the file returning is a change");
            assert!(doc.to_text().contains("light"));
        });
    }

    #[test]
    fn watching_one_group_ignores_another() {
        with_config_dir("watch-groups", |write| {
            write("appearance", "theme_mode: dark\n");
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some());
            assert_eq!(w.name(), "appearance");

            write("input", "repeat_delay: 400\n");
            assert!(w.poll().is_none(), "another group is not this one");
        });
    }

    #[test]
    fn with_nowhere_to_look_the_answer_is_the_defaults_once() {
        // Neither variable set: `path_for` yields nothing, which is the same
        // "nothing to read" as an absent file, and must not report a change on
        // every single look.
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        drop(guard);
        with_env(None, None, || {
            let mut w = Watcher::new("appearance");
            assert!(w.poll().is_some(), "the first look still answers");
            assert!(w.poll().is_none());
            assert!(w.poll().is_none());
        });
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
        let temp = ScratchDir::new("slateos-cfg-missing");
        let doc = with_env(Some(temp.dir().to_str().unwrap()), None, || {
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
        let temp = ScratchDir::new("slateos-cfg-roundtrip");
        let text = "# hand-written\nfonts:\n  size: 13  # points\n";
        let doc = Document::parse(text);
        with_env(Some(temp.dir().to_str().unwrap()), None, || {
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
        let temp = ScratchDir::new("slateos-cfg-clean");
        with_env(Some(temp.dir().to_str().unwrap()), None, || {
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
        let temp = ScratchDir::new("slateos-cfg-mkdir");
        let nested = temp.dir().join("deep").join("deeper");
        with_env(Some(nested.to_str().unwrap()), None, || {
            store("appearance", &Document::parse("a: 1\n")).expect("store");
            assert_eq!(load("appearance").get_i64(&["a"]), Some(1));
        });
    }
}
