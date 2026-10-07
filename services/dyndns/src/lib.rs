//! `dyndns` -- the service that keeps dynamic-DNS hostnames pointed at this
//! network.
//!
//! Dynamic DNS keeps a hostname (`myhome.duckdns.org`) pointing at a home
//! network whose public address its internet provider keeps changing. For
//! each entry of `/etc/dyndns.yaml` -- the file Settings' Dynamic DNS page
//! writes -- this service learns the network's public address every
//! `every_minutes`, and when it has changed since the provider last accepted
//! it, tells the provider. What happened goes to `/run/dyndns.yaml`, which
//! Settings shows, and to the journal. Lane E's
//! `requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`
//! asked for it; it replaces the kernel's table (`kernel/src/fs/dyndns.rs`),
//! which held entries nothing could change and contacted no one.
//!
//! # The configuration
//!
//! ```yaml
//! # Dynamic DNS: keep these hostnames pointing at this network's address.
//! entries:
//!   Home:
//!     provider: duckdns        # dynu, noip, duckdns, cloudflare, freedns, custom
//!     hostname: myhome.duckdns.org
//!     username: ""             # for the providers that sign in with a name
//!     secret: dyndns/home      # the name the password is kept under -- never the password
//!     update_url: ""           # custom only: {hostname} {ip} {username} {secret}
//!     every_minutes: 30        # 5 at the least
//!     enabled: true
//! ```
//!
//! The request sketched `entries` as a list; here it is a mapping keyed by
//! each entry's name. The YAML library every configuration file goes
//! through (`yamldoc`, which keeps a file's comments and layout when Settings
//! edits it) reads no list of mappings, and a name that is a key cannot be
//! given twice. What each provider needs is `dyndnsproviders`', the crate
//! Settings links too.
//!
//! # A check
//!
//! 1. A disabled entry, or one held by a refusal, is left alone.
//! 2. Its secret is fetched from where service passwords are kept. Where
//!    that is has not been decided (`open-questions/D-Q4.md`), so an entry
//!    that needs one waits, and its status says why ([`Secrets`]).
//! 3. The public address is learned the provider's way
//!    (`dyndnsproviders::public_address`) -- or, for a custom provider's URL
//!    that names `{ip}`, from the router ([`Service::set_router_address`]),
//!    no provider's page being the user's to ask.
//! 4. The update is sent when the provider reads the address off the request
//!    itself, when the address differs from the one last published, or when
//!    that was [`REFRESH_DAYS`] ago -- an address published once and never
//!    again can be changed at the provider's end without anyone noticing.
//! 5. The answer decides the next check: `every_minutes` later; 30 minutes
//!    after the provider's own trouble; and after a refusal that asking again
//!    would only repeat (a bad password, an unknown hostname), not until the
//!    entry changes -- providers block a client that keeps sending those.
//!
//! # Layout
//!
//! This file is the part that touches no operating system -- the
//! configuration, the checks, the status file, the journal's lines -- and is
//! tested on the development host with a replayed network. [`forwards`] is
//! the same for the router's port forwards. `main.rs` is the rest: the
//! clock, the files and the connections.

pub mod forwards;

use std::ffi::OsString;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use dyndnsproviders::encode::percent_encode;
use dyndnsproviders::{
    Address, Exchange, Outcome, Provider, Retry, Target, needs_router_address, needs_secret,
    public_address, publish,
};
use yamldoc::Document;

/// The configuration Settings writes.
pub const CONFIG_PATH: &str = "/etc/dyndns.yaml";

/// The status this service writes and Settings reads.
pub const STATUS_PATH: &str = "/run/dyndns.yaml";

/// The name this service's journal records carry.
pub const SERVICE: &str = "dyndns";

/// How often an entry is checked when its configuration does not say.
pub const DEFAULT_EVERY_MINUTES: u32 = 30;

/// The least interval an entry may ask for. A check asks the provider's
/// address page, and a provider asked every minute is a provider annoyed.
pub const MIN_EVERY_MINUTES: u32 = 5;

/// The most: a week.
pub const MAX_EVERY_MINUTES: u32 = 7 * 24 * 60;

/// After this many days an unchanged address is published again anyway.
/// `ddclient`'s `max-interval` default, chosen for the same reason: well
/// inside the 30 days after which some providers retire a hostname nothing
/// has updated.
pub const REFRESH_DAYS: u64 = 25;

