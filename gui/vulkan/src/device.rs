//! What a `VkDevice` is once the loader is in the middle, and why it is not
//! shaped like the loader's instance.
//!
//! # A device has exactly one driver behind it
//!
//! [`crate::instance`] exists because one `vkCreateInstance` becomes several —
//! the loader's instance is a fan-out object holding one driver instance per
//! driver that agreed to make one. Nothing about a device works that way. An
//! application creates a device *from a particular `VkPhysicalDevice`*, that
//! physical device belongs to one driver, and so does the device. There is
//! nothing to fan out to and nothing to choose between.
//!
//! That single fact is what makes the device level a different design rather
//! than the instance design with the names changed.
//!
//! # The handle is the driver's own, not a wrapper
//!
//! The loader wraps a `VkPhysicalDevice` ([`crate::instance::PhysicalDevice`])
//! because a bare one is un-attributable: with several drivers registered there
//! is nothing in the handle to say who made it. A `VkDevice` has the same
//! question to answer and gets a different answer — the loader **adopts** the
//! driver's device, exactly as it adopts a driver's instance, and the
//! application's `VkDevice` *is* the driver's `VkDevice`.
//!
//! The attribution comes from the dispatch word instead. Offset 0 of every
//! dispatchable handle points at a table; the loader replaces the driver's
//! magic there with the address of one of these [`Device`] records, so
//! recovering "which driver, and how do I call it" from a `VkDevice` is one
//! load ([`crate::dispatch::loader_data`]) rather than a search.
//!
//! Wrapping would have cost more than an allocation. Every device command the
//! application ever calls — and in a frame that is thousands — would arrive at
//! the loader holding a wrapper the driver has never seen, so the loader would
//! have to substitute the real handle and forward, forever, for every command
//! including ones it has never heard of. Adopting means the pointer the
//! application holds is already the one the driver wants, so a device command
//! can go **straight to the driver with the loader nowhere in the path**. That
//! is the entire reason Vulkan separates device-level dispatch from
//! instance-level dispatch, and a loader that wrapped devices would throw it
//! away.
//!
//! # Why the record is per device and not per driver
//!
//! It would be tempting to keep one of these per driver: the driver index and
//! the driver's `vkGetDeviceProcAddr` are both per-driver facts, so two devices
//! from one driver would appear to be able to share.
//!
//! They cannot, and the reason is the contract on `vkGetDeviceProcAddr`: the
//! pointer it returns is specific to the device it was asked about. Two devices
//! from the same driver may legitimately be given different function pointers
//! for the same command — that is precisely how a driver specialises a command
//! for one device without a runtime test on every call, which is the
//! optimisation device-level dispatch exists to permit. A shared record would
//! be a place to cache those pointers under the wrong key, so this one is per
//! device and the sharing question never arises.
//!
//! # Why there are no function pointers in it yet
//!
//! A loader's device dispatch table is conventionally an array of every device
//! command's function pointer, so that the loader's exported `vkCmdDraw` can
//! read offset 0 and jump. This loader exports no such commands, so such a
//! table would be a few hundred slots that nothing reads — the sort of
//! structure that looks like a working dispatch table right up until someone
//! relies on it.
//!
//! So [`Device`] holds what is used and nothing else. When the loader does
//! start exporting device commands, each one adds its pointer here and its
//! trampoline in [`crate::entry`], together, and the entry is used the moment
//! it exists.
//!
//! The name is still "the thing the dispatch word points at", which is what
//! offset 0 is required to be. Nothing outside this crate reads its layout: a
//! driver never inspects that word — the Loader–Driver Interface gives it to
//! the loader — and a layer is handed tables through the layer chain rather
//! than by parsing the loader's.

use alloc::boxed::Box;
use core::ffi::c_void;

use crate::vk::GetDeviceProcAddrFn;

