## A-TEST-RECLAIM-SPACE-RACES-THIS-HOST-BACKUP-SCANNERS (lane A, 2026-09-09)

**In short:** `scripts/test-reclaim-space.py` failed one check during a boot
test and passes every time it is run by hand. It is not testing anything that
broke; it is racing the four backup jobs the operator runs continuously on this
machine, and losing about one run in four. Each loss costs a whole boot test,
because a tooling-suite failure refuses the build.

**The failing check:** `reclaim removes the directory`
(`test_reclaims_and_returns_a_number`).

**Why it fails, and why it is nobody's bug.** `reclaim_dir` renames a directory
into a staging name before deleting it, and that rename is the safety property,
not an implementation detail — `prune-build-cache.py` states the reasoning
plainly: *"Windows refuses to rename a directory with any file open inside it
and the rename is atomic, so a success proves nothing held it at that instant;
a failure means 'in use' and the unit is skipped, not forced."*

So for this check to fail, the **rename** must have failed, which means
something outside the test had the file open. Three facts fix the culprit:

- `make_tree` writes its 4096-byte `artifact.bin` inside `with open(...)`, so
  the handle is closed before the rename. The test does not hold its own file.
- `reclaim_dir` calls `os.rename` once, inside `try/except OSError`, with **no
  retry** — correct for production, where "in use" is a real answer, and fatal
  for a test that asserts the rename succeeded.
- The operator has already named this exact failure mode. Answering A-Q7 about
  slow and failing filesystem operations: *"failing to delete things could be
  because I have two continuous local backup jobs going at all times **and**
  two continuous cloud backups."*

A file created microseconds ago is precisely what a continuous backup scanner
opens first. The test writes one and immediately renames its parent.

**Observed:** failed in boot run `bd5eihdat` (2026-09-09, 1590 s in, during the
tooling-suite phase); passed standalone immediately afterwards, exit 0; passed
in the previous boot run the same afternoon. One failure in four runs.

**Not fixed, deliberately, and this is the judgement rather than the finding.**
One occurrence is a thin basis for changing a shared script, and there are two
candidate fixes with different blast radii:

1. **Retry the rename in `reclaim_dir`** (bounded, short backoff). Tempting,
   and it would make the *production* tool better too — `reclaim-space.py` is
   the emergency valve for a full disk, and skipping directories because a
   backup job glanced at them reclaims less than it could. But it changes the
   meaning of "in use" in the one place the project deliberately made that
   meaning sharp, so it deserves its own change and its own argument.
2. **Make the test tolerate it** — retry, or report a skip rather than a
   failure when the rename loses the race. `run_checker` already has
   `--may-skip` and `check-boot-skips.py` accounts for skips, so a skip here
   would be visible rather than swallowed.

(2) is the smaller change and the honest one: the test cannot assert that a
rename succeeds on a host where another process may legitimately be holding the
file. **Do (2) if this recurs.** The expected cost of waiting is about a quarter
of a boot test per run; the cost of changing a shared harness on a single data
point is a worse trade, and today has already produced two lost runs from
changes made faster than they were checked.

**Reproduce (may take several attempts, by nature):**

```
python scripts/test-reclaim-space.py
```

…while a backup or indexing pass is touching `%TEMP%`.