/// Seconds in a day.
const DAY: u64 = 24 * 60 * 60;

// ---------------------------------------------------------------------------
// The configuration
// ---------------------------------------------------------------------------

/// One entry of the configuration, as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name: the key it is written under.
    pub name: String,
    /// The provider.
    pub provider: Provider,
    /// The hostname to keep pointed here.
    pub hostname: String,
    /// The username, for the providers that sign in with one.
    pub username: String,
    /// The name the secret is kept under: never the secret.
    pub secret: String,
    /// A custom provider's update URL.
    pub update_url: String,
    /// Minutes between checks, within [`MIN_EVERY_MINUTES`] and
    /// [`MAX_EVERY_MINUTES`].
    pub every_minutes: u32,
    /// Whether it is checked at all.
    pub enabled: bool,
}

/// An entry that cannot be used as it is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misconfigured {
    /// Its name.
    pub name: String,
    /// What is wrong, in a sentence.
    pub why: String,
}

/// A configuration file, read: its entries in the order written, and what is
/// wrong with the file as a whole, if anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Each entry, or why it cannot be used.
    pub entries: Vec<Result<Entry, Misconfigured>>,
    /// A problem with the whole file.
    pub problem: Option<String>,
}

/// Read a configuration file's text. A missing file is an empty text: no
/// entries, and nothing wrong.
#[must_use]
pub fn read_config(text: &str) -> Config {
    let doc = Document::parse(text);
    let names = doc.keys(&["entries"]);
    let mut config = Config::default();
    if names.is_empty()
        && doc
            .get_seq(&["entries"])
            .is_some_and(|items| !items.is_empty())
    {
        config.problem = Some(
            "entries is written as a list; this service reads it as a mapping from each entry's name \
             (\"Home:\" and its settings indented under it)"
                .to_owned(),
        );
        return config;
    }
    let mut seen: Vec<&str> = Vec::new();
    for name in &names {
        if seen.contains(&name.as_str()) {
            // A key given twice reads as its first: the second is not this
            // entry, and must not silently become it.
            config.entries.push(Err(Misconfigured {
                name: name.clone(),
                why: "this name is given to an entry above too; only the first is used".to_owned(),
            }));
            continue;
        }
        seen.push(name);
        config.entries.push(read_entry(&doc, name));
    }
    config
}

