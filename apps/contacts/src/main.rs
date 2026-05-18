//! OurOS Contacts / Address Book
//!
//! A full-featured contacts manager with:
//! - Contact CRUD (create, read, update, delete)
//! - Groups/categories with color coding
//! - Multi-field search (name, phone, email, company, notes)
//! - Favorites (starred contacts shown at top)
//! - Birthday reminders
//! - Duplicate detection and merge
//! - Sort by name, company, recently added, recently contacted
//! - Filter by group, has phone, has email
//! - vCard 3.0 import/export
//! - Alphabet sidebar for quick navigation
//! - Recently viewed contacts tracking
//! - Quick actions (call, email, map -- stubs for future IPC)
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha theme.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::render::{FontWeightHint, RenderCommand};
#[allow(unused_imports)]
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

#[allow(dead_code)]
const BASE: Color = Color::from_hex(0x1E1E2E);
#[allow(dead_code)]
const MANTLE: Color = Color::from_hex(0x181825);
#[allow(dead_code)]
const SURFACE0: Color = Color::from_hex(0x313244);
#[allow(dead_code)]
const SURFACE1: Color = Color::from_hex(0x45475A);
#[allow(dead_code)]
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
#[allow(dead_code)]
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
#[allow(dead_code)]
const BLUE: Color = Color::from_hex(0x89B4FA);
#[allow(dead_code)]
const GREEN: Color = Color::from_hex(0xA6E3A1);
#[allow(dead_code)]
const RED: Color = Color::from_hex(0xF38BA8);
#[allow(dead_code)]
const YELLOW: Color = Color::from_hex(0xF9E2AF);
#[allow(dead_code)]
const PEACH: Color = Color::from_hex(0xFAB387);
#[allow(dead_code)]
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
#[allow(dead_code)]
const OVERLAY0: Color = Color::from_hex(0x6C7086);
#[allow(dead_code)]
const CRUST: Color = Color::from_hex(0x11111B);
#[allow(dead_code)]
const MAUVE: Color = Color::from_hex(0xCBA6F7);
#[allow(dead_code)]
const TEAL: Color = Color::from_hex(0x94E2D5);
#[allow(dead_code)]
const PINK: Color = Color::from_hex(0xF5C2E7);
#[allow(dead_code)]
const ROSEWATER: Color = Color::from_hex(0xF5E0DC);

// ============================================================================
// Constants
// ============================================================================

const SIDEBAR_WIDTH: f32 = 280.0;
const ALPHABET_BAR_WIDTH: f32 = 24.0;
const HEADER_HEIGHT: f32 = 56.0;
const CONTACT_ROW_HEIGHT: f32 = 52.0;
const LETTER_DIVIDER_HEIGHT: f32 = 28.0;
const DETAIL_PADDING: f32 = 24.0;
const AVATAR_SIZE: f32 = 72.0;
const FIELD_HEIGHT: f32 = 36.0;
const SEARCH_BAR_HEIGHT: f32 = 40.0;
const GROUP_CHIP_HEIGHT: f32 = 28.0;
const MAX_RECENT: usize = 10;

const ALPHABET: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

// ============================================================================
// Phone types
// ============================================================================

/// Type of phone number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhoneType {
    Mobile,
    Home,
    Work,
    Fax,
    Other,
}

impl PhoneType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mobile => "Mobile",
            Self::Home => "Home",
            Self::Work => "Work",
            Self::Fax => "Fax",
            Self::Other => "Other",
        }
    }

    pub fn from_vcard(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("cell") {
            Self::Mobile
        } else if lower.contains("home") {
            Self::Home
        } else if lower.contains("work") {
            Self::Work
        } else if lower.contains("fax") {
            Self::Fax
        } else {
            Self::Other
        }
    }

    pub fn to_vcard(self) -> &'static str {
        match self {
            Self::Mobile => "CELL",
            Self::Home => "HOME",
            Self::Work => "WORK",
            Self::Fax => "FAX",
            Self::Other => "OTHER",
        }
    }
}

// ============================================================================
// Email types
// ============================================================================

/// Type of email address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmailType {
    Personal,
    Work,
    Other,
}

impl EmailType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Personal => "Personal",
            Self::Work => "Work",
            Self::Other => "Other",
        }
    }

    pub fn from_vcard(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("home") {
            Self::Personal
        } else if lower.contains("work") {
            Self::Work
        } else {
            Self::Other
        }
    }

    pub fn to_vcard(self) -> &'static str {
        match self {
            Self::Personal => "HOME",
            Self::Work => "WORK",
            Self::Other => "OTHER",
        }
    }
}

// ============================================================================
// Address types
// ============================================================================

/// Type of postal address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressType {
    Home,
    Work,
    Other,
}

impl AddressType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Work => "Work",
            Self::Other => "Other",
        }
    }

    pub fn from_vcard(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("home") {
            Self::Home
        } else if lower.contains("work") {
            Self::Work
        } else {
            Self::Other
        }
    }

    pub fn to_vcard(self) -> &'static str {
        match self {
            Self::Home => "HOME",
            Self::Work => "WORK",
            Self::Other => "OTHER",
        }
    }
}

// ============================================================================
// Social platform
// ============================================================================

/// Social media platform.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SocialPlatform {
    Twitter,
    LinkedIn,
    GitHub,
    Mastodon,
    Custom(String),
}

impl SocialPlatform {
    pub fn label(&self) -> &str {
        match self {
            Self::Twitter => "Twitter",
            Self::LinkedIn => "LinkedIn",
            Self::GitHub => "GitHub",
            Self::Mastodon => "Mastodon",
            Self::Custom(name) => name.as_str(),
        }
    }
}

// ============================================================================
// Phone number
// ============================================================================

/// A phone number with type and primary flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhoneNumber {
    pub number: String,
    pub phone_type: PhoneType,
    pub primary: bool,
}

impl PhoneNumber {
    pub fn new(number: &str, phone_type: PhoneType) -> Self {
        Self {
            number: number.to_string(),
            phone_type,
            primary: false,
        }
    }

    pub fn with_primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }
}

// ============================================================================
// Email address
// ============================================================================

/// An email address with type and primary flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmailAddress {
    pub email: String,
    pub email_type: EmailType,
    pub primary: bool,
}

impl EmailAddress {
    pub fn new(email: &str, email_type: EmailType) -> Self {
        Self {
            email: email.to_string(),
            email_type,
            primary: false,
        }
    }

    pub fn with_primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }
}

// ============================================================================
// Postal address
// ============================================================================

/// A postal / mailing address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostalAddress {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub address_type: AddressType,
}

impl PostalAddress {
    pub fn new(address_type: AddressType) -> Self {
        Self {
            street: String::new(),
            city: String::new(),
            state: String::new(),
            zip: String::new(),
            country: String::new(),
            address_type,
        }
    }

    /// Format as a single-line display string.
    pub fn display_line(&self) -> String {
        let parts: Vec<&str> = [
            self.street.as_str(),
            self.city.as_str(),
            self.state.as_str(),
            self.zip.as_str(),
            self.country.as_str(),
        ]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect();
        parts.join(", ")
    }

    /// Check if the address has any content.
    pub fn is_empty(&self) -> bool {
        self.street.is_empty()
            && self.city.is_empty()
            && self.state.is_empty()
            && self.zip.is_empty()
            && self.country.is_empty()
    }

    /// Format for vCard ADR field: PO;ext;street;city;state;zip;country
    pub fn to_vcard_adr(&self) -> String {
        format!(
            ";;{};{};{};{};{}",
            vcard_escape(&self.street),
            vcard_escape(&self.city),
            vcard_escape(&self.state),
            vcard_escape(&self.zip),
            vcard_escape(&self.country),
        )
    }

    /// Parse from vCard ADR value.
    pub fn from_vcard_adr(value: &str) -> Self {
        let parts: Vec<&str> = value.split(';').collect();
        Self {
            street: parts.get(2).map_or(String::new(), |s| vcard_unescape(s)),
            city: parts.get(3).map_or(String::new(), |s| vcard_unescape(s)),
            state: parts.get(4).map_or(String::new(), |s| vcard_unescape(s)),
            zip: parts.get(5).map_or(String::new(), |s| vcard_unescape(s)),
            country: parts.get(6).map_or(String::new(), |s| vcard_unescape(s)),
            address_type: AddressType::Home,
        }
    }
}

// ============================================================================
// Social account
// ============================================================================

/// A social media account link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocialAccount {
    pub platform: SocialPlatform,
    pub handle: String,
}

impl SocialAccount {
    pub fn new(platform: SocialPlatform, handle: &str) -> Self {
        Self {
            platform,
            handle: handle.to_string(),
        }
    }
}

// ============================================================================
// Contact group
// ============================================================================

/// A group / category for organizing contacts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactGroup {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub color: Color,
    pub member_count: usize,
}

impl ContactGroup {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            description: String::new(),
            color: BLUE,
            member_count: 0,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }
}

// ============================================================================
// Birthday
// ============================================================================

/// A simple date representation for birthdays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl SimpleDate {
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if month < 1 || month > 12 || day < 1 || day > 31 {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn format_display(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Parse from ISO date string (YYYY-MM-DD).
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 {
            return None;
        }
        let year = parts.first()?.parse::<u16>().ok()?;
        let month = parts.get(1)?.parse::<u8>().ok()?;
        let day = parts.get(2)?.parse::<u8>().ok()?;
        Self::new(year, month, day)
    }

    /// Check if this birthday is "upcoming" within the given number of days
    /// from the reference date. Since we have no real clock, this is a
    /// structural placeholder -- callers supply today's month/day.
    pub fn is_upcoming_within(&self, today_month: u8, today_day: u8, days: u16) -> bool {
        // Simple approach: compute day-of-year for both and compare distance.
        let bday_doy = day_of_year(self.month, self.day);
        let today_doy = day_of_year(today_month, today_day);
        let diff = if bday_doy >= today_doy {
            bday_doy.wrapping_sub(today_doy)
        } else {
            365u16.saturating_sub(today_doy).saturating_add(bday_doy)
        };
        diff <= days
    }
}

/// Approximate day of year (ignoring leap year -- good enough for birthday proximity).
fn day_of_year(month: u8, day: u8) -> u16 {
    let days_before: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = (month.saturating_sub(1) as usize).min(11);
    days_before[m].saturating_add(u16::from(day))
}

// ============================================================================
// Contact
// ============================================================================

/// A single contact entry.
#[derive(Clone, Debug)]
pub struct Contact {
    pub id: u64,
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub nickname: String,
    pub company: String,
    pub job_title: String,
    pub department: String,
    pub phones: Vec<PhoneNumber>,
    pub emails: Vec<EmailAddress>,
    pub addresses: Vec<PostalAddress>,
    pub social_accounts: Vec<SocialAccount>,
    pub birthday: Option<SimpleDate>,
    pub notes: String,
    pub photo_path: Option<String>,
    pub groups: Vec<u64>,
    pub favorite: bool,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_contacted: Option<u64>,
}

impl Contact {
    /// Create a new contact with the given name and auto-generated ID.
    pub fn new(id: u64, first_name: &str, last_name: &str) -> Self {
        let display = if last_name.is_empty() {
            first_name.to_string()
        } else if first_name.is_empty() {
            last_name.to_string()
        } else {
            format!("{first_name} {last_name}")
        };
        Self {
            id,
            first_name: first_name.to_string(),
            last_name: last_name.to_string(),
            display_name: display,
            nickname: String::new(),
            company: String::new(),
            job_title: String::new(),
            department: String::new(),
            phones: Vec::new(),
            emails: Vec::new(),
            addresses: Vec::new(),
            social_accounts: Vec::new(),
            birthday: None,
            notes: String::new(),
            photo_path: None,
            groups: Vec::new(),
            favorite: false,
            created_at: 0,
            updated_at: 0,
            last_contacted: None,
        }
    }

    /// Compute a display name from first/last/company.
    pub fn computed_display_name(&self) -> String {
        if !self.display_name.is_empty() {
            return self.display_name.clone();
        }
        if !self.first_name.is_empty() || !self.last_name.is_empty() {
            let mut s = String::new();
            if !self.first_name.is_empty() {
                s.push_str(&self.first_name);
            }
            if !self.last_name.is_empty() {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&self.last_name);
            }
            return s;
        }
        if !self.company.is_empty() {
            return self.company.clone();
        }
        String::from("(unnamed)")
    }

    /// Sort key: last name, then first name, both lowercased.
    pub fn sort_key_name(&self) -> String {
        let last = self.last_name.to_lowercase();
        let first = self.first_name.to_lowercase();
        if last.is_empty() {
            first
        } else if first.is_empty() {
            last
        } else {
            format!("{last} {first}")
        }
    }

    /// Get the first letter of the contact for alphabet grouping.
    pub fn first_letter(&self) -> char {
        let key = self.sort_key_name();
        key.chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .filter(|c| c.is_ascii_alphabetic())
            .unwrap_or('#')
    }

    /// Get the primary phone number, or the first one.
    pub fn primary_phone(&self) -> Option<&PhoneNumber> {
        self.phones
            .iter()
            .find(|p| p.primary)
            .or_else(|| self.phones.first())
    }

    /// Get the primary email, or the first one.
    pub fn primary_email(&self) -> Option<&EmailAddress> {
        self.emails
            .iter()
            .find(|e| e.primary)
            .or_else(|| self.emails.first())
    }

    /// Get initials for avatar display (up to 2 chars).
    pub fn initials(&self) -> String {
        let mut result = String::new();
        if let Some(c) = self.first_name.chars().next() {
            result.push(c.to_ascii_uppercase());
        }
        if let Some(c) = self.last_name.chars().next() {
            result.push(c.to_ascii_uppercase());
        }
        if result.is_empty() {
            if let Some(c) = self.company.chars().next() {
                result.push(c.to_ascii_uppercase());
            }
        }
        if result.is_empty() {
            result.push('?');
        }
        result
    }

    /// Check if this contact matches a search query (case-insensitive).
    pub fn matches_search(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        let fields = [
            &self.first_name,
            &self.last_name,
            &self.display_name,
            &self.nickname,
            &self.company,
            &self.job_title,
            &self.department,
            &self.notes,
        ];
        for field in &fields {
            if field.to_lowercase().contains(&q) {
                return true;
            }
        }
        for phone in &self.phones {
            if phone.number.to_lowercase().contains(&q) {
                return true;
            }
        }
        for email in &self.emails {
            if email.email.to_lowercase().contains(&q) {
                return true;
            }
        }
        false
    }

    /// Export this contact as a vCard 3.0 string.
    pub fn to_vcard(&self) -> String {
        let mut lines = Vec::new();
        lines.push(String::from("BEGIN:VCARD"));
        lines.push(String::from("VERSION:3.0"));
        lines.push(format!(
            "N:{};{};;;",
            vcard_escape(&self.last_name),
            vcard_escape(&self.first_name)
        ));
        lines.push(format!("FN:{}", vcard_escape(&self.computed_display_name())));

        if !self.nickname.is_empty() {
            lines.push(format!("NICKNAME:{}", vcard_escape(&self.nickname)));
        }
        if !self.company.is_empty() {
            lines.push(format!(
                "ORG:{};{}",
                vcard_escape(&self.company),
                vcard_escape(&self.department)
            ));
        }
        if !self.job_title.is_empty() {
            lines.push(format!("TITLE:{}", vcard_escape(&self.job_title)));
        }

        for phone in &self.phones {
            let pref = if phone.primary { ";PREF" } else { "" };
            lines.push(format!(
                "TEL;TYPE={}{pref}:{}",
                phone.phone_type.to_vcard(),
                vcard_escape(&phone.number)
            ));
        }
        for email in &self.emails {
            let pref = if email.primary { ";PREF" } else { "" };
            lines.push(format!(
                "EMAIL;TYPE={}{pref}:{}",
                email.email_type.to_vcard(),
                vcard_escape(&email.email)
            ));
        }
        for addr in &self.addresses {
            lines.push(format!(
                "ADR;TYPE={}:{}",
                addr.address_type.to_vcard(),
                addr.to_vcard_adr()
            ));
        }

        if let Some(ref bday) = self.birthday {
            lines.push(format!("BDAY:{}", bday.format_display()));
        }
        if !self.notes.is_empty() {
            lines.push(format!("NOTE:{}", vcard_escape(&self.notes)));
        }
        for social in &self.social_accounts {
            lines.push(format!(
                "X-SOCIALPROFILE;TYPE={}:{}",
                social.platform.label(),
                vcard_escape(&social.handle)
            ));
        }

        lines.push(String::from("END:VCARD"));
        lines.join("\r\n")
    }

    /// Parse a contact from a vCard 3.0 string. Returns None if parsing fails.
    pub fn from_vcard(data: &str, id: u64) -> Option<Self> {
        let lines = unfold_vcard_lines(data);

        let mut contact = Contact::new(id, "", "");
        let mut found_begin = false;
        let mut found_end = false;

        for line in &lines {
            let line = line.trim();
            if line.eq_ignore_ascii_case("BEGIN:VCARD") {
                found_begin = true;
                continue;
            }
            if line.eq_ignore_ascii_case("END:VCARD") {
                found_end = true;
                break;
            }
            if !found_begin {
                continue;
            }

            if let Some((prop, value)) = split_vcard_line(line) {
                let prop_upper = prop.to_uppercase();
                let prop_name = prop_upper.split(';').next().unwrap_or("");

                match prop_name {
                    "N" => {
                        let parts: Vec<&str> = value.split(';').collect();
                        if let Some(ln) = parts.first() {
                            contact.last_name = vcard_unescape(ln);
                        }
                        if let Some(fn_) = parts.get(1) {
                            contact.first_name = vcard_unescape(fn_);
                        }
                    }
                    "FN" => {
                        contact.display_name = vcard_unescape(value);
                    }
                    "NICKNAME" => {
                        contact.nickname = vcard_unescape(value);
                    }
                    "ORG" => {
                        let parts: Vec<&str> = value.split(';').collect();
                        if let Some(org) = parts.first() {
                            contact.company = vcard_unescape(org);
                        }
                        if let Some(dept) = parts.get(1) {
                            contact.department = vcard_unescape(dept);
                        }
                    }
                    "TITLE" => {
                        contact.job_title = vcard_unescape(value);
                    }
                    "TEL" => {
                        let ptype = PhoneType::from_vcard(&prop_upper);
                        let primary = prop_upper.contains("PREF");
                        contact
                            .phones
                            .push(PhoneNumber::new(&vcard_unescape(value), ptype).with_primary(primary));
                    }
                    "EMAIL" => {
                        let etype = EmailType::from_vcard(&prop_upper);
                        let primary = prop_upper.contains("PREF");
                        contact
                            .emails
                            .push(EmailAddress::new(&vcard_unescape(value), etype).with_primary(primary));
                    }
                    "ADR" => {
                        let atype = AddressType::from_vcard(&prop_upper);
                        let mut addr = PostalAddress::from_vcard_adr(value);
                        addr.address_type = atype;
                        if !addr.is_empty() {
                            contact.addresses.push(addr);
                        }
                    }
                    "BDAY" => {
                        contact.birthday = SimpleDate::parse(value);
                    }
                    "NOTE" => {
                        contact.notes = vcard_unescape(value);
                    }
                    "X-SOCIALPROFILE" => {
                        let platform_str = prop_upper
                            .split(';')
                            .find(|s| s.starts_with("TYPE="))
                            .map(|s| s.trim_start_matches("TYPE="))
                            .unwrap_or("Custom");
                        let platform = match platform_str.to_lowercase().as_str() {
                            "twitter" => SocialPlatform::Twitter,
                            "linkedin" => SocialPlatform::LinkedIn,
                            "github" => SocialPlatform::GitHub,
                            "mastodon" => SocialPlatform::Mastodon,
                            other => SocialPlatform::Custom(other.to_string()),
                        };
                        contact
                            .social_accounts
                            .push(SocialAccount::new(platform, &vcard_unescape(value)));
                    }
                    _ => {}
                }
            }
        }

        if found_begin && found_end {
            Some(contact)
        } else {
            None
        }
    }
}

