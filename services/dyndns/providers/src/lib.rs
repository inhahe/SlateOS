//! What a dynamic-DNS provider is, for the updater and for Settings alike.
//!
//! Dynamic DNS keeps a hostname (`myhome.duckdns.org`) pointing at a network
//! whose public address its internet provider keeps changing. Two programs
//! have to agree on what each provider is: Settings' Dynamic DNS page, which
//! asks the user for a provider's fields and writes `/etc/dyndns.yaml`, and
//! the `dyndns` service, which reads that file and talks to the provider.
//! This crate is the one definition both link (lane E's
//! `requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`),
//! so the page cannot offer a field the service ignores, and `noip` means one
//! thing.
//!
//! # What a provider is, here
//!
//! - **Its fields**: [`Provider::username`], [`Provider::secret`] and
//!   [`Provider::takes_update_url`] say what an entry of it needs, and the
//!   labels Settings shows them under.
//! - **How this network's public address is learned** ([`public_address`]):
//!   from the provider's own "what is my address" page, or not at all where
//!   the provider reads the address off the update request itself (DuckDNS,
//!   FreeDNS). No third party is asked: an entry for one provider never sends
//!   anything to another.
//! - **The update** ([`publish`]), and what its answer means ([`Outcome`]):
//!   the provider's own words are kept beside a plain meaning, because "the
//!   provider said badauth" is what tells a user what to fix, and providers
//!   block a client that keeps repeating a refused update -- so each refusal
//!   also says when asking again is allowed ([`Retry`]).
//!
//! # No transport
//!
//! Requests leave through an [`Exchange`] the caller supplies: the service's
//! carries them over the network, and the tests' replays the providers'
//! answers. A request holds its secret (in its URL or a header), so it is
//! journalled only as [`Request::shown`], with the secret blanked.
//!
//! # The answers
//!
//! What each provider answers is its documented protocol -- No-IP's and
//! Dynu's "dyndns2" codes, DuckDNS's `OK`/`KO`, FreeDNS's update page,
//! Cloudflare's API v4 -- and the tests replay those answers. None has been
//! checked against the live services yet: nothing on SlateOS can make an
//! HTTPS connection (no TLS), which is every provider's update. When one can,
//! that check is the next thing to do here.

pub mod encode;
pub mod json;

use core::fmt;
use core::net::IpAddr;

use encode::{basic_auth, json_string_into, percent_encode, percent_encode_into};

/// The `User-Agent` every request carries. No-IP and Dynu refuse a client that
/// sends none, or a generic one, with `badagent`.
pub const USER_AGENT: &str = concat!("SlateOS-dyndns/", env!("CARGO_PKG_VERSION"));

/// How long to wait after a provider's own trouble (dyndns2's `911` and
/// `dnserr`), in minutes: the protocol's "wait at least 30 minutes".
pub const SERVER_TROUBLE_MINUTES: u32 = 30;

/// Cloudflare's API.
const CLOUDFLARE_API: &str = "https://api.cloudflare.com/client/v4";

/// A dynamic-DNS provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Dynu (dynu.com): the dyndns2 protocol, signed in with a username.
    Dynu,
    /// No-IP (noip.com): the dyndns2 protocol, signed in with a username or
    /// the account's email.
    NoIp,
    /// DuckDNS (duckdns.org): a token in the update URL.
    DuckDns,
    /// Cloudflare: its API, with an API token that may edit the zone's DNS.
    Cloudflare,
    /// FreeDNS (freedns.afraid.org): the token of its "direct URL".
    FreeDns,
    /// Any provider updated by fetching a URL: the user writes the URL, with
    /// `{hostname}`, `{ip}`, `{username}` and `{secret}` where those go.
    Custom,
}

/// Whether an entry needs a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// The entry cannot be updated without it.
    Required,
    /// A custom URL needs it only if it names it (`{username}`, `{secret}`).
    Optional,
    /// This provider never uses it.
    Unused,
}

impl Provider {
    /// Every provider, in the order Settings offers them.
    pub const ALL: [Self; 6] = [
        Self::Dynu,
        Self::NoIp,
        Self::DuckDns,
        Self::Cloudflare,
        Self::FreeDns,
        Self::Custom,
    ];

