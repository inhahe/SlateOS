//! The scan itself: a TCP connection to every address and port asked for,
//! made on worker threads, each result sent back to the window as it comes.
//!
//! # What a connection can tell, and what it cannot
//!
//! An application can open TCP connections and nothing lower: no ICMP (ping,
//! traceroute), no raw frames (ARP, SYN scanning). So this is a *connect*
//! scan, and every verdict is one a completed or refused connection supports:
//!
//! - **Open** -- something accepted the connection.
//! - **Closed** -- the host answered, and refused: it is there, and nothing
//!   listens on that port. This is also how a host is found at all: a host
//!   is up if any port answered either way.
//! - **Silent** -- no answer within the timeout, or the network said the host
//!   cannot be reached. That is *not* "closed": a firewall that drops
//!   packets, a host that is off and an address nobody holds all look alike
//!   from here, and the window says so rather than guessing which.
//!
//! # Threads
//!
//! [`Running::start`] spawns `workers` threads that take probes from a shared
//! counter until it runs out or the scan is cancelled. Each result goes down a
//! channel, and the window's waker is woken -- at most every [`WAKE_EVERY`],
//! so a scan answering thousands of probes a second does not ask for
//! thousands of frames. When the last worker ends, the channel closes, and
//! that is what "finished" means: no count of expected reports to get wrong.

use std::io::Read;
use std::net::{SocketAddr, SocketAddrV4, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::task::Waker;
use std::time::{Duration, Instant};

/// What one connection attempt found.
#[derive(Clone, Debug, PartialEq)]
pub enum Probe {
    /// Something accepted the connection, after `ms` milliseconds; what it
    /// said first, if it is a service that speaks first.
    Open { ms: f32, banner: Option<String> },
    /// The host answered and refused, after `ms` milliseconds.
    Closed { ms: f32 },
    /// No answer, or the host cannot be reached.
    Silent,
}

/// One connection attempt. [`tcp`] is the real one; a test's stands in.
pub type Prober = fn(SocketAddrV4, Duration) -> Probe;

/// Ports whose services speak first -- a greeting line before the client
/// says anything -- so a banner can be read without sending a byte.
const SPEAKS_FIRST: &[u16] = &[21, 22, 23, 25, 110, 143, 587, 3306, 5900];

/// How long an open port on [`SPEAKS_FIRST`] is given to greet.
const BANNER_WAIT: Duration = Duration::from_millis(400);

/// The most of a banner kept: its first line, cut here.
const BANNER_MAX: usize = 120;

/// The window is woken at most this often while results arrive.
pub const WAKE_EVERY: Duration = Duration::from_millis(40);

/// Connect to `addr` within `timeout`, and read a greeting if the port is
/// one whose service gives one.
#[must_use]
pub fn tcp(addr: SocketAddrV4, timeout: Duration) -> Probe {
    let started = Instant::now();
    match TcpStream::connect_timeout(&SocketAddr::V4(addr), timeout) {
        Ok(stream) => {
            let ms = millis(started.elapsed());
            let banner = if SPEAKS_FIRST.contains(&addr.port()) {
                read_banner(stream)
            } else {
                None
            };
            Probe::Open { ms, banner }
        }
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => Probe::Closed {
            ms: millis(started.elapsed()),
        },
        // Timed out, host or network unreachable, or anything else: no
        // answer that says what is there.
        Err(_) => Probe::Silent,
    }
}

/// A prober that connects to nothing and finds nothing: what a test build of
/// the window uses unless a test chooses another, so that no test -- one that
/// forgets to choose, above all -- scans the network of whoever runs it.
#[cfg(test)]
#[must_use]
pub fn no_network(_: SocketAddrV4, _: Duration) -> Probe {
    Probe::Silent
}

/// A duration in milliseconds, as the result table shows it.
fn millis(d: Duration) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let ms = (d.as_secs_f64() * 1000.0) as f32;
    ms
}

