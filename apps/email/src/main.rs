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
//!
//! **This client cannot send or receive mail, and the window says so.** It has
//! no network access, so no account is connected and no server has been
//! contacted -- and an empty folder here is not "no new mail", which is a
//! claim about the world it has not earned. The IMAP and SMTP builders above
//! wait on a socket.
//!
//! **What it does is read and write mail kept in files.** `~/Mail`'s mbox
//! files and folders of `.eml` messages are its folders, and any message or
//! mbox can be opened with Ctrl+O (`store`). Messages are read through
//! `decode`: the charset each part names, encoded words in headers, RFC 2231
//! names, HTML read as text, mbox quoting. A message is shown whole and its
//! attachments saved where the dialog says. A message is written in a drawn
//! form -- From, To, Cc, Subject and a many-line body -- saved as a draft in
//! `~/Mail/Drafts` or as an `.eml` file anywhere, and "sent" only as far as
//! checking it and saying it cannot go.
//!
//! **It never writes mail it did not make.** Read and flagged marks are kept
//! in a file of its own under the configuration directory, and deleting mail
//! read from files is refused; its own drafts are its to replace and delete.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use std::collections::BTreeMap;
use std::fmt;

mod decode;
mod store;

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
    #[must_use]
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();

        // Try "Display Name <addr>" format
        if let Some(angle_start) = trimmed.rfind('<')
            && let Some(angle_end) = trimmed.rfind('>')
            && angle_end > angle_start
        {
            let display = trimmed.get(..angle_start)?.trim();
            let display_name = if display.is_empty() {
                None
            } else {
                // Strip surrounding quotes if present
                let d = display.trim_matches('"').trim();
                if d.is_empty() {
                    None
                } else {
                    Some(d.to_string())
                }
            };
            let addr = trimmed
                .get(angle_start.saturating_add(1)..angle_end)?
                .trim();
            let (local, domain) = Self::split_addr(addr)?;
            return Some(Self {
                display_name,
                local_part: local,
                domain,
            });
        }

        // Plain "user@domain" format
        let (local, domain) = Self::split_addr(trimmed)?;
        Some(Self {
            display_name: None,
            local_part: local,
            domain,
        })
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
    #[must_use]
    pub fn address(&self) -> String {
        format!("{}@{}", self.local_part, self.domain)
    }

    /// Render as an RFC 5322 mailbox, safe to write into a header.
    ///
    /// Separate from [`fmt::Display`] on purpose: `Display` is what the UI
    /// shows, where a name is drawn into a text run and any character in it is
    /// harmless. A header is a grammar, so the display name is quoted when it
    /// needs quoting and control characters are folded out — one rendering per
    /// destination, rather than one rendering trusted to suit both.
    #[must_use]
    pub fn to_header(&self) -> String {
        let addr = format!(
            "{}@{}",
            header_text(&self.local_part).replace(['<', '>', ' '], ""),
            header_text(&self.domain).replace(['<', '>', ' '], ""),
        );
        match self.display_name {
            Some(ref name) => format!("{} <{addr}>", display_phrase(name)),
            None => addr,
        }
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
    #[must_use]
    pub fn parse(value: &str) -> Self {
        let mut parts = value.splitn(2, ';');
        let type_part = parts.next().unwrap_or("text/plain").trim();
        let (media_type, subtype) = if let Some(slash) = type_part.find('/') {
            (
                type_part.get(..slash).unwrap_or("text").to_lowercase(),
                type_part
                    .get(slash.saturating_add(1)..)
                    .unwrap_or("plain")
                    .to_lowercase(),
            )
        } else {
            ("text".to_string(), "plain".to_string())
        };

        // Split at semicolons outside quotes, with RFC 2231's `name*=` forms
        // put together: a split at every `;` cut `name="a;b.pdf"` in two.
        let params = parts.next().map(decode::params).unwrap_or_default();

        Self {
            media_type,
            subtype,
            params,
        }
    }

    /// Full MIME type string
    #[must_use]
    pub fn mime_type(&self) -> String {
        format!("{}/{}", self.media_type, self.subtype)
    }

    /// Get charset parameter
    #[must_use]
    pub fn charset(&self) -> &str {
        self.params
            .get("charset")
            .map_or("us-ascii", |s| s.as_str())
    }

    /// Get boundary parameter for multipart messages
    #[must_use]
    pub fn boundary(&self) -> Option<&str> {
        self.params.get("boundary").map(std::string::String::as_str)
    }

    /// Check if this is a multipart type
    #[must_use]
    pub fn is_multipart(&self) -> bool {
        self.media_type == "multipart"
    }

    /// Check if text type
    #[must_use]
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
    #[must_use]
    pub fn parse(value: &str) -> Self {
        let mut parts = value.splitn(2, ';');
        let disposition = parts.next().unwrap_or("inline").trim().to_lowercase();
        // `filename*=UTF-8''...` is how a name that is not ASCII arrives.
        let filename = parts
            .next()
            .map(decode::params)
            .and_then(|mut p| p.remove("filename"));
        Self {
            disposition,
            filename,
        }
    }

    #[must_use]
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, value: &str) {
        self.headers.push((name.to_string(), value.to_string()));
    }

    /// Get first header value by name (case-insensitive)
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    /// Get all header values for a name
    #[must_use]
    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let lower = name.to_lowercase();
        self.headers
            .iter()
            .filter(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// Parse headers from raw header block
    #[must_use]
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
                current_value = line
                    .get(colon.saturating_add(1)..)
                    .unwrap_or("")
                    .trim()
                    .to_string();
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
    pub parts: Vec<MimePart>, // Sub-parts for multipart
}

impl MimePart {
    /// Check if this is an attachment
    #[must_use]
    pub fn is_attachment(&self) -> bool {
        self.disposition
            .as_ref()
            .is_some_and(ContentDisposition::is_attachment)
    }

    /// Get filename for attachment
    #[must_use]
    pub fn filename(&self) -> Option<&str> {
        self.disposition
            .as_ref()
            .and_then(|d| d.filename.as_deref())
            .or_else(|| {
                self.content_type
                    .params
                    .get("name")
                    .map(std::string::String::as_str)
            })
    }

    /// Get body as text (if text/* content type), in the charset the part
    /// names. It read only UTF-8, so a Latin-1 message had no text at all.
    #[must_use]
    pub fn body_text(&self) -> Option<String> {
        self.text().map(|d| d.text)
    }

    /// The part's text and what was assumed to read it, for a text part.
    #[must_use]
    pub fn text(&self) -> Option<decode::Decoded> {
        self.content_type
            .is_text()
            .then(|| decode::decode_charset(&self.body, self.content_type.charset()))
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
            (
                raw.get(..sep).unwrap_or(""),
                raw.get(sep.saturating_add(4)..).unwrap_or(""),
            )
        } else if let Some(sep) = raw.find("\n\n") {
            (
                raw.get(..sep).unwrap_or(""),
                raw.get(sep.saturating_add(2)..).unwrap_or(""),
            )
        } else {
            (raw, "")
        };

        let headers = EmailHeaders::parse(header_text);

        let from = headers
            .get("From")
            .and_then(EmailAddress::parse)
            .map(decoded_name);
        let to = parse_address_list(headers.get("To").unwrap_or(""));
        let cc = parse_address_list(headers.get("Cc").unwrap_or(""));
        let bcc = parse_address_list(headers.get("Bcc").unwrap_or(""));
        let reply_to = headers
            .get("Reply-To")
            .and_then(EmailAddress::parse)
            .map(decoded_name);
        let subject = headers
            .get("Subject")
            .map_or_else(|| String::from("(no subject)"), decode::header_words);
        let date = headers.get("Date").map(String::from);
        let message_id = headers
            .get("Message-ID")
            .or_else(|| headers.get("Message-Id"))
            .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string());
        let in_reply_to = headers
            .get("In-Reply-To")
            .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string());
        let references: Vec<String> = headers
            .get("References")
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

    /// A message from its bytes as stored.
    ///
    /// UTF-8 when they are; otherwise one byte one character (ISO 8859-1),
    /// which is exact both ways, and each part that was not transfer-encoded
    /// gets its bytes back from it unchanged, to be read in the charset it
    /// names. `parse` takes text, and a message with a Latin-1 body in 8-bit
    /// was not text it could be given.
    ///
    /// # Errors
    ///
    /// As [`Self::parse`].
    pub fn parse_bytes(raw: &[u8]) -> Result<Self, String> {
        if let Ok(text) = std::str::from_utf8(raw) {
            return Self::parse(text);
        }
        let text: String = raw.iter().map(|&b| char::from(b)).collect();
        let mut message = Self::parse(&text)?;
        restore_bytes(&mut message.body);
        Ok(message)
    }

    /// What a reader shows as the message: the plain text part, or the HTML
    /// one read as text, or a line saying there is neither -- with a note
    /// when the text had to be read in a charset other than the one named.
    #[must_use]
    pub fn readable_body(&self) -> decode::Decoded {
        if let Some(plain) = find_part(&self.body, "plain").and_then(MimePart::text) {
            return plain;
        }
        if let Some(html) = find_part(&self.body, "html").and_then(MimePart::text) {
            return decode::Decoded {
                text: decode::html_to_text(&html.text),
                note: html.note,
            };
        }
        decode::Decoded {
            text: String::from("(This message has no text to show.)"),
            note: None,
        }
    }

    /// Get plain text body
    #[must_use]
    pub fn plain_text(&self) -> Option<String> {
        find_text_part(&self.body, "plain")
    }

    /// Get HTML body
    #[must_use]
    pub fn html_body(&self) -> Option<String> {
        find_text_part(&self.body, "html")
    }

    /// List attachments
    #[must_use]
    pub fn attachments(&self) -> Vec<&MimePart> {
        let mut result = Vec::new();
        collect_attachments(&self.body, &mut result);
        result
    }
}

/// Parse a comma-separated list of email addresses.
///
/// Split at the commas outside quotes and angle brackets: `"Doe, Jane"
/// <jane@example.com>` is one address, which a split at every comma made two
/// broken ones. Display names are decoded (`=?UTF-8?B?...?=`).
fn parse_address_list(s: &str) -> Vec<EmailAddress> {
    let mut out = Vec::new();
    let mut piece = String::new();
    let mut quoted = false;
    let mut angle = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' if !quoted && !angle => {
                out.extend(EmailAddress::parse(piece.trim()).map(decoded_name));
                piece.clear();
                continue;
            }
            _ => {}
        }
        piece.push(c);
    }
    out.extend(EmailAddress::parse(piece.trim()).map(decoded_name));
    out
}

/// An address with its display name's encoded words decoded.
fn decoded_name(address: EmailAddress) -> EmailAddress {
    EmailAddress {
        display_name: address.display_name.map(|d| decode::header_words(&d)),
        ..address
    }
}

/// Parse MIME body from text given content type
fn parse_mime_body(ct: &ContentType, body_text: &str, headers: &EmailHeaders) -> MimePart {
    let disposition = headers
        .get("Content-Disposition")
        .map(ContentDisposition::parse);

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
        let encoding = headers
            .get("Content-Transfer-Encoding")
            .unwrap_or("7bit")
            .to_lowercase();
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
                parts.push(parse_single_part(&current_part));
            }
            break;
        }
        if line.starts_with(&delimiter) {
            if in_part && !current_part.is_empty() {
                parts.push(parse_single_part(&current_part));
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

/// Parse a single MIME part.
//
// Always returns successfully — even an empty `text` yields a default-typed
// part. Returning `MimePart` directly (rather than `Option<MimePart>`) lets
// the two callers drop their `if let Some(...)` wrappers and matches
// `unnecessary_wraps`.
fn parse_single_part(text: &str) -> MimePart {
    let (header_text, body_text) = if let Some(sep) = text.find("\n\n") {
        (
            text.get(..sep).unwrap_or(""),
            text.get(sep.saturating_add(2)..).unwrap_or(""),
        )
    } else {
        ("", text)
    };

    let headers = EmailHeaders::parse(header_text);
    let ct = ContentType::parse(headers.get("Content-Type").unwrap_or("text/plain"));
    parse_mime_body(&ct, body_text, &headers)
}

/// The bytes of every part not transfer-encoded, recovered from a message
/// read one byte one character.
fn restore_bytes(part: &mut MimePart) {
    let encoding = part
        .headers
        .get("Content-Transfer-Encoding")
        .unwrap_or("7bit")
        .trim()
        .to_ascii_lowercase();
    if part.parts.is_empty()
        && encoding != "base64"
        && encoding != "quoted-printable"
        && let Ok(text) = std::str::from_utf8(&part.body)
    {
        part.body = text
            .chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .collect();
    }
    for sub in &mut part.parts {
        restore_bytes(sub);
    }
}

/// The first text part of `subtype` that is not an attachment.
fn find_part<'a>(part: &'a MimePart, subtype: &str) -> Option<&'a MimePart> {
    if part.parts.is_empty() {
        return (part.content_type.media_type == "text"
            && part.content_type.subtype == subtype
            && !part.is_attachment())
        .then_some(part);
    }
    part.parts.iter().find_map(|p| find_part(p, subtype))
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

/// Collect all attachment parts: those marked so, and any other part with a
/// name that is not the message's text -- older mailers name an attachment
/// only in its `Content-Type`, and give it no disposition at all.
fn collect_attachments<'a>(part: &'a MimePart, result: &mut Vec<&'a MimePart>) {
    let named_file = part.parts.is_empty()
        && part.filename().is_some()
        && !(part.content_type.media_type == "text"
            && matches!(part.content_type.subtype.as_str(), "plain" | "html")
            && part.disposition.is_none());
    if part.is_attachment() || named_file {
        result.push(part);
    }
    for sub in &part.parts {
        collect_attachments(sub, result);
    }
}

/// Decode base64 (`decode::base64`, which encoded words use too).
fn base64_decode(input: &str) -> Vec<u8> {
    decode::base64(input)
}

/// Encode to base64 (`decode::base64_encode`, which the attachments and the
/// encoded words of a message written here use too).
fn base64_encode(data: &[u8]) -> String {
    decode::base64_encode(data)
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
            ) && let (Some(hi), Some(lo)) = (hex_val(h), hex_val(l))
            {
                result.push(hi.wrapping_shl(4) | lo);
                i = i.saturating_add(3);
                continue;
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
    #[must_use]
    pub fn login(tag: &str, user: &str, pass: &str) -> String {
        format!("{tag} LOGIN {user} {pass}\r\n")
    }

    #[must_use]
    pub fn logout(tag: &str) -> String {
        format!("{tag} LOGOUT\r\n")
    }

    #[must_use]
    pub fn select(tag: &str, mailbox: &str) -> String {
        format!("{tag} SELECT \"{mailbox}\"\r\n")
    }

    #[must_use]
    pub fn examine(tag: &str, mailbox: &str) -> String {
        format!("{tag} EXAMINE \"{mailbox}\"\r\n")
    }

    #[must_use]
    pub fn list(tag: &str, reference: &str, pattern: &str) -> String {
        format!("{tag} LIST \"{reference}\" \"{pattern}\"\r\n")
    }

    #[must_use]
    pub fn fetch(tag: &str, sequence: &str, items: &str) -> String {
        format!("{tag} FETCH {sequence} ({items})\r\n")
    }

    #[must_use]
    pub fn search(tag: &str, criteria: &str) -> String {
        format!("{tag} SEARCH {criteria}\r\n")
    }

    #[must_use]
    pub fn store(tag: &str, sequence: &str, action: &str, flags: &str) -> String {
        format!("{tag} STORE {sequence} {action} ({flags})\r\n")
    }

    #[must_use]
    pub fn copy(tag: &str, sequence: &str, mailbox: &str) -> String {
        format!("{tag} COPY {sequence} \"{mailbox}\"\r\n")
    }

    #[must_use]
    pub fn r#move(tag: &str, sequence: &str, mailbox: &str) -> String {
        format!("{tag} MOVE {sequence} \"{mailbox}\"\r\n")
    }

    #[must_use]
    pub fn expunge(tag: &str) -> String {
        format!("{tag} EXPUNGE\r\n")
    }

    #[must_use]
    pub fn noop(tag: &str) -> String {
        format!("{tag} NOOP\r\n")
    }

    #[must_use]
    pub fn idle(tag: &str) -> String {
        format!("{tag} IDLE\r\n")
    }

    #[must_use]
    pub fn create(tag: &str, mailbox: &str) -> String {
        format!("{tag} CREATE \"{mailbox}\"\r\n")
    }

    #[must_use]
    pub fn delete(tag: &str, mailbox: &str) -> String {
        format!("{tag} DELETE \"{mailbox}\"\r\n")
    }

    #[must_use]
    pub fn rename(tag: &str, old: &str, new: &str) -> String {
        format!("{tag} RENAME \"{old}\" \"{new}\"\r\n")
    }

    #[must_use]
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
    /// Parse an IMAP status word ("OK", "NO", "BAD", "BYE") into a status.
    /// Named `from_code` rather than `from_str` because it returns `Option`
    /// (not `Result`) and so isn't the `std::str::FromStr` contract.
    #[must_use]
    pub fn from_code(s: &str) -> Option<Self> {
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
    #[must_use]
    pub fn ehlo(hostname: &str) -> String {
        format!("EHLO {hostname}\r\n")
    }

    #[must_use]
    pub fn helo(hostname: &str) -> String {
        format!("HELO {hostname}\r\n")
    }

    #[must_use]
    pub fn auth_login() -> String {
        "AUTH LOGIN\r\n".to_string()
    }

    #[must_use]
    pub fn auth_plain(user: &str, pass: &str) -> String {
        let credentials = format!("\0{user}\0{pass}");
        let encoded = base64_encode(credentials.as_bytes());
        format!("AUTH PLAIN {encoded}\r\n")
    }

    #[must_use]
    pub fn mail_from(addr: &str) -> String {
        format!("MAIL FROM:<{addr}>\r\n")
    }

    #[must_use]
    pub fn rcpt_to(addr: &str) -> String {
        format!("RCPT TO:<{addr}>\r\n")
    }

    #[must_use]
    pub fn data() -> String {
        "DATA\r\n".to_string()
    }

    #[must_use]
    pub fn quit() -> String {
        "QUIT\r\n".to_string()
    }

    #[must_use]
    pub fn rset() -> String {
        "RSET\r\n".to_string()
    }

    #[must_use]
    pub fn starttls() -> String {
        "STARTTLS\r\n".to_string()
    }

    #[must_use]
    pub fn noop() -> String {
        "NOOP\r\n".to_string()
    }
}

/// SMTP reply code ranges
#[must_use]
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
    #[must_use]
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
    #[must_use]
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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MessageFlags {
    pub seen: bool,
    pub answered: bool,
    pub flagged: bool,
    pub deleted: bool,
    pub draft: bool,
    pub has_attachment: bool,
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

// ─── Header safety ───────────────────────────────────────────────────
//
// Everything an outgoing message writes into a header goes through one of the
// four helpers below. They exist because RFC 5322 gives a header field no way
// to contain a line break: the field *ends* at CRLF, and folding -- a CRLF
// followed by whitespace -- is a continuation the serialiser chooses, not
// something a value can ask for. So a CR or LF inside a value does not get
// escaped; it terminates the header, and the receiving MTA reads whatever
// follows as a header of its own.
//
// That is header injection, and its worst form here is silent: a subject
// carrying "\r\nBcc: someone@example.com" adds a recipient that appears
// nowhere in the sender's compose window or their Sent copy. The message goes
// where the user cannot see it went.
//
// The inbound parser is *not* the hole -- `Headers::parse` unfolds
// continuations into spaces, so no value read off the wire can carry a CR or
// LF, and a hostile Message-ID cannot ride into a reply's In-Reply-To. The
// reachable inputs are the locally composed ones: the subject and recipients
// the user types or pastes, and attachment filenames, which on SlateOS are a
// real concern rather than a theoretical one -- the design spec allows every
// byte except '/' and NUL in a path, so a newline in a filename is legal here
// even where other systems forbid it.
//
// Since the format cannot represent the character at all, each field is either
// sanitised or rejected, per the rule this audit keeps arriving at. Advisory
// prose (subject, display names) is sanitised. Recipients are load-bearing --
// quietly rewriting one could deliver the mail somewhere the user did not
// intend -- so they are rejected and reported instead.

/// Fold a value so it is safe to write as one header line.
///
/// Every control character becomes a space: this is for the advisory,
/// human-readable fields, where losing exotic whitespace is a fair price and
/// there is nothing else the grammar permits.
fn header_text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Whether an address can be written into a recipient header unchanged.
///
/// Deliberately a predicate rather than a sanitiser. A recipient decides where
/// the message goes, so an address the serialiser cannot write verbatim is
/// reported to the caller rather than repaired into some other address.
fn is_writable_address(s: &str) -> bool {
    !s.trim().is_empty() && !s.chars().any(char::is_control)
}

/// Sanitise a value written inside angle brackets (`Message-ID`, `Content-ID`).
///
/// Angle brackets delimit the value and have no escape, so they are dropped
/// along with control characters and whitespace.
fn angle_token(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ' ' | '\t'))
        .collect()
}

