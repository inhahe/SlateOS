//! Asking the home router for its internet address, and for port forwards.
//!
//! A home network reaches the internet through a router that shares one
//! internet address among its computers (NAT). Two things about that router
//! matter to a computer that wants to be reachable from outside:
//!
//! - **its internet address** -- what a dynamic-DNS hostname has to point at
//!   (the `dyndns` service publishes it);
//! - **port forwards** -- a port on that address the router passes through to
//!   this computer, without which nothing outside can connect in.
//!
//! Routers answer both over one of two protocols, and this crate speaks both,
//! asking the one that answers ("UPnP or NAT-PMP, whatever's detected", as
//! `design.txt` puts it):
//!
//! - **NAT-PMP** (RFC 6886, [`natpmp`]): small UDP datagrams to the router's
//!   port 5351. Apple's routers and most built on miniupnpd (OpenWrt and its
//!   kin) speak it. Asked first: one datagram says whether it is there.
//! - **UPnP IGD** ([`ssdp`], [`igd`]): a multicast search finds the router's
//!   description, an XML document naming the service that maps ports; that
//!   service is asked in SOAP over HTTP. Most consumer routers speak it.
//!
//! # Only the router is asked
//!
//! Both protocols are unauthenticated: anything on the network can answer a
//! search. So only the default gateway is believed: a NAT-PMP answer must
//! come from it, a UPnP device must answer the search from its address, and
//! the description's and the control service's URLs must point at it. A
//! device elsewhere on the network that claims to be the router -- to be
//! told where to forward ports, or to hand the dynamic-DNS service an address
//! of its choosing -- is not heard.
//!
//! # No transport
//!
//! Datagrams and HTTP requests leave through a [`Net`] the caller supplies:
//! the service's carries them over the network, and the tests' is a router
//! simulated in memory, so what is sent and how each answer is read is tested
//! without one.

pub mod igd;
pub mod natpmp;
#[cfg(any(test, feature = "simulate"))]
pub mod simulate;
pub mod ssdp;
pub mod xml;

use core::fmt;
use core::net::{Ipv4Addr, SocketAddrV4};
use core::time::Duration;

/// A transport protocol a port is forwarded for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    /// TCP.
    Tcp,
    /// UDP.
    Udp,
}

impl Protocol {
    /// As UPnP names it: `TCP` or `UDP`.
    #[must_use]
    pub const fn upnp_name(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }

    /// As configuration and status files write it: `tcp` or `udp`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.upnp_name())
    }
}

/// An HTTP request to the router: always plain `http://`, on its own network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    /// `GET` or `POST`.
    pub method: &'static str,
    /// The whole URL.
    pub url: String,
    /// Headers beyond the transport's own (`Host`, `Content-Length`,
    /// `Connection`).
    pub headers: Vec<(&'static str, String)>,
    /// The body; empty for a `GET`.
    pub body: Vec<u8>,
}

/// The router's answer to an [`HttpRequest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpAnswer {
    /// The HTTP status.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

/// What carries datagrams and requests to the router.
pub trait Net {
    /// Send `request` to `to` over UDP, and wait up to `wait` for a datagram
    /// back from that address and port: `Ok(None)` if none came.
    ///
    /// # Errors
    ///
    /// Why it could not be sent, or why the router turned it away (nothing
    /// listening on the port, so the router answered "port unreachable").
    fn udp_ask(
        &mut self,
        to: SocketAddrV4,
        request: &[u8],
        wait: Duration,
    ) -> Result<Option<Vec<u8>>, String>;

    /// Send each of `searches` to the SSDP group ([`ssdp::GROUP`]), and gather
    /// every datagram that comes back within `listen`, with the address each
    /// came from.
    ///
    /// # Errors
    ///
    /// Why the searches could not be sent.
    fn ssdp_search(
        &mut self,
        searches: &[Vec<u8>],
        listen: Duration,
    ) -> Result<Vec<(Ipv4Addr, Vec<u8>)>, String>;

