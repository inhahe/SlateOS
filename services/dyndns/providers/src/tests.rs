//! Each provider's requests, and its documented answers replayed.

// A test states what it expects by failing loudly when it is not so.
#![allow(clippy::expect_used, clippy::panic)]

use super::*;
use core::net::{Ipv4Addr, Ipv6Addr};

/// An exchange that answers from a script and keeps every request it was
/// sent: each step names the method and URL it expects, so a test fails on
/// the first request that is not the one the provider's protocol says.
struct Replay {
    script: Vec<(Method, String, Result<Answer, String>)>,
    sent: Vec<Request>,
}

impl Replay {
    fn new() -> Self {
        Self {
            script: Vec::new(),
            sent: Vec::new(),
        }
    }

    fn then(mut self, method: Method, url: &str, status: u16, body: &str) -> Self {
        self.script.push((
            method,
            url.to_owned(),
            Ok(Answer {
                status,
                body: body.as_bytes().to_vec(),
            }),
        ));
        self
    }

    fn then_fail(mut self, method: Method, url: &str, why: &str) -> Self {
        self.script
            .push((method, url.to_owned(), Err(why.to_owned())));
        self
    }

    /// Every scripted step was asked for.
    fn finished(&self) {
        assert!(
            self.script.is_empty(),
            "requests never sent: {:?}",
            self.script
        );
    }

    fn header(&self, i: usize, name: &str) -> Option<String> {
        self.sent
            .get(i)?
            .headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
    }
}

impl Exchange for Replay {
    fn exchange(&mut self, request: &Request) -> Result<Answer, String> {
        self.sent.push(request.clone());
        assert!(
            !self.script.is_empty(),
            "unexpected request: {} {}",
            request.method.as_str(),
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

const V4: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
const OLD_V4: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));
const V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x42));

fn target<'a>(
    provider: Provider,
    hostname: &'a str,
    username: &'a str,
    secret: &'a str,
) -> Target<'a> {
    Target {
        provider,
        hostname,
        username,
        secret,
        update_url: "",
    }
}

fn custom<'a>(update_url: &'a str, secret: &'a str) -> Target<'a> {
    Target {
        provider: Provider::Custom,
        hostname: "home.example.com",
        username: "me",
        secret,
        update_url,
    }
}

// ---------------------------------------------------------------------------
// The providers and their fields
// ---------------------------------------------------------------------------

#[test]
fn every_provider_has_one_key_and_reads_back_from_it() {
    let keys: Vec<&str> = Provider::ALL.iter().map(|p| p.key()).collect();
    assert_eq!(
        keys,
        ["dynu", "noip", "duckdns", "cloudflare", "freedns", "custom"]
    );
    for p in Provider::ALL {
        assert_eq!(Provider::from_key(p.key()), Some(p));
        assert_eq!(Provider::from_key(&p.key().to_uppercase()), Some(p));
        assert!(!p.label().is_empty() && !p.hostname_example().is_empty());
    }
    assert_eq!(Provider::from_key(" noip "), Some(Provider::NoIp));
    assert_eq!(Provider::from_key("no-ip"), None);
    assert_eq!(Provider::from_key(""), None);
    assert_eq!(Provider::NoIp.to_string(), "No-IP");
}

#[test]
fn each_provider_asks_for_its_own_fields() {
    assert_eq!(Provider::NoIp.username().0, Need::Required);
    assert_eq!(Provider::Dynu.username().0, Need::Required);
    for p in [Provider::DuckDns, Provider::Cloudflare, Provider::FreeDns] {
        assert_eq!(p.username().0, Need::Unused, "{p}");
        assert_eq!(p.secret().0, Need::Required, "{p}");
    }
    assert_eq!(Provider::Custom.username().0, Need::Optional);
    assert_eq!(Provider::Custom.secret().0, Need::Optional);
    assert!(Provider::Custom.takes_update_url());
    assert!(!Provider::NoIp.takes_update_url());
    assert_eq!(Provider::DuckDns.secret().1, "Token");
}

#[test]
fn a_custom_url_needs_what_it_names() {
    assert!(needs_secret(Provider::Custom, "https://x/u?k={secret}"));
    assert!(!needs_secret(Provider::Custom, "https://x/u?k=fixed"));
    assert!(needs_username(Provider::Custom, "https://x/u?u={username}"));
    assert!(!needs_username(Provider::Custom, "https://x/u"));
    assert!(needs_secret(Provider::DuckDns, ""));
    assert!(!needs_username(Provider::DuckDns, "{username}"));
}

