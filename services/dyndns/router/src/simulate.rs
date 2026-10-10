//! A home network simulated in memory, for tests: a router that reads the
//! NAT-PMP datagrams and SOAP envelopes it is sent, keeps a table of
//! mappings, and answers as RFC 6886 and the IGD specifications have a
//! router answer -- and other devices that answer UPnP searches.
//!
//! This crate's tests use it, and so do the `dyndns` service's, through the
//! `simulate` feature, so the service is tested against the same router the
//! protocols are. It is no part of a build that does not ask for it.

// A simulated router that is sent what no router would be sent fails the
// test that sent it, loudly.
#![allow(clippy::panic)]

use std::collections::BTreeMap;

use crate::{HttpAnswer, HttpRequest, Net, Protocol, natpmp, xml};
use core::net::{Ipv4Addr, SocketAddrV4};
use core::time::Duration;

/// The router's address on the network: the default gateway.
pub const GATEWAY: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 1);
/// The address of the computer asking.
pub const HERE: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 50);
/// Another computer on the network.
pub const NEIGHBOUR: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 77);
/// The router's internet address.
pub const INTERNET: Ipv4Addr = Ipv4Addr::new(81, 2, 69, 142);
/// Where the router's UPnP description is.
pub const LOCATION: &str = "http://192.168.1.1:5000/rootDesc.xml";
/// Where its port mapping service is.
pub const CONTROL: &str = "http://192.168.1.1:5000/ctl/IPConn";
/// The port mapping service's type.
pub const SERVICE: &str = "urn:schemas-upnp-org:service:WANIPConnection:1";

/// miniupnpd's description, cut to what matters, with `control_url` as its
/// port mapping service's control URL.
#[must_use]
pub fn description(control_url: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\r\n<root xmlns=\"urn:schemas-upnp-org:device-1-0\">\
         <specVersion><major>1</major><minor>1</minor></specVersion><device>\
         <deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:1</deviceType>\
         <friendlyName>OpenWrt Router</friendlyName><manufacturer>OpenWrt</manufacturer>\
         <modelName>MiniUPnPd</modelName><deviceList><device>\
         <deviceType>urn:schemas-upnp-org:device:WANDevice:1</deviceType><deviceList><device>\
         <deviceType>urn:schemas-upnp-org:device:WANConnectionDevice:1</deviceType>\
         <serviceList><service><serviceType>{SERVICE}</serviceType>\
         <serviceId>urn:upnp-org:serviceId:WANIPConn1</serviceId>\
         <controlURL>{control_url}</controlURL><eventSubURL>/evt/IPConn</eventSubURL>\
         <SCPDURL>/WANIPCn.xml</SCPDURL></service></serviceList>\
         </device></deviceList></device></deviceList></device></root>"
    )
}

/// How the router's NAT-PMP port behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NatPmp {
    /// Nothing listens: the router answers "port unreachable".
    Absent,
    /// Datagrams vanish.
    Silent,
    /// It speaks NAT-PMP.
    Speaks,
}

/// A mapping in the router's table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The computer it forwards to.
    pub client: Ipv4Addr,
    /// The port there.
    pub internal: u16,
    /// Its lease, in seconds; 0 for permanent.
    pub lease: u32,
    /// What it is for (UPnP's description; empty for NAT-PMP).
    pub description: String,
}

/// The router, and everything else on the network.
#[derive(Clone, Debug)]
pub struct SimulatedLan {
    /// How its NAT-PMP port behaves.
    pub natpmp: NatPmp,
    /// A NAT-PMP result code it answers every request with.
    pub natpmp_refuses: Option<u16>,
    /// NAT-PMP's seconds since start of epoch, in every answer.
    pub epoch: u32,
    /// The address its UPnP search answers come from; `None`: it does not
    /// speak UPnP.
    pub upnp_from: Option<Ipv4Addr>,
    /// The LOCATION its search answers give.
    pub location: String,
    /// The description served there.
    pub description: String,
    /// It keeps only permanent mappings (IGD v1's error 725).
    pub permanent_only: bool,
    /// A UPnP error it answers every change with (606: read-only).
    pub refuses_changes: Option<u16>,
    /// It answers 718 to any add of a port it has, even the same client's.
    pub strict: bool,
    /// Its internet address, as both protocols report it: UPnP's text as it
    /// is, NAT-PMP's as an address (0.0.0.0 when it is not one).
    pub external: String,
    /// Search answers from other devices.
    pub others: Vec<(Ipv4Addr, Vec<u8>)>,
    /// The table: (protocol, external port) to the mapping.
    pub table: BTreeMap<(Protocol, u16), Entry>,
    /// What was sent, in order: `natpmp [..]`, `ssdp xN`, `GET description`,
    /// `POST <action>`.
    pub sent: Vec<String>,
    /// The waits NAT-PMP was asked to wait.
    pub waits: Vec<Duration>,
}

