//! Keeps dynamic-DNS hostnames pointed at this network's address, and asks
//! the router for the port forwards this computer wants.
//!
//! This file is the service's loop: the clock, the files, the connections.
//! What to do is decided in `lib.rs` and `forwards.rs`; this is doing it.
//!
//! ```text
//! dyndns [--config FILE] [--status FILE] [--forwards FILE]
//!        [--forwards-status FILE] [--log FILE] [--once]
//! ```
//!
//! The service manager (`services/init`) is to start it at boot and restart
//! it if it dies. It looks at its two configuration files every ten seconds
//! and takes a changed one at once, so an entry or a forward Settings adds is
//! acted on within seconds. `--once` checks every entry and asks for every
//! forward once, writes both status files, and exits -- 0 when every entry is
//! known to point here and every forward is made, 1 otherwise -- for running
//! it by hand.
//!
//! # The network
//!
//! Requests go out over plain TCP, through the C library's resolver and
//! sockets. A request to an `https://` URL -- every provider's update -- is
//! not sent: nothing on SlateOS speaks TLS yet, so it is refused with that
//! reason, which the status file passes on to Settings ("cannot reach the
//! provider: this system cannot make HTTPS connections yet"). Nothing is
//! ever sent in the clear in its place.
//!
//! The router is asked over UDP (NAT-PMP, and UPnP's multicast search) and
//! plain HTTP (UPnP's description and actions), on this network only. Where
//! it is -- the default gateway -- and this computer's address come from the
//! kernel's record of the network interface (`SYS_NET_IF_INFO`), as `route`
//! and `ifconfig` read them; a portable program has no call for the gateway.

use std::ffi::OsString;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dyndns::forwards::{Lan, Ports, read_forwards};
use dyndns::{
    Level, Note, Options, RouterAddress, Service, Undecided, parse_args, read_config, record,
    shown_path,
};
use dyndnsproviders::{Answer, Exchange, Method, Request};
use dyndnsrouter::{HttpAnswer, HttpRequest, Net, ssdp};

/// How often the configuration files are looked at for changes, in seconds.
const CONFIG_LOOK_SECS: u64 = 10;

/// The longest a connection may take to open, and a read or a write to wait.
const IO_TIMEOUT: Duration = Duration::from_secs(15);

/// The most of an answer that is read. A provider answers in a line or a
/// small JSON document, and a router's description is a few kilobytes;
/// anything longer is not an answer to this.
const MAX_ANSWER: usize = 256 * 1024;

/// The most answers one UPnP search gathers: a network of many devices
/// answers once each, and a flood is not read past this.
const MAX_SEARCH_ANSWERS: usize = 64;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The journal: appended to, one record a line; stderr when it cannot be.
struct Journal<'a> {
    path: &'a Path,
}

impl Journal<'_> {
    fn write(&self, level: Level, text: &str) {
        let line = record(now_secs(), level, text);
        let appended = self
            .path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.path)?;
                // One write for the whole record, so another writer's record
                // cannot land inside this one.
                f.write_all(format!("{line}\n").as_bytes())
            });
        if appended.is_err() {
            eprintln!("{line}");
        }
    }

    fn note(&self, note: &Note) {
        self.write(note.level, &note.text);
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("dyndns: {msg}");
            return ExitCode::from(2);
        }
    };
    let journal = Journal { path: &opts.log };
    journal.write(
        Level::Info,
        &format!(
            "started: the entries of {} and the forwards of {}, status in {} and {}",
            shown_path(&opts.config),
            shown_path(&opts.forwards),
            shown_path(&opts.status),
            shown_path(&opts.forwards_status)
        ),
    );
    if run(&opts, &journal) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A configuration file's bytes; a missing file is an empty one.
fn read_config_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    match std::fs::read(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        other => other,
    }
}

/// Replace a status file whole: written beside it, then renamed over it, so
/// Settings never reads half of one.
fn write_status(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".new");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// One configuration file, watched: its last bytes, and the last error
/// journalled for it, so that one that cannot be read is said once.
#[derive(Default)]
struct Watched {
    bytes: Option<Vec<u8>>,
    error: Option<String>,
}

impl Watched {
    /// The file's text if it changed since the last look; `None` if not, or
    /// if it cannot be read now (kept as it was: it may be mid-replacement).
    /// A file that is not UTF-8 is journalled and read as empty.
    fn changed(&mut self, path: &Path, journal: &Journal<'_>) -> Option<String> {
        match read_config_bytes(path) {
            Ok(bytes) if self.bytes.as_ref() != Some(&bytes) => {
                self.error = None;
                self.bytes = Some(bytes.clone());
                match String::from_utf8(bytes) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        journal.write(
                            Level::Err,
                            &format!("{} is not UTF-8 text; it is not used", shown_path(path)),
                        );
                        Some(String::new())
                    }
                }
            }
            Ok(_) => None,
            Err(e) => {
                let msg = format!("cannot read {}: {e}", shown_path(path));
                if self.error.as_deref() != Some(msg.as_str()) {
                    journal.write(Level::Err, &msg);
                    self.error = Some(msg);
                }
                None
            }
        }
    }
}

