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

Three modes, cheapest first.

- `--check` matches every defect's pattern against the snapshot and builds
  nothing. Seconds, no toolchain, and it answers the only question that rots
  on its own: has a rename or a rustfmt pass stopped this defect applying?
- `--compile [names…]` applies each defect and runs `cargo check --all-targets`
  on its packages. This is the question `--check` *cannot* answer, and the
  distinction is not academic: a pattern can be findable, unambiguous and
  non-no-op and still leave source that is not Rust. Module 36 wrote four
  defects that replaced the wrong line of a two-line anchor, emitted two
  `color:` fields apiece, and were discovered an hour into the run they had
  already invalidated — see known-issues.md lesson 19. `--check` had passed
  all four.
- No flag: the real sweep. Apply, run the tests, restore, report.

`--compile` is a preflight, not a correctness gate. The full run detects a
broken defect too (`DID NOT COMPILE`); what the preflight buys is learning it
in minutes rather than at the end of an hour, before the run whose result it
spoils has been started. Filter it with the same names the real run takes, so a
new module's defects can be vetted without rebuilding against the older ones.

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
POWER = "gui/desktop/src/power.rs"
LOGIN = "gui/desktop/src/login_screen.rs"
TB = "gui/desktop/src/taskbar.rs"
LANG = "gui/desktop/src/language_settings.rs"
DAPP = "gui/desktop/src/default_apps.rs"
LAUN = "gui/desktop/src/launcher.rs"
RESMON = "gui/desktop/src/resmon.rs"
MOUSESET = "gui/desktop/src/mouse_settings.rs"
HOTKEYS = "gui/desktop/src/hotkeys.rs"
SCRCAP = "gui/desktop/src/screen_capture.rs"
SNAP = "gui/desktop/src/snap.rs"
DISP = "gui/desktop/src/display_settings.rs"
A11Y = "gui/desktop/src/accessibility_settings.rs"
FOCUS = "gui/desktop/src/focus_assist.rs"
PEEK = "gui/desktop/src/window_peek.rs"
ABOUT = "gui/desktop/src/about.rs"
CAL = "gui/desktop/src/calendar.rs"
SESS = "gui/desktop/src/session_mgr.rs"
FD = "gui/desktop/src/file_drop.rs"

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
    # ---- power.rs (module 27 of 49) ----
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the AC hint is frozen back to Mocha subtext0',
        POWER,
        [
            ('            text: "AC".to_string(),\n            color: p.subtext0,',
             '            text: "AC".to_string(),\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the AC hint is promoted to primary text',
        POWER,
        [
            ('            text: "AC".to_string(),\n            color: p.subtext0,',
             '            text: "AC".to_string(),\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the AC hint takes the accent',
        POWER,
        [
            ('            text: "AC".to_string(),\n            color: p.subtext0,',
             '            text: "AC".to_string(),\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the battery outline is frozen back to Mocha text',
        POWER,
        [
            ('        width: batt_w,\n        height: batt_h,\n        color: p.text,',
             '        width: batt_w,\n        height: batt_h,\n        color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the battery outline fades to secondary ink',
        POWER,
        [
            ('        width: batt_w,\n        height: batt_h,\n        color: p.text,',
             '        width: batt_w,\n        height: batt_h,\n        color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the battery outline takes the accent',
        POWER,
        [
            ('        width: batt_w,\n        height: batt_h,\n        color: p.text,',
             '        width: batt_w,\n        height: batt_h,\n        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the battery tip is frozen back to Mocha text',
        POWER,
        [
            ('        width: tip_w,\n        height: tip_h,\n        color: p.text,',
             '        width: tip_w,\n        height: tip_h,\n        color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the battery tip fades to the dimmest ink',
        POWER,
        [
            ('        width: tip_w,\n        height: tip_h,\n        color: p.text,',
             '        width: tip_w,\n        height: tip_h,\n        color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the battery tip takes the accent',
        POWER,
        [
            ('        width: tip_w,\n        height: tip_h,\n        color: p.text,',
             '        width: tip_w,\n        height: tip_h,\n        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the gauge's critical step is frozen back to its Mocha value",
        POWER,
        [
            ('        p.red\n',
             '        Color::from_hex(0xF38BA8)\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the gauge's critical step is swapped with its neighbour",
        POWER,
        [
            ('        p.red\n',
             '        p.yellow\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the gauge's critical step takes the accent",
        POWER,
        [
            ('        p.red\n',
             '        p.accent\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the gauge's low step is frozen back to its Mocha value",
        POWER,
        [
            ('        p.yellow\n',
             '        Color::from_hex(0xF9E2AF)\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the gauge's low step is swapped with its neighbour",
        POWER,
        [
            ('        p.yellow\n',
             '        p.red\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the gauge's low step takes the accent",
        POWER,
        [
            ('        p.yellow\n',
             '        p.accent\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the gauge's nearly full step is frozen back to its Mocha value",
        POWER,
        [
            ('        p.green\n',
             '        Color::from_hex(0xA6E3A1)\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the gauge's nearly full step is swapped with its neighbour",
        POWER,
        [
            ('        p.green\n',
             '        p.blue\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the gauge's nearly full step takes the accent",
        POWER,
        [
            ('        p.green\n',
             '        p.accent\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the gauge's normal step is frozen back to its Mocha value",
        POWER,
        [
            ('        p.blue\n',
             '        Color::from_hex(0x89B4FA)\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the gauge's normal step is swapped with its neighbour",
        POWER,
        [
            ('        p.blue\n',
             '        p.green\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the gauge's normal step takes the accent",
        POWER,
        [
            ('        p.blue\n',
             '        p.accent\n'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the charging bolt is frozen back to Mocha yellow',
        POWER,
        [
            ('            text: "\\u{26A1}".to_string(), // ⚡\n            color: p.yellow,',
             '            text: "\\u{26A1}".to_string(), // ⚡\n            color: Color::from_hex(0xF9E2AF),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the charging bolt turns into an alarm',
        POWER,
        [
            ('            text: "\\u{26A1}".to_string(), // ⚡\n            color: p.yellow,',
             '            text: "\\u{26A1}".to_string(), // ⚡\n            color: p.red,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the charging bolt takes the accent',
        POWER,
        [
            ('            text: "\\u{26A1}".to_string(), // ⚡\n            color: p.yellow,',
             '            text: "\\u{26A1}".to_string(), // ⚡\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the percentage readout is frozen back to Mocha subtext0',
        POWER,
        [
            ('            text: format!("{}%", battery.charge_pct),\n            color: p.subtext0,',
             '            text: format!("{}%", battery.charge_pct),\n            color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the percentage readout is promoted to primary text',
        POWER,
        [
            ('            text: format!("{}%", battery.charge_pct),\n            color: p.subtext0,',
             '            text: format!("{}%", battery.charge_pct),\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the percentage readout takes the accent',
        POWER,
        [
            ('            text: format!("{}%", battery.charge_pct),\n            color: p.subtext0,',
             '            text: format!("{}%", battery.charge_pct),\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the Balanced badge is frozen back to its Mocha value',
        POWER,
        [
            ('        PowerProfile::Balanced => ("Balanced", p.blue),',
             '        PowerProfile::Balanced => ("Balanced", Color::from_hex(0x89B4FA)),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the Balanced badge is swapped with another profile's hue",
        POWER,
        [
            ('        PowerProfile::Balanced => ("Balanced", p.blue),',
             '        PowerProfile::Balanced => ("Balanced", p.peach),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the Balanced badge takes the accent',
        POWER,
        [
            ('        PowerProfile::Balanced => ("Balanced", p.blue),',
             '        PowerProfile::Balanced => ("Balanced", p.accent),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_text_this_module_draws_is_in_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the Performance badge is frozen back to its Mocha value',
        POWER,
        [
            ('        PowerProfile::Performance => ("Performance", p.peach),',
             '        PowerProfile::Performance => ("Performance", Color::from_hex(0xFAB387)),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the Performance badge is swapped with another profile's hue",
        POWER,
        [
            ('        PowerProfile::Performance => ("Performance", p.peach),',
             '        PowerProfile::Performance => ("Performance", p.blue),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the Performance badge takes the accent',
        POWER,
        [
            ('        PowerProfile::Performance => ("Performance", p.peach),',
             '        PowerProfile::Performance => ("Performance", p.accent),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the Power Saver badge is frozen back to its Mocha value',
        POWER,
        [
            ('        PowerProfile::PowerSaver => ("Power Saver", p.green),',
             '        PowerProfile::PowerSaver => ("Power Saver", Color::from_hex(0xA6E3A1)),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the Power Saver badge is swapped with another profile's hue",
        POWER,
        [
            ('        PowerProfile::PowerSaver => ("Power Saver", p.green),',
             '        PowerProfile::PowerSaver => ("Power Saver", p.lavender),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the Power Saver badge takes the accent',
        POWER,
        [
            ('        PowerProfile::PowerSaver => ("Power Saver", p.green),',
             '        PowerProfile::PowerSaver => ("Power Saver", p.accent),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the Custom badge is frozen back to its Mocha value',
        POWER,
        [
            ('        PowerProfile::Custom => ("Custom", p.lavender),',
             '        PowerProfile::Custom => ("Custom", Color::from_hex(0xB4BEFE)),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'every_colour_the_taskbar_and_settings_draw_comes_from_their_palette',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the Custom badge is swapped with another profile's hue",
        POWER,
        [
            ('        PowerProfile::Custom => ("Custom", p.lavender),',
             '        PowerProfile::Custom => ("Custom", p.green),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the Custom badge takes the accent',
        POWER,
        [
            ('        PowerProfile::Custom => ("Custom", p.lavender),',
             '        PowerProfile::Custom => ("Custom", p.accent),'),
        ],
        ["desktop"],
        [
            'every_choice_this_module_makes_hands_over_the_role_it_claims',
            'nothing_this_module_draws_moves_when_the_accent_does',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the badge's wash names a role instead of tinting the badge's own hue",
        POWER,
        [
            ('            color: Color::rgba(color.r, color.g, color.b, 40),',
             '            color: Color::rgba(p.blue.r, p.blue.g, p.blue.b, 40),'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the badge's wash is drawn at full strength",
        POWER,
        [
            ('            color: Color::rgba(color.r, color.g, color.b, 40),',
             '            color: Color::rgba(color.r, color.g, color.b, 255),'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the saver's clock is drawn in the wrong hue",
        POWER,
        [
            ('            text: "12:00".to_string(),\n            color: screen_palette().lavender,',
             '            text: "12:00".to_string(),\n            color: screen_palette().blue,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the saver's clock reaches into the light palette",
        POWER,
        [
            ('            text: "12:00".to_string(),\n            color: screen_palette().lavender,',
             '            text: "12:00".to_string(),\n            color: Palette::for_mode(true).lavender,'),
        ],
        ["desktop"],
        [
            'every_colour_the_screen_saver_draws_is_a_dark_role_or_one_of_its_two_ramps',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the saver stops pinning itself to the dark palette',
        POWER,
        [
            ('fn screen_palette() -> Palette {\n    Palette::for_mode(false)\n}',
             'fn screen_palette() -> Palette {\n    Palette::for_mode(true)\n}'),
        ],
        ["desktop"],
        [
            'every_colour_the_screen_saver_draws_is_a_dark_role_or_one_of_its_two_ramps',
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
            'every_text_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the logo plate is drawn in the wrong hue',
        POWER,
        [
            ('            width: logo_w,\n            height: logo_h,\n            color: sp.blue,',
             '            width: logo_w,\n            height: logo_h,\n            color: sp.green,'),
        ],
        ["desktop"],
        [
            'every_rectangle_this_module_draws_is_in_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the logo's label names a role instead of deriving from the plate",
        POWER,
        [
            ('            text: "Slate OS".to_string(),\n            color: appearance::readable_on(sp.blue),',
             '            text: "Slate OS".to_string(),\n            color: sp.base,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
            'the_logo_label_is_readable_on_the_logo_plate',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the logo's label is drawn in the ink of the wrong surface",
        POWER,
        [
            ('            text: "Slate OS".to_string(),\n            color: appearance::readable_on(sp.blue),',
             '            text: "Slate OS".to_string(),\n            color: sp.text,'),
        ],
        ["desktop"],
        [
            'every_text_this_module_draws_is_in_the_role_it_claims',
            'the_logo_label_is_readable_on_the_logo_plate',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the blank saver lights the display instead of blacking it out',
        POWER,
        [
            ('        vec![RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Color::from_hex(0x000000),',
             '        vec![RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Palette::for_mode(false).base,'),
        ],
        ["desktop"],
        [
            'the_screen_saver_blacks_out_the_display_in_every_style_it_draws',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the clock saver lights the display with the light palette',
        POWER,
        [
            ('        // Black background.\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Color::from_hex(0x000000),',
             '        // Black background.\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Palette::for_mode(true).base,'),
        ],
        ["desktop"],
        [
            'the_screen_saver_blacks_out_the_display_in_every_style_it_draws',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the star field lights the display instead of blacking it out',
        POWER,
        [
            ('        let mut cmds = Vec::with_capacity(self.stars.len().saturating_add(1));\n\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Color::from_hex(0x000000),',
             '        let mut cmds = Vec::with_capacity(self.stars.len().saturating_add(1));\n\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Palette::for_mode(false).surface0,'),
        ],
        ["desktop"],
        [
            'the_screen_saver_blacks_out_the_display_in_every_style_it_draws',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the bouncing logo lights the display instead of blacking it out',
        POWER,
        [
            ('        let mut cmds = Vec::with_capacity(4);\n\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Color::from_hex(0x000000),',
             '        let mut cmds = Vec::with_capacity(4);\n\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Palette::for_mode(false).blue,'),
        ],
        ["desktop"],
        [
            'the_screen_saver_blacks_out_the_display_in_every_style_it_draws',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the rain's trail overlay stops being black",
        POWER,
        [
            ('        // Semi-transparent black overlay for trail effect.\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Color::rgba(0, 0, 0, 220),',
             '        // Semi-transparent black overlay for trail effect.\n        cmds.push(RenderCommand::FillRect {\n            x: 0.0,\n            y: 0.0,\n            width: self.width as f32,\n            height: self.height as f32,\n            color: Color::rgba(20, 0, 0, 220),'),
        ],
        ["desktop"],
        [
            'every_colour_the_screen_saver_draws_is_a_dark_role_or_one_of_its_two_ramps',
            'the_screen_saver_blacks_out_the_display_in_every_style_it_draws',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: a star's depth is drawn as a red ramp rather than a grey one",
        POWER,
        [
            ('                    color: Color::rgba(brightness, brightness, brightness, 255),',
             '                    color: Color::rgba(brightness, 0, 0, 255),'),
        ],
        ["desktop"],
        [
            'every_colour_the_screen_saver_draws_is_a_dark_role_or_one_of_its_two_ramps',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: a glyph's age is drawn as a magenta ramp rather than a green one",
        POWER,
        [
            ('                    color: Color::rgba(0, green, 0, 255),',
             '                    color: Color::rgba(green, 0, green, 255),'),
        ],
        ["desktop"],
        [
            'every_colour_the_screen_saver_draws_is_a_dark_role_or_one_of_its_two_ramps',
        ],
    ),
    # ---- login_screen.rs (module 28 of 49) ----
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the theme background is frozen back to Mocha crust',
        LOGIN,
        [
            ('                    height: self.screen_height,\n                    color: p.crust,',
             '                    height: self.screen_height,\n                    color: Color::from_hex(0x11111B),'),
        ],
        ["desktop"],
        [
            'a_background_with_no_colour_of_its_own_takes_the_theme',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the theme background takes the accent',
        LOGIN,
        [
            ('                    height: self.screen_height,\n                    color: p.crust,',
             '                    height: self.screen_height,\n                    color: p.accent,'),
        ],
        ["desktop"],
        [
            'a_background_with_no_colour_of_its_own_takes_the_theme',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the theme background resolves to base instead of the deepest surface',
        LOGIN,
        [
            ('                    height: self.screen_height,\n                    color: p.crust,',
             '                    height: self.screen_height,\n                    color: p.base,'),
        ],
        ["desktop"],
        [
            'a_background_with_no_colour_of_its_own_takes_the_theme',
            # Undeclared when this was written, and the reason earns the line:
            # Latte `base` *is* `LIGHT_EXTREME`, which is what
            # `on_wallpaper()` answers in both modes. A background that
            # resolves to `base` is therefore counted as a seventh piece of
            # wallpaper ink by a test that is not looking at the background.
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: a solid background the user chose is re-themed',
        LOGIN,
        [
            ('                    height: self.screen_height,\n                    color: *color,',
             '                    height: self.screen_height,\n                    color: p.crust,'),
        ],
        ["desktop"],
        [
            'a_user_chosen_background_is_never_re_themed',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the gradient band truncates again instead of rounding',
        LOGIN,
        [
            ('        v.round().clamp(0.0, 255.0) as u8',
             '        v.clamp(0.0, 255.0) as u8'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'a_gradient_between_a_colour_and_itself_is_flat',
            'a_gradient_band_rounds_to_the_nearest_step',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the gradient's far endpoint is re-themed",
        LOGIN,
        [
            ('                    let r = lerp_channel(top.r, bottom.r, t);',
             '                    let r = lerp_channel(top.r, p.base.r, t);'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'a_user_chosen_background_is_never_re_themed',
            'a_gradient_between_a_colour_and_itself_is_flat',
            'a_gradient_band_rounds_to_the_nearest_step',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: text on the login background loses its shadow',
        LOGIN,
        [
            ('    commands.push(RenderCommand::Text {\n        x: x + 1.0,\n        y: y + 1.0,\n        text: body.clone(),\n        font_size: *font_size,\n        color: p.text_shadow(),\n        font_weight: *font_weight,\n        max_width: *max_width,\n        overflow: *overflow,\n    });\n    commands.push(text);',
             '    let _ = (p, body, font_size, font_weight, max_width, overflow, x, y);\n    commands.push(text);'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the shadow is drawn exactly under the ink, so it never shows',
        LOGIN,
        [
            ('        x: x + 1.0,\n        y: y + 1.0,',
             '        x: *x,\n        y: *y,'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            # Not the counting test, which was declared here and did not fire:
            # the shadow is still black at 180 and still drawn once per
            # floating text, so the count is unchanged. Only the position
            # assertion inside `floating_text` sees this, which is the whole
            # reason that helper checks the offset rather than just the pair.
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the shadow is a theme role rather than black',
        LOGIN,
        [
            ('        color: p.text_shadow(),',
             '        color: p.crust,'),
        ],
        ["desktop"],
        [
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the clock is frozen back to Mocha text',
        LOGIN,
        [
            ('                font_size: 64.0,\n                color: p.on_wallpaper(),',
             '                font_size: 64.0,\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the clock takes panel ink on a background the shell did not choose',
        LOGIN,
        [
            ('                font_size: 64.0,\n                color: p.on_wallpaper(),',
             '                font_size: 64.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the clock is dimmed to the level of the date under it',
        LOGIN,
        [
            ('                font_size: 64.0,\n                color: p.on_wallpaper(),',
             '                font_size: 64.0,\n                color: p.on_wallpaper_dim(),'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the date is frozen back to Mocha subtext0',
        LOGIN,
        [
            ('\n                    color: p.on_wallpaper_dim(),',
             '\n                    color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the date is promoted to the same strength as the time',
        LOGIN,
        [
            ('\n                    color: p.on_wallpaper_dim(),',
             '\n                    color: p.on_wallpaper(),'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the date takes panel ink on the background',
        LOGIN,
        [
            ('\n                    color: p.on_wallpaper_dim(),',
             '\n                    color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the selected row is frozen back to Mocha surface0',
        LOGIN,
        [
            ('                color: if selected {\n                    p.surface0',
             '                color: if selected {\n                    Color::from_hex(0x313244)'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the selected row fills with the accent instead of a surface',
        LOGIN,
        [
            ('                color: if selected {\n                    p.surface0',
             '                color: if selected {\n                    p.accent'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: an unselected row is frozen back to Mocha base',
        LOGIN,
        [
            ('                    Color::rgba(p.base.r, p.base.g, p.base.b, 180)',
             '                    Color::rgba(0x1E, 0x1E, 0x2E, 180)'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: an unselected row loses the transparency that sets it apart',
        LOGIN,
        [
            ('                    Color::rgba(p.base.r, p.base.g, p.base.b, 180)',
             '                    p.base'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the selected avatar is frozen back to Mocha blue',
        LOGIN,
        [
            ('                color: if selected { p.accent } else { p.subtext0 },',
             '                color: if selected { Color::from_hex(0x89B4FA) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: no avatar is accented, so nothing marks which row you are on',
        LOGIN,
        [
            ('                color: if selected { p.accent } else { p.subtext0 },',
             '                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: every avatar is accented, so the accent marks nothing',
        LOGIN,
        [
            ('                color: if selected { p.accent } else { p.subtext0 },',
             '                color: if selected { p.accent } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a row name is frozen back to Mocha text',
        LOGIN,
        [
            ('                text: user.display_name.clone(),\n                font_size: 16.0,\n                color: p.text,',
             '                text: user.display_name.clone(),\n                font_size: 16.0,\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: a row name takes the accent',
        LOGIN,
        [
            ('                text: user.display_name.clone(),\n                font_size: 16.0,\n                color: p.text,',
             '                text: user.display_name.clone(),\n                font_size: 16.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the account type is frozen back to Mocha overlay0',
        LOGIN,
        [
            ('                font_size: 11.0,\n                color: p.overlay0,',
             '                font_size: 11.0,\n                color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the account type is promoted to primary text',
        LOGIN,
        [
            ('                font_size: 11.0,\n                color: p.overlay0,',
             '                font_size: 11.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the signing-in user's avatar is frozen back to Mocha blue",
        LOGIN,
        [
            ('                    font_size: 48.0,\n                    color: p.accent,',
             '                    font_size: 48.0,\n                    color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the signing-in user's avatar loses the accent it carried in the list",
        LOGIN,
        [
            ('                    font_size: 48.0,\n                    color: p.accent,',
             '                    font_size: 48.0,\n                    color: p.on_wallpaper(),'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the name over the password field is frozen back to Mocha text',
        LOGIN,
        [
            ('                    font_size: 18.0,\n                    color: p.on_wallpaper(),',
             '                    font_size: 18.0,\n                    color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the name over the password field takes panel ink',
        LOGIN,
        [
            ('                    font_size: 18.0,\n                    color: p.on_wallpaper(),',
             '                    font_size: 18.0,\n                    color: p.text,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the rejected-password border is frozen back to Mocha red',
        LOGIN,
        [
            ('            let border_color = if self.error_message.is_some() {\n                p.red',
             '            let border_color = if self.error_message.is_some() {\n                Color::from_hex(0xF38BA8)'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the rejected-password border takes the accent, so a refusal is decoration',
        LOGIN,
        [
            ('            let border_color = if self.error_message.is_some() {\n                p.red',
             '            let border_color = if self.error_message.is_some() {\n                p.accent'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the border at rest is frozen back to Mocha surface1',
        LOGIN,
        [
            ('            } else {\n                p.surface1\n            };',
             '            } else {\n                Color::from_hex(0x45475A)\n            };'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the password field is frozen back to Mocha surface0',
        LOGIN,
        [
            ('                height: field_h,\n                color: p.surface0,',
             '                height: field_h,\n                color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the placeholder is frozen back to Mocha overlay0',
        LOGIN,
        [
            ('                color: if self.password_input.is_empty() {\n                    p.overlay0',
             '                color: if self.password_input.is_empty() {\n                    Color::from_hex(0x6C7086)'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the typed password is frozen back to Mocha text',
        LOGIN,
        [
            ('                } else {\n                    p.text\n                },',
             '                } else {\n                    Color::from_hex(0xCDD6F4)\n                },'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the typed password is drawn at placeholder strength',
        LOGIN,
        [
            ('                } else {\n                    p.text\n                },',
             '                } else {\n                    p.overlay0\n                },'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the reveal toggle is frozen back to Mocha subtext0',
        LOGIN,
        [
            ('                }\n                .to_string(),\n                font_size: 14.0,\n                color: p.subtext0,',
             '                }\n                .to_string(),\n                font_size: 14.0,\n                color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the Sign In fill is frozen back to Mocha blue',
        LOGIN,
        [
            ('                width: 100.0,\n                height: 32.0,\n                color: p.accent,',
             '                width: 100.0,\n                height: 32.0,\n                color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the Sign In fill drops to a surface, so the default action stops inviting',
        LOGIN,
        [
            ('                width: 100.0,\n                height: 32.0,\n                color: p.accent,',
             '                width: 100.0,\n                height: 32.0,\n                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the Sign In label is named rather than derived from the fill under it',
        LOGIN,
        [
            ('                text: "Sign In".to_string(),\n                font_size: 13.0,\n                color: p.on_accent(),',
             '                text: "Sign In".to_string(),\n                font_size: 13.0,\n                color: Color::from_hex(0x11111B),'),
        ],
        ["desktop"],
        [
            'the_sign_in_label_follows_the_accent_it_sits_on',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the Sign In label takes a role instead of the ink its fill demands',
        LOGIN,
        [
            ('                text: "Sign In".to_string(),\n                font_size: 13.0,\n                color: p.on_accent(),',
             '                text: "Sign In".to_string(),\n                font_size: 13.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'the_sign_in_label_follows_the_accent_it_sits_on',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the error message is frozen back to Mocha red',
        LOGIN,
        [
            ('                        font_size: 12.0,\n                        color: p.red,',
             '                        font_size: 12.0,\n                        color: Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the error message takes the accent',
        LOGIN,
        [
            ('                        font_size: 12.0,\n                        color: p.red,',
             '                        font_size: 12.0,\n                        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the lockout notice is frozen back to Mocha yellow',
        LOGIN,
        [
            ('                        font_size: 12.0,\n                        color: p.yellow,',
             '                        font_size: 12.0,\n                        color: Color::from_hex(0xF9E2AF),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the lockout notice takes the accent',
        LOGIN,
        [
            ('                        font_size: 12.0,\n                        color: p.yellow,',
             '                        font_size: 12.0,\n                        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the back arrow is frozen back to Mocha subtext0',
        LOGIN,
        [
            ('                        font_size: 20.0,\n                        color: p.on_wallpaper_dim(),',
             '                        font_size: 20.0,\n                        color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the back arrow takes panel ink on the background',
        LOGIN,
        [
            ('                        font_size: 20.0,\n                        color: p.on_wallpaper_dim(),',
             '                        font_size: 20.0,\n                        color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'exactly_seven_things_in_the_full_render_sit_on_the_background',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the authenticating notice is frozen back to Mocha text',
        LOGIN,
        [
            ('                text: "Signing in...".to_string(),\n                font_size: 16.0,\n                color: p.on_wallpaper(),',
             '                text: "Signing in...".to_string(),\n                font_size: 16.0,\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'the_clock_and_the_status_lines_are_wallpaper_ink',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the greeting takes panel ink on the background',
        LOGIN,
        [
            ('                    text: format!("Welcome, {}!", user.display_name),\n                    font_size: 20.0,\n                    color: p.on_wallpaper(),',
             '                    text: format!("Welcome, {}!", user.display_name),\n                    font_size: 20.0,\n                    color: p.text,'),
        ],
        ["desktop"],
        [
            'the_clock_and_the_status_lines_are_wallpaper_ink',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the bottom bar is frozen back to Mocha crust',
        LOGIN,
        [
            ('            color: Color::rgba(p.crust.r, p.crust.g, p.crust.b, 180),',
             '            color: Color::rgba(0x11, 0x11, 0x1B, 180),'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the bottom bar loses its transparency',
        LOGIN,
        [
            ('            color: Color::rgba(p.crust.r, p.crust.g, p.crust.b, 180),',
             '            color: p.crust,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the keyboard-layout indicator is frozen back to Mocha subtext0',
        LOGIN,
        [
            ('                text: self.keyboard_layout.clone(),\n                font_size: 12.0,\n                color: p.subtext0,',
             '                text: self.keyboard_layout.clone(),\n                font_size: 12.0,\n                color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the bar's power button is frozen back to Mocha subtext0",
        LOGIN,
        [
            ('                text: "\\u{23FB}".to_string(),\n                font_size: 16.0,\n                color: p.subtext0,',
             '                text: "\\u{23FB}".to_string(),\n                font_size: 16.0,\n                color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the accessibility button takes the accent',
        LOGIN,
        [
            ('                text: "\\u{267F}".to_string(),\n                font_size: 16.0,\n                color: p.subtext0,',
             '                text: "\\u{267F}".to_string(),\n                font_size: 16.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the on-screen-keyboard button is frozen back to Mocha subtext0',
        LOGIN,
        [
            ('                text: "\\u{2328}".to_string(),\n                font_size: 16.0,\n                color: p.subtext0,',
             '                text: "\\u{2328}".to_string(),\n                font_size: 16.0,\n                color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the power menu is frozen back to Mocha mantle',
        LOGIN,
        [
            ('            width: menu_w,\n            height: menu_h,\n            color: p.mantle,',
             '            width: menu_w,\n            height: menu_h,\n            color: Color::from_hex(0x181825),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the power menu's border is frozen back to Mocha surface1",
        LOGIN,
        [
            ('            width: menu_w,\n            height: menu_h,\n            color: p.surface1,',
             '            width: menu_w,\n            height: menu_h,\n            color: Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: a power-menu icon is frozen back to Mocha subtext0',
        LOGIN,
        [
            ('                text: icon.to_string(),\n                font_size: 14.0,\n                color: p.subtext0,',
             '                text: icon.to_string(),\n                font_size: 14.0,\n                color: Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: a power-menu label takes the accent',
        LOGIN,
        [
            ('                text: label.to_string(),\n                font_size: 13.0,\n                color: p.text,',
             '                text: label.to_string(),\n                font_size: 13.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the whole screen ignores the palette it is handed',
        LOGIN,
        [
            ('    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {\n        let mut commands = Vec::new();',
             '    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {\n        let p = &Palette::for_mode(false);\n        let mut commands = Vec::new();'),
        ],
        ["desktop"],
        [
            'the_render_is_not_the_same_in_both_modes',
            'every_colour_the_login_screen_draws_comes_from_its_palette',
            'every_colour_in_the_user_list_is_in_the_role_it_claims',
            'every_colour_in_the_password_entry_is_in_the_role_it_claims',
            'every_colour_in_the_bar_and_the_power_menu_is_in_the_role_it_claims',
            'a_background_with_no_colour_of_its_own_takes_the_theme',
            'exactly_two_things_in_the_password_panel_carry_the_accent',
            'the_sign_in_label_follows_the_accent_it_sits_on',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            # The two wallpaper-ink tests were declared here and did not fire,
            # which is the sharpest measurement this module produced. Nine
            # tests see a render that threw its palette away; those two cannot,
            # because every colour they check is mode-independent *by
            # construction*: `on_wallpaper()` is the constant `LIGHT_EXTREME`,
            # `on_wallpaper_dim()` is that at alpha 200, and `text_shadow()` is
            # black. A module that ignores the palette still draws all three
            # correctly. Those tests earn their keep against the role-versus-
            # wallpaper confusion — Kx33, Ox33, Dx34, Vx34, Xx34 — not this.
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the power-menu fixture is dropped from the sweep',
        LOGIN,
        [
            ('        let mut s = base();\n        s.power_menu_open = true;\n        v.push(("power menu".to_string(), s));',
             '        let mut s = base();\n        s.power_menu_open = true;\n        let _ = s;'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the no-user fixture is dropped from the sweep',
        LOGIN,
        [
            ('        s.phase = LoginPhase::PasswordEntry;\n        v.push(("no user".to_string(), s));',
             '        s.phase = LoginPhase::PasswordEntry;\n        let _ = s;'),
        ],
        ["desktop"],
        [
            'the_fixtures_take_every_branch_this_module_has',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the default background names a colour again',
        LOGIN,
        [
            ('#[derive(Clone, Debug, Default, PartialEq)]\npub enum LoginBackground {',
             '#[derive(Clone, Debug, PartialEq)]\npub enum LoginBackground {'),
            ('    #[default]\n    Theme,',
             '    Theme,'),
            ('    Gradient { top: Color, bottom: Color },\n}',
             '    Gradient { top: Color, bottom: Color },\n}\n\nimpl Default for LoginBackground {\n    fn default() -> Self {\n        Self::SolidColor(Color::from_hex(0x11111B))\n    }\n}'),
        ],
        ["desktop"],
        [
            'the_default_background_defers_its_colour_to_the_palette',
        ],
    ),
    # ---- taskbar.rs (module 29 of 49) ----
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the bar background is frozen back to Mocha base',
        TB,
        [
            ('            height: bar_height,\n            color: p.base,',
             '            height: bar_height,\n            color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            # NOT test_render_empty_taskbar: it asserts the bar's *geometry*
            # (one full-width rect exists), never its colour. Declared here
            # originally on the strength of its name.
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the bar background resolves to the desktop's own surface",
        TB,
        [
            ('            height: bar_height,\n            color: p.base,',
             '            height: bar_height,\n            color: p.crust,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_empty_taskbar',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the whole bar takes the accent',
        TB,
        [
            ('            height: bar_height,\n            color: p.base,',
             '            height: bar_height,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_empty_taskbar',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the bar background sits one step behind itself',
        TB,
        [
            ('            height: bar_height,\n            color: p.base,',
             '            height: bar_height,\n            color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_empty_taskbar',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the top border is frozen back to Mocha surface0',
        TB,
        [
            ('            y2: 0.0,\n            color: p.surface0,',
             '            y2: 0.0,\n            color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the top border takes the separator role the divider uses',
        TB,
        [
            ('            y2: 0.0,\n            color: p.surface0,',
             '            y2: 0.0,\n            color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the section divider goes back to being a raised surface',
        TB,
        [
            ("                    // Judgement 6: a separator, which is `overlay0`'s job.\n                    color: p.overlay0,",
             '                    color: p.surface2,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_divider_between_pinned_and_running',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the section divider is frozen back to Mocha surface2',
        TB,
        [
            ("                    // Judgement 6: a separator, which is `overlay0`'s job.\n                    color: p.overlay0,",
             '                    color: Color::from_hex(0x585B70),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_divider_between_pinned_and_running',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the section divider is never drawn',
        TB,
        [
            ('            if i == pinned_count && pinned_count > 0 && i < self.buttons.len() {',
             '            if i == pinned_count && pinned_count > 0 && i > self.buttons.len() {'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_divider_between_pinned_and_running',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the drop caret takes the accent, so it cannot be told from the focus mark',
        TB,
        [
            ('                        color: p.green,',
             '                        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            'the_drop_caret_is_never_the_same_hue_as_the_focus_underline',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the drop caret is frozen back to Mocha blue',
        TB,
        [
            ('                        color: p.green,',
             '                        color: Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the drop caret takes drop_target's alpha as well as its hue, and vanishes",
        TB,
        [
            ('                        color: p.green,',
             '                        color: with_alpha(p.green, 60),'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the dragged ghost is opaque, so it stops reading as a ghost',
        TB,
        [
            ('                        color: with_alpha(p.surface1, 180),',
             '                        color: p.surface1,'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the dragged ghost is frozen back to its Mocha rgba triple',
        TB,
        [
            ('                        color: with_alpha(p.surface1, 180),',
             '                        color: Color::rgba(69, 71, 90, 180),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'each_button_state_draws_the_background_its_role_names',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the dragged ghost is built from the hover rung instead of the resting one',
        TB,
        [
            ('                        color: with_alpha(p.surface1, 180),',
             '                        color: with_alpha(p.surface2, 180),'),
        ],
        ["desktop"],
        [
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the hover rung goes back to where the focus rung belongs',
        TB,
        [
            ('                (_, true) => p.surface2,',
             '                (_, true) => p.surface1,'),
        ],
        ["desktop"],
        [
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the hover and focus rungs of the ladder are exchanged',
        TB,
        [
            ('                (_, true) => p.surface2,',
             '                (_, true) => p.surface1,'),
            ('                (ButtonState::Focused, false) => p.surface1,',
             '                (ButtonState::Focused, false) => p.surface2,'),
        ],
        ["desktop"],
        [
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the focused button drops a rung to surface0',
        TB,
        [
            ('                (ButtonState::Focused, false) => p.surface1,',
             '                (ButtonState::Focused, false) => p.surface0,'),
        ],
        ["desktop"],
        [
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the running button's background loses its transparency",
        TB,
        [
            ('                (ButtonState::Running, false) => with_alpha(p.surface0, 128),',
             '                (ButtonState::Running, false) => p.surface0,'),
        ],
        ["desktop"],
        [
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: an idle button paints a background it should not have',
        TB,
        [
            ('                (ButtonState::Idle, false) => Color::TRANSPARENT,',
             '                (ButtonState::Idle, false) => p.base,'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the hover rung is frozen back to Mocha surface1',
        TB,
        [
            ('                (_, true) => p.surface2,',
             '                (_, true) => Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'each_button_state_draws_the_background_its_role_names',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the focused button's background takes the accent",
        TB,
        [
            ('                (ButtonState::Focused, false) => p.surface1,',
             '                (ButtonState::Focused, false) => p.accent,'),
        ],
        ["desktop"],
        [
            'each_button_state_draws_the_background_its_role_names',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the focus underline reaches for blue, which is the stock accent's twin",
        TB,
        [
            ('                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.subtext0\n                };',
             '                let indicator_color = if button.state == ButtonState::Focused {\n                    p.blue\n                } else {\n                    p.subtext0\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the focus underline is frozen back to Mocha blue',
        TB,
        [
            ('                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.subtext0\n                };',
             '                let indicator_color = if button.state == ButtonState::Focused {\n                    Color::from_hex(0x89B4FA)\n                } else {\n                    p.subtext0\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: a merely-running app claims the accent too',
        TB,
        [
            ('                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.subtext0\n                };',
             '                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.accent\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            # This one was written before the conversion and only checks that
            # *some* 3px-high bar is drawn in `p.subtext0`; a merely-running
            # app taking the accent leaves no such bar, so it fails too. Worth
            # keeping declared — it is the oldest guard on this site.
            'test_render_shows_indicator_for_running',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the running underline goes back to lavender',
        TB,
        [
            ('                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.subtext0\n                };',
             '                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.lavender\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_shows_indicator_for_running',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the running underline is frozen back to Mocha lavender',
        TB,
        [
            ('                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.subtext0\n                };',
             '                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    Color::from_hex(0xB4BEFE)\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'test_render_shows_indicator_for_running',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: nothing on the bar says which window you are in',
        TB,
        [
            ('                let indicator_color = if button.state == ButtonState::Focused {\n                    p.accent\n                } else {\n                    p.subtext0\n                };',
             '                let indicator_color = if button.state == ButtonState::Focused {\n                    p.subtext0\n                } else {\n                    p.subtext0\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the two underline widths are exchanged',
        TB,
        [
            ('                let indicator_w = if button.state == ButtonState::Focused {\n                    16.0\n                } else {\n                    8.0\n                };',
             '                let indicator_w = if button.state == ButtonState::Focused {\n                    8.0\n                } else {\n                    16.0\n                };'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_focus_underline_is_wider_than_the_running_one',
            # Exchanging the widths also swaps which bar the caret test finds
            # as "the focus underline" (it selects by width, 16x3), so it ends
            # up comparing the caret's hue against `subtext0` instead of the
            # accent. It fails for the right reason: after this edit there is
            # no 16x3 accent bar at all.
            'the_drop_caret_is_never_the_same_hue_as_the_focus_underline',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the window count follows the accent, so it means something different per machine',
        TB,
        [
            ('                    color: p.red,',
             '                    color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            'the_count_badge_does_not_follow_the_accent',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            'test_render_shows_badge_for_multiple_windows',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the window count is drawn in the wrong named hue',
        TB,
        [
            ('                    color: p.red,',
             '                    color: p.yellow,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_count_badge_does_not_follow_the_accent',
            # NOT the_badge_digit test: it compares the digit against
            # `readable_on(badge as actually drawn)`, so swapping the badge to
            # another role keeps the pair consistent. Mocha yellow and Mocha
            # red are both light enough to want the same near-black ink, so it
            # does not even shift the value. A consistency check cannot see a
            # change that moves both sides.
            'test_render_shows_badge_for_multiple_windows',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the window count is frozen back to Mocha red',
        TB,
        [
            ('                    color: p.red,',
             '                    color: Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            # The role table is what actually names the badge `p.red`, so a
            # frozen Mocha literal fails it in the light render. Not declared
            # originally because the defect looked like a sweep case.
            'the_count_badge_does_not_follow_the_accent',
            # NOT test_render_shows_badge_for_multiple_windows: it asserts a
            # 12x12 rect exists, and a frozen colour is still a rect.
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the badge digit is named again, as the Mocha mantle it used to be',
        TB,
        [
            ('                    color: readable_on(p.red),',
             '                    color: Color::from_hex(0x181825),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the badge digit is a role instead of a reading of its own fill',
        TB,
        [
            ('                    color: readable_on(p.red),',
             '                    color: p.text,'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the badge digit is read off the accent rather than off the badge',
        TB,
        [
            ('                    color: readable_on(p.red),',
             '                    color: p.on_accent(),'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the badge digit is frozen to the pale legibility endpoint',
        TB,
        [
            ('                    color: readable_on(p.red),',
             '                    color: Color::from_hex(0xEFF1F5),'),
        ],
        ["desktop"],
        [
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: a button being carried is drawn at full strength',
        TB,
        [
            ('            with_alpha(p.text, 140)',
             '            p.text'),
        ],
        ["desktop"],
        [
            'button_ink_follows_the_button_state',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the carried button's ink is frozen back to its Mocha rgba triple",
        TB,
        [
            ('            with_alpha(p.text, 140)',
             '            Color::rgba(205, 214, 244, 140)'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'button_ink_follows_the_button_state',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: an idle button's icon is as loud as a running one's",
        TB,
        [
            ('                ButtonState::Idle => p.subtext0,',
             '                ButtonState::Idle => p.text,'),
        ],
        ["desktop"],
        [
            'button_ink_follows_the_button_state',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the idle icon is frozen back to Mocha subtext0',
        TB,
        [
            ('                ButtonState::Idle => p.subtext0,',
             '                ButtonState::Idle => Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'button_ink_follows_the_button_state',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: a running button's icon drops to the secondary heading role",
        TB,
        [
            ('                ButtonState::Running | ButtonState::Focused => p.text,',
             '                ButtonState::Running | ButtonState::Focused => p.subtext1,'),
        ],
        ["desktop"],
        [
            'button_ink_follows_the_button_state',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the running icon is frozen back to Mocha text',
        TB,
        [
            ('                ButtonState::Running | ButtonState::Focused => p.text,',
             '                ButtonState::Running | ButtonState::Focused => Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'button_ink_follows_the_button_state',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the label stops sharing the icon's colour and names Mocha text",
        TB,
        [
            ('                text: button.display_name.clone(),\n                color: icon_color,',
             '                text: button.display_name.clone(),\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'button_ink_follows_the_button_state',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the label stops sharing the icon's colour and is always secondary",
        TB,
        [
            ('                text: button.display_name.clone(),\n                color: icon_color,',
             '                text: button.display_name.clone(),\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'button_ink_follows_the_button_state',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the menu's shadow becomes a colour, so it flips with the mode",
        TB,
        [
            ('            color: p.shadow(),',
             '            color: with_alpha(p.crust, 120),'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the menu's shadow goes back to this file's private alpha",
        TB,
        [
            ('            color: p.shadow(),',
             '            color: Color::rgba(0, 0, 0, 80),'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the menu stops floating and ignores the transparency setting',
        TB,
        [
            ('            color: p.panel_bg(),',
             '            color: p.base,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the menu panel is frozen back to Mocha surface0',
        TB,
        [
            ('            color: p.panel_bg(),',
             '            color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the menu panel is a flat raised surface again',
        TB,
        [
            ('            color: p.panel_bg(),',
             '            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the menu's outline drops to the separator role",
        TB,
        [
            ('            color: p.surface2,\n            line_width: 1.0,',
             '            color: p.overlay0,\n            line_width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the menu's outline is frozen back to Mocha surface2",
        TB,
        [
            ('            color: p.surface2,\n            line_width: 1.0,',
             '            color: Color::from_hex(0x585B70),\n            line_width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the menu's items are drawn in secondary ink",
        TB,
        [
            ('                text: label.to_string(),\n                color: p.text,',
             '                text: label.to_string(),\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the menu's items are frozen back to Mocha text",
        TB,
        [
            ('                text: label.to_string(),\n                color: p.text,',
             '                text: label.to_string(),\n                color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the taskbar throws away the palette it was handed and resolves its own',
        TB,
        [
            ('        let mut cmds = Vec::new();\n\n        // Background.',
             '        let p = &Palette::for_mode(false);\n\n        let mut cmds = Vec::new();\n\n        // Background.'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'each_button_state_draws_the_background_its_role_names',
            'button_ink_follows_the_button_state',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            # A frozen dark render draws Mocha red where the light palette
            # names Latte red, so the badge's role table fails too. This is
            # the defect the whole conversion exists to prevent, and it should
            # be declared against every table that reads a role in both modes.
            'the_count_badge_does_not_follow_the_accent',
            'the_render_is_not_the_same_in_both_modes',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the probe palette's accent collides with a role again",
        TB,
        [
            ('        p.accent = Color::from_hex(0xFF00FF);',
             '        p.accent = p.blue;'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'the_fixture_takes_every_branch_this_module_has',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'each_button_state_draws_the_background_its_role_names',
            'button_ink_follows_the_button_state',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            'the_drop_caret_is_never_the_same_hue_as_the_focus_underline',
            'the_count_badge_does_not_follow_the_accent',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            'the_focus_underline_is_wider_than_the_running_one',
            # Every test in this module builds its palette with `accented()`,
            # whose guard assertions this defect trips, so the pre-conversion
            # tests fail too.
            'test_render_shows_indicator_for_running',
            'the_render_is_not_the_same_in_both_modes',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the probe palette goes back to a fully opaque panel',
        TB,
        [
            ('        p.panel_alpha = 200;',
             '        p.panel_alpha = 255;'),
        ],
        ["desktop"],
        [
            'every_colour_the_taskbar_draws_comes_from_its_palette',
            'the_fixture_takes_every_branch_this_module_has',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'each_button_state_draws_the_background_its_role_names',
            'button_ink_follows_the_button_state',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
            'exactly_one_thing_on_the_taskbar_carries_the_accent',
            'the_drop_caret_is_never_the_same_hue_as_the_focus_underline',
            'the_count_badge_does_not_follow_the_accent',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            'the_focus_underline_is_wider_than_the_running_one',
            # Every test in this module builds its palette with `accented()`,
            # whose guard assertions this defect trips, so the pre-conversion
            # tests fail too.
            'test_render_shows_indicator_for_running',
            'the_render_is_not_the_same_in_both_modes',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the fixture's drag never becomes active, so the ghost and caret go unchecked",
        TB,
        [
            ('            start_y: 20.0,\n            active: true,',
             '            start_y: 20.0,\n            active: false,'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'each_button_state_draws_the_background_its_role_names',
            'button_ink_follows_the_button_state',
            'the_drop_caret_is_never_the_same_hue_as_the_focus_underline',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the fixture's context menu is closed, so the popup goes unchecked",
        TB,
        [
            ('            x: 200.0,\n            y: 300.0,\n            visible: true,',
             '            x: 200.0,\n            y: 300.0,\n            visible: false,'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'every_colour_in_the_context_menu_is_in_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the fixture's third window is gone, so the count badge goes unchecked",
        TB,
        [
            ('        s.add_running_window(WindowId(12), "many", "Many 3");\n',
             ''),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'every_colour_on_the_bar_is_in_the_role_it_claims',
            'the_badge_digit_is_computed_from_the_badge_it_sits_on',
            # NOT the_count_badge test: dropping the third window leaves two,
            # and a badge is drawn for any count above one — so the badge is
            # still there and still `p.red`. Only the digit test notices,
            # because it selects the text by its content ("3"). A defect that
            # weakens a fixture is caught by whichever test reads the value
            # that changed, not by every test that touches the site.
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: nothing in the fixture is hovered, so the top rung of the ladder goes unchecked',
        TB,
        [
            ('        s.hover_index = Some(3);',
             '        s.hover_index = None;'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'each_button_state_draws_the_background_its_role_names',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the fixture stops being in label mode, so the label site goes unchecked',
        TB,
        [
            ('        let mut s = TaskbarState::new(TaskbarConfig {\n            icon_only: false,',
             '        let mut s = TaskbarState::new(TaskbarConfig {\n            icon_only: true,'),
        ],
        ["desktop"],
        [
            'the_fixture_takes_every_branch_this_module_has',
            'button_ink_follows_the_button_state',
            # The pre-conversion label test uses the same fixture, so it goes
            # first — it looks for the literal text "Terminal", which an
            # icon-only bar never draws.
            'test_render_label_mode',
        ],
    ),
    # ------------------------------------------------------------------
    # Module 30: language_settings.rs. Eleven constants over three tabs.
    #
    # The module's five judgements are in its docs; these defects break
    # each one in the two ways that matter -- a role frozen to the Mocha
    # value it used to be (which only the light render can see), and a
    # role swapped for a neighbouring role (which no membership sweep can
    # see at all, because both are legal). The second kind is why this
    # module grew a per-site table; several of the defects below are
    # caught by nothing else.
    # ------------------------------------------------------------------
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the panel background is frozen to Mocha base',
        LANG,
        [
            ('            color: p.base,\n            corner_radii: CornerRadii::all(8.0),\n        });\n\n        // Title',
             '            color: guitk::color::Color::from_hex(0x1E1E2E),\n            corner_radii: CornerRadii::all(8.0),\n        });\n\n        // Title'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the panel background follows the accent',
        LANG,
        [
            ('            color: p.base,\n            corner_radii: CornerRadii::all(8.0),\n        });\n\n        // Title',
             '            color: p.accent,\n            corner_radii: CornerRadii::all(8.0),\n        });\n\n        // Title'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the panel title is frozen to Mocha text',
        LANG,
        [
            ('            font_size: 22.0,\n            color: p.text,',
             '            font_size: 22.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the tab strip stops marking which tab is open',
        LANG,
        [
            ('                color: if active { p.accent } else { p.surface0 },',
             '                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the active and inactive tab fills are exchanged',
        LANG,
        [
            ('                color: if active { p.accent } else { p.surface0 },',
             '                color: if active { p.surface0 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the active tab is raised a rung instead of accented',
        LANG,
        [
            ('                color: if active { p.accent } else { p.surface0 },',
             '                color: if active { p.surface1 } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the active tab's label is frozen to near-black instead of computed",
        LANG,
        [
            ('                color: if active {\n                    readable_on(p.accent)\n                } else {\n                    p.subtext0\n                },',
             '                color: if active {\n                    guitk::color::Color::from_hex(0x11111B)\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            # Only the accent sweep. The membership sweep is blind here by
            # construction: 0x11111B is a `readable_on` endpoint, so it is
            # allowed in both renders, and it is also Mocha `crust`, which
            # is why the deleted-constants test excludes it. A site whose
            # ink is one of the two endpoints contributes nothing to "did
            # this module read its palette" -- it has to be driven.
            'the_active_tabs_label_is_computed_from_the_accent_under_it',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the active tab's label takes the accent it is sitting on",
        LANG,
        [
            ('                color: if active {\n                    readable_on(p.accent)\n                } else {\n                    p.subtext0\n                },',
             '                color: if active {\n                    p.accent\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            'the_active_tabs_label_is_computed_from_the_accent_under_it',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: an inactive tab's label is frozen to Mocha subtext0",
        LANG,
        [
            ('                color: if active {\n                    readable_on(p.accent)\n                } else {\n                    p.subtext0\n                },',
             '                color: if active {\n                    readable_on(p.accent)\n                } else {\n                    guitk::color::Color::from_hex(0xA6ADC8)\n                },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the current-language card drops to the list rows' rung",
        LANG,
        [
            ('                height: 50.0,\n                color: p.surface1,',
             '                height: 50.0,\n                color: p.surface0,'),
        ],
        ["desktop"],
        [
            # Caught by nothing but the per-site table: both rungs are roles,
            # so the membership sweep accepts either, and the card is still
            # exactly one 552x50 fill so every count still balances. This is
            # the defect that motivated writing that table.
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the current-language card is frozen to Mocha surface1',
        LANG,
        [
            ('                height: 50.0,\n                color: p.surface1,',
             '                height: 50.0,\n                color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the card's title drops to the secondary rung",
        LANG,
        [
            ('                font_size: 14.0,\n                color: p.text,',
             '                font_size: 14.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the card's native name is frozen to Mocha subtext0",
        LANG,
        [
            ('                font_size: 12.0,\n                color: p.subtext0,',
             '                font_size: 12.0,\n                color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the search box is raised to the card's rung",
        LANG,
        [
            ('            height: 30.0,\n            color: p.surface0,',
             '            height: 30.0,\n            color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the search box is frozen to Mocha surface0',
        LANG,
        [
            ('            height: 30.0,\n            color: p.surface0,',
             '            height: 30.0,\n            color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the placeholder is as bright as a query the user typed',
        LANG,
        [
            ('            color: if self.language_search.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            color: p.text,'),
        ],
        ["desktop"],
        [
            'an_empty_search_box_is_dimmer_than_one_with_a_query_in_it',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the placeholder and the typed-query rungs are exchanged',
        LANG,
        [
            ('            color: if self.language_search.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            color: if self.language_search.is_empty() {\n                p.text\n            } else {\n                p.overlay0\n            },'),
        ],
        ["desktop"],
        [
            'an_empty_search_box_is_dimmer_than_one_with_a_query_in_it',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the typed query is frozen to Mocha text',
        LANG,
        [
            ('            color: if self.language_search.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            color: if self.language_search.is_empty() {\n                p.overlay0\n            } else {\n                guitk::color::Color::from_hex(0xCDD6F4)\n            },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'an_empty_search_box_is_dimmer_than_one_with_a_query_in_it',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the selected and unselected list-row rungs are exchanged',
        LANG,
        [
            ('                color: if is_selected { p.surface1 } else { p.surface0 },',
             '                color: if is_selected { p.surface0 } else { p.surface1 },'),
        ],
        ["desktop"],
        [
            # A permutation of {surface0, surface0, surface1}. The set is
            # unchanged, so a count sees nothing; only the positional vector
            # in the per-site table can tell which row was raised.
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: a list row stops showing whether it is selected',
        LANG,
        [
            ('                color: if is_selected { p.surface1 } else { p.surface0 },',
             '                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: an unselected list row is frozen to Mocha surface0',
        LANG,
        [
            ('                color: if is_selected { p.surface1 } else { p.surface0 },',
             '                color: if is_selected {\n                    p.surface1\n                } else {\n                    guitk::color::Color::from_hex(0x313244)\n                },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the current language's marker bar stops being the accent",
        LANG,
        [
            ('                    color: p.accent,\n                    corner_radii: CornerRadii::all(2.0),',
             '                    color: p.text,\n                    corner_radii: CornerRadii::all(2.0),'),
        ],
        ["desktop"],
        [
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the current language's marker bar is not drawn at all",
        LANG,
        [
            ('            if is_current {\n                cmds.push(RenderCommand::FillRect {\n                    x: x + 4.0,\n                    y: cy + 4.0,\n                    width: 4.0,\n                    height: 32.0,\n                    color: p.accent,\n                    corner_radii: CornerRadii::all(2.0),\n                });\n            }\n\n',
             ''),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the current language's name stops being accented",
        LANG,
        [
            ('                color: if is_current { p.accent } else { p.text },\n                font_weight: if is_current {',
             '                color: p.text,\n                font_weight: if is_current {'),
        ],
        ["desktop"],
        [
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: every list row is accented, not just the current one',
        LANG,
        [
            ('                color: if is_current { p.accent } else { p.text },\n                font_weight: if is_current {',
             '                color: p.accent,\n                font_weight: if is_current {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: an ordinary row's name is frozen to Mocha text",
        LANG,
        [
            ('                color: if is_current { p.accent } else { p.text },\n                font_weight: if is_current {',
             '                color: if is_current {\n                    p.accent\n                } else {\n                    guitk::color::Color::from_hex(0xCDD6F4)\n                },\n                font_weight: if is_current {'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: a row's native name is frozen to Mocha subtext0",
        LANG,
        [
            ('                font_size: 11.0,\n                color: p.subtext0,',
             '                font_size: 11.0,\n                color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: a row's native name is promoted to the primary rung",
        LANG,
        [
            ('                font_size: 11.0,\n                color: p.subtext0,',
             '                font_size: 11.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the Partial badge follows the accent',
        LANG,
        [
            ('                    color: p.yellow,\n                    corner_radii: CornerRadii::all(9.0),',
             '                    color: p.accent,\n                    corner_radii: CornerRadii::all(9.0),'),
        ],
        ["desktop"],
        [
            'the_partial_badge_is_a_property_of_the_language_not_a_selection',
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the Partial badge is frozen to Mocha yellow',
        LANG,
        [
            ('                    color: p.yellow,\n                    corner_radii: CornerRadii::all(9.0),',
             '                    color: guitk::color::Color::from_hex(0xF9E2AF),\n                    corner_radii: CornerRadii::all(9.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'the_partial_badge_is_a_property_of_the_language_not_a_selection',
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the Partial badge's ink is frozen to near-black",
        LANG,
        [
            ('                    color: readable_on(p.yellow),',
             '                    color: guitk::color::Color::from_hex(0x11111B),'),
        ],
        ["desktop"],
        [
            # The second instance of the endpoint blindness, and the sharper
            # one: near-black is the *right* answer in the dark render, so
            # the branch-coverage test -- which runs dark only -- passes too.
            # Only the two-mode comparison sees it.
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the Partial badge's ink is computed from the panel, not the badge",
        LANG,
        [
            ('                    color: readable_on(p.yellow),',
             '                    color: readable_on(p.base),'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the Partial badge marks the complete languages instead of the incomplete ones',
        LANG,
        [
            ('            if !lang.complete {',
             '            if lang.complete {'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'the_partial_badge_is_a_property_of_the_language_not_a_selection',
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the count line is frozen to Mocha overlay0',
        LANG,
        [
            ('            font_size: 11.0,\n            color: p.overlay0,',
             '            font_size: 11.0,\n            color: guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the count line is promoted to the primary text rung',
        LANG,
        [
            ('            font_size: 11.0,\n            color: p.overlay0,',
             '            font_size: 11.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the Date Format heading is frozen to Mocha lavender',
        LANG,
        [
            ('            text: "Date Format".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Date Format".into(),\n            font_size: 15.0,\n            color: guitk::color::Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'headings_keep_their_own_rung_under_every_accent',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the Time Format heading follows the accent',
        LANG,
        [
            ('            text: "Time Format".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Time Format".into(),\n            font_size: 15.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'headings_keep_their_own_rung_under_every_accent',
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the Measurement heading drops to the sub-heading rung',
        LANG,
        [
            ('            text: "Measurement".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Measurement".into(),\n            font_size: 15.0,\n            color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'headings_keep_their_own_rung_under_every_accent',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the Available Currencies sub-heading is promoted to the heading rung',
        LANG,
        [
            ('            font_size: 13.0,\n            color: p.subtext1,',
             '            font_size: 13.0,\n            color: p.lavender,'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the Currency heading is frozen to Mocha lavender',
        LANG,
        [
            ('            text: "Currency".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Currency".into(),\n            font_size: 15.0,\n            color: guitk::color::Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'headings_keep_their_own_rung_under_every_accent',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the label and value rungs of every settings row are exchanged',
        LANG,
        [
            ('            text: label.into(),\n            font_size: 13.0,\n            color: p.subtext0,',
             '            text: label.into(),\n            font_size: 13.0,\n            color: p.text,'),
            ('            text: value.into(),\n            font_size: 13.0,\n            color: p.text,',
             '            text: value.into(),\n            font_size: 13.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: a settings row's label is frozen to Mocha subtext0",
        LANG,
        [
            ('            text: label.into(),\n            font_size: 13.0,\n            color: p.subtext0,',
             '            text: label.into(),\n            font_size: 13.0,\n            color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a settings row's value is frozen to Mocha text",
        LANG,
        [
            ('            text: value.into(),\n            font_size: 13.0,\n            color: p.text,',
             '            text: value.into(),\n            font_size: 13.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the default currency's row loses its raised rung",
        LANG,
        [
            ('                color: if is_current { p.surface1 } else { p.surface0 },',
             '                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the default currency's row stops being accented",
        LANG,
        [
            ('                color: if is_current { p.accent } else { p.text },\n                font_weight: FontWeightHint::Regular,',
             '                color: p.text,\n                font_weight: FontWeightHint::Regular,'),
        ],
        ["desktop"],
        [
            # The Region tab's only position mark. It went unguarded until
            # the accent count was extended past the first two tabs -- the
            # module doc named it from the start, which is what made the
            # gap findable.
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: every currency row is accented, not just the default one',
        LANG,
        [
            ('                color: if is_current { p.accent } else { p.text },\n                font_weight: FontWeightHint::Regular,',
             '                color: p.accent,\n                font_weight: FontWeightHint::Regular,'),
        ],
        ["desktop"],
        [
            'only_the_three_position_marks_carry_the_accent',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: render resolves its own palette instead of using the one it was handed',
        LANG,
        [
            ('    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {\n        let mut cmds = Vec::new();',
             '    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {\n        let p = &Palette::for_mode(p.light);\n        let mut cmds = Vec::new();'),
        ],
        ["desktop"],
        [
            # The whole point of part 2. Every role still matches, because
            # the palette it resolves agrees with the one it was given about
            # everything except the user's accent -- so only the tests that
            # drive the accent notice, and the membership sweep does not.
            'the_fixture_reaches_every_branch_this_module_has',
            'only_the_three_position_marks_carry_the_accent',
            'the_active_tabs_label_is_computed_from_the_accent_under_it',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the probe palette's accent goes back to a stock role",
        LANG,
        [
            ('        p.accent = Color::from_hex(0xFF00FF);',
             '        p.accent = p.blue;'),
        ],
        ["desktop"],
        [
            # The guard inside `accented()` itself, which every test in the
            # module runs -- including the three pre-conversion render tests,
            # which assert nothing but non-emptiness and so can only ever
            # fail this way.
            'every_colour_this_panel_draws_comes_from_its_palette',
            'the_fixture_reaches_every_branch_this_module_has',
            'every_site_draws_the_role_it_claims',
            'only_the_three_position_marks_carry_the_accent',
            'the_partial_badge_is_a_property_of_the_language_not_a_selection',
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
            'headings_keep_their_own_rung_under_every_accent',
            'an_empty_search_box_is_dimmer_than_one_with_a_query_in_it',
            'the_render_is_not_the_same_in_both_modes',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'test_ui_render_language_tab',
            'test_ui_render_formats_tab',
            'test_ui_render_region_tab',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the fixture's query filters the current language out of the list",
        LANG,
        [
            ('        ui.language_search = "n".to_string();',
             '        ui.language_search = "an".to_string();'),
        ],
        ["desktop"],
        [
            # "an" matches Poland and German but not English, so the row
            # carrying the marker bar and the accented name disappears --
            # which is how the first draft of this fixture silently lost two
            # branches at once.
            'the_fixture_reaches_every_branch_this_module_has',
            'every_site_draws_the_role_it_claims',
            'only_the_three_position_marks_carry_the_accent',
            'an_empty_search_box_is_dimmer_than_one_with_a_query_in_it',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the fixture's languages are all complete, so the badge site goes unchecked",
        LANG,
        [
            ('            Language::new("pl-PL", "Polish (Poland)", "Polski", false),',
             '            Language::new("pl-PL", "Polish (Poland)", "Polski", true),'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'the_partial_badge_is_a_property_of_the_language_not_a_selection',
            'the_badge_ink_is_computed_from_the_badge_it_sits_on',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the fixture selects the row that is already current, so the two marks coincide",
        LANG,
        [
            ('        ui.selected_language_index = Some(2);',
             '        ui.selected_language_index = Some(0);'),
        ],
        ["desktop"],
        [
            # Every count is unchanged -- three rows, one raised, three
            # accents. Only the positional row vector notices that the
            # raised row moved, which is the same blindness as defect S but
            # arriving through the fixture rather than the renderer.
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the fixture's search box is empty, so the typed-text branch goes unchecked",
        LANG,
        [
            ('        ui.language_search = "n".to_string();',
             '        ui.language_search = String::new();'),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'an_empty_search_box_is_dimmer_than_one_with_a_query_in_it',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the fixture drops its third language, so the selected-row branch goes unchecked",
        LANG,
        [
            ('            Language::new("de-DE", "German (Germany)", "Deutsch", true),\n',
             ''),
        ],
        ["desktop"],
        [
            'the_fixture_reaches_every_branch_this_module_has',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the active tab's label is computed from the panel instead of the tab",
        LANG,
        [
            ('                color: if active {\n                    readable_on(p.accent)\n                } else {\n                    p.subtext0\n                },',
             '                color: if active {\n                    readable_on(p.base)\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            'the_active_tabs_label_is_computed_from_the_accent_under_it',
        ],
    ),
    # ------------------------------------------------------------------
    # Module 31: default_apps.rs. Eleven constants over 33 colour sites
    # and three tabs.
    #
    # Four judgements are written into the module docs and each has a test
    # that can refute it. These defects break every one of them, in the two
    # shapes that matter: a role frozen to the Mocha value it used to be
    # (which only the *light* render can see) and a role swapped for a
    # neighbouring role (which no membership sweep can see at all, because
    # both values are legal members).
    #
    # Two sites deserve naming. The chip ink used to read `CRUST`, and
    # `readable_on` answers exactly `CRUST` for the *stock* accent -- so a
    # frozen value and the correct call are the same pixel until the accent
    # moves, and the sweep allows both endpoints outright. Only the accent
    # sweep in `the_current_chips_ink_is_computed_from_the_accent_under_it`
    # separates them. And the "None set" case is a real defect this
    # conversion found rather than an invented one: accenting a category
    # with no handler accents twelve of twelve cards and so marks nothing.
    # ------------------------------------------------------------------
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the panel background is frozen to Mocha base',
        DAPP,
        [
            ('            height,\n            color: p.base,',
             '            height,\n            color: guitk::color::Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            # Not an accident of over-broad assertion: a *light* render whose
            # panel is frozen to Mocha base puts a near-black panel behind a
            # pale well, so the recess inverts. The relational test sees the
            # freeze that the two role tests see, by a different route.
            'the_content_well_is_deeper_than_the_panel_it_sits_in',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the panel background is drawn one rung too deep',
        DAPP,
        [
            ('            height,\n            color: p.base,',
             '            height,\n            color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the panel title is frozen to Mocha text',
        DAPP,
        [
            ('            font_size: 22.0,\n            color: p.text,',
             '            font_size: 22.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the panel title is drawn at the subtitle rung',
        DAPP,
        [
            ('            font_size: 22.0,\n            color: p.text,',
             '            font_size: 22.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the open tab's fill is frozen to Mocha surface0",
        DAPP,
        [
            ('                    height: 32.0,\n                    color: p.surface0,\n                    corner_radii: CornerRadii::all(6.0),\n                });\n            }',
             '                    height: 32.0,\n                    color: guitk::color::Color::from_hex(0x313244),\n                    corner_radii: CornerRadii::all(6.0),\n                });\n            }'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the open tab's fill is raised a rung above its strip",
        DAPP,
        [
            ('                    height: 32.0,\n                    color: p.surface0,\n                    corner_radii: CornerRadii::all(6.0),\n                });\n            }',
             '                    height: 32.0,\n                    color: p.surface1,\n                    corner_radii: CornerRadii::all(6.0),\n                });\n            }'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the tab strip stops marking which tab is open',
        DAPP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the open and idle tab labels are exchanged',
        DAPP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: if is_active { p.subtext0 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: an idle tab label is frozen to Mocha subtext0',
        DAPP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: if is_active { p.accent } else { guitk::color::Color::from_hex(0xA6ADC8) },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the content well is frozen to Mocha crust',
        DAPP,
        [
            ('            // base, so this reads as a recess in either mode.\n            color: p.crust,',
             '            // base, so this reads as a recess in either mode.\n            color: guitk::color::Color::from_hex(0x11111B),'),
        ],
        ["desktop"],
        [
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the content well stops being a recess in the panel',
        DAPP,
        [
            ('            // base, so this reads as a recess in either mode.\n            color: p.crust,',
             '            // base, so this reads as a recess in either mode.\n            color: p.base,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_content_well_is_deeper_than_the_panel_it_sits_in',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the tab's subtitle is frozen to Mocha subtext0",
        DAPP,
        [
            ('            text: "Choose default apps for each type of content".to_string(),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: "Choose default apps for each type of content".to_string(),\n            font_size: 12.0,\n            color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the tab's subtitle is drawn a rung too bright",
        DAPP,
        [
            ('            text: "Choose default apps for each type of content".to_string(),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: "Choose default apps for each type of content".to_string(),\n            font_size: 12.0,\n            color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the Reset all button is frozen to Mocha surface1',
        DAPP,
        [
            ('            height: 24.0,\n            color: p.surface1,',
             '            height: 24.0,\n            color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the Reset all button sinks to the card rung',
        DAPP,
        [
            ('            height: 24.0,\n            color: p.surface1,',
             '            height: 24.0,\n            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the Reset all label is frozen to Mocha peach',
        DAPP,
        [
            ('            // the shipped defaults, which is a state rather than a position.\n            color: p.peach,',
             '            // the shipped defaults, which is a state rather than a position.\n            color: guitk::color::Color::from_hex(0xFAB387),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: undoing a customisation is marked with the accent',
        DAPP,
        [
            ('            // the shipped defaults, which is a state rather than a position.\n            color: p.peach,',
             '            // the shipped defaults, which is a state rather than a position.\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
            'peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: a category card is frozen to Mocha surface0',
        DAPP,
        [
            ('                height: card_h,\n                color: p.surface0,',
             '                height: card_h,\n                color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: a category card is raised a rung off the well',
        DAPP,
        [
            ('                height: card_h,\n                color: p.surface0,',
             '                height: card_h,\n                color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: a category's icon is frozen to Mocha text",
        DAPP,
        [
            ('                font_size: 20.0,\n                color: p.text,',
             '                font_size: 20.0,\n                color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: a category's icon is dimmer than the name beside it",
        DAPP,
        [
            ('                font_size: 20.0,\n                color: p.text,',
             '                font_size: 20.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: a category's name is frozen to Mocha text",
        DAPP,
        [
            ('                text: category.label().to_string(),\n                font_size: 14.0,\n                color: p.text,',
             '                text: category.label().to_string(),\n                font_size: 14.0,\n                color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a category's name is drawn at the heading rung",
        DAPP,
        [
            ('                text: category.label().to_string(),\n                font_size: 14.0,\n                color: p.text,',
             '                text: category.label().to_string(),\n                font_size: 14.0,\n                color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: a category with no handler is accented as though it had one',
        DAPP,
        [
            ('                color: if default_app.is_some() {\n                    p.accent\n                } else {\n                    p.overlay0\n                },',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
            'a_category_with_no_default_app_is_not_accented_as_if_it_had_one',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the app in force under a card stops being accented',
        DAPP,
        [
            ('                color: if default_app.is_some() {\n                    p.accent\n                } else {\n                    p.overlay0\n                },',
             '                color: if default_app.is_some() {\n                    p.lavender\n                } else {\n                    p.overlay0\n                },'),
        ],
        ["desktop"],
        [
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: an unset category is frozen to Mocha overlay0',
        DAPP,
        [
            ('                color: if default_app.is_some() {\n                    p.accent\n                } else {\n                    p.overlay0\n                },',
             '                color: if default_app.is_some() {\n                    p.accent\n                } else {\n                    guitk::color::Color::from_hex(0x6C7086)\n                },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'a_category_with_no_default_app_is_not_accented_as_if_it_had_one',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the expand chevron is frozen to Mocha overlay0',
        DAPP,
        [
            ('                text: if is_expanded { "\\u{25B2}" } else { "\\u{25BC}" }.to_string(),\n                font_size: 12.0,\n                color: p.overlay0,',
             '                text: if is_expanded { "\\u{25B2}" } else { "\\u{25BC}" }.to_string(),\n                font_size: 12.0,\n                color: guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the expand chevron is as bright as the text it sits beside',
        DAPP,
        [
            ('                text: if is_expanded { "\\u{25B2}" } else { "\\u{25BC}" }.to_string(),\n                font_size: 12.0,\n                color: p.overlay0,',
             '                text: if is_expanded { "\\u{25B2}" } else { "\\u{25BC}" }.to_string(),\n                font_size: 12.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the current app's chip and its rivals' are exchanged",
        DAPP,
        [
            ('                        color: if is_current { p.accent } else { p.surface1 },',
             '                        color: if is_current { p.surface1 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: a rival app's chip is frozen to Mocha surface1",
        DAPP,
        [
            ('                        color: if is_current { p.accent } else { p.surface1 },',
             '                        color: if is_current { p.accent } else { guitk::color::Color::from_hex(0x45475A) },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the current chip's ink is frozen to the CRUST it used to name",
        DAPP,
        [
            ('                        color: if is_current {\n                            readable_on(p.accent)\n                        } else {\n                            p.text\n                        },',
             '                        color: if is_current {\n                            guitk::color::Color::from_hex(0x11111B)\n                        } else {\n                            p.text\n                        },'),
        ],
        ["desktop"],
        [
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'the_current_chips_ink_is_computed_from_the_accent_under_it',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the current chip's ink is named as a role instead of computed",
        DAPP,
        [
            ('                        color: if is_current {\n                            readable_on(p.accent)\n                        } else {\n                            p.text\n                        },',
             '                        color: if is_current {\n                            p.crust\n                        } else {\n                            p.text\n                        },'),
        ],
        ["desktop"],
        [
            'the_current_chips_ink_is_computed_from_the_accent_under_it',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: an idle chip's ink is frozen to Mocha text",
        DAPP,
        [
            ('                        color: if is_current {\n                            readable_on(p.accent)\n                        } else {\n                            p.text\n                        },',
             '                        color: if is_current {\n                            readable_on(p.accent)\n                        } else {\n                            guitk::color::Color::from_hex(0xCDD6F4)\n                        },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'the_current_chips_ink_is_computed_from_the_accent_under_it',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the file-type search box is frozen to Mocha surface0',
        DAPP,
        [
            ('            height: 32.0,\n            color: p.surface0,\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search file types...".to_string()',
             '            height: 32.0,\n            color: guitk::color::Color::from_hex(0x313244),\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search file types...".to_string()'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the file-type search box is raised a rung off the well',
        DAPP,
        [
            ('            height: 32.0,\n            color: p.surface0,\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search file types...".to_string()',
             '            height: 32.0,\n            color: p.surface1,\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search file types...".to_string()'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the installed-app search box is frozen to Mocha surface0',
        DAPP,
        [
            ('            height: 32.0,\n            color: p.surface0,\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search apps...".to_string()',
             '            height: 32.0,\n            color: guitk::color::Color::from_hex(0x313244),\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search apps...".to_string()'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the installed-app search box is raised a rung off the well',
        DAPP,
        [
            ('            height: 32.0,\n            color: p.surface0,\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search apps...".to_string()',
             '            height: 32.0,\n            color: p.surface1,\n            corner_radii: CornerRadii::all(6.0),\n        });\n\n        let search_text = if self.search_query.is_empty() {\n            "Search apps...".to_string()'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the file-type placeholder and a typed query are exchanged',
        DAPP,
        [
            ('            "Search file types...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            "Search file types...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                p.text\n            } else {\n                p.overlay0\n            },'),
        ],
        ["desktop"],
        [
            'an_empty_search_box_is_dimmer_than_a_typed_query',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the file-type placeholder is frozen to Mocha overlay0',
        DAPP,
        [
            ('            "Search file types...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            "Search file types...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                guitk::color::Color::from_hex(0x6C7086)\n            } else {\n                p.text\n            },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'an_empty_search_box_is_dimmer_than_a_typed_query',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the installed-app placeholder and a typed query are exchanged',
        DAPP,
        [
            ('            "Search apps...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            "Search apps...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                p.text\n            } else {\n                p.overlay0\n            },'),
        ],
        ["desktop"],
        [
            'an_empty_search_box_is_dimmer_than_a_typed_query',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the installed-app placeholder is frozen to Mocha overlay0',
        DAPP,
        [
            ('            "Search apps...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                p.overlay0\n            } else {\n                p.text\n            },',
             '            "Search apps...".to_string()\n        } else {\n            self.search_query.clone()\n        };\n\n        cmds.push(RenderCommand::Text {\n            x: x + 12.0,\n            y: row_y + 8.0,\n            text: search_text,\n            font_size: 12.0,\n            color: if self.search_query.is_empty() {\n                guitk::color::Color::from_hex(0x6C7086)\n            } else {\n                p.text\n            },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'an_empty_search_box_is_dimmer_than_a_typed_query',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the custom-association count is frozen to Mocha subtext0',
        DAPP,
        [
            ('            ),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            ),\n            font_size: 12.0,\n            color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the custom-association count is drawn at the heading rung',
        DAPP,
        [
            ('            ),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            ),\n            font_size: 12.0,\n            color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: a file-type group heading is frozen to Mocha subtext1',
        DAPP,
        [
            ('                font_size: 13.0,\n                color: p.subtext1,',
             '                font_size: 13.0,\n                color: guitk::color::Color::from_hex(0xBAC2DE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: a file-type group heading sinks to the body rung',
        DAPP,
        [
            ('                font_size: 13.0,\n                color: p.subtext1,',
             '                font_size: 13.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: an extension row is frozen to Mocha surface0',
        DAPP,
        [
            ('                    height: 32.0,\n                    color: p.surface0,\n                    corner_radii: CornerRadii::all(4.0),',
             '                    height: 32.0,\n                    color: guitk::color::Color::from_hex(0x313244),\n                    corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: an extension row is raised a rung off the well',
        DAPP,
        [
            ('                    height: 32.0,\n                    color: p.surface0,\n                    corner_radii: CornerRadii::all(4.0),',
             '                    height: 32.0,\n                    color: p.surface1,\n                    corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the extension pill is frozen to Mocha surface1',
        DAPP,
        [
            ('                    width: 48.0,\n                    height: 20.0,\n                    color: p.surface1,',
             '                    width: 48.0,\n                    height: 20.0,\n                    color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the extension pill is raised a rung off its row',
        DAPP,
        [
            ('                    width: 48.0,\n                    height: 20.0,\n                    color: p.surface1,',
             '                    width: 48.0,\n                    height: 20.0,\n                    color: p.surface2,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the extension token is frozen to Mocha lavender',
        DAPP,
        [
            ('                    text: format!(".{ext}"),\n                    font_size: 11.0,\n                    color: p.lavender,',
             '                    text: format!(".{ext}"),\n                    font_size: 11.0,\n                    color: guitk::color::Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the extension token drifts to the neighbouring accent hue',
        DAPP,
        [
            ('                    text: format!(".{ext}"),\n                    font_size: 11.0,\n                    color: p.lavender,',
             '                    text: format!(".{ext}"),\n                    font_size: 11.0,\n                    color: p.mauve,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: a custom association stops being marked as one',
        DAPP,
        [
            ('                    color: if is_custom { p.peach } else { p.text },',
             '                    color: p.text,'),
        ],
        ["desktop"],
        [
            'peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the custom and default handler inks are exchanged',
        DAPP,
        [
            ('                    color: if is_custom { p.peach } else { p.text },',
             '                    color: if is_custom { p.text } else { p.peach },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: a default handler name is frozen to Mocha text',
        DAPP,
        [
            ('                    color: if is_custom { p.peach } else { p.text },',
             '                    color: if is_custom { p.peach } else { guitk::color::Color::from_hex(0xCDD6F4) },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the Custom badge is frozen to Mocha peach',
        DAPP,
        [
            ('                        text: "Custom".to_string(),\n                        font_size: 10.0,\n                        color: p.peach,',
             '                        text: "Custom".to_string(),\n                        font_size: 10.0,\n                        color: guitk::color::Color::from_hex(0xFAB387),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the Custom badge is marked with the accent instead',
        DAPP,
        [
            ('                        text: "Custom".to_string(),\n                        font_size: 10.0,\n                        color: p.peach,',
             '                        text: "Custom".to_string(),\n                        font_size: 10.0,\n                        color: p.accent,'),
        ],
        ["desktop"],
        [
            'the_accent_marks_which_app_is_in_force_and_nothing_else',
            'peach_marks_a_departure_from_the_defaults_and_does_not_follow_the_accent',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the installed-app count is frozen to Mocha subtext0',
        DAPP,
        [
            ('            text: format!("{total} installed apps ({third_party} third-party)"),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: format!("{total} installed apps ({third_party} third-party)"),\n            font_size: 12.0,\n            color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the installed-app count is drawn at the heading rung',
        DAPP,
        [
            ('            text: format!("{total} installed apps ({third_party} third-party)"),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: format!("{total} installed apps ({third_party} third-party)"),\n            font_size: 12.0,\n            color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: an installed-app row is frozen to Mocha surface0',
        DAPP,
        [
            ('                height: 56.0,\n                color: p.surface0,',
             '                height: 56.0,\n                color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: an installed-app row is raised a rung off the well',
        DAPP,
        [
            ('                height: 56.0,\n                color: p.surface0,',
             '                height: 56.0,\n                color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: an installed app's name is frozen to Mocha text",
        DAPP,
        [
            ('                text: app.name.clone(),\n                font_size: 14.0,\n                color: p.text,',
             '                text: app.name.clone(),\n                font_size: 14.0,\n                color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: an installed app's name is drawn at the heading rung",
        DAPP,
        [
            ('                text: app.name.clone(),\n                font_size: 14.0,\n                color: p.text,',
             '                text: app.name.clone(),\n                font_size: 14.0,\n                color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: an app's description is frozen to Mocha subtext0",
        DAPP,
        [
            ('                text: app.description.clone(),\n                font_size: 11.0,\n                color: p.subtext0,',
             '                text: app.description.clone(),\n                font_size: 11.0,\n                color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: an app's description sinks to the dimmest rung on its row",
        DAPP,
        [
            ('                text: app.description.clone(),\n                font_size: 11.0,\n                color: p.subtext0,',
             '                text: app.description.clone(),\n                font_size: 11.0,\n                color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the System badge pill is frozen to Mocha surface1',
        DAPP,
        [
            ('                    width: 52.0,\n                    height: 18.0,\n                    color: p.surface1,',
             '                    width: 52.0,\n                    height: 18.0,\n                    color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the System badge pill sinks into the row behind it',
        DAPP,
        [
            ('                    width: 52.0,\n                    height: 18.0,\n                    color: p.surface1,',
             '                    width: 52.0,\n                    height: 18.0,\n                    color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the System badge label is frozen to Mocha overlay0',
        DAPP,
        [
            ('                    text: "System".to_string(),\n                    font_size: 10.0,\n                    color: p.overlay0,',
             '                    text: "System".to_string(),\n                    font_size: 10.0,\n                    color: guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the System badge label is brighter than the pill under it',
        DAPP,
        [
            ('                    text: "System".to_string(),\n                    font_size: 10.0,\n                    color: p.overlay0,',
             '                    text: "System".to_string(),\n                    font_size: 10.0,\n                    color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the join line under an app row is frozen to Mocha overlay0',
        DAPP,
        [
            ('                    text: categories.join(", "),\n                    font_size: 10.0,\n                    color: p.overlay0,',
             '                    text: categories.join(", "),\n                    font_size: 10.0,\n                    color: guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the join line under an app row is as bright as its description',
        DAPP,
        [
            ('                    text: categories.join(", "),\n                    font_size: 10.0,\n                    color: p.overlay0,',
             '                    text: categories.join(", "),\n                    font_size: 10.0,\n                    color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the drop shadow keeps the alpha this module chose for itself, one of three different answers three popups gave',
        LAUN,
        [('            color: p.shadow(),',
          '            color: Color::rgba(0, 0, 0, 100),')],
        ["desktop"],
        [
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the drop shadow is a role, so it inverts with the theme instead of being an absence of light',
        LAUN,
        [('            color: p.shadow(),',
          '            color: p.crust,')],
        ["desktop"],
        [
            # Not the membership sweep: `p.crust` *is* a role of the light
            # palette, so the sweep is right to accept it. Only the claim that
            # a shadow is black in both modes can see this one.
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the drop shadow is the text shadow, which is three times as dark',
        LAUN,
        [('            color: p.shadow(),',
          '            color: p.text_shadow(),')],
        ["desktop"],
        [
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the dialog background is frozen to Mocha base',
        LAUN,
        [('            color: with_alpha(p.base, DIALOG_ALPHA),',
          '            color: Color::from_hex(0x1E1E2E),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the dialog background sits on the mantle rung rather than the base one',
        LAUN,
        [('            color: with_alpha(p.base, DIALOG_ALPHA),',
          '            color: with_alpha(p.mantle, DIALOG_ALPHA),')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the dialog reaches full opacity, so it stops reading as lifted off the desktop',
        LAUN,
        [('            color: with_alpha(p.base, DIALOG_ALPHA),',
          '            color: p.base,')],
        ["desktop"],
        [
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: DIALOG_ALPHA is opaque',
        LAUN,
        [('const DIALOG_ALPHA: u8 = 240;',
          'const DIALOG_ALPHA: u8 = 255;')],
        ["desktop"],
        [
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: DIALOG_ALPHA is so low the wallpaper reads through the result list',
        LAUN,
        [('const DIALOG_ALPHA: u8 = 240;',
          'const DIALOG_ALPHA: u8 = 160;')],
        ["desktop"],
        [
            "the_dialog_floats_over_a_shadow_rather_than_sitting_on_the_desktop",
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the search field is frozen to Mocha mantle',
        LAUN,
        [('            color: p.mantle,',
          '            color: Color::from_hex(0x181825),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the search field is the same rung as the dialog around it, so the well stops looking like a well',
        LAUN,
        [('            color: p.mantle,',
          '            color: p.base,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the search field is a raised rung rather than a sunken one',
        LAUN,
        [('            color: p.mantle,',
          '            color: p.surface0,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the search field's border is frozen to Mocha surface2",
        LAUN,
        [('            color: p.surface2,',
          '            color: Color::from_hex(0x585B70),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the search field's border is a rung dimmer than it claims",
        LAUN,
        [('            color: p.surface2,',
          '            color: p.surface1,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the search field's border is drawn in the border role the overlays use",
        LAUN,
        [('            color: p.surface2,',
          '            color: p.overlay0,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the placeholder is frozen to Mocha overlay0',
        LAUN,
        [('                text: "Search...".to_string(),\n                color: p.overlay0,',
          '                text: "Search...".to_string(),\n                color: Color::from_hex(0x6C7086),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
            "an_empty_query_is_dimmer_than_a_typed_one",
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the placeholder is as bright as text the user actually typed',
        LAUN,
        [('                text: "Search...".to_string(),\n                color: p.overlay0,',
          '                text: "Search...".to_string(),\n                color: p.text,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "an_empty_query_is_dimmer_than_a_typed_one",
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the placeholder is a prompt-coloured hint rather than the dimmest thing in the field',
        LAUN,
        [('                text: "Search...".to_string(),\n                color: p.overlay0,',
          '                text: "Search...".to_string(),\n                color: p.subtext0,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "an_empty_query_is_dimmer_than_a_typed_one",
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the typed query is frozen to Mocha text',
        LAUN,
        [('                color: p.text,\n                font_size: INPUT_FONT_SIZE,',
          '                color: Color::from_hex(0xCDD6F4),\n                font_size: INPUT_FONT_SIZE,')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
            "an_empty_query_is_dimmer_than_a_typed_one",
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the typed query is as dim as the prompt it replaced',
        LAUN,
        [('                color: p.text,\n                font_size: INPUT_FONT_SIZE,',
          '                color: p.overlay0,\n                font_size: INPUT_FONT_SIZE,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "an_empty_query_is_dimmer_than_a_typed_one",
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the typed query is a rung below the brightest ink',
        LAUN,
        [('                color: p.text,\n                font_size: INPUT_FONT_SIZE,',
          '                color: p.subtext1,\n                font_size: INPUT_FONT_SIZE,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "an_empty_query_is_dimmer_than_a_typed_one",
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the caret is the blue the accent happens to be, which is the trap this whole module is about: under the shipped theme it is the same pixel',
        LAUN,
        [('            y2: text_y + INPUT_FONT_SIZE,\n            color: p.accent,',
          '            y2: text_y + INPUT_FONT_SIZE,\n            color: p.blue,')],
        ["desktop"],
        [
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "the_caret_sits_where_the_query_text_ends",
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the caret is frozen to Mocha blue',
        LAUN,
        [('            y2: text_y + INPUT_FONT_SIZE,\n            color: p.accent,',
          '            y2: text_y + INPUT_FONT_SIZE,\n            color: Color::from_hex(0x89B4FA),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "the_caret_sits_where_the_query_text_ends",
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the caret is ordinary ink, so nothing marks where you are typing',
        LAUN,
        [('            y2: text_y + INPUT_FONT_SIZE,\n            color: p.accent,',
          '            y2: text_y + INPUT_FONT_SIZE,\n            color: p.text,')],
        ["desktop"],
        [
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "the_caret_sits_where_the_query_text_ends",
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the selected row is frozen to Mocha surface1',
        LAUN,
        [('                    color: p.surface1,',
          '                    color: Color::from_hex(0x45475A),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the selected row is a rung lower, so selection is nearly invisible',
        LAUN,
        [('                    color: p.surface1,',
          '                    color: p.surface0,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the selected row is drawn on the sunken rung the search field uses',
        LAUN,
        [('                    color: p.surface1,',
          '                    color: p.mantle,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the selection bar is the blue that is only coincidentally the accent',
        LAUN,
        [('                    color: p.accent,',
          '                    color: p.blue,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the selection bar is frozen to Mocha blue',
        LAUN,
        [('                    color: p.accent,',
          '                    color: Color::from_hex(0x89B4FA),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the selection bar is a border colour, so the marked row is marked with furniture rather than with the user's own accent",
        LAUN,
        [('                    color: p.accent,',
          '                    color: p.surface2,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: every row's icon is blue, so the icon stops saying what kind of thing the row is",
        LAUN,
        [('                height: 24.0,\n                color: entry.category.color(p),',
          '                height: 24.0,\n                color: p.blue,')],
        ["desktop"],
        [
            "the_five_category_hues_stay_five_distinct_colours",
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: every row's icon is frozen to Mocha blue",
        LAUN,
        [('                height: 24.0,\n                color: entry.category.color(p),',
          '                height: 24.0,\n                color: Color::from_hex(0x89B4FA),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "the_five_category_hues_stay_five_distinct_colours",
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: every row's icon takes the accent, so a mark of category becomes a mark of position five times over",
        LAUN,
        [('                height: 24.0,\n                color: entry.category.color(p),',
          '                height: 24.0,\n                color: p.accent,')],
        ["desktop"],
        [
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "the_five_category_hues_stay_five_distinct_colours",
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the selected row's name and the unselected rows' names are the wrong way round, so the list points at the row you are not on",
        LAUN,
        [('                color: if is_selected { p.text } else { p.subtext1 },',
          '                color: if is_selected { p.subtext1 } else { p.text },')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: every row's name is drawn at the unselected brightness",
        LAUN,
        [('                color: if is_selected { p.text } else { p.subtext1 },',
          '                color: p.subtext1,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the selected row's name is frozen to Mocha text",
        LAUN,
        [('                color: if is_selected { p.text } else { p.subtext1 },',
          '                color: if is_selected {\n                    Color::from_hex(0xCDD6F4)\n                } else {\n                    p.subtext1\n                },')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: an unselected row's name is frozen to Mocha subtext1",
        LAUN,
        [('                color: if is_selected { p.text } else { p.subtext1 },',
          '                color: if is_selected {\n                    p.text\n                } else {\n                    Color::from_hex(0xBAC2DE)\n                },')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: a row's description is frozen to Mocha subtext0",
        LAUN,
        [('                color: p.subtext0,',
          '                color: Color::from_hex(0xA6ADC8),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: a row's description is as bright as the name above it",
        LAUN,
        [('                color: p.subtext0,',
          '                color: p.subtext1,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: a row's description is dimmer than the placeholder in an empty field",
        LAUN,
        [('                color: p.subtext0,',
          '                color: p.overlay0,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the badge wash names a hue beside the badge rather than deriving it, so the two are free to disagree the day a category is added',
        LAUN,
        [('                color: with_alpha(entry.category.color(p), BADGE_WASH_ALPHA),',
          '                color: with_alpha(p.blue, BADGE_WASH_ALPHA),')],
        ["desktop"],
        [
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the badge wash is fully solid, so the label on it is read against its own colour',
        LAUN,
        [('                color: with_alpha(entry.category.color(p), BADGE_WASH_ALPHA),',
          '                color: entry.category.color(p),')],
        ["desktop"],
        [
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the badge wash is opaque by an explicit alpha rather than by dropping the call',
        LAUN,
        [('                color: with_alpha(entry.category.color(p), BADGE_WASH_ALPHA),',
          '                color: with_alpha(entry.category.color(p), 255),')],
        ["desktop"],
        [
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: BADGE_WASH_ALPHA is high enough that the wash is a fill rather than a tint',
        LAUN,
        [('const BADGE_WASH_ALPHA: u8 = 40;',
          'const BADGE_WASH_ALPHA: u8 = 200;')],
        ["desktop"],
        [
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the badge label is ordinary ink, so the badge says nothing the row did not already say',
        LAUN,
        [('                text: badge_text.to_string(),\n                color: entry.category.color(p),',
          '                text: badge_text.to_string(),\n                color: p.text,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "the_five_category_hues_stay_five_distinct_colours",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the badge label is frozen to Mocha blue',
        LAUN,
        [('                text: badge_text.to_string(),\n                color: entry.category.color(p),',
          '                text: badge_text.to_string(),\n                color: Color::from_hex(0x89B4FA),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
            "the_five_category_hues_stay_five_distinct_colours",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the badge label takes the accent, so every badge follows the user's accent and none of them says what kind of thing the row is",
        LAUN,
        [('                text: badge_text.to_string(),\n                color: entry.category.color(p),',
          '                text: badge_text.to_string(),\n                color: p.accent,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "the_five_category_hues_stay_five_distinct_colours",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
            "a_badge_wash_is_its_own_hue_at_a_lower_alpha",
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the no-results line is frozen to Mocha overlay0',
        LAUN,
        [('                text: "No results found".to_string(),\n                color: p.overlay0,',
          '                text: "No results found".to_string(),\n                color: Color::from_hex(0x6C7086),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the no-results line is an error rather than a quiet statement of fact',
        LAUN,
        [('                text: "No results found".to_string(),\n                color: p.overlay0,',
          '                text: "No results found".to_string(),\n                color: p.red,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the no-results line is as bright as a result would have been',
        LAUN,
        [('                text: "No results found".to_string(),\n                color: p.overlay0,',
          '                text: "No results found".to_string(),\n                color: p.text,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the App category is frozen to Mocha blue, which every test that asks Category::color what it meant will agree with',
        LAUN,
        [('            Self::Application => p.blue,',
          '            Self::Application => Color::from_hex(0x89B4FA),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the Sys category is frozen to Mocha red, which every test that asks Category::color what it meant will agree with',
        LAUN,
        [('            Self::System => p.red,',
          '            Self::System => Color::from_hex(0xF38BA8),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the Set category is frozen to Mocha peach, which every test that asks Category::color what it meant will agree with',
        LAUN,
        [('            Self::Setting => p.peach,',
          '            Self::Setting => Color::from_hex(0xFAB387),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the File category is frozen to Mocha green, which every test that asks Category::color what it meant will agree with',
        LAUN,
        [('            Self::File => p.green,',
          '            Self::File => Color::from_hex(0xA6E3A1),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the Cmd category is frozen to Mocha mauve, which every test that asks Category::color what it meant will agree with',
        LAUN,
        [('            Self::Command => p.mauve,',
          '            Self::Command => Color::from_hex(0xCBA6F7),')],
        ["desktop"],
        [
            "every_colour_this_launcher_draws_comes_from_its_palette",
            "none_of_the_thirteen_deleted_constants_is_still_drawn",
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the Application category is the accent rather than a named hue, which under the shipped theme is the same pixel and so cannot be seen at all',
        LAUN,
        [('            Self::Application => p.blue,',
          '            Self::Application => p.accent,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "the_accent_marks_where_you_are_and_never_what_a_thing_is",
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: two categories collapse onto one hue, so a badge cannot say which of them a row belongs to',
        LAUN,
        [('            Self::System => p.red,',
          '            Self::System => p.peach,')],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
            "the_five_category_hues_stay_five_distinct_colours",
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: Application and System trade hues, which no set-membership check can see because the set is unchanged',
        LAUN,
        [
         ('            Self::Application => p.blue,',
          '            Self::Application => p.red,'),
         ('            Self::System => p.red,',
          '            Self::System => p.blue,'),
        ],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: all five category hues are rotated by one: still five distinct colours, still whatever Category::color says they are, and every row wrong',
        LAUN,
        [
         ('            Self::Application => p.blue,',
          '            Self::Application => p.red,'),
         ('            Self::System => p.red,',
          '            Self::System => p.peach,'),
         ('            Self::Setting => p.peach,',
          '            Self::Setting => p.green,'),
         ('            Self::File => p.green,',
          '            Self::File => p.mauve,'),
         ('            Self::Command => p.mauve,',
          '            Self::Command => p.blue,'),
        ],
        ["desktop"],
        [
            "every_site_draws_the_role_it_claims",
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the CPU hue is frozen to its Mocha value, so a light theme still draws the dark one',
        RESMON,
        [
            ('            Self::Cpu => p.blue,',
             '            Self::Cpu => guitk::color::Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'each_measurement_is_pinned_to_the_role_it_names',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the CPU graph is accented, which makes the monitor say "you are here" about a quantity',
        RESMON,
        [
            ('            Self::Cpu => p.blue,',
             '            Self::Cpu => p.accent,'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'each_measurement_is_pinned_to_the_role_it_names',
            'test_render_expanded_has_resource_labels',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the four graphed hues are rotated by one -- still four distinct colours, every graph in its neighbour's",
        RESMON,
        [
            ('            Self::Cpu => p.blue,',
             '            Self::Cpu => p.green,'),
            ('            Self::Memory => p.green,',
             '            Self::Memory => p.peach,'),
            ('            Self::Disk => p.peach,',
             '            Self::Disk => p.mauve,'),
            ('            Self::Network => p.mauve,',
             '            Self::Network => p.blue,'),
        ],
        ["desktop"],
        [
            'each_measurement_is_pinned_to_the_role_it_names',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: all six hues rotate, so the GPU takes the temperature colour and the CPU takes memory's",
        RESMON,
        [
            ('            Self::Cpu => p.blue,',
             '            Self::Cpu => p.green,'),
            ('            Self::Memory => p.green,',
             '            Self::Memory => p.peach,'),
            ('            Self::Disk => p.peach,',
             '            Self::Disk => p.mauve,'),
            ('            Self::Network => p.mauve,',
             '            Self::Network => p.lavender,'),
            ('            Self::Gpu => p.lavender,',
             '            Self::Gpu => p.red,'),
            ('            Self::Temperature => p.red,',
             '            Self::Temperature => p.blue,'),
        ],
        ["desktop"],
        [
            'each_measurement_is_pinned_to_the_role_it_names',
            'test_render_expanded_has_resource_labels',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the GPU and the CPU are the same hue, so two graphs would be indistinguishable',
        RESMON,
        [
            ('            Self::Gpu => p.lavender,',
             '            Self::Gpu => p.blue,'),
        ],
        ["desktop"],
        [
            'each_measurement_is_pinned_to_the_role_it_names',
            'test_resource_type_colors_distinct',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the temperature hue is frozen to Mocha red -- and temperature is never plotted, so nothing that looks at the screen can see it',
        RESMON,
        [
            ('            Self::Temperature => p.red,',
             '            Self::Temperature => guitk::color::Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'each_measurement_is_pinned_to_the_role_it_names',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the compact strip's background is frozen to Mocha base",
        RESMON,
        [
            ('            color: p.base,\n            corner_radii: CornerRadii::all(4.0),',
             '            color: guitk::color::Color::from_hex(0x1E1E2E),\n            corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the compact strip sits a rung above the desktop instead of on the base',
        RESMON,
        [
            ('            color: p.base,\n            corner_radii: CornerRadii::all(4.0),',
             '            color: p.mantle,\n            corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'test_render_compact_empty_produces_background',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the compact strip's border is frozen to Mocha surface0",
        RESMON,
        [
            ('            color: p.surface0,\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(4.0),',
             '            color: guitk::color::Color::from_hex(0x313244),\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the compact strip's border is a rung too bright for furniture",
        RESMON,
        [
            ('            color: p.surface0,\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(4.0),',
             '            color: p.surface2,\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the compact sparklines are all accented rather than each drawn in its metric's hue",
        RESMON,
        [
            ('            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, res.color(p));',
             '            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, p.accent);'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'a_metric_is_one_colour_wherever_it_appears',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: every compact sparkline is drawn in the CPU hue, so the strip reads as one metric plotted four times',
        RESMON,
        [
            ('            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, res.color(p));',
             '            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, ResourceType::Cpu.color(p));'),
        ],
        ["desktop"],
        [
            'a_metric_is_one_colour_wherever_it_appears',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the compact sparklines are drawn in ink rather than in metric hues',
        RESMON,
        [
            ('            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, res.color(p));',
             '            Self::render_sparkline(&mut cmds, data, sx, sy, slot_w, slot_h, p.subtext0);'),
        ],
        ["desktop"],
        [
            'a_metric_is_one_colour_wherever_it_appears',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the expanded widget's background is frozen to Mocha base",
        RESMON,
        [
            ('            color: p.base,\n            corner_radii: CornerRadii::all(6.0),',
             '            color: guitk::color::Color::from_hex(0x1E1E2E),\n            corner_radii: CornerRadii::all(6.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the expanded widget's background is the crust, so the widget reads as a hole rather than a surface",
        RESMON,
        [
            ('            color: p.base,\n            corner_radii: CornerRadii::all(6.0),',
             '            color: p.crust,\n            corner_radii: CornerRadii::all(6.0),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the expanded widget's border is frozen to Mocha surface0",
        RESMON,
        [
            ('            color: p.surface0,\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(6.0),',
             '            color: guitk::color::Color::from_hex(0x313244),\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(6.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the expanded widget's border is its own background, so the widget has no edge",
        RESMON,
        [
            ('            color: p.surface0,\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(6.0),',
             '            color: p.base,\n            line_width: 1.0,\n            corner_radii: CornerRadii::all(6.0),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the widget's title is frozen to Mocha text",
        RESMON,
        [
            ('            color: p.text,\n            font_size: 13.0,',
             '            color: guitk::color::Color::from_hex(0xCDD6F4),\n            font_size: 13.0,'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the widget's title is dimmer than the readings underneath it",
        RESMON,
        [
            ('            color: p.text,\n            font_size: 13.0,',
             '            color: p.subtext0,\n            font_size: 13.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the widget's title is accented, which is the one colour this module never draws",
        RESMON,
        [
            ('            color: p.text,\n            font_size: 13.0,',
             '            color: p.accent,\n            font_size: 13.0,'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: a panel's label and graph are accented rather than drawn in the metric's hue",
        RESMON,
        [
            ('        let color = resource.color(p);',
             '        let color = p.accent;'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'every_site_draws_the_role_it_claims',
            'each_measurement_is_pinned_to_the_role_it_names',
            'a_metric_is_one_colour_wherever_it_appears',
            'test_render_expanded_has_resource_labels',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: every panel is drawn in the CPU hue, so all four graphs claim to be the processor',
        RESMON,
        [
            ('        let color = resource.color(p);',
             '        let color = ResourceType::Cpu.color(p);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'each_measurement_is_pinned_to_the_role_it_names',
            'a_metric_is_one_colour_wherever_it_appears',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a panel's background is the widget's own, so the stack has no depth",
        RESMON,
        [
            ('            color: p.surface0,\n            corner_radii: CornerRadii::all(4.0),',
             '            color: p.base,\n            corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'test_render_expanded_has_panel_backgrounds',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: a panel's background is frozen to Mocha surface0",
        RESMON,
        [
            ('            color: p.surface0,\n            corner_radii: CornerRadii::all(4.0),',
             '            color: guitk::color::Color::from_hex(0x313244),\n            corner_radii: CornerRadii::all(4.0),'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the current reading is frozen to Mocha text',
        RESMON,
        [
            ('            color: p.text,\n            font_size: 11.0,',
             '            color: guitk::color::Color::from_hex(0xCDD6F4),\n            font_size: 11.0,'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the current reading is as dim as the peak beneath it',
        RESMON,
        [
            ('            color: p.text,\n            font_size: 11.0,',
             '            color: p.subtext0,\n            font_size: 11.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the peak reading is frozen to Mocha subtext',
        RESMON,
        [
            ('            color: p.subtext0,\n            font_size: 9.0,',
             '            color: guitk::color::Color::from_hex(0xA6ADC8),\n            font_size: 9.0,'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the peak reading is as bright as the current one, so the two cannot be told apart',
        RESMON,
        [
            ('            color: p.subtext0,\n            font_size: 9.0,',
             '            color: p.text,\n            font_size: 9.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the peak reading is accented',
        RESMON,
        [
            ('            color: p.subtext0,\n            font_size: 9.0,',
             '            color: p.accent,\n            font_size: 9.0,'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the grid is frozen to Mocha surface1',
        RESMON,
        [
            ('        let grid_color = p.surface1;',
             '        let grid_color = guitk::color::Color::from_hex(0x45475A);'),
        ],
        ["desktop"],
        [
            'every_colour_this_monitor_draws_comes_from_its_palette',
            'none_of_the_eleven_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: a gridline is the colour of the CPU graph, so furniture reads as a reading nobody took',
        RESMON,
        [
            ('        let grid_color = p.surface1;',
             '        let grid_color = p.blue;'),
        ],
        ["desktop"],
        [
            'no_gridline_can_be_mistaken_for_a_reading',
            'every_site_draws_the_role_it_claims',
            'test_render_expanded_has_grid_lines',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the grid is as bright as the readings drawn over it',
        RESMON,
        [
            ('        let grid_color = p.surface1;',
             '        let grid_color = p.text;'),
        ],
        ["desktop"],
        [
            'no_gridline_can_be_mistaken_for_a_reading',
            'every_site_draws_the_role_it_claims',
            'test_render_expanded_has_grid_lines',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the grid is accented',
        RESMON,
        [
            ('        let grid_color = p.surface1;',
             '        let grid_color = p.accent;'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'every_site_draws_the_role_it_claims',
            'test_render_expanded_has_grid_lines',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: a panel's graph is detached from its label, so the label names a hue nothing on the screen uses",
        RESMON,
        [
            ('            Self::render_sparkline(cmds, data, graph_x, graph_y, graph_w, graph_h, color);',
             '            Self::render_sparkline(cmds, data, graph_x, graph_y, graph_w, graph_h, p.subtext0);'),
        ],
        ["desktop"],
        [
            'a_metric_is_one_colour_wherever_it_appears',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: a panel's graph is accented while its label keeps the metric's hue",
        RESMON,
        [
            ('            Self::render_sparkline(cmds, data, graph_x, graph_y, graph_w, graph_h, color);',
             '            Self::render_sparkline(cmds, data, graph_x, graph_y, graph_w, graph_h, p.accent);'),
        ],
        ["desktop"],
        [
            'no_colour_in_this_module_marks_a_position',
            'a_metric_is_one_colour_wherever_it_appears',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the panel background keeps Catppuccin Mocha's own base",
        MOUSESET,
        [
            ('            height: 900.0,\n            color: p.base,',
             '            height: 900.0,\n            color: guitk::color::Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the panel title keeps Mocha text',
        MOUSESET,
        [
            ('            font_size: 20.0,\n            color: p.text,',
             '            font_size: 20.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the open section's header keeps Mocha surface0",
        MOUSESET,
        [
            ('                color: if expanded { p.surface0 } else { p.mantle },',
             '                color: if expanded { guitk::color::Color::from_hex(0x313244) } else { p.mantle },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the closed sections' headers keep Mocha mantle",
        MOUSESET,
        [
            ('                color: if expanded { p.surface0 } else { p.mantle },',
             '                color: if expanded { p.surface0 } else { guitk::color::Color::from_hex(0x181825) },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the open section's heading keeps Mocha blue, which is the very substitution the stock accent hides",
        MOUSESET,
        [
            ('                color: if expanded { p.accent } else { p.text },',
             '                color: if expanded { guitk::color::Color::from_hex(0x89B4FA) } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the closed sections' headings keep Mocha text",
        MOUSESET,
        [
            ('                color: if expanded { p.accent } else { p.text },',
             '                color: if expanded { p.accent } else { guitk::color::Color::from_hex(0xCDD6F4) },'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the unsaved-changes banner keeps Mocha surface0',
        MOUSESET,
        [
            ('                height: 36.0,\n                color: p.surface0,',
             '                height: 36.0,\n                color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the unsaved-changes warning keeps Mocha yellow',
        MOUSESET,
        [
            ('                color: p.yellow,',
             '                color: guitk::color::Color::from_hex(0xF9E2AF),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_state_is_not_a_position',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: a setting's label keeps Mocha subtext0",
        MOUSESET,
        [
            ('            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.5),',
             '            color: guitk::color::Color::from_hex(0xA6ADC8),\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.5),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: a setting's value keeps Mocha text",
        MOUSESET,
        [
            ('            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.4),',
             '            color: guitk::color::Color::from_hex(0xCDD6F4),\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: a switch's label keeps Mocha subtext0",
        MOUSESET,
        [
            ('            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.6),',
             '            color: guitk::color::Color::from_hex(0xA6ADC8),\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.6),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: a switched-on pill keeps Mocha green',
        MOUSESET,
        [
            ('        let bg = if on { p.green } else { p.surface1 };',
             '        let bg = if on { guitk::color::Color::from_hex(0xA6E3A1) } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_state_is_not_a_position',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: a switched-off pill keeps Mocha surface1',
        MOUSESET,
        [
            ('        let bg = if on { p.green } else { p.surface1 };',
             '        let bg = if on { p.green } else { guitk::color::Color::from_hex(0x45475A) };'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the switch knob keeps Mocha text',
        MOUSESET,
        [
            ('            height: 16.0,\n            color: p.text,',
             '            height: 16.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_knob_is_the_same_ink_on_both_pills',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the slider track keeps Mocha surface1',
        MOUSESET,
        [
            ('            height: track_h,\n            color: p.surface1,',
             '            height: track_h,\n            color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the slider fill keeps Mocha blue',
        MOUSESET,
        [
            ('            // Judgement 1: how much of this control is set.\n            color: p.accent,',
             '            // Judgement 1: how much of this control is set.\n            color: guitk::color::Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'only_what_is_in_force_is_accented',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the slider thumb keeps Mocha lavender',
        MOUSESET,
        [
            ('            color: emphasized(p.accent),',
             '            color: guitk::color::Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_thumb_is_derived_from_the_fill_it_ends',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the panel background is drawn a rung up, on the same surface as the header that is meant to stand proud of it',
        MOUSESET,
        [
            ('            height: 900.0,\n            color: p.base,',
             '            height: 900.0,\n            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the panel title is drawn at a label's dimness",
        MOUSESET,
        [
            ('            font_size: 20.0,\n            color: p.text,',
             '            font_size: 20.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the section headers are the wrong way round, so the four you are not editing stand proud and the one you are recedes',
        MOUSESET,
        [
            ('                color: if expanded { p.surface0 } else { p.mantle },',
             '                color: if expanded { p.mantle } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the section headings are the wrong way round, so four sections claim to be in force and the open one does not',
        MOUSESET,
        [
            ('                color: if expanded { p.accent } else { p.text },',
             '                color: if expanded { p.text } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the unsaved-changes warning is accented, so a fact about the whole panel reads as the section you are looking at',
        MOUSESET,
        [
            ('                color: p.yellow,',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_state_is_not_a_position',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
            'ui_render_with_dirty',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a switched-on pill is accented, so 'switched on' and 'where you are' become one colour",
        MOUSESET,
        [
            ('        let bg = if on { p.green } else { p.surface1 };',
             '        let bg = if on { p.accent } else { p.surface1 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_state_is_not_a_position',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: a setting's label and its value swap, so the name is brighter than the number the user came to read",
        MOUSESET,
        [
            ('            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.5),',
             '            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.5),'),
            ('            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.4),',
             '            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.4),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the slider fill is pinned to blue, so how much is set stops following the user's accent",
        MOUSESET,
        [
            ('            // Judgement 1: how much of this control is set.\n            color: p.accent,',
             '            // Judgement 1: how much of this control is set.\n            color: p.blue,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_what_is_in_force_is_accented',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the open section's heading is pinned to blue — legal, invisible at the stock theme, and wrong at every other",
        MOUSESET,
        [
            ('                color: if expanded { p.accent } else { p.text },',
             '                color: if expanded { p.blue } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the slider thumb is pinned to lavender again, the named-beside-it shape this conversion existed to remove',
        MOUSESET,
        [
            ('            color: emphasized(p.accent),',
             '            color: p.lavender,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_thumb_is_derived_from_the_fill_it_ends',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the slider thumb is the fill's own colour, so there is no handle to see",
        MOUSESET,
        [
            ('            color: emphasized(p.accent),',
             '            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_thumb_is_derived_from_the_fill_it_ends',
            'only_what_is_in_force_is_accented',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the switch knob follows its pill, so the thing that marks the position changes meaning with the state',
        MOUSESET,
        [
            ('            height: 16.0,\n            color: p.text,',
             '            height: 16.0,\n            color: bg,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_knob_is_the_same_ink_on_both_pills',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the slider track is accented along its whole length, so every control reads as fully set',
        MOUSESET,
        [
            ('            height: track_h,\n            color: p.surface1,',
             '            height: track_h,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_what_is_in_force_is_accented',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the section header stops depending on state, so nothing in the strip says which section is open',
        MOUSESET,
        [
            ('                color: if expanded { p.surface0 } else { p.mantle },',
             '                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the section heading stops depending on state, so all five sections claim to be in force at once',
        MOUSESET,
        [
            ('                color: if expanded { p.accent } else { p.text },',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_section_reads_as_open',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the toggle pill stops depending on state, so every switch looks on',
        MOUSESET,
        [
            ('        let bg = if on { p.green } else { p.surface1 };',
             '        let bg = p.green;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: a switch's label is drawn at a value's brightness, so the two halves of a row disagree about which is the reading",
        MOUSESET,
        [
            ('            color: p.subtext0,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.6),',
             '            color: p.text,\n            font_weight: FontWeightHint::Regular,\n            max_width: Some(width * 0.6),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the panel background itself is accented, so the accent stops meaning anything at all',
        MOUSESET,
        [
            ('            height: 900.0,\n            color: p.base,',
             '            height: 900.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_what_is_in_force_is_accented',
            'only_what_is_in_force_moves_when_the_accent_moves',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the slider thumb is emphasized off blue rather than off the fill, so it is derived from a colour the track never uses',
        MOUSESET,
        [
            ('            color: emphasized(p.accent),',
             '            color: emphasized(p.blue),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'the_thumb_is_derived_from_the_fill_it_ends',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the switch knob is drawn at a label's dimness",
        MOUSESET,
        [
            ('            height: 16.0,\n            color: p.text,',
             '            height: 16.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_knob_is_the_same_ink_on_both_pills',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the drop shadow keeps its own alpha instead of the palette's",
        HOTKEYS,
        [
            ('        color: p.shadow(),\n        corner_radii: radii,',
             '        color: guitk::color::Color::rgba(0, 0, 0, 100),\n        corner_radii: radii,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_panel_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the panel background keeps Mocha base with its alpha soldered on",
        HOTKEYS,
        [
            ('        color: p.panel_bg(),',
             '        color: guitk::color::Color::rgba(30, 30, 46, 240),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_panel_is_as_transparent_as_the_user_asked',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the panel border keeps Catppuccin Mocha's own surface2",
        HOTKEYS,
        [
            ('        color: p.surface2,',
             '        color: guitk::color::Color::from_hex(0x585B70),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the header keeps Catppuccin Mocha's own text",
        HOTKEYS,
        [
            ('        color: p.text,\n        font_size: HEADER_FONT_SIZE,',
             '        color: guitk::color::Color::from_hex(0xCDD6F4),\n        font_size: HEADER_FONT_SIZE,'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the header separator keeps Catppuccin Mocha's own surface1",
        HOTKEYS,
        [
            ('        color: p.surface1,\n        width: 1.0,',
             '        color: guitk::color::Color::from_hex(0x45475A),\n        width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the selection highlight keeps Catppuccin Mocha's own surface0",
        HOTKEYS,
        [
            ('                color: p.surface0,',
             '                color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: a selected row's label keeps Catppuccin Mocha's own text",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { guitk::color::Color::from_hex(0xCDD6F4) } else { p.subtext1 };'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: an unselected row's label keeps Catppuccin Mocha's own subtext1",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { p.text } else { guitk::color::Color::from_hex(0xBAC2DE) };'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the app name beside an action keeps Catppuccin Mocha's own overlay0",
        HOTKEYS,
        [
            ('                color: p.overlay0,',
             '                color: guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: a key badge keeps Catppuccin Mocha's own mantle",
        HOTKEYS,
        [
            ('                color: p.mantle,',
             '                color: guitk::color::Color::from_hex(0x181825),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: a key badge's border keeps Catppuccin Mocha's own surface1",
        HOTKEYS,
        [
            ('                color: p.surface1,\n                line_width: 1.0,',
             '                color: guitk::color::Color::from_hex(0x45475A),\n                line_width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: a key badge's lettering keeps Catppuccin Mocha's own subtext0",
        HOTKEYS,
        [
            ('                color: p.subtext0,',
             '                color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_this_panel_draws_comes_from_its_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the panel border and the header separator trade rungs (border)",
        HOTKEYS,
        [
            ('        color: p.surface2,',
             '        color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the panel border and the header separator trade rungs (separator)",
        HOTKEYS,
        [
            ('        color: p.surface1,\n        width: 1.0,',
             '        color: p.surface2,\n        width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the header is lettered as quietly as a key badge",
        HOTKEYS,
        [
            ('        color: p.text,\n        font_size: HEADER_FONT_SIZE,',
             '        color: p.subtext0,\n        font_size: HEADER_FONT_SIZE,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the selection highlight is one rung too high",
        HOTKEYS,
        [
            ('                color: p.surface0,',
             '                color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a selected row's label is lettered in subtext0",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { p.subtext0 } else { p.subtext1 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: an unselected row's label sinks to overlay0",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { p.text } else { p.overlay0 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the app name is lettered as loudly as the action it qualifies",
        HOTKEYS,
        [
            ('                color: p.overlay0,',
             '                color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: a key badge is cut one rung too deep",
        HOTKEYS,
        [
            ('                color: p.mantle,',
             '                color: p.crust,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: a key badge is the same colour as the card it sits on",
        HOTKEYS,
        [
            ('                color: p.mantle,',
             '                color: p.base,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: a key badge's border is the same colour as its fill",
        HOTKEYS,
        [
            ('                color: p.surface1,\n                line_width: 1.0,',
             '                color: p.mantle,\n                line_width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a key badge is lettered as loudly as the heading",
        HOTKEYS,
        [
            ('                color: p.subtext0,',
             '                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the panel background stops following the transparency setting",
        HOTKEYS,
        [
            ('        color: p.panel_bg(),',
             '        color: p.base,'),
        ],
        ["desktop"],
        [
            'the_panel_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the panel background is drawn from the hover rung",
        HOTKEYS,
        [
            ('        color: p.panel_bg(),',
             '        color: p.panel_hover(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_panel_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the selection highlight is repainted with the accent",
        HOTKEYS,
        [
            ('                color: p.surface0,',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: a selected row's label is repainted with the accent",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { p.accent } else { p.subtext1 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: an unselected row's label is repainted with the accent",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { p.text } else { p.accent };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: a key badge's border is repainted with the accent",
        HOTKEYS,
        [
            ('                color: p.surface1,\n                line_width: 1.0,',
             '                color: p.accent,\n                line_width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the header is repainted with the accent",
        HOTKEYS,
        [
            ('        color: p.text,\n        font_size: HEADER_FONT_SIZE,',
             '        color: p.accent,\n        font_size: HEADER_FONT_SIZE,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: a key badge's lettering is repainted with the accent",
        HOTKEYS,
        [
            ('                color: p.subtext0,',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the panel background is repainted with the accent",
        HOTKEYS,
        [
            ('        color: p.panel_bg(),',
             '        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'the_panel_is_as_transparent_as_the_user_asked',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the header separator is repainted with the accent",
        HOTKEYS,
        [
            ('        color: p.surface1,\n        width: 1.0,',
             '        color: p.accent,\n        width: 1.0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the app name beside an action is repainted with the accent",
        HOTKEYS,
        [
            ('                color: p.overlay0,',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: a key badge is repainted with the accent",
        HOTKEYS,
        [
            ('                color: p.mantle,',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_this_panel_is_accented',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the label branch collapses to the unselected ink",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = p.subtext1;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the label branch collapses to the selected ink",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = p.text;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the label branch is inverted",
        HOTKEYS,
        [
            ('        let label_color = if is_selected { p.text } else { p.subtext1 };',
             '        let label_color = if is_selected { p.subtext1 } else { p.text };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: every row draws the selection highlight",
        HOTKEYS,
        [
            ('        // Selection highlight.\n        if is_selected {',
             '        // Selection highlight.\n        if true {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the selection lands one row past the one asked for",
        HOTKEYS,
        [
            ('        let is_selected = selected_index == Some(i);',
             '        let is_selected = selected_index == Some(i.wrapping_add(1));'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the selection highlight is drawn a row below its label",
        HOTKEYS,
        [
            ('                y: row_y + 2.0,',
             '                y: row_y + 2.0 + ROW_HEIGHT,'),
        ],
        ["desktop"],
        [
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: a key badge's border and lettering trade roles",
        HOTKEYS,
        [
            ('                color: p.surface1,\n                line_width: 1.0,',
             '                color: p.subtext0,\n                line_width: 1.0,'),
            # Anchored on the line *below* as well, because the edit above has
            # just manufactured a second `color: p.subtext0,` at a lower file
            # offset. A one-line pattern would land on that copy, revert the
            # first edit, and leave the file untouched — the defect would be a
            # no-op reported as `NO TEST FAILED`. See `check()`'s NO-OP branch.
            ('                color: p.subtext0,\n                font_size: KEY_FONT_SIZE,',
             '                color: p.surface1,\n                font_size: KEY_FONT_SIZE,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the header and a key badge trade lettering",
        HOTKEYS,
        [
            ('        color: p.text,\n        font_size: HEADER_FONT_SIZE,',
             '        color: p.subtext0,\n        font_size: HEADER_FONT_SIZE,'),
            ('                color: p.subtext0,',
             '                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_key_badge_stands_off_the_panel_it_sits_on',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the drop shadow is drawn at the scrim's weight",
        HOTKEYS,
        [
            ('        color: p.shadow(),\n        corner_radii: radii,',
             '        color: p.scrim(),\n        corner_radii: radii,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_panel_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the selection highlight and a key badge trade rungs",
        HOTKEYS,
        [
            ('                color: p.surface0,',
             '                color: p.mantle,'),
            # As in P: the edit above manufactures a second `color: p.mantle,`
            # at a lower file offset, so this one has to name the badge fill's
            # own next line to reach the badge rather than undo its partner.
            ('                color: p.mantle,\n                corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),',
             '                color: p.surface0,\n                corner_radii: CornerRadii::all(KEY_BADGE_RADIUS),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_selected_row_is_said_twice',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the panel background reads the right role and freezes the alpha again",
        HOTKEYS,
        [
            ('        color: p.panel_bg(),',
             '        color: guitk::color::Color::rgba(p.base.r, p.base.g, p.base.b, 240),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_panel_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: MOCHA_BASE survives the conversion at the pill site",
        SCRCAP,
        [
            ('        color: p.panel_bg(),',
             '        color: guitk::color::Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'the_indicator_pill_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: MOCHA_MANTLE survives the conversion at the ctrlbg site",
        SCRCAP,
        [
            # Anchored on the comment *above* rather than the `corner_radii`
            # line below, so the colour line is the last line of the pattern.
            # An anchor whose colour line is not last invites a replacement
            # that rewrites the wrong line — which is how the first four
            # versions of these four defects came to emit two `color:` fields
            # and fail to compile. See known-issues.md lesson 19.
            ('        // laid on the desktop rather than a sheet of it.\n        color: p.mantle,',
             '        // laid on the desktop rather than a sheet of it.\n        color: guitk::color::Color::from_hex(0x181825),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: MOCHA_TEXT survives the conversion at the title site",
        SCRCAP,
        [
            ('        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.text,',
             '        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: MOCHA_SUBTEXT0 survives the conversion at the statelbl site",
        SCRCAP,
        [
            ('        // word is a label on it.\n        color: p.subtext0,',
             '        // word is a label on it.\n        color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: MOCHA_RED survives the conversion at the recfill site",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.red,',
             '                width: 80.0,\n                height: btn_h,\n                color: guitk::color::Color::from_hex(0xF38BA8),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: MOCHA_YELLOW survives the conversion at the pausefill site",
        SCRCAP,
        [
            ('                width: 70.0,\n                height: btn_h,\n                color: p.yellow,',
             '                width: 70.0,\n                height: btn_h,\n                color: guitk::color::Color::from_hex(0xF9E2AF),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: MOCHA_GREEN survives the conversion at the resumefill site",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.green,',
             '                width: 80.0,\n                height: btn_h,\n                color: guitk::color::Color::from_hex(0xA6E3A1),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: MOCHA_SURFACE1 survives the conversion at the stop1fill site",
        SCRCAP,
        [
            ('                // a plain surface rung rather than a fourth hue.\n                color: p.surface1,',
             '                // a plain surface rung rather than a fourth hue.\n                color: guitk::color::Color::from_hex(0x45475A),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: MOCHA_PEACH survives the conversion at the fallback site",
        SCRCAP,
        [
            ('                color: p.peach,',
             '                color: guitk::color::Color::from_hex(0xFAB387),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: MOCHA_OVERLAY0 survives the conversion at the dot_idle site",
        SCRCAP,
        [
            ('        _ => p.overlay0,',
             '        _ => guitk::color::Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_both_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'the_recording_dot_says_the_state_and_only_the_state',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the Record and Pause buttons trade fills",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.red,',
             '                width: 80.0,\n                height: btn_h,\n                color: p.yellow,'),
            ('                width: 70.0,\n                height: btn_h,\n                color: p.yellow,',
             '                width: 70.0,\n                height: btn_h,\n                color: p.red,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the Resume button and the Stop beside it trade fills",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.green,',
             '                width: 80.0,\n                height: btn_h,\n                color: p.surface1,'),
            ('                // a plain surface rung rather than a fourth hue.\n                color: p.surface1,',
             '                // a plain surface rung rather than a fourth hue.\n                color: p.green,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the panel title and its telemetry trade ink",
        SCRCAP,
        [
            ('        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.text,',
             '        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.subtext0,'),
            ('            // Telemetry, quieter than the title it sits beside.\n            color: p.subtext0,',
             '            // Telemetry, quieter than the title it sits beside.\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the indicator pill and the controls bar trade backgrounds",
        SCRCAP,
        [
            ('        color: p.panel_bg(),',
             '        color: p.mantle,'),
            ('        // laid on the desktop rather than a sheet of it.\n        color: p.mantle,',
             '        // laid on the desktop rather than a sheet of it.\n        color: p.panel_bg(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_indicator_pill_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the recording and paused dots trade colours",
        SCRCAP,
        [
            ('        RecordingState::Recording => p.red,',
             '        RecordingState::Recording => p.yellow,'),
            ('        RecordingState::Paused => p.yellow,',
             '        RecordingState::Paused => p.red,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_recording_dot_says_the_state_and_only_the_state',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the recording dot and the inactive dot trade colours",
        SCRCAP,
        [
            ('        RecordingState::Recording => p.red,',
             '        RecordingState::Recording => p.overlay0,'),
            ('        _ => p.overlay0,',
             '        _ => p.red,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_recording_dot_says_the_state_and_only_the_state',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the indicator's clock and its state word trade ink",
        SCRCAP,
        [
            ('        text: recorder.stats.elapsed_display(),\n        font_size: 13.0,\n        color: p.text,',
             '        text: recorder.stats.elapsed_display(),\n        font_size: 13.0,\n        color: p.subtext0,'),
            ('        // word is a label on it.\n        color: p.subtext0,',
             '        // word is a label on it.\n        color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the transient-state word and the panel title trade ink",
        SCRCAP,
        [
            ('                color: p.peach,',
             '                color: p.text,'),
            ('        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.text,',
             '        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.peach,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the indicator pill is painted with the accent",
        SCRCAP,
        [
            ('        color: p.panel_bg(),',
             '        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
            'the_indicator_pill_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the controls bar is painted with the accent",
        SCRCAP,
        [
            ('        // laid on the desktop rather than a sheet of it.\n        color: p.mantle,',
             '        // laid on the desktop rather than a sheet of it.\n        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the panel title is painted with the accent",
        SCRCAP,
        [
            ('        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.text,',
             '        text: "Screen Recorder".to_string(),\n        font_size: 13.0,\n        color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the Record button is painted with the accent",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.red,',
             '                width: 80.0,\n                height: btn_h,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the Pause button is painted with the accent",
        SCRCAP,
        [
            ('                width: 70.0,\n                height: btn_h,\n                color: p.yellow,',
             '                width: 70.0,\n                height: btn_h,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the Resume button is painted with the accent",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.green,',
             '                width: 80.0,\n                height: btn_h,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the Stop button is painted with the accent",
        SCRCAP,
        [
            ('                // a plain surface rung rather than a fourth hue.\n                color: p.surface1,',
             '                // a plain surface rung rather than a fourth hue.\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the transient-state word is painted with the accent",
        SCRCAP,
        [
            ('                color: p.peach,',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the recording dot is painted with the accent",
        SCRCAP,
        [
            ('        RecordingState::Recording => p.red,',
             '        RecordingState::Recording => p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
            'the_recording_dot_says_the_state_and_only_the_state',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the telemetry is painted with the accent",
        SCRCAP,
        [
            ('            // Telemetry, quieter than the title it sits beside.\n            color: p.subtext0,',
             '            // Telemetry, quieter than the title it sits beside.\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'nothing_in_the_recorder_is_accented',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the Stop button sits a rung too high",
        SCRCAP,
        [
            ('                // a plain surface rung rather than a fourth hue.\n                color: p.surface1,',
             '                // a plain surface rung rather than a fourth hue.\n                color: p.surface2,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the other Stop button sits a rung too low",
        SCRCAP,
        [
            ('                // As in the Recording arm: the same button, the same rung.\n                color: p.surface1,',
             '                // As in the Recording arm: the same button, the same rung.\n                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the controls bar drops to the lowest rung there is",
        SCRCAP,
        [
            ('        // laid on the desktop rather than a sheet of it.\n        color: p.mantle,',
             '        // laid on the desktop rather than a sheet of it.\n        color: p.crust,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the drop-rate line is brighter than the two beside it",
        SCRCAP,
        [
            ('                recorder.stats.drop_rate_pct(),\n            ),\n            font_size: 10.0,\n            color: p.subtext0,',
             '                recorder.stats.drop_rate_pct(),\n            ),\n            font_size: 10.0,\n            color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the elapsed-time line is dimmer than the two beside it",
        SCRCAP,
        [
            ('            text: format!("Time: {}", recorder.stats.elapsed_display()),\n            font_size: 10.0,\n            color: p.subtext0,',
             '            text: format!("Time: {}", recorder.stats.elapsed_display()),\n            font_size: 10.0,\n            color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the Record button's lettering is named instead of computed",
        SCRCAP,
        [
            ('                color: readable_on(p.red),',
             '                color: p.base,'),
        ],
        ["desktop"],
        [
            'a_transport_button_is_lettered_for_its_own_fill',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the Pause button's lettering is named instead of computed",
        SCRCAP,
        [
            ('                color: readable_on(p.yellow),',
             '                color: p.base,'),
        ],
        ["desktop"],
        [
            'a_transport_button_is_lettered_for_its_own_fill',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the Resume button's lettering is named instead of computed",
        SCRCAP,
        [
            ('                color: readable_on(p.green),',
             '                color: p.base,'),
        ],
        ["desktop"],
        [
            'a_transport_button_is_lettered_for_its_own_fill',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the Record button's lettering is pinned to the dark endpoint",
        SCRCAP,
        [
            ('                color: readable_on(p.red),',
             '                color: guitk::color::Color::from_hex(0x11111B),'),
        ],
        ["desktop"],
        [
            'a_transport_button_is_lettered_for_its_own_fill',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the Pause button reads its lettering off the wrong fill",
        SCRCAP,
        [
            ('                color: readable_on(p.yellow),',
             '                color: readable_on(p.surface1),'),
        ],
        ["desktop"],
        [
            'a_transport_button_is_lettered_for_its_own_fill',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the indicator pill drops the transparency setting",
        SCRCAP,
        [
            ('        color: p.panel_bg(),',
             '        color: p.base,'),
        ],
        ["desktop"],
        [
            'the_indicator_pill_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the indicator pill freezes an alpha onto the right role",
        SCRCAP,
        [
            ('        color: p.panel_bg(),',
             '        color: guitk::color::Color::rgba(p.base.r, p.base.g, p.base.b, 220),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_indicator_pill_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the inactive dot collides with the recording dot",
        SCRCAP,
        [
            ('        _ => p.overlay0,',
             '        _ => p.red,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_recording_dot_says_the_state_and_only_the_state',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the indicator's state word is as loud as its clock",
        SCRCAP,
        [
            ('        // word is a label on it.\n        color: p.subtext0,',
             '        // word is a label on it.\n        color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the Stop button's lettering is computed off its own rung",
        SCRCAP,
        [
            ('                font_size: 12.0,\n                color: p.text,',
             '                font_size: 12.0,\n                color: readable_on(p.surface1),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the indicator's clock is as quiet as its state word",
        SCRCAP,
        [
            ('        text: recorder.stats.elapsed_display(),\n        font_size: 13.0,\n        color: p.text,',
             '        text: recorder.stats.elapsed_display(),\n        font_size: 13.0,\n        color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the Record button is a hue that means nothing",
        SCRCAP,
        [
            ('                width: 80.0,\n                height: btn_h,\n                color: p.red,',
             '                width: 80.0,\n                height: btn_h,\n                color: p.mauve,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the transient-state word borrows the pause colour",
        SCRCAP,
        [
            ('                color: p.peach,',
             '                color: p.yellow,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the recording dot thins out with the panel behind it",
        SCRCAP,
        [
            ('        color: dot_color,',
             '        color: guitk::color::Color::rgba(\n            dot_color.r,\n            dot_color.g,\n            dot_color.b,\n            p.panel_alpha,\n        ),'),
        ],
        ["desktop"],
        [
            'the_indicator_pill_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the indicator draws in every state, including the three that are not recording",
        SCRCAP,
        [
            ('    if !recorder.state.is_active() && !matches!(recorder.state, RecordingState::Processing) {\n        return cmds;\n    }',
             '    if false {\n        return cmds;\n    }'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'test_indicator_idle_empty',
            'the_indicator_is_silent_unless_something_is_happening',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: MOCHA_SURFACE0 survives at the picker's border",
        SNAP,
        [
            ('            height: picker_h,\n            color: p.surface0,',
             '            height: picker_h,\n            color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: MOCHA_SURFACE0 survives at a thumbnail's background",
        SNAP,
        [
            ('                height: THUMB_SIZE,\n                color: p.surface0,',
             '                height: THUMB_SIZE,\n                color: guitk::color::Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: ZONE_FILL survives at the resting zone's fill",
        SNAP,
        [
            ('                // Judgement 1: a zone at rest is a selection at rest.\n                color: p.selection_fill(),',
             '                // Judgement 1: a zone at rest is a selection at rest.\n                color: guitk::color::Color::rgba(137, 180, 250, 50),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: ZONE_BORDER survives at the resting zone's border",
        SNAP,
        [
            ('                // drift closing, not a change of intent.\n                color: p.selection_border(),',
             '                // drift closing, not a change of intent.\n                color: guitk::color::Color::rgba(137, 180, 250, 160),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: ZONE_HIGHLIGHT survives at the hovered zone's fill",
        SNAP,
        [
            ('            // Judgement 1, one rung louder than the zones at rest.\n            color: p.highlight_fill(),',
             '            // Judgement 1, one rung louder than the zones at rest.\n            color: guitk::color::Color::rgba(137, 180, 250, 90),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_zone_under_the_cursor_out_reads_the_zones_at_rest',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: MOCHA_BLUE survives at the hovered zone's border",
        SNAP,
        [
            ('            // out-read the eight at rest that are already wearing that wash.\n            color: p.accent,',
             '            // out-read the eight at rest that are already wearing that wash.\n            color: guitk::color::Color::from_hex(0x89B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: MOCHA_LAVENDER survives at the picker's title",
        SNAP,
        [
            ('            // the accented thumbnail below it that actually means something.\n            color: p.lavender,',
             '            // the accented thumbnail below it that actually means something.\n            color: guitk::color::Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: MOCHA_LAVENDER survives at an inactive thumbnail",
        SNAP,
        [
            ('                    } else {\n                        p.overlay0',
             '                    } else {\n                        guitk::color::Color::from_hex(0xB4BEFE)'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'an_inactive_preset_is_never_the_accent_the_user_chose',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: MOCHA_TEXT survives at a resting zone's label",
        SNAP,
        [
            ('                // be dark-on-dark under the light theme.\n                color: readable_on(p.scrim()),',
             '                // be dark-on-dark under the light theme.\n                color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: Color::WHITE survives at the hovered zone's label",
        SNAP,
        [
            ('            // because both are read off the same scrim, not by coincidence.\n            color: readable_on(p.scrim()),',
             '            // because both are read off the same scrim, not by coincidence.\n            color: Color::WHITE,'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'every_site_draws_the_role_it_claims',
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: OVERLAY_SCRIM survives, tint and all",
        SNAP,
        [
            ('            // the desktop in light mode instead of pushing it back.\n            color: p.scrim(),',
             '            // the desktop in light mode instead of pushing it back.\n            color: guitk::color::Color::rgba(30, 30, 46, 140),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_scrim_is_black_in_both_modes',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: PICKER_BG survives, frozen alpha and all",
        SNAP,
        [
            ('            // Judgement 3: the transparency setting, not a frozen 230.\n            color: p.panel_bg(),',
             '            // Judgement 3: the transparency setting, not a frozen 230.\n            color: guitk::color::Color::rgba(30, 30, 46, 230),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: PICKER_HOVER survives, frozen alpha and all",
        SNAP,
        [
            ('                    // at a frozen 200.\n                    color: p.panel_hover(),',
             '                    // at a frozen 200.\n                    color: guitk::color::Color::rgba(69, 71, 90, 200),'),
        ],
        ["desktop"],
        [
            'every_colour_all_three_renderers_draw_comes_from_their_palette',
            'none_of_the_ten_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the resting zone's fill and the hovered zone's fill trade places",
        SNAP,
        [
            ('                // Judgement 1: a zone at rest is a selection at rest.\n                color: p.selection_fill(),',
             '                // Judgement 1: a zone at rest is a selection at rest.\n                color: p.highlight_fill(),'),
            ('            // Judgement 1, one rung louder than the zones at rest.\n            color: p.highlight_fill(),',
             '            // Judgement 1, one rung louder than the zones at rest.\n            color: p.selection_fill(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_zone_under_the_cursor_out_reads_the_zones_at_rest',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the resting zone's border and the hovered zone's border trade places",
        SNAP,
        [
            ('                // drift closing, not a change of intent.\n                color: p.selection_border(),',
             '                // drift closing, not a change of intent.\n                color: p.accent,'),
            ('            // out-read the eight at rest that are already wearing that wash.\n            color: p.accent,',
             '            // out-read the eight at rest that are already wearing that wash.\n            color: p.selection_border(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_zone_under_the_cursor_out_reads_the_zones_at_rest',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the picker's background and its hover rung trade places",
        SNAP,
        [
            ('            // Judgement 3: the transparency setting, not a frozen 230.\n            color: p.panel_bg(),',
             '            // Judgement 3: the transparency setting, not a frozen 230.\n            color: p.panel_hover(),'),
            ('                    // at a frozen 200.\n                    color: p.panel_hover(),',
             '                    // at a frozen 200.\n                    color: p.panel_bg(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the picker's title and a zone's label trade inks",
        SNAP,
        [
            ('            // the accented thumbnail below it that actually means something.\n            color: p.lavender,',
             '            // the accented thumbnail below it that actually means something.\n            color: readable_on(p.scrim()),'),
            ('                // be dark-on-dark under the light theme.\n                color: readable_on(p.scrim()),',
             '                // be dark-on-dark under the light theme.\n                color: p.lavender,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the scrim and the picker's shadow trade alphas",
        SNAP,
        [
            ('            // the desktop in light mode instead of pushing it back.\n            color: p.scrim(),',
             '            // the desktop in light mode instead of pushing it back.\n            color: Color::rgba(0, 0, 0, 100),'),
            ('            // picker is a small popup rather than a window.\n            color: Color::rgba(0, 0, 0, 100),',
             '            // picker is a small popup rather than a window.\n            color: p.scrim(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_scrim_is_black_in_both_modes',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the active thumbnail and the inactive ones trade colours",
        SNAP,
        [
            ('                    color: if self.active_preset == preset {\n                        p.accent',
             '                    color: if self.active_preset == preset {\n                        p.overlay0'),
            ('                    } else {\n                        p.overlay0',
             '                    } else {\n                        p.accent'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the thumbnail background and the picker's title trade colours",
        SNAP,
        [
            ('                height: THUMB_SIZE,\n                color: p.surface0,',
             '                height: THUMB_SIZE,\n                color: p.lavender,'),
            ('            // the accented thumbnail below it that actually means something.\n            color: p.lavender,',
             '            // the accented thumbnail below it that actually means something.\n            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the picker's border and its title trade colours",
        SNAP,
        [
            ('            height: picker_h,\n            color: p.surface0,',
             '            height: picker_h,\n            color: p.lavender,'),
            ('            // the accented thumbnail below it that actually means something.\n            color: p.lavender,',
             '            // the accented thumbnail below it that actually means something.\n            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: a resting zone is washed in a fixed blue rather than the accent",
        SNAP,
        [
            ('                // Judgement 1: a zone at rest is a selection at rest.\n                color: p.selection_fill(),',
             '                // Judgement 1: a zone at rest is a selection at rest.\n                color: guitk::color::Color::rgba(p.blue.r, p.blue.g, p.blue.b, 50),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a resting zone's border is a fixed blue rather than the accent",
        SNAP,
        [
            ('                // drift closing, not a change of intent.\n                color: p.selection_border(),',
             '                // drift closing, not a change of intent.\n                color: guitk::color::Color::rgba(p.blue.r, p.blue.g, p.blue.b, 150),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the hovered zone's border is a fixed blue rather than the accent",
        SNAP,
        [
            ('            // out-read the eight at rest that are already wearing that wash.\n            color: p.accent,',
             '            // out-read the eight at rest that are already wearing that wash.\n            color: p.blue,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the active thumbnail's mini-zones are a fixed blue",
        SNAP,
        [
            ('                    color: if self.active_preset == preset {\n                        p.accent',
             '                    color: if self.active_preset == preset {\n                        p.blue'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the active preset's marker is a fixed blue",
        SNAP,
        [
            ('                    height: THUMB_SIZE,\n                    color: p.accent,',
             '                    height: THUMB_SIZE,\n                    color: p.blue,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the inactive thumbnails go back to lavender, the collision judgement 5 names",
        SNAP,
        [
            ('                    } else {\n                        p.overlay0',
             '                    } else {\n                        p.lavender'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'an_inactive_preset_is_never_the_accent_the_user_chose',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the inactive thumbnails are accented too, so nothing says which is active",
        SNAP,
        [
            ('                    } else {\n                        p.overlay0',
             '                    } else {\n                        p.accent'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'an_inactive_preset_is_never_the_accent_the_user_chose',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the picker's title is accented and competes with the thumbnail that matters",
        SNAP,
        [
            ('            // the accented thumbnail below it that actually means something.\n            color: p.lavender,',
             '            // the accented thumbnail below it that actually means something.\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the picker's border is accented",
        SNAP,
        [
            ('            height: picker_h,\n            color: p.surface0,',
             '            height: picker_h,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: a resting zone's label follows the mode instead of the scrim",
        SNAP,
        [
            ('                // be dark-on-dark under the light theme.\n                color: readable_on(p.scrim()),',
             '                // be dark-on-dark under the light theme.\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the hovered zone's label follows the mode instead of the scrim",
        SNAP,
        [
            ('            // because both are read off the same scrim, not by coincidence.\n            color: readable_on(p.scrim()),',
             '            // because both are read off the same scrim, not by coincidence.\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: a resting zone's label is read off the panel base rather than the scrim",
        SNAP,
        [
            ('                // be dark-on-dark under the light theme.\n                color: readable_on(p.scrim()),',
             '                // be dark-on-dark under the light theme.\n                color: readable_on(p.base),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the hovered zone's label is read off the accent under it",
        SNAP,
        [
            ('            // because both are read off the same scrim, not by coincidence.\n            color: readable_on(p.scrim()),',
             '            // because both are read off the same scrim, not by coincidence.\n            color: readable_on(p.accent),'),
        ],
        ["desktop"],
        [
            'a_zone_label_is_lettered_for_the_scrim_and_not_for_the_mode',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the scrim is tinted with the mode's base again",
        SNAP,
        [
            ('            // the desktop in light mode instead of pushing it back.\n            color: p.scrim(),',
             '            // the desktop in light mode instead of pushing it back.\n            color: guitk::color::Color::rgba(p.base.r, p.base.g, p.base.b, 140),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_scrim_is_black_in_both_modes',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the scrim is the panel background rather than an absence of light",
        SNAP,
        [
            ('            // the desktop in light mode instead of pushing it back.\n            color: p.scrim(),',
             '            // the desktop in light mode instead of pushing it back.\n            color: p.panel_bg(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_scrim_is_black_in_both_modes',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the picker's shadow is tinted with the mode's crust",
        SNAP,
        [
            ('            // picker is a small popup rather than a window.\n            color: Color::rgba(0, 0, 0, 100),',
             '            // picker is a small popup rather than a window.\n            color: guitk::color::Color::rgba(p.crust.r, p.crust.g, p.crust.b, 100),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the picker's background drops the transparency setting",
        SNAP,
        [
            ('            // Judgement 3: the transparency setting, not a frozen 230.\n            color: p.panel_bg(),',
             '            // Judgement 3: the transparency setting, not a frozen 230.\n            color: p.base,'),
        ],
        ["desktop"],
        [
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the picker's hover rung drops the transparency setting",
        SNAP,
        [
            ('                    // at a frozen 200.\n                    color: p.panel_hover(),',
             '                    // at a frozen 200.\n                    color: p.surface1,'),
        ],
        ["desktop"],
        [
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the picker's background freezes an alpha onto the right role",
        SNAP,
        [
            ('            // Judgement 3: the transparency setting, not a frozen 230.\n            color: p.panel_bg(),',
             '            // Judgement 3: the transparency setting, not a frozen 230.\n            color: guitk::color::Color::rgba(p.base.r, p.base.g, p.base.b, 230),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the picker's hover rung freezes an alpha onto the right role",
        SNAP,
        [
            ('                    // at a frozen 200.\n                    color: p.panel_hover(),',
             '                    // at a frozen 200.\n                    color: guitk::color::Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, 200),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the picker's shadow thins out with the panel behind it",
        SNAP,
        [
            ('            // picker is a small popup rather than a window.\n            color: Color::rgba(0, 0, 0, 100),',
             '            // picker is a small popup rather than a window.\n            color: guitk::color::Color::rgba(0, 0, 0, p.panel_alpha),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the scrim thins out with the panel setting",
        SNAP,
        [
            ('            // the desktop in light mode instead of pushing it back.\n            color: p.scrim(),',
             '            // the desktop in light mode instead of pushing it back.\n            color: guitk::color::Color::rgba(0, 0, 0, p.panel_alpha),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
            'the_scrim_is_black_in_both_modes',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the picker's shadow is the window shadow, one rung too heavy",
        SNAP,
        [
            ('            // picker is a small popup rather than a window.\n            color: Color::rgba(0, 0, 0, 100),',
             '            // picker is a small popup rather than a window.\n            color: p.shadow(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_is_as_transparent_as_the_user_asked',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the hovered zone is washed no louder than the eight at rest",
        SNAP,
        [
            ('            // Judgement 1, one rung louder than the zones at rest.\n            color: p.highlight_fill(),',
             '            // Judgement 1, one rung louder than the zones at rest.\n            color: p.selection_fill(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_zone_under_the_cursor_out_reads_the_zones_at_rest',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the hovered zone's border is washed rather than solid",
        SNAP,
        [
            ('            // out-read the eight at rest that are already wearing that wash.\n            color: p.accent,',
             '            // out-read the eight at rest that are already wearing that wash.\n            color: p.selection_border(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_zone_under_the_cursor_out_reads_the_zones_at_rest',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the resting zones are washed as loudly as the hovered one",
        SNAP,
        [
            ('                // Judgement 1: a zone at rest is a selection at rest.\n                color: p.selection_fill(),',
             '                // Judgement 1: a zone at rest is a selection at rest.\n                color: p.highlight_fill(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_zone_under_the_cursor_out_reads_the_zones_at_rest',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the picker's border drops to the lowest rung there is",
        SNAP,
        [
            ('            height: picker_h,\n            color: p.surface0,',
             '            height: picker_h,\n            color: p.crust,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: a thumbnail's background drops a rung",
        SNAP,
        [
            ('                height: THUMB_SIZE,\n                color: p.surface0,',
             '                height: THUMB_SIZE,\n                color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the inactive thumbnails climb a rung and lose the thumbnail behind them",
        SNAP,
        [
            ('                    } else {\n                        p.overlay0',
             '                    } else {\n                        p.surface2'),
        ],
        ["desktop"],
        [
            'an_inactive_preset_is_never_the_accent_the_user_chose',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the picker's title is as quiet as body text",
        SNAP,
        [
            ('            // the accented thumbnail below it that actually means something.\n            color: p.lavender,',
             '            // the accented thumbnail below it that actually means something.\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the panel goes back to being Mocha base",
        DISP,
        [
            ('            width,\n            height,\n            color: p.base,',
             '            width,\n            height,\n            color: Color::from_hex(0x001E_1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the panel drops to the rung below its own",
        DISP,
        [
            ('            width,\n            height,\n            color: p.base,',
             '            width,\n            height,\n            color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the title goes back to being Mocha text",
        DISP,
        [
            ('            font_size: 18.0,\n            color: p.text,',
             '            font_size: 18.0,\n            color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the chosen tab's pill goes back to being Mocha surface1",
        DISP,
        [
            ('                    height: 28.0,\n                    color: p.surface1,',
             '                    height: 28.0,\n                    color: Color::from_hex(0x0045_475A),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the chosen tab's pill drops a rung",
        DISP,
        [
            ('                    height: 28.0,\n                    color: p.surface1,',
             '                    height: 28.0,\n                    color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the chosen tab's label goes back to being Mocha blue",
        DISP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: if is_active { Color::from_hex(0x0089_B4FA) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the unchosen tabs' labels go back to being Mocha subtext0",
        DISP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: if is_active { p.accent } else { Color::from_hex(0x00A6_ADC8) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: every tab label is accented, chosen or not",
        DISP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: no tab label is accented, not even the chosen one",
        DISP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the tab bar accents every label except the chosen one",
        DISP,
        [
            ('                color: if is_active { p.accent } else { p.subtext0 },',
             '                color: if is_active { p.subtext0 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the General tab's heading goes back to being Mocha text",
        DISP,
        [
            ('                    if d.is_primary { "Primary" } else { "Secondary" }\n                ),\n                font_size: 14.0,\n                color: p.text,',
             '                    if d.is_primary { "Primary" } else { "Secondary" }\n                ),\n                font_size: 14.0,\n                color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the Night Light heading goes back to being Mocha text",
        DISP,
        [
            ('            text: "Night Light".to_string(),\n            font_size: 14.0,\n            color: p.text,',
             '            text: "Night Light".to_string(),\n            font_size: 14.0,\n            color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the temperature label goes back to being Mocha subtext0",
        DISP,
        [
            ('            text: format!("Color Temperature: {}K", nl.temperature.0),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: format!("Color Temperature: {}K", nl.temperature.0),\n            font_size: 12.0,\n            color: Color::from_hex(0x00A6_ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the Color Calibration heading goes back to being Mocha text",
        DISP,
        [
            ('                text: "Color Calibration".to_string(),\n                font_size: 14.0,\n                color: p.text,',
             '                text: "Color Calibration".to_string(),\n                font_size: 14.0,\n                color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the Red Gamma row follows the accent instead of the channel",
        DISP,
        [
            ('"Red Gamma", d.gamma.red, p.red);',
             '"Red Gamma", d.gamma.red, p.accent);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the Red Gamma row goes back to being Mocha red",
        DISP,
        [
            ('"Red Gamma", d.gamma.red, p.red);',
             '"Red Gamma", d.gamma.red, Color::from_hex(0x00F3_8BA8));'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the Green Gamma row goes back to being Mocha green",
        DISP,
        [
            ('                d.gamma.green,\n                p.green,',
             '                d.gamma.green,\n                Color::from_hex(0x00A6_E3A1),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the Green Gamma row draws the neighbouring role, which is in both palettes",
        DISP,
        [
            ('                d.gamma.green,\n                p.green,',
             '                d.gamma.green,\n                p.teal,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the Blue Gamma row means 'chosen' again, which is the bug the theme hid",
        DISP,
        [
            ('                // channel, and the deleted constant meant both.\n                p.blue,',
             '                // channel, and the deleted constant meant both.\n                p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the Blue Gamma row goes back to being Mocha blue",
        DISP,
        [
            ('                // channel, and the deleted constant meant both.\n                p.blue,',
             '                // channel, and the deleted constant meant both.\n                Color::from_hex(0x0089_B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the gamma indicator stops being its channel and becomes the accent",
        DISP,
        [
            ('            width: 8.0,\n            height: 12.0,\n            color,',
             '            width: 8.0,\n            height: 12.0,\n            color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the gamma label stops being its channel and becomes body text",
        DISP,
        [
            ('            text: format!("{}: {:.2}", label, value),\n            font_size: 12.0,\n            color,',
             '            text: format!("{}: {:.2}", label, value),\n            font_size: 12.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_gamma_rows_are_the_channels_and_never_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the gamma track goes back to being Mocha surface0",
        DISP,
        [
            ('            width: bar_w,\n            height: 6.0,\n            color: p.surface0,',
             '            width: bar_w,\n            height: 6.0,\n            color: Color::from_hex(0x0031_3244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the reset button goes back to being Mocha surface0",
        DISP,
        [
            ('                width: 120.0,\n                height: 28.0,\n                color: p.surface0,',
             '                width: 120.0,\n                height: 28.0,\n                color: Color::from_hex(0x0031_3244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the reset button climbs a rung and stops reading as a control",
        DISP,
        [
            ('                width: 120.0,\n                height: 28.0,\n                color: p.surface0,',
             '                width: 120.0,\n                height: 28.0,\n                color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the reset button's label goes back to being Mocha text",
        DISP,
        [
            ('                text: "Reset to Defaults".to_string(),\n                font_size: 12.0,\n                color: p.text,',
             '                text: "Reset to Defaults".to_string(),\n                font_size: 12.0,\n                color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the Test Patterns heading goes back to being Mocha text",
        DISP,
        [
            ('            text: "Test Patterns".to_string(),\n            font_size: 14.0,\n            color: p.text,',
             '            text: "Test Patterns".to_string(),\n            font_size: 14.0,\n            color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the chosen pattern chip goes back to being Mocha blue",
        DISP,
        [
            ('            let bg_color = if is_active { p.accent } else { p.surface0 };',
             '            let bg_color = if is_active { Color::from_hex(0x0089_B4FA) } else { p.surface0 };'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the unchosen pattern chips go back to being Mocha surface0",
        DISP,
        [
            ('            let bg_color = if is_active { p.accent } else { p.surface0 };',
             '            let bg_color = if is_active { p.accent } else { Color::from_hex(0x0031_3244) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: every pattern chip is accented, chosen or not",
        DISP,
        [
            ('            let bg_color = if is_active { p.accent } else { p.surface0 };',
             '            let bg_color = p.accent;'),
        ],
        ["desktop"],
        [
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: no pattern chip is accented, not even the chosen one",
        DISP,
        [
            ('            let bg_color = if is_active { p.accent } else { p.surface0 };',
             '            let bg_color = p.surface0;'),
        ],
        ["desktop"],
        [
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the chip list accents every pattern except the chosen one",
        DISP,
        [
            ('            let bg_color = if is_active { p.accent } else { p.surface0 };',
             '            let bg_color = if is_active { p.surface0 } else { p.accent };'),
        ],
        ["desktop"],
        [
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
            'an_unchosen_chip_and_an_unchosen_tab_are_never_the_accent',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the chosen chip's lettering goes back to being Mocha mantle",
        DISP,
        [
            ('                color: if is_active { p.on_accent() } else { p.text },',
             '                color: if is_active { Color::from_hex(0x0018_1825) } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the chosen chip is lettered like an unchosen one",
        DISP,
        [
            ('                color: if is_active { p.on_accent() } else { p.text },',
             '                color: p.text,'),
        ],
        ["desktop"],
        [
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the chosen chip's lettering is named beside its fill rather than read off it",
        DISP,
        [
            ('                color: if is_active { p.on_accent() } else { p.text },',
             '                color: if is_active { p.crust } else { p.text },'),
        ],
        ["desktop"],
        [
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the chip lettering rule is applied to exactly the wrong chip",
        DISP,
        [
            ('                color: if is_active { p.on_accent() } else { p.text },',
             '                color: if is_active { p.text } else { p.on_accent() },'),
        ],
        ["desktop"],
        [
            'a_selected_pattern_chip_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: a setting row's label goes back to being Mocha subtext0",
        DISP,
        [
            ('            text: label.to_string(),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: label.to_string(),\n            font_size: 12.0,\n            color: Color::from_hex(0x00A6_ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: a setting row's value goes back to being Mocha text",
        DISP,
        [
            ('            text: value.to_string(),\n            font_size: 12.0,\n            color: p.text,',
             '            text: value.to_string(),\n            font_size: 12.0,\n            color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: a setting row's label and value trade roles, which no membership table can see",
        DISP,
        [
            ('            text: label.to_string(),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: label.to_string(),\n            font_size: 12.0,\n            color: p.text,'),
            ('            text: value.to_string(),\n            font_size: 12.0,\n            color: p.text,',
             '            text: value.to_string(),\n            font_size: 12.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: a slider's label goes back to being Mocha subtext0",
        DISP,
        [
            ('            text: format!("{}: {}%", label, value),\n            font_size: 12.0,\n            color: p.subtext0,',
             '            text: format!("{}: {}%", label, value),\n            font_size: 12.0,\n            color: Color::from_hex(0x00A6_ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: a slider's track goes back to being Mocha surface0",
        DISP,
        [
            ('            width: track_w,\n            height: 6.0,\n            color: p.surface0,',
             '            width: track_w,\n            height: 6.0,\n            color: Color::from_hex(0x0031_3244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: a slider's filled portion goes back to being Mocha blue",
        DISP,
        [
            ('            // How much of the setting is chosen, so: the accent.\n            color: p.accent,',
             '            // How much of the setting is chosen, so: the accent.\n            color: Color::from_hex(0x0089_B4FA),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a slider stops showing how much of it is chosen",
        DISP,
        [
            ('            // How much of the setting is chosen, so: the accent.\n            color: p.accent,',
             '            // How much of the setting is chosen, so: the accent.\n            color: p.surface1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: a slider's track and its filled portion trade roles",
        DISP,
        [
            ('            width: track_w,\n            height: 6.0,\n            color: p.surface0,',
             '            width: track_w,\n            height: 6.0,\n            color: p.accent,'),
            ('            // How much of the setting is chosen, so: the accent.\n            color: p.accent,',
             '            // How much of the setting is chosen, so: the accent.\n            color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: a slider's thumb goes back to being Mocha text",
        DISP,
        [
            ('            width: 12.0,\n            height: 12.0,\n            color: p.text,',
             '            width: 12.0,\n            height: 12.0,\n            color: Color::from_hex(0x00CD_D6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the grey ramp is tinted, so a display's colour cast is measured against a tint",
        DISP,
        [
            ('                color: Color::rgb(gray, gray, gray),',
             '                color: Color::rgb(gray, gray, gray.saturating_add(20)),'),
        ],
        ["desktop"],
        [
            'the_test_patterns_are_the_same_in_both_modes',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the SMPTE bars' magenta becomes the theme's mauve",
        DISP,
        [
            ('            Color::rgb(255, 0, 255),   // Magenta',
             '            Color::from_hex(0x00CB_A6F7), // Magenta'),
        ],
        ["desktop"],
        [
            'the_test_patterns_are_the_same_in_both_modes',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the 18% grey card becomes a theme colour",
        DISP,
        [
            ('            color: Color::rgb(128, 128, 128),',
             '            color: Color::from_hex(0x0058_5B70),'),
        ],
        ["desktop"],
        [
            'the_test_patterns_are_the_same_in_both_modes',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the checkerboard's white cells become the theme's white",
        DISP,
        [
            ('                    Color::rgb(255, 255, 255)\n                } else {',
             '                    Color::from_hex(0x00CD_D6F4)\n                } else {'),
        ],
        ["desktop"],
        [
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_test_patterns_are_the_same_in_both_modes',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the hue sweep is drawn at less than full opacity",
        DISP,
        [
            ('            let color = hue_to_rgb(hue);',
             '            let c0 = hue_to_rgb(hue);\n            let color = Color::rgba(c0.r, c0.g, c0.b, 200);'),
        ],
        ["desktop"],
        [
            'the_test_patterns_are_the_same_in_both_modes',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the night-light swatch shows the theme instead of the temperature",
        DISP,
        [
            ('            color: preview_color,',
             '            color: p.accent,'),
        ],
        ["desktop"],
        [
            'the_night_light_swatch_shows_the_temperature_not_the_theme',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the night-light swatch stops moving with the temperature",
        DISP,
        [
            ('        let (r, g, b) = self.to_rgb_multiplier();\n        Color::rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)',
             '        let (r, g, _b) = self.to_rgb_multiplier();\n        Color::rgb((r * 255.0) as u8, (g * 255.0) as u8, 200)'),
        ],
        ["desktop"],
        [
            'the_night_light_swatch_shows_the_temperature_not_the_theme',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the panel keeps its own Mocha base",
        A11Y,
        [
            ('            height,\n            color: p.base,',
             '            height,\n            color: guitk::color::Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the panel is drawn on the sidebar rung",
        A11Y,
        [
            ('            height,\n            color: p.base,',
             '            height,\n            color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the panel is drawn on the behind-the-window rung",
        A11Y,
        [
            ('            height,\n            color: p.base,',
             '            height,\n            color: p.crust,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the title keeps its own Mocha text",
        A11Y,
        [
            ('            font_size: 22.0,\n            color: p.text,',
             '            font_size: 22.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the title is drawn as a section heading",
        A11Y,
        [
            ('            font_size: 22.0,\n            color: p.text,',
             '            font_size: 22.0,\n            color: p.lavender,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the title drops to the load-bearing-secondary rung",
        A11Y,
        [
            ('            font_size: 22.0,\n            color: p.text,',
             '            font_size: 22.0,\n            color: p.subtext1,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the active-feature count keeps its own Mocha green",
        A11Y,
        [
            ('                font_size: 12.0,\n                color: p.green,',
             '                font_size: 12.0,\n                color: guitk::color::Color::from_hex(0xA6E3A1),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the active-feature count follows the accent instead of reporting state",
        A11Y,
        [
            ('                font_size: 12.0,\n                color: p.green,',
             '                font_size: 12.0,\n                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the active-feature count is drawn as body text",
        A11Y,
        [
            ('                font_size: 12.0,\n                color: p.green,',
             '                font_size: 12.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the chosen tab keeps its own Mocha blue",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: if active_tab { guitk::color::Color::from_hex(0x89B4FA) } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'green_means_on_and_the_accent_means_chosen',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the unchosen tabs keep their own Mocha surface0",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: if active_tab { p.accent } else { guitk::color::Color::from_hex(0x313244) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the tab pill's chosen and unchosen branches are swapped",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: if active_tab { p.surface0 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: every tab is accented, chosen or not",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: no tab is ever accented",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: p.surface0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the unchosen tabs sit one rung too high",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: if active_tab { p.accent } else { p.surface1 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the chosen tab's lettering keeps its own Mocha crust",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    guitk::color::Color::from_hex(0x11111B)\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the chosen tab's lettering is the crust role rather than the legible one",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    p.crust\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the chosen tab's lettering is body text",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    p.text\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the chosen tab's lettering is read off the panel instead of off the fill",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    appearance::readable_on(p.base)\n                } else {\n                    p.subtext0\n                },'),
        ],
        ["desktop"],
        [
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the unchosen tabs' lettering keeps its own Mocha subtext0",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    guitk::color::Color::from_hex(0xA6ADC8)\n                },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the unchosen tabs' lettering is as loud as the chosen one's",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    p.text\n                } else {\n                    p.text\n                },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the tab lettering's chosen and unchosen branches are swapped",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    p.subtext0\n                } else {\n                    p.on_accent()\n                },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: every tab is lettered for the accent, chosen or not",
        A11Y,
        [
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: p.on_accent(),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: a tab's fill and its lettering are traded",
        A11Y,
        [
            ('                color: if active_tab { p.accent } else { p.surface0 },',
             '                color: if active_tab { p.on_accent() } else { p.subtext0 },'),
            ('                color: if active_tab {\n                    p.on_accent()\n                } else {\n                    p.subtext0\n                },',
             '                color: if active_tab {\n                    p.accent\n                } else {\n                    p.surface0\n                },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the Sticky Keys heading keeps its own Mocha lavender",
        A11Y,
        [
            ('            text: "Sticky Keys".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Sticky Keys".into(),\n            font_size: 15.0,\n            color: guitk::color::Color::from_hex(0xB4BEFE),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_section_headings_keep_their_hue_in_both_modes',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the Sticky Keys heading drifts to the neighbouring hue",
        A11Y,
        [
            ('            text: "Sticky Keys".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Sticky Keys".into(),\n            font_size: 15.0,\n            color: p.mauve,'),
        ],
        ["desktop"],
        [
            'the_section_headings_keep_their_hue_in_both_modes',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the Filter Keys heading drifts to the neighbouring hue",
        A11Y,
        [
            ('            text: "Filter Keys".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Filter Keys".into(),\n            font_size: 15.0,\n            color: p.sapphire,'),
        ],
        ["desktop"],
        [
            'the_section_headings_keep_their_hue_in_both_modes',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the Mouse Keys heading is demoted to body text",
        A11Y,
        [
            ('            text: "Mouse Keys".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Mouse Keys".into(),\n            font_size: 15.0,\n            color: p.text,'),
        ],
        ["desktop"],
        [
            'the_section_headings_keep_their_hue_in_both_modes',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the Captions heading drifts to the neighbouring hue",
        A11Y,
        [
            ('            text: "Captions".into(),\n            font_size: 15.0,\n            color: p.lavender,',
             '            text: "Captions".into(),\n            font_size: 15.0,\n            color: p.teal,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_section_headings_keep_their_hue_in_both_modes',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the Captions heading stops being 15pt and so stops being a heading",
        A11Y,
        [
            ('            text: "Captions".into(),\n            font_size: 15.0,',
             '            text: "Captions".into(),\n            font_size: 14.0,'),
        ],
        ["desktop"],
        [
            'the_section_headings_keep_their_hue_in_both_modes',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: a toggle's label keeps its own Mocha text",
        A11Y,
        [
            ('            font_size: 14.0,\n            color: p.text,',
             '            font_size: 14.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: a toggle's label drops to the secondary rung",
        A11Y,
        [
            ('            font_size: 14.0,\n            color: p.text,',
             '            font_size: 14.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the on switch keeps its own Mocha green",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: if enabled { guitk::color::Color::from_hex(0xA6E3A1) } else { p.surface2 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the off switch keeps its own Mocha surface2",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: if enabled { p.green } else { guitk::color::Color::from_hex(0x585B70) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the on switch follows the accent, so on means chosen",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: if enabled { p.accent } else { p.surface2 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the switch's on and off branches are swapped",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: if enabled { p.surface2 } else { p.green },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: every switch reads as on",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: p.green,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: every switch reads as off",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: p.surface2,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the off switch sits a rung too low",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: if enabled { p.green } else { p.surface0 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the switch knob keeps its own Mocha text",
        A11Y,
        [
            ('            height: 18.0,\n            color: p.text,',
             '            height: 18.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the switch knob drops to the secondary rung",
        A11Y,
        [
            ('            height: 18.0,\n            color: p.text,',
             '            height: 18.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the switch knob turns green, so every row reads as on",
        A11Y,
        [
            ('            height: 18.0,\n            color: p.text,',
             '            height: 18.0,\n            color: p.green,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a switch's pill and its knob are traded",
        A11Y,
        [
            ('            color: if enabled { p.green } else { p.surface2 },',
             '            color: p.text,'),
            ('            height: 18.0,\n            color: p.text,',
             '            height: 18.0,\n            color: if enabled { p.green } else { p.surface2 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: a row's label keeps its own Mocha subtext0",
        A11Y,
        [
            ('            font_size: 13.0,\n            color: p.subtext0,',
             '            font_size: 13.0,\n            color: guitk::color::Color::from_hex(0xA6ADC8),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: a row's value keeps its own Mocha text",
        A11Y,
        [
            ('            font_size: 13.0,\n            color: p.text,',
             '            font_size: 13.0,\n            color: guitk::color::Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: a row's label and its value are traded",
        A11Y,
        [
            ('            text: label.into(),\n            font_size: 13.0,\n            color: p.subtext0,',
             '            text: label.into(),\n            font_size: 13.0,\n            color: p.text,'),
            ('            text: value.into(),\n            font_size: 13.0,\n            color: p.text,',
             '            text: value.into(),\n            font_size: 13.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: a row's label drops below the legible rung",
        A11Y,
        [
            ('            font_size: 13.0,\n            color: p.subtext0,',
             '            font_size: 13.0,\n            color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: a row's value is as quiet as its label",
        A11Y,
        [
            ('            font_size: 13.0,\n            color: p.text,',
             '            font_size: 13.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the Audio tab is handed a palette of its own instead of the caller's",
        A11Y,
        [
            ('            A11yTab::Audio => self.render_audio(&mut cmds, p, 24.0, cy, cw),',
             '            A11yTab::Audio => {\n                self.render_audio(&mut cmds, &Palette::for_mode(false), 24.0, cy, cw)\n            }'),
        ],
        ["desktop"],
        [
            'every_colour_the_panel_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'the_section_headings_keep_their_hue_in_both_modes',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the panel rebuilds the palette from the mode and loses the accent",
        A11Y,
        [
            ('    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {\n        let mut cmds = Vec::new();',
             '    pub fn render(&self, p: &Palette, width: f32, height: f32) -> Vec<RenderCommand> {\n        let p = &Palette::for_mode(p.light);\n        let mut cmds = Vec::new();'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_selected_tab_is_lettered_for_its_own_fill',
            'exactly_one_tab_is_accented_and_it_is_the_chosen_one',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the active-feature line is drawn even when no feature is active",
        A11Y,
        [
            ('        let active = self.settings.active_feature_count();\n        if active > 0 {',
             '        let active = self.settings.active_feature_count();\n        if active < 100 {'),
        ],
        ["desktop"],
        [
            'the_active_feature_line_is_green_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the tab bar draws its tabs in a different order than A11yTab::ALL",
        A11Y,
        [
            ('    pub const ALL: [Self; 5] = [\n        Self::Visual,\n        Self::Input,',
             '    pub const ALL: [Self; 5] = [\n        Self::Input,\n        Self::Visual,'),
        ],
        ["desktop"],
        # Not the two tab tests, though both read the tab bar: each walks
        # `A11yTab::ALL.iter().enumerate()` and indexes the render by the same
        # `i`, so permuting `ALL` permutes the expectation identically and the
        # defect is invisible to them by construction. Only a test whose
        # expected order was written out independently of the renderer's list
        # can see a permutation of that list -- here the ordered-vector pin and
        # the green/accent test, which reads `tabs(&cmds)[0]` outright.
        [
            'every_site_draws_the_role_it_claims',
            'green_means_on_and_the_accent_means_chosen',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the tray icon keeps its own Mocha base',
        FOCUS,
        [
            ('            color: readable_on(color),',
             '            color: Color::from_hex(0x1E1E2E),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_tray_icon_is_inked_for_its_own_pill',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the title keeps its own Mocha text',
        FOCUS,
        [
            ('            text: "Focus Assist".to_string(),\n            font_size: 18.0,\n            color: p.text,',
             '            text: "Focus Assist".to_string(),\n            font_size: 18.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the engaged Current line keeps its own Mocha blue',
        FOCUS,
        [
            ('            color: if mode == FocusMode::Off {\n                p.subtext0\n            } else {\n                p.blue\n            },',
             '            color: if mode == FocusMode::Off {\n                p.subtext0\n            } else {\n                Color::from_hex(0x89B4FA)\n            },'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_current_line_is_quiet_when_off_and_never_the_accent',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the quiet Current line keeps its own Mocha subtext0',
        FOCUS,
        [
            ('            color: if mode == FocusMode::Off {\n                p.subtext0\n            } else {\n                p.blue\n            },',
             '            color: if mode == FocusMode::Off {\n                Color::from_hex(0xA6ADC8)\n            } else {\n                p.blue\n            },'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_current_line_is_quiet_when_off_and_never_the_accent',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the suppressed-count line keeps its own Mocha overlay0',
        FOCUS,
        [
            ('                text: format!("{} notifications suppressed", self.suppressed_count),\n                font_size: 12.0,\n                color: p.overlay0,',
             '                text: format!("{} notifications suppressed", self.suppressed_count),\n                font_size: 12.0,\n                color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the chosen row's background keeps its own Mocha surface0",
        FOCUS,
        [
            ('            let bg = if selected { p.surface0 } else { p.mantle };',
             '            let bg = if selected { Color::from_hex(0x313244) } else { p.mantle };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the unchosen rows' background keeps its own Mocha mantle",
        FOCUS,
        [
            ('            let bg = if selected { p.surface0 } else { p.mantle };',
             '            let bg = if selected { p.surface0 } else { Color::from_hex(0x181825) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the unchosen rows' icon keeps its own Mocha subtext0",
        FOCUS,
        [
            ('                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.accent } else { p.subtext0 },',
             '                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.accent } else { Color::from_hex(0xA6ADC8) },'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the chosen row's label keeps its own Mocha text",
        FOCUS,
        [
            ('                font_size: 13.0,\n                color: if selected { p.text } else { p.subtext0 },',
             '                font_size: 13.0,\n                color: if selected { Color::from_hex(0xCDD6F4) } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: a row's description keeps its own Mocha overlay0",
        FOCUS,
        [
            ('                text: m.description().to_string(),\n                font_size: 10.0,\n                color: p.overlay0,',
             '                text: m.description().to_string(),\n                font_size: 10.0,\n                color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the Automatic Rules heading keeps its own Mocha text',
        FOCUS,
        [
            ('            text: "Automatic Rules".to_string(),\n            font_size: 14.0,\n            color: p.text,',
             '            text: "Automatic Rules".to_string(),\n            font_size: 14.0,\n            color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the empty-state line keeps its own Mocha overlay0',
        FOCUS,
        [
            ('                text: "No automatic rules configured".to_string(),\n                font_size: 12.0,\n                color: p.overlay0,',
             '                text: "No automatic rules configured".to_string(),\n                font_size: 12.0,\n                color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: a rule's row keeps its own Mocha surface0",
        FOCUS,
        [
            ('                    height: 28.0,\n                    color: p.surface0,',
             '                    height: 28.0,\n                    color: Color::from_hex(0x313244),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: a rule's label keeps its own Mocha text",
        FOCUS,
        [
            ('                    text: rule.label(),\n                    font_size: 12.0,\n                    color: p.text,',
             '                    text: rule.label(),\n                    font_size: 12.0,\n                    color: Color::from_hex(0xCDD6F4),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: a rule's mode keeps its own Mocha overlay0",
        FOCUS,
        [
            ('                    text: rule.mode().label().to_string(),\n                    font_size: 10.0,\n                    color: p.overlay0,',
             '                    text: rule.mode().label().to_string(),\n                    font_size: 10.0,\n                    color: Color::from_hex(0x6C7086),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the mild-silence hue keeps its own Mocha blue',
        FOCUS,
        [
            ('            Self::PriorityOnly => Some(p.blue),',
             '            Self::PriorityOnly => Some(Color::from_hex(0x89B4FA)),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the alarms-only hue keeps its own Mocha yellow',
        FOCUS,
        [
            ('            Self::AlarmsOnly => Some(p.yellow),',
             '            Self::AlarmsOnly => Some(Color::from_hex(0xF9E2AF)),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the total-silence hue keeps its own Mocha red',
        FOCUS,
        [
            ('            Self::TotalSilence => Some(p.red),',
             '            Self::TotalSilence => Some(Color::from_hex(0xF38BA8)),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the tray icon is body text rather than the legible answer',
        FOCUS,
        [
            ('            color: readable_on(color),',
             '            color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_tray_icon_is_inked_for_its_own_pill',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the tray icon is the panel colour, which is right in Mocha by luck',
        FOCUS,
        [
            ('            color: readable_on(color),',
             '            color: p.base,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_tray_icon_is_inked_for_its_own_pill',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the tray icon is read off the accent instead of off the pill',
        FOCUS,
        [
            ('            color: readable_on(color),',
             '            color: readable_on(p.accent),'),
        ],
        ["desktop"],
        # Not the mode-change sweep, though the ink is now a constant function
        # of the accent: that test wears Mauve, whose Mocha and Latte values
        # sit on opposite sides of readable_on's step, so the ink still differs
        # between the two renders -- for the wrong reason, which is precisely
        # what a "did anything change?" test cannot tell apart from the right
        # one.
        [
            'every_site_draws_the_role_it_claims',
            'the_tray_icon_is_inked_for_its_own_pill',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the tray icon is read off the panel instead of off the pill',
        FOCUS,
        [
            ('            color: readable_on(color),',
             '            color: readable_on(p.base),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_tray_icon_is_inked_for_its_own_pill',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the title drops to the quiet rung',
        FOCUS,
        [
            ('            text: "Focus Assist".to_string(),\n            font_size: 18.0,\n            color: p.text,',
             '            text: "Focus Assist".to_string(),\n            font_size: 18.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the engaged Current line follows the accent',
        FOCUS,
        [
            ('            color: if mode == FocusMode::Off {\n                p.subtext0\n            } else {\n                p.blue\n            },',
             '            color: if mode == FocusMode::Off {\n                p.subtext0\n            } else {\n                p.accent\n            },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_current_line_is_quiet_when_off_and_never_the_accent',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the chosen row's icon is blue again, as it was before the split",
        FOCUS,
        [
            ('                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.accent } else { p.subtext0 },',
             '                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.blue } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the chosen row's icon is body text and marks nothing",
        FOCUS,
        [
            ('                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.accent } else { p.subtext0 },',
             '                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.text } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the unchosen rows' label is as loud as the chosen one's",
        FOCUS,
        [
            ('                font_size: 13.0,\n                color: if selected { p.text } else { p.subtext0 },',
             '                font_size: 13.0,\n                color: if selected { p.text } else { p.text },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the chosen row's label is as quiet as the unchosen ones'",
        FOCUS,
        [
            ('                font_size: 13.0,\n                color: if selected { p.text } else { p.subtext0 },',
             '                font_size: 13.0,\n                color: if selected { p.subtext0 } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: a row's description climbs above its rung",
        FOCUS,
        [
            ('                text: m.description().to_string(),\n                font_size: 10.0,\n                color: p.overlay0,',
             '                text: m.description().to_string(),\n                font_size: 10.0,\n                color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the suppressed-count line is as loud as the title',
        FOCUS,
        [
            ('                text: format!("{} notifications suppressed", self.suppressed_count),\n                font_size: 12.0,\n                color: p.overlay0,',
             '                text: format!("{} notifications suppressed", self.suppressed_count),\n                font_size: 12.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_suppressed_line_is_overlay_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the Automatic Rules heading stops being a heading',
        FOCUS,
        [
            ('            text: "Automatic Rules".to_string(),\n            font_size: 14.0,\n            color: p.text,',
             '            text: "Automatic Rules".to_string(),\n            font_size: 14.0,\n            color: p.subtext0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: a rule's label sinks to the faintest rung",
        FOCUS,
        [
            ('                    text: rule.label(),\n                    font_size: 12.0,\n                    color: p.text,',
             '                    text: rule.label(),\n                    font_size: 12.0,\n                    color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: a rule's mode is as loud as its label",
        FOCUS,
        [
            ('                    text: rule.mode().label().to_string(),\n                    font_size: 10.0,\n                    color: p.overlay0,',
             '                    text: rule.mode().label().to_string(),\n                    font_size: 10.0,\n                    color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: a rule's row sinks to the page behind it",
        FOCUS,
        [
            ('                    height: 28.0,\n                    color: p.surface0,',
             '                    height: 28.0,\n                    color: p.mantle,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the unchosen rows are lifted to the chosen one's rung",
        FOCUS,
        [
            ('            let bg = if selected { p.surface0 } else { p.mantle };',
             '            let bg = if selected { p.surface0 } else { p.surface0 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the chosen row sinks to the unchosen ones' rung",
        FOCUS,
        [
            ('            let bg = if selected { p.surface0 } else { p.mantle };',
             '            let bg = if selected { p.mantle } else { p.mantle };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the empty-state line is as loud as the heading above it',
        FOCUS,
        [
            ('                text: "No automatic rules configured".to_string(),\n                font_size: 12.0,\n                color: p.overlay0,',
             '                text: "No automatic rules configured".to_string(),\n                font_size: 12.0,\n                color: p.text,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the mild-silence hue drifts to a neighbouring colour',
        FOCUS,
        [
            ('            Self::PriorityOnly => Some(p.blue),',
             '            Self::PriorityOnly => Some(p.lavender),'),
        ],
        ["desktop"],
        [
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: all three mode hues follow the accent, so the scale says nothing',
        FOCUS,
        [
            ('            Self::PriorityOnly => Some(p.blue),',
             '            Self::PriorityOnly => Some(p.accent),'),
            ('            Self::AlarmsOnly => Some(p.yellow),',
             '            Self::AlarmsOnly => Some(p.accent),'),
            ('            Self::TotalSilence => Some(p.red),',
             '            Self::TotalSilence => Some(p.accent),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'settings_render_not_empty',
            'settings_render_with_rules',
            'the_current_line_is_quiet_when_off_and_never_the_accent',
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
            'the_picker_marks_the_chosen_row_with_the_accent',
            'the_suppressed_line_is_overlay_and_only_drawn_when_there_is_one',
            'the_tray_icon_is_inked_for_its_own_pill',
            'tray_indicator_hidden_when_off',
            'tray_indicator_shown_when_active',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the alarms-only and total-silence hues are swapped',
        FOCUS,
        [
            ('            Self::AlarmsOnly => Some(p.yellow),',
             '            Self::AlarmsOnly => Some(p.red),'),
            ('            Self::TotalSilence => Some(p.red),',
             '            Self::TotalSilence => Some(p.yellow),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: two modes share a rung of the severity scale',
        FOCUS,
        [
            ('            Self::AlarmsOnly => Some(p.yellow),',
             '            Self::AlarmsOnly => Some(p.blue),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: Off is given a hue, so the tray shows an indicator for no focus at all',
        FOCUS,
        [
            ('            Self::Off => None,',
             '            Self::Off => Some(p.overlay0),'),
        ],
        ["desktop"],
        [
            'the_mode_hues_are_a_severity_code_and_never_the_accent',
            'tray_indicator_hidden_when_off',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a row's icon and label colours are traded",
        FOCUS,
        [
            ('                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.accent } else { p.subtext0 },',
             '                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.text } else { p.subtext0 },'),
            ('                font_size: 13.0,\n                color: if selected { p.text } else { p.subtext0 },',
             '                font_size: 13.0,\n                color: if selected { p.accent } else { p.subtext0 },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the Current line's engaged and quiet branches are swapped",
        FOCUS,
        [
            ('            color: if mode == FocusMode::Off {\n                p.subtext0\n            } else {\n                p.blue\n            },',
             '            color: if mode == FocusMode::Off {\n                p.blue\n            } else {\n                p.subtext0\n            },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_current_line_is_quiet_when_off_and_never_the_accent',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the row background's chosen and unchosen branches are swapped",
        FOCUS,
        [
            ('            let bg = if selected { p.surface0 } else { p.mantle };',
             '            let bg = if selected { p.mantle } else { p.surface0 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the row icon's chosen and unchosen branches are swapped",
        FOCUS,
        [
            ('                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.accent } else { p.subtext0 },',
             '                font_size: 16.0,\n                // Here `BLUE` meant "chosen", not "this much silence" — the\n                // picker\'s only accent site.\n                color: if selected { p.subtext0 } else { p.accent },'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the picker offers its modes in a different order than FocusMode::ALL',
        FOCUS,
        [
            ('    pub const ALL: [Self; 4] = [\n        Self::Off,\n        Self::PriorityOnly,',
             '    pub const ALL: [Self; 4] = [\n        Self::PriorityOnly,\n        Self::Off,'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the settings page rebuilds the palette from the mode and loses the accent',
        FOCUS,
        [
            ('    pub fn render_settings(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {\n        let mut commands = Vec::new();',
             '    pub fn render_settings(&self, p: &Palette, x: f32, y: f32, width: f32) -> Vec<RenderCommand> {\n        let p = &Palette::for_mode(p.light);\n        let mut commands = Vec::new();'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the tray is handed a palette of its own instead of the caller's",
        FOCUS,
        [
            ('    pub fn render_tray_indicator(&self, p: &Palette, x: f32, y: f32) -> Vec<RenderCommand> {\n        let mut commands = Vec::new();',
             '    pub fn render_tray_indicator(&self, p: &Palette, x: f32, y: f32) -> Vec<RenderCommand> {\n        let p = &Palette::for_mode(false);\n        let mut commands = Vec::new();'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'every_site_changes_when_the_mode_does',
            'every_site_draws_the_role_it_claims',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_tray_icon_is_inked_for_its_own_pill',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: every row reads as the chosen one',
        FOCUS,
        [
            ('            let selected = self.manual_mode == *m;',
             '            let selected = self.manual_mode == *m || true;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: no row ever reads as chosen',
        FOCUS,
        [
            ('            let selected = self.manual_mode == *m;',
             '            let selected = self.manual_mode == *m && false;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_picker_marks_the_chosen_row_with_the_accent',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the suppressed-count line is drawn when nothing was suppressed',
        FOCUS,
        [
            ('        if self.is_active() && self.suppressed_count > 0 {',
             '        if self.is_active() || self.suppressed_count == 0 {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_suppressed_line_is_overlay_and_only_drawn_when_there_is_one',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the popup background is still Mocha BASE',
        PEEK,
        [
            ('        let bg = p.base;',
             '        let bg = Color::from_hex(0x1E1E2E);'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the popup border is still Mocha SURFACE2',
        PEEK,
        [
            ('        let border = p.surface2;',
             '        let border = Color::from_hex(0x585B70);'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the hover wash is still Mocha SURFACE0',
        PEEK,
        [
            ('            let wash = p.surface0;',
             '            let wash = Color::from_hex(0x313244);'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the minimized placeholder is still Mocha SURFACE0',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = if snap.is_minimized {\n            Color::from_hex(0x313244)\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the unsampled placeholder is still Mocha SURFACE1',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(Color::from_hex(0x45475A))\n        };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the focus ring is still the Mocha BLUE it was hard-coded to',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            Color::from_hex(0x89B4FA)\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the hover ring is still Mocha OVERLAY0',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            Color::from_hex(0x6C7086)\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the resting ring is still Mocha SURFACE2',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            Color::from_hex(0x585B70)\n        };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the Minimized legend is still Mocha SUBTEXT0',
        PEEK,
        [
            ('                color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, a),',
             '                color: Color::rgba(0xA6, 0xAD, 0xC8, a),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the window title is still Mocha TEXT',
        PEEK,
        [
            ('            color: Color::rgba(p.text.r, p.text.g, p.text.b, a),',
             '            color: Color::rgba(0xCD, 0xD6, 0xF4, a),'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the hovered close button is still Mocha RED',
        PEEK,
        [
            ('            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface2\n            };',
             '            let close_bg = if self.close_hovered {\n                Color::from_hex(0xF38BA8)\n            } else {\n                p.surface2\n            };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the resting close button is still Mocha SURFACE2',
        PEEK,
        [
            ('            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface2\n            };',
             '            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                Color::from_hex(0x585B70)\n            };'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the close X is still inked with Mocha TEXT, which is the original bug',
        PEEK,
        [
            ('            let ink = readable_on(close_bg);',
             '            let ink = Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_module_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_manager_renders_with_the_palette_it_was_given',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the popup background reads mantle instead of base',
        PEEK,
        [
            ('        let bg = p.base;',
             '        let bg = p.mantle;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the popup border reads overlay0 instead of surface2',
        PEEK,
        [
            ('        let border = p.surface2;',
             '        let border = p.overlay0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the hover wash reads surface1, the same value the thumbnail fill uses',
        PEEK,
        [
            ('            let wash = p.surface0;',
             '            let wash = p.surface1;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a minimized window gets the unsampled placeholder, so the two read alike',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = if snap.is_minimized {\n            p.surface1\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_minimized_window_ignores_whatever_was_sampled_from_it',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: an unsampled window gets the minimized placeholder, so the two read alike',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface0)\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the focus ring reads blue, so the accent was never actually wired up',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.blue\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the hover ring reads surface2, so hovering a thumbnail shows nothing',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.surface2\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the resting ring reads overlay0, so every thumbnail looks hovered',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.overlay0\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the Minimized legend reads text, so the aside is as loud as the title',
        PEEK,
        [
            ('                color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, a),',
             '                color: Color::rgba(p.text.r, p.text.g, p.text.b, a),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the window title reads subtext0, so the title is quieter than the aside',
        PEEK,
        [
            ('            color: Color::rgba(p.text.r, p.text.g, p.text.b, a),',
             '            color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, a),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the hovered close button reads peach, which is a warning and not a danger',
        PEEK,
        [
            ('            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface2\n            };',
             '            let close_bg = if self.close_hovered {\n                p.peach\n            } else {\n                p.surface2\n            };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the resting close button reads surface1 instead of surface2',
        PEEK,
        [
            ('            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface2\n            };',
             '            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface1\n            };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the close X reads text instead of being inked for its own button',
        PEEK,
        [
            ('            let ink = readable_on(close_bg);',
             '            let ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: a minimized window still draws the colour that was sampled before it went',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = snap.dominant_color.unwrap_or(p.surface1);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_minimized_window_ignores_whatever_was_sampled_from_it',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the sampled colour is thrown away, so every window shows the placeholder',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = p.surface1;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
            # Collapsing the whole conditional takes the minimized arm with it,
            # so the placeholder this test pins becomes SURFACE1 as well.
            'a_minimized_window_ignores_whatever_was_sampled_from_it',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the minimized and unsampled placeholders are swapped',
        PEEK,
        [
            ('        let content_color = if snap.is_minimized {\n            p.surface0\n        } else {\n            snap.dominant_color.unwrap_or(p.surface1)\n        };',
             '        let content_color = if snap.is_minimized {\n            snap.dominant_color.unwrap_or(p.surface1)\n        } else {\n            p.surface0\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'a_minimized_window_ignores_whatever_was_sampled_from_it',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: hover outranks focus, so pointing at the focused window unfocuses it',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if is_hovered {\n            p.overlay0\n        } else if snap.is_focused {\n            p.accent\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the focus test is inverted, so every window but the focused one is ringed',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if !snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the hover wash is painted under every thumbnail, hovered or not',
        PEEK,
        [
            ('        // Hover highlight background\n        if is_hovered {',
             '        // Hover highlight background\n        if true {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the hover wash is never painted, so hovering a thumbnail is invisible',
        PEEK,
        [
            ('        // Hover highlight background\n        if is_hovered {',
             '        // Hover highlight background\n        if false {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: every thumbnail gets a close button, not just the one under the pointer',
        PEEK,
        [
            ('        if is_hovered && self.config.show_close_buttons {',
             '        if self.config.show_close_buttons {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: no thumbnail gets a close button',
        PEEK,
        [
            ('        if is_hovered && self.config.show_close_buttons {',
             '        if is_hovered && false {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
            'test_popup_render_with_hovered_thumbnail',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the Minimized legend is drawn on every window except the minimized one',
        PEEK,
        [
            ('        // Minimized indicator\n        if snap.is_minimized {',
             '        // Minimized indicator\n        if !snap.is_minimized {'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'an_unsampled_window_follows_the_theme_and_a_sampled_one_does_not',
            'test_popup_render_minimized_window',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: hovering one thumbnail marks them all as hovered',
        PEEK,
        [
            ('            let is_hovered = self.hovered_slot == Some(i);',
             '            let is_hovered = self.hovered_slot.is_some();'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the manager renders with a palette of its own instead of the caller\'s',
        PEEK,
        [
            ('        self.popup.render(p)',
             '        self.popup.render(&Palette::for_mode(false))'),
        ],
        ["desktop"],
        [
            'the_manager_renders_with_the_palette_it_was_given',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the close X is inked for the popup background rather than for the button',
        PEEK,
        [
            ('            let ink = readable_on(close_bg);',
             '            let ink = readable_on(p.base);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the close X is hard-coded to the pale readable_on endpoint',
        PEEK,
        [
            ('            let ink = readable_on(close_bg);',
             '            let ink = Color::from_hex(0xEFF1F5);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the close X is hard-coded to the near-black readable_on endpoint',
        PEEK,
        [
            ('            let ink = readable_on(close_bg);',
             '            let ink = Color::from_hex(0x11111B);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the title is drawn in the window\'s own colour instead of the text role',
        PEEK,
        [
            ('            color: Color::rgba(p.text.r, p.text.g, p.text.b, a),',
             '            color: Color::rgba(content_color.r, content_color.g, content_color.b, a),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the popup\'s background and its border are swapped',
        PEEK,
        [
            ('        let bg = p.base;',
             '        let bg = p.surface2;'),
            ('        let border = p.surface2;',
             '        let border = p.base;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the focus ring wears the accent\'s ink rather than the accent',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.on_accent()\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
            'a_focused_thumbnail_stays_accented_while_the_pointer_is_over_it',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the hover ring and the resting ring are swapped',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.surface2\n        } else {\n            p.overlay0\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the Minimized legend wears the accent, so an aside claims to be a choice',
        PEEK,
        [
            ('                color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, a),',
             '                color: Color::rgba(p.accent.r, p.accent.g, p.accent.b, a),'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the hover wash wears the accent, so hovering looks like selecting',
        PEEK,
        [
            ('            let wash = p.surface0;',
             '            let wash = p.accent;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: readable_on is applied twice, which inverts the ink it just chose',
        PEEK,
        [
            ('            let ink = readable_on(close_bg);',
             '            let ink = readable_on(readable_on(close_bg));'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the close X is drawn in the button\'s own colour, so it is invisible',
        PEEK,
        [
            ('            let line_color = Color::rgba(ink.r, ink.g, ink.b, a);',
             '            let line_color = Color::rgba(close_bg.r, close_bg.g, close_bg.b, a);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the close button never notices the pointer is on it',
        PEEK,
        [
            ('            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface2\n            };',
             '            let close_bg = if false {\n                p.red\n            } else {\n                p.surface2\n            };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the close button always looks as though the pointer is on it',
        PEEK,
        [
            ('            let close_bg = if self.close_hovered {\n                p.red\n            } else {\n                p.surface2\n            };',
             '            let close_bg = if true {\n                p.red\n            } else {\n                p.surface2\n            };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_close_button_x_is_legible_on_the_button_it_marks',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the resting ring wears the accent, so every window claims to be focused',
        PEEK,
        [
            ('        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.surface2\n        };',
             '        let border_color = if snap.is_focused {\n            p.accent\n        } else if is_hovered {\n            p.overlay0\n        } else {\n            p.accent\n        };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'only_the_focused_thumbnail_wears_the_accent',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the dialog background is still Mocha BASE',
        ABOUT,
        [
            ('        let bg = p.base;',
             '        let bg = guitk::color::Color::from_hex(0x1E1E2E);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the dialog border is still Mocha SURFACE1',
        ABOUT,
        [
            ('        let border = p.surface1;',
             '        let border = guitk::color::Color::from_hex(0x45475A);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the title is still lettered in Mocha TEXT',
        ABOUT,
        [
            ('        let title_ink = p.text;',
             '        let title_ink = guitk::color::Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the chosen tab's fill is still Mocha SURFACE1",
        ABOUT,
        [
            ('        let tab_fill = p.surface1;',
             '        let tab_fill = guitk::color::Color::from_hex(0x45475A);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the logo tile is still Mocha BLUE',
        ABOUT,
        [
            ('        let logo = p.blue;',
             '        let logo = guitk::color::Color::from_hex(0x89B4FA);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the version line is still Mocha TEXT',
        ABOUT,
        [
            ('        let version_ink = p.text;',
             '        let version_ink = guitk::color::Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the build date is still Mocha SUBTEXT0',
        ABOUT,
        [
            ('        let built_ink = p.subtext0;',
             '        let built_ink = guitk::color::Color::from_hex(0xA6ADC8);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the tagline is still Mocha OVERLAY0',
        ABOUT,
        [
            ('        let tagline_ink = p.overlay0;',
             '        let tagline_ink = guitk::color::Color::from_hex(0x6C7086);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the Hardware heading is still Mocha TEXT',
        ABOUT,
        [
            ('        let hardware_heading = p.text;',
             '        let hardware_heading = guitk::color::Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the Software heading is still Mocha TEXT',
        ABOUT,
        [
            ('        let software_heading = p.text;',
             '        let software_heading = guitk::color::Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the Licenses heading is still Mocha TEXT',
        ABOUT,
        [
            ('        let licenses_heading = p.text;',
             '        let licenses_heading = guitk::color::Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: a licence's header bar is still Mocha SURFACE0",
        ABOUT,
        [
            ('        let name_bg = p.surface0;',
             '        let name_bg = guitk::color::Color::from_hex(0x313244);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: a licence's name is still Mocha LAVENDER",
        ABOUT,
        [
            ('        let name_ink = p.lavender;',
             '        let name_ink = guitk::color::Color::from_hex(0xB4BEFE);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: a licence's body is still Mocha SUBTEXT0",
        ABOUT,
        [
            ('        let body_ink = p.subtext0;',
             '        let body_ink = guitk::color::Color::from_hex(0xA6ADC8);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: a property row's label is still Mocha SUBTEXT0",
        ABOUT,
        [
            ('    let label_ink = p.subtext0;',
             '    let label_ink = guitk::color::Color::from_hex(0xA6ADC8);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: a property row's value is still Mocha TEXT",
        ABOUT,
        [
            ('    let value_ink = p.text;',
             '    let value_ink = guitk::color::Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the wordmark is back to the hard-coded Mocha MANTLE that made it unreadable in Latte',
        ABOUT,
        [
            ('        let wordmark = readable_on(logo);',
             '        let wordmark = guitk::color::Color::from_hex(0x181825);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_wordmark_is_legible_on_the_logo_tile',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the chosen tab's label is still the Mocha BLUE it was hard-coded to",
        ABOUT,
        [
            ('            let label = if is_active { p.accent } else { p.subtext0 };',
             '            let label = if is_active { guitk::color::Color::from_hex(0x89B4FA) } else { p.subtext0 };'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the unchosen tabs are still lettered in Mocha SUBTEXT0',
        ABOUT,
        [
            ('            let label = if is_active { p.accent } else { p.subtext0 };',
             '            let label = if is_active { p.accent } else { guitk::color::Color::from_hex(0xA6ADC8) };'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the empty-licence line is still Mocha SUBTEXT0 — a site only the empty fixture renders',
        ABOUT,
        [
            ('        let empty_ink = p.subtext0;',
             '        let empty_ink = guitk::color::Color::from_hex(0xA6ADC8);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'the_empty_licence_list_is_drawn_from_the_palette',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the dialog is filled with MANTLE, so it does not sit above the desktop',
        ABOUT,
        [
            ('        let bg = p.base;',
             '        let bg = p.mantle;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the dialog border is a rung too loud',
        ABOUT,
        [
            ('        let border = p.surface1;',
             '        let border = p.surface2;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the dialog's own title is as quiet as a property label",
        ABOUT,
        [
            ('        let title_ink = p.text;',
             '        let title_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the chosen tab is filled a rung too quietly to read as chosen',
        ABOUT,
        [
            ('        let tab_fill = p.surface1;',
             '        let tab_fill = p.surface0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the chosen tab's fill takes the logo's colour, so the strip competes with the branding",
        ABOUT,
        [
            ('        let tab_fill = p.surface1;',
             '        let tab_fill = p.blue;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the chosen tab stops being marked as chosen at all',
        ABOUT,
        [
            ('            let label = if is_active { p.accent } else { p.subtext0 };',
             '            let label = if is_active { p.text } else { p.subtext0 };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: every tab is lettered as loudly as the chosen one',
        ABOUT,
        [
            ('            let label = if is_active { p.accent } else { p.subtext0 };',
             '            let label = if is_active { p.accent } else { p.text };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the tab strip is inverted, so the three tabs you are not on look chosen',
        ABOUT,
        [
            ('            let label = if is_active { p.accent } else { p.subtext0 };',
             '            let label = if is_active { p.subtext0 } else { p.accent };'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the logo tile follows the user's accent, so the product's mark changes with a preference",
        ABOUT,
        [
            ('        let logo = p.blue;',
             '        let logo = p.accent;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
            'every_site_changes_when_the_mode_does',
            # The wordmark is readable_on(logo), so pointing the logo at the
            # accent re-derives the ink from the fixture's off-palette magenta.
            # readable_on answers white there, and white on magenta is 3.14:1 —
            # so the legibility test fires too, and correctly. That is the
            # off-palette fixture earning its keep twice: it is not a member of
            # the palette *and* it is not a colour anything reads well on.
            'the_wordmark_is_legible_on_the_logo_tile',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the logo tile is the wrong hue',
        ABOUT,
        [
            ('        let logo = p.blue;',
             '        let logo = p.mauve;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            # The accent test's other half is `colors[8] == p.blue` — a claim
            # about the logo, not about the accent — so it sees any hue swap
            # there, not merely a swap to the accent.
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the wordmark is body text, which is unreadable on the tile in both modes',
        ABOUT,
        [
            ('        let wordmark = readable_on(logo);',
             '        let wordmark = p.text;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_wordmark_is_legible_on_the_logo_tile',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the wordmark is inked for the page behind the tile rather than the tile',
        ABOUT,
        [
            ('        let wordmark = readable_on(logo);',
             '        let wordmark = readable_on(p.base);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_wordmark_is_legible_on_the_logo_tile',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the wordmark is CRUST — which is what readable_on answers in Mocha, so only the light render can see it',
        ABOUT,
        [
            ('        let wordmark = readable_on(logo);',
             '        let wordmark = p.crust;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            # This is the documented hole in the membership sweep — CRUST is
            # 0x11111B, which is a readable_on endpoint, so assert_drawn_from
            # is obliged to accept it in a light render and cannot see this at
            # all. The contrast test can: near-black on Latte's *dark* blue is
            # 1.36:1. A membership test cannot check a value it was told to
            # accept, but a test that computes a ratio never agreed to anything.
            'the_wordmark_is_legible_on_the_logo_tile',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: readable_on is applied twice, which inverts the ink it just chose',
        ABOUT,
        [
            ('        let wordmark = readable_on(logo);',
             '        let wordmark = readable_on(readable_on(logo));'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_wordmark_is_legible_on_the_logo_tile',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the wordmark is inked for the accent, a colour it is not drawn on',
        ABOUT,
        [
            ('        let wordmark = readable_on(logo);',
             '        let wordmark = readable_on(p.accent);'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_wordmark_is_legible_on_the_logo_tile',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the version number is as quiet as the date under it',
        ABOUT,
        [
            ('        let version_ink = p.text;',
             '        let version_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the build date is as loud as the version above it',
        ABOUT,
        [
            ('        let built_ink = p.subtext0;',
             '        let built_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the tagline is a rung louder than the quietest text on the page',
        ABOUT,
        [
            ('        let tagline_ink = p.overlay0;',
             '        let tagline_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the Hardware tab's heading is as quiet as the labels under it",
        ABOUT,
        [
            ('        let hardware_heading = p.text;',
             '        let hardware_heading = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the Software tab's heading is as quiet as the labels under it",
        ABOUT,
        [
            ('        let software_heading = p.text;',
             '        let software_heading = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the Licenses tab's heading is as quiet as the bodies under it",
        ABOUT,
        [
            ('        let licenses_heading = p.text;',
             '        let licenses_heading = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: an empty licence list is reported in RED, as though having no licences were an error',
        ABOUT,
        [
            ('        let empty_ink = p.subtext0;',
             '        let empty_ink = p.red;'),
        ],
        ["desktop"],
        [
            'the_empty_licence_list_is_drawn_from_the_palette',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: a licence's header bar is a rung too loud",
        ABOUT,
        [
            ('        let name_bg = p.surface0;',
             '        let name_bg = p.surface1;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: every licence name wears the accent, so a list with no selection looks entirely selected',
        ABOUT,
        [
            ('        let name_ink = p.lavender;',
             '        let name_ink = p.accent;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: a licence's name is indistinguishable from its body",
        ABOUT,
        [
            ('        let name_ink = p.lavender;',
             '        let name_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: licence bodies are a rung quieter than everything else on the page',
        ABOUT,
        [
            ('        let body_ink = p.subtext0;',
             '        let body_ink = p.overlay0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: a property row's label is as loud as its value, so nothing distinguishes the question from the answer",
        ABOUT,
        [
            ('    let label_ink = p.subtext0;',
             '    let label_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: a property row's value is as quiet as its label",
        ABOUT,
        [
            ('    let value_ink = p.text;',
             '    let value_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the dialog builds a palette of its own instead of drawing with the one it was handed',
        ABOUT,
        [
            ('        let mut cmds = Vec::with_capacity(64);',
             '        let mut cmds = Vec::with_capacity(64);\n        let p = &Palette::for_mode(false);'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
            'the_empty_licence_list_is_drawn_from_the_palette',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the Overview tab builds a palette of its own, so only that one tab ignores the mode',
        ABOUT,
        [
            ("        // The product's own mark: themed, but never the user's accent.\n        let logo = p.blue;",
             "        // The product's own mark: themed, but never the user's accent.\n        let p = &Palette::for_mode(false);\n        let logo = p.blue;"),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
            'the_chosen_tab_wears_the_accent_and_the_logo_never_does',
        ],
    ),
    (
        'YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the property row builds a palette of its own, so the Hardware and Software tabs ignore the mode',
        ABOUT,
        [
            ('    let label_ink = p.subtext0;\n    let value_ink = p.text;',
             '    let p = &Palette::for_mode(false);\n    let label_ink = p.subtext0;\n    let value_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_colour_the_dialog_draws_comes_from_its_palette',
            'none_of_the_nine_deleted_constants_is_still_drawn',
            'every_site_draws_the_role_it_claims',
            'every_site_changes_when_the_mode_does',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: a property row draws its label and value in each other's roles",
        ABOUT,
        [
            ('    let label_ink = p.subtext0;\n    let value_ink = p.text;',
             '    let label_ink = p.text;\n    let value_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_site_draws_the_role_it_claims',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the clock band's time is still Mocha TEXT",
        CAL,
        [
            ('        let time_ink = p.text;',
             '        let time_ink = Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
            'the_tray_clock_delegate_draws_the_palette_it_is_handed',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the clock band draws its time and its date in each other's roles",
        CAL,
        [
            ('        let time_ink = p.text;\n        let date_ink = p.subtext0;',
             '        let time_ink = p.subtext0;\n        let date_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'the_tray_clock_delegate_draws_the_palette_it_is_handed',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the clock band's extra-zone rows read a fill role as ink",
        CAL,
        [
            ('        let zone_ink = p.subtext0;',
             '        let zone_ink = p.surface2;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'the_tray_clock_delegate_draws_the_palette_it_is_handed',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the clock band builds a palette of its own, so the clock alone ignores the mode',
        CAL,
        [
            ('        let time_ink = p.text;\n        let date_ink = p.subtext0;\n        let zone_ink = p.subtext0;',
             '        let p = &Palette::for_mode(false);\n        let time_ink = p.text;\n        let date_ink = p.subtext0;\n        let zone_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
            'the_tray_clock_delegate_draws_the_palette_it_is_handed',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the popup background is still Mocha BASE',
        CAL,
        [
            ('        let popup_bg = p.base;',
             '        let popup_bg = Color::from_hex(0x1E1E2E);'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the popup border is still Mocha SURFACE1',
        CAL,
        [
            ('        let popup_border = p.surface1;',
             '        let popup_border = Color::from_hex(0x45475A);'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the popup fill and its border are drawn in each other's roles",
        CAL,
        [
            ('        let popup_bg = p.base;\n        let popup_border = p.surface1;',
             '        let popup_bg = p.surface1;\n        let popup_border = p.base;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the week-number gutter goes back to surface2, the 1.9:1 fill-role-as-ink',
        CAL,
        [
            ('                    color: p.subtext0,\n                    font_size: layout.px(WEEK_NUM_FONT),',
             '                    color: p.surface2,\n                    font_size: layout.px(WEEK_NUM_FONT),'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the month popup's drop shadow is drawn as a palette role instead of black",
        CAL,
        [
            ('            color: Color::rgba(0, 0, 0, 100),\n            corner_radii: radii,',
             '            color: p.crust,\n            corner_radii: radii,'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the clock band is handed a palette of its own rather than the popup's",
        CAL,
        [
            ('.render(p, band.x',
             '.render(&Palette::for_mode(false), band.x'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        'KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the day cells are handed a palette of their own, so only the grid ignores the mode',
        CAL,
        [
            ('            self.render_day_cell(p, &mut cmds, &layout, index, cell, store);',
             '            self.render_day_cell(&Palette::for_mode(false), &mut cmds, &layout, index, cell, store);'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
            'a_coloured_event_shows_the_users_colour_in_the_month_grid',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the month view's navigation arrows are still Mocha SUBTEXT",
        CAL,
        [
            ('        let arrow_ink = p.subtext0;\n        let month_ink = p.text;',
             '        let arrow_ink = Color::from_hex(0xA6ADC8);\n        let month_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the navigation arrows and the month title are drawn in each other's roles",
        CAL,
        [
            ('        let arrow_ink = p.subtext0;\n        let month_ink = p.text;',
             '        let arrow_ink = p.text;\n        let month_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'the_today_button_wears_the_accent',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the Today button wears a fixed blue, so it stops agreeing with today's disc",
        CAL,
        [
            ('        let today_ink = p.accent;',
             '        let today_ink = p.blue;'),
        ],
        ["desktop"],
        [
            'the_today_button_wears_the_accent',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the Today button is drawn as ordinary text, so nothing marks it as a control',
        CAL,
        [
            ('        let today_ink = p.accent;',
             '        let today_ink = p.text;'),
        ],
        ["desktop"],
        [
            'the_today_button_wears_the_accent',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the day-of-week headers are still Mocha SUBTEXT',
        CAL,
        [
            ('        let dow_ink = p.subtext0;',
             '        let dow_ink = Color::from_hex(0xA6ADC8);'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the day-of-week headers read a fill role as ink',
        CAL,
        [
            ('        let dow_ink = p.subtext0;',
             '        let dow_ink = p.surface2;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: today's disc is a fixed blue, so it stops following the accent the user chose",
        CAL,
        [
            ('        let today_disc = p.accent;\n        let selected_disc = p.surface0;',
             '        let today_disc = p.blue;\n        let selected_disc = p.surface0;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'a_coloured_event_shows_the_users_colour_in_the_month_grid',
        ],
    ),
    (
        'SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the selection disc goes back to surface1, one rung too bright to sit under text',
        CAL,
        [
            ('        let today_disc = p.accent;\n        let selected_disc = p.surface0;',
             '        let today_disc = p.accent;\n        let selected_disc = p.surface1;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            # Reconciled after the sweep. This layout test finds the disc by
            # matching `dark().surface0`, so its *locator* is a role assertion
            # it never meant to make: change the role and `find_map` returns
            # nothing, and the `.expect` fires before the geometry it exists to
            # check is ever reached.
            'the_selection_disc_is_drawn_on_the_cell_that_is_clicked',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: today's disc and the selection's are drawn in each other's roles",
        CAL,
        [
            ('        let today_disc = p.accent;\n        let selected_disc = p.surface0;',
             '        let today_disc = p.surface0;\n        let selected_disc = p.accent;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'a_coloured_event_shows_the_users_colour_in_the_month_grid',
            # Same accidental locator as the defect above.
            'the_selection_disc_is_drawn_on_the_cell_that_is_clicked',
            # Reconciled after the sweep, and caught by coincidence rather than
            # by design. The mode test's skip clause is keyed on *values*, not
            # on sites: it treats a colour as legitimately fixed when it equals
            # `readable_on(dark.accent)`, which for this fixture's magenta is
            # `#EFF1F5`. With the discs swapped, today's ink becomes
            # `readable_on(surface0)` — also `#EFF1F5` in Mocha — so the clause
            # misfiles a site that does move, demands it not move, and fails.
            # The collision only ever makes the test stricter (a site wrongly
            # classed as fixed is asserted equal, never skipped), so it cannot
            # hide a defect; it can only produce a failure that needs reading.
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the adjacent months' day numbers go back to surface2, 1.91:1 in Latte",
        CAL,
        [
            ('        } else if cell.current_month {\n            p.text\n        } else {\n            p.subtext0\n        };',
             '        } else if cell.current_month {\n            p.text\n        } else {\n            p.surface2\n        };'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: this month's day numbers are drawn as quietly as the neighbouring months'",
        CAL,
        [
            ('        } else if cell.current_month {\n            p.text\n        } else {\n            p.subtext0\n        };',
             '        } else if cell.current_month {\n            p.subtext0\n        } else {\n            p.subtext0\n        };'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: today's number is a role rather than ink derived from whatever disc it sits on",
        CAL,
        [
            ('        let text_color = if is_today {\n            readable_on(today_disc)\n        } else if cell.current_month {',
             '        let text_color = if is_today {\n            p.text\n        } else if cell.current_month {'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the month-grid dot discards the colour the user gave the event',
        CAL,
        [
            ('            let dot_color = match (first.color, is_today) {\n                (Some(chosen), _) => chosen,',
             '            let dot_color = match (first.color, is_today) {\n                (Some(_), _) => p.lavender,'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'a_coloured_event_shows_the_users_colour_in_the_month_grid',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: an uncoloured dot on today's disc ignores the disc and may vanish into it",
        CAL,
        [
            ('                (None, true) => readable_on(today_disc),',
             '                (None, true) => p.lavender,'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'a_coloured_event_shows_the_users_colour_in_the_month_grid',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the uncoloured event dot reads a fill role as a mark on the base',
        CAL,
        [
            ('                (None, false) => p.lavender,',
             '                (None, false) => p.surface2,'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'a_coloured_event_shows_the_users_colour_in_the_month_grid',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: an uncoloured event resolves to blue, which is also the stock accent',
        CAL,
        [
            ('        self.color.unwrap_or(p.lavender)',
             '        self.color.unwrap_or(p.blue)'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'the_dot_and_the_detail_bar_resolve_a_colour_the_same_way',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: dot_color ignores the colour the user chose and always answers the default',
        CAL,
        [
            ('        self.color.unwrap_or(p.lavender)',
             '        p.lavender'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'the_dot_and_the_detail_bar_resolve_a_colour_the_same_way',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the detail card goes back to surface0, where its time reads at 3.40:1 in Latte',
        CAL,
        [
            ('        let card_bg = p.mantle;',
             '        let card_bg = p.surface0;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the detail card is still Mocha MANTLE',
        CAL,
        [
            ('        let card_bg = p.mantle;',
             '        let card_bg = Color::from_hex(0x181825);'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: an event's time drops a rung to subtext0, the tier the card had to move to escape",
        CAL,
        [
            ('        let time_ink = p.subtext1;',
             '        let time_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the detail card's header and its event times are drawn in each other's roles",
        CAL,
        [
            ('        let header_ink = p.text;\n        let time_ink = p.subtext1;',
             '        let header_ink = p.subtext1;\n        let time_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the N-more overflow line reads a fill role as ink',
        CAL,
        [
            ('        let more_ink = p.subtext1;',
             '        let more_ink = p.surface1;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the detail card's colour bar ignores the event and draws a fixed role",
        CAL,
        [
            ('                color: event.dot_color(p),',
             '                color: p.lavender,'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the detail card builds a palette of its own, so only the card ignores the mode',
        CAL,
        [
            ('        let card_bg = p.mantle;\n        let header_ink = p.text;',
             '        let p = &Palette::for_mode(false);\n        let card_bg = p.mantle;\n        let header_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_month_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the year view's card is still Mocha BASE",
        CAL,
        [
            ('        let card_bg = p.base;\n        let arrow_ink = p.subtext0;\n        let title_ink = p.text;',
             '        let card_bg = Color::from_hex(0x1E1E2E);\n        let arrow_ink = p.subtext0;\n        let title_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the year view's arrows read a fill role as ink",
        CAL,
        [
            ('        let arrow_ink = p.subtext0;\n        let title_ink = p.text;',
             '        let arrow_ink = p.surface2;\n        let title_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: the year view's arrows and its year label are drawn in each other's roles",
        CAL,
        [
            ('        let arrow_ink = p.subtext0;\n        let title_ink = p.text;',
             '        let arrow_ink = p.text;\n        let title_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: the year view's drop shadow is drawn as a palette role instead of black",
        CAL,
        [
            ('            color: Color::rgba(0, 0, 0, 100),\n            corner_radii: CornerRadii::all(layout.px(CARD_RADIUS)),',
             '            color: p.crust,\n            corner_radii: CornerRadii::all(layout.px(CARD_RADIUS)),'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: the year view builds a palette of its own, so one of the two views ignores the mode',
        CAL,
        [
            ('        let card_bg = p.base;\n        let arrow_ink = p.subtext0;\n        let title_ink = p.text;',
             '        let p = &Palette::for_mode(false);\n        let card_bg = p.base;\n        let arrow_ink = p.subtext0;\n        let title_ink = p.text;'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
            'every_colour_the_calendar_draws_comes_from_its_palette',
            'every_role_the_calendar_draws_moves_with_the_mode',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: the mini months' today disc is a fixed blue, disagreeing with the day grid's",
        CAL,
        [
            ('        let today_disc = p.accent;\n        let label_color = if is_current { today_disc } else { p.text };',
             '        let today_disc = p.blue;\n        let label_color = if is_current { today_disc } else { p.text };'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: a mini month's label is drawn as quietly as the day numbers inside it",
        CAL,
        [
            ('        let today_disc = p.accent;\n        let label_color = if is_current { today_disc } else { p.text };',
             '        let today_disc = p.accent;\n        let label_color = if is_current { today_disc } else { p.subtext0 };'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the current mini month's label stops following the disc drawn beneath it",
        CAL,
        [
            ('        let today_disc = p.accent;\n        let label_color = if is_current { today_disc } else { p.text };',
             '        let today_disc = p.accent;\n        let label_color = if is_current { p.blue } else { p.text };'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: a mini month's day numbers read a fill role as ink",
        CAL,
        [
            ('            let text_color = if is_today {\n                readable_on(today_disc)\n            } else {\n                p.subtext0\n            };',
             '            let text_color = if is_today {\n                readable_on(today_disc)\n            } else {\n                p.surface2\n            };'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: a mini month's today digit is a role rather than ink derived from its disc",
        CAL,
        [
            ('            let text_color = if is_today {\n                readable_on(today_disc)\n            } else {\n                p.subtext0\n            };',
             '            let text_color = if is_today {\n                p.text\n            } else {\n                p.subtext0\n            };'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the tray-clock delegate builds a palette of its own and drops the one it was handed',
        CAL,
        [
            ('        clock.render(p, x, y, scale, utc_now, local)',
             '        clock.render(&Palette::for_mode(false), x, y, scale, utc_now, local)'),
        ],
        ["desktop"],
        [
            'the_tray_clock_delegate_draws_the_palette_it_is_handed',
        ],
    ),
    (
        'UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the tray-clock delegate transposes x and y on its way through',
        CAL,
        [
            ('        clock.render(p, x, y, scale, utc_now, local)',
             '        clock.render(p, y, x, scale, utc_now, local)'),
        ],
        ["desktop"],
        [
            'the_tray_clock_delegate_draws_the_palette_it_is_handed',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the year mode dispatches to the month view, so the year view is never drawn',
        CAL,
        [
            ('            CalendarViewMode::Year => self.render_year_view(p, x, y, scale),',
             '            CalendarViewMode::Year => self.render_month_view(p, x, y, scale, utc_now, store),'),
        ],
        ["desktop"],
        [
            'every_year_view_site_draws_the_role_it_claims',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: saving writes a colour line for every event, so an uncoloured one gains one',
        CAL,
        [
            ('            if let Some(c) = event.color {',
             '            {\n                let c = event.color.unwrap_or(Color::from_hex(0x89B4FA));'),
        ],
        ["desktop"],
        [
            'an_uncoloured_event_round_trips_without_gaining_a_colour',
        ],
    ),
    (
        'XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: loading gives an event with no colour line a default, which saving then writes back',
        CAL,
        [
            ('        let mut color: Option<Color> = None;',
             '        let mut color: Option<Color> = Some(Color::from_hex(0x89B4FA));'),
            ('                color = None;',
             '                color = Some(Color::from_hex(0x89B4FA));'),
        ],
        ["desktop"],
        [
            'an_uncoloured_event_round_trips_without_gaining_a_colour',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the picker's background is mantle again, which is also the selected row's fill",
        SESS,
        [
            ('        let picker_bg = p.base;',
             '        let picker_bg = p.mantle;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the picker's background is still the Mocha MANTLE constant",
        SESS,
        [
            ('        let picker_bg = p.base;',
             '        let picker_bg = Color::from_hex(0x181825);'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the picker's border reads surface0 instead of surface1",
        SESS,
        [
            ('        let picker_border = p.surface1;',
             '        let picker_border = p.surface0;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the picker's border is still the Mocha SURFACE1 constant",
        SESS,
        [
            ('        let picker_border = p.surface1;',
             '        let picker_border = Color::from_hex(0x45475A);'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the idle title drops to subtext0',
        SESS,
        [
            ('        let title_ink = p.text;',
             '        let title_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'the_title_wears_the_accent_only_while_searching',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the idle title is still the Mocha TEXT constant',
        SESS,
        [
            ('        let title_ink = p.text;',
             '        let title_ink = Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
        # The accent test only renders the dark palette, where this literal *is*
        # `p.text`, so it cannot see this one. Only the light render can.
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the search state goes back to the named blue instead of the accent',
        SESS,
        [
            ('        let searching_ink = p.accent;',
             '        let searching_ink = p.blue;'),
        ],
        ["desktop"],
        [
        # The pin test's fixture has an empty search box, so this site is not
        # drawn there at all. A site nothing renders is a site nothing checks.
            'the_title_wears_the_accent_only_while_searching',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the title never changes colour, so nothing marks the search as live',
        SESS,
        [
            ('        let searching_ink = p.accent;',
             '        let searching_ink = p.text;'),
        ],
        ["desktop"],
        [
            'the_title_wears_the_accent_only_while_searching',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the selected row goes back to surface0, which no quiet ink survives in Latte',
        SESS,
        [
            ('        let selected_row = p.mantle;',
             '        let selected_row = p.surface0;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the selected row is still the Mocha SURFACE0 constant',
        SESS,
        [
            ('        let selected_row = p.mantle;',
             '        let selected_row = Color::from_hex(0x313244);'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the selected row's icon is not lifted out of the quiet tier",
        SESS,
        [
            ('        let selected_icon_ink = p.text;',
             '        let selected_icon_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: an unselected row's icon drops to overlay0",
        SESS,
        [
            ('        let icon_ink = p.subtext0;',
             '        let icon_ink = p.overlay0;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: an unselected row's icon is still the Mocha SUBTEXT0 constant",
        SESS,
        [
            ('        let icon_ink = p.subtext0;',
             '        let icon_ink = Color::from_hex(0xA6ADC8);'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
        ],
    ),
    (
        "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: a workspace's name drops a tier to subtext1",
        SESS,
        [
            ('        let name_ink = p.text;',
             '        let name_ink = p.subtext1;'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: a workspace's name is still the Mocha TEXT constant",
        SESS,
        [
            ('        let name_ink = p.text;',
             '        let name_ink = Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the row captions go back to overlay0, the rung below anything readable',
        SESS,
        [
            ('        let caption_ink = p.subtext1;',
             '        let caption_ink = p.overlay0;'),
        ],
        ["desktop"],
        [
        # Not the contrast test: it reads palette values and a hand-written
        # pairing table and never calls the renderer, so it is structurally
        # incapable of failing for anything in this file. That is a property of
        # the test, not a weakness -- contrast is not a membership property and
        # the sweep cannot see it either.
            'every_picker_site_draws_the_role_it_claims',
            'the_empty_state_draws_the_caption_ink',
        ],
    ),
    (
        'QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the row captions are still the Mocha OVERLAY0 constant',
        SESS,
        [
            ('        let caption_ink = p.subtext1;',
             '        let caption_ink = Color::from_hex(0x6C7086);'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
            'the_empty_state_draws_the_caption_ink',
        ],
    ),
    (
        'RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the selected and unselected icon inks are transposed',
        SESS,
        [
            ('                color: if selected {\n                    selected_icon_ink\n                } else {\n                    icon_ink\n                },',
             '                color: if selected {\n                    icon_ink\n                } else {\n                    selected_icon_ink\n                },'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the picker's background and its border are drawn in each other's colours",
        SESS,
        [
            ('            color: picker_bg,\n            corner_radii: CornerRadii::all(12.0),\n        });\n\n        // Border.\n        commands.push(RenderCommand::StrokeRect {\n            x: px,\n            y: py,\n            width: picker_w,\n            height: picker_h,\n            color: picker_border,',
             '            color: picker_border,\n            corner_radii: CornerRadii::all(12.0),\n        });\n\n        // Border.\n        commands.push(RenderCommand::StrokeRect {\n            x: px,\n            y: py,\n            width: picker_w,\n            height: picker_h,\n            color: picker_bg,'),
        ],
        ["desktop"],
        [
        # One edit spanning both sites, not two edits. Two would undo each
        # other: the harness replaces the first match, so the second edit would
        # find the line the first one had just rewritten. See lesson 18.
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        'TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the drop shadow is tinted, so it is a colour rather than an absence of light',
        SESS,
        [
            ('            color: Color::rgba(0, 0, 0, 120),',
             '            color: Color::rgba(0, 0, 40, 120),'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the colour tag ignores the user's choice and always draws the theme's blue",
        SESS,
        [
            ('                color: ws.tag_color(p),',
             '                color: p.blue,'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'the_tag_the_picker_draws_is_the_one_the_resolver_gives',
        ],
    ),
    (
        'VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the colour tag follows the accent, so every workspace wears the same tag',
        SESS,
        [
            ('                color: ws.tag_color(p),',
             '                color: p.accent,'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'the_tag_the_picker_draws_is_the_one_the_resolver_gives',
        ],
    ),
    (
        "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: an untagged workspace's default tag follows the accent instead of blue",
        SESS,
        [
            ('        self.color.unwrap_or(p.blue)',
             '        self.color.unwrap_or(p.accent)'),
        ],
        ["desktop"],
        [
        # Not the resolver test: it compares what the renderer drew against what
        # `tag_color` answers, and both move together here. An expectation taken
        # from the code under test is an echo -- lesson 22. The pin test holds
        # because it names `p.blue` itself.
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: an untagged workspace's default tag is still the Mocha BLUE constant",
        SESS,
        [
            ('        self.color.unwrap_or(p.blue)',
             '        self.color.unwrap_or(Color::from_hex(0x89B4FA))'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the resolver drops the user's choice on the floor",
        SESS,
        [
            ('        self.color.unwrap_or(p.blue)',
             '        {\n            let _ = self.color;\n            p.blue\n        }'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'a_tagged_workspace_keeps_the_users_colour',
        ],
    ),
    (
        'ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: a new workspace is born carrying a resolved dark-mode blue',
        SESS,
        [
            ('            pinned_desktop: None,\n            color: None,',
             '            pinned_desktop: None,\n            color: Some(Color::from_hex(0x89B4FA)),'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'every_colour_the_picker_draws_comes_from_its_palette',
            'every_themed_site_the_picker_draws_moves_with_the_mode',
            'a_new_workspace_is_born_untagged',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: duplicating a workspace throws its colour tag away',
        SESS,
        [
            ('            new_ws.color = source.color;',
             '            new_ws.color = None;'),
        ],
        ["desktop"],
        [
            'a_tagged_workspace_keeps_the_users_colour',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the window count is drawn in the icon's ink",
        SESS,
        [
            ('                font_size: 11.0,\n                color: caption_ink,',
             '                font_size: 11.0,\n                color: icon_ink,'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the shortcut hint alone goes back to overlay0',
        SESS,
        [
            ('                    font_size: 10.0,\n                    color: caption_ink,',
             '                    font_size: 10.0,\n                    color: p.overlay0,'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the empty-state line alone goes back to overlay0',
        SESS,
        [
            ('                font_size: 13.0,\n                color: caption_ink,',
             '                font_size: 13.0,\n                color: p.overlay0,'),
        ],
        ["desktop"],
        [
        # The pin test's fixture always matches at least one workspace, so the
        # empty branch never runs there. This is the whole reason the empty state
        # got a fixture of its own.
            'the_empty_state_draws_the_caption_ink',
        ],
    ),
    (
        "EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: a workspace's name is drawn in the caption ink at its site",
        SESS,
        [
            ('                font_size: 14.0,\n                color: name_ink,',
             '                font_size: 14.0,\n                color: caption_ink,'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
        ],
    ),
    (
        "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the selection is filled in the picker's own background, so it is invisible",
        SESS,
        [
            ('                    color: selected_row,\n                    corner_radii: CornerRadii::all(8.0),',
             '                    color: picker_bg,\n                    corner_radii: CornerRadii::all(8.0),'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: every row is filled, not just the selected one',
        SESS,
        [
            ('            if selected {\n                commands.push(RenderCommand::FillRect {',
             '            if true {\n                commands.push(RenderCommand::FillRect {'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: no row is ever filled, so the selection is not shown at all',
        SESS,
        [
            ('            if selected {\n                commands.push(RenderCommand::FillRect {',
             '            if false {\n                commands.push(RenderCommand::FillRect {'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        "IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the picker's background and the selected row's fill are transposed",
        SESS,
        [
            ('        let picker_bg = p.base;',
             '        let picker_bg = p.mantle;'),
            ('        let selected_row = p.mantle;',
             '        let selected_row = p.base;'),
        ],
        ["desktop"],
        [
        # Two edits are safe here where they were not for the background/border
        # transposition above: the second anchor still reads `p.mantle` on the
        # *binding* line, which the first edit did not create a duplicate of.
            'every_picker_site_draws_the_role_it_claims',
            'only_the_selected_row_is_filled',
        ],
    ),
    (
        'JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the title always draws the search ink, so the idle state looks live',
        SESS,
        [
            ('            color: if self.search_text.is_empty() {\n                title_ink\n            } else {\n                searching_ink\n            },',
             '            color: searching_ink,'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'the_title_wears_the_accent_only_while_searching',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the title's two states are transposed",
        SESS,
        [
            ('            color: if self.search_text.is_empty() {\n                title_ink\n            } else {\n                searching_ink\n            },',
             '            color: if self.search_text.is_empty() {\n                searching_ink\n            } else {\n                title_ink\n            },'),
        ],
        ["desktop"],
        [
            'every_picker_site_draws_the_role_it_claims',
            'the_title_wears_the_accent_only_while_searching',
        ],
    ),
    (
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the drag card's fill is mantle, not base",
        FD,
        [
            ('    let card_fill = p.base;',
             '    let card_fill = p.mantle;'),
        ],
        ["desktop"],
        [
        # mantle is a role, moves with the mode, and `text` on it reads 12.14/6.57 -
        # so membership, the mode test and the contrast walk all pass. Only the
        # ordered pin knows which role belongs here.
            'every_drag_overlay_site_draws_the_role_it_claims',
        ],
    ),
    (
        "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the drag card's fill is still the Mocha BASE constant",
        FD,
        [
            ('    let card_fill = p.base;',
             '    let card_fill = Color::from_hex(0x1E1E2E);'),
        ],
        ["desktop"],
        [
        # the contrast walk catches this one too: Latte `text` on Mocha `base` is
        # 2.05, because the card stops following the mode but its ink does not.
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_colour_the_drag_overlay_draws_comes_from_its_palette',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: the dragged item's description drops a tier to subtext0",
        FD,
        [
            ('    let desc_ink = p.text;',
             '    let desc_ink = p.subtext0;'),
        ],
        ["desktop"],
        [
        # 7.37/4.64 on base, so it clears the floor - a readable defect is still a
        # defect, and only the pin can see it.
            'every_drag_overlay_site_draws_the_role_it_claims',
        ],
    ),
    (
        "DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the dragged item's description is still the Mocha TEXT constant",
        FD,
        [
            ('    let desc_ink = p.text;',
             '    let desc_ink = Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_colour_the_drag_overlay_draws_comes_from_its_palette',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: the badge ink is crust, the reflexive darkest role',
        FD,
        [
            ('    let badge_ink = p.base;',
             '    let badge_ink = p.crust;'),
        ],
        ["desktop"],
        [
        # the headline defect this module exists to catch: 12.61/10.59 in Mocha but
        # 3.98/3.95 in Latte. Membership and the mode test both pass - crust is a
        # perfectly good role that moves with the theme. Only a contrast check that
        # reads the real ink/fill pairing out of the rendered commands sees it.
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the badge ink is still the Mocha BASE constant',
        FD,
        [
            ('    let badge_ink = p.base;',
             '    let badge_ink = Color::from_hex(0x1E1E2E);'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_colour_the_drag_overlay_draws_comes_from_its_palette',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the item-count chip goes back to peach, colliding with Link',
        FD,
        [
            ('    let count_chip = p.text;',
             '    let count_chip = p.peach;'),
        ],
        ["desktop"],
        [
        # base ink on peach is 9.27/4.62, so the contrast walk passes: this is a
        # meaning collision, not a legibility one, and it needs its own test.
            'every_drag_overlay_site_draws_the_role_it_claims',
            'the_item_count_chip_is_never_an_effect_colour',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the item-count chip is still the Mocha PEACH constant',
        FD,
        [
            ('    let count_chip = p.text;',
             '    let count_chip = Color::from_hex(0xFAB387);'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_colour_the_drag_overlay_draws_comes_from_its_palette',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'the_item_count_chip_is_never_an_effect_colour',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        'IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII: the item-count chip is surface2, the obvious-looking chip that cannot work',
        FD,
        [
            ('    let count_chip = p.text;',
             '    let count_chip = p.surface2;'),
        ],
        ["desktop"],
        [
        # 2.46 Mocha / 1.91 Latte - a surface role is defined as being near the
        # background, so it fails as a badge fill in *both* modes.
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ: the description's ink and the badge's ink are transposed",
        FD,
        [
            ('    let desc_ink = p.text;',
             '    let desc_ink = p.base;'),
            ('    let badge_ink = p.base;',
             '    let badge_ink = p.text;'),
        ],
        ["desktop"],
        [
        # two edits are safe: each anchor carries its own binding name, so the first
        # edit cannot create a duplicate of the second's anchor (lesson 18).
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKK: the card's fill and the count chip are transposed",
        FD,
        [
            ('    let card_fill = p.base;',
             '    let card_fill = p.text;'),
            ('    let count_chip = p.text;',
             '    let count_chip = p.base;'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "LLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLLL: copy and move wear each other's colours, so green now means move",
        FD,
        [
            ('            Self::Copy => p.green,\n            Self::Move => p.blue,',
             '            Self::Copy => p.blue,\n            Self::Move => p.green,'),
        ],
        ["desktop"],
        [
        # the site pin cannot catch this and must not be declared: its expectation
        # calls `effect.color(p)`, so it is an echo of the code under test (lesson
        # 22). Both colours are palette roles that move with the mode, so
        # membership and the mode test pass, and base ink clears 4.5 on both, so
        # the contrast walk passes. Only the hand-pinned role vector sees it.
            'each_drop_effect_wears_its_own_role',
        ],
    ),
    (
        'MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM: a forbidden drop wears the link colour',
        FD,
        [
            ('            Self::None => p.red,',
             '            Self::None => p.peach,'),
        ],
        ["desktop"],
        [
            'each_drop_effect_wears_its_own_role',
        ],
    ),
    (
        'NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN: every effect wears the same blue, so the badge says nothing',
        FD,
        [
            ('            Self::None => p.red,\n            Self::Copy => p.green,\n            Self::Move => p.blue,\n            Self::Link => p.peach,',
             '            Self::None => p.blue,\n            Self::Copy => p.blue,\n            Self::Move => p.blue,\n            Self::Link => p.blue,'),
        ],
        ["desktop"],
        [
            'each_drop_effect_wears_its_own_role',
        ],
    ),
    (
        'OOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO: every effect wears the accent, so the badge tracks the theme not the verb',
        FD,
        [
            ('            Self::None => p.red,\n            Self::Copy => p.green,\n            Self::Move => p.blue,\n            Self::Link => p.peach,',
             '            Self::None => p.accent,\n            Self::Copy => p.accent,\n            Self::Move => p.accent,\n            Self::Link => p.accent,'),
        ],
        ["desktop"],
        [
        # the accent is one of the 21 roles, so membership passes, and it moves with
        # the mode, so the mode test passes.
            'each_drop_effect_wears_its_own_role',
        ],
    ),
    (
        'PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP: the link effect is still the Mocha PEACH constant',
        FD,
        [
            ('            Self::Link => p.peach,',
             '            Self::Link => Color::from_hex(0xFAB387),'),
        ],
        ["desktop"],
        [
            'each_drop_effect_wears_its_own_role',
            'every_colour_the_drag_overlay_draws_comes_from_its_palette',
            'every_colour_the_drop_highlight_draws_comes_from_its_palette',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "QQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ: the effect badge's label is drawn in the description's ink",
        FD,
        [
            ('        font_size: 9.0,\n        color: badge_ink,',
             '        font_size: 9.0,\n        color: desc_ink,'),
        ],
        ["desktop"],
        [
        # `color: badge_ink,` appears at two sites, so the anchor has to span the
        # font size above it to be unique.
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR: the item count is drawn in the chip's own colour, so it is invisible",
        FD,
        [
            ('            font_size: 10.0,\n            color: badge_ink,',
             '            font_size: 10.0,\n            color: count_chip,'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "SSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS: the item-count chip is filled with the card's own colour",
        FD,
        [
            ('            height: 16.0,\n            color: count_chip,',
             '            height: 16.0,\n            color: card_fill,'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT: the effect badge is filled with the count chip's colour",
        FD,
        [
            ('        height: 12.0,\n        color: badge_color,',
             '        height: 12.0,\n        color: count_chip,'),
        ],
        ["desktop"],
        [
        # base ink on a `text` chip is 11.34/7.06, so this is perfectly legible and
        # perfectly wrong - the badge stops saying what will happen. Contrast
        # cannot see it; only the pin can.
            'every_drag_overlay_site_draws_the_role_it_claims',
        ],
    ),
    (
        "UUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUUU: the target tooltip's label drops to overlay0",
        FD,
        [
            ('    let tooltip_ink = p.text;',
             '    let tooltip_ink = p.overlay0;'),
        ],
        ["desktop"],
        [
        # 3.36 Mocha / 2.30 Latte - the same overlay-as-ink shape module 44 found.
            'every_drop_highlight_site_draws_the_role_it_claims',
            'the_drop_tooltips_label_is_readable_on_its_fill',
        ],
    ),
    (
        "VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV: the target tooltip's label is still the Mocha TEXT constant",
        FD,
        [
            ('    let tooltip_ink = p.text;',
             '    let tooltip_ink = Color::from_hex(0xCDD6F4);'),
        ],
        ["desktop"],
        [
            'every_drop_highlight_site_draws_the_role_it_claims',
            'every_colour_the_drop_highlight_draws_comes_from_its_palette',
            'every_site_the_drop_highlight_draws_moves_with_the_mode',
            'the_drop_tooltips_label_is_readable_on_its_fill',
        ],
    ),
    (
        'WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW: the target tooltip is filled with surface1',
        FD,
        [
            ('    let tooltip_fill = p.base;',
             '    let tooltip_fill = p.surface1;'),
        ],
        ["desktop"],
        [
        # 6.31 in Mocha and 4.39 in Latte - under the floor by a tenth, and
        # invisible to any check that only looks at dark mode.
            'every_drop_highlight_site_draws_the_role_it_claims',
            'the_drop_tooltips_label_is_readable_on_its_fill',
        ],
    ),
    (
        "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX: the target tooltip's fill and label are transposed",
        FD,
        [
            ('    let tooltip_fill = p.base;',
             '    let tooltip_fill = p.text;'),
            ('    let tooltip_ink = p.text;',
             '    let tooltip_ink = p.base;'),
        ],
        ["desktop"],
        [
        # NOT the contrast test, though it walks exactly this pair: the contrast
        # ratio is symmetric in its two arguments, so a transposition of the
        # fill and the ink reproduces the ratio exactly. Only the ordered site
        # table can see this one.
            'every_drop_highlight_site_draws_the_role_it_claims',
        ],
    ),
    (
        "YYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYY: the target's glowing border is drawn in the tooltip's ink",
        FD,
        [
            ('        color,\n        line_width: 2.0,',
             '        color: tooltip_ink,\n        line_width: 2.0,'),
        ],
        ["desktop"],
        [
        # the border carries no text, so no contrast pair changes; `text` is a role
        # that moves with the mode, so membership and the mode test pass. The
        # border stops meaning anything and only the pin notices.
            'every_drop_highlight_site_draws_the_role_it_claims',
        ],
    ),
    (
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ: the target's border and its tooltip fill are transposed",
        FD,
        [
            ('        color,\n        line_width: 2.0,',
             '        color: tooltip_fill,\n        line_width: 2.0,'),
            ('            color: Color::rgba(tooltip_fill.r, tooltip_fill.g, tooltip_fill.b, 200),',
             '            color: Color::rgba(color.r, color.g, color.b, 200),'),
        ],
        ["desktop"],
        [
        # safe as two edits: the first writes `color: tooltip_fill,` which does not
        # match the second anchor's `Color::rgba(` shape.
            'every_drop_highlight_site_draws_the_role_it_claims',
            'the_drop_tooltips_label_is_readable_on_its_fill',
        ],
    ),
    (
        'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA: the drop highlight resolves its effect colour from a hardcoded dark palette',
        FD,
        [
            ('    let color = effect.color(p);',
             '    let color = effect.color(&Palette::for_mode(false));'),
        ],
        ["desktop"],
        [
            'every_drop_highlight_site_draws_the_role_it_claims',
            'every_colour_the_drop_highlight_draws_comes_from_its_palette',
            'every_site_the_drop_highlight_draws_moves_with_the_mode',
        ],
    ),
    (
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB: the drag badge resolves its effect colour from a hardcoded dark palette',
        FD,
        [
            ('    let badge_color = session.current_effect.color(p);',
             '    let badge_color = session.current_effect.color(&Palette::for_mode(false));'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_colour_the_drag_overlay_draws_comes_from_its_palette',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
        ],
    ),
    (
        'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC: a single-item drag draws a count badge reading 1',
        FD,
        [
            ('    // Item count badge (if multiple).\n    if count > 1 {',
             '    // Item count badge (if multiple).\n    if true {'),
        ],
        ["desktop"],
        [
        # `if count > 1` appears twice - the other governs the description's room -
        # so the anchor spans the comment above it.
            'a_single_item_drag_draws_no_count_chip',
        ],
    ),
    (
        'DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD: the count badge is never drawn, so a twelve-file drag looks like one file',
        FD,
        [
            ('    // Item count badge (if multiple).\n    if count > 1 {',
             '    // Item count badge (if multiple).\n    if false {'),
        ],
        ["desktop"],
        [
            'every_drag_overlay_site_draws_the_role_it_claims',
            'every_site_the_drag_overlay_draws_moves_with_the_mode',
            'every_ink_the_drag_card_draws_is_readable_on_what_it_sits_on',
            'the_item_count_chip_is_never_an_effect_colour',
            'a_multi_item_description_stops_before_the_count_badge',
            'only_the_card_and_the_tooltip_are_translucent',
        ],
    ),
    (
        'EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE: an unlabelled target draws an empty tooltip',
        FD,
        [
            ('    if !target.label.is_empty() {',
             '    if true {'),
        ],
        ["desktop"],
        [
            'an_unlabelled_target_draws_no_tooltip',
        ],
    ),
    (
        'FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF: the target tooltip is never drawn, so the target never says what it is',
        FD,
        [
            ('    if !target.label.is_empty() {',
             '    if false {'),
        ],
        ["desktop"],
        [
        # not declared against the highlight membership test: drawing *fewer*
        # colours leaves every colour still drawn a legal member.
            'every_drop_highlight_site_draws_the_role_it_claims',
            'every_site_the_drop_highlight_draws_moves_with_the_mode',
            'the_drop_tooltips_label_is_readable_on_its_fill',
            'only_the_card_and_the_tooltip_are_translucent',
        ],
    ),
    (
        'GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG: the drag card is fully opaque and hides what is being dragged onto',
        FD,
        [
            ('        color: Color::rgba(card_fill.r, card_fill.g, card_fill.b, 220),',
             '        color: Color::rgba(card_fill.r, card_fill.g, card_fill.b, 255),'),
        ],
        ["desktop"],
        [
        # every other test in the module flattens alpha so one list can be compared
        # against palette roles, which makes all of them blind to this.
            'only_the_card_and_the_tooltip_are_translucent',
        ],
    ),
    (
        'HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH: the target tooltip is fully opaque',
        FD,
        [
            ('            color: Color::rgba(tooltip_fill.r, tooltip_fill.g, tooltip_fill.b, 200),',
             '            color: Color::rgba(tooltip_fill.r, tooltip_fill.g, tooltip_fill.b, 255),'),
        ],
        ["desktop"],
        [
            'only_the_card_and_the_tooltip_are_translucent',
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
    noop = 0
    for name, path, edits, _pkgs, _expect in DEFECTS:
        text = snap[path].decode("utf-8")
        original = text
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
        # A multi-edit defect can undo itself. `str.replace(old, new, 1)` takes
        # the *first* match, so if one edit creates the pattern a later edit
        # looks for, and the created copy sits earlier in the file, the later
        # edit reverts it and the pair cancels. Module 35 wrote two such
        # defects — a badge's border and lettering trading roles, and the
        # selection highlight trading rungs with a badge — and both came back
        # from a 25-minute run as `NO TEST FAILED`.
        #
        # That verdict is not merely unhelpful, it is the *opposite* of the
        # truth: it reads as "the suite has a hole here" when what happened is
        # "the defect was never introduced, so nothing was asked of the
        # suite". A harness whose whole purpose is to stop a test being
        # trusted on faith must not itself report an unasked question as an
        # unanswered one. Checking it here rather than mid-run also means the
        # authoring mistake surfaces in the seconds-long preflight instead of
        # after the run it invalidates.
        if text == original:
            print(f"NO-OP  {name}\n    every edit was undone by a later one")
            noop += 1
    print(f"\n{len(DEFECTS)} defects, {bad} stale, {amb} ambiguous, {noop} no-op")
    return 1 if bad or amb or noop else 0


def apply_to(snap, path, edits):
    """`(patched_text, None)`, or `(None, why)` if the defect cannot be put in.

    One implementation of "apply this defect", shared by the preflights and
    the real run, so that what a preflight vets is what the run runs. The two
    refusals are the ones `check()` names — a pattern that is gone, and a set
    of edits that cancels out — and both must be refusals rather than silent
    successes: writing unmodified source and then asking the suite about it
    produces `NO TEST FAILED`, which reads as a hole in the tests for a
    question they were never asked.

    They are reported apart rather than together because they mean different
    things to whoever fixes them: a missing pattern is source that moved under
    the defect, while a no-op is the defect arguing with itself (lesson 18).
    """
    original = snap[path].decode("utf-8")
    text = original
    for old, new in edits:
        if old not in text:
            return None, "PATTERN NOT FOUND"
        text = text.replace(old, new, 1)
    if text == original:
        return None, "*** PATCH IS A NO-OP ***"
    return text, None


def cargo_check(pkg):
    """`None` if `pkg` builds, else the compiler's first error lines.

    `--all-targets` rather than a bare check: a defect reinstates a constant
    that the *tests* are the only remaining reader of in several modules, so
    checking only the lib would miss exactly the errors these defects cause.
    """
    r = subprocess.run(
        ["cargo", "check", "-p", pkg, "--target", TARGET, "--all-targets"],
        cwd=ROOT, capture_output=True, text=True, errors="replace",
    )
    if r.returncode == 0:
        return None
    out = r.stdout + r.stderr
    why = [
        ln.rstrip()
        for ln in out.splitlines()
        if ln.startswith("error[") or ln.startswith("error:")
    ]
    return "; ".join(why[:3]) if why else "no error line found"


def compile_check(snap, only):
    """Apply each selected defect, `cargo check` it, restore, report.

    See the module docstring for why this exists: `--check` proves an anchor
    is *findable*, not that the file it produces is Rust. An anchor's last
    line must be the line being replaced — anything else invites a
    replacement that rewrites the wrong line — and this is the pass that
    notices when it was not.

    Restores after every defect rather than at the end, so that an interrupted
    preflight leaves at most one file patched and the caller's `finally` puts
    even that back.
    """
    broken, skipped, ok = [], [], 0
    for name, path, edits, pkgs, _expect in DEFECTS:
        if only and name.split(":", 1)[0] not in only:
            continue
        text, why = apply_to(snap, path, edits)
        if text is None:
            # Not this pass's finding, but report it rather than skipping
            # silently: a defect that never went in was not vetted, and a
            # preflight that says nothing about it would be read as clearance.
            skipped.append(name)
            print(f"NOT APPLIED  {name}\n    {why}", flush=True)
            continue
        (ROOT / path).write_text(text, encoding="utf-8", newline="")
        try:
            why = None
            for pkg in pkgs:
                why = cargo_check(pkg)
                if why:
                    why = f"{pkg}: {why}"
                    break
        finally:
            (ROOT / path).write_bytes(snap[path])
        if why:
            broken.append(name)
            print(f"DOES NOT COMPILE  {name}\n    {why}", flush=True)
        else:
            ok += 1
            print(f"builds  {name}", flush=True)
    print(
        f"\n{ok + len(broken) + len(skipped)} defects: {ok} build, "
        f"{len(broken)} do not, {len(skipped)} not applied"
    )
    if broken:
        print(f"\nfix before running the sweep: {broken}")
    return 1 if broken or skipped else 0


def restore(files, snap, digest):
    """Write every snapshotted file back and prove it is byte-identical."""
    bad = []
    for f in files:
        (ROOT / f).write_bytes(snap[f])
        if hashlib.sha256((ROOT / f).read_bytes()).hexdigest() != digest[f]:
            bad.append(f)
    if bad:
        print(f"!!! NOT RESTORED: {bad}")
        sys.exit(2)
    print("restored: all files match their recorded SHA-256")


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

    if sys.argv[1:2] == ["--compile"]:
        # Its own `finally`: `compile_check` writes to the tree, so a Ctrl-C
        # between the patch and the per-defect restore must still be caught by
        # the same SHA-256-verified rewrite the real run gets.
        try:
            rc = compile_check(snap, sys.argv[2:])
        finally:
            restore(files, snap, digest)
        sys.exit(rc)

    only = sys.argv[1:]
    verdicts = []
    try:
        for name, path, edits, pkgs, expect in DEFECTS:
            # Split on the colon rather than taking `name[0]`: the labels ran
            # past Z, so `"AA"[0]` would select defect A as well.
            if only and name.split(":", 1)[0] not in only:
                continue
            # `apply_to` folds together the two refusals: a pattern that no
            # longer matches, and (see `check()`) a multi-edit defect that
            # cancels itself out. Running the suite against unmodified source
            # would report `NO TEST FAILED` — an accusation against the tests
            # for a question they were never asked. Which of the two it was is
            # `--check`'s to report, in seconds and without a toolchain; here
            # the only thing that matters is that the suite is not consulted.
            text, why = apply_to(snap, path, edits)
            if text is None:
                verdicts.append((name, why))
                print(f"{name}\n    {why}\n", flush=True)
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
        restore(files, snap, digest)

    print("\n=== summary ===")
    for name, verdict in verdicts:
        print(f"{name}\n    {verdict}")

    def tally(mark):
        return sum(1 for _, v in verdicts if mark in v)

    # The only verdict that is evidence *against* the suite: the defect was
    # introduced, the suite ran, and the suite said nothing.
    escaped = tally("NO TEST FAILED")
    # None of these three asked the suite anything, so none is evidence about
    # it. Counting them as "caught" would inflate the sweep with defects that
    # were never introduced; counting them as "escaped" would blame the tests
    # for the harness's own authoring error. They get their own column.
    #
    # `DID NOT COMPILE` belongs here and not with `escaped` for exactly the
    # reason the no-op does: a patch that fails to build never reaches a test
    # binary, so no test had the opportunity to fail. It is a defect in the
    # *defect*, not in the suite — see known-issues.md lesson 19, where four
    # module-36 defects emitted two `color:` fields and were reported as
    # escapes by an earlier version of this line.
    unasked = (
        tally("PATCH IS A NO-OP") + tally("PATTERN NOT FOUND") + tally("DID NOT COMPILE")
    )
    print(
        f"\n{len(verdicts)} defects: {len(verdicts) - escaped - unasked} caught, "
        f"{escaped} escaped, {unasked} never asked, "
        f"{tally('[MISSING:')} under-caught, "
        f"{tally('[UNDECLARED:')} under-declared"
    )


if __name__ == "__main__":
    main()
