//! How the desktop tells the time: which zone its clock is in, what the
//! taskbar clock shows, and the other clocks beside it.
//!
//! The desktop shell draws the clocks; the Settings application is where the
//! user changes them. A preference edited in one process and obeyed in another
//! needs one definition -- the reason `notifsettings` and `inputsettings` exist
//! -- and until this crate there was not even one: the model lived inside the
//! shell with no file behind it, so nothing chosen about the clock outlasted
//! the session, and the one panel that could choose was never on screen. See
//! `design-decisions.md` §875.
//!
//! # The zone
//!
//! [`DateTimeSettings::zone`] is `None` unless the user chose one, and then the
//! clock shows the machine's own zone -- [`system_zone`], read the way the libc
//! reads it (`tzrules::tz_source`), so that the taskbar and `date` agree about
//! the time. A zone the user *chooses* is one of [`zones`], each of which
//! carries its own POSIX rule and so needs no tzdata installed.
//!
//! # Layout
//!
//! `datetime.yaml` in the user's configuration directory, per [`settingsfile`]:
//!
//! ```yaml
//! zone: Europe/Paris      # empty: the machine's own zone
//! taskbar:
//!   seconds: false
//!   weekday: true
//!   date: true
//! clocks:                 # up to four, shown in the calendar
//!   Tokyo:
//!     zone: Asia/Tokyo
//!     visible: true
//! ```
//!
//! A clock is filed under its label because the label is what a person reads
//! and edits; two clocks with one label could not be told apart on screen
//! either, so [`DateTimeSettings::add_clock`] refuses a second.
//!
//! # What is not here
//!
//! **Network time.** `userspace/ntpd` chooses its own servers and polling, as
//! the protocol requires; a preference nothing obeys is not written down (the
//! rule `design-decisions.md` §856 draws from every setting that was stored and
//! never obeyed). **Detecting the zone.** Nothing on the machine can yet.

#![deny(clippy::all, clippy::pedantic)]

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub use tzrules::Tz;
use tzrules::{TzFile, TzSource};
use yamldoc::Document;

// ============================================================================
// The zones a user can choose
// ============================================================================

/// A zone a user can choose: an IANA name, how it is shown, and the POSIX `TZ`
/// rule that says what its clock reads.
///
/// The rule is the point. A zone that observes daylight saving has *two*
/// offsets and a rule choosing between them; an entry that stored one number
/// (this used to store `utc_offset_min: i32`) is wrong for whichever half of
/// the year it is not describing -- for Eastern Time, wrong for the roughly
/// eight months of EDT, and wrong *silently*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimezoneInfo {
    /// The IANA name (`America/New_York`): what the file stores.
    pub tz_id: String,
    /// The name shown ("Eastern Time").
    pub display_name: String,
    /// A city in it, for search and for a clock's label.
    pub city: String,
    /// The POSIX `TZ` rule this entry reads with.
    pub rule: Tz,
}

impl TimezoneInfo {
    /// Build an entry from a POSIX `TZ` string.
    ///
    /// `None` if the string is not a rule. The table is literals, so a
    /// malformed one is a typo, and the place to catch a typo is
    /// `the_table_has_every_zone` noticing the table came up short -- not a
    /// fault in a running desktop.
    #[must_use]
    pub fn new(
        tz_id: impl Into<String>,
        display_name: impl Into<String>,
        posix_tz: &str,
        city: impl Into<String>,
    ) -> Option<Self> {
        Some(Self {
            tz_id: tz_id.into(),
            display_name: display_name.into(),
            city: city.into(),
            rule: Tz::parse(posix_tz.as_bytes())?,
        })
    }

    /// The offset from UTC, in seconds, in force at `utc_secs`.
    #[must_use]
    pub fn offset_secs_at(&self, utc_secs: u64) -> i32 {
        self.rule.lookup(clamp_to_i64(utc_secs)).gmtoff
    }

    /// The offset in force at `utc_secs`, as "UTC-05:00" in January and
    /// "UTC-04:00" in July for the same New York entry.
    #[must_use]
    pub fn offset_string(&self, utc_secs: u64) -> String {
        let secs = self.offset_secs_at(utc_secs);
        let mins = secs.div_euclid(60);
        let hours = mins / 60;
        let rem = (mins % 60).unsigned_abs();
        if mins >= 0 {
            format!("UTC+{hours:02}:{rem:02}")
        } else {
            format!("UTC-{:02}:{rem:02}", hours.unsigned_abs())
        }
    }

    /// The zone's abbreviation in force at `utc_secs` ("EST" or "EDT").
    ///
    /// Byte for character: the rule grammar admits only alphanumerics, `+` and
    /// `-` in a name (`Tz::parse` refuses the rest), so every byte is ASCII
    /// and the mapping is exact -- no decoding step that could lose anything.
    #[must_use]
    pub fn abbrev_at(&self, utc_secs: u64) -> String {
        let info = self.rule.lookup(clamp_to_i64(utc_secs));
        info.name
            .as_bytes()
            .iter()
            .copied()
            .map(char::from)
            .collect()
    }

    /// Whether this zone observes daylight saving *at all*.
    #[must_use]
    pub fn observes_dst(&self) -> bool {
        self.rule.has_dst()
    }

    /// Whether daylight saving is in force at `utc_secs`: the live fact a zone
    /// picker should show, where [`observes_dst`](Self::observes_dst) is the
    /// standing property.
    #[must_use]
    pub fn is_dst_at(&self, utc_secs: u64) -> bool {
        self.rule.lookup(clamp_to_i64(utc_secs)).is_dst
    }

