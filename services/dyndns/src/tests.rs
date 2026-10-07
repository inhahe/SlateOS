//! The service's decisions, against a replayed network.

// A test states what it expects by failing loudly when it is not so.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use dyndnsproviders::{Answer, Method, Request};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A network that answers from a script, in order, and fails the test on any
/// request the script does not expect.
struct Net {
    script: Vec<(Method, String, Result<Answer, String>)>,
    sent: Vec<Request>,
}

impl Net {
    fn new() -> Self {
        Self {
            script: Vec::new(),
            sent: Vec::new(),
        }
    }

    fn then(mut self, url: &str, status: u16, body: &str) -> Self {
        self.script.push((
            Method::Get,
            url.to_owned(),
            Ok(Answer {
                status,
                body: body.as_bytes().to_vec(),
            }),
        ));
        self
    }

    fn then_fail(mut self, url: &str, why: &str) -> Self {
        self.script
            .push((Method::Get, url.to_owned(), Err(why.to_owned())));
        self
    }

    fn finished(&self) {
        assert!(self.script.is_empty(), "never sent: {:?}", self.script);
    }
}

impl Exchange for Net {
    fn exchange(&mut self, request: &Request) -> Result<Answer, String> {
        self.sent.push(request.clone());
        assert!(
            !self.script.is_empty(),
            "unexpected request: {}",
            request.url
        );
        let (method, url, answer) = self.script.remove(0);
        assert_eq!(
            (request.method, request.url.as_str()),
            (method, url.as_str())
        );
        answer
    }
}

/// Secrets kept in a table, for the tests.
#[derive(Default)]
struct Kept(HashMap<String, String>);

impl Kept {
    fn with(name: &str, secret: &str) -> Self {
        let mut k = Self::default();
        k.0.insert(name.to_owned(), secret.to_owned());
        k
    }
}

impl Secrets for Kept {
    fn secret(&self, name: &str) -> Result<String, String> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| format!("no secret {name:?}"))
    }
}

const NOW: u64 = 1_791_244_800; // 2026-10-06T00:00:00Z
const MINUTE: u64 = 60;

const DUCK: &str = "\
# Dynamic DNS: keep these hostnames pointing at this network's address.
entries:
  Home:
    provider: duckdns        # dynu, noip, duckdns, cloudflare, freedns, custom
    hostname: myhome.duckdns.org
    username: \"\"
    secret: dyndns/home      # a name in the credential store, never the secret
    update_url: \"\"
    every_minutes: 30
    enabled: true
";

const NOIP: &str = "\
entries:
  Cabin:
    provider: noip
    hostname: cabin.ddns.net
    username: me@example.com
    secret: dyndns/cabin
    every_minutes: 10
";

const DUCK_URL: &str = "https://www.duckdns.org/update?domains=myhome&token=tok&verbose=true";
const NOIP_CHECK: &str = "http://ip1.dynupdate.no-ip.com/";
const NOIP_UPDATE: &str =
    "https://dynupdate.no-ip.com/nic/update?hostname=cabin.ddns.net&myip=203.0.113.7";

fn service(text: &str) -> Service {
    let mut s = Service::new();
    s.configure(read_config(text), NOW);
    s
}

// ---------------------------------------------------------------------------
// The configuration
// ---------------------------------------------------------------------------

#[test]
fn the_requests_example_reads_as_written() {
    let c = read_config(DUCK);
    assert_eq!(c.problem, None);
    assert_eq!(
        c.entries,
        vec![Ok(Entry {
            name: "Home".to_owned(),
            provider: Provider::DuckDns,
            hostname: "myhome.duckdns.org".to_owned(),
            username: String::new(),
            secret: "dyndns/home".to_owned(),
            update_url: String::new(),
            every_minutes: 30,
            enabled: true,
        })]
    );
}

#[test]
fn what_is_left_out_takes_its_default_and_intervals_are_bounded() {
    let c = read_config(NOIP);
    let Some(Ok(e)) = c.entries.first() else {
        panic!("{c:?}")
    };
    assert_eq!(
        (e.every_minutes, e.enabled, e.update_url.as_str()),
        (10, true, "")
    );

    for (every, want) in [
        ("1", MIN_EVERY_MINUTES),
        ("0", MIN_EVERY_MINUTES),
        ("99999999999", MAX_EVERY_MINUTES),
        ("45", 45),
    ] {
        let text = format!(
            "entries:\n  A:\n    provider: duckdns\n    hostname: a\n    secret: s\n    every_minutes: {every}\n"
        );
        let c = read_config(&text);
        assert!(
            matches!(c.entries.first(), Some(Ok(e)) if e.every_minutes == want),
            "{every}: {c:?}"
        );
    }
    let c = read_config("entries:\n  A:\n    provider: duckdns\n    hostname: a\n    secret: s\n");
    assert!(matches!(c.entries.first(), Some(Ok(e)) if e.every_minutes == DEFAULT_EVERY_MINUTES));
}

