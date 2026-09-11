//! The binary `/var/run/utmp` record layout, parsed once and holding bytes.
//!
//! # Why this crate exists
//!
//! Three programs read `/var/run/utmp` and each carried its own idea of what
//! is in it:
//!
//! | Reader | Layout | Result |
//! |---|---|---|
//! | `userspace/who` | 384-byte records, every field | correct |
//! | `userspace/uptime` | 384-byte records, `ut_type` only | correct, and a second copy of the offsets |
//! | `userspace/w` | **colon-separated text** | never matches anything |
//!
//! `posix/src/linux_utmp_types.rs` already declares `UTMPX_RECORD_SIZE = 384`
//! as the canonical value, so there were four statements of one fact and one
//! of them disagreed. `userspace/w` read the file with `read_to_string` and
//! split on `:`, which cannot parse a binary record — so its user list was
//! always empty, and it fell back to inventing a session from `$USER`.
//!
//! # The record layout
//!
//! | Offset | Size | Field         |
//! |--------|------|---------------|
//! | 0      | 2    | ut_type (i16) |
//! | 2      | 2    | padding       |
//! | 4      | 4    | ut_pid (u32)  |
//! | 8      | 32   | ut_line       |
//! | 40     | 4    | ut_id         |
//! | 44     | 32   | ut_user       |
//! | 76     | 256  | ut_host       |
//! | 332    | 4    | ut_exit       |
//! | 336    | 4    | ut_session    |
//! | 340    | 4    | ut_tv_sec     |
//! | 344    | 4    | ut_tv_usec    |
//! | 348    | 16   | ut_addr_v6    |
//! | 364    | 20   | unused        |
//! | = 384 bytes total              |
//!
//! Lifted from `userspace/last`, whose copy of this table was CORRECT ABOUT THE
//! PADDING AT OFFSET 2 while this crate's code read four bytes there. The
//! documentation of the fourth copy named the thing the shared implementation
//! got wrong, which is an argument for moving prose to where the code is rather
//! than for keeping four of each.
//!
//! # Why the fields are `Vec<u8>` and not `String`
//!
//! `who` decoded them with `String::from_utf8_lossy`. CLAUDE.md names that
//! construct specifically: *no `from_utf8_lossy` — that's silent data
//! corruption*. Our usernames allow every byte except `/` and NUL, so a name
//! containing one non-UTF-8 byte becomes U+FFFD — and, worse than displaying
//! wrong, TWO DIFFERENT USERS COLLAPSE INTO THE SAME STRING. Anything that
//! compares, sorts, greps or counts by name then treats them as one person.
//!
//! Bytes in, bytes out. A caller that must have text asks for it explicitly
//! and owns the consequence.

#![forbid(unsafe_code)]

/// Size of one utmp record on x86_64 Linux, which is the layout we mirror.
///
/// Must equal `posix::linux_utmp_types::UTMPX_RECORD_SIZE`. It is repeated
/// rather than imported because `posix` is the OS's own libc and pulling it
/// into a host-side utility crate would drag the whole ABI surface along for
/// one integer.
pub const RECORD_SIZE: usize = 384;

// Field offsets within a record. These are the x86_64 `struct utmpx` layout.
//
// PUBLIC because they are the format, not an implementation detail: `RECORD_SIZE`
// has always been public for the same reason. A caller building a fixture needs
// to lay bytes out at exactly these positions, and the alternative is each test
// re-declaring the table this crate exists to hold once.
pub const UT_TYPE_OFFSET: usize = 0;
pub const UT_PID_OFFSET: usize = 4;
pub const UT_LINE_OFFSET: usize = 8;
pub const UT_LINE_SIZE: usize = 32;
pub const UT_ID_OFFSET: usize = 40;
pub const UT_ID_SIZE: usize = 4;
pub const UT_USER_OFFSET: usize = 44;
pub const UT_USER_SIZE: usize = 32;
pub const UT_HOST_OFFSET: usize = 76;
pub const UT_HOST_SIZE: usize = 256;
pub const UT_EXIT_OFFSET: usize = 332;
pub const UT_SESSION_OFFSET: usize = 336;
pub const UT_TV_SEC_OFFSET: usize = 340;
pub const UT_TV_USEC_OFFSET: usize = 344;
pub const UT_ADDR_OFFSET: usize = 348;

