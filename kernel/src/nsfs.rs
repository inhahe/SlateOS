//! Handles on namespaces: what opening `/proc/<pid>/ns/<kind>` gives, and
//! `setns(2)` takes -- Linux's `nsfs`.
//!
//! A handle names one namespace of one kind, and holds it: the namespace
//! lives while a process is in it or a handle is open on it, as on Linux,
//! where an open namespace file keeps the namespace alive with no process in
//! it (what `ip netns` relies on). A handle is a capability resource of type
//! [`ResourceType::Namespace`](crate::cap::ResourceType::Namespace), so a
//! process's exit gives its holds back and a fork's child takes its own
//! (`ipc::cleanup_handles`, `proc::fork`).
//!
//! The raw handle is the kind in the top byte and the namespace's id below
//! it ([`encode`], [`decode`]). One kind exists so far, [`NsKind::Uts`]
//! (`crate::utsns`); the others follow as they are built.

use crate::sync::Mutex;
use alloc::string::String;

/// The kinds of namespace a handle can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NsKind {
    /// A UTS namespace (`crate::utsns`): host and domain names.
    Uts,
}

impl NsKind {
    /// The top byte of a raw handle of this kind.
    const fn tag(self) -> u64 {
        match self {
            Self::Uts => 1,
        }
    }

    /// The kind a raw handle's top byte names.
    const fn from_tag(tag: u64) -> Option<Self> {
        match tag {
            1 => Some(Self::Uts),
            _ => None,
        }
    }

    /// Its name in `/proc/<pid>/ns/` and in the link text (`uts:[N]`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Uts => "uts",
        }
    }

    /// The kind `/proc/<pid>/ns/<name>` names.
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<Self> {
        match name {
            b"uts" => Some(Self::Uts),
            _ => None,
        }
    }

    /// Its `CLONE_NEW*` bit: what `setns` names a kind by.
    #[must_use]
    pub const fn clone_flag(self) -> u64 {
        match self {
            Self::Uts => 0x0400_0000,
        }
    }
}

/// The kinds `/proc/<pid>/ns/` lists, in Linux's order.
pub const KINDS: &[NsKind] = &[NsKind::Uts];

/// Bits of a raw handle below the kind's byte: the namespace's id.
const ID_MASK: u64 = (1 << 56) - 1;

/// A raw handle naming namespace `id` of `kind`.
#[must_use]
pub const fn encode(kind: NsKind, id: u64) -> u64 {
    (kind.tag() << 56) | (id & ID_MASK)
}

/// The kind and namespace a raw handle names, or `None` for one that names
/// no kind.
#[must_use]
pub const fn decode(raw: u64) -> Option<(NsKind, u64)> {
    match NsKind::from_tag(raw >> 56) {
        Some(kind) => Some((kind, raw & ID_MASK)),
        None => None,
    }
}

/// The namespace process `pid` is in, of `kind`; `None` without the process.
#[must_use]
pub fn of_process(kind: NsKind, pid: crate::proc::pcb::ProcessId) -> Option<u64> {
    match kind {
        NsKind::Uts => crate::proc::pcb::uts_ns(pid),
    }
}

/// One more hold on the namespace `raw` names; `false` if it is gone.
pub fn retain(raw: u64) -> bool {
    match decode(raw) {
        Some((NsKind::Uts, id)) => crate::utsns::retain(id),
        None => false,
    }
}

/// One hold on the namespace `raw` names given up.
pub fn release(raw: u64) {
    if let Some((NsKind::Uts, id)) = decode(raw) {
        crate::utsns::release(id);
    }
}

/// The inode number Linux reports for a namespace: `st_ino` of its file and
/// the `N` in its link text. The root namespaces' are Linux's constants
/// (`PROC_UTS_INIT_INO` is `0xEFFF_FFFE`, 4026531838), so a program that
/// knows them recognises the root; another's is a number no root has.
#[must_use]
pub const fn inum(kind: NsKind, id: u64) -> u64 {
    match kind {
        NsKind::Uts => {
            if id == crate::utsns::ROOT_UTS {
                0xEFFF_FFFE
            } else {
                0xF000_0000_u64.wrapping_add(id)
            }
        }
    }
}

/// nsfs's device number, once taken (0 until then).
static DEV: Mutex<u32> = Mutex::new(0);

/// nsfs's device number: `st_dev`'s minor, under major 0, of every
/// namespace's file. Taken the first time it is asked from the numbering the
/// mounted filesystems take theirs from (`fs::vfs::reserve_dev`), so no file
/// on a mount shares it, and kept for good, as Linux's nsfs keeps the
/// anonymous device it is given at boot.
#[must_use]
pub fn dev() -> u32 {
    let mut dev = DEV.lock();
    if *dev == 0 {
        *dev = crate::fs::vfs::reserve_dev();
    }
    *dev
}

/// The text of `/proc/<pid>/ns/<kind>`'s link: `uts:[4026531838]`.
#[must_use]
pub fn link_text(kind: NsKind, id: u64) -> String {
    alloc::format!("{}:[{}]", kind.name(), inum(kind, id))
}

/// Encode/decode round trips, a raw handle of no kind is refused, and the
/// root UTS namespace's inode is Linux's.
///
/// # Errors
///
/// `InternalError` on the first check that fails.
pub fn self_test() -> crate::error::KernelResult<()> {
    let ok = decode(encode(NsKind::Uts, 42)) == Some((NsKind::Uts, 42))
        && decode(0).is_none()
        && decode(0xFF << 56).is_none()
        && inum(NsKind::Uts, crate::utsns::ROOT_UTS) == 4_026_531_838
        && link_text(NsKind::Uts, crate::utsns::ROOT_UTS) == "uts:[4026531838]"
        && NsKind::from_name(b"uts") == Some(NsKind::Uts)
        && NsKind::from_name(b"net").is_none();
    if !ok {
        crate::serial_println!("[nsfs]   FAIL: a handle's encoding or a link's text is wrong");
        return Err(crate::error::KernelError::InternalError);
    }
    crate::serial_println!("[nsfs]   handle encoding and link text: OK");
    Ok(())
}
