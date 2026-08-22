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
