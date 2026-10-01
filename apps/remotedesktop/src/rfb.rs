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
//! **Encodings.** ZRLE first -- 64x64 tiles of palettes and runs, through one
//! zlib stream that lasts the whole session (`deflate::PiecewiseInflater`
//! inflates each rectangle's piece as it comes) -- then Hextile, 16x16 tiles
//! of a background with rectangles of colour over it, then CopyRect and Raw,
//! which every server speaks, and the DesktopSize pseudo-encoding so a
//! resized remote desktop is followed. Tight is not asked for (it wants
//! JPEG too), and a server falls back for a client that does not name an
//! encoding (RFC 6143 section 7.7).
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
use std::time::{Duration, Instant};

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
const HEXTILE: i32 = 5;
const ZRLE: i32 = 16;

/// The most a ZRLE rectangle's compressed data may be, read before it is
/// inflated: a raw 64x64 tile costs 12 KiB, so a desktop's worth is far
/// under this.
const MAX_ZRLE_BYTES: u32 = 64 << 20;
const DESKTOP_SIZE: i32 = -223;

/// How many bits a pixel crosses the network in (RFC 6143 section 7.4):
/// what a profile's colour depth asks of the server, in the three sizes RFB
/// allows. Fewer bits are fewer bytes -- a 16-bit screen is half the traffic
/// of a 32-bit one, an 8-bit one a quarter -- at the cost of colours.
///
/// Whatever crosses the network, every [`Update::Pixels`] is `0x00RRGGBB`:
/// each pixel is converted as it is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelBits {
    /// 256 colours: three bits of red, three of green and two of blue, blue
    /// highest -- the BGR233 layout VNC viewers ask for at eight bits.
    Eight,
    /// 65,536 colours: five bits of red, six of green, five of blue.
    Sixteen,
    /// Every colour a screen shows: 24 bits of it in 32, the top byte unused.
    ThirtyTwo,
}

impl PixelBits {
    /// The SetPixelFormat message asking for this: little endian, true
    /// colour, with its layout's maxima and shifts.
    fn set_pixel_format(self) -> [u8; 20] {
        let (bits, depth, [r_max, g_max, b_max], [r_shift, g_shift, b_shift]): (
            u8,
            u8,
            [u16; 3],
            [u8; 3],
        ) = match self {
            Self::Eight => (8, 8, [7, 7, 3], [0, 3, 6]),
            Self::Sixteen => (16, 16, [31, 63, 31], [11, 5, 0]),
            Self::ThirtyTwo => (32, 24, [255, 255, 255], [16, 8, 0]),
        };
        let ([r0, r1], [g0, g1], [b0, b1]) = (
            r_max.to_be_bytes(),
            g_max.to_be_bytes(),
            b_max.to_be_bytes(),
        );
        [
            0, 0, 0, 0, bits, depth, 0, 1, r0, r1, g0, g1, b0, b1, r_shift, g_shift, b_shift, 0, 0,
            0,
        ]
    }

    /// Bytes a pixel takes on the network.
    fn bytes(self) -> usize {
        match self {
            Self::Eight => 1,
            Self::Sixteen => 2,
            Self::ThirtyTwo => 4,
        }
    }

    /// Bytes a ZRLE compact pixel takes: three for 32 bits that carry 24,
    /// otherwise a pixel's own (RFC 6143 section 7.7.6).
    fn compact_bytes(self) -> usize {
        match self {
            Self::Eight => 1,
            Self::Sixteen => 2,
            Self::ThirtyTwo => 3,
        }
    }

