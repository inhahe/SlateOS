//! Tests for the widget layer as tools see it: each widget a group named by
//! its title, holding what it shows -- a clock's time and date, the meters
//! with their readings or none, a note's text, a frame's picture or what it
//! says instead, the battery's charge -- each where it is drawn; and a note's
//! text set as a paste sets it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::path::PathBuf;

use guitk::event::{Key, KeyEvent, Modifiers};
use guitk::widget::automation::{Node, Role, Value};

use super::WidgetPart;
use crate::Rect;
use crate::power::{BatteryInfo, BatteryState};
use crate::widgets::{
    DesktopWidgetManager, FramePicture, GridPos, LiveReadings, Meter, NoteKey, WidgetInstanceId,
    WidgetKind,
};

/// Readings no widget would draw by accident.
fn readings() -> LiveReadings {
    LiveReadings {
        clock_time: "07:05".to_owned(),
        clock_date: "Tuesday, 3 June".to_owned(),
        cpu_fraction: Some(0.11),
        memory_fraction: Some(0.73),
        disk_fraction: None,
        battery: BatteryInfo::default(),
        month: crate::calendar::MonthGlance::default(),
    }
}

/// A layer holding one widget of `kind`, at the grid's first cell.
fn one(kind: WidgetKind) -> (DesktopWidgetManager, WidgetInstanceId) {
    let mut layer = DesktopWidgetManager::new();
    let id = layer
        .add_widget(kind, GridPos::new(0, 0))
        .expect("the grid is empty");
    (layer, id)
}

/// The node of `part` in `layer`'s tree, with `live`'s readings.
fn node_of(
    layer: &DesktopWidgetManager,
    live: &LiveReadings,
    part: WidgetPart,
) -> Option<Node<WidgetPart>> {
    layer
        .automation(live)
        .and_then(|tree| tree.walk().find(|node| node.id == part).cloned())
}

/// Whether `inner` lies within `outer`.
fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right() + 0.01
        && inner.bottom() <= outer.bottom() + 0.01
}

/// With nothing drawn there is nothing to see: no widgets, a hidden layer,
/// a hidden widget.
#[test]
fn a_widget_not_drawn_is_not_seen() {
    let live = readings();
    assert_eq!(DesktopWidgetManager::new().automation(&live), None);

    let (mut layer, clock) = one(WidgetKind::Clock);
    assert!(layer.automation(&live).is_some());
    layer.layer_visible = false;
    assert_eq!(layer.automation(&live), None, "the hidden layer is seen");

    layer.layer_visible = true;
    let note = layer
        .add_widget(WidgetKind::Notes, GridPos::new(2, 0))
        .expect("free");
    layer.get_mut(clock).expect("placed").visible = false;
    let tree = layer.automation(&live).expect("the note is out");
    let ids: Vec<WidgetPart> = tree.children.iter().map(|node| node.id).collect();
    assert_eq!(
        ids,
        vec![WidgetPart::Widget(note)],
        "the hidden clock is seen"
    );
    assert_eq!(tree.role, Role::Group);
    assert_eq!(tree.name, "Widgets");
}

/// The layer's box is the box round its widgets.
#[test]
fn the_layer_is_the_box_round_its_widgets() {
    let (mut layer, clock) = one(WidgetKind::Clock);
    let note = layer
        .add_widget(WidgetKind::Notes, GridPos::new(3, 2))
        .expect("free");
    let tree = layer.automation(&readings()).expect("drawn");
    let first = node_of(&layer, &readings(), WidgetPart::Widget(clock)).expect("seen");
    let second = node_of(&layer, &readings(), WidgetPart::Widget(note)).expect("seen");
    assert_eq!(tree.bounds, first.bounds.union(second.bounds));
}

