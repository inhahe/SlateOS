//! The crate against a router simulated in memory ([`crate::simulate`]):
//! one that reads the NAT-PMP datagrams and SOAP envelopes it is sent, keeps
//! a table of mappings, and answers as RFC 6886 and the IGD specifications
//! have a router answer.

use super::*;
use crate::simulate::{
    CONTROL, Entry, GATEWAY, HERE, INTERNET, NEIGHBOUR, NatPmp, SERVICE, SimulatedLan as Lan,
    description,
};

fn forward(protocol: Protocol, internal_port: u16, external_port: u16) -> Forward {
    Forward {
        protocol,
        internal_port,
        external_port,
        description: "SlateOS: SSH".to_owned(),
    }
}

fn upnp_router() -> Router {
    Router {
        address: GATEWAY,
        method: Method::Upnp(igd::Control {
            service_type: SERVICE.to_owned(),
            url: CONTROL.to_owned(),
        }),
        name: "OpenWrt Router, MiniUPnPd".to_owned(),
    }
}

fn natpmp_router() -> Router {
    Router {
        address: GATEWAY,
        method: Method::NatPmp,
        name: String::new(),
    }
}

// ---------------------------------------------------------------------------
// Finding the router
// ---------------------------------------------------------------------------

#[test]
fn a_natpmp_router_is_found_by_one_datagram() {
    let mut lan = Lan::with(NatPmp::Speaks, true);
    assert_eq!(
        discover(&mut lan, GATEWAY),
        Ok((
            natpmp_router(),
            External {
                address: INTERNET,
                epoch: Some(1000),
            }
        ))
    );
    assert_eq!(lan.sent, ["natpmp [0, 0]"], "UPnP is not searched for");
}

#[test]
fn a_silent_natpmp_port_is_asked_four_times_then_upnp_is() {
    let mut lan = Lan::with(NatPmp::Silent, true);
    let found = discover(&mut lan, GATEWAY);
    assert_eq!(
        found,
        Ok((
            upnp_router(),
            External {
                address: INTERNET,
                epoch: None,
            }
        ))
    );
    let waits: Vec<u128> = lan.waits.iter().map(Duration::as_millis).collect();
    assert_eq!(waits, [250, 500, 1000, 2000]);
    assert_eq!(
        lan.sent.get(4..),
        Some(
            &[
                "ssdp x2".to_owned(),
                "GET description".to_owned(),
                "POST GetExternalIPAddress".to_owned(),
            ][..]
        )
    );
}

#[test]
fn a_closed_natpmp_port_is_asked_once() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    assert!(discover(&mut lan, GATEWAY).is_ok());
    assert_eq!(lan.waits.len(), 1);
}

#[test]
fn a_router_that_speaks_neither_says_why_for_each() {
    let mut lan = Lan::with(NatPmp::Absent, false);
    assert_eq!(
        discover(&mut lan, GATEWAY),
        Err(Problem::NotSpoken {
            natpmp: "the router refused the connection".to_owned(),
            upnp: "nothing answered a UPnP search".to_owned(),
        })
    );
}

#[test]
fn a_natpmp_refusal_is_the_answer_when_there_is_no_upnp() {
    let mut lan = Lan::with(NatPmp::Speaks, false);
    lan.natpmp_refuses = Some(2);
    let found = discover(&mut lan, GATEWAY);
    assert!(
        matches!(&found, Err(Problem::Refused { answer, .. }) if answer == "NAT-PMP result code 2"),
        "{found:?}"
    );
}

#[test]
fn devices_other_than_the_router_are_not_believed() {
    // Something else on the network answers the search as if it were the
    // router: from its own address, and from the router's address with a
    // description elsewhere.
    let mut lan = Lan::with(NatPmp::Absent, false);
    lan.others = vec![
        (
            NEIGHBOUR,
            b"HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\nLOCATION: http://192.168.1.77:1900/desc.xml\r\n\r\n"
                .to_vec(),
        ),
        (
            GATEWAY,
            b"HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\nLOCATION: http://192.168.1.77:1900/desc.xml\r\n\r\n"
                .to_vec(),
        ),
    ];
    let found = discover(&mut lan, GATEWAY);
    assert!(
        matches!(&found, Err(Problem::NotSpoken { upnp, .. })
            if upnp == "2 UPnP answers came from devices other than the router"),
        "{found:?}"
    );
    assert!(
        !lan.sent.iter().any(|s| s.starts_with("GET")),
        "nothing is fetched from them: {:?}",
        lan.sent
    );
}

#[test]
fn a_control_service_off_the_router_is_not_used() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.description = description("http://192.168.1.77:5000/ctl/IPConn");
    let found = discover(&mut lan, GATEWAY);
    assert!(
        matches!(&found, Err(Problem::Unreachable(why)) if why.contains("192.168.1.77") && why.contains("not used")),
        "{found:?}"
    );
    assert!(!lan.sent.iter().any(|s| s.starts_with("POST")));
}