#[test]
fn an_entry_that_cannot_be_used_says_why() {
    let cases = [
        ("    hostname: a\n    secret: s\n", "names no provider"),
        (
            "    provider: dyndns\n    hostname: a\n    secret: s\n",
            "is not one of",
        ),
        ("    provider: duckdns\n    secret: s\n", "no hostname"),
        (
            "    provider: noip\n    hostname: a.ddns.net\n    secret: s\n",
            "needs a username",
        ),
        (
            "    provider: duckdns\n    hostname: a\n",
            "names no secret",
        ),
        ("    provider: custom\n    hostname: a\n", "needs the URL"),
        (
            "    provider: duckdns\n    hostname: a\n    secret: s\n    every_minutes: soon\n",
            "whole number",
        ),
        (
            "    provider: duckdns\n    hostname: a\n    secret: s\n    enabled: yes\n",
            "neither true nor false",
        ),
    ];
    for (body, want) in cases {
        let c = read_config(&format!("entries:\n  A:\n{body}"));
        match c.entries.first() {
            Some(Err(m)) => {
                assert_eq!(m.name, "A");
                assert!(m.why.contains(want), "{body}: {}", m.why);
            }
            other => panic!("{body}: {other:?}"),
        }
    }
    // A custom URL that names no secret needs none.
    let c = read_config(
        "entries:\n  A:\n    provider: custom\n    hostname: a\n    update_url: https://x.example/u?h={hostname}\n",
    );
    assert!(matches!(c.entries.first(), Some(Ok(_))), "{c:?}");
}

#[test]
fn a_list_of_entries_and_a_name_given_twice_are_refused() {
    let c = read_config("entries:\n  - name: Home\n    provider: duckdns\n");
    assert!(c.entries.is_empty());
    assert!(c.problem.is_some_and(|p| p.contains("mapping")));

    let c = read_config(
        "entries:\n  A:\n    provider: duckdns\n    hostname: a\n    secret: s\n  A:\n    provider: noip\n",
    );
    assert_eq!(c.entries.len(), 2);
    assert!(matches!(c.entries.first(), Some(Ok(e)) if e.provider == Provider::DuckDns));
    assert!(matches!(c.entries.get(1), Some(Err(m)) if m.why.contains("only the first")));

    assert_eq!(read_config(""), Config::default());
    assert_eq!(read_config("# nothing yet\n"), Config::default());
}

// ---------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------

#[test]
fn without_a_store_for_secrets_an_entry_waits_and_says_why_once() {
    let mut s = service(DUCK);
    assert_eq!(s.due(NOW), ["Home"]);
    let mut net = Net::new();
    let note = s
        .check("Home", NOW, &Undecided, &mut net)
        .expect("a first report is news");
    assert_eq!(note.level, Level::Warning);
    assert!(note.text.contains("D-Q4"), "{}", note.text);
    assert!(net.sent.is_empty(), "nothing is sent without the secret");
    let r = s.report("Home").expect("tracked");
    assert_eq!(r.state, State::NoSecret);
    assert_eq!(s.due_at("Home"), Some(NOW + 30 * MINUTE));
    // The same finding again is not news.
    assert_eq!(
        s.check("Home", NOW + 30 * MINUTE, &Undecided, &mut net),
        None
    );
    // Not due: not checked.
    assert_eq!(
        s.check("Home", NOW + 31 * MINUTE, &Undecided, &mut net),
        None
    );
    assert_eq!(s.report("Home").map(|r| r.at), Some(NOW + 30 * MINUTE));
}

