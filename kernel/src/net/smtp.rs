//! SMTP — Simple Mail Transfer Protocol client (RFC 5321).
//!
//! A minimal SMTP client for sending email from the kernel or
//! userspace services (e.g., system alerts, log delivery).
//!
//! ## Features
//!
//! - SMTP client with HELO/EHLO handshake
//! - MAIL FROM / RCPT TO / DATA sequence
//! - Reply code parsing and validation
//! - Basic email message formatting
//! - Statistics tracking
//!
//! ## Usage
//!
//! ```text
//! smtp send <server> <from> <to> <subject> <body>
//! smtp status
//! smtp test
//! ```
//!
//! ## Limitations
//!
//! - No TLS/STARTTLS (plaintext only).
//! - No authentication (AUTH LOGIN/PLAIN).
//! - No MIME multipart or attachments.
//! - Single recipient per message.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use super::interface::Ipv4Addr;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// SMTP default port.
const SMTP_PORT: u16 = 25;

/// SMTP submission port.
#[allow(dead_code)] // Public API.
const SMTP_SUBMISSION_PORT: u16 = 587;

/// Timeout for connection (poll iterations).
const CONNECT_TIMEOUT_POLLS: u32 = 300;

/// Timeout for reply (poll iterations).
const REPLY_TIMEOUT_POLLS: u32 = 200;

/// Maximum reply buffer.
const MAX_REPLY_SIZE: usize = 2048;

/// Our hostname for HELO/EHLO.
const OUR_HOSTNAME: &str = "mintos.local";

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

static CONNECTIONS: AtomicU64 = AtomicU64::new(0);
static MESSAGES_SENT: AtomicU64 = AtomicU64::new(0);
static MESSAGES_FAILED: AtomicU64 = AtomicU64::new(0);
static BYTES_SENT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// SMTP reply parsing
// ---------------------------------------------------------------------------

/// An SMTP server reply.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct SmtpReply {
    /// 3-digit reply code.
    pub code: u16,
    /// Reply text (may be multi-line).
    pub message: String,
    /// Whether this is a multi-line reply continuation.
    pub continued: bool,
}

impl SmtpReply {
    /// Success — 2xx codes.
    #[allow(dead_code)] // Public API.
    pub fn is_success(&self) -> bool {
        self.code >= 200 && self.code < 300
    }

    /// Positive intermediate — 3xx codes (e.g., 354 = start data).
    #[allow(dead_code)] // Public API.
    pub fn is_intermediate(&self) -> bool {
        self.code >= 300 && self.code < 400
    }

    /// Transient failure — 4xx codes.
    #[allow(dead_code)] // Public API.
    pub fn is_transient_error(&self) -> bool {
        self.code >= 400 && self.code < 500
    }

    /// Permanent failure — 5xx codes.
    #[allow(dead_code)] // Public API.
    pub fn is_permanent_error(&self) -> bool {
        self.code >= 500
    }
}

/// Parse an SMTP reply from raw data.
///
/// SMTP replies: `NNN<SP>text\r\n` (final line)
///               `NNN-text\r\n` (continuation)
#[allow(dead_code)] // Public API.
pub fn parse_reply(data: &[u8]) -> Option<SmtpReply> {
    let text = core::str::from_utf8(data).ok()?;

    // Find the last line with a reply code.
    let mut last_code: u16 = 0;
    let mut message_parts = Vec::new();
    let mut continued = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.len() < 3 {
            continue;
        }

        if let Ok(code) = trimmed[..3].parse::<u16>() {
            last_code = code;
            let sep = trimmed.as_bytes().get(3).copied().unwrap_or(b' ');
            continued = sep == b'-';

            if trimmed.len() > 4 {
                message_parts.push(trimmed[4..].to_string());
            }
        }
    }

    if last_code == 0 {
        return None;
    }

    Some(SmtpReply {
        code: last_code,
        message: message_parts.join("; "),
        continued,
    })
}

