//! The three commands an application may call before it has anything to call
//! them on.
//!
//! # What "global" means here
//!
//! Almost every Vulkan command takes a handle. These three do not:
//! `vkEnumerateInstanceVersion`, `vkEnumerateInstanceLayerProperties` and
//! `vkEnumerateInstanceExtensionProperties` are looked up through
//! `vkGetInstanceProcAddr` with a **null** instance, and they are how an
//! application decides what to ask for in the `vkCreateInstance` it has not
//! made yet. They are the first Vulkan an application runs.
//!
//! They were deliberately **not exported** while the loader could not answer
//! them, and the [crate documentation](crate) argued the case: a command that
//! reports an empty list is indistinguishable from one that looked and found
//! nothing, so exporting a stub turns "this loader is incomplete" into "your
//! driver has no extensions", and the bug report goes to the wrong place. A
//! missing *symbol* names itself.
//!
//! That argument only justified the omission for as long as the loader had no
//! honest answer. It now has one for each, so the omission ends. The distinction
//! that matters and is easy to lose: an empty list computed from a real registry
//! is a correct answer; an empty list returned without looking is a lie that
//! happens to be short.
//!
//! # The one structure the loader reads, and why it is the exception
//!
//! [`crate::vk`] states an invariant — every Vulkan structure appears in the
//! loader's signatures as `*const c_void`, and none is declared — and gives the
//! reason: a wrong function signature is caught almost immediately, and a wrong
//! structure layout is caught by nothing, because the caller writes field `A`,
//! the driver reads field `B`, and a plausible wrong answer comes out. It also
//! names the moment to revisit it: *when a command needs the loader to look
//! inside a structure.* This is that moment, and the answer is to declare
//! exactly one.
//!
//! `vkEnumerateInstanceExtensionProperties` cannot be forwarded, because there
//! is nothing to forward it to — there is no instance and so no driver has been
//! selected. Its honest answer is the **union** of what every registered driver
//! reports, and a union has to be de-duplicated, and de-duplication has to
//! compare extension *names*. Comparing whole records instead does not work: two
//! drivers reporting the same extension at different `specVersion`s produce
//! different bytes and must still collapse to one entry.
//!
//! So [`ExtensionProperties`] is declared. What makes it a tolerable exception
//! rather than the first crack in the invariant:
//!
//! - It has **two fields**, one of which is a fixed-size byte array whose length
//!   is itself part of the ABI (`VK_MAX_EXTENSION_NAME_SIZE`, 256). There is no
//!   version-dependent tail, no `pNext`, and no enumeration to get wrong.
//! - Its size and alignment are asserted at compile time below, so a mistake is
//!   a build failure rather than a wrong answer at runtime.
//! - It is the **only** structure any of these three commands touches.
//!   `vkEnumerateInstanceLayerProperties` writes none, because the list is
//!   empty; `vkEnumerateInstanceVersion` writes a `u32`.
//!
//! The rule the invariant becomes, then, is not "never declare one" but
//! **"declare one only when the loader must read a field, and argue it there"** —
//! which is what this section is.
//!
//! # Layers: the empty list is the true one
//!
//! A Vulkan *layer* is a library interposed between the application and the
//! driver — the validation layer is the one everybody has met. Enabling one
//! means loading a shared object at runtime, and `posix::dlfcn::dlopen` on
//! SlateOS returns null with `"dynamic linking not supported"`. There is
//! therefore no mechanism by which a layer could exist, and reporting none is
//! not a shortfall being papered over; it is the state of the machine.
//!
//! This is why the command can be exported now while it could not be before.
//! Nothing changed about the answer — what changed is that the answer is now
//! written down with its reason attached, rather than being a stub that looks
//! identical to a loader that forgot to look.
//!
//! # The version the loader reports
//!
//! `vkEnumerateInstanceVersion` reports what **the loader** supports, not what
//! any driver does; a driver's version is reported by
//! `vkGetPhysicalDeviceProperties`, which is a different question asked of a
//! different object. This loader implements Vulkan 1.0's instance and
//! physical-device commands and nothing from 1.1 or later, so it says 1.0.
//!
//! Saying more would be the same failure as an empty extension list: an
//! application that sees 1.1 is entitled to call `vkGetPhysicalDeviceProperties2`
//! and get a pointer back, and this loader would answer null.
//!
//! # The limit of the extension answer, stated where the promise is made
//!
//! The version paragraph above names a failure this module committed the day it
//! was written, and the honest thing is to keep saying so here rather than let a
//! reader find it: **the loader reported extensions whose commands it could not
//! hand out.** An application that read `VK_KHR_surface` from this union,
//! enabled it, and asked [`crate::entry::get_instance_proc_addr`] for
//! `vkGetPhysicalDeviceSurfaceSupportKHR` got null, because [`crate::physical`]
//! knows exactly the nine core Vulkan 1.0 names and no others.
//!
//! Why it could not be fixed by forwarding is [`crate::physical`]'s argument
//! restated: a `VkPhysicalDevice` this loader hands out is a loader-owned
//! object the driver has never seen, so passing an unknown command straight
//! through would hand a driver a pointer to a `crate::instance::PhysicalDevice`
//! where it expects its own handle. Every command needs its first argument
//! unwrapped, and the loader has no signature for a command it has never heard
//! of.
//!
//! Why it must not be fixed by narrowing this list either: many instance
//! extensions add no commands at all — `VK_KHR_portability_enumeration` is a
//! flag and nothing else — so reporting only what the loader can dispatch would
//! deny an application extensions that need no dispatching, and for the rest it
//! would rebuild exactly the empty-list stub this module exists to avoid.
//!
//! **[`crate::unknown`] closed the physical-device half of it** on the same day,
//! with the generic trampoline the paragraph above predicted: three instructions
//! that swap argument zero and tail-jump, which is what
//! `vk_icdGetPhysicalDeviceProcAddr` exists for and what [`crate::physical`]
//! already asks through first. `vkGetPhysicalDeviceSurfaceSupportKHR` now
//! resolves.
//!
//! **The instance-level half is still open**, and this is still the module that
//! promises it: a command taking a `VkInstance` needs a *fan-out policy* per
//! command — which of several drivers answers — and a policy is not something a
//! trampoline can carry. So `vkDestroySurfaceKHR` and the platform
//! `vkCreate*SurfaceKHR` calls are still null while `VK_KHR_surface` is still
//! reported. It stays filed as
//! `C-VKLOADER-ADVERTISES-EXTENSIONS-WHOSE-ENTRY-POINTS-IT-ANSWERS-NULL-FOR` in
//! `known-issues.md` until it is not.

