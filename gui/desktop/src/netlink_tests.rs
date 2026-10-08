//! Tests for reading the network link as the kernel serves it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

/// A link directory holding `files`, as the kernel writes them: each value
/// on a line of its own.
fn served(files: &[(&str, &str)]) -> scratchdir::ScratchDir {
    let dir = scratchdir::ScratchDir::new("netlink");
    for (name, value) in files {
        std::fs::write(dir.path(name), format!("{value}\n")).expect("written");
    }
    dir
}

/// QEMU's user network, as the boot test's machine has it.
const QEMU: [(&str, &str); 6] = [
    ("up", "1"),
    ("ip", "10.0.2.15"),
    ("subnet_mask", "255.255.255.0"),
    ("gateway", "10.0.2.2"),
    ("dns", "10.0.2.3"),
    ("mac", "52:54:00:12:34:56"),
];

/// **The link is read as the kernel writes it**: up, the address with its
/// mask's prefix, the gateway, the name server and the adapter.
#[test]
fn the_link_is_read_as_the_kernel_writes_it() {
    let dir = served(&QEMU);
    assert_eq!(
        read(dir.dir()).unwrap(),
        LinkState::Link(Link {
            up: true,
            address: Some(Ipv4Addr::new(10, 0, 2, 15)),
            prefix: Some(24),
            gateway: Some(Ipv4Addr::new(10, 0, 2, 2)),
            dns: Some(Ipv4Addr::new(10, 0, 2, 3)),
            mac: "52:54:00:12:34:56".to_owned(),
        })
    );
}

/// **An address not yet given is none**, not `0.0.0.0`: a link that is down
/// and has been given nothing says so.
#[test]
fn an_address_not_given_is_none() {
    let dir = served(&[
        ("up", "0"),
        ("ip", "0.0.0.0"),
        ("subnet_mask", "0.0.0.0"),
        ("gateway", "0.0.0.0"),
        ("dns", "0.0.0.0"),
        ("mac", "52:54:00:12:34:56"),
    ]);
    let LinkState::Link(link) = read(dir.dir()).unwrap() else {
        panic!("no link read");
    };
    assert!(!link.up);
    assert_eq!(
        (link.address, link.prefix, link.gateway, link.dns),
        (None, None, None, None)
    );
}

/// **No directory is no adapter**: the kernel serves it only while there is
/// one.
#[test]
fn no_directory_is_no_adapter() {
    let dir = scratchdir::ScratchDir::new("netlink-none");
    assert_eq!(read(&dir.path("net")).unwrap(), LinkState::NoAdapter);
}

/// **A file the kernel would not write is an error, not a guess** -- an
/// up that is neither 0 nor 1, an address that is no address, a file
/// missing from a directory that is there.
#[test]
fn a_file_the_kernel_would_not_write_is_an_error() {
    for (name, bad) in [("up", "yes"), ("ip", "10.0.2"), ("dns", "localhost")] {
        let files: Vec<(&str, &str)> = QEMU
            .iter()
            .map(|&(n, v)| if n == name { (n, bad) } else { (n, v) })
            .collect();
        let dir = served(&files);
        let err = read(dir.dir()).expect_err(name);
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{name}");
    }
    let dir = served(&QEMU[..5]);
    assert!(read(dir.dir()).is_err(), "a missing file is read");
}

/// **A mask's prefix is its leading ones**, and a mask that is not a run of
/// ones has none.
#[test]
fn a_masks_prefix_is_its_leading_ones() {
    assert_eq!(prefix_len(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
    assert_eq!(prefix_len(Ipv4Addr::new(255, 255, 240, 0)), Some(20));
    assert_eq!(prefix_len(Ipv4Addr::BROADCAST), Some(32));
    assert_eq!(prefix_len(Ipv4Addr::UNSPECIFIED), Some(0));
    assert_eq!(prefix_len(Ipv4Addr::new(255, 0, 255, 0)), None);
    assert_eq!(prefix_len(Ipv4Addr::new(0, 255, 255, 255)), None);
}

/// **Off SlateOS the link is not known**, rather than said to be missing:
/// these files are SlateOS's layout, not the machine's network.
#[cfg(not(all(target_os = "linux", target_vendor = "slateos")))]
#[test]
fn off_slateos_the_link_is_not_known() {
    assert_eq!(current().unwrap(), LinkState::Unknown);
}
