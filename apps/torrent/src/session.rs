//! A download: trackers asked for peers, a thread for each peer fetching
//! pieces, every piece checked before it is written, and what is happening
//! told to the window.
//!
//! One coordinating thread owns the download. It checks what is already on
//! disk, announces to the trackers, keeps up to `max_peers` peer threads
//! running from the addresses they return, and forwards what those threads
//! report as [`Event`]s on a channel the window reads between frames. The
//! peer threads share one [`Picker`], which hands each a piece nobody else
//! is fetching -- the rarest the peer has first -- so no piece is fetched
//! twice at once.
//!
//! **What it does not do yet**, and says so in `known-issues.md`: it never
//! uploads (it answers no requests and never unchokes a peer), it does not
//! listen for peers that call it, and at the very end of a download it does
//! not ask a second peer for a piece a slow one is holding (no endgame), so
//! the last piece waits for that peer or for its silence to time out.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::peer::{Assembly, PeerConn};
use crate::storage::Storage;
use crate::tracker::{self, Announced};
use crate::{AnnounceRequest, PeerMessage, TorrentMetainfo, TrackerEvent};

/// How long a peer is given to answer a connection and a handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a tracker is given for each step of an announce.
const TRACKER_TIMEOUT: Duration = Duration::from_secs(15);
/// How long one wait for a peer's message lasts, between looks at the stop
/// flag.
const POLL: Duration = Duration::from_millis(250);
/// Blocks asked for at once from one peer.
const PIPELINE: usize = 8;
/// How long a peer may leave requests unanswered before it is dropped.
const SNUBBED_AFTER: Duration = Duration::from_mins(1);
/// How often something is sent to a quiet peer, so it does not drop us.
const KEEP_ALIVE: Duration = Duration::from_secs(90);
/// Pieces that fail their hash before a peer is dropped for sending them.
const STRIKES: u32 = 3;
/// How long an address that failed is left alone before it is tried again.
const RETRY_AFTER: Duration = Duration::from_mins(2);

/// What to download, and where.
#[derive(Debug, Clone)]
pub struct Plan {
    pub meta: TorrentMetainfo,
    /// The folder the torrent's file or folder goes in.
    pub save_dir: PathBuf,
    /// This client's id for this download.
    pub peer_id: [u8; 20],
    /// The port announced to trackers.
    pub port: u16,
    /// The most peer connections at once.
    pub max_peers: usize,
    /// Which pieces are wanted; all of them when empty.
    pub wanted: Vec<bool>,
}

/// What happened, for the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Which pieces were already on disk and whole when the download began.
    Checked(Vec<bool>),
    /// A tracker answered, with how many peers it gave; or it failed.
    Tracker {
        url: String,
        result: Result<TrackerAnswer, String>,
    },
    /// A peer answered the handshake, with its id and whether it speaks
    /// the extension protocol (BEP 10).
    PeerUp {
        addr: SocketAddr,
        id: [u8; 20],
        extensions: bool,
    },
    /// A peer's connection ended, and why.
    PeerDown { addr: SocketAddr, why: String },
    /// A block arrived: `bytes` of payload.
    Received { addr: SocketAddr, bytes: u64 },
    /// A piece matched its hash and was written.
    Piece { index: usize },
    /// A piece did not match its hash, and will be fetched again.
    BadPiece { index: usize, addr: SocketAddr },
    /// Every wanted piece is on disk.
    Finished,
    /// The download cannot go on: the files cannot be written.
    Failed(String),
}

/// What a tracker said, in brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerAnswer {
    pub peers: usize,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub warning: Option<String>,
}

