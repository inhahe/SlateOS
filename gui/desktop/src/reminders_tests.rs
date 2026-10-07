//! Tests for the calendar's reminders: when each is due, that each is raised
//! once, what is put off, what is kept on disk, and what a reminder says.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use calendarstore::{CalendarEvent, Date, DateTime, EventCategory, RecurrenceRule, Reminder, Time};
use tzrules::Tz;

use super::{
    DAY_SECS, Due, HORIZON_SECS, LOOKBACK_SECS, MAX_LEAD_SECS, ReminderKey, Reminders, body, lead,
    snooze_label, span, when,
};

const MINUTE: i64 = 60;
const HOUR: i64 = 3600;

/// The UTC instant of `(year, month, day)` at `hour:minute`, UTC.
fn utc((year, month, day): (i32, u32, u32), hour: i64, minute: i64) -> i64 {
    tzrules::days_from_civil(i64::from(year), month, day) * DAY_SECS + hour * HOUR + minute * MINUTE
}

/// New York, as the desktop's zone table spells it.
fn new_york() -> Tz {
    Tz::parse(b"EST5EDT,M3.2.0,M11.1.0").unwrap()
}

/// Event `id`, "Dentist", on `(year, month, day)` at `hour:minute` for
/// `long` minutes, repeating as `recurrence`, reminded as `reminder`.
fn event(
    id: u64,
    (year, month, day): (i32, u32, u32),
    (hour, minute): (u32, u32),
    long: u32,
    recurrence: RecurrenceRule,
    reminder: Reminder,
) -> CalendarEvent {
    let date = Date { year, month, day };
    let start = Time { hour, minute };
    let end_minutes = (hour * 60 + minute + long).min(23 * 60 + 59);
    CalendarEvent {
        id,
        title: "Dentist".to_owned(),
        description: String::new(),
        category: EventCategory::Health,
        start: DateTime::new(date, start),
        end: DateTime::new(date, Time::from_minutes(end_minutes)),
        all_day: false,
        recurrence,
        reminder,
        location: None,
        color_override: None,
    }
}

/// An all-day event `id` from `first` to `last`, reminded as `reminder`.
fn all_day(
    id: u64,
    first: (i32, u32, u32),
    last: (i32, u32, u32),
    reminder: Reminder,
) -> CalendarEvent {
    let date = |(year, month, day): (i32, u32, u32)| Date { year, month, day };
    CalendarEvent {
        all_day: true,
        start: DateTime::new(date(first), Time { hour: 0, minute: 0 }),
        end: DateTime::new(
            date(last),
            Time {
                hour: 23,
                minute: 59,
            },
        ),
        ..event(id, first, (0, 0), 0, RecurrenceRule::None, reminder)
    }
}

fn starts(due: &[Due]) -> Vec<i64> {
    due.iter().map(|d| d.key.starts).collect()
}

#[test]
fn a_reminders_lead_is_its_minutes_hours_or_day() {
    assert_eq!(lead(Reminder::None), None);
    assert_eq!(lead(Reminder::AtTime), Some(0));
    assert_eq!(lead(Reminder::MinutesBefore(15)), Some(15 * MINUTE));
    assert_eq!(lead(Reminder::HoursBefore(2)), Some(2 * HOUR));
    assert_eq!(lead(Reminder::DayBefore), Some(DAY_SECS));
    // An imported calendar may ask anything; a month ahead is the most.
    assert_eq!(lead(Reminder::HoursBefore(u32::MAX)), Some(MAX_LEAD_SECS));
}

