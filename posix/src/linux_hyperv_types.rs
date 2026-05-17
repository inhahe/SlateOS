//! `<linux/hyperv.h>` — Microsoft Hyper-V guest interface constants.
//!
//! Hyper-V provides paravirtualized interfaces to guest VMs via
//! synthetic MSRs, hypercalls, and VMBus (Virtual Machine Bus). VMBus
//! channels provide high-performance ring-buffer-based communication
//! between the guest and host partitions. Synthetic devices (netvsc,
//! storvsc, hv_balloon, etc.) communicate over VMBus channels. The
//! hypervisor also provides enlightenments for TLB flush, spinlocks,
//! timers, and APIC access.

// ---------------------------------------------------------------------------
// Hyper-V hypercall codes
// ---------------------------------------------------------------------------

/// Notify long spin wait (hint to hypervisor).
pub const HVCALL_NOTIFY_LONG_SPIN_WAIT: u32 = 0x0008;
/// Post message (synthetic interrupt).
pub const HVCALL_POST_MESSAGE: u32 = 0x005C;
/// Signal event (synthetic interrupt).
pub const HVCALL_SIGNAL_EVENT: u32 = 0x005D;
/// Flush virtual TLB.
pub const HVCALL_FLUSH_VIRTUAL_ADDRESS_SPACE: u32 = 0x0002;
/// Flush virtual TLB (specific list).
pub const HVCALL_FLUSH_VIRTUAL_ADDRESS_LIST: u32 = 0x0003;
/// Flush virtual TLB (extended, all processors).
pub const HVCALL_FLUSH_VIRTUAL_ADDRESS_SPACE_EX: u32 = 0x0013;
/// Flush virtual TLB (extended, list).
pub const HVCALL_FLUSH_VIRTUAL_ADDRESS_LIST_EX: u32 = 0x0014;
/// Send IPI (inter-processor interrupt).
pub const HVCALL_SEND_IPI: u32 = 0x000B;
/// Send IPI (extended).
pub const HVCALL_SEND_IPI_EX: u32 = 0x0015;

// ---------------------------------------------------------------------------
// Hyper-V synthetic MSR indices
// ---------------------------------------------------------------------------

/// Guest OS ID (must be set before other MSRs work).
pub const HV_X64_MSR_GUEST_OS_ID: u32 = 0x4000_0000;
/// Hypercall page address.
pub const HV_X64_MSR_HYPERCALL: u32 = 0x4000_0001;
/// Virtual processor index.
pub const HV_X64_MSR_VP_INDEX: u32 = 0x4000_0002;
/// System reset (write to reboot).
pub const HV_X64_MSR_RESET: u32 = 0x4000_0003;
/// Reference TSC page.
pub const HV_X64_MSR_REFERENCE_TSC: u32 = 0x4000_0021;
/// Time reference count.
pub const HV_X64_MSR_TIME_REF_COUNT: u32 = 0x4000_0020;
/// Synthetic interrupt control.
pub const HV_X64_MSR_SCONTROL: u32 = 0x4000_0080;
/// Synthetic timer config.
pub const HV_X64_MSR_STIMER0_CONFIG: u32 = 0x4000_00B0;
/// Synthetic timer count.
pub const HV_X64_MSR_STIMER0_COUNT: u32 = 0x4000_00B1;

// ---------------------------------------------------------------------------
// VMBus channel states
// ---------------------------------------------------------------------------

/// Channel offer (host is offering a device).
pub const VMBUS_CHANNEL_OFFER_STATE: u32 = 0;
/// Channel opened (guest accepted, ring buffers configured).
pub const VMBUS_CHANNEL_OPENED_STATE: u32 = 1;
/// Channel closing.
pub const VMBUS_CHANNEL_CLOSING_STATE: u32 = 2;
/// Channel closed.
pub const VMBUS_CHANNEL_CLOSED_STATE: u32 = 3;

// ---------------------------------------------------------------------------
// VMBus message types
// ---------------------------------------------------------------------------