/// `ut_type` for an unused slot.
pub const EMPTY: i32 = 0;
/// `ut_type` for a run-level change.
pub const RUN_LVL: i32 = 1;
/// `ut_type` for a clock change, recorded before the new time.
pub const NEW_TIME: i32 = 3;
/// `ut_type` for a clock change, recorded after the old time.
pub const OLD_TIME: i32 = 4;
/// `ut_type` for a process spawned by init.
pub const INIT_PROCESS: i32 = 5;
/// `ut_type` for a normal logged-in user session.
pub const USER_PROCESS: i32 = 7;
/// `ut_type` for a terminal waiting for a login.
pub const LOGIN_PROCESS: i32 = 6;
/// `ut_type` for the boot-time record carrying the system start.
pub const BOOT_TIME: i32 = 2;
/// `ut_type` for a session that has ended.
pub const DEAD_PROCESS: i32 = 8;

/// One utmp record.
///
/// The byte fields are trimmed at the first NUL, exactly as the C layout
/// intends, and are otherwise untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// `ut_type`. Compare against [`USER_PROCESS`] and friends.
    pub record_type: i32,
    /// `ut_user`, up to the first NUL. Not necessarily UTF-8.
    pub user: Vec<u8>,
    /// `ut_line` — the terminal, e.g. `tty1` or `pts/0`.
    pub tty: Vec<u8>,
    /// `ut_host` — remote host, or empty for a local session.
    pub host: Vec<u8>,
    /// `ut_id`, the four-byte abbreviation shown by `who --all`.
    pub id: Vec<u8>,
    /// `ut_pid`.
    pub pid: i32,
    /// `ut_tv.tv_sec` as Unix epoch seconds, or 0 if it was negative.
    pub login_time: u64,
    /// `ut_tv.tv_usec`, the microseconds part of the timestamp.
    pub login_usec: u32,
    /// `ut_exit`, the two `short`s a DEAD_PROCESS record carries: the
    /// terminating signal in the low half and the exit status in the high one.
    /// `last` prints it; nothing else reads it yet.
    pub exit_status: u32,
    /// `ut_session`, the session id.
    pub session: u32,
    /// `ut_addr_v6`: four words holding an IPv4 address in the first, or a
    /// whole IPv6 address. Kept raw because the two are told apart by whether
    /// the last three are zero, which is the caller's business.
    pub addr_v6: [u32; 4],
}

impl Record {
    /// True if this record describes a user who is logged in now.
    #[must_use]
    pub fn is_user_session(&self) -> bool {
        self.record_type == USER_PROCESS
    }
}

/// `ut_type` is a `short`, not an int.
///
/// It was read as a little-endian i32 until 2026-09-10, which is `ut_type`
/// PLUS the two padding bytes the C struct puts before `ut_pid`. That agrees
/// with a 2-byte read only while the padding is zero -- true of every record
/// this tree writes, and not a property of the format. `posix::utmpx::Utmpx`
/// declares `ut_type: i16` and `userspace/last` read it as a u16, so this
/// crate was the odd one out among three statements of the same field.
///
/// Widened to i32 on the way out so callers keep comparing against the
/// constants without a cast.
fn read_type(data: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(2)?;
    let slice = data.get(offset..end)?;
    let arr: [u8; 2] = slice.try_into().ok()?;
    Some(i32::from(i16::from_le_bytes(arr)))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = data.get(offset..end)?;
    let arr: [u8; 4] = slice.try_into().ok()?;
    Some(u32::from_le_bytes(arr))
}

fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    let slice = data.get(offset..end)?;
    let arr: [u8; 4] = slice.try_into().ok()?;
    Some(i32::from_le_bytes(arr))
}

/// The bytes of a fixed-width field, up to the first NUL.
fn field(data: &[u8], offset: usize, max_len: usize) -> Option<Vec<u8>> {
    let end = offset.checked_add(max_len)?;
    let slice = data.get(offset..end)?;
    let nul = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    Some(slice.get(..nul)?.to_vec())
}