/// **A reminder is due at its start less its lead, and raised once.**
#[test]
fn a_reminder_is_raised_at_its_time_and_once() {
    let events = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::MinutesBefore(15),
    )];
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    assert!(
        r.take_due(&events, &Tz::utc(), nine - 15 * MINUTE - 1)
            .is_empty()
    );
    let due = r.take_due(&events, &Tz::utc(), nine - 15 * MINUTE);
    assert_eq!(
        due,
        [Due {
            key: ReminderKey {
                event: 1,
                starts: nine
            },
            title: "Dentist".to_owned(),
            place: None,
            all_day: false,
        }]
    );
    assert!(
        r.take_due(&events, &Tz::utc(), nine - 14 * MINUTE)
            .is_empty(),
        "raised twice"
    );
    assert!(r.take_due(&events, &Tz::utc(), nine).is_empty());
}

/// **A start is read in the desktop's zone**: an event at nine in New York
/// is reminded of at a quarter to nine there, summer or winter -- a
/// different instant in July than in January.
#[test]
fn a_reminder_follows_the_zones_clock_through_the_year() {
    let events = [event(
        1,
        (2026, 1, 15),
        (9, 0),
        30,
        RecurrenceRule::Weekly { days: Vec::new() },
        Reminder::MinutesBefore(15),
    )];
    let zone = new_york();
    // 15 January: Eastern Standard Time, five hours behind.
    let winter = utc((2026, 1, 15), 14, 0);
    // 2 July is a Thursday too: daylight time, four hours behind.
    let summer = utc((2026, 7, 2), 13, 0);
    let mut r = Reminders::new();
    assert_eq!(
        starts(&r.take_due(&events, &zone, winter - 15 * MINUTE)),
        [winter]
    );
    assert_eq!(
        r.next_due(&events, &zone, winter - 15 * MINUTE),
        Some(winter + 7 * DAY_SECS - 15 * MINUTE),
        "the next is a week on"
    );
    let mut r = Reminders::new();
    assert!(r.take_due(&events, &zone, summer - 16 * MINUTE).is_empty());
    assert_eq!(
        starts(&r.take_due(&events, &zone, summer - 15 * MINUTE)),
        [summer]
    );
}

/// **One fallen due while nobody looked is raised late -- unless its event
/// is over.**
#[test]
fn a_missed_reminder_is_raised_late_but_not_after_its_event() {
    let events = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::MinutesBefore(15),
    )];
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    assert_eq!(
        starts(&r.take_due(&events, &Tz::utc(), nine + 30 * MINUTE)),
        [nine],
        "the meeting is still on"
    );
    let mut r = Reminders::new();
    assert!(
        r.take_due(&events, &Tz::utc(), nine + 60 * MINUTE)
            .is_empty(),
        "a reminder of a meeting already over"
    );
}

/// **An event added with less notice than its lead is reminded of at once.**
#[test]
fn an_event_added_inside_its_lead_is_reminded_of_at_once() {
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    assert!(r.take_due(&[], &Tz::utc(), nine - 10 * MINUTE).is_empty());
    let events = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::MinutesBefore(15),
    )];
    assert_eq!(
        r.next_due(&events, &Tz::utc(), nine - 10 * MINUTE),
        Some(nine - 10 * MINUTE),
        "due now"
    );
    assert_eq!(
        starts(&r.take_due(&events, &Tz::utc(), nine - 10 * MINUTE)),
        [nine]
    );
}

/// **Each occurrence of a repeating event has its own reminder**, and an
/// event moved to another time has a new one.
#[test]
fn each_occurrence_and_each_move_has_its_own_reminder() {
    let mut events = vec![event(
        1,
        (2026, 10, 8),
        (9, 0),
        30,
        RecurrenceRule::Daily,
        Reminder::AtTime,
    )];
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    assert_eq!(starts(&r.take_due(&events, &Tz::utc(), nine)), [nine]);
    assert_eq!(
        starts(&r.take_due(&events, &Tz::utc(), nine + DAY_SECS)),
        [nine + DAY_SECS],
        "the next day's"
    );
    // Moved to eleven on the third day: a reminder of its own.
    events[0].start.time = Time {
        hour: 11,
        minute: 0,
    };
    events[0].end.time = Time {
        hour: 11,
        minute: 30,
    };
    let eleven = utc((2026, 10, 10), 11, 0);
    assert!(r.take_due(&events, &Tz::utc(), eleven - 1).is_empty());
    assert_eq!(starts(&r.take_due(&events, &Tz::utc(), eleven)), [eleven]);
}

