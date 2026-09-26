//! Asking a tracker for peers: over plain HTTP (BEP 3) or UDP (BEP 15).
//!
//! A tracker is a stranger on the network, so everything it sends is read
//! under a bound: the whole answer is capped ([`MAX_RESPONSE`]), every read
//! has a deadline, a redirect is followed only a few times and only to
//! another `http://` address, and a UDP packet that does not carry the
//! transaction number this client chose is ignored rather than believed.
//!
//! `https://` trackers are refused with the reason: there is no TLS an
//! application here can use. Most public trackers speak UDP, which needs
//! none, and BitTorrent's own traffic is plain TCP.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use randrange::RandomSource;

use crate::{AnnounceRequest, AnnounceResponse, BencodeParser, BencodeValue, TrackerEvent};

/// What a tracker answered to an announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announced {
    /// How long to wait before asking again.
    pub interval: Duration,
    /// Peers to try.
    pub peers: Vec<SocketAddr>,
    /// How many peers have the whole torrent, when the tracker says.
    pub seeders: Option<u32>,
    /// How many are still fetching it, when the tracker says.
    pub leechers: Option<u32>,
    /// Something the tracker wants shown, alongside an answer that stands.
    pub warning: Option<String>,
}

/// The most of a tracker's answer that is read. A compact list of two
/// hundred peers is 1.2 KiB; a megabyte is past any honest answer.
pub const MAX_RESPONSE: usize = 1024 * 1024;
/// How many redirects an HTTP announce follows.
const MAX_REDIRECTS: usize = 3;
/// The shortest wait between announces this client accepts from a tracker.
const MIN_INTERVAL: Duration = Duration::from_secs(30);
/// The longest: a tracker that says "never" is asked again after this.
const MAX_INTERVAL: Duration = Duration::from_hours(2);
/// BEP 15's number that opens every UDP connect request.
const UDP_PROTOCOL_ID: u64 = 0x0417_2710_1980;
/// How many times a UDP request is sent before the tracker is given up on.
const UDP_TRIES: u32 = 2;

/// Announce `req` to the tracker at `url`, giving it `timeout` for each step.
///
/// # Errors
///
/// A message fit to show: the address is not one this client can use, the
/// tracker could not be reached or did not answer in time, or it answered
/// with a refusal or with something that is not an announce reply.
pub fn announce(url: &str, req: &AnnounceRequest, timeout: Duration) -> Result<Announced, String> {
    if let Some(rest) = url.strip_prefix("udp://") {
        udp_announce(rest, req, timeout)
    } else if url.starts_with("http://") {
        let body = http_get(&req.build_url(url), timeout)?;
        from_http_body(&body)
    } else if url.starts_with("https://") {
        Err(String::from(
            "an https:// tracker needs TLS, which no application here can use yet",
        ))
    } else {
        Err(format!("not a tracker address this client knows: {url}"))
    }
}

// ─── HTTP ────────────────────────────────────────────────────────────

/// Where an `http://` address points.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpTarget {
    host: String,
    port: u16,
    /// The path and query, starting with `/`.
    target: String,
}

/// `url`, an `http://` address, taken apart.
fn parse_http(url: &str) -> Result<HttpTarget, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("not an http:// address: {url}"))?;
    let (authority, target) = match rest.find(['/', '?', '#']) {
        Some(i) => (rest.get(..i).unwrap_or(""), rest.get(i..).unwrap_or("")),
        None => (rest, ""),
    };
    let target = target.split('#').next().unwrap_or("");
    let target = if target.starts_with('/') {
        target.to_string()
    } else {
        format!("/{target}")
    };
    if authority.contains('@') {
        return Err(String::from("a tracker address with a user name in it"));
    }
    let (host, port) = host_port(authority, Some(80))?;
    Ok(HttpTarget { host, port, target })
}