impl SimulatedLan {
    /// A network whose router speaks NAT-PMP as `natpmp` says, and UPnP from
    /// its own address if `upnp`; its table empty, its address
    /// [`INTERNET`].
    #[must_use]
    pub fn with(natpmp: NatPmp, upnp: bool) -> Self {
        Self {
            natpmp,
            natpmp_refuses: None,
            epoch: 1000,
            upnp_from: upnp.then_some(GATEWAY),
            location: LOCATION.to_owned(),
            description: description("/ctl/IPConn"),
            permanent_only: false,
            refuses_changes: None,
            strict: false,
            external: INTERNET.to_string(),
            others: Vec::new(),
            table: BTreeMap::new(),
            sent: Vec::new(),
            waits: Vec::new(),
        }
    }

    fn natpmp_answer(&mut self, request: &[u8]) -> Vec<u8> {
        let epoch = self.epoch.to_be_bytes();
        let code = self.natpmp_refuses.unwrap_or(0).to_be_bytes();
        match *request {
            [0, 0] => {
                let mut a = vec![0, 128];
                a.extend(code);
                a.extend(epoch);
                a.extend(
                    self.external
                        .parse::<Ipv4Addr>()
                        .unwrap_or(Ipv4Addr::UNSPECIFIED)
                        .octets(),
                );
                a
            }
            [0, op @ (1 | 2), 0, 0, i0, i1, e0, e1, l0, l1, l2, l3] => {
                let protocol = if op == 1 {
                    Protocol::Udp
                } else {
                    Protocol::Tcp
                };
                let internal = u16::from_be_bytes([i0, i1]);
                let mut external = u16::from_be_bytes([e0, e1]);
                let lifetime = u32::from_be_bytes([l0, l1, l2, l3]);
                if self.natpmp_refuses.is_none() {
                    // This client's mapping of the port goes either way.
                    self.table.retain(|(p, _), e| {
                        !(*p == protocol && e.client == HERE && e.internal == internal)
                    });
                    if lifetime == 0 {
                        external = 0;
                    } else {
                        // A port held by another is not given: the next free
                        // one is.
                        while self.table.contains_key(&(protocol, external)) {
                            external = external.wrapping_add(1);
                        }
                        self.table.insert(
                            (protocol, external),
                            Entry {
                                client: HERE,
                                internal,
                                lease: lifetime,
                                description: String::new(),
                            },
                        );
                    }
                }
                let mut a = vec![0, 128 | op];
                a.extend(code);
                a.extend(epoch);
                a.extend(internal.to_be_bytes());
                a.extend(external.to_be_bytes());
                a.extend(lifetime.to_be_bytes());
                a
            }
            _ => panic!("the client sent a datagram RFC 6886 has no form for: {request:?}"),
        }
    }

    fn soap_answer(&mut self, request: &HttpRequest) -> HttpAnswer {
        let body = String::from_utf8(request.body.clone()).unwrap_or_default();
        let envelope = xml::parse(&body).unwrap_or_default();
        let call = envelope
            .child("Body")
            .and_then(|b| b.children.first())
            .cloned()
            .unwrap_or_default();
        let action = call.name().to_owned();
        assert!(
            request
                .headers
                .contains(&("SOAPAction", format!("\"{SERVICE}#{action}\""))),
            "{action} without its SOAPAction header: {:?}",
            request.headers
        );
        self.sent.push(format!("POST {action}"));
        let arg = |name: &str| call.child_text(name).unwrap_or_default().to_owned();
        let protocol = match arg("NewProtocol").as_str() {
            "TCP" => Protocol::Tcp,
            _ => Protocol::Udp,
        };
        let external: u16 = arg("NewExternalPort").parse().unwrap_or(0);
        match action.as_str() {
            "GetExternalIPAddress" => {
                ok(&action, &[("NewExternalIPAddress", self.external.clone())])
            }
            "AddPortMapping" => {
                if let Some(code) = self.refuses_changes {
                    return fault(code, "ActionNotAuthorized");
                }
                let lease: u32 = arg("NewLeaseDuration").parse().unwrap_or(0);
                if self.permanent_only && lease != 0 {
                    return fault(725, "OnlyPermanentLeasesSupported");
                }
                let entry = Entry {
                    client: arg("NewInternalClient")
                        .parse()
                        .unwrap_or(Ipv4Addr::UNSPECIFIED),
                    internal: arg("NewInternalPort").parse().unwrap_or(0),
                    lease,
                    description: arg("NewPortMappingDescription"),
                };
                if let Some(held) = self.table.get(&(protocol, external))
                    && (self.strict
                        || held.client != entry.client
                        || held.internal != entry.internal)
                {
                    return fault(718, "ConflictInMappingEntry");
                }
                self.table.insert((protocol, external), entry);
                ok(&action, &[])
            }
            "GetSpecificPortMappingEntry" => match self.table.get(&(protocol, external)) {
                Some(e) => ok(
                    &action,
                    &[
                        ("NewInternalPort", e.internal.to_string()),
                        ("NewInternalClient", e.client.to_string()),
                        ("NewEnabled", "1".to_owned()),
                        ("NewPortMappingDescription", e.description.clone()),
                        ("NewLeaseDuration", e.lease.to_string()),
                    ],
                ),
                None => fault(714, "NoSuchEntryInArray"),
            },
            "DeletePortMapping" => {
                if let Some(code) = self.refuses_changes {
                    return fault(code, "ActionNotAuthorized");
                }
                match self.table.remove(&(protocol, external)) {
                    Some(_) => ok(&action, &[]),
                    None => fault(714, "NoSuchEntryInArray"),
                }
            }
            other => panic!("an action the router does not know: {other}"),
        }
    }
}