/// **An all-day event starts at midnight**, so "a day before" is midnight
/// the day before -- and one of several days has one reminder, before its
/// first.
#[test]
fn an_all_day_events_reminder_is_from_its_first_midnight() {
    let events = [all_day(
        1,
        (2026, 10, 8),
        (2026, 10, 10),
        Reminder::DayBefore,
    )];
    let midnight = utc((2026, 10, 8), 0, 0);
    let mut r = Reminders::new();
    assert!(
        r.take_due(&events, &Tz::utc(), midnight - DAY_SECS - 1)
            .is_empty()
    );
    assert_eq!(
        starts(&r.take_due(&events, &Tz::utc(), midnight - DAY_SECS)),
        [midnight]
    );
    // Its second and third days are no occurrences of their own.
    for day in 1..3 {
        assert!(
            r.take_due(&events, &Tz::utc(), midnight + day * DAY_SECS)
                .is_empty()
        );
    }
    // Missed, it is still raised while it is on and began inside the look
    // back; once it began longer ago than that, it is stale.
    let mut r = Reminders::new();
    assert_eq!(
        starts(&r.take_due(&events, &Tz::utc(), midnight + LOOKBACK_SECS - HOUR)),
        [midnight],
        "twenty-three hours in"
    );
    let mut r = Reminders::new();
    assert!(
        r.take_due(&events, &Tz::utc(), midnight + LOOKBACK_SECS + HOUR)
            .is_empty(),
        "a day and an hour in"
    );

    // An all-day event of one day is on until the midnight after it: missed
    // at its time, it is raised during its day.
    let one_day = [all_day(2, (2026, 10, 8), (2026, 10, 8), Reminder::AtTime)];
    let mut r = Reminders::new();
    assert_eq!(
        starts(&r.take_due(&one_day, &Tz::utc(), midnight + 10 * HOUR)),
        [midnight],
        "over at its own midnight"
    );
    let mut r = Reminders::new();
    assert!(
        r.take_due(&one_day, &Tz::utc(), midnight + DAY_SECS)
            .is_empty(),
        "on past its day"
    );
}

/// **A reminder put off comes again then** -- if its event is still there.
#[test]
fn a_reminder_put_off_comes_again_if_its_event_is_still_there() {
    let events = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::MinutesBefore(15),
    )];
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    let due = r.take_due(&events, &Tz::utc(), nine - 15 * MINUTE);
    let key = due[0].key;
    let revision = r.revision();
    r.snooze(key, nine - 10 * MINUTE);
    assert_ne!(r.revision(), revision, "a change nothing would write");
    assert_eq!(
        r.next_due(&events, &Tz::utc(), nine - 14 * MINUTE),
        Some(nine - 10 * MINUTE)
    );
    assert!(
        r.take_due(&events, &Tz::utc(), nine - 11 * MINUTE)
            .is_empty()
    );
    assert_eq!(
        starts(&r.take_due(&events, &Tz::utc(), nine - 10 * MINUTE)),
        [nine]
    );
    assert!(
        r.take_due(&events, &Tz::utc(), nine - 5 * MINUTE)
            .is_empty(),
        "it came twice"
    );

    // Put off past the event's end: nothing comes, and nothing is left.
    let mut late = r.clone();
    late.snooze(key, nine + 2 * HOUR);
    assert!(
        late.take_due(&events, &Tz::utc(), nine + 2 * HOUR)
            .is_empty(),
        "a reminder of a meeting over"
    );
    assert_eq!(late.next_due(&events, &Tz::utc(), nine + 2 * HOUR), None);

    // Put off again, and the event deleted meanwhile: nothing comes.
    r.snooze(key, nine - 5 * MINUTE);
    assert!(r.take_due(&[], &Tz::utc(), nine - 5 * MINUTE).is_empty());
    assert_eq!(
        r.next_due(&[], &Tz::utc(), nine),
        None,
        "the put-off one outlived its event"
    );
}

