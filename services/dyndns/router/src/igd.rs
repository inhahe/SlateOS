//! UPnP IGD control: finding, in a router's description, the service that
//! maps ports -- and the SOAP actions sent to it (UPnP Forum, Internet
//! Gateway Device v1 and v2: `WANIPConnection` and `WANPPPConnection`).
//!
//! A router's description lists devices within devices; the service wanted
//! is inside its `WANConnectionDevice`:
//!
//! ```text
//! InternetGatewayDevice
//!   WANDevice
//!     WANConnectionDevice
//!       service  urn:schemas-upnp-org:service:WANIPConnection:1   controlURL /ctl/IPConn
//! ```
//!
//! An action is an HTTP POST of a SOAP envelope to the service's control URL,
//! naming the action in a `SOAPAction` header; the answer is an envelope
//! holding the action's out-arguments, or a fault holding a UPnP error code.

use crate::xml::{self, Element};

/// The service type of a connection through which the router maps ports,
/// without its version: an IP connection (a router that is its own modem, or
/// behind one) ...
pub const WAN_IP_CONNECTION: &str = "urn:schemas-upnp-org:service:WANIPConnection:";

/// ... or a PPP one (a router that dials its provider over PPPoE).
pub const WAN_PPP_CONNECTION: &str = "urn:schemas-upnp-org:service:WANPPPConnection:";

/// A service that maps ports, from a description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// Its type, version and all.
    pub service_type: String,
    /// Where its actions go: an absolute `http://` URL.
    pub url: String,
}

impl Control {
    /// The version of the service: the number after its type's last colon.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.service_type
            .rsplit_once(':')
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(1)
    }
}

/// What a router's description says that matters here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Description {
    /// Its name for itself, for showing.
    pub friendly_name: String,
    /// Who made it.
    pub manufacturer: String,
    /// Its model.
    pub model: String,
    /// The services that map ports: IP connections before PPP ones, later
    /// versions first, and otherwise in the order the description lists
    /// them.
    pub controls: Vec<Control>,
}

/// Read a router's description, fetched from `location`.
///
/// # Errors
///
/// Why it cannot be read: not XML these rules read, or not a description.
pub fn read_description(doc: &str, location: &str) -> Result<Description, String> {
    let root = xml::parse(doc).map_err(|e| format!("its description is not readable XML: {e}"))?;
    if root.name() != "root" {
        return Err(format!(
            "its description is <{}>, not a device description",
            root.name()
        ));
    }
    // URLBase is UPnP 1.0's; 1.1 drops it, and the description's own URL is
    // the base.
    let base = root
        .child_text("URLBase")
        .filter(|b| !b.is_empty())
        .unwrap_or(location);
    let device = root.child("device");
    let field = |name: &str| {
        device
            .and_then(|d| d.child_text(name))
            .unwrap_or_default()
            .to_owned()
    };
    let mut controls: Vec<(u8, u32, usize, Control)> = Vec::new();
    for (order, service) in root.find_all("service").into_iter().enumerate() {
        let Some(service_type) = service.child_text("serviceType") else {
            continue;
        };
        let kind = if service_type.starts_with(WAN_IP_CONNECTION) {
            0
        } else if service_type.starts_with(WAN_PPP_CONNECTION) {
            1
        } else {
            continue;
        };
        let Some(url) = service
            .child_text("controlURL")
            .and_then(|reference| resolve(base, reference))
        else {
            continue;
        };
        let control = Control {
            service_type: service_type.to_owned(),
            url,
        };
        controls.push((kind, control.version(), order, control));
    }
    controls.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    Ok(Description {
        friendly_name: field("friendlyName"),
        manufacturer: field("manufacturer"),
        model: field("modelName"),
        controls: controls.into_iter().map(|(_, _, _, c)| c).collect(),
    })
}

