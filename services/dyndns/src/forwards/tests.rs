//! The forwards: their configuration read, and the service's steps against
//! the router `dyndnsrouter` simulates -- the same one its own protocol
//! tests are run against.

// A test states what it expects by failing loudly when it is not so.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use dyndnsrouter::simulate::{Entry, GATEWAY, HERE, INTERNET, NEIGHBOUR, NatPmp, SimulatedLan};

const NOW: u64 = 1_791_244_800; // 2026-10-06T00:00:00Z

const LAN: Lan = Lan {
    here: HERE,
    gateway: GATEWAY,
};

const SSH: &str = "\
# Port forwards: ask the router to pass these ports through to this computer.
forwards:
  SSH:
    protocol: tcp          # tcp, udp, or both
    port: 22               # this computer's port, or a range: 6881-6889
    external_port: 2222    # the internet's side; the same as port when left out
    enabled: true
";

fn ports(text: &str) -> Ports {
    let mut p = Ports::new();
    p.configure(read_forwards(text), NOW);
    p
}

fn one(text: &str) -> Result<ForwardEntry, Misconfigured> {
    let c = read_forwards(text);
    assert_eq!(c.problem, None);
    assert_eq!(c.entries.len(), 1, "{:?}", c.entries);
    c.entries.into_iter().next().unwrap()
}

