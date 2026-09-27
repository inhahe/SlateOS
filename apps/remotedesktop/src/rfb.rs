//! VNC's protocol -- RFB 3.8, RFC 6143 -- the client half.
//!
//! A [`Session`] connects on a thread of its own, agrees the version,
//! authenticates (no password, or VNC's password challenge, `des`), learns
//! the desktop's size and name, asks for the whole screen, and from then on
//! reads the server's messages and hands them to the window as [`Update`]s,
//! asking for the next frame as each one arrives. Keys and the pointer go
//! the other way through [`Session::key`] and [`Session::pointer`].
//!
//! **Plain text.** RFB's own framing carries no encryption; the password is
//! proven by a challenge rather than sent, but everything on screen and every
//! key typed crosses the network readable. The window says so.
//!
//! **Encodings.** Raw and CopyRect, which every server speaks, and the
//! DesktopSize pseudo-encoding so a resized remote desktop is followed. The
//! compressed encodings (Tight, ZRLE, Hextile) are faster on a slow link and
//! are not asked for; a server must fall back to Raw for a client that does
//! not name them (RFC 6143 section 7.7).
//!
//! **Pixels.** The client asks for 32 bits a pixel, true colour, little
//! endian, red at bit 16, green at 8, blue at 0 -- so a pixel read as a
//! little-endian `u32` is `0x00RRGGBB`, one opaque byte from the window's
//! `0xAARRGGBB`.
//!
//! Every length the server states is bounded before anything is allocated
//! for it ([`MAX_PIXELS`], [`MAX_TEXT`]), so a hostile server cannot make the
//! window allocate what it likes.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::task::Waker;
use std::time::Duration;

use crate::des;

/// The largest desktop, in pixels, this client will hold: 16k x 16k.
pub const MAX_PIXELS: u64 = 16_384 * 16_384;

/// The longest name or clipboard text read.
pub const MAX_TEXT: u32 = 1 << 20;

/// How long connecting may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// RFB's encoding numbers.
const RAW: i32 = 0;
const COPY_RECT: i32 = 1;
const DESKTOP_SIZE: i32 = -223;

/// What the session tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// The handshake is done: the desktop's size, and its name.
    Ready {
        width: u16,
        height: u16,
        name: String,
    },
    /// A rectangle's new pixels, `0x00RRGGBB` each, row by row.
    Pixels {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        pixels: Vec<u32>,
    },
    /// Copy the rectangle at `(src_x, src_y)` to `(x, y)`.
    Copy {
        src_x: u16,
        src_y: u16,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
    },
    /// The desktop has a new size; the screen should be asked for again.
    Resized { width: u16, height: u16 },
    /// The remote machine rang its bell.
    Bell,
    /// Text the remote machine cut to its clipboard.
    CutText(String),
    /// The session has ended, and why. Nothing follows it.
    Closed(String),
}

/// A session, open or on its way.
pub struct Session {
    writer: Arc<Mutex<Option<TcpStream>>>,
    updates: Receiver<Update>,
}

impl Session {
    /// Connect to `host`:`port` and authenticate with `password` (unused if
    /// the server asks for none), on a thread of its own, waking `waker` with
    /// each batch of news.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started.
    pub fn open(host: &str, port: u16, password: &str, waker: Option<Waker>) -> io::Result<Self> {
        let writer: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
        let (tx, updates) = mpsc::channel();
        let slot = Arc::clone(&writer);
        let (host, password) = (host.to_owned(), password.as_bytes().to_vec());
        std::thread::Builder::new()
            .name(String::from("vnc-session"))
            .spawn(move || {
                let tell = |update: Update| {
                    // The window may have closed; the socket goes with it.
                    let _ = tx.send(update);
                    if let Some(waker) = &waker {
                        waker.wake_by_ref();
                    }
                };
                let why = run(&host, port, &password, &slot, &tell);
                tell(Update::Closed(why));
            })?;
        Ok(Self { writer, updates })
    }

    /// Everything that has arrived, oldest first.
    #[must_use]
    pub fn drain(&self) -> Vec<Update> {
        self.updates.try_iter().collect()
    }

