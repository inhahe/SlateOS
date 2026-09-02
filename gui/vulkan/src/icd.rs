//! The contract between the loader and a driver.
//!
//! Vulkan calls a driver an **ICD** — "installable client driver" — and the
//! loader's first job on meeting one is to agree which revision of this
//! contract both sides speak. That agreement decides real things: whether the
//! loader may ask the driver to create window surfaces, whether it may look up
//! physical-device entry points through a dedicated function, and whether it
//! is the loader or the driver that rejects an application asking for a Vulkan
//! version the driver does not implement.
//!
//! ## Why the version is not kept as a number
//!
//! Every one of those questions is a `>=` against the negotiated number, and
//! in a loader they get asked from dozens of places. Storing a bare [`u32`] and
//! comparing at each site is how the reference loader has historically grown
//! off-by-one bugs — the comparison is written out fresh each time, and one
//! written `> 3` instead of `>= 3` is invisible in review because both look
//! like a version check.
//!
//! So [`InterfaceVersion`] answers questions instead of exposing a number:
//! [`may_manage_surfaces`](InterfaceVersion::may_manage_surfaces),
//! [`has_physical_device_proc_addr`](InterfaceVersion::has_physical_device_proc_addr),
//! and so on. The threshold for each capability is written down exactly once,
//! next to the sentence from the specification that fixes it.
//!
//! ## The versions, and what each one changed
//!
//! From the Vulkan-Loader `LoaderDriverInterface.md`:
//!
//! | Version | What it added |
//! |---|---|
//! | 0 | The baseline. A driver exports `vkGetInstanceProcAddr`, `vkCreateInstance` and `vkEnumerateInstanceExtensionProperties`, and nothing else is assumed. |
//! | 1 | `vk_icdGetInstanceProcAddr`. There is still no handshake, so the loader infers the version from whether that symbol is present. |
//! | 2 | `vk_icdNegotiateLoaderICDInterfaceVersion` — the handshake itself — and the dispatch-table rules in [`crate::dispatch`]. |
//! | 3 | The driver may own `VkSurfaceKHR` itself, if it implements every surface entry point. Otherwise the loader creates the surface. |
//! | 4 | `vk_icdGetPhysicalDeviceProcAddr`, for physical-device entry points belonging to extensions the loader has never heard of. |
//! | 5 | The **loader** validates the application's requested `apiVersion`; from here on a driver must not reject an instance for that reason. |
//! | 6 | `vk_icdEnumerateAdapterPhysicalDevices`, so Windows can report GPUs in the order the platform wants. |
//! | 7 | Entry points may be reached through `vk_icdGetInstanceProcAddr` rather than exported as symbols in their own right. |
//!
//! Note that 4, not 3, is where `vk_icdGetPhysicalDeviceProcAddr` arrives.
//! That is easy to misremember, because 3 is the version that starts talking
//! about physical devices at all, and getting it wrong means calling a
//! function pointer a version-3 driver never promised to provide.
//!
//! ## Separating the policy from the call
//!
//! Actually invoking a driver's negotiation function means calling a raw
//! pointer supplied by a third party, which is `unsafe` and cannot be unit
//! tested. The *decision* made from what it returns is neither, so it lives
//! here as [`settle`] — a pure function over a [`DriverReply`]. Every rule
//! below, including the clamping ones that exist only because drivers get
//! this wrong, is exercised by the tests at the bottom of this file with no
//! driver present.

/// The revision of the loader/driver contract this loader implements.
///
/// `CURRENT_LOADER_ICD_INTERFACE_VERSION` in `vk_icd.h`.
pub const CURRENT: InterfaceVersion = InterfaceVersion(7);

/// The oldest revision this loader will still talk to.
///
/// `MIN_SUPPORTED_LOADER_ICD_INTERFACE_VERSION` in `vk_icd.h`, which is 0 —
/// the loader is expected to cope with a driver that predates the handshake
/// entirely. It is named rather than written as a literal `0` because the
/// comparison against it in [`settle`] would otherwise look like a tautology
/// and invite deletion.
pub const MIN_SUPPORTED: InterfaceVersion = InterfaceVersion(0);