#[test]
fn a_upnp_device_that_maps_nothing_is_not_a_router() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.description = "<root><device><deviceType>urn:schemas-upnp-org:device:MediaServer:1\
                       </deviceType></device></root>"
        .to_owned();
    let found = discover(&mut lan, GATEWAY);
    assert!(
        matches!(&found, Err(Problem::NotSpoken { upnp, .. }) if upnp.contains("offers no port mapping service")),
        "{found:?}"
    );
}

#[test]
fn a_router_with_no_internet_address_yet_says_so() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.external = String::new();
    let found = discover(&mut lan, GATEWAY);
    assert!(
        matches!(&found, Err(Problem::Refused { meaning, .. }) if meaning.contains("no internet address")),
        "{found:?}"
    );
}

#[test]
fn the_address_is_asked_again_the_way_it_was_found() {
    let mut lan = Lan::with(NatPmp::Speaks, true);
    assert_eq!(
        natpmp_router().external(&mut lan).map(|e| e.address),
        Ok(INTERNET)
    );
    assert_eq!(
        upnp_router().external(&mut lan).map(|e| e.address),
        Ok(INTERNET)
    );
    assert_eq!(lan.sent, ["natpmp [0, 0]", "POST GetExternalIPAddress"]);
}

#[test]
fn a_private_or_shared_internet_address_is_not_public() {
    for (address, public) in [
        (INTERNET, true),
        (Ipv4Addr::new(100, 128, 0, 1), true),
        (Ipv4Addr::new(192, 168, 0, 1), false),
        (Ipv4Addr::new(10, 1, 2, 3), false),
        (Ipv4Addr::new(172, 16, 0, 1), false),
        (Ipv4Addr::new(100, 64, 0, 1), false),
        (Ipv4Addr::new(100, 127, 255, 254), false),
        (Ipv4Addr::new(169, 254, 1, 1), false),
        (Ipv4Addr::new(203, 0, 113, 7), false),
        (Ipv4Addr::UNSPECIFIED, false),
        (Ipv4Addr::new(240, 0, 0, 1), false),
    ] {
        let e = External {
            address,
            epoch: None,
        };
        assert_eq!(e.is_public(), public, "{address}");
    }
}

// ---------------------------------------------------------------------------
// Forwards over NAT-PMP
// ---------------------------------------------------------------------------

#[test]
fn natpmp_maps_and_unmaps() {
    let mut lan = Lan::with(NatPmp::Speaks, false);
    let router = natpmp_router();
    let ssh = forward(Protocol::Tcp, 22, 2222);
    let mapped = router.map(&mut lan, HERE, &ssh, natpmp::LIFETIME);
    assert_eq!(
        mapped,
        Ok(Mapped {
            external_port: 2222,
            lifetime: 7200,
            epoch: Some(1000),
        })
    );
    assert_eq!(
        lan.table
            .get(&(Protocol::Tcp, 2222))
            .map(|e| (e.client, e.internal)),
        Some((HERE, 22))
    );
    assert_eq!(
        lan.sent.last().map(String::as_str),
        Some("natpmp [0, 2, 0, 0, 0, 22, 8, 174, 0, 0, 28, 32]")
    );
    let mapped = mapped.unwrap_or(Mapped {
        external_port: 0,
        lifetime: 0,
        epoch: None,
    });
    assert_eq!(router.unmap(&mut lan, &ssh, &mapped), Ok(()));
    assert!(lan.table.is_empty());
}

#[test]
fn natpmp_gives_another_port_when_the_one_asked_for_is_taken() {
    let mut lan = Lan::with(NatPmp::Speaks, false);
    lan.table.insert(
        (Protocol::Udp, 51413),
        Entry {
            client: NEIGHBOUR,
            internal: 51413,
            lease: 7200,
            description: String::new(),
        },
    );
    let mapped = natpmp_router().map(&mut lan, HERE, &forward(Protocol::Udp, 51413, 51413), 7200);
    assert_eq!(mapped.map(|m| m.external_port), Ok(51414));
}

#[test]
fn natpmp_refusals_are_reported_in_both_words() {
    let mut lan = Lan::with(NatPmp::Speaks, false);
    lan.natpmp_refuses = Some(4);
    let mapped = natpmp_router().map(&mut lan, HERE, &forward(Protocol::Tcp, 22, 22), 7200);
    assert_eq!(
        mapped,
        Err(Problem::Refused {
            answer: "NAT-PMP result code 4".to_owned(),
            meaning: "cannot make any more mappings now".to_owned(),
        })
    );
    assert_eq!(
        mapped.map_err(|p| p.to_string()),
        Err("the router cannot make any more mappings now (NAT-PMP result code 4)".to_owned())
    );
}

// ---------------------------------------------------------------------------
// Forwards over UPnP
// ---------------------------------------------------------------------------