// ---------------------------------------------------------------------------
// Email message
// ---------------------------------------------------------------------------

/// A simple email message.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct EmailMessage {
    /// Sender email address.
    pub from: String,
    /// Recipient email address.
    pub to: String,
    /// Subject line.
    pub subject: String,
    /// Message body (plain text).
    pub body: String,
}

impl EmailMessage {
    /// Create a new email message.
    #[allow(dead_code)] // Public API.
    pub fn new(from: &str, to: &str, subject: &str, body: &str) -> Self {
        Self {
            from: String::from(from),
            to: String::from(to),
            subject: String::from(subject),
            body: String::from(body),
        }
    }

    /// Format the message as SMTP DATA content.
    ///
    /// Includes standard headers and the body, terminated by `\r\n.\r\n`.
    fn format_data(&self) -> String {
        let mut data = String::with_capacity(
            self.from.len() + self.to.len() + self.subject.len() + self.body.len() + 128,
        );

        data.push_str(&format!("From: {}\r\n", self.from));
        data.push_str(&format!("To: {}\r\n", self.to));
        data.push_str(&format!("Subject: {}\r\n", self.subject));
        data.push_str("MIME-Version: 1.0\r\n");
        data.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        data.push_str(&format!("X-Mailer: MintOS/{}\r\n", "0.1"));
        data.push_str("\r\n"); // End of headers.

        // Body — escape any leading dots (dot stuffing per RFC 5321 §4.5.2).
        for line in self.body.lines() {
            if line.starts_with('.') {
                data.push('.');
            }
            data.push_str(line);
            data.push_str("\r\n");
        }

        data.push_str(".\r\n"); // End of data.
        data
    }
}

// ---------------------------------------------------------------------------
// SMTP session
// ---------------------------------------------------------------------------

/// SMTP session state.
struct SmtpSession {
    handle: usize,
}

/// Send an SMTP command and read the reply.
fn smtp_command(session: &SmtpSession, cmd: &str) -> KernelResult<SmtpReply> {
    let mut buf = Vec::with_capacity(cmd.len() + 2);
    buf.extend_from_slice(cmd.as_bytes());
    buf.extend_from_slice(b"\r\n");

    super::tcp::send(session.handle, &buf)?;
    BYTES_SENT.fetch_add(buf.len() as u64, Ordering::Relaxed);

    // Wait for reply.
    for _ in 0..REPLY_TIMEOUT_POLLS {
        super::poll();
    }

    let mut reply_data = super::tcp::read_up_to(session.handle, MAX_REPLY_SIZE)?;
    let mut reply = parse_reply(&reply_data).ok_or(KernelError::InvalidArgument)?;

    // Multi-line replies have '-' after the code on continuation lines.
    // Keep reading until we get the final line (space after code).
    let mut retries = 0u8;
    while reply.continued && retries < 5 {
        for _ in 0..REPLY_TIMEOUT_POLLS { super::poll(); }
        let more = super::tcp::read_up_to(session.handle, MAX_REPLY_SIZE)?;
        if more.is_empty() { break; }
        reply_data.extend_from_slice(&more);
        reply = parse_reply(&reply_data).ok_or(KernelError::InvalidArgument)?;
        retries = retries.saturating_add(1);
    }

    Ok(reply)
}

