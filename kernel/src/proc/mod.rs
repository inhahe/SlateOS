//! Process and thread management.
//!
//! ## Features
//!
//! - ELF binary loader
//! - Process creation/destruction (posix_spawn-style, no fork)
//! - Thread creation/destruction
//! - Hardware exceptions → language-level exceptions (SEH-style)
//! - Structured shutdown via IPC (no Unix signals)

// TODO: Process struct (address space, capability table, threads).
// TODO: Thread struct (register state, stack, priority).
// TODO: ELF loader.
// TODO: Process creation (spawn).
// TODO: Thread creation.
// TODO: Exception delivery (SEH-style).
