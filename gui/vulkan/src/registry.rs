//! The set of drivers this loader knows about, and what each of them agreed to.
//!
//! # What a registry is
//!
//! A machine may have no graphics drivers, one, or several — an integrated GPU
//! and a discrete card, or a real driver plus a software rasteriser. The
//! registry is the loader's record of which ones exist, what interface version
//! each settled on, and — just as importantly — which ones were *rejected* and
//! why.
//!
//! The rejections are kept rather than discarded because "no Vulkan devices
//! found" is otherwise an unanswerable complaint. A driver that declared
//! itself incompatible, a driver whose handshake failed with an unrecognised
//! code, and a driver that was never registered in the first place all produce
//! the same empty device list, and they call for three different fixes. The
//! registry can tell them apart, so it does.
//!
//! # Registration, not discovery
//!
//! Drivers are handed to the registry by whoever linked them in, rather than
//! found on disk. `posix::dlfcn::dlopen` on SlateOS returns null with the
//! message `"dynamic linking not supported"`, so manifest-scanning discovery
//! would find nothing on every machine while looking like it worked. See the
//! crate documentation for how that changes when dynamic linking lands: it
//! adds a step *before* this module, and changes nothing inside it.
//!
//! # The version gate
//!
//! The reason a settled version is stored per driver rather than computed once
//! for the loader is that it is a *permission*, and it differs per driver. A
//! driver that settled at 1 has an `vk_icdGetInstanceProcAddr` the loader may
//! call; one that settled at 0 does not, even if it happens to have offered
//! the symbol. A driver that settled at 4 has a
//! `vk_icdGetPhysicalDeviceProcAddr`; one that settled at 3 does not — and 3
//! is the version people misremember as the boundary, this author included
//! (see `design-decisions.md` §577).
//!
//! So [`Driver`] does not expose the entry points it was given. It exposes
//! [`Driver::instance_proc_addr`] and [`Driver::physical_device_proc_addr`],
//! which apply the gate. A caller that never sees the ungated pointer cannot
//! call it too early.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::icd::{self, DriverReply, InterfaceVersion, Negotiation, Unusable};
use crate::unknown::{Slots, Table};
use crate::vk::{
    GetInstanceProcAddrFn, GetPhysicalDeviceProcAddrFn, NegotiateFn, VK_ERROR_INCOMPATIBLE_DRIVER,
    VK_SUCCESS, VkResult,
};

/// The entry points a driver offers the loader when it registers.
///
/// Which of them the loader is then *allowed* to use depends on the version
/// the handshake settles on — offering a symbol is not the same as being
/// entitled to have it called. [`Driver`] applies that rule; this struct is
/// only the raw offer.
#[derive(Clone, Copy)]
pub struct Entry {
    /// `vkGetInstanceProcAddr`. Every driver has one at every interface
    /// version, which is why this is the only field that is not an `Option`.
    pub get_instance_proc_addr: GetInstanceProcAddrFn,

    /// `vk_icdGetInstanceProcAddr`, added at interface version 1.
    ///
    /// Its *presence* is also the loader's only way to tell version 1 from
    /// version 0 in a driver that has no handshake function, which is why
    /// [`handshake`] reports it even when it does not call it.
    pub icd_get_instance_proc_addr: Option<GetInstanceProcAddrFn>,

    /// `vk_icdGetPhysicalDeviceProcAddr`, added at interface version 4.
    pub get_physical_device_proc_addr: Option<GetPhysicalDeviceProcAddrFn>,

    /// `vk_icdNegotiateLoaderICDInterfaceVersion`, added at interface
    /// version 2. Absent means the version has to be inferred.
    pub negotiate: Option<NegotiateFn>,
}

/// A driver the registry accepted, together with the version it settled on.
pub struct Driver {
    name: &'static str,
    entry: Entry,
    version: InterfaceVersion,
    /// This driver's answers for the extension commands the loader forwards
    /// without knowing their signatures — see [`crate::unknown`].
    ///
    /// Boxed because its *address* is copied into every physical-device wrapper
    /// enumerated from this driver, and lives in those wrappers for as long as
    /// the application holds them. The `Driver` itself is an element of a `Vec`
    /// that moves whenever another driver registers; a `Box`'s contents do not
    /// move when the box does, which is what makes the copied address stay
    /// valid. Storing the table inline here would leave every wrapper pointing
    /// into a freed buffer the moment a second driver appeared.
    ext: Box<Table>,
}

