//! Keeps dynamic-DNS hostnames pointed at this network's address.
//!
//! This file is the service's loop: the clock, the files, the connections.
//! What to do is decided in `lib.rs`; this is doing it.
//!
//! ```text
//! dyndns [--config FILE] [--status FILE] [--log FILE] [--once]
//! ```
//!
//! The service manager (`services/init`) is to start it at boot and restart
//! it if it dies. It looks at its configuration every ten seconds and takes
//! a changed one at once, so an entry Settings adds is checked within
//! seconds. `--once` checks every entry once, writes the status file, and
//! exits -- 0 when every entry is known to point here, 1 otherwise -- for
//! running it by hand.
//!
//! # The network
//!
//! Requests go out over plain TCP, through the C library's resolver and
//! sockets. A request to an `https://` URL -- every provider's update -- is
//! not sent: nothing on SlateOS speaks TLS yet, so it is refused with that
//! reason, which the status file passes on to Settings ("cannot reach the
//! provider: this system cannot make HTTPS connections yet"). Nothing is
//! ever sent in the clear in its place.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dyndns::{
    Level, Note, Options, Service, Undecided, parse_args, read_config, record, shown_path,
};
use dyndnsproviders::{Answer, Exchange, Method, Request};

/// How often the configuration file is looked at for changes, in seconds.
const CONFIG_LOOK_SECS: u64 = 10;

/// The longest a connection may take to open, and a read or a write to wait.
const IO_TIMEOUT: Duration = Duration::from_secs(15);

/// The most of an answer that is read. A provider answers in a line or a
/// small JSON document; anything longer is not an answer to this.
const MAX_ANSWER: usize = 256 * 1024;

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
            "started: the entries of {}, status in {}",
            shown_path(&opts.config),
            shown_path(&opts.status)
        ),
    );
    if run(&opts, &journal) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The configuration file's bytes; a missing file is an empty one.
fn read_config_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        other => other,
    }
}

/// Replace the status file whole: written beside it, then renamed over it,
/// so Settings never reads half of one.
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

/// The loop. Returns, only with `--once`, whether every entry ended in a
/// good state.
fn run(opts: &Options, journal: &Journal<'_>) -> bool {
    let mut service = Service::new();
    let mut config: Option<Vec<u8>> = None;
    let mut config_error: Option<String> = None;
    let mut status_error: Option<String> = None;
    let mut next_look = 0u64;
    let mut last_now = now_secs();
    let mut net = Http;
    loop {
        let now = now_secs();
        let mut changed = false;
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
            match read_config_bytes(&opts.config) {
                Ok(bytes) if config.as_ref() != Some(&bytes) => {
                    let first = config.is_none();
                    match String::from_utf8(bytes.clone()) {
                        Ok(text) => {
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
                        }
                        Err(_) => {
                            journal.write(
                                Level::Err,
                                &format!(
                                    "{} is not UTF-8 text; its entries are not used",
                                    shown_path(&opts.config)
                                ),
                            );
                            service.configure(dyndns::Config::default(), now);
                        }
                    }
                    config = Some(bytes);
                    config_error = None;
                    changed = true;
                }
                Ok(_) => {}
                Err(e) => {
                    // Kept as it was: a file that cannot be read now may be
                    // mid-replacement. Journalled once, not every ten seconds.
                    let msg = format!("cannot read {}: {e}", shown_path(&opts.config));
                    if config_error.as_deref() != Some(msg.as_str()) {
                        journal.write(Level::Err, &msg);
                        config_error = Some(msg);
                    }
                }
            }
        }

        if opts.once {
            service.all_due(now);
        }
        for name in service.due(now) {
            if let Some(note) = service.check(&name, now, &Undecided, &mut net) {
                journal.note(&note);
            }
            changed = true;
        }
        if changed {
            match write_status(&opts.status, &service.status_text(now_secs())) {
                Ok(()) => status_error = None,
                Err(e) => {
                    let msg = format!("cannot write {}: {e}", shown_path(&opts.status));
                    if status_error.as_deref() != Some(msg.as_str()) {
                        journal.write(Level::Err, &msg);
                        status_error = Some(msg);
                    }
                }
            }
        }
        if opts.once {
            return service.all_well() && config_error.is_none();
        }
        let wake = service.next_due().map_or(next_look, |d| d.min(next_look));
        let secs = wake.saturating_sub(now_secs()).clamp(1, CONFIG_LOOK_SECS);
        std::thread::sleep(Duration::from_secs(secs));
    }
}

/// The network, over plain TCP.
struct Http;

impl Exchange for Http {
    fn exchange(&mut self, request: &Request) -> Result<Answer, String> {
        let url = httpclient::Url::parse(&request.url).map_err(|e| format!("{e}"))?;
        if url.scheme == "https" {
            return Err(
                "this system cannot make HTTPS connections yet (there is no TLS on SlateOS)"
                    .to_owned(),
            );
        }
        let mut headers = httpclient::Headers::new();
        for (name, value) in &request.headers {
            headers.set(name, value);
        }
        headers.set("Accept", "*/*");
        // One request a connection, read to its end: no keep-alive to manage.
        headers.set("Connection", "close");
        let wire = httpclient::Request {
            method: match request.method {
                Method::Get => httpclient::Method::Get,
                Method::Patch => httpclient::Method::Patch,
            },
            url: url.clone(),
            headers,
            body: (!request.body.is_empty()).then(|| request.body.clone()),
            timeout_ms: 15_000,
            follow_redirects: false,
            max_redirects: 0,
        };
        let bytes = roundtrip(&url, &wire.serialize())?;
        let response = httpclient::parse_response(&bytes, &url)
            .map_err(|e| format!("{} answered something that is not HTTP: {e}", url.host))?;
        Ok(Answer {
            status: response.status,
            body: response.body,
        })
    }
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
