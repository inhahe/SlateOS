//! The file chooser's side: reading what a program asks, and answering.
//!
//! The file explorer registers [`SERVICE`](crate::protocol::SERVICE)
//! (`register`, on SlateOS only), accepts each program's connection, and hands
//! it to [`Asked::read`], which waits for the request. It shows its window in
//! the request's [`Mode`](crate::protocol::Mode), above the window the
//! request names as its owner, and answers with [`Asked::answer`] -- a
//! [`Reply::Chosen`] path, with any extension the filter adds already on it,
//! or [`Reply::Cancelled`]. While its window is up it can ask
//! [`Asked::is_abandoned`] whether the program has given up, and take the
//! window down if it has.
//!
//! A program is anyone. One that connects and says nothing is waited for
//! only so long ([`REQUEST_PATIENCE`]); one that says something that is not
//! a request, or more than one, is refused; and nothing it says is read past
//! the protocol's bounds.

use std::io;
use std::time::{Duration, Instant};

use guiremote::client::Transport;

use crate::protocol::{self, Decoded, Reply, Request};

/// How long [`Asked::read`] waits for a request on a fresh connection: a
/// program sends one as soon as it connects, so this is generous for any
/// that means to, and short enough that one that never does cannot hold the
/// chooser.
pub const REQUEST_PATIENCE: Duration = Duration::from_secs(5);

/// How long one wait for a request's bytes lasts, so the patience is kept.
const WAIT_SLICE: Duration = Duration::from_millis(100);

/// Register as the system's file chooser.
///
/// # Errors
///
/// The kernel's: another process already has the name, or this one lacks
/// the capability to register it.
#[cfg(all(target_os = "linux", target_vendor = "slateos"))]
pub fn register() -> io::Result<guiremote::channel::ChannelListener> {
    guiremote::channel::ChannelListener::register(protocol::SERVICE)
}

/// What one program asked, and the connection to answer it on.
#[derive(Debug)]
pub struct Asked<T> {
    request: Request,
    conn: T,
}

impl<T: Transport<Error = io::Error>> Asked<T> {
    /// Wait on `conn`, freshly accepted, for the program's request -- for at
    /// most `patience`.
    ///
    /// # Errors
    ///
    /// `TimedOut` if no whole request came in time; `UnexpectedEof` if the
    /// program hung up first; `InvalidData` if what it sent is not a request,
    /// or is more than one; and the transport's own errors.
    pub fn read(mut conn: T, patience: Duration) -> io::Result<Self> {
        conn.set_wait_timeout(Some(WAIT_SLICE.min(patience)))?;
        let deadline = Instant::now().checked_add(patience);
        let mut received = Vec::new();
        loop {
            conn.read(&mut received)?;
            match protocol::decode_request(&received) {
                Decoded::Complete(request, used) if used == received.len() => {
                    return Ok(Self { request, conn });
                }
                Decoded::Complete(..) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a program sent more than one request",
                    ));
                }
                Decoded::Malformed => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "a program sent something that is not a request",
                    ));
                }
                Decoded::Partial => {}
            }
            if !conn.is_open() {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            if deadline.is_none_or(|deadline| Instant::now() >= deadline) {
                return Err(io::ErrorKind::TimedOut.into());
            }
            conn.wait()?;
        }
    }

    /// What the program asked.
    #[must_use]
    pub const fn request(&self) -> &Request {
        &self.request
    }

    /// The connection, for what the transport itself can say -- on SlateOS,
    /// which process asked (`ChannelConn::peer_cred`).
    #[must_use]
    pub const fn connection(&self) -> &T {
        &self.conn
    }

    /// Whether the program has given up asking: hung up, or gone. Reads, and
    /// drops, anything it sends after its request, which it has no reason to.
    pub fn is_abandoned(&mut self) -> bool {
        let mut ignored = Vec::new();
        match self.conn.read(&mut ignored) {
            Ok(_) => !self.conn.is_open(),
            Err(_) => true,
        }
    }

    /// Answer the program, and be done with it.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if `reply` is not one this request can have: a path
    /// that is not absolute or past the protocol's bounds, or a filter the
    /// request did not offer. And the transport's errors -- `BrokenPipe`,
    /// most likely, for a program that has gone.
    pub fn answer(mut self, reply: &Reply) -> io::Result<()> {
        if let Reply::Chosen { filter, .. } = reply {
            let offered = self.request.filters.len();
            if *filter >= offered.max(1) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "an answer naming a filter the request did not offer",
                ));
            }
        }
        let frame = protocol::encode_reply(reply)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        self.conn.write(&frame)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