    /// A key pressed (`down`) or let go, as an X11 keysym (`keysym_of`).
    ///
    /// # Errors
    ///
    /// When the session is not open, or the write fails.
    pub fn key(&self, down: bool, keysym: u32) -> Result<(), String> {
        let mut msg = vec![4, u8::from(down), 0, 0];
        msg.extend_from_slice(&keysym.to_be_bytes());
        self.send(&msg)
    }

    /// The pointer at `(x, y)` with these buttons held (bit 0 left, 1 middle,
    /// 2 right, 3 and 4 the wheel's up and down).
    ///
    /// # Errors
    ///
    /// When the session is not open, or the write fails.
    pub fn pointer(&self, buttons: u8, x: u16, y: u16) -> Result<(), String> {
        let mut msg = vec![5, buttons];
        msg.extend_from_slice(&x.to_be_bytes());
        msg.extend_from_slice(&y.to_be_bytes());
        self.send(&msg)
    }

    fn send(&self, bytes: &[u8]) -> Result<(), String> {
        let mut slot = self
            .writer
            .lock()
            .map_err(|_| String::from("the session is broken"))?;
        let stream = slot
            .as_mut()
            .ok_or_else(|| String::from("not connected yet"))?;
        stream
            .write_all(bytes)
            .map_err(|e| format!("cannot send: {e}"))
    }

