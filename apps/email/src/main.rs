//! Email client application
//!
//! Features:
//! - MIME message parsing (headers, multipart, attachments)
//! - Email address parsing (RFC 5322 display name + addr-spec)
//! - Mailbox management (Inbox, Sent, Drafts, Trash, Spam, custom folders)
//! - IMAP protocol commands (LOGIN, SELECT, FETCH, SEARCH, STORE, COPY, MOVE)
//! - SMTP protocol commands (EHLO, AUTH, MAIL FROM, RCPT TO, DATA)
//! - Email composition with To/CC/BCC, subject, body, attachments
//! - HTML and plain text message rendering
//! - Message threading (In-Reply-To / References headers)
//! - Contact integration and address book
//! - Search across mailboxes
//! - Account management (multiple accounts)
//! - Signature management
//! - Rules/filters for automatic sorting
//! - Multi-panel UI: folder sidebar, message list, reading pane

#![deny(clippy::all, clippy::pedantic)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]

use std::collections::BTreeMap;
use std::fmt;

// ─── Email Address Parsing ───────────────────────────────────────────

/// Parsed email address with optional display name
#[derive(Debug, Clone, PartialEq)]
pub struct EmailAddress {
    pub display_name: Option<String>,
    pub local_part: String,
    pub domain: String,
}

impl EmailAddress {
    /// Parse "Display Name <user@domain>" or "user@domain"
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();

        // Try "Display Name <addr>" format
        if let Some(angle_start) = trimmed.rfind('<') {
            if let Some(angle_end) = trimmed.rfind('>') {
                if angle_end > angle_start {
                    let display = trimmed.get(..angle_start)?.trim();
                    let display_name = if display.is_empty() {
                        None
                    } else {
                        // Strip surrounding quotes if present
                        let d = display.trim_matches('"').trim();
                        if d.is_empty() { None } else { Some(d.to_string()) }
                    };
                    let addr = trimmed.get(angle_start.saturating_add(1)..angle_end)?.trim();
                    let (local, domain) = Self::split_addr(addr)?;
                    return Some(Self { display_name, local_part: local, domain });
                }
            }
        }

        // Plain "user@domain" format
        let (local, domain) = Self::split_addr(trimmed)?;
        Some(Self { display_name: None, local_part: local, domain })
    }

    fn split_addr(addr: &str) -> Option<(String, String)> {
        let at = addr.rfind('@')?;
        let local = addr.get(..at)?.trim();
        let domain = addr.get(at.saturating_add(1)..)?.trim();
        if local.is_empty() || domain.is_empty() || !domain.contains('.') {
            return None;
        }
        Some((local.to_string(), domain.to_string()))
    }

    /// Full address as string
    pub fn address(&self) -> String {
        format!("{}@{}", self.local_part, self.domain)
    }
}

impl fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref name) = self.display_name {
            write!(f, "{name} <{}@{}>", self.local_part, self.domain)
        } else {
            write!(f, "{}@{}", self.local_part, self.domain)
        }
    }
}

// ─── MIME Parsing ────────────────────────────────────────────────────

/// Content-Type with media type and parameters
#[derive(Debug, Clone)]
pub struct ContentType {
    pub media_type: String,
    pub subtype: String,
    pub params: BTreeMap<String, String>,
}

impl ContentType {
    pub fn parse(value: &str) -> Self {
        let mut parts = value.splitn(2, ';');
        let type_part = parts.next().unwrap_or("text/plain").trim();
        let (media_type, subtype) = if let Some(slash) = type_part.find('/') {
            (
                type_part.get(..slash).unwrap_or("text").to_lowercase(),
                type_part.get(slash.saturating_add(1)..).unwrap_or("plain").to_lowercase(),
            )
        } else {
            ("text".to_string(), "plain".to_string())
        };

        let mut params = BTreeMap::new();
        if let Some(param_str) = parts.next() {
            for param in param_str.split(';') {
                let param = param.trim();
                if let Some(eq) = param.find('=') {
                    let key = param.get(..eq).unwrap_or("").trim().to_lowercase();
                    let val = param.get(eq.saturating_add(1)..).unwrap_or("").trim()
                        .trim_matches('"').to_string();
                    params.insert(key, val);
                }
            }
        }

        Self { media_type, subtype, params }
    }

    /// Full MIME type string
    pub fn mime_type(&self) -> String {
        format!("{}/{}", self.media_type, self.subtype)
    }

    /// Get charset parameter
    pub fn charset(&self) -> &str {
        self.params.get("charset").map_or("us-ascii", |s| s.as_str())
    }

    /// Get boundary parameter for multipart messages
    pub fn boundary(&self) -> Option<&str> {
        self.params.get("boundary").map(|s| s.as_str())
    }

    /// Check if this is a multipart type
    pub fn is_multipart(&self) -> bool {
        self.media_type == "multipart"
    }

    /// Check if text type
    pub fn is_text(&self) -> bool {
        self.media_type == "text"
    }
}

/// Content-Disposition
#[derive(Debug, Clone)]
pub struct ContentDisposition {
    pub disposition: String, // "inline" or "attachment"
    pub filename: Option<String>,
}

impl ContentDisposition {
    pub fn parse(value: &str) -> Self {
        let mut parts = value.splitn(2, ';');
        let disposition = parts.next().unwrap_or("inline").trim().to_lowercase();
        let mut filename = None;

        if let Some(param_str) = parts.next() {
            for param in param_str.split(';') {
                let param = param.trim();
                if let Some(eq) = param.find('=') {
                    let key = param.get(..eq).unwrap_or("").trim().to_lowercase();
                    let val = param.get(eq.saturating_add(1)..).unwrap_or("").trim()
                        .trim_matches('"').to_string();
                    if key == "filename" {
                        filename = Some(val);
                    }
                }
            }
        }

        Self { disposition, filename }
    }

    pub fn is_attachment(&self) -> bool {
        self.disposition == "attachment"
    }
}

/// Parsed email headers
#[derive(Debug, Clone, Default)]
pub struct EmailHeaders {
    headers: Vec<(String, String)>,
}

impl EmailHeaders {
    pub fn new() -> Self {
        Self { headers: Vec::new() }
    }

    pub fn add(&mut self, name: &str, value: &str) {
        self.headers.push((name.to_string(), value.to_string()));
    }

    /// Get first header value by name (case-insensitive)
    pub fn get(&self, name: &str) -> Option<&str> {
        let lower = name.to_lowercase();
        self.headers.iter()
            .find(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    /// Get all header values for a name
    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let lower = name.to_lowercase();
        self.headers.iter()
            .filter(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// Parse headers from raw header block
    pub fn parse(text: &str) -> Self {
        let mut headers = Self::new();
        let mut current_name = String::new();
        let mut current_value = String::new();

        for line in text.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation of previous header (folding)
                if !current_name.is_empty() {
                    current_value.push(' ');
                    current_value.push_str(line.trim());
                }
            } else if let Some(colon) = line.find(':') {
                // Save previous header
                if !current_name.is_empty() {
                    headers.add(&current_name, &current_value);
                }
                current_name = line.get(..colon).unwrap_or("").trim().to_string();
                current_value = line.get(colon.saturating_add(1)..).unwrap_or("").trim().to_string();
            }
        }
        // Save last header
        if !current_name.is_empty() {
            headers.add(&current_name, &current_value);
        }

        headers
    }

    /// Iterate over all headers
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// A MIME body part
#[derive(Debug, Clone)]
pub struct MimePart {
    pub headers: EmailHeaders,
    pub content_type: ContentType,
    pub disposition: Option<ContentDisposition>,
    pub body: Vec<u8>,
    pub parts: Vec<MimePart>,  // Sub-parts for multipart
}

impl MimePart {
    /// Check if this is an attachment
    pub fn is_attachment(&self) -> bool {
        self.disposition.as_ref().map_or(false, |d| d.is_attachment())
    }

    /// Get filename for attachment
    pub fn filename(&self) -> Option<&str> {
        self.disposition.as_ref()
            .and_then(|d| d.filename.as_deref())
            .or_else(|| self.content_type.params.get("name").map(|s| s.as_str()))
    }

    /// Get body as text (if text/* content type)
    pub fn body_text(&self) -> Option<String> {
        if self.content_type.is_text() {
            String::from_utf8(self.body.clone()).ok()
        } else {
            None
        }
    }
}

/// Parsed email message
#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub headers: EmailHeaders,
    pub from: Option<EmailAddress>,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub bcc: Vec<EmailAddress>,
    pub reply_to: Option<EmailAddress>,
    pub subject: String,
    pub date: Option<String>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub content_type: ContentType,
    pub body: MimePart,
}

impl EmailMessage {
    /// Parse an email message from raw text
    pub fn parse(raw: &str) -> Result<Self, String> {
        // Split headers and body at the blank line
        let (header_text, body_text) = if let Some(sep) = raw.find("\r\n\r\n") {
            (raw.get(..sep).unwrap_or(""), raw.get(sep.saturating_add(4)..).unwrap_or(""))
        } else if let Some(sep) = raw.find("\n\n") {
            (raw.get(..sep).unwrap_or(""), raw.get(sep.saturating_add(2)..).unwrap_or(""))
        } else {
            (raw, "")
        };

        let headers = EmailHeaders::parse(header_text);

        let from = headers.get("From").and_then(EmailAddress::parse);
        let to = parse_address_list(headers.get("To").unwrap_or(""));
        let cc = parse_address_list(headers.get("Cc").unwrap_or(""));
        let bcc = parse_address_list(headers.get("Bcc").unwrap_or(""));
        let reply_to = headers.get("Reply-To").and_then(EmailAddress::parse);
        let subject = headers.get("Subject").unwrap_or("(no subject)").to_string();
        let date = headers.get("Date").map(String::from);
        let message_id = headers.get("Message-ID")
            .or_else(|| headers.get("Message-Id"))
            .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string());
        let in_reply_to = headers.get("In-Reply-To")
            .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string());
        let references: Vec<String> = headers.get("References")
            .unwrap_or("")
            .split_whitespace()
            .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let content_type = ContentType::parse(headers.get("Content-Type").unwrap_or("text/plain"));

