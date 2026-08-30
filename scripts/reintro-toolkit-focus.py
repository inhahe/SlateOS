#!/usr/bin/env python3
"""Prove the toolkit's focus, caret and selection tests are regression tests.

The fourteenth of these harnesses. It covers the rework recorded in
`design-decisions.md` §546 and in two `known-issues.md` entries:
`TD-C-TWO-TOOLKIT-TEXT-FIELDS-DRAW-NO-CARET-AT-ALL` and
`TD-C-EVERY-KEYSTROKE-WENT-TO-THE-LAST-TEXT-FIELD-IN-THE-WINDOW`.

The second of those is why this file exists. Before the rework the toolkit had
no focus at all: `Widget::handle_event` matched `Event::Key(key)` with no
guard, so a key event walked the tree and was consumed by whichever text field
happened to sit last in it. That shipped under a full suite of green text-input
tests -- green because every one of them built a tree with exactly *one* text
field, where "the focused field" and "the last field" are the same widget and
the bug is invisible. A suite that cannot tell those two apart is a suite that
would pass the bug again tomorrow.

So the same suspicion applies to everything the rework added. Each defect below
is a way the new machinery could be wrong while still reading fine:

- the key gate dropped, or the focus not cleared before it is moved, so more
  than one widget answers to a keystroke;
- Tab swallowed but moving nothing, or Shift+Tab moving forwards;
- a click not moving the focus, or moving it only when it lands on something,
  so a caret keeps blinking in a field the user has clicked away from;
- focus landing on a disabled or hidden control, or refusing to land on a
  button -- which puts a keyboard user somewhere they cannot act;
- the caret drawn in an unfocused field, which lies about where typing goes;
- the scroll frozen at zero or unclamped, so the caret leaves the box;
- an anchor equal to the caret counted as a selection, or a run of Shift+Left
  restarting the selection each press instead of extending it;
- a deleting key taking the selection *and* the character next to it;
- a password field's selection measured in bytes rather than marks, which
  re-creates in the highlight the encoded-length leak the masking exists to
  prevent.

None stops it compiling. None is visible in a screenshot of a one-field form.

Restore discipline as in the companions: a byte snapshot up front, written back
unconditionally in a `finally`, verified by SHA-256 -- not a reverse
search-and-replace, which silently leaves the tree modified if a patch
half-applied or the process died between the write and the undo.

Two modes:

- `--check` matches every pattern against the snapshot and builds nothing.
  Seconds, no toolchain, and it answers the question that rots on its own: has
  a rename or a rustfmt pass stopped a defect applying?
- No flag: the real sweep. Apply, run the tests, restore, report.

Filter either with defect letters: `reintro-toolkit-focus.py A B C`.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

WIDGET = "gui/toolkit/src/widget.rs"
EDIT = "gui/toolkit/src/textedit.rs"
MODAL = "gui/toolkit/src/modal.rs"

GUITK = ["guitk"]

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    # ---- where a keystroke goes -------------------------------------------
    (
        "A: a key event goes to every text field in the tree, not the focused one",
        WIDGET,
        [(
            "Event::Key(key) if self.focused => self.handle_key(key),",
            "Event::Key(key) => self.handle_key(key),",
        )],
        GUITK,
        [
            "typing_goes_to_the_field_that_was_clicked_and_not_the_last_one_in_the_tree",
            "a_window_nobody_has_clicked_in_swallows_typing_rather_than_guessing",
        ],
    ),
    (
        "B: moving the focus does not take it off the widget that had it",
        WIDGET,
        [(
            "        self.root.clear_focus();\n"
            "        id.is_some_and(|id| self.root.focus_by_id(id))",
            "        id.is_some_and(|id| self.root.focus_by_id(id))",
        )],
        GUITK,
        ["typing_goes_to_the_field_that_was_clicked_and_not_the_last_one_in_the_tree"],
    ),
    # ---- Tab ---------------------------------------------------------------
    (
        "C: Tab is swallowed but moves the focus nowhere",
        WIDGET,
        [(
            "                if key.modifiers.shift {\n"
            "                    self.focus_prev();\n"
            "                } else {\n"
            "                    self.focus_next();\n"
            "                }\n"
            "                return EventResult::Consumed;",
            "                return EventResult::Consumed;",
        )],
        GUITK,
        ["tab_visits_every_control_in_reading_order_and_wraps"],
    ),
    (
        "D: Shift+Tab walks forwards like a bare Tab",
        WIDGET,
        [(
            "                if key.modifiers.shift {\n"
            "                    self.focus_prev();\n"
            "                } else {\n"
            "                    self.focus_next();\n"
            "                }",
            "                self.focus_next();",
        )],
        GUITK,
        ["tab_visits_every_control_in_reading_order_and_wraps"],
    ),
    (
        "E: Tab into a window with nothing focused starts past the first control",
        WIDGET,
        [(
            "            None if forward => 0,",
            "            None if forward => 1,",
        )],
        GUITK,
        ["tab_visits_every_control_in_reading_order_and_wraps"],
    ),
    # ---- clicking ----------------------------------------------------------
    (
        "F: a click does not move the focus at all",
        WIDGET,
        [(
            "                self.focus(self.root.focus_target_at(mouse.x, mouse.y));",
            "                let _ = mouse.x;",
        )],
        GUITK,
        [
            "typing_goes_to_the_field_that_was_clicked_and_not_the_last_one_in_the_tree",
            "clicking_away_from_every_control_takes_the_focus_with_it",
        ],
    ),
    (
        "G: a click on nothing focusable leaves the old focus standing",
        WIDGET,
        [(
            "                self.focus(self.root.focus_target_at(mouse.x, mouse.y));",
            "                if let Some(hit) = self.root.focus_target_at(mouse.x, mouse.y) {\n"
            "                    self.focus(Some(hit));\n"
            "                }",
        )],
        GUITK,
        ["clicking_away_from_every_control_takes_the_focus_with_it"],
    ),
    # ---- what is focusable -------------------------------------------------
    (
        "H: focus lands on controls that are disabled or hidden",
        WIDGET,
        [(
            "        self.enabled\n"
            "            && self.visible\n"
            "            && matches!(\n"
            "                self.kind,",
            "        matches!(\n"
            "                self.kind,",
        )],
        GUITK,
        ["focusing_by_id_refuses_what_tab_would_have_skipped_and_still_lets_go"],
    ),
    (
        "I: only text fields take focus, so Tab skips every button and checkbox",
        WIDGET,
        [(
            "                WidgetKind::TextInput { .. }\n"
            "                    | WidgetKind::Button { .. }\n"
            "                    | WidgetKind::Checkbox { .. }",
            "                WidgetKind::TextInput { .. }",
        )],
        GUITK,
        [
            "tab_visits_every_control_in_reading_order_and_wraps",
            "the_keyboard_can_work_a_control_it_has_tabbed_to",
        ],
    ),
    (
        "J: focus can be given by id to a widget that does not take focus",
        WIDGET,
        [(
            "        if self.id == id && self.accepts_focus() {",
            "        if self.id == id {",
        )],
        GUITK,
        ["focusing_by_id_refuses_what_tab_would_have_skipped_and_still_lets_go"],
    ),
    (
        "K: a focused button cannot be worked from the keyboard",
        WIDGET,
        [(
            "            WidgetKind::Button { pressed, .. } => match key.key {\n"
            "                crate::event::Key::Space | crate::event::Key::Enter => {\n"
            "                    *pressed = !*pressed;\n"
            "                    EventResult::Consumed\n"
            "                }\n"
            "                _ => EventResult::Ignored,\n"
            "            },",
            "            WidgetKind::Button { .. } => EventResult::Ignored,",
        )],
        GUITK,
        ["the_keyboard_can_work_a_control_it_has_tabbed_to"],
    ),
    # ---- the caret ---------------------------------------------------------
    (
        "L: the caret is drawn in an unfocused field too",
        EDIT,
        [(
            "    if f.focused {\n"
            "        push_caret(tree, origin + caret_px, f.y, f.line_height, f.color);\n"
            "    }",
            "    push_caret(tree, origin + caret_px, f.y, f.line_height, f.color);",
        )],
        GUITK,
        [
            "a_focused_field_draws_a_caret_and_an_unfocused_one_does_not",
            "the_field_draws_a_caret_when_it_has_the_focus_and_not_when_it_does_not",
        ],
    ),
    (
        "M: the caret is drawn at the field's left edge whatever the text says",
        EDIT,
        [(
            "        push_caret(tree, origin + caret_px, f.y, f.line_height, f.color);",
            "        push_caret(tree, f.x, f.y, f.line_height, f.color);",
        )],
        GUITK,
        ["the_drawn_caret_follows_the_arrow_keys"],
    ),
    # ---- scrolling ---------------------------------------------------------
    (
        "N: nothing ever scrolls, so a long field hides its own caret",
        EDIT,
        [(
            "    if text_width <= avail {\n"
            "        return 0.0;\n"
            "    }\n"
            "    (caret_px - avail + CARET_EDGE_MARGIN).clamp(0.0, text_width - avail)",
            "    let _ = (text_width, avail, caret_px, CARET_EDGE_MARGIN);\n"
            "    0.0",
        )],
        GUITK,
        [
            "a_string_longer_than_its_box_scrolls_to_keep_the_caret_in_view",
            "an_input_dialog_longer_than_its_box_scrolls_to_keep_the_caret_in_view",
        ],
    ),
    (
        "O: the scroll is not clamped, so the text runs off past its own end",
        EDIT,
        [(
            "    (caret_px - avail + CARET_EDGE_MARGIN).clamp(0.0, text_width - avail)",
            "    (caret_px - avail + CARET_EDGE_MARGIN).max(0.0)",
        )],
        GUITK,
        ["the_scroll_never_runs_past_either_end_of_the_text"],
    ),
    (
        "P: a field that fits still scrolls, so short text drifts left",
        EDIT,
        [(
            "    if text_width <= avail {\n        return 0.0;\n    }\n",
            "",
        )],
        GUITK,
        ["nothing_scrolls_while_the_text_fits"],
    ),
    (
        "Q: the field does not clip, so a scrolled string smears over the form",
        EDIT,
        [
            (
                "    tree.clip(f.x, f.y, f.width, f.line_height);\n    let origin",
                "    let origin",
            ),
            (
                "    }\n    tree.unclip();\n}",
                "    }\n}",
            ),
        ],
        GUITK,
        ["a_scrolled_field_clips_what_it_pushes_outside_itself"],
    ),
    # ---- what counts as a selection ---------------------------------------
    (
        "R: an anchor sitting on the caret counts as a selection",
        EDIT,
        [(
            "    (range.0 < range.1).then_some(range)",
            "    Some(range)",
        )],
        GUITK,
        ["an_anchor_sitting_on_the_caret_is_not_a_selection"],
    ),
    (
        "S: a run of Shift+Left restarts the selection instead of extending it",
        EDIT,
        [(
            "        anchor.get_or_insert(cursor.byte());",
            "        *anchor = Some(cursor.byte());",
        )],
        GUITK,
        [
            "shift_and_an_arrow_select_and_a_bare_arrow_gives_the_selection_up",
            "shift_and_an_arrow_select_in_the_input_dialog_and_a_bare_arrow_gives_it_up",
        ],
    ),
    (
        "T: a bare arrow keeps the selection it should have given up",
        EDIT,
        [(
            "    if shift {\n"
            "        anchor.get_or_insert(cursor.byte());\n"
            "    } else {\n"
            "        *anchor = None;\n"
            "    }",
            "    if shift {\n"
            "        anchor.get_or_insert(cursor.byte());\n"
            "    }",
        )],
        GUITK,
        [
            "shift_and_an_arrow_select_and_a_bare_arrow_gives_the_selection_up",
            "shift_and_an_arrow_select_in_the_input_dialog_and_a_bare_arrow_gives_it_up",
        ],
    ),
    (
        "U: nothing is painted behind the selected text",
        EDIT,
        [(
            "    let range = selected_range(f.cursor, f.selection_anchor);\n"
            "    if let Some((from, to)) = range {",
            "    let range = selected_range(f.cursor, f.selection_anchor);\n"
            "    if let Some((from, to)) = None::<(usize, usize)> {",
        )],
        GUITK,
        [
            "a_selection_is_painted_and_its_text_is_drawn_over_the_paint",
            "an_input_dialogs_selection_is_painted_behind_the_text",
        ],
    ),
    # ---- cutting a selection ----------------------------------------------
    (
        "V: a cut accepts an offset that is not a character boundary",
        EDIT,
        [(
            "    if from >= to || !value.is_char_boundary(from) || !value.is_char_boundary(to) {\n"
            "        return false;\n"
            "    }",
            "    if from >= to {\n"
            "        return false;\n"
            "    }",
        )],
        GUITK,
        ["a_cut_refuses_an_offset_that_is_not_a_character_boundary"],
    ),
    (
        "W: a cut leaves the caret where it was, not where the text was taken from",
        EDIT,
        [(
            "    value.drain(from..to);\n    *cursor = TextCursor::from(from);",
            "    value.drain(from..to);\n    let _ = cursor;",
        )],
        GUITK,
        ["cutting_a_selection_leaves_the_caret_where_the_text_was_taken_from"],
    ),
    (
        "X: a cut that cut nothing reports that it cut something",
        EDIT,
        [(
            "    let Some(start) = anchor.take() else {\n        return false;\n    };",
            "    let Some(start) = anchor.take() else {\n        return true;\n    };",
        )],
        GUITK,
        ["cutting_nothing_reports_that_it_cut_nothing"],
    ),
    # ---- editing over a selection, in the widget ---------------------------
    (
        "Y: typing beside the selection instead of over it (text field)",
        WIDGET,
        [(
            "                    crate::textedit::delete_selection(value, cursor, selection_anchor);\n"
            "                    value.insert(cursor.byte(), ch);",
            "                    value.insert(cursor.byte(), ch);",
        )],
        GUITK,
        ["typing_over_a_selection_replaces_it"],
    ),
    (
        "Z: Backspace takes the selection and the character before it (text field)",
        WIDGET,
        [(
            "                        if !crate::textedit::delete_selection(value, cursor, selection_anchor) {\n"
            "                            if let Some(prev) = cursor.prev_in(value) {",
            "                        {\n"
            "                            crate::textedit::delete_selection(value, cursor, selection_anchor);\n"
            "                            if let Some(prev) = cursor.prev_in(value) {",
        )],
        GUITK,
        ["backspace_over_a_selection_takes_the_selection_and_nothing_more"],
    ),
    (
        "AA: Left never plants a selection anchor (text field)",
        WIDGET,
        [(
            "                    crate::event::Key::Left => {\n"
            "                        crate::textedit::begin_or_end_selection(shift, *cursor, selection_anchor);",
            "                    crate::event::Key::Left => {",
        )],
        GUITK,
        ["shift_and_an_arrow_select_and_a_bare_arrow_gives_the_selection_up"],
    ),
    # ---- clicking in the text ---------------------------------------------
    (
        "AB: a click puts the caret at the start of the field, not where it landed",
        WIDGET,
        [(
            "                        mouse.x - content_x,",
            "                        0.0 * (mouse.x - content_x),",
        )],
        GUITK,
        ["clicking_in_the_text_puts_the_caret_where_the_click_landed"],
    ),
    (
        "AC: a click ignores how far the field is scrolled",
        EDIT,
        [(
            "    crate::text::cursor_at(text, dx + scroll, font_size, weight)",
            "    crate::text::cursor_at(text, dx, font_size, weight)",
        )],
        GUITK,
        ["clicking_in_a_scrolled_field_accounts_for_what_has_scrolled_off"],
    ),
    (
        "AD: a click keeps the selection it landed on",
        WIDGET,
        [(
            "                    *selection_anchor = None;\n                    EventResult::Consumed",
            "                    EventResult::Consumed",
        )],
        GUITK,
        ["a_click_gives_up_the_selection_it_landed_on"],
    ),
    # ---- the input dialog --------------------------------------------------
    (
        "AE: the input dialog's caret is drawn whether or not the field is focused",
        MODAL,
        [(
            "                    focused: field_focused,",
            "                    focused: true,",
        )],
        GUITK,
        ["the_field_draws_a_caret_when_it_has_the_focus_and_not_when_it_does_not"],
    ),
    (
        "AF: an empty input dialog shows no caret",
        MODAL,
        [(
            "            if field_focused {\n"
            "                crate::textedit::push_caret(tree, text_x, text_y, FONT_SIZE, COLOR_TEXT);\n"
            "            }",
            "",
        )],
        GUITK,
        ["an_empty_input_dialog_still_shows_where_typing_will_land"],
    ),
    (
        "AG: a password selection is measured in bytes, not in marks",
        MODAL,
        [(
            "        if !self.password_mode {\n            return byte;\n        }",
            "        if true {\n            return byte;\n        }",
        )],
        GUITK,
        ["a_password_selection_is_measured_in_marks_and_not_in_bytes"],
    ),
    (
        "AH: Delete takes the next character rather than the selection (dialog)",
        MODAL,
        [(
            "            Key::Delete => {\n                if crate::textedit::delete_selection(",
            "            Key::Delete => {\n                if false && crate::textedit::delete_selection(",
        )],
        GUITK,
        ["delete_over_a_selection_takes_the_selection_rather_than_the_next_character"],
    ),
    (
        "AI: Backspace takes the selection and the character before it (dialog)",
        MODAL,
        [(
            "            Key::Backspace => {\n                if crate::textedit::delete_selection(",
            "            Key::Backspace => {\n                if false && crate::textedit::delete_selection(",
        )],
        GUITK,
        ["backspace_over_an_input_dialogs_selection_takes_it_and_nothing_more"],
    ),
    (
        "AJ: typing beside the selection instead of over it (dialog)",
        MODAL,
        [(
            "                    crate::textedit::delete_selection(\n"
            "                        &mut self.input_text,\n"
            "                        &mut self.cursor,\n"
            "                        &mut self.selection_anchor,\n"
            "                    );\n"
            "                    self.input_text.insert(self.cursor.byte(), ch);",
            "                    self.input_text.insert(self.cursor.byte(), ch);",
        )],
        GUITK,
        ["typing_over_an_input_dialogs_selection_replaces_it"],
    ),
    (
        "AK: Left never plants a selection anchor (dialog)",
        MODAL,
        [(
            "            Key::Left => {\n"
            "                self.begin_or_end_selection(event.modifiers.shift);\n"
            "                self.move_caret(false);",
            "            Key::Left => {\n                self.move_caret(false);",
        )],
        GUITK,
        ["shift_and_an_arrow_select_in_the_input_dialog_and_a_bare_arrow_gives_it_up"],
    ),
    (
        "AL: replacing the text from code keeps the anchor into the old text",
        MODAL,
        [(
            "        self.cursor = TextCursor::from(text.len());\n"
            "        // The anchor names offsets in the string that has just been replaced,\n"
            "        // so it can be past the end of the new one.\n"
            "        self.selection_anchor = None;",
            "        self.cursor = TextCursor::from(text.len());",
        )],
        GUITK,
        ["replacing_the_text_from_code_drops_a_selection_that_named_the_old_text"],
    ),
]

NO_OP: set[str] = set()


def letter(name):
    """The defect's identifier -- everything before the first colon."""
    return name.split(":", 1)[0]