#[test]
fn an_incomplete_entry_is_told_what_it_lacks() {
    assert_eq!(
        target(Provider::DuckDns, "myhome", "", "t").incomplete(),
        None
    );
    assert_eq!(
        target(Provider::DuckDns, " ", "", "t")
            .incomplete()
            .as_deref(),
        Some("it has no hostname")
    );
    assert!(
        target(Provider::DuckDns, "my home", "", "t")
            .incomplete()
            .is_some_and(|w| w.contains("not a hostname"))
    );
    assert_eq!(
        target(Provider::NoIp, "h.ddns.net", "", "pw")
            .incomplete()
            .as_deref(),
        Some("No-IP needs a username")
    );
    assert_eq!(
        target(Provider::Cloudflare, "h.example.com", "", "")
            .incomplete()
            .as_deref(),
        Some("Cloudflare needs its API token")
    );
    assert_eq!(
        custom("", "").incomplete().as_deref(),
        Some("a custom entry needs the URL that updates it")
    );
    assert!(
        custom("ftp://x/{ip}", "")
            .incomplete()
            .is_some_and(|w| w.contains("not an http"))
    );
    assert_eq!(
        custom("https://x/u?k={secret}", "").incomplete().as_deref(),
        Some("Custom needs its secret")
    );
    assert_eq!(custom("https://x/u", "").incomplete(), None);
    // A custom URL without {hostname} needs none.
    let no_host = Target {
        hostname: "",
        ..custom("https://x/u", "")
    };
    assert_eq!(no_host.incomplete(), None);

    // publish sends nothing for one.
    let mut ex = Replay::new();
    let out = publish(
        &target(Provider::NoIp, "h.ddns.net", "", "pw"),
        Address::Known(V4),
        &mut ex,
    );
    assert_eq!(
        out,
        Outcome::Refused {
            answer: String::new(),
            meaning: "not sent: No-IP needs a username".to_owned(),
            retry: Retry::AfterChange,
        }
    );
    assert!(ex.sent.is_empty());
}

// ---------------------------------------------------------------------------
// The public address
// ---------------------------------------------------------------------------

#[test]
fn the_address_comes_from_the_providers_own_page() {
    let mut ex = Replay::new().then(
        Method::Get,
        "http://ip1.dynupdate.no-ip.com/",
        200,
        "203.0.113.7\n",
    );
    let t = target(Provider::NoIp, "h.ddns.net", "u", "p");
    assert_eq!(public_address(&t, &mut ex), Ok(Address::Known(V4)));
    ex.finished();
    assert_eq!(ex.header(0, "User-Agent").as_deref(), Some(USER_AGENT));
    assert_eq!(
        ex.header(0, "Authorization"),
        None,
        "the check signs in with nothing"
    );

    let mut ex = Replay::new().then(
        Method::Get,
        "http://checkip.dynu.com/",
        200,
        "<html><head><title>Current IP Check</title></head><body>Current IP Address: 203.0.113.7</body></html>",
    );
    let t = target(Provider::Dynu, "h.dynu.net", "u", "p");
    assert_eq!(public_address(&t, &mut ex), Ok(Address::Known(V4)));
    ex.finished();

    let mut ex = Replay::new().then(
        Method::Get,
        "https://www.cloudflare.com/cdn-cgi/trace",
        200,
        "fl=123abc\nh=www.cloudflare.com\nip=2001:db8::42\nts=1700000000.0\nvisit_scheme=https\n",
    );
    let t = target(Provider::Cloudflare, "h.example.com", "", "tok");
    assert_eq!(public_address(&t, &mut ex), Ok(Address::Known(V6)));
    ex.finished();
}

#[test]
fn duckdns_freedns_and_a_url_without_ip_let_the_provider_see_it() {
    for t in [
        target(Provider::DuckDns, "myhome", "", "t"),
        target(Provider::FreeDns, "x.mooo.com", "", "t"),
        custom("https://x/u?h={hostname}", ""),
    ] {
        let mut ex = Replay::new();
        assert_eq!(public_address(&t, &mut ex), Ok(Address::ProviderSees));
        assert!(ex.sent.is_empty(), "nothing is asked of anyone");
    }
    let mut ex = Replay::new();
    assert!(matches!(
        public_address(&custom("https://x/u?ip={ip}", ""), &mut ex),
        Err(Outcome::NoAddress { .. })
    ));
    assert!(ex.sent.is_empty());
}