#[test]
fn duckdns_is_updated_every_interval_and_reports_what_it_answered() {
    let mut s = service(DUCK);
    let secrets = Kept::with("dyndns/home", "tok");
    let mut net = Net::new().then(DUCK_URL, 200, "OK\n203.0.113.7\n\nUPDATED");
    let note = s.check("Home", NOW, &secrets, &mut net).expect("news");
    net.finished();
    assert_eq!(note.level, Level::Info);
    assert_eq!(
        note.text,
        "Home: updated: myhome.duckdns.org now points at 203.0.113.7 (the provider said: OK 203.0.113.7 UPDATED)"
    );
    assert_eq!(
        s.published("Home"),
        Some(Published {
            address: Some("203.0.113.7".parse().unwrap()),
            at: NOW
        })
    );
    // The provider reads the address off the request, so each check asks it.
    let later = NOW + 30 * MINUTE;
    let mut net = Net::new().then(DUCK_URL, 200, "OK\n203.0.113.7\n\nNOCHANGE");
    let note = s
        .check("Home", later, &secrets, &mut net)
        .expect("news: updated became current");
    net.finished();
    assert!(
        note.text
            .starts_with("Home: current: myhome.duckdns.org already pointed at 203.0.113.7")
    );
    assert_eq!(s.report("Home").map(|r| r.state), Some(State::Current));
    // No token in anything shown.
    assert!(!s.status_text(later).contains("tok&") && !s.status_text(later).contains("token=tok"));
}

#[test]
fn an_unchanged_address_is_not_sent_again_until_the_refresh_is_due() {
    let mut s = service(NOIP);
    let secrets = Kept::with("dyndns/cabin", "pw");
    let mut net =
        Net::new()
            .then(NOIP_CHECK, 200, "203.0.113.7")
            .then(NOIP_UPDATE, 200, "good 203.0.113.7");
    assert!(s.check("Cabin", NOW, &secrets, &mut net).is_some());
    net.finished();
    assert_eq!(s.report("Cabin").map(|r| r.state), Some(State::Updated));

    // Ten minutes on: the address page only.
    let t1 = NOW + 10 * MINUTE;
    let mut net = Net::new().then(NOIP_CHECK, 200, "203.0.113.7\n");
    let note = s
        .check("Cabin", t1, &secrets, &mut net)
        .expect("updated became current");
    net.finished();
    assert!(
        note.text.contains("nothing needed sending"),
        "{}",
        note.text
    );
    assert_eq!(s.published("Home"), None);
    assert_eq!(
        s.published("Cabin").map(|p| p.at),
        Some(NOW),
        "published when it was sent, not when checked"
    );

    // 25 days after it was published, it is sent again.
    let t2 = NOW + REFRESH_DAYS * DAY;
    s.all_due(t2);
    let mut net =
        Net::new()
            .then(NOIP_CHECK, 200, "203.0.113.7")
            .then(NOIP_UPDATE, 200, "nochg 203.0.113.7");
    s.check("Cabin", t2, &secrets, &mut net);
    net.finished();
    assert_eq!(s.published("Cabin").map(|p| p.at), Some(t2));

    // A new address is sent at once.
    let t3 = t2 + 10 * MINUTE;
    let mut net = Net::new().then(NOIP_CHECK, 200, "198.51.100.4").then(
        "https://dynupdate.no-ip.com/nic/update?hostname=cabin.ddns.net&myip=198.51.100.4",
        200,
        "good 198.51.100.4",
    );
    let note = s.check("Cabin", t3, &secrets, &mut net).expect("news");
    net.finished();
    assert!(
        note.text.contains("now points at 198.51.100.4"),
        "{}",
        note.text
    );
}

#[test]
fn a_refusal_that_would_repeat_holds_the_entry_until_it_changes() {
    let mut s = service(NOIP);
    let secrets = Kept::with("dyndns/cabin", "wrong");
    let mut net = Net::new()
        .then(NOIP_CHECK, 200, "203.0.113.7")
        .then(NOIP_UPDATE, 401, "badauth");
    let note = s.check("Cabin", NOW, &secrets, &mut net).expect("news");
    net.finished();
    assert_eq!(note.level, Level::Err);
    assert!(
        note.text
            .contains("not asked again until this entry changes"),
        "{}",
        note.text
    );
    assert!(
        note.text.contains("(the provider said: badauth)"),
        "{}",
        note.text
    );
    assert_eq!(s.report("Cabin").map(|r| r.state), Some(State::Held));
    assert_eq!(s.due_at("Cabin"), None);
    assert!(s.due(NOW + 365 * DAY).is_empty());
    assert_eq!(s.next_due(), None);
    s.all_due(NOW + DAY);
    assert!(
        s.due(NOW + DAY).is_empty(),
        "a run by hand does not release a hold either"
    );

    // The same configuration read again changes nothing.
    s.configure(read_config(NOIP), NOW + DAY);
    assert_eq!(s.report("Cabin").map(|r| r.state), Some(State::Held));
    // A changed one is the fix, perhaps: due at once.
    s.configure(
        read_config(&NOIP.replace("me@example.com", "me2@example.com")),
        NOW + DAY,
    );
    assert_eq!(s.due(NOW + DAY), ["Cabin"]);
    assert_eq!(s.report("Cabin").map(|r| r.state), Some(State::Pending));
}

