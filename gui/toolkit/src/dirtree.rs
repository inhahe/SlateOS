//! Populating a tree from a directory.
//!
//! `design.txt` asks for a "tristate checkbox treeview — good for selecting
//! files and directories, have function to populate it with a directory". The
//! treeview is [`crate::treeview`]; this is the function. [`DirectoryTree::open`]
//! shows one directory as a tree, reads each folder as it is opened, and —
//! built with [`DirectoryTree::open_checkable`] — lets the user tick what they
//! want, answering "is this path included?" for any path under it, read or
//! not.
//!
//! # Reading is lazy, and it has to be
//!
//! Nothing is read until a folder is opened. A home directory can hold hundreds
//! of thousands of files, and a tree that read all of them before drawing its
//! first row would never draw it. [`DirectorySource`] answers "not loaded yet"
//! for an unread folder, the view shows it as openable, and opening it reads
//! it — [`DirectoryTree`] does that for its caller, which is most of why it
//! exists.
//!
//! A folder is read *every* time it is opened, not only the first. Nothing
//! watches the disk for changes, so a listing kept from an earlier opening is a
//! listing of how the folder was then; closing a folder and opening it again is
//! how a user asks to see it as it is now, and one directory read is a cheap
//! answer. [`DirectoryTree::refresh`] re-reads everything that is open.
//!
//! # Names are bytes
//!
//! A node's key is the entry's name as an [`OsString`], exactly as the
//! directory gave it. A name on this system may be any bytes but `/` and NUL,
//! and a path rebuilt from a *displayed* name — `to_string_lossy`, with U+FFFD
//! where the bytes were not text — is a different path that looks the same on
//! screen. So the label is lossy, because a row has to show *something*, and
//! everything that names a file again is built from the key.
//!
//! # What is not followed
//!
//! A symbolic link is shown and never opened, even when it points at a folder.
//! Following links makes a tree a graph: a link to an ancestor loops forever,
//! and a link to a sibling shows the same files twice — which, in a tree used
//! to choose what to back up, is the same files *counted* twice. Its row says
//! "link", so the missing arrow is explained rather than mysterious.
//!
//! # A folder that cannot be read
//!
//! One that fails to open — permission denied, removed underneath the view — is
//! drawn disabled, with the error as the reason a pointer or the keyboard can
//! reach ([`crate::treeview::TreeView::hovered_reason`]). It still takes part
//! in the ticks: included by its parent's rule like anything else, since what
//! to do about an unreadable folder in a backup is the backup's decision, and
//! it can only make it if the folder is not silently dropped from the set.
//! Entries a listing could not read individually are counted on the folder's
//! row rather than left out without a word.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::dialog::sort_key;
use crate::event::{KeyEvent, MouseEvent};
use crate::palette::Palette;
use crate::render::RenderCommand;
use crate::treeview::{TreeEvent, TreeItem, TreeSource, TreeView};

/// What a directory listing shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirOptions {
    /// Show files as well as folders. Off for a folder chooser.
    pub files: bool,
    /// Show names starting with `.`.
    pub hidden: bool,
}

impl Default for DirOptions {
    /// Files shown, hidden names not — what a file manager starts with.
    fn default() -> Self {
        Self {
            files: true,
            hidden: false,
        }
    }
}

/// What kind of thing a directory entry is, without following links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A folder: the only kind that opens.
    Directory,
    /// An ordinary file.
    File,
    /// A symbolic link, to anything. Never followed; see the module docs.
    Link,
    /// A device, socket, pipe, or a type the listing could not determine.
    Other,
}

/// One entry of a read folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntryInfo {
    /// The name, exactly as the directory gave it.
    pub name: OsString,
    /// What it is.
    pub kind: EntryKind,
    /// Its size in bytes, for a file whose size could be read.
    pub size: Option<u64>,
}

/// The state of one folder's listing.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Listing {
    /// Read. `unreadable` counts entries that the directory iterator yielded
    /// as errors: they have no name to show, so they are counted instead.
    Read {
        entries: Vec<DirEntryInfo>,
        unreadable: usize,
    },
    /// Could not be read at all, with the error to show as the reason.
    Failed(String),
}

