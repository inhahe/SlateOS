//! Reading the dynamic-DNS entries the kernel actually has.
//!
//! `fs::snapshot`'s page taught this lesson earlier the same day and this is
//! the second application of it: the kernel implements dynamic DNS
//! (`kernel/src/fs/dyndns.rs` — `add_entry`, `list_entries`, `update_now`,
//! plus UPnP/NAT-PMP forwarding) and publishes the result at `/proc/dyndns`.
//! Its own module doc names the caller it expects:
//!
//! ```text
//! Settings panel → Network → Dynamic DNS
//!   → dyndns::list_providers() → configured providers
//! ```
//!
//! This is that panel's half of the arrangement. What it replaces was an
//! in-memory model of the same idea that nothing could reach — see
//! `known-issues.md`
//! `TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING`.

use std::fmt::Write as _;

/// Where the kernel publishes the dynamic-DNS state.
pub const PROC_DYNDNS: &str = "/proc/dyndns";

/// The providers the kernel knows, spelled as `{:?}` writes them.
///
/// This is not decoration: it is the parser's anchor. See
/// [`parse_proc_dyndns`].
const PROVIDERS: [&str; 6] = ["Dynu", "NoIp", "DuckDns", "Cloudflare", "FreeDns", "Custom"];

/// The update states the kernel reports, spelled as `{:?}` writes them.
const STATUSES: [&str; 5] = ["Idle", "Updating", "Success", "Failed", "NoChange"];

/// One dynamic-DNS entry, as `/proc/dyndns` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynDnsRow {
    /// The kernel's id, which is what an update or a delete names.
    pub id: u64,
    /// The name the user gave this entry. May contain spaces.
    pub name: String,
    /// The provider, e.g. `Dynu`.
    pub provider: String,
    /// The hostname being kept up to date.
    pub hostname: String,
    /// The last update's outcome, e.g. `Success`.
    pub status: String,
    /// The address last published, or `-` if none has been.
    pub last_ip: String,
}

/// What the router half of the page shows, when a router was found.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DynDnsSummary {
    /// How many entries are configured.
    pub entries: usize,
    /// How many port forwards are in place.
    pub forwards: usize,
    /// Whether a router was detected at all.
    pub router_detected: bool,
    /// The router's address, empty if none was found.
    pub router_ip: String,
    /// The router's model string, empty if none was found.
    pub router_model: String,
    /// The address the router reports on its outside interface — the value the
    /// entries below are supposed to be publishing, and so the one fact on this
    /// page a user is most likely to be checking.
    pub external_ip: String,
    /// Whether the router offers UPnP port forwarding.
    pub upnp: bool,
    /// Whether the router offers NAT-PMP port forwarding.
    pub natpmp: bool,
}