/// One entry, or why it cannot be used.
fn read_entry(doc: &Document, name: &str) -> Result<Entry, Misconfigured> {
    let get = |key: &str| doc.get_str(&["entries", name, key]);
    let bad = |why: String| Misconfigured {
        name: name.to_owned(),
        why,
    };
    let provider_key = get("provider").unwrap_or_default();
    let Some(provider) = Provider::from_key(&provider_key) else {
        let known: Vec<&str> = Provider::ALL.iter().map(|p| p.key()).collect();
        return Err(bad(if provider_key.is_empty() {
            format!("it names no provider (one of: {})", known.join(", "))
        } else {
            format!(
                "its provider {provider_key:?} is not one of: {}",
                known.join(", ")
            )
        }));
    };
    let every_minutes = match get("every_minutes") {
        None => DEFAULT_EVERY_MINUTES,
        Some(v) => match v.trim().parse::<u64>() {
            Ok(n) => u32::try_from(n)
                .unwrap_or(MAX_EVERY_MINUTES)
                .clamp(MIN_EVERY_MINUTES, MAX_EVERY_MINUTES),
            Err(_) => {
                return Err(bad(format!(
                    "its every_minutes {v:?} is not a whole number of minutes"
                )));
            }
        },
    };
    let enabled = match get("enabled") {
        None => true,
        Some(_) => match doc.get_bool(&["entries", name, "enabled"]) {
            Some(b) => b,
            None => return Err(bad("its enabled is neither true nor false".to_owned())),
        },
    };
    let entry = Entry {
        name: name.to_owned(),
        provider,
        hostname: get("hostname").unwrap_or_default().trim().to_owned(),
        username: get("username").unwrap_or_default(),
        secret: get("secret").unwrap_or_default().trim().to_owned(),
        update_url: get("update_url").unwrap_or_default().trim().to_owned(),
        every_minutes,
        enabled,
    };
    // Everything a check needs that the file says. The secret's *name* is
    // checked here; the secret itself when it is fetched, so the check below
    // is given a stand-in for it.
    let target = Target {
        provider,
        hostname: &entry.hostname,
        username: &entry.username,
        secret: "(kept elsewhere)",
        update_url: &entry.update_url,
    };
    if let Some(why) = target.incomplete() {
        return Err(bad(why));
    }
    if needs_secret(provider, &entry.update_url) && entry.secret.is_empty() {
        return Err(bad(format!(
            "it names no secret: the name its {} is kept under, such as dyndns/{}",
            provider.secret().1.to_lowercase(),
            name.to_lowercase()
        )));
    }
    Ok(entry)
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

/// Where an entry's secret comes from: the one function the service reads
/// passwords through, so that the answer to `open-questions/D-Q4.md` changes
/// this and nothing else.
pub trait Secrets {
    /// The secret kept under `name`.
    ///
    /// # Errors
    ///
    /// Why it cannot be had, in a sentence a user can act on.
    fn secret(&self, name: &str) -> Result<String, String>;
}

/// The store there is until `open-questions/D-Q4.md` is answered: none.
/// Every lookup says so, and an entry needing a secret waits.
#[derive(Clone, Copy, Debug, Default)]
pub struct Undecided;

impl Secrets for Undecided {
    fn secret(&self, name: &str) -> Result<String, String> {
        Err(format!(
            "its secret {name:?} cannot be read: there is no store for services' passwords yet \
             (where they will be kept is open-questions D-Q4)"
        ))
    }
}

// ---------------------------------------------------------------------------
// The service's state
// ---------------------------------------------------------------------------

/// What an entry's last check found, as the status file names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Not checked yet.
    Pending,
    /// Turned off in the configuration.
    Disabled,
    /// The provider changed the record to this network's address.
    Updated,
    /// The record holds this network's address already.
    Current,
    /// A custom provider took the update without saying more.
    Accepted,
    /// The provider refused, and asking again would be refused again: not
    /// asked until the entry changes.
    Held,
    /// The provider refused or had trouble; asked again later.
    Refused,
    /// The provider could not be reached.
    Unreachable,
    /// The network's public address could not be learned.
    NoAddress,
    /// The provider's answer could not be read.
    Unreadable,
    /// The entry needs its secret, and it could not be had.
    NoSecret,
}

impl State {
    /// As the status file spells it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Disabled => "disabled",
            Self::Updated => "updated",
            Self::Current => "current",
            Self::Accepted => "accepted",
            Self::Held => "held",
            Self::Refused => "refused",
            Self::Unreachable => "unreachable",
            Self::NoAddress => "no-address",
            Self::Unreadable => "unreadable",
            Self::NoSecret => "no-secret",
        }
    }

    /// Whether the hostname is known to point here.
    #[must_use]
    pub const fn is_good(self) -> bool {
        matches!(
            self,
            Self::Updated | Self::Current | Self::Accepted | Self::Disabled
        )
    }
}

/// The address an entry last published, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Published {
    /// The address, when known: a provider that read it off the request and
    /// did not say leaves it unknown.
    pub address: Option<IpAddr>,
    /// When (seconds since the epoch).
    pub at: u64,
}

/// What a check found, for the status file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The state.
    pub state: State,
    /// What it means, in a sentence.
    pub message: String,
    /// The provider's own words, when it said any.
    pub answer: String,
    /// When (seconds since the epoch); 0 before the first check.
    pub at: u64,
}

/// A journal line a check produced: its level and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The record's level.
    pub level: Level,
    /// The message.
    pub text: String,
}

/// A journal record's level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Something went wrong.
    Err,
    /// Something did not happen that should have.
    Warning,
    /// What happened.
    Info,
}

