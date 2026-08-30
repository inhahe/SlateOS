//! `<linux/kfd_ioctl.h>` — AMD KFD compute ioctl constants.
//!
//! The AMD Kernel Fusion Driver (KFD) exposes `/dev/kfd` for ROCm
//! compute workloads. Userspace (HSA runtime, ROCr, HIP) opens the
//! device and issues these ioctls to create process address spaces,
//! map GPU memory, configure queues, and manage SVM. Constants below
//! cover the ioctl base, capability flags, queue types, and the
//! interface version.

// ---------------------------------------------------------------------------
// API version (KFD_IOCTL_MAJOR/MINOR_VERSION)
// ---------------------------------------------------------------------------

/// Major version negotiated with userspace.
pub const KFD_IOCTL_MAJOR_VERSION: u32 = 1;
/// Minor version (incremented per ABI-additive change).
pub const KFD_IOCTL_MINOR_VERSION: u32 = 16;

// ---------------------------------------------------------------------------
// ioctl group magic (KFD_IOCTL_BASE = 'K')
// ---------------------------------------------------------------------------

/// Magic byte used in the kfd ioctl encoding.
pub const KFD_IOCTL_BASE: u8 = b'K';

// ---------------------------------------------------------------------------
// Queue types (struct kfd_ioctl_create_queue_args.queue_type)
// ---------------------------------------------------------------------------

/// Compute queue (HSAIL kernels).
pub const KFD_IOC_QUEUE_TYPE_COMPUTE: u32 = 0;
/// SDMA (DMA copy/scatter) queue.
pub const KFD_IOC_QUEUE_TYPE_SDMA: u32 = 1;
/// Compute queue managed by the firmware AQL scheduler.
pub const KFD_IOC_QUEUE_TYPE_COMPUTE_AQL: u32 = 2;
/// SDMA queue that bypasses the firmware scheduler.
pub const KFD_IOC_QUEUE_TYPE_SDMA_XGMI: u32 = 3;

// ---------------------------------------------------------------------------
// Memory allocation flags (struct kfd_ioctl_alloc_memory_of_gpu_args.flags)
// ---------------------------------------------------------------------------

/// Allocate from device-local VRAM.
pub const KFD_IOC_ALLOC_MEM_FLAGS_VRAM: u32 = 1 << 0;
/// Allocate from system GTT (host pinned).
pub const KFD_IOC_ALLOC_MEM_FLAGS_GTT: u32 = 1 << 1;
/// Userptr (registers existing host pages).
pub const KFD_IOC_ALLOC_MEM_FLAGS_USERPTR: u32 = 1 << 2;
/// Doorbell page allocation.
pub const KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL: u32 = 1 << 3;
/// MMIO-remapped allocation.
pub const KFD_IOC_ALLOC_MEM_FLAGS_MMIO_REMAP: u32 = 1 << 4;
/// Buffer is writable.
pub const KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE: u32 = 1 << 31;
/// Buffer is executable.
pub const KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE: u32 = 1 << 30;
/// Buffer is publicly visible to other processes via shared handle.
pub const KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC: u32 = 1 << 29;
/// Buffer is no-substitute (driver may not migrate).
pub const KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE: u32 = 1 << 28;

// ---------------------------------------------------------------------------
// Event types (struct kfd_ioctl_create_event_args.event_type)
// ---------------------------------------------------------------------------

/// Signal event (asynchronous notification).
pub const KFD_IOC_EVENT_SIGNAL: u32 = 0;
/// Node-change event (CPU/GPU topology updated).
pub const KFD_IOC_EVENT_NODECHANGE: u32 = 1;
/// Device-state-change event.
pub const KFD_IOC_EVENT_DEVICESTATECHANGE: u32 = 2;
/// Hardware exception (page fault, memory violation).
pub const KFD_IOC_EVENT_HW_EXCEPTION: u32 = 3;
/// System event (e.g., out-of-memory).
pub const KFD_IOC_EVENT_SYSTEM_EVENT: u32 = 4;
/// Debug event (gpu-debugger attached).
pub const KFD_IOC_EVENT_DEBUG_EVENT: u32 = 5;
/// Memory event (host page-fault).
pub const KFD_IOC_EVENT_MEMORY: u32 = 8;

// ---------------------------------------------------------------------------
// Maximums
// ---------------------------------------------------------------------------

/// Maximum number of queues per process.
pub const KFD_MAX_NUM_OF_QUEUES_PER_PROCESS: u32 = 1024;
/// Maximum number of GPUs visible to one process.
pub const KFD_MAX_NUM_OF_GPUS: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_sane() {
        // Major is positive; minor must fit easily in u32.
        assert!(KFD_IOCTL_MAJOR_VERSION >= 1);
        assert!(KFD_IOCTL_MINOR_VERSION < 1024);
        assert_eq!(KFD_IOCTL_BASE, b'K');
    }

    #[test]
    fn test_queue_types_distinct() {
        let q = [
            KFD_IOC_QUEUE_TYPE_COMPUTE,
            KFD_IOC_QUEUE_TYPE_SDMA,
            KFD_IOC_QUEUE_TYPE_COMPUTE_AQL,
            KFD_IOC_QUEUE_TYPE_SDMA_XGMI,
        ];
        for i in 0..q.len() {
            for j in (i + 1)..q.len() {
                assert_ne!(q[i], q[j]);
            }
        }
    }

    #[test]
    fn test_alloc_flag_bits_distinct_and_pow2() {
        let f = [
            KFD_IOC_ALLOC_MEM_FLAGS_VRAM,
            KFD_IOC_ALLOC_MEM_FLAGS_GTT,
            KFD_IOC_ALLOC_MEM_FLAGS_USERPTR,
            KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL,
            KFD_IOC_ALLOC_MEM_FLAGS_MMIO_REMAP,
            KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE,
            KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE,
            KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC,
            KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE,
        ];
        for &flag in &f {
            assert!(flag.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_event_types_distinct() {
        let e = [
            KFD_IOC_EVENT_SIGNAL,
            KFD_IOC_EVENT_NODECHANGE,
            KFD_IOC_EVENT_DEVICESTATECHANGE,
            KFD_IOC_EVENT_HW_EXCEPTION,
            KFD_IOC_EVENT_SYSTEM_EVENT,
            KFD_IOC_EVENT_DEBUG_EVENT,
            KFD_IOC_EVENT_MEMORY,
        ];
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
    }

    #[test]
    fn test_max_limits_power_of_two() {
        assert!(KFD_MAX_NUM_OF_QUEUES_PER_PROCESS.is_power_of_two());
        assert!(KFD_MAX_NUM_OF_GPUS.is_power_of_two());
    }
}
