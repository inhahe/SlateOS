//! Asking a service over HTTP: one request, one reply, bounded in time and in
//! size.
//!
//! The request and the reply are `net/httpclient`'s -- it writes the request
//! and reads the reply, chunked or not -- and the connection is a plain
//! `TcpStream`, as `userspace/pkg` does it. Plain, not secure: this system has
//! no way yet to check a secure site's identity (E-Q2), so a request travels
//! in the open, which the window says where forecasts are turned on.
//!
//! Every call blocks, so the window makes them on a thread of its own
//! ([`crate::source`]).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// How long connecting, sending and each read may take before the request is
/// given up.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// The most a reply may be. A week's forecast is under ten kilobytes; a reply
/// past this is not one, and is not read into memory to find out.
pub const MAX_REPLY: usize = 1 << 20;

/// What the requests say they come from.
const USER_AGENT: &str = "SlateOS-Weather/1";

/// `GET http://{host}{path}`, and the reply's body as text.
///
/// # Errors
///
/// What went wrong, in words for the window: the host not found or not
/// reached, the reply too long, not HTTP, not a success, or not text.
#[cfg_attr(
    test,
    expect(
        dead_code,
        reason = "the window's requests go through it, and no test reaches the network"
    )
)]
pub fn get(host: &str, path: &str) -> Result<String, String> {
    get_at(host, 80, path)
}

/// [`get`], from `host`'s `port`.
///
/// # Errors
///
/// As [`get`].
pub fn get_at(host: &str, port: u16, path: &str) -> Result<String, String> {
    let url = if port == 80 {
        format!("http://{host}{path}")
    } else {
        format!("http://{host}:{port}{path}")
    };
    let request = httpclient::RequestBuilder::get(&url)
        .map_err(|e| format!("the address {url} is not one: {e}"))?
        .header("Accept", "application/json")
        // One request a connection, so the reply ends where the stream does.
        .header("Connection", "close")
        .user_agent(USER_AGENT)
        .build();
    let raw = exchange(host, port, &request.serialize())?;
    reply_text(&raw, &request.url)
}

/// The body of `raw`, a whole HTTP reply, if it is a success and text.
///
/// # Errors
///
/// A reply that is not HTTP, not a success (said with its status), or whose
/// body is not text.
pub fn reply_text(raw: &[u8], url: &httpclient::Url) -> Result<String, String> {
    let response =
        httpclient::parse_response(raw, url).map_err(|e| format!("the reply is not one: {e}"))?;
    if !response.is_success() {
        return Err(format!(
            "the service answered {} {}",
            response.status, response.status_text
        ));
    }
    response
        .text()
        .map(str::to_owned)
        .map_err(|_| String::from("the reply is not text"))
}

/// Send `request` to `host:port` and read the whole reply, up to
/// [`MAX_REPLY`].
fn exchange(host: &str, port: u16, request: &[u8]) -> Result<Vec<u8>, String> {
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("{host} could not be found: {e}"))?
        .collect();
    let mut last_error = None;
    let mut stream = None;
    for address in &addresses {
        match TcpStream::connect_timeout(address, TIMEOUT) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_error = Some(e),
        }
    }
    let Some(mut stream) = stream else {
        return Err(match last_error {
            Some(e) => format!("{host} could not be reached: {e}"),
            None => format!("{host} has no address"),
        });
    };
    stream
        .set_read_timeout(Some(TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
        .map_err(|e| format!("the connection to {host} could not be set up: {e}"))?;
    stream
        .write_all(request)
        .map_err(|e| format!("the request to {host} could not be sent: {e}"))?;
    let mut raw = Vec::new();
    let limit = u64::try_from(MAX_REPLY)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    (&mut stream)
        .take(limit)
        .read_to_end(&mut raw)
        .map_err(|e| format!("the reply from {host} could not be read: {e}"))?;
    if raw.len() > MAX_REPLY {
        return Err(format!(
            "the reply from {host} is too long to be a forecast"
        ));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use std::net::TcpListener;

    const RAW: &[u8] = include_bytes!("../tests/data/openmeteo-raw-chunked.http");

    fn url() -> httpclient::Url {
        httpclient::Url::parse("http://api.open-meteo.com/v1/forecast").unwrap()
    }

    #[test]
    fn a_chunked_reply_as_it_arrived_reads_as_its_json() {
        let body = reply_text(RAW, &url()).unwrap();
        let value = jsonvalue::json_parse(&body).unwrap();
        assert!(value.get("current").is_some(), "{body}");
    }

    #[test]
    fn a_reply_that_is_not_a_success_says_its_status() {
        let raw = b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n";
        let err = reply_text(raw, &url()).unwrap_err();
        assert!(err.contains("503 Service Unavailable"), "{err}");
        assert!(reply_text(b"<html>no</html>", &url()).is_err());
    }

    /// The whole exchange against a server on this machine: the request goes
    /// out with the path asked for and the connection closed after it, and
    /// the reply's body comes back.
    #[test]
    fn a_request_reaches_the_server_and_its_reply_comes_back() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            socket.write_all(RAW).unwrap();
            String::from_utf8(request).unwrap()
        });
        let body = get_at("127.0.0.1", port, "/v1/forecast?latitude=1").unwrap();
        let request = server.join().unwrap();
        assert!(
            request.starts_with("GET /v1/forecast?latitude=1 HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("Connection: close\r\n"), "{request}");
        assert!(body.contains("\"current\""), "{body}");
    }

    #[test]
    fn a_server_that_is_not_there_is_said_to_be_unreachable() {
        // A port bound and let go again: nothing listens on it now.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = get_at("127.0.0.1", port, "/").unwrap_err();
        assert!(err.contains("could not be reached"), "{err}");
    }

    #[test]
    fn a_reply_longer_than_any_forecast_is_refused_unread() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 1024];
            let _request = socket.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                MAX_REPLY + 10
            );
            let _sent = socket.write_all(head.as_bytes());
            let _sent = socket.write_all(&vec![b'x'; MAX_REPLY + 10]);
        });
        let err = get_at("127.0.0.1", port, "/").unwrap_err();
        server.join().unwrap();
        assert!(err.contains("too long"), "{err}");
    }
}