        let body = parse_mime_body(&content_type, body_text, &headers);

        Ok(Self {
            headers,
            from,
            to,
            cc,
            bcc,
            reply_to,
            subject,
            date,
            message_id,
            in_reply_to,
            references,
            content_type,
            body,
        })
    }

    /// Get plain text body
    pub fn plain_text(&self) -> Option<String> {
        find_text_part(&self.body, "plain")
    }

    /// Get HTML body
    pub fn html_body(&self) -> Option<String> {
        find_text_part(&self.body, "html")
    }

    /// List attachments
    pub fn attachments(&self) -> Vec<&MimePart> {
        let mut result = Vec::new();
        collect_attachments(&self.body, &mut result);
        result
    }
}

/// Parse a comma-separated list of email addresses
fn parse_address_list(s: &str) -> Vec<EmailAddress> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    s.split(',')
        .filter_map(|addr| EmailAddress::parse(addr.trim()))
        .collect()
}

/// Parse MIME body from text given content type
fn parse_mime_body(ct: &ContentType, body_text: &str, headers: &EmailHeaders) -> MimePart {
    let disposition = headers.get("Content-Disposition").map(ContentDisposition::parse);

    if ct.is_multipart() {
        let boundary = ct.boundary().unwrap_or("");
        let parts = parse_multipart(body_text, boundary);
        MimePart {
            headers: headers.clone(),
            content_type: ct.clone(),
            disposition,
            body: Vec::new(),
            parts,
        }
    } else {
        // Decode body based on Content-Transfer-Encoding
        let encoding = headers.get("Content-Transfer-Encoding").unwrap_or("7bit").to_lowercase();
        let body_bytes = match encoding.as_str() {
            "base64" => base64_decode(body_text),
            "quoted-printable" => quoted_printable_decode(body_text),
            _ => body_text.as_bytes().to_vec(),
        };
        MimePart {
            headers: headers.clone(),
            content_type: ct.clone(),
            disposition,
            body: body_bytes,
            parts: Vec::new(),
        }
    }
}

/// Parse multipart body into parts
fn parse_multipart(body: &str, boundary: &str) -> Vec<MimePart> {
    let delimiter = format!("--{boundary}");
    let end_delimiter = format!("--{boundary}--");
    let mut parts = Vec::new();

    let mut in_part = false;
    let mut current_part = String::new();

    for line in body.lines() {
        if line.starts_with(&end_delimiter) {
            if in_part && !current_part.is_empty() {
                if let Some(part) = parse_single_part(&current_part) {
                    parts.push(part);
                }
            }
            break;
        }
        if line.starts_with(&delimiter) {
            if in_part && !current_part.is_empty() {
                if let Some(part) = parse_single_part(&current_part) {
                    parts.push(part);
                }
            }
            in_part = true;
            current_part = String::new();
        } else if in_part {
            if !current_part.is_empty() {
                current_part.push('\n');
            }
            current_part.push_str(line);
        }
    }

    parts
}

/// Parse a single MIME part
fn parse_single_part(text: &str) -> Option<MimePart> {
    let (header_text, body_text) = if let Some(sep) = text.find("\n\n") {
        (text.get(..sep).unwrap_or(""), text.get(sep.saturating_add(2)..).unwrap_or(""))
    } else {
        ("", text)
    };

    let headers = EmailHeaders::parse(header_text);
    let ct = ContentType::parse(headers.get("Content-Type").unwrap_or("text/plain"));
    Some(parse_mime_body(&ct, body_text, &headers))
}

/// Find text part with given subtype in MIME tree
fn find_text_part(part: &MimePart, subtype: &str) -> Option<String> {
    if part.content_type.media_type == "text" && part.content_type.subtype == subtype {
        return part.body_text();
    }
    for sub in &part.parts {
        if let Some(text) = find_text_part(sub, subtype) {
            return Some(text);
        }
    }
    None
}

/// Collect all attachment parts
fn collect_attachments<'a>(part: &'a MimePart, result: &mut Vec<&'a MimePart>) {
    if part.is_attachment() {
        result.push(part);
    }
    for sub in &part.parts {
        collect_attachments(sub, result);
    }
}

/// Decode base64
fn base64_decode(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in input.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => Some(b.wrapping_sub(b'A') as u32),
            b'a'..=b'z' => Some(b.wrapping_sub(b'a').wrapping_add(26) as u32),
            b'0'..=b'9' => Some(b.wrapping_sub(b'0').wrapping_add(52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        };
        if let Some(v) = val {
            buf = (buf << 6) | v;
            bits = bits.saturating_add(6);
            if bits >= 8 {
                bits = bits.saturating_sub(8);
                result.push((buf >> bits) as u8);
                buf &= (1u32 << bits).wrapping_sub(1);
            }
        }
    }

    result
}

/// Encode to base64
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b0 = data.get(i).copied().unwrap_or(0) as u32;
        let b1 = data.get(i.saturating_add(1)).copied().unwrap_or(0) as u32;
        let b2 = data.get(i.saturating_add(2)).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if i.saturating_add(1) < data.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if i.saturating_add(2) < data.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        i = i.saturating_add(3);
    }

    result
}

/// Decode quoted-printable
fn quoted_printable_decode(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes.get(i) == Some(&b'=') {
            if bytes.get(i.saturating_add(1)) == Some(&b'\r')
                || bytes.get(i.saturating_add(1)) == Some(&b'\n')
            {
                // Soft line break
                i = i.saturating_add(2);
                if bytes.get(i) == Some(&b'\n') {
                    i = i.saturating_add(1);
                }
                continue;
            }
            if let (Some(&h), Some(&l)) = (
                bytes.get(i.saturating_add(1)),
                bytes.get(i.saturating_add(2)),
            ) {
                if let (Some(hi), Some(lo)) = (hex_val(h), hex_val(l)) {
                    result.push(hi.wrapping_shl(4) | lo);
                    i = i.saturating_add(3);
                    continue;
                }
            }
        }
        result.push(bytes.get(i).copied().unwrap_or(0));
        i = i.saturating_add(1);
    }

    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(b.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(b.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

// ─── IMAP Protocol ───────────────────────────────────────────────────

/// IMAP command builder
pub struct ImapCommand;

impl ImapCommand {
    pub fn login(tag: &str, user: &str, pass: &str) -> String {
        format!("{tag} LOGIN {user} {pass}\r\n")
    }

    pub fn logout(tag: &str) -> String {
        format!("{tag} LOGOUT\r\n")
    }

    pub fn select(tag: &str, mailbox: &str) -> String {
        format!("{tag} SELECT \"{mailbox}\"\r\n")
    }

    pub fn examine(tag: &str, mailbox: &str) -> String {
        format!("{tag} EXAMINE \"{mailbox}\"\r\n")
    }

    pub fn list(tag: &str, reference: &str, pattern: &str) -> String {
        format!("{tag} LIST \"{reference}\" \"{pattern}\"\r\n")
    }

    pub fn fetch(tag: &str, sequence: &str, items: &str) -> String {
        format!("{tag} FETCH {sequence} ({items})\r\n")
    }

    pub fn search(tag: &str, criteria: &str) -> String {
        format!("{tag} SEARCH {criteria}\r\n")
    }

    pub fn store(tag: &str, sequence: &str, action: &str, flags: &str) -> String {
        format!("{tag} STORE {sequence} {action} ({flags})\r\n")
    }

    pub fn copy(tag: &str, sequence: &str, mailbox: &str) -> String {
        format!("{tag} COPY {sequence} \"{mailbox}\"\r\n")
    }

    pub fn r#move(tag: &str, sequence: &str, mailbox: &str) -> String {
        format!("{tag} MOVE {sequence} \"{mailbox}\"\r\n")
    }

    pub fn expunge(tag: &str) -> String {
        format!("{tag} EXPUNGE\r\n")
    }

    pub fn noop(tag: &str) -> String {
        format!("{tag} NOOP\r\n")
    }

    pub fn idle(tag: &str) -> String {
        format!("{tag} IDLE\r\n")
    }

    pub fn create(tag: &str, mailbox: &str) -> String {
        format!("{tag} CREATE \"{mailbox}\"\r\n")
    }

    pub fn delete(tag: &str, mailbox: &str) -> String {
        format!("{tag} DELETE \"{mailbox}\"\r\n")
    }

    pub fn rename(tag: &str, old: &str, new: &str) -> String {
        format!("{tag} RENAME \"{old}\" \"{new}\"\r\n")
    }

    pub fn append(tag: &str, mailbox: &str, flags: &str, size: usize) -> String {
        format!("{tag} APPEND \"{mailbox}\" ({flags}) {{{size}}}\r\n")
    }
}

/// IMAP response status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImapStatus {
    Ok,
    No,
    Bad,
    Bye,
}

impl ImapStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "OK" => Some(Self::Ok),
            "NO" => Some(Self::No),
            "BAD" => Some(Self::Bad),
            "BYE" => Some(Self::Bye),
            _ => None,
        }
    }
}

// ─── SMTP Protocol ───────────────────────────────────────────────────

/// SMTP command builder
pub struct SmtpCommand;

impl SmtpCommand {
    pub fn ehlo(hostname: &str) -> String {
        format!("EHLO {hostname}\r\n")
    }

    pub fn helo(hostname: &str) -> String {
        format!("HELO {hostname}\r\n")
    }