/// A [`TreeSource`] over a directory on disk, read one folder at a time.
///
/// Keys are entry names; the empty path is the root directory itself, whose
/// entries are the top-level rows.
#[derive(Clone, Debug)]
pub struct DirectorySource {
    root: PathBuf,
    options: DirOptions,
    listings: BTreeMap<Vec<OsString>, Listing>,
}

impl DirectorySource {
    /// A source for `root`, with nothing read yet.
    pub fn new(root: impl Into<PathBuf>, options: DirOptions) -> Self {
        Self {
            root: root.into(),
            options,
            listings: BTreeMap::new(),
        }
    }

    /// The directory whose entries are the top-level rows.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What the listings show.
    #[must_use]
    pub fn options(&self) -> DirOptions {
        self.options
    }

    /// The path on disk of the node at `keys`.
    #[must_use]
    pub fn path_of(&self, keys: &[OsString]) -> PathBuf {
        let mut path = self.root.clone();
        for key in keys {
            path.push(key);
        }
        path
    }

    /// The keys of `path`, which must be the root or inside it; `None` for
    /// one that is not, or that climbs out with `..`.
    ///
    /// For a caller restoring a saved selection: rules are saved as paths and
    /// applied as keys.
    #[must_use]
    pub fn keys_of(&self, path: &Path) -> Option<Vec<OsString>> {
        let rest = path.strip_prefix(&self.root).ok()?;
        let mut keys = Vec::new();
        for component in rest.components() {
            match component {
                Component::Normal(name) => keys.push(name.to_os_string()),
                Component::CurDir => {}
                _ => return None,
            }
        }
        Some(keys)
    }

    /// Whether the folder at `keys` has been read (or has failed to be).
    #[must_use]
    pub fn is_loaded(&self, keys: &[OsString]) -> bool {
        self.listings.contains_key(keys)
    }

    /// The entries of a read folder.
    #[must_use]
    pub fn entries(&self, keys: &[OsString]) -> Option<&[DirEntryInfo]> {
        match self.listings.get(keys)? {
            Listing::Read { entries, .. } => Some(entries),
            Listing::Failed(_) => None,
        }
    }

    /// Why the folder at `keys` could not be read, if it could not.
    #[must_use]
    pub fn failure(&self, keys: &[OsString]) -> Option<&str> {
        match self.listings.get(keys)? {
            Listing::Failed(reason) => Some(reason),
            Listing::Read { .. } => None,
        }
    }

    /// Read the folder at `keys`, replacing whatever was read before.
    ///
    /// A failure is recorded as well as returned, so the view can show the
    /// folder as unreadable and say why; the caller need not do anything with
    /// the error beyond what it wants to.
    pub fn load(&mut self, keys: &[OsString]) -> io::Result<()> {
        let dir = self.path_of(keys);
        match read_listing(&dir, self.options) {
            Ok((entries, unreadable)) => {
                self.listings.insert(
                    keys.to_vec(),
                    Listing::Read {
                        entries,
                        unreadable,
                    },
                );
                Ok(())
            }
            Err(error) => {
                self.listings
                    .insert(keys.to_vec(), Listing::Failed(error.to_string()));
                Err(error)
            }
        }
    }

