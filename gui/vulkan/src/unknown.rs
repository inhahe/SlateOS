//! Physical-device commands the loader has never heard of, forwarded without
//! knowing their signatures.
//!
//! # The problem this module exists to solve
//!
//! [`crate::global`] made the loader report the union of every driver's instance
//! extensions, which is the honest answer and immediately created a dishonest
//! one next to it. An application that is told `VK_KHR_surface` exists will
//! enable it and then ask `vkGetInstanceProcAddr` for
//! `vkGetPhysicalDeviceSurfaceSupportKHR` — a command this loader has never
//! heard of, does not appear in [`crate::physical::Command`], and until now was
//! answered with null. The loader advertised a capability and then refused to
//! hand out the entry point for it.
//!
//! The reason it refused is [`crate::physical`]'s bill, stated there in the
//! abstract: because a `VkPhysicalDevice` is a loader object the driver has
//! never seen, **every** command taking one has to have its first argument
//! swapped for the driver's own handle before the driver is called. For the nine
//! core commands the loader does that by naming each one and writing a
//! trampoline with the right signature. For an extension command there is no
//! signature to write — the whole point is that the loader does not know the
//! extension exists.
//!
//! # Why a signature is not actually needed
//!
//! Every calling convention the loader targets passes the first pointer argument
//! in a register — `rdi` on System V, `rcx` on Windows x64 — and leaves every
//! other argument exactly where the caller put it, in registers and on the
//! stack. So a forwarder that:
//!
//! 1. reads two words out of the object in the first-argument register,
//! 2. overwrites that register with one of them, and
//! 3. **jumps** — rather than calls — to the driver's function,
//!
//! is correct for *every* signature at once. The stack is untouched, so stack
//! arguments and the caller's return address stay where the callee expects them;
//! the floating-point argument registers are untouched; the callee returns
//! straight to the application, so return values in `rax`/`xmm0` need no handling
//! either. Only one scratch register is used (`rax`), and it is caller-saved in
//! both conventions and is not an argument register in either.
//!
//! That cannot be written in Rust, because Rust has no guaranteed tail call and
//! no way to name "the arguments I was not told about". It is three instructions
//! of assembly, and it is the technique the Khronos loader uses for the same
//! reason (`unknown_ext_chain_*.asm`). This is also the reason
//! `vk_icdGetPhysicalDeviceProcAddr` was added to the Loader–Driver Interface at
//! version 4 at all.
//!
//! # Why the pool is fixed, and why a slot is never reused
//!
//! A trampoline has to be a distinct *address* per command, because the address
//! is all the application keeps — there is no room in the call for a "which
//! command is this" parameter. Distinct addresses that are not distinct code can
//! only come from a table, so the code is generated once per slot at compile time
//! and there is a fixed number of them ([`SLOTS`]).
//!
//! A slot, once given to a name, keeps it for the life of the process. Vulkan
//! lets an application call `vkGetInstanceProcAddr` once at startup and use the
//! pointer forever, so a slot that were ever reassigned would silently turn one
//! command into another in an application that did nothing wrong. Running out of
//! slots therefore reports "no such entry point" — the same null the C API
//! already uses — rather than recycling one.
//!
//! # What is in a slot, and what happens when a driver does not implement it
//!
//! The trampoline is one piece of code shared by every driver, so the driver's
//! function pointer cannot live in it. It lives in a per-driver [`Table`] that
//! the physical-device wrapper points at, which is why the trampoline reads
//! *two* words: the table, and the handle. A machine with three drivers has
//! three tables, and the same trampoline reaches the right one because it got
//! there through that driver's device.
//!
//! An entry no driver answered for is not left null. It holds a stub that
//! aborts, naming what happened. Calling a physical-device extension command on
//! a device whose driver does not implement it is an application error — the
//! extension has to be checked for first — but "unpredictable jump" and
//! "diagnosable abort" are very different ways to be told about it, and the
//! second costs one word per slot.
//!
//! # The rule that makes this safe, and the one that would make it corrupt
//!
//! **Only a name a driver answered through `vk_icdGetPhysicalDeviceProcAddr` may
//! be given a slot.** Falling back to the driver's `vkGetInstanceProcAddr` — which
//! [`crate::physical`] legitimately does for the nine core commands — is not
//! merely weaker here, it is memory corruption. An extension may define the same
//! command name at *device* level and at *physical-device* level; asked through
//! `vkGetInstanceProcAddr` a driver returns a pointer either way and the answer
//! does not say which it was. A device command reached through a trampoline
//! would have argument 0 — a `VkDevice` the loader must pass through untouched —
//! read as if it were a loader wrapper, and two words pulled out of the middle of
//! the driver's object and jumped through.
//!
//! The version-4 entry point exists precisely to answer that one question, and a
//! driver that does not have it contributes nothing here. That is a real
//! limitation and the right one: a version-3 driver cannot be asked the question
//! safely, so it is not asked.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::instance::PhysicalDevice;
use crate::vk::VoidFn;

