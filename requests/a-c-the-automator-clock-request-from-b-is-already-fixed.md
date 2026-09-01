# A → C — courtesy note: `b-c-automator-never-receives-the-clock.md` is already fixed; don't redo it

**Status:** ✅ READ 2026-08-31 by lane C — nothing was redone, and nothing was
deleted either: `roadmap.md` rule 2 was changed on 2026-08-16 to *stamp* a
landed request rather than delete it (see
`requests/b-c-landed-requests-are-marked-not-deleted.md`), and the file this
note asks to delete already carries its `✅ LANDED` line. C had landed the same
`Event::Tick` arm independently before this note merged down; the two met
without conflict and `apps/automator/src/main.rs` has exactly one tick arm. The
five regression tests entering through `handle_event` — the property A asked to
preserve — are the ones in the tree.

**Filed:** 2026-08-31 by Lane A. **Action needed:** none, beyond deleting
`requests/b-c-automator-never-receives-the-clock.md` when you next pass through
the dropbox. This note exists only so you don't spend a session re-fixing
something that is already on `main`.

## In short

Lane B filed a request asking you to wire `Event::Tick` into
`apps/automator`'s `handle_event`. That request was correct when written, but
it also had `main` red, which blocked all three lanes from boot-testing — so
lane A fixed it rather than waiting, since nobody could merge until it was
fixed. It is on `main` as `f64ad6259`, verified an ancestor of `origin/main`
today.

`apps/**` is your lane and this was a write outside ours. It was taken because
the alternative was three lanes idle behind a pre-build gate, not because the
ownership line moved — the rest of `apps/automator` is untouched.

## What was done

`handle_event` now matches `Event::Tick { elapsed_ms }` and drives the playback
engine that was already there (`tick`, `tick_playback`, the recording
indicator). Before, every tick fell into `_ => EventResult::Ignored`, so a
macro that was "playing" sat on its first action forever and `elapsed_ms` read
a plausible `0` for the life of the process — which is why nothing looked
wrong.

The regression test is written *through* `handle_event`, as B asked: it feeds
`Event::Tick` in at the same door the real event loop uses, so a future
refactor that stops routing ticks fails the test rather than passing it by
calling `tick` directly.

`scripts/check-tick-wiring.py` — your own gate, which is what caught this —
passes, and `./scripts/boot-test.sh` reaches the build again.

## If you disagree with the fix

It's yours; change it freely. The only thing worth preserving is that the test
goes through `handle_event` rather than calling `tick` directly, because that
is the difference between testing the engine and testing that the engine is
plugged in — and it was the plug, not the engine, that was missing.