fn texts(notes: &[Note]) -> Vec<&str> {
    notes.iter().map(|n| n.text.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Reading the file
// ---------------------------------------------------------------------------

#[test]
fn a_forward_is_read() {
    let ssh = one(SSH).unwrap();
    assert_eq!(
        ssh,
        ForwardEntry {
            name: "SSH".to_owned(),
            protocols: vec![Protocol::Tcp],
            first_port: 22,
            last_port: 22,
            first_external: 2222,
            enabled: true,
        }
    );
    assert_eq!(
        ssh.forwards(),
        [Forward {
            protocol: Protocol::Tcp,
            internal_port: 22,
            external_port: 2222,
            description: "SlateOS SSH".to_owned(),
        }]
    );
    assert_eq!(
        (ssh.protocol_text(), ssh.ports_text(), ssh.external_text()),
        ("tcp", "22".to_owned(), "2222".to_owned())
    );
}

#[test]
fn a_range_of_both_protocols_is_each_port_of_each() {
    let t = one("forwards:\n  Torrents:\n    protocol: both\n    port: 6881-6889\n").unwrap();
    assert_eq!(t.protocols, [Protocol::Tcp, Protocol::Udp]);
    assert_eq!(
        (t.count(), t.first_external, t.last_external()),
        (9, 6881, 6889)
    );
    let forwards = t.forwards();
    assert_eq!(forwards.len(), 18);
    assert_eq!(
        forwards
            .last()
            .map(|f| (f.protocol, f.internal_port, f.external_port)),
        Some((Protocol::Udp, 6889, 6889))
    );
    // The external side as its first port, or as the whole range.
    for external in ["7881", "7881-7889"] {
        let t = one(&format!(
            "forwards:\n  T:\n    protocol: udp\n    port: 6881-6889\n    external_port: {external}\n"
        ))
        .unwrap();
        assert_eq!(
            (t.first_external, t.last_external()),
            (7881, 7889),
            "{external}"
        );
        assert_eq!(t.external_text(), "7881-7889");
    }
    // Left empty, it is the same as the port.
    let t =
        one("forwards:\n  T:\n    protocol: UDP\n    port: 53\n    external_port: \"\"\n").unwrap();
    assert_eq!(
        (t.protocols.as_slice(), t.first_external),
        (&[Protocol::Udp][..], 53)
    );
}

#[test]
fn what_cannot_be_used_says_why() {
    for (fields, why) in [
        ("protocol: tcp", "it names no port"),
        ("port: 22", "it names no protocol (tcp, udp or both)"),
        (
            "protocol: sctp\n    port: 22",
            "its protocol \"sctp\" is not tcp, udp or both",
        ),
        ("protocol: tcp\n    port: 0", "is not a port (1 to 65535)"),
        (
            "protocol: tcp\n    port: 70000",
            "is not a port (1 to 65535)",
        ),
        (
            "protocol: tcp\n    port: 30-20",
            "is not a port (1 to 65535)",
        ),
        ("protocol: tcp\n    port: ssh", "is not a port (1 to 65535)"),
        (
            "protocol: tcp\n    port: 1000-1100",
            "is 101 ports; a forward may have 100",
        ),
        (
            "protocol: tcp\n    port: 6881-6889\n    external_port: 7000-7001",
            "is not as long as its port range",
        ),
        (
            "protocol: tcp\n    port: 6881-6889\n    external_port: 65530",
            "run past 65535",
        ),
        (
            "protocol: tcp\n    port: 22\n    external_port: x",
            "its external_port \"x\"",
        ),
        (
            "protocol: tcp\n    port: 22\n    enabled: maybe",
            "neither true nor false",
        ),
    ] {
        let read = one(&format!("forwards:\n  F:\n    {fields}\n"));
        assert!(
            read.as_ref()
                .is_err_and(|m| m.name == "F" && m.why.contains(why)),
            "{fields}: {read:?}"
        );
    }
}

#[test]
fn a_list_is_a_problem_with_the_whole_file() {
    let c = read_forwards("forwards:\n  - name: SSH\n    protocol: tcp\n    port: 22\n");
    assert!(c.entries.is_empty());
    assert!(c.problem.is_some_and(|p| p.contains("written as a list")));
    let empty = read_forwards("");
    assert_eq!(empty, ForwardsConfig::default());
}

#[test]
fn a_port_is_forwarded_by_one_forward() {
    let c = read_forwards(
        "forwards:\n\
         \x20 A:\n    protocol: tcp\n    port: 22\n\
         \x20 B:\n    protocol: tcp\n    port: 22\n    external_port: 2200\n\
         \x20 C:\n    protocol: tcp\n    port: 80\n    external_port: 22\n\
         \x20 D:\n    protocol: udp\n    port: 22\n\
         \x20 E:\n    protocol: tcp\n    port: 22\n    enabled: false\n",
    );
    let verdicts: Vec<Result<&str, &str>> = c
        .entries
        .iter()
        .map(|e| {
            e.as_ref()
                .map(|e| e.name.as_str())
                .map_err(|m| m.why.as_str())
        })
        .collect();
    assert_eq!(
        verdicts,
        [
            Ok("A"),
            Err("port 22 (TCP) is forwarded by A too; only the first is used"),
            Err("external port 22 (TCP) is forwarded by A too; only the first is used"),
            Ok("D"),
            Ok("E"),
        ]
    );
}

#[test]
fn a_long_name_is_cut_for_the_routers_list() {
    let d = description(&"x".repeat(100));
    assert_eq!(d.chars().count(), 63);
    assert!(d.starts_with("SlateOS xxx"));
}

// ---------------------------------------------------------------------------
// The steps
// ---------------------------------------------------------------------------

#[test]
fn forwards_are_asked_for_once_the_router_is_found() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports(SSH);
    assert_eq!(p.next_due(), 0, "the router is looked for at once");
    let notes = p.step(&mut lan, Some(LAN), NOW);
    assert_eq!(
        texts(&notes),
        [
            "found the router at 192.168.1.1: it speaks NAT-PMP",
            "this network's internet address is 81.2.69.142",
            "forward SSH: forwarded: the router forwards external port 2222 (TCP) to this \
             computer's port 22",
        ]
    );
    assert_eq!(
        lan.table
            .get(&(Protocol::Tcp, 2222))
            .map(|e| (e.client, e.internal)),
        Some((HERE, 22))
    );
    assert_eq!(p.public_address(), Some(INTERNET));
    assert_eq!(p.why_no_address(), None);
    assert!(p.all_well());
    // The address is asked again in 15 minutes; the mapping is renewed at
    // half of its two hours.
    assert_eq!(p.next_due(), NOW + ROUTER_CHECK);
    // Nothing is sent before then, and nothing journalled.
    let sent = lan.sent.len();
    assert!(p.step(&mut lan, Some(LAN), NOW + 60).is_empty());
    assert_eq!(lan.sent.len(), sent);
}

#[test]
fn mappings_are_renewed_at_half_their_lease() {
    // UPnP grants an hour: asked again at half.
    let mut lan = SimulatedLan::with(NatPmp::Absent, true);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    let adds = |lan: &SimulatedLan| {
        lan.sent
            .iter()
            .filter(|s| *s == "POST AddPortMapping")
            .count()
    };
    assert_eq!(adds(&lan), 1);
    p.step(&mut lan, Some(LAN), NOW + 1799);
    assert_eq!(adds(&lan), 1);
    let notes = p.step(&mut lan, Some(LAN), NOW + 1800);
    assert_eq!(adds(&lan), 2);
    assert!(
        notes.is_empty(),
        "a renewal that changes nothing is not news: {notes:?}"
    );
}

#[test]
fn a_restarted_router_is_asked_for_everything_again() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    // It restarts: its table is empty, and its epoch counts from 0 again.
    lan.table.clear();
    lan.epoch = 3;
    let notes = p.step(&mut lan, Some(LAN), NOW + ROUTER_CHECK);
    assert!(
        texts(&notes)
            .iter()
            .any(|t| t.contains("has restarted and forgotten its forwards")),
        "{notes:?}"
    );
    assert!(
        lan.table.contains_key(&(Protocol::Tcp, 2222)),
        "asked for again at once"
    );
}