/// Render a value as an RFC 2045 quoted-string.
///
/// Unlike a header body this *does* have an escape sequence — a quoted-string
/// may contain `\"` and `\\` — so a quote in the value is escaped rather than
/// dropped. Without that, a filename containing a quote closes the parameter
/// early and the remainder of the name is read as further parameters.
/// Control characters still have no representation and are dropped.
fn quoted_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a display name as an RFC 5322 phrase, quoting it when it needs it.
fn display_phrase(s: &str) -> String {
    let cleaned = header_text(s);
    let is_atext = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                ' ' | '!'
                    | '#'
                    | '$'
                    | '%'
                    | '&'
                    | '\''
                    | '*'
                    | '+'
                    | '-'
                    | '/'
                    | '='
                    | '?'
                    | '^'
                    | '_'
                    | '`'
                    | '{'
                    | '|'
                    | '}'
                    | '~'
            )
    };
    if cleaned.chars().all(is_atext) {
        cleaned
    } else if !cleaned.is_ascii() {
        // A phrase may be encoded words; it may not be raw UTF-8, which a
        // reader that is not RFC 6532's would show as mojibake.
        decode::header_word(&cleaned)
    } else {
        quoted_string(&cleaned)
    }
}

/// Validate a media type, falling back to the generic binary type.
///
/// A `Content-Type` value is a token, not a quoted string — it has no escaping
/// whatsoever, so a recorded type containing a quote, a space or a semicolon
/// rewrites the rest of the header. There is nothing to sanitise it *into*: a
/// mangled media type is not a media type. `application/octet-stream` is what a
/// receiver is supposed to assume for content it cannot identify, which is
/// exactly the situation.
fn media_type_or_default(s: &str) -> &str {
    fn is_token(part: &str) -> bool {
        !part.is_empty()
            && part.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(
                        c,
                        '!' | '#'
                            | '$'
                            | '%'
                            | '&'
                            | '\''
                            | '*'
                            | '+'
                            | '-'
                            | '.'
                            | '^'
                            | '_'
                            | '`'
                            | '|'
                            | '~'
                    )
            })
    }
    match s.split_once('/') {
        Some((ty, sub)) if is_token(ty) && is_token(sub) => s,
        _ => "application/octet-stream",
    }
}

/// Choose a multipart boundary that does not occur in the content it delimits.
///
/// RFC 2046 requires the boundary to appear nowhere inside an encapsulated
/// part, and a hardcoded one cannot promise that. It is a correctness bug
/// rather than a stylistic one: a body containing the delimiter — which a user
/// produces just by quoting a previous multipart mail — ends the part there,
/// and every attachment after that point silently disappears from the message.
///
/// Lengthening the candidate on collision terminates, because a finite string
/// cannot contain an arbitrarily long substring. The first candidate is the
/// boundary that was previously hardcoded, so ordinary mail is unchanged.
fn choose_boundary(content: &str) -> String {
    let mut boundary = String::from("----=_Part_Boundary_001");
    while content.contains(&boundary) {
        boundary.push('x');
    }
    boundary
}

/// A serialised message, plus anything the serialiser refused to write.
///
/// The rejected list is part of the return type rather than a silent omission
/// for the same reason `M3uExport` reports skipped tracks: a recipient that was
/// dropped is precisely the thing a sender must be told about, and a function
/// returning a bare `String` has nowhere to say it.
#[derive(Debug, Clone, Default)]
pub struct BuiltMessage {
    /// The RFC 5322 message text.
    pub text: String,
    /// Addresses omitted because they could not be written as a header value.
    pub rejected_recipients: Vec<String>,
}

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
    #[must_use]
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
    #[must_use]
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
    #[must_use]
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
    #[must_use]
    pub fn reply(msg: &MessageSummary, body_quote: &str, account_id: u32) -> Self {
        let subject = if msg.subject.starts_with("Re: ") {
            msg.subject.clone()
        } else {
            format!("Re: {}", msg.subject)
        };

        let quoted = body_quote
            .lines()
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
    #[must_use]
    pub fn forward(msg: &MessageSummary, body_text: &str, account_id: u32) -> Self {
        let subject = if msg.subject.starts_with("Fwd: ") {
            msg.subject.clone()
        } else {
            format!("Fwd: {}", msg.subject)
        };

        let body = format!(
            "\n\n---------- Forwarded message ----------\nFrom: {}\nDate: {}\nSubject: {}\nTo: {}\n\n{body_text}",
            msg.from,
            msg.date,
            msg.subject,
            msg.to
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
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

    /// Build an RFC 5322 message, reporting any recipient it could not write.
    ///
    /// Every interpolated value passes through the header-safety helpers above;
    /// see the note there for why a raw one is a header-injection bug rather
    /// than a formatting blemish.
    #[must_use]
    pub fn build_message(&self, from: &EmailAddress) -> BuiltMessage {
        self.build_message_at(Some(from), None)
    }

    /// [`Self::build_message`], with the sender optional -- a draft may not
    /// have one yet -- and a `Date:` header when `date` is given.
    #[must_use]
    pub fn build_message_at(
        &self,
        from: Option<&EmailAddress>,
        date: Option<&str>,
    ) -> BuiltMessage {
        let mut msg = String::new();
        let mut rejected: Vec<String> = Vec::new();

        // Recipients are load-bearing: partition rather than sanitise, so a
        // bad address is reported instead of being turned into a different
        // one.
        let accept = |addrs: &[String], rejected: &mut Vec<String>| -> Vec<String> {
            addrs
                .iter()
                .filter(|a| {
                    if is_writable_address(a) {
                        true
                    } else {
                        rejected.push((*a).clone());
                        false
                    }
                })
                .cloned()
                .collect()
        };
        let to = accept(&self.to, &mut rejected);
        let cc = accept(&self.cc, &mut rejected);
        let bcc = accept(&self.bcc, &mut rejected);

        if let Some(from) = from {
            msg.push_str(&format!("From: {}\r\n", from.to_header()));
        }
        if let Some(date) = date {
            msg.push_str(&format!("Date: {}\r\n", header_text(date)));
        }
        if !to.is_empty() {
            msg.push_str(&format!("To: {}\r\n", to.join(", ")));
        }
        if !cc.is_empty() {
            msg.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
        }
        if !bcc.is_empty() {
            msg.push_str(&format!("Bcc: {}\r\n", bcc.join(", ")));
        }
        msg.push_str(&format!(
            "Subject: {}\r\n",
            decode::header_word(&header_text(&self.subject))
        ));
        msg.push_str("MIME-Version: 1.0\r\n");

        // These come from the message being replied to, so they are the one
        // header input with a remote origin. `Headers::parse` unfolds
        // continuations and so cannot hand us a CR or LF, but the angle
        // brackets it does *not* strip would still let a Message-ID close the
        // addr-spec early, so sanitise here rather than rely on that.
        if let Some(ref reply_to) = self.in_reply_to {
            let id = angle_token(reply_to);
            if !id.is_empty() {
                msg.push_str(&format!("In-Reply-To: <{id}>\r\n"));
            }
        }
        let refs: Vec<String> = self
            .references
            .iter()
            .map(|r| angle_token(r))
            .filter(|r| !r.is_empty())
            .map(|r| format!("<{r}>"))
            .collect();
        if !refs.is_empty() {
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
            let ct = if self.is_html {
                "text/html"
            } else {
                "text/plain"
            };
            msg.push_str(&format!("Content-Type: {ct}; charset=utf-8\r\n"));
            msg.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
            msg.push_str("\r\n");
            msg.push_str(&decode::quoted_printable(&full_body));
        } else {
            // Multipart mixed. The boundary is derived from the body rather
            // than fixed, so a body that happens to quote this delimiter
            // cannot truncate the message and drop the attachments below it.
            let boundary = choose_boundary(&full_body);
            msg.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n"
            ));
            msg.push_str("\r\n");

            // Text part
            msg.push_str(&format!("--{boundary}\r\n"));
            let ct = if self.is_html {
                "text/html"
            } else {
                "text/plain"
            };
            msg.push_str(&format!("Content-Type: {ct}; charset=utf-8\r\n"));
            msg.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
            msg.push_str("\r\n");
            msg.push_str(&decode::quoted_printable(&full_body));
            msg.push_str("\r\n");

            // Attachments
            for att in &self.attachments {
                msg.push_str(&format!("--{boundary}\r\n"));
                let disp = if att.is_inline {
                    "inline"
                } else {
                    "attachment"
                };
                // Control characters out first -- a name is a header value --
                // then RFC 2231 for a name that is not ASCII.
                let cleaned = header_text(&att.filename);
                msg.push_str(&format!(
                    "Content-Type: {}; {}\r\n",
                    media_type_or_default(&att.mime_type),
                    decode::param("name", &cleaned),
                ));
                msg.push_str(&format!(
                    "Content-Disposition: {disp}; {}\r\n",
                    decode::param("filename", &cleaned)
                ));
                msg.push_str("Content-Transfer-Encoding: base64\r\n");
                if let Some(ref cid) = att.content_id {
                    let cid = angle_token(cid);
                    if !cid.is_empty() {
                        msg.push_str(&format!("Content-ID: <{cid}>\r\n"));
                    }
                }
                msg.push_str("\r\n");
                // Wrap base64 at 76 chars per line
                let encoded = decode::base64_encode(&att.data);
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

        BuiltMessage {
            text: msg,
            rejected_recipients: rejected,
        }
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
    #[must_use]
    pub fn matches(&self, msg: &MessageSummary) -> bool {
        match self {
            Self::FromContains(s) => msg
                .from
                .to_string()
                .to_lowercase()
                .contains(&s.to_lowercase()),
            Self::ToContains(s) => msg
                .to
                .iter()
                .any(|a| a.to_string().to_lowercase().contains(&s.to_lowercase())),
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
    #[must_use]
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
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::{Frame, Rect};
use guitk::render::RenderTree;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use guitk::textedit;
use guitk::textinput::TextInput;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textarea::TextArea;

/// What the window says above the mail, before anything is pressed.
///
/// Two lines. The second exists because an empty inbox is a *believable*
/// empty inbox -- it reads as "no new mail", which is a fact about the
/// user's correspondence rather than about this program -- and because the
/// user should learn from the window, not the manual, where mail it can read
/// comes from.
const CANNOT_FETCH_LINES: [&str; 2] = [
    "This client cannot send or receive mail: it has no network, so no server has ever been contacted.",
    "An empty folder here is not \u{201C}no new mail\u{201D}. It reads mail kept in files -- mbox files and folders of .eml messages in ~/Mail, or any opened with Ctrl+O -- and saves what you write as .eml files.",
];

/// The window size to ask for.
///
/// A request, not a promise: `render` is handed the size it is actually being
/// drawn at and lays out from that.
const WINDOW_WIDTH: f32 = 1400.0;
/// As [`WINDOW_WIDTH`].
const WINDOW_HEIGHT: f32 = 900.0;

const HEADER_H: f32 = 48.0;
const NOTICE_H: f32 = 40.0;
const TOOLBAR_H: f32 = 40.0;
const SIDEBAR_W: f32 = 210.0;
const STATUS_H: f32 = 24.0;
const ROW_H: f32 = 64.0;
const FOLDER_ROW_H: f32 = 30.0;
const BODY_LINE_H: f32 = 18.0;
const BODY_TEXT: f32 = 13.0;
const FIELD_H: f32 = 28.0;

/// The largest file attached to a message written here.
const MAX_ATTACHMENT_BYTES: usize = 25 << 20;
/// The longest a single-line field (an address list, a subject) may be.
const FIELD_CAPACITY: usize = 2_000;
/// The longest a message body may be, in characters.
const BODY_CAPACITY: usize = 1_000_000;

/// Active UI panel
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    MessageList,
    Reading,
    Compose,
    Settings,
}

/// Stepping and naming for [`SortOrder`].
impl SortOrder {
    /// Every order, in the order `Ctrl+S` steps through them.
    ///
    /// The same order the comparator is written in, so the two cannot drift.
    pub const ALL: [Self; 5] = [
        Self::DateDesc,
        Self::DateAsc,
        Self::SenderAsc,
        Self::SubjectAsc,
        Self::SizeDesc,
    ];

    /// What the window calls this order.
    pub fn label(self) -> &'static str {
        match self {
            Self::DateDesc => "newest first",
            Self::DateAsc => "oldest first",
            Self::SenderAsc => "sender",
            Self::SubjectAsc => "subject",
            Self::SizeDesc => "largest first",
        }
    }

    /// The next order round the ring.
    #[must_use]
    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|o| *o == self).unwrap_or(0);
        let ahead = at.saturating_add(1);
        let wrapped = if ahead >= Self::ALL.len() { 0 } else { ahead };
        Self::ALL.get(wrapped).copied().unwrap_or(Self::DateDesc)
    }
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

/// The keys this program answers, raised by `F1`.
///
/// `?` is not a second way in: the search box and the compose form both take
/// typed text, so a `?` has somewhere to go -- the `apps/spreadsheet` case in
/// design-decisions 863.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Up / Down", "Move through the messages"),
    ("Left / Right", "Previous / next folder"),
    ("Enter", "Read the message (a draft: go on writing it)"),
    ("Page Up / Page Down", "Scroll the message being read"),
    ("R", "Reply to this message"),
    ("F", "Forward it"),
    ("U", "Mark it unread"),
    ("S", "Flag it"),
    ("Delete", "Delete a draft (mail read from files is kept)"),
    ("Ctrl+N", "Compose"),
    ("Ctrl+O", "Open a message or an mbox file"),
    ("Ctrl+F", "Search"),
    ("Ctrl+S", "Change the sort order"),
    ("Ctrl+P", "Move the reading pane"),
    ("F5", "Look in the mail folder again"),
    ("F1", "This list"),
];

/// The keys of the compose form, listed on the form itself.
const COMPOSE_KEYS: &str = "Tab: next field  \u{00B7}  Ctrl+S: save draft  \u{00B7}  Ctrl+Enter: send  \u{00B7}  Esc: close";

/// A field of the compose form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposeField {
    /// Who it is from: a name and an address, as they will be written.
    From,
    /// The recipient list.
    #[default]
    To,
    /// The copied recipients.
    Cc,
    /// The subject line.
    Subject,
    /// The message itself.
    Body,
}

impl ComposeField {
    /// The order Tab walks the fields in: the recipients first, as a new
    /// message is written, and the sender last, since it is usually set.
    const ORDER: [Self; 5] = [Self::To, Self::Cc, Self::Subject, Self::Body, Self::From];

    fn step(self, back: bool) -> Self {
        let at = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        let len = Self::ORDER.len();
        let next = if back {
            at.checked_sub(1).unwrap_or(len.saturating_sub(1))
        } else {
            at.saturating_add(1).checked_rem(len).unwrap_or(0)
        };
        Self::ORDER.get(next).copied().unwrap_or(Self::To)
    }

    fn label(self) -> &'static str {
        match self {
            Self::From => "From",
            Self::To => "To",
            Self::Cc => "Cc",
            Self::Subject => "Subject",
            Self::Body => "Message",
        }
    }
}

/// Stepping for [`ReadingPanePosition`].
impl ReadingPanePosition {
    /// Every position, in the order `Ctrl+P` steps through them.
    pub const ALL: [Self; 3] = [Self::Right, Self::Bottom, Self::Off];

    /// The next position round the ring.
    #[must_use]
    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        let ahead = at.saturating_add(1);
        let wrapped = if ahead >= Self::ALL.len() { 0 } else { ahead };
        Self::ALL.get(wrapped).copied().unwrap_or(Self::Right)
    }

    /// What the toolbar calls it.
    fn label(self) -> &'static str {
        match self {
            Self::Right => "Reading pane: right",
            Self::Bottom => "Reading pane: below",
            Self::Off => "Reading pane: off",
        }
    }
}

/// Reading pane position
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReadingPanePosition {
    Right,
    Bottom,
    Off,
}

/// A message being written.
///
/// Its fields are text fields -- a caret, a selection, a clipboard -- and
/// its body is a field of many lines; `EmailDraft`, which the message builder
/// takes, is made from it when the message is saved or checked.
pub struct Compose {
    pub from: TextInput,
    pub to: TextInput,
    pub cc: TextInput,
    pub subject: TextInput,
    pub body: TextArea,
    pub attachments: Vec<Attachment>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// The field the keys type into.
    pub field: ComposeField,
    /// The draft file this was saved to, or opened from, which saving again
    /// replaces. `None` until the first save.
    pub saved_as: Option<PathBuf>,
    /// The form as last saved, to tell whether closing it loses anything.
    saved: String,
    /// Escape was pressed once over unsaved work: a second press discards.
    pub confirm_close: bool,
    /// How far the body is scrolled, in lines, and across, in pixels.
    pub body_scroll: usize,
    pub body_hscroll: f32,
}

/// A text field holding `text`, with the caret at its end.
fn field_with(text: &str) -> TextInput {
    let mut input = TextInput::new();
    input.set_text(text);
    input
}

/// Split an address list at the commas outside quotes and angle brackets.
fn split_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut piece = String::new();
    let mut quoted = false;
    let mut angle = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                if !piece.trim().is_empty() {
                    out.push(piece.trim().to_owned());
                }
                piece.clear();
                continue;
            }
            _ => {}
        }
        piece.push(c);
    }
    if !piece.trim().is_empty() {
        out.push(piece.trim().to_owned());
    }
    out
}

impl Compose {
    /// An empty message from `from`.
    fn new(from: &str) -> Self {
        let mut compose = Self {
            from: field_with(from),
            to: TextInput::new(),
            cc: TextInput::new(),
            subject: TextInput::new(),
            body: TextArea::new(BODY_TEXT),
            attachments: Vec::new(),
            in_reply_to: None,
            references: Vec::new(),
            field: ComposeField::To,
            saved_as: None,
            saved: String::new(),
            confirm_close: false,
            body_scroll: 0,
            body_hscroll: 0.0,
        };
        compose.saved = compose.fingerprint();
        compose
    }

    /// A message from a draft `EmailDraft` -- a reply or a forward -- with
    /// the caret where writing starts: the top of the body.
    fn from_draft(draft: &EmailDraft, from: &str) -> Self {
        let mut compose = Self::new(from);
        compose.to = field_with(&draft.to.join(", "));
        compose.cc = field_with(&draft.cc.join(", "));
        compose.subject = field_with(&draft.subject);
        compose.body.set_text(&draft.body);
        compose.body.move_to(0, false);
        compose.in_reply_to.clone_from(&draft.in_reply_to);
        compose.references.clone_from(&draft.references);
        compose.attachments.clone_from(&draft.attachments);
        compose.field = if draft.to.is_empty() {
            ComposeField::To
        } else {
            ComposeField::Body
        };
        compose.saved = compose.fingerprint();
        compose
    }

