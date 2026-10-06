//! The parts of init that are functions of their input alone, kept apart
//! from `main.rs`'s system calls so that the host can test them:
//!
//! ```sh
//! cd services/init
//! cargo test --lib --target x86_64-pc-windows-gnu
//! ```
//!
//! On the target this is `no_std`, like the binary that uses it.

#![cfg_attr(not(test), no_std)]

/// Linux's `HOST_NAME_MAX`, and the most `SYS_HOSTNAME_SET` takes.
pub const HOST_NAME_MAX: usize = 64;

/// What `/etc/hostname` says, read the way systemd's `read_etc_hostname()`
/// reads it: the first line that is neither empty nor a comment, with the
/// white space around it removed.
#[derive(Debug, PartialEq, Eq)]
pub enum EtcHostname<'a> {
    /// A valid host name, with one trailing dot dropped.
    Name(&'a [u8]),
    /// No name at all: an empty file, or only blank lines and comments.
    /// The kernel's name stands, as systemd falls back to its default.
    Empty,
    /// A line that is not a valid host name ([`hostname_is_valid`]),
    /// whole and trimmed, for the boot message that says so.
    Invalid(&'a [u8]),
}

/// The name `/etc/hostname` holds; see [`EtcHostname`].
///
/// systemd also "cleans up" the line before it checks it -- drops the
/// characters a host name cannot hold, collapses repeated dots -- and sets
/// whatever is left. This does not: a line that is not a host name is
/// reported and the kernel's name kept, so that a typo in the file is seen
/// at boot rather than turned silently into some other name. That is the
/// rule `requests/b-d-init-should-set-the-host-name-from-etc-hostname.md`
/// asks for.
#[must_use]
pub fn read_etc_hostname(file: &[u8]) -> EtcHostname<'_> {
    for line in file.split(|&b| b == b'\n') {
        let line = trim(line);
        if line.is_empty() || line.first() == Some(&b'#') {
            continue;
        }
        let name = line.strip_suffix(b".").unwrap_or(line);
        return if hostname_is_valid(name) {
            EtcHostname::Name(name)
        } else {
            EtcHostname::Invalid(line)
        };
    }
    EtcHostname::Empty
}