/// `reference` made absolute against `base`, both `http://` URLs (or
/// `reference` a path): RFC 3986's resolution for the forms descriptions
/// use. `None` when `base` is not an `http://` URL, or `reference` is
/// another scheme's.
#[must_use]
pub fn resolve(base: &str, reference: &str) -> Option<String> {
    let reference = reference.trim();
    if has_scheme(reference, "http://") {
        return Some(reference.to_owned());
    }
    if reference.contains("://") {
        return None;
    }
    let rest = strip_scheme(base, "http://")?;
    let (authority, path) = rest.find('/').map_or((rest, "/"), |i| rest.split_at(i));
    if authority.is_empty() {
        return None;
    }
    if let Some(network_path) = reference.strip_prefix("//") {
        return Some(format!("http://{network_path}"));
    }
    if reference.starts_with('/') {
        return Some(format!("http://{authority}{reference}"));
    }
    // Relative to the base's directory: its path up to its last '/', the
    // query and fragment dropped.
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let dir = path.rfind('/').and_then(|i| path.get(..=i)).unwrap_or("/");
    Some(format!("http://{authority}{dir}{reference}"))
}

/// Whether `url` begins with `scheme` (`http://`), in any case.
fn has_scheme(url: &str, scheme: &str) -> bool {
    url.get(..scheme.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
}

/// `url` after `scheme`, if it begins with it in any case.
fn strip_scheme<'a>(url: &'a str, scheme: &str) -> Option<&'a str> {
    if has_scheme(url, scheme) {
        url.get(scheme.len()..)
    } else {
        None
    }
}

/// The host and port of an `http://` URL: `None` for another scheme.
#[must_use]
pub fn host_port(url: &str) -> Option<(&str, u16)> {
    let rest = strip_scheme(url, "http://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    // A user-info part has no place in a router's URL.
    if authority.contains('@') {
        return None;
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(']') || host.ends_with(']') => {
            Some((host, port.parse().ok()?))
        }
        _ => Some((authority, 80)),
    }
}

/// The path, query and all, of an `http://` URL: what follows its authority,
/// or `/`.
#[must_use]
pub fn path_of(url: &str) -> &str {
    strip_scheme(url, "http://")
        .and_then(|rest| rest.find('/').and_then(|i| rest.get(i..)))
        .unwrap_or("/")
}

// ---------------------------------------------------------------------------
// SOAP
// ---------------------------------------------------------------------------

/// The `SOAPAction` header for `action` on `service_type`, quotes included.
#[must_use]
pub fn soap_action(service_type: &str, action: &str) -> String {
    format!("\"{service_type}#{action}\"")
}

/// The envelope that asks `service_type` for `action` with `args`.
#[must_use]
pub fn soap_envelope(service_type: &str, action: &str, args: &[(&str, String)]) -> String {
    let mut body = String::from(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body>",
    );
    body.push_str(&format!(
        "<u:{action} xmlns:u=\"{}\">",
        escape(service_type)
    ));
    for (name, value) in args {
        body.push_str(&format!("<{name}>{}</{name}>", escape(value)));
    }
    body.push_str(&format!("</u:{action}></s:Body></s:Envelope>\r\n"));
    body
}

/// `text` with XML's five special characters escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// What came back from an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoapAnswer {
    /// It was done: its out-arguments, by name, in order.
    Done(Vec<(String, String)>),
    /// The router refused, with a UPnP error.
    Refused {
        /// The error code (`718`).
        code: u16,
        /// The router's name for it (`ConflictInMappingEntry`).
        description: String,
    },
}

impl SoapAnswer {
    /// The out-argument `name` of a done action.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        match self {
            Self::Done(args) => args
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str()),
            Self::Refused { .. } => None,
        }
    }
}

