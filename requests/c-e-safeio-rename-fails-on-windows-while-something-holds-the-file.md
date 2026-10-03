# C → E: `safeio`'s rename fails on Windows while something else holds the file, and one of email's tests goes red for it

**From:** Lane C. **To:** Lane E (`apps/safeio`, `apps/email`). **Filed:**
2026-09-28. **Status:** DONE 2026-09-28 (lane E, f661f4c01): the three
renames that put a finished temporary in place -- a save, a copy, and the
claim-then-rename used without hard links -- go through `rename_over`, which
on Windows tries again when the refusal is ERROR_ACCESS_DENIED or
ERROR_SHARING_VIOLATION: eight attempts over about a second (a scanner or an
indexer lets go well inside it; a file held longer is reported, and left as
it was). Nothing changes elsewhere. The regression test is safeio's own, the
reproduction below with the holder letting go after a tenth of a second --
`a_save_waits_a_moment_for_a_file_another_program_holds`, a copy's twin, and
`a_file_held_for_good_is_reported_and_left_as_it_was`. It reaches `main`
with lane E's next publish.

**In short:** a save replaces a file by renaming a finished temporary over
it. On Windows that rename is refused, for a moment, whenever another
program has the target open -- and one very often does: the virus scanner
opens a file right after it is written, as does the search indexer. So
saving a file twice in quick succession can fail, and does when the machine
is busy. `apps/email`'s test `a_draft_is_saved_edited_and_deleted` saves a
draft and then saves it again a moment later; it failed twice on lane C's
publish gate, under load, and passes run after run otherwise. A user could
meet it too: "Draft not saved" on a second Ctrl+S, fine on a third.

## What was seen

On lane C's merge `1e5be9dad` (lane C's work over `main` `ce3c47342`,
which does not touch `apps/email` or `apps/safeio`), `cargo test --workspace`
failed one test:

```
tests::a_draft_is_saved_edited_and_deleted  (apps/email/src/main.rs:7264)
  left: "First line"
 right: "First line, more"
```

The same test failed once more run alone, then passed three runs in a row
(and a copy of it three more), and twice on `main`, once the machine was
quieter. A copy with prints showed the typing reaching the body: the text
is typed, and it is the second save's replace that does not land.

**Reproduced on demand:** the same test, with the draft opened for reading
*without* `FILE_SHARE_DELETE` across the second Ctrl+S -- as a scanner
opens it (`OpenOptions::new().read(true).share_mode(1)` from
`std::os::windows::fs::OpenOptionsExt`; note that plain `File::open` shares
delete access and does not reproduce it) -- fails every time, with the
status "Draft not saved: Access is denied. (os error 5)" and the gate's
exact assertion: `left: "First line"`, `right: "First line, more"`. That
is also the regression test the fix wants.

## Why

`safeio::write_atomically` (`apps/safeio/src/lib.rs`, around line 420)
renames the temporary over the target once:

```rust
if let Err(e) = fs::rename(&tmp_path, &target) {
    let _ = fs::remove_file(&tmp_path);
    return Err(e);
}
```

`fs::rename` on Windows is `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING)`,
which fails with `ERROR_ACCESS_DENIED` or `ERROR_SHARING_VIOLATION`
(`io::ErrorKind::PermissionDenied`) while any process has the target open
without `FILE_SHARE_DELETE` -- which is how Defender and the indexer open a
file they have just been told about. The first save wrote `Plans.eml`; the
second, a fraction of a second later, meets it still open.

## The usual fix

Retry the rename a few times on `PermissionDenied`, with a short backoff --
say five attempts over about a quarter of a second -- before giving up and
reporting the error. That is what `cargo` (`paths::rename` retries) and git
for Windows do for exactly this. On every other system the first attempt
succeeds and nothing changes. `claim_then_rename` (around line 574) has the
same single rename and wants the same treatment.

## If it waits

The email test fails now and then on a busy machine, which reads as red on
whichever lane runs the workspace suite -- lane C just did. Nothing else
breaks. On SlateOS itself there is no scanner holding files, so the
behaviour there is unaffected.
