//! The three syslog headers util-linux 2.39.3's `logger` writes, and their
//! timestamps: `syslog_local_header`, `syslog_rfc3164_header`,
//! `syslog_rfc5424_header` and `rfc3164_current_time` from
//! `misc-utils/logger.c`.
//!
//! Pure functions: the clock, the hostname and the zone are the caller's, read
//! once where upstream reads them, which is what lets the tests pin frames
//! exactly.

use localtime::Zone;

/// `NILVALUE`: RFC 5424's spelling of an absent field.
pub const NILVALUE: &[u8] = b"-";

/// Which header `generate_syslog_header` builds: upstream's `syslogfp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Header {
    /// `syslog_local_header`, the default for a local socket.
    Local,
    /// `syslog_rfc3164_header`: `--rfc3164`.
    Rfc3164,
    /// `syslog_rfc5424_header`: `--rfc5424`, and the default for `-n`.
    Rfc5424,
}

/// An instant as `gettimeofday` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeVal {
    /// Whole seconds, rounded down.
    pub sec: i64,
    /// 0..1_000_000.
    pub usec: u32,
}

/// `rfc3164_current_time`: `Mmm dd hh:mm:ss` for `t` in `zone`.
///
/// **Local time**, because that is what RFC 3164's TIMESTAMP means: it has
/// no zone marker, so a receiver can only read it as local. Upstream's own
/// English month table rather than `%b`, so the field does not follow the
/// locale, and the day padded with a space, RFC 3164's rule.
#[must_use]
pub fn rfc3164_time(zone: &Zone, t: i64) -> String {
    const MONTHNAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Upstream does not check `localtime_r` and formats whatever the
    // uninitialised `tm` holds when it fails. It fails only when the year
    // overflows an `int`, which the current time cannot do; `local`, which
    // saturates instead of failing, is at least a defined answer.
    let [mon, mday, hour, min, sec] = zone.localtime_r(t).map_or_else(
        || {
            let tm = zone.local(t, 0);
            [
                tm.month.saturating_sub(1),
                tm.day,
                tm.hour,
                tm.minute,
                tm.second,
            ]
            .map(i64::from)
        },
        |tm| [tm.tm_mon, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec].map(i64::from),
    );
    let month = usize::try_from(mon)
        .ok()
        .and_then(|m| MONTHNAMES.get(m))
        .copied()
        .unwrap_or("???");
    format!("{month} {mday:2} {hour:02}:{min:02}:{sec:02}")
}

/// The RFC 5424 TIMESTAMP, built as upstream builds it: `strftime` of
/// `"%Y-%m-%dT%H:%M:%S.%%06u%z "`, then a colon patched into the offset over
/// the trailing space (`-0400 ` becomes `-04:00`), then the microseconds
/// formatted into the `%06u` that the `%%` left behind.
///
/// `None` where upstream dies with "localtime() failed".
#[must_use]
pub fn rfc5424_time(zone: &Zone, tv: TimeVal) -> Option<Vec<u8>> {
    // `localtime_r` decides failure; the fields come from the same zone.
    zone.localtime_r(tv.sec)?;
    let tm = zone.local(tv.sec, 0);
    let mut fmt = localtime::strftime(b"%Y-%m-%dT%H:%M:%S.%%06u%z ", &tm);
    // fmt[i - 1] = fmt[i - 2]; fmt[i - 2] = fmt[i - 3]; fmt[i - 3] = ':';
    let i = fmt.len();
    let (i1, i2, i3) = (i.checked_sub(1)?, i.checked_sub(2)?, i.checked_sub(3)?);
    let (a, b) = (fmt.get(i2).copied()?, fmt.get(i3).copied()?);
    *fmt.get_mut(i1)? = a;
    *fmt.get_mut(i2)? = b;
    *fmt.get_mut(i3)? = b':';
    // The only `%` left is the `%06u` from `%%06u`: strftime's output for the
    // other fields is digits and signs.
    let at = fmt.windows(4).position(|w| w == b"%06u")?;
    let mut out = fmt.get(..at)?.to_vec();
    out.extend_from_slice(format!("{:06}", tv.usec).as_bytes());
    out.extend_from_slice(fmt.get(at.checked_add(4)?..)?);
    Some(out)
}

/// `syslog_local_header`: `<PRI>TIMESTAMP TAG[PID]: `.
#[must_use]
pub fn local_header(pri: i32, time: &str, tag: &[u8], pid: i32) -> Vec<u8> {
    let mut h = format!("<{pri}>{time} ").into_bytes();
    h.extend_from_slice(tag);
    push_pid(&mut h, pid);
    h.extend_from_slice(b": ");
    h
}

/// `syslog_rfc3164_header`: `<PRI>TIMESTAMP HOSTNAME TAG[PID]: `, with
/// upstream's precisions -- `%.15s` on the time (it is 15 wide anyway) and
/// `%.200s` on the tag, counted in bytes as C counts them.
///
/// `hostname` is `None` when `gethostname` failed, rendered as the NILVALUE;
/// otherwise it is cut at its first `.`.
#[must_use]
pub fn rfc3164_header(
    pri: i32,
    time: &str,
    hostname: Option<&[u8]>,
    tag: &[u8],
    pid: i32,
) -> Vec<u8> {
    let mut h = format!("<{pri}>").into_bytes();
    h.extend_from_slice(time.as_bytes().get(..15).unwrap_or(time.as_bytes()));
    h.push(b' ');
    match hostname {
        Some(name) => h.extend_from_slice(name.split(|&b| b == b'.').next().unwrap_or(name)),
        None => h.extend_from_slice(NILVALUE),
    }
    h.push(b' ');
    h.extend_from_slice(tag.get(..200).unwrap_or(tag));
    push_pid(&mut h, pid);
    h.extend_from_slice(b": ");
    h
}

