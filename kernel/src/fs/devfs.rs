//! Device pseudo-filesystem (`/dev`).
//!
//! Provides virtual device files that are essential for standard programs
//! and shell usage.  All content is generated/consumed dynamically.
//!
//! ## Layout
//!
//! ```text
//! /dev/
//! ├── null       Discards all writes, reads return EOF (empty)
//! ├── zero       Reads return zero bytes, writes succeed
//! ├── full       Reads return zero bytes, writes fail with DiskFull
//! ├── random     Reads return pseudo-random bytes (xorshift64)
//! ├── urandom    Same as random (no entropy blocking distinction)
//! ├── console    Reads/writes to the kernel console
//! └── tty        Controlling terminal (aliases console, single-console)
//! ```
//!
//! ## Design
//!
//! This is a minimal devfs for kernel-mode use.  In our microkernel
//! architecture, hardware devices are managed by userspace drivers via
//! IPC — the devfs does NOT expose block devices or hardware directly.
//! It provides the standard "utility" device files that programs expect.
//!
//! The PRNG for `/dev/random` uses xorshift64 seeded from the HPET
//! counter.  This is NOT cryptographically secure — it's adequate for
//! test data, shuffling, and non-security randomness.  A real CSPRNG
//! (seeded from hardware RNG / RDRAND) should replace it in the future.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::vfs::{DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo};

use spin::Mutex;

// ---------------------------------------------------------------------------
// PRNG state
// ---------------------------------------------------------------------------

/// xorshift64 PRNG state, seeded from HPET at first use.
static PRNG_STATE: Mutex<u64> = Mutex::new(0);

/// Seed the PRNG if not yet initialized.
fn prng_seed_if_needed() {
    let mut state = PRNG_STATE.lock();
    if *state == 0 {
        // Seed from HPET + a constant to avoid zero state.
        *state = crate::hpet::elapsed_ns() | 1;
    }
}