#[cfg(not(target_arch = "x86_64"))]
compile_error!(
    "gui/vulkan's unknown-command trampolines are x86-64 assembly. Porting the \
     loader to another architecture means writing the three-instruction \
     forwarder for that architecture's calling convention; there is no portable \
     fallback, because the whole point is to forward arguments the compiler was \
     never told about."
);

/// A function pointer of unknown signature, as the pool and the tables hold it.
///
/// The same shape [`VoidFn`] wraps in an `Option`; named separately because the
/// values here are never null — an unresolved entry holds [`unresolved`].
type RawFn = unsafe extern "C" fn();

/// How many distinct unknown commands this loader can forward.
///
/// Vulkan has fewer than fifty physical-device commands across every registered
/// extension, so 128 is not a limit anything reaches; it is a bound chosen
/// because the alternative is generating code at runtime. Exhausting it is
/// reported as "no such entry point" — see the module documentation for why a
/// slot is never taken back.
pub const SLOTS: usize = 128;

/// The permanent assignment of command names to trampoline slots.
///
/// Pure: it decides which slot a name gets and nothing else, so every rule about
/// assignment — first-come order, idempotence, exhaustion — is testable with no
/// driver, no assembly and no unsafe code.
pub struct Slots {
    /// The name occupying each slot, indexed by slot. Only ever appended to;
    /// see the module documentation on why a slot is never reassigned.
    names: Vec<Vec<u8>>,
}

impl Default for Slots {
    fn default() -> Self {
        Self::new()
    }
}

impl Slots {
    /// A pool with nothing assigned yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { names: Vec::new() }
    }

    /// The slot this name already holds, if it has one.
    ///
    /// Distinct from [`Slots::assign`] so that a caller can ask without
    /// consuming a slot — which is what the "was this already resolved" question
    /// in a test looks like.
    #[must_use]
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        self.names.iter().position(|held| held == name)
    }

    /// The slot for this name, assigning a fresh one if it has none.
    ///
    /// `None` means the pool is full. Idempotent: asking twice for the same name
    /// gives the same slot and consumes nothing the second time, which is what
    /// makes it safe to call on every lookup rather than only on the first.
    pub fn assign(&mut self, name: &[u8]) -> Option<usize> {
        if let Some(slot) = self.find(name) {
            return Some(slot);
        }
        let slot = self.names.len();
        if slot >= SLOTS {
            return None;
        }
        self.names.push(name.to_vec());
        Some(slot)
    }

    /// How many slots have been handed out.
    #[must_use]
    pub fn assigned(&self) -> usize {
        self.names.len()
    }

    /// The name occupying a slot, for diagnostics.
    #[must_use]
    pub fn name(&self, slot: usize) -> Option<&[u8]> {
        self.names.get(slot).map(Vec::as_slice)
    }
}

/// One driver's answers, one per slot.
///
/// `#[repr(C)]` and an array of plain words because the trampoline indexes it
/// with three instructions of assembly: the layout is part of the code below,
/// not an implementation detail.
///
/// The entries are `AtomicUsize` for the writing side only. The reading side is
/// a `mov` inside a naked function, which is outside Rust's memory model
/// entirely; what makes that read sound is that x86-64 does not reorder loads
/// with respect to prior stores from the same thread, and that a table entry is
/// always written while the driver registry's lock is held and always read
/// through a pointer the application obtained *after* that lock was released.
/// The atomics keep the writing side defined and document the intent.
#[repr(C)]
pub struct Table {
    entries: [AtomicUsize; SLOTS],
}

