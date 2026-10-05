//! The notification pane's history, kept across the desktop restarting.
//!
//! The pane keeps a notification until it is dismissed; until this, all of
//! them were forgotten whenever the desktop restarted -- a logout, a reboot
//! -- so "what did I miss?" could not be answered across one
//! (`known-issues.md` `TD-C-FOUR-NOTIFICATION-BEHAVIOURS-HAVE-A-SETTINGS-MODEL-AND-NO-IMPLEMENTATION`).
//! Now the session writes the pane's notifications whenever they change and
//! reads them back when it starts (design-decisions §1468).
//!
//! # Where
//!
//! `notifications` in the user's data directory
//! ([`settingsfile::data_dir`]): a record of what happened, not a setting,
//! so not beside `notifications.yaml`, which says how long to keep it
//! ([`notifsettings::HistoryRetention`]).
//!
//! # The format
//!
//! Text, a line a notification, newest first, after a first line naming the
//! format and its version (`slateos-notifications 1`). A line is eight
//! fields separated by tabs:
//!
//! ```text
//! timestamp  priority  read  silent  app  title  body  action
//! ```
//!
//! `timestamp` is seconds since 1970, `priority` one of `low`, `normal`,
//! `high` and `urgent`, `read` and `silent` `0` or `1`. The four text fields
//! are percent-encoded as design-decisions §426 encodes a path -- `%`, and
//! every byte that is not printable ASCII, as `%XX` -- so none holds a tab or
//! a line break and every byte comes back. `action` is empty for none and
//! `+` and the program otherwise, so that an action of nothing is told from
//! no action.
//!
//! A line it cannot read -- a hand edit, a later version's -- is left out
//! and the rest kept: one bad line must not cost the history.
//!
//! # How long
//!
//! A notification older than the user's [`HistoryRetention`] is left out on
//! reading and on writing, so a desktop that runs for weeks forgets what it
//! should as it goes; and at most the pane's own cap are kept. A retention
//! of nought keeps nothing: nothing is read, and nothing but the header is
//! written.

use std::io;

use notifsettings::HistoryRetention;

use crate::notif_pane::{NotifPriority, Notification};

/// The data file the history is kept in.
pub const DATA_NAME: &str = "notifications";

/// The first line of the file: the format and its version.
const HEADER: &str = "slateos-notifications 1";

/// The most notifications kept: the pane's own cap, read off it so the two
/// cannot disagree.
pub const MAX_KEPT: usize = crate::notif_pane::MAX_NOTIFICATIONS;

/// The history as written: the header, then `notifications` -- newest
/// first -- that `retention` keeps at `now`, a line each.
#[must_use]
pub fn encode(notifications: &[Notification], now: u64, retention: HistoryRetention) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for notif in kept(notifications.iter(), now, retention) {
        let action = notif.action.as_deref().map_or_else(String::new, |program| {
            format!("+{}", pathcodec::encode_bytes(program.as_bytes()))
        });
        let fields = [
            notif.timestamp.to_string(),
            priority_name(notif.priority).to_owned(),
            flag(notif.read).to_owned(),
            flag(notif.silent).to_owned(),
            pathcodec::encode_bytes(notif.app_name.as_bytes()),
            pathcodec::encode_bytes(notif.title.as_bytes()),
            pathcodec::encode_bytes(notif.body.as_bytes()),
            action,
        ];
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out
}

/// The notifications `bytes` holds -- newest first, ids nought, for the pane
/// to give out -- that `retention` keeps at `now`. Nothing for a file of
/// another format, or one this version does not know.
#[must_use]
pub fn decode(bytes: &[u8], now: u64, retention: HistoryRetention) -> Vec<Notification> {
    let mut lines = bytes.split(|&b| b == b'\n');
    let header = lines
        .next()
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line));
    if header != Some(HEADER.as_bytes()) {
        return Vec::new();
    }
    let read: Vec<Notification> = lines
        .filter_map(|line| std::str::from_utf8(line).ok())
        .filter_map(|line| read_line(line.strip_suffix('\r').unwrap_or(line)))
        .collect();
    kept(read.iter(), now, retention).cloned().collect()
}

/// The history kept on disk, as [`decode`] reads it: nothing where there is
/// none, or where the retention keeps none.
#[must_use]
pub fn load(now: u64, retention: HistoryRetention) -> Vec<Notification> {
    if !retention.keeps_any() {
        return Vec::new();
    }
    settingsfile::load_data(DATA_NAME)
        .map(|bytes| decode(&bytes, now, retention))
        .unwrap_or_default()
}

/// Write `notifications` as the history kept on disk, atomically.
///
/// # Errors
///
/// The write's own: no data directory, or one that cannot be written.
pub fn store(
    notifications: &[Notification],
    now: u64,
    retention: HistoryRetention,
) -> io::Result<()> {
    settingsfile::store_data(DATA_NAME, encode(notifications, now, retention).as_bytes())
}

/// Those of `notifications` that `retention` keeps at `now`: none for a
/// retention of nought, else at most [`MAX_KEPT`] younger than it. One from
/// the future -- a clock set back -- is kept, as a notification just come.
fn kept<'a>(
    notifications: impl Iterator<Item = &'a Notification>,
    now: u64,
    retention: HistoryRetention,
) -> impl Iterator<Item = &'a Notification> {
    let keep = retention.keeps_any();
    let max_age = retention.max_age_secs();
    notifications
        .filter(move |n| keep && now.saturating_sub(n.timestamp) <= max_age)
        .take(MAX_KEPT)
}

/// One line as a notification, or `None` for one this version cannot read.
fn read_line(line: &str) -> Option<Notification> {
    let mut fields = line.split('\t');
    let mut next = || fields.next();
    let timestamp = next()?.parse::<u64>().ok()?;
    let priority = priority_named(next()?)?;
    let read = flag_named(next()?)?;
    let silent = flag_named(next()?)?;
    let app_name = text(next()?)?;
    let title = text(next()?)?;
    let body = text(next()?)?;
    let action = match next()? {
        "" => None,
        encoded => Some(text(encoded.strip_prefix('+')?)?),
    };
    // A ninth field is a later version's line.
    if next().is_some() {
        return None;
    }
    Some(Notification {
        id: 0,
        app_name,
        title,
        body,
        timestamp,
        priority,
        read,
        action,
        silent,
    })
}

/// A percent-encoded field as the text it was: `None` where its bytes are
/// not text, which no field written here can be.
fn text(encoded: &str) -> Option<String> {
    String::from_utf8(pathcodec::decode_bytes(encoded)).ok()
}

fn priority_name(priority: NotifPriority) -> &'static str {
    match priority {
        NotifPriority::Low => "low",
        NotifPriority::Normal => "normal",
        NotifPriority::High => "high",
        NotifPriority::Urgent => "urgent",
    }
}

fn priority_named(name: &str) -> Option<NotifPriority> {
    Some(match name {
        "low" => NotifPriority::Low,
        "normal" => NotifPriority::Normal,
        "high" => NotifPriority::High,
        "urgent" => NotifPriority::Urgent,
        _ => return None,
    })
}

const fn flag(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

fn flag_named(text: &str) -> Option<bool> {
    match text {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
#[path = "notif_history_tests.rs"]
mod tests;