use crate::vk::{ExtensionProperties, MAX_EXTENSION_NAME_SIZE};
use alloc::vec::Vec;

/// The Vulkan version this loader implements, in the packed form
/// `vkEnumerateInstanceVersion` writes.
///
/// `VK_MAKE_API_VERSION(variant, major, minor, patch)` is
/// `variant << 29 | major << 22 | minor << 12 | patch`. This is
/// `VK_API_VERSION_1_0` — variant 0, major 1, minor 0, patch 0 — written out
/// rather than as a literal so that the shape of the encoding is visible at the
/// one place in the tree that depends on it.
pub const VERSION: u32 = make_api_version(0, 1, 0, 0);

/// `VK_MAKE_API_VERSION` from `vulkan_core.h`.
#[must_use]
pub const fn make_api_version(variant: u32, major: u32, minor: u32, patch: u32) -> u32 {
    (variant << 29) | (major << 22) | (minor << 12) | patch
}

/// One extension in the loader's own terms: a name and the version the loader
/// will report for it.
///
/// Kept separate from [`ExtensionProperties`] so the merging policy below is a
/// function on values that a test can call, rather than something that only
/// happens while writing into a caller's array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// The name, without its NUL and without the padding after it.
    pub name: Vec<u8>,
    /// The version reported for it — see [`merge`] for which one that is when
    /// drivers disagree.
    pub spec_version: u32,
}

