//! libxcrypt's API for a new password's setting: `crypt_gensalt` (and its
//! `_rn` and `_ra` forms), `crypt_checksalt` and `crypt_preferred_method`.
//! (`crypt_rn` and `crypt_ra` are `crypt.rs`'s.)
//!
//! A program that hashes a new password asks `crypt_gensalt` for a setting
//! -- a method's prefix, its cost and a fresh salt -- and gives it to
//! `crypt`: shadow-utils' `passwd` and `chpasswd`, `mkpasswd` and PAM's
//! `pam_unix` all do.  These answer as libxcrypt 4.4.36's do (`lib/crypt.c`,
//! `lib/crypt-gensalt-static.c`, `lib/util-gensalt-sha.c` and each method's
//! gensalt), held to it by `posix/tools/oracle/crypt_harness.py`, for the
//! methods `crypt` here can hash: yescrypt, scrypt, bcrypt, SHA-512,
//! SHA-256, MD5 crypt, BSDi's DES (`_`) and traditional DES (an empty
//! prefix, or any two salt characters, as libxcrypt matches it).
//!
//! For the methods it cannot -- gost-yescrypt, sha1crypt, SunMD5, NT --
//! `crypt_gensalt` fails with `EINVAL` and `crypt_checksalt` answers
//! `CRYPT_SALT_INVALID`, where libxcrypt, which has them, would make one
//! and answer `CRYPT_SALT_OK` or `CRYPT_SALT_METHOD_LEGACY`: a setting this
//! `crypt` cannot hash is no setting here.
//!
//! The default method -- `crypt_gensalt` given no prefix, and
//! `crypt_preferred_method` -- is libxcrypt's, yescrypt (`$y$`).  Given no
//! random bytes, `crypt_gensalt` takes them from `arc4random_buf`, as
//! libxcrypt does where it has one.

// A setting is short: every index here is bounded by the room checks before
// it, and every sum is of a few small lengths.
#![allow(clippy::indexing_slicing)]
#![allow(clippy::arithmetic_side_effects)]

use crate::errno;

/// `CRYPT_GENSALT_OUTPUT_SIZE`: the room `crypt_gensalt`'s buffer has, and
/// `crypt_gensalt_ra` allocates.  Every setting made here fits.
pub const CRYPT_GENSALT_OUTPUT_SIZE: usize = 192;

/// `crypt_checksalt`: a setting this library would hash, by a method
/// libxcrypt counts strong.
pub const CRYPT_SALT_OK: i32 = 0;
/// `crypt_checksalt`: a setting no method here hashes.
pub const CRYPT_SALT_INVALID: i32 = 1;
/// `crypt_checksalt`: never answered (libxcrypt's own "not implemented").
pub const CRYPT_SALT_METHOD_DISABLED: i32 = 2;
/// `crypt_checksalt`: a setting for a method libxcrypt counts weak --
/// SHA-256, MD5, bcrypt's `$2x$`, the DES methods.  It hashes; a new
/// password should not.
pub const CRYPT_SALT_METHOD_LEGACY: i32 = 3;
/// `crypt_checksalt`: never answered (libxcrypt's own "not implemented").
pub const CRYPT_SALT_TOO_CHEAP: i32 = 4;

/// The method `crypt_gensalt` makes a setting for when given no prefix.
const PREFERRED: &[u8; 4] = b"$y$\0";

/// Why a setting could not be made: libxcrypt's two `errno`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// `ERANGE`: no room for it.
    Range,
    /// `EINVAL`: a cost, a count of random bytes or a prefix it does not
    /// take.
    Invalid,
}

/// A method's gensalt: the setting for a cost and random bytes, into the
/// buffer -- the characters written, with room left for a NUL.
type Gensalt = fn(u64, &[u8], &mut [u8]) -> Result<usize, Refused>;

/// A method `crypt_gensalt` makes settings for: libxcrypt's `struct hashfn`,
/// from its `lib/hashes.conf`.
struct Method {
    prefix: &'static [u8],
    /// The random bytes it takes when the caller gives none.
    nrbytes: usize,
    /// `STRONG` in `hashes.conf`: `crypt_checksalt`'s OK rather than
    /// LEGACY.
    strong: bool,
    gensalt: Gensalt,
}