#[test]
fn only_a_custom_url_naming_the_address_needs_the_router() {
    assert!(needs_router_address(
        Provider::Custom,
        "https://x/u?ip={ip}"
    ));
    assert!(!needs_router_address(
        Provider::Custom,
        "https://x/u?h={hostname}"
    ));
    for p in Provider::ALL {
        if p != Provider::Custom {
            assert!(!needs_router_address(p, "https://x/u?ip={ip}"), "{p:?}");
        }
    }
}

#[test]
fn a_page_that_gives_no_public_address_gives_none() {
    let t = target(Provider::NoIp, "h.ddns.net", "u", "p");
    for (status, body) in [
        (200, ""),
        (200, "nothing here"),
        (200, "192.168.1.20"),
        (200, "127.0.0.1"),
        (200, "0.0.0.0"),
        (503, "203.0.113.7"),
    ] {
        let mut ex =
            Replay::new().then(Method::Get, "http://ip1.dynupdate.no-ip.com/", status, body);
        let got = public_address(&t, &mut ex);
        assert!(
            matches!(got, Err(Outcome::NoAddress { .. })),
            "{status} {body:?}: {got:?}"
        );
    }
    let mut ex = Replay::new().then_fail(
        Method::Get,
        "http://ip1.dynupdate.no-ip.com/",
        "no route to the internet",
    );
    assert_eq!(
        public_address(&t, &mut ex),
        Err(Outcome::Unreachable {
            why: "no route to the internet".to_owned()
        })
    );
    // A private address in Cloudflare's trace is a misread too.
    let mut ex = Replay::new().then(
        Method::Get,
        "https://www.cloudflare.com/cdn-cgi/trace",
        200,
        "ip=10.1.2.3\n",
    );
    let t = target(Provider::Cloudflare, "h.example.com", "", "tok");
    assert!(matches!(
        public_address(&t, &mut ex),
        Err(Outcome::NoAddress { .. })
    ));
}

#[test]
fn private_and_local_addresses_are_not_public() {
    for s in [
        "10.0.0.1",
        "172.16.5.4",
        "192.168.0.1",
        "169.254.1.1",
        "224.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "fe80::1",
        "fd00::1",
        "ff02::1",
    ] {
        let ip: IpAddr = s.parse().expect("an address");
        assert!(!plausibly_public(ip), "{s}");
    }
    for s in ["203.0.113.7", "8.8.8.8", "2001:db8::42", "2606:4700::1111"] {
        let ip: IpAddr = s.parse().expect("an address");
        assert!(plausibly_public(ip), "{s}");
    }
}

// ---------------------------------------------------------------------------
// dyndns2: No-IP and Dynu
// ---------------------------------------------------------------------------

#[test]
fn noip_is_dyndns2_signed_in_with_basic_authentication() {
    let t = target(
        Provider::NoIp,
        "myhome.ddns.net",
        "me@example.com",
        "pa:ss word",
    );
    let mut ex = Replay::new().then(
        Method::Get,
        "https://dynupdate.no-ip.com/nic/update?hostname=myhome.ddns.net&myip=203.0.113.7",
        200,
        "good 203.0.113.7\r\n",
    );
    assert_eq!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Updated {
            address: Some(V4),
            answer: "good 203.0.113.7".to_owned()
        }
    );
    ex.finished();
    assert_eq!(
        ex.header(0, "Authorization"),
        Some(encode::basic_auth("me@example.com", "pa:ss word"))
    );
    assert_eq!(ex.header(0, "User-Agent").as_deref(), Some(USER_AGENT));
    // The password is in a header, not the URL: what is shown is the URL.
    assert_eq!(
        ex.sent.first().map(|r| r.shown.as_str()),
        Some("https://dynupdate.no-ip.com/nic/update?hostname=myhome.ddns.net&myip=203.0.113.7")
    );
}