impl Extension {
    /// The extension a driver's record describes.
    ///
    /// The name stops at the first NUL, because the bytes after it are
    /// unspecified — a driver is entitled to leave whatever it likes there, and
    /// two drivers naming the same extension with different padding must still
    /// compare equal.
    ///
    /// A record whose 256 bytes contain no NUL at all is a driver bug. The name
    /// is then taken as the whole array, which is the one interpretation that
    /// neither reads past the record nor invents a terminator; the resulting
    /// name simply matches nothing.
    #[must_use]
    pub fn from_record(record: &ExtensionProperties) -> Self {
        let name = record.extension_name;
        let end = name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(MAX_EXTENSION_NAME_SIZE);
        Self {
            name: name.get(..end).unwrap_or(&name).to_vec(),
            spec_version: record.spec_version,
        }
    }

    /// The record to write into a caller's array for this extension.
    ///
    /// The name is padded with NULs to the full 256 bytes rather than left with
    /// whatever was in the caller's memory, so the loader's own output does not
    /// have the ambiguity the paragraph above describes. A name too long for the
    /// array is truncated *and* still terminated, because writing 256 bytes with
    /// no NUL would hand the application a string with no end.
    #[must_use]
    pub fn to_record(&self) -> ExtensionProperties {
        let mut record = ExtensionProperties {
            extension_name: [0; MAX_EXTENSION_NAME_SIZE],
            spec_version: self.spec_version,
        };
        let room = MAX_EXTENSION_NAME_SIZE - 1;
        let copied = self.name.len().min(room);
        if let (Some(destination), Some(source)) = (
            record.extension_name.get_mut(..copied),
            self.name.get(..copied),
        ) {
            destination.copy_from_slice(source);
        }
        record
    }
}