/// **The next reminder**: none with no reminders, the soonest with some,
/// the horizon with none that soon.
#[test]
fn the_next_reminder_is_the_soonest_or_a_look_again_at_the_horizon() {
    let now = utc((2026, 10, 8), 8, 0);
    let r = Reminders::new();
    let quiet = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::None,
    )];
    assert_eq!(r.next_due(&quiet, &Tz::utc(), now), None);
    let two = [
        event(
            1,
            (2026, 10, 8),
            (11, 0),
            60,
            RecurrenceRule::None,
            Reminder::HoursBefore(1),
        ),
        event(
            2,
            (2026, 10, 8),
            (9, 30),
            60,
            RecurrenceRule::None,
            Reminder::MinutesBefore(5),
        ),
    ];
    assert_eq!(
        r.next_due(&two, &Tz::utc(), now),
        Some(utc((2026, 10, 8), 9, 25))
    );
    let far = [event(
        1,
        (2027, 1, 1),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::AtTime,
    )];
    assert_eq!(
        r.next_due(&far, &Tz::utc(), now),
        Some(now + HORIZON_SECS),
        "looked for again a week on"
    );
}

/// **What is raised and put off survives being written and read**; a line
/// this version cannot read is left out, and a file of another format reads
/// as nothing.
#[test]
fn the_reminders_kept_on_disk_read_back() {
    let events = [event(
        7,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::AtTime,
    )];
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    let due = r.take_due(&events, &Tz::utc(), nine);
    r.snooze(due[0].key, nine + 5 * MINUTE);
    let text = r.encode();
    assert_eq!(
        text,
        format!(
            "slateos-reminders 1\nraised\t7\t{nine}\nagain\t7\t{nine}\t{}\n",
            nine + 5 * MINUTE
        )
    );
    let back = Reminders::decode(text.as_bytes());
    assert_eq!(back.encode(), text);
    // Read back, it is not raised again.
    let mut back = back;
    assert!(back.take_due(&events, &Tz::utc(), nine + MINUTE).is_empty());

    let mangled = format!("{text}raised\tseven\t1\nwhat\n");
    assert_eq!(Reminders::decode(mangled.as_bytes()).encode(), text);
    assert_eq!(
        Reminders::decode(b"slateos-reminders 2\nraised\t7\t1\n"),
        Reminders::new()
    );
    assert_eq!(Reminders::decode(&[0xFF, 0xFE]), Reminders::new());
}

/// **A reminder raised is forgotten once nothing looks back that far**, and
/// forgetting it is a change to write.
#[test]
fn a_reminder_raised_is_forgotten_after_the_look_back() {
    let events = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::AtTime,
    )];
    let nine = utc((2026, 10, 8), 9, 0);
    let mut r = Reminders::new();
    let _ = r.take_due(&events, &Tz::utc(), nine);
    assert!(r.encode().contains("raised"));
    let revision = r.revision();
    let _ = r.take_due(&events, &Tz::utc(), nine + LOOKBACK_SECS);
    assert!(r.encode().contains("raised"), "forgotten too soon");
    let _ = r.take_due(&events, &Tz::utc(), nine + LOOKBACK_SECS + 1);
    assert!(!r.encode().contains("raised"), "never forgotten");
    assert_ne!(r.revision(), revision);
}

fn due_at(starts: i64, all_day: bool) -> Due {
    Due {
        key: ReminderKey { event: 1, starts },
        title: "Dentist".to_owned(),
        place: None,
        all_day,
    }
}

