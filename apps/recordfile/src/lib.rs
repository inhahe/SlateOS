//! A program's data file, shared by its windows: each saves only its own
//! changes into what is on disk, and hears when another has written.
//!
//! The operator's answer to E-Q5 (design-decisions §1239): where two windows
//! of a program are useful -- notes, contacts, the kanban board, snippets,
//! the e-book reader's library, reminders, flashcards, finance -- opening it
//! twice must not lose anything. Each such program used to read its file when
//! a window opened and write its window's copy back whole, so whichever window
//! saved last wrote over the other's work
//! (`known-issues/E-two-windows-of-one-program-the-last-to-save-throws-away-what-the-other-saved.md`).
//!
//! What a program does instead, with the pieces here:
//!
//! 1. **Every record has an id no other window will give out** --
//!    [`fresh_id`], random rather than "the largest so far plus one", which
//!    two windows opened on one file would hand out twice.
//! 2. **A save reads the file first, and puts in only what this window
//!    changed** -- [`merge`], a three-way merge by id of what the window last
//!    read or wrote (the base), what it has now, and what the file has now.
//!    Two windows editing the same record at once is the one case where the
//!    later save wins, and only for that record.
//! 3. **A window hears when the file is written** -- [`Watch`], inotify on the
//!    file's folder, waking the window to read it again; [`Stamp`] tells a
//!    window's own write from another's.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// The largest id [`fresh_id`] gives: 2^52 - 1, so an id survives a JSON
/// number, which is an `f64` (exact to 2^53).
pub const MAX_ID: u64 = (1 << 52) - 1;

/// How long an in-place write must go quiet before the window is told: one
/// save's several writes are one change. As the settings watcher's.
pub const QUIET: Duration = Duration::from_millis(250);

/// How often the watch's thread looks up from waiting, to see whether it has
/// been asked to stop.
const POLL: Duration = Duration::from_millis(200);

/// Something kept in a data file, with an id that stays the same for as long
/// as it exists.
pub trait Record: Clone + PartialEq {
    /// Its id: unique in the file, and never reused while it exists.
    fn id(&self) -> u64;
}

/// A new id, in `1..=`[`MAX_ID`], that `taken` says is not in use.
///
/// Random -- from the hasher keys the standard library draws from the
/// system's randomness, the time and a count -- so two windows opened on one
/// file do not give out the same id: "the largest in the file plus one"
/// would, and a merge by id would then take two different records for one.
/// A clash with an id in the file is asked about (`taken`); a clash with an
/// id another window has given out and not yet saved has the odds of two
/// random 52-bit numbers agreeing.
///
/// `taken` is a finite set -- the ids of the records there are -- so the
/// count past the last draw, after sixty-four clashes, ends within one more
/// than its size.
pub fn fresh_id(taken: impl Fn(u64) -> bool) -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    static COUNT: AtomicU64 = AtomicU64::new(0);
    let keys = RandomState::new();
    let mut last = 1;
    for _ in 0..64 {
        let mut hasher = keys.build_hasher();
        hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
        hasher.write_u128(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        hasher.write_u32(std::process::id());
        last = hasher.finish() & MAX_ID;
        if last != 0 && !taken(last) {
            return last;
        }
    }
    // Sixty-four clashes is a `taken` that answers yes to nearly everything:
    // count up from the last draw to the first id it does not, which a
    // finite set of records cannot keep from being found.
    (last.max(1)..=MAX_ID)
        .chain(1..last.max(1))
        .find(|id| !taken(*id))
        .unwrap_or(MAX_ID)
}

/// What this window changed since it last read or wrote the file, put into
/// what the file holds now.
///
/// `base` is the records as this window last read or wrote them, `mine` as
/// it has them now, `theirs` as the file has them now. The answer is
/// `theirs` with this window's changes made to it:
///
/// - a record **changed or added here** (not as in `base`) is taken from
///   `mine` -- over another window's change to the same record, and over its
///   deletion of it, which is the later save winning for that record alone;
/// - a record **deleted here** (in `base`, not in `mine`) is taken out;
/// - every other record is as `theirs` has it -- what other windows changed,
///   added and deleted stands.
///
/// **Order** counts as a change too. If this window moved records about
/// (`mine` puts the records it shares with `base` in another order), `mine`'s
/// order is taken, and records only `theirs` has follow at the end;
/// otherwise `theirs`' order is kept, and each record added here goes after
/// the one before it in `mine`.
#[must_use]
pub fn merge<R: Record>(base: &[R], mine: &[R], theirs: &[R]) -> Vec<R> {
    merge_by(base, mine, theirs, R::id)
}