/// A status file, written when asked, with its last error journalled once.
#[derive(Default)]
struct Written {
    error: Option<String>,
}

impl Written {
    fn write(&mut self, path: &Path, text: &str, journal: &Journal<'_>) {
        match write_status(path, text) {
            Ok(()) => self.error = None,
            Err(e) => {
                let msg = format!("cannot write {}: {e}", shown_path(path));
                if self.error.as_deref() != Some(msg.as_str()) {
                    journal.write(Level::Err, &msg);
                    self.error = Some(msg);
                }
            }
        }
    }
}

/// The loop. Returns, only with `--once`, whether every entry and forward
/// ended in a good state.
fn run(opts: &Options, journal: &Journal<'_>) -> bool {
    let mut service = Service::new();
    let mut ports = Ports::new();
    let (mut entries, mut forwards) = (Watched::default(), Watched::default());
    let (mut status, mut forwards_status) = (Written::default(), Written::default());
    let mut first = true;
    let mut next_look = 0u64;
    let mut last_now = now_secs();
    let mut last_lan = None;
    let mut providers = Http;
    let mut router_net = RouterNet::default();
    loop {
        let now = now_secs();
        let mut changed = false;
        let mut forwards_changed = false;
        // A clock set back (a corrected real-time clock) would leave every
        // entry due in what is now the future: check them all instead.
        if now.saturating_add(60) < last_now {
            journal.write(
                Level::Warning,
                "the clock went back; checking every entry now",
            );
            service.all_due(now);
            next_look = now;
        }
        last_now = now;

        if now >= next_look {
            next_look = now.saturating_add(CONFIG_LOOK_SECS);
            if let Some(text) = entries.changed(&opts.config, journal) {
                let read = read_config(&text);
                let count = read.entries.len();
                service.configure(read, now);
                if first && let Ok(previous) = std::fs::read_to_string(&opts.status) {
                    service.remember(&previous);
                }
                journal.write(
                    Level::Info,
                    &format!(
                        "read {}: {count} entr{}",
                        shown_path(&opts.config),
                        if count == 1 { "y" } else { "ies" }
                    ),
                );
                for note in service.config_notes() {
                    journal.note(&note);
                }
                changed = true;
            }
            if let Some(text) = forwards.changed(&opts.forwards, journal) {
                let read = read_forwards(&text);
                let count = read.entries.len();
                ports.configure(read, now);
                journal.write(
                    Level::Info,
                    &format!(
                        "read {}: {count} forward{}",
                        shown_path(&opts.forwards),
                        if count == 1 { "" } else { "s" }
                    ),
                );
                for note in ports.config_notes() {
                    journal.note(&note);
                }
                forwards_changed = true;
            }
            first = false;
        }

        // The router first: a custom provider's entry takes its address.
        let lan = this_network();
        if lan != last_lan || ports.next_due() <= now || opts.once {
            last_lan = lan;
            for note in ports.step(&mut router_net, lan, now) {
                journal.note(&note);
            }
            forwards_changed = true;
        }
        service.set_router_address(
            match (ports.public_address(), ports.why_no_address()) {
                (Some(a), _) => RouterAddress::Known(a.into()),
                (None, why) => RouterAddress::Unknown(
                    why.unwrap_or_else(|| "the router has not said".to_owned()),
                ),
            },
            now,
        );

        if opts.once {
            service.all_due(now);
        }
        for name in service.due(now) {
            if let Some(note) = service.check(&name, now, &Undecided, &mut providers) {
                journal.note(&note);
            }
            changed = true;
        }
        if changed {
            status.write(&opts.status, &service.status_text(now_secs()), journal);
        }
        if forwards_changed {
            forwards_status.write(
                &opts.forwards_status,
                &ports.status_text(now_secs()),
                journal,
            );
        }
        if opts.once {
            return service.all_well() && ports.all_well() && entries.error.is_none();
        }
        let wake = [
            service.next_due().unwrap_or(next_look),
            ports.next_due(),
            next_look,
        ]
        .into_iter()
        .min()
        .unwrap_or(next_look);
        let secs = wake.saturating_sub(now_secs()).clamp(1, CONFIG_LOOK_SECS);
        std::thread::sleep(Duration::from_secs(secs));
    }
}

// ---------------------------------------------------------------------------
// This network
// ---------------------------------------------------------------------------

/// The kernel's read-only record of the network interface
/// (`kernel/src/syscall/number.rs`).
const SYS_NET_IF_INFO: u64 = 842;