impl Driver {
    /// The name this driver registered under, for diagnostics.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The interface version the handshake settled on.
    #[must_use]
    pub const fn version(&self) -> InterfaceVersion {
        self.version
    }

    /// The entry point to use for instance-level lookups against this driver.
    ///
    /// From interface version 1 a driver may have a `vk_icdGetInstanceProcAddr`
    /// that answers for entry points its `vkGetInstanceProcAddr` will not. Below
    /// that version the loader must use the plain one **even if the driver
    /// offered the other symbol**, because a version-0 driver's export of that
    /// name is not the version-1 contract — it is a coincidence of naming, and
    /// the loader has no promise about what it does.
    #[must_use]
    pub const fn instance_proc_addr(&self) -> GetInstanceProcAddrFn {
        match self.entry.icd_get_instance_proc_addr {
            Some(icd) if self.version.exports_icd_get_instance_proc_addr() => icd,
            _ => self.entry.get_instance_proc_addr,
        }
    }

    /// This driver's `vk_icdGetPhysicalDeviceProcAddr`, or `None` if it has
    /// none *or* has not settled high enough to be allowed one.
    ///
    /// The two are folded into one `None` deliberately: the caller's correct
    /// response is identical — fall back to instance-level lookup — and
    /// distinguishing them would only create an opportunity to handle the
    /// second case wrongly.
    #[must_use]
    pub const fn physical_device_proc_addr(&self) -> Option<GetPhysicalDeviceProcAddrFn> {
        if self.version.has_physical_device_proc_addr() {
            self.entry.get_physical_device_proc_addr
        } else {
            None
        }
    }

    /// This driver's table of unknown-extension entry points.
    #[must_use]
    pub fn ext(&self) -> &Table {
        &self.ext
    }
}

/// A driver that was offered to the registry and not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejection {
    /// The name it was offered under.
    pub name: &'static str,
    /// Why it was not accepted.
    pub why: Unusable,
}

/// The outcome of offering one driver to the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Accepted, at this interface version.
    Accepted(InterfaceVersion),
    /// Not accepted, for this reason. Also recorded in
    /// [`Registry::rejected`].
    Rejected(Unusable),
}

/// Perform a driver's interface-version handshake.
///
/// This is the whole of the loader's FFI for negotiation, and it does no
/// policy: it turns a call into a [`DriverReply`], and [`icd::settle`] decides
/// what that reply means. The split is what lets the interesting cases — a
/// driver that over-claims, one that refuses, one that fails with a code from
/// an extension nobody here has heard of — be tested without a driver that
/// behaves that way.
///
/// A driver with no negotiation function is not an error: interface versions 0
/// and 1 predate the function, and are distinguished by whether
/// `vk_icdGetInstanceProcAddr` is present.
///
/// # Safety
///
/// If `entry.negotiate` is `Some`, it must be a live function pointer with the
/// signature of `vk_icdNegotiateLoaderICDInterfaceVersion`, safe to call now
/// and from this thread.
#[must_use]
pub unsafe fn handshake(loader: InterfaceVersion, entry: &Entry) -> DriverReply {
    let Some(negotiate) = entry.negotiate else {
        return DriverReply::NoNegotiationFunction {
            exports_icd_get_instance_proc_addr: entry.icd_get_instance_proc_addr.is_some(),
        };
    };

    // The same word carries the proposal in and the answer out, so the
    // proposal is destroyed by the call. That is why `settle` is given the
    // loader's version separately rather than being expected to recover it.
    let mut version = loader.get();
    // SAFETY: the caller guarantees `negotiate` is callable, and `&raw mut
    // version` is a writable, initialised `u32` that lives across the call.
    let result: VkResult = unsafe { negotiate(&raw mut version) };

    match result {
        VK_SUCCESS => DriverReply::Success { reported: version },
        VK_ERROR_INCOMPATIBLE_DRIVER => DriverReply::IncompatibleDriver,
        other => DriverReply::Failed(other),
    }
}

