//! WHOIS: who holds an address, asked of the registries themselves over TCP
//! port 43 (RFC 3912).
//!
//! IANA is asked first, and names the regional registry that holds the block
//! (its reply's `refer:` line); that registry is asked next and its record
//! read. An address IANA keeps for itself -- private, loopback, reserved --
//! has no referral, and IANA's own record is the answer.
//!
//! The registries do not agree on field names (ARIN writes `OrgName` and
//! `CIDR`, RIPE `org-name` and `inetnum`, LACNIC `owner`), so each field is
//! read from the first of several spellings present. Nothing is filled in
//! from anything but the reply: a field the record lacks stays empty, which
//! is what the window shows.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// IANA's WHOIS server, which knows which registry holds every block.
///
/// Unused in a test build, whose lookups start at a port on this machine
/// instead (`DEFAULT_WHOIS`), so that no test questions the real registries.
#[cfg_attr(test, allow(dead_code))]
pub const IANA: &str = "whois.iana.org";

/// The WHOIS port.
const PORT: u16 = 43;

/// How long a server is given to connect, and then to answer.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The most of a reply read. Records are a few kilobytes; a server that sends
/// more is sending something else, and the reply is read no further.
const MAX_REPLY: u64 = 256 * 1024;

/// What a registry's record says about an address block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub org_name: String,
    pub country: String,
    pub cidr: String,
    pub net_name: String,
    pub description: String,
    pub abuse_contact: String,
}

/// A registry's answer: the record, and which server gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub server: String,
    pub record: Record,
}

/// Who holds `ip`: IANA (`iana`, normally [`IANA`]) is asked, its referral
/// followed, and the registry's record read.
///
/// # Errors
///
/// What stopped it, in words: a server that could not be found or reached, or
/// that did not answer in time.
pub fn lookup(ip: [u8; 4], iana: &str) -> Result<Answer, String> {
    let query = ip_text(ip);
    let first = ask(iana, &query)?;
    let (server, reply) = match referral(&first) {
        Some(server) if server != iana => {
            let reply = ask(&server, &query)?;
            (server, reply)
        }
        // IANA's own block, or a referral back to itself: its reply is the
        // record.
        _ => (iana.to_owned(), first),
    };
    Ok(Answer {
        server,
        record: parse(&reply),
    })
}

/// `a.b.c.d`.
fn ip_text([a, b, c, d]: [u8; 4]) -> String {
    format!("{a}.{b}.{c}.{d}")
}

/// Ask `server` -- a host name or address, with `:port` if not 43 -- about
/// `query`; its reply as text.
///
/// # Errors
///
/// A server that could not be found, reached or read in time.
pub fn ask(server: &str, query: &str) -> Result<String, String> {
    let (host, port) = endpoint(server);
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {server}: {e}"))?
        .next()
        .ok_or_else(|| format!("cannot find {server}: it has no address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT)
        .map_err(|e| format!("cannot reach {server}: {e}"))?;
    let deadline = |e: std::io::Error| format!("{server} did not answer: {e}");
    stream.set_read_timeout(Some(TIMEOUT)).map_err(deadline)?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(deadline)?;
    stream
        .write_all(format!("{query}\r\n").as_bytes())
        .map_err(|e| format!("cannot ask {server}: {e}"))?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_REPLY)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{server} did not finish answering: {e}"))?;
    Ok(text_of(bytes))
}

/// A server name and its port: `:port` when it ends in one, 43 otherwise.
fn endpoint(server: &str) -> (&str, u16) {
    match server.rsplit_once(':') {
        Some((host, port)) => port.parse().map_or((server, PORT), |port| (host, port)),
        None => (server, PORT),
    }
}

/// A reply as text: UTF-8 when it is, and otherwise Latin-1 -- which the older
/// registries still send, and which maps every byte to a character, so
/// nothing is replaced or lost.
fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect())
}

/// The server IANA's reply refers the query to: its `refer:` line.
#[must_use]
pub fn referral(reply: &str) -> Option<String> {
    field(reply, &["refer"]).map(str::to_owned)
}

/// The first value of any of `keys` in `reply`, keys compared without case.
///
/// A registry's comment line (`% ...`, `# ...`) is never read as a field, and
/// needs no rule to say so: its marker stays part of what precedes the colon,
/// so `% country: XX` has the name `% country`, which is no key. (There was
/// such a rule; the mutation sweep showed it could never change an answer.)
fn field<'a>(reply: &'a str, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        reply.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            let value = value.trim();
            (name.trim().eq_ignore_ascii_case(key) && !value.is_empty()).then_some(value)
        })
    })
}