#[test]
fn a_forward_taken_out_of_the_file_is_taken_off_the_router() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    p.configure(read_forwards(""), NOW + 10);
    assert_eq!(p.next_due(), NOW + 10);
    p.step(&mut lan, Some(LAN), NOW + 10);
    assert!(lan.table.is_empty());
    assert_eq!(p.next_due(), NOW + ROUTER_CHECK);
}

#[test]
fn a_forward_whose_port_here_changed_frees_its_external_port_first() {
    let mut lan = SimulatedLan::with(NatPmp::Absent, true);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    lan.sent.clear();
    p.configure(
        read_forwards(&SSH.replace("port: 22 ", "port: 2022 ")),
        NOW + 10,
    );
    p.step(&mut lan, Some(LAN), NOW + 10);
    assert_eq!(lan.sent, ["POST DeletePortMapping", "POST AddPortMapping"]);
    assert_eq!(
        lan.table.get(&(Protocol::Tcp, 2222)).map(|e| e.internal),
        Some(2022)
    );
}

#[test]
fn an_unchanged_forward_keeps_its_mapping_when_the_file_is_read_again() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    let sent = lan.sent.len();
    p.configure(read_forwards(SSH), NOW + 10);
    p.step(&mut lan, Some(LAN), NOW + 10);
    assert_eq!(lan.sent.len(), sent, "nothing to ask");
}

#[test]
fn no_router_is_said_and_looked_for_again_later() {
    let mut lan = SimulatedLan::with(NatPmp::Absent, false);
    let mut p = ports(SSH);
    let notes = p.step(&mut lan, Some(LAN), NOW);
    let (state, message) = p.state("SSH").unwrap();
    assert_eq!(state, ForwardState::NoRouter);
    assert!(message.contains("neither NAT-PMP"), "{message}");
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(p.next_due(), NOW + ROUTER_LOOK);
    // Looked for again then, and journalled again only if it says otherwise.
    let notes = p.step(&mut lan, Some(LAN), NOW + ROUTER_LOOK);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(lan.sent.iter().filter(|s| s.starts_with("ssdp")).count(), 2);
    assert!(!p.all_well());
}

