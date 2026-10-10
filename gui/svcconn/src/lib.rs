//! Connecting to SlateOS's named services and reading their framed messages:
//! the transport half of the small services built on `msgframe` -- the file
//! chooser (`gui/filechooser`) and the credential service
//! (`gui/credentials`).
//!
//! - [`Connect`]: how a client reaches a service by name, and
//!   [`SystemConnect`], the system's way -- SlateOS's service registry, and
//!   nothing anywhere else, so a client on a development host falls back to
//!   whatever it does without the service.
//! - [`read_frame`]: one whole message off a connection, waiting as long as
//!   the caller allows and no longer, giving up the moment the caller says it
//!   no longer wants it.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use guiremote::client::Transport;
use msgframe::Decoded;

/// How a client reaches a named service.
pub trait Connect {
    /// One connection.
    type Conn: Transport<Error = io::Error> + Send + 'static;

    /// A connection to the service registered as `service`, or `None` where
    /// there is none to reach.
    ///
    /// # Errors
    ///
    /// A failure other than the service's absence -- out of descriptors, a
    /// fault.
    fn connect(&self, service: &str) -> io::Result<Option<Self::Conn>>;
}

/// The system's services: SlateOS's registry; none anywhere else.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemConnect;

impl Connect for SystemConnect {
    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    type Conn = guiremote::channel::ChannelConn;
    #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
    type Conn = NoConn;

    #[cfg(all(target_os = "linux", target_vendor = "slateos"))]
    fn connect(&self, service: &str) -> io::Result<Option<Self::Conn>> {
        match guiremote::channel::ChannelConn::connect(service) {
            Ok(conn) => Ok(Some(conn)),
            Err(e) if guiremote::channel::connect_failure_means_absent(&e) => Ok(None),
            Err(e) => Err(e),
        }
    }

    #[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
    fn connect(&self, _service: &str) -> io::Result<Option<Self::Conn>> {
        Ok(None)
    }
}

/// The connection there is to a service where there is none: a type with
/// no values, so nothing can be sent on it.
#[derive(Debug)]
pub enum NoConn {}

impl Transport for NoConn {
    type Error = io::Error;

    fn read(&mut self, _buf: &mut Vec<u8>) -> io::Result<usize> {
        match *self {}
    }

    fn write(&mut self, _bytes: &[u8]) -> io::Result<()> {
        match *self {}
    }
}

/// How long one wait for a frame's bytes lasts, so a caller's deadline is
/// kept and its change of mind seen.
const WAIT_SLICE: Duration = Duration::from_millis(100);

/// How long [`read_frame`] may wait, and whether it is still wanted.
#[derive(Clone, Copy, Debug, Default)]
pub struct Waiting<'a> {
    /// The longest to wait for the whole frame; `None` for as long as it
    /// takes.
    pub patience: Option<Duration>,
    /// Set when the frame is no longer wanted.
    pub abandoned: Option<&'a AtomicBool>,
}

/// One whole message off `conn`, read by `decode`: `Ok(None)` if it was
/// abandoned first.
///
/// # Errors
///
/// `InvalidData` for bytes that are no message of the protocol's, or more
/// than one; `UnexpectedEof` for a peer that hung up first; `TimedOut` past
/// the patience; and the transport's own errors.
pub fn read_frame<C, T>(
    conn: &mut C,
    decode: impl Fn(&[u8]) -> Decoded<T>,
    waiting: Waiting<'_>,
) -> io::Result<Option<T>>
where
    C: Transport<Error = io::Error>,
{
    let slice = waiting.patience.map_or(WAIT_SLICE, |p| WAIT_SLICE.min(p));
    conn.set_wait_timeout(Some(slice))?;
    let deadline = waiting.patience.and_then(|p| Instant::now().checked_add(p));
    let mut received = Vec::new();
    loop {
        if waiting
            .abandoned
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            return Ok(None);
        }
        conn.read(&mut received)?;
        match decode(&received) {
            Decoded::Complete(message, used) if used == received.len() => return Ok(Some(message)),
            Decoded::Complete(..) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "a peer sent more than one message",
                ));
            }
            Decoded::Malformed => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "a peer sent something that is not a message",
                ));
            }
            Decoded::Partial => {}
        }
        if !conn.is_open() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        conn.wait()?;
    }
}

#[cfg(test)]
mod tests;
