"""Mutation test for the speed test's real measurement.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when the app stopped saying it
could not measure and began to: the network run (`net.rs`) and how its
reports move the window (`main.rs`).

Deliberately absent: the list of real servers (`real_servers`), which no test
contacts -- a test that did would reach the internet -- and the `Drop` that
cancels a run when the window closes, which no test can observe without
racing the run's own end. Both are read by eye.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# net.rs
MEASURES = "a_run_measures_what_the_server_moved"
UNREACHABLE = "a_probe_that_cannot_connect_is_reported_failed"
EXACT = "one_request_counts_every_byte"
NO_UPLOAD = "a_server_that_takes_no_uploads_skips_the_phase"
REFUSED = "a_refused_download_fails_the_run_in_the_servers_words"
CANCEL = "a_cancelled_run_stops"
INTERVALS = "the_intervals_add_up_to_the_phase"
PATTERN = "the_upload_bytes_do_not_compress"
# main.rs
PHASES = "the_runs_reports_move_the_window_through_its_phases"
FAILED = "a_failed_run_says_why_and_records_nothing"
LEFTOVERS = "a_cancelled_runs_last_reports_move_nothing"
WHOLE = "a_whole_run_against_a_local_server_is_measured"
CLOCK = "the_clock_is_armed_only_while_a_run_is_under_way"
SUSTAINED = "the_sustained_rate_leaves_out_the_climb"

NET = [
    (
        "a probe that failed is timed anyway",
        "            .map(|_| started.elapsed().as_secs_f64() * 1000.0);",
        "            .map_or(Some(0.0), |_| Some(started.elapsed().as_secs_f64() * 1000.0));",
        [UNREACHABLE],
    ),
    (
        "a server with no upload is posted to",
        "    if endpoint.upload.is_none() {\n        tell(Report::NoUpload);\n    } else if let Err(why)",
        "    if false {\n        tell(Report::NoUpload);\n    } else if let Err(why)",
        [NO_UPLOAD],
    ),
    (
        "a refused file is counted as a download",
        "    if !code.starts_with('2') {",
        "    if code.is_empty() {",
        [REFUSED],
    ),
    (
        "the body's first bytes are not counted",
        "    moved.fetch_add(early.len() as u64, Ordering::Relaxed);",
        "    let _ = early;",
        [EXACT],
    ),
    (
        "a file that ends is read for ever",
        "            Ok(0) => return Ok(()),",
        "            Ok(0) => return Err(String::from(\"ended\")),",
        [EXACT],
    ),
    (
        "the upload stops at the first write",
        "    while sent < UPLOAD_BODY {",
        "    while sent == 0 {",
        [EXACT],
    ),
    (
        "the report counts everything again each time",
        "                bytes: total.saturating_sub(reported),",
        "                bytes: total,",
        [MEASURES],
    ),
    (
        "the intervals are not measured",
        "                over: now.saturating_duration_since(last),",
        "                over: SAMPLE_EVERY,",
        [INTERVALS],
    ),
    (
        "a download that moved nothing succeeds",
        "    if moved.load(Ordering::Relaxed) == 0 && !cancel.load(Ordering::Relaxed) {",
        "    if false {",
        [REFUSED],
    ),
    (
        "the Host header drops the port",
        '        format!("{}:{}", endpoint.host, endpoint.port)',
        "        endpoint.host.clone()",
        [MEASURES],
    ),
    (
        "the upload bytes are all one value",
        "            state ^= state.wrapping_shl(13);",
        "            state = 0;",
        [PATTERN],
    ),
    (
        "a cancel does not stop the transfer",
        "    let stop = || cancel.load(Ordering::Relaxed) || Instant::now() >= deadline;",
        "    let stop = || Instant::now() >= deadline;",
        [CANCEL],
    ),
]

MAIN = [
    (
        "a failed probe is not counted",
        "            net::Report::Probe(None) => self.latency_tester.record_loss(),",
        "            net::Report::Probe(None) => {}",
        [PHASES],
    ),
    (
        "the rate ignores the time it was counted over",
        "                    bytes as f64 * 8.0 / secs / 1_000_000.0",
        "                    bytes as f64 * 8.0 / 1_000_000.0",
        [PHASES],
    ),
    (
        "an unmeasured upload is given a figure",
        "            net::Report::NoUpload => self.upload_skipped = true,",
        "            net::Report::NoUpload => {}",
        [PHASES],
    ),
    (
        "a cancelled run's reports still move the window",
        "        let SpeedTestPhase::Testing(kind) = self.phase else {\n            return;\n        };\n        match report {",
        "        let kind = match self.phase {\n            SpeedTestPhase::Testing(kind) => kind,\n            _ => TestKind::Download,\n        };\n        match report {",
        [LEFTOVERS],
    ),
    (
        "Escape leaves the run going",
        "                    self.cancel_test();",
        "                    self.phase = SpeedTestPhase::Idle;",
        [CLOCK],
    ),
    (
        "the ramp is counted in the figure",
        "            .filter(|s| s.elapsed_secs > from)",
        "            .filter(|s| s.elapsed_secs >= 0.0)",
        [SUSTAINED],
    ),
    (
        'a chord raises the list of keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_not_a_speed_test_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '            if plain && matches!(key.key, Key::Escape | Key::Enter) {',
        '            if matches!(key.key, Key::Escape | Key::Enter) {',
        ['a_chord_is_not_a_speed_test_key_and_altgr_is_not_ctrl'],
    ),
    (
        'AltGr is taken for Ctrl in the window',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {\n                Key::E => {',
        '        if key.modifiers.ctrl {\n            return match key.key {\n                Key::E => {',
        ['a_chord_is_not_a_speed_test_key_and_altgr_is_not_ctrl'],
    ),
    (
        "a chord works the window's keys",
        '        if !plain {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_chord_is_not_a_speed_test_key_and_altgr_is_not_ctrl'],
    ),
    (
        'AltGr+Q closes the window',
        '            && textline::is_ctrl_chord(key.modifiers)\n        {',
        '            && key.modifiers.ctrl\n        {',
        ['a_chord_is_not_a_speed_test_key_and_altgr_is_not_ctrl'],
    ),
]

# The list of keys takes the pointer as well as the keys (2026-10-04,
# known-issues/E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it.md).
CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

MAIN += [
    (
        "a press goes through the list of keys",
        "            Event::Mouse(mouse_event) if self.show_help => match mouse_event.kind {\n",
        "            Event::Mouse(mouse_event) if false => match mouse_event.kind {\n",
        [CARD],
    ),
    (
        "only the left button puts the list away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the list covers",
        "            Event::Mouse(mouse_event) if self.show_help => match mouse_event.kind {\n",
        "            Event::Mouse(mouse_event) if self.show_help && !matches!(mouse_event.kind, MouseEventKind::Scroll { .. }) => match mouse_event.kind {\n",
        [CARD],
    ),
    (
        "a row under the list is lit",
        "                _ => self.set_hover(None),\n            },\n",
        "                MouseEventKind::Move => self.handle_mouse_move(mouse_event.x, mouse_event.y),\n"
        "                _ => self.set_hover(None),\n            },\n",
        [CARD],
    ),
]

TABLES = {
    "net.rs": NET,
    "main.rs": MAIN,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "speedtest", timeout=900, only=mine))
    raise SystemExit(worst)