fn bcrypt_b(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    crate::bcrypt::gensalt(b'b', count, rbytes, out)
}

fn bcrypt_y(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    crate::bcrypt::gensalt(b'y', count, rbytes, out)
}

fn bcrypt_a(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    crate::bcrypt::gensalt(b'a', count, rbytes, out)
}

/// `gensalt_bcrypt_x_rn`: none -- `$2x$` is the bug's, kept only to verify.
fn bcrypt_x(_count: u64, _rbytes: &[u8], _out: &mut [u8]) -> Result<usize, Refused> {
    Err(Refused::Invalid)
}

fn sha512(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    gensalt_sha(b'6', 16, 5000, 1000, 999_999_999, count, rbytes, out)
}

fn sha256(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    gensalt_sha(b'5', 16, 5000, 1000, 999_999_999, count, rbytes, out)
}

/// `gensalt_md5crypt_rn`: MD5 crypt has no cost to ask for.
fn md5(count: u64, rbytes: &[u8], out: &mut [u8]) -> Result<usize, Refused> {
    if count != 0 {
        return Err(Refused::Invalid);
    }
    gensalt_sha(b'1', 8, 1000, 1000, 1000, 1000, rbytes, out)
}

/// The methods, in `hashes.conf`'s order: a prefix is matched in it, and
/// the one with none -- bigcrypt's, which is traditional DES's too -- last.
const METHODS: [Method; 11] = [
    Method {
        prefix: b"$y$",
        nrbytes: 16,
        strong: true,
        gensalt: crate::yescrypt::gensalt,
    },
    Method {
        prefix: b"$7$",
        nrbytes: 16,
        strong: true,
        gensalt: crate::yescrypt::gensalt_scrypt,
    },
    Method {
        prefix: b"$2b$",
        nrbytes: 16,
        strong: true,
        gensalt: bcrypt_b,
    },
    Method {
        prefix: b"$2y$",
        nrbytes: 16,
        strong: true,
        gensalt: bcrypt_y,
    },
    Method {
        prefix: b"$2a$",
        nrbytes: 16,
        strong: true,
        gensalt: bcrypt_a,
    },
    Method {
        prefix: b"$2x$",
        nrbytes: 16,
        strong: false,
        gensalt: bcrypt_x,
    },
    Method {
        prefix: b"$6$",
        nrbytes: 15,
        strong: true,
        gensalt: sha512,
    },
    Method {
        prefix: b"$5$",
        nrbytes: 15,
        strong: false,
        gensalt: sha256,
    },
    Method {
        prefix: b"$1$",
        nrbytes: 9,
        strong: false,
        gensalt: md5,
    },
    Method {
        prefix: b"_",
        nrbytes: 3,
        strong: false,
        gensalt: crate::des::gensalt_bsdi,
    },
    // bigcrypt's row, which libxcrypt matches before traditional DES's: its
    // gensalt is DES's, and so it is the one either is asked for by.
    Method {
        prefix: b"",
        nrbytes: 2,
        strong: false,
        gensalt: crate::des::gensalt,
    },
];

/// `get_hashfn`: the method whose prefix `setting` begins with -- or, for
/// the one with none, whose rule it meets: empty, or two salt characters
/// first.
fn method(setting: &[u8]) -> Option<&'static Method> {
    METHODS.iter().find(|m| {
        if m.prefix.is_empty() {
            crate::des::names_unprefixed(setting)
        } else {
            setting.starts_with(m.prefix)
        }
    })
}