#[test]
fn dynu_is_dyndns2_at_its_own_address_and_ipv6_goes_as_myipv6() {
    let t = target(Provider::Dynu, "myhome.dynu.net", "me", "pw");
    let mut ex = Replay::new().then(
        Method::Get,
        "https://api.dynu.com/nic/update?hostname=myhome.dynu.net&myipv6=2001%3Adb8%3A%3A42",
        200,
        "nochg 2001:db8::42",
    );
    assert_eq!(
        publish(&t, Address::Known(V6), &mut ex),
        Outcome::Unchanged {
            address: Some(V6),
            answer: "nochg 2001:db8::42".to_owned()
        }
    );
    ex.finished();

    let mut ex = Replay::new().then(
        Method::Get,
        "https://api.dynu.com/nic/update?hostname=myhome.dynu.net",
        200,
        "good",
    );
    assert_eq!(
        publish(&t, Address::ProviderSees, &mut ex),
        Outcome::Updated {
            address: None,
            answer: "good".to_owned()
        }
    );
    ex.finished();
}

#[test]
fn every_dyndns2_code_means_what_the_protocol_says() {
    let t = target(Provider::NoIp, "myhome.ddns.net", "me", "pw");
    let url = "https://dynupdate.no-ip.com/nic/update?hostname=myhome.ddns.net&myip=203.0.113.7";
    let stop = Retry::AfterChange;
    let wait = Retry::AfterMinutes(30);
    for (status, body, retry, meaning_has) in [
        (401, "badauth", stop, "password"),
        (200, "nohost", stop, "no such hostname"),
        (200, "notfqdn", stop, "full name"),
        (200, "numhost", stop, "too many"),
        (200, "abuse", stop, "blocked"),
        (200, "badagent", stop, "updater program"),
        (200, "!donator", stop, "paid account"),
        (200, "911", wait, "30 minutes"),
        (200, "dnserr", wait, "30 minutes"),
        (200, "servererror", wait, "30 minutes"),
    ] {
        let mut ex = Replay::new().then(Method::Get, url, status, body);
        match publish(&t, Address::Known(V4), &mut ex) {
            Outcome::Refused {
                answer,
                meaning,
                retry: r,
            } => {
                assert_eq!(answer, body, "the provider's own words are kept");
                assert_eq!(r, retry, "{body}");
                assert!(meaning.contains(meaning_has), "{body}: {meaning}");
            }
            other => panic!("{body}: {other:?}"),
        }
        ex.finished();
    }
    // No code: by the status.
    for (status, body, want_unreadable) in [
        (200, "hello", true),
        (500, "oops", false),
        (404, "missing", false),
    ] {
        let mut ex = Replay::new().then(Method::Get, url, status, body);
        let got = publish(&t, Address::Known(V4), &mut ex);
        assert_eq!(
            matches!(got, Outcome::Unreadable { .. }),
            want_unreadable,
            "{status} {body}: {got:?}"
        );
    }
    let mut ex = Replay::new().then(Method::Get, url, 503, "busy");
    assert!(matches!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Refused {
            retry: Retry::AfterMinutes(30),
            ..
        }
    ));
    let mut ex = Replay::new().then_fail(
        Method::Get,
        url,
        "this system cannot make HTTPS connections yet",
    );
    assert_eq!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Unreachable {
            why: "this system cannot make HTTPS connections yet".to_owned()
        }
    );
}

#[test]
fn a_hostname_cannot_change_the_shape_of_the_url() {
    let t = target(Provider::NoIp, "a&myip=6.6.6.6", "me", "pw");
    let mut ex = Replay::new().then(
        Method::Get,
        "https://dynupdate.no-ip.com/nic/update?hostname=a%26myip%3D6.6.6.6&myip=203.0.113.7",
        200,
        "nohost",
    );
    assert!(matches!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Refused { .. }
    ));
    ex.finished();
}

// ---------------------------------------------------------------------------
// DuckDNS
// ---------------------------------------------------------------------------

