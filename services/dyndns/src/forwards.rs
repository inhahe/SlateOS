//! Port forwards: asking the router to pass ports on its internet side
//! through to this computer, so that something outside can connect in.
//!
//! `design.txt` asks for "port range passthroughs in router via UPnP or
//! NATPMP, whatever's detected", and for the router's address and the
//! network's internet address to be shown. Settings' Port Forwarding page
//! writes the forwards to `/etc/portforwards.yaml`; this service asks the
//! router for them and says what it said in `/run/portforwards.yaml`.
//!
//! # The configuration
//!
//! ```yaml
//! # Port forwards: ask the router to pass these ports through to this computer.
//! forwards:
//!   SSH:
//!     protocol: tcp          # tcp, udp, or both
//!     port: 22               # this computer's port, or a range: 6881-6889
//!     external_port: 2222    # the internet's side; the same as port when left out
//!     enabled: true
//! ```
//!
//! A mapping keyed by name, as `/etc/dyndns.yaml`'s entries are, for the same
//! reason: `yamldoc`, which keeps a user's comments when Settings edits the
//! file, reads no list of mappings. A range forwards each of its ports, at
//! most [`MAX_PORTS`] of them; `external_port` is then a range of the same
//! length, or its first port.
//!
//! # What the service does
//!
//! 1. Finds the router: the default gateway, asked over NAT-PMP and then UPnP
//!    (`dyndnsrouter`). Again every [`ROUTER_LOOK`] while none answers, and at
//!    once when the gateway changes.
//! 2. Asks it its internet address every [`ROUTER_CHECK`]; a NAT-PMP router
//!    that has restarted since (RFC 6886's epoch) has forgotten every mapping,
//!    and each is asked for again at once.
//! 3. Asks it for each port of each enabled forward, and again when half of
//!    the time it granted has passed. A refusal is asked again after
//!    [`REFUSED_RETRY`], or at once when the forward is changed.
//! 4. Removes what it mapped that the configuration no longer wants.
//! 5. Writes the status file, and journals each change of a forward's state.

use std::net::Ipv4Addr;

use dyndnsrouter::{External, Forward, Mapped, Net, Problem, Protocol, Router, discover, natpmp};
use yamldoc::Document;

use crate::{Level, Misconfigured, Note, utc};

/// The configuration Settings writes.
pub const FORWARDS_PATH: &str = "/etc/portforwards.yaml";

/// The status this service writes and Settings reads.
pub const FORWARDS_STATUS_PATH: &str = "/run/portforwards.yaml";

/// The most ports one forward may name. A range is one request to the router
/// for each port, at every renewal.
pub const MAX_PORTS: u16 = 100;

/// How long after finding no router it is looked for again, in seconds.
pub const ROUTER_LOOK: u64 = 5 * 60;

/// How often a router that answered is asked its address again, in seconds.
pub const ROUTER_CHECK: u64 = 15 * 60;

/// How long after a refusal a forward is asked for again, in seconds, unless
/// it is changed first.
pub const REFUSED_RETRY: u64 = 30 * 60;

/// The least time between two requests for one mapping, in seconds: a router
/// that grants a few seconds is not asked every few seconds.
const MIN_RENEW: u64 = 60;

// ---------------------------------------------------------------------------
// The configuration
// ---------------------------------------------------------------------------

/// One forward of the configuration, as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardEntry {
    /// Its name: the key it is written under.
    pub name: String,
    /// The protocols it forwards: TCP, UDP or both.
    pub protocols: Vec<Protocol>,
    /// The first port here.
    pub first_port: u16,
    /// The last port here: the first, for one port.
    pub last_port: u16,
    /// The first port on the internet side.
    pub first_external: u16,
    /// Whether it is asked for at all.
    pub enabled: bool,
}

impl ForwardEntry {
    /// How many ports it forwards, per protocol.
    #[must_use]
    pub fn count(&self) -> u16 {
        self.last_port
            .saturating_sub(self.first_port)
            .saturating_add(1)
    }

    /// The last port on the internet side.
    #[must_use]
    pub fn last_external(&self) -> u16 {
        self.first_external
            .saturating_add(self.last_port.saturating_sub(self.first_port))
    }

