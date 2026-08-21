//! Capability rights — what operations a capability permits.
//!
//! Rights are a bitfield.  When delegating a capability, you can
//! only grant a subset of rights you hold (AND-mask, never add bits).
//!
//! The bits are grouped by subsystem for clarity, but any capability
//! can carry any combination of rights.

/// A set of rights that a capability grants.
///
/// Rights are a 64-bit bitfield.  Common operations (read, write,
/// etc.) occupy the low bits; subsystem-specific rights use higher
/// bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rights(u64);

impl Rights {
    // --- Common rights (bits 0–15) ---

    /// Read data from the resource.
    pub const READ: Self = Self(1 << 0);

    /// Write data to the resource.
    pub const WRITE: Self = Self(1 << 1);

    /// Execute/invoke the resource (e.g., run a program).
    pub const EXECUTE: Self = Self(1 << 2);

    /// Create child objects within the resource.
    pub const CREATE: Self = Self(1 << 3);

    /// Delete the resource or child objects within it.
    pub const DELETE: Self = Self(1 << 4);

    /// **Access** a resource's metadata — its size, times, mode, link target,
    /// extended attributes, and the filesystem statistics behind it.
    ///
    /// This bit is what `stat`, `lstat`, `readlink`, `getxattr`, `listxattr`,
    /// `statvfs` and `flock` are gated on. It is deliberately *not* implied by
    /// [`READ`](Self::READ): "may learn this file's size" and "may read this
    /// file's bytes" are different authorities, and an indexer or a `du` should
    /// be able to hold the first without the second.
    ///
    /// **It does not gate metadata *modification*.** Changing metadata —
    /// `setxattr`, `removexattr`, and in future `chmod`/`utimes` — requires
    /// [`WRITE`](Self::WRITE), on the principle that mutating a file's
    /// attributes is a write to the file's inode. This doc used to read
    /// "Modify metadata (permissions, attributes, etc.)", which described
    /// neither the gates that exist nor the ones that check this bit; the
    /// mismatch cost a cross-lane investigation, because a reader who had
    /// correctly noted that stat-by-path is gated on a *modify* right
    /// reasonably concluded that could not be the cause of an `EACCES` on a
    /// read. If a future change does want a distinct "may alter attributes"
    /// authority, give it its own bit rather than overloading this one — the
    /// same argument [`SET_CREDENTIALS`](Self::SET_CREDENTIALS) makes at
    /// length, and there are 52 free bits.
    pub const METADATA: Self = Self(1 << 5);

    /// Transfer (delegate) this capability to another task.
    pub const TRANSFER: Self = Self(1 << 6);

    /// Duplicate this capability (create another handle to the same
    /// resource, possibly with fewer rights).
    pub const DUPLICATE: Self = Self(1 << 7);

    /// Wait on this resource (register with a completion port).
    pub const WAIT: Self = Self(1 << 8);

    /// Signal this resource (e.g., write to an eventfd).
    pub const SIGNAL: Self = Self(1 << 9);

    // --- Subsystem-specific rights (bits 16–31) ---

    /// Permission to use Realtime I/O priority class.
    ///
    /// Required on an `IoScheduler` capability to submit I/O requests
    /// at the Realtime priority class.  Without this right, Realtime
    /// requests from userspace are downgraded to BestEffort.
    pub const IO_REALTIME: Self = Self(1 << 16);

    /// Debug / unilateral-introspection authority over a process.
    ///
    /// Required on a [`Process`](crate::cap::ResourceType::Process)
    /// capability to read or write the target process's memory
    /// **across address spaces** via `process_vm_readv` /
    /// `process_vm_writev` (and, in future, to `ptrace`-attach).  This
    /// is *unilateral* introspection — the target does not consent — so
    /// it must be granted explicitly (parent→child, or by a privileged
    /// debugger broker), never derived from ambient PID/uid authority.
    /// Consensual memory sharing is a separate path (channel +
    /// shared-memory IPC) that never touches this right.
    /// See design-decisions.md §24 (open-questions Q6).
    pub const DEBUG: Self = Self(1 << 17);

    /// Authority to change a process's own uid/gid credentials.
    ///
    /// Required on a [`Process`](crate::cap::ResourceType::Process)
    /// capability to be projected `CAP_SETUID`/`CAP_SETGID` in the
    /// userspace POSIX capability view (`posix::sys_capability::
    /// kernel_view::project`), which is what gates `setuid()`/`setgid()`
    /// before they issue `SYS_PROCESS_SET_CREDENTIALS`.
    ///
    /// **Why this is its own bit and not [`METADATA`](Self::METADATA).**
    /// "May become another user" is the single most escalation-prone
    /// authority in the POSIX model, and `METADATA` is a *generic* bit
    /// documented as "permissions, attributes, etc." — the bit the next
    /// person wanting "may rename this process" or "may set this
    /// process's nice value" will reach for. Because the grant sites are
    /// in the kernel and the projection is in `posix`, such a grant would
    /// silently confer root-capability with nothing in either file to say
    /// so. A distinct bit makes that aliasing impossible; `Rights` is a
    /// `u64` with 52 bits still free, so it costs nothing to be precise.
    /// This mirrors why [`DEBUG`](Self::DEBUG) is not `READ | WRITE`.
    /// See design-decisions.md §207.
    pub const SET_CREDENTIALS: Self = Self(1 << 18);

