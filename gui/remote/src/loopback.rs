//! An in-process duplex pipe implementing [`Transport`].
//!
//! Two [`Pipe`] halves share a pair of byte queues: what one writes, the other
//! reads. That is all it is — there is no compositor here and nothing is
//! simulated. Both halves speak the real wire protocol; the only thing missing
//! compared to a socket is the kernel.
//!
//! This exists for two jobs, and it is worth being precise about which:
//!
//! * **Tests on either side of the protocol.** A client test can drive a real
//!   `Connection`, and a compositor test can drive a real client, without
//!   either mocking the frame layer — which is the layer most likely to be
//!   wrong.
//! * **A hosted build with the compositor in the same process.** On a
//!   development machine there is no SlateOS IPC channel to connect to. Running
//!   the compositor in-process over this pipe exercises the same encoders,
//!   decoders and event loop that a real connection would.
//!
//! It is deliberately **not** a substitute for a transport. [`Pipe::wait`] does
//! nothing, because on one thread nothing can arrive while a caller is blocked;
//! a loop that relies on `wait` to make progress will spin. Both halves must be
//! driven by the same thread, alternately. `Rc` rather than `Arc` states that
//! in the type system rather than in a comment.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::client::Transport;

/// One end of an in-process duplex pipe.
#[derive(Clone)]
pub struct Pipe {
    /// Bytes written by this end, waiting for the other to read them.
    outgoing: Rc<RefCell<VecDeque<u8>>>,
    /// Bytes written by the other end, waiting for this one to read them.
    incoming: Rc<RefCell<VecDeque<u8>>>,
    /// Shared by both halves: closing either closes the pipe, as hanging up a
    /// socket does.
    open: Rc<Cell<bool>>,
}

/// A connected pair of pipe ends.
///
/// The two are interchangeable; by convention the first is the client's.
#[must_use]
pub fn pipe() -> (Pipe, Pipe) {
    let a_to_b = Rc::new(RefCell::new(VecDeque::new()));
    let b_to_a = Rc::new(RefCell::new(VecDeque::new()));
    let open = Rc::new(Cell::new(true));
    (
        Pipe {
            outgoing: Rc::clone(&a_to_b),
            incoming: Rc::clone(&b_to_a),
            open: Rc::clone(&open),
        },
        Pipe {
            outgoing: b_to_a,
            incoming: a_to_b,
            open,
        },
    )
}

impl Pipe {
    /// Hang up. Both ends see the pipe as closed.
    pub fn close(&self) {
        self.open.set(false);
    }

    /// How many bytes this end has written that the other has not read.
    #[must_use]
    pub fn pending_out(&self) -> usize {
        self.outgoing.borrow().len()
    }

    /// How many bytes are waiting for this end to read.
    #[must_use]
    pub fn pending_in(&self) -> usize {
        self.incoming.borrow().len()
    }
}

impl Transport for Pipe {
    /// A pipe cannot fail: there is no syscall to fail, and a closed pipe is
    /// reported through [`Transport::is_open`] rather than as an error, because
    /// hanging up is not a failure.
    type Error = std::convert::Infallible;

    fn read(&mut self, buf: &mut Vec<u8>) -> Result<usize, Self::Error> {
        let mut q = self.incoming.borrow_mut();
        let n = q.len();
        buf.extend(q.drain(..));
        Ok(n)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.outgoing.borrow_mut().extend(bytes.iter().copied());
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.open.get()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use guitk::event::Event;

    use super::*;
    use crate::client::Connection;
    use crate::control::{RequestBody, ResponseBody, WindowSpec, encode_responses};
    use crate::input::{InputEvent, encode_input_frame};

    #[test]
    fn what_one_end_writes_the_other_reads() {
        let (mut a, mut b) = pipe();
        a.write(b"hello").unwrap();
        let mut buf = Vec::new();
        assert_eq!(b.read(&mut buf).unwrap(), 5);
        assert_eq!(buf, b"hello");
        assert_eq!(b.read(&mut buf).unwrap(), 0, "a drained pipe is empty");
    }

    #[test]
    fn the_two_directions_do_not_share_a_queue() {
        // The bug this catches is a pipe wired to itself, where a client reads
        // back its own requests and every test passes for the wrong reason.
        let (mut a, mut b) = pipe();
        a.write(b"from a").unwrap();
        b.write(b"from b").unwrap();
        let mut at_a = Vec::new();
        let mut at_b = Vec::new();
        a.read(&mut at_a).unwrap();
        b.read(&mut at_b).unwrap();
        assert_eq!(at_a, b"from b");
        assert_eq!(at_b, b"from a");
    }

    #[test]
    fn hanging_up_either_end_closes_both() {
        let (a, b) = pipe();
        assert!(a.is_open() && b.is_open());
        b.close();
        assert!(!a.is_open(), "the other end must notice");
    }

    #[test]
    fn a_real_request_and_reply_cross_the_pipe_intact() {
        // The point of the whole thing: a genuine round trip, with both sides
        // running the real codecs rather than a mock of them.
        let (client_end, mut server_end) = pipe();
        let mut conn = Connection::new(client_end);

        let seq = conn
            .send(RequestBody::CreateWindow(WindowSpec::new(
                "Notes", 640, 480,
            )))
            .unwrap();

        // The server side reads bytes and decodes them with the same decoder a
        // compositor would use.
        let mut wire = Vec::new();
        server_end.read(&mut wire).unwrap();
        let (reqs, used) = crate::control::decode_requests(&wire).unwrap();
        assert_eq!(used, wire.len());
        assert_eq!(reqs[0].seq, seq);

        server_end
            .write(&encode_responses(&[crate::control::Response::new(
                seq,
                ResponseBody::WindowCreated { window: 5 },
            )]))
            .unwrap();

        conn.pump().unwrap();
        assert_eq!(
            conn.take_reply(seq),
            Some(ResponseBody::WindowCreated { window: 5 })
        );
    }

    #[test]
    fn a_frame_written_in_pieces_still_arrives_whole() {
        // A pipe is a byte stream, like the socket it stands in for: a reader
        // must not assume one write is one frame.
        let (client_end, mut server_end) = pipe();
        let mut conn = Connection::new(client_end);
        let frame = encode_input_frame(&[InputEvent::new(1, Event::FocusIn)]);
        let mid = frame.len() / 2;

        server_end.write(&frame[..mid]).unwrap();
        assert_eq!(conn.pump().unwrap(), 0, "half a frame is not a frame");
        server_end.write(&frame[mid..]).unwrap();
        assert_eq!(conn.pump().unwrap(), 1);
        assert_eq!(conn.pending_events(), 1);
    }
}
