//! Tests for the calendar's reminders as the shell raises them
//! (`crate::reminders`, design-decisions §1498): a notification from
//! Calendar that says when and where, opens the calendar program, stays on
//! screen until it is closed, and can be asked to come again later.
//!
//! `reminders`' own tests hold when a reminder is due and that it is raised
//! once; these hold the shell's part. The session's -- waking for the next
//! and keeping what was raised across a restart -- is in `session::tests`.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use std::time::Duration;

use calendarstore::{CalendarEvent, RecurrenceRule, Reminder, Time};
use guitk::menu::{MenuItem, MenuPart};
use guitk::widget::automation::{Accessible, Action};
use tzrules::Tz;

use crate::accessible::{ShellMenu, ShellPart};
use crate::{DesktopShell, calendar, launcher, notif_pane, reminders};

/// An event `title` starting `minutes_ahead` minutes after `now` (seconds
/// since 1970) by `zone`'s clock -- to the minute, rounded down -- an hour
/// long, reminded `lead` minutes before. For a test that has to run on the
/// wall clock: the menu's "remind me again" puts a reminder off from now.
pub(crate) fn event_ahead(
    title: &str,
    now: u64,
    minutes_ahead: i64,
    lead: u32,
    zone: &Tz,
) -> CalendarEvent {
    let starts = now as i64 + minutes_ahead * 60;
    let local = starts + i64::from(zone.lookup(starts).gmtoff);
    let (year, month, day) = tzrules::civil_from_days(local.div_euclid(86_400));
    let minute_of_day = (local.rem_euclid(86_400) / 60) as u32;
    let mut event = calendar::testing::event(
        title,
        (year as i32, month, day),
        minute_of_day / 60,
        RecurrenceRule::None,
        None,
    );
    event.start.time = Time::from_minutes(minute_of_day);
    event.end.time = Time::from_minutes((minute_of_day + 60).min(23 * 60 + 59));
    event.reminder = Reminder::MinutesBefore(lead);
    event
}

/// Nine o'clock on Thursday 8 October 2026 by `shell`'s clock.
fn nine(shell: &DesktopShell) -> u64 {
    let day = tzrules::days_from_civil(2026, 10, 8);
    let (utc, _) = shell.local_zone().local_to_utc(day * 86_400 + 9 * 3600, -1);
    u64::try_from(utc).unwrap()
}

/// A shell whose calendar holds a dentist's appointment at nine on 8
/// October, in Room 3, reminded fifteen minutes before.
fn shell_with_appointment() -> DesktopShell {
    let mut s = DesktopShell::new(1920, 1080);
    let mut dentist =
        calendar::testing::event("Dentist", (2026, 10, 8), 9, RecurrenceRule::None, None);
    dentist.reminder = Reminder::MinutesBefore(15);
    dentist.location = Some("Room 3".to_owned());
    s.agenda = calendar::Agenda::of(vec![dentist]);
    s
}

/// The notifications the calendar's reminders raised, oldest first.
fn reminders_raised(shell: &DesktopShell) -> Vec<notif_pane::Notification> {
    let mut raised: Vec<notif_pane::Notification> = shell
        .notifications
        .notifications()
        .iter()
        .filter(|n| n.app_name == reminders::APP_NAME)
        .cloned()
        .collect();
    raised.sort_by_key(|n| n.id);
    raised
}

/// **A reminder comes due as a notification from Calendar** -- the event's
/// title, when and where, opening the calendar program -- **whose toast
/// stays until it is closed**, and it comes once.
#[test]
fn a_reminder_is_raised_as_a_calendar_notification_that_stays() {
    appearance::config::testing::with_scratch_config("reminder-raised", |_root| {
        let mut s = shell_with_appointment();
        let due = nine(&s) - 15 * 60;
        assert!(s.reminders_due(due - 60), "never worked out");
        assert_eq!(s.raise_due_reminders(due - 60), 0, "raised early");
        assert_eq!(s.next_reminder_in(due - 60), Some(Duration::from_mins(1)));
        assert!(!s.reminders_due(due - 30), "worked out again at every pump");
        assert!(s.reminders_due(due));
        assert_eq!(s.raise_due_reminders(due), 1);

        let raised = reminders_raised(&s);
        assert_eq!(raised.len(), 1);
        let n = &raised[0];
        assert_eq!(n.title, "Dentist");
        assert_eq!(n.body, "In 15 minutes, at 09:00. Room 3");
        assert_eq!(n.priority, notif_pane::NotifPriority::High);
        assert_eq!(n.action.as_deref(), Some(launcher::CALENDAR));
        assert!(!n.read);

        s.advance_toasts(1_000);
        assert_eq!(s.toasts.ids(), [n.id]);
        s.advance_toasts(600_000);
        assert_eq!(s.toasts.ids(), [n.id], "it went by itself");

        assert_eq!(s.raise_due_reminders(due + 60), 0, "raised twice");
        assert_eq!(s.next_reminder_in(due + 60), None, "nothing more ahead");
        assert!(!s.reminders_due(due + 60));

        // The clock's zone changed: every reminder's moment may have moved.
        let before = s.local_zone();
        s.datetime.zone = Some("Asia/Tokyo".to_owned());
        assert_ne!(s.local_zone(), before, "the test's premise: another zone");
        assert!(
            s.reminders_due(due + 60),
            "a zone changed and not looked at"
        );
    });
}