    /// Every distinct right, in declaration order.
    ///
    /// Exists so that [`the aliasing assertion below`](self) can be stated
    /// once over the whole set rather than pairwise by hand. Convenience
    /// *combinations* (`ALL`, `READ_ONLY`, …) are deliberately absent — they
    /// are unions of these and would defeat the check.
    const DISTINCT: [Self; 13] = [
        Self::READ,
        Self::WRITE,
        Self::EXECUTE,
        Self::CREATE,
        Self::DELETE,
        Self::METADATA,
        Self::TRANSFER,
        Self::DUPLICATE,
        Self::WAIT,
        Self::SIGNAL,
        Self::IO_REALTIME,
        Self::DEBUG,
        Self::SET_CREDENTIALS,
    ];

    // --- Convenience combinations ---

    /// All rights.
    pub const ALL: Self = Self(u64::MAX);

    /// No rights.
    #[allow(dead_code)] // public API; convenience constant for capability creation
    pub const NONE: Self = Self(0);

    /// Typical read-only: read + wait + duplicate.
    pub const READ_ONLY: Self = Self(Self::READ.0 | Self::WAIT.0 | Self::DUPLICATE.0);

    /// Typical read-write: read + write + wait + duplicate.
    pub const READ_WRITE: Self =
        Self(Self::READ.0 | Self::WRITE.0 | Self::WAIT.0 | Self::DUPLICATE.0);

    // --- Constructors ---

    /// Create an empty rights set.
    #[must_use]
    #[allow(dead_code)] // public API; constructor for capability creation
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create from a raw bitfield value.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw bitfield value.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    // --- Operations ---

    /// Check if this rights set contains a specific right.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Union of two rights sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Intersection of two rights sets (the subset of rights
    /// common to both).
    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Remove specific rights from this set.
    #[must_use]
    #[allow(dead_code)] // public API; counterpart to union/intersect for capability narrowing
    pub const fn remove(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Check if this rights set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Check if `self` is a subset of `other` (delegation check).
    ///
    /// A task can only delegate rights it already holds.
    #[must_use]
    pub const fn is_subset_of(self, other: Self) -> bool {
        (self.0 & !other.0) == 0
    }
}

/// Two rights must never name the same bit — checked at **compile time**.
///
/// This is the guard for the failure mode `design-decisions.md` §207 is about.
/// A right is a name for an *authority*, and two authorities sharing a bit
/// means granting one silently grants the other. That is not a bug that shows
/// up in a test run: the grant site and the check site are usually in
/// different crates (often different lanes), so no single diff contains both
/// halves, and the system behaves perfectly right up until someone is handed
/// an authority nobody meant to give them.
///
/// A `const` block rather than a `self_test()` on purpose — the boot-time
/// self-tests are the convention here, but a self-test can be skipped, run
/// late, or (as `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT` found) silently not
/// run at all. An aliasing mistake is decidable from the constants alone, so
/// it should be impossible to *build*, not merely detected on boot.
const _: () = {
    let mut i = 0;
    while i < Rights::DISTINCT.len() {
        // Every right is exactly one bit.  A right that is a union has no
        // single meaning to grant or revoke, so it does not belong here.
        assert!(
            Rights::DISTINCT[i].0.count_ones() == 1,
            "each entry of Rights::DISTINCT must be a single bit"
        );
        let mut j = i + 1;
        while j < Rights::DISTINCT.len() {
            assert!(
                Rights::DISTINCT[i].0 != Rights::DISTINCT[j].0,
                "two Rights share a bit — granting one would silently grant the other"
            );
            j += 1;
        }
        i += 1;
    }
};

impl core::ops::BitOr for Rights {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl core::ops::BitAnd for Rights {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersect(rhs)
    }
}

impl core::fmt::Display for Rights {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        let flags = [
            (Self::READ, "r"),
            (Self::WRITE, "w"),
            (Self::EXECUTE, "x"),
            (Self::CREATE, "c"),
            (Self::DELETE, "d"),
            (Self::METADATA, "m"),
            (Self::TRANSFER, "t"),
            (Self::DUPLICATE, "dup"),
            (Self::WAIT, "wait"),
            (Self::SIGNAL, "sig"),
            (Self::IO_REALTIME, "io_rt"),
            (Self::DEBUG, "debug"),
            (Self::SET_CREDENTIALS, "setcred"),
        ];

        for (flag, name) in &flags {
            if self.contains(*flag) {
                if !first {
                    write!(f, "+")?;
                }
                write!(f, "{name}")?;
                first = false;
            }
        }

        if first {
            write!(f, "none")?;
        }

        Ok(())
    }
}
