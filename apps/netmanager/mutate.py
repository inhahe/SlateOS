"""Mutation test for the Network Manager's reading of the machine.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when the window stopped saying it
could not see the network and began listing the interfaces the kernel
publishes (through `hwquery`, from SlateOS's `/proc/net`): the read and what
it records, the two kinds of absent address, the banner that explains an
empty list, the status bar's count, and the Wi-Fi and VPN tabs' empty states;
and, the same day, Apply, Enable and Disable reaching the kernel through
`SYS_NET_IF_CONFIG` -- the record, the refusals in the kernel's own codes, and
the re-read that makes what is shown the kernel's word.

Deliberately absent: the call to `read_interfaces` in `main()`, and the
`cfg(not(test))` half of `machine()`.  Neither runs under `cargo test` -- the
one is the program's entry point, the other is compiled out of the test build
-- so no test can notice them broken; they are one line each, read by eye.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

BEFORE = "before_a_read_the_window_says_nothing_was_read"
LISTED = "the_kernels_interface_is_listed_as_it_is_published"
UNCONFIGURED = "an_unconfigured_card_says_it_has_no_address"
NO_CARD = "a_machine_with_no_card_says_none_was_found"
UNREADABLE = "nothing_readable_is_said_with_why"
REFRESH = "refresh_reads_the_interfaces_again"
TABS = "the_empty_tabs_say_nothing_was_examined"
RECORD = "a_change_is_the_kernels_record"
APPLY = "apply_sends_the_configuration_to_the_kernel"
APPLY_REFUSED = "a_refused_apply_says_it_needs_an_administrator"
NO_DHCP = "apply_does_not_pretend_to_switch_on_dhcp"
SWITCH = "the_switch_asks_the_kernel_and_shows_its_answer"
SWITCH_REFUSED = "a_refused_switch_stays_put"
UNKNOWN_STATE = "an_interface_of_unknown_state_is_not_switched"
HOST_SWITCH = "test_toggle_enabled"
DIAG_RUN = "test_run_diagnostics"
DIAG_OK = "diagnose_checks_the_card_and_reaches_a_server"
DIAG_NO_ADDRESS = "diagnose_names_a_card_with_no_address"

MAIN = [
    (
        "Refresh does not read the interfaces",
        "                self.read_interfaces();\n"
        "                let interfaces = std::mem::take(&mut self.status_message);",
        "                let interfaces = std::mem::take(&mut self.status_message);",
        [REFRESH],
    ),
    (
        "a failed read keeps the old rows",
        "                self.interfaces.clear();\n",
        "",
        [UNREADABLE, REFRESH],
    ),
    (
        "a read is not recorded as one",
        "                self.listing = Listing::Read;",
        "                self.listing = Listing::NotRead;",
        [LISTED, NO_CARD],
    ),
    (
        "an unassigned address reads as not reported",
        "        hwquery::Address::Unassigned => String::from(NOT_ASSIGNED),",
        "        hwquery::Address::Unassigned => String::from(NOT_REPORTED),",
        [UNCONFIGURED],
    ),
    (
        "0.0.0.0 goes into the editor",
        "        hwquery::Address::Unassigned | hwquery::Address::NotReported => String::new(),",
        '        hwquery::Address::Unassigned => String::from("0.0.0.0"),\n'
        "        hwquery::Address::NotReported => String::new(),",
        [UNCONFIGURED],
    ),
    (
        "an interface's kind is not read from its name",
        '    let interface_type = if a.name.starts_with("eth") || a.name.starts_with("en") {',
        '    let interface_type = if a.name.starts_with("en") {',
        [LISTED],
    ),
    (
        "a link that is down is called up",
        "            Some(false) => ConnectionState::Disconnected,",
        "            Some(false) => ConnectionState::Connected,",
        [UNCONFIGURED],
    ),
    (
        "DHCP is guessed",
        "        dhcp: None,\n        reported: Some(ReportedAddresses {",
        "        dhcp: Some(true),\n        reported: Some(ReportedAddresses {",
        [LISTED],
    ),
    (
        "the summary shows the editor's blank",
        "            .map_or(&self.ip_config.ip_address, |r| &r.ip_address)",
        "            .map_or(&self.ip_config.ip_address, |_| &self.ip_config.ip_address)",
        [UNCONFIGURED],
    ),
    (
        "the banner covers a listed interface",
        "    if !app.interfaces.is_empty() {\n        return None;\n    }\n",
        "",
        [LISTED],
    ),
    (
        "before a read, the empty list is not explained",
        '            String::from("Press F5 to read them."),\n'
        "            String::from(NOT_READ_LINE),",
        '            String::from("Press F5 to read them."),\n'
        "            String::new(),",
        [BEFORE],
    ),
    (
        "a failed read's reason is not in the banner",
        '            format!("Could not read the interfaces: {why}."),',
        '            String::from("Could not read the interfaces."),',
        [UNREADABLE],
    ),
    (
        "no card is reported as nothing read",
        '            String::from("No network card was found."),',
        '            String::from("No network interface has been read."),',
        [NO_CARD],
    ),
    (
        "no card is counted as unknown",
        '        (Listing::Read, 0) => String::from("no interfaces"),',
        '        (Listing::Read, 0) => String::from("interfaces unknown"),',
        [NO_CARD],
    ),
    (
        "one interface is counted in the plural",
        '        (Listing::Read, 1) => String::from("1 interface"),\n',
        "",
        [LISTED],
    ),
    (
        "an unread list is counted",
        '        (Listing::NotRead | Listing::Unreadable(_), _) => String::from("interfaces unknown"),',
        '        (Listing::NotRead | Listing::Unreadable(_), n) => format!("{n} interfaces"),',
        [BEFORE, UNREADABLE],
    ),
    (
        "the empty Wi-Fi tab claims a scan found nothing",
        '            text: "No Wi-Fi network is listed: nothing here can scan for one.".into(),',
        '            text: "No WiFi networks found. Click Refresh to scan.".into(),',
        [TABS],
    ),
    (
        "the empty VPN tab claims none is configured",
        '            text: "No VPN connection is listed: nothing here can read one.".into(),',
        '            text: "No VPN connections configured".into(),',
        [TABS],
    ),
    (
        "the record leaves out the DNS server's bit",
        "            | bit(self.dns.is_some(), config_mask::DNS)\n",
        "",
        [RECORD],
    ),
    (
        "down is sent as up",
        "            u8::from(self.up == Some(true)),",
        "            u8::from(self.up.is_some()),",
        [RECORD],
    ),
    (
        "a refusal is taken for success",
        "    if ret < 0 { Err(ret) } else { Ok(()) }",
        "    let _ = ret;\n    Ok(())",
        [HOST_SWITCH],
    ),
    (
        "permission denied is Linux's -1",
        "const KERNEL_PERMISSION_DENIED: i64 = -400;",
        "const KERNEL_PERMISSION_DENIED: i64 = -1;",
        [APPLY_REFUSED, SWITCH_REFUSED],
    ),
    (
        "DHCP is sent as a static configuration",
        "        if config.dhcp_enabled == Some(true) {\n            // Two refusals",
        "        if false {\n            // Two refusals",
        [NO_DHCP],
    ),
    (
        "a cleared gateway is not sent as none",
        "            gateway: Some(if config.gateway.is_empty() {",
        "            gateway: Some(if config.gateway.is_empty() && false {",
        [APPLY],
    ),
    (
        "the last DNS server is sent",
        "            dns: Some(match config.dns_servers.first() {",
        "            dns: Some(match config.dns_servers.last() {",
        [APPLY],
    ),
    (
        "the DNS servers not sent are not mentioned",
        "        let dropped = config.dns_servers.len().saturating_sub(1);",
        "        let dropped = 0_usize;",
        [APPLY],
    ),
    (
        "Apply does not read the list again",
        "        self.editing_ip = false;\n        self.read_interfaces();\n",
        "        self.editing_ip = false;\n",
        [APPLY],
    ),
    (
        "the switch does not read the list again",
        "                self.read_interfaces();\n                self.status_message = done;",
        "                self.status_message = done;",
        [SWITCH],
    ),
    (
        "an interface that is up is brought up",
        "            ConnectionState::Connected => false,",
        "            ConnectionState::Connected => true,",
        [SWITCH, SWITCH_REFUSED, HOST_SWITCH],
    ),
    (
        "an interface of unknown state is switched anyway",
        "                );\n                return;\n            }\n        };\n        let (act, done) = if bring_up {",
        "                );\n                true\n            }\n        };\n        let (act, done) = if bring_up {",
        [UNKNOWN_STATE],
    ),
    (
        "the switch ignores the kernel's up/down",
        "        enabled: a.up != Some(false),",
        "        enabled: true,",
        [SWITCH],
    ),
    (
        "a card that is down passes",
        '                    "Network card",\n'
        "                    DiagnosticStatus::Failed,\n"
        '                    format!("{} is down", i.name),',
        '                    "Network card",\n'
        "                    DiagnosticStatus::Passed,\n"
        '                    format!("{} is down", i.name),',
        [DIAG_NO_ADDRESS],
    ),
    (
        "no address passes",
        '                "Address",\n                DiagnosticStatus::Failed,',
        '                "Address",\n                DiagnosticStatus::Passed,',
        [DIAG_NO_ADDRESS],
    ),
    (
        "a connection made is reported as failed",
        '                        "Connection",\n                        DiagnosticStatus::Passed,',
        '                        "Connection",\n                        DiagnosticStatus::Failed,',
        [DIAG_OK],
    ),
    (
        "the Running rows are left beside the results",
        "            .retain(|d| d.status != DiagnosticStatus::Running);",
        "            .retain(|_| true);",
        [DIAG_OK, DIAG_RUN],
    ),
    (
        "the checks are never marked under way",
        "        self.diagnostics_running = true;",
        "        self.diagnostics_running = false;",
        [DIAG_RUN],
    ),
]

# The boxes are the toolkit's field, edited by the toolkit's editor; the keys
# are plain keys; the shortcut card takes the pointer (2026-10-04; lane C,
# c-e-a-theme-can-shape-the-controls).
DNS_FIELD = "the_dns_box_is_the_toolkits_field"
CHORDS = "a_chord_is_not_typed_into_a_box_and_the_caret_keys_edit_it"
WINDOW_KEYS = "a_chord_is_none_of_the_windows_keys"
PRESS = "a_press_in_a_box_puts_the_caret_under_the_pointer"
DNS_TAB = "tab_in_the_dns_box_keeps_the_keyboard_on_its_tab"
DHCP = "under_dhcp_the_address_boxes_are_disabled_and_take_no_keyboard"
RED = "an_address_box_is_red_while_it_is_not_an_address"
CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"
WALK = "tab_walks_the_ip_fields_and_typing_lands_in_the_focused_one"
CARET = "a_focused_field_shows_a_caret_so_the_keyboard_has_somewhere_visible_to_go"
ADD = "the_dns_box_takes_typing_and_add_moves_it_into_the_list"

MAIN += [
    (
        "a chord raises the card",
        "        if key.key == Key::F1 && plain {\n",
        "        if key.key == Key::F1 {\n",
        [WINDOW_KEYS],
    ),
    (
        "a chord works the window's keys",
        "        if !plain {\n            return Action::None;\n        }\n\n        match key.key {\n",
        "        match key.key {\n",
        [WINDOW_KEYS],
    ),
    (
        # What the box did before the editor: type whatever text a key
        # carried, a chord's letter among it.
        "a box types a chord's letter",
        "            _ => self.edit_field(key, field),\n",
        "            _ => {\n"
        "                let typed: String = key.typed().collect();\n"
        "                if typed.is_empty() {\n"
        "                    return self.edit_field(key, field);\n"
        "                }\n"
        "                self.field_mut(field).push_str(&typed);\n"
        "                Action::Redraw\n"
        "            }\n",
        [CHORDS],
    ),
    (
        "Shift+Tab walks forwards",
        "                let next = Self::next_field(field, key.modifiers.shift);\n",
        "                let next = Self::next_field(field, false);\n",
        [WALK],
    ),
    (
        "Tab leaves the DNS box for a box on another tab",
        "            (Field::DnsInput, _) => Field::DnsInput,\n",
        "            (Field::DnsInput, _) => Field::Ip,\n",
        [DNS_TAB],
    ),
    (
        "a press leaves the caret at the end",
        "        self.editor.set_selection_anchor(None);\n        self.editor.set_cursor(cursor);\n",
        "        self.editor.set_selection_anchor(None);\n",
        [PRESS],
    ),
    (
        "the editor keeps text the box no longer holds",
        "        if self.editor.text() != self.field_text(field) {\n            self.load_editor(field);\n",
        "        if false {\n            self.load_editor(field);\n",
        [ADD],
    ),
    (
        "a box is never lit",
        "            hovered: enabled && self.hover == Some(field),\n",
        "            hovered: false,\n",
        [DNS_FIELD],
    ),
    (
        "the pointer leaving leaves the box lit",
        "                MouseEventKind::Leave => self.set_hover(None),\n",
        "",
        [DNS_FIELD],
    ),
    (
        "a box never has the keyboard's mark",
        "            focused: enabled && self.focus == Some(field) && !self.show_help,\n",
        "            focused: false,\n",
        [DNS_FIELD],
    ),
    (
        "a box keeps its mark under the card",
        "            focused: enabled && self.focus == Some(field) && !self.show_help,\n",
        "            focused: enabled && self.focus == Some(field),\n",
        [DNS_FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [DNS_FIELD],
    ),
    (
        "nothing in a box is red",
        "            invalid: enabled && !text.is_empty() && refused,\n",
        "            invalid: false,\n",
        [DNS_FIELD, RED],
    ),
    (
        "an empty box is red",
        "            invalid: enabled && !text.is_empty() && refused,\n",
        "            invalid: enabled && refused,\n",
        [RED],
    ),
    (
        "a server already listed is not red",
        "        let refused = !is_valid_ipv4(text)\n"
        "            || (field == Field::DnsInput\n"
        "                && self.edit_ip_config.dns_servers.iter().any(|s| s == text));\n",
        "        let refused = !is_valid_ipv4(text);\n",
        [DNS_FIELD],
    ),
    (
        "the box is drawn with no caret",
        "            focused: state.focused,\n            x: tx,\n",
        "            focused: false,\n            x: tx,\n",
        [CARET],
    ),
    (
        "an address box under DHCP takes the keyboard",
        "                self.editing_ip && self.edit_ip_config.dhcp_enabled != Some(true)\n",
        "                self.editing_ip\n",
        [DHCP],
    ),
    (
        "an address box under DHCP is not drawn disabled",
        "            disabled: !enabled,\n",
        "            disabled: false,\n",
        [DHCP],
    ),
    (
        "a disabled box is a target",
        "        if app.field_enabled(field) {\n            frame.hit(Target::Focus(field), rect);\n        }\n",
        "        frame.hit(Target::Focus(field), rect);\n",
        [DHCP],
    ),
    (
        "Edit gives the keyboard to a box DHCP fills in",
        "                self.focus = None;\n                self.focus_field(Field::Ip);\n",
        "                self.focus = Some(Field::Ip);\n",
        [DHCP],
    ),
    (
        "switching DHCP on leaves the keyboard in a disabled box",
        "                if self.focus.is_some_and(|field| !self.field_enabled(field)) {\n"
        "                    self.focus = None;\n"
        "                }\n",
        "",
        [DHCP],
    ),
    (
        "a press goes through the shortcut card",
        "            Event::Mouse(mouse) if self.show_help => match mouse.kind {\n",
        "            Event::Mouse(mouse) if false => match mouse.kind {\n",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        "            Event::Mouse(mouse) if self.show_help => match mouse.kind {\n",
        "            Event::Mouse(mouse) if self.show_help && !matches!(mouse.kind, MouseEventKind::Scroll { .. }) => match mouse.kind {\n",
        [CARD],
    ),
]

# A change to the DNS list is an edit: it opens the editor, whose Apply and
# Cancel are on the DNS tab too; under DHCP the list cannot be changed; a
# re-read keeps an open edit (2026-10-04).
DNS_LOCKED = "under_dhcp_the_dns_list_cannot_be_changed_and_says_why"
DNS_EDIT = "a_dns_change_opens_the_editor_and_the_dns_tab_can_apply_it"
REREAD = "a_reread_keeps_an_open_edit_while_its_interface_is_listed"
REFUSAL = "the_dhcp_refusal_says_whether_the_switch_was_moved"

MAIN += [
    (
        "an added server leaves the editor closed",
        "        self.begin_edit();\n        self.edit_ip_config.dns_servers.push(server.to_string());\n",
        "        self.edit_ip_config.dns_servers.push(server.to_string());\n",
        [DNS_EDIT],
    ),
    (
        "a removal leaves the editor closed",
        "        self.begin_edit();\n        self.edit_ip_config.dns_servers.remove(index);\n",
        "        self.edit_ip_config.dns_servers.remove(index);\n",
        [DNS_EDIT],
    ),
    (
        "a move up leaves the editor closed",
        "        self.begin_edit();\n        // `index` is known non-zero above",
        "        // `index` is known non-zero above",
        [DNS_EDIT],
    ),
    (
        "a move down leaves the editor closed",
        "        self.begin_edit();\n        self.edit_ip_config.dns_servers.swap(index, below);\n",
        "        self.edit_ip_config.dns_servers.swap(index, below);\n",
        [DNS_EDIT],
    ),
    (
        "the DNS tab has no Apply",
        "    if app.editing_ip {\n        render_apply_cancel(frame, app, lx, y + FIELD_HEIGHT + 16.0);\n    }\n",
        "",
        [DNS_EDIT],
    ),
    (
        "an added server reads as done",
        'self.status_message = format!("Added DNS server {typed}: Apply sends it");',
        'self.status_message = format!("Added DNS server {typed}");',
        [DNS_EDIT],
    ),
    (
        "the DNS list changes under DHCP",
        "        if self.dns_editable() {\n            Ok(())\n",
        "        if true {\n            Ok(())\n",
        [DNS_LOCKED],
    ),
    (
        "the DNS rows have buttons under DHCP",
        "            if !app.dns_editable() {\n"
        "                y += DNS_ROW_HEIGHT + 2.0;\n"
        "                continue;\n"
        "            }\n",
        "",
        [DNS_LOCKED],
    ),
    (
        "the DNS box is offered under DHCP, and nothing says why not",
        "    if !app.dns_editable() {\n        // Said rather than left to be discovered",
        "    if false {\n        // Said rather than left to be discovered",
        [DNS_LOCKED],
    ),
    (
        "the selection stays at its index across a re-read",
        "        self.selected_interface = found\n            .unwrap_or(self.selected_interface)\n",
        "        self.selected_interface = None\n            .unwrap_or(self.selected_interface)\n",
        [REREAD],
    ),
    (
        "a re-read throws an open edit away",
        "            (true, Some(_)) => {\n"
        "                self.finish_reading();\n"
        "                return;\n"
        "            }\n",
        "            (true, Some(_)) => None,\n",
        [REREAD],
    ),
    (
        "an edit outlives its interface",
        "        };\n        self.editing_ip = false;\n        if self.focus.is_some_and(",
        "        };\n        self.editing_ip = self.editing_ip && dropped.is_some();\n        if self.focus.is_some_and(",
        [REREAD],
    ),
    (
        "nothing says an edit went with its interface",
        "        if let Some(name) = dropped {\n",
        "        if let Some(name) = None::<String> {\n",
        [REREAD],
    ),
    (
        "a switch nobody moved is refused as a switch to DHCP",
        "            return Err(if iface.ip_config.dhcp_enabled == Some(true) {\n",
        "            return Err(if false {\n",
        [REFUSAL],
    ),
]

# An edit Apply has not sent is not lost without a question: choosing
# another interface, Escape and the window's X ask first (2026-10-04).
ASKS = "choosing_another_interface_asks_about_an_unapplied_edit"
ONLY = "only_an_unapplied_edit_is_asked_about_and_only_when_it_would_go"
APPLY_GOES = "apply_from_the_question_sends_the_edit_and_then_goes"
REFUSED_STAYS = "a_refused_apply_from_the_question_stays_with_the_edit"
CLOSING = "closing_with_an_unapplied_edit_asks_and_keeps_the_window_open"

MAIN += [
    (
        "an unapplied edit is left without a question",
        "        if !self.unapplied() {\n            return self.go(to);\n        }\n",
        "        if true {\n            return self.go(to);\n        }\n",
        [ASKS, CLOSING],
    ),
    (
        "an untouched editor is asked about",
        "        self.editing_ip\n"
        "            && self\n"
        "                .selected_iface()\n"
        "                .is_some_and(|iface| iface.ip_config != self.edit_ip_config)\n",
        "        self.editing_ip\n",
        [ONLY],
    ),
    (
        "choosing the interface already chosen reloads it",
        "                if i == self.selected_interface {\n"
        "                    self.focus = None;\n"
        "                    return Action::Redraw;\n"
        "                }\n",
        "",
        [ONLY],
    ),
    (
        "Cancel goes anyway",
        "            Choice::Cancel => Action::Redraw,\n",
        "            Choice::Cancel => self.go(to),\n",
        [ASKS],
    ),
    (
        "a refused Apply goes anyway",
        "                if let Err(why) = self.apply_ip_config() {\n"
        "                    self.status_message = why;\n"
        "                    return Action::Redraw;\n"
        "                }\n",
        "                if let Err(why) = self.apply_ip_config() {\n"
        "                    self.status_message = why;\n"
        "                }\n",
        [REFUSED_STAYS],
    ),
    (
        "the interface being chosen is found by its old index",
        "                        if let Some(index) = target.and_then(|name| {\n"
        "                            self.interfaces.iter().position(|iface| iface.name == name)\n"
        "                        }) {\n",
        "                        if let (Some(_), Leaving::Interface(index)) = (target, to) {\n",
        [APPLY_GOES],
    ),
    (
        "\"Applied\" is lost from the status line",
        "                        self.status_message = applied;\n",
        "                        let _ = applied;\n",
        [APPLY_GOES],
    ),
    (
        "the question is not drawn",
        "            question.render(&palette, width, height, &mut tree);\n",
        "            let _ = (question, palette);\n",
        [ASKS, CLOSING],
    ),
    (
        "a close over an unapplied edit closes",
        "            _ if matches!(event, Event::CloseRequested) => Response::KeepOpen,\n",
        "",
        [CLOSING],
    ),
    (
        "a key goes under the question",
        "        if self.question.is_some() {\n            return self.ask(&Event::Key(key.clone()));\n        }\n",
        "",
        [ASKS, CLOSING],
    ),
    (
        "a press goes under the question",
        "        if self.question.is_some() && matches!(event, Event::Mouse(_)) {\n",
        "        if false {\n",
        [ASKS],
    ),
]

# An interface whose DHCP nothing reports opens as not reported, not as
# "on" (2026-10-04, known-issues/E-the-network-managers-editor-says-dhcp-
# is-on-for-an-interface-whose-dhcp-nothing-reports.md).
NOT_REPORTED = "an_interface_whose_dhcp_nothing_reports_opens_as_not_reported"

MAIN += [
    (
        "a kernel-listed interface opens with DHCP on",
        "            dns_servers,\n            dhcp_enabled: None,\n        },\n",
        "            dns_servers,\n            dhcp_enabled: Some(true),\n        },\n",
        [NOT_REPORTED],
    ),
    (
        "the IP tab guesses DHCP",
        '        None => "DHCP: Not reported",\n',
        '        None => "DHCP: Enabled",\n',
        [NOT_REPORTED],
    ),
    (
        "the switch leaves not reported where it is",
        "                    Some(self.edit_ip_config.dhcp_enabled != Some(true));\n",
        "                    self.edit_ip_config.dhcp_enabled.map(|on| !on);\n",
        [NOT_REPORTED],
    ),
]

TABLES = {
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
        worst = max(worst, sweep(SRC / file, rows, "netmanager", timeout=900, only=mine))
    raise SystemExit(worst)