/// [`merge`] for records known by a key of their own rather than by an id
/// [`fresh_id`] gave out: `key` says which record is which -- a book's file,
/// a budget's category -- and two records with one key are one record.
///
/// Where every window can make the same record (one window opens a book
/// another opened too, both budget one category), the key is what makes them
/// one rather than two.
#[must_use]
pub fn merge_by<R, K, F>(base: &[R], mine: &[R], theirs: &[R], key: F) -> Vec<R>
where
    R: Clone + PartialEq,
    K: Eq + std::hash::Hash,
    F: Fn(&R) -> K,
{
    let by_key =
        |list: &[R]| -> HashMap<K, R> { list.iter().map(|r| (key(r), r.clone())).collect() };
    let base_by = by_key(base);
    let mine_by = by_key(mine);
    let theirs_by = by_key(theirs);
    // Changed or added here: not as the base had it.
    let changed_here = |k: &K| mine_by.get(k).is_some_and(|m| base_by.get(k) != Some(m));
    let deleted_here = |k: &K| base_by.contains_key(k) && !mine_by.contains_key(k);

    let shared = |list: &[R]| -> Vec<K> {
        list.iter()
            .map(&key)
            .filter(|k| base_by.contains_key(k) && mine_by.contains_key(k))
            .collect()
    };
    let reordered = shared(base) != shared(mine);

    let mut result: Vec<R> = Vec::with_capacity(theirs.len().max(mine.len()));
    if reordered {
        for record in mine {
            let k = key(record);
            if changed_here(&k) {
                result.push(record.clone());
            } else if let Some(theirs) = theirs_by.get(&k) {
                result.push(theirs.clone());
            }
            // Unchanged here and gone from the file: another window deleted
            // it, and that stands.
        }
        for record in theirs {
            let k = key(record);
            if !mine_by.contains_key(&k) && !deleted_here(&k) {
                result.push(record.clone());
            }
        }
        return result;
    }

    for record in theirs {
        let k = key(record);
        if deleted_here(&k) {
            continue;
        }
        match mine_by.get(&k) {
            Some(mine) if changed_here(&k) => result.push(mine.clone()),
            _ => result.push(record.clone()),
        }
    }
    // Added or changed here and not in the file: each after the record
    // before it in `mine`, or first when nothing before it is in the result.
    let mut placed: HashSet<K> = result.iter().map(&key).collect();
    for (at, record) in mine.iter().enumerate() {
        let k = key(record);
        if placed.contains(&k) || !changed_here(&k) {
            continue;
        }
        let before = mine
            .get(..at)
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|prior| {
                let prior = key(prior);
                result.iter().position(|r| key(r) == prior)
            });
        let mut index = before.map_or(0, |i| i.saturating_add(1));
        // Past what other windows added at the same place since: they saved
        // first, so what this window adds there comes after theirs.
        while result.get(index).is_some_and(|r| {
            let k = key(r);
            !base_by.contains_key(&k) && !mine_by.contains_key(&k)
        }) {
            index = index.saturating_add(1);
        }
        result.insert(index.min(result.len()), record.clone());
        placed.insert(k);
    }
    result
}

/// What a file was when a window last read or wrote it: enough to tell
/// whether something else has written it since, without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// Its length.
    len: u64,
    /// When it was last written, where the system says.
    modified: Option<SystemTime>,
    /// Which file it is: a save that renames a new file into place makes a
    /// new one, so two saves in one clock tick of one length still differ.
    /// Nought where the system has no inode numbers.
    inode: u64,
}