/// **What a reminder says of when**, from the moment it is raised.
#[test]
fn a_reminder_says_when_as_a_person_would() {
    // Thursday 8 October 2026.
    let nine = utc((2026, 10, 8), 9, 0);
    let utc_zone = Tz::utc();
    let says = |starts: i64, now: i64| when(&due_at(starts, false), now, &utc_zone);
    assert_eq!(says(nine, nine - 15 * MINUTE), "In 15 minutes, at 09:00");
    assert_eq!(
        says(nine, nine - 14 * MINUTE - 30),
        "In 15 minutes, at 09:00"
    );
    assert_eq!(
        says(nine, nine - 90 * MINUTE),
        "In 1 hour 30 minutes, at 09:00"
    );
    assert_eq!(says(nine, nine - 2 * HOUR), "In 2 hours, at 09:00");
    assert_eq!(says(nine, nine - 30), "Now, at 09:00");
    assert_eq!(
        says(nine, nine + 5 * MINUTE),
        "Started 5 minutes ago, at 09:00"
    );
    assert_eq!(says(nine, nine - DAY_SECS), "Tomorrow at 09:00");
    assert_eq!(
        says(nine + DAY_SECS, nine - DAY_SECS),
        "On Friday 9 October at 09:00"
    );
    // In the zone's own hours.
    let zone = new_york();
    assert_eq!(
        when(&due_at(nine, false), nine - HOUR, &zone),
        "In 1 hour, at 05:00"
    );

    let midnight = utc((2026, 10, 8), 0, 0);
    let all = |now: i64| when(&due_at(midnight, true), now, &utc_zone);
    assert_eq!(all(midnight + HOUR), "Today");
    assert_eq!(all(midnight - HOUR), "Tomorrow");
    assert_eq!(all(midnight - 2 * DAY_SECS), "On Thursday 8 October");
}

/// **Its body is when, and where if the calendar says.**
#[test]
fn a_reminders_body_is_when_and_where() {
    let nine = utc((2026, 10, 8), 9, 0);
    let mut due = due_at(nine, false);
    assert_eq!(
        body(&due, nine - 15 * MINUTE, &Tz::utc()),
        "In 15 minutes, at 09:00"
    );
    due.place = Some("Room 3".to_owned());
    assert_eq!(
        body(&due, nine - 15 * MINUTE, &Tz::utc()),
        "In 15 minutes, at 09:00. Room 3"
    );
}

/// **A place of nothing but spaces is no place.**
#[test]
fn a_blank_place_is_left_out() {
    let mut events = [event(
        1,
        (2026, 10, 8),
        (9, 0),
        60,
        RecurrenceRule::None,
        Reminder::AtTime,
    )];
    events[0].location = Some("  ".to_owned());
    let nine = utc((2026, 10, 8), 9, 0);
    let due = Reminders::new().take_due(&events, &Tz::utc(), nine);
    assert_eq!(due[0].place, None);
    events[0].location = Some("Room 3".to_owned());
    let due = Reminders::new().take_due(&events, &Tz::utc(), nine);
    assert_eq!(due[0].place.as_deref(), Some("Room 3"));
}

#[test]
fn a_span_is_said_to_the_minute_rounded_up() {
    assert_eq!(span(1), "1 minute");
    assert_eq!(span(60), "1 minute");
    assert_eq!(span(61), "2 minutes");
    assert_eq!(span(45 * MINUTE), "45 minutes");
    assert_eq!(span(HOUR), "1 hour");
    assert_eq!(span(2 * HOUR + 10 * MINUTE), "2 hours 10 minutes");
    assert_eq!(span(HOUR + MINUTE), "1 hour 1 minute");
}

#[test]
fn the_times_to_come_again_in_are_offered_in_words() {
    let offered: Vec<String> = super::SNOOZES.iter().map(|&m| snooze_label(m)).collect();
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
