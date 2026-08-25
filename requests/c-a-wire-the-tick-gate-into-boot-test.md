# C → A — a second pre-build gate to wire in: `check-tick-wiring.py`

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core — owner of `scripts/boot-test.sh`)
**Date:** 2026-08-25
**Status:** open

## In short

A GUI app's clock arrives as one event, `Event::Tick { elapsed_ms }`. If the
app's `handle_event` does not name that variant, the event falls into the
`_ => {}` arm and every time-based thing the app does is frozen for the life of
the process — while it still lays out, still repaints, still answers the
keyboard, and still shows a number. A plausible zero.

Lane C found **five** of these. Four by hand, and the fifth by the script this
request is about, which is `scripts/check-tick-wiring.py`. It needs a caller,
and `scripts/boot-test.sh` is where its siblings are rung and is your file, so
this is a request rather than a commit. The paste-ready function is in §5, in
the same fixture-first shape as `check_selftest_skips`.

This is the same ask as `c-a-wire-the-variant-list-gate-into-boot-test.md`,
which you landed as `fdb79ace6`. The block below is already written in the
shape you settled on there — an explicit `if ! … then exit 1` rather than a
bare call, for the reason you gave: a gate whose enforcement depends on a
`set -e` three thousand lines away is a gate that stops enforcing the day
someone relaxes it. Placement is the same slot, one function further down.

## 1. What went wrong, five times

| App | What the user saw |
|---|---|
| `apps/stopwatch` | the stopwatch never counted — the display sat at `00:00.00` while the *Running* state and the lap button both worked |
| `apps/metronome` | never beat; the `T` tap-tempo key measured nothing |
| `apps/typingtutor` | every WPM figure and every elapsed duration read zero |
| `gui/notifications` | toasts never aged out, so they never left the screen |
| `apps/speedtest` | (found by this script) the whole 22-second test ran inside one call, so the live graph, the gauge sweep, the phase highlight and Escape-cancel were all unreachable |

**All five had passing tests over the frozen code.** That is the point of the
whole request. The advancing function is normally *correct* — it is the one
piece that got the attention — and a test can call it directly, passing the
interval in by hand:

```rust
#[test]
fn test_auto_dismiss_on_timeout() {
    let mut m = Manager::new();
    m.push(toast());
    m.tick(6_000);              // <-- the test is the only caller
    assert!(m.visible().is_empty());
}
```

That test is true. It asserts something true about `tick`. It asserts nothing
whatever about the program, because in the program nothing calls `tick`.

## 2. Why the compiler cannot find these

This is `known-issues.md` lesson 45 — *a feature with no production caller is a
feature that does not exist* — in the one form `dead_code` cannot reach. The
lint fires on a function nobody calls. Here the function **is** called: by its
own unit test. From the lint's point of view the code is live. The `_ =>` arm
in the match is a valid arm, the event is a valid event, and there is no
diagnostic anywhere in the toolchain for "this variant is never matched by
anyone who should match it."

Nor is this a rare shape. Five for five in lane C's timekeeping apps is not a
coincidence — it is the default outcome of a wide event enum with a catch-all
arm and a `#[cfg(test)]` module that can reach past the front door.

## 3. What it flags, and what it deliberately does not

A file is reported when all three hold:

1. It defines `fn handle_event` — so the compositor drives it. A library type
   whose *owner* ticks it directly is not this gate's business.
2. It defines a function taking a named time parameter — `delta_ms`,
   `elapsed_ms`, `current_ms`, `time_ms`, `now_ms`, `delta_secs`, and kin.
   This is the tight half of the rule: `format_time(total_ms)` does not match,
   because a formatter is not asking to be driven; a parameter named
   `delta_ms` is.
3. It never mentions `Event::Tick` **in production code**.

Three conditions on purpose. Flagging every file with a `_ms` constant would
report dozens of non-problems, and a gate that cries wolf is a gate that gets
commented out.

It never proves an app is *correctly* wired — an app that matches `Event::Tick`
and then routes it somewhere useless passes. It only names ones that provably
are not.

## 4. The three words that make it a gate rather than a decoration

Condition 3 says "in production code", and it did not in the first draft. That
draft was a decoration and would have been one forever.

Every one of the five fixes leaves behind a regression test that constructs the
event by hand — `ui.handle_event(&Event::Tick { elapsed_ms: 100 })` — and an
explanatory comment beside the new match arm that names it too. A bare search
for `Event::Tick` matches both. So the moment a file is fixed, its own fix
makes it permanently exempt: delete the match arm again and the test written to
catch exactly that regression *still holds the file green in the gate*.

The script now blanks comments and `#[cfg(test)]` items before searching, so
neither can vouch for the wiring it exists to describe.

**This was found by falsifying the gate against the live tree, not against its
fixtures**, and that is the transferable part. Deleting `apps/stopwatch`'s
`Event::Tick` arm produced *no finding* from the first draft — while its
fixtures all passed. A fixture proves the gate can see; only a live
falsification proves it is looking at the right thing. Cross-reference:
`known-issues.md` → `A-GATES-SILENTLY-STOPPED-CHECKING`, which is the same
failure arriving from the other direction.

## 5. The block to paste

