## RESOLVED 2026-08-21 — the duplicate is gone, and one feature moved rather than died

✅ **The deletion is done and this entry is closed.** All five prerequisites had
been met, so the shell's copy came out exactly as scoped: the 92-line
`render_window_decorations`, `window_chrome`, `WindowChrome`, the
`TITLE_BAR_HEIGHT`/`WINDOW_BUTTON_*` constant block, `top_corner_radii`, the
four `Hit::Window{Close,Maximize,Minimize,TitleBar}` variants with their
`handle_mouse` arms, the `main.rs` demo caller, and six `pointer_tests.rs` tests
that asserted the duplicate. `ManagedWindow::frame_rect` stays, as specified.
`Hit` is now `WindowContent(id)` for a window and nothing finer, because
everything finer belonged to the compositor and was consumed before this shell
heard about the press. Two processes now draw one title bar. Net −467/+425 lines
across five files.

**Two things the deletion would have silently taken with it were caught by
reading the duplicate line by line first, and were moved rather than dropped.**
This is the third time in this entry that reading beat trusting the entry's own
summary, and it is why the deletion was done last rather than first:

1. **Double-click-to-maximize was only ever implemented in the shell.** The
   `Hit::WindowTitleBar` arm was its sole home, and the compositor never
   produces `MouseEventKind::DoubleClick` at all — its own comment says so:
   `Enter`/`Leave` and `DoubleClick` "exist only on the client side and are
   never produced here". So deleting the arm would have removed the gesture
   from the product with nothing to notice. It now lives in
   `Compositor::handle_mouse_button`, beside the hit test that already resolves
   a press on the same strip. See `design-decisions.md` §502 for the three
   sub-decisions it needed (keying on the window, breaking the pair on an
   intervening press, and not re-arming after a completed double-click).
2. **The shell's decorator test was the only WCAG contrast check in the tree.**
   `accented_title_bars_still_mark_only_the_focused_window` measured real
   contrast *ratios* (≥ 4.5 focused, ≥ 3.0 unfocused) across 14 accents × 2
   modes. The surviving `gui/appearance` tests asserted only the *identity*
   `title_focused_fg == readable_on(accent)`, which a drifted luma threshold
   satisfies perfectly while returning the wrong extreme. The ratio assertions
   moved to `gui/appearance` as
   `a_title_is_readable_on_every_bar_the_settings_can_produce`, and the
   distinction is not theoretical: drifting `readable_on`'s threshold from 140
   to 200 leaves `readable_on_answers_with_the_palettes_own_extremes` passing
   and produces a **1.86:1** blue title bar, which the new test catches.

**One survivor was renamed, because the deletion revealed it was misnamed.**
`DesktopTheme::window_border_color` is still read — by the start menu and the
power menu — but the shell draws no window borders any more, so it is now
`panel_border_color`. It is still sourced from `DecorationColors::border_focused`
on purpose: a panel outlined in a different shade from the window beside it
looks like a bug, and would be one, because two processes had each picked a
colour. `window_fields_from` shrank from six fields to two and is now
`frame_fields_from`. The scaffold test named in the notes above
(`the_shells_window_colours_are_the_compositors_window_colours`) was deleted
with the decorator as instructed, replaced by
`the_shell_reads_its_frame_colours_rather_than_choosing_them`, which guards the
two fields that remain.

**Eleven reintroductions, eleven caught — but one only after the test was
repaired**, and that failure is the more useful half of the exercise.
`a_double_click_is_the_same_event_to_this_shell_as_a_single_one` originally
asserted that a double-click returns `Pass` and does not maximize. Both remain
true of a `DoubleClick` arm that silently returns `Pass` without dispatching
anything — so the test would have watched the shell drop click-to-focus and
every menu it opens on a press, and said nothing. It now asserts the *positive*:
that a double-click focuses a background window and opens the start menu, on
both kinds of surface the shell hit-tests. The general lesson, which has now bitten
three times in this entry's history: **a test that asserts the absence of the
behaviour you just deleted is satisfied by deleting too much.** Assert what
survives, not what went.