/// Parse the body of `/proc/dyndns`.
///
/// # Why this anchors on the enum spellings
///
/// The row is `{:<4} {:<15} {:<10} {:<25} {:<10} {}` — ID, NAME, PROVIDER,
/// HOSTNAME, STATUS, IP — with **single**-space column separators, and the
/// widths are Rust minimums that do not truncate. Two of those fields are
/// user-supplied and unescaped: a name or a hostname containing a space is
/// indistinguishable from a column break, and either one longer than its column
/// shifts everything after it. (Reported to lane A in
/// `requests/c-a-proc-snapshots-escapes-the-path-but-not-the-name.md`.)
///
/// So neither column positions nor counting tokens from the ends is safe here —
/// the snapshots reader could count from both ends because only *one* of its
/// fields was ambiguous. Two are, and they sit either side of a field that is
/// not: `PROVIDER` and `STATUS` are `{:?}` of closed enums, so they can only be
/// one of a handful of known words with no spaces in them.
///
/// The parse therefore finds those two words and reads outward:
/// id is before the name, the name runs to the provider, the hostname runs from
/// the provider to the status, and the ip is what follows. That is correct for
/// a spaced name *and* a spaced hostname at once, and it keeps working
/// unchanged if lane A escapes either.
///
/// A line that does not fit is skipped rather than failing the read: one bad
/// row should not blank the page.
#[must_use]
pub fn parse_proc_dyndns(text: &str) -> Vec<DynDnsRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // No length floor here: the shape check below already implies one.
        // A row needs a name before the provider and a hostname and an address
        // after it, so `prov_at >= 2`, `status_at >= prov_at + 2` and
        // `len >= status_at + 2` between them force `len >= 6`. A separate
        // `len < 6` test would look load-bearing and never fire.
        let Some(id) = fields.first().and_then(|f| f.parse::<u64>().ok()) else {
            continue;
        };
        // The two anchors, both found from the *right*, and the provider
        // searched only in what precedes the status.
        //
        // Rightward matters in both cases, and for the same reason: the field
        // to the left of each anchor is free text a user chose, so it may spell
        // an anchor word, while the field to the right cannot. A name of
        // "Success" must lose to the real status; a name of "Dynu" — an
        // entirely ordinary thing to call your one Dynu entry — must lose to
        // the real provider. Searching leftward would reject that second row
        // outright.
        let Some(status_at) = fields.iter().rposition(|f| STATUSES.contains(f)) else {
            continue;
        };
        let Some(prov_at) = fields
            .get(..status_at)
            .and_then(|before| before.iter().rposition(|f| PROVIDERS.contains(f)))
        else {
            continue;
        };
        // The name and hostname must be non-empty and an address must follow.
        // (`prov_at < status_at` already holds — the search was bounded by it.)
        //
        // `saturating_add` rather than `+`: both are positions in `fields`, so
        // both are below its length and cannot overflow, but the crate denies
        // unchecked arithmetic and a saturating add is correct even in the case
        // that cannot arise — it would leave the range empty and skip the row.
        let after_provider = prov_at.saturating_add(1);
        let after_status = status_at.saturating_add(1);
        if prov_at < 2 || status_at == after_provider || after_status >= fields.len() {
            continue;
        }
        let join = |r: std::ops::Range<usize>| {
            fields.get(r).map(|w| w.join(" ")).unwrap_or_default()
        };
        rows.push(DynDnsRow {
            id,
            name: join(1..prov_at),
            provider: (*fields.get(prov_at).unwrap_or(&"")).to_string(),
            hostname: join(after_provider..status_at),
            status: (*fields.get(status_at).unwrap_or(&"")).to_string(),
            last_ip: join(after_status..fields.len()),
        });
    }
    rows
}

/// Parse the counters at the top of `/proc/dyndns`, and the router block
/// underneath them when there is one.
///
/// # Why the first `Router:` line is not the last one
///
/// `Router:` appears twice. Once in the counters as a yes/no, and again as the
/// heading of a detail block — which the kernel writes **only when a router was
/// actually found**. So a parse that simply lets the last matching line win
/// reads `192.168.1.1 (SomeRouter X2)`, compares it to `"detected"`, and
/// concludes there is no router *in precisely the case where there is one*. The
/// first occurrence is the flag; a later one is the detail.
#[must_use]
pub fn parse_proc_dyndns_summary(text: &str) -> DynDnsSummary {
    let mut out = DynDnsSummary::default();
    let mut seen_router_flag = false;
    for line in text.lines() {
        // First colon only, so an IPv6 address survives as the value.
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match label.trim() {
            "DDNS entries" => out.entries = value.parse().unwrap_or(0),
            "Forwards" => out.forwards = value.parse().unwrap_or(0),
            "Router" if !seen_router_flag => {
                out.router_detected = value == "detected";
                seen_router_flag = true;
            }
            "Router" => {
                let (ip, model) = value.split_once(" (").unwrap_or((value, ""));
                out.router_ip = ip.to_string();
                out.router_model = model.trim_end_matches(')').to_string();
            }
            "External IP" => out.external_ip = value.to_string(),
            "UPnP" => out.upnp = value == "true",
            "NAT-PMP" => out.natpmp = value == "true",
            _ => {}
        }
    }
    out
}