/// Read the answer to `action`: HTTP `status`, then `body`.
///
/// # Errors
///
/// What was wrong with it, when it is neither the action's response nor a
/// UPnP error.
pub fn read_soap_answer(status: u16, body: &str, action: &str) -> Result<SoapAnswer, String> {
    let envelope = match xml::parse(body) {
        Ok(e) if e.name() == "Envelope" => e,
        Ok(e) => {
            return Err(format!(
                "HTTP {status}, and <{}> where a SOAP envelope should be",
                e.name()
            ));
        }
        Err(e) => return Err(format!("HTTP {status}, and no SOAP envelope: {e}")),
    };
    let body = envelope
        .child("Body")
        .ok_or_else(|| format!("HTTP {status}, and a SOAP envelope with no body"))?;
    if let Some(fault) = body.child("Fault") {
        let error = fault.find_all("UPnPError").into_iter().next();
        let code = error
            .and_then(|e| e.child_text("errorCode"))
            .and_then(|c| c.parse().ok());
        return match code {
            Some(code) => Ok(SoapAnswer::Refused {
                code,
                description: error
                    .and_then(|e| e.child_text("errorDescription"))
                    .unwrap_or_default()
                    .to_owned(),
            }),
            None => Err(format!(
                "HTTP {status}, and a SOAP fault with no UPnP error: {}",
                fault
                    .child_text("faultstring")
                    .unwrap_or("(no fault string)")
            )),
        };
    }
    let wanted = format!("{action}Response");
    let response = body.child(&wanted).ok_or_else(|| {
        format!(
            "HTTP {status}, and no <{wanted}> in its answer{}",
            body.children
                .first()
                .map(|c| format!(" (<{}> instead)", c.name()))
                .unwrap_or_default()
        )
    })?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}, though it answered <{wanted}>"));
    }
    Ok(SoapAnswer::Done(
        response
            .children
            .iter()
            .map(|e: &Element| (e.name().to_owned(), e.text().to_owned()))
            .collect(),
    ))
}

