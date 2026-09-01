//! Slate OS Contacts / Address Book
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

use guitk::color::Color;
use guitk::frame::{Frame, Rect};
// The shared civil-date arithmetic. This app's own copy was *correct* --
// unlike the calendar's, whose ISO week number was wrong on 38.5% of all
// dates -- but correct-and-duplicated is still two sources of truth for one
// calendar, and the one that is never edited is the one that silently stops
// agreeing. See `known-issues.md`
// C-SIX-APPS-EACH-CARRIED-THEIR-OWN-CIVIL-DATE-ARITHMETIC.
use guitk::date;
use guitk::event::{Event, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::probe::Probe;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};

use std::collections::VecDeque;
use std::process::ExitCode;
use std::time::Duration;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

// Every one of these carried an `#[allow(dead_code)]`, and five of them --
// CRUST, MAUVE, TEAL, PINK, ROSEWATER -- were never named anywhere but on
// their own definition line. The `allow` is what let that be true for as long
// as it was: it silences the one warning that would have said so. They are
// deleted rather than kept "for later", because a palette entry no drawing
// call reaches is not a palette entry, and the next reader would have had to
// grep the file to find that out.
const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT_COLOR: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const CRUST: Color = Color::from_hex(0x11111B);

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
/// Point size of a contact's notes in the detail panel.
const NOTES_FONT_SIZE: f32 = 13.0;
/// Line-to-line spacing of the notes, which are wrapped rather than clipped.
const NOTES_LINE_HEIGHT: f32 = 18.0;

const ALPHABET: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
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

/// The year whose month lengths [`day_of_year`] counts in.
///
/// Any common year would do; 2001 is named rather than written as a bare
/// literal at the one call site so that "this is deliberately not a leap year"
/// is a fact stated once instead of a constant a reader has to check.
const UNIFORM_YEAR: i32 = 2001;

/// A simple date representation for birthdays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl SimpleDate {
    /// Construct a date, returning `None` for one that never happened.
    ///
    /// The day is checked against the length of *that* month rather than
    /// against a flat `1..=31`, so 31 February -- which this used to accept --
    /// is now rejected. It matters because a birthday is typed by hand and
    /// imported from other people's address books, and an impossible one was
    /// stored, displayed back verbatim as though it were fine, and counted as a
    /// day of the year lying past the real end of February, so the "upcoming
    /// birthdays" list put it in the wrong place.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let length = Self::days_in_month(year, month)?;
        if (1..=length).contains(&day) {
            Some(Self { year, month, day })
        } else {
            None
        }
    }

    /// How many days `month` has in `year`, or `None` if there is no such
    /// month.
    ///
    /// Leap-aware, because the year is right there and 29 February is a real
    /// birthday for anyone born in 1988 and an impossible one for anyone born
    /// in 1989. Rejecting it outright would turn away a genuine date; accepting
    /// it always would keep a typo.
    fn days_in_month(year: u16, month: u8) -> Option<u8> {
        // The `None` is this function's whole contribution over the shared
        // one, and it must stay: `date::days_in_month` *clamps* an
        // out-of-range month into 1..=12, which is the right answer for a
        // caller that has already validated and the wrong one for
        // `SimpleDate::new`, whose entire job is to reject "month 13".
        if !(1..=12).contains(&month) {
            return None;
        }
        // Fits in a `u8` because no month is longer than 31 days; the
        // fallback cannot be reached and is not a policy.
        u8::try_from(date::days_in_month(i32::from(year), u32::from(month))).ok()
    }

    pub fn format_display(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Parse an ISO 8601 date in either the extended form `YYYY-MM-DD` or the
    /// basic form `YYYYMMDD`.
    ///
    /// Both forms, because this is what vCard `BDAY` properties actually
    /// contain in the wild: vCard 4.0 specifies the basic form, and the address
    /// books that emit it -- phones, mail clients -- write `19901225`. Only the
    /// extended form was accepted here, so importing such a card silently
    /// dropped the birthday and the contact arrived looking as though it had
    /// never had one. Export still writes the extended form, which every reader
    /// accepts.
    ///
    /// A partial date with no year (`--MMDD`, vCard's way of saying "I know the
    /// day but not the year") is still rejected; see `known-issues.md`, as
    /// storing one needs a representation this type does not have.
    pub fn parse(s: &str) -> Option<Self> {
        let (year, month, day) = if let Some((y, rest)) = s.split_once('-') {
            let (m, d) = rest.split_once('-')?;
            (y, m, d)
        } else if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
            // All-ASCII-digit is checked first, so these byte offsets are also
            // character boundaries.
            (s.get(..4)?, s.get(4..6)?, s.get(6..)?)
        } else {
            return None;
        };
        Self::new(
            year.parse::<u16>().ok()?,
            month.parse::<u8>().ok()?,
            day.parse::<u8>().ok()?,
        )
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

/// Day of the year, counting 1 for 1 January, in a uniform 365-day year.
///
/// Deliberately leap-agnostic even though [`SimpleDate`] knows its year: the
/// only caller compares a birthday against *today*, and those two fall in
/// different years. Applying each side's own leap rule would move one of them by
/// a day and not the other, which is a worse answer than moving neither. One
/// day of slack is well inside the tolerance a "birthdays coming up" list is
/// asking about.
fn day_of_year(month: u8, day: u8) -> u16 {
    // Clamped to 1..=13 *before* the range is built, not after: `1..13` is
    // every month, which is what "a month past December" has always meant
    // here. Clamping to 12 instead would silently drop December, and letting
    // the value through to `date::days_in_month` would be worse still --
    // that clamps internally, so month 14 would count December twice.
    let months_before = 1..u32::from(month.clamp(1, 13));
    let before: u16 = months_before
        .map(|m| u16::try_from(date::days_in_month(UNIFORM_YEAR, m)).unwrap_or(0))
        .sum();
    before.saturating_add(u16::from(day))
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
        if result.is_empty()
            && let Some(c) = self.company.chars().next()
        {
            result.push(c.to_ascii_uppercase());
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
        lines.push(format!(
            "FN:{}",
            vcard_escape(&self.computed_display_name())
        ));

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
                        contact.phones.push(
                            PhoneNumber::new(&vcard_unescape(value), ptype).with_primary(primary),
                        );
                    }
                    "EMAIL" => {
                        let etype = EmailType::from_vcard(&prop_upper);
                        let primary = prop_upper.contains("PREF");
                        contact.emails.push(
                            EmailAddress::new(&vcard_unescape(value), etype).with_primary(primary),
                        );
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

/// Escape a string for use as a vCard property value, per RFC 6350 §3.4.
///
/// A single left-to-right pass, for the same reason [`vcard_unescape`] is one:
/// a chain of `.replace()` calls has to get its ordering right, and the only
/// order that works here is the non-obvious one (backslash first, or every
/// rule after it re-escapes the backslashes it just introduced). A pass that
/// emits each character's escape as it reads it cannot get the order wrong,
/// because there is no order to get wrong.
///
/// A carriage return has no escape sequence in vCard — the grammar defines
/// only `\\`, `\,`, `\;` and `\n` — and vCard lines are CRLF-terminated, so a
/// raw CR left in a value ends the property line early and the rest of the
/// value is read as a new property. Since a CR in a text field *means* a line
/// break, it is folded into one rather than written out where it would corrupt
/// the record. A CRLF pair yields one break, not two.
fn vcard_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            _ => out.push(c),
        }
    }
    out
}