    /// The name `/etc/dyndns.yaml` spells it with.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Dynu => "dynu",
            Self::NoIp => "noip",
            Self::DuckDns => "duckdns",
            Self::Cloudflare => "cloudflare",
            Self::FreeDns => "freedns",
            Self::Custom => "custom",
        }
    }

    /// The provider `key` names, in any letter case.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.key().eq_ignore_ascii_case(key.trim()))
    }

    /// Its name, as a person writes it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dynu => "Dynu",
            Self::NoIp => "No-IP",
            Self::DuckDns => "DuckDNS",
            Self::Cloudflare => "Cloudflare",
            Self::FreeDns => "FreeDNS",
            Self::Custom => "Custom",
        }
    }

    /// Whether an entry needs a username, and what to call it.
    #[must_use]
    pub const fn username(self) -> (Need, &'static str) {
        match self {
            Self::NoIp => (Need::Required, "Username or email"),
            Self::Dynu => (Need::Required, "Username"),
            Self::Custom => (Need::Optional, "Username"),
            Self::DuckDns | Self::Cloudflare | Self::FreeDns => (Need::Unused, ""),
        }
    }

    /// Whether an entry needs a secret, and what the provider calls it.
    #[must_use]
    pub const fn secret(self) -> (Need, &'static str) {
        match self {
            Self::NoIp | Self::Dynu => (Need::Required, "Password"),
            Self::DuckDns => (Need::Required, "Token"),
            Self::Cloudflare => (Need::Required, "API token"),
            Self::FreeDns => (Need::Required, "Update token"),
            Self::Custom => (Need::Optional, "Secret"),
        }
    }

    /// Whether an entry carries its own update URL (custom only).
    #[must_use]
    pub const fn takes_update_url(self) -> bool {
        matches!(self, Self::Custom)
    }

    /// A hostname of the shape this provider expects, for a field's hint.
    #[must_use]
    pub const fn hostname_example(self) -> &'static str {
        match self {
            Self::Dynu => "myhome.dynu.net",
            Self::NoIp => "myhome.ddns.net",
            Self::DuckDns => "myhome.duckdns.org",
            Self::Cloudflare => "home.example.com",
            Self::FreeDns => "myhome.mooo.com",
            Self::Custom => "home.example.com",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One hostname to keep pointed at this network, as the update needs it.
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    /// Whose it is.
    pub provider: Provider,
    /// The name to update (`myhome.duckdns.org`).
    pub hostname: &'a str,
    /// The username, for the providers that sign in with one.
    pub username: &'a str,
    /// The password or token -- the secret itself, never its name.
    pub secret: &'a str,
    /// The update URL, for a custom provider.
    pub update_url: &'a str,
}

/// Whether an entry needs its secret: every provider's but a custom URL's
/// that names none. The service asks before it fetches one.
#[must_use]
pub fn needs_secret(provider: Provider, update_url: &str) -> bool {
    match provider.secret().0 {
        Need::Required => true,
        Need::Optional => update_url.contains("{secret}"),
        Need::Unused => false,
    }
}

/// Whether an entry needs its username, as [`needs_secret`].
#[must_use]
pub fn needs_username(provider: Provider, update_url: &str) -> bool {
    match provider.username().0 {
        Need::Required => true,
        Need::Optional => update_url.contains("{username}"),
        Need::Unused => false,
    }
}

impl Target<'_> {
    /// What the entry lacks for an update to be tried, in a sentence; `None`
    /// when nothing.
    #[must_use]
    pub fn incomplete(&self) -> Option<String> {
        let host = self.hostname.trim();
        if self.provider != Provider::Custom || self.update_url.contains("{hostname}") {
            if host.is_empty() {
                return Some("it has no hostname".to_owned());
            }
            if host.contains(|c: char| c.is_whitespace() || c == '/') {
                return Some(format!("its hostname {host:?} is not a hostname"));
            }
        }
        if self.provider == Provider::Custom {
            let url = self.update_url.trim();
            if url.is_empty() {
                return Some("a custom entry needs the URL that updates it".to_owned());
            }
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Some(format!(
                    "its update URL {url:?} is not an http:// or https:// URL"
                ));
            }
        }
        if needs_username(self.provider, self.update_url) && self.username.is_empty() {
            return Some(format!("{} needs a username", self.provider));
        }
        if needs_secret(self.provider, self.update_url) && self.secret.is_empty() {
            return Some(format!(
                "{} needs its {}",
                self.provider,
                in_a_sentence(self.provider.secret().1)
            ));
        }
        None
    }
}

/// A field's label as a word inside a sentence: its first letter lowered,
/// unless the first word is an acronym ("API token" stays as it is).
fn in_a_sentence(label: &str) -> String {
    let mut chars = label.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && !b.is_uppercase() => {
            let mut out: String = a.to_lowercase().collect();
            out.push(b);
            out.push_str(chars.as_str());
            out
        }
        _ => label.to_owned(),
    }
}