/// What the loader keeps for one live `VkDevice`, and what the device's
/// dispatch word points at.
///
/// Not `#[repr(C)]`, and that is deliberate rather than an oversight. The types
/// the *application* holds — [`crate::instance::Instance`] and
/// [`crate::instance::PhysicalDevice`] — are `#[repr(C)]` because a handle to
/// them crosses to C and something there reads offset 0. This record is never a
/// handle. It is only ever pointed *at*, by a word this crate writes and this
/// crate reads, so its layout is private and Rust may choose it.
pub struct Device {
    driver: usize,
    /// The driver's own `vkGetDeviceProcAddr`, already resolved for this
    /// device.
    ///
    /// Resolved once at creation rather than looked up per call, because the
    /// alternative is an instance-level lookup by string on the way to every
    /// device-level lookup by string.
    driver_get_device_proc_addr: GetDeviceProcAddrFn,
}

impl Device {
    /// Record a device belonging to `driver`.
    ///
    /// Boxed because its address is written into the device's dispatch word, so
    /// it has to stop moving. Unlike the loader's instance and physical-device
    /// objects this one is *not* stamped with the loader magic: the magic marks
    /// a dispatchable object for a layer that inspects one, and this record is
    /// not a dispatchable object — it is what a dispatchable object points at.
    #[must_use]
    pub fn new(driver: usize, driver_get_device_proc_addr: GetDeviceProcAddrFn) -> Box<Self> {
        Box::new(Self {
            driver,
            driver_get_device_proc_addr,
        })
    }

    /// Which registered driver this device belongs to.
    #[must_use]
    pub const fn driver(&self) -> usize {
        self.driver
    }

    /// The driver's `vkGetDeviceProcAddr` for this device.
    #[must_use]
    pub const fn driver_get_device_proc_addr(&self) -> GetDeviceProcAddrFn {
        self.driver_get_device_proc_addr
    }

    /// This record's address, in the form the dispatch machinery installs.
    ///
    /// Takes `&mut` rather than `&` because the address is about to be written
    /// into a handle the application will use, and a shared borrow would let
    /// the same record be installed into two devices.
    #[must_use]
    pub fn as_dispatch_target(self: &mut Box<Self>) -> *const c_void {
        core::ptr::from_mut::<Self>(&mut **self).cast::<c_void>()
    }
}

/// Which command a device-level lookup is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    /// A command the loader implements itself, and must keep implementing: the
    /// application has to reach *the loader's* version, not the driver's.
    Loader(LoaderCommand),
    /// Anything else. The driver answers, and its answer — including "no such
    /// command" — is the answer.
    Driver,
}

/// The device-level commands the loader owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderCommand {
    /// `vkGetDeviceProcAddr`. Owned because the driver's would hand back the
    /// driver's own, and every lookup made through it would then skip the two
    /// commands below.
    GetDeviceProcAddr,
    /// `vkDestroyDevice`. Owned because destroying a device also has to free
    /// the [`Device`] record its dispatch word points at, which the driver
    /// knows nothing about. A driver's `vkDestroyDevice` reached directly would
    /// free the driver's object and leak the loader's.
    DestroyDevice,
}

