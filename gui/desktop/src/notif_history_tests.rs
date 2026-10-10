//! Tests for the notification history: what is written, what is read back,
//! and what the retention keeps.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use notifsettings::HistoryRetention;

use super::{MAX_KEPT, decode, encode};
use crate::notif_pane::{NotifPriority, Notification};

const DAY: u64 = 24 * 60 * 60;
const NOW: u64 = 1_790_000_000;

fn notif(title: &str, timestamp: u64) -> Notification {
    Notification {
        id: 7,
        app_name: "Mail".to_owned(),
        title: title.to_owned(),
        body: String::new(),
        timestamp,
        priority: NotifPriority::Normal,
        read: false,
        action: None,
        silent: false,
    }
}

fn week() -> HistoryRetention {
    HistoryRetention::default()
}

/// The fields compared, the id left out: the pane gives ids anew.
fn fields(
    n: &Notification,
) -> (
    String,
    String,
    String,
    u64,
    NotifPriority,
    bool,
    Option<String>,
    bool,
) {
    (
        n.app_name.clone(),
        n.title.clone(),
        n.body.clone(),
        n.timestamp,
        n.priority,
        n.read,
        n.action.clone(),
        n.silent,
    )
}

/// **Every notification comes back as it went** -- text with tabs, line
/// breaks, `%` and letters past ASCII, every priority, read and silent, an
/// action of a program and an action of nothing -- newest first, each with
/// its id nought for the pane to give.
#[test]
fn notifications_come_back_as_they_went() {
    let mut odd = notif("Tab\there, new\nline, 100% caf\u{e9} \u{1F600}", NOW);
    odd.body = "a\r\nb\\c".to_owned();
    odd.app_name = "Mail: Inbox".to_owned();
    odd.priority = NotifPriority::Urgent;
    odd.read = true;
    odd.silent = true;
    odd.action = Some("/usr/bin/mail --open 3".to_owned());
    let mut empty_action = notif("empty action", NOW - 10);
    empty_action.action = Some(String::new());
    empty_action.priority = NotifPriority::Low;
    let mut high = notif("", NOW - 20);
    high.priority = NotifPriority::High;
    let written = vec![odd, empty_action, high];

    let text = encode(&written, NOW, week());
    assert!(
        text.lines().count() == 4 && text.starts_with("slateos-notifications 1\n"),
        "{text}"
    );
    let read = decode(text.as_bytes(), NOW, week());
    assert_eq!(
        read.iter().map(fields).collect::<Vec<_>>(),
        written.iter().map(fields).collect::<Vec<_>>()
    );
    assert!(read.iter().all(|n| n.id == 0));
}

/// **What is older than the retention is forgotten**, on writing and on
/// reading; one from the future -- a clock set back -- is kept; and at most
/// the pane's own cap.
#[test]
fn the_retention_keeps_what_it_says() {
    let fresh = notif("fresh", NOW - DAY);
    let edge = notif("a week", NOW - 7 * DAY);
    let old = notif("old", NOW - 7 * DAY - 1);
    let future = notif("future", NOW + DAY);
    let all = [fresh, edge, old, future];
    let titles = |list: &[Notification]| list.iter().map(|n| n.title.clone()).collect::<Vec<_>>();

    let written = encode(&all, NOW, week());
    assert_eq!(
        titles(&decode(written.as_bytes(), NOW, week())),
        ["fresh", "a week", "future"]
    );
    // Read a day later, the week-old one has gone too.
    assert_eq!(
        titles(&decode(written.as_bytes(), NOW + DAY, week())),
        ["fresh", "future"]
    );
    // Nought keeps nothing, either way.
    let none = HistoryRetention { days: 0 };
    assert_eq!(encode(&all, NOW, none), "slateos-notifications 1\n");
    assert!(decode(written.as_bytes(), NOW, none).is_empty());

    let many: Vec<Notification> = (0..MAX_KEPT + 20)
        .map(|i| notif(&format!("n{i}"), NOW))
        .collect();
    let kept = decode(encode(&many, NOW, week()).as_bytes(), NOW, week());
    assert_eq!(kept.len(), MAX_KEPT);
    assert_eq!(kept[0].title, "n0", "the newest kept");
}

