#!/usr/bin/env python3
"""Prove the input-settings carriage tests are regression tests.

The sixteenth of these harnesses. It covers the wire that closed
`TD-C-A-POINTER-SPEED-CHANGE-DOES-NOT-REACH-THE-POINTER` and is recorded in
`design-decisions.md` SS548.

The bug it closes is worth restating, because every defect below is a way of
re-opening it while the code still reads as if it were fixed.
`Compositor::reload_input` read the user's `input.yaml`, kept
`mouse.double_click_ms`, and dropped everything else on the floor -- pointer
speed, acceleration profile, button mapping, scroll direction, key repeat. All
of those are applied by whatever integrates the raw device deltas, which is
`EvdevInput`, which the compositor has no reference to and deliberately never
will: a `Compositor` that knew about its display could not also run headless,
into a recording, and onto a DRM card. So the settings had nowhere to go, and
the Settings panel's speed slider was a control that appeared not to work until
the next login.

The fix is a chain, and a chain is exactly the shape that fails silently: the
compositor keeps the whole file, `Server::run_with` polls it once a tick and
pushes any *change* into `Present::reload_input`, `Paired` hands that to its
input half, and `EvdevInput` turns it into a distance the pointer moves. Every
link can be cut without breaking the build, and a cut link looks like plumbing
that is there. So:

- the compositor storing nothing, which is the original bug verbatim;
- the loop never asking, or asking after it has already polled the device --
  the second of which is a frame of pointer motion at the speed the user just
  stopped wanting;
- "not read yet" spelled as the defaults, which is not a missing push but a
  *wrong* one: it overwrites the settings the input source was constructed
  with, so a session where nobody edits the file runs at stock speed;
- the change test dropped or the memory of what was pushed dropped, either of
  which turns a once-per-change call into a call per frame, sixty times a
  second, into a device;
- the pairing forwarding to the screen (which has no pointer) or to nothing;
- and the device itself accepting the settings and not applying them.

Restore discipline as in the companions: a byte snapshot up front, written back
unconditionally in a `finally`, verified by SHA-256 -- not a reverse
search-and-replace, which silently leaves the tree modified if a patch
half-applied or the process died between the write and the undo.

Two modes:

- `--check` matches every pattern against the snapshot and builds nothing.
  Seconds, no toolchain, and it answers the question that rots on its own: has
  a rename or a rustfmt pass stopped a defect applying?
- No flag: the real sweep. Apply, run the tests, restore, report.

Filter either with defect letters: `reintro-input-settings.py A B C`.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

LIB = "gui/compositor/src/lib.rs"
SERVER = "gui/compositor/src/server.rs"
PRESENT = "gui/compositor/src/present.rs"
EVDEV = "gui/compositor/src/present/evdev.rs"

COMP = ["compositor"]

REACHES = "the_pointer_speed_the_user_chose_reaches_the_device_without_a_relogin"
CHANGED = "a_settings_change_mid_session_is_pushed_and_an_unchanged_one_is_not"
UNREAD = "a_compositor_that_has_not_read_the_file_pushes_nothing_at_all"
ORDER = "the_settings_reach_the_device_before_the_first_event_is_polled"
PAIRED = "a_pair_hands_the_users_input_settings_to_the_source_and_not_to_the_screen"
DISTANCE = "reloading_settings_through_the_trait_changes_how_far_the_pointer_travels"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    # ---- the compositor keeps what it read ---------------------------------
    (
        "A: the compositor reads the file and keeps only the double-click window, as it used to",
        LIB,
        [("        self.input = Some(settings);", "        let _discarded = settings;")],
        COMP,
        [REACHES, CHANGED, ORDER],
    ),
    (
        "B: a compositor that has read nothing claims to have read the defaults",
        LIB,
        [("            input: None,", "            input: Some(InputSettings::default()),")],
        COMP,
        [UNREAD],
    ),
    # ---- the loop asks, and asks at the right moment ------------------------
    (
        "C: the loop never asks the compositor what the input settings are",
        SERVER,
        [(
            "            Self::reconcile_input(compositor, present, &mut pushed_input);\n",
            "            let _unpushed = &mut pushed_input;\n",
        )],
        COMP,
        [REACHES, ORDER],
    ),
    (
        "D: the settings are pushed after the device has already been polled",
        SERVER,
        [(
            "            Self::reconcile_input(compositor, present, &mut pushed_input);\n"
            "            for event in present.input() {\n"
            "                compositor.handle_input(event);\n"
            "            }\n",
            "            for event in present.input() {\n"
            "                compositor.handle_input(event);\n"
            "            }\n"
            "            Self::reconcile_input(compositor, present, &mut pushed_input);\n",
        )],
        COMP,
        [ORDER],
    ),
    (
        "E: not knowing the settings is treated as knowing they are the defaults",
        SERVER,
        [(
            "        let Some(settings) = compositor.input_settings() else {\n"
            "            return;\n"
            "        };",
            "        let owned = compositor.input_settings().cloned().unwrap_or_default();\n"
            "        let settings = &owned;",
        )],
        COMP,
        [UNREAD],
    ),
    (
        "F: the settings are pushed into the device on every tick, not on a change",
        SERVER,
        [(
            "        if pushed.as_ref() == Some(settings) {\n"
            "            return;\n"
            "        }",
            "        if false {\n"
            "            return;\n"
            "        }",
        )],
        COMP,
        [REACHES, CHANGED, ORDER],
    ),
    (
        "G: the loop pushes but never remembers what it pushed",
        SERVER,
        [(
            "        *pushed = Some(settings.clone());",
            "        let _unremembered = settings;",
        )],
        COMP,
        [REACHES, CHANGED, ORDER],
    ),
    # ---- the pairing hands it to the half that owns a pointer ---------------
    (
        "H: a pairing hands the input settings to the screen, which has no pointer",
        PRESENT,
        [("        self.input.reload_input(settings);", "        self.screen.reload_input(settings);")],
        COMP,
        [PAIRED],
    ),
    (
        "I: a pairing accepts the input settings and forwards them nowhere",
        PRESENT,
        [(
            "    fn reload_input(&mut self, settings: &InputSettings) {\n"
            "        self.input.reload_input(settings);\n"
            "    }",
            "    fn reload_input(&mut self, _settings: &InputSettings) {}",
        )],
        COMP,
        [PAIRED],
    ),
    # ---- and the device turns a number into a distance ----------------------
    (
        "J: the device accepts the settings through the trait and does not adopt them",
        EVDEV,
        [(
            "    fn reload_input(&mut self, settings: &InputSettings) {\n"
            "        self.set_settings(settings.clone());\n"
            "    }",
            "    fn reload_input(&mut self, _settings: &InputSettings) {}",
        )],
        COMP,
        [DISTANCE],
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
