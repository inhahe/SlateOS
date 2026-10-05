//! What a program asks the credential service, and what it is answered.
//!
//! One [`Query`] and one [`Answer`] per connection to [`SERVICE`], each a
//! frame of `msgframe`'s, every field bounded. The program asks for the login
//! it needs for a target -- a site's address, or a name the program and the
//! user agree on -- and is answered with the login the user let it have, or
//! with [`Answer::Refused`]: for a program with no key to ask, for a user who
//! said no, and for a login the user's vault does not hold, alike -- so a
//! program learns nothing it was not given.

use std::fmt;

pub use msgframe::{Decoded, TooLarge};
use msgframe::{Protocol, Reader, Writer};

/// The name the credential service registers, and programs connect to.
pub const SERVICE: &str = "org.slateos.Credentials";

/// The protocol this module speaks.
pub const VERSION: u8 = 1;

/// The longest target, in bytes: an address with a long path.
pub const MAX_TARGET: usize = 2048;
/// The longest user name, in bytes.
pub const MAX_USERNAME: usize = 1024;
/// The longest password, in bytes.
pub const MAX_PASSWORD: usize = 4096;
/// The longest frame either side reads: above the largest message the other
/// bounds allow.
pub const MAX_FRAME: usize = 16 * 1024;

const PROTOCOL: Protocol = Protocol {
    mark: b"SCR",
    version: VERSION,
    max_frame: MAX_FRAME,
};
const KIND_QUERY: u8 = 1;
const KIND_ANSWER: u8 = 2;
const OUTCOME_REFUSED: u8 = 0;
const OUTCOME_LOGIN: u8 = 1;

/// A password: its bytes, overwritten when it is dropped, so it stays in
/// memory only as long as something holds it -- and never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Vec<u8>);

impl Secret {
    /// The secret `bytes`.
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Its bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        // So the overwrite is not taken for a store nothing reads and left
        // out: the zeros are observed.
        std::hint::black_box(&self.0);
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// What a program asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// What the login is for: a site's address, or a name. Never empty.
    pub target: String,
    /// Which user's login, when the program knows; any otherwise.
    pub username: Option<String>,
}

/// What a program is answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The login the user let the program have.
    Login {
        /// Its user name.
        username: String,
        /// Its password.
        password: Secret,
    },
    /// Nothing: no key to ask with, a user who said no, or no such login.
    Refused,
}

/// `query` as a frame.
///
/// # Errors
///
/// [`TooLarge`] for an empty target, or a field past its bound.
pub fn encode_query(query: &Query) -> Result<Vec<u8>, TooLarge> {
    if query.target.is_empty() {
        return Err(TooLarge);
    }
    let mut out = Writer::new(PROTOCOL, KIND_QUERY);
    out.text(&query.target, MAX_TARGET)?;
    match &query.username {
        Some(username) => {
            out.u8(1);
            out.text(username, MAX_USERNAME)?;
        }
        None => out.u8(0),
    }
    out.finish()
}

/// The query at the front of `bytes`, if a whole frame of one is there.
#[must_use]
pub fn decode_query(bytes: &[u8]) -> Decoded<Query> {
    msgframe::decode(bytes, PROTOCOL, KIND_QUERY, read_query)
}

/// `answer` as a frame -- written into a buffer with room for the largest
/// answer, so the password in it is never left behind by a buffer growing
/// (`msgframe::Writer::with_capacity`). The frame holds the password: keep it
/// in a [`Secret`] until it is sent.
///
/// # Errors
///
/// [`TooLarge`] for a field past its bound.
pub fn encode_answer(answer: &Answer) -> Result<Vec<u8>, TooLarge> {
    // The outcome, then each field's two-byte length and its bound.
    let room = 1 + 2 + MAX_USERNAME + 2 + MAX_PASSWORD;
    let mut out = Writer::with_capacity(PROTOCOL, KIND_ANSWER, room);
    match answer {
        Answer::Refused => out.u8(OUTCOME_REFUSED),
        Answer::Login { username, password } => {
            out.u8(OUTCOME_LOGIN);
            out.text(username, MAX_USERNAME)?;
            out.bytes(password.as_bytes(), MAX_PASSWORD)?;
        }
    }
    out.finish()
}

