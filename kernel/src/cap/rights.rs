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

    /// Modify metadata (permissions, attributes, etc.).
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

    // --- Convenience combinations ---

    /// All rights.
    pub const ALL: Self = Self(u64::MAX);

    /// No rights.
    #[allow(dead_code)] // public API; convenience constant for capability creation
    pub const NONE: Self = Self(0);

    /// Typical read-only: read + wait + duplicate.
    pub const READ_ONLY: Self = Self(
        Self::READ.0 | Self::WAIT.0 | Self::DUPLICATE.0,
    );

    /// Typical read-write: read + write + wait + duplicate.
    pub const READ_WRITE: Self = Self(
        Self::READ.0 | Self::WRITE.0 | Self::WAIT.0 | Self::DUPLICATE.0,
    );

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
