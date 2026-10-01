//! The protocol's promises: what is appended is kept, a rewrite replaces
//! whole, and -- where `flock` exists -- a writer that arrives during a
//! rewrite or a rotation lands in the file the path names afterwards.

use super::*;
use scratchdir::ScratchDir;

#[test]
fn records_are_appended_and_the_file_made() {
    let dir = ScratchDir::new("journalio-append");
    let log = dir.path("log");
    append(&log, b"a\n").unwrap();
    append(&log, b"b\n").unwrap();
    assert_eq!(fs::read(&log).unwrap(), b"a\nb\n");
}

#[test]
fn a_missing_log_has_nothing_to_lock() {
    let dir = ScratchDir::new("journalio-missing");
    assert!(Locked::open(&dir.path("nothing")).unwrap().is_none());
}

#[test]
fn a_replacement_is_whole_and_leaves_no_temporary() {
    let dir = ScratchDir::new("journalio-replace");
    let log = dir.path("log");
    fs::write(&log, b"old\n").unwrap();
    let mut held = Locked::open(&log).unwrap().unwrap();
    assert_eq!(held.read_all().unwrap(), b"old\n");
    assert_eq!(held.path(), log.as_path());
    held.replace(b"new\n").unwrap();
    assert_eq!(fs::read(&log).unwrap(), b"new\n");
    let names: Vec<_> = fs::read_dir(dir.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("log")]);
}

#[test]
fn a_rotation_moves_the_file_and_a_removal_removes_it() {
    let dir = ScratchDir::new("journalio-rotate");
    let log = dir.path("log");
    let rotated = dir.path("log.1");
    fs::write(&log, b"a\n").unwrap();
    Locked::open(&log)
        .unwrap()
        .unwrap()
        .rename_to(&rotated)
        .unwrap();
    assert!(!log.exists());
    assert_eq!(fs::read(&rotated).unwrap(), b"a\n");
    Locked::open(&rotated).unwrap().unwrap().remove().unwrap();
    assert!(!rotated.exists());
}

#[test]
fn the_temporary_is_hidden_beside_the_log() {
    let t = temp_path(Path::new("/var/log/syslog.jsonl"));
    assert_eq!(t.parent(), Some(Path::new("/var/log")));
    let name = t.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(".syslog.jsonl."), "{name}");
    assert!(name.ends_with(".tmp"), "{name}");
}

/// Long enough for a writer thread to have opened the file and reached
/// its `flock`. Should it not have, it opens the path afterwards and the
/// assertions still hold; only a writer that wrote WITHOUT waiting -- no
/// lock at all -- fails them.
#[cfg(unix)]
const SETTLE: std::time::Duration = std::time::Duration::from_millis(300);

#[cfg(unix)]
#[test]
fn a_writer_arriving_during_a_rewrite_lands_in_the_new_file() {
    let dir = ScratchDir::new("journalio-race-rewrite");
    let log = dir.path("log");
    fs::write(&log, b"a\n").unwrap();
    let mut held = Locked::open(&log).unwrap().unwrap();
    let path = log.clone();
    let writer = std::thread::spawn(move || append(&path, b"b\n"));
    std::thread::sleep(SETTLE);
    // The writer waits for the lock: nothing of its is in the old file.
    assert_eq!(held.read_all().unwrap(), b"a\n");
    held.replace(b"A\n").unwrap();
    writer.join().unwrap().unwrap();
    assert_eq!(fs::read(&log).unwrap(), b"A\nb\n");
}

#[cfg(unix)]
#[test]
fn a_writer_arriving_during_a_rotation_starts_the_next_file() {
    let dir = ScratchDir::new("journalio-race-rotate");
    let log = dir.path("log");
    let rotated = dir.path("log.1");
    fs::write(&log, b"a\n").unwrap();
    let held = Locked::open(&log).unwrap().unwrap();
    let path = log.clone();
    let writer = std::thread::spawn(move || append(&path, b"b\n"));
    std::thread::sleep(SETTLE);
    held.rename_to(&rotated).unwrap();
    writer.join().unwrap().unwrap();
    assert_eq!(fs::read(&rotated).unwrap(), b"a\n");
    assert_eq!(fs::read(&log).unwrap(), b"b\n");
}

#[cfg(unix)]
#[test]
fn a_replacement_keeps_the_logs_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = ScratchDir::new("journalio-mode");
    let log = dir.path("log");
    fs::write(&log, b"a\n").unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o640)).unwrap();
    Locked::open(&log)
        .unwrap()
        .unwrap()
        .replace(b"b\n")
        .unwrap();
    let mode = fs::metadata(&log).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}