// ============================================================================
// vCard helpers
// ============================================================================

/// Escape special chars for vCard values.
fn vcard_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

/// Unescape vCard value.
fn vcard_unescape(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// Unfold vCard continuation lines (lines starting with space/tab are
/// continuations of the previous line).
fn unfold_vcard_lines(data: &str) -> Vec<String> {
    let mut result = Vec::new();
    for line in data.lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && !result.is_empty() {
            if let Some(last) = result.last_mut() {
                let last_val: &mut String = last;
                last_val.push_str(line.get(1..).unwrap_or(""));
            }
        } else {
            result.push(line.to_string());
        }
    }
    result
}

/// Split a vCard property line into (property-with-params, value).
fn split_vcard_line(line: &str) -> Option<(&str, &str)> {
    let colon_pos = line.find(':')?;
    let prop = line.get(..colon_pos)?;
    let value = line.get(colon_pos.checked_add(1)?..)?;
    Some((prop, value))
}

/// Export multiple contacts as a single vCard file.
pub fn export_vcards(contacts: &[Contact]) -> String {
    contacts
        .iter()
        .map(|c| c.to_vcard())
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Import contacts from a vCard file containing one or more entries.
pub fn import_vcards(data: &str, start_id: u64) -> Vec<Contact> {
    let mut contacts = Vec::new();
    let mut current_block = String::new();
    let mut in_vcard = false;
    let mut next_id = start_id;

    for line in data.lines() {
        if line.trim().eq_ignore_ascii_case("BEGIN:VCARD") {
            in_vcard = true;
            current_block.clear();
            current_block.push_str(line);
            current_block.push('\n');
        } else if line.trim().eq_ignore_ascii_case("END:VCARD") {
            current_block.push_str(line);
            current_block.push('\n');
            if in_vcard {
                if let Some(c) = Contact::from_vcard(&current_block, next_id) {
                    contacts.push(c);
                    next_id = next_id.saturating_add(1);
                }
            }
            in_vcard = false;
            current_block.clear();
        } else if in_vcard {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    contacts
}

// ============================================================================
// Sort order
// ============================================================================

/// Sort order for the contact list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Name,
    Company,
    RecentlyAdded,
    RecentlyContacted,
}

impl SortOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Company => "Company",
            Self::RecentlyAdded => "Recently Added",
            Self::RecentlyContacted => "Recently Contacted",
        }
    }
}

// ============================================================================
// Filter
// ============================================================================

/// Filter for the contact list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactFilter {
    All,
    Group(u64),
    HasPhone,
    HasEmail,
    Favorites,
}

impl ContactFilter {
    pub fn label(&self) -> &str {
        match self {
            Self::All => "All Contacts",
            Self::Group(_) => "Group",
            Self::HasPhone => "Has Phone",
            Self::HasEmail => "Has Email",
            Self::Favorites => "Favorites",
        }
    }

    /// Check if a contact passes this filter.
    pub fn matches(&self, contact: &Contact) -> bool {
        match self {
            Self::All => true,
            Self::Group(gid) => contact.groups.contains(gid),
            Self::HasPhone => !contact.phones.is_empty(),
            Self::HasEmail => !contact.emails.is_empty(),
            Self::Favorites => contact.favorite,
        }
    }
}

// ============================================================================
// Duplicate detection
// ============================================================================

/// Result of duplicate detection between two contacts.
#[derive(Clone, Debug)]
pub struct DuplicateMatch {
    pub contact_a_id: u64,
    pub contact_b_id: u64,
    pub reason: DuplicateReason,
    pub confidence: f32,
}

/// Why two contacts are considered duplicates.
#[derive(Clone, Debug, PartialEq)]
pub enum DuplicateReason {
    SameName,
    SamePhone,
    SameEmail,
    SameNameAndCompany,
}

impl DuplicateReason {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SameName => "Same name",
            Self::SamePhone => "Same phone number",
            Self::SameEmail => "Same email address",
            Self::SameNameAndCompany => "Same name & company",
        }
    }
}

/// Detect duplicate contacts in a list.
pub fn find_duplicates(contacts: &[Contact]) -> Vec<DuplicateMatch> {
    let mut duplicates = Vec::new();
    let len = contacts.len();

    for i in 0..len {
        let a = match contacts.get(i) {
            Some(c) => c,
            None => continue,
        };
        for j in (i.wrapping_add(1))..len {
            let b = match contacts.get(j) {
                Some(c) => c,
                None => continue,
            };

            // Same full name (case-insensitive, ignoring empty names)
            if !a.first_name.is_empty()
                && !a.last_name.is_empty()
                && a.first_name.eq_ignore_ascii_case(&b.first_name)
                && a.last_name.eq_ignore_ascii_case(&b.last_name)
            {
                let reason = if !a.company.is_empty()
                    && a.company.eq_ignore_ascii_case(&b.company)
                {
                    DuplicateReason::SameNameAndCompany
                } else {
                    DuplicateReason::SameName
                };
                let confidence = if reason == DuplicateReason::SameNameAndCompany {
                    0.95
                } else {
                    0.80
                };
                duplicates.push(DuplicateMatch {
                    contact_a_id: a.id,
                    contact_b_id: b.id,
                    reason,
                    confidence,
                });
                continue;
            }

            // Same phone number
            let shared_phone = a.phones.iter().any(|pa| {
                b.phones
                    .iter()
                    .any(|pb| normalize_phone(&pa.number) == normalize_phone(&pb.number))
            });
            if shared_phone {
                duplicates.push(DuplicateMatch {
                    contact_a_id: a.id,
                    contact_b_id: b.id,
                    reason: DuplicateReason::SamePhone,
                    confidence: 0.90,
                });
                continue;
            }

            // Same email
            let shared_email = a.emails.iter().any(|ea| {
                b.emails
                    .iter()
                    .any(|eb| ea.email.eq_ignore_ascii_case(&eb.email))
            });
            if shared_email {
                duplicates.push(DuplicateMatch {
                    contact_a_id: a.id,
                    contact_b_id: b.id,
                    reason: DuplicateReason::SameEmail,
                    confidence: 0.90,
                });
            }
        }
    }

    duplicates
}

