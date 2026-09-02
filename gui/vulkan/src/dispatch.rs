//! Dispatchable handles: the one memory-layout rule the whole loader rests on.
//!
//! A Vulkan handle like `VkInstance`, `VkPhysicalDevice`, `VkDevice`,
//! `VkQueue` or `VkCommandBuffer` is a *dispatchable* handle, which in the ABI
//! means it is a pointer to a structure whose **first word is a pointer to a
//! dispatch table**. Everything else in the structure belongs to whoever made
//! it and is opaque.
//!
//! That first word is not a loader convention. Layers and drivers dereference
//! it — it is how a call on a `VkQueue` finds the next layer down without
//! being told which instance the queue came from. So the rule is load-bearing
//! for code this crate does not own, and breaking it does not produce a
//! Vulkan error; it produces a jump through whatever integer happened to be
//! at offset 0.
//!
//! ## The handover, and the check that has to happen in the middle
//!
//! A driver allocates a dispatchable object and writes a known constant,
//! [`ICD_LOADER_MAGIC`], into that first word — `set_loader_magic_value` in
//! `vk_icd.h`. It then returns the object to the loader. The loader overwrites
//! the same word with a pointer to *its* dispatch table and passes the object
//! on to the application.
//!
//! The magic exists because of what sits between those two steps. The loader
//! is about to write a pointer over the first eight bytes of a structure it
//! did not allocate, whose layout it cannot see, on the strength of a driver
//! having promised to follow a convention. If the driver did not — if it
//! returned a pointer to something that is not a dispatchable object at all,
//! or forgot the `set_loader_magic_value` call — then that write lands in the
//! middle of the driver's own data. The corruption surfaces later, somewhere
//! else, as a crash inside a shared library with no symbols.
//!
//! So this module offers no way to write the dispatch pointer without checking
//! first. [`adopt`] does both, in that order, and returns
//! [`Err(NotDispatchable)`](NotDispatchable) rather than writing. There is
//! deliberately no `set_dispatch_unchecked`: the check costs one load and one
//! comparison against a constant, which is nothing next to the driver call
//! that produced the object, and every loader bug of this shape is a missing
//! check rather than a wrong one.
//!
//! ## Why the comparison is 32-bit
//!
//! `valid_loader_magic_value` in `vk_icd.h` masks the word to 32 bits before
//! comparing:
//!
//! ```c
//! return (loader_info->loaderMagic & 0xffffffff) == ICD_LOADER_MAGIC;
//! ```
//!
//! The field is a `uintptr_t`, so on a 64-bit target the upper half is not
//! part of the constant and must not be part of the test. A loader that
//! compared the whole word would reject every driver that wrote the magic with
//! a 32-bit store and left whatever was already in the upper half — which is
//! most of them, on most allocators. [`is_loader_magic`] reproduces the mask
//! exactly, and is a plain function over a `usize` so that it can be tested
//! without conjuring a fake driver object.

use core::ffi::c_void;

/// The constant a driver writes into the first word of every dispatchable
/// object it creates.
///
/// `ICD_LOADER_MAGIC` in `vk_icd.h`. Note that the identity of the value is
/// the point — it spells `ICDCODE` in the usual hexadecimal alphabet — and it
/// is only ever compared, never arithmetic.
pub const ICD_LOADER_MAGIC: u32 = 0x01CD_C0DE;

/// A dispatchable object, as the ABI lays it out.
///
/// Only the first word is ours to know about, and this type declares only that
/// word. It is deliberately *not* a description of the whole object: what
/// follows the dispatch slot is the driver's private data, of a size and shape
/// this crate has no way to learn and no business reading. A `#[repr(C)]`
/// struct with one `usize` at offset 0 is therefore the most that can be said
/// truthfully, and it is exactly what the ABI guarantees.
///
/// The loader never constructs one of these over a driver's object — it exists
/// to give the offset-0 access a name and a layout guarantee, so that the
/// unsafe code below reads as a field access rather than as pointer
/// arithmetic.
#[repr(C)]
pub struct Dispatchable {
    /// `VK_LOADER_DATA`: the magic on the way in, a dispatch-table pointer on
    /// the way out. Must be the first member; the ABI is defined in terms of
    /// offset 0.
    loader_data: usize,
}