#[test]
fn no_network_waits_for_one() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports(SSH);
    p.step(&mut lan, None, NOW);
    assert!(lan.sent.is_empty());
    let (state, message) = p.state("SSH").unwrap();
    assert_eq!(state, ForwardState::NoRouter);
    assert!(message.contains("no network address"), "{message}");
    assert_eq!(p.next_due(), u64::MAX);
    // A network appears: the router is found and the forward made.
    p.step(&mut lan, Some(LAN), NOW + 30);
    assert_eq!(p.state("SSH").map(|s| s.0), Some(ForwardState::Forwarded));
}

#[test]
fn a_network_lost_and_found_starts_again() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    p.step(&mut lan, None, NOW + 5);
    lan.table.clear();
    p.step(&mut lan, Some(LAN), NOW + 10);
    assert!(lan.table.contains_key(&(Protocol::Tcp, 2222)));
}

#[test]
fn a_refusal_is_asked_again_in_half_an_hour() {
    let mut lan = SimulatedLan::with(NatPmp::Absent, true);
    lan.refuses_changes = Some(606);
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    let (state, message) = p.state("SSH").unwrap();
    assert_eq!(state, ForwardState::Refused);
    assert!(message.contains("refuse changes"), "{message}");
    assert_eq!(
        Document::parse(&p.status_text(NOW)).get_str(&["forwards", "SSH", "answer"]),
        Some("UPnP error 606, ActionNotAuthorized".to_owned())
    );
    assert_eq!(p.next_due(), NOW + ROUTER_CHECK);
    // Its owner allows it; at the next ask it is made.
    lan.refuses_changes = None;
    p.step(&mut lan, Some(LAN), NOW + ROUTER_CHECK);
    assert_eq!(
        p.state("SSH").map(|s| s.0),
        Some(ForwardState::Refused),
        "not yet"
    );
    p.step(&mut lan, Some(LAN), NOW + REFUSED_RETRY);
    assert_eq!(p.state("SSH").map(|s| s.0), Some(ForwardState::Forwarded));
}

#[test]
fn a_router_that_stops_answering_is_not_asked_for_every_port() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    let mut p = ports("forwards:\n  Torrents:\n    protocol: tcp\n    port: 6881-6885\n");
    p.step(&mut lan, Some(LAN), NOW);
    assert_eq!(lan.table.len(), 5);
    // Its address is asked again before the renewals are due -- its epoch
    // having counted the 3000 seconds, as a router's does...
    lan.epoch = 4000;
    p.step(&mut lan, Some(LAN), NOW + 3000);
    // ...and it goes quiet just as they are: the first port's sends go
    // unanswered, and the other four are not asked for.
    lan.natpmp = NatPmp::Silent;
    let before = lan.sent.len();
    p.step(&mut lan, Some(LAN), NOW + 3600);
    assert_eq!(
        lan.sent.get(before..).map(<[String]>::len),
        Some(usize::try_from(natpmp::SENDS).unwrap())
    );
    assert_eq!(
        p.state("Torrents").map(|s| s.0),
        Some(ForwardState::Forwarded),
        "what was granted stands until its lease ends"
    );
    assert_eq!(
        p.next_due(),
        NOW + 3600,
        "the router is asked again at once"
    );
}

#[test]
fn a_range_partly_held_by_another_is_partial() {
    let mut lan = SimulatedLan::with(NatPmp::Absent, true);
    lan.table.insert(
        (Protocol::Tcp, 6883),
        Entry {
            client: NEIGHBOUR,
            internal: 6883,
            lease: 0,
            description: "theirs".to_owned(),
        },
    );
    let mut p = ports("forwards:\n  Torrents:\n    protocol: tcp\n    port: 6881-6883\n");
    p.step(&mut lan, Some(LAN), NOW);
    let (state, message) = p.state("Torrents").unwrap();
    assert_eq!(state, ForwardState::Partial);
    assert_eq!(
        message,
        "the router forwards 2 of its 3 ports; for the rest, the router already forwards \
         external port 6883 TCP to 192.168.1.77, port 6883 (UPnP error 718, \
         ConflictInMappingEntry)"
    );
}