    /// A pixel's bytes, little endian, as `0x00RRGGBB`.
    ///
    /// Each channel is widened to eight bits by scaling, rounded, so its
    /// full intensity stays full: five bits of 31 is 255, not 248.
    fn rgb(self, bytes: &[u8]) -> u32 {
        let raw = bytes
            .iter()
            .rev()
            .fold(0_u32, |acc, b| (acc << 8) | u32::from(*b));
        let widen = |value: u32, max: u32| -> u32 {
            value
                .saturating_mul(255)
                .saturating_add(max / 2)
                .checked_div(max)
                .unwrap_or(0)
        };
        let (r, g, b) = match self {
            Self::Eight => (
                widen(raw & 7, 7),
                widen((raw >> 3) & 7, 7),
                widen((raw >> 6) & 3, 3),
            ),
            Self::Sixteen => (
                widen((raw >> 11) & 31, 31),
                widen((raw >> 5) & 63, 63),
                widen(raw & 31, 31),
            ),
            Self::ThirtyTwo => return raw & 0x00FF_FFFF,
        };
        (r << 16) | (g << 8) | b
    }
}

/// What a session asks of the server's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picture {
    /// How many bits a pixel.
    pub bits: PixelBits,
    /// Frames a second at most; 0 asks for each as soon as the last arrives.
    pub frames_per_second: u8,
}

impl Picture {
    /// Full colour, as fast as the server sends it -- for the tests; a
    /// session the window opens asks for its profile's.
    #[cfg(test)]
    pub const FULL: Self = Self {
        bits: PixelBits::ThirtyTwo,
        frames_per_second: 0,
    };