/// Normalize a phone number for comparison (strip non-digits).
fn normalize_phone(phone: &str) -> String {
    phone.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Merge two contacts: keep all data from both, preferring `primary` for conflicts.
pub fn merge_contacts(primary: &Contact, secondary: &Contact, merged_id: u64) -> Contact {
    let mut merged = primary.clone();
    merged.id = merged_id;

    // Merge phones (add any from secondary that aren't already in primary)
    for phone in &secondary.phones {
        let already = merged.phones.iter().any(|p| {
            normalize_phone(&p.number) == normalize_phone(&phone.number)
        });
        if !already {
            merged.phones.push(phone.clone());
        }
    }

    // Merge emails
    for email in &secondary.emails {
        let already = merged
            .emails
            .iter()
            .any(|e| e.email.eq_ignore_ascii_case(&email.email));
        if !already {
            merged.emails.push(email.clone());
        }
    }

    // Merge addresses
    for addr in &secondary.addresses {
        let already = merged.addresses.iter().any(|a| {
            a.street == addr.street && a.city == addr.city && a.zip == addr.zip
        });
        if !already {
            merged.addresses.push(addr.clone());
        }
    }

    // Merge social accounts
    for social in &secondary.social_accounts {
        let already = merged
            .social_accounts
            .iter()
            .any(|s| s.platform == social.platform && s.handle == social.handle);
        if !already {
            merged.social_accounts.push(social.clone());
        }
    }

    // Merge groups
    for gid in &secondary.groups {
        if !merged.groups.contains(gid) {
            merged.groups.push(*gid);
        }
    }

    // Fill in empty fields from secondary
    if merged.nickname.is_empty() && !secondary.nickname.is_empty() {
        merged.nickname.clone_from(&secondary.nickname);
    }
    if merged.company.is_empty() && !secondary.company.is_empty() {
        merged.company.clone_from(&secondary.company);
    }
    if merged.job_title.is_empty() && !secondary.job_title.is_empty() {
        merged.job_title.clone_from(&secondary.job_title);
    }
    if merged.department.is_empty() && !secondary.department.is_empty() {
        merged.department.clone_from(&secondary.department);
    }
    if merged.birthday.is_none() && secondary.birthday.is_some() {
        merged.birthday = secondary.birthday;
    }
    if merged.notes.is_empty() && !secondary.notes.is_empty() {
        merged.notes.clone_from(&secondary.notes);
    }
    if merged.photo_path.is_none() && secondary.photo_path.is_some() {
        merged.photo_path.clone_from(&secondary.photo_path);
    }
    if !merged.favorite && secondary.favorite {
        merged.favorite = true;
    }

    merged
}

// ============================================================================
// Contact store
// ============================================================================

/// In-memory contact store with CRUD, search, sort, filter, and group management.
pub struct ContactStore {
    contacts: Vec<Contact>,
    groups: Vec<ContactGroup>,
    next_contact_id: u64,
    next_group_id: u64,
    recently_viewed: VecDeque<u64>,
}

impl ContactStore {
    pub fn new() -> Self {
        Self {
            contacts: Vec::new(),
            groups: Vec::new(),
            next_contact_id: 1,
            next_group_id: 1,
            recently_viewed: VecDeque::new(),
        }
    }

    // ----- Contact CRUD -----

    /// Add a new contact, returning its assigned ID.
    pub fn add_contact(&mut self, mut contact: Contact) -> u64 {
        let id = self.next_contact_id;
        self.next_contact_id = self.next_contact_id.saturating_add(1);
        contact.id = id;
        self.contacts.push(contact);
        id
    }

    /// Get a contact by ID.
    pub fn get_contact(&self, id: u64) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == id)
    }

    /// Get a mutable reference to a contact by ID.
    pub fn get_contact_mut(&mut self, id: u64) -> Option<&mut Contact> {
        self.contacts.iter_mut().find(|c| c.id == id)
    }

    /// Delete a contact by ID. Returns true if found and removed.
    pub fn delete_contact(&mut self, id: u64) -> bool {
        let before = self.contacts.len();
        self.contacts.retain(|c| c.id != id);
        // Also remove from recently viewed
        self.recently_viewed.retain(|&rid| rid != id);
        self.contacts.len() != before
    }

    /// Update a contact (replace by ID). Returns true if found and updated.
    pub fn update_contact(&mut self, contact: Contact) -> bool {
        if let Some(existing) = self.contacts.iter_mut().find(|c| c.id == contact.id) {
            *existing = contact;
            true
        } else {
            false
        }
    }

    /// Total number of contacts.
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Get all contacts (unsorted).
    pub fn all_contacts(&self) -> &[Contact] {
        &self.contacts
    }

    // ----- Group CRUD -----

    /// Add a new group, returning its assigned ID.
    pub fn add_group(&mut self, mut group: ContactGroup) -> u64 {
        let id = self.next_group_id;
        self.next_group_id = self.next_group_id.saturating_add(1);
        group.id = id;
        self.groups.push(group);
        id
    }

    /// Get a group by ID.
    pub fn get_group(&self, id: u64) -> Option<&ContactGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Get a mutable reference to a group.
    pub fn get_group_mut(&mut self, id: u64) -> Option<&mut ContactGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Delete a group and remove it from all contacts.
    pub fn delete_group(&mut self, id: u64) -> bool {
        let before = self.groups.len();
        self.groups.retain(|g| g.id != id);
        // Remove group from contacts that reference it
        for contact in &mut self.contacts {
            contact.groups.retain(|&gid| gid != id);
        }
        self.groups.len() != before
    }

    /// Get all groups.
    pub fn all_groups(&self) -> &[ContactGroup] {
        &self.groups
    }

    /// Update group member counts based on current contact data.
    pub fn refresh_group_counts(&mut self) {
        for group in &mut self.groups {
            group.member_count = self
                .contacts
                .iter()
                .filter(|c| c.groups.contains(&group.id))
                .count();
        }
    }

    /// Add a contact to a group.
    pub fn add_contact_to_group(&mut self, contact_id: u64, group_id: u64) -> bool {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == contact_id) {
            if !contact.groups.contains(&group_id) {
                contact.groups.push(group_id);
                return true;
            }
        }
        false
    }

    /// Remove a contact from a group.
    pub fn remove_contact_from_group(&mut self, contact_id: u64, group_id: u64) -> bool {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == contact_id) {
            let before = contact.groups.len();
            contact.groups.retain(|&gid| gid != group_id);
            return contact.groups.len() != before;
        }
        false
    }

    // ----- Search, sort, filter -----

    /// Search contacts across all fields.
    pub fn search(&self, query: &str) -> Vec<&Contact> {
        self.contacts
            .iter()
            .filter(|c| c.matches_search(query))
            .collect()
    }

    /// Get contacts sorted by the given order.
    pub fn sorted_contacts(&self, order: SortOrder) -> Vec<&Contact> {
        let mut refs: Vec<&Contact> = self.contacts.iter().collect();
        match order {
            SortOrder::Name => refs.sort_by(|a, b| a.sort_key_name().cmp(&b.sort_key_name())),
            SortOrder::Company => refs.sort_by(|a, b| {
                a.company
                    .to_lowercase()
                    .cmp(&b.company.to_lowercase())
                    .then_with(|| a.sort_key_name().cmp(&b.sort_key_name()))
            }),
            SortOrder::RecentlyAdded => {
                refs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            }
            SortOrder::RecentlyContacted => {
                refs.sort_by(|a, b| {
                    let a_time = a.last_contacted.unwrap_or(0);
                    let b_time = b.last_contacted.unwrap_or(0);
                    b_time.cmp(&a_time)
                });
            }
        }
        refs
    }

    /// Get contacts matching a filter, then sorted.
    pub fn filtered_sorted(
        &self,
        filter: &ContactFilter,
        order: SortOrder,
        query: &str,
    ) -> Vec<&Contact> {
        let mut refs: Vec<&Contact> = self
            .contacts
            .iter()
            .filter(|c| filter.matches(c) && c.matches_search(query))
            .collect();

        match order {
            SortOrder::Name => refs.sort_by(|a, b| {
                // Favorites first, then alphabetical
                b.favorite
                    .cmp(&a.favorite)
                    .then_with(|| a.sort_key_name().cmp(&b.sort_key_name()))
            }),
            SortOrder::Company => refs.sort_by(|a, b| {
                b.favorite.cmp(&a.favorite).then_with(|| {
                    a.company
                        .to_lowercase()
                        .cmp(&b.company.to_lowercase())
                        .then_with(|| a.sort_key_name().cmp(&b.sort_key_name()))
                })
            }),
            SortOrder::RecentlyAdded => {
                refs.sort_by(|a, b| {
                    b.favorite
                        .cmp(&a.favorite)
                        .then_with(|| b.created_at.cmp(&a.created_at))
                });
            }
            SortOrder::RecentlyContacted => {
                refs.sort_by(|a, b| {
                    b.favorite.cmp(&a.favorite).then_with(|| {
                        let a_time = a.last_contacted.unwrap_or(0);
                        let b_time = b.last_contacted.unwrap_or(0);
                        b_time.cmp(&a_time)
                    })
                });
            }
        }
        refs
    }

    // ----- Favorites -----

    /// Toggle favorite status for a contact. Returns new favorite state.
    pub fn toggle_favorite(&mut self, id: u64) -> Option<bool> {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == id) {
            contact.favorite = !contact.favorite;
            Some(contact.favorite)
        } else {
            None
        }
    }

    /// Get favorite contacts.
    pub fn favorites(&self) -> Vec<&Contact> {
        self.contacts.iter().filter(|c| c.favorite).collect()
    }

    // ----- Recently viewed -----

    /// Record that a contact was viewed.
    pub fn record_view(&mut self, id: u64) {
        // Remove existing occurrence, push to front
        self.recently_viewed.retain(|&rid| rid != id);
        self.recently_viewed.push_front(id);
        while self.recently_viewed.len() > MAX_RECENT {
            self.recently_viewed.pop_back();
        }
    }

    /// Get the recently viewed contacts list (IDs, most recent first).
    pub fn recently_viewed(&self) -> &VecDeque<u64> {
        &self.recently_viewed
    }

    /// Get recently viewed contacts as references.
    pub fn recently_viewed_contacts(&self) -> Vec<&Contact> {
        self.recently_viewed
            .iter()
            .filter_map(|&id| self.get_contact(id))
            .collect()
    }

    // ----- Recently contacted -----

    /// Mark a contact as recently contacted with the given timestamp.
    pub fn mark_contacted(&mut self, id: u64, timestamp: u64) {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == id) {
            contact.last_contacted = Some(timestamp);
        }
    }

    // ----- Duplicate detection -----

    /// Find duplicate contacts.
    pub fn find_duplicates(&self) -> Vec<DuplicateMatch> {
        find_duplicates(&self.contacts)
    }

    /// Merge two contacts (by ID). Removes both originals, adds merged.
    /// Returns the new merged contact's ID, or None if either ID wasn't found.
    pub fn merge_contacts(&mut self, id_a: u64, id_b: u64) -> Option<u64> {
        let a = self.contacts.iter().find(|c| c.id == id_a)?.clone();
        let b = self.contacts.iter().find(|c| c.id == id_b)?.clone();

        let merged_id = self.next_contact_id;
        self.next_contact_id = self.next_contact_id.saturating_add(1);

        let merged = merge_contacts(&a, &b, merged_id);
        self.contacts.retain(|c| c.id != id_a && c.id != id_b);
        self.contacts.push(merged);

        // Update recently viewed
        self.recently_viewed.retain(|&rid| rid != id_a && rid != id_b);

        Some(merged_id)
    }

    // ----- Import/Export -----

    /// Export all contacts as vCard text.
    pub fn export_all(&self) -> String {
        export_vcards(&self.contacts)
    }

    /// Import contacts from vCard text. Returns number of contacts imported.
    pub fn import_vcards(&mut self, data: &str) -> usize {
        let imported = import_vcards(data, self.next_contact_id);
        let count = imported.len();
        for contact in imported {
            let _id = self.add_contact(contact);
        }
        count
    }

    // ----- Birthday helpers -----

    /// Get contacts with upcoming birthdays.
    pub fn upcoming_birthdays(&self, today_month: u8, today_day: u8, within_days: u16) -> Vec<&Contact> {
        self.contacts
            .iter()
            .filter(|c| {
                c.birthday
                    .map_or(false, |b| b.is_upcoming_within(today_month, today_day, within_days))
            })
            .collect()
    }

    // ----- Group stats -----

    /// Get statistics about groups.
    pub fn group_stats(&self) -> Vec<(u64, String, usize)> {
        self.groups
            .iter()
            .map(|g| {
                let count = self
                    .contacts
                    .iter()
                    .filter(|c| c.groups.contains(&g.id))
                    .count();
                (g.id, g.name.clone(), count)
            })
            .collect()
    }
}

impl Default for ContactStore {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// App view state
// ============================================================================

/// Which panel is shown on the right side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetailView {
    /// No contact selected -- show welcome/empty state.
    Empty,
    /// Viewing a contact's details.
    ViewContact(u64),
    /// Editing a contact.
    EditContact(u64),
    /// Adding a new contact.
    NewContact,
    /// Viewing duplicate detection results.
    Duplicates,
    /// Viewing group management panel.
    Groups,
}

/// Top-level application state.
pub struct ContactsApp {
    pub store: ContactStore,
    pub view: DetailView,
    pub search_query: String,
    pub sort_order: SortOrder,
    pub filter: ContactFilter,
    pub scroll_offset: f32,
    pub selected_letter: Option<char>,
    pub show_search: bool,

    // Edit form state
    pub edit_first_name: String,
    pub edit_last_name: String,
    pub edit_company: String,
    pub edit_job_title: String,
    pub edit_department: String,
    pub edit_nickname: String,
    pub edit_phone: String,
    pub edit_phone_type: PhoneType,
    pub edit_email: String,
    pub edit_email_type: EmailType,
    pub edit_notes: String,
    pub edit_birthday: String,
    pub edit_street: String,
    pub edit_city: String,
    pub edit_state: String,
    pub edit_zip: String,
    pub edit_country: String,
    pub edit_address_type: AddressType,

    // Window dimensions
    pub window_width: f32,
    pub window_height: f32,
}

impl ContactsApp {
    pub fn new() -> Self {
        Self {
            store: ContactStore::new(),
            view: DetailView::Empty,
            search_query: String::new(),
            sort_order: SortOrder::Name,
            filter: ContactFilter::All,
            scroll_offset: 0.0,
            selected_letter: None,
            show_search: false,

            edit_first_name: String::new(),
            edit_last_name: String::new(),
            edit_company: String::new(),
            edit_job_title: String::new(),
            edit_department: String::new(),
            edit_nickname: String::new(),
            edit_phone: String::new(),
            edit_phone_type: PhoneType::Mobile,
            edit_email: String::new(),
            edit_email_type: EmailType::Personal,
            edit_notes: String::new(),
            edit_birthday: String::new(),
            edit_street: String::new(),
            edit_city: String::new(),
            edit_state: String::new(),
            edit_zip: String::new(),
            edit_country: String::new(),
            edit_address_type: AddressType::Home,

            window_width: 1024.0,
            window_height: 768.0,
        }
    }

    /// Load a contact's data into the edit form fields.
    pub fn load_edit_form(&mut self, contact: &Contact) {
        self.edit_first_name.clone_from(&contact.first_name);
        self.edit_last_name.clone_from(&contact.last_name);
        self.edit_company.clone_from(&contact.company);
        self.edit_job_title.clone_from(&contact.job_title);
        self.edit_department.clone_from(&contact.department);
        self.edit_nickname.clone_from(&contact.nickname);
        self.edit_notes.clone_from(&contact.notes);
        self.edit_birthday = contact
            .birthday
            .map_or(String::new(), |b| b.format_display());

        // Load first phone/email if present
        if let Some(phone) = contact.phones.first() {
            self.edit_phone.clone_from(&phone.number);
            self.edit_phone_type = phone.phone_type;
        } else {
            self.edit_phone.clear();
            self.edit_phone_type = PhoneType::Mobile;
        }
        if let Some(email) = contact.emails.first() {
            self.edit_email.clone_from(&email.email);
            self.edit_email_type = email.email_type;
        } else {
            self.edit_email.clear();
            self.edit_email_type = EmailType::Personal;
        }

        // Load first address if present
        if let Some(addr) = contact.addresses.first() {
            self.edit_street.clone_from(&addr.street);
            self.edit_city.clone_from(&addr.city);
            self.edit_state.clone_from(&addr.state);
            self.edit_zip.clone_from(&addr.zip);
            self.edit_country.clone_from(&addr.country);
            self.edit_address_type = addr.address_type;
        } else {
            self.edit_street.clear();
            self.edit_city.clear();
            self.edit_state.clear();
            self.edit_zip.clear();
            self.edit_country.clear();
            self.edit_address_type = AddressType::Home;
        }
    }

    /// Clear the edit form fields.
    pub fn clear_edit_form(&mut self) {
        self.edit_first_name.clear();
        self.edit_last_name.clear();
        self.edit_company.clear();
        self.edit_job_title.clear();
        self.edit_department.clear();
        self.edit_nickname.clear();
        self.edit_phone.clear();
        self.edit_phone_type = PhoneType::Mobile;
        self.edit_email.clear();
        self.edit_email_type = EmailType::Personal;
        self.edit_notes.clear();
        self.edit_birthday.clear();
        self.edit_street.clear();
        self.edit_city.clear();
        self.edit_state.clear();
        self.edit_zip.clear();
        self.edit_country.clear();
        self.edit_address_type = AddressType::Home;
    }

    /// Apply the edit form to create a new Contact (for add).
    pub fn build_contact_from_form(&self) -> Contact {
        let mut contact = Contact::new(0, &self.edit_first_name, &self.edit_last_name);
        contact.company.clone_from(&self.edit_company);
        contact.job_title.clone_from(&self.edit_job_title);
        contact.department.clone_from(&self.edit_department);
        contact.nickname.clone_from(&self.edit_nickname);
        contact.notes.clone_from(&self.edit_notes);
        contact.birthday = SimpleDate::parse(&self.edit_birthday);

        if !self.edit_phone.is_empty() {
            contact.phones.push(
                PhoneNumber::new(&self.edit_phone, self.edit_phone_type).with_primary(true),
            );
        }
        if !self.edit_email.is_empty() {
            contact.emails.push(
                EmailAddress::new(&self.edit_email, self.edit_email_type).with_primary(true),
            );
        }

        if !self.edit_street.is_empty()
            || !self.edit_city.is_empty()
            || !self.edit_state.is_empty()
            || !self.edit_zip.is_empty()
            || !self.edit_country.is_empty()
        {
            let mut addr = PostalAddress::new(self.edit_address_type);
            addr.street.clone_from(&self.edit_street);
            addr.city.clone_from(&self.edit_city);
            addr.state.clone_from(&self.edit_state);
            addr.zip.clone_from(&self.edit_zip);
            addr.country.clone_from(&self.edit_country);
            contact.addresses.push(addr);
        }

        contact
    }

    /// Render the full application UI.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Full window background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_sidebar(&mut cmds);
        self.render_detail_panel(&mut cmds);

