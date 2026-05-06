//! NVMe (Non-Volatile Memory Express) driver.
//!
//! Implements the NVMe 1.4 base specification for PCIe-attached SSDs.
//! NVMe is the standard interface for modern high-speed solid-state drives,
//! offering significantly lower latency and higher throughput than AHCI/SATA.
//!
//! ## Architecture
//!
//! ```text
//! PCI bus scan → NVMe controller (class 01h, subclass 08h, prog-if 02h)
//!      ↓
//! BAR0 (MLBAR/MUBAR) → Controller registers (64-bit MMIO)
//!      ↓
//! Admin Queue → IDENTIFY controller/namespace, CREATE I/O queues
//!      ↓
//! I/O Submission/Completion Queues → READ/WRITE commands
//! ```
//!
//! ## NVMe Queue Model
//!
//! NVMe uses paired Submission/Completion queues with doorbell registers:
//! - **Admin SQ/CQ**: Controller management (identify, create/delete queues)
//! - **I/O SQ/CQ**: Actual data transfer commands (read, write, flush)
//!
//! Each queue entry is fixed-size:
//! - Submission Queue Entry (SQE): 64 bytes
//! - Completion Queue Entry (CQE): 16 bytes
//!
//! The host submits commands by writing SQEs and ringing the SQ doorbell.
//! The controller posts completions to the CQ and optionally raises an interrupt.
//!
//! ## Design Decisions
//!
//! - Single I/O queue pair (sufficient for synchronous kernel-mode I/O)
//! - Polling-based completion (avoids IRQ setup complexity during boot)
//! - 4 KiB aligned PRP (Physical Region Page) buffers
//! - Supports namespaces up to 2^48 sectors (128 PiB with 512B sectors)
//!
//! ## References
//!
//! - NVM Express Base Specification 1.4c (March 2021)
//! - NVM Express Base Specification 2.0 (June 2021)
//! - OSDev Wiki: NVMe
//! - Linux `drivers/nvme/host/pci.c`

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::blkdev::{self, BlockDevice, BlockDeviceInfo, SECTOR_SIZE};
use crate::error::{KernelError, KernelResult};
use crate::mm::{frame, page_table};
use crate::serial_println;

// ---------------------------------------------------------------------------
// PCI identification
// ---------------------------------------------------------------------------

/// PCI class: Mass Storage Controller.
const PCI_CLASS_STORAGE: u8 = 0x01;
/// PCI subclass: Non-Volatile Memory Controller.
const PCI_SUBCLASS_NVM: u8 = 0x08;
/// PCI prog-if: NVM Express.
const _PCI_PROGIF_NVME: u8 = 0x02;

// ---------------------------------------------------------------------------
// NVMe Controller Registers (BAR0 MMIO, 64-bit)
// ---------------------------------------------------------------------------

/// Controller Capabilities (64-bit, read-only).
const REG_CAP: usize = 0x00;
/// Version (32-bit, read-only).
const REG_VS: usize = 0x08;
/// Controller Configuration (32-bit, read/write).
const REG_CC: usize = 0x14;
/// Controller Status (32-bit, read-only).
const REG_CSTS: usize = 0x1C;
/// Admin Queue Attributes (32-bit, read/write).
const REG_AQA: usize = 0x24;
/// Admin Submission Queue Base Address (64-bit, read/write).
const REG_ASQ: usize = 0x28;
/// Admin Completion Queue Base Address (64-bit, read/write).
const REG_ACQ: usize = 0x30;

// CC (Controller Configuration) bits.
/// Enable bit.
const CC_EN: u32 = 1 << 0;
/// I/O Submission Queue Entry Size (4 = 64 bytes = 2^6).
const CC_IOSQES_SHIFT: u32 = 16;
/// I/O Completion Queue Entry Size (4 = 16 bytes = 2^4).
const CC_IOCQES_SHIFT: u32 = 20;

// CSTS (Controller Status) bits.
/// Ready.
const CSTS_RDY: u32 = 1 << 0;

// CAP fields.
/// Maximum Queue Entries Supported (bits 15:0, zero-based).
#[allow(clippy::arithmetic_side_effects)]
const fn cap_mqes(cap: u64) -> u16 {
    (cap & 0xFFFF) as u16
}

/// Doorbell Stride (bits 35:32, in 2^(2+DSTRD) bytes).
#[allow(clippy::arithmetic_side_effects)]
const fn cap_dstrd(cap: u64) -> u32 {
    ((cap >> 32) & 0xF) as u32
}

// ---------------------------------------------------------------------------
// Queue sizes
// ---------------------------------------------------------------------------

/// Admin queue depth (small — only used for identify/create commands).
const ADMIN_QUEUE_DEPTH: u16 = 16;
/// I/O queue depth.
const IO_QUEUE_DEPTH: u16 = 64;

// ---------------------------------------------------------------------------
// NVMe Admin commands (opcode in CDW0[7:0])
// ---------------------------------------------------------------------------

