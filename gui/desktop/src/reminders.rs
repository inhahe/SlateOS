//! The calendar's reminders, which the desktop raises (`design-decisions.md`
//! §1498).
//!
//! The calendar program keeps a reminder with each event -- at its time, so
//! many minutes or hours before, a day before -- and raises none: it is a
//! window, and no window is open when most reminders fall due. The desktop
//! runs whenever someone is signed in and already reads the calendar for the
//! clock's popup ([`crate::calendar::Agenda`]), so it raises them: each as a
//! notification from Calendar that stays on screen until it is closed, whose
//! menu offers to remind again later. That is what the desktop's own reminder
//! code could do and nothing ever called; it went with its event store
//! (§1497), as the operator's rule for dead code with a live twin has it
//! (§1423), and this is the survivor taking it up.
//!
//! # When a reminder is due
//!
//! At its occurrence's start less its lead ([`lead`]), the start read in the
//! desktop's zone: an event at 09:00 with a reminder fifteen minutes before
//! is due at 08:45 by the taskbar's clock, summer or winter. An all-day event
//! starts at midnight, as iCalendar has it, so its "a day before" is midnight
//! the day before. Each occurrence of a repeating event has its own; an
//! all-day event of several days has one, before its first day.
//!
//! # Raised once, and late rather than never
//!
//! A reminder is raised when it is due and its occurrence has not ended -- a
//! reminder of a meeting already over is not news -- and never twice:
//! [`Reminders`] remembers which have been, by the event and the moment its
//! occurrence starts, and is kept on disk across the desktop restarting
//! ([`DATA_NAME`]). So one that fell due while nobody was signed in, or while
//! the machine slept, is raised when the desktop next looks; an event moved to
//! another time starts at another moment and so has a reminder of its own;
//! and one added with less notice than its lead -- a meeting in ten minutes,
//! reminded fifteen before -- is raised at once. Only occurrences that began
//! in the last day are looked back at ([`LOOKBACK_SECS`]): one still running
//! after that is a conference, not an appointment, and its reminder is stale.
//!
//! # Again later
//!
//! A reminder's menu offers it again in five, fifteen or thirty minutes or an
//! hour ([`SNOOZES`], the four the deleted code had). Then it is raised as it
//! stands then: if its event is still on the calendar at that start and not
//! over.

use calendarstore::{CalendarEvent, RecurrenceRule, Reminder};
use tzrules::Tz;

/// The data file the reminders raised and put off are kept in.
pub const DATA_NAME: &str = "reminders";

/// The program a reminder's notification is from: the calendar's, so that
/// what the user set for the calendar's notifications -- off, silent, no
/// banner -- is what its reminders do, and they are listed under its name.
pub const APP_NAME: &str = "Calendar";

/// The first line of the file: the format and its version.
const HEADER: &str = "slateos-reminders 1";

/// Seconds in a day.
const DAY_SECS: i64 = 86_400;

/// How long after its occurrence began a reminder not yet raised is still
/// raised -- for an event that has not ended -- and how long one raised is
/// remembered past its start.
pub const LOOKBACK_SECS: i64 = DAY_SECS;

/// How far ahead the next reminder is looked for: with none that soon, the
/// desktop looks again then.
pub const HORIZON_SECS: i64 = 7 * DAY_SECS;

/// The longest lead honoured: a reminder asked for further ahead -- an
/// imported calendar may ask anything -- comes this long before.
pub const MAX_LEAD_SECS: i64 = 31 * DAY_SECS;

/// How much later a reminder can be asked to come again, in minutes, as its
/// menu offers them.
pub const SNOOZES: [u32; 4] = [5, 15, 30, 60];

/// Which reminder: an event, by the number the calendar program gave it, and
/// the moment one of its occurrences starts, in seconds since 1970.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReminderKey {
    /// The event's number in the calendar program's file.
    pub event: u64,
    /// When the occurrence starts.
    pub starts: i64,
}

/// A reminder come due: what its notification says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    /// Which it is.
    pub key: ReminderKey,
    /// The event's title.
    pub title: String,
    /// Where it is, if the calendar says.
    pub place: Option<String>,
    /// Whether it lasts all day.
    pub all_day: bool,
}

