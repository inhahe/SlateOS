"""Mutation test for the Network Manager's reading of the machine.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when the window stopped saying it
could not see the network and began listing the interfaces the kernel
publishes (through `hwquery`, from SlateOS's `/proc/net`): the read and what
it records, the two kinds of absent address, the banner that explains an
empty list, the status bar's count, and the Wi-Fi and VPN tabs' empty states.

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
