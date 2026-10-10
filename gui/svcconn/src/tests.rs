//! Tests for reading one framed message off a connection: whole however it
//! arrives, refused when it is not one, and given up on when it takes too
//! long or is no longer wanted.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use guiremote::client::Transport;
use msgframe::{Decoded, Protocol, Writer};

use super::{Connect, NoConn, SystemConnect, Waiting, read_frame};

const TEST: Protocol = Protocol {
    mark: b"TST",
    version: 1,
    max_frame: 64,
};

/// A message of the test protocol: one byte.
fn message(value: u8) -> Vec<u8> {
    let mut w = Writer::new(TEST, 1);
    w.u8(value);
    w.finish().unwrap()
}

fn decode(bytes: &[u8]) -> Decoded<u8> {
    msgframe::decode(bytes, TEST, 1, |r| r.u8())
}

/// A connection on which the peer has sent `pieces`, one a read, and then
/// hangs up or not.
#[derive(Debug, Default)]
struct Pieces {
    pieces: VecDeque<Vec<u8>>,
    hung_up: bool,
}

impl Pieces {
    fn of(pieces: &[&[u8]], hung_up: bool) -> Self {
        Self {
            pieces: pieces.iter().map(|p| p.to_vec()).collect(),
            hung_up,
        }
    }
}

impl Transport for Pieces {
    type Error = io::Error;

    fn read(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        let piece = self.pieces.pop_front().unwrap_or_default();
        buf.extend_from_slice(&piece);
        Ok(piece.len())
    }

    fn write(&mut self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn is_open(&self) -> bool {
        !(self.hung_up && self.pieces.is_empty())
    }

    fn wait(&mut self) -> io::Result<()> {
        std::thread::sleep(Duration::from_millis(1));
        Ok(())
    }
}

/// **A message is read whole, however it arrives.**
#[test]
fn a_message_is_read_whole_however_it_arrives() {
    let frame = message(42);
    let (a, b) = frame.split_at(3);
    let mut conn = Pieces::of(&[a, &[], b], false);
    let read = read_frame(&mut conn, decode, Waiting::default()).unwrap();
    assert_eq!(read, Some(42));
}

/// **What is not one message is refused**: bytes that are no message, two
/// messages, a peer hanging up part way.
#[test]
fn what_is_not_one_message_is_refused() {
    let kind = |mut conn: Pieces| {
        read_frame(&mut conn, decode, Waiting::default())
            .unwrap_err()
            .kind()
    };
    assert_eq!(
        kind(Pieces::of(&[b"\x01\0\0\0X"], false)),
        io::ErrorKind::InvalidData
    );
    let mut two = message(1);
    two.extend_from_slice(&message(2));
    assert_eq!(kind(Pieces::of(&[&two], false)), io::ErrorKind::InvalidData);
    let frame = message(7);
    assert_eq!(
        kind(Pieces::of(&[&frame[..frame.len() - 1]], true)),
        io::ErrorKind::UnexpectedEof
    );
}

/// **A message is waited for only so long, and not once unwanted.**
#[test]
fn a_message_is_waited_for_only_so_long() {
    let start = Instant::now();
    let waiting = Waiting {
        patience: Some(Duration::from_millis(30)),
        abandoned: None,
    };
    let err = read_frame(&mut Pieces::default(), decode, waiting).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(5));
    let gone = AtomicBool::new(true);
    let waiting = Waiting {
        patience: None,
        abandoned: Some(&gone),
    };
    assert_eq!(
        read_frame(&mut Pieces::default(), decode, waiting).unwrap(),
        None
    );
}

/// **Off SlateOS there is no service to reach**, and nothing can be sent on
/// the connection there is not.
#[test]
fn off_slateos_there_is_no_service() {
    let conn: Option<NoConn> = SystemConnect.connect("org.slateos.Anything").unwrap();
    assert!(conn.is_none());
}