    /// Re-read every folder already read at or below `keys` — "refresh".
    ///
    /// A folder that no longer exists is forgotten rather than recorded as
    /// failed: it has gone from its parent's listing too, so there is no row
    /// left to show a failure on. Returns the first other failure, having
    /// still re-read the rest; each one is recorded on its own folder.
    pub fn reload(&mut self, keys: &[OsString]) -> io::Result<()> {
        let loaded: Vec<Vec<OsString>> = self
            .listings
            .keys()
            .filter(|path| path.starts_with(keys))
            .cloned()
            .collect();
        let mut first_error = None;
        for path in loaded {
            if let Err(error) = self.load(&path) {
                if error.kind() == io::ErrorKind::NotFound && !path.is_empty() {
                    self.listings.remove(&path);
                } else if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Forget the folder at `keys` and everything read beneath it.
    pub fn unload(&mut self, keys: &[OsString]) {
        self.listings.retain(|path, _| !path.starts_with(keys));
    }
}

impl TreeSource for DirectorySource {
    type Key = OsString;

    fn children(&self, parent: &[OsString]) -> Option<Vec<TreeItem<OsString>>> {
        let entries = match self.listings.get(parent)? {
            Listing::Read { entries, .. } => entries,
            // Known, and known to show nothing: the folder's own row says why.
            Listing::Failed(_) => return Some(Vec::new()),
        };
        let mut probe = parent.to_vec();
        let items = entries
            .iter()
            .map(|entry| {
                probe.push(entry.name.clone());
                let item = self.item_for(entry, &probe);
                probe.pop();
                item
            })
            .collect();
        Some(items)
    }
}

impl DirectorySource {
    /// The row for `entry`, whose own key path is `keys`.
    fn item_for(&self, entry: &DirEntryInfo, keys: &[OsString]) -> TreeItem<OsString> {
        // Display only. The key below is what names the file again.
        let label = pathcodec::display_os(&entry.name);
        let key = entry.name.clone();
        match entry.kind {
            EntryKind::Directory => {
                let item = TreeItem::branch(key, label);
                match self.listings.get(keys) {
                    Some(Listing::Failed(reason)) => item.disabled(reason.clone()),
                    Some(Listing::Read { unreadable, .. }) if *unreadable > 0 => {
                        item.with_detail(format!("{unreadable} unreadable"))
                    }
                    _ => item,
                }
            }
            EntryKind::File => {
                let item = TreeItem::leaf(key, label);
                match entry.size {
                    Some(size) => item.with_detail(crate::bytes::iec(size)),
                    None => item,
                }
            }
            EntryKind::Link => TreeItem::leaf(key, label).with_detail("link"),
            EntryKind::Other => TreeItem::leaf(key, label),
        }
    }
}

/// Read one directory: its entries in display order, and how many the
/// iterator could not produce.
fn read_listing(dir: &Path, options: DirOptions) -> io::Result<(Vec<DirEntryInfo>, usize)> {
    let mut entries = Vec::new();
    let mut unreadable: usize = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                // No name to show and nothing to retry; counted, and the count
                // is drawn on the folder's row (see `item_for`).
                unreadable = unreadable.saturating_add(1);
                continue;
            }
        };
        let name = entry.file_name();
        if !options.hidden && is_hidden(&name) {
            continue;
        }
        // `DirEntry::file_type` does not follow links, on any platform --
        // which is what keeps a link from being opened as the folder it
        // points at.
        let kind = match entry.file_type() {
            Ok(t) if t.is_symlink() => EntryKind::Link,
            Ok(t) if t.is_dir() => EntryKind::Directory,
            Ok(t) if t.is_file() => EntryKind::File,
            _ => EntryKind::Other,
        };
        if !options.files && kind != EntryKind::Directory {
            continue;
        }
        // A size that cannot be read leaves the detail blank rather than
        // claiming zero bytes.
        let size = match kind {
            EntryKind::File => entry.metadata().ok().map(|m| m.len()),
            _ => None,
        };
        entries.push(DirEntryInfo { name, kind, size });
    }
    entries.sort_by(|a, b| {
        let a_dir = a.kind == EntryKind::Directory;
        let b_dir = b.kind == EntryKind::Directory;
        // Folders first, as in the file dialog; then by the dialog's own name
        // key, so the two list a folder in the same order; then by the exact
        // bytes, so that `README` and `readme` are not left to the order the
        // directory happened to return them in.
        b_dir
            .cmp(&a_dir)
            .then_with(|| sort_key(&a.name).cmp(&sort_key(&b.name)))
            .then_with(|| a.name.as_encoded_bytes().cmp(b.name.as_encoded_bytes()))
    });
    Ok((entries, unreadable))
}

/// Whether a name is hidden by this system's convention: it starts with `.`.
fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// A directory shown as a tree: a [`TreeView`] and the [`DirectorySource`]
/// behind it, with each folder read the first time it is opened.
///
/// The one piece a caller of the two separately would have to write is the
/// reaction to [`TreeEvent::Expanded`] — read the folder, then refresh — and
/// every caller would have to write it identically, so it is here.
#[derive(Clone, Debug)]
pub struct DirectoryTree {
    view: TreeView<OsString>,
    source: DirectorySource,
}

impl DirectoryTree {
    /// Show `root` as a plain tree. Reads the top level.
    ///
    /// Fails only if `root` itself cannot be read — a tree of nothing is not a
    /// useful thing to hand back.
    pub fn open(root: impl Into<PathBuf>, options: DirOptions) -> io::Result<Self> {
        Self::with_view(root.into(), options, TreeView::new())
    }

