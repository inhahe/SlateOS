//! What `vkCreateInstance` means when the machine has more than one driver.
//!
//! An application calls `vkCreateInstance` once and gets one `VkInstance`
//! back. A machine with an integrated GPU, a discrete card and a software
//! rasteriser has three drivers, each of which has its own instance. The
//! loader's instance is therefore not a driver's instance: it is an object the
//! loader owns, holding one driver instance per driver that agreed to create
//! one, and it is what the application's handle actually points at.
//!
//! # What is specification and what is this loader's choice
//!
//! Very little of the multi-driver behaviour is written down. The
//! Loader–Driver Interface states one normative rule about it, `LDP_LOADER_1`:
//!
//! > A loader **must** return `VK_ERROR_INCOMPATIBLE_DRIVER` if it fails to
//! > find and load a valid Vulkan driver on the system.
//!
//! That is the whole of it. Whether the loader keeps going after one driver
//! fails, and which of several failures it reports, are not specified — so
//! this module states its rules as *policy* and says why, rather than
//! presenting them as requirements they are not. Getting that distinction
//! wrong is how a plausible invention becomes a citation later.
//!
//! # The policy, and the reasoning
//!
//! [`outcome`] decides what the application is told, from the results the
//! drivers gave:
//!
//! | Situation | Reported | Why |
//! |---|---|---|
//! | Any driver succeeded | success | The instance exists. A driver that failed is, from the application's side, indistinguishable from one that was never installed — which is a situation Vulkan already expects it to handle. |
//! | None succeeded, at least one ran out of host memory | `VK_ERROR_OUT_OF_HOST_MEMORY` | Memory exhaustion is a fact about the machine, not about the driver. Reporting "no compatible driver" here sends the user to reinstall drivers that are fine. |
//! | None succeeded, some gave a specific error | that error | The specific diagnosis is more useful than the generic one, and the loader is the only thing that ever saw it. |
//! | None succeeded, all said `VK_ERROR_INCOMPATIBLE_DRIVER`, or there were no drivers at all | `VK_ERROR_INCOMPATIBLE_DRIVER` | `LDP_LOADER_1`. |
//!
//! The one that deserves stating plainly is the first: a partial success is
//! reported as success. The alternative — failing the call because one driver
//! of three declined — would make an application's ability to start depend on
//! a GPU it was never going to use.
//!
//! # Two kinds of dispatchable object
//!
//! The loader deals with the dispatch word ([`crate::dispatch`]) from both
//! sides, and they are not the same operation:
//!
//! - The loader's **own** objects — its [`Instance`] and its
//!   [`PhysicalDevice`] wrappers — are ones it allocated, so it *stamps* the
//!   magic into them and then installs its table over it. It stamps even
//!   though it knows perfectly well what the object is, because a validation
//!   layer inspecting the handle checks for that magic, and an object without
//!   it looks like a corrupt one.
//! - A driver's **instances** are objects the driver allocated and stamped,
//!   which the loader then takes over ([`adopt_all`]). The interface says
//!   plainly that it may: *"The loader will replace the first entry with a
//!   pointer to the dispatch table which is owned by the loader."*
//!
//! Taking over a driver's instance is also the loader's only chance to notice
//! a driver that never stamped one. That word is the loader's by the interface
//! contract; a driver still using it for its own data would be silently
//! corrupted the first time anything wrote there, and the useful moment to
//! find that out is `vkCreateInstance`, not later.
//!
//! [`adopt_all`] checks every handle before writing to any of them. A driver
//! that returns one non-dispatchable handle among five is broken, and the
//! useful response is to reject that driver's objects entirely — not to end up
//! holding three adopted handles, one refusal, and one never looked at, which
//! is a state with no correct next step.
//!
//! # Why physical devices are wrapped rather than adopted
//!
//! A driver's physical devices go the other way: the loader allocates a
//! [`PhysicalDevice`] of its own for each one and hands *that* to the
//! application, keeping the driver's handle inside it. The reason is that the
//! application's handle has to answer a question the driver's handle cannot:
//! *which driver is this?* With several drivers registered, a bare
//! `VkPhysicalDevice` arriving at a loader entry point is un-attributable —
//! there is nothing in it to say who made it, and the loader would have to
//! search every driver's device list on every call. The wrapper carries the
//! driver index, so the answer is a field read.
//!
//! The cost is one small allocation per device, once per instance, and it buys
//! back the property that makes the rest of the loader simple: every handle the
//! application holds is one the loader made.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::c_void;