    /// The least time between one frame's request and the next's.
    ///
    /// Asking for the next frame the moment the last arrives is what makes
    /// a VNC client use all the bandwidth a busy screen can take; a frame
    /// rate is kept by asking no sooner than this after the last request.
    #[must_use]
    pub fn frame_gap(self) -> Option<Duration> {
        Duration::from_secs(1).checked_div(u32::from(self.frames_per_second))
    }
}

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
    /// the server asks for none), on a thread of its own, asking for
    /// `picture` and waking `waker` with each batch of news.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started.
    pub fn open(
        host: &str,
        port: u16,
        password: &str,
        picture: Picture,
        waker: Option<Waker>,
    ) -> io::Result<Self> {
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
                let why = run(&host, port, &password, picture, &slot, &tell);
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
    picture: Picture,
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
    let mut conn = Conn {
        stream,
        writer,
        zrle: deflate::PiecewiseInflater::zlib(),
        bits: picture.bits,
        gap: picture.frame_gap(),
        asked: None,
    };
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
    /// ZRLE's zlib stream, which lasts the whole session.
    zrle: deflate::PiecewiseInflater,
    /// How many bits a pixel was asked for.
    bits: PixelBits,
    /// The least time between two requests for a frame; none, no limit.
    gap: Option<Duration>,
    /// When a frame was last asked for.
    asked: Option<Instant>,
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

        // SetPixelFormat: the bits a pixel the profile asked for, little
        // endian, true colour.
        self.write(&self.bits.set_pixel_format())?;
        // SetEncodings, most wanted first: ZRLE, Hextile, CopyRect, Raw, and
        // the DesktopSize pseudo-encoding.
        let mut encodings = vec![2, 0, 0, 5];
        for e in [ZRLE, HEXTILE, COPY_RECT, RAW, DESKTOP_SIZE] {
            encodings.extend_from_slice(&e.to_be_bytes());
        }
        self.write(&encodings)?;
        Ok((width, height, name))
    }

    /// One pixel as the client asked for them, as `0x00RRGGBB`.
    fn pixel(&mut self) -> Result<u32, String> {
        let mut p = [0_u8; 4];
        let bits = self.bits;
        let bytes = p
            .get_mut(..bits.bytes())
            .ok_or_else(|| String::from("a pixel is at most four bytes"))?;
        self.read_exact(bytes)?;
        Ok(bits.rgb(bytes))
    }

    /// A Hextile rectangle (RFC 6143 section 7.7.4), `w` by `h`: 16x16 tiles
    /// left to right, top to bottom, each either raw or a background with
    /// rectangles of colour over it. The background and foreground carry over
    /// from the last tile that named them; a raw tile names neither. A
    /// subrectangle reaching past its tile is clipped to it.
    fn hextile(&mut self, w: u16, h: u16, area: u64) -> Result<Vec<u32>, String> {
        const RAW_TILE: u8 = 1;
        const BACKGROUND: u8 = 2;
        const FOREGROUND: u8 = 4;
        const ANY_SUBRECTS: u8 = 8;
        const COLOURED: u8 = 16;
        let width = usize::from(w);
        let mut out = vec![0_u32; usize::try_from(area).unwrap_or(0)];
        let (mut bg, mut fg) = (0_u32, 0_u32);
        for ty in (0..h).step_by(16) {
            for tx in (0..w).step_by(16) {
                let tw = w.saturating_sub(tx).min(16);
                let th = h.saturating_sub(ty).min(16);
                let kind = self.u8()?;
                if kind & RAW_TILE != 0 {
                    for row in 0..th {
                        for col in 0..tw {
                            let p = self.pixel()?;
                            fill(
                                &mut out,
                                width,
                                (tx.saturating_add(col), ty.saturating_add(row)),
                                (1, 1),
                                p,
                            );
                        }
                    }
                    continue;
                }
                if kind & BACKGROUND != 0 {
                    bg = self.pixel()?;
                }
                if kind & FOREGROUND != 0 {
                    fg = self.pixel()?;
                }
                fill(&mut out, width, (tx, ty), (tw, th), bg);
                if kind & ANY_SUBRECTS == 0 {
                    continue;
                }
                for _ in 0..self.u8()? {
                    let colour = if kind & COLOURED != 0 {
                        self.pixel()?
                    } else {
                        fg
                    };
                    let (xy, wh) = (self.u8()?, self.u8()?);
                    let (sx, sy) = (u16::from(xy >> 4), u16::from(xy & 0xF));
                    let (sw, sh) = (
                        u16::from(wh >> 4).saturating_add(1),
                        u16::from(wh & 0xF).saturating_add(1),
                    );
                    fill(
                        &mut out,
                        width,
                        (tx.saturating_add(sx), ty.saturating_add(sy)),
                        (sw.min(tw.saturating_sub(sx)), sh.min(th.saturating_sub(sy))),
                        colour,
                    );
                }
            }
        }
        Ok(out)
    }

    /// Ask for the screen: all of it, or what changed since the last frame
    /// -- no sooner after the last request than the frame rate allows.
    fn request(&mut self, incremental: bool, width: u16, height: u16) -> Result<(), String> {
        if let (Some(gap), Some(asked)) = (self.gap, self.asked)
            && let Some(wait) = asked
                .checked_add(gap)
                .and_then(|due| due.checked_duration_since(Instant::now()))
        {
            std::thread::sleep(wait);
        }
        self.asked = Some(Instant::now());
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
                ZRLE => {
                    let area = inside((x, y, w, h), (*width, *height))?;
                    let len = self.u32()?;
                    if len > MAX_ZRLE_BYTES {
                        return Err(format!(
                            "the server sent {len} bytes of ZRLE for one rectangle"
                        ));
                    }
                    let mut data = vec![0_u8; usize::try_from(len).unwrap_or(0)];
                    self.read_exact(&mut data)?;
                    // The most a rectangle's tiles can take: every pixel raw
                    // at a compact pixel's size, and a subencoding byte and a
                    // full palette for each tile.
                    let compact = self.bits.compact_bytes() as u64;
                    let tiles = u64::from(w.div_ceil(64)).saturating_mul(u64::from(h.div_ceil(64)));
                    let most = area.saturating_mul(compact).saturating_add(
                        tiles.saturating_mul(compact.saturating_mul(127).saturating_add(1)),
                    );
                    let raw = self
                        .zrle
                        .inflate_piece(&data, usize::try_from(most).unwrap_or(usize::MAX))
                        .map_err(|e| format!("the server's ZRLE stream is broken: {e}"))?;
                    let pixels = zrle_tiles(&raw, w, h, self.bits)?;
                    tell(Update::Pixels { x, y, w, h, pixels });
                }
                HEXTILE => {
                    let area = inside((x, y, w, h), (*width, *height))?;
                    let pixels = self.hextile(w, h, area)?;
                    tell(Update::Pixels { x, y, w, h, pixels });
                }
                RAW => {
                    let area = inside((x, y, w, h), (*width, *height))?;
                    let bits = self.bits;
                    let size = bits.bytes() as u64;
                    let mut raw =
                        vec![0_u8; usize::try_from(area.saturating_mul(size)).unwrap_or(0)];
                    self.read_exact(&mut raw)?;
                    let pixels = raw
                        .chunks_exact(bits.bytes())
                        .map(|p| bits.rgb(p))
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

/// The area of a rectangle a server sent, once it is known to lie inside
/// its own desktop and to be no larger than this client holds.
fn inside((x, y, w, h): (u16, u16, u16, u16), (width, height): (u16, u16)) -> Result<u64, String> {
    let area = u64::from(w).saturating_mul(u64::from(h));
    if area > MAX_PIXELS
        || u32::from(x).saturating_add(u32::from(w)) > u32::from(width)
        || u32::from(y).saturating_add(u32::from(h)) > u32::from(height)
    {
        return Err(format!(
            "the server sent a {w}x{h} rectangle at {x},{y}, outside its own {width}x{height} desktop"
        ));
    }
    Ok(area)
}

/// Bytes read in order, for ZRLE's inflated tiles.
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
    bits: PixelBits,
}

impl Cursor<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let b = self
            .data
            .get(self.at)
            .copied()
            .ok_or_else(|| String::from("the server's ZRLE tile ended early"))?;
        self.at = self.at.saturating_add(1);
        Ok(b)
    }

    /// A compact pixel: at 32 bits a pixel the three bytes of the four that
    /// carry colour, otherwise a whole pixel.
    fn cpixel(&mut self) -> Result<u32, String> {
        let mut p = [0_u8; 3];
        let bits = self.bits;
        for b in p.iter_mut().take(bits.compact_bytes()) {
            *b = self.u8()?;
        }
        Ok(bits.rgb(p.get(..bits.compact_bytes()).unwrap_or(&[])))
    }

    /// A run length: bytes added up while they are 255, plus one.
    fn run(&mut self) -> Result<usize, String> {
        let mut run = 1_usize;
        loop {
            let b = self.u8()?;
            run = run.saturating_add(usize::from(b));
            if b != 255 {
                return Ok(run);
            }
        }
    }

    fn palette(&mut self, n: u8) -> Result<Vec<u32>, String> {
        (0..n).map(|_| self.cpixel()).collect()
    }
}