/// What a UPnP error code means, as a clause that follows "the router".
#[must_use]
pub fn error_meaning(code: u16) -> &'static str {
    match code {
        402 => "found the request's arguments invalid",
        501 => "could not carry out the request",
        606 => "does not allow it: its owner may have set UPnP to refuse changes, or to read only",
        714 => "has no such mapping",
        715 => "does not accept a wildcard source address",
        716 => "does not accept a wildcard external port",
        718 => "already maps that external port to another computer, or for another program",
        724 => "maps a port only to the same port number here",
        725 => "keeps only permanent mappings",
        726 => "accepts only a wildcard remote host",
        727 => "accepts only a wildcard external port",
        728 => "has no room for another mapping",
        729 => "holds that port by another of its own means (a forward set by hand, say)",
        732 => "does not accept a wildcard internal port",
        _ => "refused",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A FRITZ!Box-shaped description: URLBase absent, the IGD:2 tree, the
    /// IP connection v2 and a PPP one v1 under the same device.
    const DESCRIPTION: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
<specVersion><major>1</major><minor>0</minor></specVersion>
<device>
<deviceType>urn:schemas-upnp-org:device:InternetGatewayDevice:2</deviceType>
<friendlyName>FRITZ!Box 7590</friendlyName>
<manufacturer>AVM Berlin</manufacturer>
<modelName>FRITZ!Box 7590</modelName>
<serviceList><service>
<serviceType>urn:schemas-any-com:service:Any:1</serviceType>
<controlURL>/igdupnp/control/any</controlURL>
</service></serviceList>
<deviceList><device>
<deviceType>urn:schemas-upnp-org:device:WANDevice:2</deviceType>
<deviceList><device>
<deviceType>urn:schemas-upnp-org:device:WANConnectionDevice:2</deviceType>
<serviceList>
<service>
<serviceType>urn:schemas-upnp-org:service:WANPPPConnection:1</serviceType>
<controlURL>/igdupnp/control/WANPPPConn1</controlURL>
</service>
<service>
<serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
<controlURL>igdupnp/control/WANIPConn1</controlURL>
</service>
<service>
<serviceType>urn:schemas-upnp-org:service:WANIPConnection:2</serviceType>
<controlURL>http://192.168.178.1:49000/igd2upnp/control/WANIPConn1</controlURL>
</service>
</serviceList>
</device></deviceList>
</device></deviceList>
</device>
</root>"#;

    #[test]
    fn the_mapping_services_are_found_best_first() {
        let d = read_description(DESCRIPTION, "http://192.168.178.1:49000/igd2desc.xml")
            .unwrap_or_default();
        assert_eq!(d.friendly_name, "FRITZ!Box 7590");
        assert_eq!(d.manufacturer, "AVM Berlin");
        assert_eq!(d.model, "FRITZ!Box 7590");
        let found: Vec<(&str, &str)> = d
            .controls
            .iter()
            .map(|c| (c.service_type.as_str(), c.url.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (
                    "urn:schemas-upnp-org:service:WANIPConnection:2",
                    "http://192.168.178.1:49000/igd2upnp/control/WANIPConn1"
                ),
                (
                    "urn:schemas-upnp-org:service:WANIPConnection:1",
                    "http://192.168.178.1:49000/igdupnp/control/WANIPConn1"
                ),
                (
                    "urn:schemas-upnp-org:service:WANPPPConnection:1",
                    "http://192.168.178.1:49000/igdupnp/control/WANPPPConn1"
                ),
            ]
        );
        assert_eq!(d.controls.first().map(Control::version), Some(2));
    }

    #[test]
    fn url_base_is_the_base_when_given() {
        let doc = "<root><URLBase>http://10.0.0.138:80/</URLBase><device><serviceList><service>\
                   <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>\
                   <controlURL>upnp/control/WANIPConn1</controlURL></service></serviceList></device></root>";
        let d =
            read_description(doc, "http://10.0.0.138:5431/dyndev/uuid:0000").unwrap_or_default();
        assert_eq!(
            d.controls.first().map(|c| c.url.as_str()),
            Some("http://10.0.0.138:80/upnp/control/WANIPConn1")
        );
    }

    #[test]
    fn a_description_without_a_mapping_service_has_none() {
        let doc = "<root><device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>\
                   <serviceList><service><serviceType>urn:schemas-upnp-org:service:ContentDirectory:1\
                   </serviceType><controlURL>/cd</controlURL></service></serviceList></device></root>";
        let d = read_description(doc, "http://192.168.1.20:8200/rootDesc.xml");
        assert_eq!(d.map(|d| d.controls.len()), Ok(0));
        assert!(read_description("<html><body>login</body></html>", "http://x/").is_err());
        assert!(read_description("not xml", "http://x/").is_err());
    }

    #[test]
    fn references_resolve_as_rfc_3986_has_them() {
        let base = "http://192.168.1.1:5000/dir/rootDesc.xml?x=1";
        for (reference, want) in [
            ("/ctl/IPConn", Some("http://192.168.1.1:5000/ctl/IPConn")),
            ("ctl/IPConn", Some("http://192.168.1.1:5000/dir/ctl/IPConn")),
            (
                "http://192.168.1.1:5000/abs",
                Some("http://192.168.1.1:5000/abs"),
            ),
            ("HTTP://192.168.1.1/caps", Some("HTTP://192.168.1.1/caps")),
            (
                "//192.168.1.1:6000/other",
                Some("http://192.168.1.1:6000/other"),
            ),
            ("https://192.168.1.1/x", None),
            ("ftp://192.168.1.1/x", None),
        ] {
            assert_eq!(resolve(base, reference).as_deref(), want, "{reference}");
        }
        assert_eq!(
            resolve("http://192.168.1.1:5000", "ctl").as_deref(),
            Some("http://192.168.1.1:5000/ctl")
        );
        assert_eq!(resolve("https://192.168.1.1/", "ctl"), None);
        assert_eq!(resolve("http:///nohost", "ctl"), None);
    }

    #[test]
    fn hosts_ports_and_paths_are_read_from_urls() {
        assert_eq!(
            host_port("http://192.168.1.1:5000/ctl/IPConn"),
            Some(("192.168.1.1", 5000))
        );
        assert_eq!(host_port("http://192.168.1.1/x"), Some(("192.168.1.1", 80)));
        assert_eq!(host_port("HTTP://router.lan"), Some(("router.lan", 80)));
        assert_eq!(host_port("http://192.168.1.1:99999/"), None);
        assert_eq!(host_port("http://user@192.168.1.1/"), None);
        assert_eq!(host_port("https://192.168.1.1/"), None);
        assert_eq!(
            path_of("http://192.168.1.1:5000/ctl/IPConn?a=b"),
            "/ctl/IPConn?a=b"
        );
        assert_eq!(path_of("http://192.168.1.1:5000"), "/");
    }

    #[test]
    fn an_envelope_is_the_one_miniupnpc_sends() {
        let body = soap_envelope(
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "DeletePortMapping",
            &[
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", "2222".to_owned()),
                ("NewProtocol", "TCP".to_owned()),
            ],
        );
        assert_eq!(
            body,
            "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
             <u:DeletePortMapping xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\
             <NewRemoteHost></NewRemoteHost><NewExternalPort>2222</NewExternalPort>\
             <NewProtocol>TCP</NewProtocol></u:DeletePortMapping></s:Body></s:Envelope>\r\n"
        );
        assert_eq!(
            soap_action(
                "urn:schemas-upnp-org:service:WANIPConnection:1",
                "DeletePortMapping"
            ),
            "\"urn:schemas-upnp-org:service:WANIPConnection:1#DeletePortMapping\""
        );
        // A description with markup in it is escaped, and reads back whole.
        let odd = soap_envelope("t", "A", &[("D", "<a & 'b'>".to_owned())]);
        let back = xml::parse(&odd).unwrap_or_default();
        let a = back.child("Body").and_then(|b| b.child("A"));
        assert_eq!(a.and_then(|a| a.child_text("D")), Some("<a & 'b'>"));
    }

    #[test]
    fn a_response_gives_its_out_arguments() {
        let body = "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
            s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
            <u:GetExternalIPAddressResponse xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:1\">\
            <NewExternalIPAddress>203.0.113.7</NewExternalIPAddress>\
            </u:GetExternalIPAddressResponse></s:Body></s:Envelope>";
        let answer = read_soap_answer(200, body, "GetExternalIPAddress");
        assert_eq!(
            answer,
            Ok(SoapAnswer::Done(vec![(
                "NewExternalIPAddress".to_owned(),
                "203.0.113.7".to_owned()
            )]))
        );
        assert_eq!(
            answer
                .ok()
                .as_ref()
                .and_then(|a| a.get("NewExternalIPAddress")),
            Some("203.0.113.7")
        );
        // The empty response of an action with no out-arguments.
        let empty = "<s:Envelope xmlns:s=\"x\"><s:Body><u:AddPortMappingResponse xmlns:u=\"y\"/>\
                     </s:Body></s:Envelope>";
        assert_eq!(
            read_soap_answer(200, empty, "AddPortMapping"),
            Ok(SoapAnswer::Done(Vec::new()))
        );
    }

    #[test]
    fn a_fault_gives_its_upnp_error() {
        let body = "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
            s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><s:Fault>\
            <faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail>\
            <UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>718</errorCode>\
            <errorDescription>ConflictInMappingEntry</errorDescription></UPnPError>\
            </detail></s:Fault></s:Body></s:Envelope>";
        assert_eq!(
            read_soap_answer(500, body, "AddPortMapping"),
            Ok(SoapAnswer::Refused {
                code: 718,
                description: "ConflictInMappingEntry".to_owned(),
            })
        );
        assert!(!error_meaning(718).is_empty());
        assert_eq!(error_meaning(9999), "refused");
    }

    #[test]
    fn what_is_neither_is_an_error_naming_the_status() {
        for (status, body) in [
            (404, "<html><body>Not Found</body></html>"),
            (500, "Internal Server Error"),
            (
                200,
                "<s:Envelope><s:Body><u:OtherResponse/></s:Body></s:Envelope>",
            ),
            (
                500,
                "<s:Envelope><s:Body><s:Fault><faultstring>oops</faultstring></s:Fault></s:Body></s:Envelope>",
            ),
            (
                500,
                "<s:Envelope><s:Body><u:AResponse/></s:Body></s:Envelope>",
            ),
            (200, "<s:Envelope></s:Envelope>"),
        ] {
            let read = read_soap_answer(status, body, "A");
            assert!(
                read.as_ref()
                    .is_err_and(|e| e.contains(&format!("HTTP {status}"))),
                "{status} {body}: {read:?}"
            );
        }
    }
}