/// A clock reads its time and its date, as the taskbar's clock does, the
/// time above the date, both inside it.
#[test]
fn a_clock_reads_its_time_and_date() {
    let (layer, id) = one(WidgetKind::Clock);
    let live = readings();
    let clock = node_of(&layer, &live, WidgetPart::Widget(id)).expect("seen");
    assert_eq!((clock.role, clock.name.as_str()), (Role::Group, "Clock"));
    let (x, y, w, h) = layer.content_rect(id).expect("placed");
    let content = Rect::new(x, y, w, h);
    let time = node_of(&layer, &live, WidgetPart::Time(id)).expect("seen");
    let date = node_of(&layer, &live, WidgetPart::Date(id)).expect("seen");
    assert_eq!((time.role, time.name.as_str()), (Role::Label, "07:05"));
    assert_eq!(
        (date.role, date.name.as_str()),
        (Role::Label, "Tuesday, 3 June")
    );
    assert!(
        time.bounds.bottom() <= date.bounds.y,
        "the date is not below"
    );
    assert!(within(time.bounds, content) && within(date.bounds, content));
}

/// Each meter is a progress bar named as its heading: a measured one holds
/// its reading, of a hundred and within it; one nothing measured holds none
/// and says so.
#[test]
fn the_meters_say_what_was_measured() {
    let (layer, id) = one(WidgetKind::SystemMonitor);
    let live = LiveReadings {
        // Past the whole, which the bar does not draw past either.
        memory_fraction: Some(1.5),
        ..readings()
    };
    let monitor = node_of(&layer, &live, WidgetPart::Widget(id)).expect("seen");
    let parts: Vec<WidgetPart> = monitor.children.iter().map(|node| node.id).collect();
    assert_eq!(
        parts,
        Meter::ALL
            .iter()
            .map(|&meter| WidgetPart::Meter(id, meter))
            .collect::<Vec<_>>()
    );
    let cpu = &monitor.children[0];
    assert_eq!((cpu.role, cpu.name.as_str()), (Role::ProgressBar, "CPU"));
    let Some(Value::Progress { value, max }) = cpu.value else {
        panic!("the measured meter holds no reading: {:?}", cpu.value);
    };
    assert!((value - 11.0).abs() < 0.001 && (max - 100.0).abs() < f32::EPSILON);
    assert_eq!(cpu.description, None);
    assert_eq!(
        monitor.children[1].value,
        Some(Value::Progress {
            value: 100.0,
            max: 100.0
        })
    );
    let disk = &monitor.children[2];
    assert_eq!(disk.name, "Disk");
    assert_eq!(disk.value, None, "a meter nothing measured holds a reading");
    assert_eq!(disk.description.as_deref(), Some("not measured"));
    // Top to bottom, as drawn, inside the widget.
    for pair in monitor.children.windows(2) {
        assert!(pair[0].bounds.bottom() <= pair[1].bounds.y);
    }
    assert!(
        monitor
            .children
            .iter()
            .all(|meter| within(meter.bounds, monitor.bounds))
    );
}

/// A note is a text area named by its title, holding its text, with the
/// keyboard while it is being written in.
#[test]
fn a_note_holds_its_text_and_has_the_keyboard_while_written_in() {
    let (mut layer, id) = one(WidgetKind::Notes);
    layer.get_mut(id).expect("placed").state_text = "milk\neggs".to_owned();
    let live = readings();
    let note = node_of(&layer, &live, WidgetPart::Note(id)).expect("seen");
    assert_eq!(
        (note.role, note.name.as_str()),
        (Role::TextArea, "Quick Notes")
    );
    assert_eq!(note.value, Some(Value::Text("milk\neggs".to_owned())));
    assert!(note.focusable && !note.focused);
    let (x, y, w, h) = layer.content_rect(id).expect("placed");
    assert_eq!(note.bounds, Rect::new(x, y, w, h));

    assert!(layer.note_press(x + w / 2.0, y + h / 2.0, 1));
    let note = node_of(&layer, &live, WidgetPart::Note(id)).expect("seen");
    assert!(note.focused, "the note written in has no keyboard");
}

