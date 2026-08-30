#!/usr/bin/env python3
"""Prove the `ReloadInput` tests are regression tests, one defect at a time.

The companion of `reintro-mouse-page.py`, covering the other three pieces of
`TD-C-THE-MOUSE-SETTINGS-PANEL-REACHES-NOTHING`: the shared `inputsettings`
crate, the `ReloadInput` control verb, and the compositor's handling of it.

These are worth proving separately from the Settings page because they span four
crates and a wire protocol, and the failure mode they exist to catch is the one
that looks like success from every single end: `ReloadInput` encoded as
`ReloadAppearance`, or decoded to it, or handled as it, gives a Settings
application that sends a request, a compositor that answers `Ok`, and a
double-click window that never changes. Every unit test on either side passes.

Restore discipline as in the companion script: byte snapshots taken up front,
written back unconditionally in a `finally`, verified by SHA-256.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

CONTROL = "gui/remote/src/control.rs"
WINDOW = "gui/window/src/lib.rs"
COMP = "gui/compositor/src/lib.rs"
WIRE = "gui/compositor/src/wire.rs"
INPUT = "gui/inputsettings/src/lib.rs"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    (
        "A: ReloadInput goes out on the wire as ReloadAppearance",
        CONTROL,
        [("        RequestBody::ReloadInput => out.push(RequestTag::ReloadInput as u8),",
          "        RequestBody::ReloadInput => out.push(RequestTag::ReloadAppearance as u8),")],
        ["guiremote", "compositor"],
        ["every_request_survives_the_wire",
         "an_input_reload_request_off_the_wire_reaches_the_users_settings_file"],
    ),
    (
        "B: the ReloadInput tag decodes to ReloadAppearance",
        CONTROL,
        [("        RequestTag::ReloadInput => RequestBody::ReloadInput,",
          "        RequestTag::ReloadInput => RequestBody::ReloadAppearance,")],
        ["guiremote", "compositor"],
        ["every_request_survives_the_wire",
         "an_input_reload_request_off_the_wire_reaches_the_users_settings_file"],
    ),
    (
        "C: input_changed() sends the appearance verb",
        WINDOW,
        [("        self.conn.confirm(RequestBody::ReloadInput)",
          "        self.conn.confirm(RequestBody::ReloadAppearance)")],
        ["oswindow"],
        ["telling_the_compositor_the_input_settings_changed_is_its_own_request"],
    ),
    (
        "D: the compositor handles ReloadInput by reloading appearance",
        COMP,
        [("            CompositorRequest::ReloadInput => {\n"
          "                self.reload_input();",
          "            CompositorRequest::ReloadInput => {\n"
          "                self.reload_appearance();")],
        ["compositor"],
        ["an_input_reload_request_off_the_wire_reaches_the_users_settings_file"],
    ),
    (
        "E: the wire maps ReloadInput onto the appearance request",
        WIRE,
        [("        RequestBody::ReloadInput => CompositorRequest::ReloadInput,",
          "        RequestBody::ReloadInput => CompositorRequest::ReloadAppearance,")],
        ["compositor"],
        ["an_input_reload_request_off_the_wire_reaches_the_users_settings_file"],
    ),
    (
        "F: a reload reads the file but adopts nothing",
        COMP,
        [("        let settings = inputsettings::InputFile::load().settings;\n"
          "        self.set_double_click_ms(settings.mouse.double_click_ms);",
          "        let _ = inputsettings::InputFile::load().settings;")],
        ["compositor"],
        ["an_input_reload_adopts_the_double_click_speed_from_the_users_file",
         "an_input_reload_request_off_the_wire_reaches_the_users_settings_file"],
    ),
    (
        "G: the compositor trusts a hand-edited file instead of clamping it",
        COMP,
        [("        self.double_click_interval = Duration::from_millis(u64::from(\n"
          "            ms.clamp(MIN_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS),\n"
          "        ));",
          "        self.double_click_interval = Duration::from_millis(u64::from(ms));")],
        ["compositor"],
        # Not `a_hand_edited_input_file_cannot_make_a_double_click_impossible`,
        # despite that being the test whose comment describes this clamp. It
        # reaches the interval through `InputSettings::read_from`, which clamps
        # first, so the compositor's own clamp can be deleted without that test
        # noticing — a finding of this script, and the reason that test's
        # comment now says which of the two layers it actually proves.
        ["the_double_click_interval_is_clamped_to_a_performable_range"],
    ),
    (
        "H: a reload repaints the desktop",
        COMP,
        [("        let settings = inputsettings::InputFile::load().settings;\n"
          "        self.set_double_click_ms(settings.mouse.double_click_ms);",
          "        let settings = inputsettings::InputFile::load().settings;\n"
          "        self.set_double_click_ms(settings.mouse.double_click_ms);\n"
          "        self.full_recomposite = true;")],
        ["compositor"],
        ["an_input_reload_never_repaints"],
    ),
    (
        "I: an unreadable setting becomes some other setting, not the default",
        INPUT,
        [("            doc.get_str(&[\"pointer\", \"accel_profile\"])\n"
          "                .and_then(|v| AccelProfile::from_yaml_name(&v))",
          "            doc.get_str(&[\"pointer\", \"accel_profile\"])\n"
          "                .map(|v| AccelProfile::from_yaml_name(&v)"
          ".unwrap_or(AccelProfile::Flat))")],
        ["inputsettings"],
        ["an_unknown_spelling_falls_back_to_the_default"],
    ),
    (
        "J: opening a file discards the document it was parsed from",
        INPUT,
        [("    pub fn from_document(doc: Document) -> Self {\n"
          "        Self {\n"
          "            settings: InputSettings::read_from(&doc),\n"
          "            doc,\n"
          "        }\n"
          "    }",
          "    pub fn from_document(doc: Document) -> Self {\n"
          "        Self {\n"
          "            settings: InputSettings::read_from(&doc),\n"
          "            doc: Document::new(),\n"
          "        }\n"
          "    }")],
        ["inputsettings"],
        ["saving_keeps_the_users_comments_and_unknown_keys"],
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
            if only and name[0] not in only:
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
