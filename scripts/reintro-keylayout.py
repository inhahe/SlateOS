#!/usr/bin/env python3
"""Prove the keyboard-layout tests are regression tests.

The seventeenth of these harnesses. It covers the work that closed
`TD-C-THE-SHELL-KEEPS-FIVE-KEYBOARD-LAYOUTS-THAT-NOTHING-TYPES-WITH` and steps
(1), (2) and (5) of `TD-ONLY-ONE-KEYBOARD-LAYOUT`, recorded in
`design-decisions.md` SS549.

The bug it closes is worth restating, because most of the defects below are
ways of re-opening it while the code still reads as if it were fixed. There
were two hand-written tables describing one keyboard: the compositor's, which
was what you typed with and knew only US QWERTY, and the desktop shell's, which
was what you were *shown* and knew five layouts. Choosing French AZERTY in the
shell redrew the preview, changed the tray chip to `FR`, and left you typing
QWERTY. Only one of the two tables was the keyboard.

The fix routes everything through one dependency-free crate, and the route is a
chain: row strings expand into per-key records, the settings file names one
layout by id, the compositor resolves that id and translates through it, and
the shell draws the same records as key caps. Every link can be cut without
breaking the build, and a cut link looks like plumbing that is there. So the
defects fall in five groups:

- **the level model** -- Caps Lock as a second Shift rather than a case latch
  (which types `?` for every German `ß`), Caps and Shift adding instead of
  cancelling, and the AltGr+Shift fallback dropped;
- **the table's shape** -- a scancode truncated rather than rejected, and the
  ISO extra key drawn at the wrong end of the bottom row, which slides every
  cap on that row one place along on exactly the boards that have one;
- **the settings file** -- a blank value taken as a layout named "", an id this
  build does not know being *dropped* rather than preserved (so a file written
  by a later build is silently rewritten), and the id never written at all;
- **the compositor** -- the layout never consulted, consulted on release too
  (every text field typing each letter twice), AltGr left reading as Alt (a
  German `@` opening the menu bar), AltGr cleared unconditionally (every
  application's own AltGr chord lost on layouts with no third level), an
  unknown id resetting the user mid-session, and the modifier state not
  reaching the level;
- **the shell** -- the preview drawing the shifted face, the config parser
  going back to ignoring the layout lines it writes, resolving a name against
  the catalogue before the installed list (which loses a user's own layout on
  reload), an empty resolution emptying the list, and the active index checked
  against the wrong list.

Restore discipline as in the companions: a byte snapshot up front, written back
unconditionally in a `finally`, verified by SHA-256 -- not a reverse
search-and-replace, which silently leaves the tree modified if a patch
half-applied or the process died between the write and the undo.

Two modes:

- `--check` matches every pattern against the snapshot and builds nothing.
  Seconds, no toolchain, and it answers the question that rots on its own: has
  a rename or a rustfmt pass stopped a defect applying?
- No flag: the real sweep. Apply, run the tests, restore, report.

Filter either with defect letters: `reintro-keylayout.py A B C`.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

KEYLAYOUT = "gui/keylayout/src/lib.rs"
SETTINGS = "gui/inputsettings/src/lib.rs"
COMP_LIB = "gui/compositor/src/lib.rs"
KEYMAP = "gui/compositor/src/keymap.rs"
SHELL = "gui/desktop/src/input_method.rs"

KL = ["keylayout"]
IS = ["inputsettings"]
CO = ["compositor"]
DE = ["desktop"]

# ---- test names, so a rename shows up as one edit here -----------------------

# keylayout
ESZETT = "the_german_eszett_is_not_treated_as_the_lower_case_of_a_question_mark"
CANCEL = "caps_lock_and_shift_cancel_on_a_letter"
CAPS_PUNCT = "caps_lock_capitalises_letters_and_leaves_punctuation_alone"
BOTH_FACES = "caps_applies_is_decided_by_both_faces_of_the_key"
FOURTH = "alt_gr_with_shift_falls_back_to_the_plain_alt_gr_face"
WRAPPED = "a_scancode_too_large_for_the_table_is_answered_rather_than_wrapped"
ISO_START = "the_iso_extra_key_is_drawn_at_the_start_of_the_bottom_row"
ROW_LANDS = "a_row_string_lands_on_the_scancodes_it_is_written_against"

# inputsettings
BLANK = "a_blank_layout_name_is_treated_as_absent_rather_than_as_a_layout"
UNKNOWN_KEPT = "a_layout_this_build_does_not_know_survives_a_round_trip_unchanged"
ROUND_TRIP = "test_settings_round_trip"

# compositor
DVORAK = "a_keystroke_arrives_as_the_letter_the_chosen_layout_puts_there"
SOURCE_WINS = "a_character_the_source_supplies_beats_the_layout"
ALTGR_TYPES = "alt_gr_types_a_character_rather_than_forming_an_alt_chord"
ALTGR_KEEPS = "alt_gr_still_reads_as_alt_where_the_layout_has_nothing_on_that_level"
UNNAMEABLE = "a_letter_the_key_enum_cannot_name_still_types_its_character"
RELEASE = "a_release_carries_no_character"
UNKNOWN_HELD = "a_layout_this_build_does_not_know_leaves_the_keyboard_working"
UPPER_FACE = "shift_and_caps_lock_reach_the_client_as_the_upper_face"

# desktop
PREVIEW = "the_preview_draws_the_catalogues_own_characters"
CONFIG_LIST = "the_config_carries_which_layouts_are_installed_and_in_what_order"
CUSTOM = "a_layout_the_catalogue_never_heard_of_survives_a_round_trip"
SHADOW = "a_layout_that_shadows_a_builtin_name_keeps_the_installed_version"
NO_LAYOUTS = "a_config_that_names_no_layout_leaves_the_installed_list_alone"
PAST_END = "an_active_index_past_the_end_of_the_file_s_own_list_is_refused"
STILL_A_KEYBOARD = "a_layout_this_build_does_not_know_still_leaves_a_keyboard"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    # ---- the level model ----------------------------------------------------
    (
        "A: Caps Lock is treated as a second Shift wherever the plain face is a letter",
        KEYLAYOUT,
        [(
            "        self.plain.is_alphabetic() && self.shifted.is_alphabetic()",
            "        self.plain.is_alphabetic()",
        )],
        KL,
        [ESZETT, BOTH_FACES],
    ),
    (
        "B: Caps Lock and Shift add rather than cancel",
        KEYLAYOUT,
        [("            level.shift != level.caps", "            level.shift || level.caps")],
        KL,
        [CANCEL],
    ),
    (
        "C: Caps Lock is applied to every key, punctuation included",
        KEYLAYOUT,
        [(
            "        let upper = if self.caps_applies() {\n"
            "            level.shift != level.caps\n"
            "        } else {\n"
            "            level.shift\n"
            "        };",
            "        let upper = level.shift != level.caps;",
        )],
        KL,
        [CAPS_PUNCT, ESZETT],
    ),
    (
        "D: AltGr+Shift produces nothing where the layout declared only a third level",
        KEYLAYOUT,
        [("                self.altgr_shifted.or(self.altgr)", "                self.altgr_shifted")],
        KL,
        [FOURTH],
    ),
    # ---- the table's shape --------------------------------------------------
    (
        "E: a scancode wider than the table's is truncated instead of refused",
        KEYLAYOUT,
        [(
            "        let scancode = u16::try_from(scancode).ok()?;",
            "        #[allow(clippy::cast_possible_truncation)]\n"
            "        let scancode = scancode as u16;",
        )],
        KL,
        [WRAPPED],
    ),
    (
        "F: the ISO extra key is appended to the bottom row instead of starting it",
        KEYLAYOUT,
        [(
            "            if index == 3\n"
            "                && let Some((plain, shifted)) = self.iso_extra\n"
            "            {\n"
            "                keys.push(KeyDef {\n"
            "                    scancode: sc::ISO_EXTRA,\n"
            "                    plain,\n"
            "                    shifted,\n"
            "                    altgr: None,\n"
            "                    altgr_shifted: None,\n"
            "                });\n"
            "            }\n"
            "            let plain = self.plain.get(index).copied().unwrap_or(\"\");",
            "            let plain = self.plain.get(index).copied().unwrap_or(\"\");",
        ), (
            "            if let Some(end) = row_ends.get_mut(index) {",
            "            if index == 3\n"
            "                && let Some((plain, shifted)) = self.iso_extra\n"
            "            {\n"
            "                keys.push(KeyDef {\n"
            "                    scancode: sc::ISO_EXTRA,\n"
            "                    plain,\n"
            "                    shifted,\n"
            "                    altgr: None,\n"
            "                    altgr_shifted: None,\n"
            "                });\n"
            "            }\n"
            "            if let Some(end) = row_ends.get_mut(index) {",
        )],
        KL,
        [ISO_START],
    ),
    (
        "G: a row string is laid over the scancodes right to left",
        KEYLAYOUT,
        [(
            "                scancodes.iter().zip(plain.chars()).zip(shifted.chars())",
            "                scancodes.iter().rev().zip(plain.chars()).zip(shifted.chars())",
        )],
        KL,
        [ROW_LANDS],
    ),
    # ---- the settings file --------------------------------------------------
    (
        "H: a blank layout value is taken literally as a layout named \"\"",
        SETTINGS,
        [("                .filter(|v| !v.trim().is_empty())\n", "")],
        IS,
        [BLANK],
    ),
    (
        "I: an id this build does not know is dropped on read rather than preserved",
        SETTINGS,
        [(
            "                .filter(|v| !v.trim().is_empty())\n"
            "                .map(|v| v.trim().to_string())",
            "                .filter(|v| !v.trim().is_empty())\n"
            "                .filter(|v| keylayout::by_id(v.trim()).is_some())\n"
            "                .map(|v| v.trim().to_string())",
        )],
        IS,
        [UNKNOWN_KEPT],
    ),
    (
        "J: the chosen layout is never written back to the file",
        SETTINGS,
        [(
            "        doc.set_str(&[\"keyboard\", \"layout\"], &self.keyboard.layout);\n",
            "",
        )],
        IS,
        [ROUND_TRIP, UNKNOWN_KEPT],
    ),
    # ---- the compositor -----------------------------------------------------
    (
        "K: the compositor never consults the layout, as it used to",
        KEYMAP,
        [(
            "    let key = key_for_char(def.plain).unwrap_or_else(|| key_for_scancode(scancode));\n"
            "    (key, def.character(level))",
            "    let _unused = def;\n"
            "    (key_for_scancode(scancode), None)",
        )],
        CO,
        [DVORAK, UNNAMEABLE, ALTGR_TYPES, UPPER_FACE],
    ),
    (
        "L: a character the `Key` enum cannot name loses its physical key too",
        KEYMAP,
        [(
            "    let key = key_for_char(def.plain).unwrap_or_else(|| key_for_scancode(scancode));",
            "    let key = key_for_char(def.plain).unwrap_or(Key::Escape);",
        )],
        CO,
        [UNNAMEABLE],
    ),
    (
        "M: a key release carries the character the key would have typed",
        COMP_LIB,
        [(
            "                character: character.or(if pressed { laid_out } else { None }),",
            "                character: character.or(laid_out),",
        )],
        CO,
        [RELEASE],
    ),
    (
        "N: the layout's character overrides the one the source supplied",
        COMP_LIB,
        [(
            "                character: character.or(if pressed { laid_out } else { None }),",
            "                character: if pressed { laid_out } else { None }.or(character),",
        )],
        CO,
        [SOURCE_WINS],
    ),
    (
        "O: AltGr keeps reading as Alt even when it selected a character",
        COMP_LIB,
        [(
            "            modifiers.alt = self.modifiers.left_alt();\n",
            "",
        )],
        CO,
        [ALTGR_TYPES],
    ),
    (
        "P: AltGr is cleared unconditionally, losing every AltGr chord on a US board",
        COMP_LIB,
        [(
            "        if keymap::resolves_through_alt_gr(self.layout, scancode, level) {",
            "        if level.alt_gr {",
        )],
        CO,
        [ALTGR_KEEPS],
    ),
    (
        "Q: an id this build does not know moves the user back to US QWERTY",
        COMP_LIB,
        [(
            "        if let Some(layout) = keylayout::by_id(&settings.keyboard.layout) {\n"
            "            self.layout = layout;\n"
            "        }",
            "        self.layout = keylayout::by_id(&settings.keyboard.layout)\n"
            "            .unwrap_or_else(keylayout::default_layout);",
        )],
        CO,
        [UNKNOWN_HELD],
    ),
    (
        "R: the modifier state does not reach the level, so every face is the plain one",
        KEYMAP,
        [(
            "            shift: self.left_shift || self.right_shift,\n"
            "            caps: self.caps_lock,",
            "            shift: false,\n"
            "            caps: false,",
        )],
        CO,
        [UPPER_FACE],
    ),
    # ---- the shell ----------------------------------------------------------
    (
        "S: the preview draws the shifted face of every key",
        SHELL,
        [("                let ch = def.plain;", "                let ch = def.shifted;")],
        DE,
        [PREVIEW],
    ),
    (
        "T: the config parser goes back to ignoring the layout lines it writes",
        SHELL,
        [(
            "                        named.push((position, val.to_string()));",
            "                        let _ignored = (position, val);",
        )],
        DE,
        [CONFIG_LIST, PAST_END],
    ),
    (
        "U: a name is resolved against the catalogue before the installed layouts",
        SHELL,
        [(
            "                self.layouts\n"
            "                    .iter()\n"
            "                    .find(|l| l.id == id.as_str())\n"
            "                    .or_else(|| keylayout::by_id(id))\n"
            "                    .cloned()",
            "                keylayout::by_id(id)\n"
            "                    .or_else(|| self.layouts.iter().find(|l| l.id == id.as_str()))\n"
            "                    .cloned()",
        )],
        DE,
        # Not `CUSTOM`: a layout whose id the catalogue never heard of resolves
        # the same either way round, so the round-trip test cannot see this
        # defect. Only a name that is in *both* lists can. This sweep's first
        # run reported U unproved and that is how the gap was found.
        [SHADOW],
    ),
    (
        "V: a config naming no layout empties the installed list",
        SHELL,
        [(
            "        if !resolved.is_empty() {\n"
            "            self.layouts = resolved;",
            "        if true {\n"
            "            self.layouts = resolved;",
        )],
        DE,
        [NO_LAYOUTS],
    ),
    (
        "W: the active index is checked against the list that was installed before parsing",
        SHELL,
        [(
            "                \"active\" => active = val.parse::<usize>().ok(),",
            "                \"active\" => {\n"
            "                    if let Ok(i) = val.parse::<usize>()\n"
            "                        && i < self.layouts.len()\n"
            "                    {\n"
            "                        self.active_index = i;\n"
            "                    }\n"
            "                }",
        ), (
            "        if let Some(idx) = active\n"
            "            && idx < self.layouts.len()\n"
            "        {\n"
            "            self.active_index = idx;\n"
            "        }",
            "        let _late = active;",
        )],
        DE,
        [PAST_END, CONFIG_LIST],
    ),
    (
        "X: a list of names none of which resolve leaves the user with no keyboard",
        SHELL,
        [(
            "        if layouts.is_empty() {\n"
            "            Self::default()\n"
            "        } else {\n"
            "            Self::new(layouts)\n"
            "        }",
            "        Self::new(layouts)",
        )],
        DE,
        [STILL_A_KEYBOARD],
    ),
]

NO_OP: set[str] = set()


def letter(name):
    """The defect's identifier -- everything before the first colon."""
    return name.split(":", 1)[0]


