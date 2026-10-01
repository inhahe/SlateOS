//! Secure Boot — which images may run, by their SHA-256 hash.
//!
//! Holds the boot state and the enrolled entries (UEFI's `PK`, `KEK`, `db`,
//! `dbx`, and shim's `MOK`), and answers one question: may an image with this
//! hash run?
//!
//! ## What a verdict is, and what it is not
//!
//! UEFI's `db` and `dbx` hold two kinds of entry: X.509 certificates, which
//! authenticate a signed image, and plain SHA-256 image hashes
//! (`EFI_CERT_SHA256_GUID`). This module checks the second kind only. A
//! certificate check needs an X.509 parser and signature verification, which
//! the kernel does not have and should not grow on attacker-supplied bytes; a
//! hash list needs nothing but a comparison. So [`verify_image`] answers:
//!
//! * the hash is in `dbx` → **forbidden** (`dbx` wins over `db`, as in UEFI);
//! * it is in `db` (or in `MOK`, as shim treats its list) → **allowed**;
//! * it is in neither → **unlisted**.
//!
//! Whether that lets the image run depends on the state: `Enabled` and
//! `EnforcingStrict` enforce -- only an allowed image runs, and an unlisted
//! one is refused, because the signature check that might have admitted it is
//! not implemented and a verifier must not pass what it cannot check.
//! `Disabled` and `SetupMode` do not enforce: every image may run, and the
//! verdict still says honestly what the lists hold.
//!
//! `PK` and `KEK` entries authenticate updates to `db`/`dbx` in firmware; they
//! are recorded here, and never match an image.
//!
//! Until 2026-09-27 (design-decisions §978, the operator's answer to A-Q21)
//! `verify_image` never read the hash it was given: it passed every image when
//! not enforcing, and when enforcing passed every image if any `db` entry
//! existed -- and the default table enrolled one, a placeholder fingerprint.
//! The first thing done to wire this module in was to make it check.
//!
//! ## Doors
//!
//! `SYS_SECUREBOOT_ENROLL` / `_REMOVE` (gated on
//! [`crate::cap::Rights::ENROLL_SECUREBOOT`]) and `SYS_SECUREBOOT_VERIFY`
//! (unprivileged), for `userspace/sbctl`. `/proc/secureboot` publishes the
//! state, the entries and the counts. The kernel never parses a certificate:
//! the caller computes the fingerprint or digest and passes it.

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Secure boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootState {
    Disabled,
    SetupMode,
    Enabled,
    EnforcingStrict,
}

impl BootState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::SetupMode => "Setup Mode",
            Self::Enabled => "Enabled",
            Self::EnforcingStrict => "Enforcing (Strict)",
        }
    }

    /// Whether this state refuses an image that is not allowed.
    #[must_use]
    pub fn enforces(self) -> bool {
        matches!(self, Self::Enabled | Self::EnforcingStrict)
    }
}

/// Key type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    PlatformKey,        // PK.
    KeyExchangeKey,     // KEK.
    SignatureDatabase,  // db.
    ForbiddenSignature, // dbx.
    MachineOwnerKey,    // MOK.
}

impl KeyType {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlatformKey => "PK",
            Self::KeyExchangeKey => "KEK",
            Self::SignatureDatabase => "db",
            Self::ForbiddenSignature => "dbx",
            Self::MachineOwnerKey => "MOK",
        }
    }

    /// The type named by `code` in the syscall ABI: 0 PK, 1 KEK, 2 db,
    /// 3 dbx, 4 MOK. `None` for any other value -- refused, never mapped to
    /// a default.
    #[must_use]
    pub fn from_code(code: u64) -> Option<Self> {
        match code {
            0 => Some(Self::PlatformKey),
            1 => Some(Self::KeyExchangeKey),
            2 => Some(Self::SignatureDatabase),
            3 => Some(Self::ForbiddenSignature),
            4 => Some(Self::MachineOwnerKey),
            _ => None,
        }
    }
}

