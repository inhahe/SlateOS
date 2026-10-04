"""Mutation test for the network scanner's real scanning, WHOIS and Wake-on-LAN.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when the scanner stopped refusing
every operation for want of a network and began to make connections: the
connect-scan engine (`engine.rs`), how its reports become the result the window
shows (`main.rs`), and the WHOIS conversation (`whois.rs`).

Deliberately absent: which open ports `engine::tcp` reads a greeting from.
Testing it needs a listener on one of those ports (21, 22, ...), which a test
cannot count on binding; `read_banner` itself is tested, and the port list is
a table read by eye.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# engine.rs
PROBE = "a_listening_port_is_open_and_a_quiet_one_closed"
BANNER = "a_banner_is_one_line_of_printable_text"
ORDER = "probes_go_across_the_hosts_before_the_next_port"
ONCE = "every_probe_is_reported_once_and_the_scan_ends"
CANCEL = "a_cancelled_scan_stops_early"
# main.rs
ANSWERED = "a_scan_reports_what_answered_and_nothing_else"
NOTHING = "test_app_start_scan"
TOO_BIG = "a_scan_too_big_to_finish_is_refused"
GENTLE = "the_stealth_profile_is_one_connection_at_a_time"
F5 = "test_app_key_f5_starts_scan"
WOL = "wake_on_lan_sends_the_magic_packet"
# whois.rs
FIELDS = "a_record_is_read_whichever_registry_wrote_it"
REFER = "iana_is_asked_and_its_referral_followed"
LATIN1 = "a_reply_in_latin1_loses_nothing"
PORTS = "a_server_names_its_port_or_is_on_43"

ENGINE = [
    (
        "a refusal is read as no answer",
        "        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => Probe::Closed {",
        "        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Probe::Closed {",
        [PROBE],
    ),
    (
        "a banner keeps bytes that are not printable",
        "        .filter(|b| b.is_ascii_graphic() || **b == b' ')",
        "        .filter(|_| true)",
        [BANNER],
    ),
    (
        "a banner is not cut short",
        "        .take(BANNER_MAX)",
        "        .take(usize::MAX)",
        [BANNER],
    ),
    (
        "probes go port by port on one host",
        "        let host = *self.hosts.get(index.checked_rem(hosts)?)?;\n"
        "        let port = *self.ports.get(index.checked_div(hosts)?)?;",
        "        let ports = self.ports.len().max(1);\n"
        "        let host = *self.hosts.get(index.checked_div(ports)?)?;\n"
        "        let port = *self.ports.get(index.checked_rem(ports)?)?;",
        [ORDER],
    ),
    (
        "a cancelled scan keeps going",
        "        if shared.cancel.load(Ordering::Relaxed) {\n            break;\n        }",
        "        if false {\n            break;\n        }",
        [CANCEL],
    ),
    (
        "every other probe is skipped",
        "        let index = shared.next.fetch_add(1, Ordering::Relaxed);",
        "        let index = shared.next.fetch_add(2, Ordering::Relaxed);",
        [ONCE],
    ),
]

MAIN = [
    (
        "an address that never answered is listed as a host",
        "            engine::Probe::Silent => return,",
        "            engine::Probe::Silent => (PortState::Filtered, 0.0, None),",
        [ANSWERED],
    ),
    (
        "a host's latency is its slowest answer",
        "        host.latency_ms = host.latency_ms.min(ms);",
        "        host.latency_ms = host.latency_ms.max(ms);",
        [ANSWERED],
    ),
    (
        "a scan too large to finish is started anyway",
        "        if probes > MAX_PROBES {",
        "        if false {",
        [TOO_BIG],
    ),
    (
        "the gentle profile opens many connections at once",
        "            workers: if gentle {\n                1",
        "            workers: if gentle {\n                8",
        [GENTLE],
    ),
    (
        "stopping does not stop",
        "            scan.cancel();",
        "            let _ = scan;",
        [F5],
    ),
    (
        "silence is reported as nothing being there",
        "{silent} connection(s) had no answer, which is not the same as nothing being there\"",
        "{silent} connection(s) had no answer\"",
        [NOTHING],
    ),
    (
        "a finished scan is not filed",
        "        self.history.push_front(result.clone());",
        "        let _ = result;",
        [NOTHING],
    ),
    (
        "the wake-up packet goes somewhere else",
        "            socket.send_to(&packet, self.wol_destination)",
        "            socket.send_to(&packet, DEFAULT_WOL)",
        [WOL],
    ),
    # 2026-09-27: the target is this machine's network, never an invented one.
    (
        "a mask with a hole names a range",
        "    if mask.checked_shl(prefix).unwrap_or(0) != 0 || prefix == 0 {",
        "    if prefix == 0 {",
        ["a_subnet_is_the_address_under_its_mask"],
    ),
    (
        "the window opens on no network even when it can read one",
        "        if let Some(net) = machine_subnet(provider) {",
        "        if let Some(net) = None::<String> {",
        ["the_target_is_this_machines_network"],
    ),
]

WHOIS = [
    (
        "IANA's referral is not followed",
        "        Some(server) if server != iana => {",
        "        Some(server) if false => {",
        [REFER],
    ),
    (
        "a Latin-1 reply is decoded lossily",
        "        .unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect())",
        "        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())",
        [LATIN1],
    ),
    (
        "a server's port is ignored",
        "        Some((host, port)) => port.parse().map_or((server, PORT), |port| (host, port)),",
        "        Some(_) => (server, PORT),",
        [PORTS],
    ),
    (
        "RIPE's organisation field is not read",
        '            "OrgName",\n            "org-name",\n',
        '            "OrgName",\n',
        [FIELDS],
    ),
]

# The five text boxes, where things are pressed, and the list of keys
# (2026-10-04). None of the boxes took a key; the Send WOL, Run and profile
# buttons and the overview's headings were drawn somewhere other than where a
# press looked for them; F1 did nothing.
BOXES = "every_box_takes_typing_and_enter_does_what_it_is_for"
CARET = "a_box_edits_at_a_caret_and_knows_a_chord_from_altgr"
CARET_AT = "a_press_puts_the_caret_where_it_lands"
MARKED = "only_the_box_with_the_keyboard_is_marked"
WALK = "tab_walks_the_boxes_on_screen"
LEAVES = "a_box_that_leaves_the_screen_lets_go_of_the_keyboard"
NEVER_STOPS = "enter_in_the_target_box_never_stops_a_scan"
WAKE = "wake_on_lan_is_drawn_in_the_sidebar_and_pressed_there"
RUN = "the_run_buttons_are_pressed_where_they_are_drawn"
PROFILE = "the_profile_buttons_are_pressed_where_they_are_drawn"
EXPORT = "export_is_pressed_only_where_it_is_drawn"
REACHES = "the_shortcut_list_reaches_the_window"
QUESTION = "a_question_mark_is_typed_into_a_box_and_f1_still_raises_the_list"
MODAL = "the_shortcut_list_takes_the_keys_and_a_press"
CHORD = "a_chord_is_not_one_of_this_windows_keys"

HELP_ANCHOR = "        if plain && (key.key == Key::F1 || question && self.focus.is_none()) {\n"
CLOSE_ANCHOR = "            if plain && (matches!(key.key, Key::F1 | Key::Escape) || question) {\n"
SCAN_ANCHOR = "        if (plain && key.key == Key::F5) || (ctrl && key.key == Key::Enter) {\n"
TAB_ANCHOR = "            self.step_focus(!key.modifiers.shift);\n"
PRESS_ANCHOR = (
    "                    self.show_help = false;\n"
    "                    return EventResult::Consumed;\n"
    "                }\n"
    "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n"
)

MAIN += [
    (
        "a press on a box does not take the keyboard",
        "                    self.press_field(field, rect, mx);\n",
        "                    let _ = (field, rect);\n",
        [BOXES],
    ),
    (
        "a press elsewhere leaves the keyboard in its box",
        "                }\n                self.focus = None;\n\n                // Check tab clicks\n",
        "                }\n\n                // Check tab clicks\n",
        [MARKED],
    ),
    (
        "a key typed into a box goes nowhere",
        "            *self.field_text_mut(field) = typed;\n",
        "            let _ = typed;\n",
        [BOXES],
    ),
    (
        "a box's editor is not reloaded from the box",
        "        if self.editor.text() != self.field_text(field) {\n"
        "            let text = self.field_text(field).to_owned();\n"
        "            self.editor.set_text(&text);\n"
        "        }\n",
        "        let _ = field;\n",
        [CARET],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - FIELD_TEXT_INSET,\n",
        "            0.0,\n",
        [CARET_AT],
    ),
    (
        "an empty box with the keyboard has no caret",
        "            if focused {\n                textedit::push_caret(",
        "            if false {\n                textedit::push_caret(",
        [CARET_AT],
    ),
    (
        "an empty box does not say what it is for",
        "                text: field.placeholder().to_owned(),\n",
        "                text: String::new(),\n",
        [CARET_AT],
    ),
    (
        "Enter does not do what the box is for",
        "                    self.submit(field);\n",
        "                    let _ = field;\n",
        [BOXES],
    ),
    (
        "Escape does not leave the box",
        "                Key::Escape => {\n"
        "                    self.focus = None;\n"
        "                    return EventResult::Consumed;\n"
        "                }\n"
        "                Key::Enter => {\n",
        "                Key::Escape => {\n"
        "                    return EventResult::Consumed;\n"
        "                }\n"
        "                Key::Enter => {\n",
        [BOXES],
    ),
    (
        "Enter in the target box stops a scan under way",
        "                if !self.is_scanning {\n"
        "                    self.start_scan();\n"
        "                }\n",
        "                self.start_scan();\n",
        [NEVER_STOPS],
    ),
    (
        "a box that would be refused is not red",
        "                invalid: self.field_is_wrong(field),\n",
        "                invalid: false,\n",
        [BOXES],
    ),
    (
        "an empty box is red",
        "        !text.trim().is_empty()\n            && match field {\n",
        "        match field {\n",
        [MARKED],
    ),
    (
        "a target that is not one is not red",
        "                Field::Target => ScanTarget::parse(text).is_none(),\n",
        "                Field::Target => false,\n",
        [BOXES],
    ),
    (
        "ports that are not ports are not red",
        "                Field::Ports => parse_port_spec(text).is_none(),\n",
        "                Field::Ports => false,\n",
        [BOXES],
    ),
    (
        "an address that is not one is not red",
        "                Field::Trace | Field::Whois => Ipv4Addr::parse(text).is_none(),\n",
        "                Field::Trace | Field::Whois => false,\n",
        [BOXES],
    ),
    (
        "a MAC address that is not one is not red",
        "                Field::WakeMac => parse_mac(text).is_none(),\n",
        "                Field::WakeMac => false,\n",
        [BOXES],
    ),
    (
        "the box's mark is drawn under the list of keys",
        "        let focused = self.focus == Some(field) && !self.show_help;\n",
        "        let focused = self.focus == Some(field);\n",
        [QUESTION],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [MARKED],
    ),
    (
        "Tab does not walk the boxes",
        TAB_ANCHOR,
        "            let _ = key;\n",
        [WALK],
    ),
    (
        "Shift+Tab walks forward",
        TAB_ANCHOR,
        "            self.step_focus(true);\n",
        [WALK],
    ),
    (
        "Tab does not wrap past the last box",
        "            (Some(i), true) => fields.get(i.saturating_add(1)).or(fields.first()),\n",
        "            (Some(i), true) => fields.get(i.saturating_add(1)),\n",
        [WALK],
    ),
    (
        "Shift+Tab does not wrap past the first box",
        "                .and_then(|j| fields.get(j))\n                .or(fields.last()),\n",
        "                .and_then(|j| fields.get(j)),\n",
        [WALK],
    ),
    (
        "a box that leaves the screen keeps the keyboard",
        "        if self.focus.is_some_and(|f| self.field_rect(f).is_none()) {\n",
        "        if false {\n",
        [LEAVES],
    ),
    (
        "the ports box is up for every profile",
        "            Field::Ports => (self.config.profile == ScanProfile::Custom)\n",
        "            Field::Ports => (true)\n",
        [WALK],
    ),
    (
        "the overview is up under a selected host",
        "        if self.active_tab != ViewTab::Results || self.selected_host().is_some() {\n",
        "        if self.active_tab != ViewTab::Results {\n",
        # Not LEAVES: under the keyboard the box leaves the screen only
        # by Ctrl+Tab, and a press elsewhere takes the keyboard itself.
        [EXPORT],
    ),
    (
        "Send is looked for over the MAC box",
        "                if overview.is_some_and(|o| o.wake_button.contains(mx, my)) {\n",
        "                if overview.is_some_and(|o| o.wake_field.contains(mx, my)) {\n",
        [WAKE],
    ),
    (
        "Send is drawn at the window's left edge",
        "            x: b.x,\n            y: b.y,\n",
        "            x: PADDING,\n            y: b.y,\n",
        [WAKE],
    ),
    (
        "the overview's heading is drawn at the window's left edge",
        "            x: o.x,\n            y: top_y + PADDING,\n",
        "            x: PADDING,\n            y: top_y + PADDING,\n",
        [WAKE],
    ),
    (
        "a press on the open Export menu reaches what is under it",
        "                    .is_some_and(|m| m.contains(mx, my))\n",
        "                    .is_some_and(|_| false)\n",
        [EXPORT],
    ),
    (
        "a Run button's press is looked for 18 pixels high",
        "                    && RUN_BUTTON.contains(mx, my)\n",
        "                    && RUN_BUTTON.contains(mx, my + 18.0)\n",
        [RUN],
    ),
    (
        "the traceroute's Run button is drawn away from its press",
        "        self.render_field(tree, Field::Trace);\n\n"
        "        // Run button, where a press looks for it.\n"
        "        let run = RUN_BUTTON;\n",
        "        self.render_field(tree, Field::Trace);\n\n"
        "        // Run button, where a press looks for it.\n"
        "        let run = Rect::new(PADDING, 0.0, 120.0, BUTTON_HEIGHT);\n",
        [RUN],
    ),
    (
        "the profile buttons' press is looked for 16 pixels low",
        "                let profile_y = PROFILE_Y;\n",
        "                let profile_y = PROFILE_Y + 16.0;\n",
        [PROFILE],
    ),
    (
        "the list of keys never comes up",
        "            self.show_help = true;\n            return EventResult::Consumed;\n",
        "            return EventResult::Consumed;\n",
        [REACHES],
    ),
    (
        "F1 raises nothing from a box",
        HELP_ANCHOR,
        "        if plain && self.focus.is_none() && (key.key == Key::F1 || question) {\n",
        [QUESTION],
    ),
    (
        "? raises the list from a box",
        HELP_ANCHOR,
        "        if plain && (key.key == Key::F1 || question) {\n",
        [QUESTION],
    ),
    (
        "Alt+F1 raises the list",
        HELP_ANCHOR,
        "        if key.key == Key::F1 || plain && question && self.focus.is_none() {\n",
        [REACHES],
    ),
    (
        "the list is not modal for the keys",
        "                self.show_help = false;\n"
        "            }\n"
        "            return EventResult::Consumed;\n"
        "        }\n",
        "                self.show_help = false;\n"
        "            }\n"
        "        }\n",
        [MODAL],
    ),
    (
        "Escape leaves the list up",
        CLOSE_ANCHOR,
        "            if plain && (matches!(key.key, Key::F1) || question) {\n",
        [REACHES],
    ),
    (
        "Alt+Escape puts the list away",
        CLOSE_ANCHOR,
        "            if matches!(key.key, Key::F1 | Key::Escape) || question {\n",
        [REACHES],
    ),
    (
        "the list of keys is not drawn",
        "        if self.show_help {\n            guitk::shortcut::render_card(\n",
        "        if false {\n            guitk::shortcut::render_card(\n",
        [REACHES],
    ),
    (
        "a press reaches what the list covers",
        PRESS_ANCHOR,
        "                    self.show_help = false;\n"
        "                }\n"
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        [MODAL],
    ),
    (
        "a press leaves the list up",
        PRESS_ANCHOR,
        "                    return EventResult::Consumed;\n"
        "                }\n"
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        [MODAL],
    ),
    (
        "the wheel scrolls what the list covers",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "                MouseEventKind::Scroll { .. } => {}\n",
        [MODAL],
    ),
    (
        "F5 is taken with Alt held",
        SCAN_ANCHOR,
        "        if key.key == Key::F5 || (ctrl && key.key == Key::Enter) {\n",
        [CHORD],
    ),
    (
        "AltGr+Enter is Ctrl+Enter",
        SCAN_ANCHOR,
        "        if (plain && key.key == Key::F5) || (key.modifiers.ctrl && key.key == Key::Enter) {\n",
        [CHORD],
    ),
    (
        "a list key is taken with Alt held",
        "        if !plain {\n            return EventResult::Ignored;\n        }\n        match key.key {\n",
        "        match key.key {\n",
        [CHORD],
    ),
    (
        "Ctrl+Tab does not change the view",
        "            self.next_view();\n",
        "            let _ = ctrl;\n",
        [LEAVES],
    ),
]

TABLES = {
    "engine.rs": ENGINE,
    "main.rs": MAIN,
    "whois.rs": WHOIS,
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
        worst = max(worst, sweep(SRC / file, rows, "netscan", timeout=900, only=mine))
    raise SystemExit(worst)