/// Its size: `[0..4]` address, `[4..8]` mask, `[8..12]` gateway, `[12..16]`
/// DNS server -- each in network order -- `[16..22]` hardware address, `[22]`
/// bit 0 up.
const NET_IF_INFO_SIZE: usize = 24;

/// This computer's address and its default gateway, when it has both and its
/// interface is up.
fn this_network() -> Option<Lan> {
    let mut rec = [0u8; NET_IF_INFO_SIZE];
    // SAFETY: `rec` is the record's size, which is all the kernel writes.
    let ret = unsafe { syscall2(SYS_NET_IF_INFO, rec.as_mut_ptr() as u64, rec.len() as u64) };
    if ret < 0 {
        return None;
    }
    read_net_if_info(&rec)
}

/// The network a `SYS_NET_IF_INFO` record describes.
fn read_net_if_info(rec: &[u8; NET_IF_INFO_SIZE]) -> Option<Lan> {
    let [a, b, c, d, _, _, _, _, g0, g1, g2, g3, ..] = *rec;
    let up = rec.get(22).is_some_and(|f| f & 1 != 0);
    let here = Ipv4Addr::new(a, b, c, d);
    let gateway = Ipv4Addr::new(g0, g1, g2, g3);
    (up && !here.is_unspecified() && !gateway.is_unspecified()).then_some(Lan { here, gateway })
}

#[cfg(target_vendor = "slateos")]
unsafe fn syscall2(nr: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    // SAFETY: the caller's arguments are what the call takes; the kernel
    // clobbers rcx and r11 and nothing else.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// On a development host there is no SlateOS kernel, and a raw `syscall`
/// would be another kernel's call: no network, as far as this knows.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall2(_nr: u64, _a1: u64, _a2: u64) -> i64 {
    -38 // ENOSYS
}

// ---------------------------------------------------------------------------
// The providers, over plain TCP
// ---------------------------------------------------------------------------

/// The network, over plain TCP, for the providers.
struct Http;

impl Exchange for Http {
    fn exchange(&mut self, request: &Request) -> Result<Answer, String> {
        let method = match request.method {
            Method::Get => httpclient::Method::Get,
            Method::Patch => httpclient::Method::Patch,
        };
        let (status, body) = http_call(method, &request.url, &request.headers, &request.body)?;
        Ok(Answer { status, body })
    }
}