    /// Each mapping it asks the router for: per protocol, per port.
    #[must_use]
    pub fn forwards(&self) -> Vec<Forward> {
        let mut out = Vec::new();
        for &protocol in &self.protocols {
            for offset in 0..self.count() {
                out.push(Forward {
                    protocol,
                    internal_port: self.first_port.saturating_add(offset),
                    external_port: self.first_external.saturating_add(offset),
                    description: description(&self.name),
                });
            }
        }
        out
    }

    /// Its ports here as the file writes them: `22` or `6881-6889`.
    #[must_use]
    pub fn ports_text(&self) -> String {
        range_text(self.first_port, self.last_port)
    }

    /// Its ports on the internet side, written the same way.
    #[must_use]
    pub fn external_text(&self) -> String {
        range_text(self.first_external, self.last_external())
    }

    /// Its protocols as the file writes them.
    #[must_use]
    pub fn protocol_text(&self) -> &'static str {
        match self.protocols.as_slice() {
            [Protocol::Tcp] => "tcp",
            [Protocol::Udp] => "udp",
            _ => "both",
        }
    }
}

/// `first`, or `first-last`.
fn range_text(first: u16, last: u16) -> String {
    if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    }
}

/// What the router's own list shows a mapping as: the program asking, and the
/// forward's name, cut to the 63 characters some routers keep.
fn description(name: &str) -> String {
    let mut d = format!("SlateOS {name}");
    if let Some((cut, _)) = d.char_indices().nth(63) {
        d.truncate(cut);
    }
    d
}

/// A forwards file, read: each forward in the order written, and what is
/// wrong with the file as a whole, if anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForwardsConfig {
    /// Each forward, or why it cannot be used.
    pub entries: Vec<Result<ForwardEntry, Misconfigured>>,
    /// A problem with the whole file.
    pub problem: Option<String>,
}

/// Read a forwards file's text. A missing file is an empty text: no forwards,
/// and nothing wrong.
#[must_use]
pub fn read_forwards(text: &str) -> ForwardsConfig {
    let doc = Document::parse(text);
    let names = doc.keys(&["forwards"]);
    let mut config = ForwardsConfig::default();
    if names.is_empty()
        && doc
            .get_seq(&["forwards"])
            .is_some_and(|items| !items.is_empty())
    {
        config.problem = Some(
            "forwards is written as a list; this service reads it as a mapping from each forward's \
             name (\"SSH:\" and its settings indented under it)"
                .to_owned(),
        );
        return config;
    }
    // What each enabled forward already claims: (protocol, port here) and
    // (protocol, port outside), with the claiming forward's name. A port can
    // be forwarded once.
    let mut here: Vec<(Protocol, u16, String)> = Vec::new();
    let mut outside: Vec<(Protocol, u16, String)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for name in &names {
        if seen.contains(&name.as_str()) {
            config.entries.push(Err(Misconfigured {
                name: name.clone(),
                why: "this name is given to a forward above too; only the first is used".to_owned(),
            }));
            continue;
        }
        seen.push(name);
        let entry = read_forward(&doc, name).and_then(|e| {
            if !e.enabled {
                return Ok(e);
            }
            for f in e.forwards() {
                let clash = here
                    .iter()
                    .find(|(p, port, _)| *p == f.protocol && *port == f.internal_port)
                    .map(|(_, _, by)| {
                        format!(
                            "port {} ({}) is forwarded by {by} too",
                            f.internal_port, f.protocol
                        )
                    })
                    .or_else(|| {
                        outside
                            .iter()
                            .find(|(p, port, _)| *p == f.protocol && *port == f.external_port)
                            .map(|(_, _, by)| {
                                format!(
                                    "external port {} ({}) is forwarded by {by} too",
                                    f.external_port, f.protocol
                                )
                            })
                    });
                if let Some(why) = clash {
                    return Err(Misconfigured {
                        name: name.clone(),
                        why: format!("{why}; only the first is used"),
                    });
                }
            }
            for f in e.forwards() {
                here.push((f.protocol, f.internal_port, name.clone()));
                outside.push((f.protocol, f.external_port, name.clone()));
            }
            Ok(e)
        });
        config.entries.push(entry);
    }
    config
}