use crate::dispatch::{self, NotDispatchable};
use crate::vk::{
    Handle, VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_OUT_OF_HOST_MEMORY, VK_INCOMPLETE, VK_SUCCESS,
    VkResult,
};

/// Decide what to report to the application, given what each driver returned
/// from its own `vkCreateInstance`.
///
/// `Ok(())` means at least one driver created an instance and the loader
/// should build one. `Err(code)` is the code to return to the application.
///
/// An empty slice — no drivers were registered at all — is `LDP_LOADER_1`'s
/// case and reports `VK_ERROR_INCOMPATIBLE_DRIVER`. It is not a special case
/// in the code, because "every driver failed" and "there were no drivers"
/// genuinely have the same answer; it has a test of its own so that staying
/// true is not accidental.
///
/// See the module documentation for the reasoning behind each rule, and for
/// which of them the specification actually requires.
// No `#[must_use]`: `Result` already carries one, and a second bare copy adds
// nothing a caller would see.
pub fn outcome(attempts: &[VkResult]) -> Result<(), VkResult> {
    // Vulkan defines any non-negative result as a success; only negatives are
    // errors. `vkCreateInstance` has no positive success codes today, so this
    // is the same as `== VK_SUCCESS` in practice — written the general way
    // because the cost is nothing and the failure mode of the narrow way is
    // treating a future success as an error.
    if attempts.iter().any(|&r| r >= VK_SUCCESS) {
        return Ok(());
    }

    if attempts.contains(&VK_ERROR_OUT_OF_HOST_MEMORY) {
        return Err(VK_ERROR_OUT_OF_HOST_MEMORY);
    }

    // The first driver-specific complaint, if any driver made one. Anything
    // that is not "I am not compatible with you" says more than the generic
    // answer does.
    if let Some(&specific) = attempts
        .iter()
        .find(|&&r| r != VK_ERROR_INCOMPATIBLE_DRIVER)
    {
        return Err(specific);
    }

    Err(VK_ERROR_INCOMPATIBLE_DRIVER)
}

/// How many elements to write, and what to report, for Vulkan's two-call array
/// protocol.
///
/// Every `vkEnumerate*` works the same way: the application calls once with a
/// null array to learn the count, allocates, and calls again. Between the two
/// calls the count may have shrunk or the application may simply have asked for
/// fewer, so the second call writes `min(available, capacity)` and reports
/// `VK_INCOMPLETE` when it had to truncate.
///
/// Split out as a function taking two numbers because the mistakes here are
/// arithmetic ones — reporting success after truncating, or writing `available`
/// elements into a `capacity`-sized array — and both are invisible in a test
/// that goes through a driver.
#[must_use]
pub const fn array_query(available: usize, capacity: usize) -> (usize, VkResult) {
    if capacity < available {
        (capacity, VK_INCOMPLETE)
    } else {
        (available, VK_SUCCESS)
    }
}

/// One driver's instance, and which driver it came from.
#[derive(Debug, Clone, Copy)]
pub struct DriverInstance {
    /// Index into [`crate::registry::Registry::drivers`].
    pub driver: usize,
    /// The `VkInstance` that driver returned. Owned by the driver: the loader
    /// passes it back to that driver and never writes to it.
    pub handle: Handle,
}

