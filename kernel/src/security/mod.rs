//! Security subsystem.
//!
//! Hardware security features and security policy enforcement.
//!
//! ## Features
//!
//! - Intel CET (Control-flow Enforcement Technology): shadow stack +
//!   indirect branch tracking.  < 1% overhead on supporting hardware.
//! - LLVM CFI (Control Flow Integrity): compile-time instrumentation
//!   for C/C++ code.  1-5% overhead.
//! - IOMMU setup and DMA sandboxing for drivers.

// TODO: Intel CET initialization.
// TODO: LLVM CFI setup for C/C++ code.
// TODO: IOMMU initialization.
// TODO: DMA sandbox for userspace drivers.
