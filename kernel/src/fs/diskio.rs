//! Disk I/O -- per-device I/O statistics, as the block layer counts them.
//!
//! What `/proc/diskio`, `/proc/diskstat` and the `diskio` and `diskstat`
//! shell commands show: for each registered block device, its reads and
//! writes, the bytes they moved, their total and longest times, how many
//! failed, and when the first and last request began.
//!
//! ## A projection, not a store
//!
//! Every number here is computed when it is asked for, from the counts the
//! block registry keeps as requests pass through it
//! ([`crate::blkdev::device_stats`], counted by `blkdev`'s `Accounted`
//! wrapper on every `with_device` / `try_with_device` request). The module
//! keeps nothing of its own -- design-decisions §641's shape (`fs::perfmon`,
//! `fs::pagecache`): where a subsystem already holds the numbers, join them
//! at read time rather than keep a second copy that can disagree.
//!
//! Until 2026-10-08 it was a table of its own, fed by `record_read` /
//! `record_write` / `record_*_error` functions nothing in the I/O path ever
//! called (known-issues `A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`), so
//! it showed no devices and no I/O on every boot -- a zero nothing could
//! raise, which reads exactly like a measured one. `fs::diskstat`, a second
//! such table behind `/proc/diskstat`, went at the same time; both views are
//! this module's now. The fields no source can fill went with them, as
//! §641 has it, rather than staying as zeros: queue depth (the registry runs
//! one request at a time, so none is ever waiting when this is read), merges
//! and flushes (nothing here merges requests or flushes a device's cache).
//!
//! Bytes are the block layer's 512-byte sectors times 512, whatever a
//! device's own sector size, as Linux's `/proc/diskstats` counts them.

use alloc::string::String;
use alloc::vec::Vec;

use crate::blkdev::{self, DeviceStats};

/// The size of a sector in the block layer's counts.
const SECTOR_BYTES: u64 = 512;

/// One device's I/O since it was registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIoStats {
    /// The name it is registered under (`vda`, `nvme0n1`, ...).
    pub device_name: String,
    /// Read requests completed.
    pub reads: u64,
    /// Write requests completed.
    pub writes: u64,
    /// Bytes the reads moved.
    pub bytes_read: u64,
    /// Bytes the writes moved.
    pub bytes_written: u64,
    /// Discard requests completed.
    pub discards: u64,
    /// Bytes the discards covered.
    pub bytes_discarded: u64,
    /// Nanoseconds the reads took, together.
    pub read_latency_total_ns: u64,
    /// Nanoseconds the writes took, together.
    pub write_latency_total_ns: u64,
    /// The longest read, in nanoseconds.
    pub read_latency_max_ns: u64,
    /// The longest write, in nanoseconds.
    pub write_latency_max_ns: u64,
    /// Reads the driver failed.
    pub read_errors: u64,
    /// Writes the driver failed.
    pub write_errors: u64,
    /// When the first request began (`hrtimer::now_ns`), 0 if none has.
    pub first_io_ns: u64,
    /// When the last request began, 0 if none has.
    pub last_io_ns: u64,
}

impl DeviceIoStats {
    /// The view of one registered device's block-layer counts.
    fn project(name: String, st: &DeviceStats) -> Self {
        Self {
            device_name: name,
            reads: st.reads,
            writes: st.writes,
            bytes_read: st.read_sectors.saturating_mul(SECTOR_BYTES),
            bytes_written: st.write_sectors.saturating_mul(SECTOR_BYTES),
            discards: st.discards,
            bytes_discarded: st.discard_sectors.saturating_mul(SECTOR_BYTES),
            read_latency_total_ns: st.read_ns,
            write_latency_total_ns: st.write_ns,
            read_latency_max_ns: st.read_max_ns,
            write_latency_max_ns: st.write_max_ns,
            read_errors: st.read_errors,
            write_errors: st.write_errors,
            first_io_ns: st.first_io_ns,
            last_io_ns: st.last_io_ns,
        }
    }

    /// Average read time in nanoseconds; 0 with no reads.
    #[must_use]
    pub fn avg_read_latency_ns(&self) -> u64 {
        self.read_latency_total_ns
            .checked_div(self.reads)
            .unwrap_or(0)
    }

    /// Average write time in nanoseconds; 0 with no writes.
    #[must_use]
    pub fn avg_write_latency_ns(&self) -> u64 {
        self.write_latency_total_ns
            .checked_div(self.writes)
            .unwrap_or(0)
    }

    /// Reads and writes together.
    #[must_use]
    pub fn total_ops(&self) -> u64 {
        self.reads.saturating_add(self.writes)
    }