/// The loader refused to take ownership of a handle a driver returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotDispatchable {
    /// The word actually found at offset 0, so a log can show it.
    ///
    /// This is worth printing: a plausible-looking pointer means the driver
    /// returned a real object of some other kind, whereas a small integer or
    /// zero usually means it returned a non-dispatchable handle where a
    /// dispatchable one was required.
    pub found: usize,
}

/// Does this word carry the loader magic?
///
/// Reproduces `valid_loader_magic_value` from `vk_icd.h`, including its
/// 32-bit mask — see the module documentation for why the mask is not
/// optional.
#[must_use]
pub const fn is_loader_magic(word: usize) -> bool {
    // `as u32` is exactly the `& 0xffffffff` the C does, and is a truncation
    // rather than an arithmetic operation, so it is not the kind of cast the
    // workspace's lints are aimed at.
    (word as u32) == ICD_LOADER_MAGIC
}

/// Write the loader magic into a freshly created dispatchable object.
///
/// This is `set_loader_magic_value`. The loader needs it for the dispatchable
/// objects it creates *itself* — every handle an application sees for which
/// there is no single driver behind it — so that a layer inspecting one finds
/// what it expects.
///
/// # Safety
///
/// `object` must point to an allocation of at least `size_of::<usize>()`
/// bytes, correctly aligned for `usize`, that the caller owns and that no
/// other thread is touching. In practice: an allocation this loader just made.
pub unsafe fn set_loader_magic(object: *mut c_void) {
    debug_assert!(!object.is_null(), "set_loader_magic on a null object");
    // SAFETY: the caller guarantees `object` points to a live, owned,
    // usize-aligned allocation of at least one word. `Dispatchable` is
    // `#[repr(C)]` with `loader_data` first, so this writes offset 0 and
    // nothing beyond it.
    unsafe {
        (*object.cast::<Dispatchable>()).loader_data = ICD_LOADER_MAGIC as usize;
    }
}

/// Read the first word of a dispatchable object.
///
/// # Safety
///
/// `object` must point to at least one readable, `usize`-aligned word that no
/// other thread is writing.
#[must_use]
pub unsafe fn loader_data(object: *const c_void) -> usize {
    debug_assert!(!object.is_null(), "loader_data on a null object");
    // SAFETY: the caller guarantees at least one readable aligned word at
    // `object`, which is exactly `Dispatchable`'s only field at offset 0.
    unsafe { (*object.cast::<Dispatchable>()).loader_data }
}