    pub fn auth_login() -> String {
        "AUTH LOGIN\r\n".to_string()
    }

    pub fn auth_plain(user: &str, pass: &str) -> String {
        let credentials = format!("\0{user}\0{pass}");
        let encoded = base64_encode(credentials.as_bytes());
        format!("AUTH PLAIN {encoded}\r\n")
    }

    pub fn mail_from(addr: &str) -> String {
        format!("MAIL FROM:<{addr}>\r\n")
    }

    pub fn rcpt_to(addr: &str) -> String {
        format!("RCPT TO:<{addr}>\r\n")
    }

    pub fn data() -> String {
        "DATA\r\n".to_string()
    }

    pub fn quit() -> String {
        "QUIT\r\n".to_string()
    }

    pub fn rset() -> String {
        "RSET\r\n".to_string()
    }

    pub fn starttls() -> String {
        "STARTTLS\r\n".to_string()
    }

    pub fn noop() -> String {
        "NOOP\r\n".to_string()
    }
}

/// SMTP reply code ranges
pub fn smtp_reply_class(code: u16) -> &'static str {
    match code {
        200..=299 => "Positive Completion",
        300..=399 => "Positive Intermediate",
        400..=499 => "Transient Negative",
        500..=599 => "Permanent Negative",
        _ => "Unknown",
    }
}

// ─── Account Configuration ───────────────────────────────────────────

/// Email protocol type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProtocolType {
    Imap,
    Pop3,
    Exchange,
}

impl fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Imap => write!(f, "IMAP"),
            Self::Pop3 => write!(f, "POP3"),
            Self::Exchange => write!(f, "Exchange"),
        }
    }
}

/// Connection security
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Security {
    None,
    Ssl,
    Starttls,
}

impl fmt::Display for Security {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Ssl => write!(f, "SSL/TLS"),
            Self::Starttls => write!(f, "STARTTLS"),
        }
    }
}

/// Authentication method
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    Plain,
    Login,
    OAuth2,
    CramMd5,
}

impl fmt::Display for AuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => write!(f, "PLAIN"),
            Self::Login => write!(f, "LOGIN"),
            Self::OAuth2 => write!(f, "OAuth2"),
            Self::CramMd5 => write!(f, "CRAM-MD5"),
        }
    }
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub security: Security,
    pub auth_method: AuthMethod,
}

/// Email account configuration
#[derive(Debug, Clone)]
pub struct EmailAccount {
    pub id: u32,
    pub name: String,
    pub email: String,
    pub display_name: String,
    pub incoming: ServerConfig,
    pub outgoing: ServerConfig,
    pub protocol: ProtocolType,
    pub username: String,
    pub signature: String,
    pub enabled: bool,
    pub sync_interval_minutes: u32,
    pub days_to_sync: u32,
    pub color: guitk::Color,
}

impl EmailAccount {
    /// Create a default Gmail-like account
    pub fn gmail(email: &str, name: &str) -> Self {
        Self {
            id: 0,
            name: name.to_string(),
            email: email.to_string(),
            display_name: name.to_string(),
            incoming: ServerConfig {
                hostname: "imap.gmail.com".to_string(),
                port: 993,
                security: Security::Ssl,
                auth_method: AuthMethod::OAuth2,
            },
            outgoing: ServerConfig {
                hostname: "smtp.gmail.com".to_string(),
                port: 465,
                security: Security::Ssl,
                auth_method: AuthMethod::OAuth2,
            },
            protocol: ProtocolType::Imap,
            username: email.to_string(),
            signature: format!("Best regards,\n{name}"),
            enabled: true,
            sync_interval_minutes: 5,
            days_to_sync: 30,
            color: guitk::Color::from_hex(0x89B4FA),
        }
    }

    /// Create a default Outlook-like account
    pub fn outlook(email: &str, name: &str) -> Self {
        Self {
            id: 0,
            name: name.to_string(),
            email: email.to_string(),
            display_name: name.to_string(),
            incoming: ServerConfig {
                hostname: "outlook.office365.com".to_string(),
                port: 993,
                security: Security::Ssl,
                auth_method: AuthMethod::OAuth2,
            },
            outgoing: ServerConfig {
                hostname: "smtp-mail.outlook.com".to_string(),
                port: 587,
                security: Security::Starttls,
                auth_method: AuthMethod::OAuth2,
            },
            protocol: ProtocolType::Imap,
            username: email.to_string(),
            signature: format!("Best regards,\n{name}"),
            enabled: true,
            sync_interval_minutes: 5,
            days_to_sync: 30,
            color: guitk::Color::from_hex(0xFAB387),
        }
    }
}

// ─── Mailbox ─────────────────────────────────────────────────────────

/// Standard mailbox types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MailboxType {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Spam,
    Archive,
    Custom,
}

impl fmt::Display for MailboxType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inbox => write!(f, "Inbox"),
            Self::Sent => write!(f, "Sent"),
            Self::Drafts => write!(f, "Drafts"),
            Self::Trash => write!(f, "Trash"),
            Self::Spam => write!(f, "Spam"),
            Self::Archive => write!(f, "Archive"),
            Self::Custom => write!(f, "Custom"),
        }
    }
}

/// A mailbox (folder)
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub name: String,
    pub mailbox_type: MailboxType,
    pub total_messages: u32,
    pub unread_messages: u32,
    pub account_id: u32,
    pub imap_path: String,
}

// ─── Message Store ───────────────────────────────────────────────────

/// Message flags
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MessageFlags {
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
    pub deleted: bool,
    pub draft: bool,
    pub has_attachment: bool,
}

impl Default for MessageFlags {
    fn default() -> Self {
        Self {
            seen: false,
            answered: false,
            flagged: false,
            deleted: false,
            draft: false,
            has_attachment: false,
        }
    }
}

/// Message priority
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Priority {
    Low,
    Normal,
    High,
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Normal => write!(f, "Normal"),
            Self::High => write!(f, "High"),
        }
    }
}

/// Stored message summary (not full MIME content)
#[derive(Debug, Clone)]
pub struct MessageSummary {
    pub id: u64,
    pub uid: u32,
    pub from: EmailAddress,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub subject: String,
    pub date: String,
    pub timestamp: u64,
    pub preview: String,
    pub flags: MessageFlags,
    pub priority: Priority,
    pub account_id: u32,
    pub mailbox: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub thread_id: Option<u64>,
    pub size: u64,
    pub labels: Vec<String>,
}

/// Thread of related messages
#[derive(Debug, Clone)]
pub struct MessageThread {
    pub id: u64,
    pub subject: String,
    pub participants: Vec<EmailAddress>,
    pub message_ids: Vec<u64>,
    pub last_date: String,
    pub unread_count: u32,
    pub total_count: u32,
}

// ─── Email Composition ───────────────────────────────────────────────

/// Attachment for outgoing email
#[derive(Debug, Clone)]
pub struct Attachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub is_inline: bool,
    pub content_id: Option<String>,
}

impl Attachment {
    pub fn new(filename: &str, mime_type: &str, data: Vec<u8>) -> Self {
        Self {
            filename: filename.to_string(),
            mime_type: mime_type.to_string(),
            data,
            is_inline: false,
            content_id: None,
        }
    }

    /// Size in bytes
    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// Email draft being composed
#[derive(Debug, Clone)]
pub struct EmailDraft {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub is_html: bool,
    pub attachments: Vec<Attachment>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub priority: Priority,
    pub request_read_receipt: bool,
    pub account_id: u32,
    pub signature: String,
}

impl EmailDraft {
    pub fn new(account_id: u32) -> Self {
        Self {
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            body: String::new(),
            is_html: false,
            attachments: Vec::new(),
            in_reply_to: None,
            references: Vec::new(),
            priority: Priority::Normal,
            request_read_receipt: false,
            account_id,
            signature: String::new(),
        }
    }