#[test]
fn the_providers_trouble_is_waited_out_for_thirty_minutes() {
    let mut s = service(NOIP);
    let secrets = Kept::with("dyndns/cabin", "pw");
    let mut net = Net::new()
        .then(NOIP_CHECK, 200, "203.0.113.7")
        .then(NOIP_UPDATE, 200, "911");
    let note = s.check("Cabin", NOW, &secrets, &mut net).expect("news");
    assert_eq!(note.level, Level::Warning);
    assert_eq!(s.report("Cabin").map(|r| r.state), Some(State::Refused));
    assert_eq!(s.due_at("Cabin"), Some(NOW + 30 * MINUTE));
}

#[test]
fn an_unreachable_network_is_reported_and_tried_again_next_interval() {
    let mut s = service(NOIP);
    let secrets = Kept::with("dyndns/cabin", "pw");
    let mut net = Net::new().then_fail(NOIP_CHECK, "no route to the internet");
    let note = s.check("Cabin", NOW, &secrets, &mut net).expect("news");
    assert_eq!(
        note.text,
        "Cabin: unreachable: cannot reach the provider: no route to the internet"
    );
    assert_eq!(s.due_at("Cabin"), Some(NOW + 10 * MINUTE));
    let mut net = Net::new().then(NOIP_CHECK, 200, "<html>maintenance</html>");
    let note = s
        .check("Cabin", NOW + 10 * MINUTE, &secrets, &mut net)
        .expect("news");
    assert!(
        note.text
            .starts_with("Cabin: no-address: cannot learn this network's address"),
        "{}",
        note.text
    );
}

#[test]
fn a_disabled_entry_is_never_checked() {
    let mut s = service(&DUCK.replace("enabled: true", "enabled: false"));
    assert!(s.due(NOW).is_empty());
    assert_eq!(s.next_due(), None);
    let mut net = Net::new();
    assert_eq!(s.check("Home", NOW, &Undecided, &mut net), None);
    assert_eq!(s.report("Home").map(|r| r.state), Some(State::Disabled));
    assert!(s.all_well());
    assert!(s.status_text(NOW).contains("state: disabled"));
}

#[test]
fn a_reconfiguration_keeps_what_it_can() {
    let mut s = service(NOIP);
    let secrets = Kept::with("dyndns/cabin", "pw");
    let mut net =
        Net::new()
            .then(NOIP_CHECK, 200, "203.0.113.7")
            .then(NOIP_UPDATE, 200, "good 203.0.113.7");
    s.check("Cabin", NOW, &secrets, &mut net);
    // Only the interval changed: the same record, still published.
    s.configure(
        read_config(&NOIP.replace("every_minutes: 10", "every_minutes: 20")),
        NOW + MINUTE,
    );
    assert_eq!(s.published("Cabin").map(|p| p.at), Some(NOW));
    assert_eq!(s.due(NOW + MINUTE), ["Cabin"]);
    // Another hostname is another record.
    s.configure(
        read_config(&NOIP.replace("cabin.ddns.net", "lodge.ddns.net")),
        NOW + 2 * MINUTE,
    );
    assert_eq!(s.published("Cabin"), None);
    // Gone from the file: gone.
    s.configure(read_config(""), NOW + 3 * MINUTE);
    assert_eq!(s.report("Cabin"), None);
    assert!(s.status_text(NOW).contains("written_at"));
}

// ---------------------------------------------------------------------------
// The status file
// ---------------------------------------------------------------------------