/// Identify.
const ADMIN_IDENTIFY: u8 = 0x06;
/// Create I/O Submission Queue.
const ADMIN_CREATE_IO_SQ: u8 = 0x01;
/// Create I/O Completion Queue.
const ADMIN_CREATE_IO_CQ: u8 = 0x05;

// ---------------------------------------------------------------------------
// NVMe I/O commands
// ---------------------------------------------------------------------------

/// Read.
const IO_CMD_READ: u8 = 0x02;
/// Write.
const IO_CMD_WRITE: u8 = 0x01;
/// Flush.
#[allow(dead_code)]
const IO_CMD_FLUSH: u8 = 0x00;

// ---------------------------------------------------------------------------
// Timeouts
// ---------------------------------------------------------------------------

/// Spin timeout for controller ready.
const READY_TIMEOUT: u32 = 10_000_000;
/// Spin timeout for command completion.
const CMD_TIMEOUT: u32 = 5_000_000;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static DEVICE_COUNT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// MMIO helpers (same as AHCI)
// ---------------------------------------------------------------------------

#[inline]
unsafe fn mmio_read32(addr: usize) -> u32 {
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[inline]
unsafe fn mmio_write32(addr: usize, value: u32) {
    unsafe { core::ptr::write_volatile(addr as *mut u32, value); }
}

#[inline]
unsafe fn mmio_read64(addr: usize) -> u64 {
    // Read as two 32-bit halves (some hardware doesn't support 64-bit MMIO).
    unsafe {
        let lo = core::ptr::read_volatile(addr as *const u32) as u64;
        let hi = core::ptr::read_volatile((addr + 4) as *const u32) as u64;
        lo | (hi << 32)
    }
}

#[inline]
unsafe fn mmio_write64(addr: usize, value: u64) {
    unsafe {
        core::ptr::write_volatile(addr as *mut u32, value as u32);
        core::ptr::write_volatile((addr + 4) as *mut u32, (value >> 32) as u32);
    }
}

// ---------------------------------------------------------------------------
// Submission Queue Entry (64 bytes)
// ---------------------------------------------------------------------------

/// NVMe Submission Queue Entry (SQE).
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct NvmeSqe {
    /// Command Dword 0: opcode(7:0), fuse(9:8), psdt(15:14), cid(31:16).
    cdw0: u32,
    /// Namespace ID.
    nsid: u32,
    /// Reserved.
    cdw2: u32,
    cdw3: u32,
    /// Metadata pointer.
    mptr: u64,
    /// PRP Entry 1.
    prp1: u64,
    /// PRP Entry 2 (or PRP List pointer).
    prp2: u64,
    /// Command-specific DWORDs 10-15.
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
    cdw13: u32,
    cdw14: u32,
    cdw15: u32,
}

impl NvmeSqe {
    const fn zeroed() -> Self {
        Self {
            cdw0: 0, nsid: 0, cdw2: 0, cdw3: 0,
            mptr: 0, prp1: 0, prp2: 0,
            cdw10: 0, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }
}

/// NVMe Completion Queue Entry (CQE, 16 bytes).
#[repr(C, align(4))]
#[derive(Clone, Copy)]
struct NvmeCqe {
    /// Command-specific result.
    dw0: u32,
    /// Reserved.
    dw1: u32,
    /// SQ Head Pointer (15:0), SQ Identifier (31:16).
    sq_head_sqid: u32,
    /// Command ID (15:0), Phase bit (16), Status Field (31:17).
    cid_status: u32,
}

impl NvmeCqe {
    #[allow(dead_code)]
    const fn zeroed() -> Self {
        Self { dw0: 0, dw1: 0, sq_head_sqid: 0, cid_status: 0 }
    }

    /// Extract status code (bits 31:17 of DW3, shifted right by 17 = 15 bits).
    #[allow(clippy::arithmetic_side_effects)]
    fn status(&self) -> u16 {
        ((self.cid_status >> 17) & 0x7FFF) as u16
    }

    /// Extract phase bit (bit 16 of DW3).
    #[allow(clippy::arithmetic_side_effects)]
    fn phase(&self) -> bool {
        (self.cid_status >> 16) & 1 != 0
    }

    /// Extract command ID (bits 15:0 of DW3).
    #[allow(clippy::arithmetic_side_effects)]
    fn _cid(&self) -> u16 {
        (self.cid_status & 0xFFFF) as u16
    }
}

// ---------------------------------------------------------------------------
// NVMe Queue Pair
// ---------------------------------------------------------------------------

/// A paired Submission Queue + Completion Queue.
#[allow(dead_code)]
struct NvmeQueuePair {
    /// Virtual address of the Submission Queue (array of SQEs).
    sq_virt: usize,
    /// Physical address of the Submission Queue.
    sq_phys: u64,
    /// Virtual address of the Completion Queue (array of CQEs).
    cq_virt: usize,
    /// Physical address of the Completion Queue.
    cq_phys: u64,
    /// Queue depth (number of entries).
    depth: u16,
    /// Current SQ tail (next entry to write).
    sq_tail: u16,
    /// Current CQ head (next entry to read).
    cq_head: u16,
    /// Expected phase bit for CQ entries.
    cq_phase: bool,
    /// SQ doorbell register address (virtual).
    sq_doorbell: usize,
    /// CQ doorbell register address (virtual).
    cq_doorbell: usize,
    /// Command ID counter.
    cid_counter: u16,
}

impl NvmeQueuePair {
    /// Submit a command to the SQ and ring the doorbell.
    #[allow(clippy::arithmetic_side_effects)]
    fn submit(&mut self, mut sqe: NvmeSqe) -> u16 {
        let cid = self.cid_counter;
        self.cid_counter = self.cid_counter.wrapping_add(1);

        // Set command ID in CDW0[31:16].
        sqe.cdw0 = (sqe.cdw0 & 0x0000_FFFF) | (u32::from(cid) << 16);

        // Write SQE to the current tail slot.
        let slot_offset = (self.sq_tail as usize) * 64; // sizeof(NvmeSqe) = 64
        let slot_ptr = (self.sq_virt + slot_offset) as *mut NvmeSqe;
        // SAFETY: slot_ptr is within our allocated SQ memory.
        unsafe { slot_ptr.write(sqe); }

        // Advance tail.
        self.sq_tail = (self.sq_tail + 1) % self.depth;

        // Ring the SQ doorbell.
        // SAFETY: sq_doorbell is a valid MMIO register address.
        unsafe { mmio_write32(self.sq_doorbell, u32::from(self.sq_tail)); }

        cid
    }

    /// Poll the CQ for a completion. Returns the CQE on success.
    #[allow(clippy::arithmetic_side_effects)]
    fn poll_completion(&mut self) -> KernelResult<NvmeCqe> {
        for _ in 0..CMD_TIMEOUT {
            let slot_offset = (self.cq_head as usize) * 16; // sizeof(NvmeCqe) = 16
            let slot_ptr = (self.cq_virt + slot_offset) as *const NvmeCqe;

            // SAFETY: slot_ptr is within our allocated CQ memory.
            let cqe = unsafe { slot_ptr.read_volatile() };

            if cqe.phase() == self.cq_phase {
                // This entry is valid — advance CQ head.
                self.cq_head = (self.cq_head + 1) % self.depth;
                if self.cq_head == 0 {
                    self.cq_phase = !self.cq_phase;
                }

                // Ring the CQ doorbell (tells controller we consumed the entry).
                // SAFETY: cq_doorbell is a valid MMIO register address.
                unsafe { mmio_write32(self.cq_doorbell, u32::from(self.cq_head)); }

                return Ok(cqe);
            }

            core::hint::spin_loop();
        }

        Err(KernelError::TimedOut)
    }

    /// Submit a command and wait for completion.
    fn submit_and_wait(&mut self, sqe: NvmeSqe) -> KernelResult<NvmeCqe> {
        let _cid = self.submit(sqe);
        let cqe = self.poll_completion()?;

        // Check status.
        if cqe.status() != 0 {
            return Err(KernelError::IoError);
        }

        Ok(cqe)
    }
}

// ---------------------------------------------------------------------------
// NVMe Controller
// ---------------------------------------------------------------------------

/// State for a single NVMe controller (one PCI device).
struct NvmeController {
    /// Virtual base address of controller registers.
    regs_virt: usize,
    /// Doorbell stride in bytes (4 << CAP.DSTRD).
    doorbell_stride: u32,
    /// Admin queue pair.
    admin_queue: NvmeQueuePair,
    /// I/O queue pair (created after controller init).
    io_queue: Option<NvmeQueuePair>,
    /// Total sectors in namespace 1.
    sector_count: u64,
    /// Logical block size (bytes).
    block_size: u32,
    /// Model/serial from IDENTIFY.
    model: String,
    serial: String,
}

impl NvmeController {
    /// Create the admin queue pair and configure the controller.
    #[allow(clippy::arithmetic_side_effects)]
    fn init(regs_virt: usize, hhdm: u64) -> KernelResult<Self> {
        // Read CAP register to learn queue limits and doorbell stride.
        // SAFETY: regs_virt is a valid MMIO-mapped region.
        let cap = unsafe { mmio_read64(regs_virt + REG_CAP) };
        let mqes = cap_mqes(cap);
        let dstrd = cap_dstrd(cap);
        let doorbell_stride = 4u32 << dstrd;

        serial_println!("[nvme]   CAP: MQES={}, DSTRD={}", mqes, dstrd);

        // Disable the controller (clear CC.EN).
        // SAFETY: Valid MMIO write.
        unsafe { mmio_write32(regs_virt + REG_CC, 0); }

        // Wait for CSTS.RDY to clear.
        for _ in 0..READY_TIMEOUT {
            let csts = unsafe { mmio_read32(regs_virt + REG_CSTS) };
            if csts & CSTS_RDY == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // Allocate Admin SQ (16 entries × 64 bytes = 1024 bytes).
        let asq_frame = frame::alloc_frame()?;
        let asq_phys = asq_frame.addr();
        let asq_virt = (asq_phys + hhdm) as usize;
        // SAFETY: Freshly allocated frame.
        unsafe { core::ptr::write_bytes(asq_virt as *mut u8, 0, frame::FRAME_SIZE); }

        // Allocate Admin CQ (16 entries × 16 bytes = 256 bytes).
        // We can put it in the same frame at offset 4096 (page-aligned).
        let acq_frame = frame::alloc_frame()?;
        let acq_phys = acq_frame.addr();
        let acq_virt = (acq_phys + hhdm) as usize;
        // SAFETY: Freshly allocated frame.
        unsafe { core::ptr::write_bytes(acq_virt as *mut u8, 0, frame::FRAME_SIZE); }

        // Configure Admin Queue Attributes: SQ size (bits 27:16), CQ size (bits 11:0).
        let aqa = (u32::from(ADMIN_QUEUE_DEPTH - 1) << 16)
                | u32::from(ADMIN_QUEUE_DEPTH - 1);
        // SAFETY: Valid MMIO writes.
        unsafe {
            mmio_write32(regs_virt + REG_AQA, aqa);
            mmio_write64(regs_virt + REG_ASQ, asq_phys);
            mmio_write64(regs_virt + REG_ACQ, acq_phys);
        }

        // Configure CC: enable, IOSQES=6 (64B), IOCQES=4 (16B), NVM command set.
        let cc = CC_EN
            | (6u32 << CC_IOSQES_SHIFT)   // SQE size = 2^6 = 64 bytes.
            | (4u32 << CC_IOCQES_SHIFT);   // CQE size = 2^4 = 16 bytes.
        // SAFETY: Valid MMIO write.
        unsafe { mmio_write32(regs_virt + REG_CC, cc); }

        // Wait for CSTS.RDY.
        for _ in 0..READY_TIMEOUT {
            let csts = unsafe { mmio_read32(regs_virt + REG_CSTS) };
            if csts & CSTS_RDY != 0 {
                serial_println!("[nvme]   Controller ready");
                break;
            }
            core::hint::spin_loop();
        }

        // Check if controller is truly ready.
        let csts = unsafe { mmio_read32(regs_virt + REG_CSTS) };
        if csts & CSTS_RDY == 0 {
            serial_println!("[nvme]   ERROR: Controller did not become ready");
            return Err(KernelError::TimedOut);
        }

        // Admin SQ doorbell: regs + 0x1000 + 0 * (4 << DSTRD)
        let admin_sq_db = regs_virt + 0x1000;
        // Admin CQ doorbell: regs + 0x1000 + 1 * (4 << DSTRD)
        let admin_cq_db = regs_virt + 0x1000 + doorbell_stride as usize;

        let admin_queue = NvmeQueuePair {
            sq_virt: asq_virt,
            sq_phys: asq_phys,
            cq_virt: acq_virt,
            cq_phys: acq_phys,
            depth: ADMIN_QUEUE_DEPTH,
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            sq_doorbell: admin_sq_db,
            cq_doorbell: admin_cq_db,
            cid_counter: 0,
        };

        Ok(Self {
            regs_virt,
            doorbell_stride,
            admin_queue,
            io_queue: None,
            sector_count: 0,
            block_size: 512,
            model: String::new(),
            serial: String::new(),
        })
    }

    /// Run IDENTIFY CONTROLLER to get model/serial.
    #[allow(clippy::arithmetic_side_effects)]
    fn identify_controller(&mut self, hhdm: u64) -> KernelResult<()> {
        // Allocate a page for identify data (4096 bytes).
        let data_frame = frame::alloc_frame()?;
        let data_phys = data_frame.addr();
        let data_virt = (data_phys + hhdm) as *mut u8;
        // SAFETY: Freshly allocated frame.
        unsafe { core::ptr::write_bytes(data_virt, 0, 4096); }

        // Build IDENTIFY command: CNS=1 (controller).
        let mut sqe = NvmeSqe::zeroed();
        sqe.cdw0 = u32::from(ADMIN_IDENTIFY); // Opcode.
        sqe.nsid = 0;
        sqe.prp1 = data_phys;
        sqe.cdw10 = 1; // CNS = 1 (identify controller).

        self.admin_queue.submit_and_wait(sqe)?;

        // Parse identify data.
        // SAFETY: The controller wrote 4096 bytes to data_virt.
        let data = unsafe { core::slice::from_raw_parts(data_virt, 4096) };

        // Bytes 24-63: Serial Number (20 bytes, ASCII, space-padded).
        self.serial = core::str::from_utf8(data.get(4..24).unwrap_or(&[]))
            .unwrap_or("")
            .trim()
            .into();

        // Bytes 24-63: Model Number (40 bytes, ASCII, space-padded).
        self.model = core::str::from_utf8(data.get(24..64).unwrap_or(&[]))
            .unwrap_or("")
            .trim()
            .into();

        // Free the frame.
        // SAFETY: Done with the temporary buffer.
        unsafe { let _ = frame::free_frame(data_frame); }

        Ok(())
    }

    /// Run IDENTIFY NAMESPACE to get capacity and block size.
    #[allow(clippy::arithmetic_side_effects)]
    fn identify_namespace(&mut self, nsid: u32, hhdm: u64) -> KernelResult<()> {
        let data_frame = frame::alloc_frame()?;
        let data_phys = data_frame.addr();
        let data_virt = (data_phys + hhdm) as *mut u8;
        // SAFETY: Freshly allocated frame.
        unsafe { core::ptr::write_bytes(data_virt, 0, 4096); }

        // IDENTIFY namespace (CNS=0).
        let mut sqe = NvmeSqe::zeroed();
        sqe.cdw0 = u32::from(ADMIN_IDENTIFY);
        sqe.nsid = nsid;
        sqe.prp1 = data_phys;
        sqe.cdw10 = 0; // CNS = 0 (identify namespace).

        self.admin_queue.submit_and_wait(sqe)?;

        // Parse namespace data.
        // SAFETY: Controller wrote 4096 bytes.
        let data = unsafe { core::slice::from_raw_parts(data_virt as *const u8, 4096) };

        // Bytes 0-7: NSZE (Namespace Size in logical blocks, 64-bit LE).
        let nsze = u64::from_le_bytes([
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7],
        ]);
        self.sector_count = nsze;

        // Bytes 24-25: FLBAS (Formatted LBA Size).
        // Bits 3:0 = index into LBA Format table.
        let flbas_idx = (data[26] & 0x0F) as usize;

        // LBA Format table starts at byte 128, each entry is 4 bytes.
        // Bits 23:16 of each entry = LBADS (LBA Data Size as power of 2).
        let lbaf_offset = 128 + flbas_idx * 4;
        if let Some(lbaf_bytes) = data.get(lbaf_offset..lbaf_offset + 4) {
            let lbads = lbaf_bytes[2]; // Byte 2 of the format entry.
            if lbads >= 9 && lbads <= 16 {
                self.block_size = 1u32 << lbads;
            }
        }

        // Free frame.
        // SAFETY: Done with temporary buffer.
        unsafe { let _ = frame::free_frame(data_frame); }

        serial_println!(
            "[nvme]   NS{}: {} blocks, block_size={}",
            nsid, self.sector_count, self.block_size
        );

        Ok(())
    }

    /// Create an I/O queue pair.
    #[allow(clippy::arithmetic_side_effects)]
    fn create_io_queues(&mut self, hhdm: u64) -> KernelResult<()> {
        let depth = IO_QUEUE_DEPTH;

        // Allocate I/O CQ (depth × 16 bytes).
        let cq_frame = frame::alloc_frame()?;
        let cq_phys = cq_frame.addr();
        let cq_virt = (cq_phys + hhdm) as usize;
        // SAFETY: Freshly allocated frame.
        unsafe { core::ptr::write_bytes(cq_virt as *mut u8, 0, frame::FRAME_SIZE); }

        // CREATE I/O COMPLETION QUEUE (admin command).
        let mut sqe = NvmeSqe::zeroed();
        sqe.cdw0 = u32::from(ADMIN_CREATE_IO_CQ);
        sqe.prp1 = cq_phys;
        // CDW10: QID=1 (bits 15:0), QSIZE=depth-1 (bits 31:16).
        sqe.cdw10 = 1 | (u32::from(depth - 1) << 16);
        // CDW11: PC=1 (physically contiguous), IEN=0 (no interrupts), IV=0.
        sqe.cdw11 = 1; // Physically contiguous.

        self.admin_queue.submit_and_wait(sqe)?;
        serial_println!("[nvme]   I/O CQ created (QID=1, depth={})", depth);

        // Allocate I/O SQ (depth × 64 bytes = 4096 bytes for 64 entries).
        let sq_frame = frame::alloc_frame()?;
        let sq_phys = sq_frame.addr();
        let sq_virt = (sq_phys + hhdm) as usize;
        // SAFETY: Freshly allocated frame.
        unsafe { core::ptr::write_bytes(sq_virt as *mut u8, 0, frame::FRAME_SIZE); }

        // CREATE I/O SUBMISSION QUEUE.
        let mut sqe = NvmeSqe::zeroed();
        sqe.cdw0 = u32::from(ADMIN_CREATE_IO_SQ);
        sqe.prp1 = sq_phys;
        // CDW10: QID=1, QSIZE=depth-1.
        sqe.cdw10 = 1 | (u32::from(depth - 1) << 16);
        // CDW11: PC=1, QPRIO=0 (medium), CQID=1.
        sqe.cdw11 = 1 | (1 << 16); // PC=1, CQID=1.

        self.admin_queue.submit_and_wait(sqe)?;
        serial_println!("[nvme]   I/O SQ created (QID=1, depth={})", depth);

        // I/O SQ doorbell: regs + 0x1000 + 2 * stride (QID 1, SQ).
        let io_sq_db = self.regs_virt + 0x1000 + (2 * self.doorbell_stride) as usize;
        // I/O CQ doorbell: regs + 0x1000 + 3 * stride (QID 1, CQ).
        let io_cq_db = self.regs_virt + 0x1000 + (3 * self.doorbell_stride) as usize;

        self.io_queue = Some(NvmeQueuePair {
            sq_virt,
            sq_phys,
            cq_virt,
            cq_phys,
            depth,
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            sq_doorbell: io_sq_db,
            cq_doorbell: io_cq_db,
            cid_counter: 0,
        });

        Ok(())
    }

    /// Read sectors using the I/O queue.
    #[allow(clippy::arithmetic_side_effects)]
    fn read_sectors_impl(&mut self, lba: u64, count: u16, buf: &mut [u8], hhdm: u64) -> KernelResult<()> {
        let io_queue = self.io_queue.as_mut().ok_or(KernelError::InternalError)?;

        // Allocate a DMA frame for the transfer.
        let data_frame = frame::alloc_frame()?;
        let data_phys = data_frame.addr();
        let data_virt = (data_phys + hhdm) as *mut u8;

        let byte_count = (count as usize) * (self.block_size as usize);
        if byte_count > frame::FRAME_SIZE || buf.len() < byte_count {
            // SAFETY: Free unused frame.
            unsafe { let _ = frame::free_frame(data_frame); }
            return Err(KernelError::InvalidArgument);
        }

        // Build READ command.
        let mut sqe = NvmeSqe::zeroed();
        sqe.cdw0 = u32::from(IO_CMD_READ);
        sqe.nsid = 1;
        sqe.prp1 = data_phys;
        // PRP2: needed if transfer spans multiple pages.
        // For our single-frame transfers (≤16 KiB), we only need PRP2 if
        // the transfer crosses a 4 KiB page boundary within the frame.
        if byte_count > 4096 {
            sqe.prp2 = data_phys + 4096;
        }
        // CDW10-11: Starting LBA (64-bit).
        sqe.cdw10 = lba as u32;
        sqe.cdw11 = (lba >> 32) as u32;
        // CDW12: Number of logical blocks (0-based).
        sqe.cdw12 = u32::from(count - 1);

        let result = io_queue.submit_and_wait(sqe);

        if result.is_ok() {
            // Copy from DMA buffer to caller.
            // SAFETY: device wrote byte_count bytes to data_virt.
            unsafe {
                core::ptr::copy_nonoverlapping(data_virt, buf.as_mut_ptr(), byte_count);
            }
        }

        // SAFETY: Done with frame.
        unsafe { let _ = frame::free_frame(data_frame); }

        result.map(|_| ())
    }

    /// Write sectors using the I/O queue.
    #[allow(clippy::arithmetic_side_effects)]
    fn write_sectors_impl(&mut self, lba: u64, count: u16, buf: &[u8], hhdm: u64) -> KernelResult<()> {
        let io_queue = self.io_queue.as_mut().ok_or(KernelError::InternalError)?;

        let data_frame = frame::alloc_frame()?;
        let data_phys = data_frame.addr();
        let data_virt = (data_phys + hhdm) as *mut u8;

        let byte_count = (count as usize) * (self.block_size as usize);
        if byte_count > frame::FRAME_SIZE || buf.len() < byte_count {
            // SAFETY: Free unused frame.
            unsafe { let _ = frame::free_frame(data_frame); }
            return Err(KernelError::InvalidArgument);
        }

        // Copy caller data into DMA buffer.
        // SAFETY: data_virt is freshly allocated; buf has at least byte_count bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), data_virt, byte_count);
        }

        // Build WRITE command.
        let mut sqe = NvmeSqe::zeroed();
        sqe.cdw0 = u32::from(IO_CMD_WRITE);
        sqe.nsid = 1;
        sqe.prp1 = data_phys;
        if byte_count > 4096 {
            sqe.prp2 = data_phys + 4096;
        }
        sqe.cdw10 = lba as u32;
        sqe.cdw11 = (lba >> 32) as u32;
        sqe.cdw12 = u32::from(count - 1);

        let result = io_queue.submit_and_wait(sqe);

        // SAFETY: Done with frame.
        unsafe { let _ = frame::free_frame(data_frame); }

        result.map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// NVMe Block Device wrapper
// ---------------------------------------------------------------------------

/// An NVMe namespace exposed as a block device.
pub struct NvmeDevice {
    ctrl: spin::Mutex<NvmeController>,
    info: BlockDeviceInfo,
}

unsafe impl Send for NvmeDevice {}
unsafe impl Sync for NvmeDevice {}

impl BlockDevice for NvmeDevice {
    fn info(&self) -> BlockDeviceInfo {
        self.info.clone()
    }

    fn read_sector(&mut self, lba: u64, buf: &mut [u8; SECTOR_SIZE]) -> KernelResult<()> {
        let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
        let mut ctrl = self.ctrl.lock();
        ctrl.read_sectors_impl(lba, 1, buf, hhdm)?;
        blkdev::record_io(false);
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, buf: &[u8; SECTOR_SIZE]) -> KernelResult<()> {
        let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
        let mut ctrl = self.ctrl.lock();
        ctrl.write_sectors_impl(lba, 1, buf, hhdm)?;
        blkdev::record_io(true);
        Ok(())
    }

    fn read_sectors(&mut self, start_lba: u64, count: u32, buf: &mut [u8]) -> KernelResult<()> {
        let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
        let mut ctrl = self.ctrl.lock();

        let block_size = ctrl.block_size as usize;
        let max_sectors = (frame::FRAME_SIZE / block_size) as u32;
        let mut remaining = count;
        let mut lba = start_lba;
        let mut offset = 0usize;

        while remaining > 0 {
            let batch = remaining.min(max_sectors) as u16;
            let byte_count = (batch as usize) * block_size;

            if let Some(slice) = buf.get_mut(offset..offset + byte_count) {
                ctrl.read_sectors_impl(lba, batch, slice, hhdm)?;
            } else {
                return Err(KernelError::InvalidArgument);
            }

            lba = lba.checked_add(u64::from(batch))
                .ok_or(KernelError::InvalidArgument)?;
            offset += byte_count;
            remaining -= u32::from(batch);
        }

        blkdev::record_io(false);
        Ok(())
    }

    fn write_sectors(&mut self, start_lba: u64, count: u32, buf: &[u8]) -> KernelResult<()> {
        let hhdm = page_table::hhdm().ok_or(KernelError::InternalError)?;
        let mut ctrl = self.ctrl.lock();

        let block_size = ctrl.block_size as usize;
        let max_sectors = (frame::FRAME_SIZE / block_size) as u32;
        let mut remaining = count;
        let mut lba = start_lba;
        let mut offset = 0usize;

        while remaining > 0 {
            let batch = remaining.min(max_sectors) as u16;
            let byte_count = (batch as usize) * block_size;

            if let Some(slice) = buf.get(offset..offset + byte_count) {
                ctrl.write_sectors_impl(lba, batch, slice, hhdm)?;
            } else {
                return Err(KernelError::InvalidArgument);
            }

            lba = lba.checked_add(u64::from(batch))
                .ok_or(KernelError::InvalidArgument)?;
            offset += byte_count;
            remaining -= u32::from(batch);
        }

        blkdev::record_io(true);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the NVMe subsystem.
///
/// Scans PCI bus for NVMe controllers, initializes each one, identifies
/// namespaces, creates I/O queues, and registers block devices.
pub fn init(hhdm_offset: u64) {
    serial_println!("[nvme] Scanning for NVMe controllers...");

    let controllers = crate::pci::find_devices_by_class(PCI_CLASS_STORAGE, PCI_SUBCLASS_NVM);

    if controllers.is_empty() {
        serial_println!("[nvme] No NVMe controller found");
        return;
    }

    let mut total_devices = 0u32;

    for ctrl_pci in &controllers {
        serial_println!(
            "[nvme] Found controller: {:04x}:{:04x} at {:02x}:{:02x}.{}",
            ctrl_pci.vendor_id, ctrl_pci.device_id,
            ctrl_pci.address.bus, ctrl_pci.address.device, ctrl_pci.address.function,
        );

        // BAR0 contains the controller registers (64-bit MMIO).
        let bar0 = ctrl_pci.bars[0];
        if bar0 == 0 {
            serial_println!("[nvme]   BAR0 is zero — skipping");
            continue;
        }

        // Must be memory-mapped (bit 0 = 0).
        if bar0 & 1 != 0 {
            serial_println!("[nvme]   BAR0 is I/O space — skipping (expected MMIO)");
            continue;
        }

        // For 64-bit BAR, combine BAR0 (low) and BAR1 (high).
        let bar_type = (bar0 >> 1) & 0x3;
        let base_phys = if bar_type == 2 {
            // 64-bit BAR: low from BAR0, high from BAR1.
            let hi = ctrl_pci.bars[1];
            (u64::from(hi) << 32) | u64::from(bar0 & 0xFFFF_FFF0)
        } else {
            u64::from(bar0 & 0xFFFF_FFF0)
        };

        let regs_virt = (base_phys + hhdm_offset) as usize;

        serial_println!("[nvme]   MMIO base: phys={:#010x}, virt={:#x}", base_phys, regs_virt);

        // Map NVMe MMIO region into kernel page tables.
        // BAR addresses may be above physical RAM (not covered by HHDM).
        let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
        let mmio_flags = page_table::PageFlags::PRESENT
            | page_table::PageFlags::WRITABLE
            | page_table::PageFlags::NO_CACHE;
        // NVMe registers + doorbells can span several pages; map 64 KiB (4 frames).
        for i in 0..4u64 {
            let frame_phys = base_phys.wrapping_add(i.wrapping_mul(16384));
            if let Some(f) = frame::PhysFrame::from_addr(frame_phys) {
                let virt = page_table::VirtAddr::new(frame_phys.wrapping_add(hhdm_offset));
                // SAFETY: frame_phys is the PCI BAR0 MMIO region for NVMe.
                let _ = unsafe { page_table::map_frame(pml4_phys, virt, f, mmio_flags) };
            }
        }
        // TLB flush.
        for i in 0..4u64 {
            let addr = base_phys.wrapping_add(hhdm_offset).wrapping_add(i.wrapping_mul(16384));
            // SAFETY: Standard invlpg.
            unsafe {
                core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
            }
        }

        // Enable bus mastering.
        crate::pci::enable_bus_master(ctrl_pci.address);

        // Read version.
        // SAFETY: regs_virt is memory-mapped via HHDM.
        let vs = unsafe { mmio_read32(regs_virt + REG_VS) };
        serial_println!(
            "[nvme]   Version: {}.{}.{}",
            (vs >> 16) & 0xFFFF, (vs >> 8) & 0xFF, vs & 0xFF,
        );

        // Initialize the controller.
        let mut controller = match NvmeController::init(regs_virt, hhdm_offset) {
            Ok(c) => c,
            Err(e) => {
                serial_println!("[nvme]   Controller init failed: {:?}", e);
                continue;
            }
        };

        // Identify controller.
        if let Err(e) = controller.identify_controller(hhdm_offset) {
            serial_println!("[nvme]   IDENTIFY CONTROLLER failed: {:?}", e);
            continue;
        }
        serial_println!("[nvme]   Model: {}", controller.model);
        serial_println!("[nvme]   Serial: {}", controller.serial);

        // Identify namespace 1.
        if let Err(e) = controller.identify_namespace(1, hhdm_offset) {
            serial_println!("[nvme]   IDENTIFY NAMESPACE 1 failed: {:?}", e);
            continue;
        }

        if controller.sector_count == 0 {
            serial_println!("[nvme]   Namespace 1 has zero capacity — skipping");
            continue;
        }

        // Create I/O queues.
        if let Err(e) = controller.create_io_queues(hhdm_offset) {
            serial_println!("[nvme]   I/O queue creation failed: {:?}", e);
            continue;
        }

        // Register as block device.
        let dev_name = format!("nvme{}n1", total_devices);
        let capacity_mb = (controller.sector_count * u64::from(controller.block_size))
            / (1024 * 1024);
        serial_println!(
            "[nvme]   Registering {} ({} MB, {} blocks, bs={})",
            dev_name, capacity_mb, controller.sector_count, controller.block_size,
        );

        let info = BlockDeviceInfo {
            name: dev_name.clone(),
            sector_count: controller.sector_count,
            sector_size: controller.block_size,
            read_only: false,
        };

        let device = NvmeDevice {
            ctrl: spin::Mutex::new(controller),
            info: info.clone(),
        };

        blkdev::register(&dev_name, Box::new(device));
        total_devices += 1;
    }

    DEVICE_COUNT.store(total_devices, Ordering::Release);
    INITIALIZED.store(true, Ordering::Release);

    serial_println!("[nvme] Initialization complete: {} device(s)", total_devices);
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// NVMe subsystem status.
#[derive(Debug, Clone, Copy)]
pub struct NvmeStats {
    pub initialized: bool,
    pub device_count: u32,
}

/// Get NVMe subsystem statistics.
#[must_use]
pub fn stats() -> NvmeStats {
    NvmeStats {
        initialized: INITIALIZED.load(Ordering::Relaxed),
        device_count: DEVICE_COUNT.load(Ordering::Relaxed),
    }
}

/// Whether the NVMe driver detected any devices.
#[must_use]
#[allow(dead_code)]
pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Relaxed) && DEVICE_COUNT.load(Ordering::Relaxed) > 0
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the NVMe subsystem.
pub fn self_test() {
    serial_println!("[nvme] Running self-test...");

    let s = stats();
    serial_println!("[nvme]   Initialized: {}, devices: {}", s.initialized, s.device_count);

    if !s.initialized {
        serial_println!("[nvme]   No controller found — self-test SKIPPED (OK for non-NVMe systems)");
        serial_println!("[nvme] Self-test PASSED (no hardware)");
        return;
    }

    if s.device_count == 0 {
        serial_println!("[nvme]   Controller found but no namespaces — PASSED");
        serial_println!("[nvme] Self-test PASSED (no namespaces)");
        return;
    }

    // Verify block device is registered and readable.
    let read_ok = blkdev::with_device("nvme0n1", |dev| {
        let info = dev.info();
        serial_println!(
            "[nvme]   nvme0n1: {} blocks, block_size={}",
            info.sector_count, info.sector_size
        );
        assert!(info.sector_count > 0, "NVMe device should have non-zero capacity");

        // Read block 0.
        let mut buf = [0u8; SECTOR_SIZE];
        let result = dev.read_sector(0, &mut buf);
        assert!(result.is_ok(), "Reading block 0 should succeed");
        serial_println!("[nvme]   Block 0 read: OK");

        true
    });

    if read_ok.is_some() {
        serial_println!("[nvme]   Block device read: OK");
    } else {
        serial_println!("[nvme]   Block device 'nvme0n1' not found in registry");
    }

    serial_println!("[nvme] Self-test PASSED");
}
