## TD-C-THE-CONFIG-DIRECTORY-RACE-WAS-CURED-IN-ONE-BINARY-AND-RECURS-IN-EVERY-OTHER

**Date:** 2026-09-16. **Lane:** C.
**Where:** `gui/window/src/lib.rs` — `TestDesktop::_config_turn`, which is
`#[cfg(test)]`; and every dependent test binary that both drives an event loop
and writes a scratch configuration. Repaired in `apps/settings` by the commit
carrying this entry.

**In short:** tests that run a program's event loop keep reading a setting
that says where the user's configuration files are kept. A *different* test in
the same program can point that setting at a temporary folder while they are
running, and the loop then decides the user has just changed their settings
and repaints. A fix for this landed on 2026-09-08 and works — but only for the
tests inside `gui/window` itself. Any other program using the same test
desktop gets none of it, and the fault returns the first time that program's
tests also write a temporary configuration.

**How it came back.** Adding one test to `apps/settings` that calls
`settingsfile::testing::with_scratch_config` made
`loop_tests::settings_draws_once_at_startup_and_then_only_when_something_changed`
fail with `left: 3, right: 2`. That is the same symptom
`TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`
records for `gui/window` (`left: 2, right: 1`), from the same cause, eight days
after that entry was marked FIXED.

**Why the cure did not carry.** `TestDesktop` holds a `ConfigTurn` for its
lifetime, which is the cure. The field is `#[cfg(test)]`, and its own comment
gives the reason plainly: the guard's type comes from a dev-dependency and
cannot appear in the shipped library. Under `cargo test -p oswindow` the field
exists; when `apps/settings` links `oswindow` as an ordinary dependency it does
not. The comment's justification -- "cargo test gives each binary its own
process … the race is within one binary, so the cure belongs in the same
place" -- is correct, and is precisely why the cure has to be repeated in every
binary rather than installed once in the harness. A fix that lives behind the
fixing crate's own `cfg(test)` is a fix for that crate and an assumption
everywhere else.

**Rate.** Not once in ten consecutive runs of `cargo test -p settings`. It
appeared in a run of four crates together, where the extra load changes thread
interleaving. A rate that low is exactly why this reads as noise and gets
re-run rather than diagnosed -- and re-running it "green" is what would have
buried it.

**The population, enumerated** -- because this file's own warning is about
"a confident statement about a population nobody has re-checked". Six files
use `oswindow`'s `testing::desktop()`:

| File | Writes a scratch config? | Status |
|---|---|---|
| `gui/window/src/lib.rs` | yes | guarded, `#[cfg(test)]` field |
| `gui/desktop/src/session/tests.rs` | yes | guarded, takes `config_turn()` itself |
| `apps/settings/src/main.rs` | yes | **was exposed**; nine tests now take `config_turn()` |
| `apps/editor/src/main.rs` | no | latent -- no writer in that binary today |
| `apps/match3/src/main.rs` | no | latent |
| `apps/pinball/src/main.rs` | no | latent |

So nothing is exposed right now. The three "latent" rows become live the day
someone adds a scratch-config test to them, and nothing in the build will say
so: the failure surfaces as a draw count in an unrelated test, months later.

**The proper fix.** Two options, and they are not equivalent:

- **(a) Each binary takes the turn.** What `gui/desktop` and now
  `apps/settings` do. Closes it one binary at a time and relies on whoever
  adds the tenth test knowing to do it. This is what has been applied.
- **(b) `settingsfile` stops using an environment variable to locate the
  configuration directory under test**, so there is no process-global to race
  on at all. This removes the class rather than the instance, and would let
  `TestDesktop`'s guard be deleted rather than replicated.

(b) is the right end state and is not attempted here: it changes the shape of
every test that writes a settings file, across more than one lane's trees.
Filed rather than done, with the trigger being the next recurrence -- or, more
usefully, a gate that refuses a test binary containing both
`testing::desktop()` and `with_scratch_config` without a `config_turn()`.


**Status 2026-09-16: a gate exists, and its first version was not one.**
`scripts/check-config-turn-guards.py` is pre-push gate 47. Its rule: in a crate
that writes a scratch configuration, every test that drives an event loop must
hold a `config_turn()`.

It reported a clean sweep over a tree with three unguarded readers in it. The
rule keyed on `testing::desktop()`, and `gui/desktop`'s login tests reach the
loop through `ShellSession::start_with_stores` instead, so the only handle the
gate had never fired. **One entry in a table is not a rule, it is one example
of one** -- and the tests it missed were mine, written the same morning, which
is how the miss survived review.

Three changes, each earned by a failure of the version before it:

* `HARNESS_CALLS` gained `ShellSession::start`.
* Reader-ness and guard-ness now propagate along calls *within a file*. A
  helper that wraps the harness hid the whole rule: the test names no harness
  and the helper is not a test, so neither body looked wrong alone. Guard-ness
  propagates in the opposite direction, because a caller of a helper that
  *takes* the turn inherits it through the returned value.
* The rule applies to test code only. Widening to a production API cost the
  gate its scope immediately, and its first run reported the shell's own
  `fn main` as an unguarded test. A gate that tells you to put a test-only
  mutex in `main` has stopped describing the bug.

Sabotaged before being trusted: removing the guard from *one* helper flags that
helper and the six callers that inherit it. The old version flagged none of
them.

**Still open:** the gate follows calls within a file, so a helper defined in
one module and used in another is invisible to it. A cross-file call graph is
the honest fix and is not built. A guard inherited from a helper also assumes
the caller keeps the returned turn alive -- binding it to `_` rather than
`_name` drops it at once, and nothing here can see the difference.