#[test]
fn the_status_file_says_what_settings_shows_and_brings_it_back_after_a_restart() {
    let mut s = service(&format!("{DUCK}  Broken:\n    provider: ham\n"));
    let secrets = Kept::with("dyndns/home", "tok");
    let mut net = Net::new().then(DUCK_URL, 200, "OK\n203.0.113.7\n\nUPDATED");
    s.check("Home", NOW, &secrets, &mut net);
    let text = s.status_text(NOW);
    let doc = Document::parse(&text);
    assert!(text.starts_with("# What the dynamic-DNS service"));
    assert_eq!(
        doc.get_str(&["written_at"]).as_deref(),
        Some("2026-10-06T00:00:00Z")
    );
    let get = |k: &str| doc.get_str(&["entries", "Home", k]);
    assert_eq!(get("state").as_deref(), Some("updated"));
    assert_eq!(get("provider").as_deref(), Some("duckdns"));
    assert_eq!(get("hostname").as_deref(), Some("myhome.duckdns.org"));
    assert_eq!(get("address").as_deref(), Some("203.0.113.7"));
    assert_eq!(get("answer").as_deref(), Some("OK 203.0.113.7 UPDATED"));
    assert_eq!(get("published_at").as_deref(), Some("2026-10-06T00:00:00Z"));
    assert_eq!(get("checked_at").as_deref(), Some("2026-10-06T00:00:00Z"));
    assert_eq!(
        get("next_check_at").as_deref(),
        Some("2026-10-06T00:30:00Z")
    );
    assert_eq!(
        doc.get_str(&["entries", "Broken", "state"]).as_deref(),
        Some("misconfigured")
    );
    assert!(
        doc.get_str(&["entries", "Broken", "message"])
            .is_some_and(|m| m.contains("\"ham\""))
    );
    assert!(!s.all_well());
    assert_eq!(s.config_notes().len(), 1);

    // A restart: the address and when come back, so nothing is resent...
    let mut again = service(DUCK);
    again.remember(&text);
    assert_eq!(again.published("Home"), s.published("Home"));
    // ...but not for an entry now naming another hostname.
    let mut moved = service(&DUCK.replace("myhome.duckdns.org", "elsewhere.duckdns.org"));
    moved.remember(&text);
    assert_eq!(moved.published("Home"), None);
    // Nor from a file that is not ours.
    let mut fresh = service(DUCK);
    fresh.remember("entries:\n  Home:\n    provider: duckdns\n    published_at: yesterday\n");
    assert_eq!(fresh.published("Home"), None);
}

#[test]
fn a_name_that_yaml_would_misread_survives_the_status_file() {
    let text =
        "entries:\n  \"Home: main #1\":\n    provider: duckdns\n    hostname: a\n    secret: s\n";
    let s = service(text);
    let status = s.status_text(NOW);
    let doc = Document::parse(&status);
    assert_eq!(doc.keys(&["entries"]), ["Home: main #1"]);
    assert_eq!(
        doc.get_str(&["entries", "Home: main #1", "state"])
            .as_deref(),
        Some("pending")
    );
}

#[test]
fn a_journal_record_is_one_json_line() {
    let line = record(NOW, Level::Warning, "Home: \"x\"\nforged");
    assert!(line.starts_with('{') && line.ends_with('}') && !line.contains('\n'));
    assert!(line.contains("\"service\":\"dyndns\""));
    assert!(line.contains("\"level\":\"warning\""));
}

// ---------------------------------------------------------------------------
// Times and arguments
// ---------------------------------------------------------------------------

