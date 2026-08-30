//! Atomic SPSC driver over the SlateOS netstack shared-memory data ring.
//!
//! [`netipc::ring`] defines the *pure* ring ABI — the header layout, the 32-byte
//! [`Sqe`]/16-byte [`Cqe`] (de)serialization, and the overflow-safe index
//! arithmetic — but deliberately contains no `unsafe` and no atomics (it is
//! `#![forbid(unsafe_code)]`). This crate adds the missing piece: the
//! **atomic accesses to the shared indices**, with the Acquire/Release
//! memory ordering that makes the ring safe to drive concurrently from two
//! address spaces (the kernel forwarders on one side, the ring-3 `netstack`
//! daemon on the other). Isolating the `unsafe` here — one small, audited
//! module linked into both sides — keeps the ordering logic written and
//! reviewed exactly once (see `design-decisions.md` §65).
//!
//! # Model
//!
//! Two single-producer/single-consumer rings share one region:
//!
//! - **SQ** (submission): the *kernel* is the producer ([`Ring::sq_push`]), the
//!   *daemon* the consumer ([`Ring::sq_pop`]).
//! - **CQ** (completion): the *daemon* is the producer ([`Ring::cq_push`]), the
//!   *kernel* the consumer ([`Ring::cq_pop`]).
//!
//! Each index has exactly one writer, so no locks are needed. The producer of a
//! ring advances its *tail*; the consumer advances its *head*. A `push`
//! therefore owns the tail (loaded `Relaxed`) and observes the peer's head
//! (`Acquire`); a `pop` owns the head and observes the peer's tail (`Acquire`).
//! The publishing store (tail on push, head on pop) is `Release`, which is what
//! makes the entry bytes — and, for `OP_SEND`, the data-area payload written
//! before the push — visible to the other side once it `Acquire`-loads the
//! index.
//!
//! # Safety
//!
//! A [`Ring`] wraps a raw `*mut u8` base plus the region length. Constructing
//! one via [`Ring::init`] or [`Ring::attach`] is `unsafe`: the caller promises
//! the pointer is valid for `len` bytes for the ring's lifetime and that at most
//! one producer and one consumer per ring touch it. After construction the
//! `push`/`pop`/data methods are safe (all bounds are validated against the
//! cached, length-checked geometry).

#![cfg_attr(not(test), no_std)]

use core::sync::atomic::{fence, AtomicU32, Ordering};

pub use netipc::ring;
use netipc::ring::{Cqe, Sqe};

/// A driver view over a mapped shared-memory ring region.
///
/// Holds the base pointer, region length, and the geometry cached from the
/// header at construction (so hot-path `push`/`pop` never re-parse the header).
#[derive(Debug, Clone, Copy)]
pub struct Ring {
    base: *mut u8,
    len: usize,
    sq_entries: u32,
    cq_entries: u32,
    sqe_off: usize,
    cqe_off: usize,
    data_off: usize,
    data_len: usize,
}

impl Ring {
    /// Initialise a fresh region as a ring and return a driver view.
    ///
    /// Writes the header scalars, zeroes the four indices, and publishes the
    /// magic **last** (with a release fence) so a peer that [`attach`]es and
    /// sees the magic is guaranteed to also see the geometry. `sq_entries` and
    /// `cq_entries` must be powers of two; the region must be at least
    /// [`ring::region_size`] bytes. Returns `None` if the geometry is invalid
    /// or does not fit in `len`.
    ///
    /// [`attach`]: Ring::attach
    ///
    /// # Safety
    ///
    /// `base` must be a valid, writable, `u32`-aligned pointer to at least `len`
    /// bytes that stays valid for the returned `Ring`'s lifetime, and no other
    /// party may concurrently access the region during initialisation.
    #[must_use]
    pub unsafe fn init(
        base: *mut u8,
        len: usize,
        sq_entries: u32,
        cq_entries: u32,
        data_len: u32,
    ) -> Option<Ring> {
        if !ring::is_power_of_two(sq_entries) || !ring::is_power_of_two(cq_entries) {
            return None;
        }
        let need = ring::region_size(sq_entries, cq_entries, data_len);
        if need > len {
            return None;
        }
        let sqe_off = ring::sqe_array_off();
        let cqe_off = ring::cqe_array_off(sq_entries);
        let data_off = ring::data_area_off(sq_entries, cq_entries);

        // SAFETY: every offset below is < HEADER_LEN <= need <= len, and `base`
        // is valid+aligned per the fn contract. Scalars are written before the
        // magic (published last with a release fence).
        unsafe {
            write_u32(base, ring::OFF_VERSION, ring::RING_VERSION);
            write_u32(base, ring::OFF_SQ_ENTRIES, sq_entries);
            write_u32(base, ring::OFF_CQ_ENTRIES, cq_entries);
            write_u32(base, ring::OFF_SQE_OFF, sqe_off as u32);
            write_u32(base, ring::OFF_CQE_OFF, cqe_off as u32);
            write_u32(base, ring::OFF_DATA_OFF, data_off as u32);
            write_u32(base, ring::OFF_DATA_LEN, data_len);
            // Zero the four free-running indices.
            atomic_at(base, ring::OFF_SQ_HEAD).store(0, Ordering::Relaxed);
            atomic_at(base, ring::OFF_SQ_TAIL).store(0, Ordering::Relaxed);
            atomic_at(base, ring::OFF_CQ_HEAD).store(0, Ordering::Relaxed);
            atomic_at(base, ring::OFF_CQ_TAIL).store(0, Ordering::Relaxed);
            // Publish: release-store the magic after all geometry is in place.
            atomic_at(base, ring::OFF_MAGIC).store(ring::RING_MAGIC, Ordering::Release);
        }

        Some(Ring {
            base,
            len,
            sq_entries,
            cq_entries,
            sqe_off,
            cqe_off,
            data_off,
            data_len: data_len as usize,
        })
    }