/// Decode a vCard property value, undoing [`vcard_escape`].
///
/// Single left-to-right pass. A chain of `.replace()` calls is *wrong* here,
/// and was the bug this replaced: decoding `\n` before `\\` means the escaped
/// form of the two-character text `\n` — a backslash followed by the letter n,
/// which `vcard_escape` writes as `\\n` — has its leading `\\` read as a
/// newline escape by the first pass. A contact note containing a Windows path
/// or a regex came back with a real line break in it: `C:\new` decoded to
/// `C:\`, a newline, and `ew`.
///
/// The corruption happens once, on the first load, and the damaged value is
/// then a fixed point — re-saving does not degrade it further. That makes it
/// *quieter*, not milder: the wrong value is what gets written back, so after
/// one load-and-save cycle the user's original text is gone, with no
/// accumulating drift to make the loss noticeable.
///
/// A single pass structurally cannot make that mistake: it consumes the
/// backslash and whatever follows it together, and never re-examines output it
/// has already produced.
fn vcard_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                // Unknown escape: malformed input. Keep the following
                // character as-is rather than dropping it.
                Some(other) => out.push(other),
                // Trailing backslash with nothing after it: keep it literally.
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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
            if in_vcard && let Some(c) = Contact::from_vcard(&current_block, next_id) {
                contacts.push(c);
                next_id = next_id.saturating_add(1);
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
                let reason = if !a.company.is_empty() && a.company.eq_ignore_ascii_case(&b.company)
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
        let already = merged
            .phones
            .iter()
            .any(|p| normalize_phone(&p.number) == normalize_phone(&phone.number));
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
        let already = merged
            .addresses
            .iter()
            .any(|a| a.street == addr.street && a.city == addr.city && a.zip == addr.zip);
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
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == contact_id)
            && !contact.groups.contains(&group_id)
        {
            contact.groups.push(group_id);
            return true;
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
            SortOrder::Name => refs.sort_by_key(|a| a.sort_key_name()),
            SortOrder::Company => refs.sort_by(|a, b| {
                a.company
                    .to_lowercase()
                    .cmp(&b.company.to_lowercase())
                    .then_with(|| a.sort_key_name().cmp(&b.sort_key_name()))
            }),
            SortOrder::RecentlyAdded => {
                refs.sort_by_key(|r| std::cmp::Reverse(r.created_at));
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
        self.recently_viewed
            .retain(|&rid| rid != id_a && rid != id_b);

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
    pub fn upcoming_birthdays(
        &self,
        today_month: u8,
        today_day: u8,
        within_days: u16,
    ) -> Vec<&Contact> {
        self.contacts
            .iter()
            .filter(|c| {
                c.birthday
                    .is_some_and(|b| b.is_upcoming_within(today_month, today_day, within_days))
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

// ============================================================================
// Geometry
// ============================================================================

/// The size the window opens at, and the size the probe draws at.
const WINDOW_WIDTH: f32 = 1024.0;
/// The height the window opens at.
const WINDOW_HEIGHT: f32 = 768.0;

/// The narrowest detail panel worth drawing.
///
/// Below this the panel is an ellipsis where a name should be and a row of
/// buttons cut off at the window edge. That is worse than no panel at all,
/// because the list has paid for it in width.
const MIN_PANEL_WIDTH: f32 = 260.0;

/// The narrowest contact list worth putting an A-Z rail beside.
const MIN_LIST_WIDTH: f32 = 160.0;

/// The list keeps at least one whole row. Every piece of chrome above it --
/// the search bar, the filter strip, the view strip -- is given up before the
/// list is, because a window showing three controls and no contacts is not a
/// contacts program.
const MIN_LIST_HEIGHT: f32 = CONTACT_ROW_HEIGHT;

/// Height of each of the two control strips under the search bar.
const STRIP_HEIGHT: f32 = 22.0;

/// Every rectangle the picture is built from, solved from the live window
/// size on every frame.
///
/// Nothing here is a constant offset into a window of a size nobody checked:
/// the old drawing pass wrote `SIDEBAR_WIDTH` and `self.window_height`
/// straight into the commands, so at 400 points wide the detail panel was 96
/// points of ellipses and at 200 it was off the right-hand edge entirely.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The whole window.
    pub window: Rect,
    /// The sidebar's title bar, holding the name, the count and `+`.
    pub header: Rect,
    /// The `+` button that starts a new contact.
    pub add_button: Rect,
    /// The search box. Zero-height when the window is too short for it.
    pub search: Rect,
    /// Filter on the left half, sort order on the right.
    pub strip: Rect,
    /// Groups on the left half, duplicates on the right.
    pub views: Rect,
    /// The scrolling contact list.
    pub list: Rect,
    /// The A-Z rail. Zero-width when the list is too narrow to spare it.
    pub alphabet: Rect,
    /// The detail panel. Zero-width when the window is too narrow for it.
    pub panel: Rect,
    /// The strip along the bottom saying what the last press did.
    pub status: Rect,
}

impl Layout {
    /// Solve the whole picture for a window of `w` by `h`.
    #[must_use]
    pub fn solve(w: f32, h: f32) -> Self {
        // A size that is not a number is not a size. NaN survives every
        // `max`, `min` and `clamp` below and comes out as a rectangle that
        // contains no point and clips everything away -- a blank window that
        // answers no press, with nothing in the picture to say why.
        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };
        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
        let window = Rect::new(0.0, 0.0, w, h);

        // The sidebar takes at most 45% of the window, so wherever both are
        // shown the detail panel is the wider of the two -- it holds a name,
        // an address and a paragraph of notes, and the list holds one line.
        let wanted_side = (w * 0.45).min(SIDEBAR_WIDTH + ALPHABET_BAR_WIDTH);
        let (side_w, panel_w) = if w - wanted_side >= MIN_PANEL_WIDTH {
            (wanted_side, w - wanted_side)
        } else {
            // Too narrow for both. The list is the program; the panel goes.
            (w, 0.0)
        };

        let alpha_w = if side_w - ALPHABET_BAR_WIDTH >= MIN_LIST_WIDTH {
            ALPHABET_BAR_WIDTH
        } else {
            0.0
        };
        let list_w = (side_w - alpha_w).max(0.0);

        let header_h = HEADER_HEIGHT.min(h);
        let header = Rect::new(0.0, 0.0, side_w, header_h);
        // The status strip is the last thing drawn and the first thing given
        // room: everything below stops at its top edge, so nothing can be
        // painted under it and then answer a press through it.
        let status_h = STRIP_HEIGHT.min(h);
        let status = Rect::new(0.0, (h - status_h).max(header_h), w, status_h);
        // The `+` sits at the right of the list column, not of the sidebar,
        // so the A-Z rail never draws over it.
        let add_side = 32.0_f32.min(header_h).min(list_w);
        let add_button = Rect::new(
            (list_w - add_side - 8.0).max(0.0),
            ((header_h - add_side) / 2.0).max(0.0),
            add_side,
            add_side,
        );

        // Each strip below is taken only if the list still keeps a whole row
        // after it. `take` is the one place that rule is written down.
        let mut y = header_h;
        let bottom = status.y;
        let mut take = |wanted: f32, gap: f32| -> Rect {
            let top = y + gap;
            if top + wanted + MIN_LIST_HEIGHT > bottom {
                return Rect::new(0.0, y, list_w, 0.0);
            }
            y = top + wanted;
            Rect::new(0.0, top, list_w, wanted)
        };
        let search_outer = take(SEARCH_BAR_HEIGHT, 8.0);
        let search = Rect::new(
            8.0_f32.min(list_w),
            search_outer.y,
            (list_w - 16.0).max(0.0),
            search_outer.h,
        );
        let strip = take(STRIP_HEIGHT, 6.0);
        let views = take(STRIP_HEIGHT, 2.0);

        let list = Rect::new(0.0, y, list_w, (status.y - y).max(0.0));
        let alphabet = Rect::new(list_w, header_h, alpha_w, (status.y - header_h).max(0.0));
        let panel = Rect::new(side_w, 0.0, panel_w, status.y);

        Self {
            window,
            header,
            add_button,
            search,
            strip,
            views,
            list,
            alphabet,
            panel,
            status,
        }
    }

    /// The left half of a two-cell strip.
    #[must_use]
    fn left_half(strip: Rect) -> Rect {
        Rect::new(strip.x, strip.y, strip.w / 2.0, strip.h)
    }

    /// The right half of a two-cell strip.
    #[must_use]
    fn right_half(strip: Rect) -> Rect {
        let half = strip.w / 2.0;
        Rect::new(strip.x + half, strip.y, strip.w - half, strip.h)
    }

    /// The box the `i`th letter of the A-Z rail is drawn in.
    #[must_use]
    fn letter_cell(&self, i: usize) -> Rect {
        let n = ALPHABET.len() as f32;
        let pitch = self.alphabet.h / n;
        Rect::new(
            self.alphabet.x,
            self.alphabet.y + (i as f32) * pitch,
            self.alphabet.w,
            pitch,
        )
    }
}

// ============================================================================
// What a press can land on
// ============================================================================

/// One editable line of the add/edit form.
///
/// The form used to be an array of `(&str, &str)` pairs -- a label and a
/// borrowed value -- which is enough to *draw* a field and not enough to put
/// a character into one. Naming the fields is what lets a press choose one
/// and a keystroke reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    FirstName,
    LastName,
    Nickname,
    Company,
    JobTitle,
    Department,
    Phone,
    Email,
    Birthday,
    Street,
    City,
    State,
    Zip,
    Country,
    Notes,
}

impl FormField {
    /// Every field, in the order they are drawn.
    pub const ALL: [Self; 15] = [
        Self::FirstName,
        Self::LastName,
        Self::Nickname,
        Self::Company,
        Self::JobTitle,
        Self::Department,
        Self::Phone,
        Self::Email,
        Self::Birthday,
        Self::Street,
        Self::City,
        Self::State,
        Self::Zip,
        Self::Country,
        Self::Notes,
    ];

    /// The words written above the field.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::FirstName => "First Name",
            Self::LastName => "Last Name",
            Self::Nickname => "Nickname",
            Self::Company => "Company",
            Self::JobTitle => "Job Title",
            Self::Department => "Department",
            Self::Phone => "Phone",
            Self::Email => "Email",
            Self::Birthday => "Birthday (YYYY-MM-DD)",
            Self::Street => "Street",
            Self::City => "City",
            Self::State => "State",
            Self::Zip => "ZIP Code",
            Self::Country => "Country",
            Self::Notes => "Notes",
        }
    }

    /// The next field `Tab` reaches, wrapping at the end.
    #[must_use]
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        // Wrap by falling back to `first()` rather than writing the last
        // field's name into the wrap arm: the form grew from two fields to
        // fifteen once already, and a cycle that stops short of the last
        // field is a field nothing can type into.
        Self::ALL
            .get(i.saturating_add(1))
            .or_else(|| Self::ALL.first())
            .copied()
            .unwrap_or(self)
    }
}

/// Where typed characters go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// Nothing has the keyboard; letters are shortcuts.
    None,
    /// The search box in the sidebar.
    Search,
    /// One line of the add/edit form.
    Field(FormField),
}

/// One of the three quick actions on a contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickAction {
    /// Place a call, which is recorded as having contacted them.
    Call,
    /// Send mail, likewise recorded.
    Email,
    /// Show the address, which is a look and not a contact.
    Map,
}

impl QuickAction {
    /// The word on the button.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Call => "Call",
            Self::Email => "Email",
            Self::Map => "Map",
        }
    }
}

