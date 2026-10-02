## C-LAUNCHER-HAD-NO-EVENT-LOOP-NO-MOUSE-AND-A-FRECENCY-CLOCK-AT-THE-EPOCH (lane C, 2026-08-26) — ✅ FIXED 2026-08-26

**In short:** the app launcher — the Spotlight-style box you type a program's
name into — never opened a window, so none of it ran. It also had no mouse
support at all: you could not click a result, only arrow to it. And its ranking
was broken in a way that would have looked plausible forever: the launcher sorts
by "frecency" (how often *and* how recently you have launched something), and its
clock was stuck at 1 January 1970. Every launch was recorded with that timestamp,
so every program you had ever run counted as "launched seconds ago", permanently
— the recency half of the ranking did nothing. All three are fixed, and clicking
a program that will not start now says so instead of silently closing the dialog.

### What changed

1. **The window.** `apps/launcher` gained `oswindow` and an
   `impl oswindow::app::App for LauncherState`. `fn main` used to build a
   launcher, call `show()`, render one frame into a `let _`, and return.
   `handle_key` was correct and had 20 tests; nothing routed a key to it.

2. **A `handle_event`, which did not exist.** Keys go to the existing
   `handle_key`; `Resize` sets the viewport; `Tick` reads the clock; and mouse
   events go to a new `handle_mouse`.

3. **The mouse, which did not exist either.** Click a row to launch it, hover a
   row to select it, click outside the dialog to dismiss it — and click *inside*
   the dialog's own chrome (the search box, the padding) and nothing happens,
   which is the case a naive "not on a row means outside" hit-test gets wrong.

4. **`struct Layout`, shared by the renderer and the hit-test.** The row
   positions are measured once and used by both. Two copies of that arithmetic
   is how a launcher comes to open the program *above* the one you clicked, and
   it is a bug that survives review because both copies look right.

5. **The frecency clock.** `set_now` (pure) gained `refresh_now` (reads
   `SystemTime::now`), called from the `Event::Tick` arm and once before the
   first frame. Deliberately *not* routed through `tzrules`: `now_secs` is only
   ever used in differences against stored timestamps, so it is an instant and
   not a time of day, and dressing it as a wall-clock reading would invite the
   next reader to display it.

6. **A visible failure path.** `LauncherAction::Launch(path)` now actually spawns
   the program. On success the window exits; on failure the dialog *stays up*
   with a red banner naming the path and the reason. Exiting on failure would be
   the worst of both — the program did not start, and the window that could say
   so is gone.

### How the tests were wrong before they were right

The first version of the hit-test tests measured each row with `Layout` and then
hit-tested it with `Layout`, so they agreed no matter how wrong `Layout` was. The
second version read the row positions out of the **render output** — the
dialog's own `PushTranslate`, then the 24×24 category icon every row draws — but
still only checked row *centres*, and a deliberately introduced 8-pixel error in
`row_at` passed all 51 tests. Rows are 44 pixels tall, so a centre-only assertion
proves the hit-test correct to ±22 pixels, which is to say it proves nothing
about the boundary that actually decides which row you clicked.

The version that is committed reads each row's drawn highlight (the one
`FillRect` a row's height tall, which only the *selected* row emits) and asserts
the first pixel, the last pixel, and the pixel above the top. Both mutations —
the 8-pixel offset, and a `Layout` that ignores the error banner's height — now
fail. **A geometry test that only checks centres is a geometry test that does not
check geometry.**

### Cost

- 34 → 51 tests in the crate; new dep `oswindow`.
- `scripts/check-window-wiring.py` `BASELINE` 127 → 126.
- Five pre-existing `arithmetic_side_effects` warnings in `handle_key`,
  `update_results` and the renderer fixed while here (`saturating_*`), and the
  test module gained the defensive-lint waiver every other crate in this tree
  carries.

### What is still owed

- **The app database is a hardcoded `builtin_app_database()`.** The launcher
  offers a fixed list rather than reading `.desktop`-equivalent entries from the
  package store, so a newly installed program is not launchable from it. That
  needs `pkg`'s installed-program index, which exists; wiring it is its own task.
- **The launch history is not persisted.** Frecency is now computed against a
  real clock, and then thrown away when the process exits — which is every time
  you launch something. Until the history is written somewhere, the recency half
  still contributes nothing *across* sessions. This is the more interesting half
  of the frecency fix and is deliberately a separate change.
- **`spawn_program` uses `std::process::Command`**, matching what
  `apps/installer` already does. When SlateOS has a session/service manager that
  owns process launch, this is the one call site to move.

### The lesson

**A frozen clock does not look like a bug; it looks like a preference.** The lock
screen's clock froze at 12:00 and was obvious the moment anyone looked. This one
froze at the epoch and produced *plausible* output — a launcher that ranks by
launch count, which is a perfectly reasonable-looking launcher. Nothing about the
result says "the clock is broken". That is the third instance of the same defect
in this lane in two days (lock screen, notification daemon, launcher), and all
three had the same fingerprint: **a public, documented, well-tested `set_*` for a
clock, with no production caller.** Grepping for callers of a setter is now the
first thing to do on any struct that holds a notion of "now".