#[test]
fn duckdns_takes_the_subdomain_and_the_token_and_answers_verbosely() {
    let t = target(
        Provider::DuckDns,
        "MyHome.DuckDNS.org",
        "",
        "a7c4d0ad-114e-40ef-ba1d-d217904a50f2",
    );
    let url = "https://www.duckdns.org/update?domains=MyHome&token=a7c4d0ad-114e-40ef-ba1d-d217904a50f2&ip=203.0.113.7&verbose=true";
    let mut ex = Replay::new().then(Method::Get, url, 200, "OK\n203.0.113.7\n\nUPDATED");
    assert_eq!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Updated {
            address: Some(V4),
            answer: "OK 203.0.113.7 UPDATED".to_owned()
        }
    );
    ex.finished();
    let shown = ex.sent.first().map(|r| r.shown.clone()).unwrap_or_default();
    assert_eq!(
        shown,
        "https://www.duckdns.org/update?domains=MyHome&token=[secret]&ip=203.0.113.7&verbose=true"
    );

    let mut ex = Replay::new().then(Method::Get, url, 200, "OK\n203.0.113.7\n\nNOCHANGE");
    assert!(matches!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Unchanged { address: Some(a), .. } if a == V4
    ));

    let mut ex = Replay::new().then(Method::Get, url, 200, "KO");
    assert_eq!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Refused {
            answer: "KO".to_owned(),
            meaning: "the token or the domain was not accepted".to_owned(),
            retry: Retry::AfterChange
        }
    );

    // The provider sees the address: none is sent.
    let plain = target(Provider::DuckDns, "myhome", "", "tok");
    let mut ex = Replay::new().then(
        Method::Get,
        "https://www.duckdns.org/update?domains=myhome&token=tok&verbose=true",
        200,
        "OK\n198.51.100.4\n2001:db8::42\nUPDATED\n",
    );
    assert_eq!(
        publish(&plain, Address::ProviderSees, &mut ex),
        Outcome::Updated {
            address: Some(OLD_V4),
            answer: "OK 198.51.100.4 2001:db8::42 UPDATED".to_owned()
        }
    );
    ex.finished();

    let mut ex = Replay::new().then(Method::Get, "https://www.duckdns.org/update?domains=myhome&token=tok&ipv6=2001%3Adb8%3A%3A42&verbose=true", 200, "OK");
    assert!(matches!(
        publish(&plain, Address::Known(V6), &mut ex),
        Outcome::Updated { .. }
    ));
    ex.finished();
}

// ---------------------------------------------------------------------------
// FreeDNS
// ---------------------------------------------------------------------------

#[test]
fn freedns_is_its_direct_url_with_the_token_as_written() {
    let t = target(
        Provider::FreeDns,
        "myhome.mooo.com",
        "",
        "U1hJaVNZM3B4VzpwbGFpbg==",
    );
    let url = "https://freedns.afraid.org/dynamic/update.php?U1hJaVNZM3B4VzpwbGFpbg==&address=203.0.113.7";
    let mut ex = Replay::new().then(
        Method::Get,
        url,
        200,
        "Updated 1 host(s) myhome.mooo.com to 203.0.113.7 in 0.139 seconds\n",
    );
    assert_eq!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Updated {
            address: Some(V4),
            answer: "Updated 1 host(s) myhome.mooo.com to 203.0.113.7 in 0.139 seconds".to_owned()
        }
    );
    assert_eq!(
        ex.sent.first().map(|r| r.shown.as_str()),
        Some("https://freedns.afraid.org/dynamic/update.php?[secret]&address=203.0.113.7")
    );

    let mut ex = Replay::new().then(
        Method::Get,
        url,
        200,
        "ERROR: Address 203.0.113.7 has not changed.",
    );
    assert!(
        matches!(publish(&t, Address::Known(V4), &mut ex), Outcome::Unchanged { address: Some(a), .. } if a == V4)
    );

    let mut ex = Replay::new().then(
        Method::Get,
        url,
        200,
        "ERROR: Unable to locate this record (changed password recently? deleted and re-created this dns entry?)",
    );
    assert!(matches!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Refused {
            retry: Retry::AfterChange,
            ..
        }
    ));

    let mut ex = Replay::new().then(Method::Get, url, 200, "ERROR: something else went wrong");
    assert!(matches!(
        publish(&t, Address::Known(V4), &mut ex),
        Outcome::Refused {
            retry: Retry::AfterMinutes(30),
            ..
        }
    ));

    // The provider sees the address.
    let mut ex = Replay::new().then(
        Method::Get,
        "https://freedns.afraid.org/dynamic/update.php?U1hJaVNZM3B4VzpwbGFpbg==",
        200,
        "Updated 1 host(s) myhome.mooo.com to 198.51.100.4 in 0.2 seconds",
    );
    assert!(
        matches!(publish(&t, Address::ProviderSees, &mut ex), Outcome::Updated { address: Some(a), .. } if a == OLD_V4)
    );
    ex.finished();
}

