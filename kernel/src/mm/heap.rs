//! Kernel heap allocator.
//!
//! Provides `#[global_allocator]` for kernel code that needs dynamic
//! memory (`alloc` crate: `Box`, `Vec`, `String`, etc.).
//!
//! ## Design
//!
//! Phase 1: Simple bump allocator (fast, no free, for early boot).
//! Phase 2: Geometric size-class slab allocator with per-CPU caches.
//!
//! ## Performance Target
//!
//! Common-size allocation: < 200ns (jemalloc: 20-50ns).

// TODO: Implement bump allocator for early boot.
// TODO: Implement slab allocator for general use.
// TODO: Implement GlobalAlloc trait.
// TODO: Per-CPU caches to avoid atomic contention.
