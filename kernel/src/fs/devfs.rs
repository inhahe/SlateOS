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
//! ├── random     Reads return CSPRNG bytes (ChaCha20, kernel `rng`)
//! ├── urandom    Same as random (no entropy blocking distinction)
//! ├── console    Reads/writes to the kernel console
//! ├── tty        Controlling terminal (aliases console, single-console)
//! ├── stdin/stdout/stderr, kmsg, uptime
//! ├── input/     event0 (keyboard), event1 (mouse)
//! ├── dri/       card0, renderD128
//! └── snd/       controlC0, pcmC0D0p, pcmC0D0c
//! ```
//!
//! ## Design
//!
//! This is a minimal devfs for kernel-mode use.  In our microkernel
//! architecture, hardware devices are managed by userspace drivers via
//! IPC — the devfs does NOT expose block devices or hardware directly.
//! It provides the standard "utility" device files that programs expect.
//!
//! ## The subdirectories are a namespace, not an implementation
//!
//! The nodes under `input/`, `dri/` and `snd/` are **not served by this
//! filesystem**.  `open` of one is intercepted in the syscall layer
//! (`syscall/linux.rs`), which mints a device handle rather than a VFS file;
//! reading one through the VFS gets `NotSupported`.  They are listed here
//! because existing-but-invisible is not a state a real client can cope with:
//! libinput *scans* `/dev/input/` to discover devices and checks `S_ISCHR` on
//! what it finds, libdrm and ALSA do the same for their directories, and until
//! this table existed all three would have concluded the machine had no
//! hardware while the nodes sat there working for anyone who knew the exact
//! path.  Keeping the namespace here — rather than teaching each client our
//! paths — is what makes those libraries usable unmodified.
//!
//! `/dev/random` and `/dev/urandom` both delegate to the kernel
//! CSPRNG (`crate::rng::fill`, ChaCha20-based, seeded at boot from
//! RDSEED/RDRAND/HPET/TSC).  Output is cryptographically secure; the
//! random/urandom split exists purely for API compatibility — we
//! never block waiting for entropy.

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{DirEntry, EntryType, FileAttr, FileMeta, FileSystem, FsInfo};

// ---------------------------------------------------------------------------
// Random bytes — delegates to kernel CSPRNG (rng module)
// ---------------------------------------------------------------------------

/// Fill a buffer with cryptographically-secure random bytes.
///
/// Delegates to the kernel CSPRNG (ChaCha20-based, seeded from hardware
/// entropy sources).  This replaces the previous weak xorshift64 PRNG.
fn fill_random(buf: &mut [u8]) {
    crate::rng::fill(buf);
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

/// One node in the devfs namespace.
///
/// `path` is relative to the devfs mount point and may contain a `/`, because
/// devfs has subdirectories now. They are spelled out flat here rather than
/// built as a tree: the namespace is a fixed handful of entries the kernel
/// knows at compile time, so a tree would be a data structure with no
/// variation to justify it. A devfs that grew nodes at run time would want the
/// tree; ours does not.
struct DevNode {
    /// Path relative to the devfs root — `"null"`, `"input/event0"`.
    path: &'static str,
    /// What `stat` reports. [`EntryType::CharDevice`] for real device nodes.
    entry_type: EntryType,
    /// Unix permission bits.
    mode: u16,
}

impl DevNode {
    /// A utility file this filesystem serves itself.
    const fn file(path: &'static str, mode: u16) -> Self {
        Self {
            path,
            entry_type: EntryType::File,
            mode,
        }
    }

    /// A directory. Mode 0o755, as on Linux.
    const fn dir(path: &'static str) -> Self {
        Self {
            path,
            entry_type: EntryType::Directory,
            mode: 0o755,
        }
    }

    /// A character device node, served by the syscall layer, not by devfs.
    ///
    /// Mode 0o660 for all of them: on Linux each is `crw-rw----` owned by a
    /// per-class group (`input`, `video`, `audio`). We have no groups yet, so
    /// the bits are the honest part and the ownership is not.
    const fn chr(path: &'static str) -> Self {
        Self {
            path,
            entry_type: EntryType::CharDevice,
            mode: 0o660,
        }
    }

    /// The final component of [`path`](Self::path) — what `readdir` reports.
    fn name(&self) -> &'static str {
        match self.path.rsplit_once('/') {
            Some((_, base)) => base,
            None => self.path,
        }
    }

    /// The directory this node lives in, `""` for the root.
    fn parent(&self) -> &'static str {
        match self.path.rsplit_once('/') {
            Some((dir, _)) => dir,
            None => "",
        }
    }
}