/// A ZRLE rectangle's inflated data (RFC 6143 section 7.7.6), `w` by `h`:
/// 64x64 tiles left to right, top to bottom, each raw, one colour, a packed
/// palette, or runs of colour or of palette entries. A run past its tile, a
/// palette index past its palette, or a subencoding RFB does not define ends
/// the session in words, never a read out of bounds.
fn zrle_tiles(data: &[u8], w: u16, h: u16, bits: PixelBits) -> Result<Vec<u32>, String> {
    let mut at = Cursor { data, at: 0, bits };
    let width = usize::from(w);
    let mut out = vec![0_u32; width.saturating_mul(usize::from(h))];
    for ty in (0..h).step_by(64) {
        for tx in (0..w).step_by(64) {
            let tw = w.saturating_sub(tx).min(64);
            let th = h.saturating_sub(ty).min(64);
            let area = usize::from(tw).saturating_mul(usize::from(th));
            // The tile's pixels in order, then painted in: a run can cross
            // rows, which a per-pixel `fill` would make awkward.
            let mut tile: Vec<u32> = Vec::with_capacity(area);
            match at.u8()? {
                0 => {
                    for _ in 0..area {
                        tile.push(at.cpixel()?);
                    }
                }
                1 => tile.resize(area, at.cpixel()?),
                n @ 2..=16 => {
                    let palette = at.palette(n)?;
                    // Bits per index, and the mask that keeps them.
                    let (bits, mask): (usize, u8) = match n {
                        2 => (1, 0b1),
                        3 | 4 => (2, 0b11),
                        _ => (4, 0b1111),
                    };
                    for _ in 0..th {
                        let mut byte = 0_u8;
                        let mut left = 0_usize;
                        for _ in 0..tw {
                            if left == 0 {
                                byte = at.u8()?;
                                left = 8;
                            }
                            left = left.saturating_sub(bits);
                            let index = usize::from((byte >> left) & mask);
                            let colour = palette.get(index).copied().ok_or_else(|| {
                                format!("a ZRLE tile names colour {index} of a palette of {n}")
                            })?;
                            tile.push(colour);
                        }
                    }
                }
                128 => {
                    while tile.len() < area {
                        let colour = at.cpixel()?;
                        let run = at.run()?;
                        if run > area.saturating_sub(tile.len()) {
                            return Err(String::from("a ZRLE run reaches past its tile"));
                        }
                        tile.resize(tile.len().saturating_add(run), colour);
                    }
                }
                n @ 130..=255 => {
                    let palette = at.palette(n.saturating_sub(128))?;
                    while tile.len() < area {
                        let index = at.u8()?;
                        let run = if index & 128 != 0 { at.run()? } else { 1 };
                        let colour =
                            palette
                                .get(usize::from(index & 127))
                                .copied()
                                .ok_or_else(|| {
                                    String::from("a ZRLE run names a colour past its palette")
                                })?;
                        if run > area.saturating_sub(tile.len()) {
                            return Err(String::from("a ZRLE run reaches past its tile"));
                        }
                        tile.resize(tile.len().saturating_add(run), colour);
                    }
                }
                other => {
                    return Err(format!(
                        "the server sent ZRLE subencoding {other}, which RFB does not define"
                    ));
                }
            }
            for (row, line) in tile.chunks(usize::from(tw).max(1)).enumerate() {
                let start = usize::from(ty)
                    .saturating_add(row)
                    .saturating_mul(width)
                    .saturating_add(usize::from(tx));
                if let Some(span) = out.get_mut(start..start.saturating_add(line.len())) {
                    span.copy_from_slice(line);
                }
            }
        }
    }
    Ok(out)
}

