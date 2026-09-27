//! One connection to one peer: the handshake, the framed messages after it,
//! and the bookkeeping of a piece being fetched a block at a time.
//!
//! **Reads are short, and a short read loses nothing.** A connection is read
//! with a timeout of a second or so, so that the thread driving it can notice
//! it has been asked to stop. A read that times out in the middle of a
//! message must not throw away the half it got -- `read_exact` would, and the
//! next read would start in the middle of a message and misread everything
//! after it. [`PeerConn::poll`] keeps what has arrived in a buffer and hands
//! out a message only once all of it is there.
//!
//! **A peer is a stranger.** Its messages are capped ([`MAX_MESSAGE`]); its
//! handshake must name the torrent this connection is for; a block it sends
//! is taken only if it was asked for, at the offset and length asked.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::{Handshake, PeerMessage};

/// The most one message may hold: a bitfield for a torrent of sixteen
/// million pieces, and well past a 16 KiB block.
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024;
/// The size of the blocks a piece is asked for in: what every client asks
/// for, and the most most clients will send.
pub const BLOCK: u32 = 16 * 1024;
/// A handshake's length with the standard protocol name.
const HANDSHAKE_LEN: usize = 68;

/// A connection to a peer that has answered the handshake.
#[derive(Debug)]
pub struct PeerConn {
    stream: TcpStream,
    /// What has arrived and not yet been handed out as a message.
    pending: Vec<u8>,
    /// Where the peer is.
    pub addr: SocketAddr,
    /// The id it gave.
    pub remote_id: [u8; 20],
    /// Whether it speaks the extension protocol (BEP 10).
    pub extensions: bool,
}

impl PeerConn {
    /// Connect to `addr` and trade handshakes for the torrent `info_hash`,
    /// within `timeout`.
    ///
    /// # Errors
    ///
    /// The peer could not be reached, or its handshake is not for this
    /// torrent or is not a handshake at all.
    pub fn connect(
        addr: SocketAddr,
        info_hash: [u8; 20],
        my_id: [u8; 20],
        timeout: Duration,
    ) -> Result<Self, String> {
        let stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| format!("could not reach {addr}: {e}"))?;
        Self::handshake(stream, addr, info_hash, my_id, timeout)
    }

    /// Trade handshakes on `stream`, already connected to `addr`.
    ///
    /// # Errors
    ///
    /// As [`Self::connect`].
    pub fn handshake(
        mut stream: TcpStream,
        addr: SocketAddr,
        info_hash: [u8; 20],
        my_id: [u8; 20],
        timeout: Duration,
    ) -> Result<Self, String> {
        let broke = |e: io::Error| format!("{addr} broke off the handshake: {e}");
        stream.set_nodelay(true).map_err(broke)?;
        stream.set_write_timeout(Some(timeout)).map_err(broke)?;
        stream.set_read_timeout(Some(timeout)).map_err(broke)?;
        stream
            .write_all(&Handshake::new(info_hash, my_id).encode())
            .map_err(broke)?;
        let mut theirs = [0_u8; HANDSHAKE_LEN];
        stream.read_exact(&mut theirs).map_err(broke)?;
        let got = Handshake::decode(&theirs)?;
        if got.protocol != Handshake::PROTOCOL {
            return Err(format!("{addr} does not speak BitTorrent"));
        }
        if got.info_hash != info_hash {
            return Err(format!("{addr} answered for another torrent"));
        }
        Ok(Self {
            stream,
            pending: Vec::new(),
            addr,
            remote_id: got.peer_id,
            extensions: got.supports_extensions(),
        })
    }

    /// Send `msg`.
    ///
    /// # Errors
    ///
    /// The connection is gone.
    pub fn send(&mut self, msg: &PeerMessage) -> Result<(), String> {
        self.stream
            .write_all(&msg.encode())
            .map_err(|e| format!("{} went away: {e}", self.addr))
    }

    /// The next whole message, waiting at most `wait` for more of it to
    /// arrive: `Ok(None)` when none is whole yet. What arrived is kept.
    ///
    /// # Errors
    ///
    /// The connection closed or failed, or the peer sent a message longer
    /// than [`MAX_MESSAGE`] or one that does not decode.
    pub fn poll(&mut self, wait: Duration) -> Result<Option<PeerMessage>, String> {
        if let Some(msg) = self.take_message()? {
            return Ok(Some(msg));
        }
        let deadline = Instant::now() + wait;
        let mut buf = [0_u8; 16 * 1024];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.stream
                .set_read_timeout(Some(left))
                .map_err(|e| format!("{} went away: {e}", self.addr))?;
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(format!("{} closed the connection", self.addr)),
                Ok(n) => {
                    self.pending
                        .extend_from_slice(buf.get(..n).unwrap_or_default());
                    if let Some(msg) = self.take_message()? {
                        return Ok(Some(msg));
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(format!("{} went away: {e}", self.addr)),
            }
        }
    }

    /// A whole message off the front of what has arrived, if there is one.
    fn take_message(&mut self) -> Result<Option<PeerMessage>, String> {
        let Some(len) = self
            .pending
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_be_bytes)
        else {
            return Ok(None);
        };
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        if len > MAX_MESSAGE {
            return Err(format!("{} sent a message of {len} bytes", self.addr));
        }
        let end = 4_usize.saturating_add(len);
        let Some(body) = self.pending.get(4..end) else {
            return Ok(None);
        };
        let msg = PeerMessage::decode(body).map_err(|e| format!("{}: {e}", self.addr))?;
        self.pending.drain(..end);
        Ok(Some(msg))
    }
}