impl Level {
    /// As the journal spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Err => "err",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One usable entry and what the service knows of it.
#[derive(Clone, Debug)]
struct Tracked {
    entry: Entry,
    /// When its next check is due; `None` while it is not to be checked
    /// (disabled, or held by a refusal).
    due: Option<u64>,
    published: Option<Published>,
    report: Report,
}

impl Tracked {
    fn new(entry: Entry, now: u64, published: Option<Published>) -> Self {
        let (due, state, message) = if entry.enabled {
            (Some(now), State::Pending, "not checked yet".to_owned())
        } else {
            (None, State::Disabled, "turned off".to_owned())
        };
        Self {
            entry,
            due,
            published,
            report: Report {
                state,
                message,
                answer: String::new(),
                at: 0,
            },
        }
    }
}

/// An entry of the configuration, in the order written.
#[derive(Clone, Debug)]
enum Slot {
    Tracked(Box<Tracked>),
    Misconfigured(Misconfigured),
}

/// What the router says this network's internet address is, for the entries
/// whose address only it can say (a custom provider's URL naming `{ip}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouterAddress {
    /// It said this, and it is one the internet can reach.
    Known(IpAddr),
    /// It has not said: why, as a clause.
    Unknown(String),
}

impl Default for RouterAddress {
    fn default() -> Self {
        Self::Unknown("the router has not been asked yet".to_owned())
    }
}

/// Every entry and what the service knows of each.
#[derive(Clone, Debug, Default)]
pub struct Service {
    slots: Vec<Slot>,
    problem: Option<String>,
    router_address: RouterAddress,
}

/// What a check did, before its outcome is recorded.
enum Step {
    /// The secret could not be had.
    NoSecret(String),
    /// The address is the one published, recently enough: nothing sent.
    StillCurrent(IpAddr),
    /// The provider was asked (or its address page was), with this result.
    Asked {
        outcome: Outcome,
        /// The address the update named, if it named one.
        sent: Option<IpAddr>,
    },
}

impl Service {
    /// A service with no entries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a configuration, as read at start or again after the file
    /// changed. An entry whose configuration is unchanged keeps its state and
    /// its schedule. One that changed, or is new, is due now, and a hold
    /// from a refusal ends with it -- the change may be the fix. One whose
    /// provider and hostname are unchanged keeps the address it published:
    /// its record is the same record.
    pub fn configure(&mut self, config: Config, now: u64) {
        let mut old: Vec<Tracked> = self
            .slots
            .drain(..)
            .filter_map(|s| match s {
                Slot::Tracked(t) => Some(*t),
                Slot::Misconfigured(_) => None,
            })
            .collect();
        for item in config.entries {
            let slot = match item {
                Ok(entry) => {
                    let before = old
                        .iter()
                        .position(|t| t.entry.name == entry.name)
                        .map(|i| old.swap_remove(i));
                    let tracked = match before {
                        Some(t) if t.entry == entry => t,
                        Some(t) => {
                            let same_record = t.entry.provider == entry.provider
                                && t.entry.hostname.eq_ignore_ascii_case(&entry.hostname);
                            Tracked::new(entry, now, if same_record { t.published } else { None })
                        }
                        None => Tracked::new(entry, now, None),
                    };
                    Slot::Tracked(Box::new(tracked))
                }
                Err(m) => Slot::Misconfigured(m),
            };
            self.slots.push(slot);
        }
        self.problem = config.problem;
    }

    /// Take back what an earlier run of this service published, from the
    /// status file it left: an address and when, for each entry whose
    /// provider and hostname are as they were. A restart then does not send
    /// an unchanged address again.
    pub fn remember(&mut self, status_text: &str) {
        let doc = Document::parse(status_text);
        for slot in &mut self.slots {
            let Slot::Tracked(t) = slot else { continue };
            if t.published.is_some() {
                continue;
            }
            let name = t.entry.name.as_str();
            let get = |key: &str| doc.get_str(&["entries", name, key]);
            let same = get("provider").as_deref() == Some(t.entry.provider.key())
                && get("hostname").is_some_and(|h| h.eq_ignore_ascii_case(&t.entry.hostname));
            let Some(at) = get("published_at").as_deref().and_then(parse_utc) else {
                continue;
            };
            if same {
                t.published = Some(Published {
                    address: get("address").and_then(|a| a.parse().ok()),
                    at,
                });
            }
        }
    }

    /// The names of the entries due at `now`, in the configuration's order.
    #[must_use]
    pub fn due(&self, now: u64) -> Vec<String> {
        self.tracked()
            .filter(|t| t.due.is_some_and(|d| d <= now))
            .map(|t| t.entry.name.clone())
            .collect()
    }