/// The first line a service sends on its own, printable ASCII only.
///
/// A banner is a stranger's bytes going onto the screen: anything that is not
/// a printable character is dropped rather than drawn, and it is cut short.
fn read_banner(mut stream: TcpStream) -> Option<String> {
    // Without the timeout a service that waits for the client -- or one that
    // accepts and says nothing -- would hold this worker forever. If it cannot
    // be set, no banner is read at all rather than risking that.
    stream.set_read_timeout(Some(BANNER_WAIT)).ok()?;
    let mut buf = [0_u8; 256];
    let n = stream.read(&mut buf).ok()?;
    banner_text(buf.get(..n)?)
}

/// What of a greeting is shown: its first line, printable ASCII, trimmed and
/// cut to [`BANNER_MAX`] characters. `None` when nothing printable is left.
#[must_use]
pub fn banner_text(bytes: &[u8]) -> Option<String> {
    let line = bytes
        .split(|&b| b == b'\n' || b == b'\r')
        .find(|l| !l.iter().all(u8::is_ascii_whitespace))?;
    let text: String = line
        .iter()
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .take(BANNER_MAX)
        .map(|&b| char::from(b))
        .collect();
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// What to scan, and how.
#[derive(Clone, Debug)]
pub struct Job {
    /// The addresses, in the order they are tried.
    pub hosts: Vec<[u8; 4]>,
    /// The ports, in the order they are tried.
    pub ports: Vec<u16>,
    /// How long each connection is given.
    pub timeout: Duration,
    /// How many connections are open at once.
    pub workers: usize,
    /// How long each worker waits between one probe and the next: none for
    /// an ordinary scan, some for a gentle one.
    pub pause: Duration,
}

impl Job {
    /// How many connections the job makes.
    #[must_use]
    pub fn probes(&self) -> usize {
        self.hosts.len().saturating_mul(self.ports.len())
    }

    /// Which address and port probe `index` is: every address on the first
    /// port, then every address on the next -- so a scan spreads over the
    /// hosts rather than hammering one host's ports in a row.
    #[must_use]
    pub fn probe_at(&self, index: usize) -> Option<([u8; 4], u16)> {
        let hosts = self.hosts.len();
        if hosts == 0 {
            return None;
        }
        let host = *self.hosts.get(index.checked_rem(hosts)?)?;
        let port = *self.ports.get(index.checked_div(hosts)?)?;
        Some((host, port))
    }
}

/// One probe's result, as the window receives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub host: [u8; 4],
    pub port: u16,
    pub probe: Probe,
}

/// A scan in progress. Dropping it cancels the scan.
pub struct Running {
    shared: Arc<Shared>,
    reports: mpsc::Receiver<Report>,
}

/// What the workers share: the job, where it has got to, and whether to stop.
struct Shared {
    job: Job,
    /// The next probe to make, as an index into the job ([`Job::probe_at`]).
    next: AtomicUsize,
    cancel: AtomicBool,
    /// When the window was last woken, in milliseconds since `epoch`.
    last_wake: AtomicU64,
    epoch: Instant,
}

impl Running {
    /// Start `job` on its worker threads, probing with `prober`, waking
    /// `waker` as results arrive.
    ///
    /// A worker that cannot be spawned is simply one fewer; if none can, the
    /// scan is over at once -- `drain` reports it finished with nothing found,
    /// and the window says how many probes were made.
    #[must_use]
    pub fn start(job: Job, prober: Prober, waker: Option<Waker>) -> Self {
        let (tx, reports) = mpsc::channel();
        let workers = job.workers.clamp(1, job.probes().max(1));
        let shared = Arc::new(Shared {
            job,
            next: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
            last_wake: AtomicU64::new(0),
            epoch: Instant::now(),
        });
        for _ in 0..workers {
            let (shared, tx, waker) = (Arc::clone(&shared), tx.clone(), waker.clone());
            let spawned = std::thread::Builder::new()
                .name(String::from("netscan-probe"))
                .spawn(move || work(&shared, &tx, prober, waker.as_ref()));
            // A worker that would not start is one fewer: the others take its
            // share, and the scan still ends when they do. One that did start
            // is not joined -- it ends on its own -- so its handle is let go,
            // which detaches it.
            let _detached = spawned.ok();
        }
        // The workers hold the only senders now, so the channel closes when
        // the last of them ends -- which is what "finished" is.
        drop(tx);
        Self { shared, reports }
    }

