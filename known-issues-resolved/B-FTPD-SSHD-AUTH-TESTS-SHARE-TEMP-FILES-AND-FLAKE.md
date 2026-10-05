## B-FTPD-SSHD-AUTH-TESTS-SHARE-TEMP-FILES-AND-FLAKE — 2026-08-21 — lane B — FIXED 2026-08-21, see Resolution at the end

**In short:** The tests that check FTP and SSH password handling build their
throwaway `/etc/shadow` files by stamping the current time into the filename.
The clock is not fine-grained enough for that: when cargo runs the tests side by
side, two of them get the *same* file, one overwrites the other, and whichever
reads second is authenticating against the wrong data. Measured collision rate
on this machine: **13%**. It shows up as an occasional red in a full-workspace
run and is otherwise invisible.

**Found by:** lane C, during the full-workspace gate for the window-decorator
deletion. Not lane C's change — `ftpd` depends only on `authlib`, and the
change touched nothing outside `gui/**`. Filed to lane B as
`requests/c-b-ftpd-sshd-auth-tests-share-tmp-files-and-flake.md`, which carries
the full diagnosis and three suggested fixes; this entry exists so the bug is
tracked rather than living only in a request another lane may not merge for a
while.

**Where:** `userspace/ftpd/src/main.rs:3040` (`tmp_path`), and the same helper at
`userspace/sshd/src/main.rs:4700`. sshd's copy adds a `get_pid()` prefix, which
does **not** help — every test in a binary shares one process, so it separates
concurrent *runs* of the suite and not the concurrent *threads* inside one.

**Symptom:**

```
---- tests::an_unrecomputable_entry_is_broken_not_wrong stdout ----
assertion `left == right` failed
  left: Rejected
 right: Unusable
```

`Rejected` instead of `Unusable` is the diagnostic, not noise: the test writes a
plaintext shadow field (which must report `Unusable`) and reads back some other
test's line — a locked or validly-hashed one — which it then correctly
rejects. The assertion is right; the file under it changed.

**Reproduce:** `cargo test --workspace --target x86_64-pc-windows-gnu`. It is
load-dependent — `cargo test -p ftpd --bin ftpd` alone was 8/8 green across
eight consecutive runs.

**Why it matters more than an ordinary flake.** These tests pin *authentication
outcomes*: locked accounts, plaintext shadow fields, unknown users, rate
limiting. A test reading another test's shadow file can fail spuriously — which
is what was seen — but it can just as easily **pass** spuriously, and a green
run is precisely the evidence that would be cited for "auth is covered". Until
this is fixed the suite is not a reliable witness to its own claims.

**Secondary defect in the same helper:** cleanup is `let _ =
fs::remove_file(shadow)` at the end of each test body, so a panicking test leaks
its file. No `Drop` guard. A per-test temp *directory* would fix uniqueness and
cleanup together.

**If never fixed:** an intermittent red in the shared merge gate that each lane
pays for in turn, over exactly the code where a false green is most expensive.

### Resolution — 2026-08-21 (lane B)

**There were three copies of the helper, not two.** Lane C's request closed with
a question — "please check that the on-disk tally directory is not itself derived
from a clock-based name" — and the answer differs by where you look. In
production it is a constant (`authlib::DEFAULT_FAILLOCK = "/var/run/authlib/tally"`),
and `ftpd`/`sshd` never call `with_faillock` at all, so no tally reaches the
filesystem from those two suites. But **`authlib`'s own tests**, the one suite
that does exercise the on-disk tally, held a third copy of the same
`SystemTime::now().as_nanos()` helper, feeding both the shadow fixtures and the
tally fixtures — plus four more hand-rolled fixture paths in `faillock.rs`. All
are now on the same guard, and `authlib`'s `tmp()` is a `thread_local!`
directory, which is one directory per `#[test]` because cargo gives each test its
own thread.

That is the more useful statement of the defect: it was never "ftpd's helper is
wrong", it was "this helper was copied", and the copy that mattered most sat in
the crate that neither failing test lived in.