    /// Make `request` and read the whole answer.
    ///
    /// # Errors
    ///
    /// Why there is no answer: the router could not be reached, or the
    /// connection failed.
    fn http(&mut self, request: &HttpRequest) -> Result<HttpAnswer, String>;
}

/// What went wrong asking the router.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// It could not be asked, or did not answer; the reason in a phrase.
    Unreachable(String),
    /// It speaks neither protocol -- or not to this computer: each one's
    /// reason.
    NotSpoken {
        /// Why NAT-PMP was not it.
        natpmp: String,
        /// Why UPnP was not it.
        upnp: String,
    },
    /// It refused.
    Refused {
        /// Its own words: a NAT-PMP result code, or a UPnP error.
        answer: String,
        /// What they mean, as a clause following "the router".
        meaning: String,
    },
    /// Its answer could not be read.
    Unreadable(String),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(why) | Self::Unreadable(why) => f.write_str(why),
            Self::NotSpoken { natpmp, upnp } => {
                write!(f, "it answers neither NAT-PMP ({natpmp}) nor UPnP ({upnp})")
            }
            Self::Refused { answer, meaning } => write!(f, "the router {meaning} ({answer})"),
        }
    }
}

/// How the router is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Method {
    /// NAT-PMP.
    NatPmp,
    /// UPnP IGD, through this service.
    Upnp(igd::Control),
}

impl Method {
    /// The protocol's name, for showing: `NAT-PMP` or `UPnP`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::NatPmp => "NAT-PMP",
            Self::Upnp(_) => "UPnP",
        }
    }
}

/// A router that answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Router {
    /// Its address on this network: the default gateway.
    pub address: Ipv4Addr,
    /// How it is asked.
    pub method: Method,
    /// What it calls itself (UPnP's friendly name and model); empty for
    /// NAT-PMP, which says nothing of itself.
    pub name: String,
}

/// The router's internet address, as it reported it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct External {
    /// The address.
    pub address: Ipv4Addr,
    /// NAT-PMP's seconds since start of epoch, with it; `None` for UPnP.
    pub epoch: Option<u32>,
}

impl External {
    /// Whether the address is one the internet can reach: not a private,
    /// shared (100.64.0.0/10, a provider's own NAT) or otherwise special
    /// one. A router that reports such an address is behind another NAT, and
    /// neither its address nor its port forwards reach this network from the
    /// internet.
    #[must_use]
    pub fn is_public(&self) -> bool {
        let a = self.address;
        let [first, second, ..] = a.octets();
        let shared = first == 100 && (64..128).contains(&second);
        !(a.is_unspecified()
            || a.is_loopback()
            || a.is_private()
            || a.is_link_local()
            || a.is_multicast()
            || a.is_broadcast()
            || a.is_documentation()
            || shared
            || first == 0
            || first >= 240)
    }
}

/// A port forward: `external_port` on the router's internet side passed
/// through to `internal_port` on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forward {
    /// TCP or UDP.
    pub protocol: Protocol,
    /// The port here.
    pub internal_port: u16,
    /// The port on the internet side.
    pub external_port: u16,
    /// What it is for, as the router's own list shows it.
    pub description: String,
}

/// A forward the router made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapped {
    /// The port on the internet side: the one asked for, or -- NAT-PMP only
    /// -- another the router chose when that one was taken.
    pub external_port: u16,
    /// How long it lasts, in seconds. 0 for a permanent one, from a UPnP
    /// router that keeps no other kind: it stays until removed.
    pub lifetime: u32,
    /// NAT-PMP's seconds since start of epoch, with it; `None` for UPnP.
    pub epoch: Option<u32>,
}

/// How long a UPnP mapping is asked for, in seconds; asked for again at
/// half. An hour, as miniupnpc's own client asks: long enough to be cheap,
/// short enough that a forward this computer no longer wants -- because it
/// crashed, or left the network -- does not stay open for long.
pub const UPNP_LEASE: u32 = 3600;

/// How long after a search answers are waited for: the `MX` the search
/// gives devices, and a second for the network.
#[must_use]
pub fn search_listen() -> Duration {
    Duration::from_secs(u64::from(ssdp::MX).saturating_add(1))
}