    /// End the session. The reading thread sees it end and reports
    /// [`Update::Closed`].
    pub fn close(&self) {
        if let Ok(slot) = self.writer.lock()
            && let Some(stream) = slot.as_ref()
        {
            // Closed from the other side already is closed.
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// The whole session: handshake, then messages until the end. Returns why
/// it ended.
fn run(
    host: &str,
    port: u16,
    password: &[u8],
    writer: &Mutex<Option<TcpStream>>,
    tell: &dyn Fn(Update),
) -> String {
    let stream = match connect(host, port) {
        Ok(s) => s,
        Err(why) => return why,
    };
    let for_writing = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return format!("cannot use the connection: {e}"),
    };
    if let Ok(mut w) = writer.lock() {
        *w = Some(for_writing);
    }
    let mut conn = Conn { stream, writer };
    match conn.handshake(password) {
        Ok((width, height, name)) => {
            tell(Update::Ready {
                width,
                height,
                name,
            });
            if let Err(why) = conn.request(false, width, height) {
                return why;
            }
            conn.messages(width, height, tell)
        }
        Err(why) => why,
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

/// Text a server sent, as text: UTF-8, or Latin-1 byte for byte -- RFB's
/// strings are Latin-1 by the standard, UTF-8 by many servers.
fn text_of(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect())
}

/// The connection, reading from one end and writing through the shared other.
struct Conn<'a> {
    stream: TcpStream,
    writer: &'a Mutex<Option<TcpStream>>,
}

impl Conn<'_> {
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), String> {
        self.stream.read_exact(buf).map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => String::from("the server closed the connection"),
            _ => format!("the connection broke: {e}"),
        })
    }

    fn u8(&mut self) -> Result<u8, String> {
        let mut b = [0_u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let mut b = [0_u8; 2];
        self.read_exact(&mut b)?;
        Ok(u16::from_be_bytes(b))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let mut b = [0_u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_be_bytes(b))
    }

    /// A length-prefixed string, bounded by [`MAX_TEXT`].
    fn text(&mut self) -> Result<String, String> {
        let len = self.u32()?;
        if len > MAX_TEXT {
            return Err(format!(
                "the server sent {len} bytes of text, more than any name or clipboard"
            ));
        }
        let mut buf = vec![0_u8; usize::try_from(len).unwrap_or(0)];
        self.read_exact(&mut buf)?;
        Ok(text_of(buf))
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut slot = self
            .writer
            .lock()
            .map_err(|_| String::from("the session is broken"))?;
        let stream = slot
            .as_mut()
            .ok_or_else(|| String::from("the session is closed"))?;
        stream
            .write_all(bytes)
            .map_err(|e| format!("cannot send: {e}"))
    }

    /// Version, security, initialisation. Returns the desktop's size and name.
    fn handshake(&mut self, password: &[u8]) -> Result<(u16, u16, String), String> {
        let mut version = [0_u8; 12];
        self.read_exact(&mut version)?;
        if !version.starts_with(b"RFB ") {
            return Err(String::from("the server does not speak VNC's protocol"));
        }
        // 3.8, whatever the server offers: a server older than 3.3 is
        // refused below by its security answer's shape.
        self.write(b"RFB 003.008\n")?;

        let count = self.u8()?;
        if count == 0 {
            let why = self.text()?;
            return Err(format!("the server refused the connection: {why}"));
        }
        let mut types = vec![0_u8; usize::from(count)];
        self.read_exact(&mut types)?;
        let chosen = if types.contains(&1) {
            1
        } else if types.contains(&2) {
            2
        } else {
            return Err(format!(
                "the server offers no sign-in this client knows (it offers {types:?}; this knows none and a VNC password)"
            ));
        };
        self.write(&[chosen])?;
        if chosen == 2 {
            let mut challenge = [0_u8; 16];
            self.read_exact(&mut challenge)?;
            self.write(&des::vnc_response(password, &challenge))?;
        }
        match self.u32()? {
            0 => {}
            _ => {
                // 3.8 says why; a server that does not has already hung up.
                let why = self
                    .text()
                    .unwrap_or_else(|_| String::from("no reason given"));
                return Err(format!("the server did not accept the sign-in: {why}"));
            }
        }

        // ClientInit: share the desktop with other viewers.
        self.write(&[1])?;
        let width = self.u16()?;
        let height = self.u16()?;
        let mut server_format = [0_u8; 16];
        self.read_exact(&mut server_format)?;
        let name = self.text()?;
        if u64::from(width).saturating_mul(u64::from(height)) > MAX_PIXELS {
            return Err(format!(
                "the desktop is {width}x{height}, larger than this client will hold"
            ));
        }

        // SetPixelFormat: 32 bpp, depth 24, little endian, true colour,
        // 255/255/255 maxima, red at 16, green at 8, blue at 0.
        self.write(&[
            0, 0, 0, 0, 32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0,
        ])?;
        // SetEncodings: Raw, CopyRect, DesktopSize.
        let mut encodings = vec![2, 0, 0, 3];
        for e in [COPY_RECT, RAW, DESKTOP_SIZE] {
            encodings.extend_from_slice(&e.to_be_bytes());
        }
        self.write(&encodings)?;
        Ok((width, height, name))
    }

    /// Ask for the screen: all of it, or what changed since the last frame.
    fn request(&mut self, incremental: bool, width: u16, height: u16) -> Result<(), String> {
        let mut msg = vec![3, u8::from(incremental), 0, 0, 0, 0];
        msg.extend_from_slice(&width.to_be_bytes());
        msg.extend_from_slice(&height.to_be_bytes());
        self.write(&msg)
    }

    /// Server messages until the connection ends. Returns why it ended.
    fn messages(&mut self, mut width: u16, mut height: u16, tell: &dyn Fn(Update)) -> String {
        loop {
            let kind = match self.u8() {
                Ok(k) => k,
                Err(why) => return why,
            };
            let outcome = match kind {
                0 => self.frame(&mut width, &mut height, tell),
                1 => self.colour_map(),
                2 => {
                    tell(Update::Bell);
                    Ok(())
                }
                3 => self.cut_text(tell),
                other => Err(format!(
                    "the server sent a message this client does not know ({other})"
                )),
            };
            if let Err(why) = outcome {
                return why;
            }
        }
    }

    /// A FramebufferUpdate, then a request for the next.
    fn frame(
        &mut self,
        width: &mut u16,
        height: &mut u16,
        tell: &dyn Fn(Update),
    ) -> Result<(), String> {
        let _padding = self.u8()?;
        let count = self.u16()?;
        let mut resized = false;
        for _ in 0..count {
            let x = self.u16()?;
            let y = self.u16()?;
            let w = self.u16()?;
            let h = self.u16()?;
            let encoding = i32::from_be_bytes(self.u32()?.to_be_bytes());
            match encoding {
                RAW => {
                    let area = u64::from(w).saturating_mul(u64::from(h));
                    if area > MAX_PIXELS
                        || u32::from(x).saturating_add(u32::from(w)) > u32::from(*width)
                        || u32::from(y).saturating_add(u32::from(h)) > u32::from(*height)
                    {
                        return Err(format!(
                            "the server sent a {w}x{h} rectangle at {x},{y}, outside its own {width}x{height} desktop"
                        ));
                    }
                    let mut raw = vec![0_u8; usize::try_from(area.saturating_mul(4)).unwrap_or(0)];
                    self.read_exact(&mut raw)?;
                    let pixels = raw
                        .chunks_exact(4)
                        .map(|p| match p {
                            [b, g, r, _] => u32::from_le_bytes([*b, *g, *r, 0]),
                            _ => 0,
                        })
                        .collect();
                    tell(Update::Pixels { x, y, w, h, pixels });
                }
                COPY_RECT => {
                    let src_x = self.u16()?;
                    let src_y = self.u16()?;
                    tell(Update::Copy {
                        src_x,
                        src_y,
                        x,
                        y,
                        w,
                        h,
                    });
                }
                DESKTOP_SIZE => {
                    if u64::from(w).saturating_mul(u64::from(h)) > MAX_PIXELS {
                        return Err(format!(
                            "the desktop became {w}x{h}, larger than this client will hold"
                        ));
                    }
                    *width = w;
                    *height = h;
                    resized = true;
                    tell(Update::Resized {
                        width: w,
                        height: h,
                    });
                }
                other => {
                    // A server may only send what was asked for; one that
                    // does not leaves bytes this cannot skip.
                    return Err(format!(
                        "the server used an encoding this client did not ask for ({other})"
                    ));
                }
            }
        }
        // After a resize the whole new screen is wanted, not just changes.
        self.request(!resized, *width, *height)
    }

    /// SetColourMapEntries: never asked for (the client asked for true
    /// colour), read past so the stream stays in step.
    fn colour_map(&mut self) -> Result<(), String> {
        let _padding = self.u8()?;
        let _first = self.u16()?;
        let count = self.u16()?;
        let mut skip = vec![0_u8; usize::from(count) * 6];
        self.read_exact(&mut skip)
    }

    /// ServerCutText.
    fn cut_text(&mut self, tell: &dyn Fn(Update)) -> Result<(), String> {
        let mut padding = [0_u8; 3];
        self.read_exact(&mut padding)?;
        let text = self.text()?;
        tell(Update::CutText(text));
        Ok(())
    }
}

