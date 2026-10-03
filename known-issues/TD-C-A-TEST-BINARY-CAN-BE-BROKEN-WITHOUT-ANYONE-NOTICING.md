## TD-C-A-TEST-BINARY-CAN-BE-BROKEN-WITHOUT-ANYONE-NOTICING (lane C, 2026-08-22)

**What.** A crate whose test binary fails to *compile* reports no test
failures. It reports nothing at all — no `test result` line — and in a filtered
build log that is indistinguishable from a crate with no problems. Because the
per-task gate on this lane has usually been `cargo test -p <the crate I edited>`,
a change in one crate can break a *sibling* crate's tests and nobody finds out
until something else happens to build the workspace.

**It already happened, and it lasted a day.** `apps/editor`'s
`mod against_the_real_compositor` — the four tests that put the shipped editor
on one end of a real socket and the shipped compositor on the other, i.e. the
most valuable tests in that crate — **had not compiled since commit
`7d1667856` (2026-08-21, the frame clock, `design-decisions.md` §521)**. That
change moved `set_wait_timeout` from an inherent method on
`guiremote::socket::Socket` onto the `Transport` trait, so every transport could
park with a deadline. `apps/editor`'s test module calls it and does not
otherwise name the trait, so it stopped resolving:

```
error[E0599]: no method named `set_wait_timeout` found for struct
              `guiremote::socket::Socket` in the current scope
```

§521's own gate was `cargo test -p oswindow -p guiremote -p desktop`, which is a
perfectly reasonable-looking set, and `apps/editor` is not in it. It was found
on 2026-08-22 only because the Settings event-loop task happened to run
`cargo test --workspace` at the end.

**The one-line fix is in** (`use oswindow::ConnectionTransport as _;` in
`apps/editor/src/main.rs`, with a comment pointing here). This entry is about
the *process hole*, which is still open.

**Why it is worse than an ordinary silent failure.** A test that fails is loud.
A test that does not exist is at least visibly absent from the count. A test
binary that does not compile *claims nothing* — and the crates most likely to
break this way are exactly the ones whose tests reach across crate boundaries,
which is to say the integration tests that are the whole reason
`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` was worth closing. The tests that
protect the seams are the tests most exposed to a change on the other side of a
seam.

**Reproducing it.** Delete the `use oswindow::ConnectionTransport as _;` line
from `apps/editor/src/main.rs`, then run
`cargo test -p oswindow -p guiremote -p desktop --target x86_64-pc-windows-gnu`.
It is green. `cargo test --workspace` is not.

**The proper fix**, in the order the cost forces:

1. **Make the workspace the gate, not the crate.** Done as practice from
   2026-08-22: `cargo test --workspace --target x86_64-pc-windows-gnu` before
   any commit that touches a crate other crates depend on — which, for anything
   in `gui/`, is all of them. It costs a few minutes and it is the only check
   that actually covers this.
   **Use `--no-fail-fast`.** Found the same day, and it is the second half of
   the same hazard: `cargo test --workspace` stops at the *first* failing test
   binary, so every crate scheduled after it is never run and reports nothing —
   which reads, once again, exactly like "no problems". A flaky test in
   `userspace/polkit` aborted the run before `apps/settings` was reached; the
   re-run with `--no-fail-fast` then found two *more* flaky tests, in
   `userspace/ftpd` and `posix`, that the first run had never got far enough to
   reach. All three are lane B's and are filed as
   `requests/c-b-three-flaky-tests-fail-the-workspace-gate.md`. A gate that can
   be silenced by an unrelated crate's unrelated failure is not a gate.
2. **Do not filter a build log in a way that can hide a compile error.**
   Grepping for `test result|FAILED` is exactly the filter that made this
   invisible, because a crate that did not build produces neither. Any filter
   must include `^error` and the runner's exit status must be checked; a
   `test result` tally with no `error` line and a zero exit is the only green.