        cmds
    }

    /// Render the left sidebar: header, search, contact list, alphabet bar.
    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let sidebar_total = SIDEBAR_WIDTH + ALPHABET_BAR_WIDTH;

        // Sidebar background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: sidebar_total,
            height: self.window_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Header
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: sidebar_total,
            height: HEADER_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        // App title
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 18.0,
            text: String::from("Contacts"),
            font_size: 20.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(180.0),
        });

        // Contact count
        let count_text = format!("{}", self.store.contact_count());
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: 38.0,
            text: count_text,
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(100.0),
        });

        // Add contact button (+)
        let btn_x = SIDEBAR_WIDTH - 40.0;
        cmds.push(RenderCommand::FillRect {
            x: btn_x,
            y: 12.0,
            width: 32.0,
            height: 32.0,
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: btn_x + 10.0,
            y: 18.0,
            text: String::from("+"),
            font_size: 18.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Search icon / bar
        let search_y = HEADER_HEIGHT + 8.0;
        cmds.push(RenderCommand::FillRect {
            x: 8.0,
            y: search_y,
            width: SIDEBAR_WIDTH - 16.0,
            height: SEARCH_BAR_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::all(8.0),
        });
        let search_text = if self.search_query.is_empty() {
            String::from("Search contacts...")
        } else {
            self.search_query.clone()
        };
        let search_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT_COLOR
        };
        cmds.push(RenderCommand::Text {
            x: 36.0,
            y: search_y + 12.0,
            text: search_text,
            font_size: 13.0,
            color: search_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 60.0),
        });

        // Search icon (magnifying glass placeholder)
        cmds.push(RenderCommand::Text {
            x: 16.0,
            y: search_y + 12.0,
            text: String::from("?"),
            font_size: 14.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Filter/sort indicators
        let filter_y = search_y + SEARCH_BAR_HEIGHT + 8.0;
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: filter_y,
            text: format!("{} | {}", self.filter.label(), self.sort_order.label()),
            font_size: 10.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(SIDEBAR_WIDTH - 24.0),
        });

        // Contact list
        let list_y = filter_y + 20.0;
        let list_height = self.window_height - list_y;
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: list_y,
            width: SIDEBAR_WIDTH,
            height: list_height,
        });

        let contacts = self.store.filtered_sorted(
            &self.filter,
            self.sort_order,
            &self.search_query,
        );

        let mut cy = list_y - self.scroll_offset;
        let mut current_letter: Option<char> = None;

        for contact in &contacts {
            let letter = contact.first_letter();

            // Letter divider
            if current_letter != Some(letter) && self.sort_order == SortOrder::Name {
                if cy + LETTER_DIVIDER_HEIGHT > list_y && cy < self.window_height {
                    cmds.push(RenderCommand::FillRect {
                        x: 0.0,
                        y: cy,
                        width: SIDEBAR_WIDTH,
                        height: LETTER_DIVIDER_HEIGHT,
                        color: Color::rgba(49, 50, 68, 180),
                        corner_radii: CornerRadii::ZERO,
                    });
                    cmds.push(RenderCommand::Text {
                        x: 12.0,
                        y: cy + 8.0,
                        text: letter.to_string(),
                        font_size: 12.0,
                        color: BLUE,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
                cy += LETTER_DIVIDER_HEIGHT;
                current_letter = Some(letter);
            }

            // Contact row
            if cy + CONTACT_ROW_HEIGHT > list_y && cy < self.window_height {
                let is_selected = matches!(
                    self.view,
                    DetailView::ViewContact(id) | DetailView::EditContact(id)
                    if id == contact.id
                );

                let row_bg = if is_selected { SURFACE0 } else { Color::TRANSPARENT };
                cmds.push(RenderCommand::FillRect {
                    x: 0.0,
                    y: cy,
                    width: SIDEBAR_WIDTH,
                    height: CONTACT_ROW_HEIGHT,
                    color: row_bg,
                    corner_radii: CornerRadii::ZERO,
                });

                // Avatar circle
                let avatar_x = 12.0;
                let avatar_y = cy + 6.0;
                let avatar_r = 20.0;
                let avatar_color = if contact.favorite { YELLOW } else { SURFACE1 };
                cmds.push(RenderCommand::FillRect {
                    x: avatar_x,
                    y: avatar_y,
                    width: avatar_r * 2.0,
                    height: avatar_r * 2.0,
                    color: avatar_color,
                    corner_radii: CornerRadii::all(avatar_r),
                });
                cmds.push(RenderCommand::Text {
                    x: avatar_x + 8.0,
                    y: avatar_y + 12.0,
                    text: contact.initials(),
                    font_size: 14.0,
                    color: if contact.favorite { BASE } else { TEXT_COLOR },
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });

                // Name
                cmds.push(RenderCommand::Text {
                    x: 60.0,
                    y: cy + 12.0,
                    text: contact.computed_display_name(),
                    font_size: 14.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - 80.0),
                });

                // Subtitle (company or phone)
                let subtitle = if !contact.company.is_empty() {
                    contact.company.clone()
                } else if let Some(phone) = contact.primary_phone() {
                    phone.number.clone()
                } else if let Some(email) = contact.primary_email() {
                    email.email.clone()
                } else {
                    String::new()
                };
                if !subtitle.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: 60.0,
                        y: cy + 30.0,
                        text: subtitle,
                        font_size: 11.0,
                        color: SUBTEXT0,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(SIDEBAR_WIDTH - 80.0),
                    });
                }

                // Favorite star
                if contact.favorite {
                    cmds.push(RenderCommand::Text {
                        x: SIDEBAR_WIDTH - 24.0,
                        y: cy + 18.0,
                        text: String::from("*"),
                        font_size: 16.0,
                        color: YELLOW,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                }
            }

            cy += CONTACT_ROW_HEIGHT;
        }

        cmds.push(RenderCommand::PopClip);

        // Alphabet sidebar
        self.render_alphabet_bar(cmds);

        // Sidebar divider line
        cmds.push(RenderCommand::Line {
            x1: sidebar_total,
            y1: 0.0,
            x2: sidebar_total,
            y2: self.window_height,
            color: SURFACE1,
            width: 1.0,
        });
    }

    /// Render the A-Z quick navigation bar.
    fn render_alphabet_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let bar_x = SIDEBAR_WIDTH;
        let bar_height = self.window_height - HEADER_HEIGHT;
        let letter_height = bar_height / ALPHABET.len() as f32;

        cmds.push(RenderCommand::FillRect {
            x: bar_x,
            y: HEADER_HEIGHT,
            width: ALPHABET_BAR_WIDTH,
            height: bar_height,
            color: Color::rgba(24, 24, 37, 200),
            corner_radii: CornerRadii::ZERO,
        });

        for (i, &letter) in ALPHABET.iter().enumerate() {
            let ly = HEADER_HEIGHT + (i as f32 * letter_height);
            let is_selected = self.selected_letter == Some(letter);
            let color = if is_selected { BLUE } else { SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: bar_x + 6.0,
                y: ly + 2.0,
                text: letter.to_string(),
                font_size: 9.0,
                color,
                font_weight: if is_selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
            });
        }
    }

    /// Render the right detail panel.
    fn render_detail_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let panel_x = SIDEBAR_WIDTH + ALPHABET_BAR_WIDTH;
        let panel_width = self.window_width - panel_x;

        // Panel background
        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: 0.0,
            width: panel_width,
            height: self.window_height,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        match &self.view {
            DetailView::Empty => {
                self.render_empty_state(cmds, panel_x, panel_width);
            }
            DetailView::ViewContact(id) => {
                let id_val = *id;
                if let Some(contact) = self.store.get_contact(id_val) {
                    let c = contact.clone();
                    self.render_contact_detail(cmds, panel_x, panel_width, &c);
                }
            }
            DetailView::EditContact(id) => {
                let id_val = *id;
                self.render_edit_form(cmds, panel_x, panel_width, Some(id_val));
            }
            DetailView::NewContact => {
                self.render_edit_form(cmds, panel_x, panel_width, None);
            }
            DetailView::Duplicates => {
                self.render_duplicates_panel(cmds, panel_x, panel_width);
            }
            DetailView::Groups => {
                self.render_groups_panel(cmds, panel_x, panel_width);
            }
        }
    }

    /// Render empty state (no contact selected).
    fn render_empty_state(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        panel_width: f32,
    ) {
        let center_x = panel_x + panel_width / 2.0 - 80.0;
        let center_y = self.window_height / 2.0 - 40.0;

        // Large person icon placeholder
        cmds.push(RenderCommand::FillRect {
            x: center_x + 40.0,
            y: center_y - 60.0,
            width: 80.0,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(40.0),
        });
        cmds.push(RenderCommand::Text {
            x: center_x + 62.0,
            y: center_y - 30.0,
            text: String::from("?"),
            font_size: 32.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cmds.push(RenderCommand::Text {
            x: center_x + 10.0,
            y: center_y + 40.0,
            text: String::from("Select a contact"),
            font_size: 18.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_width - 40.0),
        });
        cmds.push(RenderCommand::Text {
            x: center_x - 20.0,
            y: center_y + 64.0,
            text: String::from("or press + to add a new one"),
            font_size: 13.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_width - 40.0),
        });
    }

    /// Render the contact detail view.
    fn render_contact_detail(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        panel_width: f32,
        contact: &Contact,
    ) {
        let pad = DETAIL_PADDING;
        let cx = panel_x + pad;
        let mut cy = pad;

        // Header area with avatar
        let avatar_center_x = panel_x + panel_width / 2.0;

        // Avatar circle
        cmds.push(RenderCommand::FillRect {
            x: avatar_center_x - AVATAR_SIZE / 2.0,
            y: cy,
            width: AVATAR_SIZE,
            height: AVATAR_SIZE,
            color: if contact.favorite { YELLOW } else { BLUE },
            corner_radii: CornerRadii::all(AVATAR_SIZE / 2.0),
        });
        cmds.push(RenderCommand::Text {
            x: avatar_center_x - 16.0,
            y: cy + 24.0,
            text: contact.initials(),
            font_size: 28.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        cy += AVATAR_SIZE + 12.0;

        // Display name
        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: contact.computed_display_name(),
            font_size: 22.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width - pad * 2.0),
        });
        cy += 28.0;

        // Company / job title
        if !contact.company.is_empty() || !contact.job_title.is_empty() {
            let company_line = if !contact.job_title.is_empty() && !contact.company.is_empty() {
                format!("{} at {}", contact.job_title, contact.company)
            } else if !contact.company.is_empty() {
                contact.company.clone()
            } else {
                contact.job_title.clone()
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: company_line,
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - pad * 2.0),
            });
            cy += 20.0;
        }

        // Department
        if !contact.department.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: contact.department.clone(),
                font_size: 12.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - pad * 2.0),
            });
            cy += 18.0;
        }

        // Nickname
        if !contact.nickname.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: format!("\"{}\"", contact.nickname),
                font_size: 12.0,
                color: LAVENDER,
                font_weight: FontWeightHint::Light,
                max_width: Some(panel_width - pad * 2.0),
            });
            cy += 18.0;
        }

        cy += 8.0;

        // Quick action buttons (stubs)
        let btn_w = 80.0;
        let btn_h = 36.0;
        let btn_gap = 12.0;
        let actions = ["Call", "Email", "Map"];
        let action_colors = [GREEN, BLUE, PEACH];
        for (i, (label, color)) in actions.iter().zip(action_colors.iter()).enumerate() {
            let bx = cx + (i as f32 * (btn_w + btn_gap));
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: cy,
                width: btn_w,
                height: btn_h,
                color: *color,
                corner_radii: CornerRadii::all(6.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 20.0,
                y: cy + 10.0,
                text: (*label).to_string(),
                font_size: 13.0,
                color: BASE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
        }
        cy += btn_h + 16.0;

        // Divider
        cmds.push(RenderCommand::Line {
            x1: cx,
            y1: cy,
            x2: panel_x + panel_width - pad,
            y2: cy,
            color: SURFACE1,
            width: 1.0,
        });
        cy += 12.0;

        // Phone numbers
        if !contact.phones.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Phone"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            for phone in &contact.phones {
                let pref_marker = if phone.primary { " (primary)" } else { "" };
                cmds.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: cy,
                    text: format!("{}: {}{pref_marker}", phone.phone_type.label(), phone.number),
                    font_size: 13.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width - pad * 2.0 - 8.0),
                });
                cy += 20.0;
            }
            cy += 8.0;
        }

        // Emails
        if !contact.emails.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Email"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            for email in &contact.emails {
                let pref_marker = if email.primary { " (primary)" } else { "" };
                cmds.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: cy,
                    text: format!(
                        "{}: {}{pref_marker}",
                        email.email_type.label(),
                        email.email
                    ),
                    font_size: 13.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width - pad * 2.0 - 8.0),
                });
                cy += 20.0;
            }
            cy += 8.0;
        }

        // Addresses
        if !contact.addresses.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Address"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            for addr in &contact.addresses {
                cmds.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: cy,
                    text: format!("{}: {}", addr.address_type.label(), addr.display_line()),
                    font_size: 13.0,
                    color: TEXT_COLOR,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width - pad * 2.0 - 8.0),
                });
                cy += 20.0;
            }
            cy += 8.0;
        }

        // Social accounts
        if !contact.social_accounts.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Social"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            for social in &contact.social_accounts {
                cmds.push(RenderCommand::Text {
                    x: cx + 8.0,
                    y: cy,
                    text: format!("{}: {}", social.platform.label(), social.handle),
                    font_size: 13.0,
                    color: LAVENDER,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(panel_width - pad * 2.0 - 8.0),
                });
                cy += 20.0;
            }
            cy += 8.0;
        }

        // Birthday
        if let Some(ref bday) = contact.birthday {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Birthday"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: cy,
                text: bday.format_display(),
                font_size: 13.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            cy += 24.0;
        }

        // Notes
        if !contact.notes.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Notes"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            cmds.push(RenderCommand::Text {
                x: cx + 8.0,
                y: cy,
                text: contact.notes.clone(),
                font_size: 13.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - pad * 2.0 - 16.0),
            });
            cy += 24.0;
        }

        // Groups chips
        if !contact.groups.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("Groups"),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cy += 16.0;
            let mut chip_x = cx;
            for &gid in &contact.groups {
                if let Some(group) = self.store.get_group(gid) {
                    let chip_w = (group.name.len() as f32 * 7.0) + 16.0;
                    cmds.push(RenderCommand::FillRect {
                        x: chip_x,
                        y: cy,
                        width: chip_w,
                        height: GROUP_CHIP_HEIGHT,
                        color: group.color,
                        corner_radii: CornerRadii::all(GROUP_CHIP_HEIGHT / 2.0),
                    });
                    cmds.push(RenderCommand::Text {
                        x: chip_x + 8.0,
                        y: cy + 7.0,
                        text: group.name.clone(),
                        font_size: 11.0,
                        color: BASE,
                        font_weight: FontWeightHint::Bold,
                        max_width: Some(chip_w - 16.0),
                    });
                    chip_x += chip_w + 8.0;
                }
            }
            cy += GROUP_CHIP_HEIGHT + 12.0;
        }

        // Edit / Delete buttons at bottom
        let _ = cy; // mark used
        let btn_y = self.window_height - 60.0;
        // Edit button
        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: btn_y,
            width: 80.0,
            height: 36.0,
            color: BLUE,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: cx + 22.0,
            y: btn_y + 10.0,
            text: String::from("Edit"),
            font_size: 13.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        // Favorite toggle
        cmds.push(RenderCommand::FillRect {
            x: cx + 96.0,
            y: btn_y,
            width: 80.0,
            height: 36.0,
            color: if contact.favorite { YELLOW } else { SURFACE1 },
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: cx + 114.0,
            y: btn_y + 10.0,
            text: if contact.favorite {
                String::from("Unstar")
            } else {
                String::from("Star")
            },
            font_size: 13.0,
            color: if contact.favorite { BASE } else { TEXT_COLOR },
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        // Delete button
        cmds.push(RenderCommand::FillRect {
            x: cx + 192.0,
            y: btn_y,
            width: 80.0,
            height: 36.0,
            color: RED,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: cx + 206.0,
            y: btn_y + 10.0,
            text: String::from("Delete"),
            font_size: 13.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the add/edit contact form.
    fn render_edit_form(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        panel_width: f32,
        editing_id: Option<u64>,
    ) {
        let pad = DETAIL_PADDING;
        let cx = panel_x + pad;
        let field_w = panel_width - pad * 2.0;
        let mut cy = pad;

        // Title
        let title = if editing_id.is_some() {
            "Edit Contact"
        } else {
            "New Contact"
        };
        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: title.to_string(),
            font_size: 20.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(field_w),
        });
        cy += 36.0;

        // Helper closure-like function references
        let fields: &[(&str, &str)] = &[
            ("First Name", &self.edit_first_name),
            ("Last Name", &self.edit_last_name),
            ("Nickname", &self.edit_nickname),
            ("Company", &self.edit_company),
            ("Job Title", &self.edit_job_title),
            ("Department", &self.edit_department),
            ("Phone", &self.edit_phone),
            ("Email", &self.edit_email),
            ("Birthday (YYYY-MM-DD)", &self.edit_birthday),
            ("Street", &self.edit_street),
            ("City", &self.edit_city),
            ("State", &self.edit_state),
            ("ZIP Code", &self.edit_zip),
            ("Country", &self.edit_country),
        ];

        for &(label, value) in fields {
            // Label
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: label.to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(field_w),
            });
            cy += 14.0;

            // Input field background
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: field_w,
                height: FIELD_HEIGHT,
                color: SURFACE0,
                corner_radii: CornerRadii::all(6.0),
            });

            // Value text
            let display = if value.is_empty() {
                label.to_string()
            } else {
                value.to_string()
            };
            let text_color = if value.is_empty() { OVERLAY0 } else { TEXT_COLOR };
            cmds.push(RenderCommand::Text {
                x: cx + 10.0,
                y: cy + 10.0,
                text: display,
                font_size: 13.0,
                color: text_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(field_w - 20.0),
            });
            cy += FIELD_HEIGHT + 6.0;
        }

        // Notes (taller field)
        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: String::from("Notes"),
            font_size: 11.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w),
        });
        cy += 14.0;
        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: cy,
            width: field_w,
            height: 80.0,
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });
        let notes_text = if self.edit_notes.is_empty() {
            String::from("Notes")
        } else {
            self.edit_notes.clone()
        };
        let notes_color = if self.edit_notes.is_empty() {
            OVERLAY0
        } else {
            TEXT_COLOR
        };
        cmds.push(RenderCommand::Text {
            x: cx + 10.0,
            y: cy + 10.0,
            text: notes_text,
            font_size: 13.0,
            color: notes_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(field_w - 20.0),
        });
        cy += 92.0;

        // Save / Cancel buttons
        cmds.push(RenderCommand::FillRect {
            x: cx,
            y: cy,
            width: 80.0,
            height: 36.0,
            color: GREEN,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: cx + 22.0,
            y: cy + 10.0,
            text: String::from("Save"),
            font_size: 13.0,
            color: BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        cmds.push(RenderCommand::FillRect {
            x: cx + 96.0,
            y: cy,
            width: 80.0,
            height: 36.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: cx + 108.0,
            y: cy + 10.0,
            text: String::from("Cancel"),
            font_size: 13.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    /// Render the duplicate detection results panel.
    fn render_duplicates_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        panel_width: f32,
    ) {
        let pad = DETAIL_PADDING;
        let cx = panel_x + pad;
        let mut cy = pad;

        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: String::from("Duplicate Detection"),
            font_size: 20.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width - pad * 2.0),
        });
        cy += 32.0;

        let duplicates = self.store.find_duplicates();

        if duplicates.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("No duplicates found."),
                font_size: 14.0,
                color: GREEN,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - pad * 2.0),
            });
            return;
        }

        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: format!("Found {} potential duplicate(s):", duplicates.len()),
            font_size: 13.0,
            color: PEACH,
            font_weight: FontWeightHint::Regular,
            max_width: Some(panel_width - pad * 2.0),
        });
        cy += 24.0;

        for dup in &duplicates {
            let name_a = self
                .store
                .get_contact(dup.contact_a_id)
                .map_or(String::from("?"), |c| c.computed_display_name());
            let name_b = self
                .store
                .get_contact(dup.contact_b_id)
                .map_or(String::from("?"), |c| c.computed_display_name());

            // Duplicate card
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: panel_width - pad * 2.0,
                height: 60.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 12.0,
                y: cy + 10.0,
                text: format!("{name_a}  <->  {name_b}"),
                font_size: 13.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(panel_width - pad * 2.0 - 24.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 12.0,
                y: cy + 30.0,
                text: format!(
                    "{} (confidence: {:.0}%)",
                    dup.reason.label(),
                    dup.confidence * 100.0
                ),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - pad * 2.0 - 24.0),
            });

            // Merge button
            let merge_x = panel_x + panel_width - pad - 72.0;
            cmds.push(RenderCommand::FillRect {
                x: merge_x,
                y: cy + 14.0,
                width: 60.0,
                height: 28.0,
                color: BLUE,
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: merge_x + 10.0,
                y: cy + 20.0,
                text: String::from("Merge"),
                font_size: 11.0,
                color: BASE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            cy += 72.0;
        }
    }

    /// Render the groups management panel.
    fn render_groups_panel(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        panel_width: f32,
    ) {
        let pad = DETAIL_PADDING;
        let cx = panel_x + pad;
        let mut cy = pad;

        cmds.push(RenderCommand::Text {
            x: cx,
            y: cy,
            text: String::from("Groups"),
            font_size: 20.0,
            color: TEXT_COLOR,
            font_weight: FontWeightHint::Bold,
            max_width: Some(panel_width - pad * 2.0),
        });
        cy += 32.0;

        let stats = self.store.group_stats();

        if stats.is_empty() {
            cmds.push(RenderCommand::Text {
                x: cx,
                y: cy,
                text: String::from("No groups yet. Create one to organize contacts."),
                font_size: 14.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(panel_width - pad * 2.0),
            });
            return;
        }

        for (gid, name, count) in &stats {
            let group_color = self
                .store
                .get_group(*gid)
                .map_or(BLUE, |g| g.color);

            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: panel_width - pad * 2.0,
                height: 48.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(8.0),
            });

            // Color dot
            cmds.push(RenderCommand::FillRect {
                x: cx + 12.0,
                y: cy + 16.0,
                width: 16.0,
                height: 16.0,
                color: group_color,
                corner_radii: CornerRadii::all(8.0),
            });

            cmds.push(RenderCommand::Text {
                x: cx + 36.0,
                y: cy + 10.0,
                text: name.clone(),
                font_size: 14.0,
                color: TEXT_COLOR,
                font_weight: FontWeightHint::Bold,
                max_width: Some(panel_width - pad * 2.0 - 100.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 36.0,
                y: cy + 28.0,
                text: format!("{count} contact(s)"),
                font_size: 11.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            cy += 56.0;
        }
    }

    /// Populate with sample contacts for demonstration.
    pub fn load_sample_data(&mut self) {
        // Groups
        let g1 = self
            .store
            .add_group(ContactGroup::new(0, "Family").with_color(GREEN));
        let g2 = self
            .store
            .add_group(ContactGroup::new(0, "Work").with_color(BLUE));
        let g3 = self
            .store
            .add_group(ContactGroup::new(0, "Friends").with_color(PEACH));

        // Contact 1
        let mut c1 = Contact::new(0, "Alice", "Anderson");
        c1.company = String::from("Acme Corp");
        c1.job_title = String::from("Software Engineer");
        c1.department = String::from("Engineering");
        c1.phones.push(PhoneNumber::new("+1-555-0101", PhoneType::Mobile).with_primary(true));
        c1.phones.push(PhoneNumber::new("+1-555-0102", PhoneType::Work));
        c1.emails.push(EmailAddress::new("alice@example.com", EmailType::Personal).with_primary(true));
        c1.emails.push(EmailAddress::new("alice.anderson@acme.com", EmailType::Work));
        c1.birthday = SimpleDate::new(1990, 3, 15);
        c1.favorite = true;
        c1.groups.push(g2);
        c1.groups.push(g3);
        c1.social_accounts.push(SocialAccount::new(SocialPlatform::GitHub, "@alice"));
        c1.notes = String::from("Met at the Rust conference 2024.");
        let mut addr1 = PostalAddress::new(AddressType::Home);
        addr1.street = String::from("123 Main St");
        addr1.city = String::from("Springfield");
        addr1.state = String::from("IL");
        addr1.zip = String::from("62704");
        addr1.country = String::from("US");
        c1.addresses.push(addr1);
        c1.created_at = 1000;
        let id1 = self.store.add_contact(c1);

        // Contact 2
        let mut c2 = Contact::new(0, "Bob", "Baker");
        c2.company = String::from("Baker & Sons");
        c2.job_title = String::from("Manager");
        c2.phones.push(PhoneNumber::new("+1-555-0201", PhoneType::Mobile).with_primary(true));
        c2.emails.push(EmailAddress::new("bob@baker.com", EmailType::Work).with_primary(true));
        c2.birthday = SimpleDate::new(1985, 7, 22);
        c2.groups.push(g2);
        c2.created_at = 2000;
        let _id2 = self.store.add_contact(c2);

        // Contact 3
        let mut c3 = Contact::new(0, "Carol", "Chen");
        c3.company = String::from("Acme Corp");
        c3.phones.push(PhoneNumber::new("+1-555-0301", PhoneType::Home).with_primary(true));
        c3.emails.push(EmailAddress::new("carol@example.com", EmailType::Personal).with_primary(true));
        c3.groups.push(g1);
        c3.groups.push(g3);
        c3.favorite = true;
        c3.created_at = 3000;
        let _id3 = self.store.add_contact(c3);

        // Contact 4
        let mut c4 = Contact::new(0, "David", "Diaz");
        c4.phones.push(PhoneNumber::new("+1-555-0401", PhoneType::Mobile).with_primary(true));
        c4.groups.push(g1);
        c4.created_at = 4000;
        let _id4 = self.store.add_contact(c4);

        // Contact 5
        let mut c5 = Contact::new(0, "Emma", "Evans");
        c5.company = String::from("TechStart");
        c5.job_title = String::from("CTO");
        c5.emails.push(EmailAddress::new("emma@techstart.io", EmailType::Work).with_primary(true));
        c5.social_accounts.push(SocialAccount::new(SocialPlatform::LinkedIn, "emma-evans"));
        c5.social_accounts.push(SocialAccount::new(SocialPlatform::Twitter, "@emma_e"));
        c5.created_at = 5000;
        let _id5 = self.store.add_contact(c5);

        // Select first contact
        self.view = DetailView::ViewContact(id1);
        self.store.refresh_group_counts();
    }
}