    /// Create a reply draft
    pub fn reply(msg: &MessageSummary, body_quote: &str, account_id: u32) -> Self {
        let subject = if msg.subject.starts_with("Re: ") {
            msg.subject.clone()
        } else {
            format!("Re: {}", msg.subject)
        };

        let quoted = body_quote.lines()
            .map(|l| format!("> {l}"))
            .collect::<Vec<_>>()
            .join("\n");

        Self {
            to: vec![msg.from.to_string()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject,
            body: format!("\n\nOn {}, {} wrote:\n{quoted}", msg.date, msg.from),
            is_html: false,
            attachments: Vec::new(),
            in_reply_to: msg.message_id.clone(),
            references: msg.message_id.iter().cloned().collect(),
            priority: Priority::Normal,
            request_read_receipt: false,
            account_id,
            signature: String::new(),
        }
    }

    /// Create a forward draft
    pub fn forward(msg: &MessageSummary, body_text: &str, account_id: u32) -> Self {
        let subject = if msg.subject.starts_with("Fwd: ") {
            msg.subject.clone()
        } else {
            format!("Fwd: {}", msg.subject)
        };

        let body = format!(
            "\n\n---------- Forwarded message ----------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{body_text}",
            msg.from, msg.date, msg.subject,
            msg.to.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "),
        );

        Self {
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject,
            body,
            is_html: false,
            attachments: Vec::new(),
            in_reply_to: None,
            references: Vec::new(),
            priority: Priority::Normal,
            request_read_receipt: false,
            account_id,
            signature: String::new(),
        }
    }

    /// Build RFC 5322 email message
    pub fn build_message(&self, from: &EmailAddress) -> String {
        let mut msg = String::new();

        msg.push_str(&format!("From: {from}\r\n"));
        if !self.to.is_empty() {
            msg.push_str(&format!("To: {}\r\n", self.to.join(", ")));
        }
        if !self.cc.is_empty() {
            msg.push_str(&format!("Cc: {}\r\n", self.cc.join(", ")));
        }
        if !self.bcc.is_empty() {
            msg.push_str(&format!("Bcc: {}\r\n", self.bcc.join(", ")));
        }
        msg.push_str(&format!("Subject: {}\r\n", self.subject));
        msg.push_str("MIME-Version: 1.0\r\n");

        if let Some(ref reply_to) = self.in_reply_to {
            msg.push_str(&format!("In-Reply-To: <{reply_to}>\r\n"));
        }
        if !self.references.is_empty() {
            let refs: Vec<String> = self.references.iter().map(|r| format!("<{r}>")).collect();
            msg.push_str(&format!("References: {}\r\n", refs.join(" ")));
        }

        if self.priority != Priority::Normal {
            let (importance, x_priority) = match self.priority {
                Priority::High => ("high", "1"),
                Priority::Low => ("low", "5"),
                Priority::Normal => ("normal", "3"),
            };
            msg.push_str(&format!("Importance: {importance}\r\n"));
            msg.push_str(&format!("X-Priority: {x_priority}\r\n"));
        }

        let full_body = if self.signature.is_empty() {
            self.body.clone()
        } else {
            format!("{}\r\n\r\n-- \r\n{}", self.body, self.signature)
        };

        if self.attachments.is_empty() {
            // Simple text message
            let ct = if self.is_html { "text/html" } else { "text/plain" };
            msg.push_str(&format!("Content-Type: {ct}; charset=utf-8\r\n"));
            msg.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
            msg.push_str("\r\n");
            msg.push_str(&full_body);
        } else {
            // Multipart mixed
            let boundary = "----=_Part_Boundary_001";
            msg.push_str(&format!("Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n"));
            msg.push_str("\r\n");

            // Text part
            msg.push_str(&format!("--{boundary}\r\n"));
            let ct = if self.is_html { "text/html" } else { "text/plain" };
            msg.push_str(&format!("Content-Type: {ct}; charset=utf-8\r\n"));
            msg.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
            msg.push_str("\r\n");
            msg.push_str(&full_body);
            msg.push_str("\r\n");

            // Attachments
            for att in &self.attachments {
                msg.push_str(&format!("--{boundary}\r\n"));
                let disp = if att.is_inline { "inline" } else { "attachment" };
                msg.push_str(&format!("Content-Type: {}; name=\"{}\"\r\n", att.mime_type, att.filename));
                msg.push_str(&format!("Content-Disposition: {disp}; filename=\"{}\"\r\n", att.filename));
                msg.push_str("Content-Transfer-Encoding: base64\r\n");
                if let Some(ref cid) = att.content_id {
                    msg.push_str(&format!("Content-ID: <{cid}>\r\n"));
                }
                msg.push_str("\r\n");
                // Wrap base64 at 76 chars per line
                let encoded = base64_encode(&att.data);
                let mut pos = 0;
                while pos < encoded.len() {
                    let end = (pos.saturating_add(76)).min(encoded.len());
                    msg.push_str(encoded.get(pos..end).unwrap_or(""));
                    msg.push_str("\r\n");
                    pos = end;
                }
            }

            msg.push_str(&format!("--{boundary}--\r\n"));
        }

        msg
    }
}

// ─── Mail Filter Rules ──────────────────────────────────────────────

/// Condition for a mail filter rule
#[derive(Debug, Clone)]
pub enum FilterCondition {
    FromContains(String),
    ToContains(String),
    SubjectContains(String),
    BodyContains(String),
    HasAttachment,
    SizeGreaterThan(u64),
    SizeLessThan(u64),
}

impl FilterCondition {
    /// Test if a message matches this condition
    pub fn matches(&self, msg: &MessageSummary) -> bool {
        match self {
            Self::FromContains(s) => msg.from.to_string().to_lowercase().contains(&s.to_lowercase()),
            Self::ToContains(s) => msg.to.iter().any(|a| a.to_string().to_lowercase().contains(&s.to_lowercase())),
            Self::SubjectContains(s) => msg.subject.to_lowercase().contains(&s.to_lowercase()),
            Self::BodyContains(s) => msg.preview.to_lowercase().contains(&s.to_lowercase()),
            Self::HasAttachment => msg.flags.has_attachment,
            Self::SizeGreaterThan(n) => msg.size > *n,
            Self::SizeLessThan(n) => msg.size < *n,
        }
    }
}

/// Action to perform when a filter matches
#[derive(Debug, Clone)]
pub enum FilterAction {
    MoveTo(String),
    MarkAsRead,
    MarkAsFlagged,
    Delete,
    AddLabel(String),
    ForwardTo(String),
}

/// A mail filter rule
#[derive(Debug, Clone)]
pub struct FilterRule {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub conditions: Vec<FilterCondition>,
    pub match_all: bool, // true = AND, false = OR
    pub actions: Vec<FilterAction>,
    pub stop_processing: bool,
}

impl FilterRule {
    /// Check if a message matches this rule
    pub fn matches(&self, msg: &MessageSummary) -> bool {
        if !self.enabled || self.conditions.is_empty() {
            return false;
        }
        if self.match_all {
            self.conditions.iter().all(|c| c.matches(msg))
        } else {
            self.conditions.iter().any(|c| c.matches(msg))
        }
    }
}

// ─── Signature Manager ──────────────────────────────────────────────

/// Email signature
#[derive(Debug, Clone)]
pub struct Signature {
    pub id: u32,
    pub name: String,
    pub text: String,
    pub is_html: bool,
    pub is_default: bool,
}

// ─── Application ─────────────────────────────────────────────────────

use guitk::Color;
use guitk::render::{RenderCommand, FontWeightHint};
use guitk::style::CornerRadii;

mod colors {
    use guitk::Color;
    pub const BASE: Color = Color::from_hex(0x1E1E2E);
    pub const MANTLE: Color = Color::from_hex(0x181825);
    pub const CRUST: Color = Color::from_hex(0x11111B);
    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const SURFACE1: Color = Color::from_hex(0x45475A);
    pub const _SURFACE2: Color = Color::from_hex(0x585B70);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);
    pub const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
    pub const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const GREEN: Color = Color::from_hex(0xA6E3A1);
    pub const RED: Color = Color::from_hex(0xF38BA8);
    pub const YELLOW: Color = Color::from_hex(0xF9E2AF);
    pub const PEACH: Color = Color::from_hex(0xFAB387);
    pub const _TEAL: Color = Color::from_hex(0x94E2D5);
    pub const _LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const OVERLAY0: Color = Color::from_hex(0x6C7086);
    pub const _MAUVE: Color = Color::from_hex(0xCBA6F7);
}

/// Active UI panel
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    MessageList,
    Reading,
    Compose,
    Settings,
}

/// Sort order for message list
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    DateDesc,
    DateAsc,
    SenderAsc,
    SubjectAsc,
    SizeDesc,
}

/// Main email application
pub struct EmailApp {
    pub accounts: Vec<EmailAccount>,
    pub mailboxes: Vec<Mailbox>,
    pub messages: Vec<MessageSummary>,
    pub threads: Vec<MessageThread>,
    pub selected_mailbox: Option<String>,
    pub selected_message: Option<u64>,
    pub selected_account: Option<u32>,
    pub active_panel: Panel,
    pub search_query: String,
    pub sort_order: SortOrder,
    pub compose_draft: Option<EmailDraft>,
    pub filter_rules: Vec<FilterRule>,
    pub signatures: Vec<Signature>,
    pub next_account_id: u32,
    pub next_message_id: u64,
    pub next_rule_id: u32,
    pub next_sig_id: u32,
    pub status_message: String,
    pub show_cc: bool,
    pub show_bcc: bool,
    pub threaded_view: bool,
    pub reading_pane_position: ReadingPanePosition,
    pub unread_count: u32,
}

/// Reading pane position
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReadingPanePosition {
    Right,
    Bottom,
    Off,
}