    /// When the next check is due, if any is.
    #[must_use]
    pub fn next_due(&self) -> Option<u64> {
        self.tracked().filter_map(|t| t.due).min()
    }

    /// Take what the router says this network's internet address is. When it
    /// changes, every entry that asks the router for its address and is not
    /// held is due at `now`: one waited for it, or published another.
    pub fn set_router_address(&mut self, address: RouterAddress, now: u64) {
        if address == self.router_address {
            return;
        }
        self.router_address = address;
        for slot in &mut self.slots {
            if let Slot::Tracked(t) = slot
                && t.due.is_some()
                && needs_router_address(t.entry.provider, &t.entry.update_url)
            {
                t.due = Some(now);
            }
        }
    }

    /// Make every enabled entry that is not held due at `now`: a run by hand
    /// (`--once`) checks them all.
    pub fn all_due(&mut self, now: u64) {
        for slot in &mut self.slots {
            if let Slot::Tracked(t) = slot
                && t.due.is_some()
            {
                t.due = Some(now);
            }
        }
    }

    /// Whether every entry is in a good state ([`State::is_good`]) and the
    /// file as a whole could be read.
    #[must_use]
    pub fn all_well(&self) -> bool {
        self.problem.is_none()
            && self.slots.iter().all(|s| match s {
                Slot::Tracked(t) => t.report.state.is_good(),
                Slot::Misconfigured(_) => false,
            })
    }

    fn tracked(&self) -> impl Iterator<Item = &Tracked> {
        self.slots.iter().filter_map(|s| match s {
            Slot::Tracked(t) => Some(&**t),
            Slot::Misconfigured(_) => None,
        })
    }

    /// The report of the entry `name`, if it is a usable one.
    #[must_use]
    pub fn report(&self, name: &str) -> Option<&Report> {
        self.tracked()
            .find(|t| t.entry.name == name)
            .map(|t| &t.report)
    }

    /// The address the entry `name` last published, and when.
    #[must_use]
    pub fn published(&self, name: &str) -> Option<Published> {
        self.tracked()
            .find(|t| t.entry.name == name)
            .and_then(|t| t.published)
    }

    /// When the entry `name` is next due, if it is to be checked again.
    #[must_use]
    pub fn due_at(&self, name: &str) -> Option<u64> {
        self.tracked()
            .find(|t| t.entry.name == name)
            .and_then(|t| t.due)
    }

    /// Check the entry `name` if it is due at `now`: learn the address,
    /// publish it if it needs publishing, and record what happened. Returns
    /// the journal line when the entry's state or message changed -- a
    /// service that checks every few minutes must not write a line for each
    /// check that finds what the last one found.
    pub fn check(
        &mut self,
        name: &str,
        now: u64,
        secrets: &dyn Secrets,
        ex: &mut dyn Exchange,
    ) -> Option<Note> {
        let t = self.slots.iter_mut().find_map(|s| match s {
            Slot::Tracked(t) if t.entry.name == name => Some(t),
            _ => None,
        })?;
        if t.due.is_none_or(|d| d > now) {
            return None;
        }
        let step = run(
            &t.entry,
            t.published,
            now,
            secrets,
            ex,
            &self.router_address,
        );
        apply(t, now, step)
    }

    /// The status file's text: for each entry, its state and message, the
    /// provider's words, the address last published and when, when it was
    /// checked and when it is next.
    #[must_use]
    pub fn status_text(&self, now: u64) -> String {
        // Built from nothing and given its comment after: `yamldoc` puts the
        // first key of a document holding only comments above them.
        let mut doc = Document::new();
        doc.set_str(&["written_at"], &utc(now));
        if let Some(p) = &self.problem {
            doc.set_str(&["problem"], p);
        }
        for slot in &self.slots {
            match slot {
                Slot::Tracked(t) => {
                    let name = t.entry.name.as_str();
                    let mut set =
                        |key: &str, value: &str| doc.set_str(&["entries", name, key], value);
                    set("provider", t.entry.provider.key());
                    set("hostname", &t.entry.hostname);
                    set("state", t.report.state.key());
                    set("message", &t.report.message);
                    if !t.report.answer.is_empty() {
                        set("answer", &t.report.answer);
                    }
                    if let Some(p) = t.published {
                        set(
                            "address",
                            &p.address.map(|a| a.to_string()).unwrap_or_default(),
                        );
                        set("published_at", &utc(p.at));
                    }
                    if t.report.at > 0 {
                        set("checked_at", &utc(t.report.at));
                    }
                    if let Some(d) = t.due {
                        set("next_check_at", &utc(d));
                    }
                }
                Slot::Misconfigured(m) => {
                    let name = m.name.as_str();
                    doc.set_str(&["entries", name, "state"], "misconfigured");
                    doc.set_str(&["entries", name, "message"], &m.why);
                }
            }
        }
        let mut text = String::from(STATUS_HEADER);
        text.push_str(&doc.to_text());
        text
    }