/// A port, or a range of them (`6881-6889`): its first and last.
fn read_ports(text: &str) -> Option<(u16, u16)> {
    let text = text.trim();
    let (first, last) = match text.split_once('-') {
        Some((a, b)) => (a.trim().parse::<u16>().ok()?, b.trim().parse::<u16>().ok()?),
        None => {
            let p = text.parse::<u16>().ok()?;
            (p, p)
        }
    };
    (first != 0 && first <= last).then_some((first, last))
}

/// One forward, or why it cannot be used.
fn read_forward(doc: &Document, name: &str) -> Result<ForwardEntry, Misconfigured> {
    let get = |key: &str| doc.get_str(&["forwards", name, key]);
    let bad = |why: String| Misconfigured {
        name: name.to_owned(),
        why,
    };
    let protocol = get("protocol").unwrap_or_default();
    let protocols = match protocol.trim().to_ascii_lowercase().as_str() {
        "tcp" => vec![Protocol::Tcp],
        "udp" => vec![Protocol::Udp],
        "both" => vec![Protocol::Tcp, Protocol::Udp],
        "" => return Err(bad("it names no protocol (tcp, udp or both)".to_owned())),
        _ => {
            return Err(bad(format!(
                "its protocol {protocol:?} is not tcp, udp or both"
            )));
        }
    };
    let port = get("port").unwrap_or_default();
    if port.trim().is_empty() {
        return Err(bad("it names no port".to_owned()));
    }
    let Some((first_port, last_port)) = read_ports(&port) else {
        return Err(bad(format!(
            "its port {port:?} is not a port (1 to 65535) or a range of them (6881-6889)"
        )));
    };
    let count = last_port.saturating_sub(first_port).saturating_add(1);
    if count > MAX_PORTS {
        return Err(bad(format!(
            "its range {port} is {count} ports; a forward may have {MAX_PORTS} at the most"
        )));
    }
    let external = get("external_port").unwrap_or_default();
    let first_external = if external.trim().is_empty() {
        first_port
    } else {
        match read_ports(&external) {
            Some((a, b)) if b == a => a,
            Some((a, b)) if b.saturating_sub(a) == last_port.saturating_sub(first_port) => a,
            Some(_) => {
                return Err(bad(format!(
                    "its external_port range {external} is not as long as its port range {port}"
                )));
            }
            None => {
                return Err(bad(format!(
                    "its external_port {external:?} is not a port (1 to 65535) or a range of them"
                )));
            }
        }
    };
    if u32::from(first_external).saturating_add(u32::from(count)) > 65536 {
        return Err(bad(format!(
            "its external ports from {first_external} run past 65535"
        )));
    }
    let enabled = match get("enabled") {
        None => true,
        Some(_) => match doc.get_bool(&["forwards", name, "enabled"]) {
            Some(b) => b,
            None => return Err(bad("its enabled is neither true nor false".to_owned())),
        },
    };
    Ok(ForwardEntry {
        name: name.to_owned(),
        protocols,
        first_port,
        last_port,
        first_external,
        enabled,
    })
}

// ---------------------------------------------------------------------------
// The service's state
// ---------------------------------------------------------------------------

/// What a forward's last check found, as the status file names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardState {
    /// Not asked for yet.
    Pending,
    /// Turned off in the configuration.
    Disabled,
    /// The router forwards every port of it.
    Forwarded,
    /// The router forwards some of its ports and refused the others.
    Partial,
    /// The router refused; asked again later.
    Refused,
    /// The router could not be asked, or did not answer.
    Unreachable,
    /// No router that maps ports was found.
    NoRouter,
}

impl ForwardState {
    /// As the status file spells it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Disabled => "disabled",
            Self::Forwarded => "forwarded",
            Self::Partial => "partial",
            Self::Refused => "refused",
            Self::Unreachable => "unreachable",
            Self::NoRouter => "no-router",
        }
    }
}

/// One mapping the service wants, or made.
#[derive(Clone, Debug)]
struct Want {
    /// The forward it is part of.
    entry: String,
    /// The mapping.
    forward: Forward,
    /// What the router granted, and when (seconds since the epoch).
    mapped: Option<(Mapped, u64)>,
    /// When to ask next.
    due: u64,
    /// What went wrong the last time, if it did.
    problem: Option<Problem>,
}

