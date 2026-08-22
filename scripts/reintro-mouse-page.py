#!/usr/bin/env python3
"""Prove the Mouse-page tests are regression tests, one defect at a time.

A test that has never been seen to fail is a test nobody has checked; it may be
asserting something the code could not violate, or asserting it against the
wrong subject. So each defect below reintroduces, in the shipped source, the
exact bug some new test was written to name — then the whole package's suite is
run, and the failure list is compared against the tests that *claimed* to guard
it.

Restoring: every file that could be touched is snapshotted as bytes up front and
written back **unconditionally** in a `finally`, then verified by SHA-256. A
reverse search-and-replace would be the obvious alternative and is wrong — if a
patch half-applied, or a formatter ran, or the process died between the write
and the undo, a reverse replace silently leaves the tree modified while claiming
success. A byte snapshot cannot: the file either matches its recorded digest or
the script says so.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

SETTINGS = "apps/settings/src/main.rs"
INPUT = "gui/inputsettings/src/lib.rs"

# (name, file, [(old, new), ...], [packages to test], [tests expected to fail])
DEFECTS = [
    (
        "A: handle_event never saves the input model",
        SETTINGS,
        [(
            "        if self.input.settings != input_before {\n"
            "            self.save_input();\n"
            "        }\n",
            "",
        )],
        ["settings"],
        [
            "a_drag_on_the_mouse_page_reaches_the_file_the_compositor_reads",
            "each_file_is_written_only_by_a_change_to_its_own_settings",
            "taking_the_input_change_clears_it",
            "a_double_click_change_is_saved_whether_or_not_anyone_drains_the_flag",
            "a_double_click_change_asks_the_compositor_to_re_read_the_input_file",
        ],
    ),
    (
        "B: the input change is announced with the appearance verb",
        SETTINGS,
        [("            && let Err(e) = events.input_changed()",
          "            && let Err(e) = events.appearance_changed()")],
        ["settings"],
        ["a_double_click_change_asks_the_compositor_to_re_read_the_input_file"],
    ),
    (
        "C: the input change is never drained, so nothing is announced",
        SETTINGS,
        [(
            "        if state.take_input_change()\n"
            "            && let Err(e) = events.input_changed()\n"
            "        {\n"
            "            failure = Some(e);\n"
            "            return EventResponse::Exit;\n"
            "        }\n",
            "",
        )],
        ["settings"],
        ["a_double_click_change_asks_the_compositor_to_re_read_the_input_file"],
    ),
    (
        "D: one shared 'something changed' flag saves both files",
        SETTINGS,
        [(
            "        if self.appearance.settings != appearance_before {\n"
            "            self.save_appearance();\n"
            "        }\n"
            "        if self.input.settings != input_before {\n"
            "            self.save_input();\n"
            "        }\n",
            "        if self.appearance.settings != appearance_before\n"
            "            || self.input.settings != input_before\n"
            "        {\n"
            "            self.save_appearance();\n"
            "            self.save_input();\n"
            "        }\n",
        )],
        ["settings"],
        [
            "each_file_is_written_only_by_a_change_to_its_own_settings",
            "an_appearance_change_asks_for_no_input_reload",
        ],
    ),
    (
        "E: the track is wider than the value the model will store",
        SETTINGS,
        [("            Self::DoubleClickMs => (MIN_DOUBLE_CLICK_MS as f32, MAX_DOUBLE_CLICK_MS as f32),",
          "            Self::DoubleClickMs => (0.0, 5000.0),")],
        ["settings"],
        # Only the dedicated test, and that is the point of having it. A drag to
        # either end of the *widened* track still lands on 100 and 2000, because
        # the setter clamps — so the drag test cannot see this, and neither could
        # any test that only exercises the control. A dead zone at each end of a
        # slider is invisible to everything except a test that compares the
        # track's ends against the model's, which is what this one does.
        ["the_double_click_track_spans_exactly_what_the_model_will_store"],
    ),
    (
        "F: the setter stores whatever it is handed",
        INPUT,
        [("        self.double_click_ms = ms.clamp(MIN_DOUBLE_CLICK_MS, MAX_DOUBLE_CLICK_MS);",
          "        self.double_click_ms = ms;")],
        ["settings", "inputsettings"],
        ["the_double_click_track_spans_exactly_what_the_model_will_store"],
    ),
    (
        "G: the Mouse page carries a control nothing consumes",
        SETTINGS,
        [('        self.slider(s, "Double-click interval", SliderId::DoubleClickMs);',
          '        self.slider(s, "Double-click interval", SliderId::DoubleClickMs);\n'
          '        self.slider(s, "Pointer speed", SliderId::TextSize);')],
        ["settings"],
        ["the_mouse_page_offers_only_settings_that_reach_something"],
    ),
    (
        "H: the slider is drawn but moving it stores nothing",
        SETTINGS,
        [("            SliderId::DoubleClickMs => self\n"
          "                .input\n"
          "                .settings\n"
          "                .mouse\n"
          "                .set_double_click_ms(u32::from(round_u16(value))),",
          "            SliderId::DoubleClickMs => {\n"
          "                let _ = value;\n"
          "            }")],
        ["settings"],
        ["dragging_the_double_click_slider_sets_the_interval_it_shows"],
    ),
    (
        "I: the readout drops the unit",
        SETTINGS,
        [('            Self::DoubleClickMs => Some(format!("{whole} ms")),',
          '            Self::DoubleClickMs => Some(format!("{whole}")),')],
        ["settings"],
        ["dragging_the_double_click_slider_sets_the_interval_it_shows"],
    ),
    (
        "J: the Mouse page is not reachable from the sidebar",
        SETTINGS,
        [("                SettingsPage::Mouse,\n", "")],
        ["settings"],
        # Deliberately not a Mouse-specific test. Nothing in the Mouse page's own
        # tests names reachability, and `test_page_tab_cycle` cannot see this
        # because it was rewritten to iterate each category's own page list — it
        # would cheerfully cycle a sidebar the page had been dropped from. What
        # catches it is `test_every_slider_has_a_page_that_draws_it_draggable`,
        # which walks the sidebar and requires every named slider to turn up on
        # some page a user can actually open. That is the better guard: it covers
        # the page added next year as well as this one, so adding a
        # Mouse-specific copy would only be one more thing to keep in step.
        ["test_every_slider_has_a_page_that_draws_it_draggable"],
    ),
    (
        "K: the published double-click range drifts",
        INPUT,
        [("pub const MAX_DOUBLE_CLICK_MS: u32 = 2000;",
          "pub const MAX_DOUBLE_CLICK_MS: u32 = 1500;")],
        ["inputsettings", "settings"],
        ["the_double_click_window_keeps_its_published_range"],
    ),
]


def run_tests(pkg):
    r = subprocess.run(
        ["cargo", "test", "-p", pkg, "--target", TARGET],
        cwd=ROOT, capture_output=True, text=True, errors="replace",
    )
    out = r.stdout + r.stderr
    # `cargo test` reports a *failing test run* as "error: test failed", so a
    # bare "error:" cannot distinguish the two. Only "could not compile" means
    # the patched source did not build.
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
            if not s.startswith(("tests::", "loop_tests::", "hit_tests::")) and "::" not in s:
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
            for old, new in edits:
                if old not in text:
                    verdicts.append((name, "PATTERN NOT FOUND"))
                    text = None
                    break
                text = text.replace(old, new, 1)
            if text is None:
                print(f"{name}: PATTERN NOT FOUND")
                continue
            (ROOT / path).write_text(text, encoding="utf-8", newline="")

            all_failed, note = set(), ""
            broke = False
            for pkg in pkgs:
                failed, out = run_tests(pkg)
                if failed is None:
                    broke = True
                    note = f"{pkg} did not compile"
                    break
                all_failed |= failed
            # Undo before reporting, so an exception in reporting cannot leave
            # the tree patched.
            (ROOT / path).write_bytes(snap[path])

            if broke:
                verdict = f"DID NOT COMPILE ({note})"
            elif not all_failed:
                verdict = "*** NO TEST FAILED ***"
            else:
                named = [t for t in expect if t in all_failed]
                missing = [t for t in expect if t not in all_failed and not t.startswith("*")]
                verdict = f"caught by {len(all_failed)}: {sorted(all_failed)}"
                if missing:
                    verdict += f"  [MISSING: {missing}]"
                elif expect and expect[0].startswith("*"):
                    verdict += "  [unnamed guard]"
                else:
                    verdict += f"  [named: {len(named)}/{len(expect)}]"
            verdicts.append((name, verdict))
            print(f"{name}\n    {verdict}\n", flush=True)
    finally:
        bad = []
        for f in files:
            (ROOT / f).write_bytes(snap[f])
            now = hashlib.sha256((ROOT / f).read_bytes()).hexdigest()
            if now != digest[f]:
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