/// The crypt base-64 alphabet the SHA and MD5 salts are written in.
const ITOA64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `gensalt_sha_rn`: `$<tag>$`, a `rounds=` field when `count` -- clamped
/// to its bounds, `defcount` when 0 -- is not the default, and up to
/// `maxsalt` characters of salt, four for every three random bytes, while
/// there are more than three left.
#[allow(clippy::too_many_arguments)] // gensalt_sha_rn's own parameters
fn gensalt_sha(
    tag: u8,
    maxsalt: usize,
    defcount: u64,
    mincount: u64,
    maxcount: u64,
    count: u64,
    rbytes: &[u8],
    out: &mut [u8],
) -> Result<usize, Refused> {
    if rbytes.len() < 3 {
        return Err(Refused::Invalid);
    }
    let count = if count == 0 { defcount } else { count }.clamp(mincount, maxcount);
    // "$x$ssss" and a NUL; and "rounds=N$" when it is there.
    let mut needed = 8;
    let mut digits = [0u8; 20];
    let mut ndigits = 0;
    if count != defcount {
        let mut v = count;
        loop {
            digits[ndigits] = b'0' + (v % 10) as u8;
            ndigits += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        needed += 8 + ndigits;
    }
    if out.len() < needed {
        return Err(Refused::Range);
    }
    out[..3].copy_from_slice(&[b'$', tag, b'$']);
    let mut written = 3;
    if count != defcount {
        out[3..10].copy_from_slice(b"rounds=");
        written = 10;
        for &d in digits[..ndigits].iter().rev() {
            out[written] = d;
            written += 1;
        }
        out[written] = b'$';
        written += 1;
    }
    // libxcrypt asserts this, and aborts: its room check above is a byte
    // short of what the salt below wants, so a buffer of exactly `needed`
    // bytes passes it and then fails the assertion (Ubuntu's build keeps
    // assertions).  Refused here instead: without it, the setting would
    // have no salt at all.
    if written + 5 >= out.len() {
        return Err(Refused::Range);
    }
    let mut used = 0;
    while written + 5 < out.len() && used + 3 < rbytes.len() && used * 4 / 3 < maxsalt {
        let value = u32::from(rbytes[used])
            | (u32::from(rbytes[used + 1]) << 8)
            | (u32::from(rbytes[used + 2]) << 16);
        for k in 0..4 {
            out[written + k] = ITOA64[((value >> (6 * k)) & 0x3f) as usize];
        }
        written += 4;
        used += 3;
    }
    Ok(written)
}

/// `crypt_gensalt_rn`'s work, in Rust: the setting `prefix` names (the
/// preferred method's when `None`), at cost `count`, from `rbytes` (random
/// ones when `None`), into `out` with its NUL -- or why not.  `out` keeps
/// what it had on a failure.
pub(crate) fn gensalt_into(
    prefix: Option<&[u8]>,
    count: u64,
    rbytes: Option<&[u8]>,
    out: &mut [u8],
) -> Result<usize, Refused> {
    let prefix = prefix.unwrap_or(&PREFERRED[..3]);
    let method = method(prefix).ok_or(Refused::Invalid)?;
    let mut own = [0u8; 255];
    let random = match rbytes {
        Some(given) => given,
        None => {
            // SAFETY: `own` is writable for its 255 bytes, more than any
            // method's `nrbytes`.
            unsafe { crate::random::arc4random_buf(own.as_mut_ptr(), method.nrbytes) };
            &own[..method.nrbytes]
        }
    };
    // Into a buffer of the most room any setting needs, so that a failure
    // half-way leaves `out` as it was: libxcrypt's methods build theirs
    // apart, too.  Its room is the smaller of the two, as libxcrypt's
    // checks are.
    let mut made = [0u8; CRYPT_GENSALT_OUTPUT_SIZE];
    let room = out.len().min(CRYPT_GENSALT_OUTPUT_SIZE);
    let result = (method.gensalt)(count, random, &mut made[..room]);
    crate::crypt::wipe(&mut own);
    let len = result?;
    let with_nul = out.get_mut(..=len).ok_or(Refused::Range)?;
    with_nul[..len].copy_from_slice(&made[..len]);
    with_nul[len] = 0;
    Ok(len)
}

/// `crypt_gensalt_rn` -- a new password's setting, for `crypt`.
///
/// `prefix` names the method (`"$y$"`, `"$2b$"`, `"$6$"` ...; NULL for the
/// preferred, yescrypt); `count` its cost (0 for its default); `rbytes`
/// `nrbytes` random bytes to salt it with (NULL for the system's own).  The
/// setting goes into `output`'s `output_size` bytes, at least
/// `CRYPT_GENSALT_OUTPUT_SIZE` to be sure of room, and `output` is
/// returned; on failure NULL, with `errno` set, and `output` holds a
/// failure token, which no method takes.
///
/// # Safety
///
/// `prefix` is NULL or a C string; `rbytes` is NULL or readable for
/// `nrbytes` bytes; `output` is writable for `output_size` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn crypt_gensalt_rn(
    prefix: *const u8,
    count: u64,
    rbytes: *const u8,
    nrbytes: i32,
    output: *mut u8,
    output_size: i32,
) -> *mut u8 {
    if output.is_null() && output_size > 0 {
        // libxcrypt faults here, writing its failure token: a crash is the
        // one answer this cannot give, so `EFAULT`, as `crypt_r` answers
        // for a NULL `data`.  (Given no room, it writes no token and
        // answers `ERANGE`, below, as this does.)
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's `output_size` writable bytes at `output`; none
    // are written when `output_size` is not positive, as when it is NULL.
    unsafe { crate::crypt::write_failure_token(b"", output, output_size) };
    // Every setting is at least three bytes: a DES salt and its NUL.
    let Ok(size @ 3..) = usize::try_from(output_size) else {
        errno::set_errno(errno::ERANGE);
        return core::ptr::null_mut();
    };
    // SAFETY: as above; the slice is the caller's buffer, nothing else
    // refers to it while this runs.
    let out = unsafe { core::slice::from_raw_parts_mut(output, size) };
    // SAFETY: NULL, or the caller's C string.
    let prefix = (!prefix.is_null()).then(|| unsafe { crate::crypt::cstr_slice(prefix) });
    let rbytes = if rbytes.is_null() {
        None
    } else {
        // libxcrypt casts a negative count to a huge size and reads past
        // the bytes; this refuses it.
        let Ok(n) = usize::try_from(nrbytes) else {
            errno::set_errno(errno::EINVAL);
            return core::ptr::null_mut();
        };
        // SAFETY: the caller's `nrbytes` readable bytes at `rbytes`.
        Some(unsafe { core::slice::from_raw_parts(rbytes, n) })
    };
    match gensalt_into(prefix, count, rbytes, out) {
        Ok(_) => output,
        Err(refused) => {
            errno::set_errno(match refused {
                Refused::Range => errno::ERANGE,
                Refused::Invalid => errno::EINVAL,
            });
            core::ptr::null_mut()
        }
    }
}

/// `crypt_gensalt` -- as [`crypt_gensalt_rn`], into a static buffer that
/// the next call overwrites (and that `crypt`'s does not).
///
/// # Safety
///
/// As [`crypt_gensalt_rn`]'s, for `prefix` and `rbytes`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn crypt_gensalt(
    prefix: *const u8,
    count: u64,
    rbytes: *const u8,
    nrbytes: i32,
) -> *mut u8 {
    static mut OUTPUT: [u8; CRYPT_GENSALT_OUTPUT_SIZE] = [0; CRYPT_GENSALT_OUTPUT_SIZE];
    // SAFETY: the static's own bytes, its size given; as POSIX's `crypt`'s
    // buffer is, it is the caller's to not share between threads.
    unsafe {
        crypt_gensalt_rn(
            prefix,
            count,
            rbytes,
            nrbytes,
            (&raw mut OUTPUT).cast::<u8>(),
            CRYPT_GENSALT_OUTPUT_SIZE as i32,
        )
    }
}

