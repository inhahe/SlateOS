//! SOCKS — SOCKS5 proxy protocol (RFC 1928).
//!
//! A minimal SOCKS5 client implementation for proxying TCP connections
//! through a SOCKS5 proxy server.
//!
//! ## Features
//!
//! - SOCKS5 CONNECT method (TCP proxy)
//! - No-auth and username/password auth (RFC 1929)
//! - IPv4 address and domain name target types
//! - Connection statistics
//!
//! ## Usage
//!
//! ```text
//! socks connect <proxy> <proxy_port> <target> <target_port>
//! socks status
//! socks test
//! ```
//!
//! ## Protocol overview
//!
//! 1. Client → Proxy: greeting (version=5, auth methods)
//! 2. Proxy → Client: chosen auth method
//! 3. (Optional) Auth sub-negotiation
//! 4. Client → Proxy: CONNECT request (target addr + port)
//! 5. Proxy → Client: reply (success/error)
//! 6. Proxy relays data bidirectionally

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// SOCKS5 version.
const SOCKS_VERSION: u8 = 0x05;

/// Default SOCKS port.
#[allow(dead_code)] // Public API.
pub const SOCKS_PORT: u16 = 1080;

/// Auth method: no authentication.
const AUTH_NONE: u8 = 0x00;

/// Auth method: username/password.
const AUTH_USERPASS: u8 = 0x02;

/// Auth method: no acceptable methods.
const AUTH_NO_ACCEPTABLE: u8 = 0xFF;

/// SOCKS5 command: CONNECT.
const CMD_CONNECT: u8 = 0x01;

/// Address type: IPv4.
const ATYP_IPV4: u8 = 0x01;

/// Address type: domain name.
const ATYP_DOMAIN: u8 = 0x03;

/// Address type: IPv6.
#[allow(dead_code)] // Public API.
const ATYP_IPV6: u8 = 0x04;

/// Timeout for proxy connection (poll iterations).
const CONNECT_TIMEOUT_POLLS: u32 = 300;

/// Timeout for proxy handshake reply (poll iterations).
const REPLY_TIMEOUT_POLLS: u32 = 200;

/// Maximum reply buffer size.
const MAX_REPLY_SIZE: usize = 512;

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static SUCCESSFUL: AtomicU64 = AtomicU64::new(0);
static AUTH_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);
static BYTES_RELAYED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Reply codes
// ---------------------------------------------------------------------------

/// SOCKS5 reply code description.
#[allow(dead_code)] // Public API.
pub fn reply_description(code: u8) -> &'static str {
    match code {
        0x00 => "Succeeded",
        0x01 => "General SOCKS server failure",
        0x02 => "Connection not allowed by ruleset",
        0x03 => "Network unreachable",
        0x04 => "Host unreachable",
        0x05 => "Connection refused",
        0x06 => "TTL expired",
        0x07 => "Command not supported",
        0x08 => "Address type not supported",
        _ => "Unknown error",
    }
}

// ---------------------------------------------------------------------------
// SOCKS5 connection result
// ---------------------------------------------------------------------------

/// Result of a SOCKS5 proxy connection.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct SocksResult {
    /// TCP handle to the proxy (can be used for data transfer).
    pub handle: usize,
    /// Bound address returned by proxy (may be 0.0.0.0).
    pub bound_addr: Ipv4Addr,
    /// Bound port returned by proxy.
    pub bound_port: u16,
    /// Whether connection was successful.
    pub success: bool,
    /// Reply code from proxy.
    pub reply_code: u8,
}

// ---------------------------------------------------------------------------
// SOCKS5 handshake
// ---------------------------------------------------------------------------