/// An HTTP method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `PATCH` (Cloudflare's record update).
    Patch,
}

impl Method {
    /// As a request line spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Patch => "PATCH",
        }
    }
}

/// One request to a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The whole URL, secret included where the provider wants it there.
    /// Never journal this: [`Request::shown`] is for that.
    pub url: String,
    /// The URL with any secret in it replaced by `[secret]`.
    pub shown: String,
    /// Headers beyond the transport's own: `User-Agent` always, and
    /// `Authorization` where the provider signs in.
    pub headers: Vec<(&'static str, String)>,
    /// A JSON body, for Cloudflare's update; empty otherwise.
    pub body: Vec<u8>,
}

/// A provider's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The HTTP status.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

impl Answer {
    /// The body as text, for reading and for showing: what is not UTF-8 is
    /// shown as `\xNN` rather than replaced, so a user can still quote it.
    #[must_use]
    pub fn text(&self) -> String {
        shown_text(&self.body)
    }
}

/// `bytes` as text: UTF-8 kept, any other byte as `\xNN`.
fn shown_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    out
}

/// What carries requests to a provider and brings the answers back.
pub trait Exchange {
    /// Send `request`; its answer, or why it could not be sent or answered
    /// ("no route to the internet", "this system cannot make HTTPS
    /// connections yet").
    ///
    /// # Errors
    ///
    /// The reason, in a phrase, when there is no answer.
    fn exchange(&mut self, request: &Request) -> Result<Answer, String>;
}

/// When asking a provider again is allowed after it refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retry {
    /// Not until the entry or its secret changes: the same update would be
    /// refused again, and providers block a client that keeps sending one.
    AfterChange,
    /// Not for this many minutes: the provider's own trouble.
    AfterMinutes(u32),
}

/// How the public address is known, for an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    /// This one.
    Known(IpAddr),
    /// The provider reads it off the update request: the update says nothing,
    /// and the provider uses the address the request came from.
    ProviderSees,
}

/// What came of asking a provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The record now holds the address (`address`, when the answer says).
    Updated {
        /// The address the provider set.
        address: Option<IpAddr>,
        /// The provider's words.
        answer: String,
    },
    /// The record already held it.
    Unchanged {
        /// The address it holds.
        address: Option<IpAddr>,
        /// The provider's words.
        answer: String,
    },
    /// A custom URL's provider took the request and said nothing this crate
    /// can read as updated or unchanged.
    Accepted {
        /// Its words.
        answer: String,
    },
    /// The provider refused.
    Refused {
        /// Its words.
        answer: String,
        /// What they mean, plainly, for a person to act on.
        meaning: String,
        /// When asking again is allowed.
        retry: Retry,
    },
    /// The request could not be carried: the transport's reason.
    Unreachable {
        /// Why.
        why: String,
    },
    /// The public address could not be learned, so no update was sent.
    NoAddress {
        /// Why.
        why: String,
    },
    /// An answer this crate cannot read: kept as it came.
    Unreadable {
        /// The answer, with its HTTP status.
        answer: String,
    },
}