/// `crypt_gensalt_ra` -- as [`crypt_gensalt_rn`], into memory from
/// `malloc`, the caller's to `free`.  NULL on failure, the memory freed.
///
/// # Safety
///
/// As [`crypt_gensalt_rn`]'s, for `prefix` and `rbytes`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn crypt_gensalt_ra(
    prefix: *const u8,
    count: u64,
    rbytes: *const u8,
    nrbytes: i32,
) -> *mut u8 {
    let output = crate::malloc::malloc(CRYPT_GENSALT_OUTPUT_SIZE);
    if output.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: `output` is `CRYPT_GENSALT_OUTPUT_SIZE` bytes of ours.
    let result = unsafe {
        crypt_gensalt_rn(
            prefix,
            count,
            rbytes,
            nrbytes,
            output,
            CRYPT_GENSALT_OUTPUT_SIZE as i32,
        )
    };
    if result.is_null() {
        // SAFETY: `output` came from `malloc` above and is ours to free.
        unsafe { crate::malloc::free(output) };
    }
    result
}

/// `crypt_checksalt`'s answer for `setting`, in Rust.
pub(crate) fn checksalt(setting: Option<&[u8]>) -> i32 {
    let Some(setting) = setting else {
        return CRYPT_SALT_INVALID;
    };
    if setting.is_empty() || crate::crypt::has_bad_setting_chars(setting) {
        return CRYPT_SALT_INVALID;
    }
    match method(setting) {
        None => CRYPT_SALT_INVALID,
        Some(m) if m.strong => CRYPT_SALT_OK,
        Some(_) => CRYPT_SALT_METHOD_LEGACY,
    }
}

