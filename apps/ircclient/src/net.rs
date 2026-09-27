//! The connection to an IRC server: one TCP socket, lines in and lines out.
//!
//! Connecting happens on a thread of its own -- a name lookup and a TCP
//! handshake can take seconds, and the window must not stop for them -- and
//! that thread then reads the server's lines and hands them over through a
//! channel, waking the window for each batch. Sending is done from the
//! window's thread, through the same socket.
//!
//! **Plain text.** Nothing here can check a secure server's identity yet
//! (see `open-questions.md` E-Q2), so the connection is unencrypted: what is
//! sent, a password included, can be read by anyone on the path. The window
//! says so beside the connection state.
//!
//! Two things are done here rather than in the window, because they must
//! happen even while the window is busy: a `PING` is answered with its
//! `PONG` at once (a server drops a client that is slow to answer), and an
//! over-long or broken line is bounded ([`MAX_LINE`]) rather than read into
//! memory without end.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::time::Duration;

/// The longest line read. IRC's own limit is 512 bytes, and message tags
/// (IRCv3) allow 8191 more; anything past this is not a line a server sends.
pub const MAX_LINE: usize = 16 * 1024;

/// How long connecting may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// What the connection tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    /// The socket is open; registration can begin.
    Connected,
    /// One line from the server, without its line ending.
    Line(String),
    /// The connection has ended, and why. Nothing follows it.
    Closed(String),
}

/// A connection, open or on its way.
pub struct Connection {
    /// The socket's writing end, once the connecting thread has one.
    writer: Arc<Mutex<Option<TcpStream>>>,
    incoming: Receiver<Incoming>,
}

impl Connection {
    /// Connect to `host`:`port` on a thread of its own, waking `waker` with
    /// each batch of news.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started.
    pub fn open(host: &str, port: u16, waker: Option<Waker>) -> io::Result<Self> {
        let writer: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
        let (tx, incoming) = mpsc::channel();
        let slot = Arc::clone(&writer);
        let host = host.to_owned();
        std::thread::Builder::new()
            .name(String::from("irc-connection"))
            .spawn(move || {
                let tell = |news: Incoming| {
                    // The window may have closed; then there is nobody to
                    // tell, and the socket goes when this thread does.
                    let _ = tx.send(news);
                    if let Some(waker) = &waker {
                        waker.wake_by_ref();
                    }
                };
                match connect(&host, port) {
                    Ok(stream) => {
                        let for_writing = match stream.try_clone() {
                            Ok(s) => s,
                            Err(e) => {
                                return tell(Incoming::Closed(format!(
                                    "cannot use the connection: {e}"
                                )));
                            }
                        };
                        if let Ok(mut w) = slot.lock() {
                            *w = Some(for_writing);
                        }
                        tell(Incoming::Connected);
                        let why = read_lines(stream, &slot, &tell);
                        tell(Incoming::Closed(why));
                    }
                    Err(why) => tell(Incoming::Closed(why)),
                }
            })?;
        Ok(Self { writer, incoming })
    }

    /// Send one line. It must not hold a line break or a NUL: either would
    /// end the line early and start another, so the rest would be read as a
    /// command of its own.
    ///
    /// # Errors
    ///
    /// When the line cannot be sent: it holds a break, the connection is not
    /// open yet or any more, or the write fails.
    pub fn send(&self, line: &str) -> Result<(), String> {
        if line.contains(['\r', '\n', '\0']) {
            return Err(String::from("a line cannot hold a line break"));
        }
        let mut slot = self
            .writer
            .lock()
            .map_err(|_| String::from("the connection is broken"))?;
        let stream = slot
            .as_mut()
            .ok_or_else(|| String::from("not connected yet"))?;
        stream
            .write_all(format!("{line}\r\n").as_bytes())
            .map_err(|e| format!("cannot send: {e}"))
    }

    /// Everything that has arrived, oldest first.
    #[must_use]
    pub fn drain(&self) -> Vec<Incoming> {
        self.incoming.try_iter().collect()
    }

    /// Close the connection. The reading thread sees it end and reports
    /// [`Incoming::Closed`].
    pub fn close(&self) {
        if let Ok(slot) = self.writer.lock()
            && let Some(stream) = slot.as_ref()
        {
            // Already closed from the other side is closed; nothing to do.
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

/// Open the socket, trying each address the name resolves to.
fn connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {host}: {e}"))?;
    let mut last = format!("{host} names no address");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = format!("cannot connect to {host}: {e}"),
        }
    }
    Err(last)
}