/// The loader's instance-extension list, given what each driver reported.
///
/// # The policy, and it is policy rather than specification
///
/// The Loader–Driver Interface says nothing about either half of this. Both are
/// this loader's choices, stated as such — [`crate::instance`] makes the same
/// distinction for the same reason, and blurring it is how a plausible invention
/// becomes a citation later.
///
/// **Union, not intersection.** An extension one driver offers is reported even
/// if another does not. Intersecting would let a driver the application was
/// never going to use veto a capability its actual GPU has, which is the same
/// failure the "a partial success is a success" rule in [`crate::instance`]
/// exists to avoid. The cost is that enabling it may draw
/// `VK_ERROR_EXTENSION_NOT_PRESENT` out of a driver that lacks it — and that is
/// already handled, because one driver declining does not sink
/// `vkCreateInstance`.
///
/// **The lowest `specVersion`, not the highest.** When two drivers offer the
/// same extension at different versions the reported one is the minimum. The
/// argument is which way the mistake falls, not which number is more accurate:
/// under-reporting makes an application skip a feature it might have had, and
/// over-reporting makes it call an entry point that is not there, having asked
/// first and been told yes. One is a missed optimisation; the other is a crash
/// at a point where the application did everything right.
///
/// The order is the order first seen, not sorted: the list is small, an
/// application looks names up rather than scanning, and preserving the order
/// keeps the answer stable and explicable when reading a log.
#[must_use]
pub fn merge(per_driver: &[Vec<Extension>]) -> Vec<Extension> {
    let mut merged: Vec<Extension> = Vec::new();
    for driver in per_driver {
        for extension in driver {
            if let Some(existing) = merged.iter_mut().find(|e| e.name == extension.name) {
                existing.spec_version = existing.spec_version.min(extension.spec_version);
            } else {
                merged.push(extension.clone());
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::{Extension, VERSION, make_api_version, merge};
    use crate::vk::{ExtensionProperties, MAX_EXTENSION_NAME_SIZE};
    use alloc::vec;
    use alloc::vec::Vec;

    fn ext(name: &[u8], spec_version: u32) -> Extension {
        Extension {
            name: name.to_vec(),
            spec_version,
        }
    }

    #[test]
    fn the_reported_version_is_vulkan_one_zero() {
        // 1 << 22. Spelled as a literal here on purpose: if `make_api_version`
        // and `VERSION` were both wrong in the same way, a test written in terms
        // of `make_api_version` would agree with them.
        assert_eq!(VERSION, 0x0040_0000);
    }

    #[test]
    fn the_version_encoding_puts_each_field_where_the_header_does() {
        assert_eq!(make_api_version(0, 1, 3, 250), (1 << 22) | (3 << 12) | 250);
        assert_eq!(make_api_version(1, 0, 0, 0), 1 << 29);
    }

    #[test]
    fn one_driver_is_reported_unchanged() {
        let one = vec![ext(b"VK_KHR_surface", 25)];
        assert_eq!(merge(core::slice::from_ref(&one)), one);
    }

    #[test]
    fn no_drivers_report_nothing_rather_than_failing() {
        // The empty answer this module argues is honest: it is what the registry
        // said, not a stub.
        let none: Vec<Vec<Extension>> = Vec::new();
        assert!(merge(&none).is_empty());
        assert!(merge(&[Vec::new(), Vec::new()]).is_empty());
    }

    #[test]
    fn the_same_extension_from_two_drivers_appears_once() {
        let merged = merge(&[
            vec![ext(b"VK_KHR_surface", 25)],
            vec![ext(b"VK_KHR_surface", 25)],
        ]);
        assert_eq!(merged, vec![ext(b"VK_KHR_surface", 25)]);
    }

    #[test]
    fn drivers_disagreeing_on_a_version_are_reported_at_the_lowest() {
        // The policy argued in `merge`: the loader's instance is only as capable
        // as its weakest driver, so advertising the higher version would promise
        // something one of the machine's GPUs cannot do.
        let merged = merge(&[
            vec![ext(b"VK_KHR_surface", 27)],
            vec![ext(b"VK_KHR_surface", 25)],
            vec![ext(b"VK_KHR_surface", 26)],
        ]);
        assert_eq!(merged, vec![ext(b"VK_KHR_surface", 25)]);
    }

    #[test]
    fn the_lowest_wins_regardless_of_which_driver_was_seen_first() {
        let ascending = merge(&[
            vec![ext(b"VK_KHR_surface", 25)],
            vec![ext(b"VK_KHR_surface", 27)],
        ]);
        assert_eq!(ascending, vec![ext(b"VK_KHR_surface", 25)]);
    }

    #[test]
    fn distinct_extensions_are_all_kept_in_the_order_first_seen() {
        let merged = merge(&[
            vec![ext(b"VK_KHR_surface", 25), ext(b"VK_EXT_debug_utils", 2)],
            vec![ext(b"VK_KHR_display", 23), ext(b"VK_KHR_surface", 25)],
        ]);
        assert_eq!(
            merged,
            vec![
                ext(b"VK_KHR_surface", 25),
                ext(b"VK_EXT_debug_utils", 2),
                ext(b"VK_KHR_display", 23),
            ],
        );
    }

    #[test]
    fn an_extension_only_one_driver_offers_is_still_reported() {
        // Union rather than intersection: a driver the application was never
        // going to use does not get to veto a capability its actual GPU has.
        let merged = merge(&[
            vec![ext(b"VK_KHR_surface", 25)],
            vec![ext(b"VK_KHR_display", 23)],
        ]);
        assert_eq!(
            merged,
            vec![ext(b"VK_KHR_surface", 25), ext(b"VK_KHR_display", 23)],
        );
    }

    #[test]
    fn a_driver_that_lacks_an_extension_does_not_drag_its_version_down() {
        // The minimum is over the drivers that *have* it. A driver reporting
        // nothing contributes nothing, rather than counting as version zero —
        // which would report every extension at 0 the moment one driver was
        // silent.
        let merged = merge(&[
            Vec::new(),
            vec![ext(b"VK_KHR_surface", 25)],
            Vec::new(),
            vec![ext(b"VK_KHR_surface", 27)],
        ]);
        assert_eq!(merged, vec![ext(b"VK_KHR_surface", 25)]);
    }

    #[test]
    fn a_name_that_is_a_prefix_of_another_is_a_different_extension() {
        // Comparison is over the whole name, not a prefix — the failure mode
        // that would silently drop an extension.
        let merged = merge(&[vec![ext(b"VK_KHR_surface", 25), ext(b"VK_KHR_surf", 1)]]);
        assert_eq!(merged.len(), 2);
    }

    /// A driver's record, with `filler` in every byte after the NUL — which a
    /// driver is entitled to leave as anything at all.
    fn record(name: &[u8], spec_version: u32, filler: u8) -> ExtensionProperties {
        let mut extension_name = [filler; MAX_EXTENSION_NAME_SIZE];
        if let Some(destination) = extension_name.get_mut(..name.len()) {
            destination.copy_from_slice(name);
        }
        if let Some(byte) = extension_name.get_mut(name.len()) {
            *byte = 0;
        }
        ExtensionProperties {
            extension_name,
            spec_version,
        }
    }

    #[test]
    fn a_name_is_read_up_to_its_nul_and_no_further() {
        let read = Extension::from_record(&record(b"VK_KHR_surface", 25, 0xAA));
        assert_eq!(read, ext(b"VK_KHR_surface", 25));
    }

    #[test]
    fn two_drivers_padding_a_name_differently_still_report_one_extension() {
        // The reason `from_record` stops at the NUL rather than comparing the
        // whole 256 bytes: the padding is unspecified, so byte-equal records are
        // not what "the same extension" means.
        let one = Extension::from_record(&record(b"VK_KHR_surface", 25, 0x00));
        let two = Extension::from_record(&record(b"VK_KHR_surface", 25, 0xFF));
        assert_eq!(merge(&[vec![one], vec![two]]).len(), 1);
    }

    #[test]
    fn a_record_with_no_terminator_is_not_read_past_and_matches_nothing() {
        let unterminated = ExtensionProperties {
            extension_name: [b'x'; MAX_EXTENSION_NAME_SIZE],
            spec_version: 1,
        };
        let read = Extension::from_record(&unterminated);
        assert_eq!(read.name.len(), MAX_EXTENSION_NAME_SIZE);
        assert_ne!(read.name, b"VK_KHR_surface".to_vec());
    }

    #[test]
    fn a_record_the_loader_writes_reads_back_as_what_it_wrote() {
        let original = ext(b"VK_KHR_surface", 25);
        assert_eq!(Extension::from_record(&original.to_record()), original);
    }

    #[test]
    fn a_record_the_loader_writes_is_nul_padded_rather_than_left_as_it_found_it() {
        // An application is entitled to `strcmp` the array. Padding it with the
        // caller's previous contents would make two identical extensions compare
        // unequal to anything that looked past the NUL.
        let written = ext(b"VK_KHR_surface", 25).to_record();
        assert!(
            written
                .extension_name
                .get(b"VK_KHR_surface".len()..)
                .is_some_and(|tail| tail.iter().all(|&b| b == 0)),
            "the tail of a written name is not zeroed",
        );
    }

    #[test]
    fn a_name_too_long_for_the_array_is_still_terminated() {
        // Truncating without terminating would hand the application a string
        // with no end, which is worse than the truncation it is meant to survive.
        let long = Extension {
            name: vec![b'x'; MAX_EXTENSION_NAME_SIZE * 2],
            spec_version: 1,
        };
        let written = long.to_record();
        assert_eq!(
            written.extension_name.last(),
            Some(&0),
            "a truncated name ran to the end of the array with no NUL",
        );
        assert_eq!(
            Extension::from_record(&written).name.len(),
            MAX_EXTENSION_NAME_SIZE - 1,
        );
    }
}