/// The router, as far as the service knows.
#[derive(Clone, Debug)]
enum RouterState {
    /// Not looked for yet, or the network changed.
    Unknown,
    /// None answered.
    Absent {
        /// Why.
        problem: Problem,
        /// When it was looked for.
        at: u64,
        /// When to look again.
        next: u64,
    },
    /// One answered.
    Found {
        /// It.
        router: Router,
        /// Its internet address, as last told.
        external: External,
        /// When it was last asked (seconds since the epoch).
        at: u64,
        /// When to ask again.
        next: u64,
    },
}

/// A forward of the configuration, in the order written.
#[derive(Clone, Debug)]
enum Slot {
    Entry(ForwardEntry),
    Misconfigured(Misconfigured),
}

/// The network as the system sees it: this computer's address on it, and
/// its default gateway -- the router.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lan {
    /// This computer's address.
    pub here: Ipv4Addr,
    /// The default gateway.
    pub gateway: Ipv4Addr,
}

/// What the service knows of the router and every forward.
#[derive(Clone, Debug)]
pub struct Ports {
    slots: Vec<Slot>,
    problem: Option<String>,
    wants: Vec<Want>,
    /// Mappings made that the configuration no longer wants: to remove.
    unwanted: Vec<Want>,
    router: RouterState,
    /// The network the router was found on.
    lan: Option<Lan>,
    /// Each forward's last state and message, for journalling changes.
    shown: Vec<(String, ForwardState, String)>,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            problem: None,
            wants: Vec::new(),
            unwanted: Vec::new(),
            router: RouterState::Unknown,
            lan: None,
            shown: Vec::new(),
        }
    }
}

impl Ports {
    /// No forwards, no router found.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a configuration, as read at start or after the file changed. A
    /// mapping already made that is still wanted, unchanged, keeps its state;
    /// one that is new or changed is due now -- a change may be the fix for a
    /// refusal -- and one no longer wanted is removed from the router at the
    /// next step.
    pub fn configure(&mut self, config: ForwardsConfig, now: u64) {
        let mut before: Vec<Want> = self.wants.drain(..).collect();
        self.slots.clear();
        for item in config.entries {
            match item {
                Ok(entry) => {
                    if entry.enabled {
                        for forward in entry.forwards() {
                            let kept = before
                                .iter()
                                .position(|w| w.entry == entry.name && w.forward == forward)
                                .map(|i| before.swap_remove(i));
                            self.wants.push(kept.unwrap_or(Want {
                                entry: entry.name.clone(),
                                forward,
                                mapped: None,
                                due: now,
                                problem: None,
                            }));
                        }
                    }
                    self.slots.push(Slot::Entry(entry));
                }
                Err(m) => self.slots.push(Slot::Misconfigured(m)),
            }
        }
        // A forward removed, or changed under the same name, is unwanted in
        // its old form: removed at the next step, before anything new is
        // asked for -- a forward whose port here changed asks for the same
        // external port, which the old mapping holds.
        self.unwanted.extend(
            before
                .into_iter()
                .filter(|w| w.mapped.is_some())
                .map(|w| Want { due: now, ..w }),
        );
        self.problem = config.problem;
    }

    /// The router's internet address, when one was found and it is one the
    /// internet can reach.
    #[must_use]
    pub fn public_address(&self) -> Option<Ipv4Addr> {
        match &self.router {
            RouterState::Found { external, .. } if external.is_public() => Some(external.address),
            _ => None,
        }
    }

    /// Why the router has not given a public internet address, in a phrase;
    /// `None` when it has.
    #[must_use]
    pub fn why_no_address(&self) -> Option<String> {
        match &self.router {
            RouterState::Unknown => Some("the router has not been asked yet".to_owned()),
            RouterState::Absent { problem, .. } => {
                Some(format!("no router that can say it was found: {problem}"))
            }
            RouterState::Found { external, .. } if !external.is_public() => Some(format!(
                "the router's own internet address, {}, is a private one: another router, or the \
                 internet provider's, is between it and the internet",
                external.address
            )),
            RouterState::Found { .. } => None,
        }
    }