    /// The local hour and minute at `utc_secs`, on a 24-hour clock.
    #[must_use]
    pub fn local_time(&self, utc_secs: u64) -> (u32, u32) {
        let t = clamp_to_i64(utc_secs);
        let local = t.saturating_add(i64::from(self.rule.lookup(t).gmtoff));
        let day_secs = local.rem_euclid(86_400);
        // 0..86_400 by construction, so both conversions are exact.
        let hour = u32::try_from(day_secs / 3600).unwrap_or(0);
        let minute = u32::try_from((day_secs % 3600) / 60).unwrap_or(0);
        (hour, minute)
    }
}

/// Timestamps arrive as `u64`; `tzrules` speaks `i64`. Saturating rather than
/// wrapping, so an absurd clock reading stays absurd in the same direction
/// instead of becoming a plausible date in 1901.
fn clamp_to_i64(utc_secs: u64) -> i64 {
    i64::try_from(utc_secs).unwrap_or(i64::MAX)
}

/// Every zone a user can choose, in the order a picker lists them: UTC first,
/// then west to east.
///
/// Each rule is the POSIX `TZ` string tzdata publishes for the zone, so the
/// offsets and transition dates are the real ones rather than a snapshot of
/// whatever was in force the day the table was written. Two entries a
/// fixed-offset table got wrong: São Paulo abolished DST in 2019 and is now a
/// plain `-03`, and Sydney and Auckland are southern-hemisphere, so their DST
/// window straddles New Year.
#[must_use]
pub fn zones() -> &'static [TimezoneInfo] {
    &ZONES
}

static ZONES: LazyLock<Vec<TimezoneInfo>> = LazyLock::new(|| {
    // `flatten` drops an entry whose rule does not parse. That can only be a
    // typo in the literals, and `the_table_has_every_zone` fails when it
    // happens.
    [
        // UTC first, and by that name: nobody administering a server or
        // comparing a timestamp with a colleague goes looking for Iceland.
        TimezoneInfo::new("UTC", "Coordinated Universal Time", "UTC0", "UTC"),
        TimezoneInfo::new("Pacific/Honolulu", "Hawaii", "HST10", "Honolulu"),
        TimezoneInfo::new(
            "America/Anchorage",
            "Alaska",
            "AKST9AKDT,M3.2.0,M11.1.0",
            "Anchorage",
        ),
        TimezoneInfo::new(
            "America/Los_Angeles",
            "Pacific Time",
            "PST8PDT,M3.2.0,M11.1.0",
            "Los Angeles",
        ),
        TimezoneInfo::new(
            "America/Denver",
            "Mountain Time",
            "MST7MDT,M3.2.0,M11.1.0",
            "Denver",
        ),
        TimezoneInfo::new(
            "America/Chicago",
            "Central Time",
            "CST6CDT,M3.2.0,M11.1.0",
            "Chicago",
        ),
        TimezoneInfo::new(
            "America/New_York",
            "Eastern Time",
            "EST5EDT,M3.2.0,M11.1.0",
            "New York",
        ),
        TimezoneInfo::new(
            "America/Sao_Paulo",
            "Brasilia Time",
            "<-03>3",
            "S\u{00e3}o Paulo",
        ),
        TimezoneInfo::new("Atlantic/Reykjavik", "Iceland", "GMT0", "Reykjavik"),
        TimezoneInfo::new(
            "Europe/London",
            "GMT/BST",
            "GMT0BST,M3.5.0/1,M10.5.0",
            "London",
        ),
        TimezoneInfo::new(
            "Europe/Paris",
            "Central European",
            "CET-1CEST,M3.5.0,M10.5.0/3",
            "Paris",
        ),
        TimezoneInfo::new(
            "Europe/Berlin",
            "Central European",
            "CET-1CEST,M3.5.0,M10.5.0/3",
            "Berlin",
        ),
        TimezoneInfo::new(
            "Europe/Helsinki",
            "Eastern European",
            "EET-2EEST,M3.5.0/3,M10.5.0/4",
            "Helsinki",
        ),
        TimezoneInfo::new("Europe/Moscow", "Moscow Time", "MSK-3", "Moscow"),
        TimezoneInfo::new("Asia/Dubai", "Gulf Standard", "<+04>-4", "Dubai"),
        TimezoneInfo::new("Asia/Kolkata", "India Standard", "IST-5:30", "Mumbai"),
        TimezoneInfo::new("Asia/Shanghai", "China Standard", "CST-8", "Shanghai"),
        TimezoneInfo::new("Asia/Tokyo", "Japan Standard", "JST-9", "Tokyo"),
        TimezoneInfo::new("Asia/Seoul", "Korea Standard", "KST-9", "Seoul"),
        TimezoneInfo::new(
            "Australia/Sydney",
            "Australian Eastern",
            "AEST-10AEDT,M10.1.0,M4.1.0/3",
            "Sydney",
        ),
        TimezoneInfo::new(
            "Pacific/Auckland",
            "New Zealand",
            "NZST-12NZDT,M9.5.0,M4.1.0/3",
            "Auckland",
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
});

/// The zone named `tz_id`, if a user can choose it.
#[must_use]
pub fn zone(tz_id: &str) -> Option<&'static TimezoneInfo> {
    zones().iter().find(|z| z.tz_id == tz_id)
}

/// The zones whose name, description or city contains `query`, ignoring
/// case, in the table's order.
#[must_use]
pub fn search_zones(query: &str) -> Vec<&'static TimezoneInfo> {
    let q = query.to_lowercase();
    zones()
        .iter()
        .filter(|z| {
            z.tz_id.to_lowercase().contains(&q)
                || z.display_name.to_lowercase().contains(&q)
                || z.city.to_lowercase().contains(&q)
        })
        .collect()
}

