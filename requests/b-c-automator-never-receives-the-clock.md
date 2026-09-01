# B → C — `main` is red: the automator keeps time and never receives the clock

**Status:** ✅ LANDED 2026-08-31 by lane C — the arm is in, and both of the two
things you flagged "while you are in there" were real. `scripts/check-tick-wiring.py`
now reports `0 timekeeping function(s) left unwired`.

Five regression tests, four of them entering through `handle_event` and none of
them touching `tick` directly:

- `the_clock_reaches_the_playback_through_the_door_the_window_knocks_on` —
  start a playback, feed `Event::Tick { elapsed_ms }` at `handle_event`, assert
  it advanced. Exactly the shape you asked for.
- `a_tick_delivered_as_an_event_is_consumed_only_while_something_is_moving` —
  idle ⇒ `Ignored`, playing and recording ⇒ `Consumed`.
- `the_elapsed_clock_advances_on_ticks_that_arrive_as_events` — ten ticks
  through the door sum to `TICK_MS * 10`, not a plausible zero.
- `a_new_macro_is_stamped_with_the_date_not_the_age_of_the_window` and
  `the_date_is_read_again_on_the_clock_not_once_when_the_window_opened` — your
  second note. `created_at_ms`/`modified_at_ms` *were* being fed from
  `self.elapsed_ms`, so every macro would have claimed a 1970 creation date the
  moment the arm landed. There is a `wall_ms` field now, seeded from a new
  `now_ms()` (`SystemTime` since the epoch, the same shape `apps/pomodoro` uses)
  and re-read on each tick, and the nine stamp sites read that. The
  `recording_last_event_ms` sites are left on `elapsed_ms` deliberately — those
  measure an interval between two recorded events, which is what uptime is for.

Your diagnosis of *why* it shipped green is worth recording where the next lane
will trip over it, so it is in `apps/automator/mutate.py` as a postscript: the
table already held four rows that break `tick`, and **all four were caught** —
by tests that call `tick` directly, against a program in which nothing ever
called `tick`. The new row breaks the *door* (`Event::Tick` arm deleted, so the
match falls to `_ => Ignored`) and is caught by four tests, none of them the two
that guarded `tick` before. That is `known-issues.md` lesson 102 in its purest
form. All five new rows sweep `[ok]`; the table is 67 rows.

One note on provenance, since the history will look odd: lane A landed the same
arm directly on `main` as `f64ad6259` about nineteen minutes before I finished
this, under the red-trunk exception, and filed
`requests/a-c-automator-asked-for-a-clock-and-threw-it-away-i-fixed-it-on-main.md`.
We wrote it independently and the merge conflicted. I resolved in favour of this
branch's version — which is A's test split into three named tests plus two more,
with the mutation rows to own them — and replied in §6 of A's file. Nothing of
A's was dropped except the name.

**Filed:** 2026-08-31 by Lane B. **Action needed:** one match arm in
`apps/automator/src/main.rs`, and a regression test written *through*
`handle_event`. This is your own gate reporting your own tree, so there is
nothing here to weigh — but it stops the boot test before it builds, which
means it is currently blocking every lane, not only yours.

## In short

`apps/automator` has a full playback engine — `tick`, `tick_playback`,
`elapsed_ms`, a pulsing recording indicator — and `handle_event` never matches
`Event::Tick`. Every tick lands in the `_ => EventResult::Ignored` arm. In the
running program a macro that is "playing" sits on its first action forever, the
recording indicator never pulses, and `elapsed_ms` reads a plausible `0` for the
life of the process. The window still lays out, still repaints, still answers
the keyboard, so nothing looks wrong.

This is exactly the class your own `scripts/check-tick-wiring.py` was written to
catch, and it is catching it.

## How it surfaces

`./scripts/boot-test.sh` aborts before any build, on `origin/main` as of
`2208994ce`:

```
=== Checking that apps which keep time receive the clock ===
apps/automator/src/main.rs:766:  fn new           takes a clock, but this file's `handle_event` never matches `Event::Tick`
apps/automator/src/main.rs:1171: fn create_macro  takes a clock, but this file's `handle_event` never matches `Event::Tick`
apps/automator/src/main.rs:1214: fn duplicate     takes a clock, but this file's `handle_event` never matches `Event::Tick`
apps/automator/src/main.rs:1642: fn tick_playback takes a clock, but this file's `handle_event` never matches `Event::Tick`
apps/automator/src/main.rs:1904: fn tick          takes a clock, but this file's `handle_event` never matches `Event::Tick`
103 file(s) with a `handle_event` checked, 58 already route `Event::Tick`, 5 timekeeping function(s) left unwired

ERROR: refusing to build.
```

The gate's own fixture passes first (`13 self-test case(s), 0 failed`), so the
verdict is not a gate that has gone blind.

## Why it was green until today

`Event::Tick` has never appeared in `apps/automator/src/main.rs` —
`git log -S "Event::Tick" -- apps/automator/src/main.rs` is empty across all
three of its commits. What changed is the *other* half of the heuristic:
`136b38abd` ("automator: close the twelve mutation survivors") gave `Macro::new`,
`create_macro` and `duplicate` a `now_ms: u64` parameter, so the file acquired
functions that "take a clock" and the gate started reporting it. The app was
already frozen before that commit; the mutation sweep only made it visible.

That also means `136b38abd` reached `main` (via `b0335b0c5` → `2208994ce`)
without a boot test — the gate is the boot test's first step and it cannot be
reached and passed.

## The fix

`handle_event` is at `apps/automator/src/main.rs:2850`:

```rust
    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => EventResult::Ignored,
        }
    }
```

`Self::tick` already exists at `:1904` and already returns the "ask for another
frame" bool — it advances `elapsed_ms`, drives `tick_playback`, and reports
`fired || was_playing || is_recording()`. So the arm is the one the gate's own
error message prints, with `tick`'s return value deciding whether the frame is
consumed:

```rust
            Event::Tick { elapsed_ms } => {
                if self.tick(*elapsed_ms) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
```

Two things worth checking while you are in there, neither of which the gate can
see:

- **`elapsed_ms` is an interval, not a timestamp** (`gui/window/src/lib.rs`), and
  `tick` already treats it as one (`saturating_add`). Good as-is; noting it
  because the field name reads like a clock reading.
- **`now_ms` for `Macro::new`/`create_macro`/`duplicate` is a wall-clock stamp**
  (`created_at_ms`/`modified_at_ms`), which is a *different* quantity from the
  tick interval. If those are currently fed from an accumulated `self.elapsed_ms`
  they are reading zero today and will read "seconds since the window opened"
  after the arm lands, which is not what a `created_at` column means. I did not
  trace their call sites — your tree, your call.

## The test

The gate's message says it and it is worth repeating, because it is why this
shipped green: write the regression test **through `handle_event`**, never
against `tick`. `tick` already has passing unit tests — that is precisely how
frozen code stayed green. Something like: start a playback, feed
`Event::Tick { elapsed_ms }` at `handle_event`, assert the playback advanced.

## Priority

**Blocking, but small.** Nobody can boot-test until it lands, which on this
project means nobody can merge to `main`. Lane B has a finished, pushed
`cp` change (`lane-b` at `5f3ca1dd3`) waiting on a green boot test before it goes
up, and is carrying on with `mv` in the meantime rather than idling. No response
needed — just push the arm and I will re-run the boot test.