/// Decide who answers a `vkGetDeviceProcAddr`.
///
/// A pure function over the name so that the rule can be tested without a
/// device, a driver, or a single raw pointer — the FFI around it is in
/// [`crate::entry`].
///
/// Note what is *not* here: a list of device commands to accept. The loader
/// does not keep one and must not, because it would be a list of the commands
/// known when it was written, and every extension a driver supports that this
/// loader has never heard of would be missing from it. Forwarding by default is
/// what makes an unknown command work; the two names above are the exceptions,
/// and they are exceptions because the loader has state riding on them.
#[must_use]
pub fn lookup(name: &[u8]) -> Lookup {
    match name {
        b"vkGetDeviceProcAddr" => Lookup::Loader(LoaderCommand::GetDeviceProcAddr),
        b"vkDestroyDevice" => Lookup::Loader(LoaderCommand::DestroyDevice),
        _ => Lookup::Driver,
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
    use super::{Device, LoaderCommand, Lookup, lookup};
    use crate::vk::{GetDeviceProcAddrFn, Handle, VoidFn};
    use core::ffi::c_char;

    /// Stands in for a driver's `vkGetDeviceProcAddr`. Never called here — the
    /// tests in this module are about what is *stored*, and the calling is
    /// [`crate::entry`]'s.
    unsafe extern "C" fn a_driver_gdpa(_device: Handle, _name: *const c_char) -> VoidFn {
        None
    }

    #[test]
    fn get_device_proc_addr_is_answered_by_the_loader() {
        // If the driver's were handed back, every later lookup through it would
        // miss vkDestroyDevice too, and the loader would never hear about a
        // device being destroyed.
        assert_eq!(
            lookup(b"vkGetDeviceProcAddr"),
            Lookup::Loader(LoaderCommand::GetDeviceProcAddr)
        );
    }

    #[test]
    fn destroy_device_is_answered_by_the_loader() {
        // The driver's would free the driver's object and leak the loader's
        // record -- and worse, leave a freed device whose dispatch word still
        // points at that record.
        assert_eq!(
            lookup(b"vkDestroyDevice"),
            Lookup::Loader(LoaderCommand::DestroyDevice)
        );
    }

    #[test]
    fn an_ordinary_device_command_goes_to_the_driver() {
        assert_eq!(lookup(b"vkGetDeviceQueue"), Lookup::Driver);
        assert_eq!(lookup(b"vkCmdDraw"), Lookup::Driver);
        assert_eq!(lookup(b"vkQueueSubmit"), Lookup::Driver);
    }

    #[test]
    fn a_command_this_loader_has_never_heard_of_goes_to_the_driver() {
        // The property that matters most, and the reason there is no accept
        // list: a driver's extension command has to work through a loader that
        // predates it. A loader with a list of known commands would answer null
        // here and the extension would be unusable.
        assert_eq!(lookup(b"vkCmdDrawMeshTasksEXT"), Lookup::Driver);
        assert_eq!(lookup(b"vkSomethingInventedNextYear"), Lookup::Driver);
    }

    #[test]
    fn an_instance_level_command_is_not_special_cased_here() {
        // vkGetDeviceProcAddr is not the place to refuse an instance-level
        // name. The driver is asked and says null, which is what the Vulkan
        // specification requires of the answer -- inventing the refusal here
        // would mean maintaining a second list of instance commands in order to
        // return the same null.
        assert_eq!(lookup(b"vkCreateInstance"), Lookup::Driver);
        assert_eq!(lookup(b"vkEnumeratePhysicalDevices"), Lookup::Driver);
    }

    #[test]
    fn an_empty_name_is_not_mistaken_for_a_loader_command() {
        assert_eq!(lookup(b""), Lookup::Driver);
    }

    #[test]
    fn a_prefix_of_a_loader_command_is_not_that_command() {
        // Matching on the whole slice rather than a prefix. `CStr::to_bytes`
        // stops at the NUL, so a name that merely starts the same way is a
        // different name.
        assert_eq!(lookup(b"vkDestroyDev"), Lookup::Driver);
        assert_eq!(lookup(b"vkDestroyDeviceMemory"), Lookup::Driver);
    }

    #[test]
    fn a_device_record_remembers_its_driver() {
        let device = Device::new(2, a_driver_gdpa);
        assert_eq!(device.driver(), 2);
    }

    #[test]
    fn a_device_record_keeps_the_drivers_lookup_function() {
        // Stored so that a device-level lookup does not have to make an
        // instance-level one first.
        let device = Device::new(0, a_driver_gdpa);
        let stored = device.driver_get_device_proc_addr();
        let expected: GetDeviceProcAddrFn = a_driver_gdpa;
        assert!(
            core::ptr::fn_addr_eq(stored, expected),
            "the driver's lookup function was not the one kept"
        );
    }

    #[test]
    fn the_dispatch_target_is_the_records_own_address_and_is_stable() {
        // The address goes into a handle the application keeps, so it must not
        // depend on where the `Box` itself lives.
        let mut device = Device::new(1, a_driver_gdpa);
        let first = device.as_dispatch_target();
        let mut moved = device;
        assert_eq!(
            moved.as_dispatch_target(),
            first,
            "moving the Box moved the record, so every live handle now dangles"
        );
    }
}