/// A SOAP response to `action` with `args`.
fn ok(action: &str, args: &[(&str, String)]) -> HttpAnswer {
    let mut body = format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
         <u:{action}Response xmlns:u=\"{SERVICE}\">"
    );
    for (name, value) in args {
        body.push_str(&format!("<{name}>{value}</{name}>"));
    }
    body.push_str(&format!("</u:{action}Response></s:Body></s:Envelope>\r\n"));
    HttpAnswer {
        status: 200,
        body: body.into_bytes(),
    }
}

/// A SOAP fault carrying UPnP error `code`.
fn fault(code: u16, description: &str) -> HttpAnswer {
    HttpAnswer {
        status: 500,
        body: format!(
            "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><s:Fault>\
             <faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail>\
             <UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>{code}</errorCode>\
             <errorDescription>{description}</errorDescription></UPnPError></detail>\
             </s:Fault></s:Body></s:Envelope>\r\n"
        )
        .into_bytes(),
    }
}

impl Net for SimulatedLan {
    fn udp_ask(
        &mut self,
        to: SocketAddrV4,
        request: &[u8],
        wait: Duration,
    ) -> Result<Option<Vec<u8>>, String> {
        assert_eq!(to, SocketAddrV4::new(GATEWAY, natpmp::PORT));
        self.sent.push(format!("natpmp {request:?}"));
        self.waits.push(wait);
        match self.natpmp {
            NatPmp::Absent => Err("the router refused the connection".to_owned()),
            NatPmp::Silent => Ok(None),
            NatPmp::Speaks => Ok(Some(self.natpmp_answer(request))),
        }
    }

    fn ssdp_search(
        &mut self,
        searches: &[Vec<u8>],
        listen: Duration,
    ) -> Result<Vec<(Ipv4Addr, Vec<u8>)>, String> {
        assert_eq!(listen, Duration::from_secs(3));
        self.sent.push(format!("ssdp x{}", searches.len()));
        let mut answers = self.others.clone();
        if let Some(from) = self.upnp_from {
            for search in searches {
                let text = String::from_utf8(search.clone()).unwrap_or_default();
                let st = text
                    .lines()
                    .find_map(|l| l.strip_prefix("ST: "))
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                answers.push((
                    from,
                    format!(
                        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nST: {st}\r\n\
                         USN: uuid:0::{st}\r\nEXT:\r\nSERVER: OpenWRT UPnP/1.1 MiniUPnPd/2.3.3\r\n\
                         LOCATION: {}\r\n\r\n",
                        self.location
                    )
                    .into_bytes(),
                ));
            }
        }
        Ok(answers)
    }

    fn http(&mut self, request: &HttpRequest) -> Result<HttpAnswer, String> {
        match (request.method, request.url.as_str()) {
            ("GET", url) if url == self.location => {
                self.sent.push("GET description".to_owned());
                Ok(HttpAnswer {
                    status: 200,
                    body: self.description.clone().into_bytes(),
                })
            }
            ("POST", CONTROL) => Ok(self.soap_answer(request)),
            (method, url) => {
                self.sent.push(format!("{method} {url}"));
                Err(format!("cannot connect to {url}"))
            }
        }
    }
}