/// The loader's own `VkInstance` — a dispatchable object holding one driver
/// instance per driver that succeeded.
///
/// `#[repr(C)]` with the dispatch word first is not decoration. This object is
/// handed to the application as a `VkInstance`, and anything that treats it as
/// one — a layer, a driver being passed it back — will read offset 0 expecting
/// the dispatch slot.
#[repr(C)]
pub struct Instance {
    /// `VK_LOADER_DATA`. Must be the first member.
    dispatch: usize,
    drivers: Vec<DriverInstance>,
    /// The loader's physical-device wrappers, whose *addresses* are the
    /// `VkPhysicalDevice` handles the application is given.
    ///
    /// `clippy::vec_box` calls the inner `Box` an unnecessary indirection, and
    /// for an ordinary collection it would be right. Here it is the thing that
    /// makes the handles legal: Vulkan requires the physical devices an
    /// instance reports to be the *same* handles every time, for the life of
    /// the instance. In a `Vec<PhysicalDevice>` those handles are interior
    /// pointers into one buffer, so the next `push` — GPU hot-plug is the
    /// obvious future one — reallocates and every handle the application is
    /// still holding dangles. Boxing each device makes its lifetime
    /// independent of the spine, which is what the C API already promises.
    #[allow(clippy::vec_box)]
    physical_devices: Vec<Box<PhysicalDevice>>,
    enumerated: bool,
}

impl Instance {
    /// Allocate a loader instance over the drivers that succeeded, stamped
    /// with the loader magic.
    ///
    /// Boxed because the address is what becomes the application's
    /// `VkInstance`, so it has to stop moving.
    #[must_use]
    pub fn new(drivers: Vec<DriverInstance>) -> Box<Self> {
        let mut boxed = Box::new(Self {
            dispatch: 0,
            drivers,
            physical_devices: Vec::new(),
            enumerated: false,
        });
        // SAFETY: `boxed` is a live, owned, correctly aligned `Instance` that
        // no other thread has seen yet, and `Instance` is `#[repr(C)]` with
        // `dispatch` first, so this writes that field and nothing else.
        unsafe {
            dispatch::set_loader_magic(core::ptr::from_mut::<Self>(&mut boxed).cast::<c_void>());
        }
        boxed
    }

    /// The current contents of the dispatch word.
    ///
    /// Exists so that the stamping can be *observed* rather than assumed: a
    /// test that only checked `new` did not panic would pass just as well if
    /// the stamp were never written.
    #[must_use]
    pub const fn dispatch_word(&self) -> usize {
        self.dispatch
    }

    /// The driver instances this loader instance fans out to.
    #[must_use]
    pub fn drivers(&self) -> &[DriverInstance] {
        &self.drivers
    }

    /// The physical devices already enumerated for this instance, if any.
    ///
    /// Empty both before the first `vkEnumeratePhysicalDevices` and on a
    /// machine whose drivers have no devices; [`Instance::devices_enumerated`]
    /// is the one that tells those apart.
    #[must_use]
    pub fn physical_devices(&self) -> &[Box<PhysicalDevice>] {
        &self.physical_devices
    }

    /// Has enumeration been done for this instance yet?
    ///
    /// Vulkan requires the `VkPhysicalDevice` handles an instance reports to be
    /// the *same* handles every time it is asked, so the list is built once and
    /// kept. A driver with no devices is an ordinary answer, and re-asking it
    /// on every call — which is what testing `physical_devices().is_empty()`
    /// would do — is a per-call fan-out that can never find anything.
    #[must_use]
    pub const fn devices_enumerated(&self) -> bool {
        self.enumerated
    }

    /// Record the physical devices enumerated across every driver.
    ///
    /// Takes the whole list at once rather than offering a push, because a
    /// half-filled list is indistinguishable from a complete one afterwards and
    /// the handles are then frozen for the life of the instance.
    pub fn set_physical_devices(&mut self, devices: Vec<Box<PhysicalDevice>>) {
        self.physical_devices = devices;
        self.enumerated = true;
    }