    /// Stop starting new probes. Those already connecting finish, and their
    /// results still arrive.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether the scan was cancelled.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.shared.cancel.load(Ordering::Relaxed)
    }

    /// Every result that has arrived since the last call, and whether the
    /// scan has finished -- every worker gone, so nothing more will come.
    #[must_use]
    pub fn drain(&self) -> (Vec<Report>, bool) {
        let mut got = Vec::new();
        loop {
            match self.reports.try_recv() {
                Ok(report) => got.push(report),
                Err(mpsc::TryRecvError::Empty) => return (got, false),
                Err(mpsc::TryRecvError::Disconnected) => return (got, true),
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// One worker: probe until the job runs out or the scan is cancelled.
fn work(shared: &Shared, tx: &mpsc::Sender<Report>, prober: Prober, waker: Option<&Waker>) {
    let job = &shared.job;
    loop {
        if shared.cancel.load(Ordering::Relaxed) {
            break;
        }
        let index = shared.next.fetch_add(1, Ordering::Relaxed);
        let Some((host, port)) = job.probe_at(index) else {
            break;
        };
        let [a, b, c, d] = host;
        let probe = prober(
            SocketAddrV4::new(std::net::Ipv4Addr::new(a, b, c, d), port),
            job.timeout,
        );
        if tx.send(Report { host, port, probe }).is_err() {
            // The window has gone: nobody to report to.
            break;
        }
        if let Some(waker) = waker {
            wake_now_and_then(waker, &shared.last_wake, shared.epoch);
        }
        if !job.pause.is_zero() {
            std::thread::sleep(job.pause);
        }
    }
    if let Some(waker) = waker {
        // The last report of each worker always wakes, so the window sees the
        // end of the scan even if it came inside the throttle.
        waker.wake_by_ref();
    }
}

/// Wake the window unless it was woken within [`WAKE_EVERY`].
fn wake_now_and_then(waker: &Waker, last_wake: &AtomicU64, epoch: Instant) {
    let now = u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
    let then = last_wake.load(Ordering::Relaxed);
    let every = u64::try_from(WAKE_EVERY.as_millis()).unwrap_or(u64::MAX);
    if now.saturating_sub(then) >= every
        && last_wake
            .compare_exchange(then, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        waker.wake_by_ref();
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use std::net::TcpListener;

    /// Wait for a scan to finish, collecting everything it reports.
    fn finish(scan: &Running) -> Vec<Report> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut all = Vec::new();
        loop {
            let (got, done) = scan.drain();
            all.extend(got);
            if done {
                return all;
            }
            assert!(Instant::now() < deadline, "the scan never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A port a test knows nothing listens on: bound, then let go.
    fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    /// A real connection to a listening port is Open; to a port nothing
    /// listens on, Closed -- this host answered, and refused.
    #[test]
    fn a_listening_port_is_open_and_a_quiet_one_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let open = listener.local_addr().unwrap().port();
        let closed = closed_port();
        let at = |port| SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, port);
        assert!(matches!(
            tcp(at(open), Duration::from_secs(2)),
            Probe::Open { banner: None, .. }
        ));
        // Five seconds, not two: Windows retries a refused loopback connection
        // for about two seconds before it reports the refusal, where Linux and
        // SlateOS report the peer's reset at once.
        assert!(matches!(
            tcp(at(closed), Duration::from_secs(5)),
            Probe::Closed { .. }
        ));
    }

    /// A service that speaks first has its greeting read, as a line of
    /// printable text.
    #[test]
    fn a_greeting_is_read_from_a_port_that_speaks_first() {
        // 5900 (VNC) speaks first; a listener on any port stands in for it
        // through `read_banner`, which does not look at the port.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            use std::io::Write;
            let (mut conn, _) = listener.accept().unwrap();
            conn.write_all(b"RFB 003.008\n\x1b[31mjunk").unwrap();
        });
        let stream = TcpStream::connect(addr).unwrap();
        assert_eq!(read_banner(stream).as_deref(), Some("RFB 003.008"));
        server.join().unwrap();
    }

    /// A banner keeps printable ASCII only, its first non-empty line, cut short.
    #[test]
    fn a_banner_is_one_line_of_printable_text() {
        assert_eq!(
            banner_text(b"\r\nSSH-2.0-OpenSSH_9.6\r\nmore").as_deref(),
            Some("SSH-2.0-OpenSSH_9.6")
        );
        assert_eq!(
            banner_text(b"220 ok\x07\x1b[2J done").as_deref(),
            Some("220 ok[2J done")
        );
        assert_eq!(banner_text(b"\x00\x01\x02"), None);
        assert_eq!(banner_text(b""), None);
        assert_eq!(banner_text(&[b'x'; 500]).map(|s| s.len()), Some(BANNER_MAX));
    }

    /// Every address is tried on a port before the next port.
    #[test]
    fn probes_go_across_the_hosts_before_the_next_port() {
        let job = Job {
            hosts: vec![[10, 0, 0, 1], [10, 0, 0, 2]],
            ports: vec![22, 80],
            timeout: Duration::from_millis(10),
            workers: 1,
            pause: Duration::ZERO,
        };
        assert_eq!(job.probes(), 4);
        let order: Vec<_> = (0..5).map(|i| job.probe_at(i)).collect();
        assert_eq!(
            order,
            [
                Some(([10, 0, 0, 1], 22)),
                Some(([10, 0, 0, 2], 22)),
                Some(([10, 0, 0, 1], 80)),
                Some(([10, 0, 0, 2], 80)),
                None
            ]
        );
    }

    fn open_on_80(addr: SocketAddrV4, _: Duration) -> Probe {
        if addr.port() == 80 {
            Probe::Open {
                ms: 1.0,
                banner: None,
            }
        } else {
            Probe::Silent
        }
    }

    /// Every probe of a job is made once and reported once, whatever the
    /// number of workers, and the scan then says it has finished.
    #[test]
    fn every_probe_is_reported_once_and_the_scan_ends() {
        for workers in [1, 3, 64] {
            let job = Job {
                hosts: (1..=20).map(|d| [10, 0, 0, d]).collect(),
                ports: vec![22, 80, 443],
                timeout: Duration::from_millis(10),
                workers,
                pause: Duration::ZERO,
            };
            let scan = Running::start(job, open_on_80, None);
            let mut got = finish(&scan);
            assert_eq!(got.len(), 60, "{workers} workers");
            got.sort_by_key(|r| (r.host, r.port));
            got.dedup_by_key(|r| (r.host, r.port));
            assert_eq!(got.len(), 60, "a probe was reported twice");
            assert_eq!(
                got.iter()
                    .filter(|r| matches!(r.probe, Probe::Open { .. }))
                    .count(),
                20
            );
        }
    }

    fn slow(_: SocketAddrV4, _: Duration) -> Probe {
        std::thread::sleep(Duration::from_millis(20));
        Probe::Silent
    }

    /// A cancelled scan stops starting probes: it ends long before its job
    /// would have, having reported only what was already under way.
    #[test]
    fn a_cancelled_scan_stops_early() {
        let job = Job {
            hosts: (1..=250).map(|d| [10, 0, 0, d]).collect(),
            ports: vec![1, 2, 3, 4],
            timeout: Duration::from_millis(10),
            workers: 2,
            pause: Duration::ZERO,
        };
        let scan = Running::start(job, slow, None);
        std::thread::sleep(Duration::from_millis(60));
        scan.cancel();
        assert!(scan.cancelled());
        let got = finish(&scan);
        assert!(got.len() < 50, "{} probes after cancelling", got.len());
    }

    /// An empty job ends at once, reporting nothing.
    #[test]
    fn an_empty_job_ends_at_once() {
        let job = Job {
            hosts: Vec::new(),
            ports: vec![80],
            timeout: Duration::from_millis(10),
            workers: 8,
            pause: Duration::ZERO,
        };
        let scan = Running::start(job, open_on_80, None);
        assert!(finish(&scan).is_empty());
    }
}