/// An enrolled entry.
#[derive(Debug, Clone)]
pub struct EnrolledKey {
    pub id: u32,
    pub key_type: KeyType,
    pub subject: String,
    /// `SHA256:` and 64 lowercase hex digits: a certificate's fingerprint for
    /// `PK`/`KEK`, an image's hash for a `db`/`dbx`/`MOK` hash entry.
    pub fingerprint: String,
    pub enrolled_ns: u64,
}

/// Which list, if any, names an image's hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// In `db` or `MOK`: the entry's id.
    Allowed(u32),
    /// In `dbx`: the entry's id. Wins over an `Allowed` entry for the same
    /// hash.
    Forbidden(u32),
    /// In neither.
    Unlisted,
}

impl Listing {
    /// The syscall ABI's code: 0 allowed, 1 forbidden, 2 unlisted.
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Allowed(_) => 0,
            Self::Forbidden(_) => 1,
            Self::Unlisted => 2,
        }
    }

    /// The entry that decided it, if any.
    #[must_use]
    pub fn key_id(self) -> Option<u32> {
        match self {
            Self::Allowed(id) | Self::Forbidden(id) => Some(id),
            Self::Unlisted => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Allowed(_) => "allowed",
            Self::Forbidden(_) => "forbidden",
            Self::Unlisted => "unlisted",
        }
    }
}

/// A verification's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub listing: Listing,
    /// Whether the state at the time enforced.
    pub enforced: bool,
    /// Whether the image may run: when enforcing, only if allowed; when not,
    /// always.
    pub allowed: bool,
}

/// A boot verification record.
#[derive(Debug, Clone)]
pub struct VerifyRecord {
    /// The image's name as the caller gave it: bytes, since it may be a path,
    /// and a path is not forced into UTF-8 (see [`name_display`]).
    pub image_name: Vec<u8>,
    /// The hash as normalised: `SHA256:` and 64 lowercase hex digits.
    pub hash: String,
    pub verdict: Verdict,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_KEYS: usize = 100;
const MAX_RECORDS: usize = 200;

/// Longest subject accepted, in bytes.
pub const MAX_SUBJECT: usize = 256;

/// Longest image name accepted, in bytes.
pub const MAX_IMAGE_NAME: usize = 256;

/// Longest hash argument: `SHA256:` and 64 hex digits.
pub const MAX_HASH_ARG: usize = 71;

/// An image name for display: UTF-8 as it is, any other byte as `\xNN`, and a
/// backslash as `\\` so the two cannot be confused -- a name that is not UTF-8
/// is shown exactly rather than replaced.
#[must_use]
pub fn name_display(name: &[u8]) -> String {
    let mut out = String::with_capacity(name.len());
    for chunk in name.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '\\' {
                out.push_str("\\\\");
            } else {
                out.push(c);
            }
        }
        for b in chunk.invalid() {
            out.push_str(&alloc::format!("\\x{b:02x}"));
        }
    }
    out
}

struct State {
    boot_state: BootState,
    keys: Vec<EnrolledKey>,
    records: Vec<VerifyRecord>,
    next_key_id: u32,
    total_verified: u64,
    total_rejected: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops = state.ops.saturating_add(1);
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

/// `hash` as `SHA256:` and 64 lowercase hex digits, from either that form (any
/// case, the prefix optional) or the bare digits.
///
/// # Errors
///
/// `InvalidArgument` for anything else: a hash that cannot be read cannot be
/// matched, and treating it as unlisted would hide the caller's mistake.
pub fn normalize_sha256(hash: &str) -> KernelResult<String> {
    let digits = hash
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("sha256:"))
        .map_or(hash, |_| hash.get(7..).unwrap_or(""));
    if digits.len() != 64 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(KernelError::InvalidArgument);
    }
    let mut out = String::with_capacity(71);
    out.push_str("SHA256:");
    out.push_str(&digits.to_ascii_lowercase());
    Ok(out)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create the table if it does not exist: `Disabled`, and no entries.