/// The X11 keysym for a key, as RFB sends keys.
///
/// Printable text goes as its character: Latin-1 as itself, anything above
/// as `0x0100_0000` plus its code point, the convention every server reads.
#[must_use]
pub fn keysym_of_char(c: char) -> u32 {
    let code = u32::from(c);
    if (0x20..=0x7E).contains(&code) || (0xA0..=0xFF).contains(&code) {
        code
    } else {
        0x0100_0000 | code
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A VNC server on a loopback port, for the tests: it plays a script of
    //! what to say and how much to hear, and hands over what it heard.

    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    /// A VNC server on a loopback port that plays `script` against the
    /// client: `Say(bytes)` sends, `Hear(n)` reads n bytes and records them.
    pub enum Step {
        Say(Vec<u8>),
        Hear(usize),
    }

    pub fn server(script: Vec<Step>) -> (u16, Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, heard) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            for step in script {
                match step {
                    Step::Say(bytes) => s.write_all(&bytes).unwrap(),
                    Step::Hear(n) => {
                        let mut buf = vec![0_u8; n];
                        if s.read_exact(&mut buf).is_err() {
                            return;
                        }
                        if tx.send(buf).is_err() {
                            return;
                        }
                    }
                }
            }
            // Hold the connection open until the client goes.
            let mut rest = Vec::new();
            let _ = s.read_to_end(&mut rest);
        });
        (port, heard)
    }

    /// The server's side of a handshake with no password, a 4x2 desktop
    /// named "desk".
    pub fn handshake_none() -> Vec<Step> {
        let mut init = vec![0, 4, 0, 2];
        init.extend_from_slice(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0]);
        init.extend_from_slice(&4_u32.to_be_bytes());
        init.extend_from_slice(b"desk");
        vec![
            Step::Say(b"RFB 003.008\n".to_vec()),
            Step::Hear(12),
            Step::Say(vec![1, 1]),
            Step::Hear(1),
            Step::Say(0_u32.to_be_bytes().to_vec()),
            Step::Hear(1),
            Step::Say(init),
            Step::Hear(20), // SetPixelFormat
            Step::Hear(16), // SetEncodings, three of them
            Step::Hear(10), // the first FramebufferUpdateRequest
        ]
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation
    )]

    use super::fake::{Step, handshake_none, server};
    use super::*;
    use std::time::Instant;

    fn collect(session: &Session, until: impl Fn(&[Update]) -> bool) -> Vec<Update> {
        let give_up = Instant::now() + Duration::from_secs(5);
        let mut all = Vec::new();
        while !until(&all) {
            assert!(Instant::now() < give_up, "it never came: {all:?}");
            all.extend(session.drain());
            std::thread::sleep(Duration::from_millis(10));
        }
        all
    }

    /// **A session shakes hands and shows the screen**: version, no
    /// password, the desktop's size and name, the pixel format and
    /// encodings asked for, the whole screen requested -- and a Raw
    /// rectangle's pixels delivered as `0x00RRGGBB`, then the next frame
    /// asked for, incrementally.
    #[test]
    fn a_session_shakes_hands_and_shows_the_screen() {
        let mut script = handshake_none();
        // One FramebufferUpdate: a 2x1 Raw rectangle at (1, 1).
        let mut update = vec![0, 0, 0, 1, 0, 1, 0, 1, 0, 2, 0, 1];
        update.extend_from_slice(&RAW.to_be_bytes());
        update.extend_from_slice(&[0x33, 0x22, 0x11, 0x00, 0x66, 0x55, 0x44, 0x00]);
        script.push(Step::Say(update));
        script.push(Step::Hear(10)); // the incremental request after it
        let (port, heard) = server(script);

        let session = Session::open("127.0.0.1", port, "", None).unwrap();
        let updates = collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::Pixels { .. }))
        });
        assert_eq!(
            updates.first(),
            Some(&Update::Ready {
                width: 4,
                height: 2,
                name: String::from("desk")
            })
        );
        assert!(updates.contains(&Update::Pixels {
            x: 1,
            y: 1,
            w: 2,
            h: 1,
            pixels: vec![0x0011_2233, 0x0044_5566]
        }));

        let version = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(version, b"RFB 003.008\n");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).unwrap(),
            [1],
            "chose no password"
        );
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).unwrap(),
            [1],
            "asked to share"
        );
        let format = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            &format[4..8],
            &[32, 24, 0, 1],
            "not 32-bit little-endian true colour"
        );
        let encodings = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(&encodings[..4], &[2, 0, 0, 3]);
        let first = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            first,
            [3, 0, 0, 0, 0, 0, 0, 4, 0, 2],
            "the whole screen, not incremental"
        );
        let next = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            next[..2],
            [3, 1],
            "the next frame was not asked for incrementally"
        );
    }

    /// A password is proven by VNC's challenge, answered with DES.
    #[test]
    fn a_password_is_proven_by_the_challenge() {
        let challenge: Vec<u8> = (0..16).collect();
        let mut init = vec![0, 1, 0, 1];
        init.extend_from_slice(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0]);
        init.extend_from_slice(&0_u32.to_be_bytes());
        let script = vec![
            Step::Say(b"RFB 003.008\n".to_vec()),
            Step::Hear(12),
            Step::Say(vec![1, 2]),
            Step::Hear(1),
            Step::Say(challenge),
            Step::Hear(16),
            Step::Say(0_u32.to_be_bytes().to_vec()),
            Step::Hear(1),
            Step::Say(init),
            Step::Hear(20),
            Step::Hear(16),
            Step::Hear(10),
        ];
        let (port, heard) = server(script);
        let session = Session::open("127.0.0.1", port, "password", None).unwrap();
        collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::Ready { .. }))
        });
        let _version = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).unwrap(),
            [2],
            "did not choose the password"
        );
        let response = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        let hex: String = response.iter().fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}"); // writing to a String cannot fail
            hex
        });
        assert_eq!(hex, "b866924125c8eebb9debc1db61c538e2");
    }

    /// A refused sign-in ends the session in the server's words.
    #[test]
    fn a_refused_sign_in_is_said() {
        let mut reason = 11_u32.to_be_bytes().to_vec();
        reason.extend_from_slice(b"bad secret!");
        let mut failed = 1_u32.to_be_bytes().to_vec();
        failed.extend_from_slice(&reason);
        let script = vec![
            Step::Say(b"RFB 003.008\n".to_vec()),
            Step::Hear(12),
            Step::Say(vec![1, 2]),
            Step::Hear(1),
            Step::Say(vec![0; 16]),
            Step::Hear(16),
            Step::Say(failed),
        ];
        let (port, _heard) = server(script);
        let session = Session::open("127.0.0.1", port, "wrong", None).unwrap();
        let updates = collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::Closed(_)))
        });
        let Some(Update::Closed(why)) = updates.last() else {
            panic!("{updates:?}");
        };
        assert!(why.contains("bad secret!"), "{why}");
    }

    /// A rectangle outside the desktop ends the session instead of being
    /// read into memory the server chose.
    #[test]
    fn a_rectangle_outside_the_desktop_is_refused() {
        // Past the right edge only, then past the bottom only: each bound
        // is checked on its own, not merely the two together.
        refused_at(3, 0, 2, 1);
        refused_at(0, 1, 1, 2);
    }

    fn refused_at(x: u16, y: u16, w: u16, h: u16) {
        let mut script = handshake_none();
        let mut update = vec![0, 0, 0, 1];
        for v in [x, y, w, h] {
            update.extend_from_slice(&v.to_be_bytes());
        }
        update.extend_from_slice(&RAW.to_be_bytes());
        script.push(Step::Say(update));
        let (port, _heard) = server(script);
        let session = Session::open("127.0.0.1", port, "", None).unwrap();
        let updates = collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::Closed(_)))
        });
        let Some(Update::Closed(why)) = updates.last() else {
            panic!("{updates:?}");
        };
        assert!(why.contains("outside"), "{why}");
        assert!(!updates.iter().any(|u| matches!(u, Update::Pixels { .. })));
    }

    /// CopyRect, a resize, the bell and clipboard text all arrive.
    #[test]
    fn every_other_message_arrives() {
        let mut script = handshake_none();
        let mut update = vec![0, 0, 0, 2];
        update.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 1]);
        update.extend_from_slice(&COPY_RECT.to_be_bytes());
        update.extend_from_slice(&[0, 2, 0, 1]);
        update.extend_from_slice(&[0, 0, 0, 0, 0, 8, 0, 6]);
        update.extend_from_slice(&DESKTOP_SIZE.to_be_bytes());
        script.push(Step::Say(update));
        script.push(Step::Hear(10));
        script.push(Step::Say(vec![2]));
        let mut cut = vec![3, 0, 0, 0];
        cut.extend_from_slice(&2_u32.to_be_bytes());
        cut.extend_from_slice(b"hi");
        script.push(Step::Say(cut));
        let (port, heard) = server(script);
        let session = Session::open("127.0.0.1", port, "", None).unwrap();
        let updates = collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::CutText(_)))
        });
        assert!(updates.contains(&Update::Copy {
            src_x: 2,
            src_y: 1,
            x: 0,
            y: 0,
            w: 1,
            h: 1
        }));
        assert!(updates.contains(&Update::Resized {
            width: 8,
            height: 6
        }));
        assert!(updates.contains(&Update::Bell));
        assert!(updates.contains(&Update::CutText(String::from("hi"))));
        // The handshake's six: version, choice, share, format, encodings,
        // and the first request.
        for _ in 0..6 {
            let _ = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let after_resize = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            after_resize,
            [3, 0, 0, 0, 0, 0, 0, 8, 0, 6],
            "after a resize the whole new screen was not asked for"
        );
    }

    /// Keys and the pointer go out in RFB's shape.
    #[test]
    fn keys_and_the_pointer_are_sent_as_rfb_says() {
        let mut script = handshake_none();
        script.push(Step::Hear(8));
        script.push(Step::Hear(6));
        let (port, heard) = server(script);
        let session = Session::open("127.0.0.1", port, "", None).unwrap();
        collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::Ready { .. }))
        });
        for _ in 0..6 {
            let _ = heard.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        session.key(true, keysym_of_char('a')).unwrap();
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).unwrap(),
            [4, 1, 0, 0, 0, 0, 0, 0x61]
        );
        session.pointer(1, 300, 2).unwrap();
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).unwrap(),
            [5, 1, 1, 44, 0, 2]
        );
    }

    /// A character outside Latin-1 is sent as its code point, flagged.
    #[test]
    fn keysyms_follow_the_convention() {
        assert_eq!(keysym_of_char('A'), 0x41);
        assert_eq!(keysym_of_char('\u{e9}'), 0xE9);
        assert_eq!(keysym_of_char('\u{20ac}'), 0x0100_20AC);
    }
}