/// Channel offer message.
pub const VMBUS_MSG_OFFERCHANNEL: u32 = 1;
/// Rescind channel offer.
pub const VMBUS_MSG_RESCIND_CHANNELOFFER: u32 = 2;
/// Request offers (guest asks host for available channels).
pub const VMBUS_MSG_REQUESTOFFERS: u32 = 3;
/// All offers delivered.
pub const VMBUS_MSG_ALLOFFERS_DELIVERED: u32 = 4;
/// Open channel.
pub const VMBUS_MSG_OPENCHANNEL: u32 = 5;
/// Open channel result.
pub const VMBUS_MSG_OPENCHANNEL_RESULT: u32 = 6;
/// Close channel.
pub const VMBUS_MSG_CLOSECHANNEL: u32 = 7;
/// GPA (Guest Physical Address) list for data transfer.
pub const VMBUS_MSG_GPADL_HEADER: u32 = 8;
/// GPADL created response.
pub const VMBUS_MSG_GPADL_CREATED: u32 = 9;
/// GPADL teardown.
pub const VMBUS_MSG_GPADL_TEARDOWN: u32 = 10;
/// GPADL torndown response.
pub const VMBUS_MSG_GPADL_TORNDOWN: u32 = 11;

// ---------------------------------------------------------------------------
// Hyper-V partition privilege flags
// ---------------------------------------------------------------------------

/// Access to virtual MSRs.
pub const HV_ACCESS_VP_RUNTIME: u32 = 1 << 0;
/// Access to partition reference counter.
pub const HV_ACCESS_PARTITION_REFERENCE_COUNTER: u32 = 1 << 1;
/// Access to synthetic timers.
pub const HV_ACCESS_SYNIC_TIMERS: u32 = 1 << 2;
/// Access to APIC MSRs.
pub const HV_ACCESS_APIC_MSRS: u32 = 1 << 3;
/// Access to hypercall MSR.
pub const HV_ACCESS_HYPERCALL_MSRS: u32 = 1 << 4;
/// Access to VP index.
pub const HV_ACCESS_VP_INDEX: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hypercalls_distinct() {
        let calls = [
            HVCALL_NOTIFY_LONG_SPIN_WAIT, HVCALL_POST_MESSAGE,
            HVCALL_SIGNAL_EVENT, HVCALL_FLUSH_VIRTUAL_ADDRESS_SPACE,
            HVCALL_FLUSH_VIRTUAL_ADDRESS_LIST,
            HVCALL_FLUSH_VIRTUAL_ADDRESS_SPACE_EX,
            HVCALL_FLUSH_VIRTUAL_ADDRESS_LIST_EX,
            HVCALL_SEND_IPI, HVCALL_SEND_IPI_EX,
        ];
        for i in 0..calls.len() {
            for j in (i + 1)..calls.len() {
                assert_ne!(calls[i], calls[j]);
            }
        }
    }

    #[test]
    fn test_msrs_distinct() {
        let msrs = [
            HV_X64_MSR_GUEST_OS_ID, HV_X64_MSR_HYPERCALL,
            HV_X64_MSR_VP_INDEX, HV_X64_MSR_RESET,
            HV_X64_MSR_REFERENCE_TSC, HV_X64_MSR_TIME_REF_COUNT,
            HV_X64_MSR_SCONTROL, HV_X64_MSR_STIMER0_CONFIG,
            HV_X64_MSR_STIMER0_COUNT,
        ];
        for i in 0..msrs.len() {
            for j in (i + 1)..msrs.len() {
                assert_ne!(msrs[i], msrs[j]);
            }
        }
    }

    #[test]
    fn test_channel_states_distinct() {
        let states = [
            VMBUS_CHANNEL_OFFER_STATE, VMBUS_CHANNEL_OPENED_STATE,
            VMBUS_CHANNEL_CLOSING_STATE, VMBUS_CHANNEL_CLOSED_STATE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_vmbus_messages_distinct() {
        let msgs = [
            VMBUS_MSG_OFFERCHANNEL, VMBUS_MSG_RESCIND_CHANNELOFFER,
            VMBUS_MSG_REQUESTOFFERS, VMBUS_MSG_ALLOFFERS_DELIVERED,
            VMBUS_MSG_OPENCHANNEL, VMBUS_MSG_OPENCHANNEL_RESULT,
            VMBUS_MSG_CLOSECHANNEL, VMBUS_MSG_GPADL_HEADER,
            VMBUS_MSG_GPADL_CREATED, VMBUS_MSG_GPADL_TEARDOWN,
            VMBUS_MSG_GPADL_TORNDOWN,
        ];
        for i in 0..msgs.len() {
            for j in (i + 1)..msgs.len() {
                assert_ne!(msgs[i], msgs[j]);
            }
        }
    }

    #[test]
    fn test_privilege_flags_no_overlap() {
        let flags = [
            HV_ACCESS_VP_RUNTIME,
            HV_ACCESS_PARTITION_REFERENCE_COUNTER,
            HV_ACCESS_SYNIC_TIMERS, HV_ACCESS_APIC_MSRS,
            HV_ACCESS_HYPERCALL_MSRS, HV_ACCESS_VP_INDEX,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