    /// One journal line per misconfigured entry, and one for a problem with
    /// the whole file: what reading a configuration found wrong.
    #[must_use]
    pub fn config_notes(&self) -> Vec<Note> {
        let mut notes: Vec<Note> = self
            .problem
            .iter()
            .map(|p| Note {
                level: Level::Err,
                text: format!("{CONFIG_PATH}: {p}"),
            })
            .collect();
        for slot in &self.slots {
            if let Slot::Misconfigured(m) = slot {
                notes.push(Note {
                    level: Level::Err,
                    text: format!("{}: not used: {}", m.name, m.why),
                });
            }
        }
        notes
    }
}

/// The comment the status file opens with.
const STATUS_HEADER: &str = "\
# What the dynamic-DNS service (services/dyndns) last did for each entry of
# /etc/dyndns.yaml. Written by the service after every check, and read by
# Settings; edits here are overwritten. Times are UTC.
";

/// Do one check of `entry`: what happened, before it is recorded.
/// `router` is what the router says the address is, for an entry that asks
/// it.
fn run(
    entry: &Entry,
    published: Option<Published>,
    now: u64,
    secrets: &dyn Secrets,
    ex: &mut dyn Exchange,
    router: &RouterAddress,
) -> Step {
    let secret = if needs_secret(entry.provider, &entry.update_url) {
        match secrets.secret(&entry.secret) {
            Ok(s) => s,
            Err(why) => return Step::NoSecret(why),
        }
    } else {
        String::new()
    };
    let target = Target {
        provider: entry.provider,
        hostname: &entry.hostname,
        username: &entry.username,
        secret: &secret,
        update_url: &entry.update_url,
    };
    let learned = if needs_router_address(entry.provider, &entry.update_url) {
        match router {
            RouterAddress::Known(ip) => Ok(Address::Known(*ip)),
            RouterAddress::Unknown(why) => Err(Outcome::NoAddress {
                why: format!("its update URL needs this network's address, and {why}"),
            }),
        }
    } else {
        public_address(&target, ex)
    };
    let address = match learned {
        Ok(a) => a,
        Err(outcome) => {
            return Step::Asked {
                outcome: scrub(outcome, &secret),
                sent: None,
            };
        }
    };
    let sent = match address {
        Address::Known(ip) => Some(ip),
        Address::ProviderSees => None,
    };
    if let Some(ip) = sent
        && let Some(p) = published
        && p.address == Some(ip)
        && now.saturating_sub(p.at) < REFRESH_DAYS.saturating_mul(DAY)
    {
        return Step::StillCurrent(ip);
    }
    Step::Asked {
        outcome: scrub(publish(&target, address, ex), &secret),
        sent,
    }
}

/// `outcome` with `secret` blanked out of everything it says. What a
/// provider answers goes to the status file, which every user may read, and
/// to the journal; a provider that echoes the request back -- a custom one
/// may -- must not put the password there.
fn scrub(outcome: Outcome, secret: &str) -> Outcome {
    if secret.is_empty() {
        return outcome;
    }
    let encoded = percent_encode(secret);
    let clean = |s: String| {
        let s = s.replace(secret, "[secret]");
        if encoded == secret {
            s
        } else {
            s.replace(&encoded, "[secret]")
        }
    };
    match outcome {
        Outcome::Updated { address, answer } => Outcome::Updated {
            address,
            answer: clean(answer),
        },
        Outcome::Unchanged { address, answer } => Outcome::Unchanged {
            address,
            answer: clean(answer),
        },
        Outcome::Accepted { answer } => Outcome::Accepted {
            answer: clean(answer),
        },
        Outcome::Refused {
            answer,
            meaning,
            retry,
        } => Outcome::Refused {
            answer: clean(answer),
            meaning: clean(meaning),
            retry,
        },
        Outcome::Unreachable { why } => Outcome::Unreachable { why: clean(why) },
        Outcome::NoAddress { why } => Outcome::NoAddress { why: clean(why) },
        Outcome::Unreadable { answer } => Outcome::Unreadable {
            answer: clean(answer),
        },
    }
}