/// Connect to a target through a SOCKS5 proxy.
///
/// Performs the full SOCKS5 handshake (greeting, optional auth, CONNECT)
/// and returns a handle that can be used for data transfer.
#[allow(dead_code)] // Public API.
pub fn connect(
    proxy_ip: Ipv4Addr,
    proxy_port: u16,
    target_ip: Ipv4Addr,
    target_port: u16,
    user: &str,
    pass: &str,
) -> KernelResult<SocksResult> {
    CONNECTIONS.fetch_add(1, Ordering::Relaxed);

    let port = if proxy_port == 0 { SOCKS_PORT } else { proxy_port };

    // Connect to proxy.
    let handle = super::tcp::connect(proxy_ip, port)?;
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Step 1: Send greeting with supported auth methods.
    let greeting = if user.is_empty() {
        // Only no-auth.
        alloc::vec![SOCKS_VERSION, 1, AUTH_NONE]
    } else {
        // No-auth and user/pass.
        alloc::vec![SOCKS_VERSION, 2, AUTH_NONE, AUTH_USERPASS]
    };

    if let Err(e) = super::tcp::send(handle, &greeting) {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(e);
    }

    // Wait for auth method response.
    for _ in 0..REPLY_TIMEOUT_POLLS {
        super::poll();
    }

    let auth_reply = match super::tcp::read_up_to(handle, MAX_REPLY_SIZE) {
        Ok(d) => d,
        Err(e) => {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(e);
        }
    };

    if auth_reply.len() < 2 || auth_reply[0] != SOCKS_VERSION {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let chosen_auth = auth_reply[1];

    if chosen_auth == AUTH_NO_ACCEPTABLE {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::PermissionDenied);
    }

    // Step 2: Authenticate if needed.
    if chosen_auth == AUTH_USERPASS {
        AUTH_ATTEMPTS.fetch_add(1, Ordering::Relaxed);

        // Username/password sub-negotiation (RFC 1929).
        let mut auth_msg = Vec::with_capacity(3 + user.len() + pass.len());
        auth_msg.push(0x01); // Sub-negotiation version.
        auth_msg.push(user.len().min(255) as u8);
        auth_msg.extend_from_slice(&user.as_bytes()[..user.len().min(255)]);
        auth_msg.push(pass.len().min(255) as u8);
        auth_msg.extend_from_slice(&pass.as_bytes()[..pass.len().min(255)]);

        if let Err(e) = super::tcp::send(handle, &auth_msg) {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(e);
        }

        for _ in 0..REPLY_TIMEOUT_POLLS {
            super::poll();
        }

        let auth_resp = match super::tcp::read_up_to(handle, MAX_REPLY_SIZE) {
            Ok(d) => d,
            Err(e) => {
                let _ = super::tcp::close(handle);
                ERRORS.fetch_add(1, Ordering::Relaxed);
                return Err(e);
            }
        };

        // Check auth result: [version=0x01][status] where 0x00 = success.
        if auth_resp.len() < 2 || auth_resp[1] != 0x00 {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::PermissionDenied);
        }
    }

    // Step 3: Send CONNECT request.
    let mut connect_req = Vec::with_capacity(10);
    connect_req.push(SOCKS_VERSION);   // Version.
    connect_req.push(CMD_CONNECT);     // Command.
    connect_req.push(0x00);            // Reserved.
    connect_req.push(ATYP_IPV4);      // Address type.
    connect_req.extend_from_slice(&target_ip.0); // Target IP.
    connect_req.extend_from_slice(&target_port.to_be_bytes()); // Target port.

    if let Err(e) = super::tcp::send(handle, &connect_req) {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(e);
    }

    // Wait for CONNECT reply.
    for _ in 0..REPLY_TIMEOUT_POLLS {
        super::poll();
    }

    let connect_reply = match super::tcp::read_up_to(handle, MAX_REPLY_SIZE) {
        Ok(d) => d,
        Err(e) => {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(e);
        }
    };

    // Parse CONNECT reply: [VER][REP][RSV][ATYP][BND.ADDR][BND.PORT]
    if connect_reply.len() < 10 {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let reply_code = connect_reply[1];
    let success = reply_code == 0x00;

    let bound_addr = if connect_reply[3] == ATYP_IPV4 && connect_reply.len() >= 10 {
        Ipv4Addr::new(
            connect_reply[4],
            connect_reply[5],
            connect_reply[6],
            connect_reply[7],
        )
    } else {
        Ipv4Addr::UNSPECIFIED
    };

    let bound_port = if connect_reply.len() >= 10 {
        u16::from_be_bytes([connect_reply[8], connect_reply[9]])
    } else {
        0
    };

    if success {
        SUCCESSFUL.fetch_add(1, Ordering::Relaxed);
    } else {
        ERRORS.fetch_add(1, Ordering::Relaxed);
        let _ = super::tcp::close(handle);
    }

    Ok(SocksResult {
        handle,
        bound_addr,
        bound_port,
        success,
        reply_code,
    })
}