    /// Show `root` as a tristate checkbox tree, with everything
    /// `default_included` until ticked otherwise.
    pub fn open_checkable(
        root: impl Into<PathBuf>,
        options: DirOptions,
        default_included: bool,
    ) -> io::Result<Self> {
        Self::with_view(root.into(), options, TreeView::checkable(default_included))
    }

    fn with_view(
        root: PathBuf,
        options: DirOptions,
        mut view: TreeView<OsString>,
    ) -> io::Result<Self> {
        let mut source = DirectorySource::new(root, options);
        source.load(&[])?;
        view.refresh(&source);
        Ok(Self { view, source })
    }

    /// The view, for its read-only state: rows, selection, ticks.
    #[must_use]
    pub fn view(&self) -> &TreeView<OsString> {
        &self.view
    }

    /// The view, for what does not need the source: bounds, metrics,
    /// enabling and disabling, scrolling, restoring saved ticks.
    pub fn view_mut(&mut self) -> &mut TreeView<OsString> {
        &mut self.view
    }

    /// The source, for what has been read.
    #[must_use]
    pub fn source(&self) -> &DirectorySource {
        &self.source
    }

    /// Act on a key; see [`TreeView::handle_key`].
    pub fn handle_key(&mut self, key: &KeyEvent) -> Vec<TreeEvent<OsString>> {
        let events = self.view.handle_key(key, &self.source);
        self.load_opened(events)
    }

    /// Act on a mouse event; see [`TreeView::handle_mouse`].
    pub fn handle_mouse(&mut self, event: &MouseEvent) -> Vec<TreeEvent<OsString>> {
        let events = self.view.handle_mouse(event, &self.source);
        self.load_opened(events)
    }

    /// Draw it; see [`TreeView::render`].
    #[must_use]
    pub fn render(&self, palette: &Palette) -> Vec<RenderCommand> {
        self.view.render(palette)
    }