/// A registry's record, read under each registry's spelling of each field.
#[must_use]
pub fn parse(reply: &str) -> Record {
    let get = |keys: &[&str]| field(reply, keys).unwrap_or_default().to_owned();
    Record {
        org_name: get(&[
            "OrgName",
            "org-name",
            "owner",
            "organisation",
            "organization",
        ]),
        country: get(&["Country"]),
        cidr: get(&["CIDR", "inetnum", "NetRange", "route"]),
        net_name: get(&["NetName"]),
        description: get(&["descr", "Comment", "remarks"]),
        abuse_contact: get(&["OrgAbuseEmail", "abuse-mailbox"]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::net::TcpListener;

    const ARIN: &str = "\
#\n# ARIN WHOIS data and services are subject to the Terms of Use\n#\n\n\
NetRange:       8.8.8.0 - 8.8.8.255\nCIDR:           8.8.8.0/24\nNetName:        GOGL\n\
Organization:   Google LLC (GOGL)\nOrgName:        Google LLC\nCountry:        US\n\
Comment:        ARIN is not the only one\nOrgAbuseEmail:  network-abuse@google.com\n";

    const RIPE: &str = "\
% This is the RIPE Database query service.\n\n\
inetnum:        193.0.0.0 - 193.0.7.255\nnetname:        RIPE-NCC\n\
descr:          RIPE Network Coordination Centre\ncountry:        NL\n\
org-name:       Reseaux IP Europeens Network Coordination Centre (RIPE NCC)\n\
abuse-mailbox:  abuse@ripe.net\n";

    /// Each registry's spelling of each field is read.
    #[test]
    fn a_record_is_read_whichever_registry_wrote_it() {
        let arin = parse(ARIN);
        assert_eq!(arin.org_name, "Google LLC");
        assert_eq!(arin.country, "US");
        assert_eq!(arin.cidr, "8.8.8.0/24");
        assert_eq!(arin.net_name, "GOGL");
        assert_eq!(arin.description, "ARIN is not the only one");
        assert_eq!(arin.abuse_contact, "network-abuse@google.com");
        let ripe = parse(RIPE);
        assert_eq!(
            ripe.org_name,
            "Reseaux IP Europeens Network Coordination Centre (RIPE NCC)"
        );
        assert_eq!(ripe.country, "NL");
        assert_eq!(ripe.cidr, "193.0.0.0 - 193.0.7.255");
        assert_eq!(ripe.net_name, "RIPE-NCC");
        assert_eq!(ripe.abuse_contact, "abuse@ripe.net");
    }

    /// A field a record does not have stays empty, and a comment line is
    /// never read as a field.
    #[test]
    fn nothing_is_filled_in_from_outside_the_record() {
        let record = parse("% country: XX\n# OrgName: nobody\nNetName: ONLY\n");
        assert_eq!(
            record,
            Record {
                net_name: String::from("ONLY"),
                ..Record::default()
            }
        );
    }

    /// IANA's referral is its `refer:` line.
    #[test]
    fn the_referral_is_the_refer_line() {
        let iana = "% IANA WHOIS server\n\nrefer:        whois.arin.net\n\ninetnum: 8.0.0.0 - 8.255.255.255\n";
        assert_eq!(referral(iana).as_deref(), Some("whois.arin.net"));
        assert_eq!(referral("inetnum: 10.0.0.0 - 10.255.255.255\n"), None);
    }

    /// A reply that is not UTF-8 is read as Latin-1: every byte a character,
    /// none replaced.
    #[test]
    fn a_reply_in_latin1_loses_nothing() {
        let text = text_of(b"descr: Soci\xe9t\xe9\n".to_vec());
        assert_eq!(text, "descr: Soci\u{e9}t\u{e9}\n");
    }

    /// A server with a port says so after a colon; otherwise it is port 43.
    #[test]
    fn a_server_names_its_port_or_is_on_43() {
        assert_eq!(endpoint("whois.arin.net"), ("whois.arin.net", 43));
        assert_eq!(endpoint("127.0.0.1:4343"), ("127.0.0.1", 4343));
    }

    /// A server that answers `reply` to one query, and reports what it was asked.
    fn server(reply: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut byte = [0_u8; 1];
            while conn.read(&mut byte).unwrap() == 1 {
                asked.push(byte[0]);
                if asked.ends_with(b"\r\n") {
                    break;
                }
            }
            conn.write_all(reply.as_bytes()).unwrap();
            String::from_utf8(asked).unwrap()
        });
        (addr, handle)
    }

    /// The whole conversation: IANA asked, its referral followed, the
    /// registry's record read -- and each server asked about the address.
    #[test]
    fn iana_is_asked_and_its_referral_followed() {
        let (registry, asked_registry) = server(RIPE);
        let iana_reply: &'static str = Box::leak(format!("refer: {registry}\n").into_boxed_str());
        let (iana, asked_iana) = server(iana_reply);
        let answer = lookup([193, 0, 6, 139], &iana).unwrap();
        assert_eq!(answer.server, registry);
        assert_eq!(answer.record.country, "NL");
        assert_eq!(asked_iana.join().unwrap(), "193.0.6.139\r\n");
        assert_eq!(asked_registry.join().unwrap(), "193.0.6.139\r\n");
    }

    /// An address IANA keeps for itself has no referral: IANA's record is the
    /// answer, and nobody else is asked.
    #[test]
    fn iana_answers_for_its_own_blocks() {
        let (iana, _) =
            server("inetnum: 10.0.0.0 - 10.255.255.255\norganisation: IANA - Private Use\n");
        let answer = lookup([10, 1, 2, 3], &iana).unwrap();
        assert_eq!(answer.server, iana);
        assert_eq!(answer.record.org_name, "IANA - Private Use");
    }

    /// A server that cannot be reached is named in the reason.
    #[test]
    fn an_unreachable_server_is_named() {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let why = lookup([8, 8, 8, 8], &format!("127.0.0.1:{port}")).unwrap_err();
        assert!(
            why.starts_with(&format!("cannot reach 127.0.0.1:{port}")),
            "{why}"
        );
    }
}