impl Table {
    /// A table in which every slot aborts if called.
    ///
    /// Boxed because its *address* is stored in every physical-device wrapper
    /// belonging to the driver, and the driver record itself lives in a `Vec`
    /// that moves when another driver registers. A `Box`'s contents do not move
    /// when the box does, which is the whole reason for the indirection.
    #[must_use]
    pub fn new() -> Box<Self> {
        Box::new(Self {
            entries: core::array::from_fn(|_| AtomicUsize::new(UNRESOLVED as usize)),
        })
    }

    /// Record this driver's function for a slot, or clear it back to the
    /// aborting stub.
    ///
    /// Clearing rather than leaving a stale pointer matters because resolution
    /// is re-run on every lookup: a driver that answered for a name once and
    /// stops must not keep being called through the old pointer.
    pub fn set(&self, slot: usize, target: VoidFn) {
        let Some(entry) = self.entries.get(slot) else {
            return;
        };
        let value = match target {
            Some(function) => function as usize,
            None => UNRESOLVED as usize,
        };
        entry.store(value, Ordering::Release);
    }

    /// What a slot currently holds, as a bare address.
    ///
    /// Exists so that resolution can be *observed* in a test — the alternative
    /// is calling through the trampoline, which proves the entry is right only
    /// if the assembly is also right, and conflates two failures.
    #[must_use]
    pub fn get(&self, slot: usize) -> usize {
        self.entries
            .get(slot)
            .map_or(0, |entry| entry.load(Ordering::Acquire))
    }

    /// Is this slot still the aborting stub?
    #[must_use]
    pub fn is_unresolved(&self, slot: usize) -> bool {
        self.get(slot) == UNRESOLVED as usize
    }

    /// The address the trampolines index from.
    #[must_use]
    pub const fn as_ptr(&self) -> *const Self {
        core::ptr::from_ref(self)
    }
}

/// What an unclaimed slot points at.
///
/// Reached when an application calls a physical-device extension command on a
/// device whose driver does not implement it — which the application is
/// responsible for not doing, having been able to ask. The alternative to
/// aborting is a jump through a null or stale word, and the difference between
/// the two is entirely in how the resulting bug report reads.
///
/// `extern "C"` so that the panic aborts rather than unwinding across a boundary
/// the application's compiler knows nothing about.
// clippy::panic is warned on throughout this workspace because a panic reachable
// from bad input is a denial of service. This one is not reachable from input:
// it is reachable only from an application calling an entry point it was never
// told existed, in place of a jump to an address nobody chose.
#[allow(clippy::panic)]
unsafe extern "C" fn unresolved() -> ! {
    panic!(
        "vkloader: a physical-device extension command was called on a device \
         whose driver does not implement it"
    );
}

/// The aborting stub's address, as one named constant.
///
/// A function *item* in Rust has a zero-sized type of its own, and casting one
/// straight to an integer is a lint the workspace denies — for the good reason
/// that the cast reads as arithmetic on a function while it is really two steps,
/// a coercion to a pointer and then a cast of that. Naming the pointer once
/// makes every use below a plain pointer-to-integer cast, and gives the value a
/// name to compare against instead of repeating the coercion at each site.
const UNRESOLVED: unsafe extern "C" fn() -> ! = unresolved;