    /// Open every folder down to `path`, reading each as it goes, then select
    /// it. Returns whether `path` ended up selected — it will not if it is not
    /// under the root or does not exist.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let Some(keys) = self.source.keys_of(path) else {
            return false;
        };
        for len in 1..keys.len() {
            let Some(folder) = keys.get(..len) else {
                break;
            };
            // A folder being opened now is read now, as a click would read it;
            // one already open keeps the listing it is showing.
            if !self.view.is_expanded(folder) || !self.source.is_loaded(folder) {
                // Recorded on the folder if it fails; the reveal then stops
                // short of the path, and `selected` says so.
                if self.source.load(folder).is_err() {
                    break;
                }
            }
        }
        self.view.reveal(&keys, &self.source);
        self.view.selected() == Some(keys.as_slice())
    }

    /// Re-read everything that has been read, and redraw.
    ///
    /// Returns the view's events — a selection that moved because its file was
    /// deleted — and the first read failure, if there was one.
    pub fn refresh(&mut self) -> (Vec<TreeEvent<OsString>>, io::Result<()>) {
        let result = self.source.reload(&[]);
        (self.view.refresh(&self.source), result)
    }

    /// The selected entry's path on disk.
    #[must_use]
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.view.selected().map(|keys| self.source.path_of(keys))
    }

    /// Whether `path` is included by the ticks — for a path that was never
    /// shown as much as for one that was. `None` for a plain tree, and for a
    /// path outside the root.
    #[must_use]
    pub fn is_included(&self, path: &Path) -> Option<bool> {
        let keys = self.source.keys_of(path)?;
        Some(self.view.check_rules()?.is_included(&keys))
    }

    /// The ticks as paths, shallowest first — what to save, and what a backup
    /// reads: `(/home, true), (/home/u/.cache, false)` is "`/home`, except the
    /// cache". Empty for a plain tree. The default for paths no rule reaches
    /// is [`crate::treeview::CheckRules::default_included`].
    #[must_use]
    pub fn inclusion_rules(&self) -> Vec<(PathBuf, bool)> {
        let Some(rules) = self.view.check_rules() else {
            return Vec::new();
        };
        rules
            .rules()
            .map(|(keys, included)| (self.source.path_of(keys), included))
            .collect()
    }

    /// Read each folder the view has just opened — again, if it was read
    /// before; see the module docs — and redraw with its contents. The events
    /// pass through, followed by any the redraw produced.
    fn load_opened(&mut self, events: Vec<TreeEvent<OsString>>) -> Vec<TreeEvent<OsString>> {
        let mut loaded_any = false;
        for event in &events {
            if let TreeEvent::Expanded(keys) = event {
                // A failure is recorded on the folder and drawn as its disabled
                // reason; there is nothing more to do with it here.
                let _recorded = self.source.load(keys);
                loaded_any = true;
            }
        }
        if loaded_any {
            let mut events = events;
            events.extend(self.view.refresh(&self.source));
            return events;
        }
        events
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::event::{Key, Modifiers};
    use crate::frame::Rect;
    use crate::widget::CheckState;
    use std::fs;

    /// A scratch directory removed when dropped, named for the test so two
    /// tests running at once cannot share one.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("guitk-dirtree-{name}-{}", std::process::id()));
            // Left over from a run that was killed; stale, and ours.
            let _stale = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, rel: &str, bytes: usize) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, vec![b'x'; bytes]).unwrap();
        }

        fn dir(&self, rel: &str) {
            fs::create_dir_all(self.0.join(rel)).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leftover temp directory is harmless, and a panic
            // in a destructor during a failing test would hide its message.
            let _cleanup = fs::remove_dir_all(&self.0);
        }
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn labels(tree: &DirectoryTree) -> Vec<String> {
        tree.view()
            .rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth()), r.label))
            .collect()
    }

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn folders_come_first_then_names_ignoring_case() {
        let scratch = Scratch::new("order");
        scratch.file("b.txt", 1);
        scratch.file("A.txt", 1);
        scratch.dir("zeta");
        scratch.dir("Alpha");
        scratch.file("c.TXT", 1);
        let tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        assert_eq!(labels(&tree), ["Alpha", "zeta", "A.txt", "b.txt", "c.TXT"]);
    }

    #[test]
    fn a_folder_is_read_the_first_time_it_is_opened_and_not_before() {
        let scratch = Scratch::new("lazy");
        scratch.file("docs/report.txt", 2048);
        scratch.file("docs/deep/x.bin", 1);
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        assert!(
            !tree.source().is_loaded(&[os("docs")]),
            "nothing below the top level yet"
        );
        tree.handle_key(&key(Key::Down));
        let events = tree.handle_key(&key(Key::Right));
        assert_eq!(events, [TreeEvent::Expanded(vec![os("docs")])]);
        assert!(tree.source().is_loaded(&[os("docs")]));
        assert!(
            !tree.source().is_loaded(&[os("docs"), os("deep")]),
            "only what was opened"
        );
        assert_eq!(labels(&tree), ["docs", "  deep", "  report.txt"]);
        let report = &tree.view().rows()[2];
        assert_eq!(
            report.detail.as_deref(),
            Some("2.0 KiB"),
            "a file shows its size"
        );
    }

    #[test]
    fn closing_a_folder_and_opening_it_again_shows_it_as_it_is_now() {
        let scratch = Scratch::new("reopen");
        scratch.file("inbox/old.txt", 1);
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        tree.handle_key(&key(Key::Down));
        tree.handle_key(&key(Key::Right));
        assert_eq!(labels(&tree), ["inbox", "  old.txt"]);
        scratch.file("inbox/new.txt", 1);
        assert_eq!(
            labels(&tree),
            ["inbox", "  old.txt"],
            "nothing watches the disk"
        );
        tree.handle_key(&key(Key::Left));
        tree.handle_key(&key(Key::Right));
        assert_eq!(
            labels(&tree),
            ["inbox", "  new.txt", "  old.txt"],
            "reopening read it again"
        );
    }

    #[test]
    fn hidden_names_and_files_are_shown_only_when_asked_for() {
        let scratch = Scratch::new("options");
        scratch.file(".secret", 1);
        scratch.dir(".config");
        scratch.dir("music");
        scratch.file("notes.txt", 1);
        let plain = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        assert_eq!(labels(&plain), ["music", "notes.txt"]);
        let folders = DirectoryTree::open(
            &scratch.0,
            DirOptions {
                files: false,
                hidden: false,
            },
        )
        .unwrap();
        assert_eq!(labels(&folders), ["music"]);
        let all = DirectoryTree::open(
            &scratch.0,
            DirOptions {
                files: true,
                hidden: true,
            },
        )
        .unwrap();
        assert_eq!(labels(&all), [".config", "music", ".secret", "notes.txt"]);
    }

    #[test]
    fn a_folder_that_vanishes_before_it_is_opened_is_shown_disabled_with_the_reason() {
        let scratch = Scratch::new("vanish");
        scratch.dir("gone");
        scratch.dir("kept");
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        fs::remove_dir(scratch.0.join("gone")).unwrap();
        tree.handle_key(&key(Key::Down));
        tree.handle_key(&key(Key::Right));
        let gone = &tree.view().rows()[0];
        assert_eq!(gone.label, "gone");
        assert!(
            gone.state.is_disabled(),
            "an unreadable folder is greyed out"
        );
        assert!(!gone.expandable, "and shows no children");
        assert!(tree.view().selected_reason().is_some(), "and says why");
        assert!(tree.source().failure(&[os("gone")]).is_some());

        // A refresh notices it is gone altogether.
        let (events, result) = tree.refresh();
        assert!(
            result.is_ok(),
            "a folder that no longer exists is not an error: {result:?}"
        );
        assert_eq!(labels(&tree), ["kept"]);
        assert_eq!(events, [TreeEvent::SelectionCleared]);
    }

    #[test]
    fn refresh_picks_up_new_files_and_keeps_what_was_open() {
        let scratch = Scratch::new("refresh");
        scratch.file("src/main.rs", 10);
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        assert!(tree.reveal(&scratch.0.join("src").join("main.rs")));
        scratch.file("src/lib.rs", 10);
        let (events, result) = tree.refresh();
        result.unwrap();
        assert!(events.is_empty(), "the selection's file is still there");
        assert_eq!(labels(&tree), ["src", "  lib.rs", "  main.rs"]);
        assert_eq!(
            tree.selected_path(),
            Some(scratch.0.join("src").join("main.rs"))
        );
    }

    #[test]
    fn reveal_opens_the_folders_on_the_way_and_refuses_paths_outside() {
        let scratch = Scratch::new("reveal");
        scratch.file("a/b/c/d.txt", 1);
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        let target = scratch.0.join("a").join("b").join("c").join("d.txt");
        assert!(tree.reveal(&target));
        assert_eq!(tree.selected_path(), Some(target));
        assert_eq!(labels(&tree), ["a", "  b", "    c", "      d.txt"]);
        assert!(!tree.reveal(&std::env::temp_dir()), "outside the root");
        assert!(!tree.reveal(&scratch.0.join("a").join("missing.txt")));
    }

    #[test]
    fn ticks_answer_for_files_that_were_never_shown_and_save_as_paths() {
        let scratch = Scratch::new("ticks");
        scratch.file("home/u/docs/a.txt", 1);
        scratch.file("home/u/.cache/blob", 1);
        scratch.file("etc/hosts", 1);
        let options = DirOptions {
            files: true,
            hidden: true,
        };
        let mut tree = DirectoryTree::open_checkable(&scratch.0, options, false).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 480.0));
        // Tick /home with the keyboard.
        tree.handle_key(&key(Key::End));
        tree.handle_key(&key(Key::Home));
        let home = scratch.0.join("home");
        assert_eq!(tree.selected_path(), Some(scratch.0.join("etc")));
        tree.handle_key(&key(Key::Down));
        assert_eq!(tree.selected_path(), Some(home.clone()));
        tree.handle_key(&key(Key::Space));
        assert_eq!(
            tree.is_included(&home.join("u").join("docs").join("a.txt")),
            Some(true)
        );
        assert_eq!(
            tree.is_included(&home.join("u").join("created-tomorrow")),
            Some(true),
            "a rule on a folder covers what is not there yet"
        );
        assert_eq!(
            tree.is_included(&scratch.0.join("etc").join("hosts")),
            Some(false)
        );

        // Open down to the cache and untick it.
        assert!(tree.reveal(&home.join("u").join(".cache")));
        tree.handle_key(&key(Key::Space));
        assert_eq!(
            tree.view().check_state(&[os("home")]),
            Some(CheckState::Indeterminate)
        );
        assert_eq!(
            tree.inclusion_rules(),
            [(home.clone(), true), (home.join("u").join(".cache"), false)]
        );
        assert_eq!(
            tree.is_included(&std::env::temp_dir()),
            None,
            "outside the root"
        );
    }

    #[test]
    fn a_plain_tree_has_no_ticks_to_report() {
        let scratch = Scratch::new("plain");
        scratch.file("x", 1);
        let tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        assert_eq!(tree.is_included(&scratch.0.join("x")), None);
        assert!(tree.inclusion_rules().is_empty());
    }

    #[test]
    fn a_root_that_cannot_be_read_is_an_error_not_an_empty_tree() {
        let scratch = Scratch::new("noroot");
        let missing = scratch.0.join("does-not-exist");
        assert!(DirectoryTree::open(&missing, DirOptions::default()).is_err());
    }

    #[test]
    fn keys_and_paths_round_trip_and_dot_dot_is_refused() {
        let source = DirectorySource::new("/srv/data", DirOptions::default());
        let path = source.path_of(&[os("a"), os("b c")]);
        assert_eq!(source.keys_of(&path), Some(vec![os("a"), os("b c")]));
        assert_eq!(source.keys_of(Path::new("/srv/data")), Some(Vec::new()));
        assert_eq!(
            source.keys_of(&Path::new("/srv/data").join("..").join("etc")),
            None
        );
        assert_eq!(source.keys_of(Path::new("/elsewhere")), None);
    }

    /// A name that is not UTF-8 keeps its exact bytes as the key, so the path
    /// rebuilt from it names the real file, while the label shows the byte as
    /// an octal escape, as the terminal spells it.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_still_names_its_file() {
        use std::os::unix::ffi::OsStrExt;
        let scratch = Scratch::new("bytes");
        let raw = OsStr::from_bytes(b"caf\xE9.txt");
        fs::write(scratch.0.join(raw), b"x").unwrap();
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        tree.handle_key(&key(Key::Down));
        let path = tree.selected_path().unwrap();
        assert_eq!(path.file_name(), Some(raw));
        assert!(path.exists(), "the rebuilt path is the real file");
        assert_eq!(tree.view().rows()[0].label, r"caf\351.txt");
    }

    /// The same, for the host these tests usually run on: a Windows name with
    /// an unpaired surrogate is not valid Unicode, and must survive the same
    /// way.
    #[cfg(windows)]
    #[test]
    fn a_name_that_is_not_text_still_names_its_file() {
        use std::os::windows::ffi::OsStringExt;
        let scratch = Scratch::new("bytes");
        // "caf" + a lone high surrogate + ".txt".
        let wide: Vec<u16> = "caf"
            .encode_utf16()
            .chain([0xD800])
            .chain(".txt".encode_utf16())
            .collect();
        let raw = OsString::from_wide(&wide);
        if fs::write(scratch.0.join(&raw), b"x").is_err() {
            // Some filesystems refuse unpaired surrogates outright; then
            // there is nothing to list and nothing to test. Said out loud,
            // so a run that skipped is not mistaken for one that passed.
            eprintln!("SKIPPED: this filesystem refused an unpaired surrogate");
            return;
        }
        let mut tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        tree.view_mut()
            .set_bounds(Rect::new(0.0, 0.0, 300.0, 240.0));
        tree.handle_key(&key(Key::Down));
        let path = tree.selected_path().unwrap();
        assert_eq!(path.file_name(), Some(raw.as_os_str()));
        assert!(path.exists(), "the rebuilt path is the real file");
        assert_eq!(tree.view().rows()[0].label, r"caf\355\240\200.txt");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_shown_and_never_opened() {
        let scratch = Scratch::new("link");
        scratch.dir("real");
        std::os::unix::fs::symlink(scratch.0.join("real"), scratch.0.join("loop")).unwrap();
        let tree = DirectoryTree::open(&scratch.0, DirOptions::default()).unwrap();
        let link = tree
            .view()
            .rows()
            .iter()
            .find(|r| r.label == "loop")
            .unwrap();
        assert!(!link.expandable);
        assert_eq!(link.detail.as_deref(), Some("link"));
    }
}
