#!/usr/bin/env python3
"""Prove the `Palette` tests are regression tests, one defect at a time.

The third of these harnesses (after `reintro-mouse-page.py` and
`reintro-reload-input.py`), covering
`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`. Part 1 is
defects A–U: the resolved `Palette` type in `gui/appearance`, and the two
consumers rewritten to read roles out of it instead of repeating its values —
`DecorationColors` and `DesktopTheme`. Part 2 begins at defect V, and is a
different question: not "is the palette right?" but "did the conversion of a
module off its own colour constants actually finish?" Those defects put one
constant back and check that the module's two-mode sweep names it.

A palette is unusually easy to test *vacuously*. Almost any assertion about a
colour is satisfiable by the colour that is already there, so a suite can look
thorough and be checking that `0x1E1E2E == 0x1E1E2E`. The defects below are
therefore all of the shape the real bug had: a light-mode value left as its
dark-mode counterpart, a role silently pointing at its neighbour, a user
setting that reaches the struct's constructor and stops. Each names a test, and
the test has to name it back.

Restore discipline as in the companions: byte snapshots taken up front, written
back unconditionally in a `finally`, verified by SHA-256. A reverse
search-and-replace is not good enough — if a patch half-applied, or a formatter
ran, or the process died between the write and the undo, a reverse replace
silently leaves the tree modified while claiming success.

One finding is recorded here rather than as a defect, because it cannot be one.
Defect Q — a window's close button following the accent — went *uncaught* on
the first run, and the reason was not the test: `DecorationColors::from_settings`
built its frame from `Palette::for_mode` and painted the accent on top
afterwards, so the palette a frame was assembled from always carried the
*default* accent and `p.accent` was blue whatever the user had chosen. Both are
fixed now (`from_settings` builds from `Palette::from_settings`, and Q fails as
it should), but reintroducing that indirection on its own is not a defect this
harness can measure: no frame role reads the accent today, so the wrong palette
and the right one produce identical output. What guards it is the shape rather
than a test — `from_palette` takes a `&Palette`, so there is exactly one place
where the wrong one could be passed, and it is three lines long.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

APP = "gui/appearance/src/lib.rs"
DESK = "gui/desktop/src/lib.rs"
SEC = "gui/desktop/src/security_dialog.rs"
RUN = "gui/desktop/src/run_dialog.rs"
ICON = "gui/desktop/src/icons.rs"
NOTIF = "gui/desktop/src/notif_pane.rs"
DEV = "gui/desktop/src/device_settings.rs"
RULES = "gui/desktop/src/window_rules.rs"
ACCT = "gui/desktop/src/user_accounts.rs"
BT = "gui/desktop/src/bluetooth.rs"
UPD = "gui/desktop/src/update_settings.rs"
STOR = "gui/desktop/src/storage_settings.rs"
POW = "gui/desktop/src/power_settings.rs"
NET = "gui/desktop/src/network_indicator.rs"
CLIP = "gui/desktop/src/clipboard_viewer.rs"
NOTIF = "gui/desktop/src/notification_settings.rs"
BACKUP = "gui/desktop/src/backup_settings.rs"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    (
        "A: the light palette keeps Catppuccin's own subtext0, which is 4.37:1",
        APP,
        [("pub const LIGHT_SUBTEXT0: Color = Color::from_hex(0x686B80);",
          "pub const LIGHT_SUBTEXT0: Color = Color::from_hex(0x6C6F85);")],
        ["appearance"],
        ["every_role_a_user_reads_is_legible_on_the_base_of_its_own_palette"],
    ),
    (
        "B: one line of the light palette was left as its dark counterpart",
        APP,
        [("                subtext1: LIGHT_SUBTEXT1,", "                subtext1: SUBTEXT1,")],
        ["appearance"],
        ["every_role_has_a_different_value_in_the_two_modes"],
    ),
    (
        "C: two rungs of the light ladder are swapped",
        APP,
        [("                surface1: LIGHT_SURFACE1,\n"
          "                surface2: LIGHT_SURFACE2,",
          "                surface1: LIGHT_SURFACE2,\n"
          "                surface2: LIGHT_SURFACE1,")],
        ["appearance", "desktop"],
        ["the_surface_ladder_climbs_away_from_the_base_in_both_modes"],
    ),
    (
        "D: crust and mantle are swapped in light mode",
        APP,
        [("                crust: LIGHT_CRUST,\n"
          "                mantle: LIGHT_MANTLE,",
          "                crust: LIGHT_MANTLE,\n"
          "                mantle: LIGHT_CRUST,")],
        ["appearance"],
        ["the_recessed_layers_are_darker_than_the_base_in_both_modes"],
    ),
    (
        "E: the accent setting never reaches the palette",
        APP,
        [("        palette.accent = settings.effective_accent();\n", "")],
        ["appearance", "desktop"],
        ["the_accent_setting_moves_the_accent_and_leaves_the_categorical_hues_alone",
         "a_custom_accent_reaches_the_palette_exactly_as_chosen"],
    ),
    (
        "F: the accent overwrites the categorical blue as well",
        APP,
        [("        palette.accent = settings.effective_accent();",
          "        palette.accent = settings.effective_accent();\n"
          "        palette.blue = palette.accent;")],
        ["appearance"],
        ["the_accent_setting_moves_the_accent_and_leaves_the_categorical_hues_alone"],
    ),
    (
        "G: the transparency level never reaches the palette",
        APP,
        [("        palette.panel_alpha = settings.transparency.panel_alpha();\n", "")],
        ["appearance"],
        ["transparency_reaches_panels_and_nothing_behind_them"],
    ),
    (
        "H: a panel is drawn opaque however transparent the user asked for",
        APP,
        [("    pub fn panel_bg(&self) -> Color {\n"
          "        with_alpha(self.base, self.panel_alpha)",
          "    pub fn panel_bg(&self) -> Color {\n"
          "        self.base")],
        ["appearance"],
        ["transparency_reaches_panels_and_nothing_behind_them"],
    ),
    (
        "I: the alpha meant for panels is applied to the base itself",
        APP,
        [("        palette.panel_alpha = settings.transparency.panel_alpha();",
          "        palette.panel_alpha = settings.transparency.panel_alpha();\n"
          "        palette.base = with_alpha(palette.base, palette.panel_alpha);")],
        ["appearance"],
        ["transparency_reaches_panels_and_nothing_behind_them"],
    ),
    (
        "J: the scrim dims with the palette's own base, as the shell used to",
        APP,
        [("    pub fn scrim(&self) -> Color {\n        Color::rgba(0, 0, 0, 140)",
          "    pub fn scrim(&self) -> Color {\n        with_alpha(self.base, 140)")],
        ["appearance"],
        ["the_scrim_and_the_shadows_darken_whichever_palette_they_fall_on"],
    ),
    (
        "K: a label's shadow is no stronger than a panel's",
        APP,
        [("    pub fn text_shadow(&self) -> Color {\n        Color::rgba(0, 0, 0, 180)",
          "    pub fn text_shadow(&self) -> Color {\n        Color::rgba(0, 0, 0, 120)")],
        ["appearance"],
        ["the_scrim_and_the_shadows_darken_whichever_palette_they_fall_on"],
    ),
    (
        "L: hue() answers in dark-mode values whatever mode it is in",
        APP,
        [("        if self.light {\n"
          "            accent.color_light()\n"
          "        } else {\n"
          "            accent.color()\n"
          "        }",
          "        accent.color()")],
        ["appearance"],
        ["every_named_hue_agrees_with_the_accent_of_the_same_name"],
    ),
    (
        "M: text on the accent is the palette's text, not chosen for the accent",
        APP,
        [("    pub fn on_accent(&self) -> Color {\n        readable_on(self.accent)",
          "    pub fn on_accent(&self) -> Color {\n        self.text")],
        ["appearance"],
        ["what_is_drawn_on_the_accent_is_chosen_for_the_accent"],
    ),
    (
        "N: two of the accent washes are the same strength",
        APP,
        [("    pub fn selection_border(&self) -> Color {\n"
          "        with_alpha(self.accent, wash::EDGE)",
          "    pub fn selection_border(&self) -> Color {\n"
          "        with_alpha(self.accent, wash::HINT_EDGE)")],
        ["appearance"],
        ["the_accent_washes_are_the_accent_and_differ_only_in_how_much_shows"],
    ),
    (
        "O: a drop target is the accent, like the selection shown beside it",
        APP,
        [("    pub fn drop_target(&self) -> Color {\n        with_alpha(self.green, 60)",
          "    pub fn drop_target(&self) -> Color {\n        with_alpha(self.accent, 60)")],
        ["appearance"],
        ["the_accent_washes_are_the_accent_and_differ_only_in_how_much_shows"],
    ),
    (
        "P: a focused window's border is the unfocused one",
        APP,
        [("            border_focused: p.surface2,", "            border_focused: p.surface1,")],
        ["appearance", "desktop"],
        ["a_window_frame_is_built_from_the_palette_of_its_own_mode"],
    ),
    (
        "Q: the close button follows the accent",
        APP,
        [("            close_button: p.red,", "            close_button: p.accent,")],
        ["appearance", "desktop"],
        ["a_window_button_keeps_its_meaning_when_the_accent_changes"],
    ),
    (
        "T: SKY carries the transposed byte pair it shipped with",
        APP,
        [("pub const SKY: Color = Color::from_hex(0x89DCEB);",
          "pub const SKY: Color = Color::from_hex(0x89DCFE);")],
        ["appearance"],
        ["every_dark_constant_is_the_published_catppuccin_mocha_value"],
    ),
    (
        # Defect F is this collapse unconditionally. This one happens only in
        # light mode, which the categorical-hue test could not see until it was
        # swept over both — it read `Palette::for_mode(false)` and the default
        # (dark) settings, so the entire light arm was unexercised. The first
        # run of this defect reported NO TEST FAILED; that is what put the
        # sweep in.
        "U: in light mode only, the accent overwrites the categorical sapphire",
        APP,
        [("        palette.panel_alpha = settings.transparency.panel_alpha();",
          "        palette.panel_alpha = settings.transparency.panel_alpha();\n"
          "        if palette.light {\n"
          "            palette.sapphire = palette.accent;\n"
          "        }")],
        ["appearance"],
        ["the_accent_setting_moves_the_accent_and_leaves_the_categorical_hues_alone"],
    ),
    (
        "R: a pressed taskbar button is raised one step too far",
        DESK,
        [("            taskbar_active_bg: p.surface1,", "            taskbar_active_bg: p.surface2,")],
        ["desktop"],
        ["every_surface_of_the_theme_is_a_role_out_of_the_shared_palette"],
    ),
    (
        "S: the start menu keeps a background of its own",
        DESK,
        [("            start_menu_bg: p.base,", "            start_menu_bg: p.mantle,")],
        ["desktop"],
        ["every_surface_of_the_theme_is_a_role_out_of_the_shared_palette"],
    ),
    # --- part 2: a module converted off its own constants ---
    #
    # V, W and X are all the *same* defect — one `const … : Color` that the
    # conversion missed — placed at three different depths, because that is the
    # only failure mode a 549-substitution mechanical edit actually has. What
    # they measure is not whether the sweep can spot a wrong colour (its own
    # unit test does that) but whether the *states the sweep renders* reach the
    # line the constant was left on. A sweep that renders one dialog in one
    # mode would catch V, miss W entirely, and miss X unless it happened to
    # hover the right button.
    (
        "V: the critical risk hue is left as this module's own Mocha red",
        SEC,
        [("            Self::Critical => p.red,",
          "            Self::Critical => guitk::Color::from_hex(0xF38BA8),")],
        ["desktop"],
        ["every_colour_the_dialog_draws_comes_from_its_palette"],
    ),
    (
        # Behind the details disclosure, so only an expanded render sees it.
        "W: the details panel keeps its own Mocha mantle",
        SEC,
        [("                height: panel_h,\n"
          "                color: p.mantle,",
          "                height: panel_h,\n"
          "                color: guitk::Color::from_hex(0x181825),")],
        ["desktop"],
        ["every_colour_the_dialog_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn while the pointer is over Allow: a state the sweep has to
        # set up deliberately, and the reason it iterates `hovers` at all.
        "X: the hovered Allow button keeps its own Mocha green",
        SEC,
        [("        let allow_bg = if self.hovered_button == Some(ButtonId::Allow) {\n"
          "            p.green",
          "        let allow_bg = if self.hovered_button == Some(ButtonId::Allow) {\n"
          "            guitk::Color::from_hex(0xA6E3A1)")],
        ["desktop"],
        ["every_colour_the_dialog_draws_comes_from_its_palette"],
    ),
    # `run_dialog.rs`, 16 constants. Same three depths, plus the one thing this
    # module has that `security_dialog` did not: a label drawn *on* the accent.
    #
    # Not a defect, and it is important that it is not: this module's INPUT_BG
    # was Mocha `crust` = 0x11111B, and that is also what `readable_on` answers
    # for a light fill, so the sweep must allow it and therefore cannot see a
    # leftover one. It became `p.crust` by reading the code, not by testing it.
    # That is the documented hole (see the harness docstring for the rule, and
    # known-issues.md for the reasoning); a defect asserting it goes uncaught
    # would only encode the hole as if it were a result.
    (
        "Y: the run box's focus border is left as this module's own Mocha blue",
        RUN,
        [("            color: p.accent,\n"
          "            line_width: 1.0,\n"
          "            corner_radii: CornerRadii::all(4.0),",
          "            color: guitk::color::Color::from_hex(0x89B4FA),\n"
          "            line_width: 1.0,\n"
          "            corner_radii: CornerRadii::all(4.0),")],
        ["desktop"],
        ["every_colour_the_dialog_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn when the query matched something.
        "Z: the autocomplete dropdown keeps its own Mocha mantle",
        RUN,
        [("                color: p.mantle,", "                color: guitk::color::Color::from_hex(0x181825),")],
        ["desktop"],
        ["every_colour_the_dialog_draws_comes_from_its_palette"],
    ),
    (
        # The OK button's label. Mocha `base` on a blue fill was fine while
        # every desktop was Mocha; on Latte the accent is pale and the label
        # has to go dark by computation, not by constant.
        "AA: the OK button's label is left as this module's own Mocha base",
        RUN,
        [("        let fg = if primary { p.on_accent() } else { p.text };",
          "        let fg = if primary { guitk::color::Color::from_hex(0x1E1E2E) } else { p.text };")],
        ["desktop"],
        ["every_colour_the_dialog_draws_comes_from_its_palette"],
    ),
    # `icons.rs`, 16 constants + 2 written inline. Its sweep is the first that
    # had to *drive a gesture* to reach a colour at all, so the defects below
    # are placed to measure that specifically: BB is visible on a resting
    # desktop, CC needs a marquee being dragged, DD needs icons mid-drag. A
    # sweep of a still desktop would report green on two of the three.
    (
        "BB: the recycle bin keeps this module's own Mocha red",
        ICON,
        [("            Self::RecycleBin => p.red,",
          "            Self::RecycleBin => Color::from_hex(0xF38BA8),")],
        ["desktop"],
        ["every_colour_the_icon_layer_draws_comes_from_its_palette"],
    ),
    (
        "CC: the rubber-band outline keeps its own hardcoded blue",
        ICON,
        [("                color: p.hint_border(),",
          "                color: Color::rgba(137, 180, 250, 120),")],
        ["desktop"],
        ["every_colour_the_icon_layer_draws_comes_from_its_palette"],
    ),
    (
        # The ghost under a dragged icon. This one was never in the `theme`
        # block — it was written inline at the call site, which is how a
        # hardcoded palette spreads past the place you would think to look.
        "DD: the drag ghost keeps the inline blue it was written with",
        ICON,
        [("                        color: p.hint_fill(),",
          "                        color: Color::rgba(137, 180, 250, 30),")],
        ["desktop"],
        ["every_colour_the_icon_layer_draws_comes_from_its_palette"],
    ),
    (
        # The trap `Palette::on_wallpaper` exists to stop: a converter reaches
        # for the obvious role and a Light desktop gets dark labels under a
        # black shadow.
        #
        # This one went UNCAUGHT on its first run, and the reason is the most
        # useful thing this harness has said about part 2: the membership sweep
        # finds *leftover constants*, and a wrong role is not one. `p.text` is
        # a member of both palettes, so it passes the sweep in light mode
        # exactly as it passes in dark. The sweep is not a proof that a module
        # was converted *correctly* — only that it was converted at all.
        #
        # The answer was not to weaken the defect but to add the assertion the
        # sweep structurally cannot make:
        # `an_icon_label_does_not_change_colour_with_the_mode`, which renders
        # twice and compares the label commands. Any module whose colour must
        # NOT follow the mode needs its own such test; do not assume the sweep
        # covers it.
        "EE: an icon label is converted to `text` instead of `on_wallpaper`",
        ICON,
        [("                color: if icon.selected {\n"
          "                    p.on_wallpaper()\n"
          "                } else {\n"
          "                    p.on_wallpaper_dim()\n"
          "                },",
          "                color: if icon.selected { p.text } else { p.subtext0 },")],
        ["desktop"],
        ["an_icon_label_does_not_change_colour_with_the_mode"],
    ),
    (
        "FF: `on_wallpaper` is made to follow the mode after all",
        APP,
        [("    pub fn on_wallpaper(&self) -> Color {\n        LIGHT_EXTREME",
          "    pub fn on_wallpaper(&self) -> Color {\n        self.text")],
        ["appearance"],
        ["a_label_on_the_wallpaper_does_not_follow_the_mode"],
    ),
    (
        # The pane's own background, drawn on every frame it is open. The
        # cheapest possible miss, and the one a sweep must obviously catch.
        "GG: the notification pane keeps its own Mocha base",
        NOTIF,
        [("            height: screen_height,\n            color: p.base,",
          "            height: screen_height,\n"
          "            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_pane_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn while the pointer is over a card. If `wound_pane`'s
        # `hovered` axis were dropped the sweep would still pass, and this
        # constant would ship.
        "HH: the dismiss button, which only exists on hover, keeps Mocha surface2",
        NOTIF,
        [("                height: DISMISS_BTN_SIZE,\n                color: p.surface2,",
          "                height: DISMISS_BTN_SIZE,\n"
          "                color: Color::from_hex(0x585B70),")],
        ["desktop"],
        ["every_colour_the_pane_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn on the per-app settings page, behind the "Settings" link.
        "II: the per-app enabled pill, behind the settings view, keeps Mocha green",
        NOTIF,
        [("            let pill_bg = if app.enabled { p.green } else { p.surface2 };",
          "            let pill_bg = if app.enabled {\n"
          "                Color::from_hex(0xA6E3A1)\n"
          "            } else {\n"
          "                p.surface2\n"
          "            };")],
        ["desktop"],
        ["every_colour_the_pane_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn when there is nothing to draw. A state matrix that only
        # ever renders a populated pane never reaches this line at all.
        "JJ: the empty-list caption, drawn only when there are no notifications",
        NOTIF,
        [('                text: "No notifications".to_string(),\n'
          "                color: p.overlay0,",
          '                text: "No notifications".to_string(),\n'
          "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_pane_draws_comes_from_its_palette"],
    ),
    (
        # The part-2 lesson from defect EE, applied to this module. A priority
        # painted in the user's accent is a *wrong role*, not a leftover
        # constant: `p.accent` is a member of both palettes, so the two-mode
        # sweep passes it in light exactly as in dark. Only a test that renders
        # with two different accents and compares can see it, which is why
        # `a_notification_priority_does_not_follow_the_accent` exists.
        #
        # Expect this one to be caught by the accent test and NOT by the sweep.
        "KK: an urgent notification is painted in the accent instead of red",
        NOTIF,
        [("            Self::Urgent => p.red,", "            Self::Urgent => p.accent,")],
        ["desktop"],
        ["a_notification_priority_does_not_follow_the_accent"],
    ),
    (
        # The colour that was never a constant. `Color::rgba(243, 139, 168, 30)`
        # is Mocha red's channels written out at the call site, so emptying the
        # block of `const`s at the top of the file would have left it behind.
        # The sweep still names it, because it compares roles on RGB alone and
        # Mocha red is not a member of Latte at any alpha.
        "LL: the driver-problem banner keeps its inline wash of Mocha red",
        DEV,
        [("                color: {\n"
          "                    // A wash of the same red the banner's text is in. Written\n"
          "                    // as `Color::rgba(243, 139, 168, 30)` at this call site\n"
          "                    // before the conversion -- a hardcoded palette value that\n"
          "                    // was never in the block of constants, and so would have\n"
          "                    // survived a survey that only emptied that block.\n"
          "                    let c = p.red;\n"
          "                    Color::rgba(c.r, c.g, c.b, 30)\n"
          "                },",
          "                color: Color::rgba(243, 139, 168, 30),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn on the safe-remove tab, and only when something is
        # removable. Two axes of the state matrix have to line up for the sweep
        # to reach this line at all.
        "MM: the eject button, on one tab and only when there is something to eject",
        DEV,
        [("                    color: p.peach,", "                    color: Color::from_hex(0xFAB387),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # The trap this module's second test exists for. `DriverStatus::Updating`
        # is blue, and blue is the default accent, so `p.accent` looks like the
        # obvious role -- but it is one of five fixed badge states, and following
        # the accent would move it while its four siblings stayed put. A role is
        # a member of both palettes, so the membership sweep cannot see this.
        "NN: the `Updating` driver badge is made to follow the accent",
        DEV,
        [("            Self::Updating => p.blue,", "            Self::Updating => p.accent,")],
        ["desktop"],
        ["a_device_status_does_not_follow_the_accent"],
    ),
    (
        "OO: the rules panel's background goes back to Mocha base",
        RULES,
        [("            color: p.base,", "            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn for a one-shot rule, so this measures the fixture as much
        # as the sweep: a rule set that never sets `one_shot` would leave the
        # badge unrendered and the reintroduced constant unseen.
        "PP: the one-shot badge goes back to Mocha peach",
        RULES,
        [("                    color: p.peach,", "                    color: Color::from_hex(0xFAB387),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # Behind a tab: nothing in the rule-list view draws it.
        "QQ: the editor's Save button goes back to Mocha green",
        RULES,
        [("            color: p.green,", "            color: Color::from_hex(0xA6E3A1),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # The wash, not a named constant -- Mocha green's channels written out
        # at the call site at a fifth alpha. The sweep compares roles on RGB
        # alone, which is exactly what lets it see through the alpha to the
        # wrong hue underneath.
        "RR: the ON badge's wash goes back to a hardcoded Mocha green",
        RULES,
        [("                color: Color::rgba(status_color.r, status_color.g, status_color.b, 51),",
          "                color: Color::rgba(166, 227, 161, 51),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # A wrong *role*, not a leftover constant: `p.accent` is a member of
        # both palettes, so the membership sweep passes this in light mode
        # exactly as in dark. Only the categorical test can see it.
        "SS: the ON status badge is made to follow the accent",
        RULES,
        [('                ("ON", p.green)', '                ("ON", p.accent)')],
        ["desktop"],
        ["a_rule_rows_colours_do_not_follow_the_accent"],
    ),
    (
        # The other direction, and the reason each "does not follow" test
        # carries a negative half: a selection frozen on blue still passes the
        # sweep and still passes the equality assertion, because nothing moved.
        "TT: the selected match-type chip stops following the accent",
        RULES,
        [("                color: if selected { p.accent } else { p.surface0 },",
          "                color: if selected { p.blue } else { p.surface0 },")],
        ["desktop"],
        ["the_editors_save_button_does_not_follow_the_accent"],
    ),
    (
        "UU: the accounts panel's background goes back to Mocha base",
        ACCT,
        [("            color: p.base,", "            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn when a status message is set, so this measures the state
        # matrix too: a fixture that never sets one would leave it unrendered.
        "VV: the status message goes back to Mocha yellow",
        ACCT,
        [("                color: p.yellow,", "                color: Color::from_hex(0xF9E2AF),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # Behind a tab *and* behind an empty activity log -- the branch a
        # populated fixture alone would never reach.
        "WW: the empty activity-log caption goes back to Mocha overlay0",
        ACCT,
        [("                color: p.overlay0,", "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # The last slot of the avatar table, which only an account whose stored
        # index reduces to 6 ever draws. It is the fixture's one-account-per-slot
        # loop that makes this visible at all.
        "XX: the seventh avatar colour goes back to Mocha lavender",
        ACCT,
        [("p.mauve, p.red, p.yellow, p.lavender,",
          "p.mauve, p.red, p.yellow, Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # A wrong *role*: blue is the default accent, so a standard account's
        # badge reads like an obvious accent site. It is not one -- its two
        # siblings do not move, and moving one cell of a categorical row is the
        # bug. Invisible to the membership sweep, which sees a legal role.
        "YY: the Standard account badge is made to follow the accent",
        ACCT,
        [("            Self::Standard => p.blue,", "            Self::Standard => p.accent,")],
        ["desktop"],
        ["a_users_identity_colours_do_not_follow_the_accent"],
    ),
    (
        # The other direction, and the whole reason that test carries an
        # `assert_ne!`: freeze the one thing that should follow the accent and
        # the equality half still passes, certifying a panel that ignores the
        # accent entirely.
        "ZZ: the active tab's label stops following the accent",
        ACCT,
        [("                color: if is_active { p.accent } else { p.subtext0 },",
          "                color: if is_active { p.blue } else { p.subtext0 },")],
        ["desktop"],
        ["a_users_identity_colours_do_not_follow_the_accent"],
    ),
    # --- bluetooth.rs (module 8) -------------------------------------------
    (
        "AAA: the bluetooth panel's background goes back to Mocha base",
        BT,
        [("            color: p.base,", "            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "BBB: the \"n more\" line goes back to Mocha overlay0",
        BT,
        [("                font_size: 10.0,\n                color: p.overlay0,",
          "                font_size: 10.0,\n                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "CCC: the device icon circle goes back to Mocha lavender",
        BT,
        [("            color: p.lavender,", "            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "DDD: a healthy battery goes back to Mocha green",
        BT,
        [("                p.green\n            } else if bat > 20 {",
          "                Color::from_hex(0xA6E3A1)\n            } else if bat > 20 {")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "EEE: a connected device's status is made to follow the accent",
        BT,
        [("            Self::Connected => p.green,", "            Self::Connected => p.accent,")],
        ["desktop"],
        ["a_devices_status_colours_do_not_follow_the_accent"],
    ),
    (
        "FFF: the filled signal bars stop following the accent",
        BT,
        [("            let color = if i < bars { p.accent } else { p.surface1 };",
          "            let color = if i < bars { p.blue } else { p.surface1 };")],
        ["desktop"],
        ["a_devices_status_colours_do_not_follow_the_accent"],
    ),
    (
        "GGG: the idle scan button is made to follow the accent",
        BT,
        [("        let disc_color = if mgr.adapter.discovering {\n            p.peach\n        } else {\n            p.blue\n        };",
          "        let disc_color = if mgr.adapter.discovering {\n            p.peach\n        } else {\n            p.accent\n        };")],
        ["desktop"],
        ["the_scan_button_says_something_different_while_it_is_scanning",
         "a_devices_status_colours_do_not_follow_the_accent"],
    ),
    # --- update_settings.rs (module 9) --------------------------------------
    (
        "HHH: the status banner's background goes back to Mocha mantle",
        UPD,
        [("            color: p.mantle,", "            color: Color::from_hex(0x181825),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "III: the restart warning goes back to Mocha peach",
        UPD,
        [("                color: p.peach,", "                color: Color::from_hex(0xFAB387),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "JJJ: the schedule heading goes back to Mocha lavender",
        UPD,
        [("            color: p.lavender,", "            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "KKK: a failed install's row goes back to Mocha red",
        UPD,
        [("            let color = if entry.success { p.green } else { p.red };",
          "            let color = if entry.success { p.green } else { Color::from_hex(0xF38BA8) };")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "LLL: the error status is made to follow the accent",
        UPD,
        [("            Self::Error => p.red,", "            Self::Error => p.accent,")],
        ["desktop"],
        ["an_updates_status_colours_do_not_follow_the_accent"],
    ),
    (
        "MMM: the active tab's label stops following the accent",
        UPD,
        [("                color: if active { p.accent } else { p.subtext0 },",
          "                color: if active { p.blue } else { p.subtext0 },")],
        ["desktop"],
        ["an_updates_status_colours_do_not_follow_the_accent"],
    ),
    # The one that only a *per-site* negative half can see. The active tab
    # label keeps following the accent here, so an assert_ne! over the union of
    # both accent sites would still pass -- which is exactly how bluetooth.rs's
    # first draft missed defect FFF.
    (
        "NNN: the chosen schedule's label stops following the accent",
        UPD,
        [("                color: if active { p.accent } else { p.text },",
          "                color: if active { p.blue } else { p.text },")],
        ["desktop"],
        ["an_updates_status_colours_do_not_follow_the_accent"],
    ),
    (
        "OOO: 'updates available' is given the same colour as 'up to date'",
        UPD,
        [("            Self::Available => p.yellow,", "            Self::Available => p.green,")],
        ["desktop"],
        ["the_update_statuses_stay_distinct_in_both_modes"],
    ),
    # --- storage_settings.rs (module 10) -------------------------------------
    (
        "PPP: the panel background goes back to Mocha base",
        STOR,
        [("            color: p.base,", "            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "QQQ: the breakdown heading goes back to Mocha lavender",
        STOR,
        [("                color: p.lavender,", "                color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "RRR: the low-space warning caption goes back to Mocha red",
        STOR,
        [("                color: p.red,", "                color: Color::from_hex(0xF38BA8),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "SSS: the reclaimable estimate goes back to Mocha green",
        STOR,
        [("                    color: p.green,", "                    color: Color::from_hex(0xA6E3A1),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "TTT: the filesystem caption goes back to Mocha overlay0",
        STOR,
        [("                color: p.overlay0,", "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "UUU: the recycle bin's slice is made to follow the accent",
        STOR,
        [("            Self::Trash => p.red,", "            Self::Trash => p.accent,")],
        ["desktop"],
        ["the_storage_panels_own_colours_do_not_follow_the_accent",
         "the_ten_storage_categories_stay_distinct_in_both_modes"],
    ),
    (
        "VVV: the active tab's label stops following the accent",
        STOR,
        [("                color: if active { p.accent } else { p.subtext0 },",
          "                color: if active { p.blue } else { p.subtext0 },")],
        ["desktop"],
        ["the_storage_panels_own_colours_do_not_follow_the_accent"],
    ),
    # The FFF/NNN shape a third time. The tab label keeps moving with the
    # accent here, so an assert_ne! over the union of the two accent sites
    # would still pass and this would ship unnoticed.
    (
        "WWW: the Change buttons stop following the accent",
        STOR,
        [("                color: p.accent,", "                color: p.blue,")],
        ["desktop"],
        ["the_storage_panels_own_colours_do_not_follow_the_accent"],
    ),
    (
        "XXX: Downloads is given the same slice colour as Media",
        STOR,
        [("            Self::Downloads => p.yellow,", "            Self::Downloads => p.peach,")],
        ["desktop"],
        ["the_ten_storage_categories_stay_distinct_in_both_modes"],
    ),
    # --- power_settings.rs (module 11) ---------------------------------------
    # The labels have run out of three-letter combinations, so they widen to
    # four. `main()` compares the whole prefix, not its first character, so
    # "AAAA" and "A" are distinct selectors and nothing collides.
    (
        "YYY: the power panel keeps its own background",
        POW,
        [(
            "            height: 900.0,\n            color: p.base,",
            "            height: 900.0,\n            color: Color::from_hex(0x1E1E2E),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    # Reachable only through the ChargeState::NotPresent arm of the sweep: with
    # a battery present the summary bar is drawn and the battery tab has a body,
    # so this caption is never emitted. It is the reason NotPresent is walked as
    # its own case rather than as one more charge level.
    (
        "ZZZ: the no-battery caption keeps its own grey",
        POW,
        [("                color: p.overlay0,", "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "AAAA: the charge bar's track keeps its own grey",
        POW,
        [(
            "                height: 6.0,\n                color: p.surface1,",
            "                height: 6.0,\n                color: Color::from_hex(0x45475A),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "BBBB: the key column of every settings row keeps its own grey",
        POW,
        [(
            "            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n"
            "            max_width: Some(width * 0.65),",
            "            color: Color::from_hex(0xA6ADC8),\n"
            "            font_weight: FontWeightHint::Regular,\n"
            "            max_width: Some(width * 0.65),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    # A wrong *role* rather than a leftover constant: peach is in both palettes,
    # so the sweep is blind to it. Only varying the accent, and only checking
    # that the rungs stay apart, can see it.
    (
        "CCCC: a battery in Poor health is repainted the user's accent",
        POW,
        [("            Self::Poor => p.peach,", "            Self::Poor => p.accent,")],
        ["desktop"],
        [
            "the_power_panels_own_colours_do_not_follow_the_accent",
            "the_battery_ladders_stay_distinct_in_both_modes",
        ],
    ),
    (
        "DDDD: the active tab's label stops following the accent",
        POW,
        [(
            "                color: if active { p.accent } else { p.subtext0 },",
            "                color: if active { p.blue } else { p.subtext0 },",
        )],
        ["desktop"],
        ["the_power_panels_own_colours_do_not_follow_the_accent"],
    ),
    # The FFF/NNN/WWW shape a fourth time, and the cleanest instance of it yet:
    # the tab label above still moves with the accent, so an assert_ne! over the
    # union of this panel's two accent sites would pass with the plan list
    # frozen. Only one assertion per site sees it.
    (
        "EEEE: the selected plan's label stops following the accent",
        POW,
        [(
            "                color: if active { p.accent } else { p.text },",
            "                color: if active { p.blue } else { p.text },",
        )],
        ["desktop"],
        ["the_power_panels_own_colours_do_not_follow_the_accent"],
    ),
    (
        "FFFF: the charge bar's middle band is repainted the user's accent",
        POW,
        [("            21..=50 => p.yellow,", "            21..=50 => p.accent,")],
        ["desktop"],
        [
            "the_power_panels_own_colours_do_not_follow_the_accent",
            "the_battery_ladders_stay_distinct_in_both_modes",
        ],
    ),
    # Neither an accent bug nor a leftover constant: two rungs of the ladder
    # given the same legal role. The bar still draws a colour from the palette
    # and still ignores the accent -- it has simply stopped reporting anything,
    # because a battery at 15% now looks exactly like one at 80%.
    (
        "GGGG: two rungs of the charge ladder collapse onto one colour",
        POW,
        [("            11..=20 => p.peach,", "            11..=20 => p.green,")],
        ["desktop"],
        ["the_battery_ladders_stay_distinct_in_both_modes"],
    ),
    # Not a palette defect at all, and included because the palette conversion
    # is what put a reader in front of this code. The charge bar really did push
    # fill, track, then the same fill again; the track is opaque and covers the
    # fill exactly, so the first of the three could not be seen by anyone. A
    # hidden draw is invisible in a screenshot too, so nothing but a test on the
    # command list can find it.
    (
        "HHHH: the charge bar draws its fill under its own opaque track",
        POW,
        [(
            "        if bar_w < track_w {\n            cmds.push(RenderCommand::FillRect {",
            "        cmds.push(RenderCommand::FillRect {\n"
            "            x: x + 8.0,\n"
            "            y: y + 28.0,\n"
            "            width: bar_w,\n"
            "            height: 6.0,\n"
            "            color: charge_color,\n"
            "            corner_radii: CornerRadii::all(3.0),\n"
            "        });\n"
            "        if bar_w < track_w {\n            cmds.push(RenderCommand::FillRect {",
        )],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    # The other way to draw nothing, and the one the coverage rule is blind to:
    # an empty battery pushing a zero-width fill that no later command covers.
    (
        "IIII: an empty battery still pushes a zero-width charge fill",
        POW,
        [("        if bar_w > 0.0 {", "        if bar_w >= 0.0 {")],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    # --- network_indicator.rs (module 12) ------------------------------------
    (
        "JJJJ: the flyout keeps its own background",
        NET,
        [(
            "            width,\n            height,\n            color: p.base,",
            "            width,\n            height,\n            color: Color::from_hex(0x1E1E2E),",
        )],
        ["desktop"],
        ["every_colour_either_render_draws_comes_from_its_palette"],
    ),
    (
        "KKKK: an unremarkable network's row keeps its own background",
        NET,
        [("                    p.mantle", "                    Color::from_hex(0x181825)")],
        ["desktop"],
        ["every_colour_either_render_draws_comes_from_its_palette"],
    ),
    (
        "LLLL: the transfer-rate caption keeps its own grey",
        NET,
        [(
            "            font_size: 11.0,\n            color: p.subtext0,",
            "            font_size: 11.0,\n            color: Color::from_hex(0xA6ADC8),",
        )],
        ["desktop"],
        ["every_colour_either_render_draws_comes_from_its_palette"],
    ),
    (
        "MMMM: the tray icon's disc keeps its own grey",
        NET,
        [(
            "            height: 24.0,\n            color: p.surface0,",
            "            height: 24.0,\n            color: Color::from_hex(0x313244),",
        )],
        ["desktop"],
        ["every_colour_either_render_draws_comes_from_its_palette"],
    ),
    # The blue-state trap, made concrete. `Excellent => p.accent` is exactly
    # what a careless conversion writes, because the rung *is* blue and blue is
    # the default accent -- so it looks right until the user picks Green, at
    # which point a full-strength signal and a merely-good one become the same
    # swatch, side by side in the same list.
    (
        "NNNN: the strongest signal is repainted the user's accent",
        NET,
        [("            Self::Excellent => p.blue,", "            Self::Excellent => p.accent,")],
        ["desktop"],
        [
            "only_the_network_you_are_on_follows_the_accent",
            "signal_strength_stays_a_ladder_under_every_accent",
        ],
    ),
    # The one accent site frozen. Nothing else on this panel follows the accent,
    # so without the negative half of the test every other assertion in it would
    # still pass -- vacuously, on an indicator that ignored the accent entirely.
    (
        "OOOO: the network you are on stops following the accent",
        NET,
        [(
            "color: if net.connected { p.accent } else { p.text },",
            "color: if net.connected { p.blue } else { p.text },",
        )],
        ["desktop"],
        ["only_the_network_you_are_on_follows_the_accent"],
    ),
    (
        "PPPP: airplane mode is repainted the user's accent",
        NET,
        [("            p.peach\n        } else {", "            p.accent\n        } else {")],
        ["desktop"],
        ["only_the_network_you_are_on_follows_the_accent"],
    ),
    # The other half of the same shape, and the reason the loop walks two
    # booleans rather than one: the radio-on green is only on screen when the
    # radio is on, exactly as the airplane peach is only on screen in airplane
    # mode. Each needs its own state or it is compared with its own opposite.
    (
        "SSSS: an enabled radio is repainted the user's accent",
        NET,
        [(
            "let wifi_color = if self.wifi_enabled {\n            p.green",
            "let wifi_color = if self.wifi_enabled {\n            p.accent",
        )],
        ["desktop"],
        ["only_the_network_you_are_on_follows_the_accent"],
    ),
    (
        "QQQQ: a cellular link is drawn exactly like an ethernet one",
        NET,
        [(
            "                ConnectionType::Cellular => p.peach,",
            "                ConnectionType::Cellular => p.green,",
        )],
        ["desktop"],
        ["the_five_kinds_of_link_stay_distinct_in_both_modes"],
    ),
    (
        "RRRR: two rungs of the signal ladder collapse onto one colour",
        NET,
        [("            Self::Good => p.green,", "            Self::Good => p.yellow,")],
        ["desktop"],
        ["signal_strength_stays_a_ladder_under_every_accent"],
    ),
    (
        "TTTT: the popup's own background is left as a Mocha literal",
        CLIP,
        [(
            "            color: p.base,\n            corner_radii: CornerRadii::all(8.0),",
            "            color: Color::from_hex(0x1E1E2E),\n            corner_radii: CornerRadii::all(8.0),",
        )],
        ["desktop"],
        ["every_colour_the_popup_draws_comes_from_its_palette"],
    ),
    (
        "UUUU: the empty-list caption keeps its old grey (only drawn when the "
        "filter matches nothing)",
        CLIP,
        [(
            "                color: p.subtext0,\n                font_size: 12.0,",
            "                color: Color::from_hex(0xA6ADC8),\n                font_size: 12.0,",
        )],
        ["desktop"],
        ["every_colour_the_popup_draws_comes_from_its_palette"],
    ),
    (
        "VVVV: the pin marker keeps its old yellow (only drawn for a pinned entry)",
        CLIP,
        [(
            '                        text: "P".to_string(),\n                        color: p.yellow,',
            '                        text: "P".to_string(),\n                        color: Color::from_hex(0xF9E2AF),',
        )],
        ["desktop"],
        ["every_colour_the_popup_draws_comes_from_its_palette"],
    ),
    (
        "WWWW: the plain-text badge is repainted the user's accent",
        CLIP,
        [("            Self::PlainText => p.blue,", "            Self::PlainText => p.accent,")],
        ["desktop"],
        [
            "only_the_active_filter_tab_follows_the_accent",
            "the_format_badges_stay_distinct_under_every_accent",
        ],
    ),
    (
        "XXXX: the active filter tab is frozen to blue, so it stops tracking the accent",
        CLIP,
        [(
            "            let bg = if is_active { p.accent } else { p.surface0 };",
            "            let bg = if is_active { p.blue } else { p.surface0 };",
        )],
        ["desktop"],
        [
            "only_the_active_filter_tab_follows_the_accent",
            "the_active_filter_tabs_label_is_legible_on_it",
        ],
    ),
    (
        "YYYY: the active tab's label is fixed instead of chosen for its own fill",
        CLIP,
        [(
            "            let fg = if is_active { p.on_accent() } else { p.subtext0 };",
            "            let fg = if is_active { p.base } else { p.subtext0 };",
        )],
        ["desktop"],
        ["the_active_filter_tabs_label_is_legible_on_it"],
    ),
    (
        "ZZZZ: the sensitive marker is repainted the user's accent",
        CLIP,
        [(
            '                        text: "S".to_string(),\n                        color: p.red,',
            '                        text: "S".to_string(),\n                        color: p.accent,',
        )],
        ["desktop"],
        ["only_the_active_filter_tab_follows_the_accent"],
    ),
    (
        "AAAAA: the destructive \"Clear All\" is repainted the user's accent",
        CLIP,
        [(
            '            text: "Clear All".to_string(),\n            color: p.red,',
            '            text: "Clear All".to_string(),\n            color: p.accent,',
        )],
        ["desktop"],
        ["only_the_active_filter_tab_follows_the_accent"],
    ),
    (
        "BBBBB: a faint underlay is pushed beneath the search field, where the "
        "field's own opaque fill erases it",
        CLIP,
        [(
            "        cmds.push(RenderCommand::FillRect {\n"
            "            x: x + 8.0,\n"
            "            y: search_y,\n"
            "            width: w - 16.0,\n"
            "            height: 28.0,\n"
            "            color: search_bg,",
            "        cmds.push(RenderCommand::FillRect {\n"
            "            x: x + 8.0,\n"
            "            y: search_y,\n"
            "            width: w - 16.0,\n"
            "            height: 28.0,\n"
            "            color: p.surface2,\n"
            "            corner_radii: CornerRadii::all(6.0),\n"
            "        });\n"
            "        cmds.push(RenderCommand::FillRect {\n"
            "            x: x + 8.0,\n"
            "            y: search_y,\n"
            "            width: w - 16.0,\n"
            "            height: 28.0,\n"
            "            color: search_bg,",
        )],
        ["desktop"],
        ["the_popup_draws_nothing_that_is_immediately_erased"],
    ),
    (
        "CCCCC: the format badge's wash is emitted with zero width",
        CLIP,
        [(
            "                    y: ey + 6.0,\n                    width: 20.0,\n                    height: 20.0,",
            "                    y: ey + 6.0,\n                    width: 0.0,\n                    height: 20.0,",
        )],
        ["desktop"],
        ["the_popup_draws_nothing_that_is_immediately_erased"],
    ),
    # ---- notification_settings.rs -------------------------------------------
    (
        "DDDDD: the notification panel's own background is left as a Mocha literal",
        NOTIF,
        [(
            "            width,\n            height,\n            color: p.base,",
            "            width,\n            height,\n            color: Color::from_hex(0x1E1E2E),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "EEEEE: the \"no apps\" caption keeps its old grey (only drawn when the "
        "search matches nothing)",
        NOTIF,
        [(
            '                text: "No registered apps".into(),\n'
            "                font_size: 13.0,\n"
            "                color: p.overlay0,",
            '                text: "No registered apps".into(),\n'
            "                font_size: 13.0,\n"
            "                color: Color::from_hex(0x6C7086),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "FFFFF: the High rung of the priority scale keeps its old yellow (only "
        "drawn for a High notification, on the History tab)",
        NOTIF,
        [(
            "            Self::High => p.yellow,",
            "            Self::High => Color::from_hex(0xF9E2AF),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "GGGGG: the history filter badge's caption keeps its old blue (only "
        "drawn while a per-app history filter is set)",
        NOTIF,
        [(
            '                text: format!("Filtered: {}", filter_app),\n'
            "                font_size: 11.0,\n"
            "                color: p.blue,",
            '                text: format!("Filtered: {}", filter_app),\n'
            "                font_size: 11.0,\n"
            "                color: Color::from_hex(0x89B4FA),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "HHHHH: the active tab is frozen to blue, so it stops tracking the accent",
        NOTIF,
        [(
            "                color: if active { p.accent } else { p.surface0 },",
            "                color: if active { p.blue } else { p.surface0 },",
        )],
        ["desktop"],
        [
            "only_the_tab_you_are_on_follows_the_accent",
            "the_active_tabs_label_is_legible_on_it",
        ],
    ),
    (
        "IIIII: the active tab's label is fixed instead of chosen for its own fill",
        NOTIF,
        [(
            "                color: if active { p.on_accent() } else { p.subtext0 },",
            "                color: if active { p.crust } else { p.subtext0 },",
        )],
        ["desktop"],
        ["the_active_tabs_label_is_legible_on_it"],
    ),
    (
        "JJJJJ: the ON/OFF badge's label is fixed, which is legible on Mocha's "
        "pale green by luck and not on Latte's deep green",
        NOTIF,
        [(
            "                color: appearance::readable_on(badge_color),",
            "                color: p.crust,",
        )],
        ["desktop"],
        ["the_on_off_badges_label_is_legible_on_the_badge"],
    ),
    (
        "KKKKK: the Urgent stripe is repainted the user's accent, so a scale "
        "starts meaning selection",
        NOTIF,
        [("            Self::Urgent => p.red,", "            Self::Urgent => p.accent,")],
        ["desktop"],
        [
            "only_the_tab_you_are_on_follows_the_accent",
            "the_priority_scale_stays_distinct_under_every_accent",
        ],
    ),
    (
        "LLLLL: the volume bar's fill is repainted the user's accent, so a "
        "measurement starts meaning selection",
        NOTIF,
        [(
            "                width: fill_w,\n                height: 6.0,\n                color: p.blue,",
            "                width: fill_w,\n                height: 6.0,\n                color: p.accent,",
        )],
        ["desktop"],
        ["only_the_tab_you_are_on_follows_the_accent"],
    ),
    (
        "MMMMM: the volume bar's track is drawn even at full volume, where the "
        "fill covers it exactly",
        NOTIF,
        [("        if fill_w < bar_w {", "        if fill_w <= bar_w {")],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    (
        "NNNNN: the unread dot is emitted with zero height",
        NOTIF,
        [(
            "                    width: 8.0,\n                    height: 8.0,\n                    color: p.blue,",
            "                    width: 8.0,\n                    height: 0.0,\n                    color: p.blue,",
        )],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    (
        "OOOOO: the panel's own background is left as a Mocha literal",
        BACKUP,
        [(
            "            width,\n            height,\n            color: p.base,",
            "            width,\n            height,\n            color: Color::from_hex(0x1E1E2E),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # The membership sweep cannot catch this one and never will: 0x11111B
        # is Mocha's crust *and* one of the two answers readable_on gives, so
        # assert_drawn_from is obliged to allow it. That is what makes this
        # defect worth keeping — it is the proof that the equality test below
        # is load-bearing rather than a restatement of the sweep.
        "PPPPP: the content well behind every tab keeps its old crust literal",
        BACKUP,
        [(
            "            height: content_h,\n            color: p.crust,",
            "            height: content_h,\n            color: Color::from_hex(0x11111B),",
        )],
        ["desktop"],
        ["the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "QQQQQ: an exclusion rule's description keeps its old grey (only drawn "
        "on the exclusions tab)",
        BACKUP,
        [(
            "                text: rule.description.clone(),\n                font_size: 10.0,\n                color: p.subtext0,",
            "                text: rule.description.clone(),\n                font_size: 10.0,\n                color: Color::from_hex(0xA6ADC8),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "RRRRR: a finished run's file/size/duration line keeps its old grey "
        "(only drawn when the history has entries)",
        BACKUP,
        [(
            "                    font_size: 11.0,\n                    color: p.subtext0,\n                    font_weight: FontWeightHint::Regular,\n                    max_width: Some(width - 40.0),",
            "                    font_size: 11.0,\n                    color: Color::from_hex(0xA6ADC8),\n                    font_weight: FontWeightHint::Regular,\n                    max_width: Some(width - 40.0),",
        )],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "SSSSS: a running backup is repainted the user's accent — the "
        "blue-state trap, since blue is also the default accent",
        BACKUP,
        [("            Self::InProgress => p.blue,", "            Self::InProgress => p.accent,")],
        ["desktop"],
        [
            "the_backup_outcomes_stay_distinct_under_every_accent",
            "every_control_that_offers_something_follows_the_accent",
        ],
    ),
    (
        "TTTTT: the active tab's label is frozen to blue, so it stops tracking "
        "the accent",
        BACKUP,
        [(
            "                color: if is_active { p.accent } else { p.subtext0 },",
            "                color: if is_active { p.blue } else { p.subtext0 },",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "UUUUU: the \"Backup now\" button is frozen to blue",
        BACKUP,
        [(
            "            width: 120.0,\n            height: 36.0,\n            color: p.accent,",
            "            width: 120.0,\n            height: 36.0,\n            color: p.blue,",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "VVVVV: the automatic-backup master switch is frozen to blue (the five "
        "retention switches below it still move, so a single assertion over "
        "all six pills would not notice)",
        BACKUP,
        [(
            "        let toggle_bg = if self.settings.enabled {\n            p.accent\n        } else {\n            p.surface2\n        };",
            "        let toggle_bg = if self.settings.enabled {\n            p.blue\n        } else {\n            p.surface2\n        };",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "WWWWW: the five retention switches are frozen to blue (the master "
        "switch above them still moves — the mirror image of VVVVV)",
        BACKUP,
        [(
            "            let toggle_color = if *enabled { p.accent } else { p.surface2 };",
            "            let toggle_color = if *enabled { p.blue } else { p.surface2 };",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "XXXXX: the chosen frequency's radio ring is frozen to blue",
        BACKUP,
        [(
            "                color: if is_active { p.accent } else { p.surface2 },\n                corner_radii: CornerRadii::all(8.0),\n                line_width: 2.0,",
            "                color: if is_active { p.blue } else { p.surface2 },\n                corner_radii: CornerRadii::all(8.0),\n                line_width: 2.0,",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "YYYYY: the chosen frequency's dot is frozen to blue (its ring still "
        "moves)",
        BACKUP,
        [(
            "                    width: 8.0,\n                    height: 8.0,\n                    color: p.accent,",
            "                    width: 8.0,\n                    height: 8.0,\n                    color: p.blue,",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "ZZZZZ: the \"+ Add source\" button is frozen to blue",
        BACKUP,
        [(
            "            width: 100.0,\n            height: 24.0,\n            color: p.accent,",
            "            width: 100.0,\n            height: 24.0,\n            color: p.blue,",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "AAAAAA: a source's checkbox outline is frozen to blue",
        BACKUP,
        [(
            "                color: if source.enabled { p.accent } else { p.surface2 },",
            "                color: if source.enabled { p.blue } else { p.surface2 },",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "BBBBBB: a ticked source's tick is frozen to blue (its box still moves)",
        BACKUP,
        [(
            "                    text: \"\\u{2713}\".to_string(),\n                    font_size: 12.0,\n                    color: p.accent,",
            "                    text: \"\\u{2713}\".to_string(),\n                    font_size: 12.0,\n                    color: p.blue,",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "CCCCCC: the \"+ Add rule\" button is frozen to blue",
        BACKUP,
        [(
            "            width: 80.0,\n            height: 24.0,\n            color: p.accent,",
            "            width: 80.0,\n            height: 24.0,\n            color: p.blue,",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "DDDDDD: an exclusion rule's switch is frozen to blue",
        BACKUP,
        [(
            "            let toggle_bg = if rule.enabled { p.accent } else { p.surface2 };",
            "            let toggle_bg = if rule.enabled { p.blue } else { p.surface2 };",
        )],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "EEEEEE: the two destructive remove crosses are repainted the user's "
        "accent (listed twice because each edit replaces one occurrence, and "
        "repainting one of an identical pair is not a mistake anyone makes)",
        BACKUP,
        [
            (
                "                text: \"\\u{2715}\".to_string(),\n                font_size: 12.0,\n                color: p.red,",
                "                text: \"\\u{2715}\".to_string(),\n                font_size: 12.0,\n                color: p.accent,",
            ),
            (
                "                text: \"\\u{2715}\".to_string(),\n                font_size: 12.0,\n                color: p.red,",
                "                text: \"\\u{2715}\".to_string(),\n                font_size: 12.0,\n                color: p.accent,",
            ),
        ],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "FFFFFF: the \"Backup now\" label is fixed to crust instead of being "
        "chosen for its own fill — right in dark mode, unreadable in light",
        BACKUP,
        [(
            "            text: \"Backup now\".to_string(),\n            font_size: 13.0,\n            color: p.on_accent(),",
            "            text: \"Backup now\".to_string(),\n            font_size: 13.0,\n            color: p.crust,",
        )],
        ["desktop"],
        ["each_buttons_label_is_legible_on_it"],
    ),
    (
        "GGGGGG: the \"+ Add rule\" label is fixed instead of chosen for its "
        "own fill",
        BACKUP,
        [(
            "            text: \"+ Add rule\".to_string(),\n            font_size: 11.0,\n            color: p.on_accent(),",
            "            text: \"+ Add rule\".to_string(),\n            font_size: 11.0,\n            color: p.base,",
        )],
        ["desktop"],
        ["each_buttons_label_is_legible_on_it"],
    ),
    (
        "HHHHHH: a disabled exclusion rule's row is washed over the opaque row "
        "beneath it at full alpha, erasing it",
        BACKUP,
        [(
            "                Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)\n            };\n\n            cmds.push(RenderCommand::FillRect {\n                x,\n                y: row_y,\n                width,\n                height: 44.0,",
            "                Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)\n            };\n\n            cmds.push(RenderCommand::FillRect {\n                x,\n                y: row_y,\n                width,\n                height: 0.0,",
        )],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    (
        "IIIIII: a cancelled run and a running one collapse onto the same grey",
        BACKUP,
        [("            Self::Cancelled => p.overlay0,", "            Self::Cancelled => p.blue,")],
        ["desktop"],
        ["the_backup_outcomes_stay_distinct_under_every_accent"],
    ),
]


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
    files = sorted({d[1] for d in DEFECTS})
    snap = {f: (ROOT / f).read_bytes() for f in files}
    digest = {f: hashlib.sha256(b).hexdigest() for f, b in snap.items()}
    print("snapshot:")
    for f in files:
        print(f"  {digest[f][:16]}  {f}")
    print()

    only = sys.argv[1:]
    verdicts = []
    try:
        for name, path, edits, pkgs, expect in DEFECTS:
            # Split on the colon rather than taking `name[0]`: the labels ran
            # past Z, so `"AA"[0]` would select defect A as well.
            if only and name.split(":", 1)[0] not in only:
                continue
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
                if missing:
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


if __name__ == "__main__":
    main()
