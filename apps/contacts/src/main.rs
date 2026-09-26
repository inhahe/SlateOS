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
//! # What is kept
//!
//! Everything: every contact with all its numbers, addresses, accounts and
//! groups, the groups, and who was looked at last, in one file in the settings
//! directory (`contacts/address-book.txt`), written after every event that
//! changed it -- there is no Save. Until 2026-09-25 nothing was: the book lived
//! in memory, and the only way to keep anyone was to export a vCard file.
//!
//! The file is the notes library's kind (design-decisions §1205): tab-separated
//! text, a record a line (`book_text`, `parse_book`), read whole or not at all.
//! One that cannot be understood completely is left exactly as it is, nothing
//! is written over it, and the window says so for as long as it is open. The
//! store counts its own changes (`ContactStore::revision`), so the window
//! cannot miss one: its fields are private, and every method that can change
//! it counts.
//!
//! Uses the guitk library for UI rendering with Catppuccin Mocha theme.

use appearance::Palette;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
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
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textfmt::tsv;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

// Every one of these carried an `#[allow(dead_code)]`, and five of them --
// self.palette.crust, MAUVE, TEAL, PINK, ROSEWATER -- were never named anywhere but on
// their own definition line. The `allow` is what let that be true for as long
// as it was: it silences the one warning that would have said so. They are
// deleted rather than kept "for later", because a palette entry no drawing
// call reaches is not a palette entry, and the next reader would have had to
// grep the file to find that out.

// ============================================================================
// Constants
// ============================================================================

/// What the window says before any contact exists. Under it goes
/// [`ContactsApp::keeping_line`]: where the book is kept, or why it is not.
///
/// The second line was "Nothing is saved automatically -- press Ctrl+S to
/// write a vCard file, or anyone you add is gone when the window closes",
/// which was true until the address book was kept (2026-09-25).
const NO_CONTACTS: &str = "No contacts yet -- N adds one.";

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
}

impl ContactGroup {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            description: String::new(),
            // Content, not chrome: a group's colour is the user's own
            // choice, stored per group and shown as its swatch, so it does
            // not follow the desktop theme.
            color: Color::from_hex(0x89B4FA),
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
        let display = default_display_name(first_name, last_name);
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
    /// How many changes the store has had. Every method that can change it
    /// counts one -- the ones that hand out `&mut` included, whether or not
    /// the caller then changes anything -- so the window keeps the address
    /// book whenever this has moved (`ContactsApp::keep`). The fields are
    /// private, so there is no other way in to miss.
    revision: u64,
}

impl ContactStore {
    pub fn new() -> Self {
        Self {
            contacts: Vec::new(),
            groups: Vec::new(),
            next_contact_id: 1,
            next_group_id: 1,
            recently_viewed: VecDeque::new(),
            revision: 0,
        }
    }

    /// How many changes the store has had; see the field.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Count a change.
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    // ----- Contact CRUD -----

    /// Add a new contact, returning its assigned ID.
    pub fn add_contact(&mut self, mut contact: Contact) -> u64 {
        let id = self.next_contact_id;
        self.next_contact_id = self.next_contact_id.saturating_add(1);
        contact.id = id;
        self.contacts.push(contact);
        self.changed();
        id
    }

    /// Get a contact by ID.
    pub fn get_contact(&self, id: u64) -> Option<&Contact> {
        self.contacts.iter().find(|c| c.id == id)
    }

    /// Get a mutable reference to a contact by ID. Counted as a change: what
    /// the caller does with it cannot be seen from here.
    pub fn get_contact_mut(&mut self, id: u64) -> Option<&mut Contact> {
        self.changed();
        self.contacts.iter_mut().find(|c| c.id == id)
    }

    /// Delete a contact by ID. Returns true if found and removed.
    pub fn delete_contact(&mut self, id: u64) -> bool {
        let before = self.contacts.len();
        self.contacts.retain(|c| c.id != id);
        // Also remove from recently viewed
        self.recently_viewed.retain(|&rid| rid != id);
        let deleted = self.contacts.len() != before;
        if deleted {
            self.changed();
        }
        deleted
    }

    /// Update a contact (replace by ID). Returns true if found and updated.
    pub fn update_contact(&mut self, contact: Contact) -> bool {
        if let Some(existing) = self.contacts.iter_mut().find(|c| c.id == contact.id) {
            *existing = contact;
            self.changed();
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
        self.changed();
        id
    }

    /// Get a group by ID.
    pub fn get_group(&self, id: u64) -> Option<&ContactGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Get a mutable reference to a group. Counted as a change, as
    /// `get_contact_mut` is.
    pub fn get_group_mut(&mut self, id: u64) -> Option<&mut ContactGroup> {
        self.changed();
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
        let deleted = self.groups.len() != before;
        if deleted {
            self.changed();
        }
        deleted
    }

    /// Get all groups.
    pub fn all_groups(&self) -> &[ContactGroup] {
        &self.groups
    }

    /// Add a contact to a group -- one the store has: a contact in a group
    /// that is not there would make the address book a file that cannot be
    /// read back.
    pub fn add_contact_to_group(&mut self, contact_id: u64, group_id: u64) -> bool {
        if !self.groups.iter().any(|g| g.id == group_id) {
            return false;
        }
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == contact_id)
            && !contact.groups.contains(&group_id)
        {
            contact.groups.push(group_id);
            self.changed();
            return true;
        }
        false
    }

    /// Remove a contact from a group.
    pub fn remove_contact_from_group(&mut self, contact_id: u64, group_id: u64) -> bool {
        if let Some(contact) = self.contacts.iter_mut().find(|c| c.id == contact_id) {
            let before = contact.groups.len();
            contact.groups.retain(|&gid| gid != group_id);
            let removed = contact.groups.len() != before;
            if removed {
                self.changed();
            }
            return removed;
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
        let contact = self.contacts.iter_mut().find(|c| c.id == id)?;
        contact.favorite = !contact.favorite;
        let now_favorite = contact.favorite;
        self.changed();
        Some(now_favorite)
    }

    /// Get favorite contacts.
    pub fn favorites(&self) -> Vec<&Contact> {
        self.contacts.iter().filter(|c| c.favorite).collect()
    }

    // ----- Recently viewed -----

    /// Record that a contact was viewed.
    pub fn record_view(&mut self, id: u64) {
        // Already the most recent: nothing to change, and nothing to keep.
        if self.recently_viewed.front() == Some(&id) {
            return;
        }
        // Remove existing occurrence, push to front
        self.recently_viewed.retain(|&rid| rid != id);
        self.recently_viewed.push_front(id);
        while self.recently_viewed.len() > MAX_RECENT {
            self.recently_viewed.pop_back();
        }
        self.changed();
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
            self.changed();
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
        self.changed();

        Some(merged_id)
    }

    // ----- Import/Export -----

    /// Export all contacts as vCard text.
    pub fn export_all(&self) -> String {
        export_vcards(&self.contacts)
    }

    /// Import contacts from vCard text, as added at `now` (milliseconds since
    /// 1970), so "recently added" puts them first. Returns number of contacts
    /// imported.
    pub fn import_vcards(&mut self, data: &str, now: u64) -> usize {
        let imported = import_vcards(data, self.next_contact_id);
        let count = imported.len();
        for mut contact in imported {
            contact.created_at = now;
            contact.updated_at = now;
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
// The address book file
// ============================================================================

/// The address book file's first field, which says what the file is.
const BOOK_MAGIC: &str = "slateos-contacts";

/// The version of the address book file this writes, and the newest it reads.
const BOOK_FORMAT: u32 = 1;

/// The largest address book this will read. One cut short would be read as a
/// smaller book with no sign anyone was missing, so a larger file is refused
/// rather than read in part.
const MAX_BOOK_BYTES: usize = 64 * 1024 * 1024;

/// Why nothing is kept, when the environment names no home directory.
const NO_HOME: &str = "Nothing is kept: no home directory is set";

/// Where the address book is kept, or `None` when the environment names no
/// home directory -- an early-boot or stripped service environment, where
/// there is no user to have contacts.
fn book_path() -> Option<std::path::PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("contacts").join("address-book.txt"))
}

/// Milliseconds since 1970 by the clock; 0 if it reads earlier than that.
fn clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// A yes-or-no as written.
fn flag(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// A yes-or-no as read, or `None` for anything [`flag`] never writes.
fn read_flag(field: &str) -> Option<bool> {
    match field {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// A group's colour as written: `#RRGGBB`, or `#RRGGBBAA` when it is not
/// opaque -- every bit of it, so it comes back the same colour.
fn colour_hex(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

/// A colour read back from `#RRGGBB` or `#RRGGBBAA`.
fn parse_colour(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
        return None;
    }
    let byte = |at: usize| {
        hex.get(at..at.saturating_add(2))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, a))
}

fn phone_key(t: PhoneType) -> &'static str {
    match t {
        PhoneType::Mobile => "mobile",
        PhoneType::Home => "home",
        PhoneType::Work => "work",
        PhoneType::Fax => "fax",
        PhoneType::Other => "other",
    }
}

fn phone_from_key(key: &str) -> Option<PhoneType> {
    Some(match key {
        "mobile" => PhoneType::Mobile,
        "home" => PhoneType::Home,
        "work" => PhoneType::Work,
        "fax" => PhoneType::Fax,
        "other" => PhoneType::Other,
        _ => return None,
    })
}

fn email_key(t: EmailType) -> &'static str {
    match t {
        EmailType::Personal => "personal",
        EmailType::Work => "work",
        EmailType::Other => "other",
    }
}

fn email_from_key(key: &str) -> Option<EmailType> {
    Some(match key {
        "personal" => EmailType::Personal,
        "work" => EmailType::Work,
        "other" => EmailType::Other,
        _ => return None,
    })
}

fn address_key(t: AddressType) -> &'static str {
    match t {
        AddressType::Home => "home",
        AddressType::Work => "work",
        AddressType::Other => "other",
    }
}

fn address_from_key(key: &str) -> Option<AddressType> {
    Some(match key {
        "home" => AddressType::Home,
        "work" => AddressType::Work,
        "other" => AddressType::Other,
        _ => return None,
    })
}

/// A platform as written, and the name a custom one carries (empty for the
/// others).
fn platform_fields(platform: &SocialPlatform) -> (&'static str, &str) {
    match platform {
        SocialPlatform::Twitter => ("twitter", ""),
        SocialPlatform::LinkedIn => ("linkedin", ""),
        SocialPlatform::GitHub => ("github", ""),
        SocialPlatform::Mastodon => ("mastodon", ""),
        SocialPlatform::Custom(name) => ("custom", name.as_str()),
    }
}

fn platform_from_fields(key: &str, name: String) -> Option<SocialPlatform> {
    Some(match key {
        "twitter" => SocialPlatform::Twitter,
        "linkedin" => SocialPlatform::LinkedIn,
        "github" => SocialPlatform::GitHub,
        "mastodon" => SocialPlatform::Mastodon,
        "custom" => SocialPlatform::Custom(name),
        _ => return None,
    })
}

/// The address book as text: a first line naming the format, a line per
/// group, then each contact followed by the lines that belong to it, then the
/// recently viewed.
///
/// ```text
/// slateos-contacts  1
/// group    <id>  <#RRGGBB>  <name>  <description>
/// contact  <id>  <favourite 1|0>  <created>  <updated>  <last contacted, or nothing>
///          <birthday YYYY-MM-DD, or nothing>  <first>  <last>  <display name>
///          <nickname>  <company>  <job title>  <department>
///          <photo: nothing, or = and the path>  <notes>
/// phone    <mobile|home|work|fax|other>  <primary 1|0>  <number>
/// email    <personal|work|other>  <primary 1|0>  <address>
/// address  <home|work|other>  <street>  <city>  <state>  <zip>  <country>
/// social   <twitter|linkedin|github|mastodon|custom>  <custom name>  <handle>
/// member   <group id>
/// viewed   <contact id>...
/// ```
///
/// Fields are separated by tabs and escaped with `textfmt::tsv`; times are
/// milliseconds since 1970. A line that belongs to a contact belongs to the
/// contact line above it. A membership of a group the book does not have, or
/// a view of a contact it does not have, is not written: neither can arise
/// from the store's own methods, and either would make the file one that
/// cannot be read back.
fn book_text(store: &ContactStore) -> String {
    let mut out = format!("{BOOK_MAGIC}\t{BOOK_FORMAT}\n");
    for group in &store.groups {
        out.push_str(&format!(
            "group\t{}\t{}\t{}\t{}\n",
            group.id,
            colour_hex(group.color),
            tsv::escape(&group.name),
            tsv::escape(&group.description)
        ));
    }
    for c in &store.contacts {
        out.push_str(&format!(
            "contact\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            c.id,
            flag(c.favorite),
            c.created_at,
            c.updated_at,
            c.last_contacted.map_or_else(String::new, |t| t.to_string()),
            c.birthday.map_or_else(String::new, |b| b.format_display()),
            tsv::escape(&c.first_name),
            tsv::escape(&c.last_name),
            tsv::escape(&c.display_name),
            tsv::escape(&c.nickname),
            tsv::escape(&c.company),
            tsv::escape(&c.job_title),
            tsv::escape(&c.department),
            c.photo_path
                .as_deref()
                .map_or_else(String::new, |path| format!("={}", tsv::escape(path))),
            tsv::escape(&c.notes)
        ));
        for phone in &c.phones {
            out.push_str(&format!(
                "phone\t{}\t{}\t{}\n",
                phone_key(phone.phone_type),
                flag(phone.primary),
                tsv::escape(&phone.number)
            ));
        }
        for email in &c.emails {
            out.push_str(&format!(
                "email\t{}\t{}\t{}\n",
                email_key(email.email_type),
                flag(email.primary),
                tsv::escape(&email.email)
            ));
        }
        for a in &c.addresses {
            out.push_str(&format!(
                "address\t{}\t{}\t{}\t{}\t{}\t{}\n",
                address_key(a.address_type),
                tsv::escape(&a.street),
                tsv::escape(&a.city),
                tsv::escape(&a.state),
                tsv::escape(&a.zip),
                tsv::escape(&a.country)
            ));
        }
        for social in &c.social_accounts {
            let (key, name) = platform_fields(&social.platform);
            out.push_str(&format!(
                "social\t{key}\t{}\t{}\n",
                tsv::escape(name),
                tsv::escape(&social.handle)
            ));
        }
        for gid in &c.groups {
            if store.groups.iter().any(|g| g.id == *gid) {
                out.push_str(&format!("member\t{gid}\n"));
            }
        }
    }
    let viewed: Vec<String> = store
        .recently_viewed
        .iter()
        .filter(|id| store.contacts.iter().any(|c| c.id == **id))
        .map(u64::to_string)
        .collect();
    if !viewed.is_empty() {
        out.push_str(&format!("viewed\t{}\n", viewed.join("\t")));
    }
    out
}

/// An address book read from its text, or why it cannot be -- naming the
/// line.
///
/// All or nothing, for the finance ledger's reason (design-decisions §1202,
/// and the notes library's §1205): a book read in part and then kept again
/// would lose, without a word, whoever was not read. So is one that puts a
/// contact in a group it does not have, holds two things with one number, or
/// names a contact it does not have among the recently viewed.
fn parse_book(text: &str) -> Result<ContactStore, String> {
    let mut lines = text.lines().enumerate();
    let first = lines.next().map_or("", |(_, line)| line);
    let head: Vec<&str> = first.split('\t').collect();
    let version = match head.as_slice() {
        [BOOK_MAGIC, version] => version
            .parse::<u32>()
            .map_err(|_| String::from("line 1 names no format"))?,
        _ => return Err(String::from("it is not a SlateOS address book")),
    };
    if version > BOOK_FORMAT {
        return Err(format!(
            "it is a later format ({version}) than this version reads ({BOOK_FORMAT})"
        ));
    }
    if version < BOOK_FORMAT {
        return Err(format!("format {version} is not one this program wrote"));
    }

    let mut store = ContactStore::new();
    let mut viewed: Option<VecDeque<u64>> = None;
    // Each membership's line, to name it if its group is missing -- which is
    // only known once every group has been read.
    let mut memberships: Vec<(usize, u64)> = Vec::new();
    let mut viewed_line = 0_usize;
    for (i, line) in lines {
        let at = i.saturating_add(1);
        if line.is_empty() {
            continue;
        }
        let bad = |why: &str| format!("line {at}: {why}");
        let text = |field: &str| {
            tsv::unescape(field)
                .ok_or_else(|| bad("a text holds an escape this program never writes"))
        };
        let number = |field: &str, what: &str| {
            field
                .parse::<u64>()
                .map_err(|_| bad(&format!("{what} is not a number")))
        };
        let yes_no =
            |field: &str| read_flag(field).ok_or_else(|| bad("a yes-or-no is neither 1 nor 0"));
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["group", id, colour, name, description] => {
                let id = number(id, "a group's number")?;
                if store.groups.iter().any(|g| g.id == id) {
                    return Err(bad("two groups have one number"));
                }
                let color = parse_colour(colour).ok_or_else(|| bad("a colour is not one"))?;
                store.groups.push(
                    ContactGroup::new(id, &text(name)?)
                        .with_color(color)
                        .with_description(&text(description)?),
                );
            }
            [
                "contact",
                id,
                favourite,
                created,
                updated,
                contacted,
                birthday,
                first,
                last,
                display,
                nickname,
                company,
                job,
                department,
                photo,
                notes,
            ] => {
                let id = number(id, "a contact's number")?;
                if store.contacts.iter().any(|c| c.id == id) {
                    return Err(bad("two contacts have one number"));
                }
                let mut c = Contact::new(id, &text(first)?, &text(last)?);
                c.favorite = yes_no(favourite)?;
                c.created_at = number(created, "when a contact was added")?;
                c.updated_at = number(updated, "when a contact was changed")?;
                c.last_contacted = if contacted.is_empty() {
                    None
                } else {
                    Some(number(contacted, "when a contact was last reached")?)
                };
                c.birthday = if birthday.is_empty() {
                    None
                } else {
                    Some(
                        SimpleDate::parse(birthday)
                            .ok_or_else(|| bad("a birthday is not a date"))?,
                    )
                };
                c.display_name = text(display)?;
                c.nickname = text(nickname)?;
                c.company = text(company)?;
                c.job_title = text(job)?;
                c.department = text(department)?;
                c.photo_path = if photo.is_empty() {
                    None
                } else {
                    let path = photo
                        .strip_prefix('=')
                        .ok_or_else(|| bad("a photo is neither nothing nor a path"))?;
                    Some(text(path)?)
                };
                c.notes = text(notes)?;
                store.contacts.push(c);
            }
            ["phone", kind, primary, number_text] => {
                let phone = PhoneNumber {
                    number: text(number_text)?,
                    phone_type: phone_from_key(kind).ok_or_else(|| {
                        bad("a phone number is of a kind this version does not know")
                    })?,
                    primary: yes_no(primary)?,
                };
                store
                    .contacts
                    .last_mut()
                    .ok_or_else(|| bad("a phone number comes before any contact"))?
                    .phones
                    .push(phone);
            }
            ["email", kind, primary, address] => {
                let email = EmailAddress {
                    email: text(address)?,
                    email_type: email_from_key(kind).ok_or_else(|| {
                        bad("an email address is of a kind this version does not know")
                    })?,
                    primary: yes_no(primary)?,
                };
                store
                    .contacts
                    .last_mut()
                    .ok_or_else(|| bad("an email address comes before any contact"))?
                    .emails
                    .push(email);
            }
            ["address", kind, street, city, state, zip, country] => {
                let mut address =
                    PostalAddress::new(address_from_key(kind).ok_or_else(|| {
                        bad("an address is of a kind this version does not know")
                    })?);
                address.street = text(street)?;
                address.city = text(city)?;
                address.state = text(state)?;
                address.zip = text(zip)?;
                address.country = text(country)?;
                store
                    .contacts
                    .last_mut()
                    .ok_or_else(|| bad("an address comes before any contact"))?
                    .addresses
                    .push(address);
            }
            ["social", kind, name, handle] => {
                let platform = platform_from_fields(kind, text(name)?)
                    .ok_or_else(|| bad("an account is on a service this version does not know"))?;
                let account = SocialAccount::new(platform, &text(handle)?);
                store
                    .contacts
                    .last_mut()
                    .ok_or_else(|| bad("an account comes before any contact"))?
                    .social_accounts
                    .push(account);
            }
            ["member", group] => {
                let group = number(group, "a group's number")?;
                let contact = store
                    .contacts
                    .last_mut()
                    .ok_or_else(|| bad("a group membership comes before any contact"))?;
                if contact.groups.contains(&group) {
                    return Err(bad("a contact is in one group twice"));
                }
                contact.groups.push(group);
                memberships.push((at, group));
            }
            ["viewed", ids @ ..] => {
                if viewed.is_some() {
                    return Err(bad("the recently viewed are listed twice"));
                }
                let mut list = VecDeque::new();
                for &id in ids {
                    let id = number(id, "a recently viewed contact")?;
                    if list.contains(&id) {
                        return Err(bad("a contact is viewed twice"));
                    }
                    list.push_back(id);
                }
                viewed = Some(list);
                viewed_line = at;
            }
            _ => {
                return Err(bad(
                    "it is not a line this version reads, or has the wrong number of fields",
                ));
            }
        }
    }