/// One forwarder, for one slot.
///
/// Three instructions, in an order that matters: the table pointer is read
/// *before* the first-argument register is overwritten, because both live in the
/// object that register points at.
///
/// ```text
///     mov rax, [arg0 + EXT_OFFSET]     ; the driver's table
///     mov arg0, [arg0 + HANDLE_OFFSET] ; the driver's own VkPhysicalDevice
///     jmp [rax + 8*SLOT]               ; tail call: never comes back here
/// ```
///
/// `rax` is caller-saved in both conventions and is an argument register in
/// neither, so clobbering it is invisible to the callee and to the caller. The
/// jump is what keeps the rest of the frame — stack arguments, the return
/// address, the shadow space Windows callers reserve — exactly as the
/// application built it.
///
/// The one signature shape this would break on is a function returning a large
/// struct by value, where the ABI inserts a hidden pointer as argument 0 and
/// shifts everything along. No Vulkan command does that: every output is written
/// through a caller-supplied pointer, and every return value is a `VkResult`, a
/// handle, or nothing.
///
/// # Safety
///
/// The first argument must be a [`PhysicalDevice`] this loader allocated, whose
/// table entry for `SLOT` holds a function with the signature the caller is
/// using.
#[unsafe(naked)]
unsafe extern "C" fn tramp<const SLOT: usize>() {
    #[cfg(target_os = "windows")]
    core::arch::naked_asm!(
        "mov rax, qword ptr [rcx + {ext}]",
        "mov rcx, qword ptr [rcx + {handle}]",
        "jmp qword ptr [rax + {slot}]",
        ext = const PhysicalDevice::EXT_OFFSET,
        handle = const PhysicalDevice::HANDLE_OFFSET,
        slot = const SLOT * 8,
    );
    #[cfg(not(target_os = "windows"))]
    core::arch::naked_asm!(
        "mov rax, qword ptr [rdi + {ext}]",
        "mov rdi, qword ptr [rdi + {handle}]",
        "jmp qword ptr [rax + {slot}]",
        ext = const PhysicalDevice::EXT_OFFSET,
        handle = const PhysicalDevice::HANDLE_OFFSET,
        slot = const SLOT * 8,
    );
}

/// Instantiate [`tramp`] once per slot.
///
/// The indices are written out because a slot's trampoline must be a distinct
/// compiled function with a distinct address, and there is no loop that produces
/// those. Keeping them in one list next to [`SLOTS`] is what makes a mismatch a
/// type error rather than an out-of-range jump: the array's declared length is
/// checked against the number of elements by the compiler.
macro_rules! pool {
    ($($slot:literal),* $(,)?) => {
        static POOL: [RawFn; SLOTS] = [$(tramp::<$slot>),*];
    };
}

pool!(
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49,
    50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73,
    74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97,
    98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116,
    117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127,
);

/// The address to hand the application for a slot.
///
/// `None` for a slot outside the pool, which is the same answer as "no such
/// entry point" and reaches the application as the null `vkGetInstanceProcAddr`
/// is defined to return.
#[must_use]
pub fn trampoline(slot: usize) -> VoidFn {
    POOL.get(slot).copied()
}