/// A revision of the loader/driver contract.
///
/// Construct one with [`InterfaceVersion::new`], or use [`CURRENT`] /
/// [`MIN_SUPPORTED`]. The inner number is deliberately private: see the module
/// documentation for why call sites ask capability questions instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceVersion(u32);

impl InterfaceVersion {
    /// Wrap a raw version number, as read from a driver.
    #[must_use]
    pub const fn new(v: u32) -> Self {
        Self(v)
    }

    /// The raw number, for logging and for writing back out over the ABI.
    ///
    /// Prefer a capability query for anything that makes a decision.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Does the driver export `vk_icdGetInstanceProcAddr`? (version 1)
    ///
    /// A version-0 driver does not, and the loader must fall back to the
    /// plain `vkGetInstanceProcAddr` symbol.
    #[must_use]
    pub const fn exports_icd_get_instance_proc_addr(self) -> bool {
        self.0 >= 1
    }

    /// Does the driver export the handshake itself? (version 2)
    ///
    /// Only ever true of a version already *obtained* by a handshake, so this
    /// is a statement about a settled version rather than a question to ask
    /// before negotiating — for that, see [`DriverReply::NoNegotiationFunction`].
    #[must_use]
    pub const fn negotiates(self) -> bool {
        self.0 >= 2
    }

    /// May the driver own `VkSurfaceKHR` objects itself? (version 3)
    ///
    /// Permission, not obligation: a version-3 driver takes over surfaces only
    /// if it implements *every* surface entry point, so the loader still has
    /// to check before handing them over.
    #[must_use]
    pub const fn may_manage_surfaces(self) -> bool {
        self.0 >= 3
    }

    /// Does the driver export `vk_icdGetPhysicalDeviceProcAddr`? (version 4)
    ///
    /// Four, not three. See the table in the module documentation.
    #[must_use]
    pub const fn has_physical_device_proc_addr(self) -> bool {
        self.0 >= 4
    }

    /// Is the *loader* responsible for rejecting an unsupported `apiVersion`?
    /// (version 5)
    ///
    /// From version 5 the driver must not fail `vkCreateInstance` because the
    /// application asked for a Vulkan version it does not implement — the
    /// loader has already checked. Below 5 the loader must leave the check to
    /// the driver, and treat its refusal as a normal outcome rather than a
    /// fault.
    #[must_use]
    pub const fn loader_validates_api_version(self) -> bool {
        self.0 >= 5
    }

    /// Does the driver export `vk_icdEnumerateAdapterPhysicalDevices`?
    /// (version 6)
    ///
    /// Windows-only in practice — it exists so physical devices can be
    /// reported in the order the platform's adapter enumeration gives, which
    /// is what decides which GPU an application calls "GPU 0".
    #[must_use]
    pub const fn enumerates_adapters(self) -> bool {
        self.0 >= 6
    }

    /// May entry points be reached only through `vk_icdGetInstanceProcAddr`,
    /// rather than exported as symbols? (version 7)
    ///
    /// Below 7 the loader may look a function up as a symbol and conclude from
    /// its absence that the driver does not implement it. At 7 that inference
    /// is invalid.
    #[must_use]
    pub const fn entry_points_may_be_unexported(self) -> bool {
        self.0 >= 7
    }
}

