//! `<xen/privcmd.h>` — Xen privileged command interface constants.
//!
//! The Xen privcmd interface (/dev/xen/privcmd) allows Dom0 or
//! privileged domains to issue hypercalls directly to the Xen
//! hypervisor. This is used by management tools (xl, libxl, XAPI) to
//! create/destroy domains, manage memory, configure devices, and
//! perform live migration. The privcmd driver mediates access between
//! userspace tools and the hypervisor's hypercall interface.

// ---------------------------------------------------------------------------
// Privcmd IOCTLs
// ---------------------------------------------------------------------------

/// Issue a raw hypercall.
pub const IOCTL_PRIVCMD_HYPERCALL: u32 = 0x00;
/// Map foreign memory pages (from another domain).
pub const IOCTL_PRIVCMD_MMAP: u32 = 0x02;
/// Batch map foreign pages.
pub const IOCTL_PRIVCMD_MMAPBATCH: u32 = 0x03;
/// Batch map v2 (with per-page error reporting).
pub const IOCTL_PRIVCMD_MMAPBATCH_V2: u32 = 0x04;
/// DM (device model) operation.
pub const IOCTL_PRIVCMD_DM_OP: u32 = 0x05;
/// Restrict privcmd to a specific domain.
pub const IOCTL_PRIVCMD_RESTRICT: u32 = 0x06;
/// Map device MMIO resource.
pub const IOCTL_PRIVCMD_MMAP_RESOURCE: u32 = 0x07;
/// ioeventfd (notify on guest I/O write).
pub const IOCTL_PRIVCMD_IOEVENTFD: u32 = 0x08;
/// irqfd (inject interrupt on eventfd signal).
pub const IOCTL_PRIVCMD_IRQFD: u32 = 0x09;

// ---------------------------------------------------------------------------
// Xen hypercall numbers
// ---------------------------------------------------------------------------

/// Set trap table (IDT equivalent for Xen PV).
pub const XEN_HYPERCALL_SET_TRAP_TABLE: u32 = 0;
/// MMU update (modify page tables).
pub const XEN_HYPERCALL_MMU_UPDATE: u32 = 1;
/// Set GDT.
pub const XEN_HYPERCALL_SET_GDT: u32 = 2;
/// Stack switch.
pub const XEN_HYPERCALL_STACK_SWITCH: u32 = 3;
/// Set callbacks (event handlers).
pub const XEN_HYPERCALL_SET_CALLBACKS: u32 = 4;
/// FPU taskswitch hint.
pub const XEN_HYPERCALL_FPU_TASKSWITCH: u32 = 5;
/// Platform operation (Dom0 only).
pub const XEN_HYPERCALL_PLATFORM_OP: u32 = 6;
/// Memory operation (alloc/free/share).
pub const XEN_HYPERCALL_MEMORY_OP: u32 = 12;
/// Multicall (batch multiple hypercalls).
pub const XEN_HYPERCALL_MULTICALL: u32 = 13;
/// Event channel operation.
pub const XEN_HYPERCALL_EVENT_CHANNEL_OP: u32 = 32;
/// Grant table operation (shared memory).
pub const XEN_HYPERCALL_GRANT_TABLE_OP: u32 = 20;
/// Domain control (create/destroy/pause).
pub const XEN_HYPERCALL_DOMCTL: u32 = 36;
/// System control.
pub const XEN_HYPERCALL_SYSCTL: u32 = 35;
/// Console I/O.
pub const XEN_HYPERCALL_CONSOLE_IO: u32 = 18;
/// HVM operation.
pub const XEN_HYPERCALL_HVM_OP: u32 = 34;

// ---------------------------------------------------------------------------
// Xen domain types
// ---------------------------------------------------------------------------

/// Dom0 (privileged management domain).
pub const XEN_DOMID_DOM0: u16 = 0;
/// Self domain ID (refers to calling domain).
pub const XEN_DOMID_SELF: u16 = 0x7FF0;
/// IO domain (for device passthrough).
pub const XEN_DOMID_IO: u16 = 0x7FF1;
/// XEN domain (hypervisor internal).
pub const XEN_DOMID_XEN: u16 = 0x7FF2;
/// Invalid domain ID.
pub const XEN_DOMID_INVALID: u16 = 0x7FFF;

// ---------------------------------------------------------------------------
// Xen memory flags
// ---------------------------------------------------------------------------

/// Add to physmap (map foreign pages).
pub const XENMEM_ADD_TO_PHYSMAP: u32 = 7;
/// Remove from physmap.
pub const XENMEM_REMOVE_FROM_PHYSMAP: u32 = 15;
/// Get memory map.
pub const XENMEM_MEMORY_MAP: u32 = 9;
/// Maximum reservation.
pub const XENMEM_MAXIMUM_RESERVATION: u32 = 4;
/// Current reservation.
pub const XENMEM_CURRENT_RESERVATION: u32 = 3;
/// Increase reservation.
pub const XENMEM_INCREASE_RESERVATION: u32 = 0;
/// Decrease reservation.
pub const XENMEM_DECREASE_RESERVATION: u32 = 1;
/// Populate physmap.
pub const XENMEM_POPULATE_PHYSMAP: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            IOCTL_PRIVCMD_HYPERCALL,
            IOCTL_PRIVCMD_MMAP,
            IOCTL_PRIVCMD_MMAPBATCH,
            IOCTL_PRIVCMD_MMAPBATCH_V2,
            IOCTL_PRIVCMD_DM_OP,
            IOCTL_PRIVCMD_RESTRICT,
            IOCTL_PRIVCMD_MMAP_RESOURCE,
            IOCTL_PRIVCMD_IOEVENTFD,
            IOCTL_PRIVCMD_IRQFD,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_hypercalls_distinct() {
        let calls = [
            XEN_HYPERCALL_SET_TRAP_TABLE,
            XEN_HYPERCALL_MMU_UPDATE,
            XEN_HYPERCALL_SET_GDT,
            XEN_HYPERCALL_STACK_SWITCH,
            XEN_HYPERCALL_SET_CALLBACKS,
            XEN_HYPERCALL_FPU_TASKSWITCH,
            XEN_HYPERCALL_PLATFORM_OP,
            XEN_HYPERCALL_MEMORY_OP,
            XEN_HYPERCALL_MULTICALL,
            XEN_HYPERCALL_EVENT_CHANNEL_OP,
            XEN_HYPERCALL_GRANT_TABLE_OP,
            XEN_HYPERCALL_DOMCTL,
            XEN_HYPERCALL_SYSCTL,
            XEN_HYPERCALL_CONSOLE_IO,
            XEN_HYPERCALL_HVM_OP,
        ];
        for i in 0..calls.len() {
            for j in (i + 1)..calls.len() {
                assert_ne!(calls[i], calls[j]);
            }
        }
    }

    #[test]
    fn test_domain_ids_distinct() {
        let ids = [
            XEN_DOMID_DOM0,
            XEN_DOMID_SELF,
            XEN_DOMID_IO,
            XEN_DOMID_XEN,
            XEN_DOMID_INVALID,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_memory_ops_distinct() {
        let ops = [
            XENMEM_ADD_TO_PHYSMAP,
            XENMEM_REMOVE_FROM_PHYSMAP,
            XENMEM_MEMORY_MAP,
            XENMEM_MAXIMUM_RESERVATION,
            XENMEM_CURRENT_RESERVATION,
            XENMEM_INCREASE_RESERVATION,
            XENMEM_DECREASE_RESERVATION,
            XENMEM_POPULATE_PHYSMAP,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }
}