///
/// Until 2026-09-27 it held three placeholder entries -- fingerprints like
/// `SHA256:aabb...` -- and the `db` one was what made the old verifier pass
/// every image when enforcing. A key table starts empty, as firmware in setup
/// mode does.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        boot_state: BootState::Disabled,
        keys: Vec::new(),
        records: Vec::new(),
        next_key_id: 1,
        total_verified: 0,
        total_rejected: 0,
        ops: 0,
    });
}

/// Set boot state.
///
/// # Errors
///
/// `NotSupported` before [`init_defaults`].
pub fn set_state(state_val: BootState) -> KernelResult<()> {
    with_state(|state| {
        state.boot_state = state_val;
        Ok(())
    })
}

/// Get boot state.
pub fn get_state() -> BootState {
    STATE
        .lock()
        .as_ref()
        .map_or(BootState::Disabled, |s| s.boot_state)
}

/// Enrol an entry. `fingerprint` is normalised by [`normalize_sha256`].
///
/// # Errors
///
/// `InvalidArgument` for a fingerprint that is not a SHA-256 value, or a
/// subject that is empty or longer than [`MAX_SUBJECT`]; `AlreadyExists` for
/// an entry of the same type and fingerprint; `ResourceExhausted` when the
/// table is full; `NotSupported` before [`init_defaults`].
pub fn enroll_key(key_type: KeyType, subject: &str, fingerprint: &str) -> KernelResult<u32> {
    let fingerprint = normalize_sha256(fingerprint)?;
    if subject.is_empty() || subject.len() > MAX_SUBJECT {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state
            .keys
            .iter()
            .any(|k| k.key_type == key_type && k.fingerprint == fingerprint)
        {
            return Err(KernelError::AlreadyExists);
        }
        if state.keys.len() >= MAX_KEYS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_key_id;
        state.next_key_id = state
            .next_key_id
            .checked_add(1)
            .ok_or(KernelError::ResourceExhausted)?;
        state.keys.push(EnrolledKey {
            id,
            key_type,
            subject: String::from(subject),
            fingerprint,
            enrolled_ns: crate::hpet::elapsed_ns(),
        });
        Ok(id)
    })
}

/// Remove an entry.
///
/// # Errors
///
/// `NotFound` for an unknown id; `NotSupported` before [`init_defaults`].
pub fn remove_key(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.keys.len();
        state.keys.retain(|k| k.id != id);
        if state.keys.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// Which list names `hash` (already normalised).
fn listing_of(keys: &[EnrolledKey], hash: &str) -> Listing {
    if let Some(k) = keys
        .iter()
        .find(|k| k.key_type == KeyType::ForbiddenSignature && k.fingerprint == hash)
    {
        return Listing::Forbidden(k.id);
    }
    if let Some(k) = keys.iter().find(|k| {
        matches!(k.key_type, KeyType::SignatureDatabase | KeyType::MachineOwnerKey)
            && k.fingerprint == hash
    }) {
        return Listing::Allowed(k.id);
    }
    Listing::Unlisted
}

/// Verify an image by its SHA-256 `hash` (see the module docs for what the
/// verdict means), and record it.
///
/// # Errors
///
/// `InvalidArgument` for a hash that is not a SHA-256 value or an image name
/// longer than [`MAX_IMAGE_NAME`]; `NotSupported` before [`init_defaults`].
pub fn verify_image(image_name: &[u8], hash: &str) -> KernelResult<Verdict> {
    let hash = normalize_sha256(hash)?;
    if image_name.len() > MAX_IMAGE_NAME {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let listing = listing_of(&state.keys, &hash);
        let enforced = state.boot_state.enforces();
        let allowed = !enforced || matches!(listing, Listing::Allowed(_));
        let verdict = Verdict {
            listing,
            enforced,
            allowed,
        };
        if allowed {
            state.total_verified = state.total_verified.saturating_add(1);
        } else {
            state.total_rejected = state.total_rejected.saturating_add(1);
        }
        if state.records.len() >= MAX_RECORDS {
            state.records.remove(0);
        }
        state.records.push(VerifyRecord {
            image_name: image_name.to_vec(),
            hash,
            verdict,
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        Ok(verdict)
    })
}

/// List enrolled keys.
pub fn list_keys() -> Vec<EnrolledKey> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.keys.clone())
}