    /// Replace the magic with the loader's dispatch table, the same way a
    /// driver's object is taken over.
    ///
    /// Goes through [`dispatch::adopt`] rather than assigning the field,
    /// even though this object is the loader's own and the check cannot fail
    /// here today. The check is what guarantees the field still means what
    /// this code thinks it means; an assignment would keep working, silently,
    /// if `new` ever stopped stamping.
    ///
    /// # Safety
    ///
    /// `table` must outlive every use the application makes of this instance.
    pub unsafe fn install_table(&mut self, table: *const c_void) -> Result<(), NotDispatchable> {
        // SAFETY: `self` is a live `#[repr(C)]` `Instance` with the dispatch
        // word first, borrowed mutably so nothing else is touching it. The
        // caller guarantees `table`'s lifetime.
        unsafe { dispatch::adopt(core::ptr::from_mut::<Self>(self).cast::<c_void>(), table) }
    }
}

/// The loader's own `VkPhysicalDevice` — one driver's device, plus the notes of
/// whose it is and which of that driver's instances it came from.
///
/// `#[repr(C)]` with the dispatch word first, for the same reason as
/// [`Instance`]: this is what the application is handed, and anything treating
/// it as a `VkPhysicalDevice` reads offset 0.
///
/// Two of the fields are also read by three instructions of assembly — see
/// [`crate::unknown`] — which is why their byte offsets are published as
/// [`PhysicalDevice::HANDLE_OFFSET`] and [`PhysicalDevice::EXT_OFFSET`] rather
/// than written out there as numbers. Reordering the fields changes the
/// constants and the assembly together; writing the numbers twice would let the
/// two drift, and the symptom would be a jump through whatever word landed at
/// the old offset.
#[repr(C)]
pub struct PhysicalDevice {
    /// `VK_LOADER_DATA`. Must be the first member.
    dispatch: usize,
    driver: usize,
    instance: Handle,
    handle: Handle,
    /// This device's driver's table of unknown-extension entry points.
    ///
    /// Reached from the trampolines in [`crate::unknown`], which have one copy
    /// of the code per slot shared by every driver and so cannot hold a function
    /// pointer themselves. Points into a `Box` owned by the driver record, which
    /// outlives every device enumerated from it.
    ext: *const crate::unknown::Table,
}

// The dispatch word being first is the one layout fact the whole crate assumes,
// and the one a field reordering would break silently: a driver or a validation
// layer handed this object reads offset 0 and would find the driver index.
const _: () = assert!(core::mem::offset_of!(PhysicalDevice, dispatch) == 0);

impl PhysicalDevice {
    /// Byte offset of [`PhysicalDevice::handle`] — the driver's own device — for
    /// the assembly in [`crate::unknown`], which substitutes it for argument 0.
    pub const HANDLE_OFFSET: usize = core::mem::offset_of!(Self, handle);

    /// Byte offset of the driver's unknown-extension table, for the same
    /// assembly, which reads it *before* overwriting argument 0.
    pub const EXT_OFFSET: usize = core::mem::offset_of!(Self, ext);

    /// Wrap one device belonging to `driver`, stamped with the loader magic.
    ///
    /// `driver` indexes [`crate::registry::Registry::drivers`], `instance` is
    /// the `VkInstance` *that driver* returned, `handle` is what that driver's
    /// `vkEnumeratePhysicalDevices` returned, and `ext` is that driver's
    /// [`crate::unknown::Table`].
    #[must_use]
    pub fn new(
        driver: usize,
        instance: Handle,
        handle: Handle,
        ext: *const crate::unknown::Table,
    ) -> Box<Self> {
        let mut boxed = Box::new(Self {
            dispatch: 0,
            driver,
            instance,
            handle,
            ext,
        });
        // SAFETY: `boxed` is a live, owned, correctly aligned `PhysicalDevice`
        // that no other thread has seen yet, and the type is `#[repr(C)]` with
        // `dispatch` first, so this writes that field and nothing else.
        unsafe {
            dispatch::set_loader_magic(core::ptr::from_mut::<Self>(&mut boxed).cast::<c_void>());
        }
        boxed
    }