impl Default for ContactsApp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Main entry point
// ============================================================================

fn main() {
    let mut app = ContactsApp::new();
    app.load_sample_data();

    // Render one frame to validate the rendering pipeline
    let _commands = app.render();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helper: create a minimal contact for tests
    // -----------------------------------------------------------------------
    fn make_contact(first: &str, last: &str) -> Contact {
        Contact::new(0, first, last)
    }

    fn make_store_with_contacts() -> ContactStore {
        let mut store = ContactStore::new();
        let mut c1 = make_contact("Alice", "Anderson");
        c1.phones.push(PhoneNumber::new("+1-555-0101", PhoneType::Mobile).with_primary(true));
        c1.emails.push(EmailAddress::new("alice@example.com", EmailType::Personal).with_primary(true));
        c1.company = String::from("Acme Corp");
        c1.created_at = 1000;
        store.add_contact(c1);

        let mut c2 = make_contact("Bob", "Baker");
        c2.phones.push(PhoneNumber::new("+1-555-0201", PhoneType::Work));
        c2.emails.push(EmailAddress::new("bob@work.com", EmailType::Work));
        c2.company = String::from("Baker Inc");
        c2.created_at = 2000;
        store.add_contact(c2);

        let mut c3 = make_contact("Carol", "Chen");
        c3.phones.push(PhoneNumber::new("+1-555-0301", PhoneType::Home));
        c3.created_at = 3000;
        c3.favorite = true;
        store.add_contact(c3);

        store
    }

    // -----------------------------------------------------------------------
    // Contact creation and fields
    // -----------------------------------------------------------------------

    #[test]
    fn test_contact_new() {
        let c = Contact::new(1, "John", "Doe");
        assert_eq!(c.first_name, "John");
        assert_eq!(c.last_name, "Doe");
        assert_eq!(c.display_name, "John Doe");
        assert!(c.phones.is_empty());
        assert!(c.emails.is_empty());
        assert!(!c.favorite);
    }

    #[test]
    fn test_contact_new_first_name_only() {
        let c = Contact::new(2, "Madonna", "");
        assert_eq!(c.display_name, "Madonna");
    }

    #[test]
    fn test_contact_new_last_name_only() {
        let c = Contact::new(3, "", "Prince");
        assert_eq!(c.display_name, "Prince");
    }

    #[test]
    fn test_contact_computed_display_name_custom() {
        let mut c = Contact::new(4, "John", "Doe");
        c.display_name = String::from("Johnny D");
        assert_eq!(c.computed_display_name(), "Johnny D");
    }

    #[test]
    fn test_contact_computed_display_name_fallback_company() {
        let mut c = Contact::new(5, "", "");
        c.display_name.clear();
        c.company = String::from("ACME");
        assert_eq!(c.computed_display_name(), "ACME");
    }

    #[test]
    fn test_contact_computed_display_name_unnamed() {
        let mut c = Contact::new(6, "", "");
        c.display_name.clear();
        assert_eq!(c.computed_display_name(), "(unnamed)");
    }

    #[test]
    fn test_contact_sort_key_name() {
        let c = Contact::new(7, "Alice", "Baker");
        assert_eq!(c.sort_key_name(), "baker alice");
    }

    #[test]
    fn test_contact_sort_key_name_last_only() {
        let c = Contact::new(8, "", "Zoe");
        assert_eq!(c.sort_key_name(), "zoe");
    }

    #[test]
    fn test_contact_sort_key_name_first_only() {
        let c = Contact::new(9, "Alice", "");
        assert_eq!(c.sort_key_name(), "alice");
    }

    #[test]
    fn test_contact_first_letter() {
        let c = Contact::new(10, "Alice", "Baker");
        assert_eq!(c.first_letter(), 'B');
    }

    #[test]
    fn test_contact_first_letter_non_alpha() {
        let c = Contact::new(11, "123", "");
        assert_eq!(c.first_letter(), '#');
    }

    #[test]
    fn test_contact_initials_both_names() {
        let c = Contact::new(12, "John", "Doe");
        assert_eq!(c.initials(), "JD");
    }

    #[test]
    fn test_contact_initials_first_only() {
        let c = Contact::new(13, "Madonna", "");
        assert_eq!(c.initials(), "M");
    }

    #[test]
    fn test_contact_initials_company_fallback() {
        let mut c = Contact::new(14, "", "");
        c.company = String::from("Acme");
        assert_eq!(c.initials(), "A");
    }

    #[test]
    fn test_contact_initials_empty() {
        let c = Contact::new(15, "", "");
        assert_eq!(c.initials(), "?");
    }

    // -----------------------------------------------------------------------
    // Phone number
    // -----------------------------------------------------------------------

    #[test]
    fn test_phone_number_new() {
        let p = PhoneNumber::new("+1-555-0100", PhoneType::Mobile);
        assert_eq!(p.number, "+1-555-0100");
        assert_eq!(p.phone_type, PhoneType::Mobile);
        assert!(!p.primary);
    }

    #[test]
    fn test_phone_number_with_primary() {
        let p = PhoneNumber::new("5550100", PhoneType::Work).with_primary(true);
        assert!(p.primary);
    }

    #[test]
    fn test_phone_type_label() {
        assert_eq!(PhoneType::Mobile.label(), "Mobile");
        assert_eq!(PhoneType::Home.label(), "Home");
        assert_eq!(PhoneType::Work.label(), "Work");
        assert_eq!(PhoneType::Fax.label(), "Fax");
        assert_eq!(PhoneType::Other.label(), "Other");
    }

    #[test]
    fn test_phone_type_vcard_roundtrip() {
        for ptype in &[PhoneType::Mobile, PhoneType::Home, PhoneType::Work, PhoneType::Fax] {
            let vcard = ptype.to_vcard();
            let parsed = PhoneType::from_vcard(vcard);
            assert_eq!(*ptype, parsed);
        }
    }

    // -----------------------------------------------------------------------
    // Email address
    // -----------------------------------------------------------------------

    #[test]
    fn test_email_address_new() {
        let e = EmailAddress::new("test@example.com", EmailType::Personal);
        assert_eq!(e.email, "test@example.com");
        assert_eq!(e.email_type, EmailType::Personal);
        assert!(!e.primary);
    }

    #[test]
    fn test_email_address_with_primary() {
        let e = EmailAddress::new("x@y.com", EmailType::Work).with_primary(true);
        assert!(e.primary);
    }

    #[test]
    fn test_email_type_label() {
        assert_eq!(EmailType::Personal.label(), "Personal");
        assert_eq!(EmailType::Work.label(), "Work");
        assert_eq!(EmailType::Other.label(), "Other");
    }

    #[test]
    fn test_email_type_vcard_roundtrip() {
        let e = EmailType::Work;
        let vcard = e.to_vcard();
        let parsed = EmailType::from_vcard(vcard);
        assert_eq!(e, parsed);
    }

    // -----------------------------------------------------------------------
    // Postal address
    // -----------------------------------------------------------------------

    #[test]
    fn test_postal_address_new_is_empty() {
        let a = PostalAddress::new(AddressType::Home);
        assert!(a.is_empty());
    }

    #[test]
    fn test_postal_address_display_line() {
        let mut a = PostalAddress::new(AddressType::Work);
        a.street = String::from("123 Main");
        a.city = String::from("NYC");
        a.state = String::from("NY");
        a.zip = String::from("10001");
        assert_eq!(a.display_line(), "123 Main, NYC, NY, 10001");
    }

    #[test]
    fn test_postal_address_display_line_partial() {
        let mut a = PostalAddress::new(AddressType::Home);
        a.city = String::from("London");
        a.country = String::from("UK");
        assert_eq!(a.display_line(), "London, UK");
    }

    #[test]
    fn test_postal_address_vcard_roundtrip() {
        let mut a = PostalAddress::new(AddressType::Home);
        a.street = String::from("123 Oak St");
        a.city = String::from("Springfield");
        a.state = String::from("IL");
        a.zip = String::from("62704");
        a.country = String::from("US");

        let vcard = a.to_vcard_adr();
        let parsed = PostalAddress::from_vcard_adr(&vcard);
        assert_eq!(parsed.street, "123 Oak St");
        assert_eq!(parsed.city, "Springfield");
        assert_eq!(parsed.state, "IL");
        assert_eq!(parsed.zip, "62704");
        assert_eq!(parsed.country, "US");
    }

    #[test]
    fn test_address_type_label() {
        assert_eq!(AddressType::Home.label(), "Home");
        assert_eq!(AddressType::Work.label(), "Work");
        assert_eq!(AddressType::Other.label(), "Other");
    }

    // -----------------------------------------------------------------------
    // Social account
    // -----------------------------------------------------------------------

    #[test]
    fn test_social_account_new() {
        let s = SocialAccount::new(SocialPlatform::GitHub, "@user");
        assert_eq!(s.handle, "@user");
        assert_eq!(s.platform.label(), "GitHub");
    }

    #[test]
    fn test_social_platform_custom() {
        let p = SocialPlatform::Custom(String::from("MyNet"));
        assert_eq!(p.label(), "MyNet");
    }

    // -----------------------------------------------------------------------
    // Contact group
    // -----------------------------------------------------------------------

    #[test]
    fn test_contact_group_new() {
        let g = ContactGroup::new(1, "Friends");
        assert_eq!(g.name, "Friends");
        assert_eq!(g.member_count, 0);
    }

    #[test]
    fn test_contact_group_with_color() {
        let g = ContactGroup::new(1, "Work").with_color(RED);
        assert_eq!(g.color, RED);
    }

    #[test]
    fn test_contact_group_with_description() {
        let g = ContactGroup::new(1, "VIPs").with_description("Important contacts");
        assert_eq!(g.description, "Important contacts");
    }

    // -----------------------------------------------------------------------
    // SimpleDate / Birthday
    // -----------------------------------------------------------------------

    #[test]
    fn test_simple_date_new_valid() {
        let d = SimpleDate::new(2000, 6, 15);
        assert!(d.is_some());
        let d = d.unwrap();
        assert_eq!(d.year, 2000);
        assert_eq!(d.month, 6);
        assert_eq!(d.day, 15);
    }

    #[test]
    fn test_simple_date_new_invalid_month() {
        assert!(SimpleDate::new(2000, 0, 15).is_none());
        assert!(SimpleDate::new(2000, 13, 15).is_none());
    }

    #[test]
    fn test_simple_date_new_invalid_day() {
        assert!(SimpleDate::new(2000, 6, 0).is_none());
        assert!(SimpleDate::new(2000, 6, 32).is_none());
    }

    #[test]
    fn test_simple_date_format_display() {
        let d = SimpleDate::new(2000, 3, 5).unwrap();
        assert_eq!(d.format_display(), "2000-03-05");
    }

    #[test]
    fn test_simple_date_parse() {
        let d = SimpleDate::parse("1990-12-25");
        assert!(d.is_some());
        let d = d.unwrap();
        assert_eq!(d.year, 1990);
        assert_eq!(d.month, 12);
        assert_eq!(d.day, 25);
    }

    #[test]
    fn test_simple_date_parse_invalid() {
        assert!(SimpleDate::parse("not-a-date").is_none());
        assert!(SimpleDate::parse("2000/01/01").is_none());
        assert!(SimpleDate::parse("2000-13-01").is_none());
    }

    #[test]
    fn test_simple_date_parse_roundtrip() {
        let d = SimpleDate::new(2024, 1, 31).unwrap();
        let s = d.format_display();
        let d2 = SimpleDate::parse(&s).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn test_birthday_upcoming_same_day() {
        let b = SimpleDate::new(1990, 6, 15).unwrap();
        assert!(b.is_upcoming_within(6, 15, 0));
    }

    #[test]
    fn test_birthday_upcoming_within_range() {
        let b = SimpleDate::new(1990, 6, 20).unwrap();
        assert!(b.is_upcoming_within(6, 15, 7));
    }

    #[test]
    fn test_birthday_upcoming_past() {
        let b = SimpleDate::new(1990, 6, 10).unwrap();
        // June 10 is before June 15, so it wraps around
        assert!(!b.is_upcoming_within(6, 15, 7));
    }

    #[test]
    fn test_birthday_upcoming_year_wrap() {
        // Birthday in January, current date in December
        let b = SimpleDate::new(1990, 1, 5).unwrap();
        assert!(b.is_upcoming_within(12, 28, 15));
    }

    // -----------------------------------------------------------------------
    // Contact search
    // -----------------------------------------------------------------------

    #[test]
    fn test_search_by_first_name() {
        let mut c = make_contact("Alice", "Anderson");
        c.company = String::from("Acme");
        assert!(c.matches_search("alice"));
        assert!(c.matches_search("Ali"));
    }

    #[test]
    fn test_search_by_last_name() {
        let c = make_contact("Alice", "Anderson");
        assert!(c.matches_search("anderson"));
    }

    #[test]
    fn test_search_by_company() {
        let mut c = make_contact("Alice", "Anderson");
        c.company = String::from("Acme Corp");
        assert!(c.matches_search("acme"));
    }

    #[test]
    fn test_search_by_phone() {
        let mut c = make_contact("Alice", "Anderson");
        c.phones.push(PhoneNumber::new("+1-555-0101", PhoneType::Mobile));
        assert!(c.matches_search("555-0101"));
    }

    #[test]
    fn test_search_by_email() {
        let mut c = make_contact("Alice", "Anderson");
        c.emails.push(EmailAddress::new("alice@example.com", EmailType::Personal));
        assert!(c.matches_search("alice@example"));
    }

    #[test]
    fn test_search_by_notes() {
        let mut c = make_contact("Alice", "Anderson");
        c.notes = String::from("Met at conference");
        assert!(c.matches_search("conference"));
    }

    #[test]
    fn test_search_by_nickname() {
        let mut c = make_contact("Alice", "Anderson");
        c.nickname = String::from("Ally");
        assert!(c.matches_search("ally"));
    }

    #[test]
    fn test_search_empty_query_matches_all() {
        let c = make_contact("Alice", "Anderson");
        assert!(c.matches_search(""));
    }

    #[test]
    fn test_search_no_match() {
        let c = make_contact("Alice", "Anderson");
        assert!(!c.matches_search("zzz_nonexistent"));
    }

    #[test]
    fn test_search_case_insensitive() {
        let c = make_contact("Alice", "Anderson");
        assert!(c.matches_search("ALICE"));
        assert!(c.matches_search("aLiCe"));
    }

    // -----------------------------------------------------------------------
    // Contact store CRUD
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_add_contact() {
        let mut store = ContactStore::new();
        let c = make_contact("Alice", "Anderson");
        let id = store.add_contact(c);
        assert_eq!(id, 1);
        assert_eq!(store.contact_count(), 1);
    }

    #[test]
    fn test_store_add_multiple_contacts() {
        let mut store = ContactStore::new();
        let id1 = store.add_contact(make_contact("Alice", "A"));
        let id2 = store.add_contact(make_contact("Bob", "B"));
        assert_ne!(id1, id2);
        assert_eq!(store.contact_count(), 2);
    }

    #[test]
    fn test_store_get_contact() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "Anderson"));
        let c = store.get_contact(id).unwrap();
        assert_eq!(c.first_name, "Alice");
    }

    #[test]
    fn test_store_get_contact_not_found() {
        let store = ContactStore::new();
        assert!(store.get_contact(999).is_none());
    }

    #[test]
    fn test_store_get_contact_mut() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "Anderson"));
        let c = store.get_contact_mut(id).unwrap();
        c.first_name = String::from("Alicia");
        assert_eq!(store.get_contact(id).unwrap().first_name, "Alicia");
    }

    #[test]
    fn test_store_delete_contact() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "Anderson"));
        assert!(store.delete_contact(id));
        assert_eq!(store.contact_count(), 0);
        assert!(store.get_contact(id).is_none());
    }

    #[test]
    fn test_store_delete_contact_not_found() {
        let mut store = ContactStore::new();
        assert!(!store.delete_contact(999));
    }

    #[test]
    fn test_store_update_contact() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "Anderson"));
        let mut updated = store.get_contact(id).unwrap().clone();
        updated.company = String::from("New Corp");
        assert!(store.update_contact(updated));
        assert_eq!(store.get_contact(id).unwrap().company, "New Corp");
    }

    #[test]
    fn test_store_update_contact_not_found() {
        let mut store = ContactStore::new();
        let c = Contact::new(999, "Ghost", "Contact");
        assert!(!store.update_contact(c));
    }

    // -----------------------------------------------------------------------
    // Group CRUD
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_add_group() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Friends"));
        assert_eq!(gid, 1);
        assert_eq!(store.all_groups().len(), 1);
    }

    #[test]
    fn test_store_get_group() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Family"));
        let g = store.get_group(gid).unwrap();
        assert_eq!(g.name, "Family");
    }

    #[test]
    fn test_store_get_group_not_found() {
        let store = ContactStore::new();
        assert!(store.get_group(999).is_none());
    }

    #[test]
    fn test_store_delete_group() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Work"));
        // Add a contact to this group
        let cid = store.add_contact(make_contact("Alice", "A"));
        store.add_contact_to_group(cid, gid);
        // Delete the group
        assert!(store.delete_group(gid));
        assert!(store.get_group(gid).is_none());
        // Contact should no longer reference the group
        assert!(!store.get_contact(cid).unwrap().groups.contains(&gid));
    }

    #[test]
    fn test_store_delete_group_not_found() {
        let mut store = ContactStore::new();
        assert!(!store.delete_group(999));
    }

    #[test]
    fn test_store_add_contact_to_group() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Friends"));
        let cid = store.add_contact(make_contact("Alice", "A"));
        assert!(store.add_contact_to_group(cid, gid));
        assert!(store.get_contact(cid).unwrap().groups.contains(&gid));
    }

    #[test]
    fn test_store_add_contact_to_group_duplicate() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Friends"));
        let cid = store.add_contact(make_contact("Alice", "A"));
        store.add_contact_to_group(cid, gid);
        // Adding again should return false (already member)
        assert!(!store.add_contact_to_group(cid, gid));
    }

    #[test]
    fn test_store_remove_contact_from_group() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Friends"));
        let cid = store.add_contact(make_contact("Alice", "A"));
        store.add_contact_to_group(cid, gid);
        assert!(store.remove_contact_from_group(cid, gid));
        assert!(!store.get_contact(cid).unwrap().groups.contains(&gid));
    }

    #[test]
    fn test_store_remove_contact_from_group_not_member() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Friends"));
        let cid = store.add_contact(make_contact("Alice", "A"));
        assert!(!store.remove_contact_from_group(cid, gid));
    }

    #[test]
    fn test_store_refresh_group_counts() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Team"));
        let cid1 = store.add_contact(make_contact("A", "A"));
        let cid2 = store.add_contact(make_contact("B", "B"));
        store.add_contact_to_group(cid1, gid);
        store.add_contact_to_group(cid2, gid);
        store.refresh_group_counts();
        assert_eq!(store.get_group(gid).unwrap().member_count, 2);
    }

    #[test]
    fn test_store_group_stats() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Team"));
        let cid = store.add_contact(make_contact("A", "A"));
        store.add_contact_to_group(cid, gid);
        let stats = store.group_stats();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].2, 1);
    }

    // -----------------------------------------------------------------------
    // Search in store
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_search_by_name() {
        let store = make_store_with_contacts();
        let results = store.search("alice");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].first_name, "Alice");
    }

    #[test]
    fn test_store_search_by_company() {
        let store = make_store_with_contacts();
        let results = store.search("baker");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_store_search_empty_returns_all() {
        let store = make_store_with_contacts();
        let results = store.search("");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_store_search_no_results() {
        let store = make_store_with_contacts();
        let results = store.search("zzzzzzz");
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // Sort
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_sort_by_name() {
        let store = make_store_with_contacts();
        let sorted = store.sorted_contacts(SortOrder::Name);
        assert_eq!(sorted[0].first_name, "Alice");
        assert_eq!(sorted[1].first_name, "Bob");
        assert_eq!(sorted[2].first_name, "Carol");
    }

    #[test]
    fn test_store_sort_by_company() {
        let store = make_store_with_contacts();
        let sorted = store.sorted_contacts(SortOrder::Company);
        // Empty company sorts first, then "Acme Corp", then "Baker Inc"
        assert_eq!(sorted[0].first_name, "Carol"); // no company
        assert_eq!(sorted[1].first_name, "Alice"); // "Acme Corp"
        assert_eq!(sorted[2].first_name, "Bob"); // "Baker Inc"
    }

    #[test]
    fn test_store_sort_by_recently_added() {
        let store = make_store_with_contacts();
        let sorted = store.sorted_contacts(SortOrder::RecentlyAdded);
        assert_eq!(sorted[0].first_name, "Carol"); // created_at 3000
        assert_eq!(sorted[1].first_name, "Bob"); // created_at 2000
        assert_eq!(sorted[2].first_name, "Alice"); // created_at 1000
    }

    #[test]
    fn test_store_sort_by_recently_contacted() {
        let mut store = make_store_with_contacts();
        store.mark_contacted(2, 5000); // Bob contacted most recently
        store.mark_contacted(1, 3000); // Alice contacted earlier
        let sorted = store.sorted_contacts(SortOrder::RecentlyContacted);
        assert_eq!(sorted[0].first_name, "Bob");
        assert_eq!(sorted[1].first_name, "Alice");
    }

    // -----------------------------------------------------------------------
    // Filter
    // -----------------------------------------------------------------------

    #[test]
    fn test_filter_all() {
        let store = make_store_with_contacts();
        let f = ContactFilter::All;
        let results = store.filtered_sorted(&f, SortOrder::Name, "");
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_filter_has_phone() {
        let mut store = ContactStore::new();
        let mut c1 = make_contact("Alice", "A");
        c1.phones.push(PhoneNumber::new("123", PhoneType::Mobile));
        store.add_contact(c1);
        store.add_contact(make_contact("Bob", "B")); // no phone

        let f = ContactFilter::HasPhone;
        let results = store.filtered_sorted(&f, SortOrder::Name, "");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].first_name, "Alice");
    }

    #[test]
    fn test_filter_has_email() {
        let mut store = ContactStore::new();
        let mut c1 = make_contact("Alice", "A");
        c1.emails.push(EmailAddress::new("a@b.com", EmailType::Personal));
        store.add_contact(c1);
        store.add_contact(make_contact("Bob", "B")); // no email

        let f = ContactFilter::HasEmail;
        let results = store.filtered_sorted(&f, SortOrder::Name, "");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_filter_favorites() {
        let store = make_store_with_contacts();
        let f = ContactFilter::Favorites;
        let results = store.filtered_sorted(&f, SortOrder::Name, "");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].first_name, "Carol");
    }

    #[test]
    fn test_filter_by_group() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Team"));
        let cid1 = store.add_contact(make_contact("Alice", "A"));
        store.add_contact(make_contact("Bob", "B"));
        store.add_contact_to_group(cid1, gid);

        let f = ContactFilter::Group(gid);
        let results = store.filtered_sorted(&f, SortOrder::Name, "");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].first_name, "Alice");
    }

    #[test]
    fn test_filter_combined_with_search() {
        let mut store = ContactStore::new();
        let mut c1 = make_contact("Alice", "A");
        c1.phones.push(PhoneNumber::new("123", PhoneType::Mobile));
        store.add_contact(c1);
        let mut c2 = make_contact("Bob", "B");
        c2.phones.push(PhoneNumber::new("456", PhoneType::Mobile));
        store.add_contact(c2);

        let f = ContactFilter::HasPhone;
        let results = store.filtered_sorted(&f, SortOrder::Name, "alice");
        assert_eq!(results.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Favorites
    // -----------------------------------------------------------------------

    #[test]
    fn test_toggle_favorite() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "A"));
        assert!(!store.get_contact(id).unwrap().favorite);
        let new_state = store.toggle_favorite(id);
        assert_eq!(new_state, Some(true));
        assert!(store.get_contact(id).unwrap().favorite);
    }

    #[test]
    fn test_toggle_favorite_off() {
        let mut store = ContactStore::new();
        let mut c = make_contact("Alice", "A");
        c.favorite = true;
        let id = store.add_contact(c);
        let new_state = store.toggle_favorite(id);
        assert_eq!(new_state, Some(false));
    }

    #[test]
    fn test_toggle_favorite_not_found() {
        let mut store = ContactStore::new();
        assert!(store.toggle_favorite(999).is_none());
    }

    #[test]
    fn test_favorites_list() {
        let store = make_store_with_contacts();
        let favs = store.favorites();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].first_name, "Carol");
    }

    // -----------------------------------------------------------------------
    // Recently viewed
    // -----------------------------------------------------------------------

    #[test]
    fn test_record_view() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "A"));
        store.record_view(id);
        assert_eq!(store.recently_viewed().len(), 1);
        assert_eq!(*store.recently_viewed().front().unwrap(), id);
    }

    #[test]
    fn test_record_view_deduplicates() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "A"));
        store.record_view(id);
        store.record_view(id);
        assert_eq!(store.recently_viewed().len(), 1);
    }

    #[test]
    fn test_record_view_most_recent_first() {
        let mut store = ContactStore::new();
        let id1 = store.add_contact(make_contact("Alice", "A"));
        let id2 = store.add_contact(make_contact("Bob", "B"));
        store.record_view(id1);
        store.record_view(id2);
        assert_eq!(*store.recently_viewed().front().unwrap(), id2);
    }

    #[test]
    fn test_record_view_max_limit() {
        let mut store = ContactStore::new();
        for i in 0..15 {
            let id = store.add_contact(make_contact(&format!("User{i}"), "X"));
            store.record_view(id);
        }
        assert!(store.recently_viewed().len() <= MAX_RECENT);
    }

    #[test]
    fn test_recently_viewed_contacts() {
        let mut store = ContactStore::new();
        let id1 = store.add_contact(make_contact("Alice", "A"));
        store.record_view(id1);
        let rvcs = store.recently_viewed_contacts();
        assert_eq!(rvcs.len(), 1);
        assert_eq!(rvcs[0].first_name, "Alice");
    }

    #[test]
    fn test_recently_viewed_cleared_on_delete() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "A"));
        store.record_view(id);
        store.delete_contact(id);
        assert!(store.recently_viewed().is_empty());
    }

    // -----------------------------------------------------------------------
    // Recently contacted
    // -----------------------------------------------------------------------

    #[test]
    fn test_mark_contacted() {
        let mut store = ContactStore::new();
        let id = store.add_contact(make_contact("Alice", "A"));
        store.mark_contacted(id, 12345);
        assert_eq!(store.get_contact(id).unwrap().last_contacted, Some(12345));
    }

    #[test]
    fn test_mark_contacted_nonexistent() {
        let mut store = ContactStore::new();
        store.mark_contacted(999, 12345); // should not panic
    }

    // -----------------------------------------------------------------------
    // Duplicate detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_duplicate_same_name() {
        let contacts = vec![
            Contact::new(1, "Alice", "Anderson"),
            Contact::new(2, "Alice", "Anderson"),
        ];
        let dups = find_duplicates(&contacts);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].reason, DuplicateReason::SameName);
    }

    #[test]
    fn test_duplicate_same_name_and_company() {
        let mut c1 = Contact::new(1, "Alice", "Anderson");
        c1.company = String::from("Acme");
        let mut c2 = Contact::new(2, "Alice", "Anderson");
        c2.company = String::from("Acme");
        let dups = find_duplicates(&[c1, c2]);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].reason, DuplicateReason::SameNameAndCompany);
        assert!(dups[0].confidence > 0.9);
    }

    #[test]
    fn test_duplicate_same_phone() {
        let mut c1 = Contact::new(1, "Alice", "A");
        c1.phones.push(PhoneNumber::new("+1-555-0100", PhoneType::Mobile));
        let mut c2 = Contact::new(2, "Bob", "B");
        c2.phones.push(PhoneNumber::new("15550100", PhoneType::Work)); // same digits
        let dups = find_duplicates(&[c1, c2]);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].reason, DuplicateReason::SamePhone);
    }

    #[test]
    fn test_duplicate_same_email() {
        let mut c1 = Contact::new(1, "Alice", "A");
        c1.emails.push(EmailAddress::new("same@example.com", EmailType::Personal));
        let mut c2 = Contact::new(2, "Bob", "B");
        c2.emails.push(EmailAddress::new("SAME@EXAMPLE.COM", EmailType::Work));
        let dups = find_duplicates(&[c1, c2]);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].reason, DuplicateReason::SameEmail);
    }

    #[test]
    fn test_no_duplicates() {
        let contacts = vec![
            Contact::new(1, "Alice", "Anderson"),
            Contact::new(2, "Bob", "Baker"),
        ];
        let dups = find_duplicates(&contacts);
        assert!(dups.is_empty());
    }

    #[test]
    fn test_duplicate_empty_names_not_matched() {
        // Contacts with empty names should not be treated as duplicates
        let contacts = vec![Contact::new(1, "", ""), Contact::new(2, "", "")];
        let dups = find_duplicates(&contacts);
        assert!(dups.is_empty());
    }

    #[test]
    fn test_store_find_duplicates() {
        let mut store = ContactStore::new();
        store.add_contact(Contact::new(0, "Alice", "Anderson"));
        store.add_contact(Contact::new(0, "Alice", "Anderson"));
        let dups = store.find_duplicates();
        assert_eq!(dups.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Merge contacts
    // -----------------------------------------------------------------------

    #[test]
    fn test_merge_contacts_phones() {
        let mut c1 = Contact::new(1, "Alice", "Anderson");
        c1.phones.push(PhoneNumber::new("111", PhoneType::Mobile));
        let mut c2 = Contact::new(2, "Alice", "Anderson");
        c2.phones.push(PhoneNumber::new("222", PhoneType::Work));
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.phones.len(), 2);
    }

    #[test]
    fn test_merge_contacts_dedup_phones() {
        let mut c1 = Contact::new(1, "Alice", "Anderson");
        c1.phones.push(PhoneNumber::new("+1-555-0100", PhoneType::Mobile));
        let mut c2 = Contact::new(2, "Alice", "Anderson");
        c2.phones.push(PhoneNumber::new("15550100", PhoneType::Work)); // same digits
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.phones.len(), 1);
    }

    #[test]
    fn test_merge_contacts_emails() {
        let mut c1 = Contact::new(1, "A", "A");
        c1.emails.push(EmailAddress::new("a@a.com", EmailType::Personal));
        let mut c2 = Contact::new(2, "A", "A");
        c2.emails.push(EmailAddress::new("b@b.com", EmailType::Work));
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.emails.len(), 2);
    }

    #[test]
    fn test_merge_contacts_dedup_emails() {
        let mut c1 = Contact::new(1, "A", "A");
        c1.emails.push(EmailAddress::new("same@test.com", EmailType::Personal));
        let mut c2 = Contact::new(2, "A", "A");
        c2.emails.push(EmailAddress::new("SAME@TEST.COM", EmailType::Work));
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.emails.len(), 1);
    }

    #[test]
    fn test_merge_contacts_fills_empty_fields() {
        let c1 = Contact::new(1, "Alice", "Anderson");
        let mut c2 = Contact::new(2, "Alice", "Anderson");
        c2.nickname = String::from("Ally");
        c2.company = String::from("Acme");
        c2.job_title = String::from("Engineer");
        c2.birthday = SimpleDate::new(1990, 1, 1);
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.nickname, "Ally");
        assert_eq!(merged.company, "Acme");
        assert_eq!(merged.job_title, "Engineer");
        assert!(merged.birthday.is_some());
    }

    #[test]
    fn test_merge_contacts_preserves_primary_fields() {
        let mut c1 = Contact::new(1, "Alice", "Anderson");
        c1.company = String::from("Primary Corp");
        let mut c2 = Contact::new(2, "Alice", "Anderson");
        c2.company = String::from("Secondary Corp");
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.company, "Primary Corp");
    }

    #[test]
    fn test_merge_contacts_groups() {
        let mut c1 = Contact::new(1, "A", "A");
        c1.groups.push(1);
        let mut c2 = Contact::new(2, "A", "A");
        c2.groups.push(2);
        c2.groups.push(1); // duplicate
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.groups.len(), 2);
        assert!(merged.groups.contains(&1));
        assert!(merged.groups.contains(&2));
    }

    #[test]
    fn test_merge_contacts_favorite() {
        let c1 = Contact::new(1, "A", "A");
        let mut c2 = Contact::new(2, "A", "A");
        c2.favorite = true;
        let merged = merge_contacts(&c1, &c2, 3);
        assert!(merged.favorite);
    }

    #[test]
    fn test_store_merge_contacts() {
        let mut store = ContactStore::new();
        let id1 = store.add_contact(Contact::new(0, "Alice", "Anderson"));
        let id2 = store.add_contact(Contact::new(0, "Alice", "Anderson"));
        let merged_id = store.merge_contacts(id1, id2);
        assert!(merged_id.is_some());
        assert_eq!(store.contact_count(), 1);
        assert!(store.get_contact(id1).is_none());
        assert!(store.get_contact(id2).is_none());
    }

    #[test]
    fn test_store_merge_contacts_not_found() {
        let mut store = ContactStore::new();
        let id1 = store.add_contact(make_contact("A", "A"));
        assert!(store.merge_contacts(id1, 999).is_none());
    }

    // -----------------------------------------------------------------------
    // vCard export
    // -----------------------------------------------------------------------

    #[test]
    fn test_vcard_export_basic() {
        let c = Contact::new(1, "John", "Doe");
        let vcard = c.to_vcard();
        assert!(vcard.contains("BEGIN:VCARD"));
        assert!(vcard.contains("VERSION:3.0"));
        assert!(vcard.contains("N:Doe;John;;;"));
        assert!(vcard.contains("FN:John Doe"));
        assert!(vcard.contains("END:VCARD"));
    }

    #[test]
    fn test_vcard_export_with_phone() {
        let mut c = Contact::new(1, "John", "Doe");
        c.phones.push(PhoneNumber::new("+1-555-0100", PhoneType::Mobile).with_primary(true));
        let vcard = c.to_vcard();
        assert!(vcard.contains("TEL;TYPE=CELL;PREF:+1-555-0100"));
    }

    #[test]
    fn test_vcard_export_with_email() {
        let mut c = Contact::new(1, "John", "Doe");
        c.emails.push(EmailAddress::new("john@example.com", EmailType::Work));
        let vcard = c.to_vcard();
        assert!(vcard.contains("EMAIL;TYPE=WORK:john@example.com"));
    }

    #[test]
    fn test_vcard_export_with_org() {
        let mut c = Contact::new(1, "John", "Doe");
        c.company = String::from("Acme");
        c.department = String::from("Engineering");
        let vcard = c.to_vcard();
        assert!(vcard.contains("ORG:Acme;Engineering"));
    }

    #[test]
    fn test_vcard_export_with_birthday() {
        let mut c = Contact::new(1, "John", "Doe");
        c.birthday = SimpleDate::new(1990, 12, 25);
        let vcard = c.to_vcard();
        assert!(vcard.contains("BDAY:1990-12-25"));
    }

    #[test]
    fn test_vcard_export_with_address() {
        let mut c = Contact::new(1, "John", "Doe");
        let mut addr = PostalAddress::new(AddressType::Home);
        addr.street = String::from("123 Main");
        addr.city = String::from("NYC");
        c.addresses.push(addr);
        let vcard = c.to_vcard();
        assert!(vcard.contains("ADR;TYPE=HOME:"));
        assert!(vcard.contains("123 Main"));
    }

    #[test]
    fn test_vcard_export_with_notes() {
        let mut c = Contact::new(1, "John", "Doe");
        c.notes = String::from("A note");
        let vcard = c.to_vcard();
        assert!(vcard.contains("NOTE:A note"));
    }

    #[test]
    fn test_vcard_export_with_social() {
        let mut c = Contact::new(1, "John", "Doe");
        c.social_accounts.push(SocialAccount::new(SocialPlatform::Twitter, "@johnd"));
        let vcard = c.to_vcard();
        assert!(vcard.contains("X-SOCIALPROFILE;TYPE=Twitter:@johnd"));
    }

    // -----------------------------------------------------------------------
    // vCard import
    // -----------------------------------------------------------------------

    #[test]
    fn test_vcard_import_basic() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Doe;John;;;\r\nFN:John Doe\r\nEND:VCARD";
        let c = Contact::from_vcard(data, 1).unwrap();
        assert_eq!(c.first_name, "John");
        assert_eq!(c.last_name, "Doe");
        assert_eq!(c.display_name, "John Doe");
    }

    #[test]
    fn test_vcard_import_with_phone() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Doe;John;;;\r\nFN:John Doe\r\nTEL;TYPE=CELL;PREF:+1-555-0100\r\nEND:VCARD";
        let c = Contact::from_vcard(data, 1).unwrap();
        assert_eq!(c.phones.len(), 1);
        assert_eq!(c.phones[0].number, "+1-555-0100");
        assert_eq!(c.phones[0].phone_type, PhoneType::Mobile);
        assert!(c.phones[0].primary);
    }

    #[test]
    fn test_vcard_import_with_email() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Doe;John;;;\r\nFN:John\r\nEMAIL;TYPE=WORK:john@work.com\r\nEND:VCARD";
        let c = Contact::from_vcard(data, 1).unwrap();
        assert_eq!(c.emails.len(), 1);
        assert_eq!(c.emails[0].email, "john@work.com");
        assert_eq!(c.emails[0].email_type, EmailType::Work);
    }

    #[test]
    fn test_vcard_import_with_org() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:;John;;;\r\nFN:John\r\nORG:Acme;Engineering\r\nTITLE:CTO\r\nEND:VCARD";
        let c = Contact::from_vcard(data, 1).unwrap();
        assert_eq!(c.company, "Acme");
        assert_eq!(c.department, "Engineering");
        assert_eq!(c.job_title, "CTO");
    }

    #[test]
    fn test_vcard_import_with_birthday() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:;John;;;\r\nFN:John\r\nBDAY:1990-06-15\r\nEND:VCARD";
        let c = Contact::from_vcard(data, 1).unwrap();
        assert_eq!(c.birthday.unwrap().year, 1990);
        assert_eq!(c.birthday.unwrap().month, 6);
        assert_eq!(c.birthday.unwrap().day, 15);
    }

    #[test]
    fn test_vcard_import_invalid_no_begin() {
        let data = "VERSION:3.0\r\nN:;John;;;\r\nFN:John\r\nEND:VCARD";
        assert!(Contact::from_vcard(data, 1).is_none());
    }

    #[test]
    fn test_vcard_import_invalid_no_end() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:;John;;;\r\nFN:John";
        assert!(Contact::from_vcard(data, 1).is_none());
    }

    #[test]
    fn test_vcard_roundtrip() {
        let mut c = Contact::new(1, "Jane", "Smith");
        c.company = String::from("TechCo");
        c.job_title = String::from("Dev");
        c.nickname = String::from("JS");
        c.phones.push(PhoneNumber::new("+1-555-0999", PhoneType::Work).with_primary(true));
        c.emails.push(EmailAddress::new("jane@tech.co", EmailType::Work).with_primary(true));
        c.birthday = SimpleDate::new(1988, 11, 3);
        c.notes = String::from("Test note");

        let vcard = c.to_vcard();
        let parsed = Contact::from_vcard(&vcard, 2).unwrap();

        assert_eq!(parsed.first_name, "Jane");
        assert_eq!(parsed.last_name, "Smith");
        assert_eq!(parsed.company, "TechCo");
        assert_eq!(parsed.job_title, "Dev");
        assert_eq!(parsed.nickname, "JS");
        assert_eq!(parsed.phones.len(), 1);
        assert_eq!(parsed.phones[0].number, "+1-555-0999");
        assert!(parsed.phones[0].primary);
        assert_eq!(parsed.emails.len(), 1);
        assert_eq!(parsed.emails[0].email, "jane@tech.co");
        assert_eq!(parsed.birthday.unwrap().year, 1988);
        assert_eq!(parsed.notes, "Test note");
    }

    #[test]
    fn test_import_multiple_vcards() {
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Doe;John;;;\r\nFN:John Doe\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nN:Smith;Jane;;;\r\nFN:Jane Smith\r\nEND:VCARD";
        let contacts = import_vcards(data, 100);
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].first_name, "John");
        assert_eq!(contacts[1].first_name, "Jane");
    }

    #[test]
    fn test_export_multiple_vcards() {
        let c1 = Contact::new(1, "John", "Doe");
        let c2 = Contact::new(2, "Jane", "Smith");
        let output = export_vcards(&[c1, c2]);
        let count = output.matches("BEGIN:VCARD").count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_store_import_vcards() {
        let mut store = ContactStore::new();
        let data = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Doe;John;;;\r\nFN:John Doe\r\nEND:VCARD";
        let count = store.import_vcards(data);
        assert_eq!(count, 1);
        assert_eq!(store.contact_count(), 1);
    }

    #[test]
    fn test_store_export_all() {
        let mut store = ContactStore::new();
        store.add_contact(Contact::new(0, "Alice", "A"));
        store.add_contact(Contact::new(0, "Bob", "B"));
        let output = store.export_all();
        assert!(output.contains("BEGIN:VCARD"));
        assert_eq!(output.matches("END:VCARD").count(), 2);
    }

    // -----------------------------------------------------------------------
    // vCard escape/unescape
    // -----------------------------------------------------------------------

    #[test]
    fn test_vcard_escape() {
        assert_eq!(vcard_escape("hello, world"), "hello\\, world");
        assert_eq!(vcard_escape("a;b"), "a\\;b");
        assert_eq!(vcard_escape("line\nnewline"), "line\\nnewline");
        assert_eq!(vcard_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn test_vcard_unescape() {
        assert_eq!(vcard_unescape("hello\\, world"), "hello, world");
        assert_eq!(vcard_unescape("a\\;b"), "a;b");
        assert_eq!(vcard_unescape("line\\nnewline"), "line\nnewline");
        assert_eq!(vcard_unescape("back\\\\slash"), "back\\slash");
    }

    #[test]
    fn test_vcard_escape_roundtrip() {
        let original = "Hello, World; test\nnewline\\backslash";
        let escaped = vcard_escape(original);
        let unescaped = vcard_unescape(&escaped);
        assert_eq!(unescaped, original);
    }

    // -----------------------------------------------------------------------
    // Unfold vCard lines
    // -----------------------------------------------------------------------

    #[test]
    fn test_unfold_vcard_lines() {
        let data = "PROP:value\r\n continues here";
        let lines = unfold_vcard_lines(data);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "PROP:valuecontinues here");
    }

    #[test]
    fn test_unfold_vcard_no_continuation() {
        let data = "LINE1\nLINE2";
        let lines = unfold_vcard_lines(data);
        assert_eq!(lines.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Birthday reminders via store
    // -----------------------------------------------------------------------

    #[test]
    fn test_upcoming_birthdays() {
        let mut store = ContactStore::new();
        let mut c1 = make_contact("Alice", "A");
        c1.birthday = SimpleDate::new(1990, 6, 20);
        store.add_contact(c1);
        let mut c2 = make_contact("Bob", "B");
        c2.birthday = SimpleDate::new(1985, 12, 25);
        store.add_contact(c2);

        let upcoming = store.upcoming_birthdays(6, 15, 10);
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].first_name, "Alice");
    }

    #[test]
    fn test_upcoming_birthdays_none() {
        let mut store = ContactStore::new();
        let mut c = make_contact("Alice", "A");
        c.birthday = SimpleDate::new(1990, 12, 25);
        store.add_contact(c);

        let upcoming = store.upcoming_birthdays(6, 15, 10);
        assert!(upcoming.is_empty());
    }

    #[test]
    fn test_upcoming_birthdays_no_birthday() {
        let mut store = ContactStore::new();
        store.add_contact(make_contact("Alice", "A")); // no birthday set
        let upcoming = store.upcoming_birthdays(6, 15, 30);
        assert!(upcoming.is_empty());
    }

    // -----------------------------------------------------------------------
    // App state
    // -----------------------------------------------------------------------

    #[test]
    fn test_app_new() {
        let app = ContactsApp::new();
        assert_eq!(app.store.contact_count(), 0);
        assert_eq!(app.view, DetailView::Empty);
        assert_eq!(app.sort_order, SortOrder::Name);
        assert_eq!(app.filter, ContactFilter::All);
    }

    #[test]
    fn test_app_load_sample_data() {
        let mut app = ContactsApp::new();
        app.load_sample_data();
        assert!(app.store.contact_count() >= 5);
        assert!(app.store.all_groups().len() >= 3);
    }

    #[test]
    fn test_app_render_produces_commands() {
        let mut app = ContactsApp::new();
        app.load_sample_data();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_render_empty_state() {
        let app = ContactsApp::new();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_app_clear_edit_form() {
        let mut app = ContactsApp::new();
        app.edit_first_name = String::from("Test");
        app.edit_last_name = String::from("User");
        app.clear_edit_form();
        assert!(app.edit_first_name.is_empty());
        assert!(app.edit_last_name.is_empty());
    }

    #[test]
    fn test_app_load_edit_form() {
        let mut app = ContactsApp::new();
        let mut c = Contact::new(1, "John", "Doe");
        c.company = String::from("ACME");
        c.phones.push(PhoneNumber::new("123", PhoneType::Home));
        c.emails.push(EmailAddress::new("j@d.com", EmailType::Work));
        c.birthday = SimpleDate::new(1990, 5, 10);
        let mut addr = PostalAddress::new(AddressType::Work);
        addr.street = String::from("456 Elm");
        c.addresses.push(addr);

        app.load_edit_form(&c);
        assert_eq!(app.edit_first_name, "John");
        assert_eq!(app.edit_last_name, "Doe");
        assert_eq!(app.edit_company, "ACME");
        assert_eq!(app.edit_phone, "123");
        assert_eq!(app.edit_phone_type, PhoneType::Home);
        assert_eq!(app.edit_email, "j@d.com");
        assert_eq!(app.edit_email_type, EmailType::Work);
        assert_eq!(app.edit_birthday, "1990-05-10");
        assert_eq!(app.edit_street, "456 Elm");
        assert_eq!(app.edit_address_type, AddressType::Work);
    }

    #[test]
    fn test_app_build_contact_from_form() {
        let mut app = ContactsApp::new();
        app.edit_first_name = String::from("Jane");
        app.edit_last_name = String::from("Smith");
        app.edit_company = String::from("TechCo");
        app.edit_phone = String::from("555-0100");
        app.edit_phone_type = PhoneType::Work;
        app.edit_email = String::from("jane@tech.co");
        app.edit_email_type = EmailType::Work;
        app.edit_birthday = String::from("1988-03-15");
        app.edit_street = String::from("789 Pine");
        app.edit_city = String::from("Portland");

        let c = app.build_contact_from_form();
        assert_eq!(c.first_name, "Jane");
        assert_eq!(c.last_name, "Smith");
        assert_eq!(c.company, "TechCo");
        assert_eq!(c.phones.len(), 1);
        assert_eq!(c.phones[0].number, "555-0100");
        assert!(c.phones[0].primary);
        assert_eq!(c.emails.len(), 1);
        assert_eq!(c.emails[0].email, "jane@tech.co");
        assert!(c.birthday.is_some());
        assert_eq!(c.addresses.len(), 1);
        assert_eq!(c.addresses[0].city, "Portland");
    }

    #[test]
    fn test_app_build_contact_from_form_empty_phone() {
        let mut app = ContactsApp::new();
        app.edit_first_name = String::from("Test");
        let c = app.build_contact_from_form();
        assert!(c.phones.is_empty());
    }

    #[test]
    fn test_app_build_contact_from_form_empty_address() {
        let mut app = ContactsApp::new();
        app.edit_first_name = String::from("Test");
        let c = app.build_contact_from_form();
        assert!(c.addresses.is_empty());
    }

    // -----------------------------------------------------------------------
    // Rendering detail views
    // -----------------------------------------------------------------------

    #[test]
    fn test_render_with_view_contact() {
        let mut app = ContactsApp::new();
        app.load_sample_data();
        // The sample data sets view to ViewContact(1)
        let cmds = app.render();
        // Should produce render commands for contact detail
        assert!(cmds.len() > 20);
    }

    #[test]
    fn test_render_with_new_contact_view() {
        let mut app = ContactsApp::new();
        app.view = DetailView::NewContact;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_edit_contact_view() {
        let mut app = ContactsApp::new();
        app.load_sample_data();
        app.view = DetailView::EditContact(1);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_duplicates_view() {
        let mut app = ContactsApp::new();
        app.view = DetailView::Duplicates;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_groups_view() {
        let mut app = ContactsApp::new();
        app.load_sample_data();
        app.view = DetailView::Groups;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // -----------------------------------------------------------------------
    // Normalize phone
    // -----------------------------------------------------------------------

    #[test]
    fn test_normalize_phone() {
        assert_eq!(normalize_phone("+1-555-0100"), "15550100");
        assert_eq!(normalize_phone("(555) 010-0100"), "5550100100");
        assert_eq!(normalize_phone("5550100"), "5550100");
    }

    // -----------------------------------------------------------------------
    // SortOrder and ContactFilter labels
    // -----------------------------------------------------------------------

    #[test]
    fn test_sort_order_label() {
        assert_eq!(SortOrder::Name.label(), "Name");
        assert_eq!(SortOrder::Company.label(), "Company");
        assert_eq!(SortOrder::RecentlyAdded.label(), "Recently Added");
        assert_eq!(SortOrder::RecentlyContacted.label(), "Recently Contacted");
    }

    #[test]
    fn test_contact_filter_label() {
        assert_eq!(ContactFilter::All.label(), "All Contacts");
        assert_eq!(ContactFilter::HasPhone.label(), "Has Phone");
        assert_eq!(ContactFilter::HasEmail.label(), "Has Email");
        assert_eq!(ContactFilter::Favorites.label(), "Favorites");
        assert_eq!(ContactFilter::Group(1).label(), "Group");
    }

    #[test]
    fn test_contact_filter_matches() {
        let mut c = Contact::new(1, "A", "A");
        c.phones.push(PhoneNumber::new("123", PhoneType::Mobile));
        c.emails.push(EmailAddress::new("a@b.com", EmailType::Personal));
        c.groups.push(5);
        c.favorite = true;

        assert!(ContactFilter::All.matches(&c));
        assert!(ContactFilter::HasPhone.matches(&c));
        assert!(ContactFilter::HasEmail.matches(&c));
        assert!(ContactFilter::Favorites.matches(&c));
        assert!(ContactFilter::Group(5).matches(&c));
        assert!(!ContactFilter::Group(99).matches(&c));
    }

    // -----------------------------------------------------------------------
    // DuplicateReason label
    // -----------------------------------------------------------------------

    #[test]
    fn test_duplicate_reason_label() {
        assert_eq!(DuplicateReason::SameName.label(), "Same name");
        assert_eq!(DuplicateReason::SamePhone.label(), "Same phone number");
        assert_eq!(DuplicateReason::SameEmail.label(), "Same email address");
        assert_eq!(DuplicateReason::SameNameAndCompany.label(), "Same name & company");
    }

    // -----------------------------------------------------------------------
    // Primary phone/email helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_primary_phone_returns_primary() {
        let mut c = Contact::new(1, "A", "A");
        c.phones.push(PhoneNumber::new("111", PhoneType::Home));
        c.phones.push(PhoneNumber::new("222", PhoneType::Mobile).with_primary(true));
        assert_eq!(c.primary_phone().unwrap().number, "222");
    }

    #[test]
    fn test_primary_phone_fallback_first() {
        let mut c = Contact::new(1, "A", "A");
        c.phones.push(PhoneNumber::new("111", PhoneType::Home));
        assert_eq!(c.primary_phone().unwrap().number, "111");
    }

    #[test]
    fn test_primary_phone_none() {
        let c = Contact::new(1, "A", "A");
        assert!(c.primary_phone().is_none());
    }

    #[test]
    fn test_primary_email_returns_primary() {
        let mut c = Contact::new(1, "A", "A");
        c.emails.push(EmailAddress::new("a@a.com", EmailType::Personal));
        c.emails.push(EmailAddress::new("b@b.com", EmailType::Work).with_primary(true));
        assert_eq!(c.primary_email().unwrap().email, "b@b.com");
    }

    #[test]
    fn test_primary_email_fallback_first() {
        let mut c = Contact::new(1, "A", "A");
        c.emails.push(EmailAddress::new("a@a.com", EmailType::Personal));
        assert_eq!(c.primary_email().unwrap().email, "a@a.com");
    }

    #[test]
    fn test_primary_email_none() {
        let c = Contact::new(1, "A", "A");
        assert!(c.primary_email().is_none());
    }

    // -----------------------------------------------------------------------
    // ContactStore default trait
    // -----------------------------------------------------------------------

    #[test]
    fn test_contact_store_default() {
        let store = ContactStore::default();
        assert_eq!(store.contact_count(), 0);
        assert!(store.all_groups().is_empty());
    }

    #[test]
    fn test_contacts_app_default() {
        let app = ContactsApp::default();
        assert_eq!(app.view, DetailView::Empty);
    }

    // -----------------------------------------------------------------------
    // Filtered + sorted with favorites at top
    // -----------------------------------------------------------------------

    #[test]
    fn test_filtered_sorted_favorites_first() {
        let store = make_store_with_contacts();
        let results = store.filtered_sorted(&ContactFilter::All, SortOrder::Name, "");
        // Carol is favorite, should come first
        assert_eq!(results[0].first_name, "Carol");
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_vcard_import() {
        let contacts = import_vcards("", 1);
        assert!(contacts.is_empty());
    }

    #[test]
    fn test_garbage_vcard_import() {
        let contacts = import_vcards("not a vcard at all", 1);
        assert!(contacts.is_empty());
    }

    #[test]
    fn test_split_vcard_line_no_colon() {
        assert!(split_vcard_line("no colon here").is_none());
    }

    #[test]
    fn test_split_vcard_line_with_colon() {
        let (prop, val) = split_vcard_line("FN:John Doe").unwrap();
        assert_eq!(prop, "FN");
        assert_eq!(val, "John Doe");
    }

    #[test]
    fn test_day_of_year() {
        assert_eq!(day_of_year(1, 1), 1);
        assert_eq!(day_of_year(2, 1), 32);
        assert_eq!(day_of_year(12, 31), 365);
    }

    #[test]
    fn test_social_platform_labels() {
        assert_eq!(SocialPlatform::Twitter.label(), "Twitter");
        assert_eq!(SocialPlatform::LinkedIn.label(), "LinkedIn");
        assert_eq!(SocialPlatform::GitHub.label(), "GitHub");
        assert_eq!(SocialPlatform::Mastodon.label(), "Mastodon");
    }

    #[test]
    fn test_merge_contacts_social_dedup() {
        let mut c1 = Contact::new(1, "A", "A");
        c1.social_accounts.push(SocialAccount::new(SocialPlatform::GitHub, "@alice"));
        let mut c2 = Contact::new(2, "A", "A");
        c2.social_accounts.push(SocialAccount::new(SocialPlatform::GitHub, "@alice"));
        c2.social_accounts.push(SocialAccount::new(SocialPlatform::Twitter, "@alice"));
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.social_accounts.len(), 2);
    }

    #[test]
    fn test_merge_contacts_address_dedup() {
        let mut c1 = Contact::new(1, "A", "A");
        let mut addr = PostalAddress::new(AddressType::Home);
        addr.street = String::from("123 Main");
        addr.city = String::from("NYC");
        addr.zip = String::from("10001");
        c1.addresses.push(addr);

        let mut c2 = Contact::new(2, "A", "A");
        let mut addr2 = PostalAddress::new(AddressType::Home);
        addr2.street = String::from("123 Main");
        addr2.city = String::from("NYC");
        addr2.zip = String::from("10001");
        c2.addresses.push(addr2);

        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.addresses.len(), 1);
    }
}