// The five defensive lints the workspace turns on are for production code: a
// test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and an assertion that fails by panicking is a
// test doing its job rather than a robustness hole.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {
    use super::{POOL, RawFn, SLOTS, Slots, Table, UNRESOLVED, trampoline};
    use crate::instance::PhysicalDevice;
    use crate::vk::Handle;
    use alloc::format;
    use alloc::vec::Vec;
    use core::ffi::c_void;

    #[test]
    fn a_name_keeps_the_slot_it_was_first_given() {
        let mut slots = Slots::new();
        let first = slots
            .assign(b"vkGetPhysicalDeviceSurfaceSupportKHR")
            .unwrap();
        let second = slots
            .assign(b"vkGetPhysicalDeviceSurfaceFormatsKHR")
            .unwrap();
        assert_ne!(first, second, "two commands were given one address");

        // The property the module's correctness rests on: an application that
        // looked a command up at startup and calls it an hour later must reach
        // the same command.
        assert_eq!(
            slots.assign(b"vkGetPhysicalDeviceSurfaceSupportKHR"),
            Some(first)
        );
        assert_eq!(slots.assigned(), 2, "a repeat lookup consumed a slot");
    }

    #[test]
    fn an_unassigned_name_is_not_found_without_being_assigned() {
        let mut slots = Slots::new();
        assert_eq!(slots.find(b"vkNothing"), None);
        assert_eq!(slots.assigned(), 0);
        slots.assign(b"vkSomething");
        assert_eq!(slots.find(b"vkNothing"), None);
        assert_eq!(slots.find(b"vkSomething"), Some(0));
    }

    #[test]
    fn a_slot_remembers_its_name() {
        let mut slots = Slots::new();
        slots.assign(b"vkFirst");
        slots.assign(b"vkSecond");
        assert_eq!(slots.name(0), Some(b"vkFirst" as &[u8]));
        assert_eq!(slots.name(1), Some(b"vkSecond" as &[u8]));
        assert_eq!(slots.name(2), None);
    }

    #[test]
    fn a_full_pool_refuses_rather_than_recycling() {
        // Recycling is the failure this refusal exists to prevent: it would turn
        // one command into another in an application that did nothing wrong.
        let mut slots = Slots::new();
        let names: Vec<_> = (0..SLOTS).map(|n| format!("vkCommand{n}")).collect();
        for (n, name) in names.iter().enumerate() {
            assert_eq!(slots.assign(name.as_bytes()), Some(n));
        }
        assert_eq!(slots.assigned(), SLOTS);
        assert_eq!(slots.assign(b"vkOneTooMany"), None);
        assert_eq!(slots.assigned(), SLOTS);
        // And the full pool still answers for everything it did take.
        assert_eq!(slots.assign(names[0].as_bytes()), Some(0));
        assert_eq!(slots.assign(names[SLOTS - 1].as_bytes()), Some(SLOTS - 1));
    }

    #[test]
    fn every_slot_has_its_own_trampoline() {
        // Distinct addresses are the entire mechanism: the address is all the
        // application keeps, so two slots sharing one would make two commands
        // indistinguishable.
        let mut seen: Vec<usize> = POOL.iter().map(|&f| f as usize).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, SLOTS);
        assert_eq!(seen.len(), SLOTS, "two slots share one trampoline address");

        assert!(
            trampoline(SLOTS).is_none(),
            "a slot outside the pool answered"
        );
        assert_eq!(
            trampoline(0).map(|f| f as usize),
            Some(POOL[0] as usize),
            "the pool and the accessor disagree about slot 0"
        );
    }

    #[test]
    fn a_fresh_table_aborts_in_every_slot_rather_than_jumping_to_zero() {
        let table = Table::new();
        for slot in 0..SLOTS {
            assert!(
                table.is_unresolved(slot),
                "slot {slot} would have been jumped to as a null pointer"
            );
        }
        assert_eq!(table.get(SLOTS), 0, "a slot outside the table was readable");
    }

    unsafe extern "C" fn probe() {}

    #[test]
    fn recording_and_clearing_a_slot_leaves_its_neighbours_alone() {
        let table = Table::new();
        // As a fn *pointer*, so that comparing it to a table entry is a pointer
        // cast rather than a cast of a zero-sized function item.
        let probe: RawFn = probe;
        table.set(4, Some(probe));
        assert_eq!(table.get(4), probe as usize);
        assert!(!table.is_unresolved(4));
        assert!(table.is_unresolved(3) && table.is_unresolved(5));

        // A driver that stops answering must stop being called, not keep being
        // reached through the pointer it gave last time.
        table.set(4, None);
        assert!(table.is_unresolved(4));
        assert_eq!(table.get(4), UNRESOLVED as usize);
    }

    #[test]
    fn setting_a_slot_outside_the_table_is_ignored_rather_than_scribbling() {
        let table = Table::new();
        table.set(SLOTS, Some(probe));
        table.set(usize::MAX, Some(probe));
        for slot in 0..SLOTS {
            assert!(table.is_unresolved(slot));
        }
    }

    /// Stands in for a driver's own physical-device object. Its address is what
    /// the trampoline must substitute for the wrapper's.
    #[repr(C)]
    struct DriverObject {
        loader_data: usize,
    }

    /// What the driver's end of a trampolined call saw.
    ///
    /// A `static` rather than a captured variable because the callee is an
    /// `extern "C"` function reached through assembly: there is nowhere to put a
    /// closure environment.
    static mut SEEN: (usize, u64, u64, u64, u64, u64, u64) = (0, 0, 0, 0, 0, 0, 0);

    /// Seven arguments, so that the ones the callee reads off the *stack* are
    /// covered too. Windows x64 passes four in registers and the rest on the
    /// stack above the shadow space; System V passes six. Either way this
    /// signature has arguments in both places, which is the property a jump
    /// preserves and a call would not.
    unsafe extern "C" fn record(
        device: Handle,
        a: u64,
        b: u64,
        c: u64,
        d: u64,
        e: u64,
        f: u64,
    ) -> u64 {
        // SAFETY: single-threaded test, and nothing else touches `SEEN` while
        // this runs.
        unsafe {
            SEEN = (device as usize, a, b, c, d, e, f);
        }
        0xC0FF_EE00_1234_5678
    }

    /// [`record`] as a fn *pointer*, which is the shape a table entry holds and
    /// the shape that can be compared with one.
    const RECORD: unsafe extern "C" fn(Handle, u64, u64, u64, u64, u64, u64) -> u64 = record;

    #[test]
    fn a_trampolined_call_reaches_the_driver_with_its_own_handle_and_every_argument() {
        let mut driver_instance = DriverObject { loader_data: 0 };
        let mut driver_device = DriverObject { loader_data: 0 };
        let instance: Handle = core::ptr::from_mut(&mut driver_instance).cast::<c_void>();
        let handle: Handle = core::ptr::from_mut(&mut driver_device).cast::<c_void>();

        let table = Table::new();
        table.set(
            7,
            Some(unsafe {
                core::mem::transmute::<
                    unsafe extern "C" fn(Handle, u64, u64, u64, u64, u64, u64) -> u64,
                    unsafe extern "C" fn(),
                >(record)
            }),
        );

        let wrapper = PhysicalDevice::new(0, instance, handle, table.as_ptr());
        let wrapper_handle: Handle = core::ptr::from_ref::<PhysicalDevice>(&*wrapper)
            .cast_mut()
            .cast::<c_void>();

        let entry = trampoline(7).unwrap();
        // SAFETY: `entry` is the slot-7 trampoline, the wrapper is a live
        // `PhysicalDevice` whose table entry for slot 7 holds `record`, and
        // `record` has exactly the signature being transmuted to.
        let called = unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(),
                unsafe extern "C" fn(Handle, u64, u64, u64, u64, u64, u64) -> u64,
            >(entry)(wrapper_handle, 1, 2, 3, 4, 5, 6)
        };

        assert_eq!(
            called, 0xC0FF_EE00_1234_5678,
            "the driver's return value did not reach the caller"
        );
        // SAFETY: the call above has returned, so nothing else is writing.
        let seen = unsafe { SEEN };
        assert_eq!(
            seen.0, handle as usize,
            "the driver was handed the loader's wrapper instead of its own device"
        );
        assert_ne!(seen.0, wrapper_handle as usize);
        assert_eq!(
            (seen.1, seen.2, seen.3, seen.4, seen.5, seen.6),
            (1, 2, 3, 4, 5, 6),
            "an argument was lost or shifted -- the stack ones are the likely half"
        );
    }

    #[test]
    fn two_drivers_reach_two_different_functions_through_one_trampoline() {
        // The reason the function pointer lives in a per-driver table rather
        // than in the trampoline: one compiled slot serves every driver, and
        // which one it lands in is decided by the device it was called on.
        let mut one_object = DriverObject { loader_data: 0 };
        let mut two_object = DriverObject { loader_data: 0 };
        let one_handle: Handle = core::ptr::from_mut(&mut one_object).cast::<c_void>();
        let two_handle: Handle = core::ptr::from_mut(&mut two_object).cast::<c_void>();

        let one_table = Table::new();
        let two_table = Table::new();
        // SAFETY: `record` is being erased to the pool's untyped shape and is
        // transmuted back to its own signature before being called.
        let erased = unsafe {
            core::mem::transmute::<
                unsafe extern "C" fn(Handle, u64, u64, u64, u64, u64, u64) -> u64,
                unsafe extern "C" fn(),
            >(record)
        };
        one_table.set(9, Some(erased));
        two_table.set(9, None);

        assert_eq!(one_table.get(9), RECORD as usize);
        assert!(
            two_table.is_unresolved(9),
            "a driver that never answered was given another driver's function"
        );

        let one = PhysicalDevice::new(0, one_handle, one_handle, one_table.as_ptr());
        let two = PhysicalDevice::new(1, two_handle, two_handle, two_table.as_ptr());
        assert_ne!(one.ext(), two.ext(), "both devices point at one table");
        assert_eq!(one.ext(), one_table.as_ptr());
        assert_eq!(two.ext(), two_table.as_ptr());
    }
}