/// Find the router at `gateway` and ask its internet address: NAT-PMP first,
/// then UPnP IGD.
///
/// # Errors
///
/// [`Problem::NotSpoken`] when neither answered, with each one's reason;
/// otherwise what the protocol that answered said -- a NAT-PMP refusal when
/// UPnP is not there either (a router whose owner turned NAT-PMP off says
/// so).
pub fn discover(net: &mut dyn Net, gateway: Ipv4Addr) -> Result<(Router, External), Problem> {
    let natpmp = match natpmp_external(net, gateway) {
        Ok(external) => {
            return Ok((
                Router {
                    address: gateway,
                    method: Method::NatPmp,
                    name: String::new(),
                },
                external,
            ));
        }
        Err(problem) => problem,
    };
    match find_upnp(net, gateway) {
        Ok(found) => Ok(found),
        Err(FindUpnp::Absent(upnp)) => Err(match natpmp {
            refused @ Problem::Refused { .. } => refused,
            other => Problem::NotSpoken {
                natpmp: other.to_string(),
                upnp,
            },
        }),
        Err(FindUpnp::Problem(problem)) => Err(problem),
    }
}

impl Router {
    /// Ask the router its internet address again.
    ///
    /// # Errors
    ///
    /// Why it could not say.
    pub fn external(&self, net: &mut dyn Net) -> Result<External, Problem> {
        match &self.method {
            Method::NatPmp => natpmp_external(net, self.address),
            Method::Upnp(control) => upnp_external(net, control),
        }
    }

    /// Ask the router to forward `forward` to `here`, this computer's address
    /// on the network, for `lifetime` seconds (NAT-PMP; a UPnP mapping is
    /// asked for [`UPNP_LEASE`]).
    ///
    /// # Errors
    ///
    /// Why it was not made.
    pub fn map(
        &self,
        net: &mut dyn Net,
        here: Ipv4Addr,
        forward: &Forward,
        lifetime: u32,
    ) -> Result<Mapped, Problem> {
        match &self.method {
            Method::NatPmp => natpmp_map(net, self.address, forward, lifetime),
            Method::Upnp(control) => upnp_map(net, control, here, forward),
        }
    }