#[test]
fn times_are_rfc_3339_utc_both_ways() {
    for (ts, text) in [
        (0, "1970-01-01T00:00:00Z"),
        (86_399, "1970-01-01T23:59:59Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (1_000_000_000, "2001-09-09T01:46:40Z"),
        (NOW, "2026-10-06T00:00:00Z"),
        (4_102_444_800, "2100-01-01T00:00:00Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
    ] {
        assert_eq!(utc(ts), text);
        assert_eq!(parse_utc(text), Some(ts), "{text}");
    }
    for bad in [
        "",
        "yesterday",
        "2026-10-06 00:00:00Z",
        "2026-10-06T00:00:00",
        "2026-02-30T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-10-06T24:00:00Z",
        "1969-12-31T23:59:59Z",
        "2026-1a-06T00:00:00Z",
        "+026-10-06T00:00:00Z",
    ] {
        assert_eq!(parse_utc(bad), None, "{bad}");
    }
}

#[test]
fn the_command_line_is_read() {
    let args = |a: &[&str]| parse_args(&a.iter().map(OsString::from).collect::<Vec<_>>());
    let d = args(&[]).expect("no arguments");
    assert_eq!(
        d,
        Options {
            config: PathBuf::from(CONFIG_PATH),
            status: PathBuf::from(STATUS_PATH),
            forwards: PathBuf::from("/etc/portforwards.yaml"),
            forwards_status: PathBuf::from("/run/portforwards.yaml"),
            log: PathBuf::from(journalrec::MAIN_LOG_PATH),
            once: false,
        }
    );
    let o = args(&[
        "--config",
        "/tmp/c.yaml",
        "--status",
        "/tmp/s.yaml",
        "--forwards",
        "/tmp/f.yaml",
        "--forwards-status",
        "/tmp/fs.yaml",
        "--log",
        "/tmp/l",
        "--once",
    ])
    .expect("all of them");
    assert_eq!(
        o,
        Options {
            config: PathBuf::from("/tmp/c.yaml"),
            status: PathBuf::from("/tmp/s.yaml"),
            forwards: PathBuf::from("/tmp/f.yaml"),
            forwards_status: PathBuf::from("/tmp/fs.yaml"),
            log: PathBuf::from("/tmp/l"),
            once: true,
        }
    );
    assert!(args(&["--forwards"]).is_err());
    assert!(args(&["--config"]).is_err());
    assert!(args(&["--config", ""]).is_err());
    assert!(
        args(&["--frobnicate"]).is_err_and(|e| e.starts_with("unknown argument \"--frobnicate\""))
    );
    assert!(
        args(&["--config=/tmp/c"]).is_err(),
        "a file is the argument after its option"
    );
    assert!(args(&["--help"]).is_err_and(|e| e == USAGE));
    assert_eq!(shown_path(Path::new("a\"b\nc")), "\"a\\\"b\\u{a}c\"");
}

/// A provider that echoes the request -- secret and all -- does not get the
/// secret into the status file or the journal.
#[test]
fn a_secret_echoed_back_is_blanked() {
    let text = "entries:\n  Mine:\n    provider: custom\n    hostname: h.example.net\n    secret: dyndns/mine\n    \
                update_url: https://dns.example.net/u?h={hostname}&k={secret}\n";
    let mut s = service(text);
    let secrets = Kept::with("dyndns/mine", "hunter 2");
    let mut net = Net::new().then(
        "https://dns.example.net/u?h=h.example.net&k=hunter%202",
        403,
        "bad key hunter 2 (hunter%202)",
    );
    let note = s.check("Mine", NOW, &secrets, &mut net).expect("news");
    net.finished();
    let status = s.status_text(NOW);
    for shown in [note.text.as_str(), status.as_str()] {
        assert!(!shown.contains("hunter"), "{shown}");
    }
    assert!(
        note.text.contains("bad key [secret] ([secret])"),
        "{}",
        note.text
    );
}

/// A custom URL that names `{ip}` takes the address the router says, waits
/// while it has said none, and is checked again as soon as it does.
#[test]
fn a_custom_url_naming_the_address_takes_the_routers() {
    let text = "entries:\n  Mine:\n    provider: custom\n    hostname: h.example.net\n    \
                update_url: http://dns.example.net/u?h={hostname}&ip={ip}\n";
    let mut s = service(text);
    let mut net = Net::new();
    let note = s.check("Mine", NOW, &Undecided, &mut net).expect("news");
    net.finished();
    assert_eq!(s.report("Mine").map(|r| r.state), Some(State::NoAddress));
    assert!(
        note.text.contains("the router has not been asked yet"),
        "{}",
        note.text
    );
    // Told while nothing is due, the entry is due at once.
    let later = NOW + MINUTE;
    s.set_router_address(RouterAddress::Known("81.2.69.142".parse().unwrap()), later);
    assert_eq!(s.due(later), ["Mine"]);
    let mut net = Net::new().then(
        "http://dns.example.net/u?h=h.example.net&ip=81.2.69.142",
        200,
        "OK",
    );
    s.check("Mine", later, &Undecided, &mut net);
    net.finished();
    assert_eq!(
        s.published("Mine").and_then(|p| p.address),
        Some("81.2.69.142".parse().unwrap())
    );
    // The same address again changes nothing.
    let next = s.due_at("Mine");
    s.set_router_address(
        RouterAddress::Known("81.2.69.142".parse().unwrap()),
        later + 1,
    );
    assert_eq!(s.due_at("Mine"), next);
}

/// An entry of any other provider never waits on the router.
#[test]
fn other_providers_do_not_wait_on_the_router() {
    let mut s = service(DUCK);
    let next = s.due_at("Home");
    s.set_router_address(
        RouterAddress::Known("81.2.69.142".parse().unwrap()),
        NOW + 5,
    );
    assert_eq!(s.due_at("Home"), next);
}
