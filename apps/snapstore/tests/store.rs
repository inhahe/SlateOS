//! The store, driven as the backup tool and System Restore drive it.
//!
//! Each test is one of the promises in the crate's docs, or one of the faults
//! the move out of `apps/backup` found: an id reused within a second, a file
//! that could not be read silently left out, an incremental that brought back
//! what had been deleted, a parent taken from another folder.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use scratchdir::ScratchDir;
use snapstore::{BackupType, CaptureOptions, Progress, RestoreOptions, Store};

fn opts(kind: BackupType) -> CaptureOptions {
    CaptureOptions {
        kind,
        excludes: Vec::new(),
        follow_symlinks: false,
    }
}

fn quiet(_: &Progress) {}

fn put(dir: &Path, rel: &str, body: &[u8]) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn read(dir: &Path, rel: &str) -> Option<Vec<u8>> {
    fs::read(dir.join(rel)).ok()
}

/// A scratch directory with a store in `store/` and a folder in `src/`.
fn setup(tag: &str) -> (ScratchDir, Store, PathBuf) {
    let scratch = ScratchDir::new(&format!("snapstore_{tag}"));
    let store = Store::open(&scratch.dir().join("store")).unwrap();
    let src = scratch.dir().join("src");
    fs::create_dir_all(&src).unwrap();
    (scratch, store, src)
}

fn paths(store: &Store, id: &str) -> Vec<String> {
    let mut names: Vec<String> = store
        .files(id)
        .unwrap()
        .into_iter()
        .map(|f| f.path.to_str().expect("test names are text").to_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_capture_restores_as_it_was() {
    let (scratch, store, src) = setup("roundtrip");
    put(&src, "a.txt", b"alpha");
    put(&src, "deep/er/b.txt", b"beta");
    fs::create_dir_all(src.join("empty")).unwrap();
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    assert!(cap.unread.is_empty());
    assert_eq!(cap.meta.file_count, 2);

    let out = scratch.dir().join("out");
    let report = store
        .restore(&cap.meta.id, &out, &RestoreOptions::default())
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.written, 2);
    assert_eq!(read(&out, "a.txt").as_deref(), Some(&b"alpha"[..]));
    assert_eq!(read(&out, "deep/er/b.txt").as_deref(), Some(&b"beta"[..]));
}

/// Ids were `<seconds>-<kind>` with nothing checking that one was free: two
/// snapshots in one second shared a directory and the second's manifest
/// replaced the first's. Every name the capture could start at is taken
/// here, so it must pick another -- and leave the taken ones alone.
#[test]
fn a_capture_never_reuses_a_taken_id() {
    let (_scratch, store, src) = setup("ids");
    put(&src, "a.txt", b"alpha");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let taken: Vec<PathBuf> = (now..now + 4)
        .map(|t| store.root().join("backups").join(format!("{t}-full")))
        .collect();
    for dir in &taken {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("marker"), b"someone else's").unwrap();
    }
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    assert!(cap.meta.id.ends_with("-full-2"), "{}", cap.meta.id);
    for dir in &taken {
        assert!(!dir.join("manifest.json").exists(), "wrote into {dir:?}");
        assert!(dir.join("marker").exists());
    }
    let listed = store.list().unwrap().snapshots;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, cap.meta.id);
}