/// A piece being fetched: which of its blocks have been asked for and
/// which have come, and the bytes so far.
#[derive(Debug, Clone)]
pub struct Assembly {
    /// Which piece.
    pub index: u32,
    data: Vec<u8>,
    /// Per block: asked for and not yet come.
    asked: Vec<bool>,
    /// Per block: come.
    got: Vec<bool>,
}

impl Assembly {
    /// Piece `index`, `len` bytes long.
    #[must_use]
    pub fn new(index: u32, len: usize) -> Self {
        let blocks = len.div_ceil(BLOCK as usize);
        Self {
            index,
            data: vec![0; len],
            asked: vec![false; blocks],
            got: vec![false; blocks],
        }
    }

    /// Where block `b` starts, and how long it is.
    fn block(&self, b: usize) -> (u32, u32) {
        let start = b.saturating_mul(BLOCK as usize);
        let len = self.data.len().saturating_sub(start).min(BLOCK as usize);
        (
            u32::try_from(start).unwrap_or(u32::MAX),
            u32::try_from(len).unwrap_or(0),
        )
    }

    /// Requests for blocks not yet asked for or come, as many as keep
    /// `outstanding` in flight at most. Each is marked asked.
    pub fn next_requests(&mut self, outstanding: usize) -> Vec<PeerMessage> {
        let in_flight = self.asked.iter().filter(|&&a| a).count();
        let room = outstanding.saturating_sub(in_flight);
        let wanted: Vec<usize> = (0..self.got.len())
            .filter(|&b| {
                !self.got.get(b).copied().unwrap_or(true)
                    && !self.asked.get(b).copied().unwrap_or(true)
            })
            .take(room)
            .collect();
        wanted
            .into_iter()
            .map(|b| {
                if let Some(a) = self.asked.get_mut(b) {
                    *a = true;
                }
                let (begin, length) = self.block(b);
                PeerMessage::Request {
                    index: self.index,
                    begin,
                    length,
                }
            })
            .collect()
    }

    /// A block that came: taken if it is one this piece asked for, at its
    /// offset and whole. Whether it was taken.
    pub fn receive(&mut self, begin: u32, bytes: &[u8]) -> bool {
        if !begin.is_multiple_of(BLOCK) {
            return false;
        }
        let b = (begin / BLOCK) as usize;
        let (start, len) = self.block(b);
        if !self.asked.get(b).copied().unwrap_or(false) || bytes.len() != len as usize {
            return false;
        }
        let start = start as usize;
        let Some(slot) = self.data.get_mut(start..start.saturating_add(bytes.len())) else {
            return false;
        };
        slot.copy_from_slice(bytes);
        if let Some(a) = self.asked.get_mut(b) {
            *a = false;
        }
        if let Some(g) = self.got.get_mut(b) {
            *g = true;
        }
        true
    }