    for (at, group) in memberships {
        if !store.groups.iter().any(|g| g.id == group) {
            return Err(format!(
                "line {at}: a contact is in a group the book does not have"
            ));
        }
    }
    let viewed = viewed.unwrap_or_default();
    if viewed.len() > MAX_RECENT {
        return Err(format!(
            "line {viewed_line}: more are listed as recently viewed than are kept"
        ));
    }
    if let Some(missing) = viewed
        .iter()
        .find(|id| !store.contacts.iter().any(|c| c.id == **id))
    {
        return Err(format!(
            "line {viewed_line}: contact {missing}, recently viewed, is not in the book"
        ));
    }
    store.recently_viewed = viewed;
    store.next_contact_id = next_id_after(store.contacts.iter().map(|c| c.id));
    store.next_group_id = next_id_after(store.groups.iter().map(|g| g.id));
    Ok(store)
}

/// The first number after `ids`, for the next thing made: past every one
/// read, so nothing made later takes the number of something kept.
fn next_id_after(ids: impl Iterator<Item = u64>) -> u64 {
    ids.max().map_or(1, |highest| highest.saturating_add(1))
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

        // The status strip is the last thing drawn and the first thing given
        // room: everything above stops at its top edge, so nothing can be
        // painted under it and then answer a press through it.
        //
        // It is placed before the header, and the header is what gives way
        // when the two do not both fit. Written the other way round -- the
        // status pushed down to clear a full-height header -- a window
        // thirty pixels tall put the status line at y=30 and drew it in the
        // eight pixels *below* the window's own bottom edge.
        let status_h = STRIP_HEIGHT.min(h);
        let status = Rect::new(0.0, (h - status_h).max(0.0), w, status_h);
        let header_h = HEADER_HEIGHT.min(status.y);
        let header = Rect::new(0.0, 0.0, side_w, header_h);
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
/// The most of a vCard file one open will read.
///
/// Reported when it bites: a vCard cut mid-entry is not the address book the
/// user chose, and `import_vcards` would simply stop at the truncation without
/// anything saying a name was lost.
pub const MAX_VCARD_BYTES: usize = 8 * 1024 * 1024;

/// Leads a message about a file operation that did not happen.
const FILE_FAILED_PREFIX: &str = "Could not";

/// The keys this program answers, raised by `F1`.
///
/// `?` is not a second way in: the search box and the contact fields both take
/// typed text, so a `?` has somewhere to go -- the `apps/spreadsheet` case in
/// design-decisions 863.
///
/// `S` and `O` appear twice with different meanings, and the rows say which:
/// unmodified they search and cycle the sort, and the handler's own comment
/// records that the file dialogs had to take `Ctrl` *because* those two were
/// already spoken for.
const SHORTCUTS: &[(&str, &str)] = &[
    ("N", "A new contact"),
    ("E", "Edit the selected one"),
    ("Delete", "Delete it"),
    ("S", "Search"),
    ("G", "Groups"),
    ("D", "Duplicates"),
    ("F", "Cycle the filter"),
    ("O", "Cycle the sort order"),
    ("Tab", "Move the keyboard on"),
    ("Enter", "Open, or confirm what you typed"),
    ("Backspace", "Rub out a letter"),
    ("Esc", "Back, or give the keyboard up"),
    ("Ctrl+S", "Save now -- the book is saved as you go"),
    ("Ctrl+E / Ctrl+O", "Export / import vCards"),
    ("F1", "This list"),
];

pub struct ContactsApp {
    /// The open or save picker. Holds the dialog, the saving flag and
    /// the routing eleven applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last open or save did, for the status line.
    pub last_file_action: Option<String>,
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
    /// The last stamp [`stamp`](Self::stamp) gave, so the next is later.
    ///
    /// It was `clock`, a counter from 2,000,000,000 standing in for the wall
    /// clock, and "recently added" had nothing to order by at all: no contact
    /// was ever given a time.
    last_stamp: u64,
    /// Whether changes are kept. Off in `new`, so no test can write the
    /// user's address book; `from_settings`, which `main` uses, turns it on,
    /// and a book that cannot be read turns it off again.
    persist: bool,
    /// The store's revision when the book was last written or read.
    kept_revision: u64,
    /// Why the book is not being kept, drawn for as long as it is true: it
    /// could not be read (and so is left exactly as it is), there is nowhere
    /// to keep it, or the last save failed.
    store_error: Option<String>,
    /// The question asked when the window is closed over a contact being
    /// edited, or while a save is failing.
    question: Option<unsaved::Question<Pending>>,
    /// Set when the question has been answered with leave, so the event loop
    /// can let the window go.
    quit: bool,

    // Window dimensions, remembered so a press that arrives between a resize
    // and the next frame is answered against the size the window really is.
    pub window_width: f32,
    pub window_height: f32,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut card is up.
    show_help: bool,
}

impl ContactsApp {
    pub fn new() -> Self {
        Self {
            show_help: false,
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::new(),
            last_file_action: None,
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
            last_stamp: 0,
            persist: false,
            kept_revision: 0,
            store_error: None,
            question: None,
            quit: false,

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
        f.push(fill(l.window, self.palette.base, 0.0));

        self.draw_sidebar(&mut f, &l);
        self.draw_alphabet(&mut f, &l);
        self.draw_panel(&mut f, &l);
        self.draw_status(&mut f, &l);

        // Last, so nothing paints over it. Keyed on the store being empty so
        // it retires itself at the first real contact.
        if self.store.contacts.is_empty() {
            let keeping = self.keeping_line();
            for (i, line) in [NO_CONTACTS, keeping.as_str()].iter().enumerate() {
                #[expect(clippy::cast_precision_loss, reason = "two lines; index is 0 or 1")]
                let y = l.window.y + 2.0 + i as f32 * 12.0;
                // A window can be two pixels across. `w - 16.0` goes negative
                // there, and a line that does not fit vertically must not be
                // drawn at all -- both caught by
                // `every_run_of_text_is_bounded_and_inside_the_window`, which
                // is the second render-invariant test in this sweep to reject
                // one of these banners. They read commands rather than pixels,
                // so a clip cannot hide an overrun from them.
                let avail = (l.window.w - 16.0).max(0.0);
                if avail <= 0.0 || y + 12.0 > l.window.y + l.window.h {
                    break;
                }
                f.push(RenderCommand::Text {
                    x: l.window.x + 8.0,
                    y,
                    text: (*line).to_string(),
                    color: if i == 0 {
                        self.palette.ink(self.palette.yellow)
                    } else {
                        self.palette.subtext0
                    },
                    font_size: if i == 0 { 11.0 } else { 9.0 },
                    font_weight: if i == 0 {
                        FontWeightHint::Bold
                    } else {
                        FontWeightHint::Regular
                    },
                    max_width: Some(avail),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
        // What the last open or save did. This is the one place the user's
        // address book leaves the process.
        if let Some(note) = &self.last_file_action {
            let avail = (l.window.w - 16.0).max(0.0);
            if avail > 0.0 {
                f.push(RenderCommand::Text {
                    x: l.window.x + 8.0,
                    y: l.window.y + 26.0,
                    text: note.clone(),
                    color: if note.starts_with(FILE_FAILED_PREFIX) || note.starts_with("INCOMPLETE")
                    {
                        self.palette.ink(self.palette.red)
                    } else {
                        self.palette.subtext0
                    },
                    font_size: 9.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(avail),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }

        // Last, so it is above everything -- the same order in which
        // `handle_event` gives it the keystroke.
        f.extend(self.picker.render(&self.palette, l.window.w, l.window.h));

        // Last, so it is over the file dialog too: the list is the one thing
        // on screen a reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (l.window.w, l.window.h),
                0.0,
                SHORTCUTS,
                "F1 closes this",
            );
        }
        f
    }

    /// The sidebar: title, count, `+`, search, the two strips and the list.
    fn draw_sidebar(&self, f: &mut Frame<Target>, l: &Layout) {
        let side = Rect::new(0.0, 0.0, l.list.w + l.alphabet.w, l.status.y);
        f.push(fill(side, self.palette.mantle, 0.0));
        f.push(fill(l.header, self.palette.surface0, 0.0));

        // The title and the count share the header, and both stop short of
        // the `+` rather than running under it.
        let title_w = (l.add_button.x - 20.0).max(0.0);
        let half = l.header.h / 2.0;
        put_text(
            f,
            Rect::new(12.0, l.header.y, title_w, half),
            "Contacts",
            18.0,
            self.palette.text,
            FontWeightHint::Bold,
        );
        put_text(
            f,
            Rect::new(12.0, l.header.y + half, title_w, l.header.h - half),
            &format!("{} contacts", self.store.contact_count()),
            11.0,
            self.palette.subtext0,
            FontWeightHint::Regular,
        );

        if !l.add_button.is_empty() {
            f.push(fill(l.add_button, self.palette.blue, 6.0));
            put_text(
                f,
                inset(l.add_button, l.add_button.w / 3.0),
                "+",
                18.0,
                self.palette.base,
                FontWeightHint::Bold,
            );
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
                color: self.palette.surface1,
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
            if focused {
                self.palette.surface1
            } else {
                self.palette.surface0
            },
            8.0,
        ));
        let inner = inset(l.search, 10.0);
        let placeholder = self.search_query.is_empty() && !focused;
        let shown = if placeholder {
            "Search contacts..."
        } else {
            &self.search_query
        };
        put_text(
            f,
            inner,
            shown,
            13.0,
            if placeholder {
                self.palette.overlay0
            } else {
                self.palette.text
            },
            FontWeightHint::Regular,
        );
        if focused {
            // A caret rather than a character appended to the text: a box
            // whose contents change when it is focused cannot be searched for
            // by the words it shows.
            let used = text::measure(&self.search_query, 13.0, FontWeightHint::Regular);
            let caret_x = (inner.x + used).min(inner.right() - 2.0);
            f.push(fill(
                Rect::new(caret_x, inner.y + 4.0, 2.0, (inner.h - 8.0).max(0.0)),
                self.palette.blue,
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
                if on {
                    self.palette.blue
                } else {
                    self.palette.surface0
                },
                4.0,
            ));
            put_text(
                f,
                inset(cell, 8.0),
                &label,
                11.0,
                if on {
                    self.palette.base
                } else {
                    self.palette.subtext0
                },
                FontWeightHint::Regular,
            );
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
            put_text(
                f,
                inset(Rect::new(l.list.x, l.list.y, l.list.w, 24.0), 12.0),
                "No contacts match",
                12.0,
                self.palette.overlay0,
                FontWeightHint::Regular,
            );
            f.unclip();
            return;
        }

        // The cursor is advanced for every row, but a row is only *drawn*
        // while the cursor is somewhere the clip can show it. A clip makes
        // ink outside it invisible; it does not make it free. A list of six
        // hundred contacts scrolled to the end would otherwise emit six
        // hundred rows of commands and hit boxes above the window for the
        // renderer and the hit test to walk past, every frame -- and the
        // hit boxes are the worse half, because `Frame::hit` drops a box the
        // clip hides but only after it has been built and measured.
        let mut cy = l.list.y - self.scroll_offset;
        let mut current_letter: Option<char> = None;
        for contact in &contacts {
            if cy >= l.list.bottom() {
                break;
            }
            let letter = contact.first_letter();
            if current_letter != Some(letter) && self.sort_order == SortOrder::Name {
                let row = Rect::new(l.list.x, cy, l.list.w, LETTER_DIVIDER_HEIGHT);
                if row.bottom() > l.list.y && row.y < l.list.bottom() {
                    f.push(fill(row, Color::rgba(49, 50, 68, 180), 0.0));
                    put_text(
                        f,
                        inset(row, 12.0),
                        &letter.to_string(),
                        12.0,
                        self.palette.blue,
                        FontWeightHint::Bold,
                    );
                }
                cy += LETTER_DIVIDER_HEIGHT;
                current_letter = Some(letter);
                if cy >= l.list.bottom() {
                    break;
                }
            }
            if cy + CONTACT_ROW_HEIGHT > l.list.y {
                self.draw_contact_row(f, l, contact, cy);
            }
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
                self.palette.surface0
            } else {
                Color::TRANSPARENT
            },
            0.0,
        ));

        let avatar = Rect::new(row.x + 10.0, row.y + 6.0, 40.0, 40.0);
        f.push(fill(
            avatar,
            if contact.favorite {
                self.palette.yellow
            } else {
                self.palette.surface1
            },
            20.0,
        ));
        put_text(
            f,
            inset(avatar, 10.0),
            &contact.initials(),
            14.0,
            if contact.favorite {
                self.palette.base
            } else {
                self.palette.text
            },
            FontWeightHint::Bold,
        );

        // The star is drawn first so the name's box can stop short of it. The
        // name used to be given `SIDEBAR_WIDTH - 80` whatever the window was,
        // which is a column width for a sidebar that is no longer that wide.
        let star_w = if contact.favorite { 18.0 } else { 0.0 };
        let text_x = avatar.right() + 8.0;
        let text_w = (row.right() - star_w - 6.0 - text_x).max(0.0);
        if contact.favorite {
            put_text(
                f,
                Rect::new(row.right() - star_w - 4.0, row.y, star_w, row.h),
                "*",
                16.0,
                self.palette.yellow,
                FontWeightHint::Bold,
            );
        }

        let name_h = row.h * 0.55;
        put_text(
            f,
            Rect::new(text_x, row.y + 4.0, text_w, name_h - 4.0),
            &contact.computed_display_name(),
            14.0,
            self.palette.text,
            FontWeightHint::Regular,
        );
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
            put_text(
                f,
                Rect::new(text_x, row.y + name_h, text_w, row.h - name_h - 4.0),
                &subtitle,
                11.0,
                self.palette.subtext0,
                FontWeightHint::Regular,
            );
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
            put_text(
                f,
                inset_x(cell, 6.0),
                &letter.to_string(),
                9.0,
                if on {
                    self.palette.blue
                } else {
                    self.palette.subtext0
                },
                if on {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
            );
            f.hit(Target::Letter(letter), cell);
        }
    }

    /// The detail panel on the right, whichever view is showing.
    fn draw_panel(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.panel.is_empty() {
            return;
        }
        f.push(fill(l.panel, self.palette.base, 0.0));
        f.clip(l.panel);
        match &self.view {
            DetailView::Empty => Self::draw_empty_state(f, &self.palette, l),
            DetailView::ViewContact(id) => {
                if let Some(contact) = self.store.get_contact(*id) {
                    self.draw_contact_detail(f, l, contact);
                } else {
                    Self::draw_empty_state(f, &self.palette, l);
                }
            }
            DetailView::EditContact(_) | DetailView::NewContact => self.draw_edit_form(f, l),
            DetailView::Duplicates => self.draw_duplicates(f, l),
            DetailView::Groups => self.draw_groups(f, l),
        }
        f.unclip();
    }

    /// What the panel says when nothing is selected.
    fn draw_empty_state(f: &mut Frame<Target>, pal: &Palette, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let mid = body.y + body.h / 2.0;
        put_text(
            f,
            Rect::new(body.x, mid - 30.0, body.w, 24.0),
            "Select a contact",
            18.0,
            pal.subtext0,
            FontWeightHint::Regular,
        );
        put_text(
            f,
            Rect::new(body.x, mid, body.w, 18.0),
            "or press + to add a new one",
            13.0,
            pal.overlay0,
            FontWeightHint::Regular,
        );
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
            if contact.favorite {
                self.palette.yellow
            } else {
                self.palette.blue
            },
            avatar.w / 2.0,
        ));
        put_text(
            f,
            inset(avatar, avatar.w / 3.0),
            &contact.initials(),
            28.0,
            self.palette.base,
            FontWeightHint::Bold,
        );
        y += AVATAR_SIZE + 12.0;

        put_text(
            f,
            Rect::new(body.x, y, body.w, 26.0),
            &contact.computed_display_name(),
            22.0,
            self.palette.text,
            FontWeightHint::Bold,
        );
        y += 30.0;

        let company_line = match (contact.job_title.is_empty(), contact.company.is_empty()) {
            (false, false) => format!("{} at {}", contact.job_title, contact.company),
            (true, false) => contact.company.clone(),
            (false, true) => contact.job_title.clone(),
            (true, true) => String::new(),
        };
        for (line, size, color) in [
            (company_line, 14.0, self.palette.subtext0),
            (contact.department.clone(), 12.0, self.palette.overlay0),
            (
                if contact.nickname.is_empty() {
                    String::new()
                } else {
                    format!("\"{}\"", contact.nickname)
                },
                12.0,
                self.palette.lavender,
            ),
        ] {
            if line.is_empty() {
                continue;
            }
            put_text(
                f,
                Rect::new(body.x, y, body.w, size + 4.0),
                &line,
                size,
                color,
                FontWeightHint::Regular,
            );
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
                QuickAction::Call => self.palette.green,
                QuickAction::Email => self.palette.blue,
                QuickAction::Map => self.palette.peach,
            };
            f.push(fill(r, color, 6.0));
            put_text(
                f,
                inset(r, 8.0),
                action.label(),
                13.0,
                self.palette.base,
                FontWeightHint::Bold,
            );
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
            (
                Target::EditContact,
                String::from("Edit"),
                self.palette.blue,
                self.palette.base,
            ),
            (
                Target::ToggleFavorite,
                String::from(if contact.favorite { "Unstar" } else { "Star" }),
                if contact.favorite {
                    self.palette.yellow
                } else {
                    self.palette.surface1
                },
                if contact.favorite {
                    self.palette.base
                } else {
                    self.palette.text
                },
            ),
            (
                Target::DeleteContact,
                String::from("Delete"),
                self.palette.red,
                self.palette.base,
            ),
        ];
        for (i, (target, label, bg, fg)) in buttons.into_iter().enumerate() {
            let r = Rect::new(body.x + (i as f32) * (three_w + gap), btn_y, three_w, btn_h);
            if r.is_empty() {
                continue;
            }
            f.push(fill(r, bg, 6.0));
            put_text(f, inset(r, 8.0), &label, 13.0, fg, FontWeightHint::Bold);
            f.hit(target, r);
        }
    }

    /// Phones, emails, addresses, social, birthday, notes and group chips.
    ///
    /// The column is laid out into `lines` first and painted afterwards. That
    /// is not tidiness: the panel does not scroll, so a contact with more
    /// fields than the box is tall genuinely runs out of room, and the rule
    /// "stop at the bottom edge" has to hold for every field type at once.
    /// Written inline it was seven separate rules, and it was absent from all
    /// seven -- `put_text` declines to emit a run the clip cannot show and
    /// `Frame::hit` trims a hit box to nothing, so the overflow was invisible
    /// and unclickable and looked handled, but a *fill* is pushed exactly as
    /// asked, and at 640x480 the group chips were painting over the Edit /
    /// Star / Delete buttons drawn below the box.
    fn draw_contact_fields(&self, f: &mut Frame<Target>, area: Rect, contact: &Contact) {
        /// One laid-out line of the fields column, before it is given a `y`.
        enum FieldLine {
            /// A section title: small, bold, dim, flush with the left edge.
            Heading(&'static str),
            /// An indented line of detail. `height` is the box the run is
            /// inked in; `advance` is what the cursor moves, which is larger
            /// so consecutive lines are not touching.
            Detail {
                text: String,
                color: Color,
                size: f32,
                height: f32,
                advance: f32,
            },
            /// Blank space between sections.
            Gap(f32),
            /// The group chips, which wrap among themselves: `(id, name,
            /// colour, width)`.
            Chips(Vec<(u64, String, Color, f32)>),
        }

        let detail_w = (area.w - 8.0).max(0.0);
        let mut lines: Vec<FieldLine> = Vec::new();

        let detail = |lines: &mut Vec<FieldLine>, text: String, color: Color, advance: f32| {
            lines.push(FieldLine::Detail {
                text,
                color,
                size: 13.0,
                height: 18.0,
                advance,
            });
        };

        if !contact.phones.is_empty() {
            lines.push(FieldLine::Heading("Phone numbers"));
            for phone in &contact.phones {
                let primary = if phone.primary { " (primary)" } else { "" };
                let s = format!("{}: {}{primary}", phone.phone_type.label(), phone.number);
                detail(&mut lines, s, self.palette.text, 20.0);
            }
            lines.push(FieldLine::Gap(6.0));
        }
        if !contact.emails.is_empty() {
            lines.push(FieldLine::Heading("Email addresses"));
            for email in &contact.emails {
                let primary = if email.primary { " (primary)" } else { "" };
                let s = format!("{}: {}{primary}", email.email_type.label(), email.email);
                detail(&mut lines, s, self.palette.text, 20.0);
            }
            lines.push(FieldLine::Gap(6.0));
        }
        if !contact.addresses.is_empty() {
            lines.push(FieldLine::Heading("Addresses"));
            for addr in &contact.addresses {
                let s = format!("{}: {}", addr.address_type.label(), addr.display_line());
                detail(&mut lines, s, self.palette.text, 20.0);
            }
            lines.push(FieldLine::Gap(6.0));
        }
        if !contact.social_accounts.is_empty() {
            lines.push(FieldLine::Heading("Social accounts"));
            for social in &contact.social_accounts {
                let s = format!("{}: {}", social.platform.label(), social.handle);
                detail(&mut lines, s, self.palette.lavender, 20.0);
            }
            lines.push(FieldLine::Gap(6.0));
        }
        if let Some(ref bday) = contact.birthday {
            lines.push(FieldLine::Heading("Birthday"));
            detail(&mut lines, bday.format_display(), self.palette.text, 24.0);
        }
        if !contact.notes.is_empty() {
            lines.push(FieldLine::Heading("Notes"));
            // `RenderCommand::Text` clips at `max_width` instead of wrapping,
            // so the notes used to show only their first line's worth of
            // characters -- silently, with nothing to say the rest was there.
            let notes_w = detail_w.max(1.0);
            for line in text::wrap(
                &contact.notes,
                notes_w,
                NOTES_FONT_SIZE,
                FontWeightHint::Regular,
            ) {
                lines.push(FieldLine::Detail {
                    text: line,
                    color: self.palette.subtext0,
                    size: NOTES_FONT_SIZE,
                    height: NOTES_LINE_HEIGHT,
                    advance: NOTES_LINE_HEIGHT,
                });
            }
            lines.push(FieldLine::Gap(6.0));
        }
        if !contact.groups.is_empty() {
            lines.push(FieldLine::Heading("Groups"));
            let mut chips: Vec<(u64, String, Color, f32)> = Vec::new();
            for &gid in &contact.groups {
                let Some(group) = self.store.get_group(gid) else {
                    continue;
                };
                let w = text::padded_width(&group.name, 8.0, 11.0, FontWeightHint::Bold)
                    .min(area.w.max(0.0));
                chips.push((gid, group.name.clone(), group.color, w));
            }
            if !chips.is_empty() {
                lines.push(FieldLine::Chips(chips));
            }
        }

        let mut y = area.y;
        for line in lines {
            // The one place the bottom edge is honoured, for every field type.
            if y >= area.bottom() {
                return;
            }
            match line {
                FieldLine::Heading(title) => {
                    put_text(
                        f,
                        Rect::new(area.x, y, area.w, 14.0),
                        title,
                        11.0,
                        self.palette.overlay0,
                        FontWeightHint::Bold,
                    );
                    y += 16.0;
                }
                FieldLine::Detail {
                    text,
                    color,
                    size,
                    height,
                    advance,
                } => {
                    put_text(
                        f,
                        Rect::new(area.x + 8.0, y, detail_w, height),
                        &text,
                        size,
                        color,
                        FontWeightHint::Regular,
                    );
                    y += advance;
                }
                FieldLine::Gap(h) => y += h,
                FieldLine::Chips(chips) => {
                    let mut chip_x = area.x;
                    for (gid, name, color, chip_w) in chips {
                        if chip_x + chip_w > area.right() {
                            // A chip that would run off the right edge starts
                            // a new line instead of being drawn outside its
                            // own box -- and a new line can itself run off the
                            // bottom, which is why the check is repeated here.
                            chip_x = area.x;
                            y += GROUP_CHIP_HEIGHT + 4.0;
                        }
                        if y + GROUP_CHIP_HEIGHT > area.bottom() {
                            return;
                        }
                        let chip = Rect::new(chip_x, y, chip_w, GROUP_CHIP_HEIGHT);
                        f.push(fill(chip, color, GROUP_CHIP_HEIGHT / 2.0));
                        put_text(
                            f,
                            inset(chip, 8.0),
                            &name,
                            11.0,
                            self.palette.base,
                            FontWeightHint::Bold,
                        );
                        f.hit(Target::Group(gid), chip);
                        chip_x += chip_w + 8.0;
                    }
                    y += GROUP_CHIP_HEIGHT + 4.0;
                }
            }
        }
    }

    /// The add/edit form.
    fn draw_edit_form(&self, f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let btn_h = 36.0_f32.min(body.h);
        let btn_y = (body.bottom() - btn_h).max(body.y);
        let mut y = body.y;

        put_text(
            f,
            Rect::new(body.x, y, body.w, 24.0),
            if matches!(self.view, DetailView::NewContact) {
                "New Contact"
            } else {
                "Edit Contact"
            },
            20.0,
            self.palette.text,
            FontWeightHint::Bold,
        );
        y += 32.0;

        let form = Rect::new(body.x, y, body.w, (btn_y - 10.0 - y).max(0.0));
        if !form.is_empty() {
            f.clip(form);
            let mut fy = form.y - self.scroll_offset;
            for field in FormField::ALL {
                let tall = field == FormField::Notes;
                let h = if tall { 72.0 } else { FIELD_HEIGHT };
                // The form scrolls, so a field can sit wholly above the top of
                // the box or wholly below its bottom. A clip is not enough on
                // its own: `put_text` declines to emit a run the clip cannot
                // show and `Frame::hit` trims a hit box to nothing, but a fill
                // is pushed exactly as asked -- so the field's own box was
                // being painted over the Save and Cancel buttons drawn under
                // the form. Rows outside the box are skipped, not clipped.
                // The test is the *box*, not the label above it: a label with
                // no box under it is not a field, so a row whose box has run
                // off the bottom is skipped whole even when its label would
                // still have fitted.
                let box_top = fy + 14.0;
                if box_top >= form.bottom() {
                    break;
                }
                if box_top + h <= form.y {
                    fy += 14.0 + h + 6.0;
                    continue;
                }
                put_text(
                    f,
                    Rect::new(form.x, fy, form.w, 12.0),
                    field.label(),
                    11.0,
                    self.palette.overlay0,
                    FontWeightHint::Regular,
                );
                fy += 14.0;
                let box_r = Rect::new(form.x, fy, form.w, h);
                let focused = self.focus == Focus::Field(field);
                f.push(fill(
                    box_r,
                    if focused {
                        self.palette.surface1
                    } else {
                        self.palette.surface0
                    },
                    6.0,
                ));
                let value = self.field_value(field);
                let inner = inset(box_r, 10.0);
                let inner = Rect::new(inner.x, inner.y, inner.w, 20.0_f32.min(inner.h));
                if value.is_empty() {
                    put_text(
                        f,
                        inner,
                        field.label(),
                        13.0,
                        self.palette.overlay0,
                        FontWeightHint::Regular,
                    );
                } else {
                    put_text(
                        f,
                        inner,
                        value,
                        13.0,
                        self.palette.text,
                        FontWeightHint::Regular,
                    );
                }
                if focused {
                    let used = text::measure(value, 13.0, FontWeightHint::Regular);
                    let caret_x = (inner.x + used).min(inner.right() - 2.0);
                    f.push(fill(
                        Rect::new(caret_x, inner.y + 2.0, 2.0, (inner.h - 4.0).max(0.0)),
                        self.palette.blue,
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
            (Target::Save, "Save", self.palette.green, self.palette.base),
            (
                Target::Cancel,
                "Cancel",
                self.palette.surface1,
                self.palette.text,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(body.x + (i as f32) * (half + gap), btn_y, half, btn_h);
            if r.is_empty() {
                continue;
            }
            f.push(fill(r, bg, 6.0));
            put_text(f, inset(r, 8.0), label, 13.0, fg, FontWeightHint::Bold);
            f.hit(target, r);
        }
    }

    /// The duplicate-detection panel.
    fn draw_duplicates(&self, f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let mut y = body.y;
        put_text(
            f,
            Rect::new(body.x, y, body.w, 24.0),
            "Duplicate Detection",
            20.0,
            self.palette.text,
            FontWeightHint::Bold,
        );
        y += 32.0;

        let duplicates = self.store.find_duplicates();
        if duplicates.is_empty() {
            put_text(
                f,
                Rect::new(body.x, y, body.w, 20.0),
                "No duplicates found.",
                14.0,
                self.palette.green,
                FontWeightHint::Regular,
            );
            return;
        }
        put_text(
            f,
            Rect::new(body.x, y, body.w, 18.0),
            &format!("Found {} potential duplicate(s):", duplicates.len()),
            13.0,
            self.palette.peach,
            FontWeightHint::Regular,
        );
        y += 24.0;

        let merge_w = 64.0_f32.min(body.w / 3.0);
        for dup in &duplicates {
            let card = Rect::new(body.x, y, body.w, 60.0);
            f.push(fill(card, self.palette.surface0, 8.0));
            let name_a = self
                .store
                .get_contact(dup.contact_a_id)
                .map_or_else(|| String::from("?"), Contact::computed_display_name);
            let name_b = self
                .store
                .get_contact(dup.contact_b_id)
                .map_or_else(|| String::from("?"), Contact::computed_display_name);
            let text_w = (card.w - merge_w - 24.0).max(0.0);
            put_text(
                f,
                Rect::new(card.x + 12.0, card.y + 8.0, text_w, 18.0),
                &format!("{name_a}  <->  {name_b}"),
                13.0,
                self.palette.text,
                FontWeightHint::Bold,
            );
            put_text(
                f,
                Rect::new(card.x + 12.0, card.y + 30.0, text_w, 16.0),
                &format!(
                    "{} (confidence: {:.0}%)",
                    dup.reason.label(),
                    dup.confidence * 100.0
                ),
                11.0,
                self.palette.subtext0,
                FontWeightHint::Regular,
            );
            let merge = Rect::new(card.right() - merge_w - 8.0, card.y + 16.0, merge_w, 28.0);
            if !merge.is_empty() {
                f.push(fill(merge, self.palette.blue, 4.0));
                put_text(
                    f,
                    inset(merge, 6.0),
                    "Merge",
                    11.0,
                    self.palette.base,
                    FontWeightHint::Bold,
                );
                f.hit(Target::Merge(dup.contact_a_id, dup.contact_b_id), merge);
            }
            y += 72.0;
        }
    }

    /// The groups panel, whose rows filter the list.
    fn draw_groups(&self, f: &mut Frame<Target>, l: &Layout) {
        let body = inset(l.panel, DETAIL_PADDING);
        let mut y = body.y;
        put_text(
            f,
            Rect::new(body.x, y, body.w, 24.0),
            "Groups",
            20.0,
            self.palette.text,
            FontWeightHint::Bold,
        );
        y += 32.0;

        let stats = self.store.group_stats();
        if stats.is_empty() {
            put_text(
                f,
                Rect::new(body.x, y, body.w, 20.0),
                "No groups yet. Create one to organize contacts.",
                14.0,
                self.palette.subtext0,
                FontWeightHint::Regular,
            );
            return;
        }
        for (gid, name, count) in &stats {
            let row = Rect::new(body.x, y, body.w, 48.0);
            f.push(fill(row, self.palette.surface0, 8.0));
            let dot = Rect::new(row.x + 12.0, row.y + 16.0, 16.0, 16.0);
            f.push(fill(
                dot,
                self.store
                    .get_group(*gid)
                    .map_or(self.palette.blue, |g| g.color),
                8.0,
            ));
            let text_x = dot.right() + 8.0;
            let text_w = (row.right() - 12.0 - text_x).max(0.0);
            put_text(
                f,
                Rect::new(text_x, row.y + 8.0, text_w, 18.0),
                name,
                14.0,
                self.palette.text,
                FontWeightHint::Bold,
            );
            put_text(
                f,
                Rect::new(text_x, row.y + 26.0, text_w, 16.0),
                &format!("{count} contact(s)"),
                11.0,
                self.palette.subtext0,
                FontWeightHint::Regular,
            );
            f.hit(Target::Group(*gid), row);
            y += 56.0;
        }
    }

    /// The strip along the bottom, drawn last so nothing can cover it.
    fn draw_status(&self, f: &mut Frame<Target>, l: &Layout) {
        if l.status.is_empty() {
            return;
        }
        f.push(fill(l.status, self.palette.crust, 0.0));
        // Why the book is not being kept, in place of the last action, for as
        // long as it is true: it is the one thing that decides whether closing
        // the window loses anyone.
        let (said, colour) = match &self.store_error {
            Some(error) => (error.as_str(), self.palette.ink(self.palette.red)),
            None => (self.status.as_str(), self.palette.subtext0),
        };
        put_text(
            f,
            inset(l.status, 8.0),
            said,
            11.0,
            colour,
            FontWeightHint::Regular,
        );
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
    /// Groups and contacts, for tests.
    ///
    /// `#[cfg(test)]` since 2026-09-15. `main` called it, so the window opened
    /// on Alice Anderson of Acme Corp, her mobile and work numbers, two email
    /// addresses, a birthday, a home address and the note "Met at the Rust
    /// conference 2024" -- filed under Family, Work and Friends, in the place
    /// the user's own address book goes.
    ///
    /// **Note how carefully it was built.** The numbers are `+1-555-01xx`,
    /// which is the reserved fictional range, and the domain is `example.com`,
    /// which is reserved for documentation. Whoever wrote this made sure
    /// nobody could accidentally ring or mail these people. That is genuinely
    /// thoughtful, and it is the third fixture in this sweep where the
    /// *harmful* part was handled with care and the *claim* was not examined
    /// at all -- `apps/reminders` made its due dates relative so "overdue"
    /// would stay true, and `apps/habits` spread its check-ins to a plausible
    /// 70%.
    ///
    /// **A careful fixture is harder to notice than a careless one.** Stale
    /// dates, a routable phone number or a flat 100% streak would each have
    /// been questioned sooner than the polished version was.
    #[cfg(test)]
    pub fn load_sample_data(&mut self) {
        // Groups
        let g1 = self
            .store
            .add_group(ContactGroup::new(0, "Family").with_color(self.palette.green));
        let g2 = self
            .store
            .add_group(ContactGroup::new(0, "Work").with_color(self.palette.blue));
        let g3 = self
            .store
            .add_group(ContactGroup::new(0, "Friends").with_color(self.palette.peach));

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
    let ink = ink_box(r, size);
    RenderCommand::Text {
        x: ink.x,
        y: ink.y,
        text: s.to_string(),
        font_size: ink.h,
        color,
        font_weight: weight,
        max_width: Some(ink.w),
        overflow: TextOverflow::Ellipsis,
    }
}

/// The box a run of text actually inks in, given the box it was asked to sit
/// in and the size it was asked for.
///
/// This is a function rather than four lines inside [`text_in`] because
/// [`put_text`] needs the same answer to decide whether the run is worth
/// drawing at all, and a box is not its ink: a 36-tall row whose bottom edge
/// is just past the clip is partly visible, while the 16-tall run centred in
/// it is entirely past it. Asking about the row rather than the run is how a
/// star came to be drawn four pixels below everything that could show it.
fn ink_box(r: Rect, size: f32) -> Rect {
    let size = size.min(r.h);
    Rect::new(r.x, r.y + (r.h - size) / 2.0, r.w, size)
}

/// Draw a run of text in a box, unless the box has no room to read it in.
///
/// Every text run in this program goes through here. A box with no width
/// bounds its run to nothing and a box with no height clamps its font to
/// nothing: either way the run is invisible, so emitting it puts a command
/// in the frame that draws no pixel and reads, to anything inspecting the
/// picture, as a label that is present. A window shrunk to forty pixels
/// across should be an empty window, not one that claims to be showing a
/// heading nobody can see.
///
/// The clip in force is asked for the same reason. A cursor that runs down a
/// panel drawing section after section does not stop at the bottom of the
/// panel by itself; the clip hides what it draws past that point, but hiding
/// is not the same as not drawing, and a picture that claims to have drawn a
/// "Social" heading three hundred pixels below the panel is a picture that
/// cannot be asked what it is showing.
fn put_text(
    f: &mut Frame<Target>,
    r: Rect,
    s: &str,
    size: f32,
    color: Color,
    weight: FontWeightHint,
) {
    let ink = ink_box(r, size);
    if ink.is_empty() || s.is_empty() || !f.is_visible(ink) {
        return;
    }
    f.push(text_in(r, s, size, color, weight));
}

// ============================================================================
// What a press and a keystroke do
// ============================================================================

impl ContactsApp {
    /// Route an event, then keep whatever it changed.
    ///
    /// The book is written here, once per event, whenever the store's
    /// revision has moved -- not by each place that changes it.
    fn handle_event(&mut self, event: &Event, size: (f32, f32)) {
        self.route_event(event, size);
        self.keep();
    }

    /// Route an event to whatever the drawing pass put under it.
    fn route_event(&mut self, event: &Event, size: (f32, f32)) {
        // The close question takes every key and click while it is up: a
        // keystroke that reached the form under it would be a change made
        // while being asked about the changes.
        if let Some(question) = self.question.as_mut()
            && matches!(event, Event::Key(_) | Event::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                self.question = None;
                self.answer(choice);
            }
            return;
        }
        // The picker takes the event first while it is up, or a keystroke
        // meant for a filename lands in the search box behind it.
        match self.picker.handle(event, size.0, size.1) {
            Picked::Chose(path) => {
                let saving = self.picker.is_saving();
                self.last_file_action = Some(if saving {
                    self.write_vcards(&path)
                } else {
                    self.read_vcards(&path)
                });
                return;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return,
            Picked::Ignored => {}
        }
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
                // of that order. A stamp is always later than the last, so a
                // second call lands after the first rather than tying with it.
                let now = self.stamp();
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

    /// `existing` with the form written onto it: the fields the form shows,
    /// and nothing else.
    ///
    /// The form shows the names, the work fields, the notes, the birthday and
    /// the *first* phone number, email address and postal address. Saving used
    /// to build a new contact from the form and copy four things across --
    /// groups, the star, when it was added, when it was last reached -- so
    /// every other number, address and account, a display name imported from
    /// a vCard, and the photo were dropped by the first edit, with the comment
    /// above it saying they must not be.
    ///
    /// A field emptied in the form removes what it showed: an emptied phone
    /// field is the first number deleted, and the next one moves up. The
    /// display name follows the names unless it was one of its own.
    fn form_applied_to(&self, existing: &Contact) -> Contact {
        let mut c = existing.clone();
        let was_default = existing.display_name
            == default_display_name(&existing.first_name, &existing.last_name);
        c.first_name.clone_from(&self.edit_first_name);
        c.last_name.clone_from(&self.edit_last_name);
        if was_default {
            c.display_name = default_display_name(&c.first_name, &c.last_name);
        }
        c.company.clone_from(&self.edit_company);
        c.job_title.clone_from(&self.edit_job_title);
        c.department.clone_from(&self.edit_department);
        c.nickname.clone_from(&self.edit_nickname);
        c.notes.clone_from(&self.edit_notes);
        c.birthday = SimpleDate::parse(&self.edit_birthday);

        match (self.edit_phone.is_empty(), c.phones.first_mut()) {
            (true, Some(_)) => {
                c.phones.remove(0);
            }
            (false, Some(phone)) => {
                phone.number.clone_from(&self.edit_phone);
                phone.phone_type = self.edit_phone_type;
            }
            (false, None) => c
                .phones
                .push(PhoneNumber::new(&self.edit_phone, self.edit_phone_type).with_primary(true)),
            (true, None) => {}
        }
        match (self.edit_email.is_empty(), c.emails.first_mut()) {
            (true, Some(_)) => {
                c.emails.remove(0);
            }
            (false, Some(email)) => {
                email.email.clone_from(&self.edit_email);
                email.email_type = self.edit_email_type;
            }
            (false, None) => c
                .emails
                .push(EmailAddress::new(&self.edit_email, self.edit_email_type).with_primary(true)),
            (true, None) => {}
        }
        let address_typed = [
            &self.edit_street,
            &self.edit_city,
            &self.edit_state,
            &self.edit_zip,
            &self.edit_country,
        ]
        .iter()
        .any(|f| !f.is_empty());
        match (address_typed, c.addresses.first_mut()) {
            (false, Some(_)) => {
                c.addresses.remove(0);
            }
            (true, first) => {
                let mut fresh = PostalAddress::new(self.edit_address_type);
                let address = match first {
                    Some(address) => address,
                    None => &mut fresh,
                };
                address.street.clone_from(&self.edit_street);
                address.city.clone_from(&self.edit_city);
                address.state.clone_from(&self.edit_state);
                address.zip.clone_from(&self.edit_zip);
                address.country.clone_from(&self.edit_country);
                address.address_type = self.edit_address_type;
                if c.addresses.is_empty() {
                    c.addresses.push(fresh);
                }
            }
            (false, None) => {}
        }
        c
    }

    /// Whether saving the form would change anything: for a new contact,
    /// whether anything has been typed; for one being edited, whether the
    /// form now says something the contact does not.
    #[must_use]
    pub fn form_has_changes(&self) -> bool {
        match self.view {
            DetailView::NewContact => self.build_contact_from_form() != Contact::new(0, "", ""),
            DetailView::EditContact(id) => self
                .store
                .get_contact(id)
                .is_some_and(|existing| self.form_applied_to(existing) != *existing),
            _ => false,
        }
    }

    /// Write the form back, either onto the contact being edited or as a new
    /// one.
    fn save_form(&mut self) {
        match self.view {
            DetailView::EditContact(id) => {
                let Some(existing) = self.store.get_contact(id).cloned() else {
                    return;
                };
                let mut updated = self.form_applied_to(&existing);
                // The same contact again is no change, and not a new time.
                if updated != existing {
                    updated.updated_at = self.stamp();
                    self.store.update_contact(updated);
                }
                self.view = DetailView::ViewContact(id);
                self.status = String::from("Saved");
            }
            DetailView::NewContact => {
                let mut built = self.build_contact_from_form();
                let now = self.stamp();
                built.created_at = now;
                built.updated_at = now;
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
            // The panel stays on the duplicates list. Merge is only offered
            // from that list, and a run of duplicates is normally merged one
            // after another -- jumping to the merged contact would send the
            // user back through the sidebar for every pair.
            //
            // The status names the person rather than the new id. A merge
            // deletes both halves and writes a third contact under an id the
            // user has never seen, so "Merged into #7" reports a number that
            // means nothing to anyone reading it.
            let name = self
                .store
                .get_contact(kept)
                .map_or_else(|| String::from("contact"), Contact::computed_display_name);
            self.status = format!("Merged {name}");
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

    /// Put the open or save picker up.
    ///
    /// `export_vcards` and `import_vcards` were written, tested, and
    /// unreachable: full vCard, with CRLF joining per the spec and
    /// `BEGIN:VCARD` block parsing. The format was the hard part and it was
    /// already finished.
    pub fn open_file_dialog(&mut self, saving: bool) {
        if saving {
            self.picker.open_to_write("contacts.vcf");
        } else {
            self.picker.open_to_read();
        }
    }

    /// Write every contact to `path` as vCard.
    ///
    /// Through `safeio::write_str_atomically`: `fs::write` truncates before
    /// writing, and an exported address book may be the only copy of numbers
    /// the user cannot reconstruct.
    ///
    /// Refuses on an empty book rather than writing an empty file. A zero-byte
    /// `contacts.vcf` is indistinguishable from a failed export afterwards,
    /// and the user would have no way to tell which they had.
    pub fn write_vcards(&mut self, path: &std::path::Path) -> String {
        if self.store.contacts.is_empty() {
            return String::from("No contacts to write");
        }
        let text = self.store.export_all();
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!(
                "Wrote {} contact(s) to {}",
                self.store.contacts.len(),
                path.display()
            ),
            Err(err) => format!("{FILE_FAILED_PREFIX} write {}: {err}", path.display()),
        }
    }

    /// Read `path` and add every vCard in it to the book.
    ///
    /// Adds rather than replaces: the user asked to import an address book,
    /// not to discard the one they have. Duplicates are the duplicate finder's
    /// problem and this app already has one.
    ///
    /// Bounded, and it says so when it cuts, because `import_vcards` stops at
    /// a truncation without complaining -- a cut file simply yields fewer
    /// names, which is the failure mode nobody notices.
    ///
    /// Read through `safeio::read_to_string_capped`, which stops at the cap:
    /// this read the whole file into memory first and cut it afterwards, so a
    /// multi-gigabyte file was read whole to import the first 8 MiB of it.
    pub fn read_vcards(&mut self, path: &std::path::Path) -> String {
        let read = match safeio::read_to_string_capped(path, MAX_VCARD_BYTES) {
            Ok(read) => read,
            Err(err) => return format!("{FILE_FAILED_PREFIX} read {}: {err}", path.display()),
        };
        let now = self.stamp();
        let added = self.store.import_vcards(&read.text, now);
        if read.truncated {
            format!(
                "INCOMPLETE: {added} contact(s) from the first {MAX_VCARD_BYTES} bytes of {}, which is larger",
                path.display()
            )
        } else {
            format!("Added {added} contact(s) from {}", path.display())
        }
    }

    /// A keystroke: text into whatever has the keyboard, otherwise a
    /// shortcut.
    fn handle_key(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }

        // At the very top, and that position is load-bearing. The `match`
        // below claims `Escape`, `Enter`, `Tab` and `Backspace` before any
        // modifier is looked at, so a card check placed lower -- where the
        // `Ctrl` pair is handled -- never sees the keys that dismiss it. The
        // first version of this went there and `Escape` could not close the
        // card.
        if event.key == Key::F1 {
            self.show_help = !self.show_help;
            return;
        }
        if self.show_help {
            // Modal. Letting keys through would mean deleting a contact the
            // reader cannot see.
            if matches!(event.key, Key::Escape | Key::Enter | Key::F1) {
                self.show_help = false;
            }
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

        // The two that take a modifier come first: a guard on an or-pattern
        // applies to the whole of it, and `S` and `O` are already taken
        // unmodified by Search and Cycle sort.
        if event.modifiers.ctrl {
            match event.key {
                // The book is kept as it changes, so there is nothing for
                // Ctrl+S to save -- but it is the key people press to make
                // sure, and the one to press to try again after a save failed.
                // It says where the book is, or why it is not being kept.
                Key::S => {
                    self.keep();
                    self.status = self.keeping_line();
                }
                // Export was Ctrl+S until the book was kept.
                Key::E => self.open_file_dialog(true),
                Key::O => self.open_file_dialog(false),
                _ => {}
            }
            return;
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
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

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
            Event::CloseRequested => {
                if self.request_close() {
                    Response::Exit
                } else {
                    Response::KeepOpen
                }
            }
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
                if self.quit {
                    Response::Exit
                } else {
                    Response::Redraw
                }
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        self.window_width = width;
        self.window_height = height;
        let mut tree = self.frame(width, height).into_tree();
        // Over everything, the picker included: they are never up together.
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            question.render(&palette, width, height, &mut tree);
        }
        tree
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
    // drawing code and showed nobody the result. Then it opened empty, every
    // time; it opens on the book kept last time.
    let mut app = ContactsApp::from_settings();
    app::launch("contacts", &mut app)
}

/// What the close question is holding up. Only the close: nothing else in
/// this app can lose a contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    /// The window, asked to close over a contact being edited or while a save
    /// is failing.
    Close,
}

/// The name a contact is shown by when nobody gave it another: "First Last",
/// or whichever of the two it has.
fn default_display_name(first: &str, last: &str) -> String {
    if last.is_empty() {
        first.to_string()
    } else if first.is_empty() {
        last.to_string()
    } else {
        format!("{first} {last}")
    }
}

impl ContactsApp {
    /// The window's address book: the one kept last time, and every change
    /// kept from here on.
    pub fn from_settings() -> Self {
        let mut app = Self::new();
        match book_path() {
            Some(path) => {
                app.persist = true;
                app.load_book(&path);
            }
            // Nowhere to keep anything, and never will be while this window
            // is open: said once, and not asked about again at every close.
            None => app.store_error = Some(String::from(NO_HOME)),
        }
        app
    }

    /// Read the address book at `path`; with none there yet, this is a first
    /// run.
    ///
    /// One that cannot be read whole is left exactly as it is: nothing is
    /// saved over it, and the window says so for as long as it is open. A
    /// save would write back only whoever was understood.
    fn load_book(&mut self, path: &std::path::Path) {
        self.load_book_within(path, MAX_BOOK_BYTES);
    }

    /// [`load_book`](Self::load_book) with the size limit given, so a test
    /// can reach the limit without writing sixty-four megabytes.
    fn load_book_within(&mut self, path: &std::path::Path, max_bytes: usize) {
        let refused = |why: String| {
            format!(
                "{} was not read ({why}), so nothing is saved over it",
                path.display()
            )
        };
        let read = match safeio::read_to_string_capped(path, max_bytes) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                self.persist = false;
                self.store_error = Some(refused(err.to_string()));
                return;
            }
        };
        if read.truncated {
            self.persist = false;
            self.store_error = Some(refused(format!(
                "it is larger than {} MiB",
                max_bytes / (1024 * 1024)
            )));
            return;
        }
        match parse_book(&read.text) {
            Ok(store) => self.take_store(store),
            Err(why) => {
                self.persist = false;
                self.store_error = Some(refused(why));
            }
        }
    }

    /// Make `store` this window's address book.
    fn take_store(&mut self, store: ContactStore) {
        // Every time in the file, so that a change made now is later than all
        // of them even if the clock has since been set back.
        let latest = store
            .contacts
            .iter()
            .flat_map(|c| [c.created_at, c.updated_at, c.last_contacted.unwrap_or(0)])
            .max()
            .unwrap_or(0);
        self.last_stamp = self.last_stamp.max(latest);
        self.store = store;
        self.kept_revision = self.store.revision();
        self.view = DetailView::Empty;
    }

    /// Write the address book, if it has changed and this window keeps
    /// anything.
    ///
    /// A failure is kept in `store_error`, drawn in the status line, and the
    /// book stays unkept, so the next change -- or Ctrl+S, or the close
    /// question's Save -- tries again.
    pub fn keep(&mut self) {
        if !self.persist || self.store.revision() == self.kept_revision {
            return;
        }
        let Some(path) = book_path() else {
            self.store_error = Some(String::from(NO_HOME));
            return;
        };
        let text = book_text(&self.store);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| safeio::write_str_atomically(&path, &text));
        match written {
            Ok(()) => {
                self.kept_revision = self.store.revision();
                self.store_error = None;
            }
            Err(err) => {
                self.store_error = Some(format!("Not saved to {}: {err}", path.display()));
            }
        }
    }

    /// Whether the book has changes that are not written.
    #[must_use]
    pub fn unkept(&self) -> bool {
        self.persist && self.store.revision() != self.kept_revision
    }

    /// Where the book is kept -- or why it is not -- for the empty window and
    /// for Ctrl+S.
    #[must_use]
    pub fn keeping_line(&self) -> String {
        if let Some(error) = &self.store_error {
            return error.clone();
        }
        // Asked first, so a window that keeps nothing never reads where the
        // settings are: a test's window does not, and must not see another
        // test's scratch directory.
        if !self.persist {
            return String::from("Nothing added here is kept.");
        }
        book_path().map_or_else(
            || String::from(NO_HOME),
            |path| format!("Everyone added is kept in {}.", path.display()),
        )
    }

    /// A stamp for a change made now: the clock's reading in milliseconds
    /// since 1970 -- but always later than every stamp already given, so a
    /// later change sorts later even within one millisecond, or after the
    /// clock has been set back.
    fn stamp(&mut self) -> u64 {
        self.last_stamp = clock_ms().max(self.last_stamp.saturating_add(1));
        self.last_stamp
    }

    /// Whether the window may close now.
    ///
    /// A contact being edited is asked about -- the form has Save and Cancel
    /// of its own, and closing is neither -- and so is a book whose last save
    /// failed, since closing then loses what it could not write. Neither is
    /// asked in a window that keeps nothing: it has said so from the start,
    /// and Save could not keep anything anyway.
    pub fn request_close(&mut self) -> bool {
        self.keep();
        if !self.persist {
            return true;
        }
        let (message, prompt) = if self.form_has_changes() {
            (
                String::from("The contact being edited has changes that are not saved."),
                String::from("Save it before closing?"),
            )
        } else if self.unkept() {
            let detail = self.store_error.clone().unwrap_or_default();
            (
                String::from("Your latest changes to your contacts are not saved."),
                format!("{detail} -- try saving again before closing?"),
            )
        } else {
            return true;
        };
        // The question replaces whatever is up: a picker would take the keys
        // it needs, and be drawn over it.
        self.picker.close();
        self.show_help = false;
        self.question = Some(unsaved::Question::new(&message, &prompt, Pending::Close));
        false
    }

    /// Act on the close question's answer.
    fn answer(&mut self, choice: unsaved::Choice) {
        match choice {
            // The contact being edited is saved, and the window goes only if
            // the book is then written; if that fails, the error is on screen
            // and the window stays, which is what Save asked for.
            unsaved::Choice::Save => {
                if self.form_has_changes() {
                    self.save_form();
                }
                self.keep();
                self.quit = !self.unkept();
            }
            unsaved::Choice::Discard => self.quit = true,
            unsaved::Choice::Cancel => {}
        }
    }
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
    /// The picker is not merely open: it is DRAWN.
    ///
    /// `is_open()` returning true is not the same claim, and assuming it was
    /// is how `apps/flashcards` shipped a dialog that took every keystroke and
    /// painted nothing. Deleting the `picker.render` line in the renderer
    /// leaves `is_open()` true and every other test green; this is the one
    /// that notices.
    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event. This is
    /// the half routing decides: with a dialog up, a keystroke belongs to the
    /// dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    /// Every string the window draws, joined.
    fn card_text(app: &ContactsApp) -> String {
        app.frame(1000.0, 700.0)
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn stroke_event(stroke: &KeyEvent) -> Event {
        Event::Key(stroke.clone())
    }

    /// **Every key the list advertises is one this program answers.**
    ///
    /// A contact selected and the keyboard free, because `E`, `Delete` and
    /// `Enter` act on a selection and the letters are shortcuts only when
    /// nothing has the keyboard. Both refusals are correct.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                // Two focus states. The letters are shortcuts only when
                // nothing has the keyboard, and `Enter` and `Backspace` only
                // mean anything when something does -- `Enter` with
                // `Focus::None` assigns `Focus::None`, a no-op by
                // construction. No single state can answer both halves.
                let answered = [Focus::None, Focus::Field(FormField::FirstName)]
                    .into_iter()
                    .any(|focus| {
                        let mut app = ContactsApp::new();
                        app.focus = focus;
                        // A contact open, because `selected_id()` reads the *view* --
                        // `E` and `Delete` are refused with nothing selected, and that
                        // refusal is correct rather than a missing binding. Added
                        // rather than looked for: a fresh app has an empty store, so
                        // the first version of this looked for a contact, found none,
                        // and left the fixture exactly as it was.
                        let id = app.store.add_contact(make_contact("Ada", "Lovelace"));
                        app.view = DetailView::ViewContact(id);
                        // With a field focused, put the app in an *edit*:
                        // `save_form` matches on the view, so `Enter` does
                        // nothing at all unless there is a form to save.
                        if focus != Focus::None {
                            app.view = DetailView::EditContact(id);
                            app.edit_first_name = String::from("Changed");
                        }
                        // What a user could notice, which is wider than the frame.
                        // `handle_key` returns `()` and there is no consumed signal to
                        // assert on, so the test asks whether anything visible moved.
                        // The frame alone is not enough: `Tab` moves the keyboard
                        // focus, and with no form on screen that focus is drawn
                        // nowhere, so two frames compare equal while the app has
                        // plainly answered the key.
                        let snapshot = |a: &ContactsApp| {
                            format!(
                                "{:?}|{:?}|{:?}|{}|{}",
                                a.frame(1000.0, 700.0).commands(),
                                a.focus,
                                a.view,
                                a.search_query,
                                a.status
                            )
                        };
                        let before = snapshot(&app);
                        app.handle_event(&stroke_event(&stroke), (1000.0, 700.0));
                        snapshot(&app) != before
                    });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and {:?} changed nothing on screen",
                    stroke.key
                );
            }
        }
    }

    /// **The shortcut list reaches the window, and nothing acts behind it.**
    ///
    /// The control is the half that matters: `N` behind the card must not
    /// start a new contact, and asserting only that it does not would pass on
    /// an app that had lost `N` altogether.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = ContactsApp::new();
        let size = (1000.0, 700.0);
        assert!(
            !card_text(&app).contains("F1 closes this"),
            "the list is up before anybody asked for it"
        );

        let key = |k: Key| {
            Event::Key(KeyEvent {
                key: k,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::new(),
            })
        };

        app.handle_event(&key(Key::F1), size);
        let shown = card_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        // start_new_contact sets view; `editing` is not a field of this app at
        // all. Read from the function rather than guessed from the key name.
        let view = app.view.clone();
        app.handle_event(&key(Key::N), size);
        assert_eq!(
            app.view, view,
            "N started a new contact through the shortcut card"
        );

        app.handle_event(&key(Key::Escape), size);
        assert!(
            !card_text(&app).contains("F1 closes this"),
            "Escape did not close it"
        );

        app.handle_event(&key(Key::N), size);
        assert_ne!(
            app.view, view,
            "control: N does nothing even with the card down"
        );
    }

    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_form() {
        let mut app = ContactsApp::new();
        let size = (1000.0, 700.0);
        app.focus = Focus::Field(FormField::FirstName);
        let before = app.focus;

        let mut ctrl = guitk::event::Modifiers::NONE;
        ctrl.ctrl = true;
        app.handle_event(
            &Event::Key(KeyEvent {
                key: Key::O,
                pressed: true,
                modifiers: ctrl,
                text: String::new(),
            }),
            size,
        );
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(
            &Event::Key(KeyEvent {
                key: Key::Tab,
                pressed: true,
                modifiers: guitk::event::Modifiers::NONE,
                text: String::new(),
            }),
            size,
        );
        assert_eq!(
            app.focus, before,
            "Tab at the open dialog moved the focus in the form behind it"
        );
    }

    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        let mut app = ContactsApp::new();
        let before = app.frame(1024.0, 768.0).into_tree().commands.len();
        app.open_file_dialog(true);
        assert!(app.picker.is_open(), "no picker came up");
        let after = app.frame(1024.0, 768.0).into_tree().commands.len();
        let own = app.picker.render(&app.palette, 1024.0, 768.0).len();
        assert!(
            own > 0,
            "the picker itself draws nothing, so this proves nothing"
        );
        assert!(
            after >= before + own,
            "the frame does not contain the picker's own {own} command(s) ({before} before, {after} after) -- something else grew instead"
        );
    }

    /// An address book survives a write and a read.
    ///
    /// `export_vcards` and `import_vcards` were written, tested and
    /// unreachable: full vCard, CRLF joining per the spec, `BEGIN:VCARD` block
    /// parsing. The format was the hard part and it was already finished --
    /// `scripts/find-stranded-serialisers.py` exists because this is common.
    ///
    /// Round-tripped through the real functions rather than compared against a
    /// literal: the vCard shape is `export_vcards`' business, and this test is
    /// about the door.
    #[test]
    fn an_address_book_survives_a_write_and_a_read() {
        let dir = std::env::temp_dir().join("slateos-contacts-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("book.vcf");

        let mut app = ContactsApp::new();
        let mut c = Contact::new(0, "Ada", "Lovelace");
        c.phones
            .push(PhoneNumber::new("+44 20 7946 0000", PhoneType::Mobile));
        app.store.add_contact(c);

        let said = app.write_vcards(&path);
        assert!(said.starts_with("Wrote 1 contact"), "{said}");

        let mut reopened = ContactsApp::new();
        let said = reopened.read_vcards(&path);
        assert!(said.starts_with("Added 1 contact"), "{said}");
        assert_eq!(reopened.store.contacts.len(), 1);
        assert_eq!(reopened.store.contacts[0].first_name, "Ada");

        std::fs::remove_file(&path).ok();
    }

    /// An import adds rather than replaces.
    ///
    /// The user asked to import an address book, not to discard the one they
    /// have. Duplicates are the duplicate finder's problem, and this app
    /// already has one.
    #[test]
    fn an_import_adds_rather_than_replacing() {
        let dir = std::env::temp_dir().join("slateos-contacts-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("one.vcf");

        let mut source = ContactsApp::new();
        source.store.add_contact(Contact::new(0, "Grace", "Hopper"));
        source.write_vcards(&path);

        let mut app = ContactsApp::new();
        app.store.add_contact(Contact::new(0, "Ada", "Lovelace"));
        app.read_vcards(&path);
        assert_eq!(app.store.contacts.len(), 2, "the import replaced the book");

        std::fs::remove_file(&path).ok();
    }

    /// An empty book refuses rather than writing an empty file.
    ///
    /// A zero-byte contacts.vcf is indistinguishable from a failed export
    /// afterwards, and the user would have no way to tell which they had.
    #[test]
    fn an_empty_book_refuses_to_write() {
        let mut app = ContactsApp::new();
        assert!(app.store.contacts.is_empty());
        let path = std::env::temp_dir().join("slateos-contacts-should-not-exist.vcf");
        std::fs::remove_file(&path).ok();

        let said = app.write_vcards(&path);
        assert!(said.contains("No contacts to write"), "{said}");
        assert!(!path.exists(), "an empty file was written anyway");
    }

    /// A fresh window holds nobody, and says nothing is kept.
    ///
    /// `main` called `load_sample_data`, so the window opened on Alice
    /// Anderson of Acme Corp with two phone numbers, two email addresses, a
    /// birthday, a home address and a note about meeting her at a conference
    /// -- in the place the user's own address book goes.
    ///
    /// The fixture was built with real care: `+1-555-01xx` is the reserved
    /// fictional range and `example.com` is reserved for documentation, so
    /// nobody could accidentally ring or mail these people. That care is why
    /// it lasted. A careful fixture is harder to notice than a careless one.
    #[test]
    fn a_fresh_window_holds_nobody_and_says_how_to_start() {
        let app = ContactsApp::new();
        assert!(
            app.store.contacts.is_empty(),
            "contacts appeared from nowhere"
        );
        assert!(app.store.groups.is_empty(), "groups appeared from nowhere");

        let texts: Vec<String> = app
            .render()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|t| t == NO_CONTACTS),
            "the window never said {NO_CONTACTS:?}"
        );
        // The property, not the sentence: what the second line must say is
        // whether what is added here is kept. A window made by `new` keeps
        // nothing, and says so; `what_is_added_is_there_next_time` checks the
        // one `main` makes says where.
        assert!(
            texts.iter().any(|t| t == "Nothing added here is kept."),
            "a window that keeps nothing does not say so: {texts:?}"
        );
    }

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
    }

    #[test]
    fn test_contact_group_with_color() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let g = ContactGroup::new(1, "Work").with_color(pal.red);
        assert_eq!(g.color, pal.red);
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

    /// The count a group shows is the count of its members.
    ///
    /// Was `test_store_refresh_group_counts`, against a cached
    /// `ContactGroup::member_count` that `refresh_group_counts` filled and
    /// **nothing ever read** -- the number on screen has always come from
    /// `group_stats`, which recomputes it with the identical filter. Two
    /// copies of one number, and the test guarded the copy nobody saw. It now
    /// asks the function the screen asks.
    #[test]
    fn a_group_reports_the_number_of_contacts_in_it() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Team"));
        let cid1 = store.add_contact(make_contact("A", "A"));
        let cid2 = store.add_contact(make_contact("B", "B"));
        store.add_contact_to_group(cid1, gid);
        store.add_contact_to_group(cid2, gid);

        let stats = store.group_stats();
        let (_, name, count) = stats
            .iter()
            .find(|(id, _, _)| *id == gid)
            .expect("the group is not in the stats at all");
        assert_eq!(name, "Team");
        assert_eq!(*count, 2);
    }

    /// Removing a contact changes it, with no refresh step to forget.
    #[test]
    fn a_group_count_follows_its_membership() {
        let mut store = ContactStore::new();
        let gid = store.add_group(ContactGroup::new(0, "Team"));
        let cid = store.add_contact(make_contact("A", "A"));
        store.add_contact_to_group(cid, gid);
        assert_eq!(store.group_stats()[0].2, 1);

        store.remove_contact_from_group(cid, gid);
        assert_eq!(
            store.group_stats()[0].2,
            0,
            "a cached count would still read 1 here until something refreshed it"
        );
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
        let count = store.import_vcards(data, 1_234);
        assert_eq!(count, 1);
        assert_eq!(store.contact_count(), 1);
        // Stamped as added now, so "recently added" puts them first.
        let added = &store.all_contacts()[0];
        assert_eq!((added.created_at, added.updated_at), (1_234, 1_234));
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        app.render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    y,
                    text,
                    font_size,
                    color,
                    ..
                } if (font_size - NOTES_FONT_SIZE).abs() < 0.01 && color == pal.subtext0 => {
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
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
        // its heading. The heading is the bold pal.overlay0 run; the button is a
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
                    && color == pal.overlay0 =>
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

    // -----------------------------------------------------------------------
    // The window: geometry
    //
    // Everything below this line exists because the address book had no
    // window. `main` built a store, drew one frame into a `Vec` and dropped
    // it, so the picture was never wrong about anything -- nobody looked at
    // it and nothing could be pressed. These tests ask the picture the
    // questions a user would ask it with a mouse.
    // -----------------------------------------------------------------------

    /// The window sizes every geometry sweep is run at.
    ///
    /// Not a grid for its own sake: each entry breaks a different assumption
    /// -- too narrow for a detail panel, too narrow for the A-Z rail, too
    /// short for the search box, wider than it is tall and the other way
    /// round, and three that are not really windows at all.
    const GRID: [(f32, f32); 12] = [
        (1024.0, 768.0),
        (1920.0, 1080.0),
        (1280.0, 800.0),
        (640.0, 480.0),
        (480.0, 900.0),
        (1600.0, 260.0),
        (420.0, 300.0),
        (320.0, 240.0),
        (200.0, 160.0),
        (120.0, 90.0),
        (40.0, 30.0),
        (2.0, 2.0),
    ];

    /// The address book as the window opens it: sample data loaded.
    fn wired() -> ContactsApp {
        let mut app = ContactsApp::new();
        app.load_sample_data();
        app
    }

    /// The id of the first contact the list would show, at the default size.
    fn first_listed(app: &ContactsApp) -> u64 {
        app.store
            .filtered_sorted(&app.filter, app.sort_order, &app.search_query)
            .first()
            .expect("the sample data has contacts")
            .id
    }

    /// The states every geometry sweep is run over.
    ///
    /// A window is only as right as its worst state. Sweeping the default
    /// state proves the default state: it is the form with every field full,
    /// the panel showing a contact with a paragraph of notes, and the list
    /// filtered down to nothing that have somewhere to go wrong.
    fn states() -> Vec<(&'static str, ContactsApp)> {
        let mut out: Vec<(&'static str, ContactsApp)> = Vec::new();

        out.push(("empty store", ContactsApp::new()));
        out.push(("sample data, nothing selected", wired()));

        let mut viewing = wired();
        let id = first_listed(&viewing);
        viewing.select_contact(id);
        out.push(("viewing a contact", viewing));

        let mut wordy = wired();
        let id = first_listed(&wordy);
        if let Some(c) = wordy.store.get_contact_mut(id) {
            c.notes = LONG_NOTES.repeat(4);
            c.first_name = "Bartholomew".repeat(4);
        }
        wordy.select_contact(id);
        out.push(("viewing a contact with far too much to say", wordy));

        let mut editing = wired();
        let id = first_listed(&editing);
        editing.select_contact(id);
        editing.start_editing();
        out.push(("editing a contact", editing));

        let mut adding = wired();
        adding.start_new_contact();
        for field in FormField::ALL {
            *adding.field_mut(field) = "0123456789".repeat(4);
        }
        out.push(("adding, every field full", adding));

        let mut groups = wired();
        groups.activate(Target::ShowGroups);
        out.push(("the groups panel", groups));

        let mut dupes = wired();
        let dup = Contact::new(0, "Alice", "Anderson");
        dupes.store.add_contact(dup);
        dupes.activate(Target::ShowDuplicates);
        out.push(("the duplicates panel", dupes));

        let mut searching = wired();
        searching.focus = Focus::Search;
        searching.search_query = String::from("qqqqqqqqqqqqqqqqqqqq");
        out.push(("searching, nothing matches", searching));

        let mut scrolled = wired();
        for i in 0..60 {
            scrolled
                .store
                .add_contact(Contact::new(0, &format!("Filler{i}"), "Person"));
        }
        scrolled.scroll_offset = 900.0;
        out.push(("a long list, scrolled", scrolled));

        out
    }

    /// Every run of text the frame drew, as `(text, x, y, size, max_width)`.
    fn text_runs(frame: &Frame<Target>) -> Vec<(String, f32, f32, f32, Option<f32>)> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    max_width,
                    ..
                } => Some((text.clone(), *x, *y, *font_size, *max_width)),
                _ => None,
            })
            .collect()
    }

    /// Every clip the frame pushed, as a rectangle.
    fn clips(frame: &Frame<Target>) -> Vec<Rect> {
        frame
            .commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => Some(Rect::new(*x, *y, *width, *height)),
                _ => None,
            })
            .collect()
    }

    /// Every run of text, paired with the clip that was in force when it was
    /// drawn -- the intersection of the whole clip stack, or the window if
    /// nothing was clipped.
    ///
    /// The intersection matters and a "innermost clip wins" reading would be
    /// wrong: clips nest, and a pass that pushed a second, wider clip inside
    /// a narrow one would otherwise be measured against the wider of the two
    /// and let a run through that the renderer will cut.
    fn text_runs_clipped(
        frame: &Frame<Target>,
        size: (f32, f32),
    ) -> Vec<(String, f32, f32, f32, Option<f32>, Rect)> {
        // An unclipped run is measured against the window itself, not
        // against nothing: a run drawn outside a window nobody clipped is
        // just as lost as one that escaped a clip.
        let window = Rect::new(0.0, 0.0, size.0, size.1);
        let mut stack: Vec<Rect> = Vec::new();
        let mut out = Vec::new();
        for c in frame.commands() {
            match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => {
                    let next = Rect::new(*x, *y, *width, *height);
                    // `intersect` returns `None` for two rectangles that
                    // do not meet at all; nothing drawn inside such a clip
                    // is visible, and `Rect::EMPTY` says so.
                    let merged = stack
                        .last()
                        .map_or(next, |outer| outer.intersect(next).unwrap_or(Rect::EMPTY));
                    stack.push(merged);
                }
                RenderCommand::PopClip => {
                    stack.pop();
                }
                RenderCommand::Text {
                    x,
                    y,
                    text,
                    font_size,
                    max_width,
                    ..
                } => out.push((
                    text.clone(),
                    *x,
                    *y,
                    *font_size,
                    *max_width,
                    stack.last().copied().unwrap_or(window),
                )),
                _ => {}
            }
        }
        out
    }

    /// Where the picture drew a given run of text, as a point inside it.
    ///
    /// This is how a control is found when the point of the test is *which*
    /// control the press reaches. Asking the frame for the rectangle of a
    /// `Target` asks the drawing pass where it thinks its own controls are,
    /// and a pass that recorded every row one row off would answer with the
    /// same offset it made the mistake with -- finding and clicking would
    /// cancel out, and the test would pass over a list where every row
    /// chose its neighbour. The words are the only thing a user can read.
    fn text_point(app: &ContactsApp, size: (f32, f32), wanted: &str) -> Option<(f32, f32)> {
        let frame = app.frame(size.0, size.1);
        let found: Vec<(f32, f32)> = text_runs(&frame)
            .into_iter()
            .filter(|(text, ..)| text == wanted)
            .map(|(_, x, y, font_size, max_width)| {
                (
                    x + max_width.unwrap_or(font_size) * 0.5,
                    y + font_size * 0.5,
                )
            })
            .collect();
        // Two runs reading the same words make "press the thing that says X"
        // ambiguous, and the caller would silently get whichever came first.
        // Better to stop and be told than to test the wrong rectangle.
        assert!(
            found.len() <= 1,
            "{} runs of text read {wanted:?}",
            found.len()
        );
        found.first().copied()
    }

    /// Every filled box, paired with the clip that was in force when it was
    /// drawn -- the same rule `text_runs_clipped` applies to text.
    fn fills_clipped(frame: &Frame<Target>, size: (f32, f32)) -> Vec<(Rect, Rect)> {
        let window = Rect::new(0.0, 0.0, size.0, size.1);
        let mut stack: Vec<Rect> = Vec::new();
        let mut out = Vec::new();
        for c in frame.commands() {
            match c {
                RenderCommand::PushClip {
                    x,
                    y,
                    width,
                    height,
                } => {
                    let next = Rect::new(*x, *y, *width, *height);
                    let merged = stack
                        .last()
                        .map_or(next, |outer| outer.intersect(next).unwrap_or(Rect::EMPTY));
                    stack.push(merged);
                }
                RenderCommand::PopClip => {
                    stack.pop();
                }
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => out.push((
                    Rect::new(*x, *y, *width, *height),
                    stack.last().copied().unwrap_or(window),
                )),
                _ => {}
            }
        }
        out
    }

    /// `text_point`, insisting the words are there.
    fn point(app: &ContactsApp, size: (f32, f32), wanted: &str) -> (f32, f32) {
        text_point(app, size, wanted)
            .unwrap_or_else(|| panic!("nothing in the picture reads {wanted:?}"))
    }

    /// Whether the picture says something.
    fn shows(app: &ContactsApp, size: (f32, f32), wanted: &str) -> bool {
        text_runs(&app.frame(size.0, size.1))
            .iter()
            .any(|(text, ..)| text == wanted)
    }

    /// Press the control whose words the picture shows.
    ///
    /// The two steps are one function because splitting them borrows the app
    /// immutably to find the point and mutably to press it, in the same
    /// expression.
    fn click_text(app: &mut ContactsApp, size: (f32, f32), wanted: &str) {
        let at = point(app, size, wanted);
        click(app, at, size);
    }

    /// A left press at a point, answered against a window of `size`.
    ///
    /// Named `click` rather than `press` because `guitk::probe::press` builds
    /// a *key* press, and one of the two would otherwise have to be spelled
    /// out in full at every call -- in a file where both are used constantly.
    fn click(app: &mut ContactsApp, at: (f32, f32), size: (f32, f32)) {
        app.click_at(at.0, at.1, MouseButton::Left, size);
    }

    /// `text_point`, but only looking where the user is looking.
    ///
    /// A window shows the same word in two places often and legitimately:
    /// "Groups" names a view on the sidebar *and* a section of the detail
    /// panel, "E" is both a letter on the rail and a name's initial. Asking
    /// the whole window for "the run that reads Groups" is then genuinely
    /// ambiguous, and the honest answer is not "take the first" -- that
    /// would let a test that means to press a sidebar cell quietly press a
    /// heading instead, and keep passing after the cell stopped being drawn.
    /// Naming the region is how the question is made answerable: a press
    /// aimed at the rail is aimed at the rail.
    fn text_point_in(
        app: &ContactsApp,
        size: (f32, f32),
        region: Rect,
        wanted: &str,
    ) -> Option<(f32, f32)> {
        let frame = app.frame(size.0, size.1);
        let found: Vec<(f32, f32)> = text_runs(&frame)
            .into_iter()
            .filter(|(text, ..)| text == wanted)
            .map(|(_, x, y, font_size, max_width)| {
                (
                    x + max_width.unwrap_or(font_size) * 0.5,
                    y + font_size * 0.5,
                )
            })
            .filter(|(cx, cy)| region.contains(*cx, *cy))
            .collect();
        assert!(
            found.len() <= 1,
            "{} runs of text read {wanted:?} inside {region:?}",
            found.len()
        );
        found.first().copied()
    }

    /// `point`, scoped to a region, insisting the words are there.
    fn point_in(app: &ContactsApp, size: (f32, f32), region: Rect, wanted: &str) -> (f32, f32) {
        text_point_in(app, size, region, wanted)
            .unwrap_or_else(|| panic!("nothing inside {region:?} reads {wanted:?}"))
    }

    /// Press the control inside `region` whose words the picture shows.
    fn click_text_in(app: &mut ContactsApp, size: (f32, f32), region: Rect, wanted: &str) {
        let at = point_in(app, size, region, wanted);
        click(app, at, size);
    }

    /// Whether the picture says something *in a particular place*.
    fn shows_in(app: &ContactsApp, size: (f32, f32), region: Rect, wanted: &str) -> bool {
        text_point_in(app, size, region, wanted).is_some()
    }

    #[test]
    fn the_window_is_painted_edge_to_edge_at_every_size() {
        for (w, h) in GRID {
            let frame = wired().frame(w, h);
            let first = frame.commands().first().expect("a frame draws something");
            match first {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => {
                    assert!(
                        (*x - 0.0).abs() < 0.01 && (*y - 0.0).abs() < 0.01,
                        "{w}x{h}: the background starts at ({x}, {y}), not the corner"
                    );
                    assert!(
                        (*width - w).abs() < 0.01 && (*height - h).abs() < 0.01,
                        "{w}x{h}: the background is {width}x{height} -- the compositor \
                         would show whatever was in the window before us in the rest"
                    );
                }
                other => panic!("{w}x{h}: the frame opens with {other:?}, not a background"),
            }
        }
    }

    #[test]
    fn the_frame_is_balanced_at_every_size_and_state() {
        for (name, app) in states() {
            for (w, h) in GRID {
                assert!(
                    app.frame(w, h).is_balanced(),
                    "{name} at {w}x{h}: a clip was pushed and not popped, so every \
                     later hit box is measured against the wrong rectangle"
                );
            }
        }
    }

    #[test]
    fn every_control_lies_inside_the_window() {
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                for (target, rect) in frame.hits() {
                    assert!(
                        rect.x >= -0.01
                            && rect.y >= -0.01
                            && rect.right() <= w + 0.01
                            && rect.bottom() <= h + 0.01,
                        "{name} at {w}x{h}: {target:?} answers presses at {rect:?}, \
                         which is partly outside the window"
                    );
                    assert!(
                        !rect.is_empty(),
                        "{name} at {w}x{h}: {target:?} has an empty hit box"
                    );
                }
            }
        }
    }

    #[test]
    fn every_run_of_text_is_bounded_and_inside_the_window() {
        // The old drawing pass wrote `max_width: None` on most of its runs,
        // which is not a bound at all: the run is as long as the string, and
        // a name longer than its column was drawn across whatever sat beside
        // it. A bound the renderer can honour is the fix; asking for one on
        // every run is what stops the next run being written without it.
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                // Measured against the clip in force, not against the window,
                // and the two axes are not measured the same way -- because
                // nothing in this program scrolls sideways.
                //
                // Sideways, a run must fit its clip outright: a name wider
                // than its column is the exact fault `max_width: None` used
                // to cause, and a rule that let a run hang over the edge
                // "because it is clipped anyway" would let it back in.
                //
                // Vertically, a run need only be *partly* inside: the list
                // scrolls, so a row at the bottom edge is meant to be half
                // drawn, and demanding it fit whole would forbid a scrolling
                // list outright. What is still forbidden is a run wholly
                // outside the clip -- ink nobody can ever see, which is what
                // a row drawn at a scroll offset the clip has left behind
                // looks like. `every_clip_lies_inside_the_window` closes the
                // remaining half: a clip that escaped the window would let
                // everything in it escape too.
                for (text, x, y, size, max_width, clip) in text_runs_clipped(&frame, (w, h)) {
                    let Some(bound) = max_width else {
                        panic!(
                            "{name} at {w}x{h}: {text:?} is drawn with no max_width, so it \
                             runs as far as the string is long and over whatever is beside it"
                        );
                    };
                    assert!(
                        bound.is_finite() && bound > 0.0,
                        "{name} at {w}x{h}: {text:?} is bounded to {bound}"
                    );
                    assert!(
                        x >= clip.x - 0.01 && x + bound <= clip.right() + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {x}..{} across {clip:?}",
                        x + bound
                    );
                    assert!(
                        y + size > clip.y - 0.01 && y < clip.bottom() + 0.01,
                        "{name} at {w}x{h}: {text:?} spans {y}..{} down {clip:?}, \
                         which it misses entirely -- it is drawn where nothing can see it",
                        y + size
                    );
                }
            }
        }
    }

    #[test]
    fn every_clip_lies_inside_the_window() {
        // The other half of the pair above. Bounding a run to its clip is
        // only worth anything if the clip is itself inside the window: a
        // clip that reached past the bottom edge would let every run in it
        // do the same, and the test above would nod it through.
        for (name, app) in states() {
            for (w, h) in GRID {
                for clip in clips(&app.frame(w, h)) {
                    assert!(
                        clip.x >= -0.01
                            && clip.y >= -0.01
                            && clip.right() <= w + 0.01
                            && clip.bottom() <= h + 0.01,
                        "{name} at {w}x{h}: a clip of {clip:?} reaches outside the window"
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_is_drawn_over_the_status_line() {
        // The status line is the last thing drawn, so it cannot be painted
        // over -- but a control that reaches under it would still take the
        // press, and the user would be clicking a button they cannot see.
        for (name, app) in states() {
            for (w, h) in GRID {
                let l = Layout::solve(w, h);
                if l.status.is_empty() {
                    continue;
                }
                let frame = app.frame(w, h);
                for (target, rect) in frame.hits() {
                    // The *bottom* edge, not the top. Asking only where a
                    // control starts lets one through that starts above the
                    // line and ends below it, which is the whole shape of the
                    // bug: the visible half is what the user aims at and the
                    // hidden half is what takes the press.
                    assert!(
                        rect.bottom() < l.status.y + 0.01,
                        "{name} at {w}x{h}: {target:?} at {rect:?} reaches under the \
                         status line, which is drawn after it"
                    );
                }
                // Paint, too, and not only controls. A panel run down over the
                // strip answers no press but does hide the line -- the last
                // thing the app said about what it just did goes out from
                // under the user with nothing to show it went.
                for (rect, clip) in fills_clipped(&frame, (w, h)) {
                    let Some(seen) = clip.intersect(rect) else {
                        continue;
                    };
                    // The window background is the one fill that is allowed
                    // to reach the bottom edge: it is what the strip is drawn
                    // *on*. So is anything drawn inside the strip itself.
                    if seen.contains(l.window.w / 2.0, 0.5)
                        || l.status.intersect(seen) == Some(seen)
                    {
                        continue;
                    }
                    assert!(
                        seen.bottom() < l.status.y + 0.01,
                        "{name} at {w}x{h}: a filled box at {seen:?} is painted under the \
                         status line at {:?}",
                        l.status
                    );
                }
                // And the clips, which are where the rule is actually kept.
                // A box drawn under the strip is a symptom; a *clip* reaching
                // under it is the permission, and the two do not appear
                // together. Running the detail panel down to the bottom edge
                // of the window moves nothing under the line at any size in
                // the grid -- the panel's own 24-pixel padding is larger than
                // the 22-pixel strip, so the content it holds stops just
                // short every time. What changed is that it is now *allowed*
                // to go under, and the next thing added to that panel would
                // have done so with nothing to say it had.
                for clip in clips(&frame) {
                    assert!(
                        clip.bottom() <= l.status.y + 0.01,
                        "{name} at {w}x{h}: a clip of {clip:?} reaches under the status \
                         line at {:?}, so whatever is drawn in it may too",
                        l.status
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_is_painted_entirely_outside_the_clip_in_force() {
        // A clip makes what is outside it invisible; it does not make it
        // free, and it does not make a picture that claims to have painted a
        // row three hundred pixels below the list into an honest picture.
        // The list's cursor runs down over every contact whether or not the
        // list has any room left, so this is the rule that stops it.
        //
        // *Entirely* outside, not partly: a row at the bottom edge is
        // rightly half-drawn and half-cut, and so is the last letter of the
        // A-Z rail. What nothing may be is wholly invisible.
        //
        // With one exemption, which is the rule rather than a hole in it:
        // the unit of "do not draw this" is the *item*, and an item is drawn
        // whole or not at all. The two-pixel sliver of a row at the bottom
        // edge draws its avatar circle in full, forty pixels below the cut,
        // and that is correct -- a row that starts inside the list is a row
        // the list is showing. So a fill wholly outside the clip is excused
        // exactly when some other fill drawn *under the same clip* encloses
        // it and is itself partly visible. Nothing else is: a row three
        // hundred pixels below the list has no such parent, because the only
        // fill enclosing it is the row's own background, which is just as
        // invisible as it is.
        for (name, app) in states() {
            for (w, h) in GRID {
                let frame = app.frame(w, h);
                let fills = fills_clipped(&frame, (w, h));
                for (i, (rect, clip)) in fills.iter().enumerate() {
                    if rect.is_empty() || clip.intersect(*rect).is_some() {
                        continue;
                    }
                    let carried = fills.iter().enumerate().any(|(j, (outer, outer_clip))| {
                        j != i
                            && outer_clip == clip
                            && outer_clip.intersect(*outer).is_some()
                            && outer.intersect(*rect) == Some(*rect)
                    });
                    assert!(
                        carried,
                        "{name} at {w}x{h}: a filled box at {rect:?} is painted \
                         entirely outside the clip {clip:?} that was in force, and \
                         no item drawn under that clip carries it"
                    );
                }
            }
        }
    }

    #[test]
    fn a_run_of_text_is_never_inked_outside_the_box_it_was_given() {
        // Every run in the program is centred in a box by `ink_box`, and a
        // run taller than its box centres to *outside* it at both ends. That
        // is how the status line came to be drawn below the bottom edge of a
        // thirty-pixel window: the strip was 22 tall, the run was asked for
        // at 13, and the arithmetic was fine -- until the strip was 8.
        for (bw, bh) in [
            (100.0, 20.0),
            (100.0, 4.0),
            (100.0, 0.0),
            (0.0, 20.0),
            (3.0, 3.0),
        ] {
            for size in [1.0, 9.0, 11.0, 13.0, 20.0, 36.0] {
                let r = Rect::new(10.0, 20.0, bw, bh);
                let ink = ink_box(r, size);
                assert!(
                    ink.y >= r.y - 0.01 && ink.bottom() <= r.bottom() + 0.01,
                    "a {size}-point run in a {bw}x{bh} box inks {}..{}, \
                     outside the box's {}..{}",
                    ink.y,
                    ink.bottom(),
                    r.y,
                    r.bottom()
                );
            }
        }
    }

    #[test]
    fn the_detail_panel_is_never_narrower_than_the_sidebar_where_both_are_shown() {
        // The panel holds a name, an address and a paragraph of notes; the
        // list holds one line per contact. Wherever the window is wide enough
        // to show both, the wider half is the one with more to say.
        //
        // Every width, not `GRID`'s twelve. The rule is a pure function of the
        // width and the split it governs has two knees in it -- the 45% share
        // meeting its 304-pixel cap, and the panel meeting its own minimum and
        // being dropped -- so the widths where a fault can hide are a *band*,
        // not a point. Raising the share to 95% leaves the panel narrower than
        // the sidebar only between 564 and 608 pixels: below that the panel is
        // dropped outright, which is a different rule and a legitimate one,
        // and above it the cap binds and the split is unchanged. A twelve-size
        // grid stepped straight over the band.
        for step in 0..=500_u32 {
            let w = f32::from(u16::try_from(step).unwrap_or(u16::MAX)) * 4.0;
            for h in [30.0_f32, 300.0, 768.0] {
                let l = Layout::solve(w, h);
                if l.panel.is_empty() || l.list.is_empty() {
                    continue;
                }
                let sidebar = l.list.w + l.alphabet.w;
                assert!(
                    l.panel.w >= sidebar - 0.01,
                    "{w}x{h}: the sidebar is {sidebar} wide and the panel it leaves is {}",
                    l.panel.w
                );
            }
        }
    }

    #[test]
    fn a_size_that_is_not_a_size_still_produces_a_window() {
        // A compositor that hands us a zero or a NaN is misbehaving, but the
        // answer to that is an empty window, not a frame full of NaN
        // rectangles that the renderer will read as enormous.
        for (w, h) in [
            (0.0, 0.0),
            (-100.0, -100.0),
            (f32::NAN, 768.0),
            (1024.0, f32::NAN),
            (f32::INFINITY, f32::INFINITY),
        ] {
            let app = wired();
            let frame = app.frame(w, h);
            assert!(frame.is_balanced(), "{w}x{h}: unbalanced");
            for (target, rect) in frame.hits() {
                assert!(
                    rect.x.is_finite()
                        && rect.y.is_finite()
                        && rect.w.is_finite()
                        && rect.h.is_finite(),
                    "{w}x{h}: {target:?} has hit box {rect:?}"
                );
            }
            for (text, x, y, _, max_width) in text_runs(&frame) {
                assert!(
                    x.is_finite() && y.is_finite() && max_width.is_some_and(f32::is_finite),
                    "{w}x{h}: {text:?} is drawn at ({x}, {y}) bounded to {max_width:?}"
                );
            }
        }
    }

    #[test]
    fn the_detail_panel_is_given_up_before_the_list_is() {
        // The list is the program: it is how a contact is found at all. The
        // panel is what is shown about the one already found. A window too
        // narrow for both keeps the one you cannot work without.
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            if l.panel.is_empty() {
                continue;
            }
            assert!(
                l.list.w >= MIN_LIST_WIDTH - 0.01,
                "{w}x{h}: the panel is {} wide while the list is down to {}",
                l.panel.w,
                l.list.w
            );
            assert!(
                l.panel.w >= MIN_PANEL_WIDTH - 0.01,
                "{w}x{h}: the panel is kept at {} wide, narrower than a name",
                l.panel.w
            );
        }
    }

    #[test]
    fn the_alphabet_rail_is_given_up_before_the_list_falls_below_a_row() {
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            if l.alphabet.is_empty() {
                continue;
            }
            assert!(
                l.list.w >= MIN_LIST_WIDTH - 0.01,
                "{w}x{h}: the A-Z rail is kept while the list is down to {} wide",
                l.list.w
            );
        }
    }

    #[test]
    fn a_strip_is_only_taken_if_the_list_keeps_a_whole_row() {
        // The strips are laid out top-down by a running cursor, and each is
        // only taken if a whole row of list survives it. A window that gave
        // all its height to chrome would draw a search box over an empty
        // list, which is the one arrangement that makes the program useless.
        //
        // Note what this does *not* claim: that a list always gets a whole
        // row. A window ninety pixels tall has room for a header, a status
        // line and twelve pixels between them, and twelve pixels of clipped
        // row is more useful than nothing. The rule is about what the chrome
        // is allowed to take, not about what the window is obliged to have.
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            let taken: Vec<(&str, Rect)> = [
                ("search", l.search),
                ("filter", l.strip),
                ("views", l.views),
            ]
            .into_iter()
            .filter(|(_, r)| r.h > 0.0)
            .collect();
            for (name, rect) in taken {
                assert!(
                    l.list.h >= MIN_LIST_HEIGHT - 0.01,
                    "{w}x{h}: the {name} strip took {} of the height and left the list \
                     {} tall -- less than the {MIN_LIST_HEIGHT} one row needs",
                    rect.h,
                    l.list.h
                );
            }
        }
    }

    #[test]
    fn the_boxes_of_the_sidebar_do_not_overlap_each_other() {
        // A running cursor lays these out one after another; an off-by-one
        // gap would put the search box over the filter strip, and the press
        // would reach whichever was recorded last rather than the one drawn
        // where the user aimed.
        for (w, h) in GRID {
            let l = Layout::solve(w, h);
            let stack = [
                ("header", l.header),
                ("search", l.search),
                ("strip", l.strip),
                ("views", l.views),
                ("list", l.list),
            ];
            let mut floor = 0.0_f32;
            for (name, rect) in stack {
                if rect.h <= 0.0 {
                    continue;
                }
                assert!(
                    rect.y >= floor - 0.01,
                    "{w}x{h}: {name} starts at {} but the box above ends at {floor}",
                    rect.y
                );
                floor = rect.bottom();
            }
            assert!(
                floor <= l.status.y + 0.01,
                "{w}x{h}: the sidebar ends at {floor}, below the status line at {}",
                l.status.y
            );
        }
    }

    use guitk::probe::{press as key_press, type_str, typing};

    // -----------------------------------------------------------------------
    // The window: what a press does
    //
    // Every test below finds its control by the words the picture shows and
    // presses that point, rather than by asking the frame where it put a
    // `Target`. A drawing pass that recorded every row one row off would
    // answer the second question with the same offset it made the mistake
    // with, and the test would pass over a list where every row selects its
    // neighbour.
    // -----------------------------------------------------------------------

    /// The default window size, which is what `Probe::SIZE` and the real
    /// window both open at.
    const SIZE: (f32, f32) = (WINDOW_WIDTH, WINDOW_HEIGHT);

    #[test]
    fn the_add_button_opens_a_blank_form_with_the_keyboard_in_the_first_field() {
        let mut app = wired();
        // The form is dirtied first, on purpose. A fresh app has an empty
        // form already, so "every field is empty after pressing +" is true
        // of a program that never clears it -- the assertion would be
        // reading the starting state rather than the effect of the press.
        // Cancel is not used to get back out: it clears the form on the way,
        // which would put the fixture right back where it started.
        click_text(&mut app, SIZE, "Edit");
        assert!(
            FormField::ALL
                .iter()
                .any(|f| !app.field_value(*f).is_empty()),
            "the fixture failed to leave anything in the form"
        );
        // Not `== Empty`: the app opens showing the first sample contact.
        // What has to be true before the press is only that the form is not
        // already open, or "pressing + opened the form" proves nothing.
        assert_ne!(app.view, DetailView::NewContact);
        click_text(&mut app, SIZE, "+");
        assert_eq!(
            app.view,
            DetailView::NewContact,
            "pressing + did not open the form"
        );
        assert_eq!(
            app.focus,
            Focus::Field(FormField::FirstName),
            "the form opened with the keyboard nowhere, so the first thing typed is lost"
        );
        // A form that opens carrying the last one's text writes the wrong
        // contact the moment Save is pressed.
        for field in FormField::ALL {
            assert!(
                app.field_value(field).is_empty(),
                "{:?} opened holding {:?}",
                field,
                app.field_value(field)
            );
        }
    }

    #[test]
    fn pressing_a_row_selects_the_contact_whose_name_it_shows() {
        let mut app = wired();
        let names: Vec<(u64, String)> = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, &app.search_query)
            .iter()
            .map(|c| (c.id, c.computed_display_name()))
            .collect();
        assert!(
            names.len() >= 3,
            "the sample data is too small to test with"
        );
        // Scoped to the list: the detail panel shows the selected contact's
        // name too, and a press aimed at "the row reading Alice Anderson"
        // must not land on the heading of the panel already showing her.
        let l = Layout::solve(SIZE.0, SIZE.1);
        for (id, name) in names.iter().take(4) {
            let Some(at) = text_point_in(&app, SIZE, l.list, name) else {
                continue;
            };
            click(&mut app, at, SIZE);
            assert_eq!(
                app.view,
                DetailView::ViewContact(*id),
                "pressing the row reading {name:?} selected something else"
            );
        }
    }

    #[test]
    fn no_press_inside_the_list_falls_between_two_rows() {
        // A list laid out by a running cursor can leave a seam between two
        // rows that answers nothing -- a user who lands on it sees their
        // click do nothing at all, for no reason they can see -- or, worse,
        // overlap two rows so that the top of one selects the other.
        //
        // The claim is *not* "every pixel of the list is clickable": the
        // A-Z letter dividers are labels, not controls, and are rightly
        // unclickable. It is that consecutive rows either touch exactly or
        // are parted by exactly one divider. Any other gap is a seam nobody
        // meant to leave, and any negative one is an overlap.
        let app = wired();
        let frame = app.frame(SIZE.0, SIZE.1);
        let mut rows: Vec<Rect> = frame
            .hits()
            .iter()
            .filter(|(t, _)| matches!(t, Target::Contact(_)))
            .map(|(_, r)| *r)
            .collect();
        assert!(rows.len() >= 3, "the list drew {} rows", rows.len());
        rows.sort_by(|a, b| a.y.total_cmp(&b.y));
        for pair in rows.windows(2) {
            let (upper, lower) = match pair {
                [a, b] => (*a, *b),
                _ => unreachable!("windows(2) yields pairs"),
            };
            let gap = lower.y - upper.bottom();
            assert!(
                gap.abs() < 0.01 || (gap - LETTER_DIVIDER_HEIGHT).abs() < 0.01,
                "the row at {} ends at {} and the next begins at {}, a gap of {gap} -- \
                 neither touching nor one letter divider apart",
                upper.y,
                upper.bottom(),
                lower.y
            );
        }
    }

    #[test]
    fn a_row_scrolled_out_of_sight_is_not_clickable() {
        // The old list re-derived the row under a press arithmetically, so a
        // press was answered at coordinates a scrolled-away row no longer
        // occupied. The hit boxes come from the same clipped pass that draws
        // the rows now, so the two cannot disagree.
        let mut app = wired();
        let first = first_listed(&app);
        let before = app
            .frame(SIZE.0, SIZE.1)
            .hits()
            .iter()
            .any(|(t, _)| *t == Target::Contact(first));
        assert!(before, "the first contact is not clickable to begin with");

        // A small scroll first. At 4000 the row is off the window as well as
        // off the list, and a scroll offset applied to the drawing but not to
        // the hit boxes -- the fault this test was written for -- would leave
        // a row answering in the middle of the list at either offset, so the
        // small case is the one that reads like the bug. It does *not* test
        // the list's clip: a row is only drawn at all when it is at least
        // partly inside the list, so this row is not drawn under any clip.
        app.scroll_offset = CONTACT_ROW_HEIGHT + LETTER_DIVIDER_HEIGHT + 4.0;
        let nudged = app
            .frame(SIZE.0, SIZE.1)
            .hits()
            .iter()
            .any(|(t, _)| *t == Target::Contact(first));
        assert!(
            !nudged,
            "the first contact still answers presses after the list scrolled it \
             above the top of the list, where the header is"
        );

        app.scroll_offset = 4000.0;
        let after = app
            .frame(SIZE.0, SIZE.1)
            .hits()
            .iter()
            .any(|(t, _)| *t == Target::Contact(first));
        assert!(
            !after,
            "the first contact still answers presses after the list scrolled past it"
        );
    }

    #[test]
    fn every_row_answers_only_inside_the_list() {
        // The rows are hit-boxed by the same pass that draws them, inside the
        // list's clip, and `Frame::hit` trims a hit box to the clip in force.
        // That trimming is the whole mechanism keeping a row from answering
        // outside the column it is drawn in, and it cannot be seen from any
        // test of the form "is this row clickable?": a row is only ever drawn
        // when it is at least partly inside the list, so no row is ever
        // entirely clipped away and every row stays clickable either way.
        //
        // What gives it away is *where* the box reaches. Clip the list to the
        // window instead of to itself and the row straddling the top edge
        // answers over the view cells above it -- press "Groups" and select a
        // contact -- while the one straddling the bottom answers under the
        // status line.
        for (name, app) in states() {
            for (w, h) in GRID {
                let l = Layout::solve(w, h);
                let frame = app.frame(w, h);
                for (target, rect) in frame.hits() {
                    if !matches!(target, Target::Contact(_)) {
                        continue;
                    }
                    assert!(
                        l.list.intersect(*rect) == Some(*rect),
                        "{name} at {w}x{h}: a row answers over {rect:?}, which is not \
                         inside the list at {:?}",
                        l.list
                    );
                }
            }
        }
    }

    #[test]
    fn the_filter_cell_reads_the_filter_in_force_and_changes_it() {
        let mut app = wired();
        let mut seen = vec![app.filter.label().to_string()];
        for _ in 0..4 {
            let label = app.filter.label().to_string();
            assert!(
                shows(&app, SIZE, &label),
                "the filter is {label:?} but nothing in the picture says so"
            );
            click_text(&mut app, SIZE, &label);
            seen.push(app.filter.label().to_string());
        }
        assert!(
            seen.iter().collect::<std::collections::BTreeSet<_>>().len() >= 3,
            "pressing the filter cell four times only ever reached {seen:?}"
        );
    }

    #[test]
    fn the_sort_cell_reads_the_order_in_force_and_changes_it() {
        let mut app = wired();
        let mut seen = vec![app.sort_order.label().to_string()];
        for _ in 0..4 {
            let label = app.sort_order.label().to_string();
            assert!(
                shows(&app, SIZE, &label),
                "the sort order is {label:?} but nothing in the picture says so"
            );
            click_text(&mut app, SIZE, &label);
            seen.push(app.sort_order.label().to_string());
        }
        assert_eq!(
            seen.iter().collect::<std::collections::BTreeSet<_>>().len(),
            4,
            "the four sort orders should cycle, but pressing reached {seen:?}"
        );
    }

    #[test]
    fn the_view_cells_reach_the_two_panels_nothing_else_could_reach() {
        // `DetailView::Groups` and `DetailView::Duplicates` were reachable
        // only by writing the field: the app drew both panels and offered no
        // control that selected either.
        let mut app = wired();
        let l = Layout::solve(SIZE.0, SIZE.1);
        // The word "Groups" appears twice on purpose -- once as the sidebar
        // cell that selects the panel, once as the heading of the section
        // the detail panel gives a contact's groups. The press is aimed at
        // the cell and the proof is read off the panel, so each assertion
        // has to say which of the two it means.
        click_text_in(&mut app, SIZE, l.views, "Groups");
        assert_eq!(app.view, DetailView::Groups);
        // The heading proves the panel is actually drawn, not merely selected.
        assert!(shows_in(&app, SIZE, l.panel, "Groups"));

        click_text_in(&mut app, SIZE, l.views, "Duplicates");
        assert_eq!(app.view, DetailView::Duplicates);
        assert!(shows_in(&app, SIZE, l.panel, "Duplicate Detection"));
    }

    #[test]
    fn the_search_box_takes_the_keyboard_and_typing_narrows_the_list() {
        let mut app = wired();
        let all = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, &app.search_query)
            .len();
        let wanted = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, "")
            .first()
            .expect("the sample data has contacts")
            .first_name
            .clone();

        let l = Layout::solve(SIZE.0, SIZE.1);
        click(&mut app, l.search.centre(), SIZE);
        assert_eq!(
            app.focus,
            Focus::Search,
            "pressing the search box did not give it the keyboard"
        );
        for ch in wanted.chars() {
            app.key_at(&typing(&ch.to_string()), SIZE);
        }
        assert_eq!(app.search_query, wanted);
        let narrowed = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, &app.search_query)
            .len();
        assert!(
            narrowed < all,
            "typing {wanted:?} left all {all} contacts in the list"
        );

        app.key_at(&key_press(Key::Backspace), SIZE);
        assert_eq!(
            app.search_query.len(),
            wanted.len() - wanted.chars().last().map_or(0, char::len_utf8),
            "backspace did not reach the search box"
        );
    }

    #[test]
    fn a_letter_on_the_rail_scrolls_the_list_to_that_letter() {
        let mut app = wired();
        let letters: Vec<char> = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, "")
            .iter()
            .map(|c| c.first_letter())
            .collect();
        let last = *letters.last().expect("the sample data has contacts");
        let first = *letters.first().expect("the sample data has contacts");
        assert_ne!(
            first, last,
            "the sample data is filed under one letter, so a jump cannot be seen"
        );
        // Scoped to the rail: a single letter is also what a name's initial
        // divider shows, and pressing a divider is not pressing the rail.
        let l = Layout::solve(SIZE.0, SIZE.1);
        let at = point_in(&app, SIZE, l.alphabet, &last.to_string());
        click(&mut app, at, SIZE);
        assert_eq!(app.selected_letter, Some(last));
        assert!(
            app.scroll_offset > 0.0,
            "jumping to {last} left the list at the top"
        );
        // The offset is what the drawing pass would have to scroll by, so the
        // row it names must be the first one drawn.
        let frame = app.frame(SIZE.0, SIZE.1);
        let topmost = frame
            .hits()
            .iter()
            .filter_map(|(t, r)| match t {
                Target::Contact(id) => Some((*id, r.y)),
                _ => None,
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        let expected = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, "")
            .iter()
            .find(|c| c.first_letter() == last)
            .map(|c| c.id);
        assert_eq!(
            topmost, expected,
            "the jump to {last} put a different contact at the top of the list"
        );
    }

    #[test]
    fn a_letter_nothing_is_filed_under_says_so_rather_than_scrolling_somewhere_else() {
        let mut app = wired();
        let taken: Vec<char> = app
            .store
            .filtered_sorted(&app.filter, app.sort_order, "")
            .iter()
            .map(|c| c.first_letter())
            .collect();
        let free = ALPHABET
            .iter()
            .copied()
            .find(|c| !taken.contains(c))
            .expect("some letter has no contacts");
        app.scroll_offset = 0.0;
        let l = Layout::solve(SIZE.0, SIZE.1);
        click_text_in(&mut app, SIZE, l.alphabet, &free.to_string());
        assert!(
            (app.scroll_offset - 0.0).abs() < 0.01,
            "jumping to the empty letter {free} scrolled to {}",
            app.scroll_offset
        );
        assert!(
            app.status.contains(free),
            "the status line says {:?}, which does not mention {free}",
            app.status
        );
    }

    #[test]
    fn edit_star_and_delete_each_do_their_own_job() {
        let mut app = wired();
        let id = first_listed(&app);
        app.select_contact(id);

        // The button reads "Star" or "Unstar" depending on what the contact
        // already is, and the sample data does have favourites -- so the
        // press is aimed at whatever the picture currently offers, and the
        // check is that the flag *changed*. Assuming "Star" here made the
        // test depend on which contact the sample data happens to list first.
        let starred = app.store.get_contact(id).is_some_and(|c| c.favorite);
        let label = if starred { "Unstar" } else { "Star" };
        click_text(&mut app, SIZE, label);
        assert_eq!(
            app.store.get_contact(id).is_some_and(|c| c.favorite),
            !starred,
            "{label} did not change whether the contact is a favourite"
        );
        // The label now reads the other way round, which is the only thing
        // that tells the user what pressing it again would do.
        let back = if starred { "Star" } else { "Unstar" };
        click_text(&mut app, SIZE, back);
        assert_eq!(
            app.store.get_contact(id).is_some_and(|c| c.favorite),
            starred,
            "{back} did not put the favourite back the way it was"
        );

        click_text(&mut app, SIZE, "Edit");
        assert_eq!(app.view, DetailView::EditContact(id));
        assert_eq!(
            app.field_value(FormField::FirstName),
            app.store
                .get_contact(id)
                .map(|c| c.first_name.clone())
                .unwrap_or_default(),
            "the form opened without the contact's name in it"
        );

        click_text(&mut app, SIZE, "Cancel");
        assert_eq!(app.view, DetailView::ViewContact(id));

        click_text(&mut app, SIZE, "Delete");
        assert!(
            app.store.get_contact(id).is_none(),
            "Delete left the contact in the store"
        );
        assert_eq!(
            app.view,
            DetailView::Empty,
            "the panel is still showing a contact that no longer exists"
        );
    }

    #[test]
    fn the_quick_actions_record_that_the_contact_was_reached() {
        let mut app = wired();
        let id = first_listed(&app);
        app.select_contact(id);
        let before = app.store.get_contact(id).and_then(|c| c.last_contacted);

        click_text(&mut app, SIZE, "Call");
        let after = app.store.get_contact(id).and_then(|c| c.last_contacted);
        assert!(
            after.is_some() && after != before,
            "Call left last_contacted at {before:?}"
        );

        // A second call must land *after* the first, or "recently contacted"
        // cannot order two calls made in the same session.
        click_text(&mut app, SIZE, "Email");
        let later = app.store.get_contact(id).and_then(|c| c.last_contacted);
        assert!(
            later > after,
            "the second call was recorded at {later:?}, not after {after:?}"
        );

        // Map opens a map; it does not mean anyone was reached.
        let before_map = app.store.get_contact(id).and_then(|c| c.last_contacted);
        click_text(&mut app, SIZE, "Map");
        assert_eq!(
            app.store.get_contact(id).and_then(|c| c.last_contacted),
            before_map,
            "pressing Map claimed the contact had been reached"
        );
    }

    #[test]
    fn pressing_a_field_gives_it_the_keyboard_and_typing_reaches_it() {
        // The old form was an array of `(label, value)` pairs -- enough to
        // draw a field and not enough to put a character in one.
        let mut app = wired();
        app.start_new_contact();
        for field in [FormField::FirstName, FormField::Company, FormField::Email] {
            let frame = app.frame(SIZE.0, SIZE.1);
            let Some(rect) = frame.rect_of(|t| *t == Target::Field(field)) else {
                panic!("{field:?} is not in the picture");
            };
            click(&mut app, rect.centre(), SIZE);
            assert_eq!(
                app.focus,
                Focus::Field(field),
                "pressing {field:?} gave the keyboard to {:?}",
                app.focus
            );
            type_str(&mut app, "Xy");
            assert_eq!(
                app.field_value(field),
                "Xy",
                "typing did not reach {field:?}"
            );
        }
    }

    #[test]
    fn tab_reaches_every_field_and_comes_back_to_the_first() {
        // The cycle was written when the form had two fields. It has fifteen,
        // and a cycle that stops short of the last is a field nothing can
        // type into.
        let mut app = wired();
        app.start_new_contact();
        let mut seen = vec![app.focus];
        for _ in 0..FormField::ALL.len() {
            app.key_at(&key_press(Key::Tab), SIZE);
            seen.push(app.focus);
        }
        for field in FormField::ALL {
            assert!(
                seen.contains(&Focus::Field(field)),
                "{} presses of Tab never reached {:?}",
                FormField::ALL.len(),
                field
            );
        }
        assert_eq!(
            seen.last().copied(),
            Some(Focus::Field(FormField::FirstName)),
            "Tab did not wrap back to the first field"
        );
    }

    #[test]
    fn save_writes_the_form_and_cancel_does_not() {
        let mut app = wired();
        let before = app.store.contact_count();
        app.start_new_contact();
        type_str(&mut app, "Zenobia");
        click_text(&mut app, SIZE, "Save");
        assert_eq!(
            app.store.contact_count(),
            before + 1,
            "Save did not add the contact"
        );
        let added = app
            .store
            .filtered_sorted(&ContactFilter::All, SortOrder::Name, "Zenobia")
            .first()
            .map(|c| c.id);
        assert_eq!(
            app.view,
            DetailView::ViewContact(added.expect("the saved contact is not in the store")),
            "Save left the panel somewhere other than the contact it just made"
        );

        app.start_new_contact();
        type_str(&mut app, "Discarded");
        click_text(&mut app, SIZE, "Cancel");
        assert_eq!(
            app.store.contact_count(),
            before + 1,
            "Cancel added the contact anyway"
        );
    }

    #[test]
    fn saving_an_edit_keeps_what_the_form_does_not_carry() {
        // The form has no groups, no favourite star and no created-at. A save
        // that wrote only what the form holds would silently strip all three
        // from a contact whose name was corrected.
        let mut app = wired();
        let id = first_listed(&app);
        let gid = app.store.add_group(ContactGroup::new(0, "Climbing"));
        assert!(app.store.add_contact_to_group(id, gid));
        // Toggle *to* a favourite, not blindly: the sample data already
        // stars some contacts, and toggling one of those turned the star off
        // -- so the test then proved a save preserves an empty flag, which
        // an assignment of `false` would also have passed.
        if !app.store.get_contact(id).is_some_and(|c| c.favorite) {
            assert!(app.store.toggle_favorite(id).is_some());
        }
        assert!(
            app.store.get_contact(id).is_some_and(|c| c.favorite),
            "the fixture failed to star the contact it is about to edit"
        );
        let created = app.store.get_contact(id).map(|c| c.created_at);

        app.select_contact(id);
        click_text(&mut app, SIZE, "Edit");
        *app.field_mut(FormField::FirstName) = String::from("Renamed");
        click_text(&mut app, SIZE, "Save");

        let saved = app.store.get_contact(id).expect("the contact was deleted");
        assert_eq!(saved.first_name, "Renamed", "the edit was not saved");
        assert!(saved.favorite, "saving cleared the favourite star");
        assert!(
            saved.groups.contains(&gid),
            "saving dropped the contact out of its group"
        );
        assert_eq!(
            Some(saved.created_at),
            created,
            "saving rewrote when the contact was created"
        );
    }

    #[test]
    fn the_merge_button_merges_the_pair_its_row_names() {
        let mut app = wired();
        let id = first_listed(&app);
        let name = app
            .store
            .get_contact(id)
            .map(Contact::computed_display_name)
            .expect("the contact exists");
        let (first, last) = {
            let c = app.store.get_contact(id).expect("the contact exists");
            (c.first_name.clone(), c.last_name.clone())
        };
        let twin = app.store.add_contact(Contact::new(0, &first, &last));
        let before = app.store.contact_count();

        app.activate(Target::ShowDuplicates);
        click_text(&mut app, SIZE, "Merge");
        assert_eq!(
            app.store.contact_count(),
            before - 1,
            "pressing Merge on the row for {name:?} merged nothing"
        );
        // A merge writes a *third* contact under a fresh id and retains
        // neither original, so "one of the two ids survives" is the wrong
        // question. What must survive is the person: their name is still in
        // the store, once, and it is what the panel is now showing.
        let surviving: Vec<u64> = app
            .store
            .filtered_sorted(&ContactFilter::All, SortOrder::Name, "")
            .iter()
            .filter(|c| c.computed_display_name() == name)
            .map(|c| c.id)
            .collect();
        assert_eq!(
            surviving.len(),
            1,
            "after merging the pair reading {name:?}, {} contacts have that name",
            surviving.len()
        );
        assert!(
            app.store.get_contact(id).is_none() && app.store.get_contact(twin).is_none(),
            "the merge left one of the halves behind alongside the merged contact"
        );
        // Merging a run of duplicates is one press per pair, so the panel
        // stays where the Merge buttons are.
        assert_eq!(
            app.view,
            DetailView::Duplicates,
            "the merge took the panel away from the duplicates it was working through"
        );
        // The status names the person, not the internal id of a contact the
        // user has never seen.
        assert!(
            app.status.contains(&name),
            "the status line says {:?}, which does not name {name:?}",
            app.status
        );
    }

    #[test]
    fn a_group_row_narrows_the_list_to_that_group() {
        let mut app = wired();
        app.activate(Target::ShowGroups);
        let (gid, gname, count) = app
            .store
            .group_stats()
            .into_iter()
            .find(|(_, _, n)| *n > 0)
            .expect("the sample data has a group with contacts in it");
        click_text(&mut app, SIZE, &gname);
        assert_eq!(app.filter, ContactFilter::Group(gid));
        assert_eq!(
            app.store
                .filtered_sorted(&app.filter, app.sort_order, "")
                .len(),
            count,
            "the list shows a different number of contacts than the group says it has"
        );
    }

    /// A second window size the layout genuinely solves differently.
    ///
    /// 600 is narrow enough that the sidebar is 45% of the window rather than
    /// its preferred 304 pixels, so every control on the sidebar sits
    /// somewhere else than it does at `SIZE`. A second size the layout
    /// happened to solve identically would let a program that ignored the
    /// size it was given pass every test below.
    const OTHER: (f32, f32) = (600.0, 640.0);

    #[test]
    fn the_picture_is_drawn_at_the_size_render_is_given() {
        // `render` is the only door the compositor draws through. Drawing a
        // constant-sized picture there would leave the window a border of
        // whatever was on the screen before it opened.
        let mut app = wired();
        let tree = App::render(&mut app, OTHER.0, OTHER.1);
        let want = app.frame(OTHER.0, OTHER.1).into_tree();
        assert_eq!(
            format!("{:?}", tree.commands),
            format!("{:?}", want.commands),
            "render drew something other than the picture for {OTHER:?}"
        );
    }

    #[test]
    fn a_press_is_answered_against_the_size_the_last_frame_was_drawn_at() {
        // The compositor calls `render(w, h)` and then hands over presses with
        // no size attached. Answering them against a constant answers them
        // against a window that is not on the screen.
        let mut app = wired();
        let _ = App::render(&mut app, OTHER.0, OTHER.1);
        let other = Layout::solve(OTHER.0, OTHER.1);
        // A letter on the A-Z rail, because the rail is pinned to the right
        // edge of the sidebar and the sidebar is a *fraction* of the window
        // at this width. A control on the left of the sidebar, such as a view
        // cell, keeps roughly the same coordinates at both sizes and would
        // let a program that ignored the size it was given pass anyway.
        let at = point_in(&app, OTHER, other.alphabet, "A");

        // The point has to be one the two sizes disagree about, or the test
        // would pass against a program that ignored the size entirely.
        let elsewhere = wired().frame(SIZE.0, SIZE.1).hit_test(at.0, at.1);
        assert!(
            !matches!(elsewhere, Some(Target::Letter('A'))),
            "{at:?} names the A on the rail at both sizes -- pick another control"
        );

        let response = App::on_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: at.0,
                y: at.1,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert!(matches!(response, Response::Redraw));
        assert_eq!(
            app.selected_letter,
            Some('A'),
            "the press was answered against some other window"
        );
    }

    #[test]
    fn a_resize_moves_where_the_controls_answer() {
        let mut app = wired();
        let _ = App::render(&mut app, SIZE.0, SIZE.1);
        let here = Layout::solve(SIZE.0, SIZE.1);
        // The A-Z rail again, and for the same reason as above: it is pinned
        // to the right edge of a sidebar that is a fraction of the window, so
        // it is somewhere genuinely different at the two sizes. A view cell
        // barely moves, and a press aimed at where it *would* be lands on it
        // either way -- which is how a program that ignored the resize
        // entirely passed this test as first written.
        let before = point_in(&app, SIZE, here.alphabet, "A");

        let response = App::on_event(
            &mut app,
            &Event::Resize {
                width: OTHER.0 as u32,
                height: OTHER.1 as u32,
            },
        );
        assert!(matches!(response, Response::Redraw));

        let other = Layout::solve(OTHER.0, OTHER.1);
        let after = point_in(&app, OTHER, other.alphabet, "A");
        assert!(
            (after.0 - before.0).abs() > 1.0 || (after.1 - before.1).abs() > 1.0,
            "the rail did not move when the window did"
        );

        // No `render` in between: the resize alone has to be enough, because
        // a press can arrive before the next frame is asked for.
        App::on_event(
            &mut app,
            &Event::Mouse(MouseEvent {
                x: after.0,
                y: after.1,
                kind: MouseEventKind::Press(MouseButton::Left),
            }),
        );
        assert_eq!(
            app.selected_letter,
            Some('A'),
            "the resize did not reach the hit boxes"
        );
    }

    #[test]
    fn the_close_button_closes_the_window_and_nothing_else_does() {
        let mut app = wired();
        assert!(matches!(
            App::on_event(&mut app, &Event::CloseRequested),
            Response::Exit
        ));
        for event in [
            Event::FocusIn,
            Event::FocusOut,
            Event::Tick { elapsed_ms: 16 },
        ] {
            assert!(
                matches!(App::on_event(&mut app, &event), Response::Redraw),
                "{event:?} was answered with something other than a redraw"
            );
        }
    }

    #[test]
    fn a_press_on_bare_background_changes_nothing() {
        let mut app = wired();
        app.select_contact(first_listed(&app));
        let frame = app.frame(SIZE.0, SIZE.1);
        // Somewhere in the detail panel that no control claims.
        let l = Layout::solve(SIZE.0, SIZE.1);
        let mut found = None;
        let mut y = l.panel.y + 2.0;
        while y < l.panel.bottom() {
            let at = (l.panel.right() - 2.0, y);
            if frame.hit_test(at.0, at.1).is_none() {
                found = Some(at);
                break;
            }
            y += 4.0;
        }
        let at = found.expect("every point in the panel answers to something");
        let before = (
            app.view.clone(),
            app.focus,
            app.filter.clone(),
            app.store.contact_count(),
        );
        click(&mut app, at, SIZE);
        assert_eq!(
            (
                app.view.clone(),
                app.focus,
                app.filter.clone(),
                app.store.contact_count()
            ),
            before,
            "a press on bare background at {at:?} changed the app"
        );
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

        fn fills(app: &mut ContactsApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = ContactsApp::new();

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

    // ------------------------------------------------------------------
    // The address book
    //
    // Nothing was kept: the book lived in memory and was gone when the
    // window closed, and the one way to keep anyone was a vCard export.
    // ------------------------------------------------------------------

    fn key_event(key: Key, ctrl: bool, text: &str) -> Event {
        let mut modifiers = guitk::event::Modifiers::NONE;
        modifiers.ctrl = ctrl;
        Event::Key(KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        })
    }

    fn press(app: &mut ContactsApp, key: Key) {
        app.handle_event(&key_event(key, false, ""), SIZE);
    }

    fn typed_in(app: &mut ContactsApp, text: &str) {
        for c in text.chars() {
            app.handle_event(&key_event(Key::Unknown(0), false, &c.to_string()), SIZE);
        }
    }

    fn drawn(app: &ContactsApp) -> Vec<String> {
        app.render()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    /// Text a contact is full of and a line-based file mangles.
    const AWKWARD: [&str; 8] = [
        "plain",
        "tab\there",
        "new\nline",
        "cr\rhere",
        "back\\slash",
        "\\n written out",
        "caf\u{e9} \u{1F4DE}",
        "ends in \\",
    ];

    /// A book with one of everything the file has to carry.
    fn full_book() -> ContactStore {
        let mut store = ContactStore::new();
        let friends = store.add_group(
            ContactGroup::new(0, AWKWARD[1])
                .with_color(Color::rgba(0x12, 0x34, 0x56, 0x78))
                .with_description(AWKWARD[2]),
        );
        let work =
            store.add_group(ContactGroup::new(0, "Work").with_color(Color::from_hex(0xA6E3A1)));
        for (i, awkward) in AWKWARD.iter().enumerate() {
            let mut c = Contact::new(0, awkward, AWKWARD[(i + 1) % AWKWARD.len()]);
            if i % 2 == 0 {
                c.display_name = format!("Dr. {awkward}");
            }
            c.nickname = (*awkward).to_owned();
            c.company = (*awkward).to_owned();
            c.job_title = (*awkward).to_owned();
            c.department = (*awkward).to_owned();
            c.notes = (*awkward).to_owned();
            c.favorite = i % 3 == 0;
            c.created_at = 1_000 + i as u64;
            c.updated_at = 2_000 + i as u64;
            c.last_contacted = (i % 2 == 1).then_some(3_000 + i as u64);
            c.birthday = (i % 2 == 0).then(|| SimpleDate::new(1990, 2, 28).unwrap());
            c.photo_path = match i % 3 {
                0 => None,
                1 => Some(String::new()),
                _ => Some((*awkward).to_owned()),
            };
            c.phones = vec![
                PhoneNumber::new(awkward, PhoneType::Mobile).with_primary(true),
                PhoneNumber::new("555", PhoneType::Fax),
            ];
            c.emails = vec![
                EmailAddress::new(awkward, EmailType::Work),
                EmailAddress::new("b@example.org", EmailType::Other).with_primary(true),
            ];
            let mut home = PostalAddress::new(AddressType::Home);
            home.street = (*awkward).to_owned();
            home.country = "NZ".to_owned();
            c.addresses = vec![home, PostalAddress::new(AddressType::Other)];
            c.social_accounts = vec![
                SocialAccount::new(SocialPlatform::Mastodon, awkward),
                SocialAccount::new(SocialPlatform::Custom((*awkward).to_owned()), "handle"),
            ];
            let id = store.add_contact(c);
            if i % 2 == 0 {
                assert!(store.add_contact_to_group(id, friends));
            }
            assert!(store.add_contact_to_group(id, work));
            store.record_view(id);
        }
        store
    }

    #[test]
    fn an_address_book_written_and_read_again_is_the_same_book() {
        let store = full_book();
        let text = book_text(&store);
        let back = parse_book(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(back.contacts, store.contacts);
        assert_eq!(back.groups, store.groups);
        assert_eq!(back.recently_viewed, store.recently_viewed);
        assert_eq!(back.next_contact_id, store.next_contact_id);
        assert_eq!(back.next_group_id, store.next_group_id);
        // No text broke a line: every line is one of the records.
        for line in text.lines().skip(1) {
            let kind = line.split('\t').next().unwrap();
            assert!(
                [
                    "group", "contact", "phone", "email", "address", "social", "member", "viewed"
                ]
                .contains(&kind),
                "{line:?}"
            );
        }
    }

    #[test]
    fn an_address_book_that_cannot_be_read_whole_is_refused_and_says_where() {
        let head = "slateos-contacts\t1";
        let group = "group\t1\t#112233\tFriends\t";
        let contact = "contact\t1\t0\t5\t6\t\t\tAda\tLovelace\tAda Lovelace\t\t\t\t\t\t";
        let cases: [(String, &str); 17] = [
            (String::new(), "not a SlateOS address book"),
            (String::from("slateos-contacts\t2"), "a later format (2)"),
            (
                String::from("slateos-contacts\tx"),
                "line 1 names no format",
            ),
            (
                format!("{head}\n{group}\n{group}"),
                "line 3: two groups have one number",
            ),
            (
                format!("{head}\ngroup\t1\tred\tFriends\t"),
                "line 2: a colour is not one",
            ),
            (
                format!("{head}\n{contact}\n{contact}"),
                "line 3: two contacts have one number",
            ),
            (
                format!("{head}\n{contact}\nmember\t9"),
                "line 3: a contact is in a group the book does not have",
            ),
            (
                format!("{head}\n{group}\n{contact}\nmember\t1\nmember\t1"),
                "line 5: a contact is in one group twice",
            ),
            (
                format!("{head}\nphone\tmobile\t1\t555"),
                "line 2: a phone number comes before any contact",
            ),
            (
                format!("{head}\n{contact}\nphone\tpager\t1\t555"),
                "line 3: a phone number is of a kind this version does not know",
            ),
            (
                format!("{head}\n{contact}\nsocial\tmyspace\t\tada"),
                "line 3: an account is on a service this version does not know",
            ),
            (
                format!("{head}\ncontact\t1\t0\t5\t6\t\t1990-02-30\tAda\tL\tAda L\t\t\t\t\t\t"),
                "line 2: a birthday is not a date",
            ),
            (
                format!("{head}\ncontact\t1\t0\t5\t6\t\t\tAda\tL\tAda L\t\t\t\t\tphoto.png\t"),
                "line 2: a photo is neither nothing nor a path",
            ),
            (
                format!("{head}\ncontact\t1\t2\t5\t6\t\t\tAda\tL\tAda L\t\t\t\t\t\t"),
                "line 2: a yes-or-no is neither 1 nor 0",
            ),
            (
                format!("{head}\n{contact}\nviewed\t1\t7"),
                "line 3: contact 7, recently viewed, is not in the book",
            ),
            (
                format!("{head}\ncontact\t1\t0\t5\t6\t\t\tA\\q\tL\tA L\t\t\t\t\t\t"),
                "line 2: a text holds an escape this program never writes",
            ),
            (
                format!("{head}\n{contact}\nfax\t555"),
                "line 3: it is not a line this version reads",
            ),
        ];
        for (text, want) in &cases {
            match parse_book(text) {
                Ok(_) => panic!("read {text:?}"),
                Err(why) => assert!(why.contains(want), "{text:?}: said {why:?}, not {want:?}"),
            }
        }
        // The control: the pieces above make a book that reads.
        let good =
            format!("{head}\n{group}\n{contact}\nphone\tmobile\t1\t555\nmember\t1\nviewed\t1\n");
        let read = parse_book(&good).unwrap();
        assert_eq!(read.contacts.len(), 1);
        assert_eq!(read.contacts[0].groups, [1]);
        assert_eq!(read.recently_viewed, [1]);
    }

    #[test]
    fn what_is_added_is_there_next_time() {
        settingsfile::testing::with_scratch_config("contacts-kept", |_| {
            let mut app = ContactsApp::from_settings();
            assert!(app.store_error.is_none(), "{:?}", app.store_error);
            let keeping = app.keeping_line();
            assert!(
                keeping.starts_with("Everyone added is kept in "),
                "{keeping}"
            );
            assert!(
                drawn(&app).contains(&keeping),
                "the empty window does not say where the book is kept"
            );

            press(&mut app, Key::N);
            typed_in(&mut app, "Ada");
            press(&mut app, Key::Tab);
            typed_in(&mut app, "Lovelace");
            press(&mut app, Key::Enter);
            let id = app.selected_id().expect("the new contact is not shown");
            // Written by the keys themselves, with no save asked for.
            let again = ContactsApp::from_settings();
            assert_eq!(again.store.contacts, app.store.contacts);
            let ada = &again.store.contacts[0];
            assert_eq!(ada.computed_display_name(), "Ada Lovelace");
            assert!(ada.created_at > 0, "a new contact was given no time");

            // A star, from the store, kept by the next event.
            app.store.toggle_favorite(id);
            press(&mut app, Key::Unknown(0));
            let again = ContactsApp::from_settings();
            assert!(again.store.contacts[0].favorite, "the star was not kept");
            assert_eq!(again.store.recently_viewed, app.store.recently_viewed);

            let mut again = again;
            let next = again.store.add_contact(Contact::new(0, "Next", ""));
            assert_ne!(next, id, "a new contact took a kept one's number");
        });
    }

    #[test]
    fn a_window_made_by_new_keeps_nothing() {
        settingsfile::testing::with_scratch_config("contacts-quiet", |dir| {
            let mut app = ContactsApp::new();
            press(&mut app, Key::N);
            typed_in(&mut app, "Scratch");
            press(&mut app, Key::Enter);
            assert_eq!(app.store.contacts.len(), 1);
            assert!(
                !dir.join("slateos").join("contacts").exists(),
                "a window made by new() wrote the user's address book"
            );
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn a_book_that_cannot_be_read_is_left_as_it_is() {
        settingsfile::testing::with_scratch_config("contacts-broken", |_| {
            let path = book_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let broken = "slateos-contacts\t1\ncontact\t1\t0\t5\t6\t\t\tAda\tL\tAda L\t\t\t\t\t\t\nphone\tpager\t1\t555\n";
            std::fs::write(&path, broken).unwrap();
            let mut app = ContactsApp::from_settings();
            let error = app
                .store_error
                .clone()
                .expect("an unreadable book was taken without a word");
            assert!(error.contains("line 3"), "{error}");
            assert!(drawn(&app).contains(&error), "the refusal is not on screen");
            assert!(
                app.store.contacts.is_empty(),
                "half a book was taken for the whole"
            );

            press(&mut app, Key::N);
            typed_in(&mut app, "New");
            press(&mut app, Key::Enter);
            assert_eq!(app.store.contacts.len(), 1);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                broken,
                "the unreadable book was saved over"
            );
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn a_book_too_big_to_read_whole_is_refused() {
        settingsfile::testing::with_scratch_config("contacts-big", |_| {
            let mut source = ContactStore::new();
            source.add_contact(Contact::new(0, &"x".repeat(400), ""));
            let text = book_text(&source);
            let path = book_path().unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();

            let mut app = ContactsApp::new();
            app.persist = true;
            app.load_book_within(&path, text.len() - 1);
            let error = app.store_error.clone().expect("a cut-short read was taken");
            assert!(error.contains("larger than"), "{error}");
            assert!(!app.persist, "a book read in part would be saved over");

            let mut whole = ContactsApp::new();
            whole.persist = true;
            whole.load_book_within(&path, text.len());
            assert!(whole.store_error.is_none(), "{:?}", whole.store_error);
            assert_eq!(
                whole.store.contacts, source.contacts,
                "control: the whole file reads"
            );
        });
    }

    #[test]
    fn a_save_that_fails_says_so_and_the_next_change_tries_again() {
        settingsfile::testing::with_scratch_config("contacts-refused", |_| {
            let mut app = ContactsApp::from_settings();
            let path = book_path().unwrap();
            // A directory where the file goes, so the write cannot land.
            std::fs::create_dir_all(&path).unwrap();
            press(&mut app, Key::N);
            typed_in(&mut app, "First");
            press(&mut app, Key::Enter);
            let error = app.store_error.clone().expect("a failed save said nothing");
            assert!(error.starts_with("Not saved to "), "{error}");
            assert!(drawn(&app).contains(&error), "the failure is not on screen");
            assert!(app.unkept());

            std::fs::remove_dir(&path).unwrap();
            app.handle_event(&key_event(Key::S, true, ""), SIZE);
            assert!(app.store_error.is_none(), "{:?}", app.store_error);
            assert!(!app.unkept());
            assert!(
                app.status.starts_with("Everyone added is kept in "),
                "{}",
                app.status
            );
            assert_eq!(
                ContactsApp::from_settings().store.contacts,
                app.store.contacts
            );
        });
    }

    #[test]
    fn closing_over_a_contact_being_edited_asks_and_save_keeps_it() {
        settingsfile::testing::with_scratch_config("contacts-close", |_| {
            let mut app = ContactsApp::from_settings();
            press(&mut app, Key::N);
            typed_in(&mut app, "Half");
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            let asked = app.question.as_ref().expect("no question");
            assert!(
                asked.message().contains("being edited"),
                "{}",
                asked.message()
            );
            // A key under the question reaches nothing.
            app.on_event(&key_event(Key::Unknown(0), false, "x"));
            assert_eq!(app.edit_first_name, "Half");
            // Cancel: the window stays, the form as it was.
            app.on_event(&key_event(Key::Escape, false, ""));
            assert!(app.question.is_none() && !app.quit);
            assert_eq!(app.edit_first_name, "Half");
            // Save: the contact is added, kept, and the window goes.
            app.on_event(&Event::CloseRequested);
            assert!(matches!(
                app.on_event(&key_event(Key::S, false, "s")),
                Response::Exit
            ));
            let again = ContactsApp::from_settings();
            assert_eq!(again.store.contacts.len(), 1);
            assert_eq!(again.store.contacts[0].first_name, "Half");
        });
    }

    #[test]
    fn closing_over_an_edit_can_leave_without_it() {
        settingsfile::testing::with_scratch_config("contacts-discard", |_| {
            let mut app = ContactsApp::from_settings();
            press(&mut app, Key::N);
            typed_in(&mut app, "Never");
            app.on_event(&Event::CloseRequested);
            assert!(matches!(
                app.on_event(&key_event(Key::D, false, "d")),
                Response::Exit
            ));
            assert!(ContactsApp::from_settings().store.contacts.is_empty());
            // And with nothing typed, nothing is asked.
            let mut idle = ContactsApp::from_settings();
            press(&mut idle, Key::N);
            assert!(matches!(
                idle.on_event(&Event::CloseRequested),
                Response::Exit
            ));
        });
    }

    #[test]
    fn closing_while_a_save_fails_asks_first() {
        settingsfile::testing::with_scratch_config("contacts-failing", |_| {
            let mut app = ContactsApp::from_settings();
            let path = book_path().unwrap();
            std::fs::create_dir_all(&path).unwrap();
            app.store.add_contact(Contact::new(0, "Unkept", ""));
            press(&mut app, Key::Unknown(0));
            assert!(app.unkept());
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            let asked = app.question.as_ref().expect("no question");
            assert!(asked.message().contains("not saved"), "{}", asked.message());
            // Save while the save still fails: the window stays.
            assert!(matches!(
                app.on_event(&key_event(Key::S, false, "s")),
                Response::Redraw
            ));
            assert!(!app.quit, "the window left while its save was failing");
            // Put right while the question is up, then Save: it goes.
            assert!(matches!(
                app.on_event(&Event::CloseRequested),
                Response::KeepOpen
            ));
            std::fs::remove_dir(&path).unwrap();
            assert!(matches!(
                app.on_event(&key_event(Key::S, false, "s")),
                Response::Exit
            ));
            assert_eq!(
                ContactsApp::from_settings().store.contacts,
                app.store.contacts
            );
        });
    }

    /// Saving an edit rebuilt the contact from the form, which shows the
    /// names and the first phone, email and address, and dropped the rest.
    #[test]
    fn editing_keeps_what_the_form_does_not_show() {
        let mut app = ContactsApp::new();
        let mut full = full_book().contacts[1].clone();
        full.display_name = String::from("Countess of Lovelace");
        full.groups.clear();
        let id = app.store.add_contact(full);
        let before = app.store.get_contact(id).unwrap().clone();
        app.view = DetailView::ViewContact(id);
        press(&mut app, Key::E);
        assert_eq!(app.view, DetailView::EditContact(id));
        // Change one thing: the first name.
        app.edit_first_name = String::from("Augusta");
        press(&mut app, Key::Enter);
        let after = app.store.get_contact(id).unwrap();
        assert_eq!(after.first_name, "Augusta");
        assert_eq!(after.phones, before.phones, "numbers were lost");
        assert_eq!(after.emails, before.emails, "email addresses were lost");
        assert_eq!(after.addresses, before.addresses, "addresses were lost");
        assert_eq!(
            after.social_accounts, before.social_accounts,
            "accounts were lost"
        );
        assert_eq!(after.photo_path, before.photo_path, "the photo was lost");
        assert_eq!(
            after.display_name, "Countess of Lovelace",
            "a name of its own was lost"
        );
        assert!(
            after.updated_at > before.updated_at,
            "an edit was given no time"
        );

        // A display name that was only the names follows them.
        let plain = app.store.add_contact(Contact::new(0, "Ada", "Byron"));
        app.view = DetailView::ViewContact(plain);
        press(&mut app, Key::E);
        app.edit_last_name = String::from("King");
        press(&mut app, Key::Enter);
        assert_eq!(
            app.store
                .get_contact(plain)
                .unwrap()
                .computed_display_name(),
            "Ada King"
        );

        // An emptied field removes what it showed, and the next moves up.
        app.view = DetailView::ViewContact(id);
        press(&mut app, Key::E);
        app.edit_phone.clear();
        press(&mut app, Key::Enter);
        assert_eq!(
            app.store.get_contact(id).unwrap().phones,
            before.phones[1..],
            "an emptied phone field did not remove the first number"
        );
    }

    #[test]
    fn a_form_saved_unchanged_is_no_change() {
        let mut app = ContactsApp::new();
        let id = app.store.add_contact(full_book().contacts[0].clone());
        app.view = DetailView::ViewContact(id);
        press(&mut app, Key::E);
        assert!(
            !app.form_has_changes(),
            "a form just opened says it has changes"
        );
        let revision = app.store.revision();
        let before = app.store.get_contact(id).unwrap().clone();
        press(&mut app, Key::Enter);
        assert_eq!(app.store.get_contact(id).unwrap(), &before);
        assert_eq!(
            app.store.revision(),
            revision,
            "an unchanged form counted as a change"
        );
        // A new contact with nothing typed has nothing to lose either.
        press(&mut app, Key::N);
        assert!(!app.form_has_changes());
        typed_in(&mut app, "A");
        assert!(app.form_has_changes());
    }

    #[test]
    fn every_change_to_the_store_is_counted() {
        type Change = fn(&mut ContactStore, u64, u64) -> bool;
        let changes: [(&str, Change); 13] = [
            ("a new contact", |s, _, _| {
                s.add_contact(Contact::new(0, "N", ""));
                true
            }),
            ("a contact handed out to change", |s, c, _| {
                s.get_contact_mut(c).is_some()
            }),
            ("a deleted contact", |s, c, _| s.delete_contact(c)),
            ("a replaced contact", |s, c, _| {
                let mut new = s.get_contact(c).unwrap().clone();
                new.notes = String::from("changed");
                s.update_contact(new)
            }),
            ("a new group", |s, _, _| {
                s.add_group(ContactGroup::new(0, "G"));
                true
            }),
            ("a group handed out to change", |s, _, g| {
                s.get_group_mut(g).is_some()
            }),
            ("a deleted group", |s, _, g| s.delete_group(g)),
            ("a membership", |s, c, _| {
                let other = s.add_group(ContactGroup::new(0, "H"));
                s.add_contact_to_group(c, other)
            }),
            ("a membership ended", |s, c, g| {
                s.remove_contact_from_group(c, g)
            }),
            ("a star", |s, c, _| s.toggle_favorite(c).is_some()),
            ("a view", |s, c, _| {
                s.record_view(c);
                true
            }),
            ("a call", |s, c, _| {
                s.mark_contacted(c, 99);
                true
            }),
            ("an import", |s, _, _| {
                s.import_vcards("BEGIN:VCARD\r\nFN:X\r\nN:X;;;;\r\nEND:VCARD\r\n", 1) == 1
            }),
        ];
        for (what, change) in changes {
            let mut store = ContactStore::new();
            let group = store.add_group(ContactGroup::new(0, "F"));
            let contact = store.add_contact(Contact::new(0, "Ada", ""));
            let other = store.add_contact(Contact::new(0, "Bob", ""));
            assert!(store.add_contact_to_group(contact, group));
            store.record_view(other);
            let before = store.revision();
            assert!(change(&mut store, contact, group), "{what} did not happen");
            assert_ne!(store.revision(), before, "{what} was not counted");
        }
        // And a merge.
        let mut store = ContactStore::new();
        let a = store.add_contact(Contact::new(0, "Ada", ""));
        let b = store.add_contact(Contact::new(0, "Ada", ""));
        let before = store.revision();
        assert!(store.merge_contacts(a, b).is_some());
        assert_ne!(store.revision(), before, "a merge was not counted");
    }

    #[test]
    fn a_contact_is_not_put_in_a_group_that_is_not_there() {
        let mut store = ContactStore::new();
        let id = store.add_contact(Contact::new(0, "Ada", ""));
        assert!(!store.add_contact_to_group(id, 42));
        assert!(store.get_contact(id).unwrap().groups.is_empty());
        assert!(parse_book(&book_text(&store)).is_ok());
    }

    #[test]
    fn a_call_made_now_is_later_than_every_kept_time() {
        let late = 4_000_000_000_000_u64;
        let text = format!(
            "slateos-contacts\t1\ncontact\t1\t0\t5\t{late}\t\t\tAda\tL\tAda L\t\t\t\t\t\t\n"
        );
        let mut app = ContactsApp::new();
        app.take_store(parse_book(&text).unwrap());
        app.view = DetailView::ViewContact(1);
        app.quick_action(QuickAction::Call);
        let reached = app.store.get_contact(1).unwrap().last_contacted.unwrap();
        assert!(
            reached > late,
            "a call now was stamped {reached}, before a kept time"
        );
        let before = clock_ms();
        let first = app.stamp();
        assert!(first >= before && app.stamp() > first);
    }

    #[test]
    fn ctrl_s_says_where_the_book_is_kept() {
        settingsfile::testing::with_scratch_config("contacts-ctrl-s", |_| {
            let mut app = ContactsApp::from_settings();
            app.store.add_contact(Contact::new(0, "Ada", ""));
            app.handle_event(&key_event(Key::S, true, ""), SIZE);
            assert!(!app.unkept(), "Ctrl+S kept nothing");
            assert!(
                app.status.starts_with("Everyone added is kept in "),
                "{}",
                app.status
            );
            assert!(book_path().unwrap().is_file());
        });
        let mut app = ContactsApp::new();
        app.handle_event(&key_event(Key::S, true, ""), SIZE);
        assert_eq!(app.status, "Nothing added here is kept.");
        // Export moved to Ctrl+E.
        app.handle_event(&key_event(Key::E, true, ""), SIZE);
        assert!(app.picker.is_open() && app.picker.is_saving());
    }
}