// ---------------------------------------------------------------------------
// Custom
// ---------------------------------------------------------------------------

#[test]
fn a_custom_url_is_filled_in_and_encoded() {
    let t = Target {
        provider: Provider::Custom,
        hostname: "home.example.com",
        username: "me&you",
        secret: "s3cr/t",
        update_url: "https://dns.example.net/update/{hostname}?u={username}&k={secret}&ip={ip}&x={other}",
    };
    let mut ex = Replay::new().then(
        Method::Get,
        "https://dns.example.net/update/home.example.com?u=me%26you&k=s3cr%2Ft&ip=203.0.113.7&x={other}",
        200,
        "good 203.0.113.7",
    );
    assert!(
        matches!(publish(&t, Address::Known(V4), &mut ex), Outcome::Updated { address: Some(a), .. } if a == V4)
    );
    assert_eq!(
        ex.sent.first().map(|r| r.shown.as_str()),
        Some(
            "https://dns.example.net/update/home.example.com?u=me%26you&k=[secret]&ip=203.0.113.7&x={other}"
        )
    );
    ex.finished();
}

#[test]
fn a_custom_providers_answer_is_read_as_dyndns2_or_by_its_status() {
    let t = custom("https://dns.example.net/u?h={hostname}", "");
    let url = "https://dns.example.net/u?h=home.example.com";
    let mut ex = Replay::new().then(Method::Get, url, 200, "Record updated, thanks!");
    assert_eq!(
        publish(&t, Address::ProviderSees, &mut ex),
        Outcome::Accepted {
            answer: "Record updated, thanks!".to_owned()
        }
    );
    let mut ex = Replay::new().then(Method::Get, url, 200, "badauth");
    assert!(matches!(
        publish(&t, Address::ProviderSees, &mut ex),
        Outcome::Refused {
            retry: Retry::AfterChange,
            ..
        }
    ));
    let mut ex = Replay::new().then(Method::Get, url, 403, "Forbidden");
    assert!(matches!(
        publish(&t, Address::ProviderSees, &mut ex),
        Outcome::Refused { retry: Retry::AfterChange, meaning, .. } if meaning.contains("sign-in")
    ));
    let mut ex = Replay::new().then(Method::Get, url, 502, "Bad Gateway");
    assert!(matches!(
        publish(&t, Address::ProviderSees, &mut ex),
        Outcome::Refused {
            retry: Retry::AfterMinutes(30),
            ..
        }
    ));
    // {ip} without an address: nothing is sent.
    let needs_ip = custom("https://dns.example.net/u?ip={ip}", "");
    let mut ex = Replay::new();
    assert!(matches!(
        publish(&needs_ip, Address::ProviderSees, &mut ex),
        Outcome::NoAddress { .. }
    ));
    assert!(ex.sent.is_empty());
}

// ---------------------------------------------------------------------------
// Cloudflare
// ---------------------------------------------------------------------------

const ZONE_ID: &str = "023e105f4ecef8ad9ca31a8372d0c353";
const RECORD_ID: &str = "372e67954025e0ba6aaa6d586b9e0b59";

fn cf_zone(found: bool) -> String {
    if found {
        format!(
            r#"{{"result":[{{"id":"{ZONE_ID}","name":"example.com","status":"active"}}],"success":true,"errors":[],"messages":[]}}"#
        )
    } else {
        r#"{"result":[],"success":true,"errors":[],"messages":[]}"#.to_owned()
    }
}

fn cf_record(content: &str) -> String {
    format!(
        r#"{{"result":[{{"id":"{RECORD_ID}","zone_id":"{ZONE_ID}","name":"home.example.com","type":"A","content":"{content}","proxied":false,"ttl":1}}],"success":true,"errors":[],"messages":[]}}"#
    )
}

fn cf_target() -> Target<'static> {
    target(Provider::Cloudflare, "Home.Example.com.", "", "cf-token")
}

fn zone_url(name: &str) -> String {
    format!("https://api.cloudflare.com/client/v4/zones?name={name}")
}

fn records_url(kind: &str) -> String {
    format!(
        "https://api.cloudflare.com/client/v4/zones/{ZONE_ID}/dns_records?type={kind}&name=home.example.com"
    )
}