    /// Which registered driver this device belongs to.
    #[must_use]
    pub const fn driver(&self) -> usize {
        self.driver
    }

    /// The driver's own `VkInstance` this device was enumerated from.
    ///
    /// Kept because `vkCreateDevice` needs it and is not given it. That call
    /// receives a `VkPhysicalDevice` and nothing else, but the driver's
    /// `vkCreateDevice` has to be found through that driver's
    /// `vkGetInstanceProcAddr`, which for an instance-level command must be
    /// asked with a real instance rather than null. Without this field the
    /// loader would have to search every live instance for the one that
    /// enumerated this device — and it does not track its live instances, so
    /// it could not.
    #[must_use]
    pub const fn instance(&self) -> Handle {
        self.instance
    }

    /// The driver's own handle for this device — what the loader passes back
    /// down when it calls that driver.
    #[must_use]
    pub const fn handle(&self) -> Handle {
        self.handle
    }

    /// This device's driver's table of unknown-extension entry points.
    ///
    /// Exposed so that the field the assembly reads can be *checked* from Rust.
    /// A trampoline that reached the wrong table would still run; the failure
    /// would be calling one driver's function with another driver's device,
    /// which is exactly the kind of plausible wrong answer a test has to be able
    /// to look for directly.
    #[must_use]
    pub const fn ext(&self) -> *const crate::unknown::Table {
        self.ext
    }

    /// The current contents of the dispatch word. See
    /// [`Instance::dispatch_word`].
    #[must_use]
    pub const fn dispatch_word(&self) -> usize {
        self.dispatch
    }

    /// Replace the magic with the loader's dispatch table.
    ///
    /// A `VkPhysicalDevice` dispatches through the *instance* table in Vulkan —
    /// it has no table of its own — so this is given the same pointer the
    /// owning [`Instance`] was.
    ///
    /// # Safety
    ///
    /// `table` must outlive every use the application makes of this device.
    pub unsafe fn install_table(&mut self, table: *const c_void) -> Result<(), NotDispatchable> {
        // SAFETY: `self` is a live `#[repr(C)]` `PhysicalDevice` with the
        // dispatch word first, borrowed mutably so nothing else is touching it.
        // The caller guarantees `table`'s lifetime.
        unsafe { dispatch::adopt(core::ptr::from_mut::<Self>(self).cast::<c_void>(), table) }
    }
}