/// How long before its occurrence `reminder` is due, in seconds -- `None` for
/// none -- and never more than [`MAX_LEAD_SECS`].
#[must_use]
pub fn lead(reminder: Reminder) -> Option<i64> {
    let secs = match reminder {
        Reminder::None => return None,
        Reminder::AtTime => 0,
        Reminder::MinutesBefore(minutes) => i64::from(minutes).saturating_mul(60),
        Reminder::HoursBefore(hours) => i64::from(hours).saturating_mul(3600),
        Reminder::DayBefore => DAY_SECS,
    };
    Some(secs.min(MAX_LEAD_SECS))
}

/// The day number (days since 1970-01-01) of the local date at UTC instant
/// `utc` in `tz`.
fn local_day(utc: i64, tz: &Tz) -> i64 {
    utc.saturating_add(i64::from(tz.lookup(utc).gmtoff))
        .div_euclid(DAY_SECS)
}

/// The UTC instant of `day` (days since 1970) at `minutes` past local
/// midnight in `tz`. A moment the clock skips -- the hour lost in spring --
/// is read as just after the jump, and one it repeats as the first of the
/// two, as `mktime` reads them.
fn instant(day: i64, minutes: u32, tz: &Tz) -> i64 {
    let local = day
        .saturating_mul(DAY_SECS)
        .saturating_add(i64::from(minutes).saturating_mul(60));
    tz.local_to_utc(local, -1).0
}

/// The calendar's date for `day` (days since 1970), or `None` past the
/// years a date can hold.
fn date_of_day(day: i64) -> Option<calendarstore::Date> {
    let (year, month, day) = tzrules::civil_from_days(day);
    Some(calendarstore::Date {
        year: i32::try_from(year).ok()?,
        month,
        day,
    })
}

/// The day number of `date`.
fn day_of_date(date: calendarstore::Date) -> i64 {
    tzrules::days_from_civil(i64::from(date.year), date.month, date.day)
}

/// When `event`'s occurrence that starts at `starts` ends: its length after
/// its start, as the calendar program measures it -- an all-day event at the
/// midnight after its last day.
fn ends(event: &CalendarEvent, starts: i64, tz: &Tz) -> i64 {
    if event.all_day {
        let days = event
            .end
            .date
            .days_since(event.start.date)
            .clamp(0, 366)
            .saturating_add(1);
        instant(local_day(starts, tz).saturating_add(days), 0, tz)
    } else {
        // At least a minute: an event that ends as it starts is still on at
        // its start.
        starts.saturating_add(i64::from(event.duration_minutes().max(1)).saturating_mul(60))
    }
}

/// Minutes past midnight `event`'s occurrences start at: none for an all-day
/// event, which starts at midnight.
fn start_minutes(event: &CalendarEvent) -> u32 {
    if event.all_day {
        0
    } else {
        event.start.time.to_minutes()
    }
}

/// When `event`'s first occurrence starts in `tz`.
fn first_start(event: &CalendarEvent, tz: &Tz) -> i64 {
    instant(day_of_date(event.start.date), start_minutes(event), tz)
}

/// When `event`'s occurrences start in `tz`, from `from` to `upto` (both
/// included), earliest first: the day it starts and each day it repeats on,
/// at its time -- midnight for an all-day event.
fn starts_between(event: &CalendarEvent, tz: &Tz, from: i64, upto: i64) -> Vec<i64> {
    let minutes = start_minutes(event);
    let first = day_of_date(event.start.date);
    // A day either side of the window's, for the zone's offset.
    let low = local_day(from, tz).saturating_sub(1).max(first);
    let high = local_day(upto, tz).saturating_add(1);
    let mut found = Vec::new();
    let mut day = low;
    while day <= high {
        if let Some(date) = date_of_day(day)
            && (date == event.start.date || event.recurrence.matches(event.start.date, date))
        {
            let at = instant(day, minutes, tz);
            if (from..=upto).contains(&at) {
                found.push(at);
            }
        }
        day = day.saturating_add(1);
    }
    found
}

/// The reminders raised, and those asked to come again: what keeps each from
/// being raised twice, across the desktop restarting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reminders {
    /// Raised, by event and start -- forgotten a day after the start, when
    /// nothing looks back that far.
    raised: Vec<ReminderKey>,
    /// Asked to come again, and when.
    snoozed: Vec<(ReminderKey, i64)>,
    /// Counted up at every change, for whoever keeps this on disk.
    revision: u64,
}