    /// Attach to a region another party [`init`]ed. Validates the magic
    /// (`Acquire`) and version, reads the geometry, and re-checks that it fits
    /// in `len`. Returns `None` if the magic/version is wrong or the geometry is
    /// inconsistent with `len` (a corrupt or hostile region can never make the
    /// driver read/write out of bounds).
    ///
    /// [`init`]: Ring::init
    ///
    /// # Safety
    ///
    /// `base` must be a valid, `u32`-aligned pointer to at least `len` bytes
    /// that stays valid for the returned `Ring`'s lifetime.
    #[must_use]
    pub unsafe fn attach(base: *mut u8, len: usize) -> Option<Ring> {
        if len < ring::HEADER_LEN {
            return None;
        }
        // SAFETY: len >= HEADER_LEN, so all header offsets are in bounds.
        let (magic, version) = unsafe {
            let m = atomic_at(base, ring::OFF_MAGIC).load(Ordering::Acquire);
            let v = read_u32(base, ring::OFF_VERSION);
            (m, v)
        };
        if magic != ring::RING_MAGIC || version != ring::RING_VERSION {
            return None;
        }
        // SAFETY: header in bounds (checked above).
        let (sq_entries, cq_entries, sqe_off, cqe_off, data_off, data_len) = unsafe {
            (
                read_u32(base, ring::OFF_SQ_ENTRIES),
                read_u32(base, ring::OFF_CQ_ENTRIES),
                read_u32(base, ring::OFF_SQE_OFF) as usize,
                read_u32(base, ring::OFF_CQE_OFF) as usize,
                read_u32(base, ring::OFF_DATA_OFF) as usize,
                read_u32(base, ring::OFF_DATA_LEN) as usize,
            )
        };
        if !ring::is_power_of_two(sq_entries) || !ring::is_power_of_two(cq_entries) {
            return None;
        }
        // Re-derive the canonical offsets and require the region to agree — this
        // is what stops a bogus data_off/len from escaping the mapping.
        if sqe_off != ring::sqe_array_off()
            || cqe_off != ring::cqe_array_off(sq_entries)
            || data_off != ring::data_area_off(sq_entries, cq_entries)
        {
            return None;
        }
        let need = ring::region_size(sq_entries, cq_entries, data_len as u32);
        if need > len {
            return None;
        }
        Some(Ring {
            base,
            len,
            sq_entries,
            cq_entries,
            sqe_off,
            cqe_off,
            data_off,
            data_len,
        })
    }

    /// SQ capacity (power of two).
    #[must_use]
    pub fn sq_entries(&self) -> u32 {
        self.sq_entries
    }

    /// CQ capacity (power of two).
    #[must_use]
    pub fn cq_entries(&self) -> u32 {
        self.cq_entries
    }

    /// Length of the bulk data area, in bytes.
    #[must_use]
    pub fn data_len(&self) -> usize {
        self.data_len
    }

    // ---- Submission queue (kernel produces, daemon consumes) ----

