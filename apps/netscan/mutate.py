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
OUTSIDE = "nothing_is_filled_in_from_outside_the_record"
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
]

WHOIS = [
    (
        "IANA's referral is not followed",
        "        Some(server) if server != iana => {",
        "        Some(server) if false => {",
        [REFER],
    ),
    (
        "a comment line is read as a field",
        "            if line.starts_with(['%', '#']) {",
        "            if false {",
        [OUTSIDE],
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
