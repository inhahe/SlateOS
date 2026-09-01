"""Mutation test for taskscheduler's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Taskscheduler was already a window when this table was written -- it had a
`Layout` derived from the live size, hit boxes recorded by the drawing pass, an
`App`/`Probe` pair and 128 tests -- so the usual opening story of this campaign
("`main` drew one frame into a `Vec` and dropped it") does not apply here.  What
it had instead was a suite with almost nothing in it about *geometry*.  128
tests covered cron parsing, schedule arithmetic, the form, the scroll offsets
and the executor seam; exactly one asked a question about a rectangle
(`no_list_row_is_drawn_past_the_bottom_of_the_content_area`), and nothing at all
asked whether a pass stayed inside the band it was given.

That gap hid a whole class of fault, the one lesson 109 is about:

  * **Centring is not a bound.**  Every run in the file was placed by
    `band.y + (band.h - size) / 2.0`, which sits *above* the band's top edge the
    moment the band is shorter than the line and hangs the same distance below
    its bottom.  `take_top` deliberately shrinks a band to whatever is left, so
    this is not a hypothetical: a window a few points tall gives the header a
    strip shorter than its own 16-point heading, and the title was drawn above
    the header and over the toolbar.
  * **A constant is not a bound either.**  Runs were placed at a constant inset
    (`PADDING`, which is right of a band narrower than 12 points) with a constant
    `max_width`; the header's task count was pinned 200 points from the right
    edge and given a fixed 190-point width; the two lists drew six and four
    columns at constant x positions running to 815 points into a content area
    the window is free to make 400 wide.
  * **A fill is drawn at exactly the size it is given.**  The toolbar's buttons
    were 30-point fills centred in the strip, so in a six-point toolbar they
    painted over the tab bar *and* answered clicks there.  The tab underline was
    `band.bottom() - 3.0`, which is above the strip's own top edge whenever the
    strip is thinner than the rule.
  * **Both dialogs clamped their origin and left their size a constant.**  A
    window smaller than 440x380 got a dialog -- scrim, border, fields, buttons
    and hit boxes -- painted over whatever lies beyond the window.

The fix is three helpers (`centre_line`, `span`, `bottom_strip`) plus `run_in`,
which every list cell goes through, and `intersect` at each place a rectangle is
handed to a sub-pass.  The two tests that hold it are
`nothing_is_painted_outside_the_window` and
`no_pass_paints_outside_the_region_it_owns`; the second hands each control a
deliberately squeezed box, because a sub-pass's contract is "stay inside the box
you are given" for *any* box, not for the boxes today's `Layout::new` happens to
produce.

The first run of this table scored 21 of 29 and the eight survivors were worth
more than the twenty-one kills, because they were all one of two things and
neither was visible by reading the code:

  * **Bounds nothing could squeeze.**  The two list rows lived inside their
    loops, and the only rectangle they ever saw came from
    `scroll_window::visible`, which counts *whole* rows -- so the row's own
    `intersect`, the checkbox's `.min(row.h)` and every cell's `run_in` were
    exercised at exactly one height, 32 points, and could each be deleted with
    the suite still green.  The fix is not another assertion: it is
    `draw_task_row` / `draw_history_row` as functions *of the row*, so the
    sweep can hand them a three-point one.  Extracting them immediately turned
    up a real fault the containment assertions could not see -- a row squeezed
    to nothing still painted its background and still recorded a `TaskRow`
    click target.
  * **Clamps a stronger guard already dominated.**  The caret's
    `1.5_f32.min(rect.right() - caret_x)` and `FONT_SIZE.min(rect.h)` read like
    prudence, but the caret is drawn only where `centre_line` and `span` have
    both answered, and those two already say the field is at least FONT_SIZE
    tall with three points to spare on the right.  No input could make either
    `.min` bind.  A clamp no test can reach is not a guard, it is a claim; both
    are gone and the row that stood for them now mutates the `.min` that does
    bind.

The third survivor was the mirror-image failure, and it is why
`a_dialog_with_room_still_shows_its_buttons` exists: every bound in this app is
an intersection, and an intersection is just as happy to return nothing at all.
Placing the dialog's buttons by the nominal 380 points rather than by the
dialog's own cut-down height puts them below it, where the cut *deletes* them --
and containment is delighted, because ink that does not exist is inside
everything.  Containment on its own can always be satisfied by drawing nothing.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CONTAINMENT = ["no_pass_paints_outside_the_region_it_owns"]
WINDOW = ["nothing_is_painted_outside_the_window"]

MUTATIONS = [
    # -- the three helpers the rule now lives in ---------------------------
    (
        "a line is centred in a band too short to hold it",
        "    (band.h + 0.01 >= size).then(|| band.y + (band.h - size) / 2.0)",
        "    Some(band.y + (band.h - size) / 2.0)",
        # The original expression, unguarded.  This is the fault lesson 109 is
        # named for: it is above the band in a short band and below it by the
        # same amount, both at once.
        CONTAINMENT,
    ),
    (
        "a run runs past the right edge of its band",
        "    let right = (x + want.max(0.0)).min(band.right());",
        "    let right = x + want.max(0.0);",
        CONTAINMENT,
    ),
    (
        "a run starts left of its band",
        "    let left = x.max(band.x);",
        "    let left = x;",
        CONTAINMENT,
    ),
    (
        "a rule along the bottom edge is drawn at its literal height",
        "    let h = want.min(band.h).max(0.0);",
        "    let h = want;",
        # `band.bottom() - 3.0` is above `band.y` in a band under three points.
        CONTAINMENT,
    ),
    (
        "a band with no room still hands out a place for a line",
        "    (band.h + 0.01 >= size).then(|| band.y + (band.h - size) / 2.0)",
        "    Some(band.y)",
        # Not the same mutation as the first: this one is inside the band's top
        # edge, and fails only on the *bottom* -- which is the half a test that
        # only checked `y >= band.y` would have missed.
        CONTAINMENT,
    ),
    # -- header ------------------------------------------------------------
    (
        "the header's task count is given a fixed width at a fixed inset",
        "            span(band, (width - 200.0).max(PADDING), 190.0),",
        "            Some(((width - 200.0).max(PADDING), 190.0)),",
        CONTAINMENT,
    ),
    (
        "the header's title is inked whatever the header measures",
        "            centre_line(band, FONT_SIZE_HEADING),\n"
        "            span(band, PADDING, width - PADDING * 2.0),",
        "            Some(band.y + (band.h - FONT_SIZE_HEADING) / 2.0),\n"
        "            span(band, PADDING, width - PADDING * 2.0),",
        CONTAINMENT,
    ),
    # -- toolbar -----------------------------------------------------------
    (
        "a toolbar button is centred in the strip at its nominal height",
        "        let btn_h = BUTTON_HEIGHT.min(band.h);",
        "        let btn_h = BUTTON_HEIGHT;",
        # A fill and a hit box, both taken exactly as asked: a 30-point button
        # in a 6-point toolbar paints over the tab bar and answers clicks there.
        CONTAINMENT,
    ),
    (
        "four toolbar buttons are laid out whether or not the window has room",
        "            let w = (band.right() - bx).clamp(0.0, BUTTON_WIDTH);",
        "            let w = BUTTON_WIDTH;",
        CONTAINMENT,
    ),
    # -- tab bar -----------------------------------------------------------
    (
        "the tab labels are inked whatever the strip measures",
        "                (centre_line(band, FONT_SIZE), span(band, x, TAB_HIT_WIDTH))",
        "                (Some(band.y + (band.h - FONT_SIZE) / 2.0), span(band, x, TAB_HIT_WIDTH))",
        CONTAINMENT,
    ),
    (
        "a tab's hit box is a fixed 80 points wide",
        "                frame.hit(Target::Tab(tab), Rect::new(hx, band.y, hw, band.h));",
        "                frame.hit(\n"
        "                    Target::Tab(tab),\n"
        "                    Rect::new(x, band.y, TAB_HIT_WIDTH, band.h),\n"
        "                );",
        CONTAINMENT,
    ),
    # -- the two lists -----------------------------------------------------
    (
        "the task list's heading strip is a full row whatever the area measures",
        "        // row in a small window.\n"
        "        let head = Rect::new(area.x, area.y, area.w, ROW_HEIGHT.min(area.h));",
        "        // row in a small window.\n"
        "        let head = Rect::new(area.x, area.y, area.w, ROW_HEIGHT);",
        CONTAINMENT,
    ),
    (
        "the history's heading strip is a full row whatever the area measures",
        "        // rectangle at its top corner. See `render_task_list`.\n"
        "        let head = Rect::new(area.x, area.y, area.w, ROW_HEIGHT.min(area.h));",
        "        // rectangle at its top corner. See `render_task_list`.\n"
        "        let head = Rect::new(area.x, area.y, area.w, ROW_HEIGHT);",
        CONTAINMENT,
    ),
    (
        "a task row squeezed to nothing still paints and still answers clicks",
        "        // See `render_text_field`: no area means no ink and no hit box. A row\n"
        "        // squeezed to nothing that still records `TaskRow` is a click target\n"
        "        // for a task the user cannot see.\n"
        "        if row.is_empty() {\n"
        "            return;\n"
        "        }\n",
        "",
        # Not a containment fault -- a zero-sized fill is inside every region
        # there is -- which is why the sweep asks separately that a control
        # given an empty box draws nothing at all.
        CONTAINMENT,
    ),
    (
        "the checkbox is cut to the world instead of to its row",
        "            CHECKBOX_SIZE,\n"
        "            CHECKBOX_SIZE,\n"
        "        )\n"
        "        .intersect(row)",
        "            CHECKBOX_SIZE,\n"
        "            CHECKBOX_SIZE,\n"
        "        )\n"
        "        .intersect(Rect::new(-10_000.0, -10_000.0, 20_000.0, 20_000.0))",
        # Reachable only because `draw_task_row` takes its row as an argument:
        # inside the loop it lived in, the row was always 32 points and a
        # centred 16-point box always fit.  This replaces a row that mutated a
        # `CHECKBOX_SIZE.min(row.h)` shrink -- which survived, because the
        # `intersect` below it made the shrink redundant in every case.
        CONTAINMENT,
    ),
    (
        "a task row's cells are placed against the list, not against the row",
        "        ] {\n"
        "            run_in(frame, row, run, text);\n"
        "        }\n"
        "    }\n"
        "\n"
        "    /// Draw the History tab.",
        "        ] {\n"
        "            run_in(frame, Rect::new(row.x, row.y, row.w, ROW_HEIGHT), run, text);\n"
        "        }\n"
        "    }\n"
        "\n"
        "    /// Draw the History tab.",
        CONTAINMENT,
    ),
    (
        "a history row squeezed to nothing still paints",
        "        // See `Self::draw_task_row`.\n"
        "        if row.is_empty() {\n"
        "            return;\n"
        "        }\n",
        "",
        CONTAINMENT,
    ),
    (
        "the task list's 'N more' note is placed without checking the area",
        "        if hidden > 0\n"
        "            && let Some(band) = Rect::new(\n"
        "                area.x,\n"
        "                rows_top + (window.count as f32) * ROW_HEIGHT,\n"
        "                area.w,\n"
        "                FONT_SIZE_SMALL,\n"
        "            )\n"
        "            .intersect(area)\n"
        "        {\n"
        "            run_in(\n"
        "                frame,\n"
        "                band,\n"
        "                Run {\n"
        "                    x: PADDING,\n"
        "                    w: width - PADDING * 2.0,\n"
        "                    size: FONT_SIZE_SMALL,\n"
        "                    color: COLOR_SUBTEXT,\n"
        "                    weight: FontWeightHint::Regular,\n"
        "                },\n"
        "                format!(\"{hidden} more\"),\n"
        "            );\n"
        "        }\n"
        "\n"
        "        // Empty state.\n"
        "        if tasks.is_empty()",
        "        if hidden > 0 {\n"
        "            let band = Rect::new(\n"
        "                area.x,\n"
        "                rows_top + (window.count as f32) * ROW_HEIGHT,\n"
        "                area.w,\n"
        "                FONT_SIZE_SMALL,\n"
        "            );\n"
        "            run_in(\n"
        "                frame,\n"
        "                band,\n"
        "                Run {\n"
        "                    x: PADDING,\n"
        "                    w: width - PADDING * 2.0,\n"
        "                    size: FONT_SIZE_SMALL,\n"
        "                    color: COLOR_SUBTEXT,\n"
        "                    weight: FontWeightHint::Regular,\n"
        "                },\n"
        "                format!(\"{hidden} more\"),\n"
        "            );\n"
        "        }\n"
        "\n"
        "        // Empty state.\n"
        "        if tasks.is_empty()",
        CONTAINMENT,
    ),
    (
        "the 'no tasks' line is placed forty points down whatever the area measures",
        "        if tasks.is_empty()\n"
        "            && let Some(band) =\n"
        "                Rect::new(area.x, top + ROW_HEIGHT + 40.0, area.w, FONT_SIZE)"
        ".intersect(area)\n"
        "        {",
        "        if tasks.is_empty()\n"
        "            && let band = Rect::new(area.x, top + ROW_HEIGHT + 40.0, area.w, FONT_SIZE)\n"
        "        {",
        CONTAINMENT,
    ),
    # -- the status bar ----------------------------------------------------
    (
        "the status message is inked whatever the bar measures",
        "        run_in(\n"
        "            frame,\n"
        "            band,\n"
        "            Run {\n"
        "                x: PADDING,\n"
        "                w: width - PADDING * 2.0,\n"
        "                size: FONT_SIZE_SMALL,\n"
        "                color: COLOR_YELLOW,\n"
        "                weight: FontWeightHint::Regular,\n"
        "            },\n"
        "            message.to_string(),\n"
        "        );",
        "        frame.push(RenderCommand::Text {\n"
        "            x: PADDING,\n"
        "            y: band.y + (bar_h - FONT_SIZE_SMALL) / 2.0,\n"
        "            text: message.to_string(),\n"
        "            color: COLOR_YELLOW,\n"
        "            font_size: FONT_SIZE_SMALL,\n"
        "            font_weight: FontWeightHint::Regular,\n"
        "            max_width: Some(width - PADDING * 2.0),\n"
        "            overflow: TextOverflow::Ellipsis,\n"
        "        });",
        CONTAINMENT,
    ),
    # -- the dialogs -------------------------------------------------------
    (
        "the add/edit dialog is 440x380 in a window of any size",
        "        // dialog loses its lower rows instead of hanging them outside.\n"
        "        let dialog = Rect::new(dx, dy, dialog_w.min(window.w), dialog_h.min(window.h));",
        "        // dialog loses its lower rows instead of hanging them outside.\n"
        "        let dialog = Rect::new(dx, dy, dialog_w, dialog_h);",
        # The scrim is the window's own size, so this escapes the *window*, not
        # merely the dialog's own box.
        WINDOW + CONTAINMENT,
    ),
    (
        "the delete dialog is 360x160 in a window of any size",
        "        // `render_add_edit_dialog`.\n"
        "        let dialog = Rect::new(dx, dy, dialog_w.min(window.w), dialog_h.min(window.h));",
        "        // `render_add_edit_dialog`.\n"
        "        let dialog = Rect::new(dx, dy, dialog_w, dialog_h);",
        WINDOW + CONTAINMENT,
    ),
    (
        "a dialog row is handed to its control uncut",
        "        let cut = |r: Rect| r.intersect(dialog).unwrap_or(Rect::EMPTY);\n"
        "        let mut label = |frame: &mut Frame, text: &str| {",
        "        let cut = |r: Rect| r;\n"
        "        let mut label = |frame: &mut Frame, text: &str| {",
        CONTAINMENT,
    ),
    (
        "the enabled checkbox is drawn outside the dialog it belongs to",
        "        if let Some(cb) =\n"
        "            Rect::new(value_x, cb_y - 1.0, CHECKBOX_SIZE, CHECKBOX_SIZE).intersect(dialog)\n"
        "        {",
        "        if let cb = Rect::new(value_x, cb_y - 1.0, CHECKBOX_SIZE, CHECKBOX_SIZE) {",
        CONTAINMENT,
    ),
    (
        "the dialog's buttons are placed 380 points below its top edge",
        "        let btn_y = dialog.bottom() - BUTTON_HEIGHT - PADDING;\n"
        "        let cancel_x = dialog.right() - PADDING - BUTTON_WIDTH;\n"
        "        let save_x = cancel_x - 8.0 - BUTTON_WIDTH;",
        "        let btn_y = dy + dialog_h - BUTTON_HEIGHT - PADDING;\n"
        "        let cancel_x = dx + dialog_w - PADDING - BUTTON_WIDTH;\n"
        "        let save_x = cancel_x - 8.0 - BUTTON_WIDTH;",
        # The one fault in this table that containment cannot see: the buttons
        # land below the cut-down dialog, `cut` deletes them, and a frame with
        # no buttons in it paints outside nothing.
        ["a_dialog_with_room_still_shows_its_buttons"],
    ),
    # -- the three controls ------------------------------------------------
    (
        "a text field squeezed to nothing still records a hit box",
        "        // that is still recording a hit box at a position outside it.\n"
        "        if rect.is_empty() {\n"
        "            return;\n"
        "        }",
        "        // that is still recording a hit box at a position outside it.",
        CONTAINMENT,
    ),
    (
        "a picker squeezed to nothing still records a hit box",
        "        // See `render_text_field`: no area means no ink and no hit box.\n"
        "        if rect.is_empty() {\n"
        "            return;\n"
        "        }",
        "        // See `render_text_field`: no area means no ink and no hit box.",
        CONTAINMENT,
    ),
    (
        "a button squeezed to nothing still records a hit box",
        "        // nothing is neither.\n"
        "        if rect.is_empty() {\n"
        "            return;\n"
        "        }",
        "        // nothing is neither.",
        CONTAINMENT,
    ),
    (
        "the caret follows the unelided value off the field's right edge",
        "            let caret_x = (x + text::measure(value, FONT_SIZE, FontWeightHint::Regular))\n"
        "                .min(rect.right() - 3.0);",
        "            let caret_x = x + text::measure(value, FONT_SIZE, FontWeightHint::Regular);",
        # The clamp that replaced two that could not be reached.  `text::measure`
        # sizes the value before eliding, so a value wider than the field puts
        # the caret past its right edge -- which is what the sweep's "a value far
        # wider than the field" state is for.
        CONTAINMENT,
    ),
    (
        "a button's label is centred whether or not the button is wide enough",
        "        if let (Some(y), Some((x, w))) = (centre_line(rect, FONT_SIZE), span(rect, text_x, rect.w))\n"
        "        {",
        "        if let (Some(y), Some((x, w))) =\n"
        "            (centre_line(rect, FONT_SIZE), Some((text_x, rect.w)))\n"
        "        {",
        CONTAINMENT,
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "taskscheduler", timeout=600, only=only))