/// The dynamic-DNS entries this machine has, and the summary above them.
///
/// Empty if the kernel does not publish them, for the reason the snapshots
/// reader gives: a settings page that cannot read `/proc` should say "none",
/// not refuse to draw.
#[must_use]
pub fn system_dyndns() -> (DynDnsSummary, Vec<DynDnsRow>) {
    std::fs::read_to_string(PROC_DYNDNS).map_or_else(
        |_| (DynDnsSummary::default(), Vec::new()),
        |text| (parse_proc_dyndns_summary(&text), parse_proc_dyndns(&text)),
    )
}

/// A one-line description of an entry, for the page to draw.
#[must_use]
pub fn describe(row: &DynDnsRow) -> String {
    let mut out = String::new();
    let _ = write!(out, "{} → {}", row.provider, row.hostname);
    if row.last_ip != "-" {
        let _ = write!(out, " ({})", row.last_ip);
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that indexes out of range should fail loudly and point at the line that did it"
)]
mod tests {
    use super::*;

    /// Exactly what `gen_dyndns()` writes.
    const SAMPLE: &str = "Dynamic DNS & Port Forwarding\n\
         =============================\n\
         \n\
         DDNS entries:  2\n\
         Forwards:      1\n\
         Router:        detected\n\
         Operations:    17\n\
         \n\
         Router: 192.168.1.1 (SomeRouter X2)\n\
         \x20 External IP: 203.0.113.7\n\
         \x20 UPnP:        true\n\
         \x20 NAT-PMP:     false\n\
         \n\
         ID   NAME            PROVIDER   HOSTNAME                  STATUS     IP\n\
         1    home            Dynu       home.dynu.net             Success    203.0.113.7\n\
         2    office          DuckDns    office.duckdns.org        Idle       -\n";

