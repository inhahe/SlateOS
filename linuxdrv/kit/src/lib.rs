//! lxkit -- the Linux driver layer's system-facing half (design-decisions
//! 1578 rule 3, 1580).
//!
//! A Linux driver process is Linux's own code, the layer's C half
//! (`linuxdrv/emul/`, which implements Linux's functions) and this crate,
//! which implements what the C half stands on -- the `slx_*` interface in
//! `linuxdrv/emul/slx.h` -- with the system's own calls. It never sees a
//! Linux header. It holds the process's entry point, its memory
//! ([`buddy`]), the console, the clock, its arguments ([`args`]), files
//! read by path ([`files`]), its tasks
//! -- Linux's kthreads and the like, coroutines on its one thread ([`task`],
//! and `sched`, which runs them, and `wait`, where it waits with none to run)
//! -- the PCI functions it was given (`pci`), a network card it serves to
//! the network stack (`nicport`, [`frames`]), and how it ends.
//!
//! What can be tested on the host is a library ([`args`], [`bars`],
//! [`buddy`], [`files`], [`frames`], [`task`], [`window`]); the rest (`nicport`,
//! `pci`, `sched`, `sys`, `wait`) is built only for the driver process's
//! own target.

// Bare only on the driver process's own target; on the host, std, for the
// tests.
#![cfg_attr(target_os = "none", no_std)]

pub mod args;
pub mod bars;
pub mod buddy;
pub mod files;
pub mod frames;
pub mod task;
pub mod window;

#[cfg(target_os = "none")]
mod nicport;
#[cfg(target_os = "none")]
mod pci;
#[cfg(target_os = "none")]
mod sched;
#[cfg(target_os = "none")]
mod sys;
#[cfg(target_os = "none")]
mod wait;

/// The exit codes a driver process ends with besides an initcall's own;
/// `slx.h` names the same, and the kernel's self-test reads them.
pub mod exit {
    /// Every initcall returned 0.
    pub const OK: i32 = 0;
    /// An argument named a Linux module parameter that does not exist,
    /// gave one a value its kind refuses, or held both a space and a quote
    /// (`lx_emul.c`'s parameters).
    pub const ARGUMENT: i32 = 118;
    /// Linux code warned in a run that must not: a network driver's serving
    /// of one stack (`lx_nic.c`).
    pub const WARNED: i32 = 119;
    /// The system gave no clock.
    pub const NO_CLOCK: i32 = 120;
    /// Linux code called `BUG()`.
    pub const BUG: i32 = 121;
    /// The layer's Rust half panicked.
    pub const PANIC: i32 = 122;
    /// The system gave no random bytes.
    pub const NO_RANDOM: i32 = 123;
    /// Every task waits on something that will never happen.
    pub const DEADLOCK: i32 = 124;
    /// Linux code reached a function the layer does not implement.
    pub const STOP: i32 = 125;
    /// An initcall failed.
    pub const INIT_FAILED: i32 = 126;
    /// The process's memory could not be set up.
    pub const NO_MEMORY: i32 = 127;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used)]

    use super::exit;
    use std::collections::HashMap;

    fn read(rel: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    /// The exit codes are written three times -- `slx.h` for the C half,
    /// [`exit`] for this one, `kernel/src/linuxdrv.rs` for the self-test
    /// that reads them -- and must be one set.
    #[test]
    fn the_exit_codes_are_one_set() {
        let header: HashMap<String, i32> = read("../emul/slx.h")
            .lines()
            .filter_map(|l| {
                let mut words = l.strip_prefix("#define SLX_EXIT_")?.split_whitespace();
                Some((words.next()?.to_string(), words.next()?.parse().ok()?))
            })
            .collect();
        let kit = [
            ("OK", exit::OK),
            ("ARGUMENT", exit::ARGUMENT),
            ("WARNED", exit::WARNED),
            ("NO_CLOCK", exit::NO_CLOCK),
            ("BUG", exit::BUG),
            ("PANIC", exit::PANIC),
            ("NO_RANDOM", exit::NO_RANDOM),
            ("DEADLOCK", exit::DEADLOCK),
            ("STOP", exit::STOP),
            ("INIT_FAILED", exit::INIT_FAILED),
            ("NO_MEMORY", exit::NO_MEMORY),
        ];
        assert_eq!(
            header.len(),
            kit.len(),
            "slx.h's codes are not lxkit::exit's: {header:?}"
        );
        for (name, code) in kit {
            assert_eq!(header.get(name), Some(&code), "SLX_EXIT_{name}");
        }
        let kernel = read("../../kernel/src/linuxdrv.rs");
        let mut seen = 0;
        for line in kernel.lines() {
            let Some(rest) = line.strip_prefix("const EXIT_") else {
                continue;
            };
            let (name, value) = rest
                .split_once(": i32 = ")
                .expect("const EXIT_<NAME>: i32 = <n>;");
            let value: i32 = value.trim_end_matches(';').parse().expect("a number");
            assert_eq!(header.get(name), Some(&value), "the kernel's EXIT_{name}");
            seen += 1;
        }
        assert!(seen >= 5, "the kernel's exit codes were not found");
    }
}