impl Outcome {
    /// Whether the record is known to hold the address now.
    #[must_use]
    pub fn published(&self) -> bool {
        matches!(
            self,
            Self::Updated { .. } | Self::Unchanged { .. } | Self::Accepted { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// Building requests
// ---------------------------------------------------------------------------

/// A URL being built twice over: as sent, and as it may be shown.
struct UrlBuf {
    real: String,
    shown: String,
}

impl UrlBuf {
    fn new(base: &str) -> Self {
        Self {
            real: base.to_owned(),
            shown: base.to_owned(),
        }
    }

    /// Text that is the same in both, as it is.
    fn lit(&mut self, s: &str) {
        self.real.push_str(s);
        self.shown.push_str(s);
    }

    /// A value, percent-encoded, in both.
    fn val(&mut self, v: &str) {
        percent_encode_into(&mut self.real, v, &[]);
        percent_encode_into(&mut self.shown, v, &[]);
    }

    /// A secret: percent-encoded (keeping the bytes in `keep`) in the URL
    /// sent, blanked in the one shown.
    fn secret(&mut self, v: &str, keep: &[u8]) {
        percent_encode_into(&mut self.real, v, keep);
        self.shown.push_str("[secret]");
    }

    fn get(self, headers: Vec<(&'static str, String)>) -> Request {
        Request {
            method: Method::Get,
            url: self.real,
            shown: self.shown,
            headers,
            body: Vec::new(),
        }
    }
}

/// The headers of a request that signs in with nothing.
fn plain_headers() -> Vec<(&'static str, String)> {
    vec![("User-Agent", USER_AGENT.to_owned())]
}

/// A GET of a fixed URL that holds no secret.
fn plain_get(url: &str) -> Request {
    UrlBuf::new(url).get(plain_headers())
}

// ---------------------------------------------------------------------------
// The public address
// ---------------------------------------------------------------------------

/// No-IP's "what is my address" page: the address alone, as text.
const NOIP_CHECK: &str = "http://ip1.dynupdate.no-ip.com/";
/// Dynu's: "Current IP Address: ..." in a line of HTML.
const DYNU_CHECK: &str = "http://checkip.dynu.com/";
/// Cloudflare's trace page: `key=value` lines, the address as `ip=`.
const CLOUDFLARE_CHECK: &str = "https://www.cloudflare.com/cdn-cgi/trace";

/// Learn this network's public address the provider's way: from its own
/// "what is my address" page, or [`Address::ProviderSees`] where the provider
/// reads it off the update request (DuckDNS, FreeDNS, a custom URL that does
/// not name `{ip}`).
///
/// # Errors
///
/// The [`Outcome`] to report when it cannot be learned: the page was out of
/// reach ([`Outcome::Unreachable`]) or said no address
/// ([`Outcome::NoAddress`]), or a custom URL needs the address and nothing can
/// learn it for one yet.
pub fn public_address(target: &Target<'_>, ex: &mut dyn Exchange) -> Result<Address, Outcome> {
    let (page, read): (&str, fn(&str) -> Option<IpAddr>) = match target.provider {
        Provider::NoIp => (NOIP_CHECK, first_address),
        Provider::Dynu => (DYNU_CHECK, first_address),
        Provider::Cloudflare => (CLOUDFLARE_CHECK, trace_address),
        Provider::DuckDns | Provider::FreeDns => return Ok(Address::ProviderSees),
        Provider::Custom => {
            if target.update_url.contains("{ip}") {
                return Err(Outcome::NoAddress {
                    why: "its update URL needs this network's address, and nothing learns it for a custom \
                          provider yet (asking the router, over UPnP or NAT-PMP, is still to be written)"
                        .to_owned(),
                });
            }
            return Ok(Address::ProviderSees);
        }
    };
    let answer = ex
        .exchange(&plain_get(page))
        .map_err(|why| Outcome::Unreachable { why })?;
    let text = answer.text();
    if !(200..300).contains(&answer.status) {
        return Err(Outcome::NoAddress {
            why: format!(
                "{page} answered HTTP {}: {}",
                answer.status,
                one_line(&text)
            ),
        });
    }
    match read(&text) {
        Some(ip) => Ok(Address::Known(ip)),
        None => Err(Outcome::NoAddress {
            why: format!("{page} gave no address: {}", one_line(&text)),
        }),
    }
}

/// The first address written in `text` that is a public one: the tokens
/// between characters no address can hold, read as IPv4 or IPv6. Markup
/// around the address (Dynu's HTML) is skipped by the same rule.
fn first_address(text: &str) -> Option<IpAddr> {
    text.split(|c: char| !(c.is_ascii_hexdigit() || c == '.' || c == ':'))
        .filter_map(|t| {
            t.trim_matches(|c| c == '.' || c == ':')
                .parse::<IpAddr>()
                .ok()
        })
        .find(|ip| plausibly_public(*ip))
}

/// Cloudflare's trace: the `ip=` line.
fn trace_address(text: &str) -> Option<IpAddr> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("ip="))
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .filter(|ip| plausibly_public(*ip))
}

/// Whether an address could be where the internet reaches this network:
/// not unspecified, loopback, link-local or multicast, nor one of the
/// private ranges a router hands out. What a provider's own page reports is
/// its view of us, so a private address there is a misread, not the answer.
fn plausibly_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast())
        }
        IpAddr::V6(v6) => {
            let seg0 = v6.segments().first().copied().unwrap_or(0);
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // fe80::/10, link-local; fc00::/7, unique local.
                || seg0 & 0xffc0 == 0xfe80
                || seg0 & 0xfe00 == 0xfc00)
        }
    }
}

/// The first line of `text`, trimmed and cut at 200 characters, for quoting
/// an answer in a sentence.
fn one_line(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or("").trim();
    let mut out: String = line.chars().take(200).collect();
    if line.chars().nth(200).is_some() {
        out.push_str("...");
    }
    if out.is_empty() {
        out.push_str("(nothing)");
    }
    out
}

// ---------------------------------------------------------------------------
// The update
// ---------------------------------------------------------------------------