    /// Push a submission entry (producer side). Returns `false` if the SQ is
    /// full (the caller should retry after the consumer drains).
    #[must_use]
    pub fn sq_push(&self, sqe: &Sqe) -> bool {
        // We own the tail (Relaxed); observe the consumer's head (Acquire).
        let tail = self.load_relaxed(ring::OFF_SQ_TAIL);
        let head = self.load_acquire(ring::OFF_SQ_HEAD);
        if ring::is_full(head, tail, self.sq_entries) {
            return false;
        }
        let slot = ring::slot(tail, self.sq_entries) as usize;
        let off = self.sqe_off + slot * ring::SQE_SIZE;
        // SAFETY: slot < sq_entries, so off + SQE_SIZE <= cqe_off <= len.
        unsafe { self.write_bytes(off, &sqe.to_bytes()) };
        // Publish (Release) so the entry bytes are visible before the new tail.
        self.store_release(ring::OFF_SQ_TAIL, tail.wrapping_add(1));
        true
    }

    /// Pop a submission entry (consumer side). Returns `None` if the SQ is
    /// empty.
    #[must_use]
    pub fn sq_pop(&self) -> Option<Sqe> {
        // We own the head (Relaxed); observe the producer's tail (Acquire).
        let head = self.load_relaxed(ring::OFF_SQ_HEAD);
        let tail = self.load_acquire(ring::OFF_SQ_TAIL);
        if ring::is_empty(head, tail) {
            return None;
        }
        let slot = ring::slot(head, self.sq_entries) as usize;
        let off = self.sqe_off + slot * ring::SQE_SIZE;
        let mut buf = [0u8; ring::SQE_SIZE];
        // SAFETY: slot < sq_entries, so off + SQE_SIZE <= cqe_off <= len.
        unsafe { self.read_bytes(off, &mut buf) };
        let sqe = Sqe::from_bytes(&buf)?;
        // Free the slot (Release) so the producer can observe the advance.
        self.store_release(ring::OFF_SQ_HEAD, head.wrapping_add(1));
        Some(sqe)
    }

    // ---- Completion queue (daemon produces, kernel consumes) ----

    /// Push a completion entry (producer side). Returns `false` if the CQ is
    /// full.
    #[must_use]
    pub fn cq_push(&self, cqe: &Cqe) -> bool {
        let tail = self.load_relaxed(ring::OFF_CQ_TAIL);
        let head = self.load_acquire(ring::OFF_CQ_HEAD);
        if ring::is_full(head, tail, self.cq_entries) {
            return false;
        }
        let slot = ring::slot(tail, self.cq_entries) as usize;
        let off = self.cqe_off + slot * ring::CQE_SIZE;
        // SAFETY: slot < cq_entries, so off + CQE_SIZE <= data_off <= len.
        unsafe { self.write_bytes(off, &cqe.to_bytes()) };
        self.store_release(ring::OFF_CQ_TAIL, tail.wrapping_add(1));
        true
    }

    /// Pop a completion entry (consumer side). Returns `None` if the CQ is
    /// empty.
    #[must_use]
    pub fn cq_pop(&self) -> Option<Cqe> {
        let head = self.load_relaxed(ring::OFF_CQ_HEAD);
        let tail = self.load_acquire(ring::OFF_CQ_TAIL);
        if ring::is_empty(head, tail) {
            return None;
        }
        let slot = ring::slot(head, self.cq_entries) as usize;
        let off = self.cqe_off + slot * ring::CQE_SIZE;
        let mut buf = [0u8; ring::CQE_SIZE];
        // SAFETY: slot < cq_entries, so off + CQE_SIZE <= data_off <= len.
        unsafe { self.read_bytes(off, &mut buf) };
        let cqe = Cqe::from_bytes(&buf)?;
        self.store_release(ring::OFF_CQ_HEAD, head.wrapping_add(1));
        Some(cqe)
    }

    // ---- Bulk data area ----

    /// Copy `src` into the data area at byte offset `off`. Returns `false` if
    /// the window `[off, off+src.len())` would fall outside the data area.
    #[must_use]
    pub fn write_data(&self, off: usize, src: &[u8]) -> bool {
        let Some(end) = off.checked_add(src.len()) else {
            return false;
        };
        if end > self.data_len {
            return false;
        }
        // SAFETY: end <= data_len, so data_off+off .. data_off+end <= len.
        unsafe { self.write_bytes(self.data_off + off, src) };
        true
    }

    /// Copy `dst.len()` bytes from the data area at byte offset `off` into
    /// `dst`. Returns `false` if the window would fall outside the data area.
    #[must_use]
    pub fn read_data(&self, off: usize, dst: &mut [u8]) -> bool {
        let Some(end) = off.checked_add(dst.len()) else {
            return false;
        };
        if end > self.data_len {
            return false;
        }
        // SAFETY: end <= data_len, so data_off+off .. data_off+end <= len.
        unsafe { self.read_bytes(self.data_off + off, dst) };
        true
    }

