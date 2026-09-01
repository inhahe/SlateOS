"""Mutation test for the compass's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The compass is the forty-seventh application in this campaign.  `main` was:

    fn main() {
        let _app = CompassApp::new();
    }

It built the whole program -- a rose, a needle, a declination model, ten
waypoints, three views, a coordinate parser -- and dropped it without drawing a
pixel or reading a key, then exited zero.

What the wiring exposed, in rough order of how badly it would have shown:

  * **Nothing was clickable but the waypoint list**, and that only by accident:
    the list re-derived its rows arithmetically from constants, so a press was
    answered at coordinates a scrolled-away or clipped-away row no longer
    occupied.  The rose, the tabs, the unit toggle, the declination steppers,
    the delete button, the entry fields and the add button did not exist as
    controls at all -- the views were reachable with `W`, `C` and `Esc`, the
    heading with the arrow keys, and declination with `D` and `Ctrl+D`, where
    `D` alone *decreased* it.
  * **The name field was painted, labelled, and impossible to type into.**
    `CoordField::next` cycled two fields, so `Tab` never reached the third and
    no pointer route existed to any of them.
  * **`key_to_char` mapped `Key::A` to `.` and `Key::B` to `-`**, ignoring both
    shift and the keyboard layout, so on any non-US layout a coordinate could
    not be typed and a `+` could not be produced at all.
  * **The waypoint list was one padded `format!` per row** -- `{:<14}`,
    `{:<12}` -- which is a claim that the font is monospaced.  It is not.
  * **All but three runs of text were drawn with `max_width: None`**, so a long
    waypoint name walked over the column beside it.
  * **Ten crate-level `allow`s** covered the file, including `dead_code` over
    an unused palette entry.

One more fault was found by the size sweep while these tests were being
written, and it is the one the geometry sweep leans on hardest: text was
vertically centred in its box at whatever size the caller asked for, so a run
taller than its box stuck out of both ends -- which is how the status line came
to be drawn below the bottom edge of a 30-pixel window.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- the layout ----------------------------------------------------------
    (
        "a window size that is not a number is laid out rather than zeroed",
        "        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };\n"
        "        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };",
        "        let w = w.max(0.0);\n        let h = h.max(0.0);",
        ["a_size_that_is_not_a_size_still_produces_a_window"],
    ),
    (
        "the status line is hung past the bottom edge of the window",
        "        let status = Rect::new(0.0, (h - status_h).max(header_h), w, status_h);",
        "        let status = Rect::new(0.0, h, w, status_h);",
        # Not `nothing_is_drawn_over_the_status_line`: moving the strip off the
        # bottom of the window does not make anything overlap it -- there is
        # nothing left to overlap. What it does is put a run of text outside
        # the window, which is the sweep's business.
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the body is given the room the status line is standing in",
        "        let body_h = (h - header_h - status_h).max(0.0);",
        "        let body_h = (h - header_h).max(0.0);",
        ["nothing_is_drawn_over_the_status_line"],
    ),
    (
        "a panel too narrow for its own numbers is taken anyway",
        "        let panel_w = if wanted_panel >= small * 8.0 {",
        "        let panel_w = if wanted_panel > 0.0 {",
        ["a_panel_narrow_enough_to_squeeze_its_readouts_is_not_taken"],
    ),
    (
        "the panel takes more of the window than it leaves the rose",
        "        let wanted_panel = (w * 0.32).clamp(0.0, 300.0);",
        "        let wanted_panel = (w * 0.72).clamp(0.0, 300.0);",
        ["the_panel_is_given_up_before_the_rose_is"],
    ),
    (
        "the rose is inscribed in the wider dimension rather than the smaller",
        "        let side = rose_w.min(body_h);",
        "        let side = rose_w;",
        ["the_rose_is_a_circle_wherever_it_is_drawn"],
    ),
    (
        "a row that half fits is counted as a row",
        "        let n = (height / pitch).floor();",
        "        let n = (height / pitch).ceil();",
        # Not `a_row_that_does_not_fit_is_neither_drawn_nor_clickable`: the
        # extra row is both drawn and hit-boxed, so the two sets grow together
        # and that test cannot see it. What gives it away is the *shape* of the
        # last hit box -- `Frame::hit` trims it to the clip, so a row that only
        # half fits answers over half a row.
        ["every_clickable_row_is_a_whole_row"],
    ),
    # -- what is painted -----------------------------------------------------
    (
        "only the body is filled, leaving the rest of the window bare",
        "        f.push(fill(l.window, BASE, 0.0));",
        "        f.push(fill(l.body, BASE, 0.0));",
        ["the_window_is_painted_edge_to_edge_at_every_size"],
    ),
    (
        "text is drawn unbounded, so a long name walks over the column beside it",
        "        max_width: Some(r.w),",
        "        max_width: None,",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "a run taller than its box is centred in it anyway",
        "    // and no caller can prevent that from where it stands.\n"
        "    let size = size.min(r.h);",
        "    // and no caller can prevent that from where it stands.\n"
        "    let size = size;",
        ["every_run_of_text_is_bounded_and_inside_the_window"],
    ),
    (
        "the body's clip is opened and never closed",
        "        f.unclip();\n\n        self.draw_status(&mut f, &l);",
        "\n        self.draw_status(&mut f, &l);",
        ["the_frame_is_balanced_at_every_size_and_state"],
    ),
    (
        "the mark button keeps its words when the list is full",
        '            if full { "List full" } else { "Mark here (M)" },',
        '            "Mark here (M)",',
        ["a_full_list_says_so_rather_than_swallowing_the_press"],
    ),
    # -- where a press goes --------------------------------------------------
    (
        "every tab answers for the compass view",
        "            f.hit(Target::Tab(*view), r);",
        "            f.hit(Target::Tab(View::Compass), r);",
        ["pressing_a_tab_switches_to_the_view_it_names"],
    ),
    (
        "the unit toggle is not a control at all",
        "        f.hit(Target::Units, units);",
        "        f.hit(Target::Rose, units);",
        ["the_unit_toggle_reads_the_unit_in_force_and_changes_it"],
    ),
    (
        "the declination steppers are wired to each other's sign",
        '        for (r, sign, nudge) in [(minus, "-", Nudge::Down), (plus, "+", Nudge::Up)] {',
        '        for (r, sign, nudge) in [(minus, "-", Nudge::Up), (plus, "+", Nudge::Down)] {',
        ["the_declination_steppers_move_it_the_way_they_are_labelled"],
    ),
    (
        "a press on the rose is read as a magnetic bearing, declination and all",
        "                    self.heading = wrap_360(deg - self.declination);",
        "                    self.heading = wrap_360(deg);",
        ["pressing_the_rose_points_the_compass_at_the_pressed_point"],
    ),
    (
        "the pressed bearing is measured from east rather than from north",
        "                    let deg = f64::from(dx.atan2(dy)) * RAD_TO_DEG;",
        "                    let deg = f64::from(dy.atan2(dx)) * RAD_TO_DEG;",
        ["pressing_the_rose_points_the_compass_at_the_pressed_point"],
    ),
    (
        "the rose is not a control, so a heading can only be typed",
        "        f.hit(Target::Rose, l.rose);",
        "        f.hit(Target::Units, l.rose);",
        ["pressing_the_rose_points_the_compass_at_the_pressed_point"],
    ),
    (
        "a row answers for the waypoint below it",
        "            f.hit(Target::Waypoint(i), row);",
        "            f.hit(Target::Waypoint(i.saturating_add(1)), row);",
        ["pressing_a_row_selects_the_waypoint_whose_name_it_shows"],
    ),
    (
        "the gap below a row belongs to nobody",
        "            f.hit(Target::Waypoint(i), row);",
        "            f.hit(Target::Waypoint(i), text_row);",
        ["no_press_inside_the_list_falls_between_two_rows"],
    ),
    (
        "the list never scrolls, so a selection past the last row is off screen",
        "        if sel < visible {\n            0\n        } else {\n"
        "            sel.saturating_add(1).saturating_sub(visible)\n        }",
        "        let _ = sel;\n        0",
        ["the_selected_waypoint_is_always_on_screen"],
    ),
    (
        "delete answers a press with nothing selected",
        "        if armed {\n            f.hit(Target::DeleteWaypoint, btn);\n        }",
        "        f.hit(Target::DeleteWaypoint, btn);",
        ["delete_is_only_offered_when_nothing_would_be_lost_by_pressing_it"],
    ),
    (
        "every entry field focuses the latitude",
        "            f.hit(Target::Field(field), entry);",
        "            f.hit(Target::Field(CoordField::Latitude), entry);",
        ["pressing_a_field_gives_it_the_keyboard_and_typing_reaches_it"],
    ),
    (
        "the add button is not a control",
        "            f.hit(Target::AddWaypoint, btn);",
        "            f.hit(Target::Field(CoordField::Latitude), btn);",
        ["the_add_button_makes_the_waypoint_the_fields_describe"],
    ),
    (
        "the mark button is not a control",
        "        f.hit(Target::MarkHere, r);",
        "        f.hit(Target::Rose, r);",
        ["the_mark_button_makes_a_waypoint_at_the_position_on_screen"],
    ),
    # -- what the keyboard produces -----------------------------------------
    (
        "a waypoint name may not contain a space",
        "        CoordField::Name => !c.is_control(),",
        "        CoordField::Name => c.is_alphanumeric(),",
        ["pressing_a_field_gives_it_the_keyboard_and_typing_reaches_it"],
    ),
    # -- the entry points the platform calls ---------------------------------
    (
        "the size the frame was drawn at is not remembered, so a press is answered blind",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {\n"
        "        self.width = width;\n        self.height = height;",
        "    fn render(&mut self, width: f32, height: f32) -> RenderTree {",
        ["a_press_is_answered_against_the_size_the_last_frame_was_drawn_at"],
    ),
    (
        "the picture is drawn at the size it was launched with rather than the given one",
        "        self.frame(width, height).into_tree()",
        "        self.frame(WINDOW_WIDTH, WINDOW_HEIGHT).into_tree()",
        ["the_picture_is_drawn_at_the_size_render_is_given"],
    ),
    (
        "a resize is not remembered, so a press before the next frame is answered blind",
        "                self.width = *width as f32;\n                self.height = *height as f32;",
        "                let _ = (width, height);",
        ["a_resize_moves_where_the_controls_answer"],
    ),
    (
        "the close button is answered with a redraw",
        "            Event::CloseRequested => Response::Exit,",
        "            Event::CloseRequested => Response::Redraw,",
        ["the_close_button_closes_the_window_and_nothing_else_does"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "compass", timeout=300, only=only))
