//! Every write the store makes goes through `safeio`.
//!
//! A test binary of its own: `safeio`'s audit counters are process-wide, and
//! any other test running in the same process would add its own writes to the
//! count. Alone here, the deltas are exactly this test's.
//!
//! What each routing protects:
//!
//! - **a blob** -- the store treats a blob's presence as proof that its
//!   content is there, so one left half-written by an interrupted copy would
//!   be handed back, short, to every later snapshot that deduplicated
//!   against it;
//! - **`manifest.json`** -- lost, and the blobs are there but unreachable;
//! - **`meta.json`** -- the completion marker `list` keys off, so a
//!   half-written one can make a finished snapshot unlistable;
//! - **a restored file** -- written *over* a file the user still has, so a
//!   truncating write interrupted would destroy the original and fail to
//!   supply the replacement.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use scratchdir::ScratchDir;
use snapstore::{BackupType, CaptureOptions, RestoreOptions, Store};

#[test]
fn every_write_goes_through_safeio() {
    let scratch = ScratchDir::new("snapstore_audit");
    let src = scratch.dir().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), b"alpha").unwrap();
    fs::write(src.join("b.txt"), b"beta").unwrap();
    fs::write(src.join("a-again.txt"), b"alpha").unwrap();
    let store = Store::open(&scratch.dir().join("store")).unwrap();
    let opts = CaptureOptions {
        kind: BackupType::Full,
        excludes: Vec::new(),
        follow_symlinks: false,
    };

    // Capture: two new blobs copied (the duplicate is not), and the two
    // records written.
    let (copies, writes) = (safeio::copies_performed(), safeio::writes_performed());
    let cap = store.capture(&src, &opts, &mut |_| {}).unwrap();
    assert_eq!(
        safeio::copies_performed() - copies,
        2,
        "each new blob is copied in through safeio, and a duplicate not at all"
    );
    assert_eq!(
        safeio::writes_performed() - writes,
        2,
        "manifest.json and meta.json are written through safeio"
    );

    // Restore: one write per file restored.
    let out = scratch.dir().join("out");
    let writes = safeio::writes_performed();
    let report = store
        .restore(&cap.meta.id, &out, &RestoreOptions::default())
        .unwrap();
    assert_eq!(report.written, 3);
    assert_eq!(
        safeio::writes_performed() - writes,
        3,
        "every restored file is written through safeio"
    );
}