/// Get verification records, newest first.
pub fn get_records(max: usize) -> Vec<VerifyRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut r = s.records.clone();
        r.reverse();
        r.truncate(max);
        r
    })
}

/// Statistics: (key_count, record_count, total_verified, total_rejected, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.keys.len(),
            s.records.len(),
            s.total_verified,
            s.total_rejected,
            s.ops,
        ),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() -> crate::error::KernelResult<()> {
    with_scratch_table(self_test_inner)
}

/// Run `body` against a fresh table of its own and put the live one back
/// afterwards -- for this module's suite, and for the syscall layer's test of
/// the doors, which would otherwise leave its probes in the user's table.
pub(crate) fn with_scratch_table<R>(body: impl FnOnce() -> R) -> R {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    let result = crate::fs::selftest::with_pristine(&STATE, None, body);
    OPS.store(saved_ops, Ordering::Relaxed);
    result
}

/// Three image hashes for the tests: `H1` enrolled in `db`, `H2` in `dbx`,
/// `H3` in neither.
const H1: &str = "SHA256:1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "SHA256:2222222222222222222222222222222222222222222222222222222222222222";
const H3: &str = "3333333333333333333333333333333333333333333333333333333333333333";

/// A step the suite needs to succeed: a `FAIL:` line naming it and the error
/// otherwise, never a panic -- this suite runs as a Diagnostic self-test, and
/// a panic would take every later rung's result down with it (§914).
fn need<T>(result: KernelResult<T>, step: &str) -> KernelResult<T> {
    result.map_err(|e| {
        crate::serial_println!("  FAIL: secureboot: {} returned {:?}", step, e);
        KernelError::InternalError
    })
}