/// `authority` as a host and a port: `name:port`, `1.2.3.4:port` or
/// `[v6]:port`, with `default` when there is no port and one is allowed.
fn host_port(authority: &str, default: Option<u16>) -> Result<(String, u16), String> {
    let bad = || format!("not a host and port: {authority}");
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(bad)?;
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return Err(bad()),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(bad());
    }
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|&p| p > 0).ok_or_else(bad)?,
        None => default.ok_or_else(|| format!("no port in {authority}"))?,
    };
    Ok((host.to_string(), port))
}

/// A TCP connection to `host:port`, trying each address it resolves to.
fn connect(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, String> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("could not find {host}: {e}"))?
        .collect();
    let mut last = format!("{host} has no address");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = format!("could not reach {host}:{port}: {e}"),
        }
    }
    Err(last)
}

/// GET `url` and return the body of its `200 OK`, following a few
/// redirects to other `http://` addresses.
fn http_get(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let target = parse_http(&url)?;
        let host_header = if target.host.contains(':') {
            format!("[{}]:{}", target.host, target.port)
        } else {
            format!("{}:{}", target.host, target.port)
        };
        let mut stream = connect(&target.host, target.port, timeout)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| format!("could not talk to the tracker: {e}"))?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: SlateOS-Torrent/0.1\r\n\
             Accept-Encoding: identity\r\nConnection: close\r\n\r\n",
            target.target
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("could not talk to the tracker: {e}"))?;
        let raw = read_until_closed(&mut stream, MAX_RESPONSE.saturating_add(16 * 1024), timeout)?;
        let response = parse_response(&raw)?;
        match response.status {
            200 => return response.body(),
            301 | 302 | 303 | 307 | 308 => {
                let location = response
                    .header("location")
                    .ok_or("the tracker redirected nowhere")?;
                url = if location.starts_with("http://") {
                    location.to_string()
                } else if location.starts_with('/') {
                    format!("http://{host_header}{location}")
                } else {
                    return Err(format!(
                        "the tracker redirected to {location}, which this client does not follow"
                    ));
                };
            }
            status => return Err(format!("the tracker answered HTTP {status}")),
        }
    }
    Err(String::from("the tracker redirected too many times"))
}

/// Everything `stream` sends until it closes, stopping at `cap` bytes and at
/// `timeout` from now, however the bytes trickle in.
fn read_until_closed(
    stream: &mut TcpStream,
    cap: usize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    let mut buf = [0_u8; 16 * 1024];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(String::from("the tracker did not answer in time"));
        }
        stream
            .set_read_timeout(Some(left))
            .map_err(|e| format!("could not talk to the tracker: {e}"))?;
        match stream.read(&mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) => {
                out.extend_from_slice(buf.get(..n).unwrap_or_default());
                if out.len() > cap {
                    return Err(format!("the tracker's answer is over {cap} bytes"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(String::from("the tracker did not answer in time"));
            }
            Err(e) => return Err(format!("the tracker's answer broke off: {e}")),
        }
    }
}

/// An HTTP response: its status, its headers and its raw body.
struct Response<'a> {
    status: u16,
    headers: Vec<(String, String)>,
    raw_body: &'a [u8],
}

impl Response<'_> {
    /// The value of header `name`, however it was capitalised.
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body, de-chunked if it was sent in chunks, and cut to its
    /// `Content-Length` if it gave one.
    fn body(&self) -> Result<Vec<u8>, String> {
        let chunked = self
            .header("transfer-encoding")
            .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
        let body = if chunked {
            dechunk(self.raw_body)?
        } else {
            self.raw_body.to_vec()
        };
        match self.header("content-length").map(str::trim) {
            Some(n) if !chunked => {
                let n: usize = n
                    .parse()
                    .map_err(|_| "a Content-Length that is not a number")?;
                body.get(..n)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(|| String::from("the tracker's answer ended early"))
            }
            _ => Ok(body),
        }
    }
}

