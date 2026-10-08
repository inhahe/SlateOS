//! The network link as the kernel serves it: whether it is up, the address
//! the machine has and its mask, the gateway, the name server, and the
//! adapter's hardware address -- `/sys/devices/net/{up,ip,subnet_mask,gateway,
//! dns,mac}`, a value to a file, the directory there only while there is a
//! network adapter (`kernel/src/fs/sysfs.rs`, `gen_net_file`).
//!
//! Read by the network widget at its tick (`widgets::network`). The tray's
//! network icon would read it too, and waits on an event saying it changed
//! (`known-issues/TD-C-THE-NETWORK-INDICATOR-HAS-A-SOURCE-AND-NOTHING-TO-WAKE-IT.md`):
//! a widget is put out by the user and ticks as a clock does; an icon would
//! be there always, and an idle desktop wakes for nothing (§812).
//!
//! These files are SlateOS's own layout. Anywhere else there is no such
//! directory, and that says nothing about whether the machine has a network,
//! so off SlateOS [`current`] says the state is not known rather than that
//! there is no adapter.

use std::io;
use std::net::Ipv4Addr;
use std::path::Path;

/// Where the kernel serves the link.
pub const NET_DIR: &str = "/sys/devices/net";

/// The link, as the kernel describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Whether it is up.
    pub up: bool,
    /// The machine's address; `None` while it has none (`0.0.0.0`), as
    /// before an address has been given it.
    pub address: Option<Ipv4Addr>,
    /// How many leading bits of the address name its network -- the mask
    /// `255.255.255.0` is 24; `None` for no mask, or one that is not a run
    /// of ones, which no network is given.
    pub prefix: Option<u8>,
    /// The gateway to other networks; `None` for none.
    pub gateway: Option<Ipv4Addr>,
    /// The name server; `None` for none.
    pub dns: Option<Ipv4Addr>,
    /// The adapter's hardware address, as the kernel writes it.
    pub mac: String,
}

/// What can be said of the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// There is an adapter, and this is its link.
    Link(Link),
    /// There is no network adapter.
    NoAdapter,
    /// Nothing here says: off SlateOS, whose layout this is.
    Unknown,
}

/// The link as the files in `dir` describe it: [`LinkState::NoAdapter`] when
/// `dir` is not there.
///
/// # Errors
///
/// A file in `dir` that will not read, or that holds what the kernel never
/// writes there, as [`io::ErrorKind::InvalidData`] -- a link half read is not
/// a link.
pub fn read(dir: &Path) -> io::Result<LinkState> {
    if !dir.is_dir() {
        return Ok(LinkState::NoAdapter);
    }
    let field = |name: &str| -> io::Result<String> {
        let bytes = std::fs::read(dir.join(name))?;
        String::from_utf8(bytes)
            .map(|text| text.trim().to_owned())
            .map_err(|_| invalid(name))
    };
    let address = |name: &str| -> io::Result<Option<Ipv4Addr>> {
        let text = field(name)?;
        let parsed: Ipv4Addr = text.parse().map_err(|_| invalid(name))?;
        Ok((!parsed.is_unspecified()).then_some(parsed))
    };
    let up = match field("up")?.as_str() {
        "1" => true,
        "0" => false,
        _ => return Err(invalid("up")),
    };
    let mask = address("subnet_mask")?;
    Ok(LinkState::Link(Link {
        up,
        address: address("ip")?,
        prefix: mask.and_then(prefix_len),
        gateway: address("gateway")?,
        dns: address("dns")?,
        mac: field("mac")?,
    }))
}

/// The link of this machine: [`read`] of [`NET_DIR`] on SlateOS, and
/// [`LinkState::Unknown`] anywhere else.
///
/// # Errors
///
/// As [`read`].
pub fn current() -> io::Result<LinkState> {
    if cfg!(all(target_os = "linux", target_vendor = "slateos")) {
        read(Path::new(NET_DIR))
    } else {
        Ok(LinkState::Unknown)
    }
}

/// How many leading ones `mask` has, if it is a run of ones and then zeros:
/// `255.255.255.0` is 24, `255.0.255.0` is no prefix at all.
#[must_use]
pub fn prefix_len(mask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(mask);
    let ones = bits.leading_ones();
    // A run of ones then zeros: nothing after the run.
    let rest = bits.checked_shl(ones).unwrap_or(0);
    (rest == 0).then(|| u8::try_from(ones).ok()).flatten()
}

/// The error for a file that holds what the kernel never writes there.
fn invalid(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{NET_DIR}/{name} holds something the kernel does not write"),
    )
}

#[cfg(test)]
#[path = "netlink_tests.rs"]
mod tests;