/// Parse every whole record in a utmp file image.
///
/// Returns an empty vector for an empty or short file — which is the honest
/// answer, since a utmp with no whole record genuinely describes no sessions.
/// A caller that needs to distinguish "nobody is logged in" from "I could not
/// read the file" must check the read itself; this function is only given
/// bytes and cannot tell.
///
/// Trailing bytes that do not make up a whole record are ignored rather than
/// treated as an error: utmp is appended to by other processes and a torn
/// final record is a normal thing to observe.
#[must_use]
pub fn parse(data: &[u8]) -> Vec<Record> {
    let mut records = Vec::new();
    let mut offset: usize = 0;

    while offset.saturating_add(RECORD_SIZE) <= data.len() {
        // Take the record as a slice ONCE, then read fields at constant
        // offsets within it. Adding the field offset to the running position
        // at each call site is both noisier and arithmetic the lint is right
        // to object to -- there is no bound on `offset + UT_HOST_OFFSET`
        // visible at the call site, and there is on `rec[UT_HOST_OFFSET]`.
        let Some(rec) = data.get(offset..offset.saturating_add(RECORD_SIZE)) else {
            break;
        };

        let Some(record_type) = read_type(rec, UT_TYPE_OFFSET) else {
            break;
        };
        let Some(pid) = read_i32_le(rec, UT_PID_OFFSET) else {
            break;
        };
        let Some(tty) = field(rec, UT_LINE_OFFSET, UT_LINE_SIZE) else {
            break;
        };
        let Some(id) = field(rec, UT_ID_OFFSET, UT_ID_SIZE) else {
            break;
        };
        let Some(user) = field(rec, UT_USER_OFFSET, UT_USER_SIZE) else {
            break;
        };
        let Some(host) = field(rec, UT_HOST_OFFSET, UT_HOST_SIZE) else {
            break;
        };
        let Some(tv_sec) = read_i32_le(rec, UT_TV_SEC_OFFSET) else {
            break;
        };
        let login_usec = read_u32_le(rec, UT_TV_USEC_OFFSET).unwrap_or(0);
        let exit_status = read_u32_le(rec, UT_EXIT_OFFSET).unwrap_or(0);
        let session = read_u32_le(rec, UT_SESSION_OFFSET).unwrap_or(0);
        let mut addr_v6 = [0u32; 4];
        for (i, slot) in addr_v6.iter_mut().enumerate() {
            // `chunk` walks the four words without computing an offset: the
            // record is known to be RECORD_SIZE long so every read is in
            // bounds, and an `unwrap_or(0)` here would be unreachable rather
            // than a discarded failure -- which is a distinction this crate
            // exists to keep, so it is better not to write one at all.
            let chunk = UT_ADDR_OFFSET.saturating_add(i.saturating_mul(4));
            *slot = read_u32_le(rec, chunk).unwrap_or(0);
        }

        records.push(Record {
            record_type,
            user,
            tty,
            host,
            id,
            pid,
            // tv_sec is signed but holds a positive epoch time. A negative one
            // is a corrupt record, and 0 is the only honest reading of it.
            login_time: u64::try_from(tv_sec).unwrap_or(0),
            login_usec,
            exit_status,
            session,
            addr_v6,
        });

        offset = offset.saturating_add(RECORD_SIZE);
    }

    records
}

/// The number of records describing a user logged in now.
///
/// `uptime` wants only this, and computing it here keeps the record layout in
/// one place rather than two.
#[must_use]
pub fn count_user_sessions(data: &[u8]) -> usize {
    parse(data).iter().filter(|r| r.is_user_session()).count()
}

