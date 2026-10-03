## BUG-C-MINIMIZING-A-WINDOW-MADE-IT-UNREACHABLE (lane C, 2026-08-25) — ✅ **FIXED same day**, `1b773bb71`

**In short:** minimising a window in the shell deleted its taskbar button *and*
its Alt+Tab row at the same instant, so there was no way left to get the window
back. It had not gone anywhere — the compositor still had it — but nothing the
shell draws would show it to you again.

**Found by** surveying `gui/desktop/src/pointer_tests.rs` for the reintroduction
campaign, not by a failing test. Two tests covered this behaviour and both had
the bug written down as their expectation.

**Where it lived.** `gui/desktop/src/lib.rs`, three lines in three places:

```rust
// apply_window_list
visible: info.visible && !info.minimized,
// visible_windows
.filter(|w| w.visible && w.desktop == self.current_desktop)
```

`visible_windows()` was then the set used by `taskbar_button_rect`,
`taskbar_button_width`, the `hit_test` button loop, `render_taskbar`,
`start_alt_tab` / `next_alt_tab` / `prev_alt_tab` / `finish_alt_tab`,
`render_alt_tab` and the overview — **every list the shell draws whose entire
purpose is to get a put-away window back**.

**Root cause: one accessor answering two questions** — the same shape as the
`COVERED_DIRS` bug in `reintro-palette.py` recorded above, three days apart and
in unrelated code, which is why it is worth naming as a shape rather than as an
incident. The compositor reports *unmapped* ("the program took its window
away") and *minimised* ("the user put it away") as two separate flags because
they are two different facts. `visible` ANDed them into one and then served
both questions from the result. The field's own doc comment said
`/// Whether the window is visible (not minimized to taskbar)` — the
parenthetical is the author correctly describing a taskbar button that the code
underneath it deletes.

**The `Activate`-not-`Restore` care was unreachable code for as long as this
lasted.** `handle_press` deliberately sends `ShellControlAction::Activate` for
an unfocused window, and `design-decisions` records why: `Activate` un-minimises
*and* focuses in one step, whereas `Restore` would also un-maximise, silently
dropping a state the user never asked to leave. All of that reasoning is about
clicking the taskbar button of a minimised window — which no user could ever do.

**The fix is two accessors, not one widened one**, because the narrow question
does have exactly one legitimate caller:

| | means | asked by |
|---|---|---|
| `ManagedWindow::mapped` | the compositor's flag alone; a minimised window **is** mapped | taskbar, Alt+Tab, overview |
| `ManagedWindow::on_glass()` | `mapped && state != Minimized` — is it *drawn*? | Show Desktop, and nothing else |

`visible_windows()` was renamed `taskbar_windows()` for the set it actually
returns. "Show Desktop" (Super+D) keeps the narrow question: asking an
already-minimised window to minimise is a request the compositor must ignore
and one the user would have to undo twice.

### Why no test caught it: both of them asserted the bug

This is the part worth carrying forward. The behaviour was covered *twice*, and
both tests pinned the defect as the expectation:

- `a_minimized_window_keeps_its_button_and_an_unmapped_one_does_not` asserted
  `listed == [WindowId(3)]` — i.e. that the minimised window had **lost** its
  button. **The body asserts the exact opposite of the name.** Its own leading
  comment even states the rule correctly ("a minimised window is one the user
  put away and can click to get back, so it must keep its button") and then the
  assertion three lines later contradicts it.
- `the_window_list_is_the_only_thing_that_grows_the_shells_idea_of_the_desktop`
  ended `assert!(shell.taskbar_windows().is_empty())` after a minimise, for the
  same reason.

**Lesson 30: a test whose name promises a behaviour and whose body pins the
opposite is worse than no test at all.** An absent test leaves a hole that a
coverage sweep reports. A misnamed one fills the hole with a green result, and
anyone who greps for the promise — which is exactly what a careful reader does
before changing that code — finds it and stops looking. Both of these would have
been reported as `unproved` by `--coverage` had they not existed at all; instead
they read as two independent confirmations.

The corollary for the reintroduction campaign: **`--coverage` counts names, and
a name is not a claim.** A test can be "proved" by a defect and still be
asserting the wrong thing, if the defect was written to match the test rather
than the intent. This is the one failure mode the harness cannot detect on its
own, and the only defence is reading the assertion against the *doc comment of
the code under test* rather than against the test's own name.

**A third test was vacuous in the same area** and is left as it is, noted here:
`a_click_off_the_calendar_closes_it_without_acting` says "on the taskbar button
of the minimised window" and calls `taskbar_button_rect(0)` — which, with the
only window minimised, was the rectangle of a button that was not drawn. It
passed either way because the calendar-dismiss rule consumes the click whatever
is underneath. It is now describing something real.

**Tests added:** `a_minimized_window_can_be_got_back_from_its_taskbar_button`
(the click asks `Activate`), `alt_tab_reaches_a_minimized_window`, and
`show_desktop_does_not_ask_an_already_minimized_window_to_minimize` (the
`on_glass` exemption, so a future simplification back to one accessor fails).
