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
NOTIF_PANE = "gui/desktop/src/notif_pane.rs"
DEV = "gui/desktop/src/device_settings.rs"
RULES = "gui/desktop/src/window_rules.rs"
ACCT = "gui/desktop/src/user_accounts.rs"
BT = "gui/desktop/src/bluetooth.rs"
UPD = "gui/desktop/src/update_settings.rs"
STOR = "gui/desktop/src/storage_settings.rs"
POW = "gui/desktop/src/power_settings.rs"
NET = "gui/desktop/src/network_indicator.rs"
CLIP = "gui/desktop/src/clipboard_viewer.rs"
NOTIF_SET = "gui/desktop/src/notification_settings.rs"
BACKUP = "gui/desktop/src/backup_settings.rs"
NET_SET = "gui/desktop/src/network_settings.rs"
STARTUP = "gui/desktop/src/startup_settings.rs"
DTS = "gui/desktop/src/datetime_settings.rs"
TPAD = "gui/desktop/src/touchpad.rs"
OV = "gui/desktop/src/overview.rs"
CTX = "gui/desktop/src/context_ext.rs"
WID = "gui/desktop/src/widgets.rs"
SND = "gui/desktop/src/sound_settings.rs"
OSD = "gui/desktop/src/osd.rs"
PRIV = "gui/desktop/src/privacy_settings.rs"
PRINTMGR = "gui/desktop/src/print_manager.rs"

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
        NOTIF_PANE,
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
        NOTIF_PANE,
        [("                height: DISMISS_BTN_SIZE,\n                color: p.surface2,",
          "                height: DISMISS_BTN_SIZE,\n"
          "                color: Color::from_hex(0x585B70),")],
        ["desktop"],
        ["every_colour_the_pane_draws_comes_from_its_palette"],
    ),
    (
        # Only drawn on the per-app settings page, behind the "Settings" link.
        "II: the per-app enabled pill, behind the settings view, keeps Mocha green",
        NOTIF_PANE,
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
        NOTIF_PANE,
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
        NOTIF_PANE,
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
        [("            height: 36.0,\n            color: p.mantle,",
          "            height: 36.0,\n            color: Color::from_hex(0x181825),")],
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
        [("            text: \"Update schedule\".into(),\n"
          "            font_size: 14.0,\n            color: p.lavender,",
          "            text: \"Update schedule\".into(),\n"
          "            font_size: 14.0,\n            color: Color::from_hex(0xB4BEFE),")],
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
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
        NOTIF_SET,
        [("        if fill_w < bar_w {", "        if fill_w <= bar_w {")],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    (
        "NNNNN: the unread dot is emitted with zero height",
        NOTIF_SET,
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
    # ---- module 16: network_settings.rs -------------------------------------
    #
    # Fourteen constants, eight accent sites, five categorical scales and two
    # pre-existing layout bugs that the conversion happened to expose. The
    # accent sites are listed one per defect rather than as a group because a
    # single `assert_ne!` over their union proves only that *at least one*
    # moved: n sites need n negative assertions, and n defects to prove them.
    (
        "JJJJJJ: the panel's own backdrop keeps Mocha base",
        NET_SET,
        [("            height,\n            color: p.base,",
          "            height,\n            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        # The structural hole in the two-mode membership sweep, made concrete.
        # `assert_drawn_from` must allow 0x11111B at any alpha, because that is
        # one of the two colours `readable_on` can return and a label drawn on
        # a pale accent legitimately is it. But 0x11111B is *also* Mocha crust,
        # so a content well reverted to the literal produces a render the sweep
        # is obliged to accept — in light mode as much as in dark.
        #
        # Expect this caught ONLY by the surfaces test, which asks the stronger
        # question: not "is this colour in the palette" but "is it the role it
        # is supposed to be", in both modes.
        "KKKKKK: the tab content well keeps Mocha crust, which the membership "
        "sweep is structurally unable to see",
        NET_SET,
        [("            height: content_h,\n            color: p.crust,",
          "            height: content_h,\n"
          "            color: Color::from_hex(0x11111B),")],
        ["desktop"],
        ["the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        # Reachable only with Wi-Fi switched off *and* no networks listed. A
        # state matrix that renders one populated Wi-Fi tab never touches it.
        "LLLLLL: the \"Wi-Fi is disabled\" caption keeps Mocha overlay0",
        NET_SET,
        [("                    \"Wi-Fi is disabled\".to_string()\n"
          "                },\n"
          "                font_size: 12.0,\n                color: p.overlay0,",
          "                    \"Wi-Fi is disabled\".to_string()\n"
          "                },\n"
          "                font_size: 12.0,\n"
          "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "MMMMMM: the \"No Ethernet interfaces detected\" caption keeps Mocha "
        "overlay0",
        NET_SET,
        [("                text: \"No Ethernet interfaces detected\".to_string(),\n"
          "                font_size: 14.0,\n                color: p.overlay0,",
          "                text: \"No Ethernet interfaces detected\".to_string(),\n"
          "                font_size: 14.0,\n"
          "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "NNNNNN: the \"None configured\" search-domain caption keeps Mocha "
        "overlay0",
        NET_SET,
        [("                text: \"None configured\".to_string(),\n"
          "                font_size: 11.0,\n                color: p.overlay0,",
          "                text: \"None configured\".to_string(),\n"
          "                font_size: 11.0,\n"
          "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "OOOOOO: the empty-firewall caption keeps Mocha overlay0",
        NET_SET,
        [("                text: \"No custom rules. Using default policies.\".to_string(),\n"
          "                font_size: 12.0,\n                color: p.overlay0,",
          "                text: \"No custom rules. Using default policies.\".to_string(),\n"
          "                font_size: 12.0,\n"
          "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        # The colour that never was a constant: Mocha surface0's three channels
        # written out at the call site behind an alpha. Emptying the block of
        # `const`s at the top of the file would have walked straight past it.
        "PPPPPP: the disabled-rule wash is Mocha surface0's channels typed out "
        "by hand",
        NET_SET,
        [("                    Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)",
          "                    Color::rgba(49, 50, 68, 128)")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    # ---- the eight accent sites, one defect each ----------------------------
    (
        "QQQQQQ: the active tab's label is frozen blue instead of the accent",
        NET_SET,
        [("                font_size: 13.0,\n"
          "                color: if is_active { p.accent } else { p.subtext0 },",
          "                font_size: 13.0,\n"
          "                color: if is_active { p.blue } else { p.subtext0 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "RRRRRR: the status tab's four quick toggles are frozen blue",
        NET_SET,
        [("            let toggle_x = x + width - 56.0;\n"
          "            let toggle_bg = if *enabled { p.accent } else { p.surface2 };",
          "            let toggle_x = x + width - 56.0;\n"
          "            let toggle_bg = if *enabled { p.blue } else { p.surface2 };")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "SSSSSS: the DNS mode picker's active segment is frozen blue",
        NET_SET,
        [("                width: bw,\n                height: 32.0,\n"
          "                color: if is_active { p.accent } else { p.surface0 },",
          "                width: bw,\n                height: 32.0,\n"
          "                color: if is_active { p.blue } else { p.surface0 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "TTTTTT: the DNS-over-HTTPS toggle is frozen blue",
        NET_SET,
        [("        let doh_toggle_bg = if self.settings.dns.dns_over_https {\n"
          "            p.accent",
          "        let doh_toggle_bg = if self.settings.dns.dns_over_https {\n"
          "            p.blue")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "UUUUUU: the proxy type picker's active segment is frozen blue",
        NET_SET,
        [("                width: btn_w,\n                height: 32.0,\n"
          "                color: if is_active { p.accent } else { p.surface0 },",
          "                width: btn_w,\n                height: 32.0,\n"
          "                color: if is_active { p.blue } else { p.surface0 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "VVVVVV: the proxy authentication toggle is frozen blue",
        NET_SET,
        [("                let auth_bg = if self.settings.proxy.requires_auth {\n"
          "                    p.accent",
          "                let auth_bg = if self.settings.proxy.requires_auth {\n"
          "                    p.blue")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "WWWWWW: the three firewall option toggles are frozen blue",
        NET_SET,
        [("            let toggle_bg = if *enabled { p.accent } else { p.surface2 };\n"
          "            cmds.push(RenderCommand::FillRect {\n"
          "                x: x + width - 56.0,",
          "            let toggle_bg = if *enabled { p.blue } else { p.surface2 };\n"
          "            cmds.push(RenderCommand::FillRect {\n"
          "                x: x + width - 56.0,")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "XXXXXX: the \"+ Add rule\" button is frozen blue",
        NET_SET,
        [("            height: 24.0,\n            color: p.accent,",
          "            height: 24.0,\n            color: p.blue,")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    # ---- the four labels chosen from the fill beneath them ------------------
    (
        "YYYYYY: the DNS picker's active label is fixed to crust instead of "
        "being chosen for its own fill",
        NET_SET,
        [("                font_size: 13.0,\n"
          "                color: if is_active { p.on_accent() } else { p.text },",
          "                font_size: 13.0,\n"
          "                color: if is_active { p.crust } else { p.text },")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    (
        "ZZZZZZ: the proxy picker's active label is fixed to crust",
        NET_SET,
        [("                font_size: 12.0,\n"
          "                color: if is_active { p.on_accent() } else { p.text },",
          "                font_size: 12.0,\n"
          "                color: if is_active { p.crust } else { p.text },")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    (
        "AAAAAAA: the \"+ Add rule\" label is fixed to crust",
        NET_SET,
        [("            text: \"+ Add rule\".to_string(),\n"
          "            font_size: 11.0,\n            color: p.on_accent(),",
          "            text: \"+ Add rule\".to_string(),\n"
          "            font_size: 11.0,\n            color: p.crust,")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    (
        # The one that is right by *coincidence* and so looks like nothing.
        # A firewall action badge is a categorical fill, not the accent, and
        # `p.crust` stays legible on all six of its values across the two
        # palettes purely because Mocha's green/red/yellow are pale (dark text
        # reads) while Latte's are deep (light text reads) and crust flips with
        # the mode alongside them. Nothing enforces that; it is an accident of
        # which two palettes we happen to ship. `readable_on(fill)` is what
        # turns it into a property, and this defect is what proves the test
        # asks for the property rather than for the accident.
        "BBBBBBB: a firewall action badge's label is fixed to crust rather "
        "than chosen for the categorical fill under it",
        NET_SET,
        [("                    color: readable_on(rule.action.color(p)),",
          "                    color: p.crust,")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    # ---- the five categorical scales ---------------------------------------
    (
        "CCCCCCC: a connecting interface is painted the user's accent instead "
        "of yellow",
        NET_SET,
        [("            Self::Connecting => p.yellow,", "            Self::Connecting => p.accent,")],
        ["desktop"],
        ["no_category_follows_the_accent"],
    ),
    (
        "DDDDDDD: a good signal is painted the user's accent",
        NET_SET,
        [("            Self::Good => p.yellow,", "            Self::Good => p.accent,")],
        ["desktop"],
        ["no_category_follows_the_accent"],
    ),
    (
        "EEEEEEE: connecting and connected collapse onto the same green",
        NET_SET,
        [("            Self::Connecting => p.yellow,", "            Self::Connecting => p.green,")],
        ["desktop"],
        ["every_category_stays_distinct_under_every_accent"],
    ),
    (
        "FFFFFFF: two rungs of the Wi-Fi security ladder collapse onto peach",
        NET_SET,
        [("            2 => p.yellow,", "            2 => p.peach,")],
        ["desktop"],
        ["every_category_stays_distinct_under_every_accent"],
    ),
    (
        "GGGGGGG: a firewall Ask rule is coloured as if it were a Block rule",
        NET_SET,
        [("            Self::Ask => p.yellow,", "            Self::Ask => p.red,")],
        ["desktop"],
        ["every_category_stays_distinct_under_every_accent"],
    ),
    # ---- the two layout bugs the conversion exposed -------------------------
    (
        # The bug as it actually shipped: the DNS picker advanced no x at all,
        # so "Automatic" was painted at the same rect as "Manual" and covered
        # outright. The option existed, was never visible, and could not be
        # picked.
        "HHHHHHH: the DNS picker draws both of its segments at the same x",
        NET_SET,
        [("            let (bx, bw) = segment_bounds(x, width, i, modes.len());",
          "            let (bx, bw) = segment_bounds(x, width, 0, modes.len());")],
        ["desktop"],
        ["no_picker_segment_hides_another_or_leaves_the_row",
         "the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    (
        # The other direction of the same bug: sized as if there were no gaps,
        # so `n` segments plus `n - 1` gaps run past the row's right edge —
        # invisibly at two segments, by twenty pixels at six.
        "IIIIIII: segment_bounds sizes segments without taking the gaps out of "
        "the total first",
        NET_SET,
        [("    let seg_w = (width - SEGMENT_GAP * (n_f - 1.0)) / n_f;",
          "    let seg_w = width / n_f;")],
        ["desktop"],
        ["no_picker_segment_hides_another_or_leaves_the_row"],
    ),
    (
        # Also as it shipped: four of ProxyType's six variants were offered, so
        # a user on Https or Socks4 saw a picker with nothing selected and no
        # way back to where they were.
        "JJJJJJJ: the proxy picker offers four of ProxyType's six variants",
        NET_SET,
        [("        let types = [\n"
          "            ProxyType::None,\n"
          "            ProxyType::Http,\n"
          "            ProxyType::Https,\n"
          "            ProxyType::Socks4,\n"
          "            ProxyType::Socks5,\n"
          "            ProxyType::Auto,\n"
          "        ];",
          "        let types = [\n"
          "            ProxyType::None,\n"
          "            ProxyType::Http,\n"
          "            ProxyType::Socks5,\n"
          "            ProxyType::Auto,\n"
          "        ];")],
        ["desktop"],
        ["no_picker_segment_hides_another_or_leaves_the_row"],
    ),
    (
        "KKKKKKK: a disabled firewall rule's row is washed at full alpha, "
        "erasing the row beneath it instead of dimming it",
        NET_SET,
        [("                    Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 128)",
          "                    Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 255)")],
        ["desktop"],
        ["a_disabled_rule_is_the_enabled_row_made_translucent"],
    ),

    # ---- startup_settings.rs (module 17) -----------------------------------
    (
        "LLLLLLL: the startup panel's backdrop keeps Mocha's base",
        STARTUP,
        [("            width,\n            height,\n            color: p.base,",
          "            width,\n            height,\n            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "MMMMMMM: the filter field keeps Mocha's surface0",
        STARTUP,
        [("            height: 30.0,\n            color: p.surface0,",
          "            height: 30.0,\n            color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "NNNNNNN: a selected entry's row keeps Mocha's surface1",
        STARTUP,
        [("                color: if is_selected { p.surface1 } else { p.surface0 },",
          "                color: if is_selected { Color::from_hex(0x45475A) } else { p.surface0 },")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "OOOOOOO: the last-boot-time card keeps Mocha's surface0",
        STARTUP,
        [("                height: 48.0,\n                color: p.surface0,",
          "                height: 48.0,\n                color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "PPPPPPP: the sort indicator keeps Mocha's overlay0",
        STARTUP,
        [("            font_size: 11.0,\n            color: p.overlay0,",
          "            font_size: 11.0,\n            color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "QQQQQQQ: the empty-list caption keeps Mocha's overlay0",
        STARTUP,
        [('                text: "No startup apps".into(),\n'
          "                font_size: 13.0,\n                color: p.overlay0,",
          '                text: "No startup apps".into(),\n'
          "                font_size: 13.0,\n                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "RRRRRRR: the filter field's placeholder keeps Mocha's overlay0",
        STARTUP,
        [("            color: if self.filter.is_empty() {\n                p.overlay0",
          "            color: if self.filter.is_empty() {\n"
          "                Color::from_hex(0x6C7086)")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "SSSSSSS: a disabled entry's name keeps Mocha's overlay0",
        STARTUP,
        [("                color: if entry.enabled { p.text } else { p.overlay0 },",
          "                color: if entry.enabled { p.text } else { Color::from_hex(0x6C7086) },")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "TTTTTTT: a delayed entry's delay line keeps Mocha's overlay0",
        STARTUP,
        [("                    font_size: 10.0,\n                    color: p.overlay0,",
          "                    font_size: 10.0,\n"
          "                    color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "UUUUUUU: the enable switch's off arm keeps Mocha's surface2",
        STARTUP,
        [("            let toggle_color = if entry.enabled { p.accent } else { p.surface2 };",
          "            let toggle_color = if entry.enabled { p.accent } else { Color::from_hex(0x585B70) };")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "VVVVVVV: the switch knob keeps Mocha's text",
        STARTUP,
        [("                color: p.text,\n                corner_radii: CornerRadii::all(8.0),",
          "                color: Color::from_hex(0xCDD6F4),\n"
          "                corner_radii: CornerRadii::all(8.0),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "WWWWWWW: the boot tab's heading keeps Mocha's lavender",
        STARTUP,
        [('            text: "Boot Performance".into(),\n'
          "            font_size: 15.0,\n            color: p.lavender,",
          '            text: "Boot Performance".into(),\n'
          "            font_size: 15.0,\n            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "XXXXXXX: an entry's publisher line keeps Mocha's subtext0",
        STARTUP,
        [('                text: format!("{} - {}", entry.publisher, entry.startup_type.label()),\n'
          "                font_size: 11.0,\n                color: p.subtext0,",
          '                text: format!("{} - {}", entry.publisher, entry.startup_type.label()),\n'
          "                font_size: 11.0,\n                color: Color::from_hex(0xA6ADC8),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "YYYYYYY: the high-impact banner's wash keeps Mocha's red, "
        "destructured by hand so only the light render can see it",
        STARTUP,
        [("                color: Color::rgba(p.red.r, p.red.g, p.red.b, 40),",
          "                color: Color::rgba(243, 139, 168, 40),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_high_impact_warning_is_the_red_it_warns_about"],
    ),
    (
        "ZZZZZZZ: the active tab's pill is a fixed blue, not the accent",
        STARTUP,
        [("                color: if active { p.accent } else { p.surface0 },",
          "                color: if active { p.blue } else { p.surface0 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "AAAAAAAA: an entry's enable switch is a fixed blue, not the accent",
        STARTUP,
        [("            let toggle_color = if entry.enabled { p.accent } else { p.surface2 };",
          "            let toggle_color = if entry.enabled { p.blue } else { p.surface2 };")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "BBBBBBBB: the boot tab's switches are a fixed blue, not the accent",
        STARTUP,
        [("            color: if enabled { p.accent } else { p.surface2 },",
          "            color: if enabled { p.blue } else { p.surface2 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "CCCCCCCC: the active tab's label is a fixed near-black, legible on a "
        "pale accent and not on a dark one",
        STARTUP,
        [("                color: if active { p.on_accent() } else { p.subtext0 },",
          "                color: if active { p.crust } else { p.subtext0 },")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    (
        "DDDDDDDD: the active tab's label is the accent itself, i.e. invisible "
        "on its own pill",
        STARTUP,
        [("                color: if active { p.on_accent() } else { p.subtext0 },",
          "                color: if active { p.accent } else { p.subtext0 },")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "EEEEEEEE: the impact badge's label is a fixed near-black again — the "
        "state the module shipped in, illegible on the grey NotMeasured fill",
        STARTUP,
        [("                color: readable_on(impact_color),",
          "                color: p.crust,")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    (
        "FFFFFFFF: the failure badge's label is a fixed near-black",
        STARTUP,
        [("                    color: readable_on(p.red),",
          "                    color: p.crust,")],
        ["desktop"],
        ["each_label_is_legible_on_the_fill_beneath_it"],
    ),
    (
        "GGGGGGGG: a negligible impact is painted in the accent, so a fact "
        "about the machine follows a choice about the desktop",
        STARTUP,
        [("            Self::None | Self::Low => p.green,",
          "            Self::None | Self::Low => p.accent,")],
        ["desktop"],
        ["no_category_follows_the_accent",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "HHHHHHHH: an unmeasured impact is painted in the accent",
        STARTUP,
        [("            Self::NotMeasured => p.overlay0,",
          "            Self::NotMeasured => p.accent,")],
        ["desktop"],
        ["no_category_follows_the_accent",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "IIIIIIII: medium impact collapses onto the green band, so a slow app "
        "reads as a harmless one",
        STARTUP,
        [("            Self::Medium => p.yellow,",
          "            Self::Medium => p.green,")],
        ["desktop"],
        ["every_category_stays_distinct_under_every_accent"],
    ),
    (
        "JJJJJJJJ: an unmeasured impact collapses onto high impact",
        STARTUP,
        [("            Self::NotMeasured => p.overlay0,",
          "            Self::NotMeasured => p.red,")],
        ["desktop"],
        ["every_category_stays_distinct_under_every_accent"],
    ),
    (
        "KKKKKKKK: None and Low are split into two bands, which is the "
        "'fix' the doc comment exists to forestall",
        STARTUP,
        [("            Self::None | Self::Low => p.green,",
          "            Self::None => p.green,\n            Self::Low => p.teal,")],
        ["desktop"],
        ["the_impact_light_has_fewer_bands_than_the_impact_label"],
    ),
    (
        "LLLLLLLL: the boot-time ladder's first band moves from ten seconds "
        "to one",
        STARTUP,
        [("    if ms < 10_000 {\n        p.green", "    if ms < 1_000 {\n        p.green")],
        ["desktop"],
        ["the_boot_time_bands_are_where_they_say_they_are"],
    ),
    (
        "MMMMMMMM: a bad boot reading is painted in the accent, so a forty-"
        "second boot is green on a green desktop",
        STARTUP,
        [("    } else {\n        p.red\n    }\n}", "    } else {\n        p.accent\n    }\n}")],
        ["desktop"],
        ["no_category_follows_the_accent",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "NNNNNNNN: a slow boot collapses onto a fast one",
        STARTUP,
        [("    } else if ms < 30_000 {\n        p.yellow",
          "    } else if ms < 30_000 {\n        p.green")],
        ["desktop"],
        ["every_category_stays_distinct_under_every_accent"],
    ),
    (
        "OOOOOOOO: the entry list stops advancing, so every row is painted "
        "over by the next one",
        STARTUP,
        [("            cy += 62.0;", "            cy += 0.0;")],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    # ---- datetime_settings.rs (module 18 of 49) ----------------------------
    (
        "AAAAAAAAA: the date & time panel's backdrop keeps Mocha's base",
        DTS,
        [("            color: p.base,", "            color: Color::from_hex(0x1E1E2E),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "BBBBBBBBB: the panel title keeps Mocha's text",
        DTS,
        [("            font_size: 22.0,\n            color: p.text,",
          "            font_size: 22.0,\n            color: Color::from_hex(0xCDD6F4),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "CCCCCCCCC: an unselected tab pill keeps Mocha's surface0",
        DTS,
        [("color: if active { p.accent } else { p.surface0 },",
          "color: if active { p.accent } else { Color::from_hex(0x313244) },")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "DDDDDDDDD: the selected tab pill is pinned to blue again, so the "
        "strip stops following the accent",
        DTS,
        [("color: if active { p.accent } else { p.surface0 },",
          "color: if active { p.blue } else { p.surface0 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent",
         "the_selected_tabs_label_is_legible_on_the_pill_beneath_it"],
    ),
    (
        "EEEEEEEEE: the selected tab's label is a fixed near-black, legible "
        "on a pale accent and gone on a dark one",
        DTS,
        [("color: if active { p.on_accent() } else { p.subtext0 },",
          "color: if active { p.crust } else { p.subtext0 },")],
        ["desktop"],
        ["the_selected_tabs_label_is_legible_on_the_pill_beneath_it"],
    ),
    (
        "FFFFFFFFF: the selected tab's label is painted in the accent it sits "
        "on, so it vanishes into its own pill",
        DTS,
        [("color: if active { p.on_accent() } else { p.subtext0 },",
          "color: if active { p.accent } else { p.subtext0 },")],
        ["desktop"],
        ["the_selected_tabs_label_is_legible_on_the_pill_beneath_it",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "GGGGGGGGG: the clock card keeps Mocha's surface0",
        DTS,
        [("                height: 80.0,\n                color: p.surface0,",
          "                height: 80.0,\n                color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "HHHHHHHHH: the main clock face keeps Mocha's text",
        DTS,
        [("                font_size: 36.0,\n                color: p.text,",
          "                font_size: 36.0,\n                color: Color::from_hex(0xCDD6F4),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "both_clock_faces_are_the_panels_body_text"],
    ),
    (
        "IIIIIIIII: the main clock face follows the accent, so the time reads "
        "as an invitation rather than a measurement",
        DTS,
        [("                font_size: 36.0,\n                color: p.text,",
          "                font_size: 36.0,\n                color: p.accent,")],
        ["desktop"],
        ["both_clock_faces_are_the_panels_body_text",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "JJJJJJJJJ: the zone caption under the clock keeps Mocha's subtext0",
        DTS,
        [("                    font_size: 13.0,\n                    color: p.subtext0,\n"
          "                    font_weight: FontWeightHint::Regular,\n"
          "                    max_width: Some(width),",
          "                    font_size: 13.0,\n"
          "                    color: Color::from_hex(0xA6ADC8),\n"
          "                    font_weight: FontWeightHint::Regular,\n"
          "                    max_width: Some(width),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "KKKKKKKKK: the Taskbar Clock heading keeps Mocha's lavender",
        DTS,
        [('text: "Taskbar Clock".into(),\n            font_size: 15.0,\n'
          "            color: p.lavender,",
          'text: "Taskbar Clock".into(),\n            font_size: 15.0,\n'
          "            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "LLLLLLLLL: the current-zone card keeps Mocha's surface1",
        DTS,
        [("                height: 44.0,\n                color: p.surface1,",
          "                height: 44.0,\n                color: Color::from_hex(0x45475A),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "MMMMMMMMM: the current-zone card's heading keeps Mocha's text",
        DTS,
        [("                font_size: 14.0,\n                color: p.text,\n"
          "                font_weight: FontWeightHint::Bold,\n"
          "                max_width: Some(width - 24.0),",
          "                font_size: 14.0,\n"
          "                color: Color::from_hex(0xCDD6F4),\n"
          "                font_weight: FontWeightHint::Bold,\n"
          "                max_width: Some(width - 24.0),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "NNNNNNNNN: the current-zone card's abbreviation line keeps Mocha's "
        "subtext0",
        DTS,
        [('text: format!("{} — {}", tz.tz_id, tz.abbrev_at(self.current_utc)),\n'
          "                font_size: 11.0,\n                color: p.subtext0,",
          'text: format!("{} — {}", tz.tz_id, tz.abbrev_at(self.current_utc)),\n'
          "                font_size: 11.0,\n"
          "                color: Color::from_hex(0xA6ADC8),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "OOOOOOOOO: the search field keeps Mocha's surface0",
        DTS,
        [("            height: 30.0,\n            color: p.surface0,",
          "            height: 30.0,\n            color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "PPPPPPPPP: the search placeholder keeps Mocha's overlay0",
        DTS,
        [("            color: if self.tz_search.is_empty() {\n                p.overlay0",
          "            color: if self.tz_search.is_empty() {\n"
          "                Color::from_hex(0x6C7086)")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "QQQQQQQQQ: the row under the keyboard cursor keeps Mocha's surface1",
        DTS,
        [("color: if is_selected { p.surface1 } else { p.surface0 },",
          "color: if is_selected { Color::from_hex(0x45475A) } else { p.surface0 },")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette",
         "the_zone_you_are_looking_at_is_not_the_zone_in_force"],
    ),
    (
        "RRRRRRRRR: the keyboard cursor stops raising its row, so the zone "
        "you are looking at is indistinguishable from the rest",
        DTS,
        [("color: if is_selected { p.surface1 } else { p.surface0 },",
          "color: p.surface0,")],
        ["desktop"],
        ["the_panels_own_surfaces_come_from_the_palette",
         "the_zone_you_are_looking_at_is_not_the_zone_in_force"],
    ),
    (
        "SSSSSSSSS: the marker strip beside the zone in force is pinned to "
        "blue again",
        DTS,
        [("                    height: 28.0,\n                    color: p.accent,",
          "                    height: 28.0,\n                    color: p.blue,")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent",
         "the_zone_you_are_looking_at_is_not_the_zone_in_force"],
    ),
    (
        "TTTTTTTTT: the name of the zone in force is pinned to blue again",
        DTS,
        [("color: if is_current { p.accent } else { p.text },",
          "color: if is_current { p.blue } else { p.text },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent",
         "the_zone_you_are_looking_at_is_not_the_zone_in_force"],
    ),
    (
        "UUUUUUUUU: the zone in force stops being named differently at all, "
        "so the panel cannot say which zone the machine is on",
        DTS,
        [("color: if is_current { p.accent } else { p.text },", "color: p.text,")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent",
         "the_zone_you_are_looking_at_is_not_the_zone_in_force"],
    ),
    (
        "VVVVVVVVV: a zone row's offset keeps Mocha's subtext0",
        DTS,
        [("text: tz.offset_string(self.current_utc),\n                font_size: 13.0,\n"
          "                color: p.subtext0,",
          "text: tz.offset_string(self.current_utc),\n                font_size: 13.0,\n"
          "                color: Color::from_hex(0xA6ADC8),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "WWWWWWWWW: the DST badge keeps Mocha's yellow",
        DTS,
        [("                    font_size: 10.0,\n                    color: p.yellow,",
          "                    font_size: 10.0,\n"
          "                    color: Color::from_hex(0xF9E2AF),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_dst_badge_does_not_follow_the_accent"],
    ),
    (
        "XXXXXXXXX: the DST badge follows the accent, so whether a zone's "
        "clock is shifted depends on the desktop's colour",
        DTS,
        [("                    font_size: 10.0,\n                    color: p.yellow,",
          "                    font_size: 10.0,\n                    color: p.accent,")],
        ["desktop"],
        ["the_dst_badge_does_not_follow_the_accent",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "YYYYYYYYY: the zone list stops advancing, so every row is painted "
        "over by the next one",
        DTS,
        [("            cy += 40.0;\n        }\n    }\n\n    fn render_ntp_tab(",
          "            cy += 0.0;\n        }\n    }\n\n    fn render_ntp_tab(")],
        ["desktop"],
        ["the_panel_draws_nothing_that_is_immediately_erased"],
    ),
    (
        "ZZZZZZZZZ: the Time Synchronization heading keeps Mocha's lavender",
        DTS,
        [('text: "Time Synchronization".into(),\n            font_size: 15.0,\n'
          "            color: p.lavender,",
          'text: "Time Synchronization".into(),\n            font_size: 15.0,\n'
          "            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "AAAAAAAAAA: the sync-status card keeps Mocha's surface0",
        DTS,
        [("            height: 36.0,\n            color: p.surface0,",
          "            height: 36.0,\n            color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "BBBBBBBBBB: a disabled clock is reported in the accent, so a fact "
        "about the machine follows a choice about the desktop",
        DTS,
        [("            Self::Disabled => p.overlay0,", "            Self::Disabled => p.accent,")],
        ["desktop"],
        ["no_sync_state_follows_the_accent",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "CCCCCCCCCC: a failed sync is reported in the accent, so a broken "
        "clock is green on a green desktop",
        DTS,
        [("            Self::Error => p.red,", "            Self::Error => p.accent,")],
        ["desktop"],
        ["no_sync_state_follows_the_accent",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "DDDDDDDDDD: syncing collapses onto synchronized, so a clock that is "
        "still trying reads as one that succeeded",
        DTS,
        [("            Self::Syncing => p.yellow,", "            Self::Syncing => p.green,")],
        ["desktop"],
        ["every_sync_state_stays_distinct_under_every_accent"],
    ),
    (
        "EEEEEEEEEE: disabled collapses onto error, so a clock nobody asked "
        "to sync reads as one that failed",
        DTS,
        [("            Self::Disabled => p.overlay0,", "            Self::Disabled => p.red,")],
        ["desktop"],
        ["every_sync_state_stays_distinct_under_every_accent"],
    ),
    (
        "FFFFFFFFFF: the sync dot stops reporting the state, reading the "
        "enabled flag instead",
        DTS,
        [("            color: status_color,", "            color: if ntp.enabled { p.green } else { p.overlay0 },")],
        ["desktop"],
        ["the_sync_dot_reports_the_state_it_is_in"],
    ),
    (
        "GGGGGGGGGG: the sync-status line keeps Mocha's text",
        DTS,
        [('text: format!("Status: {}", ntp.status.label()),\n            font_size: 13.0,\n'
          "            color: p.text,",
          'text: format!("Status: {}", ntp.status.label()),\n            font_size: 13.0,\n'
          "            color: Color::from_hex(0xCDD6F4),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "HHHHHHHHHH: an NTP server row keeps Mocha's surface0",
        DTS,
        [("                height: 28.0,\n                color: p.surface0,",
          "                height: 28.0,\n                color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "IIIIIIIIII: an NTP server's name keeps Mocha's text",
        DTS,
        [("text: server.clone(),\n                font_size: 13.0,\n                color: p.text,",
          "text: server.clone(),\n                font_size: 13.0,\n"
          "                color: Color::from_hex(0xCDD6F4),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "JJJJJJJJJJ: the empty-clocks caption keeps Mocha's overlay0",
        DTS,
        [("                font_size: 13.0,\n                color: p.overlay0,",
          "                font_size: 13.0,\n"
          "                color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "KKKKKKKKKK: a world-clock card keeps Mocha's surface0",
        DTS,
        [("                height: 60.0,\n                color: p.surface0,",
          "                height: 60.0,\n                color: Color::from_hex(0x313244),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette",
         "the_panels_own_surfaces_come_from_the_palette"],
    ),
    (
        "LLLLLLLLLL: a world-clock face goes back to the blue it shipped in, "
        "disagreeing with the main clock face about the same kind of value",
        DTS,
        [("                    font_size: 20.0,\n                    color: p.text,",
          "                    font_size: 20.0,\n                    color: p.blue,")],
        ["desktop"],
        ["both_clock_faces_are_the_panels_body_text"],
    ),
    (
        "MMMMMMMMMM: a world-clock face follows the accent",
        DTS,
        [("                    font_size: 20.0,\n                    color: p.text,",
          "                    font_size: 20.0,\n                    color: p.accent,")],
        ["desktop"],
        ["both_clock_faces_are_the_panels_body_text",
         "every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "NNNNNNNNNN: a world clock's label keeps Mocha's text",
        DTS,
        [("text: clock.label.clone(),\n                font_size: 14.0,\n"
          "                color: p.text,",
          "text: clock.label.clone(),\n                font_size: 14.0,\n"
          "                color: Color::from_hex(0xCDD6F4),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "OOOOOOOOOO: a world clock's zone line keeps Mocha's subtext0",
        DTS,
        [("                    font_size: 11.0,\n                    color: p.subtext0,",
          "                    font_size: 11.0,\n"
          "                    color: Color::from_hex(0xA6ADC8),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "PPPPPPPPPP: the Hidden mark on a world clock keeps Mocha's overlay0",
        DTS,
        [('text: "Hidden".into(),\n                    font_size: 10.0,\n'
          "                    color: p.overlay0,",
          'text: "Hidden".into(),\n                    font_size: 10.0,\n'
          "                    color: Color::from_hex(0x6C7086),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "QQQQQQQQQQ: a toggle row's label keeps Mocha's text",
        DTS,
        [("            color: p.text,\n            font_weight: FontWeightHint::Regular,\n"
          "            max_width: Some(width - 80.0),",
          "            color: Color::from_hex(0xCDD6F4),\n"
          "            font_weight: FontWeightHint::Regular,\n"
          "            max_width: Some(width - 80.0),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "RRRRRRRRRR: the enable switch is pinned to green again, so it stops "
        "following the accent",
        DTS,
        [("            color: if enabled { p.accent } else { p.surface2 },",
          "            color: if enabled { p.green } else { p.surface2 },")],
        ["desktop"],
        ["every_control_that_offers_something_follows_the_accent"],
    ),
    (
        "SSSSSSSSSS: the enable switch's off arm keeps Mocha's surface2",
        DTS,
        [("            color: if enabled { p.accent } else { p.surface2 },",
          "            color: if enabled { p.accent } else { Color::from_hex(0x585B70) },")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "TTTTTTTTTT: the switch knob keeps Mocha's text",
        DTS,
        [("            color: p.text,\n            corner_radii: CornerRadii::all(9.0),",
          "            color: Color::from_hex(0xCDD6F4),\n"
          "            corner_radii: CornerRadii::all(9.0),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "UUUUUUUUUU: a label/value row's label keeps Mocha's subtext0",
        DTS,
        [("            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n"
          "            max_width: Some(width * 0.4),",
          "            color: Color::from_hex(0xA6ADC8),\n"
          "            font_weight: FontWeightHint::Regular,\n"
          "            max_width: Some(width * 0.4),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "VVVVVVVVVV: a label/value row's value keeps Mocha's text",
        DTS,
        [("            color: p.text,\n            font_weight: FontWeightHint::Regular,\n"
          "            max_width: Some(width * 0.55),",
          "            color: Color::from_hex(0xCDD6F4),\n"
          "            font_weight: FontWeightHint::Regular,\n"
          "            max_width: Some(width * 0.55),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "WWWWWWWWWW: the NTP Servers heading keeps Mocha's lavender",
        DTS,
        [('text: "NTP Servers".into(),\n            font_size: 15.0,\n'
          "            color: p.lavender,",
          'text: "NTP Servers".into(),\n            font_size: 15.0,\n'
          "            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "XXXXXXXXXX: the Additional Clocks heading keeps Mocha's lavender",
        DTS,
        [('text: "Additional Clocks".into(),\n            font_size: 15.0,\n'
          "            color: p.lavender,",
          'text: "Additional Clocks".into(),\n            font_size: 15.0,\n'
          "            color: Color::from_hex(0xB4BEFE),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    (
        "YYYYYYYYYY: the clocks-configured count keeps Mocha's subtext0",
        DTS,
        [("            font_size: 12.0,\n            color: p.subtext0,",
          "            font_size: 12.0,\n            color: Color::from_hex(0xA6ADC8),")],
        ["desktop"],
        ["every_colour_the_panel_draws_comes_from_its_palette"],
    ),
    # ---- touchpad.rs (module 19 of 49) -------------------------------------
    (
        "AAAAAAAAAAA: the touchpad panel's backdrop keeps Mocha's base",
        TPAD,
        [
            ('            color: p.base,',
             '            color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "BBBBBBBBBBB: the panel's title bar keeps Mocha's mantle",
        TPAD,
        [
            ('            color: p.mantle,',
             '            color: Color::from_hex(0x181825),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "CCCCCCCCCCC: the panel title keeps Mocha's text",
        TPAD,
        [
            ('            text: "Touchpad & Gestures".to_string(),\n            font_size: 16.0,\n            color: p.text,',
             '            text: "Touchpad & Gestures".to_string(),\n            font_size: 16.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "DDDDDDDDDDD: the attached device's name keeps Mocha's subtext0",
        TPAD,
        [
            ('                text: dev.name.clone(),\n                font_size: 12.0,\n                color: p.subtext0,',
             '                text: dev.name.clone(),\n                font_size: 12.0,\n                color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "EEEEEEEEEEE: an unselected section pill keeps Mocha's surface0",
        TPAD,
        [
            ('color: if active { p.accent } else { p.surface0 },',
             'color: if active { p.accent } else { Color::from_hex(0x313244) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "FFFFFFFFFFF: an unselected section label keeps Mocha's text",
        TPAD,
        [
            ('color: if active { p.on_accent() } else { p.text },',
             'color: if active { p.on_accent() } else { Color::from_hex(0xCDD6F4) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_selected_sections_label_is_legible_on_the_pill_beneath_it',
        ],
    ),
    (
        "GGGGGGGGGGG: the status line keeps Mocha's text",
        TPAD,
        [
            ('            text: format!("Status: {}", status),\n            font_size: 12.0,\n            color: p.text,',
             '            text: format!("Status: {}", status),\n            font_size: 12.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "HHHHHHHHHHH: the gesture section's heading keeps Mocha's text",
        TPAD,
        [
            ('            text: "Multi-finger gestures".to_string(),\n            font_size: 13.0,\n            color: p.text,',
             '            text: "Multi-finger gestures".to_string(),\n            font_size: 13.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "IIIIIIIIIII: the gesture table's Fingers heading keeps Mocha's subtext0",
        TPAD,
        [
            ('            text: "Fingers".to_string(),\n            font_size: 10.0,\n            color: p.subtext0,',
             '            text: "Fingers".to_string(),\n            font_size: 10.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "JJJJJJJJJJJ: the rule under the gesture table's headings keeps Mocha's surface1",
        TPAD,
        [
            ('            x2: x + 400.0,\n            y2: cy,\n            color: p.surface1,',
             '            x2: x + 400.0,\n            y2: cy,\n            color: Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "KKKKKKKKKKK: the cursor behind the selected gesture row keeps Mocha's surface0",
        TPAD,
        [
            ('                    width: 420.0,\n                    height: 22.0,\n                    color: p.surface0,',
             '                    width: 420.0,\n                    height: 22.0,\n                    color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "LLLLLLLLLLL: the gesture-count line keeps Mocha's subtext0",
        TPAD,
        [
            ('            text: counter,\n            font_size: 10.0,\n            color: p.subtext0,',
             '            text: counter,\n            font_size: 10.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "MMMMMMMMMMM: the typing-delay readout keeps Mocha's text",
        TPAD,
        [
            ('            text: format!("Typing delay: {} ms", mgr.config.typing_disable_delay_ms),\n            font_size: 12.0,\n            color: p.text,',
             '            text: format!("Typing delay: {} ms", mgr.config.typing_disable_delay_ms),\n            font_size: 12.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "NNNNNNNNNNN: a toggle's label keeps Mocha's text",
        TPAD,
        [
            ('            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: None,\n            overflow: TextOverflow::Clip,\n        });\n        // Toggle track.',
             '            color: Color::from_hex(0xCDD6F4),\n            font_weight: FontWeightHint::Regular,\n            max_width: None,\n            overflow: TextOverflow::Clip,\n        });\n        // Toggle track.'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "OOOOOOOOOOO: a toggle's knob keeps Mocha's text",
        TPAD,
        [
            ('            width: 14.0,\n            height: 14.0,\n            color: p.text,',
             '            width: 14.0,\n            height: 14.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "PPPPPPPPPPP: a toggle's off arm keeps Mocha's surface2",
        TPAD,
        [
            ('color: if value { p.accent } else { p.surface2 },',
             'color: if value { p.accent } else { Color::from_hex(0x585B70) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "QQQQQQQQQQQ: a slider's label keeps Mocha's text",
        TPAD,
        [
            ('            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: None,\n            overflow: TextOverflow::Clip,\n        });\n        // Slider track.',
             '            color: Color::from_hex(0xCDD6F4),\n            font_weight: FontWeightHint::Regular,\n            max_width: None,\n            overflow: TextOverflow::Clip,\n        });\n        // Slider track.'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "RRRRRRRRRRR: a slider's track keeps Mocha's surface1",
        TPAD,
        [
            ('            width: track_w,\n            height: 4.0,\n            color: p.surface1,',
             '            width: track_w,\n            height: 4.0,\n            color: Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "SSSSSSSSSSS: a slider's value readout keeps Mocha's subtext0",
        TPAD,
        [
            ('            text: format!("{:.1}", value),\n            font_size: 11.0,\n            color: p.subtext0,',
             '            text: format!("{:.1}", value),\n            font_size: 11.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "TTTTTTTTTTT: a choice control's label keeps Mocha's text",
        TPAD,
        [
            ('            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: None,\n            overflow: TextOverflow::Clip,\n        });\n        cmds.push(RenderCommand::FillRect {\n            x: x + 250.0,\n            y,\n            width: 200.0,',
             '            color: Color::from_hex(0xCDD6F4),\n            font_weight: FontWeightHint::Regular,\n            max_width: None,\n            overflow: TextOverflow::Clip,\n        });\n        cmds.push(RenderCommand::FillRect {\n            x: x + 250.0,\n            y,\n            width: 200.0,'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
        ],
    ),
    (
        "UUUUUUUUUUU: a choice control's well keeps Mocha's surface0",
        TPAD,
        [
            ('            width: 200.0,\n            height: 22.0,\n            color: p.surface0,',
             '            width: 200.0,\n            height: 22.0,\n            color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_panels_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "VVVVVVVVVVV: the selected section's pill keeps its hardcoded blue",
        TPAD,
        [
            ('color: if active { p.accent } else { p.surface0 },',
             'color: if active { Color::from_hex(0x89B4FA) } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWW: the selected section's pill is drawn like an unselected one",
        TPAD,
        [
            ('color: if active { p.accent } else { p.surface0 },',
             'color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "XXXXXXXXXXX: a slider's filled portion keeps its hardcoded blue",
        TPAD,
        [
            ('                height: 4.0,\n                color: p.accent,',
             '                height: 4.0,\n                color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "YYYYYYYYYYY: a slider's filled portion is the same surface as its track",
        TPAD,
        [
            ('                height: 4.0,\n                color: p.accent,',
             '                height: 4.0,\n                color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "ZZZZZZZZZZZ: a slider's knob keeps its hardcoded blue",
        TPAD,
        [
            ('            width: 12.0,\n            height: 12.0,\n            color: p.accent,',
             '            width: 12.0,\n            height: 12.0,\n            color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "AAAAAAAAAAAA: a slider's knob follows the body text instead of the accent",
        TPAD,
        [
            ('            width: 12.0,\n            height: 12.0,\n            color: p.accent,',
             '            width: 12.0,\n            height: 12.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "BBBBBBBBBBBB: a toggle's on arm keeps its hardcoded green",
        TPAD,
        [
            ('color: if value { p.accent } else { p.surface2 },',
             'color: if value { Color::from_hex(0xA6E3A1) } else { p.surface2 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "CCCCCCCCCCCC: a toggle's on arm becomes the palette's green rather than the accent",
        TPAD,
        [
            ('color: if value { p.accent } else { p.surface2 },',
             'color: if value { p.green } else { p.surface2 },'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "DDDDDDDDDDDD: the selected section's label is a fixed near-black again",
        TPAD,
        [
            ('color: if active { p.on_accent() } else { p.text },',
             'color: if active { Color::from_hex(0x1E1E2E) } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_selected_sections_label_is_legible_on_the_pill_beneath_it',
        ],
    ),
    (
        "EEEEEEEEEEEE: the selected section's label is painted with the accent it sits on",
        TPAD,
        [
            ('color: if active { p.on_accent() } else { p.text },',
             'color: if active { p.accent } else { p.text },'),
        ],
        ["desktop"],
        [
            'the_selected_sections_label_is_legible_on_the_pill_beneath_it',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        "FFFFFFFFFFFF: a disabled touchpad is reported in the desktop's accent",
        TPAD,
        [
            ('("Disabled", p.red)\n    } else if',
             '("Disabled", p.accent)\n    } else if'),
        ],
        ["desktop"],
        [
            'no_touchpad_state_follows_the_accent',
            'the_status_light_reports_the_state_it_is_in',
        ],
    ),
    (
        "GGGGGGGGGGGG: a paused touchpad is reported in the desktop's accent",
        TPAD,
        [
            ('("Paused (typing)", p.yellow)\n    } else {',
             '("Paused (typing)", p.accent)\n    } else {'),
        ],
        ["desktop"],
        [
            'no_touchpad_state_follows_the_accent',
            'the_status_light_reports_the_state_it_is_in',
        ],
    ),
    (
        'HHHHHHHHHHHH: paused and active are the same rung of the status ladder',
        TPAD,
        [
            ('("Paused (typing)", p.yellow)\n    } else {',
             '("Paused (typing)", p.green)\n    } else {'),
        ],
        ["desktop"],
        [
            'every_touchpad_state_stays_distinct_under_every_accent',
            'the_status_light_reports_the_state_it_is_in',
        ],
    ),
    (
        "IIIIIIIIIIII: a disabled touchpad keeps Mocha's red",
        TPAD,
        [
            ('("Disabled", p.red)\n    } else if',
             '("Disabled", Color::from_hex(0xF38BA8))\n    } else if'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_status_light_reports_the_state_it_is_in',
        ],
    ),
    (
        "JJJJJJJJJJJJ: an active touchpad keeps Mocha's green",
        TPAD,
        [
            ('("Active", p.green)\n    }\n}',
             '("Active", Color::from_hex(0xA6E3A1))\n    }\n}'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_status_light_reports_the_state_it_is_in',
        ],
    ),
    (
        "KKKKKKKKKKKK: the reset button follows the desktop's accent",
        TPAD,
        [
            ('            width: 120.0,\n            height: 28.0,\n            color: p.red,',
             '            width: 120.0,\n            height: 28.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'the_reset_button_does_not_follow_the_accent',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        "LLLLLLLLLLLL: the reset button keeps Mocha's red",
        TPAD,
        [
            ('            width: 120.0,\n            height: 28.0,\n            color: p.red,',
             '            width: 120.0,\n            height: 28.0,\n            color: Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'the_reset_button_does_not_follow_the_accent',
        ],
    ),
    (
        "MMMMMMMMMMMM: the reset button's label is a fixed near-black again",
        TPAD,
        [
            ('            color: readable_on(p.red),',
             '            color: p.base,'),
        ],
        ["desktop"],
        [
            'the_reset_buttons_label_can_be_read_on_the_button',
        ],
    ),
    (
        "NNNNNNNNNNNN: the gesture table's finger count is lavender again",
        TPAD,
        [
            ('                text: format!("{}", g.fingers),\n                font_size: 12.0,\n                color: p.text,',
             '                text: format!("{}", g.fingers),\n                font_size: 12.0,\n                color: p.lavender,'),
        ],
        ["desktop"],
        [
            'a_reported_value_is_the_panels_body_text',
        ],
    ),
    (
        "OOOOOOOOOOOO: the gesture table's action column follows the accent",
        TPAD,
        [
            ('                text: g.action.label(),\n                font_size: 12.0,\n                color: p.text,',
             '                text: g.action.label(),\n                font_size: 12.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'a_reported_value_is_the_panels_body_text',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        "PPPPPPPPPPPP: a choice control's current value follows the accent",
        TPAD,
        [
            ('            text: value.to_string(),\n            font_size: 11.0,\n            color: p.text,',
             '            text: value.to_string(),\n            font_size: 11.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'a_reported_value_is_the_panels_body_text',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        "QQQQQQQQQQQQ: the gesture table's direction column keeps Mocha's text",
        TPAD,
        [
            ('                text: dir_str.to_string(),\n                font_size: 12.0,\n                color: p.text,',
             '                text: dir_str.to_string(),\n                font_size: 12.0,\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'a_reported_value_is_the_panels_body_text',
        ],
    ),
    (
        'RRRRRRRRRRRR: a slider at its floor draws a rectangle that covers no pixels',
        TPAD,
        [
            ('        if fill_w > 0.0 {',
             '        if fill_w >= 0.0 {'),
        ],
        ["desktop"],
        [
            'the_panel_draws_nothing_that_is_immediately_erased',
        ],
    ),
    # ---- overview.rs (module 20 of 49) -------------------------------------
    (
        "AAAAAAAAAAAAA: the search bar keeps Mocha's surface0",
        OV,
        [
            ('        height: bar_h,\n        color: p.surface0,',
             '        height: bar_h,\n        color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'the_overlays_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "BBBBBBBBBBBBB: an idle search bar's border keeps Mocha's surface1",
        OV,
        [
            ('    let border_color = if state.search_query.is_empty() {\n        p.surface1',
             '    let border_color = if state.search_query.is_empty() {\n        Color::from_hex(0x45475A)'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'the_overlays_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "CCCCCCCCCCCCC: the search placeholder keeps Mocha's overlay0",
        OV,
        [
            ('("Search windows...".to_string(), p.overlay0)',
             '("Search windows...".to_string(), Color::from_hex(0x6C7086))'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
        ],
    ),
    (
        "DDDDDDDDDDDDD: the typed query keeps Mocha's text",
        OV,
        [
            ('(state.search_query.clone(), p.text)',
             '(state.search_query.clone(), Color::from_hex(0xCDD6F4))'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
        ],
    ),
    (
        "EEEEEEEEEEEEE: the current desktop's label keeps Mocha's text",
        OV,
        [
            ('color: if lane.is_current { p.text } else { p.subtext0 },',
             'color: if lane.is_current { Color::from_hex(0xCDD6F4) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
        ],
    ),
    (
        "FFFFFFFFFFFFF: another desktop's label keeps Mocha's subtext0",
        OV,
        [
            ('color: if lane.is_current { p.text } else { p.subtext0 },',
             'color: if lane.is_current { p.text } else { Color::from_hex(0xA6ADC8) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
        ],
    ),
    (
        "GGGGGGGGGGGGG: a card's background keeps Mocha's surface0",
        OV,
        [
            ('    } else {\n        p.surface0\n    };',
             '    } else {\n        Color::from_hex(0x313244)\n    };'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'a_wash_keeps_its_own_alpha_and_the_colour_of_its_role',
        ],
    ),
    (
        "HHHHHHHHHHHHH: a dimmed card's title keeps Mocha's overlay0",
        OV,
        [
            ('let title_color = if is_dimmed { p.overlay0 } else { p.text };',
             'let title_color = if is_dimmed { Color::from_hex(0x6C7086) } else { p.text };'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
        ],
    ),
    (
        "IIIIIIIIIIIII: a card's title keeps Mocha's text",
        OV,
        [
            ('let title_color = if is_dimmed { p.overlay0 } else { p.text };',
             'let title_color = if is_dimmed { p.overlay0 } else { Color::from_hex(0xCDD6F4) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
        ],
    ),
    (
        "JJJJJJJJJJJJJ: a plain card's border keeps Mocha's surface2",
        OV,
        [
            ('    } else if layout.is_focused {\n        p.subtext0\n    } else {\n        p.surface2\n    };',
             '    } else if layout.is_focused {\n        p.subtext0\n    } else {\n        Color::from_hex(0x585B70)\n    };'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'a_cards_border_says_both_where_you_point_and_what_has_focus',
        ],
    ),
    (
        "KKKKKKKKKKKKK: the focused card's border is Mocha's lavender again",
        OV,
        [
            ('    } else if layout.is_focused {\n        p.subtext0',
             '    } else if layout.is_focused {\n        Color::from_hex(0xB4BEFE)'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'a_cards_border_says_both_where_you_point_and_what_has_focus',
        ],
    ),
    (
        "LLLLLLLLLLLLL: the minimised badge keeps Mocha's yellow",
        OV,
        [
            ('            height: 16.0,\n            color: p.yellow,',
             '            height: 16.0,\n            color: Color::from_hex(0xF9E2AF),'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'neither_badge_follows_the_accent',
            'each_badges_mark_can_be_read_on_the_badge',
        ],
    ),
    (
        "MMMMMMMMMMMMM: the close button keeps Mocha's red",
        OV,
        [
            ('            height: 18.0,\n            color: p.red,',
             '            height: 18.0,\n            color: Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'neither_badge_follows_the_accent',
            'each_badges_mark_can_be_read_on_the_badge',
        ],
    ),
    (
        "NNNNNNNNNNNNN: the backdrop keeps Mocha's mantle",
        OV,
        [
            ('Color::rgba(p.mantle.r, p.mantle.g, p.mantle.b, alpha)',
             'Color::rgba(0x18, 0x18, 0x25, alpha)'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'the_overlays_own_surfaces_come_from_the_palette',
            'a_wash_keeps_its_own_alpha_and_the_colour_of_its_role',
        ],
    ),
    (
        "OOOOOOOOOOOOO: a dimmed card keeps Mocha's surface0",
        OV,
        [
            ('Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 100)',
             'Color::rgba(0x31, 0x32, 0x44, 100)'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'a_wash_keeps_its_own_alpha_and_the_colour_of_its_role',
        ],
    ),
    (
        "PPPPPPPPPPPPP: an active search bar's border keeps its hardcoded blue",
        OV,
        [
            ('    } else {\n        p.accent\n    };\n    cmds.push(RenderCommand::StrokeRect {',
             '    } else {\n        Color::from_hex(0x89B4FA)\n    };\n    cmds.push(RenderCommand::StrokeRect {'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "QQQQQQQQQQQQQ: an active search bar's border is drawn like an idle one",
        OV,
        [
            ('    } else {\n        p.accent\n    };\n    cmds.push(RenderCommand::StrokeRect {',
             '    } else {\n        p.surface1\n    };\n    cmds.push(RenderCommand::StrokeRect {'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "RRRRRRRRRRRRR: the current desktop's marker keeps its hardcoded blue",
        OV,
        [
            ('                height: 24.0,\n                color: p.accent,',
             '                height: 24.0,\n                color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
        ],
    ),
    (
        "SSSSSSSSSSSSS: the current desktop's marker is drawn in the body text colour",
        OV,
        [
            ('                height: 24.0,\n                color: p.accent,',
             '                height: 24.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        "TTTTTTTTTTTTT: a hovered card's border keeps its hardcoded blue",
        OV,
        [
            ('    let border_color = if is_hovered {\n        p.accent',
             '    let border_color = if is_hovered {\n        Color::from_hex(0x89B4FA)'),
        ],
        ["desktop"],
        [
            'every_colour_the_overlay_draws_comes_from_its_palette',
            'every_control_that_offers_something_follows_the_accent',
            'a_cards_border_says_both_where_you_point_and_what_has_focus',
        ],
    ),
    (
        'UUUUUUUUUUUUU: pointing at a card looks the same as the card having focus',
        OV,
        [
            ('    let border_color = if is_hovered {\n        p.accent',
             '    let border_color = if is_hovered {\n        p.subtext0'),
        ],
        ["desktop"],
        [
            'every_control_that_offers_something_follows_the_accent',
            'a_cards_border_says_both_where_you_point_and_what_has_focus',
        ],
    ),
    (
        "VVVVVVVVVVVVV: the focused card's border collapses into a plain one",
        OV,
        [
            ('    } else if layout.is_focused {\n        p.subtext0',
             '    } else if layout.is_focused {\n        p.surface2'),
        ],
        ["desktop"],
        [
            'a_cards_border_says_both_where_you_point_and_what_has_focus',
        ],
    ),
    (
        "WWWWWWWWWWWWW: the focused card's border collapses into the pointed-at one",
        OV,
        [
            ('    } else if layout.is_focused {\n        p.subtext0',
             '    } else if layout.is_focused {\n        p.accent'),
        ],
        ["desktop"],
        [
            'a_cards_border_says_both_where_you_point_and_what_has_focus',
        ],
    ),
    (
        "XXXXXXXXXXXXX: the minimised badge follows the desktop's accent",
        OV,
        [
            ('            height: 16.0,\n            color: p.yellow,',
             '            height: 16.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'neither_badge_follows_the_accent',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        "YYYYYYYYYYYYY: the close button follows the desktop's accent",
        OV,
        [
            ('            height: 18.0,\n            color: p.red,',
             '            height: 18.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'neither_badge_follows_the_accent',
            'nothing_else_moves_when_the_accent_does',
        ],
    ),
    (
        'ZZZZZZZZZZZZZ: the minimised mark is a fixed near-black again',
        OV,
        [
            ('            color: readable_on(p.yellow),',
             '            color: p.base,'),
        ],
        ["desktop"],
        [
            'each_badges_mark_can_be_read_on_the_badge',
        ],
    ),
    (
        'AAAAAAAAAAAAAA: the close mark is a fixed near-black again',
        OV,
        [
            ('            color: readable_on(p.red),',
             '            color: p.base,'),
        ],
        ["desktop"],
        [
            'each_badges_mark_can_be_read_on_the_badge',
        ],
    ),
    (
        "BBBBBBBBBBBBBB: the minimised mark answers for the close button's red instead",
        OV,
        [
            ('            color: readable_on(p.yellow),',
             '            color: readable_on(p.red),'),
        ],
        ["desktop"],
        [
            'each_badges_mark_can_be_read_on_the_badge',
        ],
    ),
    (
        'CCCCCCCCCCCCCC: the backdrop loses the opacity that makes it a wash',
        OV,
        [
            ('Color::rgba(p.mantle.r, p.mantle.g, p.mantle.b, alpha)',
             'p.mantle'),
        ],
        ["desktop"],
        [
            'a_wash_keeps_its_own_alpha_and_the_colour_of_its_role',
        ],
    ),
    (
        'DDDDDDDDDDDDDD: a search-dimmed card loses the veil that dims it',
        OV,
        [
            ('Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 100)',
             'p.surface0'),
        ],
        ["desktop"],
        [
            'a_wash_keeps_its_own_alpha_and_the_colour_of_its_role',
        ],
    ),
    (
        'EEEEEEEEEEEEEE: a search-dimmed card is a veiled mantle rather than a veiled surface',
        OV,
        [
            ('Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 100)',
             'Color::rgba(p.mantle.r, p.mantle.g, p.mantle.b, 100)'),
        ],
        ["desktop"],
        [
            'a_wash_keeps_its_own_alpha_and_the_colour_of_its_role',
        ],
    ),
    # ---- context_ext.rs (module 21 of 49) ---------------------------------
    (
        "AAAAAAAAAAAAAAA: the menu's background keeps Mocha's base",
        CTX,
        [
            ('        height: total_height,\n        color: p.base,',
             '        height: total_height,\n        color: guitk::color::Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "BBBBBBBBBBBBBBB: the menu's border keeps Mocha's surface1",
        CTX,
        [
            ('        height: total_height,\n        color: p.surface1,',
             '        height: total_height,\n        color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "CCCCCCCCCCCCCCC: the separator keeps Mocha's surface0",
        CTX,
        [
            ('                    color: p.surface0,\n                    width: 1.0,',
             '                    color: guitk::color::Color::from_hex(0x313244),\n                    width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "DDDDDDDDDDDDDDD: a hovered built-in row keeps Mocha's surface0",
        CTX,
        [
            ('                        height: item_height,\n                        color: p.surface0,\n                        corner_radii: CornerRadii::all(4.0),\n                    });\n                }\n\n                // Icon.',
             '                        height: item_height,\n                        color: guitk::color::Color::from_hex(0x313244),\n                        corner_radii: CornerRadii::all(4.0),\n                    });\n                }\n\n                // Icon.'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "EEEEEEEEEEEEEEE: a hovered built-in item's icon keeps Mocha's text",
        CTX,
        [
            ('                    text: item.icon().to_string(),\n                    font_size: 13.0,\n                    color: if hovered { p.text }',
             '                    text: item.icon().to_string(),\n                    font_size: 13.0,\n                    color: if hovered { guitk::color::Color::from_hex(0xCDD6F4) }'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "FFFFFFFFFFFFFFF: an idle built-in item's icon keeps Mocha's subtext1",
        CTX,
        [
            ('                    text: item.icon().to_string(),\n                    font_size: 13.0,\n                    color: if hovered { p.text } else { p.subtext1 },',
             '                    text: item.icon().to_string(),\n                    font_size: 13.0,\n                    color: if hovered { p.text } else { guitk::color::Color::from_hex(0xBAC2DE) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "GGGGGGGGGGGGGGG: a shortcut hint keeps Mocha's overlay0",
        CTX,
        [
            ('                        font_size: 11.0,\n                        color: p.overlay0,\n                        font_weight: FontWeightHint::Light,\n                        max_width: None,\n                        overflow: TextOverflow::Clip,\n                    });\n                }\n\n                cy += item_height;\n            }\n            ContextMenuEntry::Extension {',
             '                        font_size: 11.0,\n                        color: guitk::color::Color::from_hex(0x6C7086),\n                        font_weight: FontWeightHint::Light,\n                        max_width: None,\n                        overflow: TextOverflow::Clip,\n                    });\n                }\n\n                cy += item_height;\n            }\n            ContextMenuEntry::Extension {'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "HHHHHHHHHHHHHHH: an idle extension icon keeps Mocha's subtext0",
        CTX,
        [
            ('                    color: if hovered { p.accent } else { p.subtext0 },',
             '                    color: if hovered { p.accent } else { guitk::color::Color::from_hex(0xA6ADC8) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'a_hovered_extensions_icon_follows_the_accent',
        ],
    ),
    (
        "IIIIIIIIIIIIIII: a slow extension's label keeps Mocha's overlay0",
        CTX,
        [
            ('                    color: if *slow {\n                        p.overlay0',
             '                    color: if *slow {\n                        guitk::color::Color::from_hex(0x6C7086)'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJ: the submenu arrow keeps Mocha's subtext0",
        CTX,
        [
            ('                        text: "\\u{25B6}".to_string(),\n                        font_size: 10.0,\n                        color: p.subtext0,',
             '                        text: "\\u{25B6}".to_string(),\n                        font_size: 10.0,\n                        color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "KKKKKKKKKKKKKKK: the settings title keeps Mocha's text",
        CTX,
        [
            ('            font_size: 18.0,\n            color: p.text,',
             '            font_size: 18.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "LLLLLLLLLLLLLLL: the settings search bar keeps Mocha's surface0",
        CTX,
        [
            ('            height: 28.0,\n            color: p.surface0,',
             '            height: 28.0,\n            color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "MMMMMMMMMMMMMMM: a selected settings row keeps Mocha's surface0",
        CTX,
        [
            ('                let row_bg = if selected { p.surface0 } else { p.mantle };',
             '                let row_bg = if selected { guitk::color::Color::from_hex(0x313244) } else { p.mantle };'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "NNNNNNNNNNNNNNN: an unselected settings row keeps Mocha's mantle",
        CTX,
        [
            ('                let row_bg = if selected { p.surface0 } else { p.mantle };',
             '                let row_bg = if selected { p.surface0 } else { guitk::color::Color::from_hex(0x181825) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "OOOOOOOOOOOOOOO: the enabled dot keeps Mocha's green",
        CTX,
        [
            ('                let status_color = if ext.enabled { p.green } else { p.red };',
             '                let status_color = if ext.enabled { guitk::color::Color::from_hex(0xA6E3A1) } else { p.red };'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'an_enabled_extension_and_a_disabled_one_never_look_alike',
        ],
    ),
    (
        "PPPPPPPPPPPPPPP: the disabled dot keeps Mocha's red",
        CTX,
        [
            ('                let status_color = if ext.enabled { p.green } else { p.red };',
             '                let status_color = if ext.enabled { p.green } else { guitk::color::Color::from_hex(0xF38BA8) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'an_enabled_extension_and_a_disabled_one_never_look_alike',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQ: the Slow badge keeps Mocha's yellow",
        CTX,
        [
            ('                        text: "Slow".to_string(),\n                        font_size: 10.0,\n                        color: p.yellow,',
             '                        text: "Slow".to_string(),\n                        font_size: 10.0,\n                        color: guitk::color::Color::from_hex(0xF9E2AF),'),
        ],
        ["desktop"],
        [
            # Not the frozen test: a badge nailed to a hardcoded yellow still
            # does not move with the accent, which is all that test claims.
            # Only the sweep can see a literal.
            'every_colour_the_context_menu_draws_comes_from_its_palette',
        ],
    ),
    (
        "RRRRRRRRRRRRRRR: a hovered extension's icon keeps its hardcoded blue",
        CTX,
        [
            ('                    color: if hovered { p.accent } else { p.subtext0 },',
             '                    color: if hovered { guitk::color::Color::from_hex(0x89B4FA) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_context_menu_draws_comes_from_its_palette',
            'a_hovered_extensions_icon_follows_the_accent',
        ],
    ),
    (
        "SSSSSSSSSSSSSSS: a hovered extension's icon is drawn like an idle one",
        CTX,
        [
            ('                    color: if hovered { p.accent } else { p.subtext0 },',
             '                    color: if hovered { p.subtext0 } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'a_hovered_extensions_icon_follows_the_accent',
        ],
    ),
    (
        "TTTTTTTTTTTTTTT: an extension's icon takes the accent whether it is pointed at or not",
        CTX,
        [
            ('                    color: if hovered { p.accent } else { p.subtext0 },',
             '                    color: if hovered { p.accent } else { p.accent },'),
        ],
        ["desktop"],
        [
            'a_hovered_extensions_icon_follows_the_accent',
        ],
    ),
    (
        "UUUUUUUUUUUUUUU: a hovered extension's icon is drawn in the body text colour",
        CTX,
        [
            ('                    color: if hovered { p.accent } else { p.subtext0 },',
             '                    color: if hovered { p.text } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'a_hovered_extensions_icon_follows_the_accent',
        ],
    ),
    (
        "VVVVVVVVVVVVVVV: the enabled dot follows the desktop's accent",
        CTX,
        [
            ('                let status_color = if ext.enabled { p.green } else { p.red };',
             '                let status_color = if ext.enabled { p.accent } else { p.red };'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_fact_follows_the_accent',
            'an_enabled_extension_and_a_disabled_one_never_look_alike',
        ],
    ),
    (
        "WWWWWWWWWWWWWWW: the disabled dot follows the desktop's accent",
        CTX,
        [
            ('                let status_color = if ext.enabled { p.green } else { p.red };',
             '                let status_color = if ext.enabled { p.green } else { p.accent };'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_fact_follows_the_accent',
            'an_enabled_extension_and_a_disabled_one_never_look_alike',
        ],
    ),
    (
        "XXXXXXXXXXXXXXX: the Slow badge follows the desktop's accent",
        CTX,
        [
            ('                        text: "Slow".to_string(),\n                        font_size: 10.0,\n                        color: p.yellow,',
             '                        text: "Slow".to_string(),\n                        font_size: 10.0,\n                        color: p.accent,'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_fact_follows_the_accent',
        ],
    ),
    (
        'YYYYYYYYYYYYYYY: an enabled extension and a disabled one are told apart by nothing',
        CTX,
        [
            ('                let status_color = if ext.enabled { p.green } else { p.red };',
             '                let status_color = if ext.enabled { p.green } else { p.green };'),
        ],
        ["desktop"],
        [
            'an_enabled_extension_and_a_disabled_one_never_look_alike',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZ: the hovered row is the same colour as the menu under it',
        CTX,
        [
            ('                        height: item_height,\n                        color: p.surface0,\n                        corner_radii: CornerRadii::all(4.0),\n                    });\n                }\n\n                // Icon.',
             '                        height: item_height,\n                        color: p.base,\n                        corner_radii: CornerRadii::all(4.0),\n                    });\n                }\n\n                // Icon.'),
        ],
        ["desktop"],
        [
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAA: a selected settings row looks exactly like an unselected one',
        CTX,
        [
            ('                let row_bg = if selected { p.surface0 } else { p.mantle };',
             '                let row_bg = if selected { p.mantle } else { p.mantle };'),
        ],
        ["desktop"],
        [
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBB: the menu's background is drawn on the mantle instead of the base",
        CTX,
        [
            ('        height: total_height,\n        color: p.base,',
             '        height: total_height,\n        color: p.mantle,'),
        ],
        ["desktop"],
        [
            'the_menus_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCC: the menu's shadow goes back to its own private depth",
        CTX,
        [
            ('        color: p.shadow(),',
             '        color: guitk::color::Color::rgba(0, 0, 0, 80),'),
        ],
        ["desktop"],
        [
            'the_menu_casts_the_shared_popup_shadow',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDD: the menu's shadow loses the opacity that makes it a shadow",
        CTX,
        [
            ('        color: p.shadow(),',
             '        color: guitk::color::Color::rgba(0, 0, 0, 255),'),
        ],
        ["desktop"],
        [
            'the_menu_casts_the_shared_popup_shadow',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEE: the menu's shadow is drawn in a palette role instead of black",
        CTX,
        [
            ('        color: p.shadow(),',
             '        color: p.crust,'),
        ],
        ["desktop"],
        [
            # `p.crust` is a palette member in both modes, so the membership
            # sweep is *supposed* to be blind to it. That blind spot is the
            # whole reason the shadow test exists.
            'the_menu_casts_the_shared_popup_shadow',
        ],
    ),
    # ---- widgets.rs (module 22 of 49) --------------------------------------
    (
        "AAAAAAAAAAAAAAAAA: the edit-mode grid keeps Mocha's surface0",
        WID,
        [
            ('color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 80),',
             'color: Color::rgba(0x31, 0x32, 0x44, 80),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBB: the grid's veil is thickened to a widget's opacity",
        WID,
        [
            ('p.surface0.b, 80),',
             'p.surface0.b, 200),'),
        ],
        ["desktop"],
        [
            # The grid's alpha is a property of the grid, which no widget owns,
            # so it is the one wash here that must NOT move with bg_opacity.
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCC: a widget's shadow is pinned to a fixed depth",
        WID,
        [
            ('color: Color::rgba(0, 0, 0, w.bg_opacity / 3),',
             'color: Color::rgba(0, 0, 0, 120),'),
        ],
        ["desktop"],
        [
            'a_translucent_widget_casts_a_translucent_shadow',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDD: a widget's shadow joins the shared popup shadow",
        WID,
        [
            ('color: Color::rgba(0, 0, 0, w.bg_opacity / 3),',
             'color: p.shadow(),'),
        ],
        ["desktop"],
        [
            # The sweep waves black through at any alpha, so it is blind to both
            # shadows by design. That is exactly why each has its own test.
            'a_translucent_widget_casts_a_translucent_shadow',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEE: a widget's panel keeps Mocha's base",
        WID,
        [
            ('color: Color::rgba(p.base.r, p.base.g, p.base.b, w.bg_opacity),',
             'color: Color::rgba(0x1E, 0x1E, 0x2E, w.bg_opacity),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFF: a widget's panel is drawn opaque",
        WID,
        [
            ('p.base.b, w.bg_opacity),',
             'p.base.b, 255),'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGG: the selected widget's ring is frozen to blue",
        WID,
        [
            ('color: p.accent,\n                line_width: 2.0,',
             'color: p.blue,\n                line_width: 2.0,'),
        ],
        ["desktop"],
        [
            # `p.blue` is a palette member in both modes, so the sweep is
            # supposed to be blind to it. Only the accent test can see this.
            'the_selected_widgets_outline_follows_the_accent',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHH: the selected widget's ring keeps Mocha's blue",
        WID,
        [
            ('color: p.accent,\n                line_width: 2.0,',
             'color: guitk::color::Color::from_hex(0x89B4FA),\n                line_width: 2.0,'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'the_selected_widgets_outline_follows_the_accent',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIII: the selection ring is drawn outside edit mode",
        WID,
        [
            ('if self.edit_mode && self.selected_widget == Some(w.id) {',
             'if self.selected_widget == Some(w.id) {'),
        ],
        ["desktop"],
        [
            'the_selected_widgets_outline_follows_the_accent',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJ: every widget gets a selection ring once anything is selected",
        WID,
        [
            ('if self.edit_mode && self.selected_widget == Some(w.id) {',
             'if self.edit_mode && self.selected_widget.is_some() {'),
        ],
        ["desktop"],
        [
            'the_selected_widgets_outline_follows_the_accent',
            'the_fixture_takes_every_branch_the_widget_layer_has',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKK: a widget's title bar keeps Mocha's surface0",
        WID,
        [
            ('color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, w.bg_opacity),',
             'color: Color::rgba(0x31, 0x32, 0x44, w.bg_opacity),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLL: a widget's title bar is emphasised like its text",
        WID,
        [
            ('p.surface0.b, w.bg_opacity),\n            corner_radii: CornerRadii {',
             'p.surface0.b, (w.bg_opacity as f32 * 1.2) as u8),\n            corner_radii: CornerRadii {'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMM: a widget's title-bar icon is drawn in body text",
        WID,
        [
            ('color: Color::rgba(\n                p.subtext0.r,\n                p.subtext0.g,\n                p.subtext0.b,\n                (w.bg_opacity as f32 * 1.2) as u8,\n            ),\n            font_weight: FontWeightHint::Regular,',
             'color: Color::rgba(\n                p.text.r,\n                p.text.g,\n                p.text.b,\n                (w.bg_opacity as f32 * 1.2) as u8,\n            ),\n            font_weight: FontWeightHint::Regular,'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNN: a widget's title-bar icon loses its emphasis over the panel",
        WID,
        [
            ('color: Color::rgba(\n                p.subtext0.r,\n                p.subtext0.g,\n                p.subtext0.b,\n                (w.bg_opacity as f32 * 1.2) as u8,\n            ),\n            font_weight: FontWeightHint::Regular,',
             'color: Color::rgba(\n                p.subtext0.r,\n                p.subtext0.g,\n                p.subtext0.b,\n                w.bg_opacity,\n            ),\n            font_weight: FontWeightHint::Regular,'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOO: a widget's title keeps Mocha's subtext0",
        WID,
        [
            ('color: Color::rgba(\n                p.subtext0.r,\n                p.subtext0.g,\n                p.subtext0.b,\n                (w.bg_opacity as f32 * 1.2) as u8,\n            ),\n            font_weight: FontWeightHint::Bold,',
             'color: Color::rgba(\n                0xA6,\n                0xAD,\n                0xC8,\n                (w.bg_opacity as f32 * 1.2) as u8,\n            ),\n            font_weight: FontWeightHint::Bold,'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPP: the clock's time keeps Mocha's text",
        WID,
        [
            ('text: "12:34".to_string(),\n                    font_size: 36.0,\n                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),',
             'text: "12:34".to_string(),\n                    font_size: 36.0,\n                    color: Color::rgba(0xCD, 0xD6, 0xF4, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQ: the clock's date is dimmed to a placeholder",
        WID,
        [
            ('text: "Sunday, May 18".to_string(),\n                    font_size: 12.0,\n                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),',
             'text: "Sunday, May 18".to_string(),\n                    font_size: 12.0,\n                    color: Color::rgba(p.overlay0.r, p.overlay0.g, p.overlay0.b, alpha),'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRR: the CPU meter's label keeps Mocha's subtext0",
        WID,
        [
            ('text: "CPU".to_string(),\n                    font_size: 10.0,\n                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),',
             'text: "CPU".to_string(),\n                    font_size: 10.0,\n                    color: Color::rgba(0xA6, 0xAD, 0xC8, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSS: the CPU meter's track steps up a surface",
        WID,
        [
            ('y: y + 14.0,\n                    width,\n                    height: bar_h,\n                    color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),',
             'y: y + 14.0,\n                    width,\n                    height: bar_h,\n                    color: Color::rgba(p.surface2.r, p.surface2.g, p.surface2.b, alpha),'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_measurement_follows_the_accent',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTT: the CPU meter follows the accent",
        WID,
        [
            ('width: width * 0.45,\n                    height: bar_h,\n                    color: Color::rgba(p.blue.r, p.blue.g, p.blue.b, alpha),',
             'width: width * 0.45,\n                    height: bar_h,\n                    color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, alpha),'),
        ],
        ["desktop"],
        [
            # This is module 19's slider rule applied to something that is not a
            # slider. The accent is a palette member, so only the frozen test sees it.
            'nothing_that_reports_a_measurement_follows_the_accent',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUU: the CPU meter is drawn opaque over a translucent panel",
        WID,
        [
            ('width: width * 0.45,\n                    height: bar_h,\n                    color: Color::rgba(p.blue.r, p.blue.g, p.blue.b, alpha),',
             'width: width * 0.45,\n                    height: bar_h,\n                    color: Color::rgba(p.blue.r, p.blue.g, p.blue.b, 255),'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVV: the Memory meter's label keeps Mocha's subtext0",
        WID,
        [
            ('text: "Memory".to_string(),\n                    font_size: 10.0,\n                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),',
             'text: "Memory".to_string(),\n                    font_size: 10.0,\n                    color: Color::rgba(0xA6, 0xAD, 0xC8, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWW: the Memory meter's track keeps Mocha's surface1",
        WID,
        [
            ('y: y + 46.0,\n                    width,\n                    height: bar_h,\n                    color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),',
             'y: y + 46.0,\n                    width,\n                    height: bar_h,\n                    color: Color::rgba(0x45, 0x47, 0x5A, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'nothing_that_reports_a_measurement_follows_the_accent',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXX: the Memory meter is the same colour as the CPU meter",
        WID,
        [
            ('width: width * 0.62,\n                    height: bar_h,\n                    color: Color::rgba(p.green.r, p.green.g, p.green.b, alpha),',
             'width: width * 0.62,\n                    height: bar_h,\n                    color: Color::rgba(p.blue.r, p.blue.g, p.blue.b, alpha),'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_measurement_follows_the_accent',
            'the_three_meters_never_look_alike',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYY: the Disk meter's label is promoted to body text",
        WID,
        [
            ('text: "Disk".to_string(),\n                    font_size: 10.0,\n                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),',
             'text: "Disk".to_string(),\n                    font_size: 10.0,\n                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZ: the Disk meter's track keeps Mocha's surface1",
        WID,
        [
            ('y: y + 78.0,\n                    width,\n                    height: bar_h,\n                    color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),',
             'y: y + 78.0,\n                    width,\n                    height: bar_h,\n                    color: Color::rgba(0x45, 0x47, 0x5A, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'nothing_that_reports_a_measurement_follows_the_accent',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAA: the Disk meter follows the accent",
        WID,
        [
            ('width: width * 0.38,\n                    height: bar_h,\n                    color: Color::rgba(p.peach.r, p.peach.g, p.peach.b, alpha),',
             'width: width * 0.38,\n                    height: bar_h,\n                    color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, alpha),'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_measurement_follows_the_accent',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBB: an empty note's placeholder is drawn like a written one",
        WID,
        [
            ('                        if w.state_text.is_empty() {\n                            p.overlay0.r\n                        } else {\n                            p.text.r\n                        },\n                        if w.state_text.is_empty() {\n                            p.overlay0.g\n                        } else {\n                            p.text.g\n                        },\n                        if w.state_text.is_empty() {\n                            p.overlay0.b\n                        } else {\n                            p.text.b\n                        },',
             '                        if w.state_text.is_empty() {\n                            p.text.r\n                        } else {\n                            p.text.r\n                        },\n                        if w.state_text.is_empty() {\n                            p.text.g\n                        } else {\n                            p.text.g\n                        },\n                        if w.state_text.is_empty() {\n                            p.text.b\n                        } else {\n                            p.text.b\n                        },'),
        ],
        ["desktop"],
        [
            'an_empty_note_and_a_written_one_never_look_alike',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCC: a written note keeps Mocha's text",
        WID,
        [
            ('                        if w.state_text.is_empty() {\n                            p.overlay0.r\n                        } else {\n                            p.text.r\n                        },\n                        if w.state_text.is_empty() {\n                            p.overlay0.g\n                        } else {\n                            p.text.g\n                        },\n                        if w.state_text.is_empty() {\n                            p.overlay0.b\n                        } else {\n                            p.text.b\n                        },',
             '                        if w.state_text.is_empty() {\n                            p.overlay0.r\n                        } else {\n                            0xCD\n                        },\n                        if w.state_text.is_empty() {\n                            p.overlay0.g\n                        } else {\n                            0xD6\n                        },\n                        if w.state_text.is_empty() {\n                            p.overlay0.b\n                        } else {\n                            0xF4\n                        },'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDD: a note is drawn opaque over a translucent panel",
        WID,
        [
            ('                        },\n                        alpha,\n                    ),\n                    font_weight: FontWeightHint::Regular,\n                    max_width: Some(width),\n                    overflow: TextOverflow::Ellipsis,\n                });\n            }\n            WidgetKind::BatteryStatus => {',
             '                        },\n                        255,\n                    ),\n                    font_weight: FontWeightHint::Regular,\n                    max_width: Some(width),\n                    overflow: TextOverflow::Ellipsis,\n                });\n            }\n            WidgetKind::BatteryStatus => {'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEE: the battery glyph follows the accent",
        WID,
        [
            ('text: "\\u{1F50B}".to_string(),\n                    font_size: 28.0,\n                    color: Color::rgba(p.green.r, p.green.g, p.green.b, alpha),',
             'text: "\\u{1F50B}".to_string(),\n                    font_size: 28.0,\n                    color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, alpha),'),
        ],
        ["desktop"],
        [
            # Green on a battery is the reading itself, not decoration: it is how
            # the widget says the charge is healthy. A red accent would make it lie.
            'nothing_that_reports_a_measurement_follows_the_accent',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFF: the battery glyph keeps Mocha's green",
        WID,
        [
            ('text: "\\u{1F50B}".to_string(),\n                    font_size: 28.0,\n                    color: Color::rgba(p.green.r, p.green.g, p.green.b, alpha),',
             'text: "\\u{1F50B}".to_string(),\n                    font_size: 28.0,\n                    color: Color::rgba(0xA6, 0xE3, 0xA1, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'nothing_that_reports_a_measurement_follows_the_accent',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGG: the battery's reading keeps Mocha's text",
        WID,
        [
            ('text: "85%".to_string(),\n                    font_size: 20.0,\n                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),',
             'text: "85%".to_string(),\n                    font_size: 20.0,\n                    color: Color::rgba(0xCD, 0xD6, 0xF4, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHH: the battery's estimate is promoted to body text",
        WID,
        [
            ('text: "3h 42m remaining".to_string(),\n                    font_size: 11.0,\n                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),',
             'text: "3h 42m remaining".to_string(),\n                    font_size: 11.0,\n                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIII: the generic widget's placeholder icon keeps Mocha's surface2",
        WID,
        [
            ('font_size: 32.0,\n                    color: Color::rgba(p.surface2.r, p.surface2.g, p.surface2.b, alpha),',
             'font_size: 32.0,\n                    color: Color::rgba(0x58, 0x5B, 0x70, alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJ: the generic widget's label is dimmed to a placeholder",
        WID,
        [
            ('font_size: 13.0,\n                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),',
             'font_size: 13.0,\n                    color: Color::rgba(p.overlay0.r, p.overlay0.g, p.overlay0.b, alpha),'),
        ],
        ["desktop"],
        [
            'every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKK: the picker keeps its own shadow depth",
        WID,
        [
            ('            blur: 20.0,\n            spread: 0.0,\n            color: p.shadow(),',
             '            blur: 20.0,\n            spread: 0.0,\n            color: guitk::color::Color::rgba(0, 0, 0, 100),'),
        ],
        ["desktop"],
        [
            'the_picker_casts_the_shared_popup_shadow',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLL: the picker's panel keeps Mocha's mantle",
        WID,
        [
            ('            color: p.mantle,',
             '            color: guitk::color::Color::from_hex(0x181825),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMM: the picker's border keeps Mocha's surface1",
        WID,
        [
            ('            color: p.surface1,\n            line_width: 1.0,',
             '            color: guitk::color::Color::from_hex(0x45475A),\n            line_width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNN: the picker's title follows the accent",
        WID,
        [
            ('text: "Add Widget".to_string(),\n            font_size: 16.0,\n            color: p.text,',
             'text: "Add Widget".to_string(),\n            font_size: 16.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOO: every picker row's icon follows the accent",
        WID,
        [
            ('font_size: 16.0,\n                color: p.blue,',
             'font_size: 16.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            # Every row is drawn identically, so an accent here says nothing about
            # any row -- and it costs the accent its one job, which is the ring.
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPP: the picker's row labels keep Mocha's text",
        WID,
        [
            ('font_size: 13.0,\n                color: p.text,',
             'font_size: 13.0,\n                color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQ: the picker's size hints keep Mocha's overlay0",
        WID,
        [
            ('font_size: 10.0,\n                color: p.overlay0,',
             'font_size: 10.0,\n                color: guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_the_widget_layer_draws_comes_from_its_palette',
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRR: the edit-mode grid stops being drawn in edit mode",
        WID,
        [
            ('        if self.edit_mode {\n            self.render_grid(p, &mut commands);',
             '        if !self.edit_mode {\n            self.render_grid(p, &mut commands);'),
        ],
        ["desktop"],
        [
            # Not a colour bug. The four defects here exist because module 21 lost
            # three defects to a fixture that never drew them: a branch that stops
            # firing silently removes a colour site from every test at once.
            'the_fixture_takes_every_branch_the_widget_layer_has',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSS: the picker stops being drawn while a widget is selected",
        WID,
        [
            ('        if self.picker_open {',
             '        if self.picker_open && self.selected_widget.is_none() {'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_the_widget_layer_has',
            'the_picker_casts_the_shared_popup_shadow',
            'the_pickers_own_surfaces_come_from_the_palette',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTT: a hidden widget is drawn anyway",
        WID,
        [
            ('            if !w.visible {\n                continue;\n            }\n            self.render_widget(w, p, &mut commands);',
             '            if !w.visible && w.bg_opacity == 0 {\n                continue;\n            }\n            self.render_widget(w, p, &mut commands);'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_the_widget_layer_has',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUU: a hidden widget layer still draws itself",
        WID,
        [
            ('        if !self.layer_visible {\n            return Vec::new();\n        }',
             '        if !self.layer_visible && self.widgets.is_empty() {\n            return Vec::new();\n        }'),
        ],
        ["desktop"],
        [
            'a_hidden_widget_layer_draws_nothing',
        ],
    ),
    # ---- sound_settings.rs (module 23 of 49) --------------------------------
    (
        "AAAAAAAAAAAAAAAAAAA: the panel background is frozen back to Mocha base",
        SND,
        [
            ('color: p.base,',
             'color: guitk::color::Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_sound_panel_draws_comes_from_its_palette',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBB: the panel background becomes a sidebar's",
        SND,
        [
            ('color: p.base,',
             'color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCC: the title is frozen back to Mocha text",
        SND,
        [
            ('font_size: 20.0,\n            color: p.text,',
             'font_size: 20.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_sound_panel_draws_comes_from_its_palette',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDD: the title drops to secondary text",
        SND,
        [
            ('font_size: 20.0,\n            color: p.text,',
             'font_size: 20.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEE: a muted master volume stops going red",
        SND,
        [
            ('master_muted {\n                p.red\n',
             'master_muted {\n                p.text\n'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFF: an unmuted master volume drops to secondary text",
        SND,
        [
            ('p.red\n            } else {\n                p.text\n            },',
             'p.red\n            } else {\n                p.subtext0\n            },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGG: the active tab stops being raised",
        SND,
        [
            ('width: tab_w - 2.0,\n                height: 32.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'width: tab_w - 2.0,\n                height: 32.0,\n                color: if active { p.mantle } else { p.mantle },'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHH: every tab looks like the active one",
        SND,
        [
            ('width: tab_w - 2.0,\n                height: 32.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'width: tab_w - 2.0,\n                height: 32.0,\n                color: if active { p.surface0 } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIII: the active tab's label is frozen back to Mocha blue",
        SND,
        [
            ('color: if active { p.accent } else { p.subtext0 },',
             'color: if active { guitk::color::Color::from_hex(0x89B4FA) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_sound_panel_draws_comes_from_its_palette',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'the_three_accent_sites_follow_the_accent',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJ: the active tab's label reads like an inactive one",
        SND,
        [
            ('color: if active { p.accent } else { p.subtext0 },',
             'color: if active { p.subtext0 } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'the_three_accent_sites_follow_the_accent',
            'every_pair_this_panel_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKK: every tab's label takes the accent",
        SND,
        [
            ('color: if active { p.accent } else { p.subtext0 },',
             'color: if active { p.accent } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'every_pair_this_panel_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLL: the empty-output line is promoted to secondary text",
        SND,
        [
            ('text: "No output devices detected.".into(),\n                font_size: 13.0,\n                color: p.overlay0,',
             'text: "No output devices detected.".into(),\n                font_size: 13.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMM: the default output device stops being raised",
        SND,
        [
            ('let bg = if dev.is_default { p.surface0 } else { p.mantle };',
             'let bg = if dev.is_default { p.mantle } else { p.mantle };'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNN: every output device looks like the default",
        SND,
        [
            ('let bg = if dev.is_default { p.surface0 } else { p.mantle };',
             'let bg = if dev.is_default { p.surface0 } else { p.surface0 };'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOO: an output device's name drops to secondary text",
        SND,
        [
            ('text: format!("{}{}", dev.name, name_suffix),\n                font_size: 14.0,\n                color: p.text,',
             'text: format!("{}{}", dev.name, name_suffix),\n                font_size: 14.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPP: an output device's format line drops to the faintest role",
        SND,
        [
            ('text: dev.format_string(),\n                font_size: 11.0,\n                color: p.subtext0,\n                font_weight: FontWeightHint::Regular,\n                max_width: Some(width - 24.0),\n                overflow: TextOverflow::Ellipsis,\n            });\n\n            // Volume bar',
             'text: dev.format_string(),\n                font_size: 11.0,\n                color: p.overlay0,\n                font_weight: FontWeightHint::Regular,\n                max_width: Some(width - 24.0),\n                overflow: TextOverflow::Ellipsis,\n            });\n\n            // Volume bar'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQ: the empty-input line is promoted to secondary text",
        SND,
        [
            ('text: "No input devices detected.".into(),\n                font_size: 13.0,\n                color: p.overlay0,',
             'text: "No input devices detected.".into(),\n                font_size: 13.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRR: the default input device stops being raised",
        SND,
        [
            ('height: 48.0,\n                color: if dev.is_default { p.surface0 } else { p.mantle },',
             'height: 48.0,\n                color: if dev.is_default { p.mantle } else { p.mantle },'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSS: every input device looks like the default",
        SND,
        [
            ('height: 48.0,\n                color: if dev.is_default { p.surface0 } else { p.mantle },',
             'height: 48.0,\n                color: if dev.is_default { p.surface0 } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTT: an input device's name drops to secondary text",
        SND,
        [
            ('text: format!("{}{}", dev.name, def_txt),\n                font_size: 14.0,\n                color: p.text,',
             'text: format!("{}{}", dev.name, def_txt),\n                font_size: 14.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUU: an input device's format line drops to the faintest role",
        SND,
        [
            ('text: dev.format_string(),\n                font_size: 11.0,\n                color: p.subtext0,\n                font_weight: FontWeightHint::Regular,\n                max_width: Some(width - 24.0),\n                overflow: TextOverflow::Ellipsis,\n            });\n            y += 56.0;',
             'text: dev.format_string(),\n                font_size: 11.0,\n                color: p.overlay0,\n                font_weight: FontWeightHint::Regular,\n                max_width: Some(width - 24.0),\n                overflow: TextOverflow::Ellipsis,\n            });\n            y += 56.0;'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVV: the microphone heading follows the accent",
        SND,
        [
            ('text: "Microphone Settings".into(),\n            font_size: 14.0,\n            color: p.lavender,',
             'text: "Microphone Settings".into(),\n            font_size: 14.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWW: the empty-app line is promoted to secondary text",
        SND,
        [
            ('text: "No applications are currently producing audio.".into(),\n                font_size: 13.0,\n                color: p.overlay0,',
             'text: "No applications are currently producing audio.".into(),\n                font_size: 13.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXX: an app row is raised like a default device",
        SND,
        [
            ('height: 48.0,\n                color: p.mantle,',
             'height: 48.0,\n                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYY: an app's name drops to secondary text",
        SND,
        [
            ('text: entry.display_name.clone(),\n                font_size: 13.0,\n                color: p.text,',
             'text: entry.display_name.clone(),\n                font_size: 13.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZ: a muted app's volume stops going red",
        SND,
        [
            ('color: if entry.muted { p.red } else { p.subtext0 },',
             'color: if entry.muted { p.subtext0 } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAA: an unmuted app's volume is promoted to body text",
        SND,
        [
            ('color: if entry.muted { p.red } else { p.subtext0 },',
             'color: if entry.muted { p.red } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBB: a system-sound row is raised off the panel",
        SND,
        [
            ('height: 28.0,\n                color: p.mantle,',
             'height: 28.0,\n                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCC: a system sound's label drops to secondary text",
        SND,
        [
            ('text: label.into(),\n                font_size: 12.0,\n                color: p.text,',
             'text: label.into(),\n                font_size: 12.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDD: an enabled sound's status follows the accent",
        SND,
        [
            ('color: if sc.enabled { p.green } else { p.overlay0 },',
             'color: if sc.enabled { p.accent } else { p.overlay0 },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEE: a disabled sound's status is promoted to secondary text",
        SND,
        [
            ('color: if sc.enabled { p.green } else { p.overlay0 },',
             'color: if sc.enabled { p.green } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFF: a custom sound's name is promoted to body text",
        SND,
        [
            ('text: custom.into(),\n                font_size: 12.0,\n                color: p.subtext0,',
             'text: custom.into(),\n                font_size: 12.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGG: the spatial heading follows the accent",
        SND,
        [
            ('text: "Spatial Audio".into(),\n            font_size: 14.0,\n            color: p.lavender,',
             'text: "Spatial Audio".into(),\n            font_size: 14.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHH: the selected spatial mode stops being raised",
        SND,
        [
            ('width,\n                height: 32.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'width,\n                height: 32.0,\n                color: if active { p.mantle } else { p.mantle },'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIII: every spatial mode looks selected",
        SND,
        [
            ('width,\n                height: 32.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'width,\n                height: 32.0,\n                color: if active { p.surface0 } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJ: the selected spatial mode's label is frozen back to Mocha blue",
        SND,
        [
            ('color: if active { p.accent } else { p.text },',
             'color: if active { guitk::color::Color::from_hex(0x89B4FA) } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_colour_the_sound_panel_draws_comes_from_its_palette',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'the_three_accent_sites_follow_the_accent',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKK: the selected spatial mode's label reads like an unselected one",
        SND,
        [
            ('color: if active { p.accent } else { p.text },',
             'color: if active { p.text } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'the_three_accent_sites_follow_the_accent',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLL: every spatial mode's label takes the accent",
        SND,
        [
            ('color: if active { p.accent } else { p.text },',
             'color: if active { p.accent } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMM: a volume bar's track is frozen back to Mocha surface1",
        SND,
        [
            ('height: bar_h,\n            color: p.surface1,',
             'height: bar_h,\n            color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_sound_panel_draws_comes_from_its_palette',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNN: a volume bar's track drops a step, to surface0",
        SND,
        [
            ('height: bar_h,\n            color: p.surface1,',
             'height: bar_h,\n            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOO: a volume bar's fill is frozen back to Mocha blue",
        SND,
        [
            ('let fill_color = if muted { p.red } else { p.accent };',
             'let fill_color = if muted { p.red } else { guitk::color::Color::from_hex(0x89B4FA) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_sound_panel_draws_comes_from_its_palette',
            'the_three_accent_sites_follow_the_accent',
            'a_muted_volume_bar_never_looks_like_an_unmuted_one',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPP: a muted volume bar stops going red",
        SND,
        [
            ('let fill_color = if muted { p.red } else { p.accent };',
             'let fill_color = if muted { p.accent } else { p.accent };'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'a_muted_volume_bar_never_looks_like_an_unmuted_one',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQ: an unmuted volume bar goes red too",
        SND,
        [
            ('let fill_color = if muted { p.red } else { p.accent };',
             'let fill_color = if muted { p.red } else { p.red };'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'the_three_accent_sites_follow_the_accent',
            'a_muted_volume_bar_never_looks_like_an_unmuted_one',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRR: a label-value row's label drops to the faintest role",
        SND,
        [
            ('text: label.into(),\n            font_size: 13.0,\n            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.5),',
             'text: label.into(),\n            font_size: 13.0,\n            color: p.overlay0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.5),'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSS: a label-value row's value drops to secondary text",
        SND,
        [
            ('text: value.into(),\n            font_size: 13.0,\n            color: p.text,',
             'text: value.into(),\n            font_size: 13.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTT: a toggle row's label drops to the faintest role",
        SND,
        [
            ('text: label.into(),\n            font_size: 13.0,\n            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.6),',
             'text: label.into(),\n            font_size: 13.0,\n            color: p.overlay0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.6),'),
        ],
        ["desktop"],
        [
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUU: an on toggle's pill follows the accent",
        SND,
        [
            ('let bg = if on { p.green } else { p.surface1 };',
             'let bg = if on { p.accent } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVV: an off toggle's pill is raised a step",
        SND,
        [
            ('let bg = if on { p.green } else { p.surface1 };',
             'let bg = if on { p.green } else { p.surface2 };'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWW: a toggle's knob drops to secondary text",
        SND,
        [
            ('width: 16.0,\n            height: 16.0,\n            color: p.text,',
             'width: 16.0,\n            height: 16.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXX: the master volume label never admits to being muted",
        SND,
        [
            ('                    " (Muted)"\n                } else {',
             '                    ""\n                } else {'),
        ],
        ["desktop"],
        [
            # A fixture that stops reaching an arm stops checking it, silently.
            # These four exist to prove the coverage test can tell.
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYY: the monitor row is drawn only when monitoring is off",
        SND,
        [
            ('if mic.monitor {\n            y = Self::render_label_val(',
             'if !mic.monitor {\n            y = Self::render_label_val('),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'monitor_loopback_renders_volume',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZ: a disabled system sound still reads as on",
        SND,
        [
            ('let status = if sc.enabled { "On" } else { "Off" };',
             'let status = "On";'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'every_pair_this_panel_uses_to_tell_things_apart_stays_apart',
            'nothing_that_reports_a_state_follows_the_accent',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAA: no tab is ever marked active",
        SND,
        [
            ('let active = self.active_tab == i;',
             'let active = false;'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_sound_panel_has',
            'every_text_the_sound_panel_draws_is_in_the_role_it_claims',
            'every_rectangle_the_sound_panel_draws_is_in_the_role_it_claims',
            'the_three_accent_sites_follow_the_accent',
            'every_pair_this_panel_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    # ---- osd.rs (module 24 of 49) -------------------------------------------
    (
        "AAAAAAAAAAAAAAAAAAAAAA: the overlay's shadow becomes a role instead of an absence of light",
        OSD,
        [
            ('color: Color::rgba(0, 0, 0, base_alpha / 2),',
             'color: Color::rgba(p.crust.r, p.crust.g, p.crust.b, base_alpha / 2),'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBB: the overlay's shadow stops following the overlay's own fade",
        OSD,
        [
            ('color: Color::rgba(0, 0, 0, base_alpha / 2),',
             'color: Color::rgba(0, 0, 0, base_alpha),'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCC: the overlay's panel is frozen back to Mocha base",
        OSD,
        [
            ('color: Color::rgba(p.base.r, p.base.g, p.base.b, base_alpha),',
             'color: Color::rgba(0x1E, 0x1E, 0x2E, base_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDD: the overlay's panel becomes a sidebar's mantle",
        OSD,
        [
            ('color: Color::rgba(p.base.r, p.base.g, p.base.b, base_alpha),',
             'color: Color::rgba(p.mantle.r, p.mantle.g, p.mantle.b, base_alpha),'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEE: the overlay's panel follows the accent",
        OSD,
        [
            ('color: Color::rgba(p.base.r, p.base.g, p.base.b, base_alpha),',
             'color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, base_alpha),'),
        ],
        ["desktop"],
        [
            'no_colour_the_overlay_draws_ever_follows_the_accent',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFF: the overlay's border is frozen back to Mocha surface1",
        OSD,
        [
            ('color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, base_alpha),',
             'color: Color::rgba(0x45, 0x47, 0x5A, base_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGG: the overlay's border sinks into its own fill",
        OSD,
        [
            ('color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, base_alpha),',
             'color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, base_alpha),'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHH: the slider's icon is frozen back to Mocha blue",
        OSD,
        [
            ('font_size: icon_size,\n            color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),',
             'font_size: icon_size,\n            color: Color::rgba(0x89, 0xB4, 0xFA, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIII: the slider's icon stops saying which kind of slider it is",
        OSD,
        [
            ('font_size: icon_size,\n            color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),',
             'font_size: icon_size,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJ: the slider's label is frozen back to Mocha text",
        OSD,
        [
            ('font_size: 14.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: None,',
             'font_size: 14.0,\n            color: Color::rgba(0xCD, 0xD6, 0xF4, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: None,'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKK: the slider's label drops to secondary text",
        OSD,
        [
            ('font_size: 14.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: None,',
             'font_size: 14.0,\n            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: None,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLL: the slider's track is frozen back to Mocha surface0",
        OSD,
        [
            ('color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, text_alpha),',
             'color: Color::rgba(0x31, 0x32, 0x44, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMM: the slider's track lightens one step",
        OSD,
        [
            ('color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, text_alpha),',
             'color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNN: the slider's fill is frozen back to Mocha blue",
        OSD,
        [
            ('width: fill_w,\n                height: track_h,\n                color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),',
             'width: fill_w,\n                height: track_h,\n                color: Color::rgba(0x89, 0xB4, 0xFA, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOO: the slider's fill follows the accent, as if you could drag an OSD",
        OSD,
        [
            ('width: fill_w,\n                height: track_h,\n                color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),',
             'width: fill_w,\n                height: track_h,\n                color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'no_colour_the_overlay_draws_ever_follows_the_accent',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPP: the slider's knob is frozen back to Mocha text",
        OSD,
        [
            ('height: 10.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),',
             'height: 10.0,\n            color: Color::rgba(0xCD, 0xD6, 0xF4, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQ: the slider's knob sinks into its own track",
        OSD,
        [
            ('height: 10.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),',
             'height: 10.0,\n            color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRR: a slider at zero draws a fill anyway',
        OSD,
        [
            ('if fill_w > 0.0 {',
             'if fill_w >= 0.0 {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSS: a slider never draws its fill at all',
        OSD,
        [
            ('if fill_w > 0.0 {',
             'if fill_w < 0.0 {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTT: the music note is frozen back to Mocha lavender',
        OSD,
        [
            ('font_size: 28.0,\n            color: Color::rgba(p.lavender.r, p.lavender.g, p.lavender.b, text_alpha),',
             'font_size: 28.0,\n            color: Color::rgba(0xB4, 0xBE, 0xFE, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUU: the music note stops being a music note and becomes text',
        OSD,
        [
            ('font_size: 28.0,\n            color: Color::rgba(p.lavender.r, p.lavender.g, p.lavender.b, text_alpha),',
             'font_size: 28.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVV: the track title is frozen back to Mocha text',
        OSD,
        [
            ('font_size: 14.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: Some(max_text_w),',
             'font_size: 14.0,\n            color: Color::rgba(0xCD, 0xD6, 0xF4, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: Some(max_text_w),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWW: the track title drops to the artist's secondary text",
        OSD,
        [
            ('font_size: 14.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: Some(max_text_w),',
             'font_size: 14.0,\n            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: Some(max_text_w),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXX: the track title follows the accent',
        OSD,
        [
            ('font_size: 14.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: Some(max_text_w),',
             'font_size: 14.0,\n            color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, text_alpha),\n            font_weight: FontWeightHint::Bold,\n            max_width: Some(max_text_w),'),
        ],
        ["desktop"],
        [
            'no_colour_the_overlay_draws_ever_follows_the_accent',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYY: the artist line is frozen back to Mocha subtext0',
        OSD,
        [
            ('font_size: 12.0,\n            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha),',
             'font_size: 12.0,\n            color: Color::rgba(0xA6, 0xAD, 0xC8, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZ: the artist line is promoted to the title's own weight of text",
        OSD,
        [
            ('font_size: 12.0,\n            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha),',
             'font_size: 12.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAA: the album line is frozen back to Mocha subtext0',
        OSD,
        [
            ('color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha / 2),',
             'color: Color::rgba(0xA6, 0xAD, 0xC8, text_alpha / 2),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBB: the album line is promoted to primary text',
        OSD,
        [
            ('color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha / 2),',
             'color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha / 2),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCC: the media bar is frozen back to Mocha lavender',
        OSD,
        [
            ('color: Color::rgba(p.lavender.r, p.lavender.g, p.lavender.b, text_alpha / 3),',
             'color: Color::rgba(0xB4, 0xBE, 0xFE, text_alpha / 3),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDD: the media bar follows the accent',
        OSD,
        [
            ('color: Color::rgba(p.lavender.r, p.lavender.g, p.lavender.b, text_alpha / 3),',
             'color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, text_alpha / 3),'),
        ],
        ["desktop"],
        [
            'no_colour_the_overlay_draws_ever_follows_the_accent',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEE: a track with no album draws an empty album line',
        OSD,
        [
            ('if !album.is_empty() {',
             'if true {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFF: a track with an album never draws it',
        OSD,
        [
            ('if !album.is_empty() {',
             'if false {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'osd_text_is_bounded_by_width_not_pre_truncated',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGG: the notice icon is frozen back to Mocha red',
        OSD,
        [
            ('font_size: 20.0,\n            color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),',
             'font_size: 20.0,\n            color: Color::rgba(0xF3, 0x8B, 0xA8, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHH: the notice icon stops saying what kind of notice it is',
        OSD,
        [
            ('font_size: 20.0,\n            color: Color::rgba(accent.r, accent.g, accent.b, text_alpha),',
             'font_size: 20.0,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIII: the notice label is frozen back to Mocha text',
        OSD,
        [
            ('font_size: OSD_LABEL_SIZE,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),',
             'font_size: OSD_LABEL_SIZE,\n            color: Color::rgba(0xCD, 0xD6, 0xF4, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJ: the notice label drops to secondary text',
        OSD,
        [
            ('font_size: OSD_LABEL_SIZE,\n            color: Color::rgba(p.text.r, p.text.g, p.text.b, text_alpha),',
             'font_size: OSD_LABEL_SIZE,\n            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, text_alpha),'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKK: an unmuted volume overlay reads as muted',
        OSD,
        [
            ('if *muted { p.red } else { p.blue },',
             'p.red,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLL: a muted volume overlay reads as unmuted',
        OSD,
        [
            ('if *muted { p.red } else { p.blue },',
             'p.blue,'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMM: brightness takes volume's blue, so the pair collapses",
        OSD,
        [
            ('p.yellow,\n                    commands,',
             'p.blue,\n                    commands,'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNN: brightness follows the accent',
        OSD,
        [
            ('p.yellow,\n                    commands,',
             'p.accent,\n                    commands,'),
        ],
        ["desktop"],
        [
            'no_colour_the_overlay_draws_ever_follows_the_accent',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'volume_and_brightness_stay_a_pair_you_can_tell_apart',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOO: play/pause loses its own colour to plain text',
        OSD,
        [
            ('p, ox, oy, osd_w, text_alpha, icon, label, p.lavender, commands,',
             'p, ox, oy, osd_w, text_alpha, icon, label, p.text, commands,'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPP: a lock that is on reads as off',
        OSD,
        [
            ('let color = if *active { p.green } else { p.subtext0 };',
             'let color = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQ: a lock that is off reads as on',
        OSD,
        [
            ('let color = if *active { p.green } else { p.subtext0 };',
             'let color = p.green;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRR: an ejected device still reads as connected',
        OSD,
        [
            ('let color = if *ejected { p.subtext0 } else { p.green };',
             'let color = p.green;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSS: a connected device reads as ejected',
        OSD,
        [
            ('let color = if *ejected { p.subtext0 } else { p.green };',
             'let color = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTT: the screenshot notice is frozen back to Mocha green',
        OSD,
        [
            ('"\\u{1F4F7}",\n                    &label,\n                    p.green,',
             '"\\u{1F4F7}",\n                    &label,\n                    Color::from_hex(0xA6E3A1),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUU: the screenshot notice follows the accent',
        OSD,
        [
            ('"\\u{1F4F7}",\n                    &label,\n                    p.green,',
             '"\\u{1F4F7}",\n                    &label,\n                    p.accent,'),
        ],
        ["desktop"],
        [
            'no_colour_the_overlay_draws_ever_follows_the_accent',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVV: a muted microphone still reads as live',
        OSD,
        [
            ('let color = if *muted { p.red } else { p.green };',
             'let color = p.green;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWW: a live microphone reads as muted',
        OSD,
        [
            ('let color = if *muted { p.red } else { p.green };',
             'let color = p.red;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXX: a dropped network connection still reads as connected',
        OSD,
        [
            ('let color = if *connected { p.green } else { p.red };',
             'let color = p.green;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYY: a live network connection reads as dropped',
        OSD,
        [
            ('let color = if *connected { p.green } else { p.red };',
             'let color = p.red;'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZ: a low battery stops being a warning and becomes a caution',
        OSD,
        [
            ('"\\u{1F50B}",\n                    &label,\n                    p.red,',
             '"\\u{1F50B}",\n                    &label,\n                    p.peach,'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAA: a low battery is frozen back to Mocha red',
        OSD,
        [
            ('"\\u{1F50B}",\n                    &label,\n                    p.red,',
             '"\\u{1F50B}",\n                    &label,\n                    Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBB: the Info icon stops being informational and turns into a success',
        OSD,
        [
            ('OsdIcon::Info => ("\\u{2139}", p.blue),',
             'OsdIcon::Info => ("\\u{2139}", p.green),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_pair_this_module_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCC: the Success icon stops being green',
        OSD,
        [
            ('OsdIcon::Success => ("\\u{2705}", p.green),',
             'OsdIcon::Success => ("\\u{2705}", p.blue),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_pair_this_module_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDD: the Warning icon is frozen back to Mocha yellow',
        OSD,
        [
            ('OsdIcon::Warning => ("\\u{26A0}", p.yellow),',
             'OsdIcon::Warning => ("\\u{26A0}", Color::from_hex(0xF9E2AF)),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEE: the Error icon collides with the battery warning',
        OSD,
        [
            ('OsdIcon::Error => ("\\u{274C}", p.red),',
             'OsdIcon::Error => ("\\u{274C}", p.peach),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_pair_this_module_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFF: the Speaker icon stops sharing the volume overlay's blue",
        OSD,
        [
            ('OsdIcon::Speaker => ("\\u{1F50A}", p.blue),',
             'OsdIcon::Speaker => ("\\u{1F50A}", p.text),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGG: the Brightness icon stops sharing the brightness overlay's yellow",
        OSD,
        [
            ('OsdIcon::Brightness => ("\\u{2600}", p.yellow),',
             'OsdIcon::Brightness => ("\\u{2600}", p.green),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHH: the Network icon turns into an error',
        OSD,
        [
            ('OsdIcon::Network => ("\\u{1F310}", p.green),',
             'OsdIcon::Network => ("\\u{1F310}", p.red),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIII: the Battery icon loses its peach and collides with the error red',
        OSD,
        [
            ('OsdIcon::Battery => ("\\u{1F50B}", p.peach),',
             'OsdIcon::Battery => ("\\u{1F50B}", p.red),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_pair_this_module_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJ: the Lock icon loses its lavender and collides with the info blue',
        OSD,
        [
            ('OsdIcon::Lock => ("\\u{1F512}", p.lavender),',
             'OsdIcon::Lock => ("\\u{1F512}", p.blue),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
            'every_pair_this_module_uses_to_tell_things_apart_stays_apart',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKK: the Camera icon stops confirming anything',
        OSD,
        [
            ('OsdIcon::Camera => ("\\u{1F4F7}", p.green),',
             'OsdIcon::Camera => ("\\u{1F4F7}", p.subtext0),'),
        ],
        ["desktop"],
        [
            'every_kind_draws_its_icon_in_the_colour_that_kind_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLL: the medium volume icon collapses into the low one',
        OSD,
        [
            ('} else if level < 66 {\n        "\\u{1F509}" // medium',
             '} else if level < 66 {\n        "\\u{1F508}" // medium'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
            'volume_icon_levels',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMM: the settings title is frozen back to Mocha text',
        OSD,
        [
            ('font_size: 18.0,\n            color: p.text,',
             'font_size: 18.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNN: the settings title drops to a heading's secondary text",
        OSD,
        [
            ('font_size: 18.0,\n            color: p.text,',
             'font_size: 18.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOO: the enable pill is frozen back to Mocha green',
        OSD,
        [
            ('let enable_color = if self.config.enabled {\n            p.green\n        } else {\n            p.subtext0\n        };',
             'let enable_color = if self.config.enabled {\n            Color::from_hex(0xA6E3A1)\n        } else {\n            p.subtext0\n        };'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPP: a disabled OSD's pill still reads as enabled",
        OSD,
        [
            ('let enable_color = if self.config.enabled {\n            p.green\n        } else {\n            p.subtext0\n        };',
             'let enable_color = p.green;'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQ: the enable pill reports its state in the accent',
        OSD,
        [
            ('let enable_color = if self.config.enabled {\n            p.green\n        } else {\n            p.subtext0\n        };',
             'let enable_color = p.accent;'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_state_follows_the_accent',
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRR: the pill's knob is frozen back to Mocha text",
        OSD,
        [
            ('height: 16.0,\n            color: p.text,',
             'height: 16.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSS: the pill's knob sinks into an unselected grey",
        OSD,
        [
            ('height: 16.0,\n            color: p.text,',
             'height: 16.0,\n            color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTT: the enable label is frozen back to Mocha text',
        OSD,
        [
            ('text: "Enable OSD overlays".to_string(),\n            font_size: 14.0,\n            color: p.text,',
             'text: "Enable OSD overlays".to_string(),\n            font_size: 14.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUU: the enable label drops to secondary text',
        OSD,
        [
            ('text: "Enable OSD overlays".to_string(),\n            font_size: 14.0,\n            color: p.text,',
             'text: "Enable OSD overlays".to_string(),\n            font_size: 14.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVV: the Position heading is frozen back to Mocha subtext0',
        OSD,
        [
            ('text: "Position".to_string(),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: "Position".to_string(),\n            font_size: 13.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWW: the Position heading is promoted to primary text',
        OSD,
        [
            ('text: "Position".to_string(),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: "Position".to_string(),\n            font_size: 13.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXX: the selected position dot stops following the accent',
        OSD,
        [
            ('let dot_color = if selected { p.accent } else { p.surface1 };',
             'let dot_color = if selected { p.blue } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYY: every position dot reads as selected',
        OSD,
        [
            ('let dot_color = if selected { p.accent } else { p.surface1 };',
             'let dot_color = p.accent;'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZ: an unselected position's label reads as selected",
        OSD,
        [
            ('color: if selected { p.text } else { p.subtext0 },',
             'color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAA: the selected position's label is frozen back to Mocha text",
        OSD,
        [
            ('color: if selected { p.text } else { p.subtext0 },',
             'color: if selected { Color::from_hex(0xCDD6F4) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBB: the Timeout heading is frozen back to Mocha subtext0',
        OSD,
        [
            ('text: format!("Timeout: {}ms", self.config.timeout_ms),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: format!("Timeout: {}ms", self.config.timeout_ms),\n            font_size: 13.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCC: the Timeout heading is promoted to primary text',
        OSD,
        [
            ('text: format!("Timeout: {}ms", self.config.timeout_ms),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: format!("Timeout: {}ms", self.config.timeout_ms),\n            font_size: 13.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDD: the timeout slider's track is frozen back to Mocha surface0",
        OSD,
        [
            ('height: 4.0,\n            color: p.surface0,',
             'height: 4.0,\n            color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEE: the timeout slider's track takes the accent too, so the fill vanishes into it",
        OSD,
        [
            ('height: 4.0,\n            color: p.surface0,',
             'height: 4.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFF: the timeout slider's fill stops following the accent",
        OSD,
        [
            ('height: 4.0,\n            color: p.accent,',
             'height: 4.0,\n            color: p.blue,'),
        ],
        ["desktop"],
        [
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGG: the Show-OSD-for heading is frozen back to Mocha subtext0',
        OSD,
        [
            ('text: "Show OSD for:".to_string(),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: "Show OSD for:".to_string(),\n            font_size: 13.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHH: the Show-OSD-for heading is promoted to primary text',
        OSD,
        [
            ('text: "Show OSD for:".to_string(),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: "Show OSD for:".to_string(),\n            font_size: 13.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIII: the checkbox is frozen back to Mocha green',
        OSD,
        [
            ('let check_color = if *enabled { p.green } else { p.surface1 };',
             'let check_color = if *enabled { Color::from_hex(0xA6E3A1) } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJ: an unchecked box still reads as checked',
        OSD,
        [
            ('let check_color = if *enabled { p.green } else { p.surface1 };',
             'let check_color = p.green;'),
        ],
        ["desktop"],
        [
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKK: the checkbox reports its state in the accent',
        OSD,
        [
            ('let check_color = if *enabled { p.green } else { p.surface1 };',
             'let check_color = p.accent;'),
        ],
        ["desktop"],
        [
            'nothing_that_reports_a_state_follows_the_accent',
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLL: the tick inside the box is frozen back to Mocha base',
        OSD,
        [
            ('color: appearance::readable_on(p.green),',
             'color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'ink_drawn_on_a_coloured_fill_is_readable_in_both_modes',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMM: the tick is named instead of derived from the box it sits on',
        OSD,
        [
            ('color: appearance::readable_on(p.green),',
             'color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'ink_drawn_on_a_coloured_fill_is_readable_in_both_modes',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNN: an unchecked box draws a tick anyway',
        OSD,
        [
            ('if *enabled {\n                commands.push(RenderCommand::Text {',
             'if true {\n                commands.push(RenderCommand::Text {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOO: a checked box never draws its tick',
        OSD,
        [
            ('if *enabled {\n                commands.push(RenderCommand::Text {',
             'if false {\n                commands.push(RenderCommand::Text {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_the_osd_has',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'ink_drawn_on_a_coloured_fill_is_readable_in_both_modes',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPP: the toggle labels are frozen back to Mocha text',
        OSD,
        [
            ('font_size: 12.0,\n                color: p.text,',
             'font_size: 12.0,\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQ: the toggle labels drop to secondary text',
        OSD,
        [
            ('font_size: 12.0,\n                color: p.text,',
             'font_size: 12.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRR: the Preview button stops following the accent',
        OSD,
        [
            ('height: 32.0,\n            color: p.accent,',
             'height: 32.0,\n            color: p.blue,'),
        ],
        ["desktop"],
        [
            'the_settings_panel_has_exactly_three_accent_sites',
            'every_rectangle_the_osd_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSS: the Preview button's label is frozen back to Mocha base",
        OSD,
        [
            ('color: p.on_accent(),',
             'color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_osd_draws_comes_from_its_palette',
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'ink_drawn_on_a_coloured_fill_is_readable_in_both_modes',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTT: the Preview button's label is named instead of derived from the accent",
        OSD,
        [
            ('color: p.on_accent(),',
             'color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_the_osd_draws_is_in_the_role_it_claims',
            'ink_drawn_on_a_coloured_fill_is_readable_in_both_modes',
        ],
    ),

    # ---- privacy_settings.rs (module 25 of 49) ----
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAA: the panel background is frozen back to Mocha base',
        PRIV,
        [
            ('height: 900.0,\n            color: p.base,',
             'height: 900.0,\n            color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBB: the panel background sinks to the recessed role',
        PRIV,
        [
            ('height: 900.0,\n            color: p.base,',
             'height: 900.0,\n            color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCC: the panel title is frozen back to Mocha text',
        PRIV,
        [
            ('font_size: 20.0,\n            color: p.text,',
             'font_size: 20.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDD: the panel title drops to secondary text',
        PRIV,
        [
            ('font_size: 20.0,\n            color: p.text,',
             'font_size: 20.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEE: the tab strip highlights every tab except the one you are on',
        PRIV,
        [
            ('height: 30.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'height: 30.0,\n                color: if active { p.mantle } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFF: the selected tab's fill takes the accent as well as its label",
        PRIV,
        [
            ('height: 30.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'height: 30.0,\n                color: if active { p.accent } else { p.mantle },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGG: the active tab's label is frozen back to Mocha blue",
        PRIV,
        [
            ('font_size: 12.0,\n                color: if active { p.accent } else { p.subtext0 },',
             'font_size: 12.0,\n                color: if active { Color::from_hex(0x89B4FA) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'every_colour_this_panel_draws_comes_from_its_palette',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHH: the active tab's label names blue instead of following the accent",
        PRIV,
        [
            ('font_size: 12.0,\n                color: if active { p.accent } else { p.subtext0 },',
             'font_size: 12.0,\n                color: if active { p.blue } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIII: every tab but the active one reads as selected',
        PRIV,
        [
            ('font_size: 12.0,\n                color: if active { p.accent } else { p.subtext0 },',
             'font_size: 12.0,\n                color: if active { p.subtext0 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJ: the resource heading is frozen back to Mocha lavender',
        PRIV,
        [
            ('font_size: 16.0,\n                color: p.lavender,',
             'font_size: 16.0,\n                color: Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKK: the resource heading takes the accent, so a category reads as a position',
        PRIV,
        [
            ('font_size: 16.0,\n                color: p.lavender,',
             'font_size: 16.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLL: the resource description drops to the dimmest role',
        PRIV,
        [
            ('font_size: 12.0,\n                color: p.subtext0,',
             'font_size: 12.0,\n                color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMM: the no-apps notice is frozen back to Mocha overlay0',
        PRIV,
        [
            ('font_size: 12.0,\n                    color: p.overlay0,',
             'font_size: 12.0,\n                    color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNN: the no-apps notice is promoted to ordinary secondary text',
        PRIV,
        [
            ('font_size: 12.0,\n                    color: p.overlay0,',
             'font_size: 12.0,\n                    color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOO: an app row's background rises out of its well",
        PRIV,
        [
            ('height: 32.0,\n                        color: p.mantle,',
             'height: 32.0,\n                        color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPP: an app's name is frozen back to Mocha text",
        PRIV,
        [
            ('text: app.app_name.clone(),\n                        font_size: 13.0,\n                        color: p.text,',
             'text: app.app_name.clone(),\n                        font_size: 13.0,\n                        color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQ: an app's name drops to secondary text",
        PRIV,
        [
            ('text: app.app_name.clone(),\n                        font_size: 13.0,\n                        color: p.text,',
             'text: app.app_name.clone(),\n                        font_size: 13.0,\n                        color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRR: an app's permission state stops being drawn in that state's colour",
        PRIV,
        [
            ('color: app.state.color(p),',
             'color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSS: the access counter is promoted to secondary text',
        PRIV,
        [
            ('font_size: 11.0,\n                        color: p.overlay0,',
             'font_size: 11.0,\n                        color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTT: an overview row's background rises out of its well",
        PRIV,
        [
            ('height: 40.0,\n                    color: p.mantle,',
             'height: 40.0,\n                    color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUU: an overview row's resource name drops to secondary text",
        PRIV,
        [
            ('font_size: 14.0,\n                    color: p.text,',
             'font_size: 14.0,\n                    color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVV: the overview status line reports allowed as denied and denied as allowed',
        PRIV,
        [
            ('color: if enabled { p.green } else { p.red },',
             'color: if enabled { p.red } else { p.green },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWW: an enabled resource reports its state in the accent',
        PRIV,
        [
            ('color: if enabled { p.green } else { p.red },',
             'color: if enabled { p.accent } else { p.red },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXX: the overview status line is frozen back to Mocha green',
        PRIV,
        [
            ('color: if enabled { p.green } else { p.red },',
             'color: if enabled { Color::from_hex(0xA6E3A1) } else { p.red },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYY: the overview description is promoted to secondary text',
        PRIV,
        [
            ('font_size: 10.0,\n                    color: p.overlay0,',
             'font_size: 10.0,\n                    color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZ: the empty-log notice is frozen back to Mocha overlay0',
        PRIV,
        [
            ('text: "No activity recorded yet.".into(),\n                font_size: 13.0,\n                color: p.overlay0,',
             'text: "No activity recorded yet.".into(),\n                font_size: 13.0,\n                color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAA: the empty-log notice is promoted to secondary text',
        PRIV,
        [
            ('text: "No activity recorded yet.".into(),\n                font_size: 13.0,\n                color: p.overlay0,',
             'text: "No activity recorded yet.".into(),\n                font_size: 13.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBB: the activity count heading drops to secondary text',
        PRIV,
        [
            ('text: format!("{} recent access events", log.len()),\n            font_size: 13.0,\n            color: p.text,',
             'text: format!("{} recent access events", log.len()),\n            font_size: 13.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCC: an activity row's background rises out of its well",
        PRIV,
        [
            ('height: 28.0,\n                color: p.mantle,',
             'height: 28.0,\n                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDD: the activity log reports allowed accesses as denied and denied as allowed',
        PRIV,
        [
            ('let color = if entry.allowed { p.green } else { p.red };',
             'let color = if entry.allowed { p.red } else { p.green };'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEE: an allowed access is logged in the accent instead of green',
        PRIV,
        [
            ('let color = if entry.allowed { p.green } else { p.red };',
             'let color = if entry.allowed { p.accent } else { p.red };'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFF: the Telemetry heading takes the accent, so a category reads as a position',
        PRIV,
        [
            ('text: "Telemetry".into(),\n            font_size: 14.0,\n            color: p.lavender,',
             'text: "Telemetry".into(),\n            font_size: 14.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGG: the Telemetry heading is frozen back to Mocha lavender',
        PRIV,
        [
            ('text: "Telemetry".into(),\n            font_size: 14.0,\n            color: p.lavender,',
             'text: "Telemetry".into(),\n            font_size: 14.0,\n            color: Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHH: every telemetry level looks selected except the one that is',
        PRIV,
        [
            ('height: 28.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'height: 28.0,\n                color: if active { p.mantle } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIII: the selected telemetry row's fill takes the accent as well as its label",
        PRIV,
        [
            ('height: 28.0,\n                color: if active { p.surface0 } else { p.mantle },',
             'height: 28.0,\n                color: if active { p.accent } else { p.mantle },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJ: every telemetry label reads as selected except the one that is',
        PRIV,
        [
            ('font_size: 13.0,\n                color: if active { p.accent } else { p.text },',
             'font_size: 13.0,\n                color: if active { p.text } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKK: the selected telemetry label names blue instead of following the accent',
        PRIV,
        [
            ('font_size: 13.0,\n                color: if active { p.accent } else { p.text },',
             'font_size: 13.0,\n                color: if active { p.blue } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLL: the Other heading drops to secondary text',
        PRIV,
        [
            ('text: "Other".into(),\n            font_size: 14.0,\n            color: p.lavender,',
             'text: "Other".into(),\n            font_size: 14.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMM: a toggle's label is promoted to primary text",
        PRIV,
        [
            ('text: label.into(),\n            font_size: 13.0,\n            color: p.subtext0,',
             'text: label.into(),\n            font_size: 13.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNN: every toggle pill reports the opposite of its switch',
        PRIV,
        [
            ('let bg = if on { p.green } else { p.surface1 };',
             'let bg = if on { p.surface1 } else { p.green };'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOO: a switched-on toggle reports its state in the accent',
        PRIV,
        [
            ('let bg = if on { p.green } else { p.surface1 };',
             'let bg = if on { p.accent } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
            'only_the_two_selection_labels_follow_the_accent',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPP: the on-pill is frozen back to Mocha green',
        PRIV,
        [
            ('let bg = if on { p.green } else { p.surface1 };',
             'let bg = if on { Color::from_hex(0xA6E3A1) } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQ: the toggle knob is frozen back to Mocha text',
        PRIV,
        [
            ('height: 16.0,\n            color: p.text,',
             'height: 16.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRR: the toggle knob dims to secondary text',
        PRIV,
        [
            ('height: 16.0,\n            color: p.text,',
             'height: 16.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSS: an allowed permission stops being green',
        PRIV,
        [
            ('Self::Allowed => p.green,',
             'Self::Allowed => p.blue,'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTT: a denied permission is reported in the same green as an allowed one',
        PRIV,
        [
            ('Self::Denied => p.red,',
             'Self::Denied => p.green,'),
        ],
        ["desktop"],
        [
            'allowed_and_denied_stay_apart_under_every_accent_and_mode',
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUU: an undecided permission is dressed up as an ordinary secondary label',
        PRIV,
        [
            ('Self::NotDecided => p.overlay0,',
             'Self::NotDecided => p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVV: an allowed permission follows the accent instead of meaning allowed',
        PRIV,
        [
            ('Self::Allowed => p.green,',
             'Self::Allowed => p.accent,'),
        ],
        ["desktop"],
        [
            'allowed_and_denied_stay_apart_under_every_accent_and_mode',
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'every_text_this_panel_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_labels_moves_when_the_accent_does',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWW: the detail view never shows its no-apps notice',
        PRIV,
        [
            ('if apps.is_empty() {',
             'if false {'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
            'the_fixtures_take_every_branch_this_panel_has',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXX: the activity tab never shows its empty state',
        PRIV,
        [
            ('if log.is_empty() {',
             'if false {'),
        ],
        ["desktop"],
        [
            'every_text_this_panel_draws_is_in_the_role_it_claims',
            'the_fixtures_take_every_branch_this_panel_has',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYY: the overview never reports how many apps are allowed',
        PRIV,
        [
            ('} else if count > 0 {',
             '} else if false {'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'the_fixtures_take_every_branch_this_panel_has',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZ: a denied access is logged with the tick of an allowed one',
        PRIV,
        [
            ('let status = if entry.allowed { "✓" } else { "✕" };',
             'let status = if entry.allowed { "✓" } else { "✓" };'),
        ],
        ["desktop"],
        [
            'every_choice_this_panel_makes_hands_over_the_role_it_claims',
            'the_fixtures_take_every_branch_this_panel_has',
        ],
    ),
    # ---- print_manager.rs (module 26 of 49) ----
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAA: the dialog box is frozen back to Mocha base',
        PRINTMGR,
        [
            ('            height: dh,\n            color: p.base,',
             '            height: dh,\n            color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBB: the dialog box sinks to the recessed role',
        PRINTMGR,
        [
            ('            height: dh,\n            color: p.base,',
             '            height: dh,\n            color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCC: the title is frozen back to Mocha text',
        PRINTMGR,
        [
            ('            font_size: 16.0,\n            color: p.text,',
             '            font_size: 16.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDD: the title drops to secondary text',
        PRINTMGR,
        [
            ('            font_size: 16.0,\n            color: p.text,',
             '            font_size: 16.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEE: the document name is promoted to primary text',
        PRINTMGR,
        [
            ('            text: format!("Document: {}", self.document_name),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: format!("Document: {}", self.document_name),\n            font_size: 12.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFF: the document name is frozen back to Mocha subtext',
        PRINTMGR,
        [
            ('            text: format!("Document: {}", self.document_name),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: format!("Document: {}", self.document_name),\n            font_size: 12.0,\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGG: the printer caption dims to secondary text',
        PRINTMGR,
        [
            ('            text: "Printer:".to_string(),\n            font_size: 12.0,\n            color: p.text,',
             '            text: "Printer:".to_string(),\n            font_size: 12.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHH: the selected printer's name stops following the accent",
        PRINTMGR,
        [
            ('            text: printer_name.to_string(),\n            font_size: 12.0,\n            color: p.accent,',
             '            text: printer_name.to_string(),\n            font_size: 12.0,\n            color: p.blue,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIII: the selected printer's name is drawn as ordinary body text",
        PRINTMGR,
        [
            ('            text: printer_name.to_string(),\n            font_size: 12.0,\n            color: p.accent,',
             '            text: printer_name.to_string(),\n            font_size: 12.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the printer field's fill takes the accent as well as its label",
        PRINTMGR,
        [
            ('            height: 24.0,\n            color: p.surface0,',
             '            height: 24.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKK: the printer field is frozen back to Mocha surface0',
        PRINTMGR,
        [
            ('            height: 24.0,\n            color: p.surface0,',
             '            height: 24.0,\n            color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLL: every settings label dims to secondary text',
        PRINTMGR,
        [
            ('                font_size: 12.0,\n                color: p.text,',
             '                font_size: 12.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMM: every settings value is promoted to primary text',
        PRINTMGR,
        [
            ('                text: value.clone(),\n                font_size: 12.0,\n                color: p.subtext0,',
             '                text: value.clone(),\n                font_size: 12.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNN: a validation error is reported in the accent instead of red',
        PRINTMGR,
        [
            ('                font_size: 11.0,\n                color: p.red,',
             '                font_size: 11.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOO: a validation error is frozen back to Mocha red',
        PRINTMGR,
        [
            ('                font_size: 11.0,\n                color: p.red,',
             '                font_size: 11.0,\n                color: Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPP: a validation error is drawn as ordinary body text',
        PRINTMGR,
        [
            ('                font_size: 11.0,\n                color: p.red,',
             '                font_size: 11.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the Print button stops being the accent-coloured default action',
        PRINTMGR,
        [
            ('            height: 28.0,\n            color: p.accent,',
             '            height: 28.0,\n            color: p.blue,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRR: the Print button is frozen back to Mocha blue',
        PRINTMGR,
        [
            ('            height: 28.0,\n            color: p.accent,',
             '            height: 28.0,\n            color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSS: the Print button's ink is named rather than derived from its fill",
        PRINTMGR,
        [
            ('            text: "Print".to_string(),\n            font_size: 12.0,\n            color: p.on_accent(),',
             '            text: "Print".to_string(),\n            font_size: 12.0,\n            color: p.base,'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
            'the_default_action_ink_stays_readable_in_both_modes',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTT: the Print button's ink is frozen back to Mocha base",
        PRINTMGR,
        [
            ('            text: "Print".to_string(),\n            font_size: 12.0,\n            color: p.on_accent(),',
             '            text: "Print".to_string(),\n            font_size: 12.0,\n            color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
            'the_default_action_ink_stays_readable_in_both_modes',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUU: the Cancel button is dressed up as a second default action',
        PRINTMGR,
        [
            ('            width: 80.0,\n            height: 28.0,\n            color: p.surface1,',
             '            width: 80.0,\n            height: 28.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVV: the Cancel button is frozen back to Mocha surface1',
        PRINTMGR,
        [
            ('            width: 80.0,\n            height: 28.0,\n            color: p.surface1,',
             '            width: 80.0,\n            height: 28.0,\n            color: Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_this_dialog_draws_comes_from_its_palette',
            'every_rectangle_this_dialog_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWW: the Cancel label is inked as though it sat on the accent',
        PRINTMGR,
        [
            ('            text: "Cancel".to_string(),\n            font_size: 12.0,\n            color: p.text,',
             '            text: "Cancel".to_string(),\n            font_size: 12.0,\n            color: p.on_accent(),'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXX: an offline printer stops reporting itself in red',
        PRINTMGR,
        [
            ('        if !self.online {\n            p.red',
             '        if !self.online {\n            p.yellow'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYY: an offline printer reports its state in the accent',
        PRINTMGR,
        [
            ('        if !self.online {\n            p.red',
             '        if !self.online {\n            p.accent'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: a busy printer is reported as ready',
        PRINTMGR,
        [
            ('        } else if self.queue_count > 0 {\n            p.yellow',
             '        } else if self.queue_count > 0 {\n            p.green'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAA: a ready printer is reported as offline',
        PRINTMGR,
        [
            ('        } else {\n            p.green\n        }\n    }',
             '        } else {\n            p.red\n        }\n    }'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBB: a queued job follows the accent instead of meaning queued',
        PRINTMGR,
        [
            ('            Self::Queued => p.blue,',
             '            Self::Queued => p.accent,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCC: a queued job is frozen back to Mocha blue',
        PRINTMGR,
        [
            ('            Self::Queued => p.blue,',
             '            Self::Queued => Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDD: a queued job is coloured as though it had finished',
        PRINTMGR,
        [
            ('            Self::Queued => p.blue,',
             '            Self::Queued => p.green,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_state_a_job_can_be_in_stays_apart_from_every_other',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEE: a printing job is indistinguishable from a queued one',
        PRINTMGR,
        [
            ('            Self::Printing => p.peach,',
             '            Self::Printing => p.blue,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_state_a_job_can_be_in_stays_apart_from_every_other',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFF: a paused job is indistinguishable from a cancelled one',
        PRINTMGR,
        [
            ('            Self::Paused => p.yellow,',
             '            Self::Paused => p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_state_a_job_can_be_in_stays_apart_from_every_other',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGG: a completed job is reported in the red of a failed one',
        PRINTMGR,
        [
            ('            Self::Completed => p.green,',
             '            Self::Completed => p.red,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_state_a_job_can_be_in_stays_apart_from_every_other',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHH: a failed job is reported in the green of a completed one',
        PRINTMGR,
        [
            ('            Self::Failed => p.red,',
             '            Self::Failed => p.green,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_state_a_job_can_be_in_stays_apart_from_every_other',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIII: a cancelled job follows the accent',
        PRINTMGR,
        [
            ('            Self::Cancelled => p.overlay0,',
             '            Self::Cancelled => p.accent,'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the hidden dialog draws itself anyway',
        PRINTMGR,
        [
            ('        if !self.visible {\n            return cmds;',
             '        if false {\n            return cmds;'),
        ],
        ["desktop"],
        [
            'nothing_but_the_selection_and_the_default_action_moves_with_the_accent',
            'test_dialog_render_hidden',
            'the_fixtures_take_every_branch_this_dialog_has',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the orientation row always reads Portrait',
        PRINTMGR,
        [
            ('                if self.settings.orientation == Orientation::Portrait {',
             '                if true {'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_this_dialog_has',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the duplex row always reads Off',
        PRINTMGR,
        [
            ('                if self.settings.duplex { "On" } else { "Off" }.to_string(),',
             '                if false { "On" } else { "Off" }.to_string(),'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_this_dialog_has',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the printer field never names the selected printer',
        PRINTMGR,
        [
            ('            .map(|p| p.name.as_str())\n            .unwrap_or("None");',
             '            .map(|_p| "None")\n            .unwrap_or("None");'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'the_fixtures_take_every_branch_this_dialog_has',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNN: validation errors are never drawn',
        PRINTMGR,
        [
            ('        for err in &self.validation_errors {',
             '        for err in &self.validation_errors[..0] {'),
        ],
        ["desktop"],
        [
            'every_text_this_dialog_draws_is_in_the_role_it_claims',
            'the_fixtures_take_every_branch_this_dialog_has',
        ],
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


def check(snap):
    """Report every defect whose pattern no longer matches, without building.

    A defect that has silently stopped applying is worse than no defect at
    all: the harness prints PATTERN NOT FOUND in the middle of a run that
    takes an hour, and the line scrolls past. Worse, nothing forces that run
    to happen — a rustfmt pass, a rename, or (as actually happened here) two
    file constants given the same name will rot a defect that nobody looks at
    again until the module it guards is next touched. This pass is seconds,
    takes no toolchain, and answers the only question that rots.
    """
    bad = 0
    amb = 0
    for name, path, edits, _pkgs, _expect in DEFECTS:
        text = snap[path].decode("utf-8")
        # A defect may list the same edit twice on purpose, to wound both of an
        # identical pair; only an ambiguity the defect does *not* acknowledge
        # is a problem, so count the listed copies and subtract them.
        listed = {}
        for old, _new in edits:
            listed[old] = listed.get(old, 0) + 1
        for i, (old, new) in enumerate(edits):
            if old not in text:
                print(f"PATTERN NOT FOUND  {name}\n    edit {i} in {path}")
                bad += 1
                break
            # Reported once per defect, on the first edit, against the
            # untouched file: an unacknowledged second match means the defect
            # silently patches whichever copy happens to come first, and will
            # move to the other one the day someone reorders the module.
            if i == 0:
                n = snap[path].decode("utf-8").count(old)
                if n > listed[old]:
                    print(f"AMBIGUOUS ({n} matches, {listed[old]} listed)  {name}")
                    amb += 1
            text = text.replace(old, new, 1)
    print(f"\n{len(DEFECTS)} defects, {bad} stale, {amb} ambiguous")
    return 1 if bad or amb else 0


def main():
    files = sorted({d[1] for d in DEFECTS})
    snap = {f: (ROOT / f).read_bytes() for f in files}
    digest = {f: hashlib.sha256(b).hexdigest() for f, b in snap.items()}
    print("snapshot:")
    for f in files:
        print(f"  {digest[f][:16]}  {f}")
    print()

    if sys.argv[1:2] == ["--check"]:
        sys.exit(check(snap))

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
                failed, out = run_tests(pkg)
                if failed is None:
                    # Carry the compiler's own words. A bare "did not compile"
                    # sends the reader looking for a bug in the *test*, when the
                    # cause is almost always that the defect cannot be spelled
                    # in the converted source's namespace -- e.g. reinstating a
                    # hex literal in a module whose `Color` import became
                    # test-only. Twenty minutes of a run were spent rediscovering
                    # that once; the error message was there all along.
                    why = [
                        ln.rstrip()
                        for ln in out.splitlines()
                        if ln.startswith("error[") or ln.startswith("error:")
                    ]
                    broke, note = True, f"{pkg} did not compile: " + (
                        "; ".join(why[:3]) if why else "no error line found"
                    )
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
                # Report the other direction too. A test that fired but was
                # not declared means the declaration understates what the
                # suite proves -- and the declarations are the only record of
                # that, so the next person to prune a "redundant" test has no
                # way to see what they would be giving up. This half used to
                # be an out-of-band audit script run by hand, which meant it
                # was run when someone remembered to.
                extra = [t for t in sorted(all_failed) if t not in expect]
                if extra:
                    verdict += f"  [UNDECLARED: {extra}]"
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

    def tally(mark):
        return sum(1 for _, v in verdicts if mark in v)

    escaped = tally("NO TEST FAILED") + tally("DID NOT COMPILE")
    print(
        f"\n{len(verdicts)} defects: {len(verdicts) - escaped} caught, "
        f"{escaped} escaped, {tally('[MISSING:')} under-caught, "
        f"{tally('[UNDECLARED:')} under-declared"
    )


if __name__ == "__main__":
    main()