/// `raw` taken apart at its blank line.
fn parse_response(raw: &[u8]) -> Result<Response<'_>, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("the tracker's answer is not HTTP")?;
    let head = std::str::from_utf8(raw.get(..split).unwrap_or_default())
        .map_err(|_| "the tracker's headers are not text")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut words = status_line.split_whitespace();
    if !words.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(String::from("the tracker's answer is not HTTP"));
    }
    let status = words
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("the tracker's answer has no status")?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Ok(Response {
        status,
        headers,
        raw_body: raw.get(split.saturating_add(4)..).unwrap_or_default(),
    })
}

/// A chunked body put back together.
fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("a chunked answer that breaks off")?;
        let size_text = std::str::from_utf8(data.get(..line_end).unwrap_or_default())
            .map_err(|_| "a chunk size that is not text")?;
        let size_text = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| "a chunk size that is not a number")?;
        let start = line_end.saturating_add(2);
        if size == 0 {
            return Ok(out);
        }
        let end = start.checked_add(size).ok_or("a chunk too large")?;
        out.extend_from_slice(data.get(start..end).ok_or("a chunk that breaks off")?);
        if out.len() > MAX_RESPONSE {
            return Err(format!("the tracker's answer is over {MAX_RESPONSE} bytes"));
        }
        data = data
            .get(end.saturating_add(2)..)
            .ok_or("a chunk that breaks off")?;
    }
}

/// An HTTP tracker's bencoded answer.
fn from_http_body(body: &[u8]) -> Result<Announced, String> {
    let response = AnnounceResponse::from_bencode(body)?;
    if let Some(reason) = response.failure_reason {
        return Err(format!("the tracker refused: {reason}"));
    }
    let mut peers: Vec<SocketAddr> = response
        .peers
        .iter()
        .filter_map(|(ip, port)| {
            ip.parse::<IpAddr>()
                .ok()
                .map(|ip| SocketAddr::new(ip, *port))
        })
        .collect();
    // BEP 7: IPv6 peers, eighteen bytes each, beside the IPv4 ones.
    if let Ok((BencodeValue::Dict(dict), _)) = BencodeParser::parse(body)
        && let Some(v6) = dict.get("peers6").and_then(BencodeValue::as_bytes)
    {
        peers.extend(v6.chunks_exact(18).filter_map(peer_v6));
    }
    Ok(Announced {
        interval: Duration::from_secs(response.interval).clamp(MIN_INTERVAL, MAX_INTERVAL),
        peers,
        seeders: Some(response.complete),
        leechers: Some(response.incomplete),
        warning: response.warning_message,
    })
}

// ─── UDP (BEP 15) ────────────────────────────────────────────────────