/// A note's text set as a paste over all of it: the widget's at once, one
/// edit that Ctrl+Z takes back; nothing to do when it is already so, and
/// nothing with no note open.
#[test]
fn a_notes_text_is_set_as_a_paste_over_it() {
    let (mut layer, id) = one(WidgetKind::Notes);
    assert_eq!(layer.note_set_text("x"), NoteKey::NotWriting);
    layer.get_mut(id).expect("placed").state_text = "old words".to_owned();
    let (x, y, w, h) = layer.content_rect(id).expect("placed");
    assert!(layer.note_press(x + w / 2.0, y + h / 2.0, 1));

    assert_eq!(layer.note_set_text("new\nwords"), NoteKey::Changed);
    assert_eq!(layer.get(id).expect("placed").state_text, "new\nwords");
    assert_eq!(layer.note_set_text("new\nwords"), NoteKey::Handled);

    let undo = KeyEvent {
        key: Key::Z,
        pressed: true,
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        text: String::new(),
    };
    assert_eq!(layer.note_key(&undo), NoteKey::Changed);
    assert_eq!(
        layer.get(id).expect("placed").state_text,
        "old words",
        "one Ctrl+Z does not take the paste back"
    );
}

/// A note whose widget went while it was open is closed by setting its
/// text, as by a key.
#[test]
fn setting_the_text_of_a_note_gone_closes_it() {
    let (mut layer, id) = one(WidgetKind::Notes);
    let (x, y, w, h) = layer.content_rect(id).expect("placed");
    assert!(layer.note_press(x + w / 2.0, y + h / 2.0, 1));
    // Gone from under the open note without `remove_widget`, which would
    // have closed it: as a layout read back over it does.
    layer.read_from(&yamldoc::Document::default());
    assert_eq!(layer.note_set_text("x"), NoteKey::Closed);
    assert_eq!(layer.writing_note(), None);
}

/// A photo frame shows tools what it shows: nothing while its first picture
/// is on its way, what it says when it has none, and the picture up on it,
/// named by its file and boxed where it is drawn.
#[test]
fn a_photo_frame_names_its_picture_or_says_why_not() {
    let (mut layer, id) = one(WidgetKind::PhotoFrame);
    let live = readings();
    let frame = node_of(&layer, &live, WidgetPart::Widget(id)).expect("seen");
    assert!(frame.children.is_empty(), "nothing drawn, something seen");

    let folder = PathBuf::from("/home/ann/Pictures");
    layer.step_frames(Some(&folder), &|_| Vec::new());
    let said = node_of(&layer, &live, WidgetPart::Picture(id)).expect("seen");
    assert_eq!(
        (said.role, said.name.as_str()),
        (Role::Label, "No pictures in Pictures")
    );

    // A picture in the folder by the end of its interval, when it looks again.
    let beach = folder.join("beach.jpg");
    layer.tick(u64::MAX);
    layer.step_frames(Some(&folder), &|_| vec![beach.clone()]);
    assert!(layer.frame_wants(id, &beach));
    layer.frame_picture_ready(
        id,
        FramePicture {
            path: beach.clone(),
            image_id: crate::widgets::FRAME_PICTURE_TAG | 1,
            width: 400,
            height: 100,
            fit: (400, 100),
        },
    );
    let picture = node_of(&layer, &live, WidgetPart::Picture(id)).expect("seen");
    assert_eq!(
        (picture.role, picture.name.as_str()),
        (Role::Image, "beach.jpg")
    );
    // The whole path, as the file dialog writes one -- with the separator
    // the host's `join` used.
    assert_eq!(picture.description, Some(pathcodec::display_path(&beach)));
    let (x, y, w, h) = layer.content_rect(id).expect("placed");
    assert!(within(picture.bounds, Rect::new(x, y, w, h)));
    // Fitted by its own proportions: four times as wide as it is tall.
    assert!((picture.bounds.w / picture.bounds.h - 4.0).abs() < 0.01);
}

