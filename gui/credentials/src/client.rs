//! A program's side: asking the credential service for a login.
//!
//! One [`ask`] is one connection: the query sent, the answer read, the
//! connection let go. The answer comes when the user has answered the
//! service's prompt, so `ask` blocks for as long as that takes -- or as long
//! as the caller's [`Waiting`] allows; a program with a window to keep
//! drawing asks from a thread of its own.
//!
//! The program is told the login or [`Answer::Refused`], and nothing more:
//! not whether it lacked the key, the user said no, or the vault holds
//! nothing for what it asked.

use std::io;

use guiremote::client::Transport;
use svcconn::{Connect, Waiting};

use crate::protocol::{self, Answer, Query, SERVICE};

/// Ask the credential service, reached through `connect`, for a login for
/// `query`.
///
/// `Ok(None)` where there is no service to ask -- a development host, a
/// session not running one -- and when `waiting` was abandoned first.
///
/// # Errors
///
/// `InvalidInput` for a query the protocol cannot carry -- an empty target,
/// a field past its bound -- found before anything is sent. `InvalidData`
/// for an answer that is not one; `UnexpectedEof` if the service hung up
/// first; `TimedOut` past `waiting`'s patience; and the transport's own
/// errors.
pub fn ask<C: Connect>(
    connect: &C,
    query: &Query,
    waiting: Waiting<'_>,
) -> io::Result<Option<Answer>> {
    let frame = protocol::encode_query(query)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let Some(mut conn) = connect.connect(SERVICE)? else {
        return Ok(None);
    };
    conn.write(&frame)?;
    svcconn::read_frame(&mut conn, protocol::decode_answer, waiting)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]

    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};

    use guiremote::client::Transport;
    use svcconn::{Connect, Waiting};

    use super::ask;
    use crate::protocol::{self, Answer, Decoded, MAX_TARGET, Query, Secret};

    /// A stand-in service's line: what the program sent, and what the
    /// service says back.
    #[derive(Debug, Default)]
    struct Line {
        sent: Vec<u8>,
        said: VecDeque<u8>,
        hung_up: bool,
    }

    #[derive(Debug)]
    struct End(Arc<Mutex<Line>>);

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
    }

    /// A stand-in service: there or not, and what it answers.
    #[derive(Debug, Default)]
    struct Stand {
        present: bool,
        line: Arc<Mutex<Line>>,
        connects: Cell<usize>,
    }

    impl Connect for Stand {
        type Conn = End;

        fn connect(&self, service: &str) -> io::Result<Option<End>> {
            assert_eq!(service, crate::SERVICE);
            self.connects.set(self.connects.get() + 1);
            Ok(self.present.then(|| End(Arc::clone(&self.line))))
        }
    }

    impl Stand {
        fn answering(answer: &Answer) -> Self {
            let stand = Self {
                present: true,
                ..Self::default()
            };
            let frame = protocol::encode_answer(answer).unwrap();
            stand.line.lock().unwrap().said.extend(frame);
            stand
        }

        fn asked(&self) -> Query {
            match protocol::decode_query(&self.line.lock().unwrap().sent) {
                Decoded::Complete(query, _) => query,
                other => panic!("not a query: {other:?}"),
            }
        }
    }

    fn query() -> Query {
        Query {
            target: "https://example.com/login".into(),
            username: Some("ann".into()),
        }
    }

    /// **The service is asked what the program asks, and its answer is the
    /// program's**: a login, or a refusal.
    #[test]
    fn the_service_is_asked_and_its_answer_returned() {
        let login = Answer::Login {
            username: "ann".into(),
            password: Secret::new(b"hunter2".to_vec()),
        };
        let stand = Stand::answering(&login);
        assert_eq!(
            ask(&stand, &query(), Waiting::default()).unwrap(),
            Some(login)
        );
        assert_eq!(stand.asked(), query());
        let stand = Stand::answering(&Answer::Refused);
        assert_eq!(
            ask(&stand, &query(), Waiting::default()).unwrap(),
            Some(Answer::Refused)
        );
    }

    /// **Where there is no service, there is no answer** -- and a query the
    /// protocol cannot carry is refused before anything is reached.
    #[test]
    fn no_service_no_answer_and_no_unsendable_query() {
        let absent = Stand::default();
        assert_eq!(ask(&absent, &query(), Waiting::default()).unwrap(), None);
        let stand = Stand::answering(&Answer::Refused);
        let long = Query {
            target: "x".repeat(MAX_TARGET + 1),
            username: None,
        };
        let err = ask(&stand, &long, Waiting::default()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(stand.connects.get(), 0);
    }

    /// **A service that hangs up without answering is an error**, not a
    /// refusal: the program is not told "no" by a service that said nothing.
    #[test]
    fn a_service_that_hangs_up_is_an_error() {
        let stand = Stand {
            present: true,
            ..Stand::default()
        };
        stand.line.lock().unwrap().hung_up = true;
        let err = ask(&stand, &query(), Waiting::default()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}