**And in the end there were five, so it became a crate.** Sweeping the rest of
the lane for the same idiom found it again in `doas` and `logind` -- also
`authlib`-backed shadow fixtures, also security tests. By then four crates had
each been given a locally-written `TempDir` guard carrying the same `Drop` and
the same doc comment: the original defect reproduced inside its own repair. All
five now use `userspace/scratchdir`, one shared and tested implementation,
following the same convention as `textfind`, `byteread` and `randrange`. See
design-decisions.md §349, and `TD-B-SCRATCH-PATH-HELPERS-NOT-YET-ON-SCRATCHDIR`
for the seven non-auth crates still to convert.

`ScratchDir` hands each test a private directory instead of a shared one, and
the directory removes itself on `Drop`. Uniqueness comes from the pid and a
process-wide `AtomicU64`, which cover the two axes exactly: the pid separates
concurrent *runs* of the suite, the counter separates concurrent *threads*
inside one run. Neither consults the clock, so neither depends on it having
advanced.

That last point is the substance of the fix and worth stating, because the
obvious repair — more digits, a finer clock, `Instant` instead of `SystemTime`
— would not have worked. The old scheme was not merely *low-resolution*; it
was asking a periodically-refreshed value to serve as a unique id, which it is
not, at any resolution. A counter is unique by construction.

`get_pid()` was already present in sshd's version and was already correct; it
was the nanosecond half that was load-bearing and wrong. It is kept.

The secondary defect — a panicking test leaking its file, because
`let _ = fs::remove_file(shadow)` sits at the *end* of the test body and an
unwind goes straight past it — is fixed by the same change, since `Drop` runs
during unwind. All nineteen manual cleanup tails are deleted.

**Regression tests, split between the shared crate and its consumers**, because
the original defect was detectable only statistically and that is what let it
live. `scratchdir` owns the properties that are the same everywhere; each
consumer keeps one test of its own *wiring*, which is the half a shared crate
cannot check for it:

- In `scratchdir`:
  - `two_scratch_dirs_alive_at_once_never_share_a_path` — sixty-four guards held
    alive at once, asserting both that the paths are distinct *and* that each
    file still holds its own bytes. Distinct paths alone would not be enough: a
    later guard clearing an earlier one's directory would produce the same
    read-someone-else's-data symptom.
  - `concurrent_threads_never_share_a_path` — eight threads building 200 guards
    each, all 1600 paths distinct. This is lane C's collision probe turned into
    an assertion; it is the exact experiment that fails 13% of the time against
    the code it replaces.
  - `the_directory_is_removed_even_when_the_test_panics` — panics inside
    `catch_unwind` carrying the path as the payload, then asserts the directory
    is gone. This pins the case the old cleanup tail structurally could not
    reach.
  - `a_path_is_not_created_until_the_caller_writes_it`, because several fixtures
    specifically need an *absent* path, and
    `the_directory_and_its_contents_go_together`.
- In each of `ftpd`, `sshd`, `doas` and `logind`:
  `twenty_fixtures_alive_at_once_each_authenticate_their_own_user` — twenty
  fixtures held alive simultaneously, each authenticating **its own** user. A
  fixture that returned the path and let the guard drop would compile, read
  fine, and leave every test in the suite checking against a file that no longer
  exists; this is what catches that. `logind`'s copy is
  `twenty_daemons_alive_at_once_each_read_their_own_store`, and matters more
  there because the daemon re-reads its store at every `authenticate_session`
  rather than once at construction.
- In `authlib`: `tmp_gives_each_thread_its_own_directory_but_repeats_within_one`
  — its fixture is a `thread_local!`, precisely because cargo gives each
  `#[test]` its own thread, and repeated calls inside one test must return the
  same path.

None of them is load-dependent, which is the point: the property is now checked
directly rather than inferred from the absence of a red.

**The rate limiter itself was cleared of the second charge.** Lane C asked
whether `sshd`'s `RateLimited`-became-`Rejected` failure meant a tally could be
reset by an unrelated concurrent writer. It does not, and the failure needs no
such mechanism: that test's tally is in-memory and daemon-wide, and what the
colliding writer destroyed was the *shadow line*. With `alice`'s line clobbered
each wrong guess is a lookup miss, which is `Rejected` without being counted, so
the loop never accumulates a tally and the final assertion correctly sees
`Rejected`. The limiter behaved correctly on the input it was handed; the input
changed underneath it.

`cargo test` on the five converted crates plus `scratchdir`: 112, 140, 90, 32,
126 and 5 passed, 0 failed; clippy clean on all six; the full workspace gate
green.