/// Take over every handle in `handles`, or none of them.
///
/// Used for the `VkInstance` each driver returns from its own
/// `vkCreateInstance`. Every handle is checked before any is written, so a
/// driver that returns one bad handle among several leaves the loader with
/// nothing half-done — see the module documentation.
///
/// # Safety
///
/// Every handle must point to at least one readable-and-writable,
/// `usize`-aligned word that no other thread is touching, and must be a handle
/// the driver returned from an entry point that produces dispatchable objects.
/// `table` must outlive every use the application makes of any of them.
pub unsafe fn adopt_all(handles: &[Handle], table: *const c_void) -> Result<(), NotDispatchable> {
    for &handle in handles {
        // SAFETY: forwarded from this function's contract.
        let found = unsafe { dispatch::loader_data(handle) };
        if !dispatch::is_loader_magic(found) {
            return Err(NotDispatchable { found });
        }
    }

    for &handle in handles {
        // SAFETY: as above, and the loop before this one established that
        // every handle carries the magic, so `adopt` cannot fail here. Its
        // result is still not discarded -- see below.
        unsafe { dispatch::adopt(handle, table) }?;
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
    use super::{DriverInstance, Instance, PhysicalDevice, adopt_all, array_query, outcome};
    use crate::dispatch::{ICD_LOADER_MAGIC, is_loader_magic};
    use crate::vk::{
        Handle, VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INITIALIZATION_FAILED,
        VK_ERROR_OUT_OF_HOST_MEMORY, VK_INCOMPLETE, VK_SUCCESS,
    };
    use alloc::vec;
    use alloc::vec::Vec;
    use core::ffi::c_void;

    /// Stands in for a driver's physical-device object: the dispatch word
    /// first, then private data the loader must not disturb.
    #[repr(C)]
    struct DriverObject {
        loader_data: usize,
        private: u64,
    }

    impl DriverObject {
        const fn stamped() -> Self {
            Self {
                loader_data: ICD_LOADER_MAGIC as usize,
                private: 0xFEED_FACE_CAFE_BEEF,
            }
        }

        const fn unstamped() -> Self {
            Self {
                loader_data: 0,
                private: 0xFEED_FACE_CAFE_BEEF,
            }
        }

        fn handle(&mut self) -> Handle {
            core::ptr::from_mut::<Self>(self).cast::<c_void>()
        }
    }

    /// An address to install as a dispatch table. Never dereferenced.
    fn a_table() -> *const c_void {
        core::ptr::without_provenance::<c_void>(0x1234_5678)
    }

    /// A driver's unknown-extension table, for the wrappers below.
    ///
    /// A real one rather than a null pointer, even though nothing here jumps
    /// through it: a `PhysicalDevice` with a null `ext` is an object that could
    /// not exist in the loader, and a fixture that models an impossible state is
    /// a fixture that stops catching the possible ones.
    fn an_ext_table() -> alloc::boxed::Box<crate::unknown::Table> {
        crate::unknown::Table::new()
    }

    #[test]
    fn one_success_among_failures_is_a_success() {
        // The rule with the most user-visible consequence: an application must
        // not fail to start because of a GPU it was never going to use.
        assert_eq!(
            outcome(&[
                VK_ERROR_INCOMPATIBLE_DRIVER,
                VK_SUCCESS,
                VK_ERROR_INITIALIZATION_FAILED
            ]),
            Ok(())
        );
    }

    #[test]
    fn a_success_outranks_even_running_out_of_memory() {
        assert_eq!(
            outcome(&[VK_ERROR_OUT_OF_HOST_MEMORY, VK_SUCCESS]),
            Ok(()),
            "an instance that exists was reported as a failure"
        );
    }

    #[test]
    fn no_drivers_at_all_is_ldp_loader_1() {
        // The one rule here the specification actually requires.
        assert_eq!(outcome(&[]), Err(VK_ERROR_INCOMPATIBLE_DRIVER));
    }

    #[test]
    fn every_driver_declining_is_also_ldp_loader_1() {
        assert_eq!(
            outcome(&[VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INCOMPATIBLE_DRIVER]),
            Err(VK_ERROR_INCOMPATIBLE_DRIVER)
        );
    }

    #[test]
    fn running_out_of_host_memory_is_reported_as_such_not_as_incompatibility() {
        // Telling the user "no compatible driver" when the machine is out of
        // memory sends them to reinstall drivers that are fine.
        assert_eq!(
            outcome(&[VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_OUT_OF_HOST_MEMORY]),
            Err(VK_ERROR_OUT_OF_HOST_MEMORY)
        );
    }

    #[test]
    fn a_specific_failure_beats_the_generic_one() {
        assert_eq!(
            outcome(&[VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INITIALIZATION_FAILED]),
            Err(VK_ERROR_INITIALIZATION_FAILED),
            "the only diagnosis anybody had was thrown away"
        );
    }

    #[test]
    fn out_of_memory_outranks_another_specific_failure() {
        assert_eq!(
            outcome(&[VK_ERROR_INITIALIZATION_FAILED, VK_ERROR_OUT_OF_HOST_MEMORY]),
            Err(VK_ERROR_OUT_OF_HOST_MEMORY)
        );
    }

    #[test]
    fn a_new_instance_carries_the_loader_magic() {
        // Checked by reading the word, not by trusting `new` to have run: a
        // test that only asserted `new` returned would pass with the stamp
        // deleted.
        let instance = Instance::new(Vec::new());
        assert!(
            is_loader_magic(instance.dispatch_word()),
            "a layer inspecting this handle would call it corrupt"
        );
    }

    #[test]
    fn an_instance_remembers_which_drivers_it_fans_out_to() {
        let mut a = DriverObject::stamped();
        let mut b = DriverObject::stamped();
        let instance = Instance::new(vec![
            DriverInstance {
                driver: 0,
                handle: a.handle(),
            },
            DriverInstance {
                driver: 2,
                handle: b.handle(),
            },
        ]);
        assert_eq!(instance.drivers().len(), 2);
        assert_eq!(instance.drivers()[0].driver, 0);
        assert_eq!(instance.drivers()[1].driver, 2);
    }

    #[test]
    fn installing_the_table_replaces_the_magic() {
        let mut instance = Instance::new(Vec::new());
        // SAFETY: the address is never dereferenced by this crate.
        let result = unsafe { instance.install_table(a_table()) };
        assert_eq!(result, Ok(()));
        assert_eq!(instance.dispatch_word(), a_table() as usize);
        assert!(
            !is_loader_magic(instance.dispatch_word()),
            "the magic survived, so the table was not installed"
        );
    }

    #[test]
    fn adopting_a_whole_batch_leaves_the_drivers_private_data_alone() {
        let mut one = DriverObject::stamped();
        let mut two = DriverObject::stamped();
        let handles = [one.handle(), two.handle()];
        // SAFETY: both objects are live, aligned, and stamped.
        let result = unsafe { adopt_all(&handles, a_table()) };
        assert_eq!(result, Ok(()));
        assert_eq!(one.loader_data, a_table() as usize);
        assert_eq!(two.loader_data, a_table() as usize);
        assert_eq!(one.private, 0xFEED_FACE_CAFE_BEEF);
        assert_eq!(two.private, 0xFEED_FACE_CAFE_BEEF);
    }

    #[test]
    fn one_bad_handle_in_a_batch_leaves_every_handle_untouched() {
        // The all-or-nothing rule. Written so the bad handle is *last*, which
        // is the ordering a one-pass implementation passes the easy version of
        // this test with -- it would already have adopted the first two.
        let mut one = DriverObject::stamped();
        let mut two = DriverObject::stamped();
        let mut bad = DriverObject::unstamped();
        let handles = [one.handle(), two.handle(), bad.handle()];

        // SAFETY: all three objects are live and aligned.
        let result = unsafe { adopt_all(&handles, a_table()) };
        assert!(result.is_err(), "a non-dispatchable handle was accepted");
        assert_eq!(result.unwrap_err().found, 0);

        assert!(
            is_loader_magic(one.loader_data) && is_loader_magic(two.loader_data),
            "a rejected batch still overwrote the handles ahead of the bad one"
        );
        assert_eq!(bad.loader_data, 0);
    }

    #[test]
    fn an_empty_batch_is_adopted_vacuously() {
        // A driver with no physical devices is ordinary, not an error.
        // SAFETY: no handles are read.
        assert_eq!(unsafe { adopt_all(&[], a_table()) }, Ok(()));
    }

    #[test]
    fn a_big_enough_array_gets_everything_and_a_plain_success() {
        assert_eq!(array_query(3, 3), (3, VK_SUCCESS));
        assert_eq!(array_query(3, 10), (3, VK_SUCCESS));
    }

    #[test]
    fn a_short_array_is_truncated_and_reported_incomplete() {
        // Reporting success here is the bug this function exists to prevent:
        // the application would take the count it asked for as the whole set.
        assert_eq!(array_query(5, 2), (2, VK_INCOMPLETE));
    }

    #[test]
    fn asking_for_none_of_several_is_still_incomplete() {
        // The count-only first call passes a null array, not a zero capacity,
        // so a zero capacity really does mean "write nothing" -- and writing
        // nothing out of five is a truncation like any other.
        assert_eq!(array_query(5, 0), (0, VK_INCOMPLETE));
    }

    #[test]
    fn having_nothing_to_report_is_a_success_at_any_capacity() {
        assert_eq!(array_query(0, 0), (0, VK_SUCCESS));
        assert_eq!(array_query(0, 4), (0, VK_SUCCESS));
    }

    #[test]
    fn a_wrapped_physical_device_remembers_its_driver_and_handle() {
        let mut driver_instance = DriverObject::stamped();
        let mut object = DriverObject::stamped();
        let instance = driver_instance.handle();
        let handle = object.handle();
        let ext = an_ext_table();
        let device = PhysicalDevice::new(3, instance, handle, ext.as_ptr());
        assert_eq!(device.driver(), 3);
        assert_eq!(device.handle(), handle);
        // The field three instructions of assembly read; see `crate::unknown`.
        assert_eq!(device.ext(), ext.as_ptr());
        // Kept because `vkCreateDevice` is given a physical device and nothing
        // else, yet has to find the driver's `vkCreateDevice` through an
        // instance-level lookup, which needs a real instance.
        assert_eq!(device.instance(), instance);
        assert!(
            is_loader_magic(device.dispatch_word()),
            "a layer inspecting this handle would call it corrupt"
        );
    }

    #[test]
    fn wrapping_a_device_does_not_touch_the_drivers_object() {
        // The whole point of wrapping rather than adopting: the driver's handle
        // goes back to the driver exactly as it came out.
        let mut driver_instance = DriverObject::stamped();
        let mut object = DriverObject::stamped();
        let ext = an_ext_table();
        let mut device =
            PhysicalDevice::new(0, driver_instance.handle(), object.handle(), ext.as_ptr());
        // SAFETY: the address is never dereferenced by this crate.
        assert_eq!(unsafe { device.install_table(a_table()) }, Ok(()));
        assert_eq!(device.dispatch_word(), a_table() as usize);
        assert!(
            is_loader_magic(object.loader_data),
            "the driver's own dispatch word was overwritten"
        );
        assert_eq!(object.private, 0xFEED_FACE_CAFE_BEEF);
    }

    #[test]
    fn an_instance_starts_out_not_having_enumerated() {
        // Distinguishing "not asked yet" from "asked, and there were none" is
        // what stops a machine with no devices being re-scanned on every call.
        let mut instance = Instance::new(Vec::new());
        assert!(!instance.devices_enumerated());
        assert!(instance.physical_devices().is_empty());

        instance.set_physical_devices(Vec::new());
        assert!(instance.devices_enumerated());
        assert!(instance.physical_devices().is_empty());
    }

    #[test]
    fn enumerated_devices_are_kept_in_order() {
        let mut one_instance = DriverObject::stamped();
        let mut two_instance = DriverObject::stamped();
        let mut one = DriverObject::stamped();
        let mut two = DriverObject::stamped();
        let mut instance = Instance::new(Vec::new());
        let one_ext = an_ext_table();
        let two_ext = an_ext_table();
        instance.set_physical_devices(vec![
            PhysicalDevice::new(0, one_instance.handle(), one.handle(), one_ext.as_ptr()),
            PhysicalDevice::new(1, two_instance.handle(), two.handle(), two_ext.as_ptr()),
        ]);
        assert_eq!(instance.physical_devices().len(), 2);
        assert_eq!(instance.physical_devices()[0].driver(), 0);
        assert_eq!(instance.physical_devices()[1].driver(), 1);
    }
}