/// Announce over UDP to `rest`, the address after `udp://`.
fn udp_announce(rest: &str, req: &AnnounceRequest, timeout: Duration) -> Result<Announced, String> {
    let authority = rest.split(['/', '?']).next().unwrap_or(rest);
    let (host, port) = host_port(authority, None)?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("could not find {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host} has no address"))?;
    let local: SocketAddr = if addr.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(local).map_err(|e| format!("could not open a UDP socket: {e}"))?;
    socket
        .connect(addr)
        .map_err(|e| format!("could not reach {host}:{port}: {e}"))?;
    let mut rng = randrange::seeded_from_system(u64::from(port) ^ 0x5eed);

    // Connect: the tracker hands out a connection number to announce with.
    let tid = rng.next_u32();
    let mut connect = Vec::with_capacity(16);
    connect.extend_from_slice(&UDP_PROTOCOL_ID.to_be_bytes());
    connect.extend_from_slice(&0_u32.to_be_bytes());
    connect.extend_from_slice(&tid.to_be_bytes());
    let reply = udp_exchange(&socket, &connect, tid, 0, 16, timeout)?;
    let connection_id = be_u64(&reply, 8).ok_or("a short connect reply")?;

    // Announce.
    let tid = rng.next_u32();
    let event: u32 = match req.event {
        TrackerEvent::None => 0,
        TrackerEvent::Completed => 1,
        TrackerEvent::Started => 2,
        TrackerEvent::Stopped => 3,
    };
    let num_want = req
        .numwant
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(-1);
    let mut packet = Vec::with_capacity(98);
    packet.extend_from_slice(&connection_id.to_be_bytes());
    packet.extend_from_slice(&1_u32.to_be_bytes());
    packet.extend_from_slice(&tid.to_be_bytes());
    packet.extend_from_slice(&req.info_hash);
    packet.extend_from_slice(&req.peer_id);
    packet.extend_from_slice(&req.downloaded.to_be_bytes());
    packet.extend_from_slice(&req.left.to_be_bytes());
    packet.extend_from_slice(&req.uploaded.to_be_bytes());
    packet.extend_from_slice(&event.to_be_bytes());
    packet.extend_from_slice(&0_u32.to_be_bytes()); // IP: the one it came from
    packet.extend_from_slice(&rng.next_u32().to_be_bytes()); // key
    packet.extend_from_slice(&num_want.to_be_bytes());
    packet.extend_from_slice(&req.port.to_be_bytes());
    let reply = udp_exchange(&socket, &packet, tid, 1, 20, timeout)?;
    let (interval, leechers, seeders) = (
        be_u32(&reply, 8).unwrap_or(0),
        be_u32(&reply, 12),
        be_u32(&reply, 16),
    );
    let list = reply.get(20..).unwrap_or_default();
    let peers = if addr.is_ipv4() {
        list.chunks_exact(6).filter_map(peer_v4).collect()
    } else {
        list.chunks_exact(18).filter_map(peer_v6).collect()
    };
    Ok(Announced {
        interval: Duration::from_secs(u64::from(interval)).clamp(MIN_INTERVAL, MAX_INTERVAL),
        peers,
        seeders,
        leechers,
        warning: None,
    })
}

/// Send `packet` and wait for the reply carrying `tid` with `action`, at
/// least `min_len` bytes long; send again if none comes in `timeout`.
/// A reply with another transaction number is not ours, and is ignored; an
/// error reply (action 3) is the tracker's refusal.
fn udp_exchange(
    socket: &UdpSocket,
    packet: &[u8],
    tid: u32,
    action: u32,
    min_len: usize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut buf = vec![0_u8; 64 * 1024];
    for _ in 0..UDP_TRIES {
        socket
            .send(packet)
            .map_err(|e| format!("could not send to the tracker: {e}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            socket
                .set_read_timeout(Some(left))
                .map_err(|e| format!("could not talk to the tracker: {e}"))?;
            let n = match socket.recv(&mut buf) {
                Ok(n) => n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("the tracker could not be reached: {e}")),
            };
            let reply = buf.get(..n).unwrap_or_default();
            if be_u32(reply, 4) != Some(tid) {
                continue;
            }
            match be_u32(reply, 0) {
                Some(3) => {
                    let why = String::from_utf8(reply.get(8..).unwrap_or_default().to_vec())
                        .unwrap_or_else(|_| String::from("(a reason that is not text)"));
                    return Err(format!("the tracker refused: {why}"));
                }
                Some(a) if a == action && reply.len() >= min_len => return Ok(reply.to_vec()),
                _ => {
                    return Err(String::from(
                        "the tracker's reply is not an answer to what was asked",
                    ));
                }
            }
        }
    }
    Err(String::from("the tracker did not answer in time"))
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at.checked_add(4)?)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_be_bytes)
}

fn be_u64(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at.checked_add(8)?)
        .and_then(|b| b.try_into().ok())
        .map(u64::from_be_bytes)
}

/// A compact IPv4 peer: four bytes of address, two of port.
fn peer_v4(chunk: &[u8]) -> Option<SocketAddr> {
    let ip: [u8; 4] = chunk.get(..4)?.try_into().ok()?;
    let port = u16::from_be_bytes(chunk.get(4..6)?.try_into().ok()?);
    (port != 0).then(|| SocketAddr::from((ip, port)))
}

