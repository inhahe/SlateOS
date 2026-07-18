//! Content-Addressed Store (CAS) for data deduplication and integrity.
//!
//! A CAS stores arbitrary byte blobs indexed by their SHA-256 hash.
//! Identical content is automatically deduplicated — storing the same
//! data twice returns the same hash and increments the reference count
//! rather than duplicating the bytes.
//!
//! ## Design
//!
//! - **In-memory**: blobs live in a `BTreeMap<Hash256, CasBlob>` behind a
//!   spinlock.  This keeps the implementation simple and avoids filesystem
//!   dependencies.  Persistence (writing blobs to `/.cas/` on disk) is a
//!   future enhancement.
//! - **Reference counting**: each blob tracks how many logical references
//!   point to it.  `put()` increments, `release()` decrements.  Blobs
//!   with refcount 0 are candidates for garbage collection.
//! - **Bounded**: a configurable `max_bytes` cap prevents OOM.  When the
//!   store exceeds this limit, `put()` returns `DiskFull`.
//! - **Integrity on read**: `get()` recomputes the SHA-256 hash and
//!   verifies it matches the key, detecting bit rot or memory corruption.
//!
//! ## Use cases
//!
//! - **Package manager**: store package contents by hash for atomic
//!   installs, rollback, and cross-generation deduplication.
//! - **File deduplication**: detect duplicate files by comparing their
//!   content hashes against the CAS.
//! - **Snapshot/backup**: store only unique blocks, referencing existing
//!   blobs for unchanged data.
//!
//! ## Reference
//!
//! design.txt: "Use content-addressed packages for isolation and rollback"
//! design.txt: "Per-block hashing [...] detects which part is corrupt,
//! enables dedup"

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A 256-bit (32-byte) SHA-256 hash used as the content address.
pub type Hash256 = [u8; 32];

/// A single blob stored in the CAS.
struct CasBlob {
    /// The raw data.
    data: Vec<u8>,
    /// How many logical references point to this blob.
    refcount: u64,
}