impl Reminders {
    /// Nothing raised and nothing put off.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counted up at every change: the state on disk is out of date when it
    /// was written at another.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The reminders of `events` due at `now` in `tz`, each now counted as
    /// raised: those whose time has come, whose occurrence has not ended and
    /// that have not been raised before, then those asked to come again by
    /// now whose occurrence is still on the calendar and not over.
    pub fn take_due(&mut self, events: &[CalendarEvent], tz: &Tz, now: i64) -> Vec<Due> {
        let mut due = Vec::new();
        let mut changed = false;
        for event in events {
            let Some(lead) = lead(event.reminder) else {
                continue;
            };
            let from = now.saturating_sub(LOOKBACK_SECS);
            for starts in starts_between(event, tz, from, now.saturating_add(lead)) {
                let key = ReminderKey {
                    event: event.id,
                    starts,
                };
                if starts.saturating_sub(lead) > now
                    || self.raised.contains(&key)
                    || ends(event, starts, tz) <= now
                {
                    continue;
                }
                self.raised.push(key);
                changed = true;
                due.push(due_of(event, key));
            }
        }
        let (come, waiting): (Vec<_>, Vec<_>) = self
            .snoozed
            .iter()
            .copied()
            .partition(|&(_, until)| until <= now);
        if !come.is_empty() {
            self.snoozed = waiting;
            changed = true;
        }
        for (key, _) in come {
            let still = events.iter().find(|event| {
                event.id == key.event
                    && starts_between(event, tz, key.starts, key.starts).contains(&key.starts)
                    && ends(event, key.starts, tz) > now
            });
            if let Some(event) = still {
                due.push(due_of(event, key));
            }
        }
        let horizon = now.saturating_sub(LOOKBACK_SECS);
        let before = self.raised.len();
        self.raised.retain(|key| key.starts >= horizon);
        changed |= self.raised.len() != before;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        due
    }

    /// When the next reminder of `events` falls due after `now` in `tz`,
    /// looking [`HORIZON_SECS`] ahead: `now` itself for one due already and
    /// not yet raised, the horizon -- to look again then -- for a calendar
    /// with reminders further ahead than that, and `None` for one with none
    /// ahead at all and nothing put off.
    #[must_use]
    pub fn next_due(&self, events: &[CalendarEvent], tz: &Tz, now: i64) -> Option<i64> {
        let horizon = now.saturating_add(HORIZON_SECS);
        let mut next: Option<i64> = None;
        let mut any = false;
        for event in events {
            let Some(lead) = lead(event.reminder) else {
                continue;
            };
            let from = now.saturating_sub(LOOKBACK_SECS);
            let upto = horizon.saturating_add(lead);
            // More past the horizon: a repeating event's, or one event's
            // that starts beyond it. One that never repeats and starts
            // before has none ahead however far one looks, and a calendar of
            // those wakes nobody.
            any |=
                !matches!(event.recurrence, RecurrenceRule::None) || first_start(event, tz) > upto;
            for starts in starts_between(event, tz, from, upto) {
                let key = ReminderKey {
                    event: event.id,
                    starts,
                };
                if self.raised.contains(&key) || ends(event, starts, tz) <= now {
                    continue;
                }
                let at = starts.saturating_sub(lead).max(now);
                next = Some(next.map_or(at, |n| n.min(at)));
                // Each later start is due later: the first is this event's.
                break;
            }
        }
        for &(_, until) in &self.snoozed {
            next = Some(next.map_or(until, |n| n.min(until)));
        }
        match next {
            Some(at) => Some(at.min(horizon)),
            None if any => Some(horizon),
            None => None,
        }
    }

    /// Raise `key` again at `until`.
    pub fn snooze(&mut self, key: ReminderKey, until: i64) {
        self.snoozed.retain(|(k, _)| *k != key);
        self.snoozed.push((key, until));
        self.revision = self.revision.wrapping_add(1);
    }

