//! Tests for asking for a file: against a stand-in for the chooser that
//! records what it is asked and answers what a test tells it to.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Wake, Waker};
use std::time::{Duration, Instant};

use guiremote::client::Transport;
use guitk::dialog::{DialogMode, FileDialog, Picked};
use guitk::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};

use super::Picker;
use crate::protocol::{self, Decoded, Filter, MAX_PATH, Mode, Reply, Request};
use svcconn::{Connect, NoConn, SystemConnect};

/// What the stand-in chooser and the picker's connection share.
#[derive(Debug, Default)]
struct Line {
    /// What the picker sent.
    sent: Vec<u8>,
    /// What the chooser has said, not yet read.
    said: VecDeque<u8>,
    /// Whether the chooser has hung up.
    hung_up: bool,
    /// Whether the picker's end has been dropped.
    dropped: bool,
}

/// The picker's end of a connection to the stand-in.
#[derive(Debug)]
struct End(Arc<Mutex<Line>>);

impl Drop for End {
    fn drop(&mut self) {
        self.0.lock().unwrap().dropped = true;
    }
}

impl Transport for End {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let mut line = self.0.lock().unwrap();
        let count = line.said.len();
        buf.extend(line.said.drain(..));
        Ok(count)
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.lock().unwrap().sent.extend_from_slice(bytes);
        Ok(())
    }

    fn is_open(&self) -> bool {
        let line = self.0.lock().unwrap();
        !(line.hung_up && line.said.is_empty())
    }

    fn wait(&mut self) -> io::Result<()> {
        std::thread::sleep(Duration::from_millis(1));
        Ok(())
    }
}

/// A stand-in chooser: present or not, and the line to it once asked.
#[derive(Debug, Default)]
struct Stand {
    present: bool,
    line: Arc<Mutex<Line>>,
}

impl Stand {
    fn present() -> Self {
        Self {
            present: true,
            ..Self::default()
        }
    }

    /// What the picker asked.
    fn asked(&self) -> Request {
        match protocol::decode_request(&self.line.lock().unwrap().sent) {
            Decoded::Complete(request, _) => request,
            other => panic!("not a request: {other:?}"),
        }
    }

    /// Answer `reply`.
    fn answer(&self, reply: &Reply) {
        self.say(&protocol::encode_reply(reply).unwrap());
    }

    /// Say `bytes`, whatever they are.
    fn say(&self, bytes: &[u8]) {
        self.line.lock().unwrap().said.extend(bytes.iter().copied());
    }

    fn hang_up(&self) {
        self.line.lock().unwrap().hung_up = true;
    }

    fn dropped(&self) -> bool {
        self.line.lock().unwrap().dropped
    }
}

/// How a picker reaches the stand-in.
#[derive(Debug)]
struct Chooser(Arc<Stand>);

impl Connect for Chooser {
    type Conn = End;

    fn connect(&self, _service: &str) -> io::Result<Option<End>> {
        Ok(self.0.present.then(|| End(Arc::clone(&self.0.line))))
    }
}

/// A waker that counts its wakes.
#[derive(Debug, Default)]
struct Count(AtomicUsize);

impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Wait, up to five seconds, for `done`.
fn eventually(done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(5), "never happened");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A picker asking `stand`, with a counting waker.
fn picker_for(stand: &Arc<Stand>) -> (Picker<Chooser>, Arc<Count>) {
    let count = Arc::new(Count::default());
    let picker = Picker::with_connect(Chooser(Arc::clone(stand)))
        .with_waker(Waker::from(Arc::clone(&count)));
    (picker, count)
}

/// A save dialog for `report.pdf` in a folder of its own, offering PDF.
fn save_dialog() -> FileDialog {
    FileDialog::save()
        .with_initial_path(std::env::temp_dir())
        .with_filter("PDF", &["*.pdf"])
        .with_filename("report.pdf")
}

fn key(k: Key) -> Event {
    Event::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: Modifiers::NONE,
        text: String::new(),
    })
}

fn click() -> Event {
    Event::Mouse(MouseEvent {
        x: 5.0,
        y: 5.0,
        kind: MouseEventKind::Press(MouseButton::Left),
    })
}

/// **Where there is no chooser, the toolkit's dialog is drawn**, as before
/// -- on a development host, and wherever nothing serves the name.
#[test]
fn where_there_is_no_chooser_the_toolkits_dialog_is_drawn() {
    let mut system = Picker::new();
    system.put_up(save_dialog(), true);
    assert!(system.local().is_open() && !system.is_asking());
    assert!(system.is_open() && system.is_saving());

    let stand = Arc::new(Stand::default());
    let (mut absent, _) = picker_for(&stand);
    absent.open_to_read();
    assert!(absent.local().is_open() && !absent.is_asking());
    // The no-connection type's one property: it has no values.
    assert!(
        SystemConnect
            .connect(crate::SERVICE)
            .is_ok_and(|conn: Option<NoConn>| conn.is_none())
    );
}