/// A download running on its own threads.
#[derive(Debug)]
pub struct Session {
    stop: Arc<AtomicBool>,
    /// Set once no thread of this download can write another byte: its peer
    /// threads have ended. Telling the trackers it stopped comes after.
    quiet: Arc<AtomicBool>,
    /// What happened, in order.
    pub events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    /// Start downloading `plan` in the background.
    #[must_use]
    pub fn start(plan: Plan) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let quiet = Arc::new(AtomicBool::new(false));
        let (tx, events) = mpsc::channel();
        let (flag, hush) = (Arc::clone(&stop), Arc::clone(&quiet));
        let thread = thread::Builder::new()
            .name(String::from("torrent-session"))
            .spawn(move || coordinate(&plan, &flag, &hush, &tx))
            .ok();
        Self {
            stop,
            quiet,
            events,
            thread,
        }
    }

    /// Ask the download to stop; its threads end within a moment.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Stop, and wait until nothing of this download can write to its
    /// files any more -- a moment, the length of one peer poll -- but not for
    /// the trackers to be told, which can take seconds and needs no waiting
    /// on. What deleting the files needs first.
    pub fn stop_until_quiet(&self) {
        self.stop();
        if self.thread.is_none() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.quiet.load(Ordering::Relaxed) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Stop, and wait for the coordinating thread to end -- trackers told
    /// and all. For the tests, which must not leave threads behind them; the
    /// window never needs to wait that long ([`Self::stop_until_quiet`]).
    #[cfg(test)]
    pub fn stop_and_wait(mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            // A panic in the session is already over; there is nothing to do
            // with it here but not propagate it into the window.
            let _ = thread.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Which pieces are had, which are being fetched, which are wanted, and how
/// many connected peers have each: shared by the peer threads.
#[derive(Debug)]
struct Picker {
    have: Vec<bool>,
    busy: Vec<bool>,
    wanted: Vec<bool>,
    seen: Vec<u32>,
}

impl Picker {
    /// The rarest wanted piece that `peer` has and nobody is fetching,
    /// marked as being fetched.
    fn pick(&mut self, peer: &[bool]) -> Option<usize> {
        let best = (0..self.have.len())
            .filter(|&i| {
                peer.get(i).copied().unwrap_or(false)
                    && self.wanted.get(i).copied().unwrap_or(false)
                    && !self.have.get(i).copied().unwrap_or(true)
                    && !self.busy.get(i).copied().unwrap_or(true)
            })
            .min_by_key(|&i| self.seen.get(i).copied().unwrap_or(0))?;
        if let Some(b) = self.busy.get_mut(best) {
            *b = true;
        }
        Some(best)
    }

    fn done(&mut self, i: usize) {
        if let Some(h) = self.have.get_mut(i) {
            *h = true;
        }
        self.release(i);
    }

    fn release(&mut self, i: usize) {
        if let Some(b) = self.busy.get_mut(i) {
            *b = false;
        }
    }

    /// A peer that has these pieces joined (`+1`) or left (`-1`).
    fn count(&mut self, has: &[bool], joined: bool) {
        for (n, &h) in self.seen.iter_mut().zip(has) {
            if h {
                *n = if joined {
                    n.saturating_add(1)
                } else {
                    n.saturating_sub(1)
                };
            }
        }
    }

    fn complete(&self) -> bool {
        self.have.iter().zip(&self.wanted).all(|(&h, &w)| h || !w)
    }

    /// Whether `peer` has anything left that is wanted.
    fn useful(&self, peer: &[bool]) -> bool {
        (0..self.have.len()).any(|i| {
            peer.get(i).copied().unwrap_or(false)
                && self.wanted.get(i).copied().unwrap_or(false)
                && !self.have.get(i).copied().unwrap_or(true)
        })
    }
}

fn lock(picker: &Mutex<Picker>) -> std::sync::MutexGuard<'_, Picker> {
    // A peer thread that panicked holding the lock left the picker as it
    // was between two whole updates -- every change under it is one field
    // write -- so what is inside is still true.
    picker.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a peer thread tells the coordinator.
enum Report {
    Up(SocketAddr, [u8; 20], bool),
    Down(SocketAddr, String),
    Received(SocketAddr, u64),
    Piece(usize),
    Bad(usize, SocketAddr),
    Fatal(String),
}

/// Every tracker the torrent names, first tier first, each once.
fn trackers(meta: &TorrentMetainfo) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for url in meta
        .announce_list
        .iter()
        .flatten()
        .chain(std::iter::once(&meta.announce))
    {
        if !url.is_empty() && !out.contains(url) {
            out.push(url.clone());
        }
    }
    out
}

/// The download's own thread.
fn coordinate(plan: &Plan, stop: &Arc<AtomicBool>, quiet: &AtomicBool, tx: &Sender<Event>) {
    let hush = || quiet.store(true, Ordering::Relaxed);
    let say = |event: Event| {
        // The window has gone and dropped its end: there is nobody left to
        // tell, and the stop flag will end this thread shortly.
        let _ = tx.send(event);
    };
    let storage = match Storage::new(&plan.meta, &plan.save_dir) {
        Ok(storage) => Arc::new(storage),
        Err(e) => {
            hush();
            return say(Event::Failed(e));
        }
    };
    let have = match storage.have() {
        Ok(have) => have,
        Err(e) => {
            hush();
            return say(Event::Failed(e));
        }
    };
    say(Event::Checked(have.clone()));
    let count = storage.piece_count();
    let wanted = if plan.wanted.len() == count {
        plan.wanted.clone()
    } else {
        vec![true; count]
    };
    // The pieces the window has been told of: what was on disk, then each
    // Piece event as it is sent. Whether the download is done is judged by
    // this, not by the picker -- a peer thread marks a piece done there
    // before its report reaches this thread, so the picker can be complete
    // while reports are still queued, and finishing then would drop them.
    let mut told = have.clone();
    let complete = |told: &[bool]| told.iter().zip(&wanted).all(|(&h, &w)| h || !w);
    let picker = Arc::new(Mutex::new(Picker {
        have,
        busy: vec![false; count],
        wanted: wanted.clone(),
        seen: vec![0; count],
    }));
    let left = |picker: &Mutex<Picker>| -> u64 {
        let p = lock(picker);
        (0..count)
            .filter(|&i| !p.have.get(i).copied().unwrap_or(false))
            .map(|i| storage.piece_len(i))
            .sum()
    };
    let urls = trackers(&plan.meta);
    let request = |event: TrackerEvent, remaining: u64, downloaded: u64| AnnounceRequest {
        info_hash: plan.meta.info_hash,
        peer_id: plan.peer_id,
        port: plan.port,
        uploaded: 0,
        downloaded,
        left: remaining,
        compact: true,
        event,
        numwant: Some(50),
    };

    if complete(&told) {
        hush();
        return say(Event::Finished);
    }

    let (reports, from_peers) = mpsc::channel::<Report>();
    let mut pool: Vec<SocketAddr> = Vec::new();
    let mut running: HashMap<SocketAddr, JoinHandle<()>> = HashMap::new();
    let mut failed: HashMap<SocketAddr, Instant> = HashMap::new();
    let mut next_announce = Instant::now();
    let mut first = true;
    let mut downloaded = 0_u64;

    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_announce {
            let event = if first {
                TrackerEvent::Started
            } else {
                TrackerEvent::None
            };
            let mut wait = Duration::from_mins(30);
            for url in &urls {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let result = tracker::announce(
                    url,
                    &request(event, left(&picker), downloaded),
                    TRACKER_TIMEOUT,
                );
                if let Ok(Announced {
                    interval, peers, ..
                }) = &result
                {
                    wait = wait.min(*interval);
                    for peer in peers {
                        if !pool.contains(peer) {
                            pool.push(*peer);
                        }
                    }
                }
                say(Event::Tracker {
                    url: url.clone(),
                    result: result.map(|a| TrackerAnswer {
                        peers: a.peers.len(),
                        seeders: a.seeders,
                        leechers: a.leechers,
                        warning: a.warning,
                    }),
                });
            }
            first = false;
            next_announce = Instant::now() + wait;
        }

        // Keep up to `max_peers` peer threads going.
        running.retain(|_, handle| !handle.is_finished());
        for addr in pool.clone() {
            if running.len() >= plan.max_peers.max(1) {
                break;
            }
            if running.contains_key(&addr)
                || failed.get(&addr).is_some_and(|t| t.elapsed() < RETRY_AFTER)
            {
                continue;
            }
            let job = PeerJob {
                addr,
                info_hash: plan.meta.info_hash,
                my_id: plan.peer_id,
                storage: Arc::clone(&storage),
                picker: Arc::clone(&picker),
                stop: Arc::clone(stop),
                reports: reports.clone(),
            };
            if let Ok(handle) = thread::Builder::new()
                .name(format!("torrent-peer-{addr}"))
                .spawn(move || job.run())
            {
                running.insert(addr, handle);
            }
        }

        match from_peers.recv_timeout(POLL) {
            Ok(report) => match report {
                Report::Up(addr, id, extensions) => say(Event::PeerUp {
                    addr,
                    id,
                    extensions,
                }),
                Report::Down(addr, why) => {
                    failed.insert(addr, Instant::now());
                    say(Event::PeerDown { addr, why });
                }
                Report::Received(addr, bytes) => {
                    downloaded = downloaded.saturating_add(bytes);
                    say(Event::Received { addr, bytes });
                }
                Report::Piece(index) => {
                    say(Event::Piece { index });
                    if let Some(t) = told.get_mut(index) {
                        *t = true;
                    }
                    if complete(&told) {
                        stop.store(true, Ordering::Relaxed);
                        finish(&urls, &request(TrackerEvent::Completed, 0, downloaded));
                        say(Event::Finished);
                    }
                }
                Report::Bad(index, addr) => say(Event::BadPiece { index, addr }),
                Report::Fatal(why) => {
                    stop.store(true, Ordering::Relaxed);
                    say(Event::Failed(why));
                }
            },
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
    }

    // Stopped: the peer threads see the flag within a poll, and once they
    // have ended nothing more is written. Then trackers that were told this
    // client started are told it stopped, unless it finished (they were told
    // that instead).
    for (_, handle) in running {
        // Each ends within a poll of the flag; a panic in one is over.
        let _ = handle.join();
    }
    hush();
    if !lock(&picker).complete() && !first {
        finish(
            &urls,
            &request(TrackerEvent::Stopped, left(&picker), downloaded),
        );
    }
}

/// Tell every tracker, briefly, how the download ended. Their answers do not
/// matter: this is a courtesy, and a tracker that cannot hear it forgets
/// this client on its own.
fn finish(urls: &[String], request: &AnnounceRequest) {
    for url in urls {
        // A failure here changes nothing: see above.
        let _ = tracker::announce(url, request, Duration::from_secs(3));
    }
}

/// One peer thread's work.
struct PeerJob {
    addr: SocketAddr,
    info_hash: [u8; 20],
    my_id: [u8; 20],
    storage: Arc<Storage>,
    picker: Arc<Mutex<Picker>>,
    stop: Arc<AtomicBool>,
    reports: Sender<Report>,
}

impl PeerJob {
    fn run(self) {
        let mut has = Vec::new();
        let why = self.fetch(&mut has);
        lock(&self.picker).count(&has, false);
        // The coordinator has gone: nobody is listening.
        let _ = self.reports.send(Report::Down(self.addr, why));
    }

    /// Fetch pieces from this peer until it has nothing more to give, the
    /// download is told to stop, or the connection fails; why it ended.
    /// `has` is left holding the pieces the peer said it has, so they can be
    /// uncounted.
    fn fetch(&self, has: &mut Vec<bool>) -> String {
        let count = self.storage.piece_count();
        let mut conn =
            match PeerConn::connect(self.addr, self.info_hash, self.my_id, CONNECT_TIMEOUT) {
                Ok(conn) => conn,
                Err(e) => return e,
            };
        let _ = self
            .reports
            .send(Report::Up(self.addr, conn.remote_id, conn.extensions)); // A gone coordinator is handled by `run`.
        *has = vec![false; count];
        if let Err(e) = conn.send(&PeerMessage::Interested) {
            return e;
        }
        let mut choked = true;
        let mut current: Option<Assembly> = None;
        let mut strikes = 0_u32;
        let mut last_data = Instant::now();
        let mut last_sent = Instant::now();
        let mut first_message = true;

        loop {
            if self.stop.load(Ordering::Relaxed) {
                return String::from("the download stopped");
            }
            if current.is_none() && !choked {
                let pick = lock(&self.picker).pick(has);
                match pick {
                    Some(index) => {
                        let len = usize::try_from(self.storage.piece_len(index)).unwrap_or(0);
                        current =
                            Some(Assembly::new(u32::try_from(index).unwrap_or(u32::MAX), len));
                        last_data = Instant::now();
                    }
                    None => {
                        let p = lock(&self.picker);
                        if p.complete() {
                            return String::from("the download is complete");
                        }
                        if !first_message && !p.useful(has) {
                            return String::from("it has nothing more that is wanted");
                        }
                    }
                }
            }
            if !choked && let Some(assembly) = current.as_mut() {
                for request in assembly.next_requests(PIPELINE) {
                    if let Err(e) = conn.send(&request) {
                        return self.give_back(current, e);
                    }
                    last_sent = Instant::now();
                }
            }

            let msg = match conn.poll(POLL) {
                Ok(Some(msg)) => msg,
                Ok(None) => {
                    if current.as_ref().is_some_and(|a| a.in_flight() > 0)
                        && last_data.elapsed() > SNUBBED_AFTER
                    {
                        return self.give_back(current, String::from("it stopped sending"));
                    }
                    if last_sent.elapsed() > KEEP_ALIVE {
                        if let Err(e) = conn.send(&PeerMessage::KeepAlive) {
                            return self.give_back(current, e);
                        }
                        last_sent = Instant::now();
                    }
                    continue;
                }
                Err(e) => return self.give_back(current, e),
            };
            let was_first = first_message;
            first_message = false;
            match msg {
                PeerMessage::Choke => {
                    choked = true;
                    if let Some(assembly) = current.as_mut() {
                        assembly.forget_asked();
                    }
                }
                PeerMessage::Unchoke => choked = false,
                PeerMessage::Have { piece_index } => {
                    let i = piece_index as usize;
                    match has.get_mut(i) {
                        Some(h) if !*h => {
                            *h = true;
                            let mut one = vec![false; count];
                            if let Some(o) = one.get_mut(i) {
                                *o = true;
                            }
                            lock(&self.picker).count(&one, true);
                        }
                        Some(_) => {}
                        None => {
                            return self.give_back(
                                current,
                                format!("it claimed piece {i}, which is not in the torrent"),
                            );
                        }
                    }
                }
                PeerMessage::Bitfield(bits) => {
                    // Only as the first message, exactly long enough, with
                    // the spare bits at the end clear.
                    let Some(parsed) = was_first.then(|| bitfield(&bits, count)).flatten() else {
                        return self.give_back(
                            current,
                            String::from("it sent a bitfield that does not fit the torrent"),
                        );
                    };
                    *has = parsed;
                    lock(&self.picker).count(has, true);
                }
                PeerMessage::Piece { index, begin, data } => {
                    let Some(assembly) = current.as_mut().filter(|a| a.index == index) else {
                        continue;
                    };
                    if !assembly.receive(begin, &data) {
                        continue;
                    }
                    last_data = Instant::now();
                    let _ = self
                        .reports
                        .send(Report::Received(self.addr, data.len() as u64)); // As above.
                    if !assembly.is_whole() {
                        continue;
                    }
                    let Some(done) = current.take() else {
                        continue;
                    };
                    let i = done.index as usize;
                    let bytes = done.into_data();
                    if self.storage.matches(i, &bytes) {
                        if let Err(e) = self.storage.write_piece(i, &bytes) {
                            lock(&self.picker).release(i);
                            let _ = self.reports.send(Report::Fatal(e.clone())); // As above.
                            return e;
                        }
                        lock(&self.picker).done(i);
                        let _ = self.reports.send(Report::Piece(i)); // As above.
                        // Telling the peer is a courtesy; a failure shows on
                        // the next send.
                        let _ = conn.send(&PeerMessage::Have {
                            piece_index: done_index(i),
                        });
                    } else {
                        lock(&self.picker).release(i);
                        let _ = self.reports.send(Report::Bad(i, self.addr)); // As above.
                        strikes = strikes.saturating_add(1);
                        if strikes >= STRIKES {
                            return format!("{strikes} of its pieces failed their hash");
                        }
                    }
                }
                // Nothing is uploaded yet, so what a peer asks of us, and
                // what it offers beyond pieces, is left unanswered.
                PeerMessage::KeepAlive
                | PeerMessage::Interested
                | PeerMessage::NotInterested
                | PeerMessage::Request { .. }
                | PeerMessage::Cancel { .. }
                | PeerMessage::Port { .. }
                | PeerMessage::Extended { .. } => {}
            }
        }
    }

    /// Hand a half-fetched piece back to the picker; `why`, for returning.
    fn give_back(&self, current: Option<Assembly>, why: String) -> String {
        if let Some(assembly) = current {
            lock(&self.picker).release(assembly.index as usize);
        }
        why
    }
}

fn done_index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// A bitfield message's bits as pieces, if it is exactly as long as `count`
/// pieces need and its spare bits are clear.
fn bitfield(bits: &[u8], count: usize) -> Option<Vec<bool>> {
    if bits.len() != count.div_ceil(8) {
        return None;
    }
    let all: Vec<bool> = (0..bits.len().saturating_mul(8))
        .map(|i| bits.get(i / 8).is_some_and(|b| b & (0x80 >> (i % 8)) != 0))
        .collect();
    if all.iter().skip(count).any(|&b| b) {
        return None;
    }
    Some(all.into_iter().take(count).collect())
}

/// A tracker and seeding peers on loopback ports, and a torrent for them to
/// serve: what the session's tests and the window's run whole downloads
/// against.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
pub(crate) mod testnet {
    use super::*;
    use crate::TorrentFile;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::AtomicUsize;

    pub(crate) const PIECE: u64 = 32 * 1024;

    /// A scratch folder, removed when dropped.
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(tag: &str) -> Self {
            use std::sync::atomic::AtomicU64;
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "slateos-torrent-session-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// Three files over five pieces of 32 KiB -- two blocks each -- whose
    /// bytes are not all the same, so a misplaced block shows.
    pub(crate) fn content() -> (TorrentMetainfo, Arc<Vec<u8>>) {
        let stream: Vec<u8> = (0..(PIECE * 4 + 5000))
            .map(|i| (i * 7 % 251) as u8)
            .collect();
        let pieces = stream.chunks(PIECE as usize).map(sha1::sha1).collect();
        let mut meta = crate::create_sample_torrent("Set", stream.len() as u64, PIECE, "");
        meta.pieces = pieces;
        meta.multi_file = true;
        meta.name_bytes = b"Set".to_vec();
        meta.info_hash = [0x5A; 20];
        meta.files = vec![
            TorrentFile::named("one.bin", 40_000),
            TorrentFile::named("sub/two.bin", 50_000),
            TorrentFile::named("three.bin", stream.len() as u64 - 90_000),
        ];
        (meta, Arc::new(stream))
    }

    /// How a test peer behaves.
    #[derive(Clone, Copy)]
    pub(crate) enum Serve {
        /// Every piece, honestly.
        Honest,
        /// Every piece, with piece `n` wrong.
        Corrupt(usize),
        /// Never unchokes.
        Never,
    }

    /// A seeding peer on a loopback port, serving `stream` to any number of
    /// connections until `stop`.
    pub(crate) fn seeder(
        meta: &TorrentMetainfo,
        stream: Arc<Vec<u8>>,
        serve: Serve,
        stop: Arc<AtomicBool>,
    ) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let (hash, count) = (meta.info_hash, meta.pieces.len());
        let handle = thread::spawn(move || {
            let mut conns = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                if let Ok((s, _)) = listener.accept() {
                    let stream = Arc::clone(&stream);
                    let stop = Arc::clone(&stop);
                    conns.push(thread::spawn(move || {
                        serve_one(s, &stream, hash, count, serve, &stop);
                    }));
                }
                thread::sleep(Duration::from_millis(10));
            }
            for c in conns {
                drop(c.join());
            }
        });
        (addr, handle)
    }

    fn serve_one(
        mut s: TcpStream,
        stream: &[u8],
        hash: [u8; 20],
        count: usize,
        serve: Serve,
        stop: &AtomicBool,
    ) {
        s.set_nonblocking(false).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut theirs = [0_u8; 68];
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = 0;
        while got < 68 && Instant::now() < deadline {
            match s.read(&mut theirs[got..]) {
                Ok(0) => return,
                Ok(n) => got += n,
                Err(_) => {}
            }
        }
        s.write_all(&crate::Handshake::new(hash, *b"-TT0001-seedseedseed").encode())
            .unwrap();
        let mut bits = vec![0xFF_u8; count.div_ceil(8)];
        let spare = bits.len() * 8 - count;
        if let Some(last) = bits.last_mut() {
            *last &= 0xFF << spare;
        }
        s.write_all(&PeerMessage::Bitfield(bits).encode()).unwrap();
        let mut pending = Vec::new();
        let mut buf = [0_u8; 4096];
        while !stop.load(Ordering::Relaxed) {
            match s.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => pending.extend_from_slice(&buf[..n]),
                Err(_) => continue,
            }
            while pending.len() >= 4 {
                let len = u32::from_be_bytes(pending[..4].try_into().unwrap()) as usize;
                if pending.len() < 4 + len {
                    break;
                }
                let msg = PeerMessage::decode(&pending[4..4 + len]).unwrap();
                pending.drain(..4 + len);
                let reply = match (msg, serve) {
                    (PeerMessage::Interested, Serve::Never) => continue,
                    (PeerMessage::Interested, _) => PeerMessage::Unchoke,
                    (
                        PeerMessage::Request {
                            index,
                            begin,
                            length,
                        },
                        _,
                    ) => {
                        let at = index as usize * PIECE as usize + begin as usize;
                        let mut data = stream[at..at + length as usize].to_vec();
                        if matches!(serve, Serve::Corrupt(n) if n == index as usize) {
                            data[0] ^= 0xFF;
                        }
                        PeerMessage::Piece { index, begin, data }
                    }
                    _ => continue,
                };
                if s.write_all(&reply.encode()).is_err() {
                    return;
                }
            }
        }
    }

    /// An HTTP tracker on a loopback port that gives `peers` to every
    /// announce, and counts them, until `stop`.
    pub(crate) fn tracker_for(
        peers: Vec<SocketAddr>,
        stop: Arc<AtomicBool>,
        asked: Arc<AtomicUsize>,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut body = b"d8:intervali1800e5:peers".to_vec();
        let compact: Vec<u8> = peers
            .iter()
            .flat_map(|p| match p {
                SocketAddr::V4(v4) => [&v4.ip().octets()[..], &v4.port().to_be_bytes()].concat(),
                SocketAddr::V6(_) => Vec::new(),
            })
            .collect();
        body.extend_from_slice(format!("{}:", compact.len()).as_bytes());
        body.extend_from_slice(&compact);
        body.push(b'e');
        let handle = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok((mut s, _)) = listener.accept() {
                    s.set_nonblocking(false).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut got = Vec::new();
                    let mut buf = [0_u8; 2048];
                    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => got.extend_from_slice(&buf[..n]),
                        }
                    }
                    asked.fetch_add(1, Ordering::Relaxed);
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                    drop(s.write_all(&[head.as_bytes(), &body].concat()));
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        (format!("http://127.0.0.1:{port}/announce"), handle)
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
    use super::testnet::*;
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn plan(meta: TorrentMetainfo, dir: &Scratch) -> Plan {
        Plan {
            meta,
            save_dir: dir.0.clone(),
            peer_id: *b"-SL0001-testtesttest",
            port: 6881,
            max_peers: 8,
            wanted: Vec::new(),
        }
    }

    /// Every event until `Finished` or `Failed`, or until `limit` passes.
    fn run_to_end(session: &Session, limit: Duration) -> Vec<Event> {
        let deadline = Instant::now() + limit;
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            match session.events.recv_timeout(Duration::from_millis(100)) {
                Ok(e) => {
                    let end = matches!(e, Event::Finished | Event::Failed(_));
                    seen.push(e);
                    if end {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        seen
    }

    /// **A torrent is downloaded**: the tracker is asked, the peer connected,
    /// every piece fetched, checked and written into the right files.
    #[test]
    fn a_torrent_is_downloaded_from_a_peer_the_tracker_names() {
        let (mut meta, stream) = content();
        let stop = Arc::new(AtomicBool::new(false));
        let asked = Arc::new(AtomicUsize::new(0));
        let (peer, peer_thread) =
            seeder(&meta, Arc::clone(&stream), Serve::Honest, Arc::clone(&stop));
        let (url, tracker_thread) = tracker_for(vec![peer], Arc::clone(&stop), Arc::clone(&asked));
        meta.announce = url.clone();
        let dir = Scratch::new("whole");
        let session = Session::start(plan(meta, &dir));
        let events = run_to_end(&session, Duration::from_mins(1));
        session.stop_and_wait();
        stop.store(true, Ordering::Relaxed);
        peer_thread.join().unwrap();
        tracker_thread.join().unwrap();

        assert_eq!(events.last(), Some(&Event::Finished), "{events:?}");
        assert_eq!(events.first(), Some(&Event::Checked(vec![false; 5])));
        assert!(events.iter().any(
            |e| matches!(e, Event::Tracker { url: u, result: Ok(a) } if *u == url && a.peers == 1)
        ));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::PeerUp { addr, .. } if *addr == peer))
        );
        let mut written: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                Event::Piece { index } => Some(*index),
                _ => None,
            })
            .collect();
        written.sort_unstable();
        assert_eq!(written, vec![0, 1, 2, 3, 4]);
        let received: u64 = events
            .iter()
            .filter_map(|e| match e {
                Event::Received { bytes, .. } => Some(*bytes),
                _ => None,
            })
            .sum();
        assert_eq!(received, stream.len() as u64);
        let root = dir.0.join("Set");
        let got = [
            std::fs::read(root.join("one.bin")).unwrap(),
            std::fs::read(root.join("sub").join("two.bin")).unwrap(),
            std::fs::read(root.join("three.bin")).unwrap(),
        ]
        .concat();
        assert!(got == *stream, "the files do not hold the torrent's bytes");
        assert!(
            asked.load(Ordering::Relaxed) >= 2,
            "started and completed were not both announced"
        );
    }

    /// A piece that fails its hash is thrown away and fetched again from a
    /// peer that sends it right; nothing wrong reaches the disk.
    #[test]
    fn a_bad_piece_is_fetched_again() {
        let (mut meta, stream) = content();
        let stop = Arc::new(AtomicBool::new(false));
        let (liar, liar_thread) = seeder(
            &meta,
            Arc::clone(&stream),
            Serve::Corrupt(2),
            Arc::clone(&stop),
        );
        let (honest, honest_thread) =
            seeder(&meta, Arc::clone(&stream), Serve::Honest, Arc::clone(&stop));
        let (url, tracker_thread) = tracker_for(
            vec![liar, honest],
            Arc::clone(&stop),
            Arc::new(AtomicUsize::new(0)),
        );
        meta.announce = url;
        let dir = Scratch::new("bad");
        let session = Session::start(plan(meta, &dir));
        let events = run_to_end(&session, Duration::from_mins(1));
        session.stop_and_wait();
        stop.store(true, Ordering::Relaxed);
        liar_thread.join().unwrap();
        honest_thread.join().unwrap();
        tracker_thread.join().unwrap();

        assert_eq!(events.last(), Some(&Event::Finished), "{events:?}");
        let root = dir.0.join("Set");
        let got = [
            std::fs::read(root.join("one.bin")).unwrap(),
            std::fs::read(root.join("sub").join("two.bin")).unwrap(),
            std::fs::read(root.join("three.bin")).unwrap(),
        ]
        .concat();
        assert!(got == *stream, "a bad piece reached the disk");
        // Whether the liar was asked for piece 2 is up to the picker; when
        // it was, the bad copy is said.
        for e in &events {
            if let Event::BadPiece { index, addr } = e {
                assert_eq!((*index, *addr), (2, liar));
            }
        }
    }

    /// What is already on disk and whole is not fetched again.
    #[test]
    fn a_download_started_again_fetches_only_what_is_missing() {
        let (mut meta, stream) = content();
        let dir = Scratch::new("resume");
        let st = Storage::new(&meta, &dir.0).unwrap();
        for i in [0, 3] {
            let start = i * PIECE as usize;
            st.write_piece(i, &stream[start..start + PIECE as usize])
                .unwrap();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (peer, peer_thread) =
            seeder(&meta, Arc::clone(&stream), Serve::Honest, Arc::clone(&stop));
        let (url, tracker_thread) =
            tracker_for(vec![peer], Arc::clone(&stop), Arc::new(AtomicUsize::new(0)));
        meta.announce = url;
        let session = Session::start(plan(meta, &dir));
        let events = run_to_end(&session, Duration::from_mins(1));
        session.stop_and_wait();
        stop.store(true, Ordering::Relaxed);
        peer_thread.join().unwrap();
        tracker_thread.join().unwrap();

        assert_eq!(
            events.first(),
            Some(&Event::Checked(vec![true, false, false, true, false]))
        );
        let mut written: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                Event::Piece { index } => Some(*index),
                _ => None,
            })
            .collect();
        written.sort_unstable();
        assert_eq!(written, vec![1, 2, 4]);
        assert_eq!(events.last(), Some(&Event::Finished));
    }

    /// Stopping ends the download promptly, even with a peer that never
    /// lets it fetch anything.
    #[test]
    fn stopping_ends_the_download_promptly() {
        let (mut meta, stream) = content();
        let stop = Arc::new(AtomicBool::new(false));
        let (peer, peer_thread) = seeder(&meta, stream, Serve::Never, Arc::clone(&stop));
        let (url, tracker_thread) =
            tracker_for(vec![peer], Arc::clone(&stop), Arc::new(AtomicUsize::new(0)));
        meta.announce = url;
        let dir = Scratch::new("stop");
        let session = Session::start(plan(meta, &dir));
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(Event::PeerUp { .. }) =
                session.events.recv_timeout(Duration::from_millis(100))
            {
                break;
            }
        }
        let asked_to_stop = Instant::now();
        session.stop_and_wait();
        assert!(
            asked_to_stop.elapsed() < Duration::from_secs(5),
            "took {:?} to stop",
            asked_to_stop.elapsed()
        );
        stop.store(true, Ordering::Relaxed);
        peer_thread.join().unwrap();
        tracker_thread.join().unwrap();
    }

    /// Waiting for a stopped download to go quiet does not wait for its
    /// trackers: a tracker that takes its time answering "stopped" costs the
    /// window nothing.
    #[test]
    fn a_stopped_download_goes_quiet_before_its_trackers_are_told() {
        let (mut meta, stream) = content();
        let stop = Arc::new(AtomicBool::new(false));
        let (peer, peer_thread) = seeder(&meta, stream, Serve::Never, Arc::clone(&stop));
        // A tracker that answers the first announce and then keeps the
        // "stopped" one waiting.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/announce", listener.local_addr().unwrap());
        let slow = thread::spawn(move || {
            use std::io::{Read, Write};
            let compact = match peer {
                SocketAddr::V4(v4) => [&v4.ip().octets()[..], &v4.port().to_be_bytes()].concat(),
                SocketAddr::V6(_) => unreachable!(),
            };
            let body = [b"d8:intervali1800e5:peers6:".as_slice(), &compact, b"e"].concat();
            for answer in [true, false] {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0_u8; 4096];
                let _ = s.read(&mut buf);
                if answer {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                    s.write_all(&[head.as_bytes(), &body].concat()).unwrap();
                } else {
                    thread::sleep(Duration::from_secs(4));
                }
            }
        });
        meta.announce = url;
        let dir = Scratch::new("quiet");
        let session = Session::start(plan(meta, &dir));
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(Event::PeerUp { .. }) =
                session.events.recv_timeout(Duration::from_millis(100))
            {
                break;
            }
        }
        let asked = Instant::now();
        session.stop_until_quiet();
        assert!(
            asked.elapsed() < Duration::from_secs(2),
            "waited {:?} for the trackers",
            asked.elapsed()
        );
        session.stop_and_wait();
        stop.store(true, Ordering::Relaxed);
        peer_thread.join().unwrap();
        slow.join().unwrap();
    }

    /// A download whose files cannot be written says so and ends.
    #[test]
    fn files_that_cannot_be_written_end_the_download() {
        let (mut meta, stream) = content();
        let stop = Arc::new(AtomicBool::new(false));
        let (peer, peer_thread) = seeder(&meta, stream, Serve::Honest, Arc::clone(&stop));
        let (url, tracker_thread) =
            tracker_for(vec![peer], Arc::clone(&stop), Arc::new(AtomicUsize::new(0)));
        meta.announce = url;
        let dir = Scratch::new("blocked");
        // A file where the torrent's folder must go.
        std::fs::write(dir.0.join("Set"), b"in the way").unwrap();
        let session = Session::start(plan(meta, &dir));
        let events = run_to_end(&session, Duration::from_mins(1));
        session.stop_and_wait();
        stop.store(true, Ordering::Relaxed);
        peer_thread.join().unwrap();
        tracker_thread.join().unwrap();
        assert!(
            matches!(events.last(), Some(Event::Failed(_))),
            "{events:?}"
        );
    }

    /// A bitfield is taken only at its exact length with its spare bits
    /// clear.
    #[test]
    fn a_bitfield_must_fit_the_torrent() {
        assert_eq!(bitfield(&[0b1010_0000], 3), Some(vec![true, false, true]));
        assert_eq!(bitfield(&[0b1010_0001], 3), None, "a spare bit set");
        assert_eq!(bitfield(&[0xFF, 0x00], 3), None, "too long");
        assert_eq!(bitfield(&[], 3), None, "too short");
        assert_eq!(bitfield(&[0xFF], 8), Some(vec![true; 8]));
    }

    /// The picker hands out the rarest wanted piece the peer has, never one
    /// that is had or being fetched.
    #[test]
    fn the_picker_hands_out_the_rarest_piece_nobody_is_fetching() {
        let mut p = Picker {
            have: vec![true, false, false, false],
            busy: vec![false; 4],
            wanted: vec![true, true, true, false],
            seen: vec![0; 4],
        };
        p.count(&[true, true, true, true], true);
        p.count(&[false, true, false, false], true);
        let peer = [true, true, true, true];
        assert_eq!(p.pick(&peer), Some(2), "the rarest wanted piece not had");
        assert_eq!(p.pick(&peer), Some(1), "then the next");
        assert_eq!(
            p.pick(&peer),
            None,
            "3 is not wanted; the rest are had or busy"
        );
        p.release(1);
        assert_eq!(p.pick(&[false, true, false, false]), Some(1));
        p.done(1);
        p.done(2);
        assert!(p.complete());
    }
}