/// Take ownership of a dispatchable object a driver returned, installing the
/// loader's dispatch table into it.
///
/// Checks the magic *first*. If it is absent the object is left exactly as it
/// was and [`NotDispatchable`] is returned, carrying the word that was found
/// there. This is the only way in this crate to write a dispatch pointer into
/// a driver's object, which is deliberate — see the module documentation.
///
/// `table` is stored as an opaque address. This crate does not model the
/// contents of a dispatch table, because only the address is ever needed here:
/// what dereferences it is the layer or driver on the other side of the call,
/// using its own declaration.
///
/// # Safety
///
/// `object` must point to at least one readable-and-writable, `usize`-aligned
/// word, and must be a pointer the driver returned from a Vulkan entry point
/// that produces a dispatchable handle. No other thread may be touching that
/// word. `table` must outlive every use the application makes of `object` —
/// in practice the loader's per-instance or per-device table, which lives
/// until the instance or device is destroyed.
pub unsafe fn adopt(object: *mut c_void, table: *const c_void) -> Result<(), NotDispatchable> {
    // SAFETY: the caller's guarantees for `object` cover this read.
    let found = unsafe { loader_data(object) };
    if !is_loader_magic(found) {
        return Err(NotDispatchable { found });
    }
    // SAFETY: as above, and the magic check just established that the driver
    // did follow the convention, so offset 0 is the dispatch slot rather than
    // the driver's own data.
    unsafe {
        (*object.cast::<Dispatchable>()).loader_data = table as usize;
    }
    Ok(())
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
    use super::{
        Dispatchable, ICD_LOADER_MAGIC, NotDispatchable, adopt, is_loader_magic, loader_data,
        set_loader_magic,
    };
    use core::ffi::c_void;
    use core::mem::{align_of, size_of};

    /// A stand-in for the structure a driver allocates: the mandated first
    /// word, then private data of its own that the loader must not disturb.
    #[repr(C)]
    struct DriverObject {
        loader_data: usize,
        private_a: u64,
        private_b: u64,
    }

    impl DriverObject {
        /// What a well-behaved driver produces: `set_loader_magic_value` has
        /// been called, and the private fields hold recognisable values.
        fn well_behaved() -> Self {
            Self {
                loader_data: ICD_LOADER_MAGIC as usize,
                private_a: 0xA1A1_A1A1_A1A1_A1A1,
                private_b: 0xB2B2_B2B2_B2B2_B2B2,
            }
        }

        fn as_ptr(&mut self) -> *mut c_void {
            core::ptr::from_mut(self).cast::<c_void>()
        }
    }

    #[test]
    fn the_magic_matches_the_khronos_header() {
        // vk_icd.h: #define ICD_LOADER_MAGIC 0x01CDC0DE
        assert_eq!(ICD_LOADER_MAGIC, 0x01CD_C0DE);
    }

    #[test]
    fn the_dispatch_slot_is_one_word_at_offset_zero() {
        // The ABI is defined in terms of offset 0, so this is the layout
        // assumption every unsafe block in this module rests on. A stray
        // field added above `loader_data` would be caught here rather than by
        // a driver dereferencing garbage.
        assert_eq!(size_of::<Dispatchable>(), size_of::<usize>());
        assert_eq!(align_of::<Dispatchable>(), align_of::<usize>());
    }

    #[test]
    fn only_the_low_thirty_two_bits_decide_the_magic() {
        // valid_loader_magic_value masks with 0xffffffff. A driver that wrote
        // the constant with a 32-bit store leaves whatever was already in the
        // upper half, and must still be accepted.
        assert!(is_loader_magic(ICD_LOADER_MAGIC as usize));
        #[cfg(target_pointer_width = "64")]
        {
            assert!(
                is_loader_magic(0xDEAD_BEEF_01CD_C0DE),
                "upper half must be ignored, or most drivers are rejected"
            );
            assert!(
                !is_loader_magic(0x0000_0000_01CD_C0DF),
                "the low half must still have to match exactly"
            );
        }
    }

    #[test]
    fn a_plausible_pointer_is_not_mistaken_for_the_magic() {
        // The failure mode this guards: a driver returns an object whose first
        // word is a real pointer to something of its own. Nothing about such a
        // word resembles the magic, and it must not.
        let mut victim = 0u64;
        let addr = core::ptr::from_mut(&mut victim) as usize;
        assert!(!is_loader_magic(addr), "a stack address matched the magic");
        assert!(!is_loader_magic(0));
    }

    #[test]
    fn adopting_a_well_behaved_object_installs_the_table_and_leaves_the_rest_alone() {
        let mut obj = DriverObject::well_behaved();
        let table = 0x1234_5678_usize;

        // SAFETY: `obj` is a live, owned, correctly aligned `#[repr(C)]`
        // structure whose first field is a `usize`, and nothing else holds a
        // reference to it.
        let outcome = unsafe { adopt(obj.as_ptr(), table as *const c_void) };

        assert_eq!(outcome, Ok(()));
        assert_eq!(
            obj.loader_data, table,
            "the dispatch pointer was not installed"
        );
        // The half that matters as much: the loader wrote *one* word.
        assert_eq!(
            obj.private_a, 0xA1A1_A1A1_A1A1_A1A1,
            "the driver's data was overwritten"
        );
        assert_eq!(
            obj.private_b, 0xB2B2_B2B2_B2B2_B2B2,
            "the driver's data was overwritten"
        );
    }

    #[test]
    fn an_object_without_the_magic_is_refused_and_not_written_to() {
        // The whole reason the check exists. A driver that forgot
        // set_loader_magic_value, or returned something that is not a
        // dispatchable object at all, must come back with its memory intact --
        // writing a dispatch pointer into it would corrupt the driver's own
        // data and surface much later, somewhere else.
        let mut obj = DriverObject {
            loader_data: 0xFEED_FACE,
            private_a: 0xA1A1_A1A1_A1A1_A1A1,
            private_b: 0xB2B2_B2B2_B2B2_B2B2,
        };

        // SAFETY: as in the previous test.
        let outcome = unsafe { adopt(obj.as_ptr(), 0x1234_5678 as *const c_void) };

        assert_eq!(outcome, Err(NotDispatchable { found: 0xFEED_FACE }));
        assert_eq!(
            obj.loader_data, 0xFEED_FACE,
            "a refused object must be left exactly as it was"
        );
        assert_eq!(obj.private_a, 0xA1A1_A1A1_A1A1_A1A1);
        assert_eq!(obj.private_b, 0xB2B2_B2B2_B2B2_B2B2);
    }

    #[test]
    fn the_refusal_reports_the_word_it_found() {
        // So that a log can distinguish "the driver returned a pointer to
        // something else" from "the driver returned a non-dispatchable handle".
        let mut obj = DriverObject {
            loader_data: 7,
            private_a: 0,
            private_b: 0,
        };
        // SAFETY: as above.
        let err = unsafe { adopt(obj.as_ptr(), core::ptr::null()) }
            .expect_err("an object with 7 at offset 0 is not dispatchable");
        assert_eq!(err.found, 7);
    }

    #[test]
    fn an_object_the_loader_stamps_itself_is_then_adoptable() {
        // The round trip for handles the loader creates rather than a driver:
        // stamp, then adopt. If these two disagreed about where the word lives
        // the loader would reject its own objects.
        let mut obj = DriverObject {
            loader_data: 0,
            private_a: 1,
            private_b: 2,
        };

        // SAFETY: `obj` is live, owned and correctly aligned.
        unsafe {
            set_loader_magic(obj.as_ptr());
        }
        // SAFETY: as above.
        assert!(is_loader_magic(unsafe {
            loader_data(obj.as_ptr().cast_const())
        }));

        // SAFETY: as above.
        let outcome = unsafe { adopt(obj.as_ptr(), 0xABCD as *const c_void) };
        assert_eq!(outcome, Ok(()));
        assert_eq!(obj.loader_data, 0xABCD);
        assert_eq!(
            obj.private_a, 1,
            "stamping must not touch the driver's data either"
        );
    }

    #[test]
    fn adopting_twice_is_refused_the_second_time() {
        // A real consequence of checking rather than trusting: once the
        // dispatch pointer is installed the magic is gone, so a second adopt
        // -- which would mean the loader lost track of an object it already
        // owns -- fails loudly instead of silently rewriting the table
        // pointer. Note this is a *diagnostic* property, not a safety one: an
        // already-adopted object is ours, and rewriting the word would be
        // harmless. It is worth having because "adopted twice" always means a
        // bookkeeping bug somewhere above.
        let mut obj = DriverObject::well_behaved();
        // SAFETY: `obj` is live, owned and correctly aligned.
        assert_eq!(
            unsafe { adopt(obj.as_ptr(), 0x1111 as *const c_void) },
            Ok(())
        );
        // SAFETY: as above.
        let second = unsafe { adopt(obj.as_ptr(), 0x2222 as *const c_void) };
        assert_eq!(second, Err(NotDispatchable { found: 0x1111 }));
        assert_eq!(obj.loader_data, 0x1111, "the first table must survive");
    }
}