/// Tell the provider that `target`'s hostname is at `address`.
///
/// One request for every provider but Cloudflare, which takes up to one per
/// label of the hostname to find its zone, one to find the record, and one to
/// change it -- and none to change it when it already holds the address.
#[must_use]
pub fn publish(target: &Target<'_>, address: Address, ex: &mut dyn Exchange) -> Outcome {
    if let Some(why) = target.incomplete() {
        return Outcome::Refused {
            answer: String::new(),
            meaning: format!("not sent: {why}"),
            retry: Retry::AfterChange,
        };
    }
    match target.provider {
        Provider::NoIp => dyndns2(
            target,
            "https://dynupdate.no-ip.com/nic/update",
            address,
            ex,
        ),
        Provider::Dynu => dyndns2(target, "https://api.dynu.com/nic/update", address, ex),
        Provider::DuckDns => duckdns(target, address, ex),
        Provider::FreeDns => freedns(target, address, ex),
        Provider::Cloudflare => cloudflare(target, address, ex),
        Provider::Custom => custom(target, address, ex),
    }
}

/// Send `request` and hand back its answer, or the outcome that it had none.
fn send(ex: &mut dyn Exchange, request: &Request) -> Result<Answer, Outcome> {
    ex.exchange(request)
        .map_err(|why| Outcome::Unreachable { why })
}

/// A provider speaking dyndns2 (No-IP, Dynu): `hostname` and `myip` (or
/// `myipv6`) in the query, the account in HTTP Basic authentication, and a
/// one-word code answering.
fn dyndns2(target: &Target<'_>, base: &str, address: Address, ex: &mut dyn Exchange) -> Outcome {
    let mut url = UrlBuf::new(base);
    url.lit("?hostname=");
    url.val(target.hostname.trim());
    match address {
        Address::Known(ip @ IpAddr::V4(_)) => {
            url.lit("&myip=");
            url.val(&ip.to_string());
        }
        Address::Known(ip @ IpAddr::V6(_)) => {
            url.lit("&myipv6=");
            url.val(&ip.to_string());
        }
        Address::ProviderSees => {}
    }
    let mut headers = plain_headers();
    headers.push(("Authorization", basic_auth(target.username, target.secret)));
    let request = url.get(headers);
    match send(ex, &request) {
        Ok(answer) => read_dyndns2(&answer),
        Err(outcome) => outcome,
    }
}

/// What a dyndns2 answer means. The first word of the first line is the code,
/// whatever the HTTP status (No-IP sends `badauth` with a 401); an answer with
/// no code it knows is read by its status alone.
fn read_dyndns2(answer: &Answer) -> Outcome {
    let text = answer.text();
    let line = text.trim().lines().next().unwrap_or("").trim().to_owned();
    let mut words = line.split_whitespace();
    let code = words.next().unwrap_or("");
    let address = words.next().and_then(|w| w.parse::<IpAddr>().ok());
    let refused = |meaning: &str, retry: Retry| Outcome::Refused {
        answer: line.clone(),
        meaning: meaning.to_owned(),
        retry,
    };
    let later = Retry::AfterMinutes(SERVER_TROUBLE_MINUTES);
    match code {
        "good" => Outcome::Updated {
            address,
            answer: line,
        },
        "nochg" => Outcome::Unchanged {
            address,
            answer: line,
        },
        "badauth" => refused(
            "the username or the password was not accepted",
            Retry::AfterChange,
        ),
        "nohost" => refused("the account has no such hostname", Retry::AfterChange),
        "notfqdn" => refused(
            "the hostname is not a full name -- it needs its domain, as in myhome.example.com",
            Retry::AfterChange,
        ),
        "numhost" => refused("the request named too many hostnames", Retry::AfterChange),
        "abuse" => refused(
            "the provider has blocked this hostname for sending too many updates",
            Retry::AfterChange,
        ),
        "badagent" => refused(
            "the provider refuses this updater program; nothing in the entry can change that",
            Retry::AfterChange,
        ),
        "!donator" => refused(
            "the update asked for something only a paid account may",
            Retry::AfterChange,
        ),
        "911" | "servererror" => refused(
            "the provider is having trouble; asking again in 30 minutes",
            later,
        ),
        "dnserr" => refused(
            "the provider's DNS servers had an error; asking again in 30 minutes",
            later,
        ),
        _ => by_status(answer.status, &text),
    }
}

