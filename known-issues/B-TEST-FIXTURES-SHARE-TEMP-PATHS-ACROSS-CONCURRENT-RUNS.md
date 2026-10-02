## B-TEST-FIXTURES-SHARE-TEMP-PATHS-ACROSS-CONCURRENT-RUNS (lane B, 2026-08-22) — lane B's half FIXED, lane C's half FILED

**In short:** a test that names its scratch directory after a fixed string, or
after the clock, shares that path with every other copy of the suite running at
the same time. Two `cargo test` runs overlapping is ordinary, not exotic, so the
tests delete and overwrite each other's fixtures. The failures land on the code
under test — `du: cannot access …`, `save: Io(NotFound)` — which is why they
were not recognised as fixture bugs for as long as they were.

**How it was found.** A workspace run of mine failed two `apps/screenshot` tests
while a second workspace run was still in flight. Chasing that rather than
re-running it turned up the same defect in twelve crates.

**Why the clock is not a fix, only a disguise.** The system clock is refreshed
on a timer interrupt rather than recomputed per read, so two threads reading it
inside one tick get the same value however many digits it carries — and `cargo
test` runs a suite's tests as threads of one process. Lane C measured 2133
collisions in 16000 draws (13%); the figure is recorded in
`userspace/scratchdir/src/lib.rs`. Adding the pid separates concurrent *runs*
and does nothing for concurrent *threads*. `userspace/scratchdir` draws from the
pid **and** a process-wide `AtomicU64`, which covers both axes by construction.

**Measured, 2026-08-22** — six copies of one test binary, run at once, no source
changes:

| suite | runs failing (of 6) | distinct tests |
|---|---|---|
| `screenshot` (lane C) | 6 | 6 |
| `explorer` (lane C) | 3 | 2 |
| `imageviewer` (lane C) | 1 | 1 |
| `du` (lane B, before the fix) | 1 | 1 |

**Lane B — fixed.** `ca36f3e47` (du, 3 sites) and `d733787ee` (crond2 4, userdb
2, vi 2, polkit 1). All now use `ScratchDir`; the hand-written cleanup tails are
gone with them, since `Drop` covers the failing test that a trailing
`remove_dir_all` structurally cannot reach. Re-measured at 15 binaries running
at once: green.

Deliberately left alone, and sound as they stand: `fio` (pid plus a distinct
per-test tag), `filekind` and `tail` (pid plus `ThreadId`, which Rust guarantees
is never reused).

**Lane C — filed**, not fixed, because `apps/**` and `gui/**` are not mine to
edit: `requests/b-c-test-fixtures-in-apps-and-gui-race-on-shared-temp-paths.md`
lists 6 fixed-name sites and 12 clock-tagged ones with file and line, the
one-command reproduction, and the conversion. `apps/installer/src/grub.rs`
already does it correctly and is left alone.

**The lesson worth keeping.** `polkit` had already been fixed once — from a
fixed name to a nanosecond tag — with a comment that diagnoses the race
correctly and then picks a fix that does not work. A rarer, stranger failure is
worse than an obvious one. When a fixture needs to be unique, take the
uniqueness from a counter, never from a clock.

### Addendum (2026-08-23): the race poisons its path permanently — and blocked the merge gate

The entry above described a probabilistic failure. It is worse: the race can
convert itself into a **deterministic, permanent** failure that survives the run
that caused it, and it did.

A clean single-run `cargo test --workspace` — nothing else in flight — failed in
`screenshot` at `apps/screenshot/src/main.rs:2044` with `AlreadyExists` from
`create_dir_all`. The cause was that
`std::env::temp_dir()/slateos-screenshot-litter` had become **a 118-byte 4×4 BMP
file** where the helper expects a directory: `remove_dir_all` therefore failed
(not a directory), the helper discarded that error with `let _ =`, and
`create_dir_all` failed on the file already there. Measured both ways — poisoned:
3 runs, 3 failures; after deleting that one file: 69 passed, 0 failed.

The poison is written by the very test that then cannot run.
`a_save_leaves_no_temporary_files_behind` ends with a deliberate negative case
that writes **to the directory path itself** and asserts the write fails —
which holds only while that path *is* a directory. When a concurrent run's
`temp_dir("litter")` deletes the path in the window before that line, the write
succeeds and leaves a BMP at the directory's name.

Three properties make this materially worse than the parent entry:

- It is **not cleared by `cargo clean`** — the poison is in the system temp
  directory, not `target/`.
- It is **invisible from the repository**: nothing in the tree names
  `slateos-screenshot-litter`, so the next reader gets `AlreadyExists` from a
  line that says `create_dir_all` and no reason to suspect a file.
- It **blocks all three lanes**, because `cargo test --workspace` is the shared
  merge gate. It blocked lane B's merge to `main` on 2026-08-23, for a defect in
  a crate lane B may not edit.

Recorded in full, with the mechanism and the `write_bmp(&dir, …)` line that
writes the poison, in the addendum to
`requests/b-c-test-fixtures-in-apps-and-gui-race-on-shared-temp-paths.md`.
`ScratchDir` prevents both halves: it never reuses a name and never opens by
deleting. Workaround until then: delete `%TEMP%/slateos-screenshot-litter` — it
is a file, so `rmdir` will not remove it.

### Addendum (2026-08-26): "lane B — fixed" was three crates short, and the survey that missed them was looking for the wrong thing

Lane C's `requests/c-b-fixed-temp-paths-make-userspace-tests-fail-when-two-runs-
overlap.md` reported `firejail` still failing on a fixed path, four days after
the entry above recorded lane B's half as done. Auditing every `temp_dir()` in
lane B's tree rather than just that one crate found two more. Fixed in
`f051d93b0`: `firejail` (8 sites), `useradd` (1 fixture struct), `sed` (3).

Measured on Windows, six processes each looping the whole suite, 720 runs:

| suite | runs failing before | after | distinct tests |
|---|---:|---:|---:|
| `useradd` | 531 (74%) | 0 | 13 |
| `sed` | 137 (19%) | 0 | 3 |
| `firejail` | 605/1200 (50%)¹ | 0 | 5 |

¹ measured over 1200 runs filtered to the sandbox-file tests.

**Why the first sweep missed them, which is the part worth keeping.** It looked
for the two spellings the incident had taught it — a fixed name, and a name
drawn from the clock. `useradd` matched neither. It named its directory from a
process-wide `AtomicU64` counter, which is *the instrument this very entry
recommends*, and it is genuinely the right fix for the axis a clock misses. But
a counter restarts at 0 in every run, so two concurrent runs walk
`useradd_test_0`, `_1`, `_2` in lockstep — and `TestEnv::new` opened by
`remove_dir_all`-ing that path, so each run deleted the other's fixture
mid-test. It reads as the corrected version of the bug while being the bug.

So the property to grep for is not a spelling. **A fixture name must vary along
both axes — across concurrent runs (pid) and across the threads within one run
(counter) — and a name that varies along only one is broken whichever one it
picks.** A pid alone is safe only where exactly one test uses that name, which
is why `wc`, `fio` and several oils sites are sound and were left alone;
`filekind` and `tail` use pid plus a `ThreadId`, which Rust guarantees is never
reused, and are also sound.

The three crates each gained a `[dev-dependencies]` section for `scratchdir`;
none had one, which is a second reason a survey by grep alone under-reports —
a crate with no dev-dependencies cannot be using the shared fixture.