/// The whole devfs namespace.
///
/// The character devices in the subdirectories are **not readable through this
/// filesystem** — `open` of one is intercepted in the syscall layer
/// (`syscall/linux.rs`) and mints a device handle instead of a VFS file. They
/// are listed here anyway, and that is the point of this table: a client that
/// *scans* `/dev/input/` to find input devices — which is exactly what libinput
/// does — has to be able to see them, and `stat` has to report `S_IFCHR` or
/// libinput will reject a device that works. Before this table those nodes were
/// openable by exact path and invisible to everything else, which is the kind
/// of half-existence that makes a port fail for no discoverable reason.
const DEV_NODES: &[DevNode] = &[
    // Utility files, served by this filesystem's own read/write.
    DevNode::file("null", 0o666),
    DevNode::file("zero", 0o666),
    DevNode::file("full", 0o666),
    DevNode::file("random", 0o666),
    DevNode::file("urandom", 0o666),
    DevNode::file("console", 0o600),
    DevNode::file("tty", 0o666),
    DevNode::file("stdin", 0o666),
    DevNode::file("stdout", 0o666),
    DevNode::file("stderr", 0o666),
    DevNode::file("kmsg", 0o666),
    // Read-only: `write_file` refuses it with NotSupported, so 0o444 is what
    // the mode bits should have said all along.
    DevNode::file("uptime", 0o444),
    // Device-node directories.
    DevNode::dir("input"),
    DevNode::dir("dri"),
    DevNode::dir("snd"),
    // Input devices — `evdev_ioctl` / `evdev_read` in the syscall layer.
    DevNode::chr("input/event0"),
    DevNode::chr("input/event1"),
    // DRM card and render node.
    DevNode::chr("dri/card0"),
    DevNode::chr("dri/renderD128"),
    // ALSA control and PCM substreams.
    DevNode::chr("snd/controlC0"),
    DevNode::chr("snd/pcmC0D0p"),
    DevNode::chr("snd/pcmC0D0c"),
];

/// Look up a node by its devfs-relative path.
fn find_node(rel: &str) -> Option<&'static DevNode> {
    DEV_NODES.iter().find(|n| n.path == rel)
}