    /// A saved draft, opened to go on writing it.
    fn from_saved(stored: &store::Stored) -> Self {
        let message = &stored.message;
        let mut compose = Self::new(
            &message
                .from
                .as_ref()
                .map(EmailAddress::to_string)
                .unwrap_or_default(),
        );
        let list = |addrs: &[EmailAddress]| {
            addrs
                .iter()
                .map(EmailAddress::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        compose.to = field_with(&list(&message.to));
        compose.cc = field_with(&list(&message.cc));
        compose.subject = field_with(if message.subject == "(no subject)" {
            ""
        } else {
            &message.subject
        });
        compose.body.set_text(&message.readable_body().text);
        compose.in_reply_to.clone_from(&message.in_reply_to);
        compose.references.clone_from(&message.references);
        compose.attachments = message
            .attachments()
            .iter()
            .map(|part| {
                Attachment::new(
                    part.filename().unwrap_or("attachment"),
                    &part.content_type.mime_type(),
                    part.body.clone(),
                )
            })
            .collect();
        compose.saved_as = Some(stored.origin.clone());
        compose.saved = compose.fingerprint();
        compose
    }

    /// Everything the form holds, to compare with what was saved.
    fn fingerprint(&self) -> String {
        let names: Vec<&str> = self
            .attachments
            .iter()
            .map(|a| a.filename.as_str())
            .collect();
        format!(
            "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
            self.from.text(),
            self.to.text(),
            self.cc.text(),
            self.subject.text(),
            self.body.text(),
            names.join("\u{1}")
        )
    }

    /// Whether the form holds anything not saved.
    #[must_use]
    pub fn unsaved(&self) -> bool {
        self.fingerprint() != self.saved
    }

    /// The draft the message builder takes.
    #[must_use]
    pub fn draft(&self) -> EmailDraft {
        let mut draft = EmailDraft::new(0);
        draft.to = split_list(self.to.text());
        draft.cc = split_list(self.cc.text());
        draft.subject = self.subject.text().to_owned();
        draft.body = self.body.text().to_owned();
        draft.attachments.clone_from(&self.attachments);
        draft.in_reply_to.clone_from(&self.in_reply_to);
        draft.references.clone_from(&self.references);
        draft
    }

    /// The sender, when the From field holds an address.
    fn sender(&self) -> Option<EmailAddress> {
        EmailAddress::parse(self.from.text()).map(decoded_name)
    }

    /// The single-line field for `field`.
    fn line_mut(&mut self, field: ComposeField) -> Option<&mut TextInput> {
        match field {
            ComposeField::From => Some(&mut self.from),
            ComposeField::To => Some(&mut self.to),
            ComposeField::Cc => Some(&mut self.cc),
            ComposeField::Subject => Some(&mut self.subject),
            ComposeField::Body => None,
        }
    }

    fn line(&self, field: ComposeField) -> Option<&TextInput> {
        match field {
            ComposeField::From => Some(&self.from),
            ComposeField::To => Some(&self.to),
            ComposeField::Cc => Some(&self.cc),
            ComposeField::Subject => Some(&self.subject),
            ComposeField::Body => None,
        }
    }
}

/// What a file dialog is up for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerFor {
    /// A message or an mbox to read.
    Open,
    /// Where to save the attachment at this place in the message's list.
    SaveAttachment(usize),
    /// Where to save the message being written, as an `.eml` file.
    SaveAs,
    /// A file to attach.
    Attach,
}

/// What a press can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Help,
    HelpCard,
    SearchBox,
    Compose,
    Reply,
    Forward,
    Delete,
    OpenFile,
    Refresh,
    Sort,
    PanePosition,
    /// A folder in the sidebar, by its place among the ones shown.
    Folder(usize),
    MessageList,
    /// A message, by its id.
    Message(u64),
    ReadingPane,
    MarkUnread,
    Flag,
    EditDraft,
    /// An attachment of the message being read, by its place in the list.
    SaveAttachment(usize),
    // The compose form.
    Field(ComposeField),
    Send,
    SaveDraft,
    SaveAs,
    Attach,
    /// An attachment of the message being written, by its place: a press
    /// takes it off.
    Unattach(usize),
    CloseCompose,
}

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
    /// The message being written, when one is.
    pub compose: Option<Compose>,
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
    /// Whether the search box has the keyboard.
    pub searching: bool,

    // -- Mail kept in files ------------------------------------------------------
    /// Where the folders are: `~/Mail`. `None` with no home to find it in.
    pub mail_dir: Option<PathBuf>,
    /// The folders it holds, and "Opened" once a file has been.
    pub folders: Vec<store::Folder>,
    /// The files opened this session, read as the "Opened" folder.
    pub opened: Vec<PathBuf>,
    /// The folders whose messages have been read in.
    loaded: BTreeSet<String>,
    /// Each message read from a file, by its id.
    pub stored: std::collections::BTreeMap<u64, store::Stored>,
    /// The marks set on mail read from files, kept apart from it.
    pub flags: store::Flags,
    /// Where the marks are kept; `None` keeps them for the session only --
    /// the tests, and a marks file that would not read, which is not written
    /// over.
    flags_path: Option<PathBuf>,
    /// The From of the last message written, for the next.
    pub identity: String,
    /// A draft asked to be deleted once: a second Delete deletes it.
    doomed: Option<u64>,

    pub picker: FilePicker,
    pub picker_for: PickerFor,
    /// What a copy or a cut in a field took, for a paste.
    clipboard: String,
    /// How far the list is scrolled, in rows, and the message read, in lines.
    pub list_scroll: usize,
    pub read_scroll: usize,
    hover: Option<Target>,
    last_hits: Vec<(Target, Rect)>,
    wheel: wheel::Accumulator,
    /// The window's size, as last drawn -- a `Cell` so drawing at another
    /// size for a test needs no `&mut`.
    size: std::cell::Cell<(f32, f32)>,

    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
}

impl Default for EmailApp {
    fn default() -> Self {
        Self::new()
    }
}