    /// The reminders as kept on disk: the header, then a line for each raised
    /// (`raised`, the event, the start) and each put off (`again`, the event,
    /// the start, when), tab-separated.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut out = String::from(HEADER);
        out.push('\n');
        for key in &self.raised {
            out.push_str(&format!("raised\t{}\t{}\n", key.event, key.starts));
        }
        for (key, until) in &self.snoozed {
            out.push_str(&format!("again\t{}\t{}\t{until}\n", key.event, key.starts));
        }
        out
    }

    /// The reminders `bytes` holds, as [`encode`](Self::encode) writes them:
    /// none from a file of another format, and a line this version cannot
    /// read left out -- one bad line must not raise every reminder again.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let mut kept = Self::new();
        let Ok(text) = std::str::from_utf8(bytes) else {
            return kept;
        };
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return kept;
        }
        for line in lines {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                ["raised", event, starts] => {
                    if let Some(key) = key_of(event, starts) {
                        kept.raised.push(key);
                    }
                }
                ["again", event, starts, until] => {
                    if let (Some(key), Ok(until)) = (key_of(event, starts), until.parse()) {
                        kept.snoozed.push((key, until));
                    }
                }
                _ => {}
            }
        }
        kept
    }

    /// The reminders kept on disk, or none: no file is a desktop that has
    /// raised none yet.
    #[must_use]
    pub fn load() -> Self {
        settingsfile::load_data(DATA_NAME)
            .map(|bytes| Self::decode(&bytes))
            .unwrap_or_default()
    }

    /// Write the reminders to disk, atomically.
    ///
    /// # Errors
    ///
    /// The write's own: no data directory, or one that cannot be written.
    pub fn store(&self) -> std::io::Result<()> {
        settingsfile::store_data(DATA_NAME, self.encode().as_bytes())
    }
}

/// A key from its two fields as written, or `None` for one that is not.
fn key_of(event: &str, starts: &str) -> Option<ReminderKey> {
    Some(ReminderKey {
        event: event.parse().ok()?,
        starts: starts.parse().ok()?,
    })
}

/// What `event`'s reminder `key` says.
fn due_of(event: &CalendarEvent, key: ReminderKey) -> Due {
    Due {
        key,
        title: event.title.clone(),
        place: event
            .location
            .as_ref()
            .filter(|place| !place.trim().is_empty())
            .cloned(),
        all_day: event.all_day,
    }
}

/// When `due`'s occurrence is, said from `now` in `tz`, as its notification
/// says it: "In 15 minutes, at 09:00", "Now, at 09:00", "Started 5 minutes
/// ago, at 09:00", "Tomorrow at 09:00", "On Friday 9 October at 09:00" --
/// and for an all-day event "Today", "Tomorrow", "On Friday 9 October".
#[must_use]
pub fn when(due: &Due, now: i64, tz: &Tz) -> String {
    let starts = due.key.starts;
    let today = local_day(now, tz);
    let day = local_day(starts, tz);
    let date = date_of_day(day);
    let on = || {
        date.map_or_else(String::new, |d| {
            format!("On {} {} {}", d.day_of_week_name(), d.day, d.month_name())
        })
    };
    if due.all_day {
        return match day.saturating_sub(today) {
            0 => "Today".to_owned(),
            1 => "Tomorrow".to_owned(),
            _ => on(),
        };
    }
    let local = starts.saturating_add(i64::from(tz.lookup(starts).gmtoff));
    let minute_of_day = local.rem_euclid(DAY_SECS) / 60;
    let clock = format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60);
    let ahead = starts.saturating_sub(now);
    if ahead.unsigned_abs() < 60 {
        return format!("Now, at {clock}");
    }
    if ahead < 0 {
        return format!("Started {} ago, at {clock}", span(ahead.saturating_neg()));
    }
    match day.saturating_sub(today) {
        0 => format!("In {}, at {clock}", span(ahead)),
        1 => format!("Tomorrow at {clock}"),
        _ => format!("{} at {clock}", on()),
    }
}

/// `secs` as a person says it, to the minute and rounded up -- a meeting in
/// fourteen and a half minutes is "in 15 minutes": "1 minute", "45
/// minutes", "1 hour", "2 hours 10 minutes".
fn span(secs: i64) -> String {
    let minutes = secs.saturating_add(59) / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    let unit = |n: i64, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    match (hours, minutes) {
        (0, m) => unit(m, "minute", "minutes"),
        (h, 0) => unit(h, "hour", "hours"),
        (h, m) => format!(
            "{} {}",
            unit(h, "hour", "hours"),
            unit(m, "minute", "minutes")
        ),
    }
}

/// How a time to come again in is offered on a reminder's menu: "In 5
/// minutes", "In 1 hour".
#[must_use]
pub fn snooze_label(minutes: u32) -> String {
    format!("In {}", span(i64::from(minutes).saturating_mul(60)))
}

/// What a reminder's notification says: its title is the event's, and this
/// its body -- when, from `now` in `tz`, and where if the calendar says.
#[must_use]
pub fn body(due: &Due, now: i64, tz: &Tz) -> String {
    let when = when(due, now, tz);
    match &due.place {
        Some(place) => format!("{when}. {place}"),
        None => when,
    }
}

#[cfg(test)]
#[path = "reminders_tests.rs"]
mod tests;
