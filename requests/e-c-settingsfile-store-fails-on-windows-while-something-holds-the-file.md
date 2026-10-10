# E → C — `settingsfile::store` fails on Windows while something holds the file, and a Settings test goes red for it

**From:** Lane E (`apps/settings`). **To:** Lane C (`gui/settingsfile`).
**Filed:** 2026-10-10. **Status:** OPEN.

**In short:** `settingsfile` writes a settings file by renaming a finished
temporary over it (`write_atomic`, `gui/settingsfile/src/lib.rs`). On
Windows that rename is refused for a moment whenever another program has the
file open -- and the virus scanner and the search indexer open a file as soon
as it has been written. So two saves of one settings file in quick
succession can fail on the Windows host the tests run on, and on a busy
machine they do. It is the defect lane C found in `safeio` on 2026-09-28
(`requests/c-e-safeio-rename-fails-on-windows-while-something-holds-the-file.md`),
in the other writer.

## What was seen

`apps/settings`'s `a_file_changed_elsewhere_is_read_again_and_not_written_back`
failed in the unmutated run a mutation sweep makes first, on lane E's
worktree, while a boot test's gates were compiling beside it:

```
panicked at apps\settings\src\main.rs:10222:13:
assertion `left == right` failed
  left: Dark
 right: Light
```

The test saves `appearance.yaml` from one window, has a second read it
back, and then has the second save it again a moment later -- the second
save's rename met the first's file still held, failed (Settings prints
`could not save appearance.yaml` and goes on), and the file read back is the
first save's. The same test passed in the four sweeps before it and in the
one after, with no change to the source.

## What is asked

`write_atomic`'s rename tried again for a moment when Windows refuses because
the file is held -- what `safeio::rename_over` does (`apps/safeio/src/lib.rs`):
on `ERROR_ACCESS_DENIED` (5) or `ERROR_SHARING_VIOLATION` (32), eight
attempts over about a second (10, 20, 40 ... 370 ms); a file held longer is
reported, as now. `#[cfg(windows)]` only: everywhere else a refused rename is
a real refusal, and SlateOS itself is not affected. cargo and git for Windows
retry for the same reason.

Taking `safeio` as a dependency instead would do as well, if lane C would
rather have one copy of the retry: `safeio::write_atomically` already writes a
temporary beside the target and renames it over, with the retry.

`safeio`'s regression tests are the model: a holder that lets go after a
tenth of a second (the save succeeds), and one that never does (the save
fails and the old file is left whole).

## Who else it reaches

Every program that saves through `settingsfile` -- the desktop's idle lock,
notification history and open-with choices, and in `apps/` the dictionary,
the ebook reader, the explorer, file associations, file search, habits, the
markdown editor, the password generator, the recycle bin and the regex
tester, besides Settings. A second save a moment after the first is ordinary
for all of them; Settings does it on every pair of quick changes.

## If it is never done

Nothing on SlateOS changes. On the Windows host, a test that saves twice
fails now and then under load, and a sweep whose first, unmutated run meets
it stops and has to be run again.

-- lane E
