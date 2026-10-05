//! Which stored login a program's question is about: a stored target -- a
//! site's address, or a name -- held against the target a program asks for.
//!
//! Domains are compared without regard to ASCII case, as DNS compares them.
//! An address's scheme (`https://`, `imap://`, any), any `user:password@`
//! before its host, its port and its path are not its domain; a target that
//! is not an address at all -- `work-vpn` -- is its own domain, matched
//! exactly.
//!
//! The scheme is not the domain, but it is not ignored either: where both
//! targets name one, a login goes only to its own scheme -- or from `http` up
//! to `https`, the same place reached more safely. Never down: a login saved
//! for `https://bank.example` does not go to `http://bank.example`, a page
//! anyone on the network between can rewrite.

/// How well a stored target matches the one asked for: higher is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchPriority {
    /// Not the same place.
    None,
    /// A stored `*.example.com` and a host under it, or `example.com` itself.
    Wildcard,
    /// A stored `example.com` and a host under it.
    ParentDomain,
    /// The same domain.
    ExactDomain,
    /// The same domain, and the stored path a prefix of the one asked for.
    ExactWithPath,
}

/// `target` without its scheme: what follows `://`, or all of it.
fn without_scheme(target: &str) -> &str {
    target.split_once("://").map_or(target, |(_, rest)| rest)
}

/// Whether a login stored for `stored` may go to `asked` as far as their
/// schemes go: either names none, both name the same, or `http` is asked for
/// over `https`.
fn schemes_agree(stored: &str, asked: &str) -> bool {
    match (stored.split_once("://"), asked.split_once("://")) {
        (Some((stored, _)), Some((asked, _))) => {
            stored.eq_ignore_ascii_case(asked)
                || (stored.eq_ignore_ascii_case("http") && asked.eq_ignore_ascii_case("https"))
        }
        _ => true,
    }
}

/// The domain of `target`: its host, without scheme, user, port or path.
fn domain(target: &str) -> &str {
    let rest = without_scheme(target);
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    host.split(':').next().unwrap_or(host)
}

/// The path of `target`: from the first `/` after its authority, or `/`.
fn path(target: &str) -> &str {
    let rest = without_scheme(target);
    rest.find('/').and_then(|at| rest.get(at..)).unwrap_or("/")
}

/// Whether `host` is `base` or a host under it, without regard to case.
fn is_under(host: &str, base: &str) -> bool {
    let (host, base) = (host.as_bytes(), base.as_bytes());
    let Some(cut) = host.len().checked_sub(base.len()) else {
        return false;
    };
    let Some(dot) = cut.checked_sub(1) else {
        return false;
    };
    !base.is_empty()
        && host
            .get(cut..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(base))
        && host.get(dot) == Some(&b'.')
}

/// How well the stored target `stored` matches `asked`, the target a
/// program asks for.
#[must_use]
pub fn match_url(stored: &str, asked: &str) -> MatchPriority {
    let stored_domain = domain(stored);
    let asked_domain = domain(asked);
    if stored_domain.is_empty() || asked_domain.is_empty() || !schemes_agree(stored, asked) {
        return MatchPriority::None;
    }
    if let Some(base) = stored_domain.strip_prefix("*.") {
        return if asked_domain.eq_ignore_ascii_case(base) || is_under(asked_domain, base) {
            MatchPriority::Wildcard
        } else {
            MatchPriority::None
        };
    }
    if stored_domain.eq_ignore_ascii_case(asked_domain) {
        let stored_path = path(stored);
        return if stored_path != "/" && path(asked).starts_with(stored_path) {
            MatchPriority::ExactWithPath
        } else {
            MatchPriority::ExactDomain
        };
    }
    if is_under(asked_domain, stored_domain) {
        return MatchPriority::ParentDomain;
    }
    MatchPriority::None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{MatchPriority, match_url};

    /// **A stored target matches the place it names, at four strengths**,
    /// and nowhere else.
    #[test]
    fn a_stored_target_matches_the_place_it_names() {
        assert_eq!(
            match_url("github.com", "https://github.com/login"),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("https://github.com/org", "https://github.com/org/repo"),
            MatchPriority::ExactWithPath
        );
        assert_eq!(
            match_url("*.example.com", "https://login.example.com/auth"),
            MatchPriority::Wildcard
        );
        assert_eq!(
            match_url("*.example.com", "example.com"),
            MatchPriority::Wildcard
        );
        assert_eq!(
            match_url("example.com", "https://sub.example.com/page"),
            MatchPriority::ParentDomain
        );
        assert_eq!(
            match_url("github.com", "https://gitlab.com/repo"),
            MatchPriority::None
        );
        // A suffix that is not under the domain is not it.
        assert_eq!(
            match_url("example.com", "evil-example.com"),
            MatchPriority::None
        );
        assert_eq!(
            match_url("*.example.com", "badexample.com"),
            MatchPriority::None
        );
    }

    /// **Domains are compared as DNS compares them**: case does not matter,
    /// in any of the four ways a target can match.
    #[test]
    fn domains_are_compared_without_case() {
        assert_eq!(
            match_url("GitHub.com", "https://github.COM/"),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("*.Example.com", "LOGIN.example.COM"),
            MatchPriority::Wildcard
        );
        assert_eq!(
            match_url("example.com", "Sub.EXAMPLE.com"),
            MatchPriority::ParentDomain
        );
    }

    /// **An address's scheme, user, port and path are not its domain**, for
    /// any scheme; a name that is no address is matched as itself.
    #[test]
    fn an_addresss_other_parts_are_not_its_domain() {
        assert_eq!(
            match_url("mail.example.com", "imap://mail.example.com:993"),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("example.com", "https://me:secret@example.com/x"),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("work-vpn", "work-vpn"),
            MatchPriority::ExactDomain
        );
        assert_eq!(match_url("work-vpn", "home-vpn"), MatchPriority::None);
        assert_eq!(match_url("", "example.com"), MatchPriority::None);
        assert_eq!(match_url("*.", "example.com"), MatchPriority::None);
    }

    /// **A login goes to its own scheme, or up from `http` to `https`** --
    /// never down, and never to another protocol; a target naming no scheme
    /// takes any.
    #[test]
    fn a_login_goes_to_its_own_scheme_or_up() {
        let bank = "https://bank.example/login";
        assert_eq!(
            match_url("https://bank.example", bank),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("HTTPS://bank.example", bank),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("http://bank.example", bank),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("https://bank.example", "http://bank.example/login"),
            MatchPriority::None
        );
        assert_eq!(
            match_url("https://mail.example", "imap://mail.example"),
            MatchPriority::None
        );
        assert_eq!(
            match_url("*.example.com", "ftp://files.example.com"),
            MatchPriority::Wildcard
        );
        assert_eq!(
            match_url("https://*.example.com", "http://files.example.com"),
            MatchPriority::None
        );
        assert_eq!(
            match_url("bank.example", "http://bank.example"),
            MatchPriority::ExactDomain
        );
        assert_eq!(
            match_url("https://bank.example", "bank.example"),
            MatchPriority::ExactDomain
        );
    }
}