/// Make one HTTP request and read its whole answer: the status and the body.
fn http_call(
    method: httpclient::Method,
    url: &str,
    extra: &[(&'static str, String)],
    body: &[u8],
) -> Result<(u16, Vec<u8>), String> {
    let url = httpclient::Url::parse(url).map_err(|e| format!("{e}"))?;
    if url.scheme == "https" {
        return Err(
            "this system cannot make HTTPS connections yet (there is no TLS on SlateOS)".to_owned(),
        );
    }
    let mut headers = httpclient::Headers::new();
    for (name, value) in extra {
        headers.set(name, value);
    }
    headers.set("Accept", "*/*");
    // One request a connection, read to its end: no keep-alive to manage.
    headers.set("Connection", "close");
    let wire = httpclient::Request {
        method,
        url: url.clone(),
        headers,
        body: (!body.is_empty()).then(|| body.to_vec()),
        timeout_ms: 15_000,
        follow_redirects: false,
        max_redirects: 0,
    };
    let bytes = roundtrip(&url, &wire.serialize())?;
    let response = httpclient::parse_response(&bytes, &url)
        .map_err(|e| format!("{} answered something that is not HTTP: {e}", url.host))?;
    Ok((response.status, response.body))
}

/// Send `bytes` to the URL's host and read the whole answer.
fn roundtrip(url: &httpclient::Url, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let addrs: Vec<_> = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot find {}: {e}", url.host))?
        .collect();
    let mut last = format!("{} has no address", url.host);
    for addr in addrs {
        let mut stream = match TcpStream::connect_timeout(&addr, IO_TIMEOUT) {
            Ok(s) => s,
            Err(e) => {
                last = format!("cannot connect to {} ({addr}): {e}", url.host);
                continue;
            }
        };
        let io = |e: std::io::Error| format!("talking to {}: {e}", url.host);
        stream.set_read_timeout(Some(IO_TIMEOUT)).map_err(io)?;
        stream.set_write_timeout(Some(IO_TIMEOUT)).map_err(io)?;
        stream.write_all(bytes).map_err(io)?;
        let mut out = Vec::new();
        let limit = u64::try_from(MAX_ANSWER)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        (&mut stream)
            .take(limit)
            .read_to_end(&mut out)
            .map_err(io)?;
        if out.len() > MAX_ANSWER {
            return Err(format!(
                "{} answered with more than {MAX_ANSWER} bytes",
                url.host
            ));
        }
        return Ok(out);
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// The router, over UDP and plain HTTP
// ---------------------------------------------------------------------------

/// The network, for the router.
#[derive(Default)]
struct RouterNet {
    /// The socket NAT-PMP's requests to one router go out on, kept across the
    /// sends of a request: an answer to an earlier send that arrives late is
    /// still an answer, and RFC 6886 has the client listen for it.
    natpmp: Option<(SocketAddrV4, UdpSocket)>,
}

impl RouterNet {
    /// The socket for `to`, opened and connected the first time: connected,
    /// so only the router's datagrams are received, and its "port
    /// unreachable" is an error rather than silence.
    fn natpmp_socket(&mut self, to: SocketAddrV4) -> std::io::Result<&UdpSocket> {
        if self.natpmp.as_ref().is_none_or(|(at, _)| *at != to) {
            let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
            socket.connect(to)?;
            self.natpmp = Some((to, socket));
        }
        match &self.natpmp {
            Some((_, socket)) => Ok(socket),
            None => Err(std::io::Error::other("no socket")),
        }
    }
}

impl Net for RouterNet {
    fn udp_ask(
        &mut self,
        to: SocketAddrV4,
        request: &[u8],
        wait: Duration,
    ) -> Result<Option<Vec<u8>>, String> {
        let socket = self
            .natpmp_socket(to)
            .map_err(|e| format!("cannot open a socket to {to}: {e}"))?;
        let refused = |e: &std::io::Error| e.kind() == ErrorKind::ConnectionRefused;
        let result = (|| {
            socket.send(request)?;
            socket.set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
            let mut buf = [0u8; 1100];
            match socket.recv(&mut buf) {
                Ok(n) => Ok(Some(buf.get(..n).unwrap_or_default().to_vec())),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    Ok(None)
                }
                Err(e) => Err(e),
            }
        })();
        result.map_err(|e| {
            // Whatever went wrong, the next request starts on a new socket.
            self.natpmp = None;
            if refused(&e) {
                format!("nothing answers on {to}: the router refused")
            } else {
                format!("talking to {to}: {e}")
            }
        })
    }

    fn ssdp_search(
        &mut self,
        searches: &[Vec<u8>],
        listen: Duration,
    ) -> Result<Vec<(Ipv4Addr, Vec<u8>)>, String> {
        let io = |e: std::io::Error| format!("cannot search for UPnP devices: {e}");
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).map_err(io)?;
        // The architecture asks for a TTL of 2; a stack that cannot set it
        // sends with its own, which reaches the router all the same.
        if socket.set_multicast_ttl_v4(2).is_err() {
            // Not fatal: see above.
        }
        for search in searches {
            socket.send_to(search, ssdp::GROUP).map_err(io)?;
        }
        let started = Instant::now();
        let mut answers = Vec::new();
        let mut buf = [0u8; 2048];
        while answers.len() < MAX_SEARCH_ANSWERS {
            let left = listen.saturating_sub(started.elapsed());
            if left.is_zero() {
                break;
            }
            socket.set_read_timeout(Some(left)).map_err(io)?;
            match socket.recv_from(&mut buf) {
                Ok((n, SocketAddr::V4(from))) => {
                    answers.push((*from.ip(), buf.get(..n).unwrap_or_default().to_vec()));
                }
                // An answer from an IPv6 address is not the router's.
                Ok((_, SocketAddr::V6(_))) => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    break;
                }
                Err(e) => return Err(io(e)),
            }
        }
        Ok(answers)
    }

    fn http(&mut self, request: &HttpRequest) -> Result<HttpAnswer, String> {
        let method = match request.method {
            "POST" => httpclient::Method::Post,
            _ => httpclient::Method::Get,
        };
        let (status, body) = http_call(method, &request.url, &request.headers, &request.body)?;
        Ok(HttpAnswer { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interface_record_is_read_as_route_reads_it() {
        let mut rec = [0u8; NET_IF_INFO_SIZE];
        rec[..12].copy_from_slice(&[10, 0, 2, 15, 255, 255, 255, 0, 10, 0, 2, 2]);
        rec[22] = 1;
        assert_eq!(
            read_net_if_info(&rec),
            Some(Lan {
                here: Ipv4Addr::new(10, 0, 2, 15),
                gateway: Ipv4Addr::new(10, 0, 2, 2),
            })
        );
        // Down, or with no address or no gateway, there is no network to ask.
        let mut down = rec;
        down[22] = 0;
        assert_eq!(read_net_if_info(&down), None);
        let mut no_gateway = rec;
        no_gateway[8..12].fill(0);
        assert_eq!(read_net_if_info(&no_gateway), None);
        let mut no_address = rec;
        no_address[..4].fill(0);
        assert_eq!(read_net_if_info(&no_address), None);
    }

    #[test]
    fn the_host_has_no_network_to_ask() {
        assert_eq!(this_network(), None);
    }
}
