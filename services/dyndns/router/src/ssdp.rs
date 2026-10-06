//! SSDP, the discovery half of UPnP (UPnP Device Architecture 1.1, §1.3): a
//! search sent to a multicast group, and the answers devices send back.
//!
//! A search is an HTTP-shaped request in one UDP datagram, to
//! 239.255.255.250:1900:
//!
//! ```text
//! M-SEARCH * HTTP/1.1
//! HOST: 239.255.255.250:1900
//! MAN: "ssdp:discover"
//! MX: 2
//! ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1
//! ```
//!
//! Every device of the type `ST` names, or of a later version of it, answers
//! within `MX` seconds with a datagram to the searcher's address and port:
//! `HTTP/1.1 200 OK`, and headers of which `LOCATION`, the URL of the device's
//! description, is the one that matters here.

use core::net::{Ipv4Addr, SocketAddrV4};

/// Where a search is sent.
pub const GROUP: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);

/// What is searched for: an Internet Gateway Device, version 1 -- which a
/// version 2 device answers too, a device answering for every version up to
/// its own (UPnP Device Architecture 1.1, §1.3.2).
pub const IGD: &str = "urn:schemas-upnp-org:device:InternetGatewayDevice:1";

/// Searched for besides: every root device. Some routers answer nothing but
/// this (miniupnpc searches for it for that reason); the answers are sorted
/// out by their descriptions, which name what each device is.
pub const ROOT_DEVICE: &str = "upnp:rootdevice";

/// How long a device may wait before it answers, in seconds: the `MX` a search
/// sends, and so how long the searcher listens (plus a second for the
/// network).
pub const MX: u8 = 2;

/// A search for `target`.
#[must_use]
pub fn search(target: &str) -> Vec<u8> {
    format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: 239.255.255.250:1900\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: {MX}\r\n\
         ST: {target}\r\n\
         USER-AGENT: SlateOS UPnP/1.1 dyndns/{}\r\n\
         \r\n",
        env!("CARGO_PKG_VERSION")
    )
    .into_bytes()
}

/// A device's answer to a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The URL of its description.
    pub location: String,
    /// What it answered as: the search target it matched.
    pub st: String,
    /// Its unique name for the answer.
    pub usn: String,
    /// What it says it runs, for showing.
    pub server: String,
}

/// Read a datagram as an answer to a search: `None` if it is not one -- not
/// `200 OK`, or with no `LOCATION` -- to be ignored.
///
/// Header names are matched without regard to case, and a value is trimmed,
/// as HTTP has them; the first of a repeated header is the one taken.
#[must_use]
pub fn read_answer(datagram: &[u8]) -> Option<Answer> {
    let text = core::str::from_utf8(datagram).ok()?;
    let mut lines = text.split("\r\n").flat_map(|l| l.split('\n'));
    let status = lines.next()?;
    let mut words = status.split_ascii_whitespace();
    let (version, code) = (words.next()?, words.next()?);
    if !version.starts_with("HTTP/1.") || code != "200" {
        return None;
    }
    let mut answer = Answer {
        location: String::new(),
        st: String::new(),
        usn: String::new(),
        server: String::new(),
    };
    for line in lines {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let slot = match name.trim().to_ascii_uppercase().as_str() {
            "LOCATION" => &mut answer.location,
            "ST" => &mut answer.st,
            "USN" => &mut answer.usn,
            "SERVER" => &mut answer.server,
            _ => continue,
        };
        if slot.is_empty() {
            value.trim().clone_into(slot);
        }
    }
    (!answer.location.is_empty()).then_some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_is_what_the_architecture_asks() {
        let text = String::from_utf8(search(IGD)).unwrap_or_default();
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines.first(), Some(&"M-SEARCH * HTTP/1.1"));
        assert!(lines.contains(&"HOST: 239.255.255.250:1900"));
        assert!(lines.contains(&"MAN: \"ssdp:discover\""));
        assert!(lines.contains(&"MX: 2"));
        assert!(lines.contains(&"ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1"));
        // It ends with the blank line that ends the headers.
        assert!(text.ends_with("\r\n\r\n"));
    }

    /// miniupnpd's answer, as it sends it.
    const MINIUPNPD: &str = "HTTP/1.1 200 OK\r\n\
        CACHE-CONTROL: max-age=120\r\n\
        ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
        USN: uuid:3c5b4d40-1dd2-11b2-a4a5-6c3b6b8e1f00::urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\
        EXT:\r\n\
        SERVER: OpenWRT/23.05 UPnP/1.1 MiniUPnPd/2.3.3\r\n\
        LOCATION: http://192.168.1.1:5000/rootDesc.xml\r\n\
        OPT: \"http://schemas.upnp.org/upnp/1/0/\"; ns=01\r\n\
        01-NLS: 1\r\n\
        BOOTID.UPNP.ORG: 1\r\n\
        CONFIGID.UPNP.ORG: 1337\r\n\
        \r\n";

    #[test]
    fn an_answer_is_read() {
        assert_eq!(
            read_answer(MINIUPNPD.as_bytes()),
            Some(Answer {
                location: "http://192.168.1.1:5000/rootDesc.xml".to_owned(),
                st: IGD.to_owned(),
                usn: "uuid:3c5b4d40-1dd2-11b2-a4a5-6c3b6b8e1f00::urn:schemas-upnp-org:device:\
                      InternetGatewayDevice:1"
                    .to_owned(),
                server: "OpenWRT/23.05 UPnP/1.1 MiniUPnPd/2.3.3".to_owned(),
            })
        );
    }

    #[test]
    fn header_names_are_any_case_and_lines_may_end_bare() {
        let answer = "HTTP/1.1 200 OK\nLocation:http://10.0.0.1:49000/igddesc.xml \n\
                      st: upnp:rootdevice\nLOCATION: http://elsewhere/\n\n";
        let read = read_answer(answer.as_bytes());
        assert_eq!(
            read.as_ref().map(|a| a.location.as_str()),
            Some("http://10.0.0.1:49000/igddesc.xml"),
            "the first LOCATION, trimmed"
        );
        assert_eq!(read.map(|a| a.st), Some(ROOT_DEVICE.to_owned()));
    }

    #[test]
    fn what_is_not_an_answer_is_ignored() {
        for datagram in [
            // Another searcher's search, a notification, an error, nothing
            // to fetch, not text at all.
            "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nST: ssdp:all\r\n\r\n".as_bytes(),
            b"NOTIFY * HTTP/1.1\r\nLOCATION: http://192.168.1.1/\r\n\r\n",
            b"HTTP/1.1 404 Not Found\r\nLOCATION: http://192.168.1.1/\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\n\r\n",
            b"\xff\xfe\x00",
            b"",
        ] {
            assert_eq!(read_answer(datagram), None, "{datagram:?}");
        }
    }

    #[test]
    fn headers_end_at_the_blank_line() {
        let answer = "HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\n\r\nLOCATION: http://late/\r\n";
        assert_eq!(read_answer(answer.as_bytes()), None);
    }
}