/// `syslog_rfc5424_header`'s format: `<PRI>1 TIME HOST APP PROCID MSGID SD `.
/// Every field is the caller's, the NILVALUE already substituted where one is
/// absent; the checks upstream makes first (a tag over 48 bytes, a hostname
/// over 255) are the caller's too, because they end the program.
#[must_use]
pub fn rfc5424_header(pri: i32, fields: [&[u8]; 6]) -> Vec<u8> {
    let mut h = format!("<{pri}>1").into_bytes();
    for f in fields {
        h.push(b' ');
        h.extend_from_slice(f);
    }
    h.push(b' ');
    h
}

/// `[PID]` after the tag, or nothing when no id was asked for (upstream's
/// `pid == 0`). `%d` of a `pid_t`: an `--id` past `INT_MAX` prints negative.
fn push_pid(h: &mut Vec<u8>, pid: i32) {
    if pid != 0 {
        h.extend_from_slice(format!("[{pid}]").as_bytes());
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn eastern() -> Zone {
        Zone::from_tz(Some(b"EST5EDT,M3.2.0,M11.1.0".as_slice()))
    }

    /// Local time, not UTC -- the defect TD-B-EVERY-SYSLOG-TIMESTAMP-... was.
    #[test]
    fn the_rfc3164_timestamp_is_local_time() {
        // 2020-01-20 06:00:00 UTC is 01:00 EST.
        assert_eq!(rfc3164_time(&eastern(), 1_579_500_000), "Jan 20 01:00:00");
        // 2020-03-08 09:00:00 UTC is 05:00 EDT, after that morning's change,
        // and the 8th is padded with a space.
        assert_eq!(rfc3164_time(&eastern(), 1_583_658_000), "Mar  8 05:00:00");
        assert_eq!(rfc3164_time(&Zone::utc(), 1_583_658_000), "Mar  8 09:00:00");
    }

    #[test]
    fn a_half_hour_zone_moves_the_minutes_and_the_date() {
        let india = Zone::from_tz(Some(b"IST-5:30".as_slice()));
        assert_eq!(rfc3164_time(&india, 1_609_444_800), "Jan  1 01:30:00");
    }

    /// Measured against util-linux 2.39.3: `-04:00` in New York in September,
    /// `+05:30` in Kolkata, six digits of microseconds.
    #[test]
    fn the_rfc5424_timestamp_is_patched_as_upstream_patches_it() {
        let tv = TimeVal {
            sec: 1_790_419_036,
            usec: 599_632,
        };
        assert_eq!(
            rfc5424_time(&eastern(), tv).as_deref(),
            Some(b"2026-09-26T06:37:16.599632-04:00".as_slice())
        );
        let india = Zone::from_tz(Some(b"IST-5:30".as_slice()));
        assert_eq!(
            rfc5424_time(
                &india,
                TimeVal {
                    sec: 1_790_419_036,
                    usec: 7
                }
            )
            .as_deref(),
            Some(b"2026-09-26T16:07:16.000007+05:30".as_slice())
        );
        assert_eq!(
            rfc5424_time(&Zone::utc(), TimeVal { sec: 0, usec: 0 }).as_deref(),
            Some(b"1970-01-01T00:00:00.000000+00:00".as_slice())
        );
    }

    #[test]
    fn the_local_header() {
        assert_eq!(
            local_header(13, "Sep 26 06:37:16", b"mytag", 0),
            b"<13>Sep 26 06:37:16 mytag: "
        );
        assert_eq!(
            local_header(13, "Sep 26 06:37:16", b"t", 4242),
            b"<13>Sep 26 06:37:16 t[4242]: "
        );
        assert_eq!(
            local_header(13, "Sep 26 06:37:16", b"t", -1),
            b"<13>Sep 26 06:37:16 t[-1]: "
        );
    }

    #[test]
    fn the_rfc3164_header_cuts_the_hostname_and_the_tag() {
        assert_eq!(
            rfc3164_header(13, "Sep 26 06:37:16", Some(b"host.example.com"), b"t", 4242),
            b"<13>Sep 26 06:37:16 host t[4242]: "
        );
        assert_eq!(
            rfc3164_header(13, "Sep 26 06:37:16", None, b"t", 0),
            b"<13>Sep 26 06:37:16 - t: "
        );
        let long = vec![b'x'; 250];
        let h = rfc3164_header(13, "Sep 26 06:37:16", Some(b"h"), &long, 0);
        assert_eq!(h.len(), "<13>Sep 26 06:37:16 h ".len() + 200 + ": ".len());
    }

    #[test]
    fn the_rfc5424_header() {
        let h = rfc5424_header(13, [b"-", b"host", b"t", b"4242", b"-", b"-"]);
        assert_eq!(h, b"<13>1 - host t 4242 - - ");
    }
}