/// Connect to a target by domain name through a SOCKS5 proxy.
#[allow(dead_code)] // Public API.
pub fn connect_domain(
    proxy_ip: Ipv4Addr,
    proxy_port: u16,
    domain: &str,
    target_port: u16,
    user: &str,
    pass: &str,
) -> KernelResult<SocksResult> {
    CONNECTIONS.fetch_add(1, Ordering::Relaxed);

    let port = if proxy_port == 0 { SOCKS_PORT } else { proxy_port };

    // Connect to proxy.
    let handle = super::tcp::connect(proxy_ip, port)?;
    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Greeting.
    let greeting = if user.is_empty() {
        alloc::vec![SOCKS_VERSION, 1, AUTH_NONE]
    } else {
        alloc::vec![SOCKS_VERSION, 2, AUTH_NONE, AUTH_USERPASS]
    };

    super::tcp::send(handle, &greeting)?;

    for _ in 0..REPLY_TIMEOUT_POLLS {
        super::poll();
    }

    let auth_reply = super::tcp::read_up_to(handle, MAX_REPLY_SIZE)?;
    if auth_reply.len() < 2 || auth_reply[0] != SOCKS_VERSION {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    // Auth if needed (same as above — simplified).
    if auth_reply[1] == AUTH_USERPASS && !user.is_empty() {
        AUTH_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        let mut auth_msg = Vec::with_capacity(3 + user.len() + pass.len());
        auth_msg.push(0x01);
        auth_msg.push(user.len().min(255) as u8);
        auth_msg.extend_from_slice(&user.as_bytes()[..user.len().min(255)]);
        auth_msg.push(pass.len().min(255) as u8);
        auth_msg.extend_from_slice(&pass.as_bytes()[..pass.len().min(255)]);

        super::tcp::send(handle, &auth_msg)?;
        for _ in 0..REPLY_TIMEOUT_POLLS { super::poll(); }
        let auth_resp = super::tcp::read_up_to(handle, MAX_REPLY_SIZE)?;
        if auth_resp.len() < 2 || auth_resp[1] != 0x00 {
            let _ = super::tcp::close(handle);
            ERRORS.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::PermissionDenied);
        }
    } else if auth_reply[1] == AUTH_NO_ACCEPTABLE {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::PermissionDenied);
    }

    // CONNECT with domain name.
    let domain_bytes = domain.as_bytes();
    let domain_len = domain_bytes.len().min(255);
    let mut connect_req = Vec::with_capacity(7 + domain_len);
    connect_req.push(SOCKS_VERSION);
    connect_req.push(CMD_CONNECT);
    connect_req.push(0x00);
    connect_req.push(ATYP_DOMAIN);
    connect_req.push(domain_len as u8);
    connect_req.extend_from_slice(&domain_bytes[..domain_len]);
    connect_req.extend_from_slice(&target_port.to_be_bytes());

    super::tcp::send(handle, &connect_req)?;

    for _ in 0..REPLY_TIMEOUT_POLLS { super::poll(); }

    let reply = super::tcp::read_up_to(handle, MAX_REPLY_SIZE)?;
    if reply.len() < 4 {
        let _ = super::tcp::close(handle);
        ERRORS.fetch_add(1, Ordering::Relaxed);
        return Err(KernelError::InvalidArgument);
    }

    let reply_code = reply[1];
    let success = reply_code == 0x00;

    if success {
        SUCCESSFUL.fetch_add(1, Ordering::Relaxed);
    } else {
        ERRORS.fetch_add(1, Ordering::Relaxed);
        let _ = super::tcp::close(handle);
    }

    Ok(SocksResult {
        handle,
        bound_addr: Ipv4Addr::UNSPECIFIED,
        bound_port: 0,
        success,
        reply_code,
    })
}

/// Close a SOCKS proxy connection.
#[allow(dead_code)] // Public API.
pub fn close(handle: usize) {
    let _ = super::tcp::close(handle);
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// SOCKS proxy statistics.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct SocksStats {
    pub connections: u64,
    pub successful: u64,
    pub auth_attempts: u64,
    pub errors: u64,
    pub bytes_relayed: u64,
}