/// A calendar shows tools its month: a grid named by the month and year,
/// each day a cell named by its date and described as the popup's days are,
/// seven to a row, top to bottom, inside the widget -- and no grid while it
/// has no month to show.
#[test]
fn a_calendar_shows_its_month() {
    use crate::calendar::{CalendarConfig, CalendarEvent, CalendarView, EventStore};
    let (layer, id) = one(WidgetKind::Calendar);
    let empty = node_of(&layer, &readings(), WidgetPart::Widget(id)).expect("seen");
    assert!(empty.children.is_empty(), "a month of nothing is seen");

    // 12:00 UTC on 6 October 2026, with an event then.
    let noon = 1_791_288_000;
    let mut store = EventStore::new();
    store
        .add_event(CalendarEvent {
            id: 0,
            title: "Dentist".to_owned(),
            start_timestamp: noon,
            end_timestamp: noon + 3_600,
            all_day: false,
            repeat: None,
            color: None,
            description: String::new(),
        })
        .expect("an id");
    let live = LiveReadings {
        month: CalendarView::new(CalendarConfig::default()).month_glance(
            noon,
            &tzrules::Tz::UTC,
            &store,
        ),
        ..readings()
    };
    let month = node_of(&layer, &live, WidgetPart::Month(id)).expect("seen");
    assert_eq!(
        (month.role, month.name.as_str()),
        (Role::Grid, "October 2026")
    );
    assert_eq!(month.children.len(), 42);
    let first = &month.children[0];
    assert_eq!(
        (first.id, first.role, first.name.as_str()),
        (
            WidgetPart::Day(id, 0),
            Role::GridCell,
            "Sunday 27 September 2026"
        )
    );
    assert_eq!(first.description.as_deref(), Some("another month"));
    let today = &month.children[9];
    assert_eq!(today.name, "Tuesday 6 October 2026");
    assert_eq!(today.description.as_deref(), Some("today, 1 event"));

    let (x, y, w, h) = layer.content_rect(id).expect("placed");
    let content = Rect::new(x, y, w, h);
    assert!(month.children.iter().all(|day| within(day.bounds, content)));
    for week in month.children.chunks(7) {
        for pair in week.windows(2) {
            assert!(pair[0].bounds.right() <= pair[1].bounds.x + 0.01);
        }
    }
    for column in 0..7 {
        let (above, below) = (&month.children[column], &month.children[column + 7]);
        assert!(above.bounds.bottom() <= below.bounds.y + 0.01);
    }
}

/// The battery widget says its charge -- or that there is no battery, which
/// is not a flat one -- and what its icon shows of it, and the time left
/// where something says.
#[test]
fn the_battery_says_its_charge() {
    let (layer, id) = one(WidgetKind::BatteryStatus);
    let charge = |battery: BatteryInfo| {
        let live = LiveReadings {
            battery,
            ..readings()
        };
        node_of(&layer, &live, WidgetPart::Charge(id)).expect("seen")
    };
    let none = charge(BatteryInfo::default());
    assert_eq!((none.role, none.name.as_str()), (Role::Label, "No battery"));
    assert_eq!(none.description, None);

    let charging = charge(BatteryInfo {
        present: true,
        charge_pct: 37,
        state: BatteryState::Charging,
        time_remaining_secs: Some(7_500),
        ..BatteryInfo::default()
    });
    assert_eq!(charging.name, "37%");
    assert_eq!(
        charging.description.as_deref(),
        Some("charging, 2h 05m remaining")
    );
    let low = charge(BatteryInfo {
        present: true,
        charge_pct: 0,
        state: BatteryState::Critical,
        ..BatteryInfo::default()
    });
    assert_eq!(
        (low.name.as_str(), low.description.as_deref()),
        ("0%", Some("critically low"))
    );
    let plain = charge(BatteryInfo {
        present: true,
        charge_pct: 80,
        state: BatteryState::Discharging,
        ..BatteryInfo::default()
    });
    assert_eq!(plain.description, None);
}

/// A widget with no content of its own yet shows its kind's name where its
/// content will be, and is named by its own title, which may be the user's.
#[test]
fn a_widget_with_nothing_to_show_says_what_it_is() {
    let (mut layer, id) = one(WidgetKind::Weather);
    layer.get_mut(id).expect("placed").title_override = Some("Outside".to_owned());
    let live = readings();
    let widget = node_of(&layer, &live, WidgetPart::Widget(id)).expect("seen");
    assert_eq!(widget.name, "Outside");
    let shown = node_of(&layer, &live, WidgetPart::Placeholder(id)).expect("seen");
    assert_eq!((shown.role, shown.name.as_str()), (Role::Label, "Weather"));
    assert!(within(shown.bounds, widget.bounds));
}