    /// When [`Ports::step`] is next due. Forwards wait on a router: while
    /// none is found, only looking for one again is due.
    #[must_use]
    pub fn next_due(&self) -> u64 {
        match &self.router {
            RouterState::Unknown => 0,
            RouterState::Absent { next, .. } => *next,
            RouterState::Found { next, .. } => self
                .wants
                .iter()
                .chain(&self.unwanted)
                .map(|w| w.due)
                .fold(*next, u64::min),
        }
    }

    /// Do what is due at `now` on the network `lan` (`None`: this computer
    /// has no address, or no gateway). Returns the journal's lines: one for
    /// each forward whose state or message changed, and one for each change
    /// of router.
    pub fn step(&mut self, net: &mut dyn Net, lan: Option<Lan>, now: u64) -> Vec<Note> {
        let mut notes = Vec::new();
        if lan != self.lan {
            // Another network, or none: what was found is of the old one, and
            // so is every mapping on its router.
            self.lan = lan;
            self.router = RouterState::Unknown;
            for w in &mut self.wants {
                w.mapped = None;
                w.due = now;
            }
            self.unwanted.clear();
        }
        let Some(lan) = lan else {
            if !matches!(self.router, RouterState::Absent { .. }) {
                self.router = RouterState::Absent {
                    problem: Problem::Unreachable(
                        "this computer has no network address, or no default gateway".to_owned(),
                    ),
                    at: now,
                    next: u64::MAX,
                };
            }
            notes.extend(self.changed());
            return notes;
        };
        self.look(net, lan, now, &mut notes);
        if let RouterState::Found { router, .. } = &self.router {
            let router = router.clone();
            let answered = self.remove_unwanted(net, &router, now)
                && self.map_due(net, &router, lan.here, now);
            // A router that stopped answering is asked its address at the
            // next step, and looked for again if it does not answer that.
            if !answered && let RouterState::Found { next, .. } = &mut self.router {
                *next = now;
            }
        }
        notes.extend(self.changed());
        notes
    }

