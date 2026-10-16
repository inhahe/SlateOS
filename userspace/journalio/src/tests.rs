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

// --- Following ----------------------------------------------------------------------

/// What a follower's read of `path` gives, as text.
fn read_text(follow: &mut Follow, path: &Path, hold_back: bool) -> Option<String> {
    follow
        .read(path, hold_back)
        .unwrap()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

fn append_bytes(path: &Path, bytes: &[u8]) {
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .unwrap();
    f.write_all(bytes).unwrap();
}

#[test]
fn a_follower_reads_a_new_file_whole_then_only_what_is_appended() {
    let dir = ScratchDir::new("journalio-follow-append");
    let log = dir.path("log");
    fs::write(&log, b"a\nb\n").unwrap();
    let mut follow = Follow::new();
    assert_eq!(
        read_text(&mut follow, &log, true).as_deref(),
        Some("a\nb\n")
    );
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some(""));
    append_bytes(&log, b"c\n");
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some("c\n"));
}

#[test]
fn a_line_still_being_written_waits_for_its_newline() {
    let dir = ScratchDir::new("journalio-follow-partial");
    let log = dir.path("log");
    fs::write(&log, b"a\nhal").unwrap();
    let mut follow = Follow::new();
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some("a\n"));
    append_bytes(&log, b"f");
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some(""));
    append_bytes(&log, b"\nnext");
    assert_eq!(
        read_text(&mut follow, &log, true).as_deref(),
        Some("half\n")
    );
    append_bytes(&log, b"\n");
    assert_eq!(
        read_text(&mut follow, &log, true).as_deref(),
        Some("next\n")
    );
}

#[test]
fn a_listing_takes_the_unfinished_line_too() {
    let dir = ScratchDir::new("journalio-follow-listing");
    let log = dir.path("log");
    fs::write(&log, b"a\nunfinished").unwrap();
    let mut follow = Follow::new();
    assert_eq!(
        read_text(&mut follow, &log, false).as_deref(),
        Some("a\nunfinished")
    );
}

#[test]
fn a_file_truncated_in_place_is_read_again_from_its_start() {
    let dir = ScratchDir::new("journalio-follow-truncate");
    let log = dir.path("log");
    fs::write(&log, b"one\ntwo\n").unwrap();
    let mut follow = Follow::new();
    assert!(read_text(&mut follow, &log, true).is_some());
    // Emptied where it is -- the same file, not a replacement -- and written
    // again, shorter than before.
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&log)
        .unwrap();
    append_bytes(&log, b"new\n");
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some("new\n"));
}

#[test]
fn a_file_that_is_not_there_is_none() {
    let dir = ScratchDir::new("journalio-follow-missing");
    let nothing = dir.path("nothing");
    let mut follow = Follow::new();
    assert_eq!(read_text(&mut follow, &nothing, true), None);
    follow.skip_to_end(&nothing).unwrap();
    assert_eq!(read_text(&mut follow, &nothing, true), None);
}

#[test]
fn following_from_the_end_shows_only_what_comes_after() {
    let dir = ScratchDir::new("journalio-follow-skip");
    let log = dir.path("log");
    fs::write(&log, b"old\nbeing writ").unwrap();
    let mut follow = Follow::new();
    follow.skip_to_end(&log).unwrap();
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some(""));
    append_bytes(&log, b"ten\nnew\n");
    // The record that was half-written at the start is shown whole once
    // finished; the one before it is not shown at all.
    assert_eq!(
        read_text(&mut follow, &log, true).as_deref(),
        Some("being written\nnew\n")
    );
}

#[test]
fn following_from_the_end_of_one_unfinished_line_keeps_it() {
    let dir = ScratchDir::new("journalio-follow-skip-one");
    let log = dir.path("log");
    fs::write(&log, b"unfini").unwrap();
    let mut follow = Follow::new();
    follow.skip_to_end(&log).unwrap();
    append_bytes(&log, b"shed\n");
    assert_eq!(
        read_text(&mut follow, &log, true).as_deref(),
        Some("unfinished\n")
    );
}

#[test]
fn a_line_longer_than_the_window_is_not_shown_half() {
    let dir = ScratchDir::new("journalio-follow-skip-long");
    let log = dir.path("log");
    let window = usize::try_from(super::follow::LAST_LINE_WINDOW).unwrap();
    let mut long = vec![b'x'; window + 10];
    long.splice(0..0, b"old\n".iter().copied());
    fs::write(&log, &long).unwrap();
    let mut follow = Follow::new();
    follow.skip_to_end(&log).unwrap();
    append_bytes(&log, b"\nnext\n");
    // The rest of the long line is a line of its own -- the price of a
    // window -- and what follows is whole.
    assert_eq!(
        read_text(&mut follow, &log, true).as_deref(),
        Some("\nnext\n")
    );
}

#[test]
fn a_file_not_seen_in_a_round_is_forgotten() {
    let dir = ScratchDir::new("journalio-follow-forget");
    let log = dir.path("log");
    fs::write(&log, b"a\n").unwrap();
    let mut follow = Follow::new();
    assert!(read_text(&mut follow, &log, true).is_some());
    follow.end_round(); // seen in this round: kept
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some(""));
    follow.end_round(); // seen again: kept
    follow.end_round(); // not seen: forgotten
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some("a\n"));
}

/// What a follower has read of a file goes with the file when a rotation
/// renames it: the rotated file gives what was appended to it before the
/// rename and nothing more, and the new live log is read from its start.
/// Keyed by name, `.1`'s old offset was applied to the file that had just
/// become `.1`.
#[cfg(unix)]
#[test]
fn a_rotation_takes_what_was_read_with_the_file() {
    let dir = ScratchDir::new("journalio-follow-rotate");
    let log = dir.path("log");
    let first = dir.path("log.1");
    let second = dir.path("log.2");
    fs::write(&log, b"a\n").unwrap();
    fs::write(&first, b"older and longer\n").unwrap();
    let mut follow = Follow::new();
    for path in [&log, &first] {
        assert!(read_text(&mut follow, path, true).is_some());
    }
    follow.end_round();
    append_bytes(&log, b"b\n"); // before the rotation, not read yet
    fs::rename(&first, &second).unwrap();
    fs::rename(&log, &first).unwrap();
    append_bytes(&log, b"c\n"); // the new live log
    assert_eq!(read_text(&mut follow, &second, true).as_deref(), Some(""));
    assert_eq!(read_text(&mut follow, &first, true).as_deref(), Some("b\n"));
    assert_eq!(read_text(&mut follow, &log, true).as_deref(), Some("c\n"));
}

#[test]
fn lines_end_at_newlines_and_a_final_one_starts_nothing() {
    let got: Vec<&[u8]> = lines(b"a\nb\n").collect();
    assert_eq!(got, [&b"a"[..], b"b"]);
    let got: Vec<&[u8]> = lines(b"a\n\nb").collect();
    assert_eq!(got, [&b"a"[..], b"", b"b"]);
    assert_eq!(lines(b"").count(), 0);
    let got: Vec<&[u8]> = lines(b"\n").collect();
    assert_eq!(got, [&b""[..]]);
}
