## B-SCREENSHOT-EVERY-SCREENSHOT-OVERWROTE-THE-ONE-BEFORE-IT (lane C, 2026-08-16) — FIXED

**In short:** The screenshot tool named each saved file after the moment it was
taken, down to the second — `screenshot_20260815_142233.bmp`. Two screenshots
taken in the same second therefore got the *same* name, and the second one was
written straight over the first. The pop-up said "Screenshot saved to …" both
times, so nothing told you one of them was gone. Worse, the capture code
currently fills in a **fixed placeholder timestamp** rather than reading the
clock, so right now *every* screenshot produces the identical filename and each
one destroys the last. Separately, the file was written in a way that empties
the old file first, so a save that ran out of disk space left you with neither
the new screenshot nor the one you already had.

**Where:** `apps/screenshot/src/main.rs` — `write_bmp`, `ScreenshotApp::finish_capture`,
`ScreenshotApp::save_current`, and `Capture::default_filename`.

**What was wrong**

1. **The filename was assumed to be unique and is not.**

   ```rust
   let save_path = self.settings.save_directory.join(capture.default_filename());
   ```

   `default_filename` is `format!("screenshot_{:04}{:02}{:02}_{:02}{:02}{:02}.bmp", …)`
   over `self.timestamp` and nothing else. One second of resolution is not
   enough for a burst of captures, which is an ordinary way to use this tool.
   And `Capture::new` sets `timestamp: (2026, 1, 1, 0, 0, 0)` — a placeholder
   with a `// real implementation reads from system clock` comment — so the
   collision is currently total rather than occasional. Both auto-save
   (`finish_capture`) and manual save (`save_current`) used the name as given.

2. **`write_bmp` used `fs::write`,** which opens with `O_TRUNC`: the target is
   emptied before the new bytes are written. Re-saving an annotated capture
   over its own file and failing part-way through therefore loses both
   versions. This is the exact failure `apps/safeio` was created to prevent,
   and the editor, markdown editor, paint and installer already route through
   it; screenshot did not even depend on the crate.

**The general shape.** Same rule as the rest of this sweep, in its naming form:
*a derived name is only unique if something guarantees the input is.* A
timestamp is not an identity — it is a measurement, and measurements collide at
their resolution. Treating a derived filename as a claim to a free path is the
same mistake as treating a file's existence as a claim about its contents: in
both cases the program acts on a promise nothing made.

**The fix**

- `unused_save_path(dir, filename)` returns the given name if it is free and
  otherwise `stem (N).ext` for the first free `N` — suffix *before* the
  extension, so the file stays a `.bmp` to anything dispatching on extension.
  Bounded at 10 000 so an unreadable directory fails the save instead of
  spinning.
- Re-saving the same capture must still rewrite its *own* file — the "draw an
  arrow, save, draw another, save again" loop must not leave a trail of
  near-duplicates, and an existing test (`annotations_reach_the_saved_file`)
  already asserted `path == path2`. The filename cannot distinguish "same
  capture again" from "different capture, same second", so the path is
  remembered in `ScreenshotApp::current_saved_path`: cleared when a new capture
  arrives (`finish_capture`) or the current one is dropped (`discard_current`),
  and set **only on a successful write** — a failed first save must not claim a
  name it did not create.
- `write_bmp` now calls `safeio::write_atomically`.

**Tests** (7 new, 68 pass): `a_second_capture_does_not_overwrite_the_first_ones_file`,
`a_disambiguated_name_keeps_its_extension`, `re_saving_one_capture_rewrites_its_own_file`,
`a_discarded_capture_does_not_lend_its_file_to_the_next_one`,
`a_failed_save_claims_no_filename`, `a_rejected_encode_leaves_the_previous_file_untouched`,
`a_save_leaves_no_temporary_files_behind`.

**Verified by injection.** Restoring the plain `join` reddens exactly two of the
new tests and no pre-existing one.

**Also in this commit.** The `#[cfg(test)]` module was missing the
`unwrap_used`/`expect_used`/`panic`/`indexing_slicing` allowances that CLAUDE.md
sanctions for tests and that every other app crate carries, so it was
generating 94 clippy warnings. Now 1 (pre-existing, in the binary).

**Still open in this program**

- **`Capture::new`'s timestamp is a placeholder.** `(2026, 1, 1, 0, 0, 0)` with
  a comment saying the real implementation reads the system clock. Until that
  is wired up every filename is `screenshot_20260101_000000*.bmp` and the
  disambiguating suffix is doing all the work. Not a data-loss bug any more,
  but the names are useless for finding anything. Needs the clock syscall.