/// `crypt_checksalt` -- whether `setting` names a method this library
/// hashes, and whether it is one for new passwords: `CRYPT_SALT_OK`,
/// `CRYPT_SALT_METHOD_LEGACY` (SHA-256, MD5, `$2x$`, the DES methods), or
/// `CRYPT_SALT_INVALID` (NULL, empty, a character no setting has, or no
/// method).  It reads the prefix only, as libxcrypt's does: the rest is
/// `crypt`'s to judge.
///
/// # Safety
///
/// `setting` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn crypt_checksalt(setting: *const u8) -> i32 {
    // SAFETY: NULL, or the caller's C string.
    checksalt((!setting.is_null()).then(|| unsafe { crate::crypt::cstr_slice(setting) }))
}

/// `crypt_preferred_method` -- the prefix `crypt_gensalt` uses when given
/// none: `"$y$"`, yescrypt, as libxcrypt's.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crypt_preferred_method() -> *const u8 {
    PREFERRED.as_ptr()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(
        prefix: Option<&[u8]>,
        count: u64,
        rbytes: Option<&[u8]>,
    ) -> Result<std::string::String, Refused> {
        let mut out = [0x55u8; CRYPT_GENSALT_OUTPUT_SIZE];
        let n = gensalt_into(prefix, count, rbytes, &mut out)?;
        assert_eq!(out[n], 0, "NUL-terminated");
        Ok(std::string::String::from_utf8(out[..n].to_vec()).unwrap())
    }

    /// The settings each method makes from given bytes, and what `crypt`
    /// makes of them: a hash, by the method named.  (libxcrypt's own
    /// answers, byte for byte, are `crypt.rs`'s `libxcrypt_answers`.)
    #[test]
    fn each_method_makes_a_setting_crypt_takes() {
        let bytes: std::vec::Vec<u8> = (1..=64).collect();
        // The prefix, and what the setting begins with: for traditional DES,
        // which any two salt characters ask for, its salt, from the bytes.
        for (prefix, count, begins) in [
            (&b"$y$"[..], 0, &b"$y$"[..]),
            (b"$y$", 1, b"$y$"),
            (b"$7$", 6, b"$7$"),
            (b"$2b$", 4, b"$2b$"),
            (b"$2y$", 4, b"$2y$"),
            (b"$2a$", 4, b"$2a$"),
            (b"$6$", 0, b"$6$"),
            (b"$6$", 1000, b"$6$"),
            (b"$5$", 5000, b"$5$"),
            (b"$1$", 0, b"$1$"),
            (b"_", 0, b"_J9.."),
            (b"_", 5000, b"_7C/."),
            (b"", 0, b"/0"),
            (b"ab", 0, b"/0"),
        ] {
            let setting = made(Some(prefix), count, Some(&bytes)).unwrap();
            assert!(setting.as_bytes().starts_with(begins), "{setting}");
            let mut out = crate::crypt::buf();
            let hashed = crate::crypt::hash_into(b"pw", setting.as_bytes(), &mut out);
            assert!(hashed.is_some(), "{setting} hashes");
        }
    }

    /// The preferred method is yescrypt at its default cost; and the
    /// system's random bytes salt it when given none, differently each time.
    #[test]
    fn the_defaults() {
        let first = made(None, 0, None).unwrap();
        let second = made(None, 0, None).unwrap();
        assert!(first.starts_with("$y$j9T$"), "{first}");
        assert_eq!(first.len(), "$y$j9T$".len() + 22);
        assert_ne!(first, second);
        // SAFETY: the returned pointer is to a static C string.
        let preferred = unsafe { core::ffi::CStr::from_ptr(crypt_preferred_method().cast()) };
        assert_eq!(preferred.to_bytes(), b"$y$");
    }

    /// The refusals, and that a refused setting leaves the output as it was.
    #[test]
    fn refusals() {
        let bytes = [7u8; 64];
        for (prefix, count, n) in [
            (&b"$2x$"[..], 0, 16),
            (b"$1$", 5, 16),
            (b"$y$", 12, 16),
            (b"$y$", 0, 15),
            (b"$7$", 5, 16),
            (b"$2b$", 3, 16),
            (b"$2b$", 32, 16),
            (b"$2b$", 5, 15),
            (b"$6$", 0, 2),
            (b"$gy$", 0, 16),
            (b"_", 0, 2),
            (b"", 1, 16),
            (b"ab", 0, 1),
            (b"a", 0, 16),
            (b"a$", 0, 16),
        ] {
            assert_eq!(
                made(Some(prefix), count, Some(&bytes[..n])),
                Err(Refused::Invalid),
                "{prefix:?} {count} {n}"
            );
        }
        let mut small = [b'x'; 40];
        assert_eq!(
            gensalt_into(Some(b"$y$"), 0, Some(&bytes[..16]), &mut small),
            Err(Refused::Range)
        );
        assert_eq!(small, [b'x'; 40], "untouched");
    }

    /// A probe's string argument: NULL for `NULL`, empty for `-`.
    fn c_arg(word: &str) -> Option<std::ffi::CString> {
        match word {
            "NULL" => None,
            "-" => Some(std::ffi::CString::default()),
            other => Some(std::ffi::CString::new(other).unwrap()),
        }
    }

    fn ptr(arg: Option<&std::ffi::CString>) -> *const u8 {
        arg.map_or(core::ptr::null(), |c| c.as_ptr().cast())
    }

    /// What `crypt_gensalt_rn` answers for a method this library has not:
    /// NULL, `ERANGE` below the three bytes every setting needs and
    /// `EINVAL` from there -- libxcrypt's order of checks -- and the
    /// failure token, as much of it as there is room for.
    fn no_method(size: i32) -> (i32, &'static str) {
        match size {
            ..=1 => (errno::ERANGE, ""),
            2 => (errno::ERANGE, "*"),
            _ => (errno::EINVAL, "*0"),
        }
    }

    /// libxcrypt's own answers (`posix/tools/oracle/crypt_harness.py`):
    /// `crypt_gensalt_rn` at every method, cost, count of random bytes and
    /// room -- the setting, `errno`, and what the buffer held after --
    /// `crypt_checksalt` and `crypt_preferred_method`.  For the methods this
    /// library has not (gost-yescrypt, NT, SunMD5, sha1crypt) the answer
    /// is [`no_method`]'s, or `CRYPT_SALT_INVALID`, where
    /// libxcrypt's may be a setting.  (The probes that make libxcrypt abort
    /// are left out of the oracle; `where_libxcrypt_aborts` has this
    /// library's answer to them.)
    #[test]
    fn libxcrypt_answers() {
        const ORACLE: &str = include_str!("gensalt_oracle.txt");
        let mut lines = 0;
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            lines += 1;
            let (probe, answer) = line.split_once(" = ").unwrap();
            let words: std::vec::Vec<&str> = probe.split(' ').collect();
            match words[0] {
                "gensalt" => {
                    let prefix = c_arg(words[1]);
                    let count: u64 = words[2].parse().unwrap();
                    // The harness never asks for the system's random bytes:
                    // libxcrypt's answer would be its own.
                    let rbytes: std::vec::Vec<u8> = match words[3] {
                        "-" => std::vec::Vec::new(),
                        hex => (0..hex.len())
                            .step_by(2)
                            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                            .collect(),
                    };
                    let nrbytes: i32 = words[4].parse().unwrap();
                    assert_eq!(usize::try_from(nrbytes).unwrap(), rbytes.len(), "{line}");
                    let size: i32 = words[5].parse().unwrap();
                    let (result, rest) = answer.split_once(' ').unwrap();
                    let (errno_name, held_after) = rest.split_once(" out=").unwrap();

                    // As the harness's: 'Q's and a NUL.
                    let mut output = [b'Q'; 256];
                    output[255] = 0;
                    let base = output.as_mut_ptr();
                    errno::set_errno(0);
                    // SAFETY: the probe's C string or NULL, `nrbytes`
                    // bytes, and a buffer of more than any probe's size.
                    let got = unsafe {
                        crypt_gensalt_rn(
                            ptr(prefix.as_ref()),
                            count,
                            rbytes.as_ptr(),
                            nrbytes,
                            base,
                            size,
                        )
                    };
                    let got_errno = errno::get_errno();
                    let end = output.iter().position(|&b| b == 0).unwrap();
                    let held = core::str::from_utf8(&output[..end]).unwrap();
                    if size <= 0 {
                        assert_eq!(end, 255, "untouched: {line}");
                    }

                    let named = prefix.as_ref().map_or(&PREFERRED[..3], |p| p.as_bytes());
                    if method(named).is_none() {
                        let (want_errno, token) = no_method(size);
                        assert!(got.is_null(), "{line}");
                        assert_eq!(got_errno, want_errno, "{line}");
                        if size > 0 {
                            assert_eq!(held, token, "{line}");
                        }
                        continue;
                    }
                    if result == "NULL" {
                        assert!(got.is_null(), "{line}");
                    } else {
                        assert_eq!(got, base, "{line}");
                        assert_eq!(held, result, "{line}");
                    }
                    let want_errno = match errno_name {
                        "0" => 0,
                        "EINVAL" => errno::EINVAL,
                        "ERANGE" => errno::ERANGE,
                        other => panic!("an errno this test does not know: {other}"),
                    };
                    assert_eq!(got_errno, want_errno, "{line}");
                    if size > 0 {
                        assert_eq!(held, held_after, "{line}");
                    }
                }
                "checksalt" => {
                    let setting = c_arg(words[1]);
                    let want: i32 = answer.parse().unwrap();
                    // SAFETY: the probe's string, or NULL.
                    let got = unsafe { crypt_checksalt(ptr(setting.as_ref())) };
                    let named = setting
                        .as_ref()
                        .is_some_and(|s| method(s.as_bytes()).is_some());
                    let want = if named { want } else { CRYPT_SALT_INVALID };
                    assert_eq!(got, want, "{line}");
                }
                "preferred" => {
                    // SAFETY: a static C string.
                    let got = unsafe { core::ffi::CStr::from_ptr(crypt_preferred_method().cast()) };
                    assert_eq!(got.to_str().unwrap(), answer);
                }
                other => panic!("a probe this test does not know: {other}"),
            }
        }
        assert_eq!(lines, 836, "the oracle's every line");
    }

    /// Where libxcrypt aborts -- a buffer exactly the length its room check
    /// computes for a SHA or MD5 setting, which its salt loop's assertion
    /// refuses -- this refuses with ERANGE, the failure token in the buffer.
    #[test]
    fn where_libxcrypt_aborts() {
        let bytes = [9u8; 16];
        for (prefix, count, size) in [
            (&b"$6$\0"[..], 0, 8),
            (b"$5$\0", 0, 8),
            (b"$1$\0", 0, 8),
            (b"$6$\0", 999_999_999, 25),
            // 1000's four digits, which libxcrypt counts as three: its
            // room check passes 19 bytes as well as 20.
            (b"$5$\0", 1000, 19),
            (b"$5$\0", 1000, 20),
        ] {
            let mut out = std::vec![b'Q'; size];
            errno::set_errno(0);
            // SAFETY: a C string, the 16 bytes, and `size` bytes of room.
            let got = unsafe {
                crypt_gensalt_rn(
                    prefix.as_ptr(),
                    count,
                    bytes.as_ptr(),
                    16,
                    out.as_mut_ptr(),
                    i32::try_from(size).unwrap(),
                )
            };
            assert!(got.is_null(), "{prefix:?} {count} {size}");
            assert_eq!(
                errno::get_errno(),
                errno::ERANGE,
                "{prefix:?} {count} {size}"
            );
            assert_eq!(&out[..3], b"*0\0", "{prefix:?} {count} {size}");
        }
        // A byte more, and there is room for four characters of salt.
        let mut out = [0u8; 26];
        let n = gensalt_into(Some(b"$6$"), 999_999_999, Some(&bytes), &mut out).unwrap();
        assert_eq!(&out[..=n], b"$6$rounds=999999999$7YE0\0");
    }

    /// A NULL buffer: `ERANGE` where libxcrypt has no room to write its
    /// token into and so writes none; `EFAULT` where it faults writing one.
    #[test]
    fn into_null() {
        for (size, want) in [
            (0, errno::ERANGE),
            (-1, errno::ERANGE),
            (1, errno::EFAULT),
            (192, errno::EFAULT),
        ] {
            errno::set_errno(0);
            // SAFETY: NULL with no room, or NULL refused before a write.
            let got = unsafe {
                crypt_gensalt_rn(
                    core::ptr::null(),
                    0,
                    core::ptr::null(),
                    0,
                    core::ptr::null_mut(),
                    size,
                )
            };
            assert!(got.is_null(), "{size}");
            assert_eq!(errno::get_errno(), want, "{size}");
        }
    }

    /// `crypt_gensalt` and `crypt_gensalt_ra` are `crypt_gensalt_rn` into a
    /// static buffer and into `malloc`'s -- and NULL with `errno` where it
    /// is, `_ra`'s memory freed.
    #[test]
    fn the_static_and_the_allocated_forms() {
        let bytes = [3u8; 16];
        let mut out = [0u8; CRYPT_GENSALT_OUTPUT_SIZE];
        // SAFETY: a C string, 16 bytes, and the buffer's room.
        let made = unsafe {
            crypt_gensalt_rn(
                b"$2b$\0".as_ptr(),
                7,
                bytes.as_ptr(),
                16,
                out.as_mut_ptr(),
                i32::try_from(out.len()).unwrap(),
            )
        };
        assert_eq!(made, out.as_mut_ptr());
        let want = core::ffi::CStr::from_bytes_until_nul(&out)
            .unwrap()
            .to_bytes();
        assert!(want.starts_with(b"$2b$07$"));

        // SAFETY: as above; the static buffer is this test's alone.
        let fixed = unsafe { crypt_gensalt(b"$2b$\0".as_ptr(), 7, bytes.as_ptr(), 16) };
        // SAFETY: the static buffer's C string.
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(fixed.cast()) }.to_bytes(),
            want
        );
        // SAFETY: as above.
        let allocated = unsafe { crypt_gensalt_ra(b"$2b$\0".as_ptr(), 7, bytes.as_ptr(), 16) };
        assert!(!allocated.is_null());
        // SAFETY: `malloc`'s memory, holding the setting's C string.
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(allocated.cast()) }.to_bytes(),
            want
        );
        // SAFETY: crypt_gensalt_ra's allocation, the caller's to free, once.
        unsafe { crate::malloc::free(allocated) };

        // A cost bcrypt does not take.
        errno::set_errno(0);
        // SAFETY: as above.
        assert!(unsafe { crypt_gensalt(b"$2b$\0".as_ptr(), 3, bytes.as_ptr(), 16) }.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        // SAFETY: as above.
        assert!(unsafe { crypt_gensalt_ra(b"$2b$\0".as_ptr(), 3, bytes.as_ptr(), 16) }.is_null());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn checksalt_judges_the_prefix() {
        assert_eq!(checksalt(Some(b"$y$j9T$abc")), CRYPT_SALT_OK);
        assert_eq!(checksalt(Some(b"$6$salt")), CRYPT_SALT_OK);
        assert_eq!(checksalt(Some(b"$2b$05$whatever")), CRYPT_SALT_OK);
        assert_eq!(checksalt(Some(b"$5$salt")), CRYPT_SALT_METHOD_LEGACY);
        assert_eq!(checksalt(Some(b"$1$salt")), CRYPT_SALT_METHOD_LEGACY);
        assert_eq!(
            checksalt(Some(b"$2x$05$whatever")),
            CRYPT_SALT_METHOD_LEGACY
        );
        assert_eq!(checksalt(Some(b"ab")), CRYPT_SALT_METHOD_LEGACY);
        assert_eq!(checksalt(Some(b"abHashHashHas")), CRYPT_SALT_METHOD_LEGACY);
        assert_eq!(checksalt(Some(b"_J9..abcd")), CRYPT_SALT_METHOD_LEGACY);
        assert_eq!(checksalt(Some(b"$gy$j9T$abc")), CRYPT_SALT_INVALID);
        assert_eq!(checksalt(Some(b"a")), CRYPT_SALT_INVALID);
        assert_eq!(checksalt(Some(b"a$")), CRYPT_SALT_INVALID);
        assert_eq!(checksalt(Some(b"")), CRYPT_SALT_INVALID);
        assert_eq!(checksalt(Some(b"$6$a b")), CRYPT_SALT_INVALID);
        assert_eq!(checksalt(None), CRYPT_SALT_INVALID);
    }
}