#[cfg(test)]
// The fixture builder writes fields at fixed offsets into a fixed-size array,
// so it indexes and adds constants throughout. CLAUDE.md allows these two in
// test modules for exactly this reason: a panic on bad data is the intended
// behaviour here, where in the parser above it would be the defect.
// `procinfo/src/tests.rs` carries the same pair.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    /// Build one record with the fields a test cares about.
    fn record(
        ut_type: i32,
        pid: i32,
        line: &[u8],
        id: &[u8],
        user: &[u8],
        host: &[u8],
        tv: i32,
    ) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_SIZE];
        r[UT_TYPE_OFFSET..UT_TYPE_OFFSET + 4].copy_from_slice(&ut_type.to_le_bytes());
        r[UT_PID_OFFSET..UT_PID_OFFSET + 4].copy_from_slice(&pid.to_le_bytes());
        r[UT_LINE_OFFSET..UT_LINE_OFFSET + line.len()].copy_from_slice(line);
        r[UT_ID_OFFSET..UT_ID_OFFSET + id.len()].copy_from_slice(id);
        r[UT_USER_OFFSET..UT_USER_OFFSET + user.len()].copy_from_slice(user);
        r[UT_HOST_OFFSET..UT_HOST_OFFSET + host.len()].copy_from_slice(host);
        r[UT_TV_SEC_OFFSET..UT_TV_SEC_OFFSET + 4].copy_from_slice(&tv.to_le_bytes());
        r
    }

    #[test]
    fn one_record_round_trips() {
        let data = record(
            USER_PROCESS,
            1234,
            b"pts/0",
            b"ts/0",
            b"alice",
            b"10.0.0.2",
            1_700_000_000,
        );
        let got = parse(&data);
        assert_eq!(got.len(), 1);
        let r = &got[0];
        assert_eq!(r.record_type, USER_PROCESS);
        assert_eq!(r.pid, 1234);
        assert_eq!(r.tty, b"pts/0");
        assert_eq!(r.id, b"ts/0");
        assert_eq!(r.user, b"alice");
        assert_eq!(r.host, b"10.0.0.2");
        assert_eq!(r.login_time, 1_700_000_000);
        assert!(r.is_user_session());
    }

    #[test]
    fn a_username_that_is_not_utf8_survives_intact() {
        // THE REASON THIS CRATE HOLDS BYTES. `who` decoded ut_user with
        // String::from_utf8_lossy, so 0xFF became U+FFFD.
        let data = record(USER_PROCESS, 1, b"tty1", b"", b"caf\xe9", b"", 0);
        let got = parse(&data);
        assert_eq!(got[0].user, b"caf\xe9");
    }

    #[test]
    fn two_users_differing_only_outside_utf8_stay_different() {
        // The failure that is worse than displaying wrong: under lossy
        // decoding both of these become "caf\u{FFFD}" and anything that
        // compares, sorts or counts by name treats them as one person.
        let mut data = record(USER_PROCESS, 1, b"tty1", b"", b"caf\xe9", b"", 0);
        data.extend(record(USER_PROCESS, 2, b"tty2", b"", b"caf\xfe", b"", 0));
        let got = parse(&data);
        assert_eq!(got.len(), 2);
        assert_ne!(got[0].user, got[1].user);
        assert_eq!(
            String::from_utf8_lossy(&got[0].user),
            String::from_utf8_lossy(&got[1].user),
            "the two names are indistinguishable ONCE DECODED, which is the bug"
        );
    }

    #[test]
    fn a_field_is_cut_at_its_first_nul_not_its_width() {
        let data = record(USER_PROCESS, 1, b"tty1", b"", b"bob", b"", 0);
        assert_eq!(parse(&data)[0].user, b"bob");
        assert_eq!(parse(&data)[0].host, b"");
    }

    #[test]
    fn several_records_all_parse() {
        let mut data = record(USER_PROCESS, 1, b"tty1", b"", b"a", b"", 10);
        data.extend(record(LOGIN_PROCESS, 2, b"tty2", b"", b"LOGIN", b"", 20));
        data.extend(record(USER_PROCESS, 3, b"tty3", b"", b"c", b"", 30));
        let got = parse(&data);
        assert_eq!(got.len(), 3);
        assert_eq!(count_user_sessions(&data), 2);
    }

    #[test]
    fn an_empty_or_short_file_yields_no_records() {
        assert!(parse(b"").is_empty());
        assert!(parse(&[0u8; RECORD_SIZE - 1]).is_empty());
        assert_eq!(count_user_sessions(b""), 0);
    }

    #[test]
    fn a_torn_final_record_is_ignored_rather_than_guessed() {
        // utmp is appended to by other processes, so observing a partial
        // record is normal. Parsing it would invent fields from whatever
        // happens to follow.
        let mut data = record(USER_PROCESS, 1, b"tty1", b"", b"alice", b"", 0);
        data.extend_from_slice(&[0u8; 100]);
        let got = parse(&data);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].user, b"alice");
    }

    #[test]
    fn a_negative_login_time_reads_as_zero_not_as_a_huge_one() {
        // The cast this replaced was `tv_sec as u64`, which turns -1 into
        // 18446744073709551615 and prints as a date in the year 584942417355.
        let data = record(USER_PROCESS, 1, b"tty1", b"", b"alice", b"", -1);
        assert_eq!(parse(&data)[0].login_time, 0);
    }

    #[test]
    fn dead_records_parse_but_are_not_sessions() {
        let data = record(DEAD_PROCESS, 9, b"tty9", b"", b"ghost", b"", 0);
        let got = parse(&data);
        assert_eq!(got.len(), 1, "a dead record is still a record");
        assert!(!got[0].is_user_session());
        assert_eq!(count_user_sessions(&data), 0);
    }

    #[test]
    fn ut_type_is_read_as_two_bytes_not_four() {
        // THE BUG THIS PINS. `ut_type` is a `short` and the C struct puts two
        // padding bytes after it, before `ut_pid` at offset 4. Reading a
        // little-endian i32 there is ut_type PLUS the padding, which agrees
        // with the truth only while the padding is zero -- a property of the
        // writers this tree happens to have, not of the format.
        //
        // `posix::utmpx::Utmpx` declares `ut_type: i16` and `userspace/last`
        // read a u16, so this crate was the odd one out among three statements
        // of one field.
        let mut data = record(USER_PROCESS, 1, b"tty1", b"", b"alice", b"", 0);
        data[2] = 0xAB;
        data[3] = 0xCD;
        let got = parse(&data);
        assert_eq!(
            got[0].record_type, USER_PROCESS,
            "padding after ut_type must not change the type"
        );
        assert!(got[0].is_user_session());
    }

    #[test]
    fn the_tail_of_the_record_is_carried() {
        // exit, session, the microseconds and the address were readable only
        // by `userspace/last`, which had its own copy of the layout for them.
        let mut data = record(DEAD_PROCESS, 42, b"pts/3", b"ts/3", b"bob", b"h", 1000);
        data[UT_EXIT_OFFSET..UT_EXIT_OFFSET + 4].copy_from_slice(&9u32.to_le_bytes());
        data[UT_SESSION_OFFSET..UT_SESSION_OFFSET + 4].copy_from_slice(&7u32.to_le_bytes());
        data[UT_TV_USEC_OFFSET..UT_TV_USEC_OFFSET + 4].copy_from_slice(&500u32.to_le_bytes());
        data[UT_ADDR_OFFSET..UT_ADDR_OFFSET + 4].copy_from_slice(&0x0100_007Fu32.to_le_bytes());
        let r = &parse(&data)[0];
        assert_eq!(r.exit_status, 9);
        assert_eq!(r.session, 7);
        assert_eq!(r.login_usec, 500);
        assert_eq!(r.addr_v6[0], 0x0100_007F);
        assert_eq!(
            r.addr_v6[1..],
            [0, 0, 0],
            "an IPv4 address leaves the rest zero"
        );
    }

    #[test]
    fn the_ut_type_values_are_the_linux_ones() {
        // These are ABI, not preference: a record written by anything else on
        // this system carries these numbers. They were spread across `who`
        // (all nine) and `uptime` (one), and the two agreed -- which is luck
        // rather than a guarantee, and is what this crate removes.
        assert_eq!(
            [
                EMPTY,
                RUN_LVL,
                BOOT_TIME,
                NEW_TIME,
                OLD_TIME,
                INIT_PROCESS,
                LOGIN_PROCESS,
                USER_PROCESS,
                DEAD_PROCESS
            ],
            [0, 1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn the_record_size_matches_the_one_posix_declares() {
        // posix/src/linux_utmp_types.rs: UTMPX_RECORD_SIZE = 384. If that ever
        // changes, this crate is wrong and every reader with it.
        assert_eq!(RECORD_SIZE, 384);
    }
}