    /// Ask the router to stop forwarding `forward`, which it mapped to
    /// `mapped.external_port`. A forward it no longer has is taken as
    /// removed.
    ///
    /// # Errors
    ///
    /// Why it was not removed.
    pub fn unmap(
        &self,
        net: &mut dyn Net,
        forward: &Forward,
        mapped: &Mapped,
    ) -> Result<(), Problem> {
        match &self.method {
            Method::NatPmp => natpmp_unmap(net, self.address, forward),
            Method::Upnp(control) => {
                upnp_unmap(net, control, forward.protocol, mapped.external_port)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// NAT-PMP
// ---------------------------------------------------------------------------

/// Send `request` to the router's NAT-PMP port until `read` finds an answer
/// in what comes back, [`natpmp::SENDS`] times at the most: `None` if none
/// did.
fn natpmp_ask<T>(
    net: &mut dyn Net,
    gateway: Ipv4Addr,
    request: &[u8],
    read: impl Fn(&[u8]) -> Option<T>,
) -> Result<Option<T>, Problem> {
    let to = SocketAddrV4::new(gateway, natpmp::PORT);
    for send in 0..natpmp::SENDS {
        let datagram = net
            .udp_ask(to, request, natpmp::wait_after(send))
            .map_err(Problem::Unreachable)?;
        if let Some(answer) = datagram.as_deref().and_then(&read) {
            return Ok(Some(answer));
        }
    }
    Ok(None)
}

/// No answer from NAT-PMP, as a problem.
fn natpmp_silent() -> Problem {
    Problem::Unreachable(format!(
        "no answer from its NAT-PMP port in {} tries",
        natpmp::SENDS
    ))
}

/// A NAT-PMP refusal, as a problem.
fn natpmp_refused(refused: natpmp::Refused) -> Problem {
    Problem::Refused {
        answer: format!("NAT-PMP result code {}", refused.refusal.code()),
        meaning: refused.refusal.meaning(),
    }
}

fn natpmp_external(net: &mut dyn Net, gateway: Ipv4Addr) -> Result<External, Problem> {
    match natpmp_ask(
        net,
        gateway,
        &natpmp::external_address_request(),
        natpmp::read_external_address,
    )? {
        Some(Ok(answer)) => Ok(External {
            address: answer.address,
            epoch: Some(answer.epoch),
        }),
        Some(Err(refused)) => Err(natpmp_refused(refused)),
        None => Err(natpmp_silent()),
    }
}

fn natpmp_map(
    net: &mut dyn Net,
    gateway: Ipv4Addr,
    forward: &Forward,
    lifetime: u32,
) -> Result<Mapped, Problem> {
    let request = natpmp::map_request(
        forward.protocol,
        forward.internal_port,
        forward.external_port,
        lifetime.max(1),
    );
    match natpmp_ask(net, gateway, &request, |b| {
        natpmp::read_map(b, forward.protocol, forward.internal_port)
    })? {
        Some(Ok(granted)) if granted.lifetime == 0 || granted.external_port == 0 => {
            Err(Problem::Unreadable(
                "it answered the request with a mapping of no port, or for no time".to_owned(),
            ))
        }
        Some(Ok(granted)) => Ok(Mapped {
            external_port: granted.external_port,
            lifetime: granted.lifetime,
            epoch: Some(granted.epoch),
        }),
        Some(Err(refused)) => Err(natpmp_refused(refused)),
        None => Err(natpmp_silent()),
    }
}

fn natpmp_unmap(net: &mut dyn Net, gateway: Ipv4Addr, forward: &Forward) -> Result<(), Problem> {
    let request = natpmp::unmap_request(forward.protocol, forward.internal_port);
    match natpmp_ask(net, gateway, &request, |b| {
        natpmp::read_map(b, forward.protocol, forward.internal_port)
    })? {
        Some(Ok(_)) => Ok(()),
        Some(Err(refused)) => Err(natpmp_refused(refused)),
        None => Err(natpmp_silent()),
    }
}

// ---------------------------------------------------------------------------
// UPnP IGD
// ---------------------------------------------------------------------------

/// Why UPnP was not found: nothing there, or something there that failed.
enum FindUpnp {
    /// No UPnP router: why, in a phrase.
    Absent(String),
    /// A UPnP router that could not be used.
    Problem(Problem),
}

/// Whether `url` is an `http://` URL on `host`.
fn url_on(url: &str, host: Ipv4Addr) -> bool {
    igd::host_port(url).is_some_and(|(h, _)| h.parse::<Ipv4Addr>() == Ok(host))
}

fn find_upnp(net: &mut dyn Net, gateway: Ipv4Addr) -> Result<(Router, External), FindUpnp> {
    let searches = [ssdp::search(ssdp::IGD), ssdp::search(ssdp::ROOT_DEVICE)];
    let answers = net
        .ssdp_search(&searches, search_listen())
        .map_err(|why| FindUpnp::Problem(Problem::Unreachable(why)))?;
    let mut locations: Vec<String> = Vec::new();
    let mut elsewhere = 0usize;
    for (from, datagram) in &answers {
        let Some(answer) = ssdp::read_answer(datagram) else {
            continue;
        };
        if *from != gateway || !url_on(&answer.location, gateway) {
            elsewhere = elsewhere.saturating_add(1);
            continue;
        }
        if !locations.contains(&answer.location) {
            locations.push(answer.location);
        }
    }
    if locations.is_empty() {
        return Err(FindUpnp::Absent(if elsewhere == 0 {
            "nothing answered a UPnP search".to_owned()
        } else {
            format!(
                "{elsewhere} UPnP answer{} came from devices other than the router",
                if elsewhere == 1 { "" } else { "s" }
            )
        }));
    }
    // What went wrong last, if nothing goes right.
    let mut last = FindUpnp::Absent("no UPnP device of the router's could be read".to_owned());
    for location in &locations {
        let description = match fetch_description(net, location) {
            Ok(d) => d,
            Err(e) => {
                last = FindUpnp::Problem(Problem::Unreachable(e));
                continue;
            }
        };
        if description.controls.is_empty() {
            last = FindUpnp::Absent(format!(
                "its UPnP description ({location}) offers no port mapping service"
            ));
            continue;
        }
        let name = [
            description.friendly_name.as_str(),
            description.model.as_str(),
        ]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
        for control in description.controls {
            if !url_on(&control.url, gateway) {
                last = FindUpnp::Problem(Problem::Unreachable(format!(
                    "its port mapping service is at {}, which is not the router; not used",
                    control.url
                )));
                continue;
            }
            match upnp_external(net, &control) {
                Ok(external) => {
                    return Ok((
                        Router {
                            address: gateway,
                            method: Method::Upnp(control),
                            name: name.clone(),
                        },
                        external,
                    ));
                }
                Err(problem) => last = FindUpnp::Problem(problem),
            }
        }
    }
    Err(last)
}

/// GET the description at `location`.
fn fetch_description(net: &mut dyn Net, location: &str) -> Result<igd::Description, String> {
    let answer = net
        .http(&HttpRequest {
            method: "GET",
            url: location.to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        })
        .map_err(|e| format!("its UPnP description ({location}) could not be fetched: {e}"))?;
    if answer.status != 200 {
        return Err(format!(
            "its UPnP description ({location}) answered HTTP {}",
            answer.status
        ));
    }
    let text = String::from_utf8(answer.body)
        .map_err(|_| format!("its UPnP description ({location}) is not UTF-8 text"))?;
    igd::read_description(&text, location)
}

/// Send `action` with `args` to `control`.
fn soap(
    net: &mut dyn Net,
    control: &igd::Control,
    action: &str,
    args: &[(&str, String)],
) -> Result<igd::SoapAnswer, Problem> {
    let request = HttpRequest {
        method: "POST",
        url: control.url.clone(),
        headers: vec![
            ("Content-Type", "text/xml; charset=\"utf-8\"".to_owned()),
            (
                "SOAPAction",
                igd::soap_action(&control.service_type, action),
            ),
        ],
        body: igd::soap_envelope(&control.service_type, action, args).into_bytes(),
    };
    let answer = net
        .http(&request)
        .map_err(|e| Problem::Unreachable(format!("{action}: {e}")))?;
    let text = String::from_utf8(answer.body).map_err(|_| {
        Problem::Unreadable(format!(
            "{action}: HTTP {}, and an answer that is not UTF-8 text",
            answer.status
        ))
    })?;
    igd::read_soap_answer(answer.status, &text, action)
        .map_err(|e| Problem::Unreadable(format!("{action}: {e}")))
}

/// A UPnP refusal, as a problem.
fn upnp_refused(code: u16, description: &str) -> Problem {
    Problem::Refused {
        answer: if description.is_empty() {
            format!("UPnP error {code}")
        } else {
            format!("UPnP error {code}, {description}")
        },
        meaning: igd::error_meaning(code).to_owned(),
    }
}

fn upnp_external(net: &mut dyn Net, control: &igd::Control) -> Result<External, Problem> {
    match soap(net, control, "GetExternalIPAddress", &[])? {
        igd::SoapAnswer::Refused { code, description } => Err(upnp_refused(code, &description)),
        done @ igd::SoapAnswer::Done(_) => {
            let text = done.get("NewExternalIPAddress").unwrap_or_default();
            if text.is_empty() {
                return Err(Problem::Refused {
                    answer: "an empty NewExternalIPAddress".to_owned(),
                    meaning: "has no internet address of its own yet".to_owned(),
                });
            }
            let address = text.parse::<Ipv4Addr>().map_err(|_| {
                Problem::Unreadable(format!(
                    "GetExternalIPAddress: {text:?} is not an IPv4 address"
                ))
            })?;
            Ok(External {
                address,
                epoch: None,
            })
        }
    }
}

/// The arguments naming a mapping: no remote host (any), the external port,
/// the protocol.
fn mapping_key(protocol: Protocol, external_port: u16) -> [(&'static str, String); 3] {
    [
        ("NewRemoteHost", String::new()),
        ("NewExternalPort", external_port.to_string()),
        ("NewProtocol", protocol.upnp_name().to_owned()),
    ]
}

fn add_port_mapping(
    net: &mut dyn Net,
    control: &igd::Control,
    here: Ipv4Addr,
    forward: &Forward,
    lease: u32,
) -> Result<igd::SoapAnswer, Problem> {
    let mut args = mapping_key(forward.protocol, forward.external_port).to_vec();
    args.extend([
        ("NewInternalPort", forward.internal_port.to_string()),
        ("NewInternalClient", here.to_string()),
        ("NewEnabled", "1".to_owned()),
        ("NewPortMappingDescription", forward.description.clone()),
        ("NewLeaseDuration", lease.to_string()),
    ]);
    soap(net, control, "AddPortMapping", &args)
}

/// Add the mapping for [`UPNP_LEASE`], or permanently if the router keeps
/// only permanent ones (error 725, IGD v1's).
fn add_with_lease(
    net: &mut dyn Net,
    control: &igd::Control,
    here: Ipv4Addr,
    forward: &Forward,
) -> Result<Mapped, Problem> {
    let mut lease = UPNP_LEASE;
    loop {
        match add_port_mapping(net, control, here, forward, lease)? {
            igd::SoapAnswer::Done(_) => {
                return Ok(Mapped {
                    external_port: forward.external_port,
                    lifetime: lease,
                    epoch: None,
                });
            }
            igd::SoapAnswer::Refused { code: 725, .. } if lease != 0 => lease = 0,
            igd::SoapAnswer::Refused { code, description } => {
                return Err(upnp_refused(code, &description));
            }
        }
    }
}

fn upnp_map(
    net: &mut dyn Net,
    control: &igd::Control,
    here: Ipv4Addr,
    forward: &Forward,
) -> Result<Mapped, Problem> {
    let conflict = match add_with_lease(net, control, here, forward) {
        Err(Problem::Refused { answer, meaning }) if answer.starts_with("UPnP error 718") => {
            Problem::Refused { answer, meaning }
        }
        other => return other,
    };
    // Someone holds the external port. If it is this computer's own mapping
    // of the same port -- left by an earlier run, or by this one before the
    // router forgot it had renewed -- it is replaced, so that its lease and
    // description are this run's; anyone else's is left alone and named.
    let entry = soap(
        net,
        control,
        "GetSpecificPortMappingEntry",
        &mapping_key(forward.protocol, forward.external_port),
    )?;
    let client = entry
        .get("NewInternalClient")
        .unwrap_or_default()
        .to_owned();
    let port = entry.get("NewInternalPort").unwrap_or_default().to_owned();
    let ours =
        client.parse::<Ipv4Addr>() == Ok(here) && port.parse::<u16>() == Ok(forward.internal_port);
    if !ours {
        return Err(match entry {
            igd::SoapAnswer::Done(_) => Problem::Refused {
                answer: "UPnP error 718, ConflictInMappingEntry".to_owned(),
                meaning: format!(
                    "already forwards external port {} {} to {client}, port {port}",
                    forward.external_port, forward.protocol
                ),
            },
            igd::SoapAnswer::Refused { .. } => conflict,
        });
    }
    upnp_unmap(net, control, forward.protocol, forward.external_port)?;
    add_with_lease(net, control, here, forward)
}

fn upnp_unmap(
    net: &mut dyn Net,
    control: &igd::Control,
    protocol: Protocol,
    external_port: u16,
) -> Result<(), Problem> {
    match soap(
        net,
        control,
        "DeletePortMapping",
        &mapping_key(protocol, external_port),
    )? {
        // 714: it has no such mapping, which is what was wanted.
        igd::SoapAnswer::Done(_) | igd::SoapAnswer::Refused { code: 714, .. } => Ok(()),
        igd::SoapAnswer::Refused { code, description } => Err(upnp_refused(code, &description)),
    }
}

#[cfg(test)]
mod tests;