/// Send raw data (for DATA phase).
fn smtp_send_data(session: &SmtpSession, data: &str) -> KernelResult<SmtpReply> {
    super::tcp::send(session.handle, data.as_bytes())?;
    BYTES_SENT.fetch_add(data.len() as u64, Ordering::Relaxed);

    // Wait for reply.
    for _ in 0..REPLY_TIMEOUT_POLLS {
        super::poll();
    }

    let mut reply_data = super::tcp::read_up_to(session.handle, MAX_REPLY_SIZE)?;
    let mut reply = parse_reply(&reply_data).ok_or(KernelError::InvalidArgument)?;

    // Handle multi-line reply continuation (same as smtp_command).
    let mut retries = 0u8;
    while reply.continued && retries < 5 {
        for _ in 0..REPLY_TIMEOUT_POLLS { super::poll(); }
        let more = super::tcp::read_up_to(session.handle, MAX_REPLY_SIZE)?;
        if more.is_empty() { break; }
        reply_data.extend_from_slice(&more);
        reply = parse_reply(&reply_data).ok_or(KernelError::InvalidArgument)?;
        retries = retries.saturating_add(1);
    }

    Ok(reply)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of sending an email.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct SendResult {
    /// Whether the message was accepted.
    pub accepted: bool,
    /// Server reply message.
    pub server_reply: String,
    /// Server banner (greeting).
    pub banner: String,
}

/// Send an email message via SMTP.
///
/// Performs the full SMTP conversation: connect, EHLO, MAIL FROM,
/// RCPT TO, DATA, QUIT.
#[allow(dead_code)] // Public API.
pub fn send_email(
    server: Ipv4Addr,
    port: u16,
    message: &EmailMessage,
) -> KernelResult<SendResult> {
    CONNECTIONS.fetch_add(1, Ordering::Relaxed);

    let smtp_port = if port == 0 { SMTP_PORT } else { port };

    // Connect.
    let handle = super::tcp::connect(server, smtp_port)?;

    for _ in 0..CONNECT_TIMEOUT_POLLS {
        super::poll();
    }

    // Read greeting banner (220).
    let banner_data = match super::tcp::read_up_to(handle, MAX_REPLY_SIZE) {
        Ok(d) => d,
        Err(_) => {
            let _ = super::tcp::close(handle);
            MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::TimedOut);
        }
    };

    let banner = match parse_reply(&banner_data) {
        Some(r) => r,
        None => {
            let _ = super::tcp::close(handle);
            MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::InvalidArgument);
        }
    };

    if banner.code != 220 {
        let _ = super::tcp::close(handle);
        MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
        return Ok(SendResult {
            accepted: false,
            server_reply: format!("{} {}", banner.code, banner.message),
            banner: banner.message,
        });
    }

    let session = SmtpSession { handle };

    // EHLO.
    let ehlo = smtp_command(&session, &format!("EHLO {}", OUR_HOSTNAME))?;
    if !ehlo.is_success() {
        // Fall back to HELO.
        let helo = smtp_command(&session, &format!("HELO {}", OUR_HOSTNAME))?;
        if !helo.is_success() {
            let _ = smtp_command(&session, "QUIT");
            let _ = super::tcp::close(handle);
            MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
            return Ok(SendResult {
                accepted: false,
                server_reply: format!("{} {}", helo.code, helo.message),
                banner: banner.message,
            });
        }
    }

    // MAIL FROM.
    let mail_from = smtp_command(&session, &format!("MAIL FROM:<{}>", message.from))?;
    if !mail_from.is_success() {
        let _ = smtp_command(&session, "QUIT");
        let _ = super::tcp::close(handle);
        MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
        return Ok(SendResult {
            accepted: false,
            server_reply: format!("{} {}", mail_from.code, mail_from.message),
            banner: banner.message,
        });
    }

    // RCPT TO.
    let rcpt_to = smtp_command(&session, &format!("RCPT TO:<{}>", message.to))?;
    if !rcpt_to.is_success() {
        let _ = smtp_command(&session, "QUIT");
        let _ = super::tcp::close(handle);
        MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
        return Ok(SendResult {
            accepted: false,
            server_reply: format!("{} {}", rcpt_to.code, rcpt_to.message),
            banner: banner.message,
        });
    }

    // DATA.
    let data_reply = smtp_command(&session, "DATA")?;
    if data_reply.code != 354 {
        let _ = smtp_command(&session, "QUIT");
        let _ = super::tcp::close(handle);
        MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
        return Ok(SendResult {
            accepted: false,
            server_reply: format!("{} {}", data_reply.code, data_reply.message),
            banner: banner.message,
        });
    }

    // Send message content.
    let data_content = message.format_data();
    let final_reply = smtp_send_data(&session, &data_content)?;

    // QUIT.
    let _ = smtp_command(&session, "QUIT");
    let _ = super::tcp::close(handle);

    let accepted = final_reply.is_success();
    if accepted {
        MESSAGES_SENT.fetch_add(1, Ordering::Relaxed);
    } else {
        MESSAGES_FAILED.fetch_add(1, Ordering::Relaxed);
    }

    Ok(SendResult {
        accepted,
        server_reply: format!("{} {}", final_reply.code, final_reply.message),
        banner: banner.message,
    })
}