impl EmailApp {
    #[must_use]
    pub fn new() -> Self {
        Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
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
            compose: None,
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
            searching: false,
            mail_dir: None,
            folders: Vec::new(),
            opened: Vec::new(),
            loaded: BTreeSet::new(),
            stored: std::collections::BTreeMap::new(),
            flags: store::Flags::default(),
            flags_path: None,
            identity: String::new(),
            doomed: None,
            picker: FilePicker::default(),
            picker_for: PickerFor::Open,
            clipboard: String::new(),
            list_scroll: 0,
            read_scroll: 0,
            hover: None,
            last_hits: Vec::new(),
            wheel: wheel::Accumulator::default(),
            size: std::cell::Cell::new((WINDOW_WIDTH, WINDOW_HEIGHT)),
        }
    }

    /// The client `main` opens: `~/Mail` listed and its first folder read,
    /// with the marks set before put back.
    ///
    /// Everything it writes is its own -- the marks under the configuration
    /// directory, the drafts under `~/Mail/Drafts` -- and a marks file that
    /// will not read is left alone rather than written over.
    #[must_use]
    pub fn from_settings() -> Self {
        let mut app = Self::new();
        app.mail_dir = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Mail"));
        if let Some(dir) = settingsfile::config_dir() {
            let path = dir.join("email").join("flags.txt");
            match safeio::read_to_string_capped(&path, 16 << 20) {
                Ok(read) if read.truncated => {
                    app.status_message = String::from(
                        "The saved marks are too long to read; they are kept for this session only.",
                    );
                }
                Ok(read) => match store::Flags::parse(&read.text) {
                    Ok(flags) => {
                        app.flags = flags;
                        app.flags_path = Some(path);
                    }
                    Err(why) => {
                        app.status_message = format!(
                            "{} is not a marks file this reads ({why}); marks are kept for this session only.",
                            path.display()
                        );
                    }
                },
                // Not there yet: the first mark makes it.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => app.flags_path = Some(path),
                Err(e) => {
                    app.status_message = format!(
                        "Could not read the saved marks ({e}); marks are kept for this session only."
                    );
                }
            }
        }
        app.rescan();
        app
    }

    // -----------------------------------------------------------------------
    // Mail kept in files
    // -----------------------------------------------------------------------

    /// List the mail directory again, and read the folder on screen afresh.
    pub fn rescan(&mut self) {
        let keep = self.selected_mailbox.clone();
        // What was read from files goes; mail the model holds otherwise (an
        // account's, in the tests) stays.
        let from_files: BTreeSet<u64> = self.stored.keys().copied().collect();
        self.messages.retain(|m| !from_files.contains(&m.id));
        self.stored.clear();
        self.loaded.clear();
        self.folders = match &self.mail_dir {
            Some(dir) => match store::folders(dir) {
                Ok(found) => found,
                Err(why) => {
                    self.status_message = format!("Cannot list the mail folder: {why}");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        if !self.opened.is_empty() {
            self.folders.push(store::Folder {
                name: String::from("Opened"),
                source: store::FolderSource::Opened(self.opened.clone()),
                own: false,
            });
        }
        self.mailboxes.retain(|m| m.account_id != 0);
        for folder in &self.folders {
            self.mailboxes.push(Mailbox {
                name: folder.name.clone(),
                mailbox_type: if folder.own {
                    MailboxType::Drafts
                } else if folder.name.eq_ignore_ascii_case("inbox") {
                    MailboxType::Inbox
                } else {
                    MailboxType::Custom
                },
                total_messages: 0,
                unread_messages: 0,
                account_id: 0,
                imap_path: String::new(),
            });
        }
        self.recount_unread();
        let still_there = keep
            .as_ref()
            .is_some_and(|name| self.mailboxes.iter().any(|m| &m.name == name));
        if let Some(name) = keep.filter(|_| still_there) {
            self.select_mailbox(&name);
        } else if let Some(first) = self.folders.first().map(|f| f.name.clone()) {
            self.select_mailbox(&first);
        }
    }

    /// Show the folder `name`, reading its messages in the first time.
    pub fn select_mailbox(&mut self, name: &str) {
        self.selected_mailbox = Some(name.to_owned());
        self.selected_message = None;
        self.list_scroll = 0;
        self.read_scroll = 0;
        self.doomed = None;
        self.load_folder(name);
        self.reanchor_selection();
    }

    /// Read folder `name`'s messages in, unless they are already.
    fn load_folder(&mut self, name: &str) {
        if self.loaded.contains(name) {
            return;
        }
        let Some(folder) = self.folders.iter().find(|f| f.name == name).cloned() else {
            return;
        };
        self.loaded.insert(name.to_owned());
        let loaded = store::load(&folder);
        for stored in loaded.messages {
            let marks = self.flags.get(&stored.key());
            let summary = summary_of(&stored, &folder.name, marks, folder.own);
            let id = self.add_message(summary);
            self.stored.insert(id, stored);
        }
        if !loaded.notes.is_empty() {
            self.status_message = loaded.notes.join(" ");
        }
    }

    /// Put a folder's messages back as they are on disk now.
    fn reload_folder(&mut self, name: &str) {
        let ids: BTreeSet<u64> = self
            .messages
            .iter()
            .filter(|m| m.mailbox == name && m.account_id == 0)
            .map(|m| m.id)
            .collect();
        self.messages.retain(|m| !ids.contains(&m.id));
        self.stored.retain(|id, _| !ids.contains(id));
        if let Some(mb) = self
            .mailboxes
            .iter_mut()
            .find(|m| m.name == name && m.account_id == 0)
        {
            mb.total_messages = 0;
            mb.unread_messages = 0;
        }
        self.loaded.remove(name);
        self.load_folder(name);
        self.recount_unread();
        self.reanchor_selection();
    }

    /// The unread count, from the messages as they stand.
    fn recount_unread(&mut self) {
        self.unread_count = u32::try_from(self.messages.iter().filter(|m| !m.flags.seen).count())
            .unwrap_or(u32::MAX);
    }

    /// Keep message `id`'s marks, if it came from a file: in the marks file,
    /// not in the file, which is never rewritten.
    fn remember(&mut self, id: u64) {
        let (Some(stored), Some(summary)) = (
            self.stored.get(&id),
            self.messages.iter().find(|m| m.id == id),
        ) else {
            return;
        };
        self.flags.set(
            &stored.key(),
            store::Marks {
                seen: summary.flags.seen,
                flagged: summary.flags.flagged,
            },
        );
        if let Some(path) = &self.flags_path {
            let saved = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| safeio::write_str_atomically(path, &self.flags.to_text()));
            if let Err(e) = saved {
                self.status_message = format!("The mark was set, but could not be saved: {e}");
            }
        }
    }

    /// Open message `id` to read -- or, a draft, to go on writing it.
    pub fn open_message(&mut self, id: u64) {
        self.selected_message = Some(id);
        self.read_scroll = 0;
        let own = self
            .messages
            .iter()
            .find(|m| m.id == id)
            .is_some_and(|m| m.flags.draft && m.account_id == 0);
        if own && let Some(stored) = self.stored.get(&id) {
            self.compose = Some(Compose::from_saved(stored));
            self.active_panel = Panel::Compose;
            return;
        }
        self.mark_read(id);
        self.remember(id);
        self.active_panel = Panel::Reading;
    }

    /// The text a reply or a forward quotes: the whole message when it was
    /// read from a file, the preview otherwise.
    fn quote_of(&self, id: u64) -> Option<String> {
        if let Some(stored) = self.stored.get(&id) {
            return Some(stored.message.readable_body().text);
        }
        self.messages
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.preview.clone())
    }

    /// Delete message `id` -- a draft of this client's own, after a second
    /// press. Mail read from files is never changed, and says so.
    fn delete(&mut self, id: u64) {
        let Some(summary) = self.messages.iter().find(|m| m.id == id) else {
            return;
        };
        let from_file = self.stored.get(&id).map(|s| s.origin.clone());
        match from_file {
            Some(origin) if summary.flags.draft => {
                if self.doomed != Some(id) {
                    self.doomed = Some(id);
                    self.status_message = format!(
                        "Delete the draft \u{201C}{}\u{201D}? Press Delete again to delete it.",
                        summary.subject
                    );
                    return;
                }
                self.doomed = None;
                match std::fs::remove_file(&origin) {
                    Ok(()) => {
                        let folder = summary.mailbox.clone();
                        self.status_message = String::from("Draft deleted");
                        self.reload_folder(&folder);
                    }
                    Err(e) => self.status_message = format!("Could not delete the draft: {e}"),
                }
            }
            Some(_) => {
                self.status_message = String::from(
                    "Mail read from files is never changed here; only drafts written here can be deleted.",
                );
            }
            None => {
                self.delete_message(id);
                self.reanchor_selection();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Writing
    // -----------------------------------------------------------------------

    /// Start composing a new email
    pub fn compose_new(&mut self) {
        self.compose = Some(Compose::new(&self.identity));
        self.active_panel = Panel::Compose;
    }

    /// Start composing a reply, quoting the whole message.
    pub fn compose_reply(&mut self, message_id: u64) {
        let Some(quote) = self.quote_of(message_id) else {
            return;
        };
        if let Some(msg) = self.messages.iter().find(|m| m.id == message_id) {
            let draft = EmailDraft::reply(msg, &quote, msg.account_id);
            self.compose = Some(Compose::from_draft(&draft, &self.identity));
            self.active_panel = Panel::Compose;
        }
    }

    /// Start composing a forward of the whole message, its attachments with
    /// it.
    pub fn compose_forward(&mut self, message_id: u64) {
        let Some(quote) = self.quote_of(message_id) else {
            return;
        };
        if let Some(msg) = self.messages.iter().find(|m| m.id == message_id) {
            let mut draft = EmailDraft::forward(msg, &quote, msg.account_id);
            if let Some(stored) = self.stored.get(&message_id) {
                draft.attachments = stored
                    .message
                    .attachments()
                    .iter()
                    .map(|part| {
                        Attachment::new(
                            part.filename().unwrap_or("attachment"),
                            &part.content_type.mime_type(),
                            part.body.clone(),
                        )
                    })
                    .collect();
            }
            self.compose = Some(Compose::from_draft(&draft, &self.identity));
            self.active_panel = Panel::Compose;
        }
    }

    /// The message being written, as it would be sent: checked, and built.
    fn built(&mut self) -> Result<BuiltMessage, String> {
        let Some(compose) = self.compose.as_ref() else {
            return Err(String::from("nothing is being written"));
        };
        let draft = compose.draft();
        let from = compose.sender();
        if !compose.from.text().trim().is_empty() && from.is_none() {
            return Err(format!(
                "\u{201C}{}\u{201D} is not an address to send from",
                compose.from.text().trim()
            ));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        let built = draft.build_message_at(from.as_ref(), Some(&decode::rfc5322_date(now)));
        if !built.rejected_recipients.is_empty() {
            return Err(format!(
                "bad address {}",
                built.rejected_recipients.join(", ")
            ));
        }
        self.identity = compose.from.text().trim().to_owned();
        Ok(built)
    }

    /// Check the message and say plainly that it cannot be sent.
    ///
    /// The draft stays open: the composing window is the only place an
    /// unsaved message exists. It used to be cleared, the panel switched
    /// away, and the status line read `sent "..." to 3 recipient(s)` --
    /// the message destroyed, nothing transmitted, and a delivery reported.
    fn send(&mut self) {
        let Some(compose) = self.compose.as_ref() else {
            return;
        };
        let draft = compose.draft();
        let recipients = draft.to.len().saturating_add(draft.cc.len());
        let subject = draft.subject.clone();
        match self.built() {
            Err(why) => self.status_message = format!("not sent: {why}"),
            Ok(_) if recipients == 0 => {
                self.status_message = String::from("not sent: no recipients");
            }
            Ok(_) => {
                self.status_message = format!(
                    "\u{201C}{subject}\u{201D} was not sent -- nothing here can reach a mail server. \
                     {recipients} recipient(s) checked out; Ctrl+S keeps it as a draft, \
                     Save as file writes an .eml to send from elsewhere."
                );
            }
        }
    }

    /// Save the message being written as a draft in `~/Mail/Drafts`: over
    /// its own draft file when it has one, as a new file otherwise.
    fn save_draft(&mut self) {
        let Some(dir) = self.mail_dir.as_ref().map(|d| d.join(store::DRAFTS)) else {
            self.status_message =
                String::from("No mail folder to keep drafts in (no home folder is set)");
            return;
        };
        let built = match self.built() {
            Ok(built) => built,
            Err(why) => {
                self.status_message = format!("Draft not saved: {why}");
                return;
            }
        };
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.status_message = format!("Draft not saved: {}: {e}", dir.display());
            return;
        }
        let result = match &compose.saved_as {
            Some(own) => safeio::write_str_atomically(own, &built.text).map(|()| own.clone()),
            None => {
                let stem = store::draft_stem(compose.subject.text());
                let mut result = Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
                for n in 1..=999_u32 {
                    let name = if n == 1 {
                        format!("{stem}.eml")
                    } else {
                        format!("{stem} ({n}).eml")
                    };
                    let path = dir.join(name);
                    match safeio::write_new_atomically(&path, built.text.as_bytes()) {
                        Ok(()) => {
                            result = Ok(path);
                            break;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(e) => {
                            result = Err(e);
                            break;
                        }
                    }
                }
                result
            }
        };
        match result {
            Ok(path) => {
                compose.saved_as = Some(path.clone());
                compose.saved = compose.fingerprint();
                compose.confirm_close = false;
                self.status_message = format!("Draft saved as {}", path.display());
                self.reload_folder(store::DRAFTS);
            }
            Err(e) => self.status_message = format!("Draft not saved: {e}"),
        }
    }

    /// Close the form -- at once when nothing is unsaved, and on a second
    /// press when something is.
    fn close_compose(&mut self) {
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        if compose.unsaved() && !compose.confirm_close {
            compose.confirm_close = true;
            self.status_message = String::from(
                "This message is not saved. Ctrl+S keeps it as a draft; close again to discard it.",
            );
            return;
        }
        self.compose = None;
        self.active_panel = Panel::MessageList;
    }

    /// Put a file dialog up for `purpose`.
    fn ask(&mut self, purpose: PickerFor) {
        self.picker_for = purpose;
        match purpose {
            PickerFor::Open | PickerFor::Attach => self.picker.open_to_read(),
            PickerFor::SaveAs => {
                let stem = self.compose.as_ref().map_or_else(
                    || String::from("Message"),
                    |c| store::draft_stem(c.subject.text()),
                );
                self.picker.open_to_write(format!("{stem}.eml"));
            }
            PickerFor::SaveAttachment(i) => {
                let name = self
                    .selected_message
                    .and_then(|id| self.stored.get(&id))
                    .and_then(|s| {
                        s.message
                            .attachments()
                            .get(i)
                            .map(|p| p.filename().unwrap_or("attachment").to_owned())
                    })
                    .unwrap_or_else(|| String::from("attachment"));
                // A name out of a message is a stranger's: only its last part
                // is used, so it cannot name a place outside the folder chosen.
                let name = Path::new(&name).file_name().map_or_else(
                    || String::from("attachment"),
                    |n| Path::new(n).display().to_string(),
                );
                self.picker.open_to_write(name);
            }
        }
    }

    /// What the file dialog chose.
    fn picked(&mut self, path: &Path) {
        match self.picker_for {
            PickerFor::Open => {
                if !self.opened.iter().any(|p| p == path) {
                    self.opened.push(path.to_path_buf());
                }
                self.rescan();
                self.select_mailbox("Opened");
                self.status_message = format!("Opened {}", path.display());
            }
            PickerFor::SaveAs => match self.built() {
                Ok(built) => {
                    self.status_message = match safeio::write_str_atomically(path, &built.text) {
                        Ok(()) => format!("Saved as {}", path.display()),
                        Err(e) => format!("Could not save {}: {e}", path.display()),
                    };
                }
                Err(why) => self.status_message = format!("Not saved: {why}"),
            },
            PickerFor::Attach => match safeio::read_capped(path, MAX_ATTACHMENT_BYTES) {
                Ok(read) if read.truncated => {
                    self.status_message = format!(
                        "{} is larger than {} MiB, more than a message here attaches",
                        path.display(),
                        MAX_ATTACHMENT_BYTES >> 20
                    );
                }
                Ok(read) => {
                    let name = path.file_name().map_or_else(
                        || String::from("attachment"),
                        |n| Path::new(n).display().to_string(),
                    );
                    let mime = mime_for(&name);
                    if let Some(compose) = self.compose.as_mut() {
                        compose
                            .attachments
                            .push(Attachment::new(&name, mime, read.bytes));
                        compose.confirm_close = false;
                    }
                    self.status_message = format!("Attached {name}");
                }
                Err(e) => self.status_message = format!("Could not attach {}: {e}", path.display()),
            },
            PickerFor::SaveAttachment(i) => {
                let bytes = self
                    .selected_message
                    .and_then(|id| self.stored.get(&id))
                    .and_then(|s| s.message.attachments().get(i).map(|p| p.body.clone()));
                self.status_message = match bytes {
                    Some(bytes) => match safeio::write_atomically(path, &bytes) {
                        Ok(()) => format!("Saved {}", path.display()),
                        Err(e) => format!("Could not save {}: {e}", path.display()),
                    },
                    None => String::from("That attachment is no longer on screen"),
                };
            }
        }
    }

    // ====================================================================
    // Input
    // ====================================================================

    /// Handle one event from the window.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The dialog takes input first while it is up, or a name typed into it
        // would reach the window behind.
        match self.picker.handle(event, self.width(), self.height()) {
            Picked::Chose(path) => {
                self.picked(&path);
                return EventResult::Consumed;
            }
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) if key.pressed => {
                let before = self.selected_message;
                let result = self.handle_key(key);
                if self.selected_message != before {
                    self.follow_selection();
                }
                result
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                self.size.set((*width as f32, *height as f32));
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handle a key press.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        // Above the compose form and the search box, because `F1` is not text
        // and a reader may want the keys from either.
        if key.key == Key::F1 {
            self.show_help = !self.show_help;
            return EventResult::Consumed;
        }
        if self.show_help {
            // Modal: letting keys through would mean deleting a message you
            // cannot see.
            if matches!(key.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
            return EventResult::Consumed;
        }
        if self.compose.is_some() {
            return self.handle_compose_key(key);
        }
        if self.searching {
            return self.handle_search_key(key);
        }

        if key.modifiers.ctrl {
            return match key.key {
                Key::N => {
                    self.compose_new();
                    EventResult::Consumed
                }
                Key::O => {
                    self.ask(PickerFor::Open);
                    EventResult::Consumed
                }
                // The sort order and the reading pane, both drawn from and
                // neither changeable before `scripts/frozen-flag-survey.py`
                // found them.
                Key::S => {
                    self.sort_order = self.sort_order.next();
                    EventResult::Consumed
                }
                Key::P => {
                    self.reading_pane_position = self.reading_pane_position.next();
                    EventResult::Consumed
                }
                Key::F => {
                    self.searching = true;
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            };
        }

        match key.key {
            Key::Up | Key::Down => {
                self.move_selection(if key.key == Key::Down { 1 } else { -1 });
                self.read_scroll = 0;
            }
            // Read it. `mark_read` was called from nowhere else, so a message
            // opened stayed unread and the count in the title never moved.
            Key::Enter => {
                if let Some(id) = self.selected_message {
                    self.open_message(id);
                }
            }
            Key::Escape => {
                if self.active_panel == Panel::Reading {
                    self.active_panel = Panel::MessageList;
                } else if !self.search_query.is_empty() {
                    self.search_query.clear();
                    self.reanchor_selection();
                } else {
                    return EventResult::Ignored;
                }
            }
            Key::PageUp | Key::PageDown => {
                let page = self.read_rows().saturating_sub(2).max(1);
                self.read_scroll = if key.key == Key::PageDown {
                    self.read_scroll.saturating_add(page)
                } else {
                    self.read_scroll.saturating_sub(page)
                };
                self.clamp_read_scroll();
            }
            // Reply and forward. Both existed, both were tested, and neither
            // had a key -- so a mail client could read mail and not answer it.
            Key::R => {
                if let Some(id) = self.selected_message {
                    self.compose_reply(id);
                }
            }
            Key::F => {
                if let Some(id) = self.selected_message {
                    self.compose_forward(id);
                }
            }
            Key::U => {
                if let Some(id) = self.selected_message {
                    self.mark_unread(id);
                    self.remember(id);
                }
            }
            Key::S => {
                if let Some(id) = self.selected_message {
                    self.toggle_flagged(id);
                    self.remember(id);
                }
            }
            Key::Delete => {
                if let Some(id) = self.selected_message {
                    self.delete(id);
                }
            }
            Key::F5 => {
                self.rescan();
                self.status_message = String::from("Looked in the mail folder again");
            }
            // Through the mailboxes.
            Key::Left | Key::Right => {
                self.move_mailbox(if key.key == Key::Right { 1 } else { -1 });
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Keys while the search box is open.
    fn handle_search_key(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.searching = false;
                self.search_query.clear();
                self.reanchor_selection();
                EventResult::Consumed
            }
            Key::Enter => {
                self.searching = false;
                EventResult::Consumed
            }
            Key::Backspace => {
                self.search_query.pop();
                self.reanchor_selection();
                EventResult::Consumed
            }
            _ => {
                let typed: String = key.typed().collect();
                if typed.is_empty() {
                    return EventResult::Ignored;
                }
                self.search_query.push_str(&typed);
                self.reanchor_selection();
                EventResult::Consumed
            }
        }
    }

    /// Keys while a message is being written.
    ///
    /// Tab walks the fields, Ctrl+S saves a draft, Ctrl+Enter checks and
    /// says it cannot send, Escape closes (asking first over unsaved work),
    /// and everything else types into the field.
    fn handle_compose_key(&mut self, key: &KeyEvent) -> EventResult {
        let ctrl = key.modifiers.ctrl;
        match key.key {
            Key::Escape => self.close_compose(),
            Key::Enter if ctrl => self.send(),
            Key::S if ctrl => self.save_draft(),
            Key::Tab => {
                if let Some(compose) = self.compose.as_mut() {
                    compose.field = compose.field.step(key.modifiers.shift);
                }
            }
            _ => {
                let page = self.body_rows().saturating_sub(1).max(1);
                let clipboard = self.clipboard.clone();
                let Some(compose) = self.compose.as_mut() else {
                    return EventResult::Ignored;
                };
                let copied = if compose.field == ComposeField::Body {
                    let edited = compose.body.apply_key(key, BODY_CAPACITY, &clipboard, page);
                    if !edited.handled {
                        return EventResult::Ignored;
                    }
                    edited.copied
                } else {
                    let field = compose.field;
                    let Some(input) = compose.line_mut(field) else {
                        return EventResult::Ignored;
                    };
                    edit_line(input, key, FIELD_CAPACITY, &clipboard)
                };
                compose.confirm_close = false;
                if let Some(text) = copied {
                    self.clipboard = text;
                }
                self.keep_body_caret_visible();
            }
        }
        EventResult::Consumed
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
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id)
            && !msg.flags.seen
        {
            msg.flags.seen = true;
            self.unread_count = self.unread_count.saturating_sub(1);
            for mb in &mut self.mailboxes {
                if mb.name == msg.mailbox && mb.account_id == msg.account_id {
                    mb.unread_messages = mb.unread_messages.saturating_sub(1);
                }
            }
        }
    }

    /// Mark a message as unread
    pub fn mark_unread(&mut self, id: u64) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id)
            && msg.flags.seen
        {
            msg.flags.seen = false;
            self.unread_count = self.unread_count.saturating_add(1);
            for mb in &mut self.mailboxes {
                if mb.name == msg.mailbox && mb.account_id == msg.account_id {
                    mb.unread_messages = mb.unread_messages.saturating_add(1);
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

    /// Move the message selection by `delta` rows through what is on screen.
    ///
    /// Held as an id rather than an index, so a message arriving or being
    /// deleted does not silently move the selection to its neighbour. Stops at
    /// the ends rather than wrapping.
    fn move_selection(&mut self, delta: isize) {
        let ids: Vec<u64> = self.current_messages().iter().map(|m| m.id).collect();
        if ids.is_empty() {
            self.selected_message = None;
            return;
        }
        let last = (ids.len() as isize).saturating_sub(1);
        let current = self
            .selected_message
            .and_then(|id| ids.iter().position(|other| *other == id));
        let next = match current {
            Some(index) => (index as isize).saturating_add(delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        self.selected_message = ids.get(next.unsigned_abs()).copied();
    }

    /// Move to another mailbox, and take the selection with it.
    fn move_mailbox(&mut self, delta: isize) {
        let names: Vec<String> = self
            .visible_mailboxes()
            .iter()
            .map(|m| m.name.clone())
            .collect();
        if names.is_empty() {
            return;
        }
        let last = (names.len() as isize).saturating_sub(1);
        let current = self
            .selected_mailbox
            .as_ref()
            .and_then(|name| names.iter().position(|other| other == name));
        let next = match current {
            Some(index) => (index as isize).saturating_add(delta).clamp(0, last),
            None => 0,
        };
        // Through `select_mailbox`, which reads a folder in the first time it
        // is shown.
        if let Some(name) = names.get(next.unsigned_abs()).cloned() {
            self.select_mailbox(&name);
        }
    }

    /// Keep the selection on a message that is still on screen.
    ///
    /// Called after anything that changes the list -- a search, a delete, a
    /// change of mailbox.
    fn reanchor_selection(&mut self) {
        let ids: Vec<u64> = self.current_messages().iter().map(|m| m.id).collect();
        if self.selected_message.is_some_and(|id| ids.contains(&id)) {
            return;
        }
        self.selected_message = ids.first().copied();
    }

    /// Get messages for current mailbox, filtered and sorted
    #[must_use]
    pub fn current_messages(&self) -> Vec<&MessageSummary> {
        let account_id = self.selected_account;
        let mailbox = self.selected_mailbox.as_deref();

        let mut msgs: Vec<&MessageSummary> = self
            .messages
            .iter()
            .filter(|m| {
                account_id.is_none_or(|aid| m.account_id == aid)
                    && mailbox.is_none_or(|mb| m.mailbox == mb)
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

        msgs.sort_by(|a, b| match self.sort_order {
            SortOrder::DateDesc => b.timestamp.cmp(&a.timestamp),
            SortOrder::DateAsc => a.timestamp.cmp(&b.timestamp),
            SortOrder::SenderAsc => a.from.to_string().cmp(&b.from.to_string()),
            SortOrder::SubjectAsc => a.subject.cmp(&b.subject),
            SortOrder::SizeDesc => b.size.cmp(&a.size),
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
                    if let Some(m) = self.messages.iter_mut().find(|m| m.id == message_id)
                        && !m.labels.contains(label)
                    {
                        m.labels.push(label.clone());
                    }
                }
                FilterAction::ForwardTo(_) => {
                    // Would forward via SMTP — not implemented in offline mode
                }
            }
        }

        applied
    }

    // ====================================================================
    // Layout
    // ====================================================================

    fn width(&self) -> f32 {
        self.size.get().0
    }

    fn height(&self) -> f32 {
        self.size.get().1
    }

    /// Where the mail starts, under the header, the notice and the toolbar.
    fn content_top() -> f32 {
        HEADER_H + NOTICE_H + TOOLBAR_H
    }

    /// The message list's box and, when the reading pane is shown beside or
    /// under it, the pane's.
    fn panes(&self) -> (Rect, Option<Rect>) {
        let top = Self::content_top();
        let bottom = self.height() - STATUS_H;
        let x = SIDEBAR_W;
        let w = (self.width() - x).max(0.0);
        let h = (bottom - top).max(0.0);
        match self.reading_pane_position {
            ReadingPanePosition::Right => {
                let list_w = (w * 0.4).floor();
                (
                    Rect::new(x, top, list_w, h),
                    Some(Rect::new(x + list_w, top, (w - list_w).max(0.0), h)),
                )
            }
            ReadingPanePosition::Bottom => {
                let list_h = (h * 0.45).floor();
                (
                    Rect::new(x, top, w, list_h),
                    Some(Rect::new(x, top + list_h, w, (h - list_h).max(0.0))),
                )
            }
            // Off: the list, or the message read in its place.
            ReadingPanePosition::Off => {
                let whole = Rect::new(x, top, w, h);
                if self.active_panel == Panel::Reading {
                    (Rect::new(x, top, 0.0, 0.0), Some(whole))
                } else {
                    (whole, None)
                }
            }
        }
    }

    /// How many whole rows the list shows.
    fn list_rows(&self) -> usize {
        let (list, _) = self.panes();
        ((list.h - 4.0) / ROW_H).floor().max(1.0) as usize
    }

    /// The lines of the message being read, wrapped to the pane.
    fn read_lines(&self) -> Vec<String> {
        let (_, Some(pane)) = self.panes() else {
            return Vec::new();
        };
        let Some(id) = self.selected_message else {
            return Vec::new();
        };
        let body = match self.stored.get(&id) {
            Some(stored) => stored.message.readable_body().text,
            None => self
                .messages
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.preview.clone())
                .unwrap_or_default(),
        };
        text::wrap_hard(
            &body,
            (pane.w - 32.0).max(1.0),
            BODY_TEXT,
            FontWeightHint::Regular,
        )
    }

    /// Where the body of the message being read starts in its pane: under
    /// the subject, four header lines, the buttons and the attachments.
    fn read_body_top(&self, pane: Rect) -> f32 {
        let attachments = self
            .selected_message
            .and_then(|id| self.stored.get(&id))
            .map_or(0, |s| s.message.attachments().len());
        let chips = if attachments == 0 { 0.0 } else { 34.0 };
        pane.y + 16.0 + 26.0 + 18.0 * 4.0 + 8.0 + 36.0 + chips + 12.0
    }

    /// How many lines of the message being read show.
    fn read_rows(&self) -> usize {
        let (_, Some(pane)) = self.panes() else {
            return 1;
        };
        let top = self.read_body_top(pane);
        (((pane.y + pane.h) - top - 8.0) / BODY_LINE_H)
            .floor()
            .max(1.0) as usize
    }

    fn clamp_read_scroll(&mut self) {
        let total = self.read_lines().len();
        self.read_scroll = self.read_scroll.min(total.saturating_sub(self.read_rows()));
    }

    /// Scroll the list so the chosen message is on screen.
    fn follow_selection(&mut self) {
        let Some(id) = self.selected_message else {
            return;
        };
        if let Some(at) = self.current_messages().iter().position(|m| m.id == id) {
            let rows = self.list_rows();
            if at < self.list_scroll {
                self.list_scroll = at;
            } else if at >= self.list_scroll.saturating_add(rows) {
                self.list_scroll = at.saturating_add(1).saturating_sub(rows);
            }
        }
    }

    /// The compose form's box, over the list and the reading pane.
    fn compose_rect(&self) -> Rect {
        let top = Self::content_top();
        Rect::new(
            SIDEBAR_W,
            top,
            (self.width() - SIDEBAR_W).max(0.0),
            (self.height() - STATUS_H - top).max(0.0),
        )
    }

    /// The box of a compose field.
    fn field_rect(&self, field: ComposeField) -> Rect {
        let r = self.compose_rect();
        let label_w = 80.0;
        let row = |i: f32| {
            Rect::new(
                r.x + 16.0 + label_w,
                r.y + 44.0 + i * 36.0,
                (r.w - 32.0 - label_w).max(0.0),
                FIELD_H,
            )
        };
        match field {
            ComposeField::From => row(0.0),
            ComposeField::To => row(1.0),
            ComposeField::Cc => row(2.0),
            ComposeField::Subject => row(3.0),
            ComposeField::Body => {
                let top = r.y + 44.0 + 4.0 * 36.0 + 8.0;
                let attachments = self.compose.as_ref().map_or(0, |c| c.attachments.len());
                let chips = if attachments == 0 { 0.0 } else { 32.0 };
                Rect::new(
                    r.x + 16.0,
                    top,
                    (r.w - 32.0).max(0.0),
                    (r.y + r.h - top - 56.0 - chips).max(BODY_LINE_H),
                )
            }
        }
    }

    /// How many lines of the body being written show.
    fn body_rows(&self) -> usize {
        let body = self.field_rect(ComposeField::Body);
        ((body.h - 8.0) / BODY_LINE_H).floor().max(1.0) as usize
    }

    /// Scroll the body being written so its caret shows, down and across.
    fn keep_body_caret_visible(&mut self) {
        let rows = self.body_rows();
        let width = (self.field_rect(ComposeField::Body).w - 16.0).max(1.0);
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        let caret = compose.body.caret();
        let line = compose.body.line_index(caret);
        if line < compose.body_scroll {
            compose.body_scroll = line;
        } else if line >= compose.body_scroll.saturating_add(rows) {
            compose.body_scroll = line.saturating_add(1).saturating_sub(rows);
        }
        let start = compose.body.line_start(caret);
        let x = text::measure(
            compose.body.text().get(start..caret).unwrap_or(""),
            BODY_TEXT,
            FontWeightHint::Regular,
        );
        if x < compose.body_hscroll {
            compose.body_hscroll = (x - 40.0).max(0.0);
        } else if x > compose.body_hscroll + width - 8.0 {
            compose.body_hscroll = x - width + 40.0;
        }
    }

    // ====================================================================
    // Drawing
    // ====================================================================

    /// Everything the window draws, at `width` by `height`, for the tests
    /// that read what was drawn.
    #[cfg(test)]
    pub fn render_commands(&self, width: f32, height: f32) -> Vec<RenderCommand> {
        self.frame_at(width, height).into_tree().commands
    }

    /// The window at its own size.
    fn frame(&self) -> Frame<Target> {
        self.frame_at(self.width(), self.height())
    }

    /// The window at `width` by `height`, and where every control in it is.
    fn frame_at(&self, width: f32, height: f32) -> Frame<Target> {
        // Laid out at the size asked for, and the window's own put back: a
        // test draws at any size without the window's changing under it.
        let own = self.size.replace((width, height));
        let mut f = Frame::new(width, height);
        f.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        self.render_header(&mut f);
        self.render_toolbar(&mut f);
        self.render_sidebar(&mut f);
        if self.compose.is_some() {
            self.render_compose(&mut f);
        } else {
            let (list, pane) = self.panes();
            if list.w > 0.0 && list.h > 0.0 {
                self.render_message_list(&mut f, list);
            }
            if let Some(pane) = pane {
                self.render_reading_pane(&mut f, pane);
            }
        }
        self.render_status(&mut f);
        self.size.set(own);
        if self.picker.is_open() {
            f.extend(self.picker.render(&self.palette, width, height));
        }
        // Over the reading pane and the compose form both.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (width, height),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));
        }
        f
    }

    /// A button: a press on it does `target`; a disabled one takes no press.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface2
            } else {
                self.palette.surface1
            },
            corner_radii: CornerRadii::all(4.0),
        });
        f.push(RenderCommand::Text {
            x: text::center_x(label, rect.x + rect.w / 2.0, 12.0, FontWeightHint::Regular)
                .max(rect.x + 4.0),
            y: rect.y + (rect.h - 12.0) / 2.0,
            text: label.to_string(),
            font_size: 12.0,
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 8.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// Buttons left to right from `x`, each as wide as its label; answers
    /// where the last ended.
    fn buttons_from(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        buttons: &[(&str, Target, bool)],
    ) -> f32 {
        let mut at = x;
        for (label, target, enabled) in buttons {
            let w = text::padded_width(label, 12.0, 12.0, FontWeightHint::Regular);
            self.button(f, Rect::new(at, y, w, 28.0), label, *target, *enabled);
            at += w + 6.0;
        }
        at
    }

    /// One line of text.
    fn line(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: Color,
        max: f32,
    ) {
        f.push(RenderCommand::Text {
            x,
            y,
            text,
            font_size: size,
            color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(max.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
    }

    fn render_header(&self, f: &mut Frame<Target>) {
        let width = self.width();
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            width,
            HEADER_H,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        f.push(RenderCommand::Text {
            x: 16.0,
            y: 14.0,
            text: "Mail".to_string(),
            font_size: 18.0,
            color: self.palette.ink(self.palette.blue),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        if self.unread_count > 0 {
            f.push(RenderCommand::FillRect {
                x: 70.0,
                y: 12.0,
                width: 40.0,
                height: 22.0,
                color: self.palette.red,
                corner_radii: CornerRadii::all(11.0),
            });
            f.push(RenderCommand::Text {
                x: text::center_x(
                    &self.unread_count.to_string(),
                    90.0,
                    12.0,
                    FontWeightHint::Bold,
                ),
                y: 16.0,
                text: self.unread_count.to_string(),
                font_size: 12.0,
                color: self.palette.ink(self.palette.red),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        // The search box: a press puts the keyboard in it.
        let search = Rect::new(128.0, 10.0, 320.0, 28.0);
        self.palette.push_surface(
            f,
            search.x,
            search.y,
            search.w,
            search.h,
            6.0,
            Surface::Card,
        );
        if self.searching {
            f.push(RenderCommand::StrokeRect {
                x: search.x,
                y: search.y,
                width: search.w,
                height: search.h,
                color: self.palette.blue,
                line_width: 2.0,
                corner_radii: CornerRadii::all(6.0),
            });
        }
        let (shown, color) = if self.search_query.is_empty() && !self.searching {
            ("Search mail (Ctrl+F)".to_string(), self.palette.subtext0)
        } else if self.searching {
            (format!("{}\u{258F}", self.search_query), self.palette.text)
        } else {
            (self.search_query.clone(), self.palette.text)
        };
        self.line(
            f,
            search.x + 12.0,
            search.y + 7.0,
            shown,
            12.0,
            color,
            search.w - 24.0,
        );
        f.hit(Target::SearchBox, search);
        self.button(
            f,
            Rect::new(width - 44.0, 10.0, 32.0, 28.0),
            "?",
            Target::Help,
            true,
        );
        // Why there is no mail from a server, before anybody wonders.
        for (i, text) in CANNOT_FETCH_LINES.iter().enumerate() {
            let first = i == 0;
            f.push(RenderCommand::Text {
                x: 16.0,
                y: HEADER_H + 4.0 + i as f32 * 17.0,
                text: (*text).to_string(),
                color: if first {
                    self.palette.ink(self.palette.yellow)
                } else {
                    self.palette.subtext0
                },
                font_size: if first { 12.0 } else { 11.0 },
                font_weight: if first {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((width - 32.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    fn render_toolbar(&self, f: &mut Frame<Target>) {
        let width = self.width();
        let y = HEADER_H + NOTICE_H;
        self.palette.push_surface(
            f,
            0.0,
            y,
            width,
            TOOLBAR_H,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        let chosen = self.selected_message.is_some();
        let composing = self.compose.is_some();
        let end = self.buttons_from(
            f,
            16.0,
            y + 6.0,
            &[
                ("Compose", Target::Compose, !composing),
                ("Reply", Target::Reply, chosen && !composing),
                ("Forward", Target::Forward, chosen && !composing),
                ("Delete", Target::Delete, chosen && !composing),
                ("Open file\u{2026}", Target::OpenFile, !composing),
                (
                    "Refresh",
                    Target::Refresh,
                    !composing && self.mail_dir.is_some(),
                ),
            ],
        );
        let sort = format!("Sort: {}", self.sort_order.label());
        let pane = self.reading_pane_position.label();
        let right = width - 16.0;
        let pane_w = text::padded_width(pane, 12.0, 12.0, FontWeightHint::Regular);
        let sort_w = text::padded_width(&sort, 12.0, 12.0, FontWeightHint::Regular);
        let sort_x = (right - pane_w - 6.0 - sort_w).max(end + 12.0);
        self.button(
            f,
            Rect::new(sort_x, y + 6.0, sort_w, 28.0),
            &sort,
            Target::Sort,
            true,
        );
        self.button(
            f,
            Rect::new(sort_x + sort_w + 6.0, y + 6.0, pane_w, 28.0),
            pane,
            Target::PanePosition,
            true,
        );
    }

    fn render_sidebar(&self, f: &mut Frame<Target>) {
        let top = Self::content_top();
        let h = (self.height() - STATUS_H - top).max(0.0);
        self.palette
            .push_surface(f, 0.0, top, SIDEBAR_W, h, 0.0, Surface::Sidebar);
        f.push(RenderCommand::Text {
            x: 12.0,
            y: top + 10.0,
            text: String::from("FOLDERS"),
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(SIDEBAR_W - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        let place = match (
            &self.mail_dir,
            self.selected_account
                .and_then(|id| self.accounts.iter().find(|a| a.id == id)),
        ) {
            (_, Some(account)) => account.email.clone(),
            (Some(dir), None) => dir.display().to_string(),
            (None, None) => String::from("No mail folder"),
        };
        self.line(
            f,
            12.0,
            top + 26.0,
            place,
            10.0,
            self.palette.subtext1,
            SIDEBAR_W - 24.0,
        );
        let mut y = top + 48.0;
        for (i, mb) in self.visible_mailboxes().into_iter().enumerate() {
            let chosen = self.selected_mailbox.as_deref() == Some(mb.name.as_str());
            let row = Rect::new(4.0, y, SIDEBAR_W - 8.0, FOLDER_ROW_H - 2.0);
            if chosen || self.hover == Some(Target::Folder(i)) {
                self.palette.push_surface(
                    f,
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    4.0,
                    if chosen {
                        Surface::Selected
                    } else {
                        Surface::Card
                    },
                );
            }
            let icon = match mb.mailbox_type {
                MailboxType::Inbox => "\u{1F4E5}",
                MailboxType::Sent => "\u{1F4E4}",
                MailboxType::Drafts => "\u{1F4DD}",
                MailboxType::Trash => "\u{1F5D1}",
                MailboxType::Spam => "\u{26A0}",
                MailboxType::Archive => "\u{1F4E6}",
                MailboxType::Custom => "\u{1F4C1}",
            };
            f.push(RenderCommand::Text {
                x: 12.0,
                y: y + 7.0,
                text: format!("{icon} {}", mb.name),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: if chosen {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(SIDEBAR_W - 60.0),
                overflow: TextOverflow::Ellipsis,
            });
            if mb.unread_messages > 0 {
                f.push(RenderCommand::Text {
                    x: SIDEBAR_W - 40.0,
                    y: y + 7.0,
                    text: mb.unread_messages.to_string(),
                    font_size: 11.0,
                    color: self.palette.ink(self.palette.blue),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            f.hit(Target::Folder(i), row);
            y += FOLDER_ROW_H;
        }
    }

    /// The folders the sidebar shows: the chosen account's, or all.
    fn visible_mailboxes(&self) -> Vec<&Mailbox> {
        self.mailboxes
            .iter()
            .filter(|mb| self.selected_account.is_none_or(|aid| mb.account_id == aid))
            .collect()
    }

    fn render_message_list(&self, f: &mut Frame<Target>, r: Rect) {
        f.hit(Target::MessageList, r);
        let messages = self.current_messages();
        if messages.is_empty() {
            let why = match &self.selected_mailbox {
                Some(name) if !self.search_query.is_empty() => {
                    format!(
                        "Nothing in {name} matches \u{201C}{}\u{201D}.",
                        self.search_query
                    )
                }
                Some(name) => format!("No messages in {name}."),
                None => String::from("Choose a folder."),
            };
            self.line(
                f,
                r.x + 16.0,
                r.y + 16.0,
                why,
                13.0,
                self.palette.subtext0,
                r.w - 32.0,
            );
            return;
        }
        let rows = self.list_rows();
        for (shown, msg) in messages
            .iter()
            .skip(self.list_scroll)
            .take(rows)
            .enumerate()
        {
            let y = r.y + 4.0 + shown as f32 * ROW_H;
            let row = Rect::new(r.x + 4.0, y, (r.w - 8.0).max(0.0), ROW_H - 2.0);
            let chosen = self.selected_message == Some(msg.id);
            if chosen || self.hover == Some(Target::Message(msg.id)) {
                self.palette.push_surface(
                    f,
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    4.0,
                    if chosen {
                        Surface::Selected
                    } else {
                        Surface::Card
                    },
                );
            }
            if !msg.flags.seen {
                f.push(RenderCommand::FillRect {
                    x: row.x + 6.0,
                    y: y + 12.0,
                    width: 8.0,
                    height: 8.0,
                    color: self.palette.blue,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            let weight = if msg.flags.seen {
                FontWeightHint::Regular
            } else {
                FontWeightHint::Bold
            };
            f.push(RenderCommand::Text {
                x: row.x + 20.0,
                y: y + 6.0,
                text: msg.from.to_string(),
                font_size: 13.0,
                color: self.palette.text,
                font_weight: weight,
                max_width: Some((row.w - 150.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let date = short_date(msg);
            f.push(RenderCommand::Text {
                x: text::right_x(&date, row.x + row.w - 8.0, 11.0, FontWeightHint::Regular),
                y: y + 8.0,
                text: date,
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            let mut marks = String::new();
            if msg.flags.flagged {
                marks.push_str("\u{2691} ");
            }
            if msg.flags.has_attachment {
                marks.push_str("\u{1F4CE} ");
            }
            if msg.flags.draft {
                marks.push_str("[Draft] ");
            }
            f.push(RenderCommand::Text {
                x: row.x + 20.0,
                y: y + 24.0,
                text: format!("{marks}{}", msg.subject),
                font_size: 12.0,
                color: self.palette.text,
                font_weight: weight,
                max_width: Some((row.w - 28.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            self.line(
                f,
                row.x + 20.0,
                y + 42.0,
                msg.preview.clone(),
                11.0,
                self.palette.subtext0,
                row.w - 28.0,
            );
            f.hit(Target::Message(msg.id), row);
        }
    }

    fn render_reading_pane(&self, f: &mut Frame<Target>, r: Rect) {
        f.push(RenderCommand::FillRect {
            x: r.x,
            y: r.y,
            width: 1.0,
            height: r.h,
            color: self.palette.surface0,
            corner_radii: CornerRadii::ZERO,
        });
        f.hit(Target::ReadingPane, r);
        let Some(msg) = self
            .selected_message
            .and_then(|id| self.messages.iter().find(|m| m.id == id))
        else {
            self.line(
                f,
                r.x + 16.0,
                r.y + 16.0,
                String::from("Choose a message to read."),
                13.0,
                self.palette.subtext0,
                r.w - 32.0,
            );
            return;
        };
        let stored = self.stored.get(&msg.id);
        let px = r.x + 16.0;
        let max_w = (r.w - 32.0).max(0.0);
        let mut py = r.y + 16.0;
        f.push(RenderCommand::Text {
            x: px,
            y: py,
            text: msg.subject.clone(),
            font_size: 16.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(max_w),
            overflow: TextOverflow::Ellipsis,
        });
        py += 26.0;
        let list = |addrs: &[EmailAddress]| {
            addrs
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        for text in [
            format!("From: {}", msg.from),
            format!("To: {}", list(&msg.to)),
            if msg.cc.is_empty() {
                String::new()
            } else {
                format!("Cc: {}", list(&msg.cc))
            },
            format!("Date: {}", msg.date),
        ] {
            self.line(f, px, py, text, 12.0, self.palette.subtext1, max_w);
            py += 18.0;
        }
        py += 8.0;
        let draft = msg.flags.draft && stored.is_some();
        let mut buttons = vec![
            ("Reply", Target::Reply, true),
            ("Forward", Target::Forward, true),
            (
                if msg.flags.seen {
                    "Mark unread"
                } else {
                    "Mark read"
                },
                Target::MarkUnread,
                true,
            ),
            (
                if msg.flags.flagged { "Unflag" } else { "Flag" },
                Target::Flag,
                true,
            ),
        ];
        if draft {
            buttons.insert(0, ("Edit draft", Target::EditDraft, true));
        }
        self.buttons_from(f, px, py, &buttons);
        py += 36.0;
        if let Some(stored) = stored {
            let attachments = stored.message.attachments();
            if !attachments.is_empty() {
                let mut x = px;
                for (i, part) in attachments.iter().enumerate() {
                    let label = format!(
                        "\u{1F4CE} {} ({})  Save\u{2026}",
                        part.filename().unwrap_or("attachment"),
                        guitk::bytes::iec(u64::try_from(part.body.len()).unwrap_or(u64::MAX))
                    );
                    let w =
                        text::padded_width(&label, 12.0, 11.0, FontWeightHint::Regular).min(max_w);
                    if x + w > px + max_w && x > px {
                        break;
                    }
                    self.button(
                        f,
                        Rect::new(x, py, w, 26.0),
                        &label,
                        Target::SaveAttachment(i),
                        true,
                    );
                    x += w + 6.0;
                }
                py += 34.0;
            }
        }
        let body_top = self.read_body_top(r);
        let _ = py;
        f.push(RenderCommand::FillRect {
            x: px,
            y: body_top - 8.0,
            width: max_w,
            height: 1.0,
            color: self.palette.surface0,
            corner_radii: CornerRadii::ZERO,
        });
        if let Some(note) = stored.and_then(|s| s.message.readable_body().note) {
            self.line(
                f,
                px,
                body_top - 26.0,
                note,
                11.0,
                self.palette.ink(self.palette.yellow),
                max_w,
            );
        }
        let rows = self.read_rows();
        for (i, line) in self
            .read_lines()
            .into_iter()
            .skip(self.read_scroll)
            .take(rows)
            .enumerate()
        {
            f.push(RenderCommand::Text {
                x: px,
                y: body_top + i as f32 * BODY_LINE_H,
                text: line,
                font_size: BODY_TEXT,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some(max_w),
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn render_compose(&self, f: &mut Frame<Target>) {
        let Some(compose) = &self.compose else {
            return;
        };
        let r = self.compose_rect();
        self.palette
            .push_surface(f, r.x, r.y, r.w, r.h, 0.0, Surface::Card);
        let title = match (&compose.saved_as, compose.unsaved()) {
            (None, _) => String::from("New message"),
            (Some(_), true) => String::from("Draft \u{2014} changes not saved"),
            (Some(_), false) => String::from("Draft \u{2014} saved"),
        };
        f.push(RenderCommand::Text {
            x: r.x + 16.0,
            y: r.y + 12.0,
            text: title,
            font_size: 15.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: Some(300.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.line(
            f,
            r.x + 340.0,
            r.y + 15.0,
            String::from(COMPOSE_KEYS),
            11.0,
            self.palette.subtext0,
            r.w - 356.0,
        );
        for field in [
            ComposeField::From,
            ComposeField::To,
            ComposeField::Cc,
            ComposeField::Subject,
        ] {
            let rect = self.field_rect(field);
            self.line(
                f,
                r.x + 16.0,
                rect.y + 7.0,
                field.label().to_owned(),
                12.0,
                self.palette.subtext1,
                76.0,
            );
            if let Some(input) = compose.line(field) {
                self.render_field(f, input, rect, compose.field == field, field);
            }
        }
        self.render_body(f, compose);
        let body = self.field_rect(ComposeField::Body);
        let mut y = body.y + body.h + 8.0;
        if !compose.attachments.is_empty() {
            let mut x = r.x + 16.0;
            for (i, att) in compose.attachments.iter().enumerate() {
                let label = format!(
                    "\u{1F4CE} {} ({})  \u{2715}",
                    att.filename,
                    guitk::bytes::iec(u64::try_from(att.size()).unwrap_or(u64::MAX))
                );
                let w = text::padded_width(&label, 12.0, 11.0, FontWeightHint::Regular);
                if x + w > r.x + r.w - 16.0 && x > r.x + 16.0 {
                    break;
                }
                self.button(
                    f,
                    Rect::new(x, y, w, 24.0),
                    &label,
                    Target::Unattach(i),
                    true,
                );
                x += w + 6.0;
            }
            y += 32.0;
        }
        self.buttons_from(
            f,
            r.x + 16.0,
            y,
            &[
                ("Send", Target::Send, true),
                ("Save draft", Target::SaveDraft, self.mail_dir.is_some()),
                ("Save as file\u{2026}", Target::SaveAs, true),
                ("Attach\u{2026}", Target::Attach, true),
                ("Close", Target::CloseCompose, true),
            ],
        );
    }

    /// A single-line field of the compose form.
    fn render_field(
        &self,
        f: &mut Frame<Target>,
        input: &TextInput,
        rect: Rect,
        focused: bool,
        field: ComposeField,
    ) {
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });
        if input.text().is_empty() && !focused {
            let hint = match field {
                ComposeField::From => "Your name <you@example.com>",
                ComposeField::To => "Who it is to, separated by commas",
                ComposeField::Cc => "Who else sees it",
                ComposeField::Subject | ComposeField::Body => "",
            };
            self.line(
                f,
                rect.x + 8.0,
                rect.y + 7.0,
                hint.to_owned(),
                12.0,
                self.palette.subtext0,
                rect.w - 16.0,
            );
        } else {
            let mut tree = RenderTree::new();
            textedit::draw(
                &mut tree,
                &textedit::SingleLine {
                    text: input.text(),
                    cursor: if focused {
                        input.cursor()
                    } else {
                        text::TextCursor::default()
                    },
                    selection_anchor: if focused {
                        input.selection_anchor()
                    } else {
                        None
                    },
                    focused,
                    x: rect.x + 8.0,
                    y: rect.y + 6.0,
                    width: (rect.w - 16.0).max(0.0),
                    line_height: 16.0,
                    font_size: 13.0,
                    weight: FontWeightHint::Regular,
                    color: self.palette.text,
                    selection_bg: self.palette.blue,
                    selection_fg: self.palette.crust,
                    caret_width: textedit::CARET_WIDTH,
                },
            );
            f.extend(tree.commands);
        }
        f.hit(Target::Field(field), rect);
    }

    /// The body being written: its lines from the scroll, its selection and
    /// its caret, clipped to its box.
    fn render_body(&self, f: &mut Frame<Target>, compose: &Compose) {
        let rect = self.field_rect(ComposeField::Body);
        let focused = compose.field == ComposeField::Body;
        self.palette
            .push_surface(f, rect.x, rect.y, rect.w, rect.h, 4.0, Surface::Card);
        f.push(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if focused {
                self.palette.blue
            } else {
                self.palette.surface1
            },
            line_width: if focused { 2.0 } else { 1.0 },
            corner_radii: CornerRadii::all(4.0),
        });
        f.hit(Target::Field(ComposeField::Body), rect);
        let area = Rect::new(
            rect.x + 8.0,
            rect.y + 4.0,
            (rect.w - 16.0).max(0.0),
            (rect.h - 8.0).max(0.0),
        );
        let origin = area.x - compose.body_hscroll;
        let body = &compose.body;
        let selection = if focused { body.selection() } else { None };
        let rows = self.body_rows();
        f.clip(area);
        let mut start = 0_usize;
        for (i, line) in body.text().split('\n').enumerate() {
            let end = start.saturating_add(line.len());
            if i >= compose.body_scroll && i < compose.body_scroll.saturating_add(rows) {
                let y = area.y + i.saturating_sub(compose.body_scroll) as f32 * BODY_LINE_H;
                if let Some((from, to)) = selection {
                    let a = from.clamp(start, end).saturating_sub(start);
                    let b = to.clamp(start, end).saturating_sub(start);
                    if a < b {
                        for (left, w) in
                            text::selection_boxes(line, a, b, BODY_TEXT, FontWeightHint::Regular)
                        {
                            f.push(RenderCommand::FillRect {
                                x: origin + left,
                                y,
                                width: w,
                                height: BODY_LINE_H,
                                color: self.palette.surface2,
                                corner_radii: CornerRadii::ZERO,
                            });
                        }
                    }
                }
                f.push(RenderCommand::Text {
                    x: origin,
                    y: y + 2.0,
                    text: line.to_owned(),
                    font_size: BODY_TEXT,
                    color: self.palette.text,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                if focused && (start..=end).contains(&body.caret()) {
                    let x = text::measure(
                        line.get(..body.caret().saturating_sub(start)).unwrap_or(""),
                        BODY_TEXT,
                        FontWeightHint::Regular,
                    );
                    f.push(RenderCommand::FillRect {
                        x: origin + x,
                        y,
                        width: textedit::CARET_WIDTH,
                        height: BODY_LINE_H,
                        color: self.palette.text,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
            start = end.saturating_add(1);
        }
        f.unclip();
    }

    fn render_status(&self, f: &mut Frame<Target>) {
        let y = self.height() - STATUS_H;
        let width = self.width();
        self.palette
            .push_surface(f, 0.0, y, width, STATUS_H, 0.0, Surface::Card);
        let total = self.current_messages().len();
        self.line(
            f,
            12.0,
            y + 6.0,
            format!(
                "{total} messages, {} unread  |  {}",
                self.unread_count, self.status_message
            ),
            11.0,
            self.palette.subtext0,
            width - 24.0,
        );
    }

    // ====================================================================
    // The pointer
    // ====================================================================

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let Some(target) = self.frame().hit_test(event.x, event.y) else {
                    return EventResult::Ignored;
                };
                let result = self.press(target, event.x, event.y);
                self.follow_selection();
                result
            }
            MouseEventKind::Move => {
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Leave => {
                if self.hover.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// A press on `target`, at `(x, y)`.
    fn press(&mut self, target: Target, x: f32, y: f32) -> EventResult {
        match target {
            Target::HelpCard => self.show_help = false,
            Target::Help => self.show_help = true,
            Target::SearchBox => {
                if self.searching {
                    return EventResult::Ignored;
                }
                self.searching = true;
            }
            Target::Compose => self.compose_new(),
            Target::Reply => {
                let Some(id) = self.selected_message else {
                    return EventResult::Ignored;
                };
                self.compose_reply(id);
            }
            Target::Forward => {
                let Some(id) = self.selected_message else {
                    return EventResult::Ignored;
                };
                self.compose_forward(id);
            }
            Target::Delete => {
                let Some(id) = self.selected_message else {
                    return EventResult::Ignored;
                };
                self.delete(id);
            }
            Target::OpenFile => self.ask(PickerFor::Open),
            Target::Refresh => {
                self.rescan();
                self.status_message = String::from("Looked in the mail folder again");
            }
            Target::Sort => self.sort_order = self.sort_order.next(),
            Target::PanePosition => self.reading_pane_position = self.reading_pane_position.next(),
            Target::Folder(i) => {
                let Some(name) = self.visible_mailboxes().get(i).map(|m| m.name.clone()) else {
                    return EventResult::Ignored;
                };
                self.searching = false;
                self.active_panel = Panel::MessageList;
                self.select_mailbox(&name);
            }
            Target::MessageList => {
                self.searching = false;
                return EventResult::Ignored;
            }
            // A press on a message opens it: reading changes nothing on disk.
            Target::Message(id) => {
                self.searching = false;
                self.open_message(id);
            }
            Target::ReadingPane => return EventResult::Ignored,
            Target::MarkUnread => {
                let Some(id) = self.selected_message else {
                    return EventResult::Ignored;
                };
                let seen = self
                    .messages
                    .iter()
                    .find(|m| m.id == id)
                    .is_some_and(|m| m.flags.seen);
                if seen {
                    self.mark_unread(id);
                } else {
                    self.mark_read(id);
                }
                self.remember(id);
            }
            Target::Flag => {
                let Some(id) = self.selected_message else {
                    return EventResult::Ignored;
                };
                self.toggle_flagged(id);
                self.remember(id);
            }
            Target::EditDraft => {
                let Some(id) = self.selected_message else {
                    return EventResult::Ignored;
                };
                self.open_message(id);
            }
            Target::SaveAttachment(i) => self.ask(PickerFor::SaveAttachment(i)),
            Target::Field(field) => {
                let rect = self.field_rect(field);
                let Some(compose) = self.compose.as_mut() else {
                    return EventResult::Ignored;
                };
                let was_focused = compose.field == field;
                compose.field = field;
                if field == ComposeField::Body {
                    let line = compose.body_scroll.saturating_add(
                        ((y - rect.y - 4.0) / BODY_LINE_H).floor().max(0.0) as usize,
                    );
                    compose
                        .body
                        .click(line, x - rect.x - 8.0 + compose.body_hscroll, false);
                } else if let Some(input) = compose.line_mut(field) {
                    // Measured against the field as it was drawn: from its
                    // start unfocused, scrolled to its caret focused.
                    let drawn = if was_focused {
                        input.cursor()
                    } else {
                        text::TextCursor::default()
                    };
                    let at = textedit::cursor_at_click(
                        input.text(),
                        drawn,
                        (rect.w - 16.0).max(0.0),
                        13.0,
                        FontWeightHint::Regular,
                        x - rect.x - 8.0,
                    );
                    input.set_selection_anchor(None);
                    input.set_cursor(at);
                }
            }
            Target::Send => self.send(),
            Target::SaveDraft => self.save_draft(),
            Target::SaveAs => self.ask(PickerFor::SaveAs),
            Target::Attach => self.ask(PickerFor::Attach),
            Target::Unattach(i) => {
                if let Some(compose) = self.compose.as_mut()
                    && i < compose.attachments.len()
                {
                    let gone = compose.attachments.remove(i);
                    compose.confirm_close = false;
                    self.status_message = format!("Took {} off", gone.filename);
                }
            }
            Target::CloseCompose => self.close_compose(),
        }
        EventResult::Consumed
    }

    /// The wheel over the list, or over the message being read or written.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        let rows = self.wheel.rows(dy);
        if rows == 0 {
            return EventResult::Ignored;
        }
        let step = |now: usize, max: usize| {
            if rows < 0 {
                now.saturating_sub(rows.unsigned_abs())
            } else {
                now.saturating_add(rows.unsigned_abs()).min(max)
            }
        };
        match self.target_at(x, y) {
            Some(Target::MessageList | Target::Message(_)) => {
                let max = self
                    .current_messages()
                    .len()
                    .saturating_sub(self.list_rows());
                let next = step(self.list_scroll, max);
                if next == self.list_scroll {
                    return EventResult::Ignored;
                }
                self.list_scroll = next;
            }
            Some(Target::ReadingPane | Target::SaveAttachment(_)) => {
                let max = self.read_lines().len().saturating_sub(self.read_rows());
                let next = step(self.read_scroll, max);
                if next == self.read_scroll {
                    return EventResult::Ignored;
                }
                self.read_scroll = next;
            }
            Some(Target::Field(ComposeField::Body)) => {
                let rows_shown = self.body_rows();
                let Some(compose) = self.compose.as_mut() else {
                    return EventResult::Ignored;
                };
                let max = compose.body.line_count().saturating_sub(rows_shown);
                let next = step(compose.body_scroll, max);
                if next == compose.body_scroll {
                    return EventResult::Ignored;
                }
                compose.body_scroll = next;
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

/// A message read from a file, as the list shows it.
fn summary_of(
    stored: &store::Stored,
    folder: &str,
    marks: store::Marks,
    own: bool,
) -> MessageSummary {
    let message = &stored.message;
    let body = message.readable_body().text;
    let preview: String = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect();
    let date = message.date.clone().unwrap_or_default();
    // The Status header an mbox writer leaves is the mark until one is set
    // here: "R" is read.
    let seen_in_file = message
        .headers
        .get("Status")
        .is_some_and(|s| s.contains('R'));
    MessageSummary {
        id: 0,
        uid: 0,
        from: message.from.clone().unwrap_or(EmailAddress {
            display_name: Some(String::from("(no sender)")),
            local_part: String::new(),
            domain: String::new(),
        }),
        to: message.to.clone(),
        cc: message.cc.clone(),
        subject: message.subject.clone(),
        timestamp: decode::parse_date(&date).map_or(0, |t| u64::try_from(t).unwrap_or(0)),
        date,
        preview,
        flags: MessageFlags {
            seen: marks.seen || seen_in_file || own,
            answered: false,
            flagged: marks.flagged,
            deleted: false,
            draft: own,
            has_attachment: !message.attachments().is_empty(),
        },
        priority: Priority::Normal,
        account_id: 0,
        mailbox: folder.to_owned(),
        message_id: message.message_id.clone(),
        in_reply_to: message.in_reply_to.clone(),
        thread_id: None,
        size: stored.size,
        labels: Vec::new(),
    }
}

/// The date a list row shows: the day and month, or the time for today --
/// the header as written when it is not a date this reads.
fn short_date(msg: &MessageSummary) -> String {
    if msg.timestamp == 0 {
        return msg.date.chars().take(16).collect();
    }
    let secs = i64::try_from(msg.timestamp).unwrap_or(0);
    let date = guitk::date::Date::from_unix_utc(secs);
    let (y, m, d) = date.ymd();
    format!("{y}-{m:02}-{d:02}")
}

/// A media type for an attached file, from its name.
fn mime_for(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "zip" => "application/zip",
        "wav" => "audio/wav",
        "eml" => "message/rfc822",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
}

/// Apply a keystroke to a one-line field, as `apps/finance` and five more
/// do (see `requests/e-c-a-text-field-that-takes-its-own-keys.md`); answers
/// what a copy or a cut took.
fn edit_line(
    input: &mut TextInput,
    key: &KeyEvent,
    capacity: usize,
    clipboard: &str,
) -> Option<String> {
    let shift = key.modifiers.shift;
    let ctrl = key.modifiers.ctrl;
    let mut copied = None;
    match key.key {
        Key::Left => input.move_cursor_left(shift, 13.0, FontWeightHint::Regular),
        Key::Right => input.move_cursor_right(shift, 13.0, FontWeightHint::Regular),
        Key::Home => input.move_home(shift),
        Key::End => input.move_end(shift),
        Key::Backspace => input.backspace(),
        Key::Delete => input.delete(),
        Key::A if ctrl => input.select_all(),
        Key::C if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
            }
        }
        Key::X if ctrl => {
            if input.has_selection() {
                copied = Some(input.selected_text().to_string());
                input.delete_selection();
            }
        }
        Key::V if ctrl => insert_limited(input, clipboard, capacity),
        _ => {
            if !ctrl {
                insert_limited(input, &key.text, capacity);
            }
        }
    }
    copied
}

/// Type `typed` into `input` over its selection, up to `capacity`
/// characters, leaving control characters out.
fn insert_limited(input: &mut TextInput, typed: &str, capacity: usize) {
    if typed.chars().all(char::is_control) {
        return;
    }
    if input.has_selection() {
        input.delete_selection();
    }
    for ch in typed.chars() {
        if ch.is_control() {
            continue;
        }
        if input.text().chars().count() >= capacity {
            break;
        }
        input.insert_char(ch);
    }
}

// ─── Main ────────────────────────────────────────────────────────────

impl App for EmailApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        // The unread count, because that is what a mail window is consulted
        // for without being raised. The harness re-reads this as the program
        // runs, so it follows the mail rather than freezing at startup.
        if self.unread_count == 0 {
            "Mail".to_string()
        } else {
            format!("Mail ({} unread)", self.unread_count)
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    /// No clock.
    ///
    /// Nothing here ages. Mail arrives over a network and this tree has none,
    /// so no message arrives while the window is open, and the files it reads
    /// are read again when asked (F5).
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.size.set((width, height));
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

impl EmailApp {
    /// An account and a handful of messages, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. `main` called it, so every launch
    /// opened on an account for `user@gmail.com` in the name of "John Doe",
    /// an inbox of messages nobody had received, and a filter rule acting on
    /// them. The account is the part worth singling out: a configured account
    /// in a mail client is read as *credentials are stored and a server was
    /// reached*, which is a claim about the user's setup and about what this
    /// program has been given.
    #[cfg(test)]
    pub fn seed_sample_mail(&mut self) {
        let acct = EmailAccount::gmail("user@gmail.com", "John Doe");
        let acct_id = self.add_account(acct);
        for msg in create_sample_messages(acct_id) {
            self.add_message(msg);
        }
        self.add_filter_rule(FilterRule {
            id: 0,
            name: "Newsletter to Archive".to_string(),
            enabled: true,
            conditions: vec![FilterCondition::SubjectContains("newsletter".to_string())],
            match_all: false,
            actions: vec![
                FilterAction::MoveTo("Archive".to_string()),
                FilterAction::MarkAsRead,
            ],
            stop_processing: true,
        });
        self.add_signature(Signature {
            id: 0,
            name: "Default".to_string(),
            text: "Best regards,\nJohn Doe\njohn@example.com".to_string(),
            is_html: false,
            is_default: true,
        });
    }
}

fn main() -> ExitCode {
    let mut app = EmailApp::from_settings();
    app::launch("email", &mut app)
}

#[cfg(test)]
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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // -- Mail as it is stored --------------------------------------------------

    /// A message whose body is Latin-1 in 8-bit reads in its own charset --
    /// `parse` takes text, and these bytes were not UTF-8 it could be given.
    #[test]
    fn an_eight_bit_latin_1_message_reads_as_written() {
        let mut raw = b"From: =?ISO-8859-1?Q?Ren=E9?= <rene@example.com>\r\n\
            Subject: =?UTF-8?B?Q2Fmw6k=?= menu\r\n\
            Content-Type: text/plain; charset=iso-8859-1\r\n\
            Content-Transfer-Encoding: 8bit\r\n\r\n"
            .to_vec();
        raw.extend_from_slice(b"Cr\xe8me br\xfbl\xe9e\r\n");
        let msg = EmailMessage::parse_bytes(&raw).unwrap();
        assert_eq!(msg.subject, "Caf\u{e9} menu");
        assert_eq!(
            msg.from.as_ref().unwrap().display_name.as_deref(),
            Some("Ren\u{e9}")
        );
        let body = msg.readable_body();
        assert_eq!(body.text.trim_end(), "Cr\u{e8}me br\u{fb}l\u{e9}e");
        assert_eq!(body.note, None);
    }

    /// Mixed within alternative within mixed: the plain text is the body, the
    /// attachment's name arrives in RFC 2231 form and its bytes decode.
    #[test]
    fn nested_parts_give_the_text_and_every_attachment() {
        let raw = "From: a@example.com\r\n\
            Subject: report\r\n\
            MIME-Version: 1.0\r\n\
            Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n\
            --outer\r\n\
            Content-Type: multipart/alternative; boundary=\"inner\"\r\n\r\n\
            --inner\r\n\
            Content-Type: text/plain; charset=utf-8\r\n\
            Content-Transfer-Encoding: quoted-printable\r\n\r\n\
            Totals attached =E2=80=94 see page 2.\r\n\
            --inner\r\n\
            Content-Type: text/html; charset=utf-8\r\n\r\n\
            <p>Totals attached</p>\r\n\
            --inner--\r\n\
            --outer\r\n\
            Content-Type: application/pdf\r\n\
            Content-Disposition: attachment; filename*=UTF-8''%E2%82%AC%20totals.pdf\r\n\
            Content-Transfer-Encoding: base64\r\n\r\n\
            JVBERi0xLjQK\r\n\
            --outer\r\n\
            Content-Type: image/png; name=\"chart.png\"\r\n\
            Content-Transfer-Encoding: base64\r\n\r\n\
            iVBORw0K\r\n\
            --outer--\r\n";
        let msg = EmailMessage::parse(raw).unwrap();
        assert_eq!(
            msg.readable_body().text.trim_end(),
            "Totals attached \u{2014} see page 2."
        );
        let attachments = msg.attachments();
        let names: Vec<&str> = attachments.iter().filter_map(|a| a.filename()).collect();
        assert_eq!(
            names,
            ["\u{20AC} totals.pdf", "chart.png"],
            "a name only in Content-Type is an attachment too"
        );
        assert_eq!(attachments[0].body, b"%PDF-1.4\n");
    }

    /// A message with only HTML is read as text, not shown as markup and not
    /// shown as nothing.
    #[test]
    fn an_html_only_message_is_read_as_text() {
        let raw = "From: a@example.com\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
            <html><body><p>Hi&nbsp;there</p><p>Two</p></body></html>";
        let msg = EmailMessage::parse(raw).unwrap();
        assert_eq!(msg.readable_body().text, "Hi\u{a0}there\n\nTwo");
        let bare = EmailMessage::parse("From: a@example.com\r\nContent-Type: image/png\r\n\r\nxx")
            .unwrap();
        assert!(bare.readable_body().text.contains("no text to show"));
    }

    /// A message written here reads back as it was written: the body, its
    /// `=` signs and its accents, the subject, the sender's name and the
    /// attachment's name and bytes.
    ///
    /// The body was declared quoted-printable and written raw, so `x=41`
    /// came back as `xA`, and a subject or a file name that was not ASCII
    /// went into the headers as raw UTF-8.
    #[test]
    fn a_message_written_here_reads_back_as_written() {
        let mut draft = EmailDraft::new(0);
        draft.to = vec![String::from("bob@example.com")];
        draft.subject = String::from("R\u{e9}sum\u{e9} for 3=3");
        draft.body = String::from("Totals: x=41, caf\u{e9}\nsecond line  ");
        draft.attachments.push(Attachment::new(
            "\u{20AC} totals.pdf",
            "application/pdf",
            b"%PDF".to_vec(),
        ));
        let from = EmailAddress {
            display_name: Some(String::from("Ren\u{e9} Dupont")),
            local_part: String::from("rene"),
            domain: String::from("example.com"),
        };
        let built = draft.build_message_at(Some(&from), Some("Fri, 25 Sep 2026 12:03:07 +0000"));
        assert!(
            built.text.is_ascii(),
            "a header or a body line is not ASCII"
        );
        let back = EmailMessage::parse(&built.text).unwrap();
        assert_eq!(back.subject, draft.subject);
        assert_eq!(
            back.from
                .as_ref()
                .and_then(|f| f.display_name.clone())
                .as_deref(),
            Some("Ren\u{e9} Dupont")
        );
        assert_eq!(
            back.date.as_deref(),
            Some("Fri, 25 Sep 2026 12:03:07 +0000")
        );
        assert_eq!(
            back.readable_body().text,
            draft.body,
            "exactly, line end and all"
        );
        let attachments = back.attachments();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename(), Some("\u{20AC} totals.pdf"));
        assert_eq!(attachments[0].body, b"%PDF");
    }

    /// A display name with a comma in quotes is one address, not two broken
    /// ones.
    #[test]
    fn a_quoted_comma_does_not_split_an_address() {
        let list = parse_address_list("\"Doe, Jane\" <jane@example.com>, bob@example.com");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].display_name.as_deref(), Some("Doe, Jane"));
        assert_eq!(list[0].address(), "jane@example.com");
        assert_eq!(list[1].address(), "bob@example.com");
    }

    /// A fresh client has no account and no mail.
    ///
    /// `main` called `seed_sample_mail`, so every launch opened on an account
    /// for `user@gmail.com` in the name of "John Doe", an inbox of messages
    /// nobody had received, and a filter rule acting on them. The account is
    /// the part worth singling out: a configured account in a mail client is
    /// read as *credentials are stored and a server was reached*.
    ///
    /// Note where the seeding lived. It was in `main`, not in `new` -- which
    /// is why this is the first application in twelve where removing the
    /// fabrication broke no tests at all. Every other one wired its fixture
    /// into the constructor, so every test got it without asking; here the
    /// tests had to call for it, and still can.
    #[test]
    fn a_fresh_client_has_no_account_and_no_mail() {
        let app = EmailApp::new();
        assert!(app.accounts.is_empty(), "the client opened on an account");
        assert!(
            app.messages.is_empty(),
            "the client opened on mail nobody received"
        );
    }

    /// And the window says why, so an empty inbox is not read as "no new mail".
    #[test]
    fn the_window_says_it_cannot_fetch_mail() {
        let app = EmailApp::new();
        let texts: Vec<String> = app
            .render_commands(WINDOW_WIDTH, WINDOW_HEIGHT)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for line in CANNOT_FETCH_LINES {
            assert!(
                texts.iter().any(|t| t == line),
                "the window never said {line:?}"
            );
        }
        assert!(
            CANNOT_FETCH_LINES
                .iter()
                .any(|l| l.contains("is not \u{201C}no new mail\u{201D}")),
            "nothing forecloses reading the empty inbox as no new mail",
        );
        assert!(
            CANNOT_FETCH_LINES.iter().any(|l| l.contains("~/Mail")),
            "nothing says where the mail it can read comes from",
        );
    }

    // ------------------------------------------------------------------
    // Wiring
    //
    // This program had no input handling. Thirty-four functions had no caller
    // outside the tests -- about twenty are IMAP and SMTP command builders
    // with no socket to write to, and the rest are the client's own verbs.
    // ------------------------------------------------------------------

    /// **The message list can be re-sorted, and the order follows.**
    ///
    /// `sort_order` was `DateDesc` at construction with no writer anywhere, so
    /// the comparator's five arms were one reachable arm and four dead ones.
    ///
    /// Asserts the order of the listed messages, not the field: a key that
    /// sets the enum while the comparator ignores it would pass the weaker
    /// version.
    #[test]
    fn the_message_list_can_be_re_sorted() {
        let mut app = seeded();
        let subjects = |app: &EmailApp| -> Vec<String> {
            app.current_messages()
                .iter()
                .map(|m| m.subject.clone())
                .collect()
        };
        let by_date = subjects(&app);
        assert!(by_date.len() > 1, "the fixture needs at least two messages");

        // Step to "subject", which orders alphabetically rather than by time.
        let mut guard = 0;
        while app.sort_order != SortOrder::SubjectAsc && guard < SortOrder::ALL.len() {
            assert_eq!(
                app.handle_event(&key_ev(Key::S, true)),
                EventResult::Consumed,
                "Ctrl+S was not answered"
            );
            guard += 1;
        }
        assert_eq!(app.sort_order, SortOrder::SubjectAsc);

        let by_subject = subjects(&app);
        assert_ne!(by_subject, by_date, "re-sorting changed nothing");
        let mut sorted = by_subject.clone();
        sorted.sort();
        assert_eq!(by_subject, sorted, "the subject order is not alphabetical");
    }

    /// **The reading pane can be moved and switched off.**
    ///
    /// `reading_pane_position` was `Right` with no writer, and it decides the
    /// list's width and whether the pane is drawn at all -- so two of its
    /// three positions were unreachable.
    #[test]
    fn the_reading_pane_can_be_moved_and_switched_off() {
        let mut app = seeded();
        assert_eq!(app.reading_pane_position, ReadingPanePosition::Right);

        assert_eq!(
            app.handle_event(&key_ev(Key::P, true)),
            EventResult::Consumed,
            "Ctrl+P was not answered"
        );
        assert_eq!(app.reading_pane_position, ReadingPanePosition::Bottom);

        app.handle_event(&key_ev(Key::P, true));
        assert_eq!(app.reading_pane_position, ReadingPanePosition::Off);

        app.handle_event(&key_ev(Key::P, true));
        assert_eq!(
            app.reading_pane_position,
            ReadingPanePosition::Right,
            "the cycle did not come back round"
        );
    }

    fn key_ev(key: Key, ctrl: bool) -> Event {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = ctrl;
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    fn press(k: Key) -> Event {
        key_ev(k, false)
    }

    fn types(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: c.to_string(),
        })
    }

    fn seeded() -> EmailApp {
        let mut app = EmailApp::new();
        app.seed_sample_mail();
        app
    }

    /// Every string the window draws, joined.
    fn drawn(app: &EmailApp) -> String {
        app.render_commands(1200.0, 800.0)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// A message has to be selected for `R`, `F`, `U`, `S` and `Delete` to
    /// have work -- all five act on the selection and are correctly refused
    /// without one, which is not the same as being unbound.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let mut app = seeded();
                app.handle_event(&press(Key::Down));
                assert_eq!(
                    app.handle_event(&Event::Key(stroke.clone())),
                    EventResult::Consumed,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The shortcut list reaches the window.**
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = seeded();
        assert!(
            !drawn(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = drawn(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        app.handle_event(&press(Key::Escape));
        assert!(
            !drawn(&app).contains("F1 closes this"),
            "Escape did not close it"
        );
    }

    /// **A key pressed behind the card does not act.**
    ///
    /// This one matters more here than in most apps: `Delete` behind the card
    /// would destroy a message the reader cannot see. The control presses the
    /// same key with the card down, so the test cannot pass on an app that has
    /// lost `Delete` altogether.
    #[test]
    fn a_key_behind_the_card_does_not_act() {
        // `delete_message` moves to Trash and only *removes* a message that is
        // already there, so the observable is the mailbox and not the count.
        // The first version of this test watched `messages.len()`, and its
        // control caught that: the card-down case changed nothing either.
        let mailbox_of = |app: &EmailApp, id: u64| {
            app.messages
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.mailbox.clone())
                .unwrap_or_default()
        };

        let mut app = seeded();
        app.handle_event(&press(Key::Down));
        let id = app.selected_message.expect("a message is selected");
        let before = mailbox_of(&app, id);
        assert_ne!(before, "Trash", "the fixture must not start in Trash");

        app.handle_event(&press(Key::F1));
        app.handle_event(&press(Key::Delete));
        assert_eq!(
            mailbox_of(&app, id),
            before,
            "Delete moved a message through the shortcut card"
        );

        app.handle_event(&press(Key::F1));
        app.handle_event(&press(Key::Delete));
        assert_eq!(
            mailbox_of(&app, id),
            "Trash",
            "control: Delete does nothing even with the card down"
        );
    }

    /// A test cannot call `main`, so the mail the window opens on lives in a
    /// method -- and a mail client that opens on an empty inbox looks broken.
    #[test]
    fn the_window_opens_with_mail_in_it() {
        let app = seeded();
        assert!(!app.accounts.is_empty(), "there should be an account");
        assert!(!app.messages.is_empty(), "and messages");
        assert!(app.unread_count > 0, "and some of them unread");
    }

    #[test]
    fn the_arrows_walk_the_message_list_and_stop_at_the_ends() {
        let mut app = seeded();
        let ids: Vec<u64> = app.current_messages().iter().map(|m| m.id).collect();
        assert!(ids.len() > 1);

        app.handle_event(&press(Key::Down));
        assert_eq!(app.selected_message, ids.first().copied());
        app.handle_event(&press(Key::Up));
        assert_eq!(
            app.selected_message,
            ids.first().copied(),
            "stopping, not wrapping to the bottom"
        );

        for _ in 0..ids.len() + 3 {
            app.handle_event(&press(Key::Down));
        }
        assert_eq!(app.selected_message, ids.last().copied());
    }

    /// Opening a message marks it read, which is the only thing that moves the
    /// unread count in the title.
    #[test]
    fn enter_reads_the_selected_message() {
        let mut app = seeded();
        let before = app.unread_count;
        assert!(before > 0);

        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.active_panel, Panel::Reading);
        assert_eq!(app.unread_count, before - 1, "the count did not move");

        // And back again: `mark_unread` had a test and no caller.
        app.handle_event(&press(Key::U));
        assert_eq!(app.unread_count, before);
    }

    #[test]
    fn s_flags_the_selected_message_and_unflags_it() {
        let mut app = seeded();
        app.handle_event(&press(Key::Down));
        let id = app.selected_message.expect("a selection");
        let flagged = |app: &EmailApp| {
            app.messages
                .iter()
                .find(|m| m.id == id)
                .is_some_and(|m| m.flags.flagged)
        };
        let before = flagged(&app);
        app.handle_event(&press(Key::S));
        assert_ne!(flagged(&app), before);
        app.handle_event(&press(Key::S));
        assert_eq!(flagged(&app), before);
    }

    #[test]
    fn delete_removes_the_message_and_moves_the_selection() {
        let mut app = seeded();
        app.handle_event(&press(Key::Down));
        let id = app.selected_message.expect("a selection");
        let before = app.current_messages().len();

        app.handle_event(&press(Key::Delete));
        assert_ne!(
            app.selected_message,
            Some(id),
            "the selection should move on"
        );
        assert!(
            app.current_messages().len() < before,
            "the message should have left the inbox"
        );
    }

    // -- composing --

    /// Reply and forward both existed, both were tested, and neither had a
    /// key: a mail client that could read mail and not answer it.
    #[test]
    fn r_replies_and_f_forwards_the_selected_message() {
        let mut app = seeded();
        app.handle_event(&press(Key::Down));
        let subject = app
            .current_messages()
            .first()
            .map(|m| m.subject.clone())
            .expect("a message");

        app.handle_event(&press(Key::R));
        assert_eq!(app.active_panel, Panel::Compose);
        let draft = app.compose.as_ref().expect("a reply draft").draft();
        assert!(
            draft.subject.contains(&subject) || draft.subject.starts_with("Re:"),
            "a reply should quote the subject, got {:?}",
            draft.subject
        );

        // Nothing typed yet: the reply is as it was made, so one Escape
        // closes it.
        app.handle_event(&press(Key::Escape));
        assert!(app.compose.is_none(), "Escape should abandon it");

        app.handle_event(&press(Key::F));
        let draft = app.compose.as_ref().expect("a forward draft").draft();
        assert!(draft.subject.starts_with("Fwd:"), "got {:?}", draft.subject);
    }

    /// The compose window takes every key, or typing a subject with a `d` in
    /// it would delete the message behind it.
    #[test]
    fn the_compose_window_swallows_the_keys_the_list_would_use() {
        let mut app = seeded();
        app.handle_event(&press(Key::Down));
        let before = app.current_messages().len();
        app.handle_event(&key_ev(Key::N, true));
        assert_eq!(app.active_panel, Panel::Compose);

        app.handle_event(&press(Key::Delete));
        assert_eq!(
            app.current_messages().len(),
            before,
            "Delete in the compose window deleted a message"
        );
    }

    #[test]
    fn tab_moves_between_the_compose_fields_and_typing_fills_them() {
        let mut app = seeded();
        app.handle_event(&key_ev(Key::N, true));

        for c in "bob@example.com".chars() {
            app.handle_event(&types(c));
        }
        // To, then Cc, then the subject, then the body.
        app.handle_event(&press(Key::Tab));
        for c in "carol@example.com".chars() {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Tab));
        for c in "Hello".chars() {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Tab));
        for c in "Body".chars() {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Enter));
        for c in "two".chars() {
            app.handle_event(&types(c));
        }

        let draft = app.compose.as_ref().expect("a draft").draft();
        assert_eq!(draft.to, ["bob@example.com"]);
        assert_eq!(draft.cc, ["carol@example.com"]);
        assert_eq!(draft.subject, "Hello");
        assert_eq!(draft.body, "Body\ntwo", "Enter is a line break in the body");
    }

    /// Ctrl+Enter checks the draft, keeps it, and does not claim to have sent it.
    ///
    /// This test used to be `ctrl_enter_sends_the_draft` and asserted
    /// `compose_draft.is_none()` -- "the draft should have gone" -- plus a
    /// status line containing "sent". Its own comment records why it was
    /// written: `build_message` had twelve tests and no caller, so it was
    /// wired to Ctrl+Enter to give the stranded builder a door.
    ///
    /// **The door led nowhere.** This client has no `std::net` and there is no
    /// SMTP client in the tree, so wiring the builder to a Send control
    /// produced a program that destroyed the user's draft and reported a
    /// delivery. Giving a finished serialiser a caller is only a repair if the
    /// caller can perform the act; otherwise it converts an unreachable
    /// function into a false claim, which is strictly worse than leaving it
    /// unreachable.
    #[test]
    fn ctrl_enter_checks_the_draft_and_keeps_it() {
        let mut app = seeded();
        app.handle_event(&key_ev(Key::N, true));
        for c in "bob@example.com".chars() {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Tab));
        for c in "Hi".chars() {
            app.handle_event(&types(c));
        }

        app.handle_event(&key_ev(Key::Enter, true));
        assert!(
            app.compose.is_some(),
            "the draft was destroyed; it is the only place this message exists"
        );
        assert_eq!(
            app.active_panel,
            Panel::Compose,
            "switching away hides the draft that was kept"
        );
        assert!(
            app.status_message.contains("was not sent"),
            "claimed a delivery it cannot perform: {:?}",
            app.status_message
        );
    }

    /// A draft with nobody to send it to is not sent, and says so.
    #[test]
    fn a_draft_with_no_recipient_is_refused() {
        let mut app = seeded();
        app.handle_event(&key_ev(Key::N, true));
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Tab));
        for c in "Subject only".chars() {
            app.handle_event(&types(c));
        }

        app.handle_event(&key_ev(Key::Enter, true));
        assert!(app.compose.is_some(), "the draft should still be open");
        assert!(
            app.status_message.contains("no recipients"),
            "got {:?}",
            app.status_message
        );
    }

    /// A recipient the builder will not accept stops the send rather than
    /// being quietly repaired into a different address. That is what
    /// `build_message` partitions for, and it had no caller to act on it.
    #[test]
    fn a_draft_with_a_bad_recipient_is_refused_and_names_it() {
        let mut app = seeded();
        app.handle_event(&key_ev(Key::N, true));
        // A header value cannot contain a newline; the builder rejects it
        // rather than folding it into a second header.
        // Typed, a line break never reaches a single-line field; this is the
        // builder's own guard, reached as a paste of stored text would.
        if let Some(compose) = app.compose.as_mut() {
            compose.to.set_text("bad\nname@example.com");
            compose.subject.set_text("Hi");
        }

        app.handle_event(&key_ev(Key::Enter, true));
        assert!(app.compose.is_some(), "it should not have been sent");
        assert!(
            app.status_message.contains("bad address"),
            "got {:?}",
            app.status_message
        );
    }

    // -- search and mailboxes --

    #[test]
    fn ctrl_f_searches_and_escape_clears_it() {
        let mut app = seeded();
        let all = app.current_messages().len();
        let needle = app
            .current_messages()
            .first()
            .map(|m| m.subject.chars().take(4).collect::<String>())
            .expect("a message");

        app.handle_event(&key_ev(Key::F, true));
        assert!(app.searching);
        for c in needle.chars() {
            app.handle_event(&types(c));
        }
        assert_eq!(app.search_query, needle);
        assert!(app.current_messages().len() <= all);

        app.handle_event(&press(Key::Escape));
        assert!(!app.searching);
        assert_eq!(
            app.current_messages().len(),
            all,
            "the search should be cleared"
        );
    }

    /// While the search box is open it takes every key, so typing a query with
    /// an `r` in it must not open a reply.
    #[test]
    fn the_search_box_swallows_the_list_keys() {
        let mut app = seeded();
        app.handle_event(&key_ev(Key::F, true));
        app.handle_event(&types('r'));
        assert!(app.compose.is_none(), "typing `r` opened a reply");
        assert_eq!(app.search_query, "r");
    }

    #[test]
    fn the_side_arrows_move_between_mailboxes() {
        let mut app = seeded();
        assert!(app.mailboxes.len() > 1);
        let before = app.selected_mailbox.clone();
        app.handle_event(&press(Key::Right));
        assert_ne!(app.selected_mailbox, before);
        app.handle_event(&press(Key::Left));
        assert_eq!(app.selected_mailbox, before, "and back again");
    }

    #[test]
    fn the_title_counts_the_unread_mail() {
        let mut app = seeded();
        let title = app.title();
        assert!(title.contains("unread"), "got {title:?}");

        // Read them all; the title should go back to plain.
        let ids: Vec<u64> = app.messages.iter().map(|m| m.id).collect();
        for id in ids {
            app.mark_read(id);
        }
        assert_eq!(app.title(), "Mail");
    }

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
        assert_eq!(
            headers.get("Subject"),
            Some("This is a very long subject line")
        );
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
        assert_eq!(
            msg.from.as_ref().unwrap().display_name.as_deref(),
            Some("Alice")
        );
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
        assert_eq!(
            SmtpCommand::ehlo("client.example.com"),
            "EHLO client.example.com\r\n"
        );
    }

    #[test]
    fn test_smtp_mail_from() {
        assert_eq!(
            SmtpCommand::mail_from("user@example.com"),
            "MAIL FROM:<user@example.com>\r\n"
        );
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
        let draft = app.compose.as_ref().unwrap().draft();
        assert!(draft.subject.starts_with("Re: "));
    }

    #[test]
    fn test_compose_forward() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        let msg = create_sample_messages(acct_id).into_iter().next().unwrap();
        let id = app.add_message(msg);
        app.compose_forward(id);
        let draft = app.compose.as_ref().unwrap().draft();
        assert!(draft.subject.starts_with("Fwd: "));
    }

    #[test]
    fn test_build_message() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.subject = "Test Subject".to_string();
        draft.body = "Hello".to_string();
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
        assert!(msg.contains("From: alice@example.com"));
        assert!(msg.contains("To: bob@example.com"));
        assert!(msg.contains("Subject: Test Subject"));
        assert!(msg.contains("Hello"));
    }

    // -- header injection ----------------------------------------------------

    /// The header block a receiving MTA would see: everything before the first
    /// empty line, with folded continuations joined as RFC 5322 requires.
    ///
    /// The tests below must count *headers*, not substrings. Correctly escaped
    /// output legitimately contains the payload -- a quoted display name
    /// contains the text `Bcc: x@evil.test` -- so `msg.contains("Bcc:")` would
    /// fail on output that is perfectly safe, and a test written that way
    /// tells you nothing about whether the defence works. This is the same
    /// trap the CSV and SQL tests hit; parse, then count.
    fn header_names(msg: &str) -> Vec<String> {
        let mut names = Vec::new();
        for line in msg.split("\r\n") {
            if line.is_empty() {
                break;
            }
            if line.starts_with(' ') || line.starts_with('\t') {
                continue; // folded continuation of the previous header
            }
            if let Some((name, _)) = line.split_once(':') {
                names.push(name.trim().to_lowercase());
            }
        }
        names
    }

    fn count_header(msg: &str, name: &str) -> usize {
        header_names(msg).iter().filter(|n| n == &name).count()
    }

    /// Split on any line terminator, not just CRLF.
    ///
    /// RFC 5322 says CRLF, but receivers are famously lenient and many treat a
    /// bare LF -- and some a bare CR -- as ending the line. A defence that only
    /// stops CRLF is not a defence, so the test that checks it must not be
    /// stricter than the attacker is.
    fn any_lines(msg: &str) -> Vec<&str> {
        msg.split(['\r', '\n']).collect()
    }

    /// Count lines *anywhere* in the message that a parser would read as the
    /// named header.
    ///
    /// Deliberately not limited to the top-level header block: a MIME part has
    /// its own headers, after the blank line that ends the main block, so a
    /// check that stops at that blank line cannot see an attachment filename
    /// forging one. An earlier version of these tests made exactly that
    /// mistake and could never have failed.
    ///
    /// Counting only line *starts* is what keeps this honest in the other
    /// direction: a correctly quoted display name legitimately contains the
    /// text `Bcc: mallory@evil.test`, and a substring search would flag it.
    fn forged_header_lines(msg: &str, name: &str) -> usize {
        let prefix = format!("{name}:");
        any_lines(msg)
            .iter()
            .filter(|l| l.to_lowercase().starts_with(&prefix))
            .count()
    }

    fn injection_draft(field: &str) -> (EmailDraft, EmailAddress) {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.subject = field.to_string();
        draft.body = "Hello".to_string();
        (draft, EmailAddress::parse("alice@example.com").unwrap())
    }

    #[test]
    fn a_subject_cannot_add_a_bcc_recipient() {
        // The one that matters most: a Bcc added this way appears nowhere in
        // the compose window or the sender's Sent copy.
        let (draft, from) = injection_draft("Lunch?\r\nBcc: mallory@evil.test");
        let msg = draft.build_message(&from).text;
        assert_eq!(
            forged_header_lines(&msg, "bcc"),
            0,
            "subject forged a Bcc:\n{msg}"
        );
        assert_eq!(count_header(&msg, "subject"), 1);
    }

    #[test]
    fn a_subject_cannot_end_the_header_block_early() {
        // A bare CRLF CRLF would terminate the headers and make the rest of
        // the subject the message body.
        let (draft, from) = injection_draft("Hi\r\n\r\nnot the real body");
        let msg = draft.build_message(&from).text;
        assert_eq!(
            count_header(&msg, "content-type"),
            1,
            "headers cut short:\n{msg}"
        );
        assert!(msg.contains("Hello"), "real body lost:\n{msg}");
    }

    #[test]
    fn a_lone_cr_or_lf_in_a_subject_is_also_neutralised() {
        // Not every MTA needs a full CRLF; a bare LF ends the line for many.
        for payload in ["a\nBcc: m@evil.test", "a\rBcc: m@evil.test"] {
            let (draft, from) = injection_draft(payload);
            let msg = draft.build_message(&from).text;
            assert_eq!(
                forged_header_lines(&msg, "bcc"),
                0,
                "{payload:?} forged a Bcc:\n{msg}"
            );
        }
    }

    #[test]
    fn an_unwritable_recipient_is_reported_not_silently_dropped() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec![
            "bob@example.com".to_string(),
            "eve@example.com\r\nBcc: mallory@evil.test".to_string(),
        ];
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let built = draft.build_message(&from);
        assert_eq!(forged_header_lines(&built.text, "bcc"), 0);
        assert_eq!(
            built.rejected_recipients.len(),
            1,
            "a dropped recipient must be reported to the sender"
        );
        assert!(built.text.contains("bob@example.com"), "good address lost");
    }

    #[test]
    fn a_display_name_cannot_forge_a_header() {
        let mut from = EmailAddress::parse("alice@example.com").unwrap();
        from.display_name = Some("Alice\r\nBcc: mallory@evil.test".to_string());
        let (draft, _) = injection_draft("Hi");
        let msg = draft.build_message(&from).text;
        assert_eq!(
            forged_header_lines(&msg, "bcc"),
            0,
            "display name forged a Bcc:\n{msg}"
        );
        assert_eq!(count_header(&msg, "from"), 1);
    }

    #[test]
    fn a_hostile_message_id_cannot_forge_a_header_on_reply() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.in_reply_to = Some("x>\r\nBcc: mallory@evil.test\r\nX: <y".to_string());
        draft.references = vec!["a>\r\nBcc: mallory@evil.test\r\n<b".to_string()];
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
        assert_eq!(
            forged_header_lines(&msg, "bcc"),
            0,
            "Message-ID forged a Bcc:\n{msg}"
        );
    }

    // -- attachment parameters ------------------------------------------------

    #[test]
    fn a_quote_in_a_filename_cannot_end_the_parameter() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.attachments.push(Attachment::new(
            "in\"voice.txt",
            "text/plain",
            b"x".to_vec(),
        ));
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
        // A quoted-string escapes an internal quote; it must not appear bare.
        for line in msg
            .split("\r\n")
            .filter(|l| l.starts_with("Content-Disposition"))
        {
            let bare_quotes = line
                .char_indices()
                .filter(|&(i, c)| c == '"' && !line.get(..i).is_some_and(|p| p.ends_with('\\')))
                .count();
            assert_eq!(bare_quotes, 2, "parameter not properly quoted: {line}");
        }
    }

    #[test]
    fn a_newline_in_a_filename_cannot_forge_a_header() {
        // Our own filesystem permits every byte but '/' and NUL in a path, so
        // this filename is legal on SlateOS rather than merely hypothetical.
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.attachments.push(Attachment::new(
            "ok.txt\r\nBcc: mallory@evil.test",
            "text/plain",
            b"x".to_vec(),
        ));
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
        assert_eq!(
            forged_header_lines(&msg, "bcc"),
            0,
            "filename forged a Bcc:\n{msg}"
        );
    }

    #[test]
    fn a_mangled_media_type_falls_back_to_octet_stream() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.attachments.push(Attachment::new(
            "x.txt",
            "text/plain\r\nBcc: mallory@evil.test",
            b"x".to_vec(),
        ));
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
        assert_eq!(
            forged_header_lines(&msg, "bcc"),
            0,
            "media type forged a Bcc:\n{msg}"
        );
        assert!(
            msg.contains("application/octet-stream"),
            "no fallback:\n{msg}"
        );
    }

    #[test]
    fn a_body_quoting_the_boundary_cannot_drop_the_attachments() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        // Exactly what forwarding a previous multipart mail puts in a body.
        draft.body = "quoted:\r\n----=_Part_Boundary_001--\r\ntrailing".to_string();
        draft
            .attachments
            .push(Attachment::new("x.txt", "text/plain", b"x".to_vec()));
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
        let boundary = msg
            .split("boundary=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("a multipart message declares its boundary");
        assert!(
            !draft.body.contains(boundary),
            "boundary {boundary} occurs in the body it delimits"
        );
        // The attachment part must still be reachable: two delimiters plus the
        // closing one.
        let delims = msg.matches(&format!("--{boundary}")).count();
        assert!(delims >= 3, "attachment part lost:\n{msg}");
    }

    #[test]
    fn test_build_message_with_attachment() {
        let mut draft = EmailDraft::new(1);
        draft.to = vec!["bob@example.com".to_string()];
        draft.subject = "With File".to_string();
        draft.body = "See attached.".to_string();
        draft.attachments.push(Attachment::new(
            "test.txt",
            "text/plain",
            b"file content".to_vec(),
        ));
        let from = EmailAddress::parse("alice@example.com").unwrap();
        let msg = draft.build_message(&from).text;
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
        assert_eq!(ImapStatus::from_code("OK"), Some(ImapStatus::Ok));
        assert_eq!(ImapStatus::from_code("BAD"), Some(ImapStatus::Bad));
        assert_eq!(ImapStatus::from_code("NOPE"), None);
    }

    #[test]
    fn test_render_produces_commands() {
        let mut app = EmailApp::new();
        let acct_id = app.add_account(EmailAccount::gmail("t@g.com", "T"));
        for msg in create_sample_messages(acct_id) {
            app.add_message(msg);
        }
        let cmds = app.render_commands(1400.0, 900.0);
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

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        use guitk::Color;

        fn fills(app: &mut EmailApp) -> Vec<Color> {
            app.render(1100.0, 750.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = EmailApp::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // -- The window over mail kept in files ------------------------------------

    use guitk::probe::{self, Probe};

    impl Probe for EmailApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }))
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
            })))
        }
    }

    /// A directory for one test, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("email-app-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a leftover temporary directory is harmless.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const INBOX: &str = "From alice@example.com Thu Sep 24 09:00:00 2026\n\
        From: Alice <alice@example.com>\n\
        To: me@example.com\n\
        Subject: Lunch\n\
        Date: Thu, 24 Sep 2026 09:00:00 +0000\n\
        Message-ID: <lunch@example.com>\n\n\
        Soup at noon?\n\n\
        From bob@example.com Fri Sep 25 10:00:00 2026\n\
        From: Bob <bob@example.com>\n\
        To: me@example.com\n\
        Subject: Report\n\
        Date: Fri, 25 Sep 2026 10:00:00 +0000\n\
        Message-ID: <report@example.com>\n\
        Status: RO\n\
        MIME-Version: 1.0\n\
        Content-Type: multipart/mixed; boundary=\"b\"\n\n\
        --b\n\
        Content-Type: text/plain; charset=utf-8\n\n\
        The totals are attached.\n\
        --b\n\
        Content-Type: text/csv\n\
        Content-Disposition: attachment; filename=\"totals.csv\"\n\
        Content-Transfer-Encoding: base64\n\n\
        YSxiCjEsMgo=\n\
        --b--\n";

    /// `~/Mail` with an mbox of two messages and a folder holding one long
    /// saved message; the marks kept beside it.
    fn mail_fixture(tag: &str) -> (Scratch, EmailApp) {
        let dir = Scratch::new(tag);
        let mail = dir.0.join("Mail");
        std::fs::create_dir_all(mail.join("Saved")).unwrap();
        std::fs::write(mail.join("Inbox.mbox"), INBOX).unwrap();
        let mut note = String::from(
            "From: Carol <carol@example.com>\nSubject: Saved note\nDate: Wed, 23 Sep 2026 08:00:00 +0000\n\n",
        );
        for i in 0..120 {
            note.push_str(&format!("Line {i} of a long saved note.\n"));
        }
        std::fs::write(mail.join("Saved").join("note.eml"), note).unwrap();
        let mut app = EmailApp::new();
        app.mail_dir = Some(mail);
        app.flags_path = Some(dir.0.join("flags.txt"));
        app.rescan();
        (dir, app)
    }

    fn message_id(app: &EmailApp, subject: &str) -> u64 {
        app.messages
            .iter()
            .find(|m| m.subject == subject)
            .unwrap_or_else(|| panic!("no message {subject:?}"))
            .id
    }

    /// The mail directory's folders are listed and the first read in; a
    /// message's Status header is its read mark until one is set here, and a
    /// mark set here is kept -- in a file of the client's own, never in the
    /// mail, which is not rewritten.
    #[test]
    fn a_mail_folder_is_read_and_its_marks_are_kept() {
        let (dir, mut app) = mail_fixture("marks");
        let names: Vec<&str> = app.mailboxes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Inbox", "Saved", "Drafts"]);
        assert_eq!(app.selected_mailbox.as_deref(), Some("Inbox"));
        assert_eq!(app.current_messages().len(), 2);
        assert_eq!(
            app.unread_count, 1,
            "Report is marked read by its Status header"
        );
        let before = std::fs::read(dir.0.join("Mail").join("Inbox.mbox")).unwrap();
        let lunch = message_id(&app, "Lunch");
        probe::click(&mut app, Target::Message(lunch));
        assert_eq!(app.active_panel, Panel::Reading);
        assert_eq!(app.unread_count, 0);
        app.handle_event(&press(Key::S));
        assert!(
            app.messages
                .iter()
                .find(|m| m.id == lunch)
                .unwrap()
                .flags
                .flagged
        );
        assert_eq!(
            std::fs::read(dir.0.join("Mail").join("Inbox.mbox")).unwrap(),
            before,
            "the mail was rewritten"
        );
        let kept = std::fs::read_to_string(dir.0.join("flags.txt")).unwrap();
        assert!(kept.contains("11\tid:lunch@example.com"), "{kept}");
        // A new window finds them.
        let mut again = EmailApp::new();
        again.mail_dir = app.mail_dir.clone();
        again.flags = store::Flags::parse(&kept).unwrap();
        again.rescan();
        let lunch = message_id(&again, "Lunch");
        let m = again.messages.iter().find(|m| m.id == lunch).unwrap();
        assert!(m.flags.seen && m.flags.flagged);
    }

    /// A message is read whole -- not the one-line preview it was -- and its
    /// attachment saved where the dialog says.
    #[test]
    fn a_message_is_read_whole_and_its_attachment_saved() {
        let (dir, mut app) = mail_fixture("read");
        let report = message_id(&app, "Report");
        probe::click(&mut app, Target::Message(report));
        let text = drawn(&app);
        assert!(text.contains("The totals are attached."), "{text}");
        assert!(text.contains("totals.csv"), "the attachment is not listed");
        probe::click(&mut app, Target::SaveAttachment(0));
        assert!(app.picker.is_open() && app.picker_for == PickerFor::SaveAttachment(0));
        app.picker.close();
        let to = dir.0.join("saved.csv");
        app.picked(&to);
        assert_eq!(std::fs::read(&to).unwrap(), b"a,b\n1,2\n");
        // A long message scrolls, by key and by wheel.
        app.select_mailbox("Saved");
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&press(Key::PageDown));
        assert!(app.read_scroll > 0);
        let after_key = app.read_scroll;
        probe::scroll_at_point(&mut app, Target::ReadingPane, -3.0);
        assert!(app.read_scroll > after_key);
        // A reply quotes the whole of it, not the line the list shows.
        app.handle_event(&press(Key::R));
        let quoted = app.compose.as_ref().unwrap().body.text().to_owned();
        assert!(
            quoted.contains("> Line 119 of a long saved note."),
            "{quoted}"
        );
    }

    /// Mail read from files is never deleted here, and the window says so.
    #[test]
    fn mail_read_from_files_is_never_deleted() {
        let (dir, mut app) = mail_fixture("keep");
        let before = std::fs::read(dir.0.join("Mail").join("Inbox.mbox")).unwrap();
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Delete));
        app.handle_event(&press(Key::Delete));
        assert!(
            app.status_message.contains("never changed"),
            "{}",
            app.status_message
        );
        assert_eq!(app.current_messages().len(), 2);
        assert_eq!(
            std::fs::read(dir.0.join("Mail").join("Inbox.mbox")).unwrap(),
            before
        );
    }

    fn type_into(app: &mut EmailApp, text: &str) {
        for c in text.chars() {
            app.handle_event(&types(c));
        }
    }

    /// A draft is saved into Drafts, listed there, opened to go on writing,
    /// saved over its own file, and deleted after a second Delete.
    #[test]
    fn a_draft_is_saved_edited_and_deleted() {
        let (dir, mut app) = mail_fixture("draft");
        app.handle_event(&key_ev(Key::N, true));
        type_into(&mut app, "bob@example.com");
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Tab));
        type_into(&mut app, "Plans");
        app.handle_event(&press(Key::Tab));
        type_into(&mut app, "First line");
        app.handle_event(&key_ev(Key::S, true));
        let file = dir.0.join("Mail").join("Drafts").join("Plans.eml");
        let saved = EmailMessage::parse_bytes(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved.subject, "Plans");
        assert_eq!(saved.readable_body().text, "First line");
        assert_eq!(
            app.compose.as_ref().unwrap().saved_as.as_deref(),
            Some(file.as_path())
        );
        // Saved: one Escape closes it.
        app.handle_event(&press(Key::Escape));
        assert!(app.compose.is_none());
        app.select_mailbox("Drafts");
        assert_eq!(app.current_messages().len(), 1);
        let draft = app.current_messages()[0].id;
        app.handle_event(&press(Key::Enter));
        let compose = app
            .compose
            .as_ref()
            .expect("the draft did not open to be written");
        assert_eq!(compose.subject.text(), "Plans");
        assert_eq!(compose.body.text(), "First line");
        if let Some(compose) = app.compose.as_mut() {
            compose.field = ComposeField::Body;
        }
        type_into(&mut app, ", more");
        app.handle_event(&key_ev(Key::S, true));
        let names: Vec<_> = std::fs::read_dir(dir.0.join("Mail").join("Drafts"))
            .unwrap()
            .collect();
        assert_eq!(names.len(), 1, "saving again made a second file");
        let saved = EmailMessage::parse_bytes(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved.readable_body().text, "First line, more");
        app.handle_event(&press(Key::Escape));
        let draft = app.current_messages().first().map_or(draft, |m| m.id);
        app.selected_message = Some(draft);
        app.handle_event(&press(Key::Delete));
        assert!(file.exists(), "one Delete deleted it");
        app.handle_event(&press(Key::Delete));
        assert!(!file.exists());
        assert!(app.current_messages().is_empty());
    }

    /// Closing over unsaved work asks first; a second close discards.
    #[test]
    fn closing_unsaved_work_asks_first() {
        let mut app = EmailApp::new();
        app.handle_event(&key_ev(Key::N, true));
        type_into(&mut app, "x@example.com");
        app.handle_event(&press(Key::Escape));
        assert!(app.compose.is_some(), "unsaved work was discarded at once");
        assert!(
            app.status_message.contains("not saved"),
            "{}",
            app.status_message
        );
        probe::click(&mut app, Target::CloseCompose);
        assert!(app.compose.is_none());
    }

    /// Save as writes an .eml file that reads back as the message written,
    /// with a Date and the attachment it was given.
    #[test]
    fn save_as_writes_a_message_that_reads_back() {
        let dir = Scratch::new("saveas");
        let attached = dir.0.join("data.txt");
        std::fs::write(&attached, b"payload").unwrap();
        let mut app = EmailApp::new();
        app.handle_event(&key_ev(Key::N, true));
        if let Some(compose) = app.compose.as_mut() {
            compose.from.set_text("Me <me@example.com>");
            compose.to.set_text("you@example.com");
            compose.subject.set_text("Caf\u{e9}");
            compose.body.set_text("Hello");
        }
        app.picker_for = PickerFor::Attach;
        app.picked(&attached);
        assert_eq!(app.compose.as_ref().unwrap().attachments.len(), 1);
        app.picker_for = PickerFor::SaveAs;
        let out = dir.0.join("message.eml");
        app.picked(&out);
        let back = EmailMessage::parse_bytes(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(back.subject, "Caf\u{e9}");
        assert_eq!(back.readable_body().text, "Hello");
        assert!(back.date.is_some(), "no Date header");
        assert_eq!(back.attachments()[0].body, b"payload");
        assert_eq!(
            app.identity, "Me <me@example.com>",
            "the next message is not from the same sender"
        );
    }

    /// A message file or an mbox from anywhere is opened, into a folder of
    /// its own.
    #[test]
    fn a_file_is_opened_from_anywhere() {
        let dir = Scratch::new("open");
        let file = dir.0.join("forwarded.eml");
        std::fs::write(&file, "From: d@example.com\nSubject: From elsewhere\n\nHi").unwrap();
        let mut app = EmailApp::new();
        app.handle_event(&key_ev(Key::O, true));
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Open);
        app.picker.close();
        app.picked(&file);
        assert_eq!(app.selected_mailbox.as_deref(), Some("Opened"));
        let subjects: Vec<&str> = app
            .current_messages()
            .iter()
            .map(|m| m.subject.as_str())
            .collect();
        assert_eq!(subjects, ["From elsewhere"]);
    }

    /// The reading pane is drawn beside, under, or in place of the list;
    /// "below" drew nothing at all.
    #[test]
    fn the_reading_pane_is_drawn_wherever_it_is() {
        let (_dir, mut app) = mail_fixture("pane");
        let lunch = message_id(&app, "Lunch");
        app.open_message(lunch);
        for position in ReadingPanePosition::ALL {
            app.reading_pane_position = position;
            assert!(
                drawn(&app).contains("Soup at noon?"),
                "{position:?} did not show the message"
            );
        }
        app.reading_pane_position = ReadingPanePosition::Off;
        assert!(
            probe::rect_of(&app, Target::Message(lunch)).is_none(),
            "with the pane off, the message is read in the list's place"
        );
        app.handle_event(&press(Key::Escape));
        assert!(
            probe::rect_of(&app, Target::ReadingPane).is_none(),
            "with the pane off, Escape goes back to the list alone"
        );
        assert!(probe::rect_of(&app, Target::Message(lunch)).is_some());
    }

    /// Every control answers the pointer, in the window and in the form.
    #[test]
    fn every_control_answers_the_pointer() {
        let (_dir, mut app) = mail_fixture("pointer");
        probe::click(&mut app, Target::Folder(1));
        assert_eq!(app.selected_mailbox.as_deref(), Some("Saved"));
        probe::click(&mut app, Target::Folder(0));
        let report = message_id(&app, "Report");
        probe::click(&mut app, Target::Message(report));
        assert_eq!(app.selected_message, Some(report));
        probe::click(&mut app, Target::Flag);
        assert!(
            app.messages
                .iter()
                .find(|m| m.id == report)
                .unwrap()
                .flags
                .flagged
        );
        probe::click(&mut app, Target::MarkUnread);
        assert!(
            !app.messages
                .iter()
                .find(|m| m.id == report)
                .unwrap()
                .flags
                .seen
        );
        let sort = app.sort_order;
        probe::click(&mut app, Target::Sort);
        assert_ne!(app.sort_order, sort);
        let pane = app.reading_pane_position;
        probe::click(&mut app, Target::PanePosition);
        assert_ne!(app.reading_pane_position, pane);
        app.reading_pane_position = ReadingPanePosition::Right;
        probe::click(&mut app, Target::SearchBox);
        assert!(app.searching);
        app.handle_event(&press(Key::Escape));
        probe::click(&mut app, Target::OpenFile);
        assert!(app.picker.is_open());
        app.picker.close();
        assert_eq!(
            probe::click(&mut app, Target::Refresh),
            EventResult::Consumed
        );
        probe::click(&mut app, Target::Help);
        assert!(app.show_help);
        probe::click(&mut app, Target::HelpCard);
        let report = message_id(&app, "Report");
        probe::click(&mut app, Target::Message(report));
        probe::click(&mut app, Target::Reply);
        let compose = app.compose.as_ref().expect("Reply did not open the form");
        assert!(
            compose.body.text().contains("> The totals are attached."),
            "the reply does not quote the message"
        );
        probe::click(&mut app, Target::Field(ComposeField::Subject));
        assert_eq!(app.compose.as_ref().unwrap().field, ComposeField::Subject);
        probe::click(&mut app, Target::Field(ComposeField::Body));
        assert_eq!(app.compose.as_ref().unwrap().field, ComposeField::Body);
        probe::click(&mut app, Target::Send);
        assert!(
            app.status_message.contains("was not sent"),
            "{}",
            app.status_message
        );
        probe::click(&mut app, Target::Attach);
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Attach);
        app.picker.close();
        probe::click(&mut app, Target::SaveAs);
        assert!(app.picker.is_open() && app.picker_for == PickerFor::SaveAs);
        app.picker.close();
        probe::click(&mut app, Target::SaveDraft);
        assert!(
            app.status_message.contains("Draft saved"),
            "{}",
            app.status_message
        );
        probe::click(&mut app, Target::CloseCompose);
        assert!(app.compose.is_none(), "a saved draft closes at once");
        let report = message_id(&app, "Report");
        probe::click(&mut app, Target::Message(report));
        probe::click(&mut app, Target::Forward);
        assert_eq!(
            app.compose.as_ref().unwrap().attachments.len(),
            1,
            "the attachment was not forwarded"
        );
        probe::click(&mut app, Target::Unattach(0));
        assert!(app.compose.as_ref().unwrap().attachments.is_empty());
    }

    /// The list scrolls under the wheel and follows the keys.
    #[test]
    fn the_message_list_scrolls_and_follows_the_keys() {
        let dir = Scratch::new("scroll");
        let mail = dir.0.join("Mail");
        std::fs::create_dir_all(&mail).unwrap();
        let mut mbox = String::new();
        for i in 0..40 {
            mbox.push_str(&format!(
                "From x@example.com Thu Sep 24 09:{:02}:00 2026\nFrom: x@example.com\nSubject: Message {i}\nDate: Thu, 24 Sep 2026 09:{:02}:00 +0000\n\nbody {i}\n\n",
                i % 60,
                i % 60
            ));
        }
        std::fs::write(mail.join("Inbox.mbox"), mbox).unwrap();
        let mut app = EmailApp::new();
        app.mail_dir = Some(mail);
        app.rescan();
        assert_eq!(app.current_messages().len(), 40);
        let rows = app.list_rows();
        assert!(rows < 40);
        for _ in 0..30 {
            probe::scroll_at_point(&mut app, Target::MessageList, -3.0);
        }
        assert_eq!(app.list_scroll, 40 - rows);
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Up));
        let first = app.current_messages()[0].id;
        assert_eq!(app.selected_message, Some(first));
        assert_eq!(app.list_scroll, 0, "the choice was left off screen");
    }
}