    /// Bytes read and written together.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.bytes_read.saturating_add(self.bytes_written)
    }
}

/// Every registered device's I/O, in registration order.
#[must_use]
pub fn all_devices() -> Vec<DeviceIoStats> {
    blkdev::device_stats()
        .into_iter()
        .map(|(info, st)| DeviceIoStats::project(info.name, &st))
        .collect()
}

/// One device's I/O, by the name it is registered under.
#[must_use]
pub fn device_stats(name: &str) -> Option<DeviceIoStats> {
    all_devices().into_iter().find(|d| d.device_name == name)
}

/// Totals over every registered device: `(devices, reads, writes,
/// bytes read, bytes written)`.
#[must_use]
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let devices = all_devices();
    let mut totals = (devices.len(), 0u64, 0u64, 0u64, 0u64);
    for d in &devices {
        totals.1 = totals.1.saturating_add(d.reads);
        totals.2 = totals.2.saturating_add(d.writes);
        totals.3 = totals.3.saturating_add(d.bytes_read);
        totals.4 = totals.4.saturating_add(d.bytes_written);
    }
    totals
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The projection against a scratch RAM disk: a device nothing has used
/// shows nothing; a read, a write, a failed read and a discard show as
/// exactly those, in bytes, with times that cover them; the totals include
/// them; and an unregistered device is gone from the view.
///
/// # Errors
///
/// `InternalError` on the first check that does not hold.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::blkdev::{RamBlockDevice, SECTOR_SIZE};
    use crate::error::KernelError;
    use crate::serial_println;

    const NAME: &str = "zzdiskio0";

    fn fail(what: &str) -> crate::error::KernelResult<()> {
        serial_println!("[diskio]   FAIL: {}", what);
        blkdev::unregister(NAME);
        Err(KernelError::InternalError)
    }

    serial_println!("[diskio] Running self-test...");
    blkdev::register(NAME, alloc::boxed::Box::new(RamBlockDevice::new(32)));

    let Some(fresh) = device_stats(NAME) else {
        return fail("a registered device is not in the view");
    };
    if fresh.total_ops() != 0 || fresh.total_bytes() != 0 || fresh.first_io_ns != 0 {
        return fail("a device nothing has used shows I/O");
    }
    let (devices_before, reads_before, writes_before, _, _) = stats();

    // Three sectors read, one written, one read past the end (refused by
    // the driver: a failed read, still a request), eight discarded.
    let mut three = [0u8; 3 * SECTOR_SIZE];
    let one = [0xA5u8; SECTOR_SIZE];
    let mut past = [0u8; SECTOR_SIZE];
    let io = blkdev::with_device(NAME, |dev| {
        dev.read_sectors(0, 3, &mut three)?;
        dev.write_sector(4, &one)?;
        let refused = dev.read_sector(32, &mut past).is_err();
        dev.discard(8, 8)?;
        Ok::<bool, KernelError>(refused)
    });
    if io != Some(Ok(true)) {
        serial_println!("[diskio]   (scratch disk I/O gave {:?})", io);
        return fail("the scratch disk's I/O did not go as planned");
    }

    let Some(d) = device_stats(NAME) else {
        return fail("the device left the view while still registered");
    };
    if (d.reads, d.bytes_read, d.read_errors) != (2, 4 * 512, 1) {
        return fail("two reads of four sectors, one failed, were not what the view shows");
    }
    if (d.writes, d.bytes_written, d.write_errors) != (1, 512, 0) {
        return fail("one write of one sector was not what the view shows");
    }
    if (d.discards, d.bytes_discarded) != (1, 8 * 512) {
        return fail("one discard of eight sectors was not what the view shows");
    }
    if d.read_latency_max_ns > d.read_latency_total_ns
        || d.write_latency_max_ns > d.write_latency_total_ns
        || d.first_io_ns == 0
        || d.last_io_ns < d.first_io_ns
    {
        return fail("the times do not cover the requests");
    }
    let (devices, reads, writes, _, _) = stats();
    if devices != devices_before
        || reads < reads_before.saturating_add(2)
        || writes < writes_before.saturating_add(1)
    {
        return fail("the totals do not include the scratch disk's requests");
    }

    blkdev::unregister(NAME);
    if device_stats(NAME).is_some() {
        return fail("an unregistered device is still in the view");
    }
    serial_println!(
        "[diskio]   the block layer's counts, projected: reads, writes, a failed read, a \
         discard, their bytes and times: OK"
    );
    Ok(())
}