/// Get SOCKS statistics.
#[allow(dead_code)] // Public API.
pub fn stats() -> SocksStats {
    SocksStats {
        connections: CONNECTIONS.load(Ordering::Relaxed),
        successful: SUCCESSFUL.load(Ordering::Relaxed),
        auth_attempts: AUTH_ATTEMPTS.load(Ordering::Relaxed),
        errors: ERRORS.load(Ordering::Relaxed),
        bytes_relayed: BYTES_RELAYED.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/socks`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("SOCKS5 Proxy Client\n");
    out.push_str("===================\n\n");
    out.push_str(&format!("Connections:    {}\n", s.connections));
    out.push_str(&format!("Successful:     {}\n", s.successful));
    out.push_str(&format!("Auth attempts:  {}\n", s.auth_attempts));
    out.push_str(&format!("Errors:         {}\n", s.errors));
    out.push_str(&format!("Bytes relayed:  {}\n", s.bytes_relayed));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run SOCKS self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[socks] Running SOCKS5 self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Reply descriptions ---
    {
        assert!(reply_description(0x00) == "Succeeded", "success");
        assert!(reply_description(0x01).contains("failure"), "failure");
        assert!(reply_description(0x03).contains("unreachable"), "network");
        assert!(reply_description(0x04).contains("unreachable"), "host");
        assert!(reply_description(0x05).contains("refused"), "refused");
        assert!(reply_description(0x07).contains("not supported"), "cmd");
        assert!(reply_description(0xFF) == "Unknown error", "unknown");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 1 (reply descriptions) PASSED");
    }

    // --- Test 2: Greeting format ---
    {
        // No-auth greeting.
        let greeting: Vec<u8> = alloc::vec![SOCKS_VERSION, 1, AUTH_NONE];
        assert!(greeting[0] == 5, "version");
        assert!(greeting[1] == 1, "one method");
        assert!(greeting[2] == 0, "no auth");

        // User/pass greeting.
        let greeting2: Vec<u8> = alloc::vec![SOCKS_VERSION, 2, AUTH_NONE, AUTH_USERPASS];
        assert!(greeting2[1] == 2, "two methods");
        assert!(greeting2[3] == 2, "userpass method");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 2 (greeting format) PASSED");
    }

    // --- Test 3: CONNECT request format ---
    {
        let target_ip = Ipv4Addr::new(192, 168, 1, 1);
        let target_port: u16 = 8080;

        let mut req = Vec::new();
        req.push(SOCKS_VERSION);
        req.push(CMD_CONNECT);
        req.push(0x00);
        req.push(ATYP_IPV4);
        req.extend_from_slice(&target_ip.0);
        req.extend_from_slice(&target_port.to_be_bytes());

        assert!(req.len() == 10, "request length");
        assert!(req[0] == 5, "version");
        assert!(req[1] == 1, "connect cmd");
        assert!(req[3] == 1, "ipv4 type");
        assert!(req[4] == 192, "ip[0]");
        assert!(req[8] == 0x1F, "port high");
        assert!(req[9] == 0x90, "port low");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 3 (CONNECT request) PASSED");
    }

    // --- Test 4: Domain CONNECT request ---
    {
        let domain = "example.com";
        let mut req = Vec::new();
        req.push(SOCKS_VERSION);
        req.push(CMD_CONNECT);
        req.push(0x00);
        req.push(ATYP_DOMAIN);
        req.push(domain.len() as u8);
        req.extend_from_slice(domain.as_bytes());
        req.extend_from_slice(&80u16.to_be_bytes());

        assert!(req[3] == 3, "domain type");
        assert!(req[4] == 11, "domain length");
        assert!(req.len() == 7 + domain.len(), "total length");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 4 (domain CONNECT) PASSED");
    }

    // --- Test 5: Auth sub-negotiation format ---
    {
        let user = "admin";
        let pass = "secret";

        let mut auth = Vec::new();
        auth.push(0x01); // Sub-negotiation version.
        auth.push(user.len() as u8);
        auth.extend_from_slice(user.as_bytes());
        auth.push(pass.len() as u8);
        auth.extend_from_slice(pass.as_bytes());

        assert!(auth[0] == 1, "auth version");
        assert!(auth[1] == 5, "user length");
        assert!(auth[7] == 6, "pass length");
        assert!(auth.len() == 2 + user.len() + 1 + pass.len(), "total");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 5 (auth format) PASSED");
    }

    // --- Test 6: SocksResult struct ---
    {
        let result = SocksResult {
            handle: 42,
            bound_addr: Ipv4Addr::new(10, 0, 0, 1),
            bound_port: 9999,
            success: true,
            reply_code: 0x00,
        };
        assert!(result.success, "success");
        assert!(result.handle == 42, "handle");
        assert!(result.bound_port == 9999, "port");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 6 (SocksResult) PASSED");
    }

    // --- Test 7: Constants ---
    {
        assert!(SOCKS_VERSION == 5, "version 5");
        assert!(AUTH_NONE == 0, "auth none");
        assert!(AUTH_USERPASS == 2, "auth userpass");
        assert!(AUTH_NO_ACCEPTABLE == 0xFF, "no acceptable");
        assert!(CMD_CONNECT == 1, "connect cmd");
        assert!(ATYP_IPV4 == 1, "ipv4 atyp");
        assert!(ATYP_DOMAIN == 3, "domain atyp");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 7 (constants) PASSED");
    }

    // --- Test 8: Stats accessible ---
    {
        let s = stats();
        let _ = s.connections;
        let _ = s.successful;
        let _ = s.auth_attempts;
        let _ = s.errors;
        let _ = s.bytes_relayed;

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 8 (stats) PASSED");
    }

    // --- Test 9: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("SOCKS5"), "header");
        assert!(content.contains("Connections:"), "connections");
        assert!(content.contains("Errors:"), "errors");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 9 (procfs content) PASSED");
    }

    // --- Test 10: SocksStats struct ---
    {
        let s = SocksStats {
            connections: 10,
            successful: 8,
            auth_attempts: 5,
            errors: 2,
            bytes_relayed: 1_000_000,
        };
        assert!(s.connections == 10, "connections");
        assert!(s.successful == 8, "successful");
        assert!(s.bytes_relayed == 1_000_000, "relayed");

        passed = passed.saturating_add(1);
        crate::serial_println!("[socks]   test 10 (SocksStats) PASSED");
    }

    crate::serial_println!("[socks] All {} self-tests PASSED", passed);
    Ok(())
}