impl EmailApp {
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            mailboxes: Vec::new(),
            messages: Vec::new(),
            threads: Vec::new(),
            selected_mailbox: Some("Inbox".to_string()),
            selected_message: None,
            selected_account: None,
            active_panel: Panel::MessageList,
            search_query: String::new(),
            sort_order: SortOrder::DateDesc,
            compose_draft: None,
            filter_rules: Vec::new(),
            signatures: Vec::new(),
            next_account_id: 1,
            next_message_id: 1,
            next_rule_id: 1,
            next_sig_id: 1,
            status_message: "Ready".to_string(),
            show_cc: false,
            show_bcc: false,
            threaded_view: true,
            reading_pane_position: ReadingPanePosition::Right,
            unread_count: 0,
        }
    }

    /// Add an email account
    pub fn add_account(&mut self, mut account: EmailAccount) -> u32 {
        let id = self.next_account_id;
        self.next_account_id = self.next_account_id.saturating_add(1);
        account.id = id;

        // Create standard mailboxes for this account
        let standard = [
            ("Inbox", MailboxType::Inbox, "INBOX"),
            ("Sent", MailboxType::Sent, "Sent"),
            ("Drafts", MailboxType::Drafts, "Drafts"),
            ("Trash", MailboxType::Trash, "Trash"),
            ("Spam", MailboxType::Spam, "Spam"),
            ("Archive", MailboxType::Archive, "Archive"),
        ];
        for (name, mtype, imap_path) in &standard {
            self.mailboxes.push(Mailbox {
                name: name.to_string(),
                mailbox_type: *mtype,
                total_messages: 0,
                unread_messages: 0,
                account_id: id,
                imap_path: imap_path.to_string(),
            });
        }

        self.accounts.push(account);
        self.selected_account = Some(id);
        self.status_message = format!("Account added (ID: {id})");
        id
    }

    /// Remove an account
    pub fn remove_account(&mut self, id: u32) {
        self.accounts.retain(|a| a.id != id);
        self.mailboxes.retain(|m| m.account_id != id);
        self.messages.retain(|m| m.account_id != id);
        if self.selected_account == Some(id) {
            self.selected_account = self.accounts.first().map(|a| a.id);
        }
    }

    /// Add a message to the store
    pub fn add_message(&mut self, mut msg: MessageSummary) -> u64 {
        let id = self.next_message_id;
        self.next_message_id = self.next_message_id.saturating_add(1);
        msg.id = id;
        if !msg.flags.seen {
            self.unread_count = self.unread_count.saturating_add(1);
        }
        // Update mailbox counts
        for mb in &mut self.mailboxes {
            if mb.name == msg.mailbox && mb.account_id == msg.account_id {
                mb.total_messages = mb.total_messages.saturating_add(1);
                if !msg.flags.seen {
                    mb.unread_messages = mb.unread_messages.saturating_add(1);
                }
            }
        }
        self.messages.push(msg);
        id
    }

    /// Mark a message as read
    pub fn mark_read(&mut self, id: u64) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
            if !msg.flags.seen {
                msg.flags.seen = true;
                self.unread_count = self.unread_count.saturating_sub(1);
                for mb in &mut self.mailboxes {
                    if mb.name == msg.mailbox && mb.account_id == msg.account_id {
                        mb.unread_messages = mb.unread_messages.saturating_sub(1);
                    }
                }
            }
        }
    }

    /// Mark a message as unread
    pub fn mark_unread(&mut self, id: u64) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
            if msg.flags.seen {
                msg.flags.seen = false;
                self.unread_count = self.unread_count.saturating_add(1);
                for mb in &mut self.mailboxes {
                    if mb.name == msg.mailbox && mb.account_id == msg.account_id {
                        mb.unread_messages = mb.unread_messages.saturating_add(1);
                    }
                }
            }
        }
    }

    /// Toggle flagged status
    pub fn toggle_flagged(&mut self, id: u64) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
            msg.flags.flagged = !msg.flags.flagged;
        }
    }

    /// Move message to a different mailbox
    pub fn move_message(&mut self, id: u64, target_mailbox: &str) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
            let old_mailbox = msg.mailbox.clone();
            let account_id = msg.account_id;
            let was_unread = !msg.flags.seen;

            // Update old mailbox counts
            for mb in &mut self.mailboxes {
                if mb.name == old_mailbox && mb.account_id == account_id {
                    mb.total_messages = mb.total_messages.saturating_sub(1);
                    if was_unread {
                        mb.unread_messages = mb.unread_messages.saturating_sub(1);
                    }
                }
            }

            msg.mailbox = target_mailbox.to_string();

            // Update new mailbox counts
            for mb in &mut self.mailboxes {
                if mb.name == target_mailbox && mb.account_id == account_id {
                    mb.total_messages = mb.total_messages.saturating_add(1);
                    if was_unread {
                        mb.unread_messages = mb.unread_messages.saturating_add(1);
                    }
                }
            }
        }
    }

    /// Delete a message (move to trash, or permanently delete if already in trash)
    pub fn delete_message(&mut self, id: u64) {
        if let Some(msg) = self.messages.iter().find(|m| m.id == id) {
            if msg.mailbox == "Trash" {
                // Permanent delete
                let was_unread = !msg.flags.seen;
                let mailbox = msg.mailbox.clone();
                let account_id = msg.account_id;
                self.messages.retain(|m| m.id != id);
                for mb in &mut self.mailboxes {
                    if mb.name == mailbox && mb.account_id == account_id {
                        mb.total_messages = mb.total_messages.saturating_sub(1);
                        if was_unread {
                            mb.unread_messages = mb.unread_messages.saturating_sub(1);
                        }
                    }
                }
                if was_unread {
                    self.unread_count = self.unread_count.saturating_sub(1);
                }
            } else {
                self.move_message(id, "Trash");
            }
        }
    }

    /// Start composing a new email
    pub fn compose_new(&mut self) {
        let account_id = self.selected_account.unwrap_or(0);
        let mut draft = EmailDraft::new(account_id);
        if let Some(acct) = self.accounts.iter().find(|a| a.id == account_id) {
            draft.signature = acct.signature.clone();
        }
        self.compose_draft = Some(draft);
        self.active_panel = Panel::Compose;
    }

    /// Start composing a reply
    pub fn compose_reply(&mut self, message_id: u64) {
        if let Some(msg) = self.messages.iter().find(|m| m.id == message_id) {
            let account_id = self.selected_account.unwrap_or(msg.account_id);
            let draft = EmailDraft::reply(msg, &msg.preview, account_id);
            self.compose_draft = Some(draft);
            self.active_panel = Panel::Compose;
        }
    }

    /// Start composing a forward
    pub fn compose_forward(&mut self, message_id: u64) {
        if let Some(msg) = self.messages.iter().find(|m| m.id == message_id) {
            let account_id = self.selected_account.unwrap_or(msg.account_id);
            let draft = EmailDraft::forward(msg, &msg.preview, account_id);
            self.compose_draft = Some(draft);
            self.active_panel = Panel::Compose;
        }
    }

    /// Get messages for current mailbox, filtered and sorted
    pub fn current_messages(&self) -> Vec<&MessageSummary> {
        let account_id = self.selected_account;
        let mailbox = self.selected_mailbox.as_deref();

        let mut msgs: Vec<&MessageSummary> = self.messages.iter()
            .filter(|m| {
                account_id.map_or(true, |aid| m.account_id == aid)
                    && mailbox.map_or(true, |mb| m.mailbox == mb)
            })
            .filter(|m| {
                if self.search_query.is_empty() {
                    true
                } else {
                    let q = self.search_query.to_lowercase();
                    m.subject.to_lowercase().contains(&q)
                        || m.from.to_string().to_lowercase().contains(&q)
                        || m.preview.to_lowercase().contains(&q)
                }
            })
            .collect();

        msgs.sort_by(|a, b| {
            match self.sort_order {
                SortOrder::DateDesc => b.timestamp.cmp(&a.timestamp),
                SortOrder::DateAsc => a.timestamp.cmp(&b.timestamp),
                SortOrder::SenderAsc => a.from.to_string().cmp(&b.from.to_string()),
                SortOrder::SubjectAsc => a.subject.cmp(&b.subject),
                SortOrder::SizeDesc => b.size.cmp(&a.size),
            }
        });

        msgs
    }

    /// Add a filter rule
    pub fn add_filter_rule(&mut self, mut rule: FilterRule) -> u32 {
        let id = self.next_rule_id;
        self.next_rule_id = self.next_rule_id.saturating_add(1);
        rule.id = id;
        self.filter_rules.push(rule);
        id
    }

    /// Add a signature
    pub fn add_signature(&mut self, mut sig: Signature) -> u32 {
        let id = self.next_sig_id;
        self.next_sig_id = self.next_sig_id.saturating_add(1);
        sig.id = id;
        self.signatures.push(sig);
        id
    }

    /// Apply filter rules to a message
    pub fn apply_filters(&mut self, message_id: u64) -> Vec<String> {
        let mut applied = Vec::new();
        let msg = match self.messages.iter().find(|m| m.id == message_id) {
            Some(m) => m.clone(),
            None => return applied,
        };

        // Collect matching actions first to avoid borrow conflict
        let mut pending_actions: Vec<FilterAction> = Vec::new();
        for rule in &self.filter_rules {
            if rule.matches(&msg) {
                applied.push(rule.name.clone());
                pending_actions.extend(rule.actions.iter().cloned());
                if rule.stop_processing {
                    break;
                }
            }
        }

        // Now apply collected actions
        for action in &pending_actions {
            match action {
                FilterAction::MoveTo(target) => {
                    self.move_message(message_id, target);
                }
                FilterAction::MarkAsRead => {
                    self.mark_read(message_id);
                }
                FilterAction::MarkAsFlagged => {
                    if let Some(m) = self.messages.iter_mut().find(|m| m.id == message_id) {
                        m.flags.flagged = true;
                    }
                }
                FilterAction::Delete => {
                    self.delete_message(message_id);
                }
                FilterAction::AddLabel(label) => {
                    if let Some(m) = self.messages.iter_mut().find(|m| m.id == message_id) {
                        if !m.labels.contains(label) {
                            m.labels.push(label.clone());
                        }
                    }
                }
                FilterAction::ForwardTo(_) => {
                    // Would forward via SMTP — not implemented in offline mode
                }
            }
        }

        applied
    }

    /// Render the UI
    pub fn render(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        let header_h = 48.0;
        let toolbar_h = 40.0;
        let sidebar_w = 200.0;
        let status_h = 24.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height,
            color: colors::BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: 0.0, width, height: header_h,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Text {
            x: 16.0, y: 14.0,
            text: "Mail".to_string(),
            font_size: 18.0,
            color: colors::BLUE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Unread badge
        if self.unread_count > 0 {
            cmds.push(RenderCommand::FillRect {
                x: 70.0, y: 10.0, width: 32.0, height: 22.0,
                color: colors::RED,
                corner_radii: CornerRadii::all(11.0),
            });
            cmds.push(RenderCommand::Text {
                x: 78.0, y: 14.0,
                text: self.unread_count.to_string(),
                font_size: 12.0, color: colors::BASE,
                font_weight: FontWeightHint::Bold, max_width: None,
            });
        }

        // Search bar
        cmds.push(RenderCommand::FillRect {
            x: 120.0, y: 10.0, width: 300.0, height: 28.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let search_text = if self.search_query.is_empty() {
            "Search mail...".to_string()
        } else {
            self.search_query.clone()
        };
        cmds.push(RenderCommand::Text {
            x: 132.0, y: 17.0,
            text: search_text,
            font_size: 12.0,
            color: if self.search_query.is_empty() { colors::OVERLAY0 } else { colors::TEXT },
            font_weight: FontWeightHint::Regular,
            max_width: Some(276.0),
        });

        // Toolbar
        let ty = header_h;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: ty, width, height: toolbar_h,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        let buttons = ["Compose", "Reply", "Forward", "Delete", "Archive", "Spam"];
        let mut bx = 16.0;
        for label in &buttons {
            let bw = label.len() as f32 * 7.5 + 20.0;
            cmds.push(RenderCommand::FillRect {
                x: bx, y: ty + 6.0, width: bw, height: 28.0,
                color: if *label == "Compose" { colors::BLUE } else { colors::SURFACE0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 10.0, y: ty + 12.0,
                text: label.to_string(),
                font_size: 12.0,
                color: if *label == "Compose" { colors::BASE } else { colors::TEXT },
                font_weight: if *label == "Compose" { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: None,
            });
            bx += bw + 8.0;
        }

        // Sidebar (mailbox list)
        let content_y = header_h + toolbar_h;
        let content_h = height - content_y - status_h;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: content_y, width: sidebar_w, height: content_h,
            color: colors::MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Account name
        if let Some(acct) = self.selected_account
            .and_then(|id| self.accounts.iter().find(|a| a.id == id))
        {
            cmds.push(RenderCommand::Text {
                x: 12.0, y: content_y + 8.0,
                text: acct.name.clone(),
                font_size: 12.0, color: acct.color,
                font_weight: FontWeightHint::Bold, max_width: Some(sidebar_w - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: 12.0, y: content_y + 24.0,
                text: acct.email.clone(),
                font_size: 10.0, color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: Some(sidebar_w - 24.0),
            });
        }

        // Mailbox list
        let mut my = content_y + 48.0;
        let account_id = self.selected_account;
        for mb in &self.mailboxes {
            if account_id.map_or(false, |aid| mb.account_id != aid) {
                continue;
            }
            let is_sel = self.selected_mailbox.as_deref() == Some(&mb.name);
            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: 4.0, y: my, width: sidebar_w - 8.0, height: 28.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            let icon = match mb.mailbox_type {
                MailboxType::Inbox => "📥",
                MailboxType::Sent => "📤",
                MailboxType::Drafts => "📝",
                MailboxType::Trash => "🗑",
                MailboxType::Spam => "⚠",
                MailboxType::Archive => "📦",
                MailboxType::Custom => "📁",
            };

            cmds.push(RenderCommand::Text {
                x: 12.0, y: my + 7.0,
                text: format!("{icon} {}", mb.name),
                font_size: 12.0,
                color: if is_sel { colors::BLUE } else { colors::SUBTEXT1 },
                font_weight: if is_sel { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(sidebar_w - 60.0),
            });

            if mb.unread_messages > 0 {
                cmds.push(RenderCommand::Text {
                    x: sidebar_w - 40.0, y: my + 7.0,
                    text: mb.unread_messages.to_string(),
                    font_size: 11.0,
                    color: colors::BLUE,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
            }

            my += 32.0;
        }

        // Message list area
        let list_x = sidebar_w;
        let list_w = match self.reading_pane_position {
            ReadingPanePosition::Right => (width - sidebar_w) * 0.4,
            ReadingPanePosition::Bottom => width - sidebar_w,
            ReadingPanePosition::Off => width - sidebar_w,
        };

        self.render_message_list(&mut cmds, list_x, content_y, list_w, content_h);

        // Reading pane
        if self.reading_pane_position == ReadingPanePosition::Right {
            let reading_x = list_x + list_w;
            let reading_w = width - reading_x;
            self.render_reading_pane(&mut cmds, reading_x, content_y, reading_w, content_h);
        }

        // Status bar
        let sy = height - status_h;
        cmds.push(RenderCommand::FillRect {
            x: 0.0, y: sy, width, height: status_h,
            color: colors::CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        let total_msgs: u32 = self.mailboxes.iter()
            .filter(|mb| self.selected_account.map_or(true, |aid| mb.account_id == aid))
            .map(|mb| mb.total_messages)
            .sum();
        cmds.push(RenderCommand::Text {
            x: 12.0, y: sy + 6.0,
            text: format!(
                "{} messages, {} unread  |  {}  |  {}",
                total_msgs, self.unread_count,
                self.accounts.len(),
                self.status_message,
            ),
            font_size: 11.0, color: colors::SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width - 24.0),
        });

        cmds
    }

    fn render_message_list(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, h: f32) {
        let messages = self.current_messages();
        let row_h = 64.0;
        let mut ry = y + 4.0;

        if messages.is_empty() {
            cmds.push(RenderCommand::Text {
                x: x + w / 2.0 - 60.0, y: y + h / 2.0 - 10.0,
                text: "No messages".to_string(),
                font_size: 14.0, color: colors::OVERLAY0,
                font_weight: FontWeightHint::Regular, max_width: None,
            });
            return;
        }

        for msg in messages.iter().take(((h - 4.0) / row_h) as usize) {
            if ry + row_h > y + h { break; }

            let is_sel = self.selected_message == Some(msg.id);
            let is_unread = !msg.flags.seen;

            if is_sel {
                cmds.push(RenderCommand::FillRect {
                    x: x + 4.0, y: ry, width: w - 8.0, height: row_h - 2.0,
                    color: colors::SURFACE0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }

            // Unread indicator
            if is_unread {
                cmds.push(RenderCommand::FillRect {
                    x: x + 6.0, y: ry + 24.0, width: 6.0, height: 6.0,
                    color: colors::BLUE,
                    corner_radii: CornerRadii::all(3.0),
                });
            }

            // Flagged indicator
            if msg.flags.flagged {
                cmds.push(RenderCommand::Text {
                    x: x + w - 24.0, y: ry + 6.0,
                    text: "★".to_string(),
                    font_size: 14.0, color: colors::YELLOW,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });
            }

            let text_x = x + 20.0;
            let max_w = w - 48.0;

            // Sender
            cmds.push(RenderCommand::Text {
                x: text_x, y: ry + 6.0,
                text: msg.from.display_name.clone().unwrap_or_else(|| msg.from.address()),
                font_size: 12.0,
                color: if is_unread { colors::TEXT } else { colors::SUBTEXT1 },
                font_weight: if is_unread { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(max_w * 0.6),
            });

            // Date
            cmds.push(RenderCommand::Text {
                x: x + w - 80.0, y: ry + 6.0,
                text: msg.date.clone(),
                font_size: 10.0, color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular, max_width: Some(70.0),
            });

            // Subject
            cmds.push(RenderCommand::Text {
                x: text_x, y: ry + 24.0,
                text: msg.subject.clone(),
                font_size: 12.0,
                color: if is_unread { colors::TEXT } else { colors::SUBTEXT1 },
                font_weight: if is_unread { FontWeightHint::Bold } else { FontWeightHint::Regular },
                max_width: Some(max_w),
            });

            // Preview
            cmds.push(RenderCommand::Text {
                x: text_x, y: ry + 42.0,
                text: msg.preview.clone(),
                font_size: 11.0, color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
            });

            // Attachment indicator
            if msg.flags.has_attachment {
                cmds.push(RenderCommand::Text {
                    x: x + w - 44.0, y: ry + 24.0,
                    text: "📎".to_string(),
                    font_size: 12.0, color: colors::SUBTEXT0,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });
            }

            // Priority indicator
            if msg.priority == Priority::High {
                cmds.push(RenderCommand::Text {
                    x: x + w - 44.0, y: ry + 42.0,
                    text: "❗".to_string(),
                    font_size: 12.0, color: colors::RED,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });
            }

            // Labels
            let mut lx = text_x;
            for label in &msg.labels {
                let lw = label.len() as f32 * 6.0 + 10.0;
                if lx + lw > x + max_w { break; }
                cmds.push(RenderCommand::FillRect {
                    x: lx, y: ry + 54.0, width: lw, height: 14.0,
                    color: colors::SURFACE1,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: lx + 5.0, y: ry + 55.0,
                    text: label.clone(),
                    font_size: 9.0, color: colors::PEACH,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });
                lx += lw + 4.0;
            }

            ry += row_h;
        }
    }

    fn render_reading_pane(&self, cmds: &mut Vec<RenderCommand>, x: f32, y: f32, w: f32, _h: f32) {
        // Separator line
        cmds.push(RenderCommand::FillRect {
            x, y, width: 1.0, height: _h,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let msg = match self.selected_message
            .and_then(|id| self.messages.iter().find(|m| m.id == id))
        {
            Some(m) => m,
            None => {
                cmds.push(RenderCommand::Text {
                    x: x + w / 2.0 - 80.0, y: y + _h / 2.0 - 10.0,
                    text: "Select a message to read".to_string(),
                    font_size: 13.0, color: colors::OVERLAY0,
                    font_weight: FontWeightHint::Regular, max_width: None,
                });
                return;
            }
        };

        let px = x + 16.0;
        let max_w = w - 32.0;
        let mut py = y + 16.0;

        // Subject
        cmds.push(RenderCommand::Text {
            x: px, y: py,
            text: msg.subject.clone(),
            font_size: 16.0, color: colors::TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
        });
        py += 28.0;

        // From
        cmds.push(RenderCommand::Text {
            x: px, y: py,
            text: format!("From: {}", msg.from),
            font_size: 12.0, color: colors::SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
        });
        py += 18.0;

        // To
        let to_str = msg.to.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
        cmds.push(RenderCommand::Text {
            x: px, y: py,
            text: format!("To: {to_str}"),
            font_size: 12.0, color: colors::SUBTEXT1,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
        });
        py += 18.0;

        // CC
        if !msg.cc.is_empty() {
            let cc_str = msg.cc.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
            cmds.push(RenderCommand::Text {
                x: px, y: py,
                text: format!("Cc: {cc_str}"),
                font_size: 12.0, color: colors::SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
            });
            py += 18.0;
        }

        // Date
        cmds.push(RenderCommand::Text {
            x: px, y: py,
            text: format!("Date: {}", msg.date),
            font_size: 11.0, color: colors::OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        py += 24.0;

        // Separator
        cmds.push(RenderCommand::FillRect {
            x: px, y: py, width: max_w, height: 1.0,
            color: colors::SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });
        py += 12.0;

        // Body preview
        cmds.push(RenderCommand::Text {
            x: px, y: py,
            text: msg.preview.clone(),
            font_size: 13.0, color: colors::TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max_w),
        });
    }
}

// ─── Main ────────────────────────────────────────────────────────────

fn main() {
    let mut app = EmailApp::new();

    // Add sample account
    let acct = EmailAccount::gmail("user@gmail.com", "John Doe");
    let acct_id = app.add_account(acct);

    // Add sample messages
    let sample_messages = create_sample_messages(acct_id);
    for msg in sample_messages {
        app.add_message(msg);
    }

    // Add a filter rule
    app.add_filter_rule(FilterRule {
        id: 0,
        name: "Newsletter to Archive".to_string(),
        enabled: true,
        conditions: vec![FilterCondition::SubjectContains("newsletter".to_string())],
        match_all: false,
        actions: vec![FilterAction::MoveTo("Archive".to_string()), FilterAction::MarkAsRead],
        stop_processing: true,
    });

    // Add a signature
    app.add_signature(Signature {
        id: 0,
        name: "Default".to_string(),
        text: "Best regards,\nJohn Doe\njohn@example.com".to_string(),
        is_html: false,
        is_default: true,
    });

    let cmds = app.render(1400.0, 900.0);
    let _ = cmds;
}

fn create_sample_messages(account_id: u32) -> Vec<MessageSummary> {
    vec![
        MessageSummary {
            id: 0,
            uid: 1001,
            from: EmailAddress {
                display_name: Some("Alice Smith".to_string()),
                local_part: "alice".to_string(),
                domain: "example.com".to_string(),
            },
            to: vec![EmailAddress {
                display_name: Some("John Doe".to_string()),
                local_part: "john".to_string(),
                domain: "gmail.com".to_string(),
            }],
            cc: Vec::new(),
            subject: "Meeting Tomorrow".to_string(),
            date: "May 18, 2026".to_string(),
            timestamp: 1_779_000_000,
            preview: "Hi John, just a reminder about our meeting tomorrow at 10am. We'll be discussing the Q2 roadmap and resource allocation.".to_string(),
            flags: MessageFlags { seen: false, ..MessageFlags::default() },
            priority: Priority::Normal,
            account_id,
            mailbox: "Inbox".to_string(),
            message_id: Some("msg001@example.com".to_string()),
            in_reply_to: None,
            thread_id: Some(1),
            size: 2048,
            labels: Vec::new(),
        },
        MessageSummary {
            id: 0,
            uid: 1002,
            from: EmailAddress {
                display_name: Some("Bob Wilson".to_string()),
                local_part: "bob".to_string(),
                domain: "company.org".to_string(),
            },
            to: vec![EmailAddress {
                display_name: Some("John Doe".to_string()),
                local_part: "john".to_string(),
                domain: "gmail.com".to_string(),
            }],
            cc: Vec::new(),
            subject: "Project Update: Phase 2 Complete".to_string(),
            date: "May 17, 2026".to_string(),
            timestamp: 1_778_900_000,
            preview: "Great news! Phase 2 of the project has been completed ahead of schedule. All tests are passing and documentation is up to date.".to_string(),
            flags: MessageFlags { seen: true, flagged: true, has_attachment: true, ..MessageFlags::default() },
            priority: Priority::Normal,
            account_id,
            mailbox: "Inbox".to_string(),
            message_id: Some("msg002@company.org".to_string()),
            in_reply_to: None,
            thread_id: Some(2),
            size: 15360,
            labels: vec!["Work".to_string()],
        },
        MessageSummary {
            id: 0,
            uid: 1003,
            from: EmailAddress {
                display_name: Some("GitHub".to_string()),
                local_part: "noreply".to_string(),
                domain: "github.com".to_string(),
            },
            to: vec![EmailAddress {
                display_name: Some("John Doe".to_string()),
                local_part: "john".to_string(),
                domain: "gmail.com".to_string(),
            }],
            cc: Vec::new(),
            subject: "[repo/project] Pull Request #42: Fix memory leak in cache".to_string(),
            date: "May 17, 2026".to_string(),
            timestamp: 1_778_850_000,
            preview: "A new pull request has been opened. The PR fixes a memory leak in the LRU cache that was causing the process to consume increasing amounts of memory.".to_string(),
            flags: MessageFlags { seen: false, ..MessageFlags::default() },
            priority: Priority::Normal,
            account_id,
            mailbox: "Inbox".to_string(),
            message_id: Some("msg003@github.com".to_string()),
            in_reply_to: None,
            thread_id: Some(3),
            size: 4096,
            labels: vec!["GitHub".to_string()],
        },
        MessageSummary {
            id: 0,
            uid: 1004,
            from: EmailAddress {
                display_name: Some("Security Team".to_string()),
                local_part: "security".to_string(),
                domain: "company.org".to_string(),
            },
            to: vec![EmailAddress {
                display_name: Some("John Doe".to_string()),
                local_part: "john".to_string(),
                domain: "gmail.com".to_string(),
            }],
            cc: Vec::new(),
            subject: "URGENT: Password Reset Required".to_string(),
            date: "May 16, 2026".to_string(),
            timestamp: 1_778_800_000,
            preview: "Due to a recent security audit, all employees are required to reset their passwords by end of day Friday. Please use the secure portal.".to_string(),
            flags: MessageFlags { seen: false, ..MessageFlags::default() },
            priority: Priority::High,
            account_id,
            mailbox: "Inbox".to_string(),
            message_id: Some("msg004@company.org".to_string()),
            in_reply_to: None,
            thread_id: Some(4),
            size: 1536,
            labels: vec!["Important".to_string()],
        },
        MessageSummary {
            id: 0,
            uid: 1005,
            from: EmailAddress {
                display_name: Some("Newsletter".to_string()),
                local_part: "newsletter".to_string(),
                domain: "techblog.com".to_string(),
            },
            to: vec![EmailAddress {
                display_name: Some("John Doe".to_string()),
                local_part: "john".to_string(),
                domain: "gmail.com".to_string(),
            }],
            cc: Vec::new(),
            subject: "Weekly Tech Digest: Rust 2024, WebAssembly Updates".to_string(),
            date: "May 15, 2026".to_string(),
            timestamp: 1_778_700_000,
            preview: "This week in tech: Rust 2024 edition is out with new language features, WebAssembly gets component model support, and Linux 7.0 is released.".to_string(),
            flags: MessageFlags { seen: true, ..MessageFlags::default() },
            priority: Priority::Low,
            account_id,
            mailbox: "Inbox".to_string(),
            message_id: Some("msg005@techblog.com".to_string()),
            in_reply_to: None,
            thread_id: Some(5),
            size: 8192,
            labels: vec!["Newsletter".to_string()],
        },
    ]
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Email address parsing tests
    #[test]
    fn test_parse_simple_address() {
        let addr = EmailAddress::parse("user@example.com").unwrap();
        assert_eq!(addr.local_part, "user");
        assert_eq!(addr.domain, "example.com");
        assert!(addr.display_name.is_none());
    }

    #[test]
    fn test_parse_display_name_address() {
        let addr = EmailAddress::parse("John Doe <john@example.com>").unwrap();
        assert_eq!(addr.display_name.as_deref(), Some("John Doe"));
        assert_eq!(addr.local_part, "john");
        assert_eq!(addr.domain, "example.com");
    }

    #[test]
    fn test_parse_quoted_display_name() {
        let addr = EmailAddress::parse("\"Doe, John\" <john@example.com>").unwrap();
        assert_eq!(addr.display_name.as_deref(), Some("Doe, John"));
    }

    #[test]
    fn test_parse_invalid_address() {
        assert!(EmailAddress::parse("not-an-email").is_none());
        assert!(EmailAddress::parse("@domain.com").is_none());
        assert!(EmailAddress::parse("user@").is_none());
    }

    #[test]
    fn test_address_display() {
        let addr = EmailAddress {
            display_name: Some("Alice".to_string()),
            local_part: "alice".to_string(),
            domain: "test.com".to_string(),
        };
        assert_eq!(addr.to_string(), "Alice <alice@test.com>");
    }

    // Content-Type tests
    #[test]
    fn test_content_type_simple() {
        let ct = ContentType::parse("text/plain");
        assert_eq!(ct.media_type, "text");
        assert_eq!(ct.subtype, "plain");
    }

    #[test]
    fn test_content_type_with_charset() {
        let ct = ContentType::parse("text/html; charset=utf-8");
        assert_eq!(ct.mime_type(), "text/html");
        assert_eq!(ct.charset(), "utf-8");
    }

    #[test]
    fn test_content_type_multipart() {
        let ct = ContentType::parse("multipart/mixed; boundary=\"----=_Part_001\"");
        assert!(ct.is_multipart());
        assert_eq!(ct.boundary(), Some("----=_Part_001"));
    }

    // Base64 tests
    #[test]
    fn test_base64_roundtrip() {
        let original = b"Hello, World!";
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
    }

    // Quoted-printable tests
    #[test]
    fn test_qp_decode() {
        let decoded = quoted_printable_decode("Hello=20World");
        assert_eq!(decoded, b"Hello World");
    }

    #[test]
    fn test_qp_soft_linebreak() {
        let decoded = quoted_printable_decode("Hello=\r\nWorld");
        assert_eq!(decoded, b"HelloWorld");
    }

    // Header parsing tests
    #[test]
    fn test_header_parse() {
        let raw = "From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test";
        let headers = EmailHeaders::parse(raw);
        assert_eq!(headers.get("From"), Some("alice@example.com"));
        assert_eq!(headers.get("Subject"), Some("Test"));
    }

    #[test]
    fn test_header_case_insensitive() {
        let raw = "Content-Type: text/plain";
        let headers = EmailHeaders::parse(raw);
        assert_eq!(headers.get("content-type"), Some("text/plain"));
    }

    #[test]
    fn test_header_folding() {
        let raw = "Subject: This is a very long\r\n subject line";
        let headers = EmailHeaders::parse(raw);
        assert_eq!(headers.get("Subject"), Some("This is a very long subject line"));
    }

    // MIME parsing tests
    #[test]
    fn test_parse_simple_email() {
        let raw = "From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test\r\nContent-Type: text/plain\r\n\r\nHello World";
        let msg = EmailMessage::parse(raw).unwrap();
        assert_eq!(msg.subject, "Test");
        assert_eq!(msg.plain_text().as_deref(), Some("Hello World"));
    }

    #[test]
    fn test_parse_email_addresses() {
        let raw = "From: Alice <alice@example.com>\r\nTo: bob@example.com, Charlie <charlie@test.com>\r\n\r\nBody";
        let msg = EmailMessage::parse(raw).unwrap();
        assert_eq!(msg.from.as_ref().unwrap().display_name.as_deref(), Some("Alice"));
        assert_eq!(msg.to.len(), 2);
    }

    // IMAP command tests
    #[test]
    fn test_imap_login() {
        let cmd = ImapCommand::login("A001", "user", "pass");
        assert_eq!(cmd, "A001 LOGIN user pass\r\n");
    }

    #[test]
    fn test_imap_select() {
        let cmd = ImapCommand::select("A002", "INBOX");
        assert_eq!(cmd, "A002 SELECT \"INBOX\"\r\n");
    }

    #[test]
    fn test_imap_search() {
        let cmd = ImapCommand::search("A003", "UNSEEN");
        assert_eq!(cmd, "A003 SEARCH UNSEEN\r\n");
    }

    #[test]
    fn test_imap_store() {
        let cmd = ImapCommand::store("A004", "1:5", "+FLAGS", "\\Seen");
        assert_eq!(cmd, "A004 STORE 1:5 +FLAGS (\\Seen)\r\n");
    }

    // SMTP command tests
    #[test]
    fn test_smtp_ehlo() {
        assert_eq!(SmtpCommand::ehlo("client.example.com"), "EHLO client.example.com\r\n");
    }

    #[test]
    fn test_smtp_mail_from() {
        assert_eq!(SmtpCommand::mail_from("user@example.com"), "MAIL FROM:<user@example.com>\r\n");
    }

    #[test]
    fn test_smtp_auth_plain() {
        let cmd = SmtpCommand::auth_plain("user", "pass");
        assert!(cmd.starts_with("AUTH PLAIN "));
    }

    // Account tests
    #[test]
    fn test_gmail_account() {
        let acct = EmailAccount::gmail("user@gmail.com", "Test User");
        assert_eq!(acct.incoming.hostname, "imap.gmail.com");
        assert_eq!(acct.outgoing.hostname, "smtp.gmail.com");
        assert_eq!(acct.incoming.port, 993);
    }

    #[test]
    fn test_outlook_account() {
        let acct = EmailAccount::outlook("user@outlook.com", "Test User");
        assert_eq!(acct.incoming.hostname, "outlook.office365.com");
        assert_eq!(acct.outgoing.port, 587);
    }

    // App tests
    #[test]
    fn test_app_add_account() {
        let mut app = EmailApp::new();
        let acct = EmailAccount::gmail("test@gmail.com", "Test");
        let id = app.add_account(acct);
        assert_eq!(app.accounts.len(), 1);
        assert_eq!(app.mailboxes.len(), 6); // 6 standard folders
        assert_eq!(app.selected_account, Some(id));
    }

    #[test]
    fn test_app_add_message() {
        let mut app = EmailApp::new();
        let acct = EmailAccount::gmail("test@gmail.com", "Test");
        let acct_id = app.add_account(acct);
        let msgs = create_sample_messages(acct_id);
        for msg in msgs {
            app.add_message(msg);
        }
        assert_eq!(app.messages.len(), 5);
        assert!(app.unread_count > 0);
    }

    #[test]
    fn test_mark_read_unread() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        assert!(!app.messages[0].flags.seen);
        app.mark_read(id);
        assert!(app.messages[0].flags.seen);
        app.mark_unread(id);
        assert!(!app.messages[0].flags.seen);
    }

    #[test]
    fn test_toggle_flagged() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        assert!(!app.messages[0].flags.flagged);
        app.toggle_flagged(id);
        assert!(app.messages[0].flags.flagged);
    }

    #[test]
    fn test_move_message() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        assert_eq!(app.messages[0].mailbox, "Inbox");
        app.move_message(id, "Archive");
        assert_eq!(app.messages[0].mailbox, "Archive");
    }

    #[test]
    fn test_delete_to_trash() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        app.delete_message(id);
        assert_eq!(app.messages[0].mailbox, "Trash");
    }

    #[test]
    fn test_compose_reply() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        app.compose_reply(id);
        assert!(app.compose_draft.is_some());
        let draft = app.compose_draft.as_ref().unwrap();
        assert!(draft.subject.starts_with("Re: "));
    }

    #[test]
    fn test_compose_forward() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        app.compose_forward(id);
        assert!(app.compose_draft.is_some());
        let draft = app.compose_draft.as_ref().unwrap();
        assert!(draft.subject.starts_with("Fwd: "));
    }

    #[test]
    fn test_build_message() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.subject = "Test Subject".to_string();
        draft.body = "Hello".to_string();
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from);
        assert!(msg.contains("From: alice@example.com"));
        assert!(msg.contains("To: bob@example.com"));
        assert!(msg.contains("Subject: Test Subject"));
        assert!(msg.contains("Hello"));
    }

    #[test]
    fn test_build_message_with_attachment() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.subject = "With File".to_string();
        draft.body = "See attached.".to_string();
        draft.attachments.push(Attachment::new("test.txt", "text/plain", b"file content".to_vec()));
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from);
        assert!(msg.contains("multipart/mixed"));
        assert!(msg.contains("test.txt"));
    }

    // Filter tests
    #[test]
    fn test_filter_from_contains() {
        let cond = FilterCondition::FromContains("alice".to_string());
        let msg = &create_sample_messages(1)[0];
        assert!(cond.matches(msg));
    }

    #[test]
    fn test_filter_subject_contains() {
        let cond = FilterCondition::SubjectContains("Meeting".to_string());
        let msg = &create_sample_messages(1)[0];
        assert!(cond.matches(msg));
    }

    #[test]
    fn test_filter_rule_match() {
        let rule = FilterRule {
            id: 1,
            name: "Test".to_string(),
            enabled: true,
            conditions: vec![FilterCondition::SubjectContains("Meeting".to_string())],
            match_all: false,
            actions: vec![FilterAction::MarkAsRead],
            stop_processing: false,
        };
        let msg = &create_sample_messages(1)[0];
        assert!(rule.matches(msg));
    }

    #[test]
    fn test_filter_rule_disabled() {
        let rule = FilterRule {
            id: 1,
            name: "Disabled".to_string(),
            enabled: false,
            conditions: vec![FilterCondition::SubjectContains("Meeting".to_string())],
            match_all: false,
            actions: Vec::new(),
            stop_processing: false,
        };
        let msg = &create_sample_messages(1)[0];
        assert!(!rule.matches(msg));
    }

    #[test]
    fn test_apply_filter() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        app.add_filter_rule(FilterRule {
            id: 0,
            name: "Flag meetings".to_string(),
            enabled: true,
            conditions: vec![FilterCondition::SubjectContains("Meeting".to_string())],
            match_all: false,
            actions: vec![FilterAction::MarkAsFlagged],
            stop_processing: false,
        });
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        let applied = app.apply_filters(id);
        assert_eq!(applied.len(), 1);
        assert!(app.messages[0].flags.flagged);
    }

    #[test]
    fn test_search_messages() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        for msg in create_sample_messages(acct_id) {
            app.add_message(msg);
        }
        app.search_query = "meeting".to_string();
        let results = app.current_messages();
        assert_eq!(results.len(), 1);
        assert!(results[0].subject.contains("Meeting"));
    }

    #[test]
    fn test_smtp_reply_class() {
        assert_eq!(smtp_reply_class(250), "Positive Completion");
        assert_eq!(smtp_reply_class(354), "Positive Intermediate");
        assert_eq!(smtp_reply_class(550), "Permanent Negative");
    }

    #[test]
    fn test_imap_status() {
        assert_eq!(ImapStatus::from_str("OK"), Some(ImapStatus::Ok));
        assert_eq!(ImapStatus::from_str("BAD"), Some(ImapStatus::Bad));
        assert_eq!(ImapStatus::from_str("NOPE"), None);
    }

    #[test]
    fn test_render_produces_commands() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        for msg in create_sample_messages(acct_id) {
            app.add_message(msg);
        }
        let cmds = app.render(1400.0, 900.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_content_disposition() {
        let cd = ContentDisposition::parse("attachment; filename=\"report.pdf\"");
        assert!(cd.is_attachment());
        assert_eq!(cd.filename.as_deref(), Some("report.pdf"));
    }

    #[test]
    fn test_parse_address_list() {
        let addrs = parse_address_list("alice@test.com, Bob <bob@test.com>");
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0].local_part, "alice");
        assert_eq!(addrs[1].display_name.as_deref(), Some("Bob"));
    }
}