def run_tests(pkg):
    r = subprocess.run(
        ["cargo", "test", "-p", pkg, "--target", TARGET],
        cwd=ROOT, capture_output=True, text=True, errors="replace",
    )
    out = r.stdout + r.stderr
    # "error: test failed" is what a *failing test run* prints, so only
    # "could not compile" distinguishes a build break.
    if "could not compile" in out:
        return None, out
    failed = set()
    collecting = False
    for line in out.splitlines():
        s = line.strip()
        if s == "failures:":
            collecting = True
            continue
        if collecting:
            if "::" not in s:
                collecting = False
                continue
            failed.add(s.rsplit("::", 1)[-1])
    return failed, out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    check_only = "--check" in sys.argv[1:]

    files = sorted({d[1] for d in DEFECTS})
    snap = {f: (ROOT / f).read_bytes() for f in files}
    digest = {f: hashlib.sha256(b).hexdigest() for f, b in snap.items()}
    print("snapshot:")
    for f in files:
        print(f"  {digest[f][:16]}  {f}")
    print()

    selected = [d for d in DEFECTS
                if letter(d[0]) not in NO_OP
                and (not args or letter(d[0]) in args)]

    if check_only:
        bad = 0
        for name, path, edits, _pkgs, _expect in selected:
            text = snap[path].decode("utf-8")
            problems = []
            for old, new in edits:
                n = text.count(old)
                if n == 0:
                    problems.append("PATTERN NOT FOUND")
                elif n > 1:
                    problems.append(f"AMBIGUOUS ({n} matches)")
                elif old == new:
                    problems.append("NO-OP")
                else:
                    text = text.replace(old, new, 1)
            verdict = "; ".join(problems) if problems else "ok"
            if problems:
                bad += 1
            print(f"{name}\n    {verdict}")
        print(f"\n{len(selected) - bad}/{len(selected)} patterns apply cleanly")
        sys.exit(1 if bad else 0)

    verdicts = []
    try:
        for name, path, edits, pkgs, expect in selected:
            text = snap[path].decode("utf-8")
            ok = True
            for old, new in edits:
                if old not in text:
                    ok = False
                    break
                text = text.replace(old, new, 1)
            if not ok:
                verdicts.append((name, "PATTERN NOT FOUND"))
                print(f"{name}\n    PATTERN NOT FOUND\n", flush=True)
                continue
            (ROOT / path).write_text(text, encoding="utf-8", newline="")

            all_failed, note, broke = set(), "", False
            for pkg in pkgs:
                failed, _out = run_tests(pkg)
                if failed is None:
                    broke, note = True, f"{pkg} did not compile"
                    break
                all_failed |= failed
            (ROOT / path).write_bytes(snap[path])

            if broke:
                verdict = f"DID NOT COMPILE ({note})"
            elif not all_failed:
                verdict = "*** NO TEST FAILED ***"
            else:
                verdict = f"caught by {len(all_failed)}: {sorted(all_failed)}"
                missing = [t for t in expect if t not in all_failed]
                if missing and len(missing) == len(expect):
                    verdict += f"  [MISSING: {missing}]"
            verdicts.append((name, verdict))
            print(f"{name}\n    {verdict}\n", flush=True)
    finally:
        bad = []
        for f in files:
            (ROOT / f).write_bytes(snap[f])
            if hashlib.sha256((ROOT / f).read_bytes()).hexdigest() != digest[f]:
                bad.append(f)
        if bad:
            print(f"!!! NOT RESTORED: {bad}")
            sys.exit(2)
        print("restored: all files match their recorded SHA-256")

    print("\n=== summary ===")
    for name, verdict in verdicts:
        print(f"{name}\n    {verdict}")
    unproved = [n for n, v in verdicts
                if "NO TEST FAILED" in v or "NOT FOUND" in v
                or "DID NOT COMPILE" in v]
    print(f"\n{len(verdicts) - len(unproved)}/{len(verdicts)} defects caught")
    if unproved:
        print("unproved:")
        for n in unproved:
            print(f"  {n}")


if __name__ == "__main__":
    main()