/// An answer no provider-specific rule recognised, by its HTTP status: a 2xx
/// is [`Outcome::Unreadable`] (it said something, but not what), a 5xx or a
/// 429 is trouble worth waiting out, and any other status a refusal.
fn by_status(status: u16, text: &str) -> Outcome {
    let shown = format!("HTTP {status}: {}", one_line(text));
    match status {
        200..=299 => Outcome::Unreadable { answer: shown },
        429 | 500..=599 => Outcome::Refused {
            answer: shown,
            meaning: "the provider is busy or having trouble; asking again in 30 minutes"
                .to_owned(),
            retry: Retry::AfterMinutes(SERVER_TROUBLE_MINUTES),
        },
        401 | 403 => Outcome::Refused {
            answer: shown,
            meaning: "the provider did not accept the sign-in".to_owned(),
            retry: Retry::AfterChange,
        },
        _ => Outcome::Refused {
            answer: shown,
            meaning: "the provider refused the update".to_owned(),
            retry: Retry::AfterChange,
        },
    }
}

/// DuckDNS's subdomain: the hostname without `.duckdns.org`, which is what its
/// `domains` parameter takes.
fn duckdns_subdomain(hostname: &str) -> &str {
    let h = hostname.trim().trim_end_matches('.');
    let suffix = ".duckdns.org";
    if h.len() > suffix.len()
        && let Some(cut) = h.len().checked_sub(suffix.len())
        && h.get(cut..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
    {
        return h.get(..cut).unwrap_or(h);
    }
    h
}

/// DuckDNS: the subdomain and the token in the query, `verbose=true` so the
/// answer says whether anything changed.
fn duckdns(target: &Target<'_>, address: Address, ex: &mut dyn Exchange) -> Outcome {
    let mut url = UrlBuf::new("https://www.duckdns.org/update?domains=");
    url.val(duckdns_subdomain(target.hostname));
    url.lit("&token=");
    url.secret(target.secret.trim(), &[]);
    match address {
        Address::Known(ip @ IpAddr::V4(_)) => {
            url.lit("&ip=");
            url.val(&ip.to_string());
        }
        Address::Known(ip @ IpAddr::V6(_)) => {
            url.lit("&ipv6=");
            url.val(&ip.to_string());
        }
        Address::ProviderSees => {}
    }
    url.lit("&verbose=true");
    let request = url.get(plain_headers());
    match send(ex, &request) {
        Ok(answer) => read_duckdns(&answer),
        Err(outcome) => outcome,
    }
}

/// DuckDNS's answer: `OK` or `KO` on its first line; with `verbose=true`,
/// then the IPv4 address, the IPv6 address and `UPDATED` or `NOCHANGE`.
fn read_duckdns(answer: &Answer) -> Outcome {
    let text = answer.text();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let words = lines
        .iter()
        .filter(|l| !l.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    match lines.first().copied() {
        Some("OK") => {
            let address = lines
                .get(1)
                .and_then(|l| l.parse::<IpAddr>().ok())
                .or_else(|| lines.get(2).and_then(|l| l.parse::<IpAddr>().ok()));
            if lines.contains(&"NOCHANGE") {
                Outcome::Unchanged {
                    address,
                    answer: words,
                }
            } else {
                Outcome::Updated {
                    address,
                    answer: words,
                }
            }
        }
        Some("KO") => Outcome::Refused {
            answer: words,
            meaning: "the token or the domain was not accepted".to_owned(),
            retry: Retry::AfterChange,
        },
        _ => by_status(answer.status, &text),
    }
}

/// FreeDNS: the "direct URL" of its dynamic DNS page,
/// `update.php?<token>`, with `address=` when the address is known.
fn freedns(target: &Target<'_>, address: Address, ex: &mut dyn Exchange) -> Outcome {
    let mut url = UrlBuf::new("https://freedns.afraid.org/dynamic/update.php?");
    // The token is the whole query, which FreeDNS reads as written: keep its
    // `=` padding unencoded.
    url.secret(target.secret.trim(), b"=");
    if let Address::Known(ip) = address {
        url.lit("&address=");
        url.val(&ip.to_string());
    }
    let request = url.get(plain_headers());
    match send(ex, &request) {
        Ok(answer) => read_freedns(&answer),
        Err(outcome) => outcome,
    }
}

/// FreeDNS answers in a sentence: `Updated 1 host(s) <name> to <address> in
/// <t> seconds`, `ERROR: Address <address> has not changed.`, or another
/// `ERROR: ...`.
fn read_freedns(answer: &Answer) -> Outcome {
    let text = answer.text();
    let line = one_line(&text);
    if line.starts_with("Updated") {
        return Outcome::Updated {
            address: first_address(&line),
            answer: line,
        };
    }
    if line.contains("has not changed") {
        return Outcome::Unchanged {
            address: first_address(&line),
            answer: line,
        };
    }
    if line.starts_with("ERROR") {
        let (meaning, retry) =
            if line.contains("Unable to locate") || line.contains("Invalid update URL") {
                (
                    "FreeDNS does not know this update token",
                    Retry::AfterChange,
                )
            } else {
                (
                    "FreeDNS refused the update; asking again in 30 minutes",
                    Retry::AfterMinutes(SERVER_TROUBLE_MINUTES),
                )
            };
        return Outcome::Refused {
            answer: line,
            meaning: meaning.to_owned(),
            retry,
        };
    }
    by_status(answer.status, &text)
}

/// A custom provider: the user's URL with `{hostname}`, `{ip}`, `{username}`
/// and `{secret}` filled in, each percent-encoded; any other `{...}` is left
/// as written.
fn custom(target: &Target<'_>, address: Address, ex: &mut dyn Exchange) -> Outcome {
    let template = target.update_url.trim();
    let ip = match address {
        Address::Known(ip) => Some(ip.to_string()),
        Address::ProviderSees => None,
    };
    if ip.is_none() && template.contains("{ip}") {
        return Outcome::NoAddress {
            why: "its update URL needs this network's address, and it was not learned".to_owned(),
        };
    }
    let mut url = UrlBuf::new("");
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        url.lit(rest.get(..open).unwrap_or(""));
        let after = rest.get(open..).unwrap_or("");
        let len = if after.starts_with("{hostname}") {
            url.val(target.hostname.trim());
            "{hostname}".len()
        } else if after.starts_with("{ip}") {
            url.val(ip.as_deref().unwrap_or(""));
            "{ip}".len()
        } else if after.starts_with("{username}") {
            url.val(target.username);
            "{username}".len()
        } else if after.starts_with("{secret}") {
            url.secret(target.secret, &[]);
            "{secret}".len()
        } else {
            url.lit("{");
            1
        };
        rest = after.get(len..).unwrap_or("");
    }
    url.lit(rest);
    let request = url.get(plain_headers());
    match send(ex, &request) {
        Ok(answer) => read_custom(&answer),
        Err(outcome) => outcome,
    }
}

/// A custom provider's answer: a dyndns2 code if it gives one (most such
/// services speak dyndns2), else its HTTP status.
fn read_custom(answer: &Answer) -> Outcome {
    let dyndns2 = read_dyndns2(answer);
    let text = answer.text();
    match dyndns2 {
        Outcome::Unreadable { .. } if (200..300).contains(&answer.status) => Outcome::Accepted {
            answer: one_line(&text),
        },
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Cloudflare
// ---------------------------------------------------------------------------

/// The zones a hostname could be in, nearest first: the name itself (an
/// apex record), then each parent with at least two labels.
fn zone_candidates(hostname: &str) -> Vec<String> {
    let host = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    let n = labels.len();
    (0..n.saturating_sub(1))
        .filter_map(|i| labels.get(i..).map(|l| l.join(".")))
        .collect()
}

/// A Cloudflare API answer read: its JSON when `success` is true, else the
/// outcome to report.
fn cloudflare_call(ex: &mut dyn Exchange, request: &Request) -> Result<json::Value, Outcome> {
    let answer = send(ex, request)?;
    let text = answer.text();
    let Ok(value) = json::parse(&text) else {
        // Not JSON: unreadable if it came with a 2xx, else read by its status.
        return Err(by_status(answer.status, &text));
    };
    if value.get("success").and_then(json::Value::as_bool) == Some(true) {
        return Ok(value);
    }
    let errors: Vec<String> = value
        .get("errors")
        .map(json::Value::items)
        .unwrap_or_default()
        .iter()
        .map(|e| {
            let code = e
                .get("code")
                .and_then(json::Value::as_number)
                .unwrap_or("?");
            let message = e.get("message").and_then(json::Value::as_str).unwrap_or("");
            format!("{code}: {message}")
        })
        .collect();
    let words = if errors.is_empty() {
        format!("HTTP {}: {}", answer.status, one_line(&text))
    } else {
        errors.join("; ")
    };
    let (meaning, retry) = match answer.status {
        401 | 403 => (
            "Cloudflare did not accept the API token for this",
            Retry::AfterChange,
        ),
        429 | 500..=599 => (
            "Cloudflare is busy or having trouble; asking again in 30 minutes",
            Retry::AfterMinutes(SERVER_TROUBLE_MINUTES),
        ),
        _ => ("Cloudflare refused the request", Retry::AfterChange),
    };
    Err(Outcome::Refused {
        answer: words,
        meaning: meaning.to_owned(),
        retry,
    })
}

/// A Cloudflare request: the bearer token, and a JSON body if any.
fn cloudflare_request(method: Method, url: String, token: &str, body: Vec<u8>) -> Request {
    let mut headers = plain_headers();
    let mut auth = String::from("Bearer ");
    auth.push_str(token.trim());
    headers.push(("Authorization", auth));
    if !body.is_empty() {
        headers.push(("Content-Type", "application/json".to_owned()));
    }
    Request {
        method,
        shown: url.clone(),
        url,
        headers,
        body,
    }
}

/// The `id` of the first element of an answer's `result` array.
fn first_result(value: &json::Value) -> Option<&json::Value> {
    value
        .get("result")
        .map(json::Value::items)
        .and_then(<[json::Value]>::first)
}

/// An id Cloudflare gave, if it is one that can go into a URL path as it is
/// (Cloudflare's are 32 hexadecimal digits): an answer is never trusted to
/// shape a later request's URL.
fn checked_id(v: Option<&json::Value>) -> Option<String> {
    let id = v?.get("id")?.as_str()?;
    (!id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| id.to_owned())
}

/// Cloudflare: find the zone holding the hostname, then its `A` (or `AAAA`)
/// record, then set the record's content to the address -- unless it holds
/// it already. The record must exist: this updates one, as every DDNS
/// client for Cloudflare does, and never creates one.
fn cloudflare(target: &Target<'_>, address: Address, ex: &mut dyn Exchange) -> Outcome {
    let Address::Known(ip) = address else {
        return Outcome::NoAddress {
            why: "Cloudflare's update needs the address, and it was not learned".to_owned(),
        };
    };
    let token = target.secret;
    let host = target
        .hostname
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let mut zone = None;
    for candidate in zone_candidates(&host) {
        let url = format!("{CLOUDFLARE_API}/zones?name={}", percent_encode(&candidate));
        match cloudflare_call(ex, &cloudflare_request(Method::Get, url, token, Vec::new())) {
            Ok(v) => {
                if let Some(id) = checked_id(first_result(&v)) {
                    zone = Some((candidate, id));
                    break;
                }
            }
            Err(outcome) => return outcome,
        }
    }
    let Some((zone_name, zone_id)) = zone else {
        return Outcome::Refused {
            answer: format!("no zone holding {host}"),
            meaning: format!("Cloudflare has no zone this API token can see that holds {host}"),
            retry: Retry::AfterChange,
        };
    };
    let kind = if ip.is_ipv4() { "A" } else { "AAAA" };
    let url = format!(
        "{CLOUDFLARE_API}/zones/{zone_id}/dns_records?type={kind}&name={}",
        percent_encode(&host)
    );
    let record = match cloudflare_call(ex, &cloudflare_request(Method::Get, url, token, Vec::new()))
    {
        Ok(v) => first_result(&v).cloned(),
        Err(outcome) => return outcome,
    };
    let Some(record) = record else {
        return Outcome::Refused {
            answer: format!("no {kind} record named {host} in {zone_name}"),
            meaning: format!(
                "the zone {zone_name} has no {kind} record named {host}; create it once in Cloudflare's dashboard"
            ),
            retry: Retry::AfterChange,
        };
    };
    let Some(record_id) = checked_id(Some(&record)) else {
        return Outcome::Unreadable {
            answer: format!("Cloudflare's {kind} record for {host} has no usable id"),
        };
    };
    let current = record
        .get("content")
        .and_then(json::Value::as_str)
        .and_then(|c| c.parse::<IpAddr>().ok());
    if current == Some(ip) {
        return Outcome::Unchanged {
            address: Some(ip),
            answer: format!("the {kind} record {host} already holds {ip}"),
        };
    }
    let mut body = String::from("{\"content\":");
    json_string_into(&mut body, &ip.to_string());
    body.push('}');
    let url = format!("{CLOUDFLARE_API}/zones/{zone_id}/dns_records/{record_id}");
    match cloudflare_call(
        ex,
        &cloudflare_request(Method::Patch, url, token, body.into_bytes()),
    ) {
        Ok(v) => {
            let set = first_result(&v)
                .or_else(|| v.get("result"))
                .and_then(|r| r.get("content"))
                .and_then(json::Value::as_str)
                .and_then(|c| c.parse::<IpAddr>().ok());
            Outcome::Updated {
                address: set.or(Some(ip)),
                answer: format!("the {kind} record {host} set to {}", set.unwrap_or(ip)),
            }
        }
        Err(outcome) => outcome,
    }
}

#[cfg(test)]
mod tests;