#[test]
fn a_router_behind_another_gives_no_address_to_publish() {
    let mut lan = SimulatedLan::with(NatPmp::Speaks, false);
    lan.external = "100.64.3.4".to_owned();
    let mut p = ports(SSH);
    let notes = p.step(&mut lan, Some(LAN), NOW);
    assert_eq!(p.public_address(), None);
    assert!(p.why_no_address().is_some_and(|w| w.contains("private")));
    assert!(
        notes
            .iter()
            .any(|n| n.level == Level::Warning && n.text.contains("100.64.3.4")),
        "{notes:?}"
    );
    assert_eq!(
        Document::parse(&p.status_text(NOW)).get_bool(&["router", "public"]),
        Some(false)
    );
}

#[test]
fn a_permanent_mapping_no_longer_wanted_is_removed_once_the_router_agrees() {
    let mut lan = SimulatedLan::with(NatPmp::Absent, true);
    lan.permanent_only = true;
    let mut p = ports(SSH);
    p.step(&mut lan, Some(LAN), NOW);
    assert_eq!(
        lan.table.get(&(Protocol::Tcp, 2222)).map(|e| e.lease),
        Some(0)
    );
    p.configure(read_forwards(""), NOW + 10);
    lan.refuses_changes = Some(501);
    p.step(&mut lan, Some(LAN), NOW + 10);
    assert!(lan.table.contains_key(&(Protocol::Tcp, 2222)), "refused");
    assert_eq!(p.next_due(), NOW + 10 + ROUTER_LOOK);
    lan.refuses_changes = None;
    p.step(&mut lan, Some(LAN), NOW + 10 + ROUTER_LOOK);
    assert!(lan.table.is_empty());
}

#[test]
fn the_status_file_says_it_all() {
    let mut lan = SimulatedLan::with(NatPmp::Absent, true);
    let text = format!(
        "{SSH}  Broken:\n    protocol: tcp\n  Off:\n    protocol: udp\n    port: 9\n    enabled: false\n"
    );
    let mut p = ports(&text);
    p.step(&mut lan, Some(LAN), NOW);
    let status = p.status_text(NOW);
    assert!(
        status.starts_with("# What the dynamic-DNS service"),
        "{status}"
    );
    let doc = Document::parse(&status);
    let get = |path: &[&str]| doc.get_str(path).unwrap_or_default();
    assert_eq!(get(&["written_at"]), "2026-10-06T00:00:00Z");
    assert_eq!(get(&["router", "state"]), "found");
    assert_eq!(get(&["router", "address"]), "192.168.1.1");
    assert_eq!(get(&["router", "protocol"]), "UPnP");
    assert_eq!(get(&["router", "name"]), "OpenWrt Router, MiniUPnPd");
    assert_eq!(get(&["router", "internet_address"]), "81.2.69.142");
    assert_eq!(doc.get_bool(&["router", "public"]), Some(true));
    assert_eq!(get(&["forwards", "SSH", "state"]), "forwarded");
    assert_eq!(get(&["forwards", "SSH", "protocol"]), "tcp");
    assert_eq!(get(&["forwards", "SSH", "port"]), "22");
    assert_eq!(get(&["forwards", "SSH", "external_port"]), "2222");
    assert_eq!(
        get(&["forwards", "SSH", "checked_at"]),
        "2026-10-06T00:00:00Z"
    );
    assert_eq!(
        get(&["forwards", "SSH", "next_check_at"]),
        "2026-10-06T00:30:00Z"
    );
    assert_eq!(get(&["forwards", "Broken", "state"]), "misconfigured");
    assert_eq!(get(&["forwards", "Broken", "message"]), "it names no port");
    assert_eq!(get(&["forwards", "Off", "state"]), "disabled");
    assert_eq!(
        texts(&p.config_notes()),
        ["forward Broken: not used: it names no port"]
    );
}