/// **A reminder put off goes, and comes again then**: its toast leaves, its
/// card is read, the loop is told to wake then, and then it is raised anew,
/// saying how near the event is now.
#[test]
fn a_reminder_put_off_goes_and_comes_again() {
    appearance::config::testing::with_scratch_config("reminder-put-off", |_root| {
        let mut s = shell_with_appointment();
        let due = nine(&s) - 15 * 60;
        assert_eq!(s.raise_due_reminders(due), 1);
        s.advance_toasts(1_000);
        let first = reminders_raised(&s)[0].id;

        s.snooze_reminder(first, 15, due);
        s.advance_toasts(1_000);
        assert!(s.toasts.ids().is_empty(), "its toast stayed");
        assert!(reminders_raised(&s)[0].read, "its card is unread");
        assert!(s.reminders_due(due), "the next is not worked out again");
        assert_eq!(s.raise_due_reminders(due), 0);
        assert_eq!(s.next_reminder_in(due), Some(Duration::from_mins(15)));

        assert_eq!(s.raise_due_reminders(due + 15 * 60), 1);
        let raised = reminders_raised(&s);
        assert_eq!(raised.len(), 2);
        assert_eq!(raised[1].body, "Now, at 09:00. Room 3");

        // A notification that is no reminder is not put off.
        let before = s.reminders.encode();
        s.snooze_reminder(9_999, 15, due);
        assert_eq!(s.reminders.encode(), before);
    });
}

/// **A reminder's menu offers it again in five, fifteen or thirty minutes or
/// an hour**, behind "Remind me again", ahead of the program's two rows --
/// and choosing one, through the menu as the user does, puts it off from
/// now. Another program's notification offers no such thing.
#[test]
fn a_reminders_menu_offers_it_again_later() {
    appearance::config::testing::with_scratch_config("reminder-menu", |_root| {
        let mut s = DesktopShell::new(1920, 1080);
        let now = DesktopShell::unix_now();
        let zone = s.local_zone();
        // In ten minutes, reminded fifteen before: due as soon as it is read.
        s.agenda = calendar::Agenda::of(vec![event_ahead("Dentist", now, 10, 15, &zone)]);
        assert_eq!(s.raise_due_reminders(now), 1);
        s.advance_toasts(1_000);
        let id = reminders_raised(&s)[0].id;

        s.open_notification_menu(reminders::APP_NAME.to_owned(), Some(id), 900.0, 500.0, true);
        let (menu, ..) = s.notification_menu.as_ref().expect("the menu is open");
        assert_eq!(menu.items().len(), 3, "{:?}", menu.items());
        match &menu.items()[0] {
            MenuItem::Submenu {
                label, children, ..
            } => {
                assert_eq!(label, "Remind me again");
                let offered: Vec<&str> = children
                    .iter()
                    .filter_map(|c| match c {
                        MenuItem::Action { label, .. } => Some(label.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    offered,
                    [
                        "In 5 minutes",
                        "In 15 minutes",
                        "In 30 minutes",
                        "In 1 hour"
                    ]
                );
            }
            other => panic!("the first row is {other:?}"),
        }

        let press = |s: &mut DesktopShell, row| {
            s.invoke(
                &ShellPart::Menu(ShellMenu::Notification, MenuPart::Item(row)),
                Action::Press,
                0.0,
                0.0,
            )
        };
        press(&mut s, DesktopShell::MENU_REMIND_AGAIN).expect("the times did not open");
        assert!(
            s.notification_menu.is_some(),
            "opening the times closed the menu"
        );
        press(&mut s, DesktopShell::MENU_REMIND_IN + 1).expect("fifteen minutes not chosen");
        assert!(s.notification_menu.is_none(), "the menu stayed");
        let later = DesktopShell::unix_now();
        assert_eq!(s.raise_due_reminders(later), 0);
        let wait = s.next_reminder_in(later).expect("nothing to wake for");
        assert!(
            (Duration::from_mins(14)..=Duration::from_mins(15)).contains(&wait),
            "{wait:?}"
        );

        // Another program's notification: its program's two rows alone.
        let chat = s.notify(notif_pane::Notification {
            id: 0,
            app_name: "Chat".to_owned(),
            title: "Lunch?".to_owned(),
            body: String::new(),
            timestamp: now,
            priority: notif_pane::NotifPriority::Normal,
            read: false,
            action: None,
            silent: false,
        });
        s.open_notification_menu("Chat".to_owned(), Some(chat), 900.0, 500.0, false);
        let (menu, ..) = s.notification_menu.as_ref().expect("the menu is open");
        assert!(
            menu.items()
                .iter()
                .all(|item| matches!(item, MenuItem::Action { .. })),
            "{:?}",
            menu.items()
        );
    });
}

/// **The calendar read anew is looked at again**: an event added since the
/// reminders were worked out brings its reminder forward, without waiting
/// for the next one already worked out.
#[test]
fn a_calendar_read_anew_is_looked_at_again() {
    appearance::config::testing::with_scratch_config("reminder-reread", |root| {
        let dir = root.join("slateos").join("calendar");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("events.txt");
        let mut s = DesktopShell::new(1920, 1080);
        let now = DesktopShell::unix_now();
        let zone = s.local_zone();
        let far = event_ahead("Review", now, 6 * 60, 15, &zone);
        std::fs::write(
            &file,
            calendarstore::calendar_text(std::slice::from_ref(&far)),
        )
        .unwrap();
        assert!(s.refresh_agenda());
        assert_eq!(s.raise_due_reminders(now), 0);
        let first = s.next_reminder_in(now).unwrap();
        assert!(first > Duration::from_hours(5), "{first:?}");

        let near = event_ahead("Dentist", now, 60, 15, &zone);
        std::fs::write(&file, calendarstore::calendar_text(&[far, near])).unwrap();
        assert!(s.refresh_agenda());
        assert!(s.reminders_due(now), "read anew and not looked at again");
        assert_eq!(s.raise_due_reminders(now), 0);
        let next = s.next_reminder_in(now).unwrap();
        assert!(next <= Duration::from_mins(45), "{next:?}");
    });
}