    /// Forget what was asked and not come: after a choke, the peer drops
    /// every request it had, and they must be asked again.
    pub fn forget_asked(&mut self) {
        self.asked.iter_mut().for_each(|a| *a = false);
    }

    /// How many blocks are asked for and not come.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.asked.iter().filter(|&&a| a).count()
    }

    /// Whether every block has come.
    #[must_use]
    pub fn is_whole(&self) -> bool {
        self.got.iter().all(|&g| g)
    }

    /// The bytes, once whole.
    #[must_use]
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
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

    const PATIENT: Duration = Duration::from_secs(5);
    const HASH: [u8; 20] = [0x11; 20];
    const ME: [u8; 20] = *b"-SL0001-000000000000";
    const THEM: [u8; 20] = *b"-XX0001-111111111111";

    /// A peer on a loopback port that answers the handshake for `hash` and
    /// then runs `then` on the connection.
    fn peer(
        hash: [u8; 20],
        then: impl FnOnce(TcpStream) + Send + 'static,
    ) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut theirs = [0_u8; HANDSHAKE_LEN];
            stream.read_exact(&mut theirs).unwrap();
            stream
                .write_all(&Handshake::new(hash, THEM).encode())
                .unwrap();
            then(stream);
        });
        (addr, handle)
    }

    /// The handshake is traded, and the peer's id and extension bit read.
    #[test]
    fn a_handshake_is_traded() {
        let (addr, server) = peer(HASH, |mut s| {
            let mut interested = [0_u8; 5];
            s.read_exact(&mut interested).unwrap();
            assert_eq!(
                interested,
                [0, 0, 0, 1, 2],
                "what was sent after the handshake"
            );
        });
        let mut conn = PeerConn::connect(addr, HASH, ME, PATIENT).unwrap();
        assert_eq!(conn.remote_id, THEM);
        assert!(conn.extensions);
        conn.send(&PeerMessage::Interested).unwrap();
        server.join().unwrap();
    }

    /// A peer answering for another torrent is refused.
    #[test]
    fn a_handshake_for_another_torrent_is_refused() {
        let (addr, server) = peer([0x22; 20], |_| {});
        let err = PeerConn::connect(addr, HASH, ME, PATIENT).unwrap_err();
        assert!(err.contains("another torrent"), "{err}");
        server.join().unwrap();
    }

    /// A message split across reads -- and across a timeout -- arrives
    /// whole; nothing that came before the timeout is lost.
    #[test]
    fn a_message_split_across_a_timeout_arrives_whole() {
        let piece = PeerMessage::Piece {
            index: 3,
            begin: 0,
            data: vec![9; 1000],
        }
        .encode();
        // The second half is sent only once the client has taken the first
        // and timed out waiting for more -- a handshake with the test rather
        // than a sleep, so a loaded machine cannot reorder it.
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (addr, server) = peer(HASH, move |mut s| {
            s.write_all(&piece[..500]).unwrap();
            s.flush().unwrap();
            wait.recv_timeout(PATIENT).unwrap();
            s.write_all(&piece[500..]).unwrap();
            s.write_all(&PeerMessage::Unchoke.encode()).unwrap();
            // Held open until the client has read it all; a signal or the
            // test giving up are the same cue here, so which it was is moot.
            let _ = wait.recv_timeout(PATIENT);
        });
        let mut conn = PeerConn::connect(addr, HASH, ME, PATIENT).unwrap();
        // Polls give up, holding the half that came, until all of it has.
        let deadline = Instant::now() + PATIENT;
        while conn.pending.len() < 500 && Instant::now() < deadline {
            assert_eq!(conn.poll(Duration::from_millis(50)).unwrap(), None);
        }
        assert_eq!(conn.pending.len(), 500, "the first half never came");
        assert_eq!(conn.poll(Duration::from_millis(50)).unwrap(), None);
        go.send(()).unwrap();
        let mut got = Vec::new();
        while got.len() < 2 && Instant::now() < deadline + PATIENT {
            if let Some(msg) = conn.poll(Duration::from_millis(100)).unwrap() {
                got.push(msg);
            }
        }
        go.send(()).unwrap();
        assert_eq!(
            got,
            vec![
                PeerMessage::Piece {
                    index: 3,
                    begin: 0,
                    data: vec![9; 1000]
                },
                PeerMessage::Unchoke
            ]
        );
        server.join().unwrap();
    }

    /// A message longer than the cap ends the connection; so does a close.
    #[test]
    fn an_oversized_message_or_a_close_ends_the_connection() {
        let (addr, server) = peer(HASH, |mut s| {
            s.write_all(&u32::try_from(MAX_MESSAGE + 1).unwrap().to_be_bytes())
                .unwrap();
            thread::sleep(Duration::from_millis(200));
        });
        let mut conn = PeerConn::connect(addr, HASH, ME, PATIENT).unwrap();
        let err = loop {
            match conn.poll(Duration::from_millis(100)) {
                Ok(_) => {}
                Err(e) => break e,
            }
        };
        assert!(err.contains("message of"), "{err}");
        server.join().unwrap();

        let (addr, server) = peer(HASH, drop);
        let mut conn = PeerConn::connect(addr, HASH, ME, PATIENT).unwrap();
        server.join().unwrap();
        let err = loop {
            match conn.poll(Duration::from_millis(100)) {
                Ok(_) => {}
                Err(e) => break e,
            }
        };
        assert!(err.contains("closed") || err.contains("went away"), "{err}");
    }

    /// A piece is asked for a block at a time, a few in flight; a block
    /// that was not asked for, is misplaced, or is the wrong length is not
    /// taken; after a choke everything outstanding is asked again.
    #[test]
    fn a_piece_is_assembled_from_the_blocks_asked_for() {
        let len = (BLOCK * 2 + 100) as usize;
        let mut a = Assembly::new(5, len);
        let first = a.next_requests(2);
        assert_eq!(
            first,
            vec![
                PeerMessage::Request {
                    index: 5,
                    begin: 0,
                    length: BLOCK
                },
                PeerMessage::Request {
                    index: 5,
                    begin: BLOCK,
                    length: BLOCK
                },
            ]
        );
        assert!(a.next_requests(2).is_empty(), "two already in flight");
        assert!(
            !a.receive(BLOCK + 1, &vec![1; BLOCK as usize]),
            "a whole block, asked for, but off a block's edge"
        );
        assert!(!a.receive(BLOCK * 2, &[1; 100]), "not asked for yet");
        assert!(!a.receive(1, &[1; 100]), "not on a block's edge");
        assert!(!a.receive(0, &[1; 10]), "the wrong length");
        assert!(a.receive(0, &vec![1; BLOCK as usize]));
        assert!(
            !a.receive(0, &vec![1; BLOCK as usize]),
            "the same block twice"
        );
        assert_eq!(
            a.next_requests(2),
            vec![PeerMessage::Request {
                index: 5,
                begin: BLOCK * 2,
                length: 100
            }],
            "the last block is short"
        );
        a.forget_asked();
        assert_eq!(a.in_flight(), 0);
        assert_eq!(a.next_requests(8).len(), 2, "both outstanding asked again");
        assert!(a.receive(BLOCK, &vec![2; BLOCK as usize]));
        assert!(!a.is_whole());
        assert!(a.receive(BLOCK * 2, &[3; 100]));
        assert!(a.is_whole());
        let data = a.into_data();
        assert_eq!(data.len(), len);
        assert_eq!((data[0], data[BLOCK as usize], data[len - 1]), (1, 2, 3));
    }
}