    /// Find the router if it is not known, or ask it its address if that is
    /// due.
    fn look(&mut self, net: &mut dyn Net, lan: Lan, now: u64, notes: &mut Vec<Note>) {
        match &self.router {
            RouterState::Unknown => {}
            RouterState::Absent { next, .. } if *next <= now => {}
            RouterState::Absent { .. } => return,
            RouterState::Found {
                router,
                external,
                at,
                next,
            } => {
                if *next > now {
                    return;
                }
                let (router, earlier, earlier_at) = (router.clone(), *external, *at);
                match router.external(net) {
                    Ok(external) => {
                        if let (Some(e0), Some(e1)) = (earlier.epoch, external.epoch)
                            && natpmp::router_restarted(e0, earlier_at, e1, now)
                        {
                            notes.push(Note {
                                level: Level::Warning,
                                text: format!(
                                    "the router at {} has restarted and forgotten its forwards; \
                                     asking for them again",
                                    lan.gateway
                                ),
                            });
                            self.all_due(now);
                        }
                        if external.address != earlier.address {
                            notes.push(address_note(&external));
                        }
                        self.router = RouterState::Found {
                            router,
                            external,
                            at: now,
                            next: now.saturating_add(ROUTER_CHECK),
                        };
                        return;
                    }
                    Err(problem) => {
                        notes.push(Note {
                            level: Level::Warning,
                            text: format!(
                                "the router at {} did not say its internet address ({problem}); \
                                 looking for it again",
                                lan.gateway
                            ),
                        });
                    }
                }
            }
        }
        match discover(net, lan.gateway) {
            Ok((router, external)) => {
                let name = if router.name.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", router.name)
                };
                notes.push(Note {
                    level: Level::Info,
                    text: format!(
                        "found the router at {}{name}: it speaks {}",
                        lan.gateway,
                        router.method.name()
                    ),
                });
                notes.push(address_note(&external));
                self.router = RouterState::Found {
                    router,
                    external,
                    at: now,
                    next: now.saturating_add(ROUTER_CHECK),
                };
                // A router found anew has none of this run's mappings that
                // anyone knows of.
                self.all_due(now);
            }
            Err(problem) => {
                if !matches!(&self.router, RouterState::Absent { problem: p, .. } if *p == problem)
                {
                    notes.push(Note {
                        level: Level::Warning,
                        text: format!(
                            "no router that can forward ports at {}: {problem}",
                            lan.gateway
                        ),
                    });
                }
                self.router = RouterState::Absent {
                    problem,
                    at: now,
                    next: now.saturating_add(ROUTER_LOOK),
                };
            }
        }
    }

    /// Make every mapping due at `now`.
    fn all_due(&mut self, now: u64) {
        for w in &mut self.wants {
            w.due = now;
        }
    }

    /// Ask the router to remove what is no longer wanted. A mapping with a
    /// lease ends by itself if the router cannot be asked, and is forgotten
    /// after one try; a permanent one is asked about again every
    /// [`ROUTER_LOOK`] until the router removes it. Returns false if the
    /// router did not answer, after which nothing more is asked of it: each
    /// request would wait out its own timeout.
    fn remove_unwanted(&mut self, net: &mut dyn Net, router: &Router, now: u64) -> bool {
        let mut answering = true;
        let unwanted = std::mem::take(&mut self.unwanted);
        for mut w in unwanted {
            let Some((mapped, _)) = w.mapped else {
                continue;
            };
            if w.due > now {
                self.unwanted.push(w);
                continue;
            }
            let removed = if answering {
                router.unmap(net, &w.forward, &mapped)
            } else {
                Err(Problem::Unreachable(String::new()))
            };
            if let Err(Problem::Unreachable(_)) = removed {
                answering = false;
            }
            if removed.is_err() && mapped.lifetime == 0 {
                w.due = now.saturating_add(ROUTER_LOOK);
                self.unwanted.push(w);
            }
        }
        answering
    }

    /// Ask for every mapping that is due. Returns false if the router did not
    /// answer: the mappings after the first that went unanswered are not
    /// asked for in this step -- each would wait out its own timeout, a
    /// hundred ports of a range at fifteen seconds each -- and take its
    /// problem.
    fn map_due(&mut self, net: &mut dyn Net, router: &Router, here: Ipv4Addr, now: u64) -> bool {
        let mut silent: Option<Problem> = None;
        for w in &mut self.wants {
            if w.due > now {
                continue;
            }
            let asked = match &silent {
                Some(problem) => Err(problem.clone()),
                None => router.map(net, here, &w.forward, natpmp::LIFETIME),
            };
            match asked {
                Ok(mapped) => {
                    w.due = if mapped.lifetime == 0 {
                        now.saturating_add(ROUTER_CHECK)
                    } else {
                        now.saturating_add((u64::from(mapped.lifetime) / 2).max(MIN_RENEW))
                    };
                    w.mapped = Some((mapped, now));
                    w.problem = None;
                }
                Err(problem) => {
                    w.due = now.saturating_add(match problem {
                        Problem::Refused { .. } => REFUSED_RETRY,
                        _ => ROUTER_LOOK,
                    });
                    // What was granted before stands until its lease ends.
                    if w.mapped.is_some_and(|(m, at)| {
                        m.lifetime != 0 && now >= at.saturating_add(u64::from(m.lifetime))
                    }) {
                        w.mapped = None;
                    }
                    if matches!(problem, Problem::Unreachable(_)) && silent.is_none() {
                        silent = Some(problem.clone());
                    }
                    w.problem = Some(problem);
                }
            }
        }
        silent.is_none()
    }

    /// The state and message of the forward `entry`, from its mappings.
    fn report(&self, entry: &ForwardEntry) -> (ForwardState, String, String) {
        if !entry.enabled {
            return (
                ForwardState::Disabled,
                "turned off".to_owned(),
                String::new(),
            );
        }
        let wants: Vec<&Want> = self
            .wants
            .iter()
            .filter(|w| w.entry == entry.name)
            .collect();
        let forwarded = wants.iter().filter(|w| w.mapped.is_some()).count();
        let problem = wants.iter().find_map(|w| w.problem.as_ref());
        let answer = match problem {
            Some(Problem::Refused { answer, .. }) => answer.clone(),
            _ => String::new(),
        };
        if forwarded == wants.len() && !wants.is_empty() {
            let external: Vec<u16> = wants
                .iter()
                .filter_map(|w| w.mapped.map(|(m, _)| m.external_port))
                .collect();
            let asked: Vec<u16> = wants.iter().map(|w| w.forward.external_port).collect();
            let ports = if external == asked {
                format!(
                    "external port{} {}",
                    plural(entry.count()),
                    entry.external_text()
                )
            } else {
                format!(
                    "external port{} {} (the router chose them: {} was asked for)",
                    plural(entry.count()),
                    list(&external),
                    entry.external_text()
                )
            };
            return (
                ForwardState::Forwarded,
                format!(
                    "the router forwards {ports} ({}) to this computer's port{} {}",
                    protocol_words(entry),
                    plural(entry.count()),
                    entry.ports_text()
                ),
                String::new(),
            );
        }
        let why = match (&self.router, problem) {
            (RouterState::Unknown, _) => {
                return (
                    ForwardState::Pending,
                    "not asked for yet".to_owned(),
                    String::new(),
                );
            }
            (RouterState::Absent { problem, .. }, _) => {
                return (
                    ForwardState::NoRouter,
                    format!("no router that can forward ports was found: {problem}"),
                    String::new(),
                );
            }
            (RouterState::Found { .. }, Some(p)) => p,
            (RouterState::Found { .. }, None) => {
                return (
                    ForwardState::Pending,
                    "not asked for yet".to_owned(),
                    String::new(),
                );
            }
        };
        if forwarded > 0 {
            return (
                ForwardState::Partial,
                format!(
                    "the router forwards {forwarded} of its {} ports; for the rest, {why}",
                    wants.len()
                ),
                answer,
            );
        }
        let state = match why {
            Problem::Refused { .. } => ForwardState::Refused,
            _ => ForwardState::Unreachable,
        };
        (state, why.to_string(), answer)
    }

    /// The journal lines for forwards whose state or message changed since
    /// they were last journalled.
    fn changed(&mut self) -> Vec<Note> {
        let mut notes = Vec::new();
        let mut shown = Vec::new();
        for slot in &self.slots {
            let Slot::Entry(entry) = slot else { continue };
            let (state, message, _) = self.report(entry);
            let was = self.shown.iter().find(|(n, _, _)| *n == entry.name);
            if was.is_none_or(|(_, s, m)| *s != state || *m != message) {
                notes.push(Note {
                    level: match state {
                        ForwardState::Forwarded
                        | ForwardState::Disabled
                        | ForwardState::Pending => Level::Info,
                        _ => Level::Warning,
                    },
                    text: format!("forward {}: {}: {message}", entry.name, state.key()),
                });
            }
            shown.push((entry.name.clone(), state, message));
        }
        self.shown = shown;
        notes
    }

    /// One journal line per misconfigured forward, and one for a problem with
    /// the whole file.
    #[must_use]
    pub fn config_notes(&self) -> Vec<Note> {
        let mut notes: Vec<Note> = self
            .problem
            .iter()
            .map(|p| Note {
                level: Level::Err,
                text: format!("{FORWARDS_PATH}: {p}"),
            })
            .collect();
        for slot in &self.slots {
            if let Slot::Misconfigured(m) = slot {
                notes.push(Note {
                    level: Level::Err,
                    text: format!("forward {}: not used: {}", m.name, m.why),
                });
            }
        }
        notes
    }

    /// Whether every forward is forwarded or turned off, and the file could
    /// be read.
    #[must_use]
    pub fn all_well(&self) -> bool {
        self.problem.is_none()
            && self.slots.iter().all(|s| match s {
                Slot::Entry(e) => matches!(
                    self.report(e).0,
                    ForwardState::Forwarded | ForwardState::Disabled
                ),
                Slot::Misconfigured(_) => false,
            })
    }

    /// The state of the forward `name`, if it is a usable one.
    #[must_use]
    pub fn state(&self, name: &str) -> Option<(ForwardState, String)> {
        self.slots.iter().find_map(|s| match s {
            Slot::Entry(e) if e.name == name => {
                let (state, message, _) = self.report(e);
                Some((state, message))
            }
            _ => None,
        })
    }

    /// The status file's text: the router, and each forward.
    #[must_use]
    pub fn status_text(&self, now: u64) -> String {
        let mut doc = Document::new();
        doc.set_str(&["written_at"], &utc(now));
        if let Some(p) = &self.problem {
            doc.set_str(&["problem"], p);
        }
        let mut public = None;
        let mut router = |key: &str, value: &str| doc.set_str(&["router", key], value);
        match &self.router {
            RouterState::Unknown => {
                router("state", "looking");
                router("message", "not looked for yet");
            }
            RouterState::Absent { problem, at, .. } => {
                router("state", "none");
                router("message", &problem.to_string());
                if let Some(lan) = self.lan {
                    router("address", &lan.gateway.to_string());
                }
                router("checked_at", &utc(*at));
            }
            RouterState::Found {
                router: r,
                external,
                at,
                ..
            } => {
                router("state", "found");
                router("address", &r.address.to_string());
                router("protocol", r.method.name());
                if !r.name.is_empty() {
                    router("name", &r.name);
                }
                router("internet_address", &external.address.to_string());
                public = Some(external.is_public());
                router(
                    "message",
                    &if external.is_public() {
                        format!(
                            "the router speaks {}; this network's internet address is {}",
                            r.method.name(),
                            external.address
                        )
                    } else {
                        format!(
                            "the router speaks {}, but its own internet address, {}, is a private \
                             one: another router, or the internet provider's, is between it and \
                             the internet, so its forwards do not reach this computer from outside",
                            r.method.name(),
                            external.address
                        )
                    },
                );
                router("checked_at", &utc(*at));
            }
        }
        if let Some(public) = public {
            doc.set_bool(&["router", "public"], public);
        }
        for slot in &self.slots {
            match slot {
                Slot::Entry(entry) => {
                    let name = entry.name.as_str();
                    let (state, message, answer) = self.report(entry);
                    let mut set =
                        |key: &str, value: &str| doc.set_str(&["forwards", name, key], value);
                    set("protocol", entry.protocol_text());
                    set("port", &entry.ports_text());
                    set("external_port", &entry.external_text());
                    set("state", state.key());
                    set("message", &message);
                    if !answer.is_empty() {
                        set("answer", &answer);
                    }
                    let wants = self.wants.iter().filter(|w| w.entry == name);
                    if let Some(at) = wants
                        .clone()
                        .filter_map(|w| w.mapped.map(|(_, at)| at))
                        .max()
                    {
                        set("checked_at", &utc(at));
                    }
                    if let Some(due) = wants.map(|w| w.due).min()
                        && matches!(self.router, RouterState::Found { .. })
                    {
                        set("next_check_at", &utc(due));
                    }
                }
                Slot::Misconfigured(m) => {
                    let name = m.name.as_str();
                    doc.set_str(&["forwards", name, "state"], "misconfigured");
                    doc.set_str(&["forwards", name, "message"], &m.why);
                }
            }
        }
        let mut text = String::from(STATUS_HEADER);
        text.push_str(&doc.to_text());
        text
    }
}

/// The comment the status file opens with.
const STATUS_HEADER: &str = "\
# What the dynamic-DNS service (services/dyndns) last heard from the router,
# and what it did for each forward of /etc/portforwards.yaml. Written by the
# service after every change, and read by Settings; edits here are
# overwritten. Times are UTC.
";

/// The journal line for a router's internet address.
fn address_note(external: &External) -> Note {
    if external.is_public() {
        Note {
            level: Level::Info,
            text: format!("this network's internet address is {}", external.address),
        }
    } else {
        Note {
            level: Level::Warning,
            text: format!(
                "the router's internet address, {}, is a private one: another router, or the \
                 internet provider's, is between it and the internet, and port forwards on it do \
                 not reach this computer from outside",
                external.address
            ),
        }
    }
}

/// `s` for more than one.
const fn plural(n: u16) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Ports as a list: `2222, 2223`.
fn list(ports: &[u16]) -> String {
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// `TCP`, `UDP`, or `TCP and UDP`.
fn protocol_words(entry: &ForwardEntry) -> String {
    entry
        .protocols
        .iter()
        .map(|p| p.upnp_name())
        .collect::<Vec<_>>()
        .join(" and ")
}

#[cfg(test)]
mod tests;