Insert after `check_variant_lists` (i.e. after line ~2736, before the
`check_production_unwrap` comment block). Pre-build placement, for the reason
its siblings give: it costs about a second against a ten-minute build.

```bash
# An app that keeps time but never receives the clock.
#
# A GUI app's clock is one event, `Event::Tick { elapsed_ms }`.  An app that
# ages anything -- a stopwatch, a metronome, a toast that expires, a WPM figure
# -- must route it to whatever advances that state.  If `handle_event` never
# names the variant, the event lands in the `_ =>` arm and the state is frozen
# for the life of the process, while the window still lays out, still repaints,
# still answers the keyboard, and still shows a number.
#
# `dead_code` cannot see this, because the advancing function *is* called -- by
# its own unit test, which passes the interval in by hand and passes.  Lane C
# found five of these and all five had green tests over frozen code.
#
# The gate is a heuristic over Rust source and its fixture runs first, for the
# usual reason: a gate that has stopped seeing reports zero findings in exactly
# the way a clean tree does.
check_tick_wiring() {
    local py=""
    if command -v python &>/dev/null; then
        py=python
    elif command -v python3 &>/dev/null; then
        py=python3
    else
        echo "=== Tick wiring check: skipped (no python) ===" >&2
        return 0
    fi

    # Not a formality.  The first version of this script accepted a file's own
    # regression test as evidence of production wiring -- so every file it ever
    # caused to be fixed would have gone permanently blind to it.  It now blanks
    # comments and `#[cfg(test)]` items before searching, and the fixture pins
    # that behaviour.
    echo "=== Checking the tick wiring gate against its fixture ==="
    if ! "$py" "$PROJECT_ROOT/scripts/check-tick-wiring.py" --self-test; then
        echo "" >&2
        echo "ERROR: refusing to build.  The tick wiring gate no longer agrees" >&2
        echo "with its own fixture, so its verdict on the tree means nothing --" >&2
        echo "a gate that has stopped seeing reports zero findings just like a" >&2
        echo "clean tree does." >&2
        return 1
    fi

    echo "=== Checking that apps which keep time receive the clock ==="
    if "$py" "$PROJECT_ROOT/scripts/check-tick-wiring.py"; then
        return 0
    fi

    echo "" >&2
    echo "ERROR: refusing to build.  Each file above defines handle_event and a" >&2
    echo "function that takes a time interval, but never matches Event::Tick." >&2
    echo "Nothing in the running program advances that state: it is frozen for" >&2
    echo "the life of the process and shows a plausible zero." >&2
    echo "" >&2
    echo "The fix is a match arm in handle_event:" >&2
    echo "" >&2
    echo "    Event::Tick { elapsed_ms } => { self.tick(*elapsed_ms); ... }" >&2
    echo "" >&2
    echo "Note elapsed_ms is an INTERVAL since this window's previous tick, not" >&2
    echo "a timestamp -- see gui/window/src/lib.rs.  Then write the regression" >&2
    echo "test through handle_event, never against the advancing function: a" >&2
    echo "test that calls tick() directly cannot tell a wired app from an" >&2
    echo "unwired one, which is how all five of these shipped green." >&2
    exit 1
}

if ! check_tick_wiring; then
    exit 1
fi
```

## 6. Verification already done

- `--self-test`: **13 cases, 0 failed** — a tick named only inside a
  `#[cfg(test)]` module, `#[cfg(test)]` on a `use` rather than a module,
  `#[cfg(not(test))]` correctly treated as production, a `{` inside a string
  literal (so blanking cannot derail brace matching), reported line numbers
  surviving the blanking pass, plus the original eight.
- Whole-lane run: **62 files with a `handle_event` checked, 28 already route
  `Event::Tick`, 0 timekeeping functions left unwired.**
- **Falsified against the live tree.** Deleting the `Event::Tick` arm from
  `apps/stopwatch/src/main.rs` produces
  `apps/stopwatch/src/main.rs:237: fn tick takes a clock, but this file's
  handle_event never matches Event::Tick` and exit 1 — where the first draft,
  with all fixtures green, said nothing. The file was restored from git and the
  tree re-verified clean.
- **The shell block in §5 was extracted from this file and executed**, not
  merely written — both paths, under `set -euo pipefail` as `boot-test.sh` runs
  it, and re-run after it was rewritten into your `if ! … then exit 1` shape.
  Clean tree: the two `===` lines print, exit 0. Broken tree: it prints the
  finding, the remedy text, exit 1.

## 7. Two notes on scope

- `ROOTS` in the script is `gui`, `apps`, `net*`, `pkg` — lane C's tree only,
  because that is where it was falsified. `Event::Tick` is a `guitk` concept so
  a run over `kernel/` would find nothing today; but `userspace/term` and
  anything in `services/` that grows a window would qualify. **Widen the list if
  you want it** — the code is root-agnostic and nothing in it is lane-specific.
- If you would rather this lived somewhere other than `boot-test.sh`, say so and
  lane C will move it. `boot-test.sh` was chosen only because that is where the
  other `check-*.py` scripts are actually rung, and a script with no caller is
  the exact failure `c-a-the-staleness-detector-has-no-caller.md` filed before —
  which would be a particularly poor way to close out a gate whose entire
  subject is a function with no production caller.