/// Everything a press can land on, recorded by the drawing pass at the
/// coordinates it drew the control at.
///
/// Row-bearing variants carry a **contact or group id**, never a row index:
/// the list re-sorts under the pointer whenever the sort order or the filter
/// changes, and an index would then name whoever moved into that slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The `+` in the sidebar header.
    AddContact,
    /// The search box.
    Search,
    /// The filter cell, which cycles the filter.
    CycleFilter,
    /// The sort cell, which cycles the sort order.
    CycleSort,
    /// The groups cell.
    ShowGroups,
    /// The duplicates cell.
    ShowDuplicates,
    /// One letter of the A-Z rail.
    Letter(char),
    /// A contact's row in the list.
    Contact(u64),
    /// One of Call / Email / Map in the detail panel.
    Action(QuickAction),
    /// The Edit button.
    EditContact,
    /// The Star / Unstar button.
    ToggleFavorite,
    /// The Delete button.
    DeleteContact,
    /// One line of the add/edit form.
    Field(FormField),
    /// Save the form.
    Save,
    /// Abandon the form.
    Cancel,
    /// Merge one pair on the duplicates panel, named by the two contacts.
    Merge(u64, u64),
    /// A group row, which filters the list to that group.
    Group(u64),
}

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

    /// Where typed characters go. Without this the name field was painted,
    /// labelled, and impossible to put a character into.
    pub focus: Focus,
    /// What the last press did, drawn along the bottom of the sidebar so a
    /// press that changed nothing visible still says so.
    pub status: String,
    /// A monotonic stand-in for the wall clock, so "recently contacted" has
    /// something to order by. Pressing Call twice must put the second call
    /// after the first.
    pub clock: u64,

    // Window dimensions, remembered so a press that arrives between a resize
    // and the next frame is answered against the size the window really is.
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

            focus: Focus::None,
            status: String::from("Ready"),
            // Later than any timestamp the sample data carries, so a call
            // placed now sorts above a call recorded in the fixture.
            clock: 2_000_000_000,

            window_width: WINDOW_WIDTH,
            window_height: WINDOW_HEIGHT,
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
            contact
                .phones
                .push(PhoneNumber::new(&self.edit_phone, self.edit_phone_type).with_primary(true));
        }
        if !self.edit_email.is_empty() {
            contact
                .emails
                .push(EmailAddress::new(&self.edit_email, self.edit_email_type).with_primary(true));
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

    // ── Drawing ─────────────────────────────────────────────────────────

    /// The commands for one frame, at the size the app was last told about.
    ///
    /// Kept because it is what the older tests read, and because it is the
    /// honest shape of "what does this program paint". Everything it knows
    /// comes from [`ContactsApp::frame`].
    #[must_use]
    pub fn render(&self) -> Vec<RenderCommand> {
        self.frame(self.window_width, self.window_height)
            .into_tree()
            .commands
    }

    /// The picture, and the clickable boxes that painting it created.
    ///
    /// This is the only place a window size becomes a coordinate and the only
    /// place a control's box is written down, which is what makes "drawn" and
    /// "clickable" the same fact. The old pass wrote `SIDEBAR_WIDTH` and
    /// `self.window_height` straight into the commands and recorded no boxes
    /// at all, so every control in the program was a painted rectangle that
    /// answered nothing.
    #[must_use]
    pub fn frame(&self, w: f32, h: f32) -> Frame<Target> {
        let l = Layout::solve(w, h);
        let mut f = Frame::new(l.window.w, l.window.h);

        // Edge to edge at every size. The old fill was `self.window_width` by
        // `self.window_height` -- the size the app believed it was, which
        // after a resize is the size it used to be.
        f.push(fill(l.window, BASE, 0.0));

        self.draw_sidebar(&mut f, &l);
        self.draw_alphabet(&mut f, &l);
        self.draw_panel(&mut f, &l);
        self.draw_status(&mut f, &l);
        f
    }

    /// The sidebar: title, count, `+`, search, the two strips and the list.
    fn draw_sidebar(&self, f: &mut Frame<Target>, l: &Layout) {
        let side = Rect::new(0.0, 0.0, l.list.w + l.alphabet.w, l.status.y);
        f.push(fill(side, MANTLE, 0.0));
        f.push(fill(l.header, SURFACE0, 0.0));

        // The title and the count share the header, and both stop short of
        // the `+` rather than running under it.
        let title_w = (l.add_button.x - 20.0).max(0.0);
        let half = l.header.h / 2.0;
        f.push(text_in(
            Rect::new(12.0, l.header.y, title_w, half),
            "Contacts",
            18.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
        ));
        f.push(text_in(
            Rect::new(12.0, l.header.y + half, title_w, l.header.h - half),
            &format!("{} contacts", self.store.contact_count()),
            11.0,
            SUBTEXT0,
            FontWeightHint::Regular,
        ));

        if !l.add_button.is_empty() {
            f.push(fill(l.add_button, BLUE, 6.0));
            f.push(text_in(
                inset(l.add_button, l.add_button.w / 3.0),
                "+",
                18.0,
                BASE,
                FontWeightHint::Bold,
            ));
            f.hit(Target::AddContact, l.add_button);
        }

        self.draw_search(f, l);
        self.draw_strips(f, l);
        self.draw_list(f, l);

        // The line that separates the sidebar from the panel, drawn only
        // where there is a panel on the other side of it.
        if !l.panel.is_empty() {
            f.push(RenderCommand::Line {
                x1: l.panel.x,
                y1: 0.0,
                x2: l.panel.x,
                y2: l.status.y,
                color: SURFACE1,
                width: 1.0,
            });
        }
    }

    /// The search box, and the caret when it has the keyboard.
    fn draw_search(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.search.is_empty() {
            return;
        }
        let focused = self.focus == Focus::Search;
        f.push(fill(
            l.search,
            if focused { SURFACE1 } else { SURFACE0 },
            8.0,
        ));
        let inner = inset(l.search, 10.0);
        let placeholder = self.search_query.is_empty() && !focused;
        let shown = if placeholder {
            "Search contacts..."
        } else {
            &self.search_query
        };
        f.push(text_in(
            inner,
            shown,
            13.0,
            if placeholder { OVERLAY0 } else { TEXT_COLOR },
            FontWeightHint::Regular,
        ));
        if focused {
            // A caret rather than a character appended to the text: a box
            // whose contents change when it is focused cannot be searched for
            // by the words it shows.
            let used = text::measure(&self.search_query, 13.0, FontWeightHint::Regular);
            let caret_x = (inner.x + used).min(inner.right() - 2.0);
            f.push(fill(
                Rect::new(caret_x, inner.y + 4.0, 2.0, (inner.h - 8.0).max(0.0)),
                BLUE,
                0.0,
            ));
        }
        f.hit(Target::Search, l.search);
    }

    /// The two two-cell strips: filter / sort, and groups / duplicates.
    fn draw_strips(&self, f: &mut Frame<Target>, l: &Layout) {
        let cells = [
            (
                Layout::left_half(l.strip),
                self.filter.label().to_string(),
                Target::CycleFilter,
            ),
            (
                Layout::right_half(l.strip),
                self.sort_order.label().to_string(),
                Target::CycleSort,
            ),
            (
                Layout::left_half(l.views),
                String::from("Groups"),
                Target::ShowGroups,
            ),
            (
                Layout::right_half(l.views),
                String::from("Duplicates"),
                Target::ShowDuplicates,
            ),
        ];
        for (cell, label, target) in cells {
            if cell.is_empty() {
                continue;
            }
            let on = match target {
                Target::ShowGroups => self.view == DetailView::Groups,
                Target::ShowDuplicates => self.view == DetailView::Duplicates,
                _ => false,
            };
            f.push(fill(
                inset_x(cell, 4.0),
                if on { BLUE } else { SURFACE0 },
                4.0,
            ));
            f.push(text_in(
                inset(cell, 8.0),
                &label,
                11.0,
                if on { BASE } else { SUBTEXT0 },
                FontWeightHint::Regular,
            ));
            f.hit(target, cell);
        }
    }

    /// The scrolling contact list.
    ///
    /// Every row is drawn and hit-boxed inside the list's clip, so a row that
    /// has scrolled out of sight stops being clickable in the same step that
    /// stops it being visible. The old list did the opposite: it re-derived
    /// the row under a press arithmetically, so a press was answered at
    /// coordinates a scrolled-away row no longer occupied.
    fn draw_list(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.list.is_empty() {
            return;
        }
        f.clip(l.list);

        let contacts =
            self.store
                .filtered_sorted(&self.filter, self.sort_order, &self.search_query);

        if contacts.is_empty() {
            f.push(text_in(
                inset(Rect::new(l.list.x, l.list.y, l.list.w, 24.0), 12.0),
                "No contacts match",
                12.0,
                OVERLAY0,
                FontWeightHint::Regular,
            ));
            f.unclip();
            return;
        }

        let mut cy = l.list.y - self.scroll_offset;
        let mut current_letter: Option<char> = None;
        for contact in &contacts {
            let letter = contact.first_letter();
            if current_letter != Some(letter) && self.sort_order == SortOrder::Name {
                let row = Rect::new(l.list.x, cy, l.list.w, LETTER_DIVIDER_HEIGHT);
                f.push(fill(row, Color::rgba(49, 50, 68, 180), 0.0));
                f.push(text_in(
                    inset(row, 12.0),
                    &letter.to_string(),
                    12.0,
                    BLUE,
                    FontWeightHint::Bold,
                ));
                cy += LETTER_DIVIDER_HEIGHT;
                current_letter = Some(letter);
            }
            self.draw_contact_row(f, l, contact, cy);
            cy += CONTACT_ROW_HEIGHT;
        }

        f.unclip();
    }

    /// One row of the contact list, at `cy` before clipping.
    fn draw_contact_row(&self, f: &mut Frame<Target>, l: &Layout, contact: &Contact, cy: f32) {
        let row = Rect::new(l.list.x, cy, l.list.w, CONTACT_ROW_HEIGHT);
        let selected = matches!(
            self.view,
            DetailView::ViewContact(id) | DetailView::EditContact(id) if id == contact.id
        );
        f.push(fill(
            row,
            if selected {
                SURFACE0
            } else {
                Color::TRANSPARENT
            },
            0.0,
        ));

        let avatar = Rect::new(row.x + 10.0, row.y + 6.0, 40.0, 40.0);
        f.push(fill(
            avatar,
            if contact.favorite { YELLOW } else { SURFACE1 },
            20.0,
        ));
        f.push(text_in(
            inset(avatar, 10.0),
            &contact.initials(),
            14.0,
            if contact.favorite { BASE } else { TEXT_COLOR },
            FontWeightHint::Bold,
        ));

        // The star is drawn first so the name's box can stop short of it. The
        // name used to be given `SIDEBAR_WIDTH - 80` whatever the window was,
        // which is a column width for a sidebar that is no longer that wide.
        let star_w = if contact.favorite { 18.0 } else { 0.0 };
        let text_x = avatar.right() + 8.0;
        let text_w = (row.right() - star_w - 6.0 - text_x).max(0.0);
        if contact.favorite {
            f.push(text_in(
                Rect::new(row.right() - star_w - 4.0, row.y, star_w, row.h),
                "*",
                16.0,
                YELLOW,
                FontWeightHint::Bold,
            ));
        }

        let name_h = row.h * 0.55;
        f.push(text_in(
            Rect::new(text_x, row.y + 4.0, text_w, name_h - 4.0),
            &contact.computed_display_name(),
            14.0,
            TEXT_COLOR,
            FontWeightHint::Regular,
        ));
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
            f.push(text_in(
                Rect::new(text_x, row.y + name_h, text_w, row.h - name_h - 4.0),
                &subtitle,
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
            ));
        }

        f.hit(Target::Contact(contact.id), row);
    }

    /// The A-Z rail down the right edge of the sidebar.
    fn draw_alphabet(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.alphabet.is_empty() {
            return;
        }
        f.push(fill(l.alphabet, Color::rgba(24, 24, 37, 200), 0.0));
        for (i, &letter) in ALPHABET.iter().enumerate() {
            let cell = l.letter_cell(i);
            let on = self.selected_letter == Some(letter);
            f.push(text_in(
                inset_x(cell, 6.0),
                &letter.to_string(),
                9.0,
                if on { BLUE } else { SUBTEXT0 },
                if on {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            ));
            f.hit(Target::Letter(letter), cell);
        }
    }

    /// The detail panel on the right, whichever view is showing.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.panel.is_empty() {
            return;
        }
        f.push(fill(l.panel, BASE, 0.0));
        f.clip(l.panel);
        match &self.view {
            DetailView::Empty => Self::draw_empty_state(f, l),
            DetailView::ViewContact(id) => {
                if let Some(contact) = self.store.get_contact(*id) {
                    self.draw_contact_detail(f, l, contact);
                } else {
                    Self::draw_empty_state(f, l);
                }
            }
            DetailView::EditContact(_) | DetailView::NewContact => self.draw_edit_form(f, l),
            DetailView::Duplicates => self.draw_duplicates(f, l),
            DetailView::Groups => self.draw_groups(f, l),
        }
        f.unclip();
    }

    /// What the panel says when nothing is selected.
    fn draw_empty_state(f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let mid = body.y + body.h / 2.0;
        f.push(text_in(
            Rect::new(body.x, mid - 30.0, body.w, 24.0),
            "Select a contact",
            18.0,
            SUBTEXT0,
            FontWeightHint::Regular,
        ));
        f.push(text_in(
            Rect::new(body.x, mid, body.w, 18.0),
            "or press + to add a new one",
            13.0,
            OVERLAY0,
            FontWeightHint::Regular,
        ));
    }

    /// A contact's details, with the three quick actions and the three
    /// buttons along the bottom.
    fn draw_contact_detail(&self, f: &mut Frame<Target>, l: &Layout, contact: &Contact) {
        let body = inset(l.panel, DETAIL_PADDING);
        // The buttons along the bottom are placed first, because everything
        // above them scrolls into whatever room is left rather than over
        // them. The old pass put them at `window_height - 60` and drew the
        // details from the top, so on a short window the notes ran through
        // them.
        let btn_h = 36.0_f32.min(body.h);
        let btn_y = (body.bottom() - btn_h).max(body.y);
        let mut y = body.y;

        let avatar = Rect::new(body.x, y, AVATAR_SIZE.min(body.w), AVATAR_SIZE);
        f.push(fill(
            avatar,
            if contact.favorite { YELLOW } else { BLUE },
            avatar.w / 2.0,
        ));
        f.push(text_in(
            inset(avatar, avatar.w / 3.0),
            &contact.initials(),
            28.0,
            BASE,
            FontWeightHint::Bold,
        ));
        y += AVATAR_SIZE + 12.0;

        f.push(text_in(
            Rect::new(body.x, y, body.w, 26.0),
            &contact.computed_display_name(),
            22.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
        ));
        y += 30.0;

        let company_line = match (contact.job_title.is_empty(), contact.company.is_empty()) {
            (false, false) => format!("{} at {}", contact.job_title, contact.company),
            (true, false) => contact.company.clone(),
            (false, true) => contact.job_title.clone(),
            (true, true) => String::new(),
        };
        for (line, size, color) in [
            (company_line, 14.0, SUBTEXT0),
            (contact.department.clone(), 12.0, OVERLAY0),
            (
                if contact.nickname.is_empty() {
                    String::new()
                } else {
                    format!("\"{}\"", contact.nickname)
                },
                12.0,
                LAVENDER,
            ),
        ] {
            if line.is_empty() {
                continue;
            }
            f.push(text_in(
                Rect::new(body.x, y, body.w, size + 4.0),
                &line,
                size,
                color,
                FontWeightHint::Regular,
            ));
            y += size + 6.0;
        }
        y += 8.0;

        // Quick actions. Their width comes from the panel, not from a
        // constant 80: three 80-point buttons need 264 points and the panel
        // can be 260.
        let gap = 10.0;
        let action_w = ((body.w - gap * 2.0) / 3.0).max(0.0);
        let action_h = 32.0_f32.min((btn_y - y).max(0.0));
        for (i, action) in [QuickAction::Call, QuickAction::Email, QuickAction::Map]
            .into_iter()
            .enumerate()
        {
            let r = Rect::new(
                body.x + (i as f32) * (action_w + gap),
                y,
                action_w,
                action_h,
            );
            if r.is_empty() {
                continue;
            }
            let color = match action {
                QuickAction::Call => GREEN,
                QuickAction::Email => BLUE,
                QuickAction::Map => PEACH,
            };
            f.push(fill(r, color, 6.0));
            f.push(text_in(
                inset(r, 8.0),
                action.label(),
                13.0,
                BASE,
                FontWeightHint::Bold,
            ));
            f.hit(Target::Action(action), r);
        }
        y += action_h + 14.0;

        // The fields, in a box that stops at the buttons.
        let fields = Rect::new(body.x, y, body.w, (btn_y - 12.0 - y).max(0.0));
        if !fields.is_empty() {
            f.clip(fields);
            self.draw_contact_fields(f, fields, contact);
            f.unclip();
        }

        // Edit / Star / Delete, sharing the panel's width.
        let three_w = ((body.w - gap * 2.0) / 3.0).max(0.0);
        let buttons = [
            (Target::EditContact, String::from("Edit"), BLUE, BASE),
            (
                Target::ToggleFavorite,
                String::from(if contact.favorite { "Unstar" } else { "Star" }),
                if contact.favorite { YELLOW } else { SURFACE1 },
                if contact.favorite { BASE } else { TEXT_COLOR },
            ),
            (Target::DeleteContact, String::from("Delete"), RED, BASE),
        ];
        for (i, (target, label, bg, fg)) in buttons.into_iter().enumerate() {
            let r = Rect::new(body.x + (i as f32) * (three_w + gap), btn_y, three_w, btn_h);
            if r.is_empty() {
                continue;
            }
            f.push(fill(r, bg, 6.0));
            f.push(text_in(
                inset(r, 8.0),
                &label,
                13.0,
                fg,
                FontWeightHint::Bold,
            ));
            f.hit(target, r);
        }
    }

    /// Phones, emails, addresses, social, birthday, notes and group chips.
    fn draw_contact_fields(&self, f: &mut Frame<Target>, area: Rect, contact: &Contact) {
        let mut y = area.y;
        let section = |f: &mut Frame<Target>, y: &mut f32, title: &str| {
            f.push(text_in(
                Rect::new(area.x, *y, area.w, 14.0),
                title,
                11.0,
                OVERLAY0,
                FontWeightHint::Bold,
            ));
            *y += 16.0;
        };
        let line_box = |y: f32| Rect::new(area.x + 8.0, y, (area.w - 8.0).max(0.0), 18.0);

        if !contact.phones.is_empty() {
            section(f, &mut y, "Phone");
            for phone in &contact.phones {
                let primary = if phone.primary { " (primary)" } else { "" };
                let s = format!("{}: {}{primary}", phone.phone_type.label(), phone.number);
                f.push(text_in(
                    line_box(y),
                    &s,
                    13.0,
                    TEXT_COLOR,
                    FontWeightHint::Regular,
                ));
                y += 20.0;
            }
            y += 6.0;
        }
        if !contact.emails.is_empty() {
            section(f, &mut y, "Email");
            for email in &contact.emails {
                let primary = if email.primary { " (primary)" } else { "" };
                let s = format!("{}: {}{primary}", email.email_type.label(), email.email);
                f.push(text_in(
                    line_box(y),
                    &s,
                    13.0,
                    TEXT_COLOR,
                    FontWeightHint::Regular,
                ));
                y += 20.0;
            }
            y += 6.0;
        }
        if !contact.addresses.is_empty() {
            section(f, &mut y, "Address");
            for addr in &contact.addresses {
                let s = format!("{}: {}", addr.address_type.label(), addr.display_line());
                f.push(text_in(
                    line_box(y),
                    &s,
                    13.0,
                    TEXT_COLOR,
                    FontWeightHint::Regular,
                ));
                y += 20.0;
            }
            y += 6.0;
        }
        if !contact.social_accounts.is_empty() {
            section(f, &mut y, "Social");
            for social in &contact.social_accounts {
                let s = format!("{}: {}", social.platform.label(), social.handle);
                f.push(text_in(
                    line_box(y),
                    &s,
                    13.0,
                    LAVENDER,
                    FontWeightHint::Regular,
                ));
                y += 20.0;
            }
            y += 6.0;
        }
        if let Some(ref bday) = contact.birthday {
            section(f, &mut y, "Birthday");
            f.push(text_in(
                line_box(y),
                &bday.format_display(),
                13.0,
                TEXT_COLOR,
                FontWeightHint::Regular,
            ));
            y += 24.0;
        }
        if !contact.notes.is_empty() {
            section(f, &mut y, "Notes");
            // `RenderCommand::Text` clips at `max_width` instead of wrapping,
            // so the notes used to show only their first line's worth of
            // characters -- silently, with nothing to say the rest was there.
            let notes_w = (area.w - 8.0).max(1.0);
            for line in &text::wrap(
                &contact.notes,
                notes_w,
                NOTES_FONT_SIZE,
                FontWeightHint::Regular,
            ) {
                f.push(text_in(
                    Rect::new(area.x + 8.0, y, notes_w, NOTES_LINE_HEIGHT),
                    line,
                    NOTES_FONT_SIZE,
                    SUBTEXT0,
                    FontWeightHint::Regular,
                ));
                y += NOTES_LINE_HEIGHT;
            }
            y += 6.0;
        }
        if !contact.groups.is_empty() {
            section(f, &mut y, "Groups");
            let mut chip_x = area.x;
            for &gid in &contact.groups {
                let Some(group) = self.store.get_group(gid) else {
                    continue;
                };
                let chip_w =
                    text::padded_width(&group.name, 8.0, 11.0, FontWeightHint::Bold).min(area.w);
                if chip_x + chip_w > area.right() {
                    // A chip that would run off the right edge starts a new
                    // line instead of being drawn outside its own box.
                    chip_x = area.x;
                    y += GROUP_CHIP_HEIGHT + 4.0;
                }
                let chip = Rect::new(chip_x, y, chip_w, GROUP_CHIP_HEIGHT);
                f.push(fill(chip, group.color, GROUP_CHIP_HEIGHT / 2.0));
                f.push(text_in(
                    inset(chip, 8.0),
                    &group.name,
                    11.0,
                    BASE,
                    FontWeightHint::Bold,
                ));
                f.hit(Target::Group(gid), chip);
                chip_x += chip_w + 8.0;
            }
        }
    }

    /// The add/edit form.
    fn draw_edit_form(&self, f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let btn_h = 36.0_f32.min(body.h);
        let btn_y = (body.bottom() - btn_h).max(body.y);
        let mut y = body.y;

        f.push(text_in(
            Rect::new(body.x, y, body.w, 24.0),
            if matches!(self.view, DetailView::NewContact) {
                "New Contact"
            } else {
                "Edit Contact"
            },
            20.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
        ));
        y += 32.0;

        let form = Rect::new(body.x, y, body.w, (btn_y - 10.0 - y).max(0.0));
        if !form.is_empty() {
            f.clip(form);
            let mut fy = form.y - self.scroll_offset;
            for field in FormField::ALL {
                let tall = field == FormField::Notes;
                let h = if tall { 72.0 } else { FIELD_HEIGHT };
                f.push(text_in(
                    Rect::new(form.x, fy, form.w, 12.0),
                    field.label(),
                    11.0,
                    OVERLAY0,
                    FontWeightHint::Regular,
                ));
                fy += 14.0;
                let box_r = Rect::new(form.x, fy, form.w, h);
                let focused = self.focus == Focus::Field(field);
                f.push(fill(box_r, if focused { SURFACE1 } else { SURFACE0 }, 6.0));
                let value = self.field_value(field);
                let inner = inset(box_r, 10.0);
                let inner = Rect::new(inner.x, inner.y, inner.w, 20.0_f32.min(inner.h));
                if value.is_empty() {
                    f.push(text_in(
                        inner,
                        field.label(),
                        13.0,
                        OVERLAY0,
                        FontWeightHint::Regular,
                    ));
                } else {
                    f.push(text_in(
                        inner,
                        value,
                        13.0,
                        TEXT_COLOR,
                        FontWeightHint::Regular,
                    ));
                }
                if focused {
                    let used = text::measure(value, 13.0, FontWeightHint::Regular);
                    let caret_x = (inner.x + used).min(inner.right() - 2.0);
                    f.push(fill(
                        Rect::new(caret_x, inner.y + 2.0, 2.0, (inner.h - 4.0).max(0.0)),
                        BLUE,
                        0.0,
                    ));
                }
                f.hit(Target::Field(field), box_r);
                fy += h + 6.0;
            }
            f.unclip();
        }

        let gap = 10.0;
        let half = ((body.w - gap) / 2.0).max(0.0);
        for (i, (target, label, bg, fg)) in [
            (Target::Save, "Save", GREEN, BASE),
            (Target::Cancel, "Cancel", SURFACE1, TEXT_COLOR),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(body.x + (i as f32) * (half + gap), btn_y, half, btn_h);
            if r.is_empty() {
                continue;
            }
            f.push(fill(r, bg, 6.0));
            f.push(text_in(
                inset(r, 8.0),
                label,
                13.0,
                fg,
                FontWeightHint::Bold,
            ));
            f.hit(target, r);
        }
    }

    /// The duplicate-detection panel.
    fn draw_duplicates(&self, f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let mut y = body.y;
        f.push(text_in(
            Rect::new(body.x, y, body.w, 24.0),
            "Duplicate Detection",
            20.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
        ));
        y += 32.0;

        let duplicates = self.store.find_duplicates();
        if duplicates.is_empty() {
            f.push(text_in(
                Rect::new(body.x, y, body.w, 20.0),
                "No duplicates found.",
                14.0,
                GREEN,
                FontWeightHint::Regular,
            ));
            return;
        }
        f.push(text_in(
            Rect::new(body.x, y, body.w, 18.0),
            &format!("Found {} potential duplicate(s):", duplicates.len()),
            13.0,
            PEACH,
            FontWeightHint::Regular,
        ));
        y += 24.0;

        let merge_w = 64.0_f32.min(body.w / 3.0);
        for dup in &duplicates {
            let card = Rect::new(body.x, y, body.w, 60.0);
            f.push(fill(card, SURFACE0, 8.0));
            let name_a = self
                .store
                .get_contact(dup.contact_a_id)
                .map_or_else(|| String::from("?"), Contact::computed_display_name);
            let name_b = self
                .store
                .get_contact(dup.contact_b_id)
                .map_or_else(|| String::from("?"), Contact::computed_display_name);
            let text_w = (card.w - merge_w - 24.0).max(0.0);
            f.push(text_in(
                Rect::new(card.x + 12.0, card.y + 8.0, text_w, 18.0),
                &format!("{name_a}  <->  {name_b}"),
                13.0,
                TEXT_COLOR,
                FontWeightHint::Bold,
            ));
            f.push(text_in(
                Rect::new(card.x + 12.0, card.y + 30.0, text_w, 16.0),
                &format!(
                    "{} (confidence: {:.0}%)",
                    dup.reason.label(),
                    dup.confidence * 100.0
                ),
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
            ));
            let merge = Rect::new(card.right() - merge_w - 8.0, card.y + 16.0, merge_w, 28.0);
            if !merge.is_empty() {
                f.push(fill(merge, BLUE, 4.0));
                f.push(text_in(
                    inset(merge, 6.0),
                    "Merge",
                    11.0,
                    BASE,
                    FontWeightHint::Bold,
                ));
                f.hit(Target::Merge(dup.contact_a_id, dup.contact_b_id), merge);
            }
            y += 72.0;
        }
    }

    /// The groups panel, whose rows filter the list.
    fn draw_groups(&self, f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let mut y = body.y;
        f.push(text_in(
            Rect::new(body.x, y, body.w, 24.0),
            "Groups",
            20.0,
            TEXT_COLOR,
            FontWeightHint::Bold,
        ));
        y += 32.0;

        let stats = self.store.group_stats();
        if stats.is_empty() {
            f.push(text_in(
                Rect::new(body.x, y, body.w, 20.0),
                "No groups yet. Create one to organize contacts.",
                14.0,
                SUBTEXT0,
                FontWeightHint::Regular,
            ));
            return;
        }
        for (gid, name, count) in &stats {
            let row = Rect::new(body.x, y, body.w, 48.0);
            f.push(fill(row, SURFACE0, 8.0));
            let dot = Rect::new(row.x + 12.0, row.y + 16.0, 16.0, 16.0);
            f.push(fill(
                dot,
                self.store.get_group(*gid).map_or(BLUE, |g| g.color),
                8.0,
            ));
            let text_x = dot.right() + 8.0;
            let text_w = (row.right() - 12.0 - text_x).max(0.0);
            f.push(text_in(
                Rect::new(text_x, row.y + 8.0, text_w, 18.0),
                name,
                14.0,
                TEXT_COLOR,
                FontWeightHint::Bold,
            ));
            f.push(text_in(
                Rect::new(text_x, row.y + 26.0, text_w, 16.0),
                &format!("{count} contact(s)"),
                11.0,
                SUBTEXT0,
                FontWeightHint::Regular,
            ));
            f.hit(Target::Group(*gid), row);
            y += 56.0;
        }
    }

    /// The strip along the bottom, drawn last so nothing can cover it.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.status.is_empty() {
            return;
        }
        f.push(fill(l.status, CRUST, 0.0));
        f.push(text_in(
            inset(l.status, 8.0),
            &self.status,
            11.0,
            SUBTEXT0,
            FontWeightHint::Regular,
        ));
    }

    /// The text currently in one form field.
    #[must_use]
    pub fn field_value(&self, field: FormField) -> &str {
        match field {
            FormField::FirstName => &self.edit_first_name,
            FormField::LastName => &self.edit_last_name,
            FormField::Nickname => &self.edit_nickname,
            FormField::Company => &self.edit_company,
            FormField::JobTitle => &self.edit_job_title,
            FormField::Department => &self.edit_department,
            FormField::Phone => &self.edit_phone,
            FormField::Email => &self.edit_email,
            FormField::Birthday => &self.edit_birthday,
            FormField::Street => &self.edit_street,
            FormField::City => &self.edit_city,
            FormField::State => &self.edit_state,
            FormField::Zip => &self.edit_zip,
            FormField::Country => &self.edit_country,
            FormField::Notes => &self.edit_notes,
        }
    }

    /// The same field, to write into.
    fn field_mut(&mut self, field: FormField) -> &mut String {
        match field {
            FormField::FirstName => &mut self.edit_first_name,
            FormField::LastName => &mut self.edit_last_name,
            FormField::Nickname => &mut self.edit_nickname,
            FormField::Company => &mut self.edit_company,
            FormField::JobTitle => &mut self.edit_job_title,
            FormField::Department => &mut self.edit_department,
            FormField::Phone => &mut self.edit_phone,
            FormField::Email => &mut self.edit_email,
            FormField::Birthday => &mut self.edit_birthday,
            FormField::Street => &mut self.edit_street,
            FormField::City => &mut self.edit_city,
            FormField::State => &mut self.edit_state,
            FormField::Zip => &mut self.edit_zip,
            FormField::Country => &mut self.edit_country,
            FormField::Notes => &mut self.edit_notes,
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
        c1.phones
            .push(PhoneNumber::new("+1-555-0101", PhoneType::Mobile).with_primary(true));
        c1.phones
            .push(PhoneNumber::new("+1-555-0102", PhoneType::Work));
        c1.emails
            .push(EmailAddress::new("alice@example.com", EmailType::Personal).with_primary(true));
        c1.emails.push(EmailAddress::new(
            "alice.anderson@acme.com",
            EmailType::Work,
        ));
        c1.birthday = SimpleDate::new(1990, 3, 15);
        c1.favorite = true;
        c1.groups.push(g2);
        c1.groups.push(g3);
        c1.social_accounts
            .push(SocialAccount::new(SocialPlatform::GitHub, "@alice"));
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
        c2.phones
            .push(PhoneNumber::new("+1-555-0201", PhoneType::Mobile).with_primary(true));
        c2.emails
            .push(EmailAddress::new("bob@baker.com", EmailType::Work).with_primary(true));
        c2.birthday = SimpleDate::new(1985, 7, 22);
        c2.groups.push(g2);
        c2.created_at = 2000;
        let _id2 = self.store.add_contact(c2);

        // Contact 3
        let mut c3 = Contact::new(0, "Carol", "Chen");
        c3.company = String::from("Acme Corp");
        c3.phones
            .push(PhoneNumber::new("+1-555-0301", PhoneType::Home).with_primary(true));
        c3.emails
            .push(EmailAddress::new("carol@example.com", EmailType::Personal).with_primary(true));
        c3.groups.push(g1);
        c3.groups.push(g3);
        c3.favorite = true;
        c3.created_at = 3000;
        let _id3 = self.store.add_contact(c3);

        // Contact 4
        let mut c4 = Contact::new(0, "David", "Diaz");
        c4.phones
            .push(PhoneNumber::new("+1-555-0401", PhoneType::Mobile).with_primary(true));
        c4.groups.push(g1);
        c4.created_at = 4000;
        let _id4 = self.store.add_contact(c4);

        // Contact 5
        let mut c5 = Contact::new(0, "Emma", "Evans");
        c5.company = String::from("TechStart");
        c5.job_title = String::from("CTO");
        c5.emails
            .push(EmailAddress::new("emma@techstart.io", EmailType::Work).with_primary(true));
        c5.social_accounts
            .push(SocialAccount::new(SocialPlatform::LinkedIn, "emma-evans"));
        c5.social_accounts
            .push(SocialAccount::new(SocialPlatform::Twitter, "@emma_e"));
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
// Drawing helpers
// ============================================================================

/// A filled, optionally rounded rectangle.
fn fill(r: Rect, color: Color, radius: f32) -> RenderCommand {
    RenderCommand::FillRect {
        x: r.x,
        y: r.y,
        width: r.w,
        height: r.h,
        color,
        corner_radii: CornerRadii::all(radius),
    }
}

/// `r` shrunk by `pad` on every side, never below nothing.
fn inset(r: Rect, pad: f32) -> Rect {
    Rect::new(
        r.x + pad,
        r.y + pad,
        (r.w - pad * 2.0).max(0.0),
        (r.h - pad * 2.0).max(0.0),
    )
}

/// `r` shrunk by `pad` on the left and right only.
fn inset_x(r: Rect, pad: f32) -> Rect {
    Rect::new(r.x + pad, r.y, (r.w - pad * 2.0).max(0.0), r.h)
}

/// A run of text bounded by the box it is drawn in, on both axes.
///
/// Two things here are the whole point. `max_width` is always `Some`, so a
/// long name cannot walk across the column beside it -- the old pass left it
/// `None` on nineteen of its runs. And the size is clamped to the box's
/// height, because a run taller than its box sticks out of *both* ends of it
/// once it is centred, and no caller can prevent that from where it stands:
/// that is how a status line came to be drawn below the bottom edge of a
/// short window.
fn text_in(r: Rect, s: &str, size: f32, color: Color, weight: FontWeightHint) -> RenderCommand {
    let size = size.min(r.h);
    RenderCommand::Text {
        x: r.x,
        y: r.y + (r.h - size) / 2.0,
        text: s.to_string(),
        font_size: size,
        color,
        font_weight: weight,
        max_width: Some(r.w),
        overflow: TextOverflow::Ellipsis,
    }
}

// ============================================================================
// What a press and a keystroke do
// ============================================================================

impl ContactsApp {
    /// Route an event to whatever the drawing pass put under it.
    fn handle_event(&mut self, event: &Event, size: (f32, f32)) {
        match event {
            Event::Key(ke) => self.handle_key(ke),
            Event::Mouse(me) => self.handle_mouse(me, size),
            _ => {}
        }
    }

    /// Route a press to the control the last frame drew at that point.
    ///
    /// The hit boxes come from a frame drawn at the same size, so a control
    /// the window was too small to draw is a control that cannot be pressed.
    fn handle_mouse(&mut self, event: &MouseEvent, size: (f32, f32)) {
        let MouseEventKind::Press(MouseButton::Left) = event.kind else {
            return;
        };
        let frame = self.frame(size.0, size.1);
        let Some(target) = frame.hit_test(event.x, event.y) else {
            return;
        };
        self.activate(target);
    }

    /// Do what pressing `target` means.
    fn activate(&mut self, target: Target) {
        match target {
            Target::AddContact => self.start_new_contact(),
            Target::Search => {
                self.focus = Focus::Search;
                self.status = String::from("Type to search");
            }
            Target::CycleFilter => {
                self.filter = match self.filter {
                    ContactFilter::All => ContactFilter::Favorites,
                    ContactFilter::Favorites => ContactFilter::HasPhone,
                    ContactFilter::HasPhone => ContactFilter::HasEmail,
                    _ => ContactFilter::All,
                };
                self.scroll_offset = 0.0;
                self.status = format!("Filter: {}", self.filter.label());
            }
            Target::CycleSort => {
                self.sort_order = match self.sort_order {
                    SortOrder::Name => SortOrder::Company,
                    SortOrder::Company => SortOrder::RecentlyAdded,
                    SortOrder::RecentlyAdded => SortOrder::RecentlyContacted,
                    SortOrder::RecentlyContacted => SortOrder::Name,
                };
                self.scroll_offset = 0.0;
                self.status = format!("Sort: {}", self.sort_order.label());
            }
            Target::ShowGroups => {
                self.view = DetailView::Groups;
                self.focus = Focus::None;
                self.status = String::from("Groups");
            }
            Target::ShowDuplicates => {
                self.view = DetailView::Duplicates;
                self.focus = Focus::None;
                self.status = String::from("Duplicate detection");
            }
            Target::Letter(letter) => self.jump_to_letter(letter),
            Target::Contact(id) => self.select_contact(id),
            Target::Action(action) => self.quick_action(action),
            Target::EditContact => self.start_editing(),
            Target::ToggleFavorite => self.toggle_selected_favorite(),
            Target::DeleteContact => self.delete_selected(),
            Target::Field(field) => {
                self.focus = Focus::Field(field);
                self.status = format!("Editing {}", field.label());
            }
            Target::Save => self.save_form(),
            Target::Cancel => self.cancel_form(),
            Target::Merge(a, b) => self.merge_pair(a, b),
            Target::Group(gid) => self.filter_to_group(gid),
        }
    }

    /// Open the form on a blank contact.
    fn start_new_contact(&mut self) {
        self.clear_edit_form();
        self.view = DetailView::NewContact;
        // The first field takes the keyboard, so the form can be typed into
        // without first finding somewhere to click.
        self.focus = Focus::Field(FormField::FirstName);
        self.scroll_offset = 0.0;
        self.status = String::from("New contact");
    }

    /// Show a contact, and record that it was looked at.
    fn select_contact(&mut self, id: u64) {
        if self.store.get_contact(id).is_none() {
            return;
        }
        self.store.record_view(id);
        self.view = DetailView::ViewContact(id);
        self.focus = Focus::None;
        self.status = self
            .store
            .get_contact(id)
            .map_or_else(String::new, Contact::computed_display_name);
    }

    /// The contact the panel is showing, if it is showing one.
    #[must_use]
    pub fn selected_id(&self) -> Option<u64> {
        match self.view {
            DetailView::ViewContact(id) | DetailView::EditContact(id) => Some(id),
            _ => None,
        }
    }

    /// Call, mail or map the selected contact.
    fn quick_action(&mut self, action: QuickAction) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let name = self
            .store
            .get_contact(id)
            .map_or_else(String::new, Contact::computed_display_name);
        match action {
            QuickAction::Call | QuickAction::Email => {
                // Reaching someone is what "recently contacted" orders by, so
                // the two buttons that reach them move the contact to the top
                // of that order. The clock advances so a second call lands
                // after the first rather than tying with it.
                self.clock = self.clock.saturating_add(1);
                let now = self.clock;
                self.store.mark_contacted(id, now);
            }
            QuickAction::Map => {}
        }
        self.status = format!("{}: {name}", action.label());
    }

    /// Open the form on the selected contact.
    fn start_editing(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let Some(contact) = self.store.get_contact(id).cloned() else {
            return;
        };
        self.load_edit_form(&contact);
        self.view = DetailView::EditContact(id);
        self.focus = Focus::Field(FormField::FirstName);
        self.scroll_offset = 0.0;
        self.status = format!("Editing {}", contact.computed_display_name());
    }

    /// Star or unstar the selected contact.
    fn toggle_selected_favorite(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        if let Some(now_favorite) = self.store.toggle_favorite(id) {
            self.status = String::from(if now_favorite { "Starred" } else { "Unstarred" });
        }
    }

    /// Delete the selected contact and go back to the empty panel.
    fn delete_selected(&mut self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let name = self
            .store
            .get_contact(id)
            .map_or_else(String::new, Contact::computed_display_name);
        if self.store.delete_contact(id) {
            self.view = DetailView::Empty;
            self.focus = Focus::None;
            self.status = format!("Deleted {name}");
        }
    }

    /// Write the form back, either onto the contact being edited or as a new
    /// one.
    fn save_form(&mut self) {
        let built = self.build_contact_from_form();
        match self.view {
            DetailView::EditContact(id) => {
                let mut updated = built;
                updated.id = id;
                if let Some(existing) = self.store.get_contact(id) {
                    // Everything the form does not carry -- groups, extra
                    // phone numbers, the favourite flag, when it was created
                    // -- belongs to the contact and not to the form, and
                    // saving must not quietly drop it.
                    updated.groups.clone_from(&existing.groups);
                    updated.favorite = existing.favorite;
                    updated.created_at = existing.created_at;
                    updated.last_contacted = existing.last_contacted;
                }
                if self.store.update_contact(updated) {
                    self.view = DetailView::ViewContact(id);
                    self.status = String::from("Saved");
                }
            }
            DetailView::NewContact => {
                let id = self.store.add_contact(built);
                self.view = DetailView::ViewContact(id);
                self.status = String::from("Added");
            }
            _ => return,
        }
        self.focus = Focus::None;
        self.scroll_offset = 0.0;
    }

    /// Abandon the form without writing anything.
    fn cancel_form(&mut self) {
        self.view = match self.view {
            DetailView::EditContact(id) => DetailView::ViewContact(id),
            _ => DetailView::Empty,
        };
        self.clear_edit_form();
        self.focus = Focus::None;
        self.scroll_offset = 0.0;
        self.status = String::from("Cancelled");
    }

    /// Merge a pair the duplicate panel found.
    fn merge_pair(&mut self, a: u64, b: u64) {
        if let Some(kept) = self.store.merge_contacts(a, b) {
            self.status = format!("Merged into #{kept}");
        }
    }

    /// Narrow the list to one group.
    fn filter_to_group(&mut self, gid: u64) {
        self.filter = ContactFilter::Group(gid);
        self.scroll_offset = 0.0;
        self.status = self
            .store
            .get_group(gid)
            .map_or_else(|| String::from("Group"), |g| format!("Group: {}", g.name));
    }

    /// Scroll the list so the first contact filed under `letter` is at the
    /// top of it.
    ///
    /// The offset is measured by walking the same rows the drawing pass
    /// walks, dividers included, rather than multiplying an index by a row
    /// height -- the dividers only appear when the sort is by name, so the
    /// arithmetic version is wrong under every other sort.
    fn jump_to_letter(&mut self, letter: char) {
        self.selected_letter = Some(letter);
        let contacts =
            self.store
                .filtered_sorted(&self.filter, self.sort_order, &self.search_query);
        let mut offset = 0.0_f32;
        let mut current: Option<char> = None;
        for contact in &contacts {
            let first = contact.first_letter();
            if current != Some(first) && self.sort_order == SortOrder::Name {
                if first == letter {
                    self.scroll_offset = offset;
                    self.status = format!("Jumped to {letter}");
                    return;
                }
                offset += LETTER_DIVIDER_HEIGHT;
                current = Some(first);
            } else if first == letter {
                self.scroll_offset = offset;
                self.status = format!("Jumped to {letter}");
                return;
            }
            offset += CONTACT_ROW_HEIGHT;
        }
        self.status = format!("Nothing filed under {letter}");
    }

    /// A keystroke: text into whatever has the keyboard, otherwise a
    /// shortcut.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }
        match event.key {
            Key::Escape => {
                if self.focus == Focus::None {
                    self.view = DetailView::Empty;
                    self.status = String::from("Closed");
                } else {
                    self.focus = Focus::None;
                    self.status = String::from("Keyboard released");
                }
                return;
            }
            Key::Tab => {
                self.focus = match self.focus {
                    Focus::Field(field) => Focus::Field(field.next()),
                    _ => Focus::Field(FormField::FirstName),
                };
                return;
            }
            Key::Backspace => {
                match self.focus {
                    Focus::Search => {
                        self.search_query.pop();
                        self.scroll_offset = 0.0;
                    }
                    Focus::Field(field) => {
                        self.field_mut(field).pop();
                    }
                    Focus::None => {}
                }
                return;
            }
            Key::Enter => {
                match self.focus {
                    Focus::Field(_) => self.save_form(),
                    _ => self.focus = Focus::None,
                }
                return;
            }
            _ => {}
        }

        // Printable text. `KeyEvent::text` is what the platform's keyboard
        // layout produced, shift and dead keys included; deriving a character
        // from the key code instead is what made a `+` impossible to type on
        // any layout but the one the table was written for.
        let typed = event.text.clone();
        if !typed.is_empty() && !typed.chars().any(char::is_control) {
            match self.focus {
                Focus::Search => {
                    self.search_query.push_str(&typed);
                    self.scroll_offset = 0.0;
                    return;
                }
                Focus::Field(field) => {
                    self.field_mut(field).push_str(&typed);
                    return;
                }
                Focus::None => {}
            }
        }

        // Nothing has the keyboard, so letters are shortcuts.
        match event.key {
            Key::N => self.start_new_contact(),
            Key::S => self.activate(Target::Search),
            Key::G => self.activate(Target::ShowGroups),
            Key::D => self.activate(Target::ShowDuplicates),
            Key::F => self.activate(Target::CycleFilter),
            Key::O => self.activate(Target::CycleSort),
            Key::E => self.start_editing(),
            Key::Delete => self.delete_selected(),
            _ => {}
        }
    }
}