/// Paint `size` pixels of `colour` at `at` into a rectangle `width` wide;
/// what falls outside it is not painted.
fn fill(out: &mut [u32], width: usize, at: (u16, u16), size: (u16, u16), colour: u32) {
    for row in 0..size.1 {
        let start = usize::from(at.1.saturating_add(row))
            .saturating_mul(width)
            .saturating_add(usize::from(at.0));
        let end = start.saturating_add(usize::from(size.0));
        if let Some(span) = out.get_mut(start..end) {
            span.fill(colour);
        }
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
        handshake_sized(4, 2)
    }

    /// As [`handshake_none`], for a `w` by `h` desktop.
    pub fn handshake_sized(w: u16, h: u16) -> Vec<Step> {
        let mut init = w.to_be_bytes().to_vec();
        init.extend_from_slice(&h.to_be_bytes());
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
            Step::Hear(24), // SetEncodings, five of them
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

        let session = Session::open("127.0.0.1", port, "", Picture::FULL, None).unwrap();
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
        assert_eq!(
            &encodings[..12],
            &[2, 0, 0, 5, 0, 0, 0, 16, 0, 0, 0, 5],
            "ZRLE and then Hextile are not asked for first"
        );
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
            Step::Hear(24),
            Step::Hear(10),
        ];
        let (port, heard) = server(script);
        let session = Session::open("127.0.0.1", port, "password", Picture::FULL, None).unwrap();
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
        let session = Session::open("127.0.0.1", port, "wrong", Picture::FULL, None).unwrap();
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
        let session = Session::open("127.0.0.1", port, "", Picture::FULL, None).unwrap();
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
        let session = Session::open("127.0.0.1", port, "", Picture::FULL, None).unwrap();
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

    /// **Hextile tiles are drawn**: a tile of background with a rectangle of
    /// foreground over it, a raw tile, and a tile naming nothing, which takes
    /// the last background named -- across the raw tile between.
    #[test]
    fn hextile_tiles_are_drawn() {
        use super::fake::handshake_sized;
        let mut script = handshake_sized(36, 2);
        let mut update = vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 36, 0, 2];
        update.extend_from_slice(&HEXTILE.to_be_bytes());
        // Tile 1, 16x2: background, foreground, one subrectangle at (1,0) 2x1.
        update.extend_from_slice(&[
            2 | 4 | 8,
            0x33,
            0x22,
            0x11,
            0,
            0x66,
            0x55,
            0x44,
            0,
            1,
            0x10,
            0x10,
        ]);
        // Tile 2, 16x2: raw, 32 pixels numbered.
        update.push(1);
        for n in 0..32_u8 {
            update.extend_from_slice(&[n, 0, 0, 0]);
        }
        // Tile 3, 4x2: nothing named -- the background carries over.
        update.push(0);
        script.push(Step::Say(update));
        script.push(Step::Hear(10));
        let (port, _heard) = server(script);
        let session = Session::open("127.0.0.1", port, "", Picture::FULL, None).unwrap();
        let updates = collect(&session, |u| {
            u.iter().any(|x| matches!(x, Update::Pixels { .. }))
        });
        let Some(Update::Pixels {
            w: 36,
            h: 2,
            pixels,
            ..
        }) = updates.iter().find(|u| matches!(u, Update::Pixels { .. }))
        else {
            panic!("{updates:?}");
        };
        let (bg, fg) = (0x0011_2233, 0x0044_5566);
        assert_eq!(
            pixels[..4],
            [bg, fg, fg, bg],
            "the subrectangle is not where it was put"
        );
        assert_eq!(
            pixels[36 + 1],
            bg,
            "the subrectangle reached the second row"
        );
        assert_eq!(pixels[16..20], [0, 1, 2, 3], "the raw tile's first row");
        assert_eq!(pixels[36 + 16], 16, "the raw tile's second row");
        assert_eq!(pixels[32..36], [bg; 4], "the background did not carry over");
        assert_eq!(pixels[36 + 35], bg);
    }

    /// **ZRLE rectangles are drawn**, each a piece of one zlib stream (real
    /// zlib 1.3 output, sync-flushed per rectangle, so the later pieces
    /// reach back into the earlier): a packed palette, a plain run, runs of
    /// palette entries, one colour, and raw pixels.
    #[test]
    fn zrle_rectangles_are_drawn_through_one_stream() {
        const PIECES: [&[u8]; 5] = [
            &[
                0x78, 0xda, 0x62, 0x32, 0x56, 0x12, 0x4c, 0x0b, 0x75, 0x49, 0xf8, 0x00, 0x00, 0x00,
                0x00, 0xff, 0xff,
            ],
            &[
                0x6a, 0x98, 0xd9, 0x51, 0xce, 0x0e, 0x00, 0x00, 0x00, 0xff, 0xff,
            ],
            &[
                0x6a, 0x62, 0x64, 0x60, 0x60, 0x62, 0x60, 0x68, 0x60, 0x62, 0x6c, 0x64, 0x06, 0x00,
                0x00, 0x00, 0xff, 0xff,
            ],
            &[0x62, 0x5c, 0xb5, 0xfb, 0x0c, 0x00, 0x00, 0x00, 0xff, 0xff],
            &[
                0x62, 0x00, 0x02, 0x88, 0x10, 0x33, 0x03, 0x03, 0x0b, 0x03, 0x03, 0x2b, 0x03, 0x03,
                0x1b, 0x03, 0x03, 0x3b, 0x03, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff,
            ],
        ];
        let mut script = handshake_none();
        let mut update = vec![0, 0, 0, 5];
        for piece in PIECES {
            update.extend_from_slice(&[0, 0, 0, 0, 0, 4, 0, 2]);
            update.extend_from_slice(&ZRLE.to_be_bytes());
            update.extend_from_slice(&u32::try_from(piece.len()).unwrap().to_be_bytes());
            update.extend_from_slice(piece);
        }
        script.push(Step::Say(update));
        script.push(Step::Hear(10));
        let (port, _heard) = server(script);
        let session = Session::open("127.0.0.1", port, "", Picture::FULL, None).unwrap();
        let updates = collect(&session, |u| {
            u.iter()
                .filter(|x| matches!(x, Update::Pixels { .. }))
                .count()
                == 5
        });
        let frames: Vec<&Vec<u32>> = updates
            .iter()
            .filter_map(|u| match u {
                Update::Pixels { pixels, .. } => Some(pixels),
                _ => None,
            })
            .collect();
        let (c0, c1) = (0x0011_2233, 0x0044_5566);
        assert_eq!(
            *frames[0],
            [c0, c1, c1, c0, c1, c1, c1, c1],
            "the packed palette"
        );
        assert_eq!(*frames[1], [0x0077_8899; 8], "the plain run");
        assert_eq!(*frames[2], [1, 1, 1, 2, 2, 2, 2, 2], "the palette runs");
        assert_eq!(*frames[3], [0x00CC_BBAA; 8], "the solid tile");
        assert_eq!(*frames[4], [0, 1, 2, 3, 4, 5, 6, 7], "the raw tile");
    }

    /// A ZRLE run past its tile ends the session instead of writing on.
    #[test]
    fn a_zrle_run_past_its_tile_is_refused() {
        // Plain RLE: one colour for a run of nine in a 4x2 tile.
        let refused = zrle_tiles(&[128, 1, 2, 3, 8], 4, 2, PixelBits::ThirtyTwo).unwrap_err();
        assert!(refused.contains("past its tile"), "{refused}");
        let unknown = zrle_tiles(&[17], 4, 2, PixelBits::ThirtyTwo).unwrap_err();
        assert!(unknown.contains("does not define"), "{unknown}");
        let short = zrle_tiles(&[0, 1, 2], 4, 2, PixelBits::ThirtyTwo).unwrap_err();
        assert!(short.contains("ended early"), "{short}");
    }

    /// Keys and the pointer go out in RFB's shape.
    #[test]
    fn keys_and_the_pointer_are_sent_as_rfb_says() {
        let mut script = handshake_none();
        script.push(Step::Hear(8));
        script.push(Step::Hear(6));
        let (port, heard) = server(script);
        let session = Session::open("127.0.0.1", port, "", Picture::FULL, None).unwrap();
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
    /// **A profile's colour depth is what the server is asked for**, and
    /// what it sends in fewer bits arrives as the same `0x00RRGGBB`: a 16-bit
    /// Raw rectangle, an 8-bit one, and a 16-bit ZRLE tile of one colour.
    #[test]
    fn fewer_bits_a_pixel_are_asked_for_and_read_as_full_colour() {
        for (bits, format, raw, want) in [
            (
                PixelBits::Sixteen,
                [16_u8, 16, 0, 1, 0, 31, 0, 63, 0, 31, 11, 5, 0],
                // Pure red, then pure blue, in 5-6-5, little endian.
                vec![0x00, 0xF8, 0x1F, 0x00],
                vec![0x00FF_0000, 0x0000_00FF],
            ),
            (
                PixelBits::Eight,
                [8, 8, 0, 1, 0, 7, 0, 7, 0, 3, 0, 3, 6],
                // Pure green, then white, in BGR233.
                vec![0b0011_1000, 0xFF],
                vec![0x0000_FF00, 0x00FF_FFFF],
            ),
        ] {
            let mut script = super::fake::handshake_none();
            let mut update = vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 2, 0, 1];
            update.extend_from_slice(&RAW.to_be_bytes());
            update.extend_from_slice(&raw);
            script.push(Step::Say(update));
            script.push(Step::Hear(10));
            let (port, heard) = server(script);
            let picture = Picture {
                bits,
                frames_per_second: 0,
            };
            let session = Session::open("127.0.0.1", port, "", picture, None).unwrap();
            let updates = collect(&session, |u| {
                u.iter().any(|x| matches!(x, Update::Pixels { .. }))
            });
            assert!(
                updates.contains(&Update::Pixels {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 1,
                    pixels: want.clone()
                }),
                "{bits:?}: {updates:?}"
            );
            for _ in 0..3 {
                heard.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            let asked = heard.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(
                &asked[4..17],
                &format,
                "{bits:?}: the wrong pixel format was asked for"
            );
        }
    }

    #[test]
    fn a_zrle_compact_pixel_is_a_whole_pixel_below_32_bits() {
        // One 2x1 tile, solid (subencoding 1), in 16 bits: pure green.
        let pixels = zrle_tiles(&[1, 0xE0, 0x07], 2, 1, PixelBits::Sixteen).unwrap();
        assert_eq!(pixels, [0x0000_FF00, 0x0000_FF00]);
        let eight = zrle_tiles(&[1, 0b1100_0000], 2, 1, PixelBits::Eight).unwrap();
        assert_eq!(eight, [0x0000_00FF, 0x0000_00FF], "pure blue in BGR233");
    }

    #[test]
    fn every_channel_widens_to_its_full_range() {
        for (bits, bytes, want) in [
            (PixelBits::Sixteen, vec![0xFF, 0xFF], 0x00FF_FFFF),
            (PixelBits::Sixteen, vec![0x00, 0x00], 0),
            (PixelBits::Eight, vec![0xFF], 0x00FF_FFFF),
            (PixelBits::Eight, vec![0x00], 0),
            (
                PixelBits::ThirtyTwo,
                vec![0x33, 0x22, 0x11, 0xFF],
                0x0011_2233,
            ),
            // Half of five bits: 16 of 31 is 132 of 255, rounded.
            (PixelBits::Sixteen, vec![0x00, 0x80], 0x0084_0000),
        ] {
            assert_eq!(bits.rgb(&bytes), want, "{bits:?} {bytes:x?}");
        }
    }

    /// **A frame rate is kept by asking no sooner than it allows.** At ten
    /// frames a second, the request after a frame waits for the tenth of a
    /// second since the last; with no rate it goes at once.
    #[test]
    fn a_frame_rate_spaces_the_requests() {
        assert_eq!(Picture::FULL.frame_gap(), None);
        let ten = Picture {
            bits: PixelBits::ThirtyTwo,
            frames_per_second: 10,
        };
        assert_eq!(ten.frame_gap(), Some(Duration::from_millis(100)));

        let mut script = super::fake::handshake_none();
        for _ in 0..2 {
            // An update with no rectangles, then the request after it.
            script.push(Step::Say(vec![0, 0, 0, 0]));
            script.push(Step::Hear(10));
        }
        let (port, heard) = server(script);
        // Timed from before the session opens to the third request: the
        // client cannot send it sooner than two gaps after the first, so
        // this is a floor that a slow test machine can only raise.
        let opened = Instant::now();
        let _session = Session::open("127.0.0.1", port, "", ten, None).unwrap();
        // Version, sign-in, share, pixel format, encodings, then three
        // requests: the first, and one after each frame.
        for _ in 0..8 {
            heard.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let took = opened.elapsed();
        assert!(
            took >= Duration::from_millis(195),
            "three requests in {took:?} at ten frames a second"
        );
    }

    #[test]
    fn keysyms_follow_the_convention() {
        assert_eq!(keysym_of_char('A'), 0x41);
        assert_eq!(keysym_of_char('\u{e9}'), 0xE9);
        assert_eq!(keysym_of_char('\u{20ac}'), 0x0100_20AC);
    }
}