    // ---- Internal atomic / byte helpers ----

    fn load_relaxed(&self, off: usize) -> u32 {
        // SAFETY: `off` is a header index offset (< HEADER_LEN <= len), 4-aligned.
        unsafe { atomic_at(self.base, off).load(Ordering::Relaxed) }
    }

    fn load_acquire(&self, off: usize) -> u32 {
        // SAFETY: as `load_relaxed`.
        unsafe { atomic_at(self.base, off).load(Ordering::Acquire) }
    }

    fn store_release(&self, off: usize, val: u32) {
        // SAFETY: as `load_relaxed`.
        unsafe { atomic_at(self.base, off).store(val, Ordering::Release) }
    }

    /// # Safety
    /// `off + src.len() <= self.len` and the region is writable.
    unsafe fn write_bytes(&self, off: usize, src: &[u8]) {
        debug_assert!(off.saturating_add(src.len()) <= self.len);
        // SAFETY: caller guarantees the range is in bounds; `base` is valid.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), self.base.add(off), src.len());
        }
    }

    /// # Safety
    /// `off + dst.len() <= self.len` and the region is readable.
    unsafe fn read_bytes(&self, off: usize, dst: &mut [u8]) {
        debug_assert!(off.saturating_add(dst.len()) <= self.len);
        // SAFETY: caller guarantees the range is in bounds; `base` is valid.
        unsafe {
            core::ptr::copy_nonoverlapping(self.base.add(off), dst.as_mut_ptr(), dst.len());
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers (raw pointer ↔ atomic / scalar at a byte offset)
// ---------------------------------------------------------------------------

/// Borrow the `AtomicU32` living at byte offset `off` from `base`.
///
/// # Safety
/// `off + 4 <= region length`, `base + off` is 4-byte aligned, and the bytes
/// are only ever accessed through atomics for the borrow's lifetime.
unsafe fn atomic_at<'a>(base: *mut u8, off: usize) -> &'a AtomicU32 {
    // SAFETY: caller guarantees alignment, bounds, and atomic-only access.
    unsafe { AtomicU32::from_ptr(base.add(off).cast::<u32>()) }
}

/// # Safety: `off + 4 <= len`, `base+off` 4-aligned.
unsafe fn write_u32(base: *mut u8, off: usize, val: u32) {
    // A plain (non-atomic) write, used only for the geometry scalars during
    // `init` before the magic is published; ordered by the subsequent release
    // fence + release-store of the magic.
    fence(Ordering::Release);
    // SAFETY: caller guarantees bounds+alignment.
    unsafe { base.add(off).cast::<u32>().write(val.to_le()) }
}

/// # Safety: `off + 4 <= len`, `base+off` 4-aligned.
unsafe fn read_u32(base: *mut u8, off: usize) -> u32 {
    // SAFETY: caller guarantees bounds+alignment.
    u32::from_le(unsafe { base.add(off).cast::<u32>().read() })
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec;

    /// Allocate a zeroed, 8-byte-aligned backing buffer of `n` bytes.
    fn region(n: usize) -> vec::Vec<u64> {
        // u64 vec gives 8-byte alignment; length rounded up to u64 units.
        vec::from_elem(0u64, n.div_ceil(8))
    }

    #[test]
    fn init_then_attach_reads_geometry() {
        let sq = 8u32;
        let cq = 8u32;
        let data = 4096u32;
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();
        let r = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();
        assert_eq!(r.sq_entries(), sq);
        assert_eq!(r.cq_entries(), cq);
        assert_eq!(r.data_len(), data as usize);

        let a = unsafe { Ring::attach(base, need) }.unwrap();
        assert_eq!(a.sq_entries(), sq);
        assert_eq!(a.cq_entries(), cq);
        assert_eq!(a.data_len(), data as usize);
    }

    #[test]
    fn attach_rejects_uninitialised() {
        let mut buf = region(4096);
        let base = buf.as_mut_ptr().cast::<u8>();
        assert!(unsafe { Ring::attach(base, 4096) }.is_none());
    }

    #[test]
    fn init_rejects_non_power_of_two_and_overflow() {
        let mut buf = region(8192);
        let base = buf.as_mut_ptr().cast::<u8>();
        assert!(unsafe { Ring::init(base, 8192, 3, 8, 128) }.is_none());
        assert!(unsafe { Ring::init(base, 8192, 8, 6, 128) }.is_none());
        // Geometry too big for the region.
        assert!(unsafe { Ring::init(base, 64, 8, 8, 4096) }.is_none());
    }

    #[test]
    fn sq_push_pop_round_trip() {
        let (sq, cq, data) = (4u32, 4u32, 256u32);
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();
        let r = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();

        assert!(r.sq_pop().is_none()); // empty
        let sqe = Sqe {
            op: ring::OP_SEND,
            conn_id: 7,
            data_off: 0,
            data_len: 4,
            user_data: 0xCAFE,
            aux: 0,
        };
        assert!(r.sq_push(&sqe));
        let got = r.sq_pop().unwrap();
        assert_eq!(got, sqe);
        assert!(r.sq_pop().is_none());
    }

    #[test]
    fn sq_fills_and_reports_full() {
        let (sq, cq, data) = (4u32, 4u32, 64u32);
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();
        let r = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();
        for i in 0..sq {
            let sqe = Sqe { op: ring::OP_NOP, user_data: i as u64, ..Sqe::default() };
            assert!(r.sq_push(&sqe), "push {i} should fit");
        }
        // Now full.
        assert!(!r.sq_push(&Sqe::default()));
        // Drain one, then one more fits.
        let first = r.sq_pop().unwrap();
        assert_eq!(first.user_data, 0);
        assert!(r.sq_push(&Sqe { op: ring::OP_NOP, user_data: 99, ..Sqe::default() }));
    }

    #[test]
    fn sq_wraps_many_times() {
        let (sq, cq, data) = (2u32, 2u32, 32u32);
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();
        let r = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();
        // Push/pop 1000 entries through a 2-slot ring.
        for i in 0..1000u64 {
            let sqe = Sqe { op: ring::OP_SEND, user_data: i, ..Sqe::default() };
            assert!(r.sq_push(&sqe));
            let got = r.sq_pop().unwrap();
            assert_eq!(got.user_data, i);
        }
    }

    #[test]
    fn cq_push_pop_round_trip() {
        let (sq, cq, data) = (4u32, 4u32, 64u32);
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();
        let r = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();
        assert!(r.cq_pop().is_none());
        let cqe = Cqe { user_data: 0xCAFE, result: 42, flags: 0 };
        assert!(r.cq_push(&cqe));
        assert_eq!(r.cq_pop().unwrap(), cqe);
    }

    #[test]
    fn data_area_bounds() {
        let (sq, cq, data) = (4u32, 4u32, 64u32);
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();
        let r = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();
        assert!(r.write_data(0, b"hello"));
        let mut out = [0u8; 5];
        assert!(r.read_data(0, &mut out));
        assert_eq!(&out, b"hello");
        // Out of bounds.
        assert!(!r.write_data(60, b"toolong"));
        assert!(!r.read_data(64, &mut [0u8; 1]));
        // Boundary: last byte exactly.
        assert!(r.write_data(63, b"x"));
    }

    #[test]
    fn end_to_end_echo_through_ring() {
        // Simulate the kernel↔daemon echo: producer submits an OP_SEND whose
        // data window holds "PING"; consumer reads it, upper-cases into the same
        // window, and posts a completion; producer reads the completion + result.
        let (sq, cq, data) = (4u32, 4u32, 256u32);
        let need = ring::region_size(sq, cq, data);
        let mut buf = region(need);
        let base = buf.as_mut_ptr().cast::<u8>();

        // Kernel side inits.
        let k = unsafe { Ring::init(base, need, sq, cq, data) }.unwrap();
        assert!(k.write_data(0, b"ping"));
        let sqe = Sqe { op: ring::OP_SEND, conn_id: 1, data_off: 0, data_len: 4, user_data: 0xABCD, aux: 0 };
        assert!(k.sq_push(&sqe));

        // Daemon side attaches to the same region.
        let d = unsafe { Ring::attach(base, need) }.unwrap();
        let req = d.sq_pop().unwrap();
        assert_eq!(req.op, ring::OP_SEND);
        let mut payload = [0u8; 4];
        assert!(d.read_data(req.data_off as usize, &mut payload));
        assert_eq!(&payload, b"ping");
        for b in &mut payload {
            *b = b.to_ascii_uppercase();
        }
        assert!(d.write_data(req.data_off as usize, &payload));
        assert!(d.cq_push(&Cqe { user_data: req.user_data, result: 4, flags: 0 }));

        // Kernel reaps the completion and sees the transformed data.
        let cqe = k.cq_pop().unwrap();
        assert_eq!(cqe.user_data, 0xABCD);
        assert_eq!(cqe.result, 4);
        let mut echoed = [0u8; 4];
        assert!(k.read_data(0, &mut echoed));
        assert_eq!(&echoed, b"PING");
    }
}