// ============================================================================
// The window
// ============================================================================

impl App for ContactsApp {
    fn title(&self) -> String {
        String::from("Contacts")
    }

    fn app_id(&self) -> String {
        String::from("contacts")
    }

    fn initial_size(&self) -> (u32, u32) {
        (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
    }

    fn tick_interval(&self) -> Option<Duration> {
        // An address book changes when someone edits it and at no other time.
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            Event::CloseRequested => Response::Exit,
            Event::Resize { width, height } => {
                // Remembered here as well as in `render`, because a press can
                // arrive after a resize and before the next frame, and it has
                // to be answered against the window's real size.
                self.window_width = *width as f32;
                self.window_height = *height as f32;
                Response::Redraw
            }
            _ => {
                self.handle_event(event, (self.window_width, self.window_height));
                Response::Redraw
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window_width = width;
        self.window_height = height;
        self.frame(width, height).into_tree()
    }
}

impl Probe for ContactsApp {
    type Target = Target;
    type Outcome = ();
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    fn draw(&self, size: (f32, f32)) -> Frame<Target> {
        self.frame(size.0, size.1)
    }

    fn click_at(&mut self, x: f32, y: f32, button: MouseButton, size: (f32, f32)) {
        self.window_width = size.0;
        self.window_height = size.1;
        self.handle_event(
            &Event::Mouse(MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(button),
            }),
            size,
        );
    }

    fn key_at(&mut self, key: &KeyEvent, size: (f32, f32)) {
        self.window_width = size.0;
        self.window_height = size.1;
        self.handle_event(&Event::Key(key.clone()), size);
    }
}

fn main() -> ExitCode {
    // The previous `main` was three lines: build the store, load the sample
    // data, render one frame into a `Vec` and drop it. It exercised the
    // drawing code and showed nobody the result.
    let mut app = ContactsApp::new();
    app.load_sample_data();
    app::launch("contacts", &mut app)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the line
    // that did it -- that is the diagnosis. The defensive lints exist to keep
    // panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // -----------------------------------------------------------------------
    // Helper: create a minimal contact for tests
    // -----------------------------------------------------------------------
    fn make_contact(first: &str, last: &str) -> Contact {
        Contact::new(0, first, last)
    }

    // -- vCard escaping ------------------------------------------------------

    /// Values that discriminate a correct decoder from a `.replace()` chain.
    /// The first is the one that matters: a backslash followed by the letter
    /// `n` is what a chained decoder turns into a real newline.
    const HOSTILE_VALUES: &[&str] = &[
        r"C:\notes\new",
        r"\n",
        r"\\n",
        r"\",
        r"trailing\",
        "a,b;c",
        "line\nbreak",
        "line\r\nbreak",
        "lone\rcr",
        r"mixed \, and \; and \\ and \n",
        "",
    ];

    #[test]
    fn a_backslash_before_an_n_survives_the_round_trip() {
        // The regression. `vcard_escape(r"\n")` is `\\n`; a decoder that
        // replaces "\\n" -> newline before "\\\\" -> "\\" reads the leading
        // two characters as a newline escape and returns "\n" the character.
        assert_eq!(vcard_unescape(&vcard_escape(r"\n")), r"\n");
        assert_eq!(
            vcard_unescape(&vcard_escape(r"C:\notes\new")),
            r"C:\notes\new"
        );
    }

    #[test]
    fn every_escaped_value_decodes_to_itself() {
        for v in HOSTILE_VALUES {
            let decoded = vcard_unescape(&vcard_escape(v));
            // A CR is deliberately normalised to a line break -- it has no
            // vCard escape and would otherwise end the property line.
            let want = v.replace("\r\n", "\n").replace('\r', "\n");
            assert_eq!(&decoded, &want, "round trip changed {v:?}");
        }
    }

    /// Stability under repeated save/load, which is a *separate* property from
    /// correctness and is not what the replace-chain bug violated — that one
    /// corrupted a value once and then held it steady. This guards the other
    /// failure mode: a codec that keeps rewriting its own output (one more
    /// backslash per save is the classic shape) and grows without bound.
    #[test]
    fn repeated_round_trips_reach_a_fixed_point() {
        for v in HOSTILE_VALUES {
            let once = vcard_unescape(&vcard_escape(v));
            let mut cur = once.clone();
            for _ in 0..5 {
                cur = vcard_unescape(&vcard_escape(&cur));
                assert_eq!(cur, once, "value {v:?} kept drifting on re-save");
            }
        }
    }

    #[test]
    fn a_newline_in_a_note_cannot_forge_a_vcard_property() {
        let mut c = make_contact("Ann", "Ash");
        // If the newline reached the file unescaped, everything after it
        // would be read as a new property line.
        c.notes = String::from("harmless\nTEL:+1-555-9999\nmore");
        let card = c.to_vcard();
        let tel_lines = card.lines().filter(|l| l.starts_with("TEL")).count();
        assert_eq!(tel_lines, 0, "a note forged a TEL property:\n{card}");
    }

    #[test]
    fn a_carriage_return_in_a_note_cannot_end_the_property_line() {
        let mut c = make_contact("Ann", "Ash");
        c.notes = String::from("harmless\rTEL:+1-555-9999");
        let card = c.to_vcard();
        assert!(
            !card.contains('\r') || !card.contains("\rTEL:"),
            "a bare CR survived into the card:\n{card}"
        );
        assert_eq!(card.lines().filter(|l| l.starts_with("TEL")).count(), 0);
    }

    #[test]
    fn a_contact_note_with_a_backslash_survives_export_and_import() {
        let mut c = make_contact("Ann", "Ash");
        c.notes = String::from(r"path C:\new\table, and a ; too");
        let card = c.to_vcard();
        let back = Contact::from_vcard(&card, 0).expect("card should parse back");
        assert_eq!(back.notes, c.notes, "note corrupted by a real round trip");
    }

    fn make_store_with_contacts() -> ContactStore {
        let mut store = ContactStore::new();
        let mut c1 = make_contact("Alice", "Anderson");
        c1.phones
            .push(PhoneNumber::new("+1-555-0101", PhoneType::Mobile).with_primary(true));
        c1.emails
            .push(EmailAddress::new("alice@example.com", EmailType::Personal).with_primary(true));
        c1.company = String::from("Acme Corp");
        c1.created_at = 1000;
        store.add_contact(c1);

        let mut c2 = make_contact("Bob", "Baker");
        c2.phones
            .push(PhoneNumber::new("+1-555-0201", PhoneType::Work));
        c2.emails
            .push(EmailAddress::new("bob@work.com", EmailType::Work));
        c2.company = String::from("Baker Inc");
        c2.created_at = 2000;
        store.add_contact(c2);

        let mut c3 = make_contact("Carol", "Chen");
        c3.phones
            .push(PhoneNumber::new("+1-555-0301", PhoneType::Home));
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
        for ptype in &[
            PhoneType::Mobile,
            PhoneType::Home,
            PhoneType::Work,
            PhoneType::Fax,
        ] {
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

    /// A day past the end of its own month is not a date, even though it is
    /// within `1..=31`.
    ///
    /// The flat range check this replaces accepted every one of these, so a
    /// mistyped or badly-exported birthday was stored and shown back as if it
    /// were real. Each month here is checked one day past its length and on its
    /// last day, so the test also catches the table being off by one in either
    /// direction rather than merely being consulted.
    #[test]
    fn a_day_past_the_end_of_its_month_is_not_a_date() {
        let lengths = [
            (1, 31),
            (2, 28),
            (3, 31),
            (4, 30),
            (5, 31),
            (6, 30),
            (7, 31),
            (8, 31),
            (9, 30),
            (10, 31),
            (11, 30),
            (12, 31),
        ];
        for (month, length) in lengths {
            assert!(
                SimpleDate::new(2001, month, length).is_some(),
                "month {month} should have a day {length}"
            );
            assert!(
                SimpleDate::new(2001, month, length + 1).is_none(),
                "month {month} has no day {}",
                length + 1
            );
        }
    }

    /// 29 February exists in a leap year and not otherwise, and the century
    /// rules are part of that.
    ///
    /// 1900 and 2000 are the pair that separates a real leap rule from
    /// `year % 4`: both are divisible by four, only one is a leap year.
    #[test]
    fn the_twenty_ninth_of_february_follows_the_leap_rule() {
        assert!(
            SimpleDate::new(1988, 2, 29).is_some(),
            "1988 was a leap year"
        );
        assert!(SimpleDate::new(1989, 2, 29).is_none(), "1989 was not");
        assert!(
            SimpleDate::new(2000, 2, 29).is_some(),
            "2000 was, by the 400 rule"
        );
        assert!(
            SimpleDate::new(1900, 2, 29).is_none(),
            "1900 was not, by the 100 rule"
        );
        // 30 February is not a date in any year.
        assert!(SimpleDate::new(2000, 2, 30).is_none());
    }

    /// vCard's basic date form must import, not vanish.
    ///
    /// vCard 4.0 writes `BDAY:19901225` with no separators, and that is what
    /// phones and mail clients export. Accepting only the extended form meant
    /// such a card imported with its birthday silently missing -- indistinguish-
    /// able, to the person looking at the contact afterwards, from a card that
    /// never carried one.
    #[test]
    fn a_vcard_basic_format_birthday_is_not_dropped() {
        let d = SimpleDate::parse("19901225").expect("basic ISO form should parse");
        assert_eq!((d.year, d.month, d.day), (1990, 12, 25));
        assert_eq!(
            d,
            SimpleDate::parse("1990-12-25").expect("extended form should parse"),
            "the two spellings of one date must produce one date"
        );

        let vcard =
            "BEGIN:VCARD\r\nVERSION:4.0\r\nN:;Jane;;;\r\nFN:Jane\r\nBDAY:19880229\r\nEND:VCARD";
        let imported = Contact::from_vcard(vcard, 1).expect("card should import");
        assert_eq!(
            imported.birthday,
            SimpleDate::new(1988, 2, 29),
            "the birthday came through as {:?}",
            imported.birthday
        );
    }

    /// Eight digits that are not a date are still not a date.
    ///
    /// The basic form is recognised by shape alone -- eight ASCII digits -- so
    /// the validation in `new` is the only thing standing between that shape
    /// and nonsense getting through.
    #[test]
    fn eight_digits_alone_do_not_make_a_date() {
        assert!(SimpleDate::parse("19901325").is_none(), "month 13");
        assert!(SimpleDate::parse("19900230").is_none(), "30 February");
        assert!(SimpleDate::parse("1990122").is_none(), "seven digits");
        assert!(SimpleDate::parse("199012255").is_none(), "nine digits");
        assert!(SimpleDate::parse("1990DEC25").is_none(), "not all digits");
    }

    /// The first of each month, written out rather than derived.
    ///
    /// This used to build its expectation by accumulating the same
    /// `MONTH_LENGTHS` table that `day_of_year` summed, so both sides moved
    /// together and the only thing that could actually fail was the closing
    /// "adds up to 365" check. A table of `[30, 29, 31, …]` — wrong in two
    /// months, right in total — passed it. That is the shape this whole change
    /// is about: an assertion that restates the implementation cannot fail.
    ///
    /// The numbers below are the ordinal of the first of each month in a
    /// common year, and they are independent of anything in this file.
    #[test]
    fn the_first_of_each_month_is_the_day_after_the_last_of_the_one_before() {
        let firsts: [u16; 12] = [1, 32, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];
        for (index, &want) in firsts.iter().enumerate() {
            let month = u8::try_from(index + 1).unwrap_or(1);
            assert_eq!(day_of_year(month, 1), want, "the first of month {month}");
        }
        // The last day of a common year, which pins the far end the same way
        // the literals above pin each boundary.
        assert_eq!(day_of_year(12, 31), 365, "31 December");
        // February is 28 days here whatever the year, which is the documented
        // point of `day_of_year`: it compares a birthday against today, and
        // those fall in different years, so applying each side's own leap rule
        // would move one of them and not the other.
        assert_eq!(day_of_year(2, 28), 59);
        assert_eq!(day_of_year(3, 1), 60);
    }

    /// A month past December counts every month, not December twice.
    ///
    /// `date::days_in_month` clamps an out-of-range month into 1..=12, so
    /// handing it a 14 unclamped would have counted December's 31 days twice
    /// over and put the answer past the end of the year. The clamp therefore
    /// has to happen on the *range*, before the lookup, and this is what says
    /// so.
    #[test]
    fn a_month_outside_the_calendar_clamps_to_the_whole_year() {
        assert_eq!(day_of_year(13, 1), 366, "one past December");
        assert_eq!(day_of_year(14, 1), 366, "two past December");
        assert_eq!(day_of_year(255, 1), 366, "as far past as a u8 goes");
        assert_eq!(day_of_year(0, 1), 1, "month zero counts nothing before it");
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
        c.phones
            .push(PhoneNumber::new("+1-555-0101", PhoneType::Mobile));
        assert!(c.matches_search("555-0101"));
    }

    #[test]
    fn test_search_by_email() {
        let mut c = make_contact("Alice", "Anderson");
        c.emails
            .push(EmailAddress::new("alice@example.com", EmailType::Personal));
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
        c1.emails
            .push(EmailAddress::new("a@b.com", EmailType::Personal));
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
        c1.phones
            .push(PhoneNumber::new("+1-555-0100", PhoneType::Mobile));
        let mut c2 = Contact::new(2, "Bob", "B");
        c2.phones
            .push(PhoneNumber::new("15550100", PhoneType::Work)); // same digits
        let dups = find_duplicates(&[c1, c2]);
        assert_eq!(dups.len(), 1);
        assert_eq!(dups[0].reason, DuplicateReason::SamePhone);
    }

    #[test]
    fn test_duplicate_same_email() {
        let mut c1 = Contact::new(1, "Alice", "A");
        c1.emails
            .push(EmailAddress::new("same@example.com", EmailType::Personal));
        let mut c2 = Contact::new(2, "Bob", "B");
        c2.emails
            .push(EmailAddress::new("SAME@EXAMPLE.COM", EmailType::Work));
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
        c1.phones
            .push(PhoneNumber::new("+1-555-0100", PhoneType::Mobile));
        let mut c2 = Contact::new(2, "Alice", "Anderson");
        c2.phones
            .push(PhoneNumber::new("15550100", PhoneType::Work)); // same digits
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.phones.len(), 1);
    }

    #[test]
    fn test_merge_contacts_emails() {
        let mut c1 = Contact::new(1, "A", "A");
        c1.emails
            .push(EmailAddress::new("a@a.com", EmailType::Personal));
        let mut c2 = Contact::new(2, "A", "A");
        c2.emails
            .push(EmailAddress::new("b@b.com", EmailType::Work));
        let merged = merge_contacts(&c1, &c2, 3);
        assert_eq!(merged.emails.len(), 2);
    }

    #[test]
    fn test_merge_contacts_dedup_emails() {
        let mut c1 = Contact::new(1, "A", "A");
        c1.emails
            .push(EmailAddress::new("same@test.com", EmailType::Personal));
        let mut c2 = Contact::new(2, "A", "A");
        c2.emails
            .push(EmailAddress::new("SAME@TEST.COM", EmailType::Work));
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
        c.phones
            .push(PhoneNumber::new("+1-555-0100", PhoneType::Mobile).with_primary(true));
        let vcard = c.to_vcard();
        assert!(vcard.contains("TEL;TYPE=CELL;PREF:+1-555-0100"));
    }

    #[test]
    fn test_vcard_export_with_email() {
        let mut c = Contact::new(1, "John", "Doe");
        c.emails
            .push(EmailAddress::new("john@example.com", EmailType::Work));
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
        c.social_accounts
            .push(SocialAccount::new(SocialPlatform::Twitter, "@johnd"));
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
        let data =
            "BEGIN:VCARD\r\nVERSION:3.0\r\nN:;John;;;\r\nFN:John\r\nBDAY:1990-06-15\r\nEND:VCARD";
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
        c.phones
            .push(PhoneNumber::new("+1-555-0999", PhoneType::Work).with_primary(true));
        c.emails
            .push(EmailAddress::new("jane@tech.co", EmailType::Work).with_primary(true));
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

    const LONG_NOTES: &str = "Met at the Rust conference in Amsterdam; wants an \
        introduction to the storage team before the next planning round, and \
        prefers email over phone for anything that is not urgent.";

    /// An app showing one contact's detail panel, with the given notes.
    fn app_viewing_notes(notes: &str) -> ContactsApp {
        let mut app = ContactsApp::new();
        let mut c = Contact::new(0, "Dana", "Devlin");
        c.notes = notes.to_string();
        let id = app.store.add_contact(c);
        app.view = DetailView::ViewContact(id);
        app
    }

    /// The `(y, text)` of every notes line drawn in the detail panel.
    fn notes_lines_drawn(app: &ContactsApp) -> Vec<(f32, String)> {
        app.render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    font_size,
                    color,
                    ..
                } if (font_size - NOTES_FONT_SIZE).abs() < 0.01 && color == SUBTEXT0 => {
                    Some((y, text))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn long_notes_are_wrapped_not_truncated_to_one_line() {
        // `RenderCommand::Text` clips at `max_width` rather than wrapping, so
        // the notes used to show one line's worth of characters and no more.
        let app = app_viewing_notes(LONG_NOTES);
        let lines = notes_lines_drawn(&app);
        assert!(
            lines.len() > 1,
            "the notes were drawn as {} command(s)",
            lines.len()
        );
        let drawn: String = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in LONG_NOTES.split_whitespace() {
            assert!(drawn.contains(word), "the notes lost the word {word:?}");
        }
    }

    #[test]
    fn the_groups_section_starts_below_the_notes() {
        // The detail panel is a running cursor, so wrapping the notes without
        // advancing it would have drawn the groups heading over the notes.
        let mut app = app_viewing_notes(LONG_NOTES);
        let gid = app.store.add_group(ContactGroup::new(0, "Storage team"));
        let DetailView::ViewContact(cid) = app.view else {
            panic!("the app is not viewing a contact");
        };
        assert!(app.store.add_contact_to_group(cid, gid));

        let notes_bottom = notes_lines_drawn(&app)
            .iter()
            .map(|(y, _)| y + NOTES_LINE_HEIGHT)
            .fold(f32::MIN, f32::max);
        // The sidebar now carries a `Groups` *button* as well, so matching on
        // the word alone would find that button -- which sits at the top of
        // the window and would fail this test no matter where the panel drew
        // its heading. The heading is the bold OVERLAY0 run; the button is a
        // regular-weight one. Assert there is exactly one of each shape so
        // that a future third `Groups` run cannot quietly be picked instead.
        let headings: Vec<f32> = app
            .render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    ref text,
                    color,
                    font_weight,
                    ..
                } if text == "Groups"
                    && font_weight == FontWeightHint::Bold
                    && color == OVERLAY0 =>
                {
                    Some(y)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            headings.len(),
            1,
            "expected exactly one groups section heading, found {headings:?}"
        );
        let groups_heading_y = headings.first().copied().unwrap();
        assert!(
            groups_heading_y >= notes_bottom,
            "the groups heading at {groups_heading_y} sits inside the notes, \
             which end at {notes_bottom}"
        );
    }

    #[test]
    fn a_contact_with_no_notes_draws_none() {
        let app = app_viewing_notes("");
        assert!(notes_lines_drawn(&app).is_empty());
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
        c.emails
            .push(EmailAddress::new("a@b.com", EmailType::Personal));
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
        assert_eq!(
            DuplicateReason::SameNameAndCompany.label(),
            "Same name & company"
        );
    }

    // -----------------------------------------------------------------------
    // Primary phone/email helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_primary_phone_returns_primary() {
        let mut c = Contact::new(1, "A", "A");
        c.phones.push(PhoneNumber::new("111", PhoneType::Home));
        c.phones
            .push(PhoneNumber::new("222", PhoneType::Mobile).with_primary(true));
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
        c.emails
            .push(EmailAddress::new("a@a.com", EmailType::Personal));
        c.emails
            .push(EmailAddress::new("b@b.com", EmailType::Work).with_primary(true));
        assert_eq!(c.primary_email().unwrap().email, "b@b.com");
    }

    #[test]
    fn test_primary_email_fallback_first() {
        let mut c = Contact::new(1, "A", "A");
        c.emails
            .push(EmailAddress::new("a@a.com", EmailType::Personal));
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
        c1.social_accounts
            .push(SocialAccount::new(SocialPlatform::GitHub, "@alice"));
        let mut c2 = Contact::new(2, "A", "A");
        c2.social_accounts
            .push(SocialAccount::new(SocialPlatform::GitHub, "@alice"));
        c2.social_accounts
            .push(SocialAccount::new(SocialPlatform::Twitter, "@alice"));
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