/// **The chooser is asked what the dialog asks**: its mode, where it starts,
/// the name offered, the filters as the dialog offers them and the one in
/// force, and the window asking.
#[test]
fn the_chooser_is_asked_what_the_dialog_asks() {
    let stand = Arc::new(Stand::present());
    let (mut picker, _) = picker_for(&stand);
    picker.set_owner(42);
    picker.put_up(save_dialog(), true);
    assert!(picker.is_asking() && !picker.local().is_open());
    assert!(picker.is_open() && picker.is_saving());
    assert_eq!(
        stand.asked(),
        Request {
            mode: Mode::Save,
            owner: 42,
            title: String::new(),
            start: std::env::temp_dir(),
            name: OsString::from("report.pdf"),
            filters: vec![
                Filter {
                    label: "PDF".into(),
                    patterns: vec!["*.pdf".into()],
                },
                Filter {
                    label: "All files".into(),
                    patterns: vec!["*".into()],
                },
            ],
            filter: 0,
        }
    );
    // Opening offers no name.
    let stand = Arc::new(Stand::present());
    let (mut opening, _) = picker_for(&stand);
    opening.open_to_read();
    assert_eq!(stand.asked().mode, Mode::Open);
    assert!(stand.asked().name.is_empty());
}

/// **The chooser's answer wakes the program and is collected**: the path
/// chosen, or that nothing was.
#[test]
fn the_answer_wakes_the_program_and_is_collected() {
    let stand = Arc::new(Stand::present());
    let (mut picker, wakes) = picker_for(&stand);
    picker.put_up(save_dialog(), true);
    assert_eq!(picker.poll(), Picked::Ignored);
    let chosen = std::env::temp_dir().join("report.pdf");
    stand.answer(&Reply::Chosen {
        path: chosen.clone(),
        filter: 0,
    });
    eventually(|| wakes.0.load(Ordering::SeqCst) == 1);
    assert_eq!(picker.poll(), Picked::Chose(chosen));
    assert!(!picker.is_open());
    assert_eq!(picker.poll(), Picked::Ignored);

    let stand = Arc::new(Stand::present());
    let (mut picker, wakes) = picker_for(&stand);
    picker.open_to_read();
    stand.answer(&Reply::Cancelled);
    eventually(|| wakes.0.load(Ordering::SeqCst) == 1);
    // Collected by the next event as well as by `poll`.
    assert_eq!(picker.handle(&click(), 100.0, 100.0), Picked::Cancelled);
    assert!(!picker.is_open());
}

/// **While the chooser has it, the program's window is behind a dialog**: a
/// key or a click does nothing, Escape gives up asking -- and the chooser's
/// line is let go -- and time still reaches the program.
#[test]
fn while_the_chooser_has_it_the_window_is_behind_a_dialog() {
    let stand = Arc::new(Stand::present());
    let (mut picker, _) = picker_for(&stand);
    picker.open_to_read();
    assert_eq!(
        picker.handle(&key(Key::Space), 100.0, 100.0),
        Picked::Handled
    );
    assert_eq!(picker.handle(&click(), 100.0, 100.0), Picked::Handled);
    let tick = Event::Tick { elapsed_ms: 16 };
    assert_eq!(picker.handle(&tick, 100.0, 100.0), Picked::Ignored);
    assert_eq!(
        picker.handle(&key(Key::Escape), 100.0, 100.0),
        Picked::Cancelled
    );
    assert!(!picker.is_open());
    eventually(|| stand.dropped());
}

/// **If the chooser fails, the toolkit's dialog comes up instead**, with the
/// same settings: when it hangs up without answering, and when it answers
/// something that is not an answer.
#[test]
fn if_the_chooser_fails_the_toolkits_dialog_comes_up() {
    for fail in [Stand::hang_up as fn(&Stand), |stand: &Stand| {
        stand.say(b"\x05\0\0\0nope!");
    }] {
        let stand = Arc::new(Stand::present());
        let (mut picker, wakes) = picker_for(&stand);
        picker.put_up(save_dialog(), true);
        fail(&stand);
        eventually(|| wakes.0.load(Ordering::SeqCst) == 1);
        assert_eq!(picker.poll(), Picked::Handled);
        assert!(!picker.is_asking() && picker.local().is_open());
        let dialog = picker.local().dialog().unwrap();
        assert_eq!(dialog.mode(), DialogMode::Save);
        assert_eq!(dialog.filename(), OsString::from("report.pdf"));
        assert!(picker.is_saving());
    }
}

/// **A request the protocol cannot carry is drawn here**: a start folder
/// longer than a path may be is not sent, and the toolkit's dialog asks.
#[test]
fn a_request_the_protocol_cannot_carry_is_drawn_here() {
    let stand = Arc::new(Stand::present());
    let (mut picker, _) = picker_for(&stand);
    let deep = std::env::temp_dir().join("d".repeat(MAX_PATH));
    picker.put_up(FileDialog::open().with_initial_path(&deep), false);
    assert!(!picker.is_asking() && picker.local().is_open());
    assert!(stand.line.lock().unwrap().sent.is_empty());
}

/// **Closing lets the chooser go**: its line is dropped, so its window goes,
/// and a later answer is not collected.
#[test]
fn closing_lets_the_chooser_go() {
    let stand = Arc::new(Stand::present());
    let (mut picker, _) = picker_for(&stand);
    picker.open_to_read();
    picker.close();
    assert!(!picker.is_open());
    eventually(|| stand.dropped());
    stand.answer(&Reply::Chosen {
        path: std::env::temp_dir(),
        filter: 0,
    });
    assert_eq!(picker.poll(), Picked::Ignored);
}