fn record_url() -> String {
    format!("https://api.cloudflare.com/client/v4/zones/{ZONE_ID}/dns_records/{RECORD_ID}")
}

#[test]
fn cloudflare_finds_the_zone_then_the_record_then_changes_it() {
    let mut ex = Replay::new()
        .then(Method::Get, &zone_url("home.example.com"), 200, &cf_zone(false))
        .then(Method::Get, &zone_url("example.com"), 200, &cf_zone(true))
        .then(Method::Get, &records_url("A"), 200, &cf_record("198.51.100.4"))
        .then(
            Method::Patch,
            &record_url(),
            200,
            &format!(r#"{{"result":{{"id":"{RECORD_ID}","type":"A","name":"home.example.com","content":"203.0.113.7"}},"success":true,"errors":[],"messages":[]}}"#),
        );
    assert_eq!(
        publish(&cf_target(), Address::Known(V4), &mut ex),
        Outcome::Updated {
            address: Some(V4),
            answer: "the A record home.example.com set to 203.0.113.7".to_owned()
        }
    );
    ex.finished();
    for i in 0..4 {
        assert_eq!(
            ex.header(i, "Authorization").as_deref(),
            Some("Bearer cf-token"),
            "request {i}"
        );
    }
    assert_eq!(
        ex.header(3, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(ex.header(0, "Content-Type"), None);
    assert_eq!(
        ex.sent.get(3).map(|r| r.body.clone()),
        Some(b"{\"content\":\"203.0.113.7\"}".to_vec())
    );
    assert!(
        ex.sent
            .iter()
            .all(|r| !r.shown.contains("cf-token") && !r.url.contains("cf-token"))
    );
}

#[test]
fn cloudflare_leaves_a_record_that_already_holds_the_address() {
    let mut ex = Replay::new()
        .then(
            Method::Get,
            &zone_url("home.example.com"),
            200,
            &cf_zone(true),
        )
        .then(
            Method::Get,
            &records_url("AAAA"),
            200,
            &cf_record("2001:db8::42"),
        );
    assert_eq!(
        publish(&cf_target(), Address::Known(V6), &mut ex),
        Outcome::Unchanged {
            address: Some(V6),
            answer: "the AAAA record home.example.com already holds 2001:db8::42".to_owned()
        }
    );
    ex.finished();
}

#[test]
fn cloudflare_without_the_zone_or_the_record_says_which() {
    let mut ex = Replay::new()
        .then(
            Method::Get,
            &zone_url("home.example.com"),
            200,
            &cf_zone(false),
        )
        .then(Method::Get, &zone_url("example.com"), 200, &cf_zone(false));
    match publish(&cf_target(), Address::Known(V4), &mut ex) {
        Outcome::Refused { meaning, retry, .. } => {
            assert!(meaning.contains("no zone"), "{meaning}");
            assert_eq!(retry, Retry::AfterChange);
        }
        other => panic!("{other:?}"),
    }
    ex.finished();

    let mut ex = Replay::new()
        .then(
            Method::Get,
            &zone_url("home.example.com"),
            200,
            &cf_zone(true),
        )
        .then(
            Method::Get,
            &records_url("A"),
            200,
            r#"{"result":[],"success":true,"errors":[],"messages":[]}"#,
        );
    match publish(&cf_target(), Address::Known(V4), &mut ex) {
        Outcome::Refused { meaning, retry, .. } => {
            assert!(
                meaning.contains("no A record named home.example.com"),
                "{meaning}"
            );
            assert_eq!(retry, Retry::AfterChange);
        }
        other => panic!("{other:?}"),
    }
    ex.finished();
}

#[test]
fn cloudflare_errors_are_reported_in_its_words() {
    let mut ex = Replay::new().then(
        Method::Get,
        &zone_url("home.example.com"),
        403,
        r#"{"success":false,"errors":[{"code":9109,"message":"Invalid access token"}],"messages":[],"result":null}"#,
    );
    assert_eq!(
        publish(&cf_target(), Address::Known(V4), &mut ex),
        Outcome::Refused {
            answer: "9109: Invalid access token".to_owned(),
            meaning: "Cloudflare did not accept the API token for this".to_owned(),
            retry: Retry::AfterChange
        }
    );
    let mut ex = Replay::new().then(
        Method::Get,
        &zone_url("home.example.com"),
        500,
        "<html>oops</html>",
    );
    assert!(matches!(
        publish(&cf_target(), Address::Known(V4), &mut ex),
        Outcome::Refused {
            retry: Retry::AfterMinutes(30),
            ..
        }
    ));
    let mut ex = Replay::new().then(
        Method::Get,
        &zone_url("home.example.com"),
        200,
        "<html>not JSON</html>",
    );
    assert!(matches!(
        publish(&cf_target(), Address::Known(V4), &mut ex),
        Outcome::Unreadable { .. }
    ));
    // An address is needed: the provider cannot see it through the API.
    let mut ex = Replay::new();
    assert!(matches!(
        publish(&cf_target(), Address::ProviderSees, &mut ex),
        Outcome::NoAddress { .. }
    ));
    assert!(ex.sent.is_empty());
}

#[test]
fn an_id_from_an_answer_never_shapes_a_url() {
    let hostile =
        r#"{"result":[{"id":"../../accounts","name":"example.com"}],"success":true,"errors":[]}"#;
    let mut ex = Replay::new()
        .then(Method::Get, &zone_url("home.example.com"), 200, hostile)
        .then(Method::Get, &zone_url("example.com"), 200, hostile);
    assert!(matches!(
        publish(&cf_target(), Address::Known(V4), &mut ex),
        Outcome::Refused { .. }
    ));
    ex.finished();
}

#[test]
fn zones_are_tried_nearest_first() {
    assert_eq!(
        zone_candidates("a.b.Example.co.uk."),
        [
            "a.b.example.co.uk",
            "b.example.co.uk",
            "example.co.uk",
            "co.uk"
        ]
    );
    assert_eq!(zone_candidates("example.com"), ["example.com"]);
    assert!(zone_candidates("localhost").is_empty());
}

// ---------------------------------------------------------------------------
// Secrets and answers as shown
// ---------------------------------------------------------------------------

/// Whatever the provider and wherever the secret goes, the URL shown never
/// holds it, in any spelling.
#[test]
fn no_shown_url_holds_its_secret() {
    for secret in ["plainsecret", "with space&amp=", "s3cr/t%41"] {
        let encoded = encode::percent_encode(secret);
        for t in [
            target(Provider::NoIp, "h.ddns.net", "me", secret),
            target(Provider::Dynu, "h.dynu.net", "me", secret),
            target(Provider::DuckDns, "h", "", secret),
            target(Provider::FreeDns, "h.mooo.com", "", secret),
            Target {
                provider: Provider::Custom,
                hostname: "h",
                username: "me",
                secret,
                update_url: "https://x.example/u?k={secret}&again={secret}",
            },
        ] {
            let mut ex = Recorder::default();
            let _ = publish(&t, Address::Known(V4), &mut ex);
            let shown = ex.0.first().map(|r| r.shown.clone()).unwrap_or_default();
            assert!(!shown.is_empty(), "{}", t.provider);
            assert!(
                !shown.contains(secret) && !shown.contains(&encoded),
                "{}: {shown}",
                t.provider
            );
        }
    }
}

/// An exchange that keeps what it is sent and answers nothing.
#[derive(Default)]
struct Recorder(Vec<Request>);

impl Exchange for Recorder {
    fn exchange(&mut self, request: &Request) -> Result<Answer, String> {
        self.0.push(request.clone());
        Err("not sent".to_owned())
    }
}

#[test]
fn an_answer_is_shown_as_it_came() {
    let a = Answer {
        status: 200,
        body: b"ok \xff\xfe done \xc3\xa9".to_vec(),
    };
    assert_eq!(a.text(), "ok \\xFF\\xFE done é");
    assert_eq!(one_line("\n\n  first  \nsecond"), "first");
    assert_eq!(one_line(""), "(nothing)");
    let long = "x".repeat(250);
    assert_eq!(one_line(&long).len(), 203);
    assert!(
        Outcome::Accepted {
            answer: String::new()
        }
        .published()
    );
    assert!(!Outcome::NoAddress { why: String::new() }.published());
}

#[test]
fn a_label_reads_as_a_word_in_a_sentence() {
    assert_eq!(in_a_sentence("Password"), "password");
    assert_eq!(in_a_sentence("Update token"), "update token");
    assert_eq!(in_a_sentence("API token"), "API token");
    assert_eq!(in_a_sentence("X"), "X");
    assert_eq!(in_a_sentence(""), "");
}