/// A line's text: UTF-8 as most servers and clients send, or read byte for
/// byte as Latin-1 when it is not -- older clients send Latin-1 still, and
/// no byte is lost or replaced either way.
fn text_of(raw: Vec<u8>) -> String {
    String::from_utf8(raw).unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect())
}

/// Read lines until the connection ends, answering `PING` on the spot.
/// Returns why it ended.
fn read_lines(
    stream: TcpStream,
    writer: &Mutex<Option<TcpStream>>,
    tell: &dyn Fn(Incoming),
) -> String {
    let mut reader = BufReader::new(stream);
    loop {
        let mut raw = Vec::new();
        let limit = u64::try_from(MAX_LINE)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        match (&mut reader).take(limit).read_until(b'\n', &mut raw) {
            Ok(0) => return String::from("the server closed the connection"),
            Ok(_) if !raw.ends_with(b"\n") && raw.len() > MAX_LINE => {
                return String::from("the server sent a line longer than IRC allows");
            }
            Ok(_) => {}
            Err(e) => return format!("the connection broke: {e}"),
        }
        while raw.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
            raw.pop();
        }
        let line = text_of(raw);
        if let Some(token) = ping_token(&line) {
            let pong = format!("PONG :{token}\r\n");
            if let Ok(mut slot) = writer.lock()
                && let Some(stream) = slot.as_mut()
                && let Err(e) = stream.write_all(pong.as_bytes())
            {
                return format!("cannot answer the server's PING: {e}");
            }
            continue;
        }
        if !line.is_empty() {
            tell(Incoming::Line(line));
        }
    }
}

/// The token of a `PING`, with or without a source prefix and a leading
/// colon: `PING :abc`, `PING abc`, `:server PING :abc`.
fn ping_token(line: &str) -> Option<&str> {
    let rest = if let Some(after) = line.strip_prefix(':') {
        after.split_once(' ')?.1
    } else {
        line
    };
    let token = rest.strip_prefix("PING ")?;
    Some(token.strip_prefix(':').unwrap_or(token))
}