/// Every driver the loader knows about.
pub struct Registry {
    loader: InterfaceVersion,
    drivers: Vec<Driver>,
    rejected: Vec<Rejection>,
    /// Which trampoline slot each unknown extension command was given.
    ///
    /// Process-wide rather than per-driver, because the address handed to the
    /// application is one address for all drivers: a name has to mean the same
    /// slot whichever driver's device it is later called on. It lives here
    /// because it is written in the same breath as the per-driver tables, under
    /// the same lock, and a second lock over the two halves of one operation is
    /// a deadlock waiting for an ordering mistake.
    slots: Slots,
}

impl Registry {
    /// An empty registry that will propose `loader` to each driver.
    #[must_use]
    pub const fn new(loader: InterfaceVersion) -> Self {
        Self {
            loader,
            drivers: Vec::new(),
            rejected: Vec::new(),
            slots: Slots::new(),
        }
    }

    /// The name-to-slot assignment for unknown extension commands.
    #[must_use]
    pub const fn slots(&self) -> &Slots {
        &self.slots
    }

    /// The slot for `name`, assigning a fresh one if it has none.
    ///
    /// `None` means the pool is exhausted, which reaches the application as the
    /// null `vkGetInstanceProcAddr` returns for a command it does not have. See
    /// [`crate::unknown`] for why a slot is never taken back to make room.
    pub fn slot_for(&mut self, name: &[u8]) -> Option<usize> {
        self.slots.assign(name)
    }

    /// The interface version this registry proposes to drivers.
    #[must_use]
    pub const fn proposes(&self) -> InterfaceVersion {
        self.loader
    }

    /// Add a driver whose handshake has already been performed.
    ///
    /// This is the half of registration that decides things, kept callable
    /// without a driver so that every outcome can be tested. [`Registry::register`]
    /// is the same thing with the call in front of it.
    pub fn admit(&mut self, name: &'static str, entry: Entry, reply: DriverReply) -> Admission {
        match icd::settle(self.loader, reply) {
            Negotiation::Agreed(version) | Negotiation::Assumed(version) => {
                self.drivers.push(Driver {
                    name,
                    entry,
                    version,
                    ext: Table::new(),
                });
                Admission::Accepted(version)
            }
            Negotiation::Unusable(why) => {
                self.rejected.push(Rejection { name, why });
                Admission::Rejected(why)
            }
        }
    }

    /// Handshake with a driver and add it if it is usable.
    ///
    /// # Safety
    ///
    /// `entry`'s function pointers must all be live and callable — see
    /// [`handshake`] — and must remain so for as long as this registry does,
    /// since the accepted ones are stored and called later.
    pub unsafe fn register(&mut self, name: &'static str, entry: Entry) -> Admission {
        // SAFETY: forwarded from this function's own contract.
        let reply = unsafe { handshake(self.loader, &entry) };
        self.admit(name, entry, reply)
    }

    /// The accepted drivers, in registration order.
    #[must_use]
    pub fn drivers(&self) -> &[Driver] {
        &self.drivers
    }

    /// The drivers that were offered and refused, in the order they were
    /// offered, each with its reason.
    ///
    /// A loader that reports "no devices" without consulting this is throwing
    /// away the only evidence of what went wrong.
    #[must_use]
    pub fn rejected(&self) -> &[Rejection] {
        &self.rejected
    }