/// Whether `name` is a host name the way systemd's `hostname_is_valid()`
/// judges one (its trailing dot already dropped): at most [`HOST_NAME_MAX`]
/// bytes of ASCII letters, digits, hyphens and dots, in labels that are not
/// empty and neither begin nor end with a hyphen.
#[must_use]
pub fn hostname_is_valid(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > HOST_NAME_MAX {
        return false;
    }
    name.split(|&b| b == b'.').all(|label| {
        !label.is_empty()
            && label.first() != Some(&b'-')
            && label.last() != Some(&b'-')
            && label
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// One record of `/var/run/utmp` and `/var/log/wtmp`: glibc's x86-64
/// `struct utmp`, 384 bytes -- the layout every program here reads and
/// writes them in, through `posix/src/utmpx.rs`'s `FileRecord`, and the
/// one every Linux tool reads.
pub const UTMP_RECORD: usize = 384;

/// `<utmp.h>`'s `BOOT_TIME`: the type of the record a system writes once
/// at every boot, which `who -b` and `last reboot` read back.
pub const BOOT_TIME: i16 = 2;

/// Offsets of the fields [`boot_record`] fills, in glibc's layout:
/// `ut_type` (a `short`) at 0, then after two bytes of padding `ut_pid`,
/// `ut_line[32]`, `ut_id[4]`, `ut_user[32]`, `ut_host[256]`, `ut_exit`,
/// `ut_session`, and the time as two 32-bit fields.
mod field {
    pub const TYPE: usize = 0;
    pub const LINE: usize = 8;
    pub const ID: usize = 40;
    pub const USER: usize = 44;
    pub const HOST: usize = 76;
    /// `ut_host`'s size, as `UT_HOSTSIZE`.
    pub const HOST_LEN: usize = 256;
    pub const TV_SEC: usize = 340;
    pub const TV_USEC: usize = 344;
}

/// The record an init writes at boot to both files, as systemd's
/// `utmp_put_reboot` and sysvinit's `write_utmp_wtmp` do: `BOOT_TIME`,
/// `ut_line` "~", `ut_id` "~~", `ut_user` "reboot", no pid, the kernel's
/// release in `ut_host` -- the version `last reboot` prints -- and the time
/// the system started, each half cut to 32 bits, as glibc's structure holds
/// them. A release longer than `ut_host` keeps its end, as systemd's
/// `copy_suffix` does; a string that fills its field has no NUL, as in
/// every utmp field.
#[must_use]
pub fn boot_record(boot_sec: i64, boot_usec: i64, release: &[u8]) -> [u8; UTMP_RECORD] {
    let mut rec = [0u8; UTMP_RECORD];
    put(&mut rec, field::TYPE, &BOOT_TIME.to_le_bytes());
    put(&mut rec, field::LINE, b"~");
    put(&mut rec, field::ID, b"~~");
    put(&mut rec, field::USER, b"reboot");
    let host = release
        .get(release.len().saturating_sub(field::HOST_LEN)..)
        .unwrap_or(release);
    put(&mut rec, field::HOST, host);
    // Cut to 32 bits on purpose: glibc's `ut_tv` is two `int32_t`s on
    // x86-64 (`__WORDSIZE_TIME64_COMPAT32`).
    #[allow(clippy::cast_possible_truncation)]
    let (sec, usec) = (boot_sec as i32, boot_usec as i32);
    put(&mut rec, field::TV_SEC, &sec.to_le_bytes());
    put(&mut rec, field::TV_USEC, &usec.to_le_bytes());
    rec
}

/// When the system started, from the two clocks: now on the real-time clock,
/// less the time since boot -- the moment systemd stamps the record with.
/// Seconds and microseconds; a real-time clock behind the monotonic one (an
/// unset RTC) gives the epoch rather than a time before it.
#[must_use]
pub fn boot_time(realtime_ns: i64, monotonic_ns: i64) -> (i64, i64) {
    let boot = realtime_ns.saturating_sub(monotonic_ns).max(0);
    (boot / 1_000_000_000, (boot % 1_000_000_000) / 1_000)
}

/// Copy `bytes` into `rec` at `at`, as far as it fits.
fn put(rec: &mut [u8], at: usize, bytes: &[u8]) {
    let room = rec.get_mut(at..).unwrap_or_default();
    let n = bytes.len().min(room.len());
    if let (Some(dst), Some(src)) = (room.get_mut(..n), bytes.get(..n)) {
        dst.copy_from_slice(src);
    }
}

/// `s` without the ASCII white space around it -- spaces, tabs, and the
/// carriage return of a file written on Windows.
fn trim(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i.saturating_add(1));
    s.get(start..end).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_is_the_name() {
        assert_eq!(
            read_etc_hostname(b"slate-test\n"),
            EtcHostname::Name(b"slate-test")
        );
        assert_eq!(
            read_etc_hostname(b"slate-test"),
            EtcHostname::Name(b"slate-test")
        );
    }

    #[test]
    fn blank_lines_and_comments_come_first() {
        let file = b"\n   \n# the machine's name\n\t# another\n  box.example.org  \nignored\n";
        assert_eq!(
            read_etc_hostname(file),
            EtcHostname::Name(b"box.example.org")
        );
    }

    #[test]
    fn a_file_with_no_name_is_empty() {
        assert_eq!(read_etc_hostname(b""), EtcHostname::Empty);
        assert_eq!(read_etc_hostname(b"\n\n  \n"), EtcHostname::Empty);
        assert_eq!(read_etc_hostname(b"# only a comment\n"), EtcHostname::Empty);
    }

    #[test]
    fn white_space_and_a_windows_line_end_are_trimmed() {
        assert_eq!(
            read_etc_hostname(b"  slate \r\n"),
            EtcHostname::Name(b"slate")
        );
    }

    #[test]
    fn one_trailing_dot_is_dropped() {
        assert_eq!(
            read_etc_hostname(b"box.example.org.\n"),
            EtcHostname::Name(b"box.example.org")
        );
        assert_eq!(
            read_etc_hostname(b"box..\n"),
            EtcHostname::Invalid(b"box..")
        );
    }

    #[test]
    fn what_is_not_a_host_name_is_reported_whole() {
        for bad in [
            &b"my_host"[..],
            b"two words",
            b"-leading",
            b"trailing-",
            b"a..b",
            b".lead",
            b"caf\xc3\xa9",
            b".",
        ] {
            let mut file = bad.to_vec();
            file.push(b'\n');
            assert_eq!(
                read_etc_hostname(&file),
                EtcHostname::Invalid(bad),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn sixty_four_bytes_is_the_limit() {
        let ok = [b'a'; HOST_NAME_MAX];
        assert!(hostname_is_valid(&ok));
        let too_long = [b'a'; HOST_NAME_MAX + 1];
        assert!(!hostname_is_valid(&too_long));
        // A trailing dot does not count against the limit: it is dropped first.
        let mut with_dot = ok.to_vec();
        with_dot.push(b'.');
        assert_eq!(read_etc_hostname(&with_dot), EtcHostname::Name(&ok));
    }

    #[test]
    fn digits_and_hyphens_inside_labels_are_fine() {
        assert!(hostname_is_valid(b"node-01.rack-7.example"));
        assert!(hostname_is_valid(b"3com"));
        assert!(!hostname_is_valid(b""));
    }

    /// The field at `at`, up to its first NUL or `len` bytes.
    fn string_at(rec: &[u8], at: usize, len: usize) -> &[u8] {
        let f = &rec[at..at + len];
        &f[..f.iter().position(|&b| b == 0).unwrap_or(len)]
    }

    fn i32_at(rec: &[u8], at: usize) -> i32 {
        i32::from_le_bytes(rec[at..at + 4].try_into().unwrap())
    }

    /// Each field where glibc's x86-64 `struct utmp` keeps it -- read back by
    /// offset, as `utmpfile` and `posix/src/utmpx.rs` read the file.
    #[test]
    fn the_boot_record_is_systemds_in_glibcs_layout() {
        let rec = boot_record(1_782_921_600, 250_000, b"6.6.0-slateos");
        assert_eq!(rec.len(), 384);
        assert_eq!(i16::from_le_bytes([rec[0], rec[1]]), 2, "BOOT_TIME");
        assert_eq!(i32_at(&rec, 4), 0, "no pid");
        assert_eq!(string_at(&rec, 8, 32), b"~", "ut_line");
        assert_eq!(string_at(&rec, 40, 4), b"~~", "ut_id");
        assert_eq!(string_at(&rec, 44, 32), b"reboot", "ut_user");
        assert_eq!(string_at(&rec, 76, 256), b"6.6.0-slateos", "ut_host");
        assert_eq!(i32_at(&rec, 340), 1_782_921_600, "ut_tv.tv_sec");
        assert_eq!(i32_at(&rec, 344), 250_000, "ut_tv.tv_usec");
        // Nothing else is written: the exit status, the session, the
        // address and the reserved bytes stay zero.
        assert!(rec[332..340].iter().all(|&b| b == 0));
        assert!(rec[348..].iter().all(|&b| b == 0));
    }

    /// A release too long for `ut_host` keeps its end, as systemd's
    /// `copy_suffix` does, and fills the field with no NUL.
    #[test]
    fn a_long_release_keeps_its_end() {
        let mut release = vec![b'x'; 300];
        release.extend_from_slice(b"-tail");
        let rec = boot_record(0, 0, &release);
        let host = &rec[76..76 + 256];
        assert!(host.ends_with(b"-tail"));
        assert!(host.iter().all(|&b| b != 0));
        assert_eq!(i32_at(&rec, 340), 0, "the field after ut_host is untouched");
    }

    #[test]
    fn the_boot_time_is_now_less_the_uptime() {
        // 2026-07-01 00:00:00.5 UTC, two seconds and a quarter after boot.
        let now = 1_782_864_000_500_000_000_i64;
        assert_eq!(boot_time(now, 2_250_000_000), (1_782_863_998, 250_000));
        // An unset real-time clock -- behind the monotonic one -- is the
        // epoch, not a time before it.
        assert_eq!(boot_time(1_000, 5_000_000_000), (0, 0));
    }
}