/// Every manifest lists every file, so a file deleted before an
/// incremental snapshot is not in it -- and restoring it does not bring the
/// file back, which rebuilding from a chain of differences did. A file
/// unchanged since before the parent is in it too, though it was not read
/// again: its reading is the parent's.
#[test]
fn a_file_deleted_before_an_incremental_stays_deleted() {
    let (scratch, store, src) = setup("deleted");
    put(&src, "keep.txt", b"keep");
    put(&src, "gone.txt", b"gone");
    // An hour old, so the incremental may take its reading from the parent:
    // a file changed in the second the parent began is read again.
    fs::File::options()
        .write(true)
        .open(src.join("keep.txt"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_hours(1))
        .unwrap();
    let full = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    fs::remove_file(src.join("gone.txt")).unwrap();
    put(&src, "new.txt", b"new");
    let inc = store
        .capture(&src, &opts(BackupType::Incremental), &mut quiet)
        .unwrap();
    assert_eq!(inc.meta.backup_type, BackupType::Incremental);
    assert_eq!(inc.meta.parent_id.as_deref(), Some(full.meta.id.as_str()));
    assert_eq!(paths(&store, &inc.meta.id), ["keep.txt", "new.txt"]);
    assert_eq!(
        (inc.meta.new_blobs, inc.meta.dedup_blobs),
        (1, 1),
        "new.txt stored; keep.txt's reading taken from the parent"
    );

    let out = scratch.dir().join("out");
    store
        .restore(&inc.meta.id, &out, &RestoreOptions::default())
        .unwrap();
    assert!(read(&out, "gone.txt").is_none(), "a deleted file came back");
    assert!(read(&out, "keep.txt").is_some() && read(&out, "new.txt").is_some());
}

/// An incremental's parent is the newest snapshot *of the same folder*. It
/// was the newest of any, so a store holding two folders compared one with
/// the other.
#[test]
fn an_incremental_takes_its_parent_from_the_same_folder() {
    let (scratch, store, a) = setup("parent");
    let b = scratch.dir().join("other");
    put(&a, "a.txt", b"a");
    put(&b, "b.txt", b"b");
    let a_full = store
        .capture(&a, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let b_full = store
        .capture(&b, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let a_inc = store
        .capture(&a, &opts(BackupType::Incremental), &mut quiet)
        .unwrap();
    assert_eq!(
        a_inc.meta.parent_id.as_deref(),
        Some(a_full.meta.id.as_str())
    );
    assert_ne!(
        a_inc.meta.parent_id.as_deref(),
        Some(b_full.meta.id.as_str())
    );
}

/// A blob's name is the hash of the bytes stored. A caller whose hash is
/// stale -- the file changed after it was read -- gets the content filed
/// under its real hash, and nothing is filed under the stale one.
#[test]
fn a_blob_is_named_by_what_was_stored() {
    let (_scratch, store, src) = setup("ingest");
    put(&src, "f", b"what the file holds now");
    let stale = "0".repeat(64);
    let stored = store.blobs().ingest(&src.join("f"), &stale).unwrap();
    assert_eq!(
        stored.hash,
        snapstore::sha256_hex(b"what the file holds now")
    );
    assert!(stored.new);
    assert!(store.blobs().has_blob(&stored.hash));
    assert!(!store.blobs().has_blob(&stale));
    // Ingesting it again stores nothing new.
    let again = store.blobs().ingest(&src.join("f"), &stored.hash).unwrap();
    assert!(!again.new);
}

/// A mirror restore makes the folder what the snapshot says: a changed file
/// put back, a deleted one brought back, an added one removed, an empty
/// folder the snapshot had recreated, and one it did not have removed.
#[test]
fn a_mirror_restore_makes_the_folder_match() {
    let (_scratch, store, src) = setup("mirror");
    put(&src, "same.txt", b"same");
    put(&src, "changed.txt", b"before");
    put(&src, "deleted.txt", b"deleted");
    fs::create_dir_all(src.join("was_empty")).unwrap();
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();

    put(&src, "changed.txt", b"after, and longer");
    fs::remove_file(src.join("deleted.txt")).unwrap();
    put(&src, "added.txt", b"added");
    put(&src, "added_dir/inner.txt", b"inner");
    fs::remove_dir(src.join("was_empty")).unwrap();
    fs::create_dir_all(src.join("new_empty")).unwrap();

    let report = store
        .restore(
            &cap.meta.id,
            &src,
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(read(&src, "same.txt").as_deref(), Some(&b"same"[..]));
    assert_eq!(read(&src, "changed.txt").as_deref(), Some(&b"before"[..]));
    assert_eq!(read(&src, "deleted.txt").as_deref(), Some(&b"deleted"[..]));
    assert!(read(&src, "added.txt").is_none());
    assert!(
        !src.join("added_dir").exists(),
        "a folder it did not have stays"
    );
    assert!(
        src.join("was_empty").is_dir(),
        "an empty folder it had is gone"
    );
    assert!(!src.join("new_empty").exists());
    assert_eq!(report.unchanged, 1, "same.txt is left alone");
    assert_eq!(report.written, 2, "changed.txt and deleted.txt");
    assert_eq!(
        report.removed, 4,
        "added.txt, inner.txt, added_dir, new_empty"
    );
}

/// What the capture excluded is not in the snapshot on purpose, and a
/// mirror restore leaves it where it is.
#[test]
fn a_mirror_restore_leaves_what_was_excluded() {
    let (_scratch, store, src) = setup("excluded");
    put(&src, "kept.txt", b"kept");
    let cap = store
        .capture(
            &src,
            &CaptureOptions {
                // A folder is excluded by its name: this dialect has no
                // trailing-slash form.
                excludes: vec!["*.tmp".to_string(), "cache".to_string()],
                ..opts(BackupType::Full)
            },
            &mut quiet,
        )
        .unwrap();
    put(&src, "scratch.tmp", b"scratch");
    put(&src, "cache/big", b"cache");
    store
        .restore(
            &cap.meta.id,
            &src,
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap();
    assert!(read(&src, "scratch.tmp").is_some());
    assert!(read(&src, "cache/big").is_some());
}

#[test]
fn a_mirror_restore_takes_no_filter() {
    let (_scratch, store, src) = setup("mirrorfilter");
    put(&src, "a.txt", b"a");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let err = store
        .restore(
            &cap.meta.id,
            &src,
            &RestoreOptions {
                mirror: true,
                filter: Some("*.txt".to_string()),
            },
        )
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

/// A snapshot that could not read something says so, and a mirror restore
/// never removes it: nobody knows what it held. A link whose target is gone
/// is what cannot be read when links are followed.
#[cfg(unix)]
#[test]
fn a_mirror_restore_leaves_what_could_not_be_read() {
    let (_scratch, store, src) = setup("unread");
    put(&src, "a.txt", b"a");
    std::os::unix::fs::symlink("/nonexistent/snapstore-test", src.join("dangling")).unwrap();
    let cap = store
        .capture(
            &src,
            &CaptureOptions {
                follow_symlinks: true,
                ..opts(BackupType::Full)
            },
            &mut quiet,
        )
        .unwrap();
    assert_eq!(cap.unread.len(), 1, "{:?}", cap.unread);
    assert_eq!(cap.unread[0].0, PathBuf::from("dangling"));
    assert_eq!(cap.meta.unread_count, 1);
    store
        .restore(
            &cap.meta.id,
            &src,
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap();
    assert!(
        fs::symlink_metadata(src.join("dangling")).is_ok(),
        "the restore removed something it never read"
    );
}

/// A folder replaced by a link does not carry a restore's writes out of the
/// folder being restored. An ordinary restore refuses the file; a mirror
/// restore removes the link, which the snapshot does not have, and puts the
/// real folder back.
#[cfg(unix)]
#[test]
fn a_restore_does_not_write_through_a_link() {
    let (scratch, store, src) = setup("throughlink");
    put(&src, "sub/a.txt", b"inside");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let outside = scratch.dir().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::remove_dir_all(src.join("sub")).unwrap();
    std::os::unix::fs::symlink(&outside, src.join("sub")).unwrap();

    let report = store
        .restore(&cap.meta.id, &src, &RestoreOptions::default())
        .unwrap();
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(!outside.join("a.txt").exists(), "wrote through the link");

    let report = store
        .restore(
            &cap.meta.id,
            &src,
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!outside.join("a.txt").exists());
    assert!(fs::symlink_metadata(src.join("sub")).unwrap().is_dir());
    assert_eq!(read(&src, "sub/a.txt").as_deref(), Some(&b"inside"[..]));
}

/// A file somebody deleted comes back with the permissions it had. It came
/// back with the default mode, so a private file came back readable by all.
#[cfg(unix)]
#[test]
fn a_private_file_comes_back_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_scratch, store, src) = setup("mode");
    put(&src, "vault", b"secret");
    fs::set_permissions(src.join("vault"), fs::Permissions::from_mode(0o600)).unwrap();
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    fs::remove_file(src.join("vault")).unwrap();
    store
        .restore(&cap.meta.id, &src, &RestoreOptions::default())
        .unwrap();
    let mode = fs::metadata(src.join("vault"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

/// A restore over a file that already holds the snapshot's content leaves it
/// alone rather than rewriting it.
#[test]
fn an_unchanged_file_is_not_rewritten() {
    let (_scratch, store, src) = setup("unchanged");
    put(&src, "a.txt", b"a");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let report = store
        .restore(&cap.meta.id, &src, &RestoreOptions::default())
        .unwrap();
    assert_eq!((report.written, report.unchanged), (0, 1));
}

/// A record that will not read is not a snapshot anyone can restore -- but
/// the blobs only it names are not orphans, so collection refuses.
#[test]
fn collection_refuses_while_a_record_is_unreadable() {
    let (_scratch, store, src) = setup("gcrefuse");
    put(&src, "a.txt", b"a");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let broken = store.root().join("backups").join("broken");
    fs::create_dir_all(&broken).unwrap();
    fs::write(broken.join("meta.json"), b"{ not json").unwrap();
    let listing = store.list().unwrap();
    assert_eq!(listing.snapshots.len(), 1);
    assert_eq!(listing.unreadable.len(), 1);
    store.remove(&cap.meta.id).unwrap();
    assert!(store.collect_garbage(Duration::ZERO).is_err());
    assert!(
        store.blobs().all_blobs().unwrap().len() == 1,
        "a refused collection removed a blob"
    );
}

/// Collection removes what no snapshot names -- but not what was written in
/// its grace period, which a capture running now may be about to name.
#[test]
fn collection_leaves_young_orphans_and_removes_old_ones() {
    let (_scratch, store, src) = setup("gcgrace");
    put(&src, "a.txt", b"a");
    put(&src, "b.txt", b"b");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    store.remove(&cap.meta.id).unwrap();
    assert_eq!(store.collect_garbage(Duration::from_hours(1)).unwrap(), 0);
    assert_eq!(store.blobs().all_blobs().unwrap().len(), 2);
    assert_eq!(store.collect_garbage(Duration::ZERO).unwrap(), 2);
    assert!(store.blobs().all_blobs().unwrap().is_empty());
}

/// Collection keeps what a snapshot names.
#[test]
fn collection_keeps_what_a_snapshot_names() {
    let (_scratch, store, src) = setup("gckeep");
    put(&src, "a.txt", b"a");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    assert_eq!(store.collect_garbage(Duration::ZERO).unwrap(), 0);
    assert!(store.verify(&cap.meta.id).unwrap().missing.is_empty());
}

/// An id is one name in the store: `..` or a path is refused before it is
/// joined onto anything.
#[test]
fn an_id_that_leaves_the_store_is_refused() {
    let (_scratch, store, _src) = setup("ids_refused");
    for id in ["..", "../elsewhere", "a/b", "", "."] {
        let err = store.meta(id).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{id:?}");
        assert!(store.remove(id).is_err(), "{id:?}");
    }
}

/// Verify finds a damaged blob and a missing one.
#[test]
fn verify_finds_damage() {
    let (_scratch, store, src) = setup("verify");
    put(&src, "a.txt", b"alpha");
    put(&src, "b.txt", b"beta");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    let a = snapstore::sha256_hex(b"alpha");
    let b = snapstore::sha256_hex(b"beta");
    fs::write(store.blobs().blob_path(&a), b"tampered").unwrap();
    store.blobs().remove_blob(&b).unwrap();
    let report = store.verify(&cap.meta.id).unwrap();
    assert_eq!(report.corrupt.len(), 1);
    assert_eq!(report.missing, vec![PathBuf::from("b.txt")]);
    assert_eq!(report.ok, 0);
}

/// Stores written before manifest version 3 still read: an incremental that
/// held only its changes is rebuilt from its chain, and -- since it cannot
/// say what the folder held -- is refused a mirror restore.
#[test]
fn a_legacy_incremental_is_rebuilt_from_its_chain() {
    let (scratch, store, _src) = setup("legacy");
    let snap = |id: &str, kind: &str, parent: &str, files: &str| {
        let dir = store.root().join("backups").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            format!(r#"{{"version": 2, "files": [{files}]}}"#),
        )
        .unwrap();
        fs::write(
            dir.join("meta.json"),
            format!(
                r#"{{"version": 2, "id": "{id}", "backup_type": "{kind}", "timestamp": 100,
                    "source": "/src", "parent_id": {parent}}}"#
            ),
        )
        .unwrap();
    };
    let file = |path: &str, hash: &str| {
        format!(
            r#"{{"path": "{path}", "size": 1, "mtime": 1, "hash": "{hash}", "is_symlink": false}}"#
        )
    };
    snap(
        "old-full",
        "full",
        "null",
        &format!("{}, {}", file("a", "aa"), file("b", "bb")),
    );
    snap(
        "old-inc",
        "incremental",
        r#""old-full""#,
        &format!("{}, {}", file("b", "b2"), file("c", "cc")),
    );
    assert_eq!(paths(&store, "old-inc"), ["a", "b", "c"]);
    let b = store
        .files("old-inc")
        .unwrap()
        .into_iter()
        .find(|f| f.path == Path::new("b"))
        .unwrap();
    assert_eq!(b.hash, "b2", "the newer link of the chain wins");
    let err = store
        .restore(
            "old-inc",
            &scratch.dir().join("out"),
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    // And a legacy full snapshot is complete, so it may be mirrored.
    assert!(store.manifest("old-full").unwrap().complete);
}

/// Progress counts every file found, and ends at the total.
#[test]
fn progress_reaches_the_total() {
    let (_scratch, store, src) = setup("progress");
    put(&src, "a", b"12345");
    put(&src, "b", b"678");
    let mut last = Progress::default();
    store
        .capture(&src, &opts(BackupType::Full), &mut |p: &Progress| {
            last = p.clone();
        })
        .unwrap();
    assert_eq!((last.total_files, last.processed_files), (2, 2));
    assert_eq!((last.total_bytes, last.processed_bytes), (8, 8));
}

/// The same promise on Windows, where what cannot be read is a file another
/// program holds open without sharing it.
#[cfg(windows)]
#[test]
fn a_mirror_restore_leaves_a_file_that_was_locked() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_scratch, store, src) = setup("locked");
    put(&src, "a.txt", b"a");
    put(&src, "locked.db", b"in use");
    let cap = {
        // No sharing at all: every other open fails while this is held.
        let _held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(src.join("locked.db"))
            .unwrap();
        store
            .capture(&src, &opts(BackupType::Full), &mut quiet)
            .unwrap()
    };
    assert_eq!(cap.unread.len(), 1, "{:?}", cap.unread);
    assert_eq!(cap.unread[0].0, PathBuf::from("locked.db"));
    store
        .restore(
            &cap.meta.id,
            &src,
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        read(&src, "locked.db").as_deref(),
        Some(&b"in use"[..]),
        "the restore removed a file it never read"
    );
}

/// A snapshot from before folders were recorded cannot say which folders
/// the source had, so a mirror restore of it removes none -- where an empty
/// list read as "there were none" would have removed every empty one.
#[test]
fn a_legacy_snapshot_removes_no_folder() {
    let (_scratch, store, src) = setup("legacydirs");
    let dir = store.root().join("backups").join("old-full");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.json"), br#"{"version": 2, "files": []}"#).unwrap();
    fs::write(
        dir.join("meta.json"),
        br#"{"version": 2, "id": "old-full", "backup_type": "full", "timestamp": 100,
            "source": "/src", "parent_id": null}"#,
    )
    .unwrap();
    fs::create_dir_all(src.join("empty")).unwrap();
    store
        .restore(
            "old-full",
            &src,
            &RestoreOptions {
                mirror: true,
                ..RestoreOptions::default()
            },
        )
        .unwrap();
    assert!(src.join("empty").is_dir());
}

/// A file of the same size with other content is rewritten: the check that
/// leaves a file alone asks for its content, not only its size.
#[test]
fn a_same_size_change_is_restored() {
    let (_scratch, store, src) = setup("samesize");
    put(&src, "a.txt", b"12345");
    let cap = store
        .capture(&src, &opts(BackupType::Full), &mut quiet)
        .unwrap();
    put(&src, "a.txt", b"54321");
    let report = store
        .restore(&cap.meta.id, &src, &RestoreOptions::default())
        .unwrap();
    assert_eq!((report.written, report.unchanged), (1, 0));
    assert_eq!(read(&src, "a.txt").as_deref(), Some(&b"12345"[..]));
}