    #[test]
    fn a_normal_table_parses() {
        let rows = parse_proc_dyndns(SAMPLE);
        assert_eq!(rows.len(), 2, "header or router lines were taken for entries");
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].name, "home");
        assert_eq!(rows[0].provider, "Dynu");
        assert_eq!(rows[0].hostname, "home.dynu.net");
        assert_eq!(rows[0].status, "Success");
        assert_eq!(rows[0].last_ip, "203.0.113.7");
        assert_eq!(rows[1].last_ip, "-", "an entry that has never updated");
    }

    #[test]
    fn the_summary_above_the_table_parses() {
        let s = parse_proc_dyndns_summary(SAMPLE);
        assert_eq!(s.entries, 2);
        assert_eq!(s.forwards, 1);
        assert!(s.router_detected);
        assert_eq!(s.router_ip, "192.168.1.1");
        assert_eq!(s.router_model, "SomeRouter X2");
        assert_eq!(s.external_ip, "203.0.113.7");
        assert!(s.upnp);
        assert!(!s.natpmp, "NAT-PMP is false in the sample");
    }

    /// The regression the sample caught: the detail block exists *only* when a
    /// router was found, so letting the last `Router:` line win reported "no
    /// router" exactly when there was one.
    #[test]
    fn the_router_detail_block_does_not_cancel_the_router_flag() {
        let text = "DDNS entries:  0
Router:        detected

Router: 10.0.0.1 (Box)
";
        assert!(parse_proc_dyndns_summary(text).router_detected);
    }

    #[test]
    fn an_ipv6_router_survives_the_split() {
        let text = "Router:        detected

Router: 2001:db8::1 (Box)
  External IP: 2001:db8::99
";
        let s = parse_proc_dyndns_summary(text);
        assert_eq!(s.router_ip, "2001:db8::1");
        assert_eq!(s.external_ip, "2001:db8::99");
    }

    /// A name with spaces in it. The kernel emits the name unescaped into a
    /// single-space-delimited table, so this row has more whitespace-separated
    /// fields than the format promises.
    ///
    /// Reported to lane A; until it changes, the reader has to cope. It does,
    /// because the provider is a closed set and cannot be confused with a name.
    #[test]
    fn a_name_containing_spaces_is_read_correctly() {
        let line = "3    my home server   Dynu       host.dynu.net   Success   1.2.3.4\n";
        let rows = parse_proc_dyndns(line);
        assert_eq!(rows.len(), 1, "the row was rejected for having spaces");
        assert_eq!(rows[0].name, "my home server");
        assert_eq!(rows[0].hostname, "host.dynu.net");
    }

    /// And a *hostname* with spaces, which is the case the snapshots reader's
    /// count-from-both-ends trick could not have handled: two ambiguous fields
    /// at once. The provider and status anchors sit between them.
    #[test]
    fn a_spaced_name_and_a_spaced_hostname_at_once() {
        let line = "4  my home  Dynu  not a valid host  Failed  -\n";
        let rows = parse_proc_dyndns(line);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "my home");
        assert_eq!(rows[0].hostname, "not a valid host");
        assert_eq!(rows[0].status, "Failed");
        assert_eq!(rows[0].last_ip, "-");
    }

    /// A name that happens to spell a *status* must not be mistaken for one.
    ///
    /// The status is found from the right and the provider from the left, so a
    /// word in the name can never win against the real status further along.
    #[test]
    fn a_name_that_reads_like_a_status_does_not_confuse_the_parse() {
        let line = "5  Success  Dynu  host.dynu.net  Idle  1.2.3.4\n";
        let rows = parse_proc_dyndns(line);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Success", "the name was swallowed as a status");
        assert_eq!(rows[0].status, "Idle");
    }

    /// A row naming a provider this build does not know is skipped rather than
    /// guessed at — a wrong provider on a settings page is worse than a
    /// missing row, because the user would act on it.
    /// The companion to the status case, and the one that decided the search
    /// direction: naming your only Dynu entry "Dynu" is ordinary.
    #[test]
    fn a_name_that_reads_like_a_provider_does_not_confuse_the_parse() {
        let rows = parse_proc_dyndns("7  Dynu  Dynu  host.dynu.net  Idle  1.2.3.4
");
        assert_eq!(rows.len(), 1, "the row was rejected as having no name");
        assert_eq!(rows[0].name, "Dynu");
        assert_eq!(rows[0].hostname, "host.dynu.net");
    }

    /// Rows with a field missing are skipped, not shifted along by one — which
    /// is what makes a wrong hostname on a settings page impossible.
    #[test]
    fn a_row_with_a_field_missing_is_skipped() {
        // No hostname between the provider and the status.
        assert!(parse_proc_dyndns("8  my long name  Dynu  Idle  -
").is_empty());
        // No address after the status.
        assert!(parse_proc_dyndns("9  my home  Dynu  host.example  Idle
").is_empty());
        // No name before the provider.
        assert!(parse_proc_dyndns("10  Dynu  host.example  Idle  -  x
").is_empty());
    }

    #[test]
    fn an_unknown_provider_skips_the_row() {
        assert!(parse_proc_dyndns("6  home  Wat  h.example  Idle  -\n").is_empty());
    }

    #[test]
    fn header_and_router_lines_are_not_entries() {
        for line in [
            "ID   NAME            PROVIDER   HOSTNAME                  STATUS     IP",
            "Router: 192.168.1.1 (SomeRouter X2)",
            "Dynamic DNS & Port Forwarding",
            "DDNS entries:  2",
        ] {
            assert!(
                parse_proc_dyndns(line).is_empty(),
                "parsed {line:?} as an entry"
            );
        }
    }

    #[test]
    fn no_entries_is_no_rows() {
        let text = "Dynamic DNS & Port Forwarding\n\n\
                    DDNS entries:  0\nForwards:      0\nRouter:        none\n";
        assert!(parse_proc_dyndns(text).is_empty());
        let s = parse_proc_dyndns_summary(text);
        assert_eq!(s.entries, 0);
        assert!(!s.router_detected);
    }

    #[test]
    fn describe_omits_an_address_that_does_not_exist_yet() {
        let mut row = DynDnsRow {
            id: 1,
            name: "home".into(),
            provider: "Dynu".into(),
            hostname: "h.dynu.net".into(),
            status: "Idle".into(),
            last_ip: "-".into(),
        };
        assert_eq!(describe(&row), "Dynu → h.dynu.net");
        row.last_ip = "1.2.3.4".into();
        assert_eq!(describe(&row), "Dynu → h.dynu.net (1.2.3.4)");
    }
}
