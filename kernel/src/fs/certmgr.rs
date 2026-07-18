//! Certificate manager — SSL/TLS certificate storage, creation, and renewal.
//!
//! Manages X.509 certificates for the OS: system trust store, per-service
//! certificates, Let's Encrypt ACME integration, and certificate lifecycle.
//!
//! ## Design Reference
//!
//! design.txt line 1287: "create SSL certificate? (letsencrypt)"
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Security → Certificates
//!   → certmgr::list_certs() → installed certificates
//!   → certmgr::request_cert(domain) → ACME flow
//!   → certmgr::import_cert(pem) → add to store
//!
//! Web server / services
//!   → certmgr::get_cert_for(domain) → CertInfo
//!   → certmgr::needs_renewal(cert_id) → bool
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Certificate type/purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertType {
    /// CA root certificate (trust anchor).
    Root,
    /// Intermediate CA certificate.
    Intermediate,
    /// Server TLS certificate.
    Server,
    /// Client authentication certificate.
    Client,
    /// Code signing certificate.
    CodeSigning,
    /// Self-signed certificate.
    SelfSigned,
}

/// Certificate source/origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertSource {
    /// System-bundled trust store.
    System,
    /// User-imported certificate.
    UserImported,
    /// Let's Encrypt ACME.
    LetsEncrypt,
    /// Other ACME provider.
    Acme,
    /// Self-generated (e.g., for development).
    Generated,
}

/// Certificate status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertStatus {
    /// Valid and trusted.
    Valid,
    /// Expired.
    Expired,
    /// Revoked.
    Revoked,
    /// Not yet valid (future notBefore).
    NotYetValid,
    /// Untrusted (missing chain).
    Untrusted,
    /// Disabled by user.
    Disabled,
}

/// Key type/algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Rsa2048,
    Rsa4096,
    EcdsaP256,
    EcdsaP384,
    Ed25519,
}

/// ACME challenge type for domain validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeType {
    Http01,
    Dns01,
    TlsAlpn01,
}

/// A stored certificate.
#[derive(Debug, Clone)]
pub struct CertInfo {
    /// Unique ID.
    pub id: u64,
    /// Common Name (CN) or primary domain.
    pub common_name: String,
    /// Subject Alternative Names (SANs) — additional domains.
    pub alt_names: Vec<String>,
    /// Certificate type.
    pub cert_type: CertType,
    /// Source/origin.
    pub source: CertSource,
    /// Current status.
    pub status: CertStatus,
    /// Key algorithm.
    pub key_type: KeyType,
    /// Issuer common name.
    pub issuer: String,
    /// Serial number (hex string).
    pub serial: String,
    /// Not-before timestamp (ns since boot — simplified).
    pub not_before_ns: u64,
    /// Not-after timestamp (ns since boot — simplified).
    pub not_after_ns: u64,
    /// SHA-256 fingerprint (hex).
    pub fingerprint: String,
    /// Associated service/application (empty = system-wide).
    pub service: String,
    /// Whether auto-renewal is enabled (ACME certs).
    pub auto_renew: bool,
    /// ACME account email (for Let's Encrypt).
    pub acme_email: String,
    /// Challenge type for ACME.
    pub challenge_type: ChallengeType,
    /// Path to PEM file on disk.
    pub cert_path: String,
    /// Path to private key file.
    pub key_path: String,
    /// Timestamp of last renewal attempt (ns).
    pub last_renewal_ns: u64,
    /// Number of successful renewals.
    pub renewal_count: u32,
    /// Whether this is pinned (cannot be auto-removed).
    pub pinned: bool,
}

/// ACME order/request state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcmeState {
    Idle,
    Pending,
    Processing,
    Valid,
    Invalid,
    Revoked,
}

/// An in-progress ACME certificate request.
#[derive(Debug, Clone)]
pub struct AcmeRequest {
    pub id: u64,
    pub domain: String,
    pub alt_names: Vec<String>,
    pub email: String,
    pub key_type: KeyType,
    pub challenge: ChallengeType,
    pub state: AcmeState,
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    certs: Vec<CertInfo>,
    requests: Vec<AcmeRequest>,
    /// Days before expiry to trigger renewal (default 30).
    renewal_threshold_days: u32,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    certs: Vec::new(),
    requests: Vec::new(),
    renewal_threshold_days: 30,
    changes: 0,
});

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Certificate management
// ---------------------------------------------------------------------------