// ============================================================================
// The clock
// ============================================================================

/// The desktop's wall clock: seconds since the epoch.
///
/// One function for every decision the desktop dates against the wall clock --
/// the taskbar clock, quiet hours, the automatic light/dark mode -- behind a
/// seam a test can fix. Without the seam, a test of anything scheduled by the
/// time of day passes by day and fails by night.
// `missing_const_for_thread_local` fires on initializers that already are
// `const` blocks on rustc/clippy 1.95 -- a false positive with no true
// positives, measured and written up in the workspace `Cargo.toml`. That allow
// does not reach a crate that denies `clippy::all` in its source, as this one
// does; and an allow on the `thread_local!` invocation does not reach the item
// the macro generates, hence the module. Remove both when the toolchain moves
// on.
#[allow(clippy::missing_const_for_thread_local)]
pub mod clock {
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(any(test, feature = "testing"))]
    thread_local! {
        static FIXED: core::cell::Cell<Option<u64>> = const { core::cell::Cell::new(None) };
    }

    /// Seconds since the epoch: this thread's fixed time, if a test has set
    /// one ([`with_time`]), else the system clock. A clock set before the
    /// epoch -- one that is badly wrong -- reads as the epoch.
    #[must_use]
    pub fn now_utc_secs() -> u64 {
        #[cfg(any(test, feature = "testing"))]
        if let Some(fixed) = FIXED.with(core::cell::Cell::get) {
            return fixed;
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    /// Run `body` with this thread's clock reading `utc_secs`, restored on
    /// every way out -- a failing assertion included, which is the usual way
    /// a test ends badly.
    #[cfg(any(test, feature = "testing"))]
    pub fn with_time<T>(utc_secs: u64, body: impl FnOnce() -> T) -> T {
        struct Restore(Option<u64>);
        impl Drop for Restore {
            fn drop(&mut self) {
                let previous = self.0;
                FIXED.with(|f| f.set(previous));
            }
        }
        let _restore = Restore(FIXED.with(|f| f.replace(Some(utc_secs))));
        body()
    }
}

// ============================================================================
// The machine's zone
// ============================================================================

/// The largest zoneinfo file read. tzdata's largest is under 4 KiB; the libc
/// caps at a 16 KiB page, and a file larger than that is refused rather than
/// truncated -- half a transition table would render confidently wrong times.
pub const MAX_ZONEINFO_BYTES: u64 = 16 * 1024;

/// The zone this machine is in, read the way the libc reads it: `TZ` if the
/// environment sets it, else `/etc/localtime`, else UTC
/// (`tzrules::tz_source` has the whole order). So the desktop's clock and
/// `date` agree.
///
/// Reads the environment and possibly a file, so it is asked when the
/// settings are read, not per frame.
#[must_use]
pub fn system_zone() -> Tz {
    let tz = env::var_os("TZ");
    let tzdir = env::var_os("TZDIR");
    resolve_zone(
        tzrules::tz_source(tz.as_deref().map(OsStr::as_encoded_bytes)),
        tzdir.as_deref().map(Path::new),
        read_zoneinfo,
    )
}

/// What `source` resolves to, reading any file through `read` -- the testable
/// core of [`system_zone`].
///
/// A file is read as the rule in force from its last recorded transition on
/// (its *tail*, `TzFile::tail`): exact for every date from then to the far
/// future, which is every date a desktop clock shows. Anything that cannot
/// be read, parsed or resolved is UTC, as it is for the libc.
pub fn resolve_zone(
    source: TzSource<'_>,
    tzdir: Option<&Path>,
    mut read: impl FnMut(&Path) -> Option<Vec<u8>>,
) -> Tz {
    let path = match source {
        TzSource::Utc | TzSource::Refused => return Tz::utc(),
        TzSource::Rule(rule) => return rule,
        TzSource::Path(path) => path_from_bytes(path),
        TzSource::System => path_from_bytes(tzrules::LOCALTIME),
        TzSource::Named(name) => {
            // `TZDIR` is the glibc spelling for an alternate tree; an empty one
            // is no tree at all, as the libc treats it.
            let dir = tzdir
                .filter(|d| !d.as_os_str().is_empty())
                .map_or_else(|| path_from_bytes(tzrules::ZONEINFO_DIR), Path::to_path_buf);
            dir.join(path_from_bytes(name))
        }
    };
    read(&path)
        .as_deref()
        .and_then(TzFile::parse)
        .and_then(|file| file.tail())
        .unwrap_or_else(Tz::utc)
}

/// A path from the bytes `tzrules` deals in: exact on the target, where a
/// path is bytes (`pathcodec` states the development host's limits).
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(pathcodec::os_string_from_bytes(bytes.to_vec()))
}