/// Statistics about the CAS.
#[derive(Debug, Clone, Copy)]
pub struct CasStats {
    /// Number of unique blobs stored.
    pub blob_count: usize,
    /// Total bytes of stored data (sum of all blob sizes).
    pub total_bytes: u64,
    /// Total number of logical references (sum of all refcounts).
    pub total_refs: u64,
    /// Maximum byte budget.
    pub max_bytes: u64,
    /// Number of put operations that deduplicated (content already existed).
    pub dedup_hits: u64,
    /// Number of get operations that detected integrity failures.
    pub integrity_failures: u64,
    /// Number of blobs removed by garbage collection.
    pub gc_collected: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct CasInner {
    blobs: BTreeMap<Hash256, CasBlob>,
    total_bytes: u64,
    max_bytes: u64,
    dedup_hits: u64,
    total_refs: u64,
    integrity_failures: u64,
    gc_collected: u64,
}

static CAS: Mutex<CasInner> = Mutex::new(CasInner {
    blobs: BTreeMap::new(),
    total_bytes: 0,
    max_bytes: 64 * 1024 * 1024, // 64 MiB default
    dedup_hits: 0,
    total_refs: 0,
    integrity_failures: 0,
    gc_collected: 0,
});

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the maximum byte budget for the store.
///
/// Existing blobs are not evicted; the limit is enforced on new `put()` calls.
pub fn set_max_bytes(max: u64) {
    CAS.lock().max_bytes = max;
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Store a blob, returning its SHA-256 hash.
///
/// If content with the same hash already exists, the reference count is
/// incremented and no data is duplicated (deduplication).
///
/// Returns `DiskFull` if the store would exceed `max_bytes`.
pub fn put(data: &[u8]) -> KernelResult<Hash256> {
    let hash = crate::crypto::sha256(data);

    let mut cas = CAS.lock();

    // Check if already stored (deduplication).
    if let Some(blob) = cas.blobs.get_mut(&hash) {
        blob.refcount = blob.refcount.saturating_add(1);
        cas.total_refs = cas.total_refs.saturating_add(1);
        cas.dedup_hits = cas.dedup_hits.saturating_add(1);
        return Ok(hash);
    }

    // Check space budget.
    let new_total = cas.total_bytes.saturating_add(data.len() as u64);
    if new_total > cas.max_bytes {
        return Err(KernelError::DiskFull);
    }

    // Store new blob.
    cas.blobs.insert(hash, CasBlob {
        data: data.to_vec(),
        refcount: 1,
    });
    cas.total_bytes = new_total;
    cas.total_refs = cas.total_refs.saturating_add(1);

    Ok(hash)
}

/// Retrieve a blob by its hash.
///
/// Returns a clone of the data.  Verifies integrity by recomputing
/// the SHA-256 hash — returns `CorruptedData` if it doesn't match.
///
/// Returns `NotFound` if no blob with that hash exists.
pub fn get(hash: &Hash256) -> KernelResult<Vec<u8>> {
    let cas = CAS.lock();

    let blob = cas.blobs.get(hash).ok_or(KernelError::NotFound)?;
    let data = blob.data.clone();

    // Integrity check: recompute hash and verify.
    let actual_hash = crate::crypto::sha256(&data);
    if actual_hash != *hash {
        // This should never happen in normal operation — indicates
        // memory corruption.
        drop(cas);
        // Acquire the lock exactly once: `CAS.lock().x = CAS.lock().y` keeps
        // both temporary guards alive until the statement ends, deadlocking
        // the non-reentrant mutex on the second acquisition.
        {
            let mut inner = CAS.lock();
            inner.integrity_failures = inner.integrity_failures.saturating_add(1);
        }
        return Err(KernelError::CorruptedData);
    }

    Ok(data)
}

/// Check if a blob with the given hash exists in the store.
pub fn has(hash: &Hash256) -> bool {
    CAS.lock().blobs.contains_key(hash)
}

/// Get the size of a blob without retrieving its full content.
///
/// Returns `NotFound` if the hash is not in the store.
pub fn blob_size(hash: &Hash256) -> KernelResult<u64> {
    let cas = CAS.lock();
    cas.blobs
        .get(hash)
        .map(|b| b.data.len() as u64)
        .ok_or(KernelError::NotFound)
}

/// Get the reference count of a blob.
///
/// Returns `NotFound` if the hash is not in the store.
pub fn refcount(hash: &Hash256) -> KernelResult<u64> {
    let cas = CAS.lock();
    cas.blobs
        .get(hash)
        .map(|b| b.refcount)
        .ok_or(KernelError::NotFound)
}

/// Release a reference to a blob (decrement refcount).
///
/// Does NOT delete the blob even if refcount reaches 0 — call `gc()`
/// to actually reclaim space.  This separation allows "undo" of a
/// release before GC runs.
///
/// Returns `NotFound` if the hash is not in the store.
pub fn release(hash: &Hash256) -> KernelResult<()> {
    let mut cas = CAS.lock();

    let blob = cas.blobs.get_mut(hash).ok_or(KernelError::NotFound)?;
    if blob.refcount > 0 {
        blob.refcount = blob.refcount.saturating_sub(1);
        cas.total_refs = cas.total_refs.saturating_sub(1);
    }

    Ok(())
}

/// Garbage-collect blobs with refcount == 0.
///
/// Returns the number of blobs removed and total bytes reclaimed.
pub fn gc() -> (usize, u64) {
    let mut cas = CAS.lock();

    let mut removed = 0usize;
    let mut reclaimed = 0u64;

    // Collect keys to remove (can't modify BTreeMap while iterating).
    let dead_keys: Vec<Hash256> = cas
        .blobs
        .iter()
        .filter(|(_, blob)| blob.refcount == 0)
        .map(|(k, _)| *k)
        .collect();

    for key in &dead_keys {
        if let Some(blob) = cas.blobs.remove(key) {
            let size = blob.data.len() as u64;
            cas.total_bytes = cas.total_bytes.saturating_sub(size);
            reclaimed = reclaimed.saturating_add(size);
            removed = removed.saturating_add(1);
        }
    }

    cas.gc_collected = cas.gc_collected.saturating_add(removed as u64);
    (removed, reclaimed)
}

/// Return a snapshot of CAS statistics.
pub fn stats() -> CasStats {
    let cas = CAS.lock();
    CasStats {
        blob_count: cas.blobs.len(),
        total_bytes: cas.total_bytes,
        total_refs: cas.total_refs,
        max_bytes: cas.max_bytes,
        dedup_hits: cas.dedup_hits,
        integrity_failures: cas.integrity_failures,
        gc_collected: cas.gc_collected,
    }
}

/// Remove all blobs from the store.
pub fn clear() {
    let mut cas = CAS.lock();
    cas.blobs.clear();
    cas.total_bytes = 0;
    cas.total_refs = 0;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a Hash256 as a lowercase hexadecimal string.
pub fn hash_to_hex(hash: &Hash256) -> alloc::string::String {
    let mut s = alloc::string::String::with_capacity(64);
    for byte in hash {
        let hi = byte >> 4;
        let lo = byte & 0x0F;
        s.push(hex_digit(hi));
        s.push(hex_digit(lo));
    }
    s
}

/// Parse a hex string back into a Hash256.
///
/// Returns `None` if the string is not exactly 64 hex characters.
pub fn hex_to_hash(hex: &str) -> Option<Hash256> {
    if hex.len() != 64 {
        return None;
    }
    let mut hash = [0u8; 32];
    let bytes = hex.as_bytes();
    let mut i: usize = 0;
    while i < 32 {
        let hi_idx = i.wrapping_mul(2);
        let lo_idx = hi_idx.wrapping_add(1);
        let hi = from_hex_digit(bytes.get(hi_idx).copied().unwrap_or(0))?;
        let lo = from_hex_digit(bytes.get(lo_idx).copied().unwrap_or(0))?;
        hash[i] = (hi << 4) | lo;
        i = i.wrapping_add(1);
    }
    Some(hash)
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0'.wrapping_add(nibble)) as char,
        10..=15 => (b'a'.wrapping_add(nibble.wrapping_sub(10))) as char,
        _ => '?',
    }
}

fn from_hex_digit(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(ch.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(ch.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the content-addressed store.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[cas] Running self-test...");

    // Clean slate.
    clear();
    set_max_bytes(1024 * 1024); // 1 MiB for testing

    // --- Test 1: put and get ---
    {
        let data = b"Hello, content-addressed world!";
        let hash = put(data)?;

        // Verify we can retrieve it.
        let retrieved = get(&hash)?;
        if retrieved.as_slice() != data {
            serial_println!("[cas]   ERROR: get returned wrong data");
            return Err(KernelError::InternalError);
        }

        // Verify has().
        if !has(&hash) {
            serial_println!("[cas]   ERROR: has() returned false for stored blob");
            return Err(KernelError::InternalError);
        }

        // Verify size.
        let size = blob_size(&hash)?;
        if size != data.len() as u64 {
            serial_println!("[cas]   ERROR: blob_size mismatch");
            return Err(KernelError::InternalError);
        }

        serial_println!("[cas]   put/get OK");
    }

    // --- Test 2: deduplication ---
    {
        let data = b"duplicate content";
        let h1 = put(data)?;
        let h2 = put(data)?;

        // Same hash.
        if h1 != h2 {
            serial_println!("[cas]   ERROR: dedup should return same hash");
            return Err(KernelError::InternalError);
        }

        // Refcount should be 3 (1 from test 1, 2 from this test... no wait,
        // different data so it's a different blob). Actually "duplicate content"
        // is a new blob, so refcount should be 2.
        let rc = refcount(&h1)?;
        if rc != 2 {
            serial_println!("[cas]   ERROR: expected refcount 2, got {}", rc);
            return Err(KernelError::InternalError);
        }

        let st = stats();
        if st.dedup_hits < 1 {
            serial_println!("[cas]   ERROR: dedup_hits should be >= 1");
            return Err(KernelError::InternalError);
        }

        serial_println!("[cas]   deduplication OK (hits: {})", st.dedup_hits);
    }

    // --- Test 3: release and GC ---
    {
        let data = b"ephemeral data";
        let hash = put(data)?;
        let rc_before = refcount(&hash)?;
        if rc_before != 1 {
            serial_println!("[cas]   ERROR: expected refcount 1 for new blob");
            return Err(KernelError::InternalError);
        }

        release(&hash)?;
        let rc_after = refcount(&hash)?;
        if rc_after != 0 {
            serial_println!("[cas]   ERROR: expected refcount 0 after release");
            return Err(KernelError::InternalError);
        }

        // Blob still exists (GC not run yet).
        if !has(&hash) {
            serial_println!("[cas]   ERROR: blob should still exist before GC");
            return Err(KernelError::InternalError);
        }

        let (removed, reclaimed) = gc();
        if removed < 1 {
            serial_println!("[cas]   ERROR: GC should have removed at least 1 blob");
            return Err(KernelError::InternalError);
        }

        // Now it's gone.
        if has(&hash) {
            serial_println!("[cas]   ERROR: blob should be gone after GC");
            return Err(KernelError::InternalError);
        }

        serial_println!("[cas]   release/GC OK (removed {}, reclaimed {} bytes)", removed, reclaimed);
    }

    // --- Test 4: hash hex round-trip ---
    {
        let data = b"hex test";
        let hash = put(data)?;
        let hex = hash_to_hex(&hash);
        if hex.len() != 64 {
            serial_println!("[cas]   ERROR: hex string should be 64 chars, got {}", hex.len());
            return Err(KernelError::InternalError);
        }
        let parsed = hex_to_hash(&hex);
        if parsed != Some(hash) {
            serial_println!("[cas]   ERROR: hex round-trip failed");
            return Err(KernelError::InternalError);
        }
        // Invalid hex.
        if hex_to_hash("not-a-hash").is_some() {
            serial_println!("[cas]   ERROR: should reject invalid hex");
            return Err(KernelError::InternalError);
        }
        serial_println!("[cas]   hex round-trip OK");
    }

    // --- Test 5: not found ---
    {
        let fake_hash = [0xFFu8; 32];
        match get(&fake_hash) {
            Err(KernelError::NotFound) => {}
            other => {
                serial_println!("[cas]   ERROR: expected NotFound, got {:?}", other);
                return Err(KernelError::InternalError);
            }
        }
        serial_println!("[cas]   not-found OK");
    }

    // --- Test 6: capacity limit ---
    {
        clear();
        set_max_bytes(100); // Very small limit.

        let small = b"fits";
        let _h = put(small)?;

        let big = [0u8; 200]; // Exceeds the 100-byte limit.
        match put(&big) {
            Err(KernelError::DiskFull) => {}
            other => {
                serial_println!("[cas]   ERROR: expected DiskFull, got {:?}", other);
                set_max_bytes(1024 * 1024);
                return Err(KernelError::InternalError);
            }
        }
        set_max_bytes(1024 * 1024); // Restore.
        serial_println!("[cas]   capacity limit OK");
    }

    // Clean up.
    clear();

    let st = stats();
    serial_println!(
        "[cas] Self-test passed (dedup_hits: {}, gc_collected: {}).",
        st.dedup_hits,
        st.gc_collected,
    );

    Ok(())
}