/// The answer at the front of `bytes`, if a whole frame of one is there.
#[must_use]
pub fn decode_answer(bytes: &[u8]) -> Decoded<Answer> {
    msgframe::decode(bytes, PROTOCOL, KIND_ANSWER, read_answer)
}

fn read_query(r: &mut Reader<'_>) -> Option<Query> {
    let target = r.text(MAX_TARGET).filter(|target| !target.is_empty())?;
    let username = match r.u8()? {
        0 => None,
        1 => Some(r.text(MAX_USERNAME)?),
        _ => return None,
    };
    Some(Query { target, username })
}

fn read_answer(r: &mut Reader<'_>) -> Option<Answer> {
    match r.u8()? {
        OUTCOME_REFUSED => Some(Answer::Refused),
        OUTCOME_LOGIN => {
            let username = r.text(MAX_USERNAME)?;
            let password = Secret::new(r.bytes(MAX_PASSWORD)?.to_vec());
            Some(Answer::Login { username, password })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{
        Answer, Decoded, MAX_TARGET, Query, Secret, TooLarge, decode_answer, decode_query,
        encode_answer, encode_query,
    };

    /// **What is asked and answered is what is read**: a query with and
    /// without a user name, a login, a refusal.
    #[test]
    fn what_is_asked_and_answered_is_read() {
        for query in [
            Query {
                target: "https://example.com/login".into(),
                username: Some("ann".into()),
            },
            Query {
                target: "work-vpn".into(),
                username: None,
            },
        ] {
            let frame = encode_query(&query).unwrap();
            assert_eq!(decode_query(&frame), Decoded::Complete(query, frame.len()));
        }
        for answer in [
            Answer::Login {
                username: "ann".into(),
                password: Secret::new(b"pa\0ss\xff word".to_vec()),
            },
            Answer::Refused,
        ] {
            let frame = encode_answer(&answer).unwrap();
            assert_eq!(
                decode_answer(&frame),
                Decoded::Complete(answer, frame.len())
            );
        }
    }

    /// **A query is for something**: an empty target cannot be sent, and one
    /// received is malformed; nor can one past its bound; and a username
    /// flag that is neither is malformed.
    #[test]
    fn a_query_is_for_something() {
        let empty = Query {
            target: String::new(),
            username: None,
        };
        assert_eq!(encode_query(&empty), Err(TooLarge));
        let long = Query {
            target: "x".repeat(MAX_TARGET + 1),
            username: None,
        };
        assert_eq!(encode_query(&long), Err(TooLarge));
        // By hand: an empty target.
        let mut frame = encode_query(&Query {
            target: "a".into(),
            username: None,
        })
        .unwrap();
        // The target's length (after the 4-byte frame length, the 3-byte
        // mark, the version and the kind) set to 0, and its byte dropped.
        frame.remove(4 + 5 + 2);
        frame[4 + 5] = 0;
        let length = u32::try_from(frame.len() - 4).unwrap().to_le_bytes();
        frame[..4].copy_from_slice(&length);
        assert_eq!(decode_query(&frame), Decoded::Malformed);
        let mut flag = encode_query(&Query {
            target: "a".into(),
            username: None,
        })
        .unwrap();
        let last = flag.len() - 1;
        flag[last] = 2;
        assert_eq!(decode_query(&flag), Decoded::Malformed);
    }

    /// **A secret is never printed.**
    #[test]
    fn a_secret_is_never_printed() {
        let secret = Secret::new(b"hunter2".to_vec());
        assert_eq!(format!("{secret:?}"), "Secret(..)");
        let answer = Answer::Login {
            username: "ann".into(),
            password: secret,
        };
        assert!(!format!("{answer:?}").contains("hunter2"));
    }
}