def source(snap, path):
    """The snapshotted file as LF-only text, whatever it is on disk.

    `gui/inputsettings/src/lib.rs` is checked out with CRLF while the other
    four files are LF: git here runs `core.autocrlf=input`, which normalises
    line endings on commit but never on checkout, so a file once written by
    something CRLF-aware stays CRLF in the working tree while the committed
    bytes are LF. A patch spelled with `\\n` then stops matching in that one
    file -- and reports as PATTERN NOT FOUND, which is indistinguishable from
    the rename this script exists to catch. So every pattern below is spelled
    LF, matched against LF, and the file's own convention is put back on write.
    """
    return snap[path].decode("utf-8").replace("\r\n", "\n")


def write_source(path, text, crlf):
    """Write patched text back in the newline convention the file arrived in.

    Not for correctness -- rustc does not care -- but so that a patched file
    differs from its snapshot only where the patch touched it. A whole-file
    newline flip would make any accidental leftover invisible in `git diff`
    under `autocrlf=input`, which is exactly when you most want to see it.
    """
    (ROOT / path).write_text(
        text.replace("\n", "\r\n") if crlf else text, encoding="utf-8", newline=""
    )


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
    crlf = {f: b"\r\n" in b for f, b in snap.items()}
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
            text = source(snap, path)
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
            text = source(snap, path)
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
            write_source(path, text, crlf[path])

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
