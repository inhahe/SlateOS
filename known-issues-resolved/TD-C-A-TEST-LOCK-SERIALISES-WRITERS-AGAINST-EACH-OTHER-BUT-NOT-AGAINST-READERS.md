## TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS -- FIXED 2026-09-08

**Marker added 2026-09-13, not the fix.** The fix landed on 2026-09-08 and
this entry's body has described it in the past tense ever since -- the
reentrant `ConfigTurn`, the thread-local depth, `TestDesktop` holding one for
its lifetime. Only the heading was never marked, so the entry read as open
work. It was picked up as the next task today and four screens of it were
read before the code was checked and found to already carry the fix, with all
94 tests passing.

That is the cost of an unmarked heading, and it is the same shape as every
other near-miss in this file: a confident statement about a population nobody
has re-checked. Three other entries had it too -- the numeric keypad, the
clock display, the reattached attribute -- all fixed in August, all still
reading as open in September.

**Date:** 2026-09-08. **Lane:** C.
**Where:** `gui/settingsfile/src/…` — `testing::with_scratch_config`;
`gui/window/src/app.rs` — the 27 tests that call `drive()`.

**In short:** four tests in the window library point the "where is the config
file" setting at a temporary directory of their own, so they can write a theme
file and watch it being noticed. That setting is one value shared by the whole
test *process*, and the other twenty-three tests read it without knowing. When
the timing lines up, one of those twenty-three sees a theme file that a
different test put there, redraws because of it, and fails an assertion about
how many times it drew.

**How it shows.** `cargo test -p oswindow --lib` fails
`app::tests::a_file_written_while_answering_idle_is_still_announced` with
`left: 2, right: 1` — "announcing is not redrawing: only the unprompted first
frame". Run **alone**, `cargo test -p oswindow a_file_written_while` passes.
That difference is the whole diagnosis: a test that passes by itself and fails
in company is sharing something.

**What is shared.** `with_scratch_config` takes an `ENV_LOCK` mutex and then
sets a process-global **environment variable** naming the config directory. The
lock makes its own callers take turns, which is what it was written for. It
does nothing about a *reader* that never calls it: `drive()` constructs a
`ThemeWatch`, which constructs an `appearance::config::Watcher`, which resolves
the config directory from that same env var. Twenty-three tests reach that path
and none of them hold the lock.

So the invariant the lock provides is "two scratch configs are never installed
at once", and the invariant actually needed is "nobody reads the config
directory while a scratch one is installed". The second is strictly stronger.

**Why it is worth more than a retry.** The failing assertion is about a real
rule — *announcing a settings change to an application must not redraw it* —
and the test is the only thing holding that rule. A flaky test on a real rule
gets muted, and then the rule is unprotected. It is also latent for every test
added to this file in future: 23 of 27 are one scheduling accident from the
same failure.

**Not caused by, but found during,** the blur work: `cargo test -p oswindow`
had not been run directly in a while, and the failure reproduces with the blur
change stashed.

**The fix.** Make the reader take the lock too, rather than wrapping
twenty-three tests by hand: the shared `desktop()` test helper that every
`drive()` test already uses should hold the config lock for the life of the
test. That serialises all 27 against the 4, which is affordable — the whole
suite runs in hundredths of a second — and it cannot be forgotten by the next
test added, which wrapping by hand can.

An env var is process-wide by definition, so making the override thread-local
is not available; serialising the readers is the only option that does not
change what `Watcher` reads from.

**Fixed 2026-09-08, and the first attempt deadlocked** — worth recording,
because the failure mode is worse than the bug. `TestDesktop::new` was made to
take `ENV_LOCK` directly, which is correct for the twenty-three readers and
fatal for the four writers: they call `with_scratch_config` and build a
`TestDesktop` **inside** it, and `std::sync::Mutex` is not reentrant. A second
`lock()` on the same thread is not an error, it is a stop — the suite hangs
with no failing test and no message, which is harder to diagnose than the
flake it replaced.

The turn is now reentrant by a thread-local depth: the outermost holder on a
thread takes the mutex and everything nested inside it merely raises the count.
`cargo test` runs a binary's tests as threads of one process, so per-thread is
exactly the right grain. `TestDesktop` holds a `ConfigTurn` for its own
lifetime, so a test takes the turn whether or not its author knew there was
one.

**A second, benign oddity found on the way:** `settingsfile` has *two*
`ENV_LOCK` statics — one in `mod testing` (line 352) and one in its own
`#[cfg(test)] mod tests` (line 515) — guarding the same process-global
environment with different mutexes, so they do not exclude each other. It is
harmless today because the two are never live in one build: the crate's own
unit tests run with the `testing` feature off, and a dependent's run with it
on. Left as it is rather than merged, since merging them would mean exposing
the lock from a module that is compiled out of exactly the build that needs the
other one. Noted so that a future `[features] default = ["testing"]` is
understood to make them overlap.