/// Record what a check did: the report, the address published, when the next
/// check is due. The journal line, if the report changed.
fn apply(t: &mut Tracked, now: u64, step: Step) -> Option<Note> {
    let next = Some(now.saturating_add(u64::from(t.entry.every_minutes).saturating_mul(60)));
    let host = t.entry.hostname.clone();
    let (state, message, answer, due) = match step {
        Step::NoSecret(why) => (State::NoSecret, why, String::new(), next),
        Step::StillCurrent(ip) => (
            State::Current,
            format!("{host} still points at {ip}; nothing needed sending"),
            String::new(),
            next,
        ),
        Step::Asked { outcome, sent } => match outcome {
            Outcome::Updated { address, answer } => {
                let address = address.or(sent);
                t.published = Some(Published { address, at: now });
                let message = match address {
                    Some(a) => format!("{host} now points at {a}"),
                    None => format!(
                        "{host} now points at the address this network's requests come from"
                    ),
                };
                (State::Updated, message, answer, next)
            }
            Outcome::Unchanged { address, answer } => {
                let address = address.or(sent);
                t.published = Some(Published { address, at: now });
                let message = match address {
                    Some(a) => format!("{host} already pointed at {a}"),
                    None => format!("{host} already pointed at this network"),
                };
                (State::Current, message, answer, next)
            }
            Outcome::Accepted { answer } => {
                t.published = Some(Published {
                    address: sent,
                    at: now,
                });
                (
                    State::Accepted,
                    "the provider took the update".to_owned(),
                    answer,
                    next,
                )
            }
            Outcome::Refused {
                answer,
                meaning,
                retry: Retry::AfterChange,
            } => (
                State::Held,
                format!("{meaning} -- not asked again until this entry changes"),
                answer,
                None,
            ),
            Outcome::Refused {
                answer,
                meaning,
                retry: Retry::AfterMinutes(m),
            } => (
                State::Refused,
                meaning,
                answer,
                Some(now.saturating_add(u64::from(m).saturating_mul(60))),
            ),
            Outcome::Unreachable { why } => (
                State::Unreachable,
                format!("cannot reach the provider: {why}"),
                String::new(),
                next,
            ),
            Outcome::NoAddress { why } => (
                State::NoAddress,
                format!("cannot learn this network's address: {why}"),
                String::new(),
                next,
            ),
            Outcome::Unreadable { answer } => (
                State::Unreadable,
                "the provider's answer could not be read".to_owned(),
                answer,
                next,
            ),
        },
    };
    t.due = due;
    let changed = t.report.state != state || t.report.message != message;
    t.report = Report {
        state,
        message,
        answer,
        at: now,
    };
    changed.then(|| {
        let level = match state {
            State::Updated
            | State::Current
            | State::Accepted
            | State::Pending
            | State::Disabled => Level::Info,
            State::Unreachable | State::NoAddress | State::NoSecret | State::Refused => {
                Level::Warning
            }
            State::Held | State::Unreadable => Level::Err,
        };
        let mut text = format!("{}: {}: {}", t.entry.name, state.key(), t.report.message);
        if !t.report.answer.is_empty() {
            text.push_str(&format!(" (the provider said: {})", t.report.answer));
        }
        Note { level, text }
    })
}

/// One journal record from this service, as a JSON line without its newline.
#[must_use]
pub fn record(ts: u64, level: Level, text: &str) -> String {
    journalrec::Record {
        ts,
        level: level.name().to_owned(),
        service: SERVICE.to_owned(),
        msg: text.to_owned(),
        pid: None,
    }
    .to_json_line()
}

// ---------------------------------------------------------------------------
// Times
// ---------------------------------------------------------------------------