/// The entries directly inside `dir` (`""` for the devfs root).
fn children_of(dir: &str) -> Vec<DirEntry> {
    DEV_NODES
        .iter()
        .filter(|n| n.parent() == dir)
        .map(|n| DirEntry {
            name: PathBuf::from(n.name()),
            entry_type: n.entry_type,
            // Special files have no meaningful static size.
            size: 0,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// FileSystem trait implementation
// ---------------------------------------------------------------------------

/// Strip the leading "/" to get the path relative to the devfs root, and
/// decode.
///
/// This is where devfs stops caring about bytes. Paths are byte strings in
/// general (see [`super::path`]), but devfs's namespace is built entirely by
/// the kernel out of fixed ASCII names, so a path that is not UTF-8 cannot
/// name anything that exists and `NotFound` is the honest answer. A lossy
/// conversion would be strictly worse: it would let a garbage byte alias a
/// real entry. This is the only decode in the module.
fn strip_root(path: &Path) -> KernelResult<&str> {
    let s = path.to_str().ok_or(KernelError::NotFound)?;
    Ok(s.strip_prefix('/').unwrap_or(s))
}

/// The answer for a path that this filesystem does not itself serve.
///
/// A character device node exists — `stat` says so, and `readdir` lists it —
/// but its contents do not come from here: `open` of one is intercepted in the
/// syscall layer, which mints a device handle. Reaching it through the VFS's
/// own read/write is therefore not "no such file" but "not that way", and
/// saying `NotFound` about a path `stat` just confirmed would send whoever hits
/// it looking for a missing entry that is right there.
fn unserved(rel: &str) -> KernelError {
    match find_node(rel) {
        Some(n) if n.entry_type == EntryType::Directory => KernelError::IsADirectory,
        Some(_) => KernelError::NotSupported,
        None => KernelError::NotFound,
    }
}

impl FileSystem for DevFs {
    fn fs_type(&self) -> &'static str {
        "devfs"
    }

    fn readdir(&mut self, path: &Path) -> KernelResult<Vec<DirEntry>> {
        let rel = strip_root(path)?;

        if rel.is_empty() {
            return Ok(children_of(""));
        }
        match find_node(rel) {
            Some(node) if node.entry_type == EntryType::Directory => Ok(children_of(rel)),
            Some(_) => Err(KernelError::NotADirectory),
            None => Err(KernelError::NotFound),
        }
    }

    fn read_file(&mut self, path: &Path) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path)?;

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
            _ => Err(unserved(rel)),
        }
    }

    fn read_at(&mut self, path: &Path, offset: u64, len: usize) -> KernelResult<Vec<u8>> {
        let rel = strip_root(path)?;

        // For streaming devices, offset is ignored — they always produce
        // fresh data.  This is important for file-handle reads that advance
        // a cursor: reading /dev/zero at offset 8192 should still produce
        // zeros, not EOF.
        match rel {
            "" => Err(KernelError::IsADirectory),
            "null" => Ok(Vec::new()),
            "zero" | "full" => Ok(vec![0u8; len.min(65536)]),
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
                let (written, _last_seq) = crate::klog::read_logs(u64::MAX, &mut buf);
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
            _ => Err(unserved(rel)),
        }
    }

    fn write_file(&mut self, path: &Path, data: &[u8]) -> KernelResult<()> {
        let rel = strip_root(path)?;

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
                // Mix user-supplied data into the kernel CSPRNG.
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
                    crate::rng::add_interrupt_entropy(hash);
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
            _ => Err(unserved(rel)),
        }
    }

    fn write_at(&mut self, path: &Path, _offset: u64, data: &[u8]) -> KernelResult<()> {
        // For device files, write_at behaves the same as write_file —
        // offset is meaningless for streaming devices.
        self.write_file(path, data)
    }

    fn stat(&mut self, path: &Path) -> KernelResult<DirEntry> {
        let rel = strip_root(path)?;

        if rel.is_empty() {
            return Ok(DirEntry {
                name: PathBuf::from("/"),
                entry_type: EntryType::Directory,
                size: 0,
            });
        }

        let node = find_node(rel).ok_or(KernelError::NotFound)?;
        Ok(DirEntry {
            name: PathBuf::from(node.name()),
            entry_type: node.entry_type,
            size: 0,
        })
    }

    fn metadata(&mut self, path: &Path) -> KernelResult<FileMeta> {
        let rel = strip_root(path)?;

        if rel.is_empty() {
            return Ok(FileMeta {
                size: 0,
                entry_type: EntryType::Directory,
                permissions: 0o755,
                nlinks: 1,
                blocks: 0,
                ..FileMeta::minimal(EntryType::Directory, 0)
            });
        }

        let node = find_node(rel).ok_or(KernelError::NotFound)?;
        Ok(FileMeta {
            size: 0,
            entry_type: node.entry_type,
            permissions: node.mode,
            attributes: FileAttr::NONE,
            // A directory here has no `.`/`..` on disk to count, and every
            // other node is a single unlinked-from-nowhere device: one link.
            nlinks: 1,
            blocks: 0,
            ..FileMeta::minimal(node.entry_type, 0)
        })
    }

    fn statvfs(&mut self) -> KernelResult<FsInfo> {
        Ok(FsInfo {
            fs_type: String::from("devfs"),
            volume_label: String::new(),
            block_size: 0,
            total_blocks: 0,
            free_blocks: 0,
            total_inodes: DEV_NODES.len() as u64,
            free_inodes: 0,
            max_name_len: 255,
            read_only: false,
        })
    }

    fn debug_stats(&self) -> String {
        format!("devfs: {} nodes", DEV_NODES.len())
    }
}

