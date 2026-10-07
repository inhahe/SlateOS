//! Tests for the chooser's side: a program's request read whole however it
//! arrives, a program that misbehaves refused, and an answer checked
//! against what was asked.

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
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use guiremote::client::Transport;

use super::Asked;
use crate::protocol::{self, Decoded, Filter, Mode, Reply, Request};

/// What the program has sent, in the pieces it sent it, whether it has hung
/// up, and what it has been answered.
#[derive(Debug, Default)]
struct Line {
    pieces: VecDeque<Vec<u8>>,
    hung_up: bool,
    answered: Vec<u8>,
}

/// The chooser's end: each read takes the next piece the program sent.
#[derive(Debug, Clone, Default)]
struct End(Arc<Mutex<Line>>);

impl Transport for End {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let piece = self
            .0
            .lock()
            .unwrap()
            .pieces
            .pop_front()
            .unwrap_or_default();
        buf.extend_from_slice(&piece);
        Ok(piece.len())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut line = self.0.lock().unwrap();
        if line.hung_up {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        line.answered.extend_from_slice(bytes);
        Ok(())
    }

    fn is_open(&self) -> bool {
        let line = self.0.lock().unwrap();
        !(line.hung_up && line.pieces.is_empty())
    }

    fn wait(&mut self) -> io::Result<()> {
        std::thread::sleep(Duration::from_millis(1));
        Ok(())
    }
}

impl End {
    /// A line on which the program has sent `pieces`.
    fn sent(pieces: &[&[u8]]) -> Self {
        let line = Self::default();
        line.0.lock().unwrap().pieces = pieces.iter().map(|p| p.to_vec()).collect();
        line
    }

    fn hang_up(&self) {
        self.0.lock().unwrap().hung_up = true;
    }

    fn answered(&self) -> Decoded<Reply> {
        protocol::decode_reply(&self.0.lock().unwrap().answered)
    }
}

fn opening() -> Request {
    Request {
        mode: Mode::Open,
        owner: 7,
        title: String::new(),
        start: std::env::temp_dir(),
        name: OsString::new(),
        filters: vec![Filter {
            label: "Text".into(),
            patterns: vec!["*.txt".into()],
        }],
        filter: 0,
    }
}

/// **A request is read whole, however it arrives**, and answered.
#[test]
fn a_request_is_read_whole_however_it_arrives() {
    let frame = protocol::encode_request(&opening()).unwrap();
    let (a, rest) = frame.split_at(3);
    let (b, c) = rest.split_at(10);
    let line = End::sent(&[a, b, &[], c]);
    let asked = Asked::read(line.clone(), Duration::from_secs(5)).unwrap();
    assert_eq!(asked.request(), &opening());
    let chosen = std::env::temp_dir().join("notes.txt");
    asked
        .answer(&Reply::Chosen {
            path: chosen.clone(),
            filter: 0,
        })
        .unwrap();
    assert!(matches!(
        line.answered(),
        Decoded::Complete(Reply::Chosen { path, filter: 0 }, _) if path == chosen
    ));
}

/// **A program that misbehaves is refused**: one that sends something that
/// is not a request, two requests, or hangs up part way through one.
#[test]
fn a_program_that_misbehaves_is_refused() {
    let kind = |line: End| {
        Asked::read(line, Duration::from_secs(5))
            .unwrap_err()
            .kind()
    };
    assert_eq!(
        kind(End::sent(&[b"\x04\0\0\0junk"])),
        io::ErrorKind::InvalidData
    );
    let frame = protocol::encode_request(&opening()).unwrap();
    let mut twice = frame.clone();
    twice.extend_from_slice(&frame);
    assert_eq!(kind(End::sent(&[&twice])), io::ErrorKind::InvalidData);
    let part = End::sent(&[&frame[..frame.len() - 1]]);
    part.hang_up();
    assert_eq!(kind(part), io::ErrorKind::UnexpectedEof);
}

/// **A program that says nothing is waited for only so long.**
#[test]
fn a_program_that_says_nothing_is_waited_for_only_so_long() {
    let start = Instant::now();
    let err = Asked::read(End::default(), Duration::from_millis(50)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(5));
}

/// **A program that gives up is seen to have**: not while it waits, but
/// once it hangs up.
#[test]
fn a_program_that_gives_up_is_seen_to_have() {
    let frame = protocol::encode_request(&opening()).unwrap();
    let line = End::sent(&[&frame]);
    let mut asked = Asked::read(line.clone(), Duration::from_secs(5)).unwrap();
    assert!(!asked.is_abandoned());
    line.hang_up();
    assert!(asked.is_abandoned());
    // Answering it then is the transport's broken pipe.
    assert_eq!(
        asked.answer(&Reply::Cancelled).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

/// **An answer is checked against the request**: a filter it did not offer,
/// or a path that is not absolute, is not sent.
#[test]
fn an_answer_is_checked_against_the_request() {
    let frame = protocol::encode_request(&opening()).unwrap();
    let invalid = |reply: Reply| {
        let line = End::sent(&[&frame]);
        let asked = Asked::read(line.clone(), Duration::from_secs(5)).unwrap();
        let kind = asked.answer(&reply).unwrap_err().kind();
        assert!(line.0.lock().unwrap().answered.is_empty());
        kind
    };
    assert_eq!(
        invalid(Reply::Chosen {
            path: std::env::temp_dir().join("a.txt"),
            filter: 1,
        }),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        invalid(Reply::Chosen {
            path: PathBuf::from("a.txt"),
            filter: 0,
        }),
        io::ErrorKind::InvalidInput
    );
}