/// `ts` (seconds since the epoch) as an RFC 3339 UTC time:
/// `2026-10-06T07:55:00Z`.
#[must_use]
pub fn utc(ts: u64) -> String {
    let days = ts / DAY;
    let secs = ts % DAY;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// An RFC 3339 UTC time as [`utc`] writes it, back to seconds since the
/// epoch. `None` for anything else.
#[must_use]
pub fn parse_utc(s: &str) -> Option<u64> {
    let b = s.trim().as_bytes();
    if b.len() != 20
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || b.get(19) != Some(&b'Z')
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<u64> {
        let part = s.trim().get(from..to)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1970..=9999).contains(&y)
        || !(1..=12).contains(&mo)
        || !(1..=31).contains(&d)
        || h > 23
        || mi > 59
        || se > 59
    {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    // The day must exist: 2026-02-30 comes back as March 2nd.
    if civil_from_days(days) != (y, mo, d) {
        return None;
    }
    days.checked_mul(DAY)?
        .checked_add(seconds_of_day(h, mi, se))
}

/// Seconds into a day at `h:mi:se`.
// Bounded: the caller has checked h <= 23, mi <= 59, se <= 59.
#[allow(clippy::arithmetic_side_effects)]
fn seconds_of_day(h: u64, mi: u64, se: u64) -> u64 {
    h * 3600 + mi * 60 + se
}

/// The date `days` after 1970-01-01, by Howard Hinnant's `civil_from_days`,
/// for days on or after the epoch.
// The arithmetic is bounded: `days` < 2^64 / 86400, and every intermediate
// stays within u64 for it.
#[allow(clippy::arithmetic_side_effects)]
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

/// The days from 1970-01-01 to a date in 1970..=9999, the inverse of
/// [`civil_from_days`].
// Bounded by the caller's range check: year 9999 is about 2.9 million days.
#[allow(clippy::arithmetic_side_effects)]
fn days_from_civil(y: u64, m: u64, d: u64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/// The command line, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The configuration to read.
    pub config: PathBuf,
    /// The status file to write.
    pub status: PathBuf,
    /// The port forwards to read.
    pub forwards: PathBuf,
    /// The port forwards' status file to write.
    pub forwards_status: PathBuf,
    /// The journal to append to.
    pub log: PathBuf,
    /// Check every entry and forward once, then exit.
    pub once: bool,
}

/// The usage line.
pub const USAGE: &str = "usage: dyndns [--config FILE] [--status FILE] [--forwards FILE] \
                         [--forwards-status FILE] [--log FILE] [--once]";

/// Read the command line (without the program's name). A file is the
/// argument after its option, as the bytes it is: a path need not be text.
///
/// # Errors
///
/// What is wrong with it, for a message.
pub fn parse_args(args: &[OsString]) -> Result<Options, String> {
    let mut opts = Options {
        config: PathBuf::from(CONFIG_PATH),
        status: PathBuf::from(STATUS_PATH),
        forwards: PathBuf::from(forwards::FORWARDS_PATH),
        forwards_status: PathBuf::from(forwards::FORWARDS_STATUS_PATH),
        log: PathBuf::from(journalrec::MAIN_LOG_PATH),
        once: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut file = |name: &str| -> Result<PathBuf, String> {
            match it.next() {
                Some(v) if !v.is_empty() => Ok(PathBuf::from(v)),
                _ => Err(format!("{name} needs a file\n{USAGE}")),
            }
        };
        match arg.to_str() {
            Some("--config") => opts.config = file("--config")?,
            Some("--status") => opts.status = file("--status")?,
            Some("--forwards") => opts.forwards = file("--forwards")?,
            Some("--forwards-status") => opts.forwards_status = file("--forwards-status")?,
            Some("--log") => opts.log = file("--log")?,
            Some("--once") => opts.once = true,
            Some("-h" | "--help") => return Err(USAGE.to_owned()),
            _ => {
                return Err(format!(
                    "unknown argument {}\n{USAGE}",
                    shown_path(Path::new(arg))
                ));
            }
        }
    }
    Ok(opts)
}

/// A path for a message: the text it is, or its bytes escaped where they
/// are not text -- never a lossy decode.
#[must_use]
pub fn shown_path(path: &Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut out = String::with_capacity(bytes.len().saturating_add(2));
    out.push('"');
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '"' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", u32::from(c))),
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests;