impl Stamp {
    /// The stamp of the file at `path` now; `None` when there is none.
    ///
    /// # Errors
    ///
    /// What reading its metadata meets, but its not being there.
    pub fn of(path: &Path) -> io::Result<Option<Self>> {
        match fs::metadata(path) {
            Ok(meta) => Ok(Some(Self {
                len: meta.len(),
                modified: meta.modified().ok(),
                inode: inode(&meta),
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(unix)]
fn inode(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    meta.ino()
}

#[cfg(not(unix))]
fn inode(_meta: &fs::Metadata) -> u64 {
    0
}

/// What an event in the data file's folder means for the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not about the file.
    Nothing,
    /// The file is whole and new: a save renamed into place, or gone.
    Now,
    /// The file is being written in place: tell the window once it is quiet.
    WhenQuiet,
    /// The folder's watch has ended -- the folder went: nothing more will be
    /// heard.
    Ended,
}

/// What an inotify event with `mask` about `name` means for a window
/// keeping the file called `file` in the folder watched. Pure, so it is
/// tested on every host.
#[must_use]
pub fn verdict(mask: u32, name: &[u8], file: &[u8]) -> Verdict {
    verdict_for(mask, name, |n| n == file)
}

/// [`verdict`] for a window keeping every file in the folder that `ours`
/// says is one of its own -- a library of one file a record, as the
/// flashcards' decks are.
#[must_use]
pub fn verdict_for(mask: u32, name: &[u8], ours: impl Fn(&[u8]) -> bool) -> Verdict {
    use libcall::inotify::{
        IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_IGNORED, IN_MODIFY, IN_MOVE_SELF, IN_MOVED_FROM,
        IN_MOVED_TO, IN_Q_OVERFLOW,
    };
    if mask & IN_Q_OVERFLOW != 0 {
        // Events were lost, ours perhaps among them.
        return Verdict::Now;
    }
    if mask & (IN_IGNORED | IN_DELETE_SELF | IN_MOVE_SELF) != 0 && name.is_empty() {
        return Verdict::Ended;
    }
    if !ours(name) {
        return Verdict::Nothing;
    }
    if mask & (IN_MOVED_TO | IN_MOVED_FROM | IN_DELETE) != 0 {
        Verdict::Now
    } else if mask & (IN_MODIFY | IN_CREATE) != 0 {
        Verdict::WhenQuiet
    } else {
        Verdict::Nothing
    }
}

/// Watching a data file for writes: a thread waiting on inotify for the
/// file's folder, calling `wake` when the file may have changed. Stopped when
/// dropped.
///
/// A window's own save wakes it too; it tells its own write from another's
/// by the file's [`Stamp`], which it noted when it wrote.
pub struct Watch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    /// Watch the file at `path`. `None` where it cannot be watched -- a
    /// system without inotify, as the development host is, or a path with no
    /// folder or name: the window then hears of other windows' saves only
    /// when it next reads the file, before its own save.
    pub fn start(path: &Path, wake: impl Fn() + Send + 'static) -> Option<Self> {
        let folder = path.parent()?;
        let file = path.file_name()?.as_encoded_bytes().to_vec();
        Self::start_folder(folder, move |name| name == file.as_slice(), wake)
    }

    /// Watch every file in `folder` that `ours` says is one of the window's
    /// -- a library of one file a record. `None` where it cannot be watched,
    /// as for [`start`](Self::start).
    ///
    /// The folder is made if it is not there yet. A program's folder is made
    /// by its first save, so on a first run it is not there when a window
    /// opens, and a watch cannot be put on a folder that is not there: two
    /// windows opened on a first run would never hear each other's saves.
    pub fn start_folder(
        folder: &Path,
        ours: impl Fn(&[u8]) -> bool + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Self> {
        let inotify = libcall::inotify::Inotify::new().ok()?;
        fs::create_dir_all(folder).ok()?;
        let c_folder = c_path(folder)?;
        let mask = libcall::inotify::IN_MOVED_TO
            | libcall::inotify::IN_MOVED_FROM
            | libcall::inotify::IN_DELETE
            | libcall::inotify::IN_MODIFY
            | libcall::inotify::IN_CREATE
            | libcall::inotify::IN_ONLYDIR;
        inotify.watch(&c_folder, mask).ok()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name(String::from("recordfile-watch"))
            .spawn(move || watch_loop(&inotify, &ours, &stopping, &wake))
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A watch thread that panicked has nothing left to stop.
            let _finished = thread.join();
        }
    }
}

/// The watch's thread: wait, read, decide, wake -- until asked to stop or
/// the watch ends.
fn watch_loop(
    inotify: &libcall::inotify::Inotify,
    ours: &impl Fn(&[u8]) -> bool,
    stop: &AtomicBool,
    wake: &(impl Fn() + Send),
) {
    let mut buf = vec![
        0_u8;
        libcall::inotify::HEADER
            .saturating_add(256)
            .saturating_mul(16)
    ];
    let mut quiet_until: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let wait = quiet_until.map_or(POLL, |due| due.saturating_duration_since(now).min(POLL));
        let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        let Ok(ready) = inotify.wait(ms) else {
            // The watch can no longer be waited on; the window still reads
            // the file before every save.
            return;
        };
        let mut tell = false;
        if ready {
            let Ok(events) = inotify.read(&mut buf) else {
                return;
            };
            for event in events {
                let Ok(event) = event else { break };
                match verdict_for(event.mask, event.name, ours) {
                    Verdict::Now => tell = true,
                    Verdict::WhenQuiet => quiet_until = Instant::now().checked_add(QUIET),
                    Verdict::Ended => {
                        wake();
                        return;
                    }
                    Verdict::Nothing => {}
                }
            }
        }
        if quiet_until.is_some_and(|due| Instant::now() >= due) {
            quiet_until = None;
            tell = true;
        }
        if tell {
            wake();
        }
    }
}

#[cfg(unix)]
fn c_path(path: &Path) -> Option<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt as _;
    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

#[cfg(not(unix))]
fn c_path(_path: &Path) -> Option<std::ffi::CString> {
    None
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Note {
        id: u64,
        text: &'static str,
    }

    impl Record for Note {
        fn id(&self) -> u64 {
            self.id
        }
    }

    fn n(id: u64, text: &'static str) -> Note {
        Note { id, text }
    }

    fn texts(list: &[Note]) -> Vec<&'static str> {
        list.iter().map(|r| r.text).collect()
    }

    /// Records known by a key of their own -- here a book's file and how far
    /// through it the reader is -- merge as records by id do; and the same
    /// key added in both windows is one record, this window's, not two.
    #[test]
    fn records_with_keys_of_their_own_merge_by_their_keys() {
        let book = |path: &'static str, at: u32| (path, at);
        let key = |b: &(&'static str, u32)| b.0;
        let base = [book("/a.txt", 0)];
        let mine = [book("/a.txt", 10), book("/both.txt", 1)];
        let theirs = [book("/a.txt", 0), book("/b.txt", 3), book("/both.txt", 7)];
        assert_eq!(
            merge_by(&base, &mine, &theirs, key),
            [book("/a.txt", 10), book("/b.txt", 3), book("/both.txt", 1)]
        );
        // Taken out here, and left alone there: gone.
        assert_eq!(
            merge_by(&base, &[], &[book("/a.txt", 0), book("/b.txt", 3)], key),
            [book("/b.txt", 3)]
        );
    }

    /// **Two windows each add a note; both are kept.** Before, the window
    /// that saved second wrote its copy over the first's note.
    #[test]
    fn what_two_windows_added_is_all_kept() {
        let base = vec![n(1, "a")];
        let first = vec![n(1, "a"), n(2, "from the first")];
        let on_disk = merge(&base, &first, &base);
        let second = vec![n(1, "a"), n(3, "from the second")];
        let merged = merge(&base, &second, &on_disk);
        assert_eq!(texts(&merged), ["a", "from the first", "from the second"]);
    }

    /// **Each window's edit to a different record stands; to the same
    /// record, the later save wins -- for that record alone.**
    #[test]
    fn edits_to_different_records_both_stand_and_to_one_the_later_wins() {
        let base = vec![n(1, "a"), n(2, "b")];
        let theirs = vec![n(1, "a edited there"), n(2, "b")];
        let mine = vec![n(1, "a"), n(2, "b edited here")];
        assert_eq!(
            texts(&merge(&base, &mine, &theirs)),
            ["a edited there", "b edited here"]
        );

        let mine = vec![n(1, "a edited here"), n(2, "b")];
        assert_eq!(texts(&merge(&base, &mine, &theirs)), ["a edited here", "b"]);
    }

    /// **A deletion here takes the record out; one elsewhere stands** unless
    /// this window changed the record since, which is the later save winning.
    #[test]
    fn deletions_stand_but_not_over_a_later_edit() {
        let base = vec![n(1, "a"), n(2, "b"), n(3, "c")];
        let theirs = vec![n(1, "a"), n(3, "c")]; // b deleted there
        let mine = vec![n(1, "a"), n(2, "b")]; // c deleted here
        assert_eq!(texts(&merge(&base, &mine, &theirs)), ["a"]);

        let mine = vec![n(1, "a"), n(2, "b edited here"), n(3, "c")];
        assert_eq!(
            texts(&merge(&base, &mine, &theirs)),
            ["a", "b edited here", "c"],
            "an edit made here was lost to a deletion there"
        );
    }

    /// **An order changed here is kept; otherwise the file's is**, and a
    /// record added here goes after the one before it.
    #[test]
    fn order_is_a_change_too() {
        let base = vec![n(1, "a"), n(2, "b"), n(3, "c")];
        let theirs = vec![n(1, "a"), n(2, "b"), n(3, "c"), n(4, "d from there")];
        let mine = vec![n(3, "c"), n(1, "a"), n(2, "b")];
        assert_eq!(
            texts(&merge(&base, &mine, &theirs)),
            ["c", "a", "b", "d from there"]
        );

        let theirs = vec![n(2, "b"), n(1, "a"), n(3, "c")]; // moved there
        let mine = vec![n(1, "a"), n(5, "e here"), n(2, "b"), n(3, "c")];
        assert_eq!(
            texts(&merge(&base, &mine, &theirs)),
            ["b", "a", "e here", "c"]
        );
        let mine = vec![n(5, "first here"), n(1, "a"), n(2, "b"), n(3, "c")];
        assert_eq!(texts(&merge(&base, &mine, &theirs))[0], "first here");
    }

    /// **A window that moved records about keeps every other rule too:** its
    /// edit stands and its deletion stays; another window's edit, deletion
    /// and addition stand, the addition at the end.
    #[test]
    fn a_reordered_merge_keeps_both_windows_changes() {
        let base = vec![n(1, "a"), n(2, "b"), n(3, "c"), n(4, "d")];
        // Here: c moved to the front and edited, d deleted.
        let mine = vec![n(3, "c edited here"), n(1, "a"), n(2, "b")];
        // There: a deleted, b edited, e added.
        let theirs = vec![
            n(2, "b edited there"),
            n(3, "c"),
            n(4, "d"),
            n(5, "e from there"),
        ];
        assert_eq!(
            texts(&merge(&base, &mine, &theirs)),
            ["c edited here", "b edited there", "e from there"]
        );
    }

    /// Nothing changed here: the file is taken as it is.
    #[test]
    fn a_window_that_changed_nothing_takes_the_file_as_it_is() {
        let base = vec![n(1, "a"), n(2, "b")];
        let theirs = vec![n(2, "b edited"), n(9, "new")];
        assert_eq!(merge(&base, &base, &theirs), theirs);
        assert!(merge::<Note>(&[], &[], &[]).is_empty());
    }

    #[test]
    fn a_fresh_id_is_in_range_and_not_taken() {
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let id = fresh_id(|id| seen.contains(&id));
            assert!((1..=MAX_ID).contains(&id));
            assert!(seen.insert(id), "an id given twice");
        }
        // A clash is drawn again, as often as it takes -- and past sixty-four
        // draws, the next id up that is free is given.
        let asked = std::cell::Cell::new(0_u32);
        let id = fresh_id(|_| {
            asked.set(asked.get() + 1);
            asked.get() <= 3
        });
        assert_eq!(asked.get(), 4, "a clash was given out");
        assert!((1..=MAX_ID).contains(&id));
        asked.set(0);
        let id = fresh_id(|_| {
            asked.set(asked.get() + 1);
            asked.get() <= 100
        });
        assert_eq!(asked.get(), 101);
        assert!((1..=MAX_ID).contains(&id));
    }

    #[test]
    fn a_stamp_changes_when_the_file_is_written() {
        let scratch = scratchdir::ScratchDir::new("recordfile_stamp");
        let path = scratch.dir().join("data.txt");
        assert_eq!(Stamp::of(&path).unwrap(), None);
        fs::write(&path, "one").unwrap();
        let first = Stamp::of(&path).unwrap().unwrap();
        assert_eq!(Stamp::of(&path).unwrap(), Some(first));
        fs::write(&path, "three").unwrap();
        assert_ne!(Stamp::of(&path).unwrap(), Some(first));
    }

    /// The length and the time each tell a write apart on their own: a
    /// write of the same length is told by its time, and one inside the
    /// same clock tick by its length. Written in place, as by a program that
    /// does not rename, so the file stays the same file.
    #[test]
    fn a_stamp_tells_a_write_by_its_length_or_its_time() {
        use std::time::Duration;
        let scratch = scratchdir::ScratchDir::new("recordfile_stamp_parts");
        let path = scratch.dir().join("data.txt");
        let at = |when: SystemTime| {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        };
        let then = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::write(&path, "one").unwrap();
        at(then);
        let first = Stamp::of(&path).unwrap().unwrap();

        fs::write(&path, "two").unwrap();
        at(then + Duration::from_mins(1));
        let same_length = Stamp::of(&path).unwrap().unwrap();
        assert_ne!(
            same_length, first,
            "a write of the same length was not told"
        );

        fs::write(&path, "three").unwrap();
        at(then + Duration::from_mins(1));
        let same_time = Stamp::of(&path).unwrap().unwrap();
        assert_ne!(
            same_time, same_length,
            "a write in the same tick was not told"
        );
    }

    #[test]
    fn the_watch_tells_only_of_the_file_and_waits_for_an_in_place_write() {
        use libcall::inotify::{
            IN_CREATE, IN_DELETE, IN_IGNORED, IN_MODIFY, IN_MOVED_TO, IN_Q_OVERFLOW,
        };
        assert_eq!(
            verdict(IN_MOVED_TO, b"notes.txt", b"notes.txt"),
            Verdict::Now
        );
        assert_eq!(verdict(IN_DELETE, b"notes.txt", b"notes.txt"), Verdict::Now);
        assert_eq!(
            verdict(IN_MODIFY, b"notes.txt", b"notes.txt"),
            Verdict::WhenQuiet
        );
        assert_eq!(
            verdict(IN_CREATE, b"notes.txt", b"notes.txt"),
            Verdict::WhenQuiet
        );
        assert_eq!(
            verdict(IN_MOVED_TO, b"notes.txt.new", b"notes.txt"),
            Verdict::Nothing
        );
        assert_eq!(
            verdict(IN_MOVED_TO, b"other.txt", b"notes.txt"),
            Verdict::Nothing
        );
        assert_eq!(verdict(IN_Q_OVERFLOW, b"", b"notes.txt"), Verdict::Now);
        assert_eq!(verdict(IN_IGNORED, b"", b"notes.txt"), Verdict::Ended);
    }

    /// A folder of one file a record hears every file of its kind, and not
    /// the temporaries a save writes beside them.
    #[test]
    fn a_folder_watch_tells_of_every_file_of_its_kind() {
        use libcall::inotify::{IN_MODIFY, IN_MOVED_TO};
        let deck = |name: &[u8]| name.ends_with(b".deck");
        assert_eq!(verdict_for(IN_MOVED_TO, b"7.deck", deck), Verdict::Now);
        assert_eq!(verdict_for(IN_MODIFY, b"12.deck", deck), Verdict::WhenQuiet);
        assert_eq!(
            verdict_for(IN_MOVED_TO, b".7.deck.slate-save-1-0", deck),
            Verdict::Nothing
        );
        assert_eq!(
            verdict_for(IN_MOVED_TO, b"notes.txt", deck),
            Verdict::Nothing
        );
    }

    /// On a system without inotify -- the development host -- nothing is
    /// watched, and nothing fails.
    #[cfg(not(unix))]
    #[test]
    fn without_inotify_there_is_no_watch() {
        let scratch = scratchdir::ScratchDir::new("recordfile_watch");
        assert!(Watch::start(&scratch.dir().join("data.txt"), || {}).is_none());
    }
}