// ---------------------------------------------------------------------------
// Reply code descriptions
// ---------------------------------------------------------------------------

/// Get description for an SMTP reply code.
#[allow(dead_code)] // Public API.
pub fn reply_description(code: u16) -> &'static str {
    match code {
        211 => "System status",
        214 => "Help message",
        220 => "Service ready",
        221 => "Service closing transmission channel",
        250 => "Requested mail action okay, completed",
        251 => "User not local; will forward",
        252 => "Cannot VRFY user, but will accept message",
        354 => "Start mail input; end with <CRLF>.<CRLF>",
        421 => "Service not available, closing transmission channel",
        450 => "Requested mail action not taken: mailbox unavailable",
        451 => "Requested action aborted: local error in processing",
        452 => "Requested action not taken: insufficient system storage",
        500 => "Syntax error, command unrecognized",
        501 => "Syntax error in parameters or arguments",
        502 => "Command not implemented",
        503 => "Bad sequence of commands",
        504 => "Command parameter not implemented",
        550 => "Requested action not taken: mailbox unavailable",
        551 => "User not local",
        552 => "Requested mail action aborted: exceeded storage allocation",
        553 => "Requested action not taken: mailbox name not allowed",
        554 => "Transaction failed",
        _ => "Unknown reply code",
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// SMTP client statistics.
#[derive(Debug)]
#[allow(dead_code)] // Public API.
pub struct SmtpStats {
    pub connections: u64,
    pub messages_sent: u64,
    pub messages_failed: u64,
    pub bytes_sent: u64,
}

/// Get SMTP statistics.
#[allow(dead_code)] // Public API.
pub fn stats() -> SmtpStats {
    SmtpStats {
        connections: CONNECTIONS.load(Ordering::Relaxed),
        messages_sent: MESSAGES_SENT.load(Ordering::Relaxed),
        messages_failed: MESSAGES_FAILED.load(Ordering::Relaxed),
        bytes_sent: BYTES_SENT.load(Ordering::Relaxed),
    }
}

/// Generate procfs content for `/proc/smtp`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let s = stats();
    let mut out = String::with_capacity(256);
    out.push_str("SMTP Client\n");
    out.push_str("===========\n\n");
    out.push_str(&format!("Connections:     {}\n", s.connections));
    out.push_str(&format!("Messages sent:   {}\n", s.messages_sent));
    out.push_str(&format!("Messages failed: {}\n", s.messages_failed));
    out.push_str(&format!("Bytes sent:      {}\n", s.bytes_sent));
    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run SMTP self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[smtp] Running SMTP self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Reply parsing ---
    {
        let reply = parse_reply(b"220 mail.example.com ESMTP\r\n");
        assert!(reply.is_some(), "parse reply");
        let r = reply.unwrap();
        assert!(r.code == 220, "code");
        assert!(r.is_success(), "success");
        assert!(r.message.contains("mail.example.com"), "message");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 1 (reply parsing) PASSED");
    }

    // --- Test 2: Reply classification ---
    {
        let r250 = SmtpReply { code: 250, message: String::from("OK"), continued: false };
        assert!(r250.is_success(), "250 success");
        assert!(!r250.is_intermediate(), "not intermediate");

        let r354 = SmtpReply { code: 354, message: String::new(), continued: false };
        assert!(r354.is_intermediate(), "354 intermediate");
        assert!(!r354.is_success(), "not success");

        let r450 = SmtpReply { code: 450, message: String::new(), continued: false };
        assert!(r450.is_transient_error(), "450 transient");

        let r550 = SmtpReply { code: 550, message: String::new(), continued: false };
        assert!(r550.is_permanent_error(), "550 permanent");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 2 (reply classification) PASSED");
    }

    // --- Test 3: Email message formatting ---
    {
        let msg = EmailMessage::new(
            "user@example.com",
            "admin@example.com",
            "Test Subject",
            "Hello, this is a test.",
        );
        let data = msg.format_data();
        assert!(data.contains("From: user@example.com"), "from header");
        assert!(data.contains("To: admin@example.com"), "to header");
        assert!(data.contains("Subject: Test Subject"), "subject");
        assert!(data.contains("Hello, this is a test."), "body");
        assert!(data.ends_with(".\r\n"), "terminator");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 3 (email formatting) PASSED");
    }

    // --- Test 4: Dot stuffing ---
    {
        let msg = EmailMessage::new(
            "a@b.com",
            "c@d.com",
            "Dots",
            ".leading dot line\nnormal line\n..double dots",
        );
        let data = msg.format_data();
        // Lines starting with . should have an extra dot prepended.
        assert!(data.contains("..leading dot line"), "dot stuffed");
        assert!(data.contains("...double dots"), "double dot stuffed");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 4 (dot stuffing) PASSED");
    }

    // --- Test 5: Reply descriptions ---
    {
        assert!(reply_description(220) == "Service ready", "220");
        assert!(reply_description(250).contains("okay"), "250");
        assert!(reply_description(354).contains("Start mail"), "354");
        assert!(reply_description(550).contains("unavailable"), "550");
        assert!(reply_description(9999) == "Unknown reply code", "unknown");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 5 (reply descriptions) PASSED");
    }

    // --- Test 6: Multi-line reply ---
    {
        let data = b"250-mail.example.com Hello\r\n250-SIZE 52428800\r\n250 ENHANCEDSTATUSCODES\r\n";
        let reply = parse_reply(data);
        assert!(reply.is_some(), "multi-line parse");
        let r = reply.unwrap();
        assert!(r.code == 250, "multi-line code");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 6 (multi-line reply) PASSED");
    }

    // --- Test 7: Stats accessible ---
    {
        let s = stats();
        let _ = s.connections;
        let _ = s.messages_sent;
        let _ = s.messages_failed;
        let _ = s.bytes_sent;

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 7 (stats) PASSED");
    }

    // --- Test 8: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("SMTP"), "header");
        assert!(content.contains("Messages sent:"), "sent field");
        assert!(content.contains("Bytes sent:"), "bytes field");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 8 (procfs content) PASSED");
    }

    // --- Test 9: EmailMessage struct ---
    {
        let msg = EmailMessage {
            from: String::from("a@b.com"),
            to: String::from("c@d.com"),
            subject: String::from("Test"),
            body: String::from("Body"),
        };
        assert!(msg.from == "a@b.com", "from");
        assert!(msg.to == "c@d.com", "to");
        assert!(msg.subject == "Test", "subject");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 9 (EmailMessage) PASSED");
    }

    // --- Test 10: SendResult struct ---
    {
        let result = SendResult {
            accepted: true,
            server_reply: String::from("250 OK"),
            banner: String::from("Welcome"),
        };
        assert!(result.accepted, "accepted");
        assert!(result.server_reply.contains("250"), "reply");

        passed = passed.saturating_add(1);
        crate::serial_println!("[smtp]   test 10 (SendResult) PASSED");
    }

    crate::serial_println!("[smtp] All {} self-tests PASSED", passed);
    Ok(())
}