/// Read a zoneinfo file within [`MAX_ZONEINFO_BYTES`].
fn read_zoneinfo(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    // `ok()` because every failure means the same thing here -- no zone from
    // this file, so UTC -- and there is no one to report it to: a clock that
    // could not read `/etc/localtime` still has to show a time.
    file.take(MAX_ZONEINFO_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (u64::try_from(bytes.len()).unwrap_or(u64::MAX) <= MAX_ZONEINFO_BYTES).then_some(bytes)
}

// ============================================================================
// The settings
// ============================================================================

/// Most clocks shown beside the main one. Four fit the calendar's header band
/// at the largest scale; a fifth would push the month grid down for a reading
/// most people glance at once a day.
pub const MAX_CLOCKS: usize = 4;

/// A clock for another zone, shown beside the main one in the calendar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdditionalClock {
    /// The zone it reads, by IANA name -- one of [`zones`] when chosen through
    /// [`DateTimeSettings::add_clock`]. One that is not (a hand-edited file) is
    /// kept, so saving does not lose it, and not shown, since a clock at the
    /// wrong offset under a right label is worse than none.
    pub tz_id: String,
    /// What it is called on screen, and its key in the file.
    pub label: String,
    /// Whether it is shown. A hidden clock keeps its place and its label, for
    /// someone who wants Tokyo's time during the Tokyo project and not after.
    pub visible: bool,
}

impl AdditionalClock {
    /// A shown clock for `tz_id`, called `label`.
    pub fn new(tz_id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            tz_id: tz_id.into(),
            label: label.into(),
            visible: true,
        }
    }
}

/// How the desktop tells the time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTimeSettings {
    /// The zone the clock is in, by IANA name; `None` for the machine's own
    /// ([`system_zone`]).
    ///
    /// A name that is not one of [`zones`] -- a file edited by hand, or a
    /// zone a later version dropped -- is kept as written, so saving does not
    /// quietly lose it, and read as `None` is: the machine's zone, not an
    /// offset invented for a name nothing here knows.
    pub zone: Option<String>,
    /// Whether the taskbar clock shows seconds.
    pub show_seconds: bool,
    /// Whether the taskbar clock shows the day of the week.
    pub show_day_of_week: bool,
    /// Whether the taskbar clock shows the date.
    pub show_date: bool,
    /// Other zones' clocks, in the order they are shown; at most
    /// [`MAX_CLOCKS`].
    pub additional_clocks: Vec<AdditionalClock>,
}

impl Default for DateTimeSettings {
    fn default() -> Self {
        Self {
            // The machine's zone. This was `America/New_York` for as long as
            // there was no file to change it in: everyone outside one zone saw
            // the wrong time, and nothing on screen said whose time it was.
            zone: None,
            show_seconds: false,
            show_day_of_week: true,
            show_date: true,
            additional_clocks: Vec::new(),
        }
    }
}

impl DateTimeSettings {
    /// The chosen zone, if one was chosen and a user can choose it.
    #[must_use]
    pub fn current_zone(&self) -> Option<&'static TimezoneInfo> {
        self.zone.as_deref().and_then(zone)
    }

    /// Choose the zone: `Some(name)` for one of [`zones`], `None` for the
    /// machine's own. Answers whether it was taken -- a name that is not in
    /// the table is refused, leaving the zone as it was.
    pub fn set_zone(&mut self, tz_id: Option<&str>) -> bool {
        match tz_id {
            None => {
                self.zone = None;
                true
            }
            Some(id) if zone(id).is_some() => {
                self.zone = Some(id.to_owned());
                true
            }
            Some(_) => false,
        }
    }

    /// The rule the clock reads with: the chosen zone's, or `system` -- the
    /// machine's, which the caller read once ([`system_zone`]) rather than
    /// this re-reading a file per question.
    #[must_use]
    pub fn rule(&self, system: Tz) -> Tz {
        self.current_zone().map_or(system, |z| z.rule)
    }

    /// Add a clock for `tz_id` called `label`. Refused -- answering `false`
    /// -- when there are already [`MAX_CLOCKS`], when `tz_id` is not a zone a
    /// user can choose, or when a clock already has the label (the file keys
    /// clocks by label, and two clocks with one name could not be told apart
    /// on screen either). A label is trimmed, and must not be empty.
    pub fn add_clock(&mut self, tz_id: &str, label: &str) -> bool {
        let label = label.trim();
        if self.additional_clocks.len() >= MAX_CLOCKS
            || zone(tz_id).is_none()
            || label.is_empty()
            || self.additional_clocks.iter().any(|c| c.label == label)
        {
            return false;
        }
        self.additional_clocks
            .push(AdditionalClock::new(tz_id, label));
        true
    }

    /// Remove the clock at `index`, answering whether there was one.
    pub fn remove_clock(&mut self, index: usize) -> bool {
        if index < self.additional_clocks.len() {
            self.additional_clocks.remove(index);
            true
        } else {
            false
        }
    }

    /// Read the settings from `datetime.yaml`, taking the default for
    /// anything absent or unreadable -- the rule every settings reader here
    /// follows: one mistyped value costs that value, not the file.
    #[must_use]
    pub fn read_from(doc: &Document) -> Self {
        let mut s = Self::default();
        if let Some(name) = doc.get_str(&["zone"]) {
            let name = name.trim();
            // Empty is the machine's zone, written as a key so it can be seen
            // and edited.
            s.zone = (!name.is_empty()).then(|| name.to_owned());
        }
        if let Some(on) = doc.get_bool(&["taskbar", "seconds"]) {
            s.show_seconds = on;
        }
        if let Some(on) = doc.get_bool(&["taskbar", "weekday"]) {
            s.show_day_of_week = on;
        }
        if let Some(on) = doc.get_bool(&["taskbar", "date"]) {
            s.show_date = on;
        }
        for label in doc.keys(&["clocks"]) {
            if s.additional_clocks.len() >= MAX_CLOCKS {
                break;
            }
            // A clock with no zone reads nothing; skipped rather than guessed.
            let Some(tz_id) = doc.get_str(&["clocks", &label, "zone"]) else {
                continue;
            };
            let tz_id = tz_id.trim();
            let label = label.trim();
            if tz_id.is_empty()
                || label.is_empty()
                || s.additional_clocks.iter().any(|c| c.label == label)
            {
                continue;
            }
            s.additional_clocks.push(AdditionalClock {
                tz_id: tz_id.to_owned(),
                label: label.to_owned(),
                visible: doc.get_bool(&["clocks", label, "visible"]).unwrap_or(true),
            });
        }
        s
    }

    /// Write the settings into `datetime.yaml`, leaving every comment, blank
    /// line and unrelated key in it as it was.
    pub fn write_into(&self, doc: &mut Document) {
        doc.set_str(&["zone"], self.zone.as_deref().unwrap_or(""));
        doc.set_bool(&["taskbar", "seconds"], self.show_seconds);
        doc.set_bool(&["taskbar", "weekday"], self.show_day_of_week);
        doc.set_bool(&["taskbar", "date"], self.show_date);

        // The clocks are a list whose order is the order they are shown, kept
        // as a map keyed by label. When the file already lists the same
        // labels in the same order, each entry is updated where it stands,
        // keeping any comment beside it; otherwise the block is written anew,
        // because a map can only be put in order by writing it in order.
        let labels: Vec<&str> = self
            .additional_clocks
            .iter()
            .map(|c| c.label.as_str())
            .collect();
        if doc.keys(&["clocks"]) != labels {
            doc.remove(&["clocks"]);
        }
        for clock in &self.additional_clocks {
            doc.set_str(&["clocks", &clock.label, "zone"], &clock.tz_id);
            doc.set_bool(&["clocks", &clock.label, "visible"], clock.visible);
        }
    }
}

