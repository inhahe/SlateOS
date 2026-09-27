"""Mutation test for hwquery's network reader.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when `query_network` began
reading SlateOS's `/proc/net` -- a file, where Linux has a directory -- instead
of only Linux's `/proc/net/dev`, which SlateOS does not serve: the placeholder
the kernel writes with no card, the three kinds of address, what is filled in
from the file, and how a name that is not text is drawn.

And what changed on 2026-09-27: the processor read end to end from
`/sys/devices/system/cpu`, and every value nothing publishes -- a cache the
processor lacks, the memory's speed and slots, the adapter's memory, a refresh
rate with no primary output -- `None` rather than a 0 that reads as a
measurement.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

READ = "slateos_proc_net_is_read"
UNCONFIGURED = "an_unconfigured_card_has_no_addresses_rather_than_unreported_ones"
PLACEHOLDER = "the_placeholder_for_no_card_is_not_an_adapter"
MISSING = "a_missing_line_is_not_reported"
ESCAPED = "a_name_that_is_not_text_is_escaped_not_replaced"
NEITHER = "neither_file_is_not_available"
RELEASE = "the_kernel_release_is_read_from_proc_version"
CPU = "the_processor_is_read_from_sys_devices"
UNPUBLISHED = "what_nothing_publishes_is_none_not_zero"
MALFORMED = "a_file_that_holds_no_number_is_a_parse_error_not_a_missing_file"

QUERY = [
    (
        "SlateOS's /proc/net is not read",
        "        if let Some(interfaces) = procfs.net_interfaces().ok().flatten() {",
        "        if let Some(interfaces) = None::<Vec<procinfo::NetInterface>> {",
        [READ, UNCONFIGURED, PLACEHOLDER, MISSING],
    ),
    (
        "the kernel's placeholder is listed as a card",
        "                .filter(|iface| is_a_card(iface))\n",
        "",
        [PLACEHOLDER],
    ),
    (
        "every interface is taken for the placeholder",
        "        .is_none_or(|mac| !mac.iter().all(|&b| b == b'0' || b == b':'))",
        "        .is_none_or(|_| false)",
        [READ, UNCONFIGURED, MISSING],
    ),
    (
        "0.0.0.0 is shown as an address",
        '        Some(b"0.0.0.0") => Address::Unassigned,\n',
        "",
        [UNCONFIGURED],
    ),
    (
        "a missing line is called unassigned",
        "        None => Address::NotReported,",
        "        None => Address::Unassigned,",
        [MISSING],
    ),
    (
        "the link state is dropped",
        "        up: iface.up,",
        "        up: None,",
        [READ, UNCONFIGURED],
    ),
    (
        "the MAC is dropped",
        "        mac_address: iface.mac.as_deref().map(shown).unwrap_or_default(),",
        "        mac_address: String::new(),",
        [READ],
    ),
    (
        "the netmask is read from the gateway line",
        "        subnet: address_of(iface.netmask.as_deref()),",
        "        subnet: address_of(iface.gateway.as_deref()),",
        [READ],
    ),
    (
        "the DNS server is dropped",
        "        dns: address_of(iface.dns.as_deref()),",
        "        dns: Address::NotReported,",
        [READ, UNCONFIGURED],
    ),
    (
        "a name that is not text is replaced, not escaped",
        "    quoting::escape_unprintable(bytes)",
        "    String::from_utf8_lossy(bytes).into_owned()",
        [ESCAPED],
    ),
    (
        "neither file is an empty list",
        "                .ok_or_else(|| HwQueryError::NotAvailable {\n"
        '                    path: self.rooted("/proc/net"),',
        "                .or(Some(Vec::new()))\n"
        "                .ok_or_else(|| HwQueryError::NotAvailable {\n"
        '                    path: self.rooted("/proc/net"),',
        [NEITHER],
    ),
    (
        "the kernel's name is taken for its release",
        "            .nth(2)",
        "            .nth(0)",
        [RELEASE],
    ),
    (
        "an unread cache is a 0 again",
        '            l3_kb: self.cache_kb(3, "Unified"),',
        '            l3_kb: self.cache_kb(3, "Unified").or(Some(0)),',
        [CPU],
    ),
    (
        "the L1 caches are read the other way round",
        '            l1_data_kb: self.cache_kb(1, "Data"),',
        '            l1_data_kb: self.cache_kb(1, "Instruction"),',
        [CPU],
    ),
    (
        "a cache is matched on its type alone",
        "            if found_level != level || !found_kind.eq_ignore_ascii_case(kind) {",
        "            if !found_kind.eq_ignore_ascii_case(kind) {",
        [CPU],
    ),
    (
        "the cores are counted per thread",
        "                let pair = (socket, core);",
        '                let pair = (socket, format!("{cpu}"));',
        [CPU],
    ),
    (
        "the memory's speed is a 0 again",
        "            speed_mhz: None,\n            slots_used: None,",
        "            speed_mhz: Some(0),\n            slots_used: None,",
        [UNPUBLISHED],
    ),
    (
        "the first row is taken for the primary",
        "            refresh_rate_hz: primary.map(|mon| mon.refresh_hz),",
        "            refresh_rate_hz: mons.outputs.first().map(|mon| mon.refresh_hz),",
        [UNPUBLISHED],
    ),
    (
        "a malformed file is called missing",
        "        text.parse().map_err(|_| HwQueryError::ParseError {\n"
        '            detail: format!("{}: expected a number, got {text:?}", self.rooted(path)),\n',
        "        text.parse().map_err(|_| HwQueryError::NotAvailable {\n"
        "            path: self.rooted(path),\n",
        [MALFORMED],
    ),
    (
        "a malformed file is named by the path not opened",
        '            detail: format!("{}: expected a number, got {text:?}", self.rooted(path)),',
        '            detail: format!("{path}: expected a number, got {text:?}"),',
        [MALFORMED],
    ),
    (
        "a malformed file's content is not shown",
        '            detail: format!("{}: expected a number, got {text:?}", self.rooted(path)),',
        '            detail: format!("{}: expected a number", self.rooted(path)),',
        [MALFORMED],
    ),
]

TABLES = {
    "query.rs": QUERY,
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
        worst = max(worst, sweep(SRC / file, rows, "hwquery", timeout=900, only=mine))
    raise SystemExit(worst)