/// Import a certificate into the store.
pub fn import_cert(
    common_name: &str,
    cert_type: CertType,
    source: CertSource,
    key_type: KeyType,
    issuer: &str,
    cert_path: &str,
    key_path: &str,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.certs.len() >= 1024 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();

    // Generate a simplified fingerprint from name+id.
    let fp = {
        use alloc::format;
        let hash = common_name.bytes().fold(0x811c9dc5_u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100000001b3)
        });
        format!("{:016x}", hash ^ id)
    };

    state.certs.push(CertInfo {
        id,
        common_name: String::from(common_name),
        alt_names: Vec::new(),
        cert_type,
        source,
        status: CertStatus::Valid,
        key_type,
        issuer: String::from(issuer),
        serial: {
            use alloc::format;
            format!("{:08X}", id)
        },
        not_before_ns: now,
        not_after_ns: now.wrapping_add(365 * 24 * 3600 * 1_000_000_000), // ~1 year
        fingerprint: fp,
        service: String::new(),
        auto_renew: source == CertSource::LetsEncrypt || source == CertSource::Acme,
        acme_email: String::new(),
        challenge_type: ChallengeType::Http01,
        cert_path: String::from(cert_path),
        key_path: String::from(key_path),
        last_renewal_ns: 0,
        renewal_count: 0,
        pinned: cert_type == CertType::Root,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Remove a certificate (fails if pinned).
pub fn remove_cert(cert_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    if cert.pinned {
        return Err(KernelError::PermissionDenied);
    }
    state.certs.retain(|c| c.id != cert_id);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get certificate by ID.
pub fn get_cert(cert_id: u64) -> KernelResult<CertInfo> {
    STATE.lock().certs.iter().find(|c| c.id == cert_id).cloned()
        .ok_or(KernelError::NotFound)
}

/// List all certificates.
pub fn list_certs() -> Vec<CertInfo> {
    STATE.lock().certs.clone()
}

/// Find certificates matching a domain.
pub fn certs_for_domain(domain: &str) -> Vec<CertInfo> {
    let state = STATE.lock();
    state.certs.iter().filter(|c| {
        c.common_name == domain || c.alt_names.iter().any(|n| n == domain)
    }).cloned().collect()
}

/// Add a Subject Alternative Name to a certificate.
pub fn add_san(cert_id: u64, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter_mut().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    if cert.alt_names.len() >= 100 {
        return Err(KernelError::ResourceExhausted);
    }
    if cert.alt_names.iter().any(|n| n == name) {
        return Err(KernelError::AlreadyExists);
    }
    cert.alt_names.push(String::from(name));
    state.changes += 1;
    Ok(())
}

/// Set the service/application a certificate is bound to.
pub fn set_service(cert_id: u64, service: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter_mut().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    cert.service = String::from(service);
    state.changes += 1;
    Ok(())
}

/// Set certificate status (e.g., disable/revoke).
pub fn set_status(cert_id: u64, status: CertStatus) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter_mut().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    cert.status = status;
    state.changes += 1;
    Ok(())
}

/// Toggle auto-renewal.
pub fn set_auto_renew(cert_id: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter_mut().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    cert.auto_renew = enabled;
    state.changes += 1;
    Ok(())
}

/// Pin or unpin a certificate.
pub fn set_pinned(cert_id: u64, pinned: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter_mut().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    cert.pinned = pinned;
    state.changes += 1;
    Ok(())
}

/// Set renewal threshold (days before expiry to renew).
pub fn set_renewal_threshold(days: u32) -> KernelResult<()> {
    if days == 0 || days > 365 {
        return Err(KernelError::InvalidArgument);
    }
    STATE.lock().renewal_threshold_days = days;
    Ok(())
}

/// Get renewal threshold.
pub fn renewal_threshold() -> u32 {
    STATE.lock().renewal_threshold_days
}

/// Check if a certificate needs renewal.
pub fn needs_renewal(cert_id: u64) -> KernelResult<bool> {
    let state = STATE.lock();
    let cert = state.certs.iter().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    if !cert.auto_renew {
        return Ok(false);
    }
    let now = crate::hpet::elapsed_ns();
    let threshold_ns = state.renewal_threshold_days as u64 * 24 * 3600 * 1_000_000_000;
    Ok(cert.not_after_ns.saturating_sub(now) < threshold_ns)
}

/// Simulate renewing a certificate (increment count, extend expiry).
pub fn renew_cert(cert_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let cert = state.certs.iter_mut().find(|c| c.id == cert_id)
        .ok_or(KernelError::NotFound)?;
    let now = crate::hpet::elapsed_ns();
    cert.not_before_ns = now;
    cert.not_after_ns = now.wrapping_add(90 * 24 * 3600 * 1_000_000_000); // 90 days (LE)
    cert.last_renewal_ns = now;
    cert.renewal_count += 1;
    cert.status = CertStatus::Valid;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// ACME / Let's Encrypt requests
// ---------------------------------------------------------------------------

/// Request a new Let's Encrypt certificate.
pub fn request_cert(
    domain: &str,
    email: &str,
    key_type: KeyType,
    challenge: ChallengeType,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.requests.len() >= 64 {
        return Err(KernelError::ResourceExhausted);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    state.requests.push(AcmeRequest {
        id,
        domain: String::from(domain),
        alt_names: Vec::new(),
        email: String::from(email),
        key_type,
        challenge,
        state: AcmeState::Pending,
        created_ns: crate::hpet::elapsed_ns(),
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(id)
}

/// Complete an ACME request (simulated: creates the cert in the store).
pub fn complete_request(request_id: u64) -> KernelResult<u64> {
    let mut state = STATE.lock();
    let req_idx = state.requests.iter().position(|r| r.id == request_id)
        .ok_or(KernelError::NotFound)?;
    {
        let req = &state.requests[req_idx];
        if req.state != AcmeState::Pending && req.state != AcmeState::Processing {
            return Err(KernelError::InvalidArgument);
        }
    }

    // Clone the data we need before mutating.
    let domain = state.requests[req_idx].domain.clone();
    let alt_names = state.requests[req_idx].alt_names.clone();
    let email = state.requests[req_idx].email.clone();
    let key_type = state.requests[req_idx].key_type;
    let challenge = state.requests[req_idx].challenge;

    state.requests[req_idx].state = AcmeState::Valid;

    // Create the certificate.
    let cert_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = crate::hpet::elapsed_ns();
    let fp = {
        use alloc::format;
        let hash = domain.bytes().fold(0x811c9dc5_u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100000001b3)
        });
        format!("{:016x}", hash ^ cert_id)
    };

    if state.certs.len() >= 1024 {
        return Err(KernelError::ResourceExhausted);
    }

    let cert_path = {
        use alloc::format;
        format!("/etc/ssl/certs/{}.pem", domain)
    };
    let key_path = {
        use alloc::format;
        format!("/etc/ssl/private/{}.key", domain)
    };

    state.certs.push(CertInfo {
        id: cert_id,
        common_name: domain,
        alt_names,
        cert_type: CertType::Server,
        source: CertSource::LetsEncrypt,
        status: CertStatus::Valid,
        key_type,
        issuer: String::from("Let's Encrypt Authority X3"),
        serial: {
            use alloc::format;
            format!("{:08X}", cert_id)
        },
        not_before_ns: now,
        not_after_ns: now.wrapping_add(90 * 24 * 3600 * 1_000_000_000),
        fingerprint: fp,
        service: String::new(),
        auto_renew: true,
        acme_email: email,
        challenge_type: challenge,
        cert_path,
        key_path,
        last_renewal_ns: 0,
        renewal_count: 0,
        pinned: false,
    });
    state.changes += 1;
    Ok(cert_id)
}

/// Cancel an ACME request.
pub fn cancel_request(request_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.requests.iter().position(|r| r.id == request_id)
        .ok_or(KernelError::NotFound)?;
    state.requests.remove(idx);
    state.changes += 1;
    Ok(())
}

/// List pending ACME requests.
pub fn list_requests() -> Vec<AcmeRequest> {
    STATE.lock().requests.clone()
}

// ---------------------------------------------------------------------------
// Trust store helpers
// ---------------------------------------------------------------------------

/// Count certificates by type.
pub fn count_by_type(cert_type: CertType) -> usize {
    STATE.lock().certs.iter().filter(|c| c.cert_type == cert_type).count()
}

/// Count certificates by status.
pub fn count_by_status(status: CertStatus) -> usize {
    STATE.lock().certs.iter().filter(|c| c.status == status).count()
}

/// List all expired certificates.
pub fn list_expired() -> Vec<CertInfo> {
    STATE.lock().certs.iter().filter(|c| c.status == CertStatus::Expired).cloned().collect()
}

/// List certificates needing renewal.
pub fn list_needing_renewal() -> Vec<CertInfo> {
    let state = STATE.lock();
    let now = crate::hpet::elapsed_ns();
    let threshold_ns = state.renewal_threshold_days as u64 * 24 * 3600 * 1_000_000_000;
    state.certs.iter().filter(|c| {
        c.auto_renew && c.not_after_ns.saturating_sub(now) < threshold_ns
    }).cloned().collect()
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise the certificate-manager state.
///
/// Starts with an EMPTY trust store (no certificates, no pending requests) and
/// the default 30-day renewal threshold. A certificate enters the store only
/// through a real [`import_cert`] (a CA bundle or user-imported cert) or through
/// the ACME path ([`request_cert`] / [`complete_request`]). The `/proc/certmgr`
/// generator and the `certmgr` kshell command surface the certificate list (and
/// [`list_certs`] / [`stats`]) as if it reflects the real trust store, so
/// seeding it with well-known root CAs would be fabricated procfs data — and
/// uniquely dangerous on a security surface, because those phantom roots carried
/// FABRICATED cryptographic material: FNV-hash fingerprints (not real SHA-256
/// digests), made-up serial numbers, a synthetic 10-year validity window, and
/// no backing PEM file at all. Presenting them as `Valid`, pinned, system-
/// trusted roots would claim the OS trusts CAs it has never actually loaded a
/// certificate for. A real default trust store is legitimate OS behaviour, but
/// it must come from importing an actual bundled CA PEM via [`import_cert`], not
/// from inventing entries here.
///
/// (Previously this seeded five well-known root CAs — ISRG Root X1, DigiCert
/// Global Root G2, GlobalSign Root CA, Baltimore CyberTrust Root and Amazon Root
/// CA 1 — each marked Root/System/Valid/pinned with an RSA-4096 key type, an
/// FNV-hash "fingerprint", a `{id:08X}` serial, a now..now+10y validity window
/// and an empty `key_path` — i.e. no real certificate behind any of them.)
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.certs.is_empty() {
        return;
    }
    state.renewal_threshold_days = 30;
}

/// Return (cert_count, root_count, server_count, request_count, ops).
pub fn stats() -> (usize, usize, usize, usize, u64) {
    let state = STATE.lock();
    let total = state.certs.len();
    let roots = state.certs.iter().filter(|c| c.cert_type == CertType::Root).count();
    let servers = state.certs.iter().filter(|c| c.cert_type == CertType::Server).count();
    let requests = state.requests.len();
    let ops = OP_COUNT.load(Ordering::Relaxed);
    (total, roots, servers, requests, ops)
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.certs.clear();
    state.requests.clear();
    state.renewal_threshold_days = 30;
    state.changes = 0;
    NEXT_ID.store(1, Ordering::Relaxed);
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: import certificates.
    serial_println!("certmgr::self_test 1: import certs");
    let c1 = import_cert(
        "example.com", CertType::Server, CertSource::UserImported,
        KeyType::EcdsaP256, "DigiCert", "/etc/ssl/certs/example.pem", "/etc/ssl/private/example.key",
    )?;
    let c2 = import_cert(
        "ISRG Root X1", CertType::Root, CertSource::System,
        KeyType::Rsa4096, "ISRG", "/etc/ssl/certs/isrg.pem", "",
    )?;
    assert_eq!(list_certs().len(), 2);

    // Test 2: domain lookup and SAN.
    serial_println!("certmgr::self_test 2: domain lookup");
    add_san(c1, "www.example.com")?;
    add_san(c1, "api.example.com")?;
    assert!(add_san(c1, "www.example.com").is_err()); // duplicate
    let matches = certs_for_domain("www.example.com");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, c1);

    // Test 3: set service and status.
    serial_println!("certmgr::self_test 3: service/status");
    set_service(c1, "nginx")?;
    set_status(c1, CertStatus::Expired)?;
    let cert = get_cert(c1)?;
    assert_eq!(cert.service, "nginx");
    assert_eq!(cert.status, CertStatus::Expired);

    // Test 4: pin/remove.
    serial_println!("certmgr::self_test 4: pin/remove");
    assert!(remove_cert(c2).is_err()); // pinned root
    set_pinned(c2, false)?;
    remove_cert(c2)?;
    assert_eq!(list_certs().len(), 1);

    // Test 5: renewal.
    serial_println!("certmgr::self_test 5: renewal");
    set_status(c1, CertStatus::Valid)?;
    set_auto_renew(c1, true)?;
    renew_cert(c1)?;
    let cert = get_cert(c1)?;
    assert_eq!(cert.renewal_count, 1);
    assert!(cert.last_renewal_ns > 0);

    // Test 6: ACME request.
    serial_println!("certmgr::self_test 6: ACME request");
    let req = request_cert("test.example.com", "admin@example.com", KeyType::EcdsaP256, ChallengeType::Http01)?;
    assert_eq!(list_requests().len(), 1);
    let cert_id = complete_request(req)?;
    let le_cert = get_cert(cert_id)?;
    assert_eq!(le_cert.source, CertSource::LetsEncrypt);
    assert!(le_cert.auto_renew);

    // Test 7: init_defaults — the trust store starts EMPTY (no phantom roots);
    // certificates appear only through a real import. After init the store has
    // zero certs and zero roots.
    serial_println!("certmgr::self_test 7: init defaults");
    clear_all();
    init_defaults();
    let (total, roots, _, _, _) = stats();
    assert_eq!(total, 0); // empty trust store — no fabricated roots
    assert_eq!(roots, 0);

    clear_all();
    serial_println!("certmgr::self_test: all 7 tests passed");
    Ok(())
}