#[cfg(test)]
impl Connection {
    /// A connection already open, to a loopback peer that reads nothing and
    /// answers nothing: what a test needs to have lines sent, with no server
    /// to reply. The peer is kept open for the life of the test process --
    /// a fixture, not a leak that outlives anything that matters.
    #[allow(clippy::unwrap_used)]
    pub(crate) fn to_nowhere() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        Box::leak(Box::new(peer));
        Self {
            writer: Arc::new(Mutex::new(Some(client))),
            incoming: mpsc::channel().1,
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An IRC server on a loopback port, for the tests: it records every
    //! line it is sent, and sends whatever a test gives it.

    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use std::io::{BufRead, BufReader, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub struct Server {
        pub port: u16,
        /// Lines the client sent, in order.
        pub heard: Receiver<String>,
        /// Lines to send the client.
        pub say: Sender<String>,
        /// The accepted connection, to hang up on.
        socket: Arc<Mutex<Option<TcpStream>>>,
    }

    impl Server {
        /// Close the connection from the server's side.
        ///
        /// Waits for the connection first: the client can see itself
        /// connected before this server's thread has stored the socket it
        /// accepted, and a hang-up that found no socket used to do nothing
        /// -- so under load the client waited for an end that never came.
        pub fn hang_up(&self) {
            let give_up = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(s) = self.socket.lock().unwrap().as_ref() {
                    // A connection the client already closed has nothing
                    // left to hang up; that is not this test's failure.
                    let _ = s.shutdown(Shutdown::Both);
                    return;
                }
                assert!(
                    std::time::Instant::now() < give_up,
                    "the client never connected"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        /// The next line the client sent, waiting up to five seconds.
        pub fn next(&self) -> String {
            self.heard
                .recv_timeout(Duration::from_secs(5))
                .expect("the client said nothing")
        }
    }

    pub fn server() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (heard_tx, heard) = mpsc::channel();
        let (say, say_rx) = mpsc::channel::<String>();
        let socket = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&socket);
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            *slot.lock().unwrap() = Some(stream.try_clone().unwrap());
            let mut writer: TcpStream = stream.try_clone().unwrap();
            std::thread::spawn(move || {
                for line in say_rx {
                    if writer.write_all(format!("{line}\r\n").as_bytes()).is_err() {
                        return;
                    }
                }
            });
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { return };
                if heard_tx.send(line).is_err() {
                    return;
                }
            }
        });
        Server {
            port,
            heard,
            say,
            socket,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use std::time::Instant;

    /// Wait for the next piece of news, up to five seconds.
    fn next(conn: &Connection, pending: &mut Vec<Incoming>) -> Incoming {
        let give_up = Instant::now() + Duration::from_secs(5);
        loop {
            if !pending.is_empty() {
                return pending.remove(0);
            }
            pending.extend(conn.drain());
            assert!(Instant::now() < give_up, "nothing arrived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// **A connection carries lines both ways**, answers a PING by itself
    /// without troubling the window, and says when the server hangs up.
    #[test]
    fn lines_go_both_ways_and_a_ping_is_answered() {
        let server = fake::server();
        let conn = Connection::open("127.0.0.1", server.port, None).unwrap();
        let mut pending = Vec::new();
        assert_eq!(next(&conn, &mut pending), Incoming::Connected);

        conn.send("NICK tester").unwrap();
        assert_eq!(server.next(), "NICK tester");

        server.say.send(String::from("PING :irc.example")).unwrap();
        assert_eq!(
            server.next(),
            "PONG :irc.example",
            "the PING went unanswered"
        );
        server
            .say
            .send(String::from(":irc.example 001 tester :Welcome"))
            .unwrap();
        assert_eq!(
            next(&conn, &mut pending),
            Incoming::Line(String::from(":irc.example 001 tester :Welcome")),
            "the PING reached the window, or the welcome did not"
        );

        server.hang_up();
        assert!(
            matches!(next(&conn, &mut pending), Incoming::Closed(_)),
            "the end of the connection was not said"
        );
    }

    /// A line that would break into two is never sent.
    #[test]
    fn a_line_with_a_break_is_refused() {
        let server = fake::server();
        let conn = Connection::open("127.0.0.1", server.port, None).unwrap();
        let mut pending = Vec::new();
        assert_eq!(next(&conn, &mut pending), Incoming::Connected);
        for bad in ["PRIVMSG #a :hi\r\nQUIT", "PRIVMSG #a :hi\nJOIN #b", "x\0y"] {
            assert!(conn.send(bad).is_err(), "{bad:?} was sent");
        }
        conn.send("PRIVMSG #a :fine").unwrap();
        assert_eq!(
            server.next(),
            "PRIVMSG #a :fine",
            "a refused line got through first"
        );
    }

    /// Sending before the socket is open says so, rather than dropping the
    /// line.
    #[test]
    fn sending_before_the_connection_is_open_says_so() {
        let conn = Connection {
            writer: Arc::new(Mutex::new(None)),
            incoming: mpsc::channel().1,
        };
        assert_eq!(conn.send("NICK x"), Err(String::from("not connected yet")));
    }

    /// Nothing listening is an ending, with a reason, not a hang.
    #[test]
    fn a_refused_connection_is_reported() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let conn = Connection::open("127.0.0.1", port, None).unwrap();
        let mut pending = Vec::new();
        let news = next(&conn, &mut pending);
        assert!(
            matches!(&news, Incoming::Closed(why) if why.contains("cannot connect")),
            "{news:?}"
        );
    }

    /// Text that is not UTF-8 is read as Latin-1, byte for byte.
    #[test]
    fn a_latin1_line_loses_no_byte() {
        assert_eq!(text_of(b"caf\xe9".to_vec()), "caf\u{e9}");
        assert_eq!(text_of("caf\u{e9}".as_bytes().to_vec()), "caf\u{e9}");
    }

    /// Every shape of PING yields its token.
    #[test]
    fn every_shape_of_ping_is_recognised() {
        assert_eq!(ping_token("PING :abc"), Some("abc"));
        assert_eq!(ping_token("PING abc"), Some("abc"));
        assert_eq!(ping_token(":irc.example PING :abc def"), Some("abc def"));
        assert_eq!(ping_token("PRIVMSG #a :PING me"), None);
        assert_eq!(ping_token(":nick!u@h PRIVMSG #a :PING"), None);
    }
}