/// What a driver did when the loader tried to negotiate with it.
///
/// This is the whole of what [`settle`] needs to know, which is the point:
/// producing one of these is the only part that requires calling into a
/// driver, so it is the only part that cannot be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverReply {
    /// The driver exports no `vk_icdNegotiateLoaderICDInterfaceVersion`.
    ///
    /// It predates the handshake, so its version is inferred from whether it
    /// exports `vk_icdGetInstanceProcAddr`: version 1 if it does, 0 if not.
    NoNegotiationFunction {
        /// Whether `vk_icdGetInstanceProcAddr` resolved as a symbol.
        exports_icd_get_instance_proc_addr: bool,
    },
    /// The handshake returned `VK_SUCCESS`, leaving this in `*pSupportedVersion`.
    Success {
        /// What the driver wrote back. Not necessarily sane — see [`settle`].
        reported: u32,
    },
    /// The handshake returned `VK_ERROR_INCOMPATIBLE_DRIVER`.
    ///
    /// The specification gives this one meaning and only one: the driver
    /// cannot work with a loader of the proposed version. It is not an error
    /// to report to the application; it means skip this driver.
    IncompatibleDriver,
    /// The handshake returned some other `VkResult`.
    ///
    /// Undefined by the specification, and therefore not something to guess
    /// about. Carried as the raw code so a log can name it.
    Failed(i32),
}

/// The result of settling a version with one driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Negotiation {
    /// Both sides ran the handshake and agreed on this version.
    Agreed(InterfaceVersion),
    /// The driver predates the handshake; this version was inferred from
    /// which symbols it exports.
    Assumed(InterfaceVersion),
    /// This driver cannot be used, for the reason given.
    Unusable(Unusable),
}

impl Negotiation {
    /// The settled version, if the driver is usable at all.
    #[must_use]
    pub const fn version(self) -> Option<InterfaceVersion> {
        match self {
            Self::Agreed(v) | Self::Assumed(v) => Some(v),
            Self::Unusable(_) => None,
        }
    }
}

/// Why a driver was rejected.
///
/// Kept as an enum rather than folded into one "incompatible" case because
/// these have genuinely different operational meanings: the first is a driver
/// doing exactly what it should, and the last two are a driver misbehaving.
/// A machine with no working graphics is diagnosed very differently depending
/// on which of them every installed driver produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unusable {
    /// The driver said so itself, via `VK_ERROR_INCOMPATIBLE_DRIVER`. Normal.
    DeclaredIncompatible,
    /// The driver settled below this loader's floor.
    TooOld {
        /// What it offered.
        offered: u32,
        /// The lowest this loader accepts.
        floor: u32,
    },
    /// The handshake failed with a result the specification does not define
    /// for it.
    HandshakeFailed(i32),
}