/// A compact IPv6 peer: sixteen bytes of address, two of port.
fn peer_v6(chunk: &[u8]) -> Option<SocketAddr> {
    let ip: [u8; 16] = chunk.get(..16)?.try_into().ok()?;
    let port = u16::from_be_bytes(chunk.get(16..18)?.try_into().ok()?);
    (port != 0).then(|| SocketAddr::from((ip, port)))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Long enough that a loaded machine does not turn a test about what
    /// a tracker said into one about how fast it said it.
    const SHORT: Duration = Duration::from_secs(5);

    fn request() -> AnnounceRequest {
        AnnounceRequest {
            info_hash: [0xAB; 20],
            peer_id: *b"-SL0001-abcdefghijkl",
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: 1000,
            compact: true,
            event: TrackerEvent::Started,
            numwant: Some(50),
        }
    }

    /// A tracker on a loopback port that answers one connection with
    /// `reply(request line)`, and says what it was asked.
    fn http_tracker(
        reply: impl Fn(&str) -> Vec<u8> + Send + 'static,
    ) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0_u8; 4096];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            let text = String::from_utf8(got).unwrap();
            let line = text.lines().next().unwrap_or("").to_string();
            stream.write_all(&reply(&line)).unwrap();
            text
        });
        (format!("http://127.0.0.1:{port}/announce"), handle)
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        [
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes(),
            body,
        ]
        .concat()
    }

    /// A compact peer list: 127.0.0.1:6881 and 10.0.0.2:51413.
    const TWO_PEERS: &[u8] =
        b"d8:intervali900e5:peers12:\x7f\x00\x00\x01\x1a\xe1\x0a\x00\x00\x02\xc8\xd5e";

    /// An HTTP tracker is asked with the torrent's hash and this client's
    /// id, and its compact peer list becomes addresses.
    #[test]
    fn an_http_tracker_is_asked_and_answers_with_peers() {
        let (url, server) = http_tracker(|_| ok(TWO_PEERS));
        let got = announce(&url, &request(), SHORT).unwrap();
        assert_eq!(
            got.peers,
            vec![
                "127.0.0.1:6881".parse::<SocketAddr>().unwrap(),
                "10.0.0.2:51413".parse().unwrap()
            ]
        );
        assert_eq!(got.interval, Duration::from_mins(15));
        let asked = server.join().unwrap();
        assert!(
            asked.starts_with("GET /announce?info_hash=%AB%AB"),
            "{asked}"
        );
        assert!(asked.contains("peer_id=-SL0001-abcdefghijkl"), "{asked}");
        assert!(asked.contains("event=started"), "{asked}");
        assert!(asked.contains("\r\nHost: 127.0.0.1:"), "{asked}");
    }

    /// A chunked answer is put back together; IPv6 peers are read too; an
    /// interval under the floor is raised to it.
    #[test]
    fn a_chunked_answer_with_ipv6_peers() {
        let body = [
            b"d8:intervali5e5:peers0:6:peers618:".as_slice(),
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0x1a, 0xe1],
            b"e",
        ]
        .concat();
        let (first, second) = body.split_at(7);
        let reply = [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
            format!("{:x}\r\n", first.len()).as_bytes(),
            first,
            b"\r\n",
            format!("{:x};ext=1\r\n", second.len()).as_bytes(),
            second,
            b"\r\n0\r\n\r\n",
        ]
        .concat();
        let (url, server) = http_tracker(move |_| reply.clone());
        let got = announce(&url, &request(), SHORT).unwrap();
        assert_eq!(got.peers, vec!["[::1]:6881".parse::<SocketAddr>().unwrap()]);
        assert_eq!(got.interval, MIN_INTERVAL);
        server.join().unwrap();
    }

    /// A refusal is an error carrying the tracker's reason; an HTTP error
    /// status is an error; an answer that is not bencode is an error.
    #[test]
    fn a_refusal_is_said_as_the_tracker_said_it() {
        let (url, server) = http_tracker(|_| ok(b"d14:failure reason17:torrent not founde"));
        let err = announce(&url, &request(), SHORT).unwrap_err();
        assert!(err.contains("torrent not found"), "{err}");
        server.join().unwrap();

        let (url, server) =
            http_tracker(|_| b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec());
        assert!(
            announce(&url, &request(), SHORT)
                .unwrap_err()
                .contains("404")
        );
        server.join().unwrap();

        let (url, server) = http_tracker(|_| ok(b"<html>not a tracker</html>"));
        assert!(announce(&url, &request(), SHORT).is_err());
        server.join().unwrap();
    }

    /// A redirect to another http:// address is followed.
    #[test]
    fn a_redirect_is_followed() {
        let (real, real_server) = http_tracker(|_| ok(TWO_PEERS));
        let moved = real.clone();
        let (url, server) = http_tracker(move |_| {
            format!("HTTP/1.1 302 Found\r\nLocation: {moved}\r\nContent-Length: 0\r\n\r\n")
                .into_bytes()
        });
        assert_eq!(announce(&url, &request(), SHORT).unwrap().peers.len(), 2);
        server.join().unwrap();
        real_server.join().unwrap();
    }

    /// An answer past the cap, or one that never comes, is an error -- not
    /// a download of whatever the tracker cares to send, nor a wait forever.
    #[test]
    fn an_answer_too_big_or_too_slow_is_given_up() {
        let (url, server) = http_tracker(|_| {
            let mut body = b"d5:peers".to_vec();
            body.extend(vec![b'x'; MAX_RESPONSE + 100_000]);
            ok(&body)
        });
        assert!(
            announce(&url, &request(), Duration::from_secs(10))
                .unwrap_err()
                .contains("over")
        );
        drop(server.join());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let silent = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(1500));
            drop(stream);
        });
        let started = Instant::now();
        let err = announce(
            &format!("http://127.0.0.1:{port}/a"),
            &request(),
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(err.contains("in time"), "{err}");
        assert!(
            started.elapsed() < Duration::from_millis(1400),
            "waited past the timeout"
        );
        silent.join().unwrap();
    }

    /// https:// and other schemes are refused with a reason.
    #[test]
    fn an_address_this_client_cannot_use_says_why() {
        assert!(
            announce("https://t.example/a", &request(), SHORT)
                .unwrap_err()
                .contains("TLS")
        );
        assert!(
            announce("wss://t.example/a", &request(), SHORT)
                .unwrap_err()
                .contains("not a tracker address")
        );
        assert!(announce("http://user@t.example/a", &request(), SHORT).is_err());
    }

    /// Addresses are taken apart as HTTP writes them.
    #[test]
    fn an_http_address_is_taken_apart() {
        let t = parse_http("http://t.example:8080/a/b?x=1#frag").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.target.as_str()),
            ("t.example", 8080, "/a/b?x=1")
        );
        let t = parse_http("http://t.example?x=1").unwrap();
        assert_eq!((t.port, t.target.as_str()), (80, "/?x=1"));
        let t = parse_http("http://[::1]:99/").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("::1", 99));
        assert!(parse_http("http://t.example:0/").is_err());
        assert!(parse_http("http://t.example:99999/").is_err());
        assert!(parse_http("http://:80/").is_err());
    }

    /// A UDP tracker on a loopback port, answering with `answer`.
    fn udp_tracker(
        answer: impl Fn(&[u8]) -> Vec<Vec<u8>> + Send + 'static,
    ) -> (String, thread::JoinHandle<Vec<Vec<u8>>>) {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            let mut buf = [0_u8; 2048];
            // Every packet until the client falls quiet: a slow machine may
            // send one again, and a server that stopped after two would
            // leave the announce unanswered.
            for _ in 0..8 {
                let Ok((n, from)) = socket.recv_from(&mut buf) else {
                    break;
                };
                seen.push(buf[..n].to_vec());
                for reply in answer(&buf[..n]) {
                    socket.send_to(&reply, from).unwrap();
                }
            }
            seen
        });
        (format!("udp://127.0.0.1:{port}/announce"), handle)
    }

    /// The honest UDP tracker: a connection number, then two peers.
    fn honest(packet: &[u8]) -> Vec<Vec<u8>> {
        let action = be_u32(packet, 8).unwrap();
        if action == 0 {
            let tid = &packet[12..16];
            vec![[&0_u32.to_be_bytes()[..], tid, &77_u64.to_be_bytes()].concat()]
        } else {
            let tid = &packet[12..16];
            vec![
                [
                    &1_u32.to_be_bytes()[..],
                    tid,
                    &1800_u32.to_be_bytes(),
                    &4_u32.to_be_bytes(),
                    &9_u32.to_be_bytes(),
                    &[127, 0, 0, 1, 0x1a, 0xe1, 10, 0, 0, 2, 0xc8, 0xd5],
                ]
                .concat(),
            ]
        }
    }

    /// A UDP tracker: connect, then announce with the number it gave; the
    /// packets are laid out as BEP 15 says.
    #[test]
    fn a_udp_tracker_is_asked_as_bep_15_says() {
        let (url, server) = udp_tracker(honest);
        let got = announce(&url, &request(), SHORT).unwrap();
        assert_eq!(got.peers.len(), 2);
        assert_eq!(
            got.peers[0],
            "127.0.0.1:6881".parse::<SocketAddr>().unwrap()
        );
        assert_eq!((got.seeders, got.leechers), (Some(9), Some(4)));
        assert_eq!(got.interval, Duration::from_mins(30));
        let seen = server.join().unwrap();
        // Found by what they are, not by position: a loaded machine may have
        // sent one of them twice.
        let connect = seen
            .iter()
            .find(|p| p.len() == 16)
            .expect("a connect request");
        assert_eq!(be_u64(connect, 0), Some(UDP_PROTOCOL_ID));
        assert_eq!(be_u32(connect, 8), Some(0));
        let ann = seen.iter().find(|p| p.len() != 16).expect("an announce");
        assert_eq!(ann.len(), 98);
        assert_eq!(
            be_u64(ann, 0),
            Some(77),
            "the connection number was not used"
        );
        assert_eq!(be_u32(ann, 8), Some(1));
        assert_eq!(&ann[16..36], &[0xAB; 20]);
        assert_eq!(&ann[36..56], b"-SL0001-abcdefghijkl");
        assert_eq!(be_u64(ann, 64), Some(1000), "left");
        assert_eq!(be_u32(ann, 80), Some(2), "started");
        assert_eq!(be_u32(ann, 92), Some(50), "numwant");
        assert_eq!(&ann[96..98], &6881_u16.to_be_bytes());
    }

    /// A reply with someone else's transaction number is ignored, not
    /// believed; the tracker's error is said.
    #[test]
    fn a_udp_reply_that_is_not_ours_is_ignored() {
        let (url, server) = udp_tracker(|packet| {
            let mut forged = honest(packet);
            let mut wrong = forged[0].clone();
            wrong[4] ^= 0xFF;
            // The forgery first, then the real one.
            forged.insert(0, wrong);
            forged
        });
        assert_eq!(announce(&url, &request(), SHORT).unwrap().peers.len(), 2);
        server.join().unwrap();

        let (url, server) = udp_tracker(|packet| {
            let tid = packet[12..16].to_vec();
            vec![[&3_u32.to_be_bytes()[..], &tid, b"not registered"].concat()]
        });
        let err = announce(&url, &request(), SHORT).unwrap_err();
        assert!(err.contains("not registered"), "{err}");
        drop(server.join());
    }

    /// A UDP tracker that never answers is given up after its tries.
    #[test]
    fn a_silent_udp_tracker_is_given_up() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let started = Instant::now();
        let err = announce(
            &format!("udp://127.0.0.1:{port}"),
            &request(),
            Duration::from_millis(200),
        )
        .unwrap_err();
        assert!(err.contains("in time") || err.contains("reached"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(3));
        drop(socket);
    }
}