fn self_test_inner() -> KernelResult<()> {
    use crate::selftest::{check, check_eq};

    crate::serial_println!("secureboot::self_test() — running tests...");
    init_defaults();

    // 1: A fresh table is Disabled and holds no entries -- no placeholders.
    check_eq!(get_state(), BootState::Disabled, "a fresh table's state");
    check!(list_keys().is_empty(), "a fresh table holds an entry");
    crate::serial_println!("  [1/9] defaults (Disabled, no entries): OK");

    // 2: What cannot be read is refused, not treated as unlisted.
    check_eq!(
        enroll_key(KeyType::SignatureDatabase, "placeholder", "SHA256:aabb..."),
        Err(KernelError::InvalidArgument),
        "a truncated fingerprint"
    );
    check_eq!(
        enroll_key(KeyType::SignatureDatabase, "", H1),
        Err(KernelError::InvalidArgument),
        "an empty subject"
    );
    check_eq!(
        verify_image(b"kernel", "SHA256:not-a-hash").map(|v| v.allowed),
        Err(KernelError::InvalidArgument),
        "a hash that is not hex"
    );
    crate::serial_println!("  [2/9] malformed fingerprints and hashes refused: OK");

    // 3: Not enforcing: an unlisted image may run, and the verdict says it
    //    is unlisted rather than pretending it was checked.
    let v = need(verify_image(b"kernel", H3), "verify, disabled")?;
    check_eq!(v.listing, Listing::Unlisted, "an unlisted hash, not enforcing");
    check!(!v.enforced && v.allowed, "not enforcing: {:?}", v);
    crate::serial_println!("  [3/9] not enforcing: unlisted, runs, says so: OK");

    // 4: Enrol a db and a dbx hash; the forms are normalised.
    let db = need(
        enroll_key(KeyType::SignatureDatabase, "kernel build 1", H1),
        "enrol db",
    )?;
    let dbx = need(
        enroll_key(KeyType::ForbiddenSignature, "revoked build", &H2.to_ascii_lowercase()),
        "enrol dbx",
    )?;
    check_eq!(list_keys().len(), 2, "entries after two enrolments");
    check_eq!(
        enroll_key(
            KeyType::SignatureDatabase,
            "again",
            &H1.to_ascii_uppercase().replace("SHA256:", "")
        ),
        Err(KernelError::AlreadyExists),
        "the same db hash in another spelling"
    );
    crate::serial_println!("  [4/9] enrol db and dbx; a duplicate is refused: OK");

    // 5: Enforcing: allowed runs, forbidden and unlisted do not.
    need(set_state(BootState::Enabled), "enable")?;
    let v = need(verify_image(b"kernel", H1), "verify db")?;
    check_eq!(v.listing, Listing::Allowed(db), "a db hash, enforcing");
    check!(v.enforced && v.allowed, "a db hash, enforcing: {:?}", v);
    let v = need(verify_image(b"old-kernel", H2), "verify dbx")?;
    check_eq!(v.listing, Listing::Forbidden(dbx), "a dbx hash, enforcing");
    check!(!v.allowed, "a dbx hash may run: {:?}", v);
    let v = need(verify_image(b"unknown", H3), "verify unlisted")?;
    check_eq!(v.listing, Listing::Unlisted, "an unlisted hash, enforcing");
    check!(!v.allowed, "an unchecked image passed while enforcing");
    crate::serial_println!("  [5/9] enforcing: db runs, dbx and unlisted refused: OK");

    // 6: dbx wins over db for the same hash.
    need(
        enroll_key(KeyType::ForbiddenSignature, "revoke build 1", H1),
        "revoke",
    )?;
    let v = need(verify_image(b"kernel", H1), "verify revoked")?;
    check!(
        matches!(v.listing, Listing::Forbidden(_)) && !v.allowed,
        "a hash in both db and dbx: {:?}",
        v
    );
    crate::serial_println!("  [6/9] dbx wins over db: OK");

    // 7: A PK/KEK entry never matches an image; a MOK hash allows as db does.
    need(enroll_key(KeyType::PlatformKey, "platform", H3), "enrol pk")?;
    check_eq!(
        need(verify_image(b"unknown", H3), "verify a PK-only hash")?.listing,
        Listing::Unlisted,
        "a hash only a PK entry names"
    );
    let mok = need(
        enroll_key(KeyType::MachineOwnerKey, "local module", H3),
        "enrol mok",
    )?;
    check_eq!(
        need(verify_image(b"module", H3), "verify a MOK hash")?.listing,
        Listing::Allowed(mok),
        "a MOK hash"
    );
    crate::serial_println!("  [7/9] PK never matches; MOK allows: OK");

    // 8: Remove, and the entry stops counting.
    need(remove_key(mok), "remove")?;
    check_eq!(
        need(verify_image(b"module", H3), "verify after remove")?.listing,
        Listing::Unlisted,
        "the removed MOK hash"
    );
    check_eq!(remove_key(mok), Err(KernelError::NotFound), "removing twice");
    crate::serial_println!("  [8/9] remove: OK");

    // 9: Records and counts: every verification above, newest first.
    let records = get_records(100);
    check_eq!(records.len(), 8, "records kept");
    let newest = records.first().map(|r| (r.image_name.as_slice(), r.hash.as_str()));
    let h3 = need(normalize_sha256(H3), "normalise H3")?;
    check_eq!(
        newest,
        Some((&b"module"[..], h3.as_str())),
        "the newest record"
    );
    let (keys, recs, verified, rejected, ops) = stats();
    check_eq!((keys, recs), (4, 8), "entries and records counted");
    // Allowed: disabled/unlisted, db, MOK. Refused: dbx, unlisted, revoked,
    // PK-only, removed-MOK.
    check_eq!((verified, rejected), (3, 5), "verdicts counted");
    check!(ops > 0, "no operations counted");
    crate::serial_println!("  [9/9] records and counts: OK");

    crate::serial_println!("secureboot::self_test() — all 9 tests passed");
    Ok(())
}