/// Decide the interface version to use with one driver.
///
/// `loader` is the version this loader proposed — normally [`CURRENT`], but
/// taken as a parameter so the rules can be tested at other values rather than
/// only at whatever today's constant happens to be.
///
/// ## The two clamps, and why both are needed
///
/// The specification has the driver clamp downward: it receives the loader's
/// proposal and returns something no higher. Drivers nonetheless return higher
/// values — the reference loader clamps defensively for this reason — and the
/// consequence of trusting one is not a cosmetic version mismatch. Believing a
/// version-2 driver when it claims 7 means calling
/// `vk_icdGetPhysicalDeviceProcAddr` on a driver that never exported it, which
/// is a jump through a null or stale pointer inside somebody else's shared
/// library. So the loader clamps too, and this is the only place it does.
///
/// The other direction is the floor: a driver that settles below
/// [`MIN_SUPPORTED`] is unusable. With `MIN_SUPPORTED` at 0 that is currently
/// unreachable — the comparison is kept because the floor is a policy that has
/// moved before and will move again, and a floor enforced nowhere is a floor
/// that silently stops existing.
///
/// ## The result is a minimum, on every path
///
/// Whatever the reply, a usable outcome is `min(what the driver can do, what
/// the loader can do)` — including the inferred path, where the driver never
/// spoke. That is easy to get wrong there, because the inferred value looks
/// like a fact about the driver rather than a negotiation, and 1 is below
/// every plausible `loader` so the bug hides. It is still a bug: a settled
/// version is the loader's licence to *use* a capability, and a loader must
/// not licence itself past its own implementation just because the driver
/// would have allowed it. So the clamp is applied on both paths, and the
/// property test drives both.
#[must_use]
pub fn settle(loader: InterfaceVersion, reply: DriverReply) -> Negotiation {
    match reply {
        DriverReply::NoNegotiationFunction {
            exports_icd_get_instance_proc_addr,
        } => {
            // No handshake exists, so the version is whatever the driver's
            // symbol table implies. This is the only inference the loader is
            // permitted to make about a version, and it is permitted only
            // because versions 0 and 1 are exactly distinguished by it.
            let implied = if exports_icd_get_instance_proc_addr {
                1
            } else {
                0
            };
            Negotiation::Assumed(InterfaceVersion(implied.min(loader.0)))
        }
        DriverReply::Success { reported } => {
            // Compared as versions rather than as the integers behind them.
            // That is not only tidier: `clamped < MIN_SUPPORTED.0` on the raw
            // `u32`s is, with today's floor of 0, a comparison clippy can
            // prove is always false, and it is right to say so. The floor is
            // real policy that has moved before and will move again, so the
            // answer is to state it in the vocabulary where it means
            // something — `Ord` on `InterfaceVersion` — rather than to keep a
            // provably-dead integer comparison alive behind an `allow`.
            let clamped = InterfaceVersion(reported.min(loader.0));
            if clamped < MIN_SUPPORTED {
                Negotiation::Unusable(Unusable::TooOld {
                    offered: clamped.0,
                    floor: MIN_SUPPORTED.0,
                })
            } else {
                Negotiation::Agreed(clamped)
            }
        }
        DriverReply::IncompatibleDriver => Negotiation::Unusable(Unusable::DeclaredIncompatible),
        DriverReply::Failed(code) => Negotiation::Unusable(Unusable::HandshakeFailed(code)),
    }
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
        CURRENT, DriverReply, InterfaceVersion, MIN_SUPPORTED, Negotiation, Unusable, settle,
    };

    /// `VK_ERROR_INCOMPATIBLE_DRIVER`, for the `Failed` cases below to not
    /// accidentally coincide with.
    const VK_ERROR_INCOMPATIBLE_DRIVER: i32 = -9;

    #[test]
    fn the_constants_match_the_khronos_header() {
        // vk_icd.h: CURRENT_LOADER_ICD_INTERFACE_VERSION 7,
        //           MIN_SUPPORTED_LOADER_ICD_INTERFACE_VERSION 0.
        // Spelled out here so that raising CURRENT is a deliberate act that
        // updates a test, rather than a one-character edit nothing notices.
        assert_eq!(CURRENT.get(), 7);
        assert_eq!(MIN_SUPPORTED.get(), 0);
    }

    #[test]
    fn each_capability_appears_at_the_version_the_specification_says() {
        // The whole table, asserted at the boundary on both sides. A test that
        // only checked the "yes" side would pass for a capability that is
        // enabled from version 0.
        let at = InterfaceVersion::new;

        assert!(!at(0).exports_icd_get_instance_proc_addr());
        assert!(at(1).exports_icd_get_instance_proc_addr());

        assert!(!at(1).negotiates());
        assert!(at(2).negotiates());

        assert!(!at(2).may_manage_surfaces());
        assert!(at(3).may_manage_surfaces());

        // The one that is easy to misremember as 3.
        assert!(!at(3).has_physical_device_proc_addr());
        assert!(at(4).has_physical_device_proc_addr());

        assert!(!at(4).loader_validates_api_version());
        assert!(at(5).loader_validates_api_version());

        assert!(!at(5).enumerates_adapters());
        assert!(at(6).enumerates_adapters());

        assert!(!at(6).entry_points_may_be_unexported());
        assert!(at(7).entry_points_may_be_unexported());
    }

    #[test]
    fn capabilities_are_monotonic_up_to_the_current_version() {
        // Once a capability exists it never goes away again. Stated as a
        // property because the alternative -- one assertion per version per
        // capability -- is the sort of table that gets half-updated.
        type Query = (&'static str, fn(InterfaceVersion) -> bool);
        let queries: [Query; 7] = [
            (
                "exports_icd_gipa",
                InterfaceVersion::exports_icd_get_instance_proc_addr,
            ),
            ("negotiates", InterfaceVersion::negotiates),
            ("may_manage_surfaces", InterfaceVersion::may_manage_surfaces),
            (
                "has_pd_proc_addr",
                InterfaceVersion::has_physical_device_proc_addr,
            ),
            (
                "validates_api_version",
                InterfaceVersion::loader_validates_api_version,
            ),
            ("enumerates_adapters", InterfaceVersion::enumerates_adapters),
            (
                "unexported_entry_points",
                InterfaceVersion::entry_points_may_be_unexported,
            ),
        ];
        for (name, q) in queries {
            let mut seen_true = false;
            for v in 0..=CURRENT.get() {
                let now = q(InterfaceVersion::new(v));
                if now {
                    seen_true = true;
                } else {
                    assert!(
                        !seen_true,
                        "{name} was true before version {v} and is false at it"
                    );
                }
            }
            assert!(
                seen_true,
                "{name} is never true, up to version {}",
                CURRENT.get()
            );
        }
    }

    #[test]
    fn a_driver_with_no_handshake_is_version_one_if_it_exports_the_icd_entry_point() {
        let reply = DriverReply::NoNegotiationFunction {
            exports_icd_get_instance_proc_addr: true,
        };
        assert_eq!(
            settle(CURRENT, reply),
            Negotiation::Assumed(InterfaceVersion::new(1))
        );
    }

    #[test]
    fn a_driver_with_neither_the_handshake_nor_the_icd_entry_point_is_version_zero() {
        let reply = DriverReply::NoNegotiationFunction {
            exports_icd_get_instance_proc_addr: false,
        };
        let settled = settle(CURRENT, reply);
        assert_eq!(settled, Negotiation::Assumed(InterfaceVersion::new(0)));
        // The consequence that matters: nothing may be called on it beyond the
        // three version-0 symbols.
        let v = settled
            .version()
            .expect("version 0 is usable, just limited");
        assert!(!v.exports_icd_get_instance_proc_addr());
        assert!(!v.has_physical_device_proc_addr());
    }

    #[test]
    fn a_driver_that_settles_lower_than_the_loader_wins() {
        // The ordinary case, and the one the specification describes: the
        // driver clamps.
        let settled = settle(CURRENT, DriverReply::Success { reported: 3 });
        assert_eq!(settled, Negotiation::Agreed(InterfaceVersion::new(3)));
    }

    #[test]
    fn a_driver_claiming_more_than_the_loader_offered_is_clamped_to_the_offer() {
        // The defensive clamp. Believing this driver would mean calling
        // vk_icdGetPhysicalDeviceProcAddr on something that may never have
        // exported it.
        let settled = settle(
            InterfaceVersion::new(2),
            DriverReply::Success { reported: 7 },
        );
        assert_eq!(settled, Negotiation::Agreed(InterfaceVersion::new(2)));

        let v = settled.version().expect("a clamped driver is still usable");
        assert!(
            !v.has_physical_device_proc_addr(),
            "the clamp must actually withhold the capability, not just lower a number"
        );
    }

    #[test]
    fn a_driver_that_declares_itself_incompatible_is_skipped_not_reported() {
        let settled = settle(CURRENT, DriverReply::IncompatibleDriver);
        assert_eq!(
            settled,
            Negotiation::Unusable(Unusable::DeclaredIncompatible)
        );
        assert!(settled.version().is_none());
    }

    #[test]
    fn an_undefined_handshake_failure_keeps_its_code_and_is_not_confused_with_incompatibility() {
        // A driver returning VK_ERROR_OUT_OF_HOST_MEMORY from the handshake is
        // doing something the specification does not describe. It must not be
        // silently folded into the ordinary "skip this driver" case, because
        // the two want different diagnoses on a machine with no graphics.
        let settled = settle(CURRENT, DriverReply::Failed(-1));
        assert_eq!(
            settled,
            Negotiation::Unusable(Unusable::HandshakeFailed(-1))
        );
        assert_ne!(
            settled,
            Negotiation::Unusable(Unusable::DeclaredIncompatible)
        );
    }

    #[test]
    fn incompatibility_reported_as_a_failure_code_is_still_distinguishable() {
        // Guards the mapping at the call boundary: whoever builds a
        // DriverReply must map VK_ERROR_INCOMPATIBLE_DRIVER to the dedicated
        // variant rather than to Failed(-9). This test does not enforce that
        // -- it cannot, from here -- but it records that the two are different
        // values so the distinction is not quietly dropped later.
        let declared = settle(CURRENT, DriverReply::IncompatibleDriver);
        let as_code = settle(CURRENT, DriverReply::Failed(VK_ERROR_INCOMPATIBLE_DRIVER));
        assert_ne!(declared, as_code);
    }

    #[test]
    fn settling_never_exceeds_what_the_loader_proposed() {
        // The property the clamp exists for, over the whole range plus some
        // nonsense above it.
        for loader in 0..=CURRENT.get() {
            for reported in [0, 1, 2, 3, 4, 5, 6, 7, 8, 99, u32::MAX] {
                let settled = settle(
                    InterfaceVersion::new(loader),
                    DriverReply::Success { reported },
                );
                let got = settled
                    .version()
                    .expect("Success always settles usably today");
                assert!(
                    got.get() <= loader,
                    "loader offered {loader}, driver claimed {reported}, settled {}",
                    got.get()
                );
                assert!(
                    got.get() <= reported,
                    "settled above what the driver reported"
                );
            }
        }
    }

    #[test]
    fn the_inferred_version_is_clamped_to_the_loader_too() {
        // The trap this closes: a driver with no handshake but with the
        // version-1 entry point is "version 1" as a fact about the driver, so
        // it is tempting to return 1 unconditionally. A loader that only
        // implements version 0 would then hold a licence to call
        // `vk_icdGetInstanceProcAddr` through paths it does not have. The
        // clamp is invisible at today's CURRENT of 7, which is exactly why it
        // needs its own test rather than relying on the range sweep.
        let settled = settle(
            InterfaceVersion::new(0),
            DriverReply::NoNegotiationFunction {
                exports_icd_get_instance_proc_addr: true,
            },
        );
        assert_eq!(
            settled,
            Negotiation::Assumed(InterfaceVersion::new(0)),
            "a version-0 loader inferred a version-1 driver and kept the 1"
        );
    }

    #[test]
    fn no_reply_of_any_shape_settles_above_the_loader() {
        // The same property as `settling_never_exceeds_what_the_loader_proposed`,
        // but over every *variant* rather than every value of one variant --
        // which is what catches a new reply shape being added with its own
        // unclamped path.
        for loader in 0..=CURRENT.get() {
            let replies = [
                DriverReply::NoNegotiationFunction {
                    exports_icd_get_instance_proc_addr: false,
                },
                DriverReply::NoNegotiationFunction {
                    exports_icd_get_instance_proc_addr: true,
                },
                DriverReply::Success { reported: 0 },
                DriverReply::Success { reported: 7 },
                DriverReply::Success { reported: u32::MAX },
                DriverReply::IncompatibleDriver,
                DriverReply::Failed(-9),
            ];
            for reply in replies {
                if let Some(got) = settle(InterfaceVersion::new(loader), reply).version() {
                    assert!(
                        got.get() <= loader,
                        "loader offered {loader}, {reply:?} settled {}",
                        got.get()
                    );
                }
            }
        }
    }
}