#[test]
fn upnp_maps_for_an_hour_and_unmaps() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    let router = upnp_router();
    let ssh = forward(Protocol::Tcp, 22, 2222);
    let mapped = router.map(&mut lan, HERE, &ssh, natpmp::LIFETIME);
    assert_eq!(
        mapped,
        Ok(Mapped {
            external_port: 2222,
            lifetime: UPNP_LEASE,
            epoch: None,
        })
    );
    assert_eq!(
        lan.table.get(&(Protocol::Tcp, 2222)),
        Some(&Entry {
            client: HERE,
            internal: 22,
            lease: 3600,
            description: "SlateOS: SSH".to_owned(),
        })
    );
    let mapped = mapped.unwrap_or(Mapped {
        external_port: 0,
        lifetime: 0,
        epoch: None,
    });
    assert_eq!(router.unmap(&mut lan, &ssh, &mapped), Ok(()));
    assert!(lan.table.is_empty());
    // A mapping the router no longer has is gone, which was the point.
    assert_eq!(router.unmap(&mut lan, &ssh, &mapped), Ok(()));
}

#[test]
fn a_router_that_keeps_only_permanent_mappings_gets_one() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.permanent_only = true;
    let mapped = upnp_router().map(&mut lan, HERE, &forward(Protocol::Udp, 1194, 1194), 0);
    assert_eq!(mapped.map(|m| m.lifetime), Ok(0));
    assert_eq!(
        lan.sent,
        ["POST AddPortMapping", "POST AddPortMapping"],
        "an hour, refused; then permanent"
    );
}

#[test]
fn a_port_another_computer_holds_is_left_to_it_and_named() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    let theirs = Entry {
        client: NEIGHBOUR,
        internal: 22,
        lease: 0,
        description: "their SSH".to_owned(),
    };
    lan.table.insert((Protocol::Tcp, 2222), theirs.clone());
    let mapped = upnp_router().map(&mut lan, HERE, &forward(Protocol::Tcp, 22, 2222), 0);
    assert!(
        matches!(&mapped, Err(Problem::Refused { meaning, .. })
            if meaning == "already forwards external port 2222 TCP to 192.168.1.77, port 22"),
        "{mapped:?}"
    );
    assert_eq!(lan.table.get(&(Protocol::Tcp, 2222)), Some(&theirs));
}

#[test]
fn this_computers_own_mapping_is_replaced_where_the_router_will_not_overwrite_it() {
    // The IGD specification has a repeat from the same client overwrite its
    // mapping, as miniupnpd does; some routers answer 718 to it all the
    // same. The mapping is this computer's, of the same port -- left by an
    // earlier run -- so it is removed and made again.
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.strict = true;
    lan.table.insert(
        (Protocol::Tcp, 2222),
        Entry {
            client: HERE,
            internal: 22,
            lease: 0,
            description: "from last week".to_owned(),
        },
    );
    let mapped = upnp_router().map(&mut lan, HERE, &forward(Protocol::Tcp, 22, 2222), 0);
    assert_eq!(mapped.map(|m| m.external_port), Ok(2222));
    assert_eq!(
        lan.sent,
        [
            "POST AddPortMapping",
            "POST GetSpecificPortMappingEntry",
            "POST DeletePortMapping",
            "POST AddPortMapping",
        ]
    );
    assert_eq!(
        lan.table
            .get(&(Protocol::Tcp, 2222))
            .map(|e| e.description.as_str()),
        Some("SlateOS: SSH")
    );
}

#[test]
fn this_computers_mapping_of_another_port_is_not_taken_over() {
    // Another program here may have asked for it (a game does): it is named,
    // not replaced.
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.table.insert(
        (Protocol::Tcp, 2222),
        Entry {
            client: HERE,
            internal: 2022,
            lease: 0,
            description: "a game".to_owned(),
        },
    );
    let mapped = upnp_router().map(&mut lan, HERE, &forward(Protocol::Tcp, 22, 2222), 0);
    assert!(
        matches!(&mapped, Err(Problem::Refused { meaning, .. })
            if meaning == "already forwards external port 2222 TCP to 192.168.1.50, port 2022"),
        "{mapped:?}"
    );
    assert_eq!(
        lan.table.get(&(Protocol::Tcp, 2222)).map(|e| e.internal),
        Some(2022)
    );
}

#[test]
fn a_router_set_to_refuse_changes_says_so() {
    let mut lan = Lan::with(NatPmp::Absent, true);
    lan.refuses_changes = Some(606);
    let mapped = upnp_router().map(&mut lan, HERE, &forward(Protocol::Tcp, 22, 22), 0);
    assert!(
        matches!(&mapped, Err(Problem::Refused { answer, meaning })
            if answer == "UPnP error 606, ActionNotAuthorized" && meaning.contains("refuse changes")),
        "{mapped:?}"
    );
}