3. **A `cargo check --workspace --all-targets`** would catch it in a fraction
   of the time of a full test run, since it is a compile problem and not a
   behavioural one.

   **UPDATE 2026-09-13: that guard already exists, and this item did not know
   it.** `scripts/boot-test.sh` runs
   `cargo clippy --workspace --exclude kernel --all-targets --target
   x86_64-unknown-linux-gnu`, which compiles every test target in the
   workspace. A test binary that does not build fails it. The reason nobody
   here counted it is that it runs inside the gate *named* `cfg-unix`, and a
   gate whose name describes a narrower population than it checks is invisible
   to anyone looking for the wider one -- which is this entry, looking for
   exactly that guard and concluding it did not exist.

   Lane A found the same confusion from the other side on 2026-09-13 and has
   written it into `scripts/check-cfg-unix.py`'s docstring: the *script* of
   that name checks 62 crates on their default targets, while `boot-test.sh`
   checks the whole workspace on a unix target with `--all-targets`, so "a
   pass here does not predict a pass there". Two gates, one name, two
   populations.

   `--exclude kernel` is load-bearing and is why the naive form does not work:
   `cargo check --workspace --all-targets` fails on the kernel with
   `duplicate lang item panic_impl`, because `--all-targets` builds a test
   harness for a `no_std` binary that defines its own panic handler. That is
   `TD-C-CARGO-BUILD-WORKSPACE-ON-THE-HOST-TARGET-FAILS-ON-THE-KERNEL`.

   **The push-time half was the open question, and the measurement closes it:
   no.** Timed on 2026-09-13 in `os-lane-c`:

   | `cargo check --workspace --exclude kernel --all-targets` | |
   |---|---|
   | run after a `cargo test --workspace` | **3 m 10 s** |
   | run again immediately after itself | **1.75 s** |

   The check is nearly free *if the previous command was also a check*, and
   costs three minutes if anything ran the tests first -- `check` and `test`
   invalidate each other's fingerprints in the shared `target/`, which
   `boot-test.sh` already says of `check` and `clippy`. Since this lane's
   practice is `cargo test --workspace` **before** every push (item 1 above),
   the realistic case is always the three-minute one, against a pre-push hook
   that currently takes 90-115 s in total. It would triple the push.

   **And it would not even cover the same population.** A push-time check
   would run the Windows target; `boot-test.sh` runs the unix one, which is
   what makes it see `#[cfg(unix)]` arms and unix-only test code at all.
   Matching its coverage means a second target directory's worth of
   artifacts, which the global build-output rules forbid leaving behind.

   So the guard stays where it is, in the boot test. What this lane can do at
   push time is what it already does: run the workspace tests, and read the
   exit status rather than a filtered log -- item 2 above, which is the half
   that actually catches this and costs nothing extra.

**Update 2026-09-13 (lane C) — a fourth trap, in the gate itself.**

`scripts/workspace-test.py` exists precisely to make the three log-reading
mistakes impossible, and it was missing the one this entry is about. Every
check in it asked whether something went *wrong*; none asked whether anything
*happened*. A run producing no `test result` line at all -- a `--target` typo, a
filter matching nothing, cargo's output going somewhere other than the log --
reached the end with no failures and a zero exit, printed `targets passed: 0`,
and then printed `PASS` on the next line. The two disagree and the second is
the one anybody acts on.

It now exits 2 there, not 1: the tree is not red, the measurement is missing,
and those deserve different answers. Proved able to fire by making `RESULT_OK`
match nothing and watching a genuinely green `-p guitk --lib` run come back
*NO TARGET REPORTED A RESULT* instead of *PASS*.

That is this entry's own defect one level up. A crate whose test binary does not
compile reports nothing and reads as clean; a *gate* that reports nothing reads
as clean for the same reason and over a much larger population. The guard is
eight lines and would have caught both of the afternoon's redirection mistakes
-- `2>&1 > log` puts cargo's output on the terminal and leaves the log empty,
which is exactly the zero this now refuses.

**Severity.** Medium, and it is a *meta*-defect: it does not itself break
anything a user can see, it removes the evidence that something else did. The
specific instance is closed; the hole that let it persist for a day is only
closed by (1) being kept up, which is a habit rather than a mechanism until (3)
exists.

**Where.** Not a location in the code — a property of how this lane runs its
gate. The instance was `apps/editor/src/main.rs`'s
`mod against_the_real_compositor`; the cause was `gui/remote/src/client.rs:143`
(`Transport::set_wait_timeout`).