    /// Were any drivers accepted?
    ///
    /// Named for the question a caller actually has. `drivers().is_empty()`
    /// says the same thing, but reads as a fact about a slice rather than as
    /// the condition under which `vkCreateInstance` must report
    /// `VK_ERROR_INCOMPATIBLE_DRIVER` to the application.
    #[must_use]
    pub fn has_no_usable_driver(&self) -> bool {
        self.drivers.is_empty()
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
    use super::{Admission, Entry, Registry, handshake};
    use crate::icd::{CURRENT, DriverReply, InterfaceVersion, Unusable};
    use crate::vk::{
        Handle, VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INITIALIZATION_FAILED, VK_SUCCESS, VkResult,
        VoidFn,
    };
    use core::ffi::c_char;
    use core::ptr;

    /// A function a `GetProcAddr` stub can hand back so that the caller has
    /// something non-null to observe. Never called.
    unsafe extern "C" fn marker() {}

    /// The one entry point the stub drivers below admit to having.
    ///
    /// The stubs answer for this name and null for everything else, which is
    /// what a real `GetProcAddr` does. Writing them to answer *unconditionally*
    /// would be simpler and is wrong twice over: it stops modelling the null
    /// return that is the whole reason [`VoidFn`] is an `Option`, and clippy
    /// then observes — correctly — that a function which can only return
    /// `Some` should not be returning an `Option` at all. Its suggested fix,
    /// dropping the `Option` from the signature, would take the stub out of
    /// ABI agreement with `GetInstanceProcAddrFn`, so the lint is pointing at
    /// a defect in the fixture rather than at a false positive.
    const KNOWN: &core::ffi::CStr = c"vkTestEntryPoint";

    /// `vkGetInstanceProcAddr` for a driver that knows no entry points at all.
    ///
    /// Distinguishable from [`icd_gipa`] by what it answers, so a test can
    /// tell which of the two a [`Driver`] chose *by calling it* — a stronger
    /// claim than comparing function addresses, and one clippy does not object
    /// to on principle.
    unsafe extern "C" fn plain_gipa(_instance: Handle, _name: *const c_char) -> VoidFn {
        None
    }

    /// `vk_icdGetInstanceProcAddr` for a driver that knows [`KNOWN`].
    unsafe extern "C" fn icd_gipa(_instance: Handle, name: *const c_char) -> VoidFn {
        // SAFETY: `GetInstanceProcAddrFn`'s contract requires `name` to be a
        // NUL-terminated string that stays valid for the call, and every
        // caller in this module passes `KNOWN.as_ptr()`.
        let name = unsafe { core::ffi::CStr::from_ptr(name) };
        (name == KNOWN).then_some(marker as unsafe extern "C" fn())
    }

    /// `vk_icdGetPhysicalDeviceProcAddr`, same shape.
    unsafe extern "C" fn pdpa(_instance: Handle, name: *const c_char) -> VoidFn {
        // SAFETY: as `icd_gipa` above.
        let name = unsafe { core::ffi::CStr::from_ptr(name) };
        (name == KNOWN).then_some(marker as unsafe extern "C" fn())
    }

    /// Settles at 2 — a driver that has the handshake and little else.
    unsafe extern "C" fn negotiate_to_two(version: *mut u32) -> VkResult {
        // SAFETY: the contract of `NegotiateFn` guarantees a writable u32.
        unsafe { *version = 2 };
        VK_SUCCESS
    }

    /// Succeeds without touching the word — the loader must read back its own
    /// proposal, which is how a fully current driver behaves.
    unsafe extern "C" fn negotiate_silently(_version: *mut u32) -> VkResult {
        VK_SUCCESS
    }

    /// Claims far more than it was offered. Drivers really do this.
    unsafe extern "C" fn negotiate_greedily(version: *mut u32) -> VkResult {
        // SAFETY: as above.
        unsafe { *version = 99 };
        VK_SUCCESS
    }

    unsafe extern "C" fn negotiate_refusing(_version: *mut u32) -> VkResult {
        VK_ERROR_INCOMPATIBLE_DRIVER
    }

    unsafe extern "C" fn negotiate_failing(_version: *mut u32) -> VkResult {
        VK_ERROR_INITIALIZATION_FAILED
    }

    /// The baseline offer: a driver with nothing but `vkGetInstanceProcAddr`.
    const fn bare() -> Entry {
        Entry {
            get_instance_proc_addr: plain_gipa,
            icd_get_instance_proc_addr: None,
            get_physical_device_proc_addr: None,
            negotiate: None,
        }
    }

    /// Call whichever instance entry point the driver settled on, and report
    /// whether it was the `vk_icd` one.
    fn used_icd_entry_point(driver: &super::Driver) -> bool {
        let f = driver.instance_proc_addr();
        // SAFETY: both stubs ignore the instance, and `KNOWN` is a
        // NUL-terminated `'static` string, which is all `name` requires.
        let got = unsafe { f(ptr::null_mut(), KNOWN.as_ptr()) };
        got.is_some()
    }

    #[test]
    fn a_driver_without_a_handshake_is_inferred_from_the_symbols_it_offered() {
        let without = bare();
        // SAFETY: `negotiate` is None, so nothing is called.
        let reply = unsafe { handshake(CURRENT, &without) };
        assert_eq!(
            reply,
            DriverReply::NoNegotiationFunction {
                exports_icd_get_instance_proc_addr: false
            }
        );

        let with = Entry {
            icd_get_instance_proc_addr: Some(icd_gipa),
            ..bare()
        };
        // SAFETY: as above.
        let reply = unsafe { handshake(CURRENT, &with) };
        assert_eq!(
            reply,
            DriverReply::NoNegotiationFunction {
                exports_icd_get_instance_proc_addr: true
            }
        );
    }

    #[test]
    fn a_driver_that_succeeds_without_writing_settles_at_the_loaders_proposal() {
        // The word is in/out, so "returned VK_SUCCESS and changed nothing"
        // means "yes, that version". A loader that zeroed the word before the
        // call would read 0 here and quietly demote a current driver to the
        // baseline -- which is why `handshake` seeds it with the proposal.
        let entry = Entry {
            negotiate: Some(negotiate_silently),
            ..bare()
        };
        // SAFETY: `negotiate_silently` is a live stub that ignores its argument.
        let reply = unsafe { handshake(CURRENT, &entry) };
        assert_eq!(
            reply,
            DriverReply::Success {
                reported: CURRENT.get()
            }
        );
    }

    #[test]
    fn a_greedy_driver_is_admitted_only_at_the_version_the_loader_offered() {
        let mut registry = Registry::new(CURRENT);
        let entry = Entry {
            negotiate: Some(negotiate_greedily),
            ..bare()
        };
        // SAFETY: `negotiate_greedily` is a live stub writing through a valid pointer.
        let admission = unsafe { registry.register("greedy", entry) };
        assert_eq!(admission, Admission::Accepted(CURRENT));
        assert_eq!(registry.drivers().len(), 1);
        assert_eq!(registry.drivers()[0].version(), CURRENT);
    }

    #[test]
    fn a_modest_driver_keeps_the_version_it_asked_for() {
        let mut registry = Registry::new(CURRENT);
        let entry = Entry {
            negotiate: Some(negotiate_to_two),
            ..bare()
        };
        // SAFETY: live stub.
        let admission = unsafe { registry.register("modest", entry) };
        assert_eq!(admission, Admission::Accepted(InterfaceVersion::new(2)));
    }

    #[test]
    fn a_driver_that_refuses_is_recorded_rather_than_forgotten() {
        let mut registry = Registry::new(CURRENT);
        let entry = Entry {
            negotiate: Some(negotiate_refusing),
            ..bare()
        };
        // SAFETY: live stub.
        let admission = unsafe { registry.register("refuser", entry) };
        assert_eq!(
            admission,
            Admission::Rejected(Unusable::DeclaredIncompatible)
        );
        assert!(registry.drivers().is_empty());
        assert_eq!(registry.rejected().len(), 1);
        assert_eq!(registry.rejected()[0].name, "refuser");
        assert_eq!(registry.rejected()[0].why, Unusable::DeclaredIncompatible);
    }

    #[test]
    fn an_unrecognised_failure_keeps_its_code_all_the_way_into_the_record() {
        // The point of carrying the code rather than collapsing every failure
        // into one variant: the operator's next step differs, and the loader
        // is the only place that ever saw the number.
        let mut registry = Registry::new(CURRENT);
        let entry = Entry {
            negotiate: Some(negotiate_failing),
            ..bare()
        };
        // SAFETY: live stub.
        let admission = unsafe { registry.register("broken", entry) };
        assert_eq!(
            admission,
            Admission::Rejected(Unusable::HandshakeFailed(VK_ERROR_INITIALIZATION_FAILED))
        );
        assert_eq!(
            registry.rejected()[0].why,
            Unusable::HandshakeFailed(VK_ERROR_INITIALIZATION_FAILED)
        );
    }

    #[test]
    fn a_registry_with_nothing_but_rejections_says_so_and_still_explains_itself() {
        let mut registry = Registry::new(CURRENT);
        let refuses = Entry {
            negotiate: Some(negotiate_refusing),
            ..bare()
        };
        let fails = Entry {
            negotiate: Some(negotiate_failing),
            ..bare()
        };
        // SAFETY: live stubs.
        unsafe {
            registry.register("a", refuses);
            registry.register("b", fails);
        }
        assert!(registry.has_no_usable_driver());
        assert_eq!(registry.rejected().len(), 2, "a reason was thrown away");
        assert_eq!(registry.rejected()[0].name, "a");
        assert_eq!(registry.rejected()[1].name, "b");
    }

    #[test]
    fn a_version_one_driver_is_called_through_its_icd_entry_point() {
        let mut registry = Registry::new(CURRENT);
        let entry = Entry {
            icd_get_instance_proc_addr: Some(icd_gipa),
            ..bare()
        };
        // No negotiation function plus the icd symbol infers version 1.
        registry.admit(
            "v1",
            entry,
            DriverReply::NoNegotiationFunction {
                exports_icd_get_instance_proc_addr: true,
            },
        );
        assert_eq!(registry.drivers()[0].version(), InterfaceVersion::new(1));
        assert!(
            used_icd_entry_point(&registry.drivers()[0]),
            "a version-1 driver was called through the plain entry point"
        );
    }

    #[test]
    fn a_version_zero_driver_is_not_called_through_an_icd_symbol_it_happens_to_have() {
        // The trap: the offer and the entitlement are different things. This
        // driver has the symbol, but settled at 0, so the loader has no
        // promise about what that symbol does and must not call it.
        let mut registry = Registry::new(CURRENT);
        let entry = Entry {
            icd_get_instance_proc_addr: Some(icd_gipa),
            ..bare()
        };
        registry.admit("v0", entry, DriverReply::Success { reported: 0 });
        assert_eq!(registry.drivers()[0].version(), InterfaceVersion::new(0));
        assert!(
            !used_icd_entry_point(&registry.drivers()[0]),
            "a version-0 driver was called through an entry point it never promised"
        );
    }

    #[test]
    fn the_physical_device_entry_point_is_withheld_below_version_four() {
        // Version 3 is the one this author misremembered as the boundary; the
        // test exists because the mistake is a call through a pointer the
        // driver never exported. Checked at 3 *and* 4 so that neither an
        // off-by-one nor a wholesale removal of the gate passes.
        let entry = Entry {
            get_physical_device_proc_addr: Some(pdpa),
            ..bare()
        };
        let mut registry = Registry::new(CURRENT);
        registry.admit("three", entry, DriverReply::Success { reported: 3 });
        registry.admit("four", entry, DriverReply::Success { reported: 4 });

        assert_eq!(registry.drivers()[0].version(), InterfaceVersion::new(3));
        assert!(
            registry.drivers()[0].physical_device_proc_addr().is_none(),
            "a version-3 driver was offered a version-4 entry point"
        );
        assert!(
            registry.drivers()[1].physical_device_proc_addr().is_some(),
            "a version-4 driver was denied the entry point it does have"
        );
    }

    #[test]
    fn a_driver_that_never_offered_the_physical_device_entry_point_does_not_gain_one() {
        let mut registry = Registry::new(CURRENT);
        registry.admit(
            "current",
            bare(),
            DriverReply::Success {
                reported: CURRENT.get(),
            },
        );
        assert_eq!(registry.drivers()[0].version(), CURRENT);
        assert!(registry.drivers()[0].physical_device_proc_addr().is_none());
    }

    #[test]
    fn an_empty_registry_reports_no_usable_driver_and_no_rejections() {
        // The third case the rejection list exists to distinguish: nothing was
        // ever offered, which is a packaging problem rather than a driver one.
        let registry = Registry::new(CURRENT);
        assert!(registry.has_no_usable_driver());
        assert!(registry.rejected().is_empty());
        assert_eq!(registry.proposes(), CURRENT);
    }
}