/// **A line it cannot read is left out and the rest kept**; a file of
/// another format, or a later version, gives nothing; Windows line ends
/// are read.
#[test]
fn a_bad_line_costs_only_itself() {
    let good = "1790000000\tnormal\t0\t0\tMail\tHello\t\t";
    let text = format!(
        "slateos-notifications 1\r\n{good}\r\n\
         1790000000\tloud\t0\t0\tMail\tBad priority\t\t\n\
         1790000000\tnormal\t2\t0\tMail\tBad flag\t\t\n\
         nonsense\n\
         1790000000\tnormal\t0\t0\tMail\tToo many\t\t\textra\n\
         1790000000\tnormal\t0\t0\tMail\tBad action\t\tx\n\
         1790000000\tnormal\t0\t0\tMail\tBad text%FF\t\t\n\
         {good}\n"
    );
    let read = decode(text.as_bytes(), NOW, week());
    assert_eq!(read.len(), 2, "{read:?}");
    assert!(read.iter().all(|n| n.title == "Hello"));
    assert!(decode(b"something else\n", NOW, week()).is_empty());
    assert!(
        decode(
            format!("slateos-notifications 2\n{good}\n").as_bytes(),
            NOW,
            week()
        )
        .is_empty()
    );
    assert!(decode(b"", NOW, week()).is_empty());
}

// ─── Through the shell ──────────────────────────────────────────────────────

/// **A desktop started again has the notifications of the last** -- as they
/// were left, read or not -- **and pops none of them up**: they are history.
/// Reading them is not a change to write back.
#[test]
fn a_desktop_started_again_has_the_notifications_of_the_last() {
    settingsfile::testing::with_scratch_config("notif-history-shell", |_root| {
        datetimesettings::clock::with_time(NOW, || {
            let mut first = crate::DesktopShell::new(1920, 1080);
            first.load_notification_rules();
            first.load_notification_history();
            assert!(!first.notification_history_dirty(), "nothing yet");
            let read = first.notify(notif("Read one", NOW - 60));
            first.notify(notif("Unread one", NOW));
            assert!(first.notification_history_dirty(), "two arrived");
            first.notifications.mark_read(read);
            first.save_notification_history().expect("saved");
            assert!(!first.notification_history_dirty(), "and written");

            let mut second = crate::DesktopShell::new(1920, 1080);
            second.load_notification_rules();
            second.load_notification_history();
            let kept: Vec<(String, bool)> = second
                .notifications
                .notifications()
                .iter()
                .map(|n| (n.title.clone(), n.read))
                .collect();
            assert_eq!(
                kept,
                [
                    ("Unread one".to_owned(), false),
                    ("Read one".to_owned(), true)
                ]
            );
            assert!(second.toast_extent().is_none(), "history popped up");
            assert!(!second.notification_history_dirty(), "reading wrote");
        });
    });
}

/// **A retention of nothing empties the history at once** -- when the
/// Settings application writes it, not at the next notification.
#[test]
fn a_retention_of_nothing_empties_the_history_at_once() {
    settingsfile::testing::with_scratch_config("notif-history-none", |_root| {
        datetimesettings::clock::with_time(NOW, || {
            let mut shell = crate::DesktopShell::new(1920, 1080);
            shell.load_notification_rules();
            shell.load_notification_history();
            shell.notify(notif("Secret", NOW));
            shell.save_notification_history().expect("saved");

            let mut file = notifsettings::NotifFile::load();
            file.settings.history.days = 0;
            file.save().expect("saved");
            // Whether the rules a notification is filtered by changed is not
            // the question here: only the retention did.
            shell.poll_notification_rules();
            assert!(shell.notification_history_dirty(), "the retention changed");
            shell.save_notification_history().expect("saved");
            assert_eq!(
                settingsfile::load_data(super::DATA_NAME).expect("written"),
                b"slateos-notifications 1\n"
            );
            // And a desktop started under it finds nothing.
            let mut next = crate::DesktopShell::new(1920, 1080);
            next.load_notification_rules();
            next.load_notification_history();
            assert!(next.notifications.notifications().is_empty());
        });
    });
}

/// **A desktop reads back only what its retention keeps** -- a retention
/// lowered while no desktop was running, by hand or by a Settings
/// application with no desktop to tell, is obeyed when one next starts, not
/// only when it next writes.
#[test]
fn a_desktop_reads_back_only_what_its_retention_keeps() {
    settingsfile::testing::with_scratch_config("notif-history-shorter", |_root| {
        datetimesettings::clock::with_time(NOW, || {
            super::store(
                &[
                    notif("an hour ago", NOW - 3600),
                    notif("three days ago", NOW - 3 * DAY),
                ],
                NOW,
                week(),
            )
            .expect("saved");
            let mut file = notifsettings::NotifFile::load();
            file.settings.history.days = 1;
            file.save().expect("saved");

            let mut shell = crate::DesktopShell::new(1920, 1080);
            shell.load_notification_rules();
            shell.load_notification_history();
            let titles: Vec<&str> = shell
                .notifications
                .notifications()
                .iter()
                .map(|n| n.title.as_str())
                .collect();
            assert_eq!(titles, ["an hour ago"]);
        });
    });
}