// ============================================================================
// The file
// ============================================================================

/// The settings group's name: `datetime.yaml`.
pub const CONFIG_NAME: &str = "datetime";

/// The date and time settings together with the document they were read
/// from, so that saving one changed value splices it into the user's file
/// instead of replacing the file.
#[derive(Clone, Debug)]
pub struct DateTimeFile {
    /// The settings, to read and change.
    pub settings: DateTimeSettings,
    doc: Document,
}

impl DateTimeFile {
    /// Read the user's saved settings. A missing or unreadable file gives the
    /// defaults: the ordinary state of a machine where nobody has changed the
    /// clock, not an error.
    #[must_use]
    pub fn load() -> Self {
        Self::from_document(settingsfile::load(CONFIG_NAME))
    }

    /// Open on an already-read document, so the format can be exercised
    /// without a filesystem.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        Self {
            settings: DateTimeSettings::read_from(&doc),
            doc,
        }
    }

    /// The file's text as it would be saved now.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut doc = self.doc.clone();
        self.settings.write_into(&mut doc);
        doc.to_text()
    }

    /// Write the settings to `datetime.yaml`, atomically.
    ///
    /// # Errors
    ///
    /// If there is no configuration directory, or the file cannot be written.
    pub fn save(&mut self) -> std::io::Result<()> {
        self.settings.write_into(&mut self.doc);
        settingsfile::store(CONFIG_NAME, &self.doc)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// 2024-01-15 12:00:00 UTC -- northern winter, southern summer.
    const JAN: u64 = 1_705_320_000;
    /// 2024-07-15 12:00:00 UTC -- northern summer, southern winter.
    const JUL: u64 = 1_721_044_800;

    /// An entry in the shipped table, so every assertion is about a rule a
    /// user can actually choose.
    fn shipped(tz_id: &str) -> &'static TimezoneInfo {
        zone(tz_id).unwrap_or_else(|| panic!("{tz_id} should be in the table"))
    }

    // ---- the clock ----

    /// A fixed time is the time on this thread until the body returns, and
    /// the clock is itself again after -- a failing assertion included.
    #[test]
    fn a_fixed_time_holds_for_its_body_and_no_longer() {
        let real = clock::now_utc_secs();
        assert!(real > 1_700_000_000, "the real clock reads a real time");
        let inside = clock::with_time(42, clock::now_utc_secs);
        assert_eq!(inside, 42);
        assert_ne!(clock::now_utc_secs(), 42);
        let unwound = std::panic::catch_unwind(|| {
            clock::with_time(7, || panic!("an assertion failed"));
        });
        assert!(unwound.is_err());
        assert_ne!(
            clock::now_utc_secs(),
            7,
            "a panic did not leave the time fixed"
        );
        // Nested, the inner time wins and the outer comes back.
        clock::with_time(1, || {
            clock::with_time(2, || assert_eq!(clock::now_utc_secs(), 2));
            assert_eq!(clock::now_utc_secs(), 1);
        });
    }

    // ---- the table ----

    /// Also the guard on the rule strings: an entry whose POSIX `TZ` fails to
    /// parse is dropped by `flatten`, so a typo shows up here.
    #[test]
    fn the_table_has_every_zone() {
        assert_eq!(zones().len(), 21);
        assert_eq!(zones()[0].tz_id, "UTC", "UTC is listed first, by name");
        let utc = shipped("UTC");
        assert_eq!(utc.offset_secs_at(JAN), 0);
        assert_eq!(utc.offset_secs_at(JUL), 0, "UTC does not observe DST");
        let mut ids: Vec<&str> = zones().iter().map(|z| z.tz_id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), zones().len(), "two entries share a name");
    }

    #[test]
    fn offsets_are_written_as_people_read_them() {
        assert_eq!(shipped("Asia/Tokyo").offset_string(JAN), "UTC+09:00");
        assert_eq!(shipped("America/New_York").offset_string(JAN), "UTC-05:00");
        assert_eq!(shipped("Asia/Kolkata").offset_string(JUL), "UTC+05:30");
        assert_eq!(
            shipped("Atlantic/Reykjavik").offset_string(JUL),
            "UTC+00:00"
        );
    }

    #[test]
    fn local_time_follows_the_offset() {
        assert_eq!(shipped("Atlantic/Reykjavik").local_time(43_200), (12, 0));
        assert_eq!(shipped("Asia/Tokyo").local_time(0), (9, 0));
        // 03:00 UTC on a January day is 22:00 the day before in New York.
        assert_eq!(
            shipped("America/New_York").local_time(JAN - 32_400),
            (22, 0)
        );
    }

    /// The bug the table used to have: one stored offset is right for at most
    /// half the year in a zone that observes daylight saving.
    #[test]
    fn a_dst_zone_reads_differently_in_january_and_july() {
        let ny = shipped("America/New_York");
        assert_eq!(ny.offset_string(JAN), "UTC-05:00");
        assert_eq!(ny.offset_string(JUL), "UTC-04:00");
        assert_eq!(ny.abbrev_at(JAN), "EST");
        assert_eq!(ny.abbrev_at(JUL), "EDT");
        assert_eq!(ny.local_time(JAN), (7, 0));
        assert_eq!(ny.local_time(JUL), (8, 0));
        assert!(!ny.is_dst_at(JAN));
        assert!(ny.is_dst_at(JUL));
        assert!(ny.observes_dst());
    }

    /// Sydney's daylight saving straddles New Year, so it is shifted in
    /// exactly the months New York is not.
    #[test]
    fn a_southern_zone_is_shifted_in_january_not_july() {
        let sydney = shipped("Australia/Sydney");
        assert_eq!(sydney.offset_string(JAN), "UTC+11:00");
        assert_eq!(sydney.offset_string(JUL), "UTC+10:00");
        assert_eq!(sydney.abbrev_at(JAN), "AEDT");
        assert!(sydney.is_dst_at(JAN));
        assert!(!sydney.is_dst_at(JUL));
    }

    /// A zone with no DST name gets no DST from the default US rules.
    #[test]
    fn fixed_offset_zones_never_shift() {
        for (tz_id, offset) in [
            ("Pacific/Honolulu", "UTC-10:00"),
            ("Europe/Moscow", "UTC+03:00"),
            ("Asia/Dubai", "UTC+04:00"),
            ("Asia/Shanghai", "UTC+08:00"),
            ("Asia/Seoul", "UTC+09:00"),
            // Brazil abolished daylight saving in 2019.
            ("America/Sao_Paulo", "UTC-03:00"),
        ] {
            let tz = shipped(tz_id);
            assert!(!tz.observes_dst(), "{tz_id}");
            assert_eq!(tz.offset_string(JAN), offset, "{tz_id} in January");
            assert_eq!(tz.offset_string(JUL), offset, "{tz_id} in July");
        }
    }

    /// Europe and the US change on different days, so for a fortnight the
    /// Atlantic gap is four hours -- the whole reason to carry rules.
    #[test]
    fn europe_and_the_us_do_not_change_on_the_same_day() {
        let between = 1_710_244_800; // 2024-03-12 12:00 UTC
        assert!(shipped("America/New_York").is_dst_at(between));
        assert!(!shipped("Europe/London").is_dst_at(between));
    }

    #[test]
    fn a_malformed_rule_is_refused_rather_than_read_as_utc() {
        for bad in ["America/New_York", "", "Mars", "EST5EDT,garbage", ":::"] {
            assert!(TimezoneInfo::new("x", "x", bad, "x").is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_search_finds_by_name_description_or_city() {
        assert_eq!(search_zones("tokyo").len(), 1);
        assert!(search_zones("utc").iter().any(|z| z.tz_id == "UTC"));
        assert!(search_zones("europe").len() >= 3);
        assert!(
            search_zones("EASTERN")
                .iter()
                .any(|z| z.tz_id == "America/New_York")
        );
        assert!(search_zones("zzzz").is_empty());
    }

    // ---- the machine's zone ----

    /// A minimal slim `TZif` v2 file: no transitions, one type, and `tail` as
    /// its footer -- the shape `zic -b slim` writes for a zone.
    fn tzif(tail: &str) -> Vec<u8> {
        fn header(out: &mut Vec<u8>) {
            out.extend_from_slice(b"TZif2");
            out.extend_from_slice(&[0; 15]);
            // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
            for count in [0u32, 0, 0, 0, 1, 4] {
                out.extend_from_slice(&count.to_be_bytes());
            }
        }
        let mut out = Vec::new();
        for _ in 0..2 {
            header(&mut out);
            out.extend_from_slice(&0i32.to_be_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(b"UTC\0");
        }
        out.push(b'\n');
        out.extend_from_slice(tail.as_bytes());
        out.push(b'\n');
        out
    }

    /// The paths a reader was asked for, in order.
    type Asked = std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>>;

    /// A reader over a fixed set of files, recording what was asked for.
    fn files<'a>(
        present: &'a [(&'a str, Vec<u8>)],
    ) -> (impl FnMut(&Path) -> Option<Vec<u8>> + 'a, Asked) {
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = asked.clone();
        let read = move |path: &Path| {
            log.borrow_mut().push(path.to_path_buf());
            present
                .iter()
                .find(|(p, _)| Path::new(p) == path)
                .map(|(_, bytes)| bytes.clone())
        };
        (read, asked)
    }

    fn rule(s: &str) -> Tz {
        Tz::parse(s.as_bytes()).unwrap()
    }

    /// `TZ` unset is `/etc/localtime`, read as the rule its footer states.
    #[test]
    fn with_no_tz_the_machines_zone_is_etc_localtime() {
        let present = [("/etc/localtime", tzif("CET-1CEST,M3.5.0,M10.5.0/3"))];
        let (read, asked) = files(&present);
        let zone = resolve_zone(tzrules::tz_source(None), None, read);
        assert_eq!(zone, rule("CET-1CEST,M3.5.0,M10.5.0/3"));
        assert_eq!(*asked.borrow(), [PathBuf::from("/etc/localtime")]);
    }

    /// A rule in `TZ` is used as it is, reading no file at all.
    #[test]
    fn a_rule_in_tz_reads_nothing() {
        let (read, asked) = files(&[]);
        let zone = resolve_zone(tzrules::tz_source(Some(b"JST-9")), None, read);
        assert_eq!(zone, rule("JST-9"));
        assert!(asked.borrow().is_empty());
    }

    /// A name in `TZ` is a file under `TZDIR`, or the standard tree without it.
    #[test]
    fn a_name_in_tz_is_a_file_under_tzdir() {
        let present = [
            ("/usr/share/zoneinfo/Asia/Tokyo", tzif("JST-9")),
            ("/opt/zones/Asia/Tokyo", tzif("<+0930>-9:30")),
        ];
        let (read, _) = files(&present);
        let zone = resolve_zone(tzrules::tz_source(Some(b"Asia/Tokyo")), None, read);
        assert_eq!(zone, rule("JST-9"));
        let (read, _) = files(&present);
        let zone = resolve_zone(
            tzrules::tz_source(Some(b"Asia/Tokyo")),
            Some(Path::new("/opt/zones")),
            read,
        );
        assert_eq!(zone, rule("<+0930>-9:30"));
    }

    /// Everything that cannot be read, parsed or resolved is UTC -- as it is
    /// for the libc, so the desktop and `date` stay in step even when both are
    /// wrong.
    #[test]
    fn what_cannot_be_resolved_is_utc() {
        let present = [
            ("/etc/localtime", b"not a zone file".to_vec()),
            ("/usr/share/zoneinfo/Big", vec![b'T'; 20_000]),
        ];
        for tz in [
            None,                      // unreadable /etc/localtime
            Some(&b""[..]),            // TZ set and empty: UTC by request
            Some(b"Nowhere/Special"),  // no such file
            Some(b"../../etc/shadow"), // refused, never opened
            Some(b"Big"),              // not a zone file
        ] {
            let (read, asked) = files(&present);
            let zone = resolve_zone(tzrules::tz_source(tz), None, read);
            assert_eq!(zone, Tz::utc(), "{tz:?}");
            let opened_shadow = asked.borrow().iter().any(|p| {
                p.as_os_str()
                    .as_encoded_bytes()
                    .windows(6)
                    .any(|w| w == b"shadow")
            });
            assert!(!opened_shadow, "a refused name was opened");
        }
    }

    /// The real reader refuses a file larger than a zone file can be, rather
    /// than reading half a transition table.
    #[test]
    fn a_zoneinfo_file_over_the_cap_is_not_read() {
        let dir = std::env::temp_dir().join(format!("datetimesettings-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big");
        std::fs::write(
            &big,
            vec![0u8; usize::try_from(MAX_ZONEINFO_BYTES).unwrap() + 1],
        )
        .unwrap();
        let small = dir.join("small");
        std::fs::write(&small, tzif("JST-9")).unwrap();
        assert_eq!(read_zoneinfo(&big), None);
        assert_eq!(read_zoneinfo(&small), Some(tzif("JST-9")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- the settings ----

    /// The default is the machine's zone -- not New York, which it was for as
    /// long as there was nowhere to change it.
    #[test]
    fn by_default_the_clock_is_in_the_machines_zone() {
        let s = DateTimeSettings::default();
        assert_eq!(s.zone, None);
        assert_eq!(s.current_zone(), None);
        let system = rule("NZST-12NZDT,M9.5.0,M4.1.0/3");
        assert_eq!(s.rule(system), system);
        assert!(s.show_day_of_week && s.show_date && !s.show_seconds);
    }

    #[test]
    fn a_chosen_zone_is_one_a_user_can_choose() {
        let mut s = DateTimeSettings::default();
        assert!(s.set_zone(Some("Asia/Tokyo")));
        assert_eq!(s.rule(Tz::utc()), shipped("Asia/Tokyo").rule);
        assert!(!s.set_zone(Some("Mars/Olympus_Mons")));
        assert_eq!(
            s.zone.as_deref(),
            Some("Asia/Tokyo"),
            "a refusal changes nothing"
        );
        assert!(s.set_zone(None));
        assert_eq!(s.zone, None);
    }

    /// A name nothing here knows -- a hand-edited file -- reads as the
    /// machine's zone rather than an offset invented for it, and is kept.
    #[test]
    fn an_unknown_zone_reads_as_the_machines_and_is_kept() {
        let doc = Document::parse("zone: Mars/Olympus_Mons\n");
        let s = DateTimeSettings::read_from(&doc);
        assert_eq!(s.zone.as_deref(), Some("Mars/Olympus_Mons"));
        let system = rule("JST-9");
        assert_eq!(s.rule(system), system);
        let mut saved = doc.clone();
        s.write_into(&mut saved);
        assert_eq!(
            saved.get_str(&["zone"]).as_deref(),
            Some("Mars/Olympus_Mons")
        );
    }

    #[test]
    fn clocks_are_limited_named_once_and_in_the_table() {
        let mut s = DateTimeSettings::default();
        assert!(s.add_clock("Asia/Tokyo", " Tokyo "));
        assert_eq!(s.additional_clocks[0].label, "Tokyo", "trimmed");
        assert!(s.additional_clocks[0].visible, "a new clock is shown");
        assert!(
            !s.add_clock("Europe/Paris", "Tokyo"),
            "a second clock with one label"
        );
        assert!(!s.add_clock("Mars/Olympus_Mons", "Mars"), "not a zone");
        assert!(!s.add_clock("Europe/Paris", "  "), "no label");
        for (zone, label) in [
            ("Europe/Paris", "Paris"),
            ("UTC", "UTC"),
            ("Asia/Seoul", "Seoul"),
        ] {
            assert!(s.add_clock(zone, label));
        }
        assert!(!s.add_clock("Europe/London", "London"), "a fifth");
        assert_eq!(s.additional_clocks.len(), MAX_CLOCKS);
        assert!(s.remove_clock(0));
        assert!(!s.remove_clock(9));
        assert_eq!(s.additional_clocks[0].label, "Paris");
    }

    // ---- the file ----

    fn everything() -> DateTimeSettings {
        let mut s = DateTimeSettings {
            zone: Some("Europe/Paris".to_owned()),
            show_seconds: true,
            show_day_of_week: false,
            show_date: false,
            additional_clocks: Vec::new(),
        };
        assert!(s.add_clock("Asia/Tokyo", "Tokyo: HQ #1"));
        assert!(s.add_clock("America/New_York", "Home"));
        s.additional_clocks[1].visible = false;
        s
    }

    /// Every field differs from the default and survives the file, labels with
    /// YAML's own punctuation in them included.
    #[test]
    fn every_setting_round_trips() {
        let s = everything();
        assert_ne!(s, DateTimeSettings::default());
        let mut doc = Document::new();
        s.write_into(&mut doc);
        assert_eq!(
            DateTimeSettings::read_from(&Document::parse(&doc.to_text())),
            s
        );
    }

    #[test]
    fn an_empty_file_is_the_defaults_and_the_defaults_write_the_zone_key() {
        assert_eq!(
            DateTimeSettings::read_from(&Document::new()),
            DateTimeSettings::default()
        );
        let mut doc = Document::new();
        DateTimeSettings::default().write_into(&mut doc);
        assert_eq!(doc.get_str(&["zone"]).as_deref(), Some(""));
        assert!(!doc.contains(&["clocks"]), "no clocks, no block");
    }

    /// A comment beside a clock survives a save that changes nothing about
    /// the list; a reorder rewrites the list in its new order.
    #[test]
    fn the_clocks_keep_their_comments_until_their_order_changes() {
        let text = "\
zone: \"\"
clocks:
  # the office
  Tokyo:
    zone: Asia/Tokyo
    visible: true
  Home:
    zone: America/New_York
    visible: false
";
        let mut file = DateTimeFile::from_document(Document::parse(text));
        file.settings.show_seconds = true;
        assert!(
            file.to_text().contains("# the office"),
            "{}",
            file.to_text()
        );

        file.settings.additional_clocks.swap(0, 1);
        let reordered = DateTimeSettings::read_from(&Document::parse(&file.to_text()));
        let labels: Vec<&str> = reordered
            .additional_clocks
            .iter()
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(labels, ["Home", "Tokyo"]);

        file.settings.additional_clocks.remove(0);
        let removed = DateTimeSettings::read_from(&Document::parse(&file.to_text()));
        assert_eq!(removed.additional_clocks.len(), 1);
        assert_eq!(removed.additional_clocks[0].label, "Tokyo");
    }

    /// A file edited by hand is read as far as it makes sense: a clock with no
    /// zone, a repeated label and a fifth clock are dropped, and a value that
    /// is not a boolean keeps the default.
    #[test]
    fn a_hand_edited_file_is_read_as_far_as_it_makes_sense() {
        let text = "\
taskbar:
  seconds: maybe
  date: false
clocks:
  A:
    zone: Asia/Tokyo
  B:
    visible: false
  C:
    zone: Europe/Paris
    visible: nope
  D:
    zone: UTC
  E:
    zone: Asia/Seoul
  F:
    zone: Asia/Dubai
";
        let s = DateTimeSettings::read_from(&Document::parse(text));
        assert!(!s.show_seconds, "`maybe` is not a boolean");
        assert!(!s.show_date);
        let labels: Vec<&str> = s
            .additional_clocks
            .iter()
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(labels, ["A", "C", "D", "E"], "B has no zone; F is a fifth");
        assert!(s.additional_clocks[1].visible, "`nope` keeps the default");
    }

    /// Through the real configuration directory: saved, and read back by a
    /// fresh load, as the Settings application's save and the shell's next
    /// start would.
    #[test]
    fn a_save_is_what_the_next_load_reads() {
        settingsfile::testing::with_scratch_config("datetime-file", |root| {
            let mut file = DateTimeFile::load();
            assert_eq!(file.settings, DateTimeSettings::default());
            file.settings = everything();
            file.save().unwrap();
            assert!(settingsfile::testing::scratch_path(root, CONFIG_NAME).is_file());
            assert_eq!(DateTimeFile::load().settings, everything());
        });
    }
}