// ---------------------------------------------------------------------------
// Mount helper
// ---------------------------------------------------------------------------

/// Mount devfs at the given path (typically `/dev`).
///
/// Takes a path rather than a `&str` (design-decisions.md 261): a mount
/// point is an ordinary directory, whose name may contain any byte but `/`
/// and NUL.
pub fn mount(mount_path: impl AsRef<Path>) -> KernelResult<()> {
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

    // Test root readdir.  The root holds every node whose path has no slash.
    let expect_root = DEV_NODES.iter().filter(|n| n.parent().is_empty()).count();
    let entries = fs.readdir(Path::new("/"))?;
    if entries.len() != expect_root {
        serial_println!(
            "[devfs]   FAIL: readdir returned {} entries, expected {}",
            entries.len(),
            expect_root
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   readdir /: {} entries OK", entries.len());

    // Test stat on root.
    let root_stat = fs.stat(Path::new("/"))?;
    if root_stat.entry_type != EntryType::Directory {
        serial_println!("[devfs]   FAIL: stat / not a directory");
        return Err(KernelError::InternalError);
    }

    // Test /dev/null: read returns empty.
    let null_data = fs.read_file(Path::new("/null"))?;
    if !null_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/null read should be empty");
        return Err(KernelError::InternalError);
    }
    // Write to null should succeed.
    fs.write_file(Path::new("/null"), b"discarded")?;
    serial_println!("[devfs]   null: read=empty, write=discard OK");

    // Test /dev/zero: read returns zeros.
    let zero_data = fs.read_file(Path::new("/zero"))?;
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
    let full_data = fs.read_file(Path::new("/full"))?;
    if full_data.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/full read should not be empty");
        return Err(KernelError::InternalError);
    }
    if full_data.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/full data contains non-zero bytes");
        return Err(KernelError::InternalError);
    }
    match fs.write_file(Path::new("/full"), b"should fail") {
        Err(KernelError::DiskFull) => {}
        other => {
            serial_println!(
                "[devfs]   FAIL: /dev/full write should return DiskFull, got {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[devfs]   full: read=zeros, write=DiskFull OK");

    // Test /dev/random: read returns data, two reads differ.
    let rand1 = fs.read_file(Path::new("/random"))?;
    let rand2 = fs.read_file(Path::new("/random"))?;
    if rand1.is_empty() || rand2.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/random read should not be empty");
        return Err(KernelError::InternalError);
    }
    if rand1 == rand2 {
        serial_println!("[devfs]   FAIL: two /dev/random reads should differ");
        return Err(KernelError::InternalError);
    }
    // Write to random (entropy contribution) should succeed.
    fs.write_file(Path::new("/random"), b"entropy seed")?;
    serial_println!(
        "[devfs]   random: {} random bytes, entropy write OK",
        rand1.len()
    );

    // Test /dev/urandom: same behavior as /dev/random.
    let urand = fs.read_file(Path::new("/urandom"))?;
    if urand.is_empty() {
        serial_println!("[devfs]   FAIL: /dev/urandom read should not be empty");
        return Err(KernelError::InternalError);
    }
    fs.write_file(Path::new("/urandom"), b"more entropy")?;
    serial_println!("[devfs]   urandom: {} random bytes OK", urand.len());

    // Test read_at on /dev/zero — should always return zeros regardless of offset.
    let zero_at = fs.read_at(Path::new("/zero"), 99999, 64)?;
    if zero_at.len() != 64 || zero_at.iter().any(|&b| b != 0) {
        serial_println!("[devfs]   FAIL: /dev/zero read_at should return 64 zero bytes");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   read_at /dev/zero: offset-independent OK");

    // Test /dev/console: write should succeed (outputs to console).
    fs.write_file(Path::new("/console"), b"[devfs]   console write test\n")?;
    serial_println!("[devfs]   console: write OK");

    // Test nonexistent device.
    if fs.stat(Path::new("/nonexistent")).is_ok() {
        serial_println!("[devfs]   FAIL: stat /nonexistent should fail");
        return Err(KernelError::InternalError);
    }
    serial_println!("[devfs]   stat /nonexistent: NotFound OK");

    // ------------------------------------------------------------------
    // Device-node subdirectories.
    //
    // This is the whole reason the node table exists, so it is tested as a
    // client would use it: scan the directory, then stat what you found and
    // check it is a character device.  A regression here does not break a
    // kernel test -- it makes libinput report no input devices on a machine
    // whose keyboard works, which is a very long way from the cause.
    // ------------------------------------------------------------------
    for (dir, want) in [("/input", 2usize), ("/dri", 2), ("/snd", 3)] {
        let listing = fs.readdir(Path::new(dir))?;
        if listing.len() != want {
            serial_println!(
                "[devfs]   FAIL: readdir {} returned {} entries, expected {}",
                dir,
                listing.len(),
                want
            );
            return Err(KernelError::InternalError);
        }
        for ent in &listing {
            if ent.entry_type != EntryType::CharDevice {
                serial_println!(
                    "[devfs]   FAIL: {}/{} is not a character device",
                    dir,
                    ent.name.display()
                );
                return Err(KernelError::InternalError);
            }
            // The name readdir reports must be a bare component, and the
            // path built from it must stat back to the same thing -- that
            // round trip is exactly what a scanning client performs.
            let mut full = String::from(dir);
            full.push('/');
            full.push_str(&alloc::format!("{}", ent.name.display()));
            let st = fs.stat(Path::new(&full))?;
            if st.entry_type != EntryType::CharDevice {
                serial_println!("[devfs]   FAIL: stat {} is not a char device", full);
                return Err(KernelError::InternalError);
            }
            let meta = fs.metadata(Path::new(&full))?;
            if meta.entry_type != EntryType::CharDevice || meta.permissions != 0o660 {
                serial_println!(
                    "[devfs]   FAIL: metadata {} is {:?}/{:o}, expected CharDevice/660",
                    full,
                    meta.entry_type,
                    meta.permissions
                );
                return Err(KernelError::InternalError);
            }
            // Reading one through the VFS must say "not that way", never
            // "no such file" -- `stat` just said it is there.
            match fs.read_file(Path::new(&full)) {
                Err(KernelError::NotSupported) => {}
                other => {
                    serial_println!(
                        "[devfs]   FAIL: read {} should be NotSupported, got {:?}",
                        full,
                        other.map(|v| v.len())
                    );
                    return Err(KernelError::InternalError);
                }
            }
        }
        // The directory itself stats as a directory, and reading it as a file
        // is IsADirectory rather than NotFound.
        if fs.stat(Path::new(dir))?.entry_type != EntryType::Directory {
            serial_println!("[devfs]   FAIL: stat {} is not a directory", dir);
            return Err(KernelError::InternalError);
        }
        match fs.read_file(Path::new(dir)) {
            Err(KernelError::IsADirectory) => {}
            _ => {
                serial_println!("[devfs]   FAIL: read {} should be IsADirectory", dir);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[devfs]   {}: {} char devices OK", dir, want);
    }

    // A plain file is not a directory, and a missing directory is not found:
    // the two failure modes must stay distinguishable, because "readdir said
    // NotADirectory" is how a client learns to stop descending.
    match fs.readdir(Path::new("/null")) {
        Err(KernelError::NotADirectory) => {}
        _ => {
            serial_println!("[devfs]   FAIL: readdir /null should be NotADirectory");
            return Err(KernelError::InternalError);
        }
    }
    match fs.readdir(Path::new("/nosuchdir")) {
        Err(KernelError::NotFound) => {}
        _ => {
            serial_println!("[devfs]   FAIL: readdir /nosuchdir should be NotFound");
            return Err(KernelError::InternalError);
        }
    }
    serial_println!("[devfs]   readdir /null=NotADirectory, /nosuchdir=NotFound OK");

    serial_println!("[devfs] Self-test PASSED");
    Ok(())
}
