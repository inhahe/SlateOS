#!/usr/bin/env python3
"""Prove the modal dialogs' mouse tests are regression tests.

The fifteenth of these harnesses. It covers the rework that closed
`TD-C-NO-MODAL-DIALOG-KNOWS-WHERE-IT-IS-WHEN-IT-IS-CLICKED` and is recorded in
`design-decisions.md` §547.

The bug that started it is the reason to distrust everything the fix added.
`AlertDialog::handle_mouse` tested clicks against `self.compute_layout(800.0,
600.0)` -- a *guess* at the parent's size, made fresh at click time, because
`handle_mouse` is not told how big the screen is and the author needed a number.
On the 800x600 the guess names, the hit areas are exactly right and every test
passes. On a 1920x1080 desktop they sit 560 px left and 240 px above the buttons
they belong to, so the dialog is a picture of two buttons that cannot be
pressed. A suite that only ever renders at the guessed size cannot see that, and
the suite did only ever render at the guessed size.

The fix makes the drawn rectangle the only rectangle: `render` takes `&mut self`
and records where it put things, and `handle_mouse` can only consult that
record. So the defects below are the ways that record can be wrong or unused
while the code still reads correctly and still compiles:

- the record never written, never read, or read from a fresh guess again;
- "not drawn yet" collapsed into "drawn at the origin with no size", which is
  what made a click dismiss a dialog in the frame before it first appeared;
- a hit test that answers the same button whatever was clicked, or a draw that
  puts every button where the first one goes -- the two halves of the same
  divergence, from opposite ends;
- each of the input dialog's three hit areas silently switched off, which is a
  button that is drawn, is focusable, and does nothing;
- a click into the text measured from the field's border rather than from the
  glyphs, so every caret lands a padding's worth of characters early;
- a click that leaves the selection it landed on behind, so the next keystroke
  deletes a run of text with nothing on screen to have warned about it;
- the empty-field guard dropped, which resolves the click against the
  *placeholder* -- an offset past the end of the string the caret lives in, and
  a panic in `String::insert` on the next keypress;
- a password click read as a byte offset into the secret rather than as a count
  of marks, which is the same encoded-length leak the masking exists to prevent,
  now with a panic attached when the offset lands inside a character;
- the progress dialog's mouse handling removed wholesale, which is where it
  started: a Cancel button that was drawn, and did nothing, on a dialog whose
  entire visible offer is that button.

The click-offset arithmetic itself -- adding the horizontal scroll before
resolving a click -- is shared with the plain text input and is proved by defect
AC of `reintro-toolkit-focus.py`, which patches the one copy in
`textedit::cursor_at_click` that both call. It is deliberately not duplicated
here.

Restore discipline as in the companions: a byte snapshot up front, written back
unconditionally in a `finally`, verified by SHA-256 -- not a reverse
search-and-replace, which silently leaves the tree modified if a patch
half-applied or the process died between the write and the undo.

Two modes:

- `--check` matches every pattern against the snapshot and builds nothing.
  Seconds, no toolchain, and it answers the question that rots on its own: has
  a rename or a rustfmt pass stopped a defect applying?
- No flag: the real sweep. Apply, run the tests, restore, report.

Filter either with defect letters: `reintro-modal-geometry.py A B C`.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

MODAL = "gui/toolkit/src/modal.rs"

GUITK = ["guitk"]

ALERT_CLICK = "an_alerts_buttons_are_clickable_where_they_were_drawn_on_any_size_of_parent"
FIELD_CLICK = "clicking_the_input_dialogs_field_focuses_it_and_puts_the_caret_where_it_landed"
PASSWORD_CLICK = "a_click_in_a_password_field_lands_on_a_character_of_the_secret"
PROGRESS_CANCEL = "a_cancelable_progress_dialogs_cancel_button_can_be_clicked"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    # ---- the alert knows where it is ---------------------------------------
    (
        "A: the alert hit-tests against a guessed 800x600 parent, as it used to",
        MODAL,
        [(
            "            let hit = self.placement.as_ref().and_then(|layout| {",
            "            let hit = Some(self.compute_layout(800.0, 600.0)).as_ref()"
            ".and_then(|layout| {",
        )],
        GUITK,
        [ALERT_CLICK],
    ),
    (
        "B: the alert draws itself but does not remember where",
        MODAL,
        [("        self.placement = Some(layout);", "        drop(layout);")],
        GUITK,
        [ALERT_CLICK],
    ),
    (
        "C: the alert never tells the overlay where its box is",
        MODAL,
        [(
            "        self.overlay\n"
            "            .set_content_rect(layout.x, layout.y, layout.width, layout.height);",
            "        let _unplaced = (layout.x, layout.y);",
        )],
        GUITK,
        ["a_click_outside_a_drawn_alert_still_dismisses_it"],
    ),
    (
        "D: a dialog that has not been drawn is treated as one drawn empty at the origin",
        MODAL,
        [(
            "            content_rect: None,",
            "            content_rect: Some((0.0, 0.0, 0.0, 0.0)),",
        )],
        GUITK,
        ["a_dialog_that_has_not_been_drawn_yet_is_not_dismissed_by_a_click"],
    ),
    (
        "E: every click on an alert presses its first button",
        MODAL,
        [(
            "                    .position(|r| point_in_rect(event.x, event.y, r.0, r.1, r.2, r.3))",
            "                    .position(|_| true)",
        )],
        GUITK,
        [ALERT_CLICK],
    ),
    (
        "F: the alert draws every button where the first one goes",
        MODAL,
        [(
            "            let Some(&(btn_x, y, _, _)) = layout.button_rects.get(i) else {",
            "            let Some(&(btn_x, y, _, _)) = layout.button_rects.first() else {",
        )],
        GUITK,
        [ALERT_CLICK],
    ),
    # ---- the input dialog's three hit areas --------------------------------
    (
        "G: the input dialog's OK button is drawn but not clickable",
        MODAL,
        [(
            "        if point_in_rect(event.x, event.y, p.ok.0, p.ok.1, p.ok.2, p.ok.3) {",
            "        if false && point_in_rect(event.x, event.y, p.ok.0, p.ok.1, p.ok.2, p.ok.3) {",
        )],
        GUITK,
        ["an_input_dialogs_ok_button_can_be_clicked"],
    ),
    (
        "H: the input dialog's Cancel button is drawn but not clickable",
        MODAL,
        [(
            "        if point_in_rect(\n"
            "            event.x, event.y, p.cancel.0, p.cancel.1, p.cancel.2, p.cancel.3,\n"
            "        ) {",
            "        if false {",
        )],
        GUITK,
        ["an_input_dialogs_cancel_button_can_be_clicked"],
    ),
    (
        "I: the input dialog's text field cannot be clicked into",
        MODAL,
        [(
            "        if point_in_rect(event.x, event.y, p.field.0, p.field.1, p.field.2, p.field.3) {",
            "        if false {",
        )],
        GUITK,
        [FIELD_CLICK],
    ),
    (
        "J: clicking the field places the caret but does not move the focus there",
        MODAL,
        [(
            "            self.focused_element = InputFocus::TextField;\n"
            "            self.place_caret_at(event.x - p.text_x, p.text_width);",
            "            self.place_caret_at(event.x - p.text_x, p.text_width);",
        )],
        GUITK,
        [FIELD_CLICK],
    ),
    (
        "K: a click in the field is measured from its border, not from the glyphs",
        MODAL,
        [(
            "            self.place_caret_at(event.x - p.text_x, p.text_width);",
            "            self.place_caret_at(event.x - p.field.0, p.field.2);",
        )],
        GUITK,
        [FIELD_CLICK],
    ),
    (
        "L: a click leaves behind the selection it landed on",
        MODAL,
        [(
            "        self.selection_anchor = None;\n\n"
            "        if self.input_text.is_empty() {",
            "        if self.input_text.is_empty() {",
        )],
        GUITK,
        ["clicking_in_the_input_dialogs_field_gives_up_the_selection_it_landed_on"],
    ),
    (
        "M: a click in an empty field is resolved against the placeholder",
        MODAL,
        [(
            "        if self.input_text.is_empty() {\n"
            "            // What is drawn is the placeholder, which is not editable text. It\n"
            "            // has no caret positions in it, so a click anywhere in the field\n"
            "            // means the one place typing can go.\n"
            "            self.cursor = TextCursor::default();\n"
            "            return;\n"
            "        }\n\n",
            "",
        )],
        GUITK,
        ["clicking_in_an_empty_input_field_leaves_the_caret_at_the_start"],
    ),
    (
        "N: a click in a password field is read as a byte offset into the secret",
        MODAL,
        [(
            "            TextCursor::from(self.byte_at_drawn(hit.byte()))",
            "            TextCursor::from(hit.byte())",
        )],
        GUITK,
        [PASSWORD_CLICK],
    ),
    (
        "O: a password click is resolved against the secret rather than the marks",
        MODAL,
        [(
            "        let display = self.display_text();",
            "        let display = self.input_text.clone();",
        )],
        GUITK,
        [PASSWORD_CLICK],
    ),
    # ---- the progress dialog's Cancel button -------------------------------
    (
        "P: the progress dialog has no mouse handling at all, as it used to",
        MODAL,
        [(
            "            Event::Mouse(mouse_event) => self.handle_mouse(mouse_event),\n"
            "            Event::Tick { elapsed_ms } => {\n"
            "                self.tick(*elapsed_ms);\n"
            "                EventResult::Consumed\n"
            "            }\n"
            "            _ => EventResult::Consumed,\n"
            "        }\n"
            "    }\n"
            "\n"
            "    /// Handle a mouse event.",
            "            Event::Tick { elapsed_ms } => {\n"
            "                self.tick(*elapsed_ms);\n"
            "                EventResult::Consumed\n"
            "            }\n"
            "            _ => EventResult::Consumed,\n"
            "        }\n"
            "    }\n"
            "\n"
            "    #[allow(dead_code)]\n"
            "    /// Handle a mouse event.",
        )],
        GUITK,
        [PROGRESS_CANCEL],
    ),
    (
        "Q: the progress dialog draws its Cancel button without recording where",
        MODAL,
        [(
            "            self.cancel_rect = Some((btn_x, btn_y, BUTTON_MIN_WIDTH, BUTTON_HEIGHT));",
            "            let _undrawn = (btn_x, btn_y);",
        )],
        GUITK,
        [PROGRESS_CANCEL],
    ),
    (
        "R: any click anywhere cancels a progress dialog",
        MODAL,
        [(
            "        if let MouseEventKind::Press(MouseButton::Left) = event.kind\n"
            "            && let Some((bx, by, bw, bh)) = self.cancel_rect\n"
            "            && point_in_rect(event.x, event.y, bx, by, bw, bh)\n"
            "        {",
            "        if let MouseEventKind::Press(MouseButton::Left) = event.kind {",
        )],
        GUITK,
        ["a_progress_dialog_without_a_cancel_button_cannot_be_cancelled_by_a_click"],
    ),
    # ---- where the focus ring lands ----------------------------------------
    (
        "S: clicking OK accepts but leaves the focus ring in the text field",
        MODAL,
        [(
            "            self.focused_element = InputFocus::OkButton;\n"
            "            self.try_accept();",
            "            self.try_accept();",
        )],
        GUITK,
        ["clicking_ok_on_a_rejected_input_moves_the_focus_ring_to_it_and_stays_open"],
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