/// Generate a pseudo-random u64 using xorshift64.
fn prng_next() -> u64 {
    prng_seed_if_needed();
    let mut state = PRNG_STATE.lock();
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Fill a buffer with pseudo-random bytes.
fn fill_random(buf: &mut [u8]) {
    let mut pos = 0;
    while pos < buf.len() {
        let val = prng_next();
        let bytes = val.to_le_bytes();
        let remaining = buf.len().saturating_sub(pos);
        let copy_len = remaining.min(8);
        if let Some(dest) = buf.get_mut(pos..pos.wrapping_add(copy_len)) {
            if let Some(src) = bytes.get(..copy_len) {
                dest.copy_from_slice(src);
            }
        }
        pos = pos.wrapping_add(copy_len);
    }
}

// ---------------------------------------------------------------------------
// DevFs implementation
// ---------------------------------------------------------------------------

/// Virtual filesystem exposing standard device files.
pub struct DevFs;

impl DevFs {
    /// Create a new DevFs instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

/// Device file names.
const DEV_FILES: &[&str] = &[
    "null",
    "zero",
    "full",
    "random",
    "urandom",
    "console",
    "tty",
    "stdin",
    "stdout",
    "stderr",
    "kmsg",
    "uptime",
];

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

impl FileSystem for DevFs {
    fn fs_type(&self) -> &str {
        "devfs"
    }

    fn readdir(&mut self, path: &str) -> KernelResult<Vec<DirEntry>> {
        let rel = path.strip_prefix('/').unwrap_or(path);

        if rel.is_empty() {
            let entries = DEV_FILES
                .iter()
                .map(|name| DirEntry {
                    name: String::from(*name),
                    entry_type: EntryType::File,
                    size: 0, // Special files have no meaningful static size.
                })
                .collect();
            Ok(entries)
        } else {
            Err(KernelError::NotADirectory)
        }
    }

    fn read_file(&mut self, path: &str) -> KernelResult<Vec<u8>> {
        let rel = path.strip_prefix('/').unwrap_or(path);

        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => {
                // /dev/null: read returns empty (EOF).
                Ok(Vec::new())
            }
            "zero" | "full" => {
                // /dev/zero and /dev/full: return a page of zero bytes.
                // Real /dev/zero is infinite; we return a bounded chunk.
                Ok(vec![0u8; 4096])
            }
            "random" | "urandom" => {
                // /dev/random, /dev/urandom: return pseudo-random bytes.
                // No distinction between the two — our PRNG never blocks.
                let mut buf = vec![0u8; 256];
                fill_random(&mut buf);
                Ok(buf)
            }
            "console" | "tty" => {
                // /dev/console, /dev/tty: reading returns whatever is in the
                // keyboard buffer; for now, return empty (non-blocking).
                // /dev/tty is the controlling terminal of the calling process;
                // since we're single-console, it aliases /dev/console.
                Ok(Vec::new())
            }
            _ => Err(KernelError::NotFound),
        }
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let rel = path.strip_prefix('/').unwrap_or(path);

        // For streaming devices, offset is ignored — they always produce
        // fresh data.  This is important for file-handle reads that advance
        // a cursor: reading /dev/zero at offset 8192 should still produce
        // zeros, not EOF.
        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => Ok(Vec::new()),
            "zero" | "full" => {
                Ok(vec![0u8; len.min(65536)])
            }
            "random" | "urandom" => {
                let actual = len.min(65536);
                let mut buf = vec![0u8; actual];
                fill_random(&mut buf);
                Ok(buf)
            }
            "console" | "tty" | "stdin" => {
                // Reading from console/tty/stdin returns empty (no interactive input).
                let _ = offset;
                Ok(Vec::new())
            }
            "stdout" | "stderr" => {
                // Reading from stdout/stderr returns empty.
                let _ = offset;
                Ok(Vec::new())
            }
            "kmsg" => {
                // /dev/kmsg: kernel log ring buffer (JSON-lines format).
                // Reads all entries from the klog ring buffer.
                let mut buf = alloc::vec![0u8; 64 * 1024];
                let (written, _last_seq) =
                    crate::klog::read_logs(u64::MAX, &mut buf);
                buf.truncate(written);
                Ok(buf)
            }
            "uptime" => {
                // /dev/uptime: system uptime as a simple decimal string.
                let ns = crate::hpet::elapsed_ns();
                let secs = ns / 1_000_000_000;
                let frac = ns % 1_000_000_000;
                let text = alloc::format!("{secs}.{frac:09}\n");
                Ok(text.into_bytes())
            }
            _ => Err(KernelError::NotFound),
        }
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> KernelResult<()> {
        let rel = path.strip_prefix('/').unwrap_or(path);

        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => {
                // /dev/null: discard all data silently.
                let _ = data;
                Ok(())
            }
            "zero" => {
                // /dev/zero: writes succeed but data is discarded.
                let _ = data;
                Ok(())
            }
            "full" => {
                // /dev/full: writes always fail with DiskFull.
                // Useful for testing error handling in programs.
                let _ = data;
                Err(KernelError::DiskFull)
            }
            "random" | "urandom" => {
                // /dev/random: writes contribute to entropy pool.
                // For our simple PRNG, XOR the data into the state.
                if !data.is_empty() {
                    let mut hash: u64 = 0;
                    for chunk in data.chunks(8) {
                        let mut buf = [0u8; 8];
                        let len = chunk.len().min(8);
                        if let Some(dest) = buf.get_mut(..len) {
                            if let Some(src) = chunk.get(..len) {
                                dest.copy_from_slice(src);
                            }
                        }
                        hash ^= u64::from_le_bytes(buf);
                    }
                    prng_seed_if_needed();
                    let mut state = PRNG_STATE.lock();
                    *state ^= hash;
                    // Ensure state never becomes zero (xorshift degenerate).
                    if *state == 0 {
                        *state = 1;
                    }
                }
                Ok(())
            }
            "console" | "tty" | "stdout" | "stderr" => {
                // /dev/console, /dev/tty, stdout, stderr: write to kernel console output.
                if let Ok(text) = core::str::from_utf8(data) {
                    crate::console_print!("{}", text);
                } else {
                    // Binary data — print hex summary.
                    crate::console_print!("[binary: {} bytes]", data.len());
                }
                Ok(())
            }
            "stdin" => {
                // Writing to stdin is a no-op (no input buffer to push into).
                let _ = data;
                Ok(())
            }
            "kmsg" => {
                // Writing to kmsg logs a message (print to serial for now).
                if let Ok(text) = core::str::from_utf8(data) {
                    crate::serial_println!("[kmsg] {}", text.trim_end());
                }
                Ok(())
            }
            "uptime" => {
                // /dev/uptime is read-only.
                Err(KernelError::NotSupported)
            }
            _ => Err(KernelError::NotFound),
        }
    }

    fn write_at(&mut self, path: &str, _offset: u64, data: &[u8]) -> KernelResult<()> {
        // For device files, write_at behaves the same as write_file —
        // offset is meaningless for streaming devices.
        self.write_file(path, data)
    }

    fn stat(&mut self, path: &str) -> KernelResult<DirEntry> {
        let rel = path.strip_prefix('/').unwrap_or(path);

        if rel.is_empty() {
            return Ok(DirEntry {
                name: String::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        if DEV_FILES.contains(&rel) {
            Ok(DirEntry {
                name: String::from(rel),
                entry_type: EntryType::File,
                size: 0,
            })
        } else {
            Err(KernelError::NotFound)
        }
    }

    fn metadata(&mut self, path: &str) -> KernelResult<FileMeta> {
        let rel = path.strip_prefix('/').unwrap_or(path);

        if rel.is_empty() {
            return Ok(FileMeta {
                size: 0,
                entry_type: EntryType::Directory,
                permissions: 0o755,
                nlinks: 1,
                ..FileMeta::minimal(EntryType::Directory, 0)
            });
        }

        if DEV_FILES.contains(&rel) {
            // Device files have special permissions:
            // - null/zero/full/urandom: world read+write (0o666)
            // - random: world read+write (0o666)
            // - console: owner read+write (0o600)
            let perms = match rel {
                "console" => 0o600,
                _ => 0o666,
            };
            Ok(FileMeta {
                size: 0,
                entry_type: EntryType::File,
                permissions: perms,
                attributes: FileAttr::NONE,
                nlinks: 1,
                ..FileMeta::minimal(EntryType::File, 0)
            })
        } else {
            Err(KernelError::NotFound)
        }
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("devfs"),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: DEV_FILES.len() as u64,
            free_inodes: 0,
            max_name_len: 255,
            read_only: false,
        })
    }

    fn debug_stats(&self) -> String {
        format!("devfs: {} device files", DEV_FILES.len())
    }
}

// ---------------------------------------------------------------------------
// Mount helper
// ---------------------------------------------------------------------------

/// Mount devfs at the given path (typically `/dev`).
pub fn mount(mount_path: &str) -> KernelResult<()> {
    let fs = DevFs::new();
    crate::fs::Vfs::mount(mount_path, alloc::boxed::Box::new(fs))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Test the devfs implementation.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    serial_println!("[devfs] Running self-test...");

    let mut fs = DevFs::new();

    // Test root readdir.
    let entries = fs.readdir("/")?;
    if entries.len() != DEV_FILES.len() {
        serial_println!(
            "[devfs]   FAIL: readdir returned {} entries, expected {}",
            entries.len(),
            DEV_FILES.len()
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   readdir /: {} entries OK", entries.len());

    // Test stat on root.
    let root_stat = fs.stat("/")?;
    if root_stat.entry_type != EntryType::Directory {
        serial_println!("[devfs]   FAIL: stat / not a directory");
        return Err(KernelError::InternalError);
    }

    // Test /dev/null: read returns empty.
    let null_data = fs.read_file("/null")?;
    if !null_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/null read should be empty");
        return Err(KernelError::InternalError);
    }
    // Write to null should succeed.
    fs.write_file("/null", b"discarded")?;
    serial_println!("[devfs]   null: read=empty, write=discard OK");

    // Test /dev/zero: read returns zeros.
    let zero_data = fs.read_file("/zero")?;
    if zero_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/zero read should not be empty");
        return Err(KernelError::InternalError);
    }
    if zero_data.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/zero data contains non-zero bytes");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   zero: {} zero bytes OK", zero_data.len());

    // Test /dev/full: read returns zeros, write fails with DiskFull.
    let full_data = fs.read_file("/full")?;
    if full_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/full read should not be empty");
        return Err(KernelError::InternalError);
    }
    if full_data.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/full data contains non-zero bytes");
        return Err(KernelError::InternalError);
    }
    match fs.write_file("/full", b"should fail") {
        Err(KernelError::DiskFull) => {}
        other => {
            serial_println!("[devfs]   FAIL: /dev/full write should return DiskFull, got {:?}", other);
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[devfs]   full: read=zeros, write=DiskFull OK");

    // Test /dev/random: read returns data, two reads differ.
    let rand1 = fs.read_file("/random")?;
    let rand2 = fs.read_file("/random")?;
    if rand1.is_empty() || rand2.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/random read should not be empty");
        return Err(KernelError::InternalError);
    }
    if rand1 == rand2 {
        serial_println!("[devfs]   FAIL: two /dev/random reads should differ");
        return Err(KernelError::InternalError);
    }
    // Write to random (entropy contribution) should succeed.
    fs.write_file("/random", b"entropy seed")?;
    serial_println!("[devfs]   random: {} random bytes, entropy write OK", rand1.len());

    // Test /dev/urandom: same behavior as /dev/random.
    let urand = fs.read_file("/urandom")?;
    if urand.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/urandom read should not be empty");
        return Err(KernelError::InternalError);
    }
    fs.write_file("/urandom", b"more entropy")?;
    serial_println!("[devfs]   urandom: {} random bytes OK", urand.len());

    // Test read_at on /dev/zero — should always return zeros regardless of offset.
    let zero_at = fs.read_at("/zero", 99999, 64)?;
    if zero_at.len() != 64 || zero_at.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/zero read_at should return 64 zero bytes");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   read_at /dev/zero: offset-independent OK");

    // Test /dev/console: write should succeed (outputs to console).
    fs.write_file("/console", b"[devfs]   console write test\n")?;
    serial_println!("[devfs]   console: write OK");

    // Test nonexistent device.
    if fs.stat("/nonexistent").is_ok() {
        serial_println!("[devfs]   FAIL: stat /nonexistent should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   stat /nonexistent: NotFound OK");

    serial_println!("[devfs] Self-test PASSED");
    Ok(())
}
