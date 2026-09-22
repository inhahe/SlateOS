//! `VK_EXT_debug_utils`' messenger: the loader's first *instance-level*
//! extension command, and the fan-out policy that makes one possible.
//!
//! # Why this module exists at all
//!
//! [`crate::unknown`] forwards physical-device extension commands the loader
//! has never heard of, in three instructions, without knowing their
//! signatures. That trick does not extend to a command whose first argument is
//! a `VkInstance`, and the reason is worth stating precisely because it is the
//! whole design constraint here.
//!
//! A physical-device command names *one* GPU, so the loader's wrapper for it
//! names exactly one driver: overwrite argument 0 with that driver's own
//! handle and jump. There is one right answer and no decision to make. A
//! `VkInstance`, by contrast, is the loader's own object standing in front of
//! **several** driver instances at once — so "which driver answers this call?"
//! has no mechanical answer. It is a policy, and a different one per command.
//!
//! # The policy this command gets, and why
//!
//! A debug messenger is a *subscription*: the application is asking to be told
//! about anything any driver considers noteworthy. So the policy is **every
//! driver that offers the command gets one**, and the loader's handle stands
//! for the whole set.
//!
//! Two rules give that policy its edges, and they are not the same rule:
//!
//! * **A driver that does not offer the command is not a failure.** It has no
//!   `vkCreateDebugUtilsMessengerEXT` to call because it does not implement
//!   `VK_EXT_debug_utils`, which is a legitimate thing for a driver to be.
//!   It contributes nothing, exactly as a driver with no physical devices
//!   contributes nothing to `vkEnumeratePhysicalDevices`.
//! * **A driver that offers the command and then fails is a failure, and it
//!   fails the whole call.** Everything already created is destroyed and the
//!   error is returned. Vulkan's contract for a create command is atomic —
//!   on failure the application's handle is untouched and nothing is left
//!   allocated — and a half-built messenger cannot be reported through the
//!   C API at all: the application would be silently subscribed to a subset
//!   of its drivers with no way to learn which, and no way to ask again.
//!
//! That second rule is why this is not simply [`crate::instance::outcome`],
//! whose "any driver succeeded is a success" is right for `vkCreateInstance`
//! (where a driver that cannot participate is one the application never needed)
//! and wrong here (where a driver that cannot participate is a source of
//! diagnostics going quiet for no stated reason). Two fan-outs over the same
//! driver list, two policies, because the commands mean different things.
//! See design-decisions.md §867.
//!
//! # The handle
//!
//! `VkDebugUtilsMessengerEXT` is *non-dispatchable* — a bare 64-bit value with
//! no layout the application may assume — so the loader is free to make it the
//! address of its own [`Messenger`]. That is the same move [`crate::instance`]
//! makes for physical devices and it carries the same obligation: the box must
//! outlive every use, so it is leaked on the way out and reclaimed only by
//! `vkDestroyDebugUtilsMessengerEXT`.
//!
//! The loader must **never** hand its own handle to a driver. Each driver is
//! given back the handle it returned; the loader's value means something only
//! to the loader. Getting this backwards would have a driver read the loader's
//! pointer as its own object, which is why the unwrapping lives in one place.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::ffi::{CStr, c_void};

use crate::instance::{DriverInstance, Instance};
use crate::registry::Registry;
use crate::vk::{Handle, VK_SUCCESS, VkResult};

/// `vkCreateDebugUtilsMessengerEXT`, as the driver exposes it.
///
/// The create-info pointer is passed through untouched. The loader does not
/// declare `VkDebugUtilsMessengerCreateInfoEXT` and does not need to: it never
/// reads a field, so §802's "declare no Vulkan structures" survives this
/// command intact. A *surface* creation command is where that finally breaks,
/// because the loader has to read one to know which platform it is for.
type CreateFn = unsafe extern "C" fn(Handle, *const c_void, *const c_void, *mut u64) -> VkResult;

/// `vkDestroyDebugUtilsMessengerEXT`, as the driver exposes it.
type DestroyFn = unsafe extern "C" fn(Handle, u64, *const c_void);

/// The name the loader resolves on each driver to build a messenger.
pub const CREATE_NAME: &CStr = c"vkCreateDebugUtilsMessengerEXT";

/// The name the loader resolves on each driver to tear one down.
pub const DESTROY_NAME: &CStr = c"vkDestroyDebugUtilsMessengerEXT";

/// One driver's messenger, and which driver it belongs to.
#[derive(Debug, Clone, Copy)]
pub struct DriverMessenger {
    /// Index into [`Registry::drivers`].
    pub driver: usize,
    /// The driver instance this messenger was created on. Held so that
    /// destruction can hand the driver the same `VkInstance` creation used —
    /// the driver's own, never the loader's.
    pub instance: Handle,
    /// The `VkDebugUtilsMessengerEXT` that driver returned.
    pub handle: u64,
}

/// The loader's own `VkDebugUtilsMessengerEXT`: one driver messenger per
/// driver that offered the command.
///
/// Unlike [`Instance`] this is not `#[repr(C)]` and has no dispatch word.
/// Non-dispatchable handles have no dispatch slot — nothing reads offset 0 of
/// this object except the loader itself — and adding one would advertise a
/// layout that is not real.
#[derive(Debug)]
pub struct Messenger {
    drivers: Vec<DriverMessenger>,
}

impl Messenger {
    /// The per-driver messengers this handle stands for.
    #[must_use]
    pub fn drivers(&self) -> &[DriverMessenger] {
        &self.drivers
    }

    /// Leak this messenger and return the 64-bit handle the application keeps.
    ///
    /// The box stops being owned by Rust here; `vkDestroyDebugUtilsMessengerEXT`
    /// is what reclaims it, which is the contract that makes the leak finite.
    #[must_use]
    pub fn into_handle(self: Box<Self>) -> u64 {
        Box::into_raw(self) as u64
    }
}

/// Resolve one command on one driver instance.
///
/// Returns `None` when the driver does not offer the command, which is the
/// ordinary case for a driver that does not implement the extension.
///
/// # Safety
///
/// `driver` must have live entry points and `instance` must be a `VkInstance`
/// that driver created.
unsafe fn resolve(
    registry: &Registry,
    driver: usize,
    instance: Handle,
    name: &CStr,
) -> Option<unsafe extern "C" fn()> {
    let driver = registry.drivers().get(driver)?;
    // SAFETY: forwarded from this function's contract — a driver's own
    // `vkGetInstanceProcAddr` applied to an instance it created.
    unsafe { (driver.instance_proc_addr())(instance, name.as_ptr()) }
}

/// Whether any driver behind `instance` offers the messenger commands.
///
/// This is what lets `vkGetInstanceProcAddr` answer **null** for a machine
/// whose drivers have no `VK_EXT_debug_utils`, instead of handing back a
/// pointer to a function that could only fail. Null is what the C API already
/// means by "no such entry point", and answering it honestly is the shape of
/// the defect this command was filed under, in the one direction the loader
/// had not yet fixed.
///
/// # Safety
///
/// As [`resolve`], for every driver instance.
#[must_use]
pub unsafe fn supported(registry: &Registry, instance: &Instance) -> bool {
    instance.drivers().iter().any(|d| {
        // SAFETY: forwarded from this function's contract.
        unsafe { resolve(registry, d.driver, d.handle, CREATE_NAME) }.is_some()
    })
}

/// Destroy one driver's messenger, if that driver still answers.
///
/// # Safety
///
/// `entry` must name a messenger this loader created on that driver and has
/// not already destroyed, and `allocator` must match the one creation used.
unsafe fn destroy_one(registry: &Registry, entry: DriverMessenger, allocator: *const c_void) {
    // SAFETY: forwarded from this function's contract.
    let Some(f) = (unsafe { resolve(registry, entry.driver, entry.instance, DESTROY_NAME) }) else {
        // A driver that answered the create and not the destroy would be
        // broken, and there is nothing this loader can do about it: there is
        // no other way to free the object. Leaking it is the only alternative
        // to calling a pointer we do not have.
        return;
    };
    // SAFETY: `DestroyFn` is `vkDestroyDebugUtilsMessengerEXT`'s signature,
    // and `f` was resolved under that name from the driver that created
    // `entry.handle`.
    let destroy: DestroyFn = unsafe { core::mem::transmute(f) };
    // SAFETY: the driver is given its own instance and its own messenger,
    // never the loader's wrappers for either.
    unsafe { destroy(entry.instance, entry.handle, allocator) };
}

/// Destroy every driver messenger in `created`.
///
/// # Safety
///
/// As [`destroy_one`], for every element.
pub unsafe fn destroy_across(
    registry: &Registry,
    created: &[DriverMessenger],
    allocator: *const c_void,
) {
    for entry in created {
        // SAFETY: forwarded from this function's contract.
        unsafe { destroy_one(registry, *entry, allocator) };
    }
}

/// Create one messenger on every driver that offers the command.
///
/// Kept separate from the exported C function so the fan-out can be driven
/// against a registry a test built, exactly as `create_across` is for
/// `vkCreateInstance`.
///
/// # Safety
///
/// Every driver in `registry` must have live entry points, and `create_info`
/// and `allocator` must be the application's own.
pub unsafe fn create_across(
    registry: &Registry,
    instance: &Instance,
    create_info: *const c_void,
    allocator: *const c_void,
) -> Result<Box<Messenger>, VkResult> {
    let mut created: Vec<DriverMessenger> = Vec::new();

    for driver_instance in instance.drivers() {
        let DriverInstance { driver, handle } = *driver_instance;
        // SAFETY: forwarded from this function's contract.
        let Some(f) = (unsafe { resolve(registry, driver, handle, CREATE_NAME) }) else {
            // Not an error: this driver does not implement the extension.
            continue;
        };
        // SAFETY: `CreateFn` is `vkCreateDebugUtilsMessengerEXT`'s signature,
        // and `f` was resolved under that name.
        let create: CreateFn = unsafe { core::mem::transmute(f) };

        let mut out: u64 = 0;
        // SAFETY: the driver is given its own instance handle, the
        // application's own create-info and allocator, and a live output slot.
        let result = unsafe { create(handle, create_info, allocator, &raw mut out) };

        if result < VK_SUCCESS {
            // A driver that offered the command and then failed fails the
            // whole call: everything built so far is unwound so that the
            // application's handle is never a partial subscription.
            // SAFETY: every entry is one this loop just created and has not
            // destroyed, with the allocator it was created with.
            unsafe { destroy_across(registry, &created, allocator) };
            return Err(result);
        }

        created.push(DriverMessenger {
            driver,
            instance: handle,
            handle: out,
        });
    }

    Ok(Box::new(Messenger { drivers: created }))
}

/// Reclaim a messenger handle this loader created.
///
/// # Safety
///
/// `messenger` must be zero, or a handle [`Messenger::into_handle`] produced
/// that has not already been reclaimed.
pub unsafe fn from_handle(messenger: u64) -> Option<Box<Messenger>> {
    if messenger == 0 {
        // Vulkan defines destroying a null handle as doing nothing, so that
        // teardown paths need not branch on how far setup got.
        return None;
    }
    // SAFETY: the caller guarantees this is a handle `into_handle` leaked and
    // has not reclaimed, so it is that `Box`'s pointer and reclaiming it here
    // frees it exactly once.
    Some(unsafe { Box::from_raw(messenger as *mut Messenger) })
}

// The five defensive lints the workspace turns on are for production code: a
// test that indexes a fixed-size fixture, or unwraps a value it just
// constructed, is *asserting*, and an assertion that fails by panicking is a
// test doing its job rather than a robustness hole.
//
// `unnecessary_box_returns` is the sixth and is here for a different reason:
// `Instance::new` hands back a `Box` because the instance's *address* is what
// becomes the application's `VkInstance`, so it has to stop moving. A test
// that unboxed it to satisfy the lint would be holding the object in a shape
// production never uses, and the one test that reads the loader's own address
// would be reading a stack slot rather than a handle.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unnecessary_box_returns,
    clippy::unwrap_used
)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::icd::{CURRENT, DriverReply};
    use crate::registry::Entry;
    use crate::vk::{VK_ERROR_OUT_OF_HOST_MEMORY, VoidFn};
    use alloc::vec;
    use core::ffi::c_char;
    use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    /// The stub drivers below report through statics, so the tests that read
    /// them cannot run at the same time.
    ///
    /// Spelled as a lock *on a static* rather than as an associated function,
    /// because `scripts/raced-globals.py` looks for exactly that and was right
    /// to: this was first written as `Order::lock()`, which serialised the
    /// tests correctly and showed a reader nothing. A guard whose name does
    /// not say what it guards is indistinguishable from an unused binding.
    static STUB_LOCK: StubLock = StubLock::new();

    /// A spin lock, because this crate is `no_std` and has no `Mutex`.
    /// `entry.rs` carries its own for the same reason.
    struct StubLock(AtomicBool);

    struct StubGuard(&'static StubLock);

    impl StubLock {
        const fn new() -> Self {
            Self(AtomicBool::new(false))
        }

        fn lock(&'static self) -> StubGuard {
            while self
                .0
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            StubGuard(self)
        }
    }

    impl Drop for StubGuard {
        fn drop(&mut self) {
            self.0.0.store(false, Ordering::Release);
        }
    }

    static CREATED: AtomicUsize = AtomicUsize::new(0);
    static DESTROYED: AtomicUsize = AtomicUsize::new(0);
    static NEXT_HANDLE: AtomicU64 = AtomicU64::new(0);

    /// The `VkInstance` a stub driver was last handed.
    ///
    /// The point of the unwrapping rule: this must be the *driver's* own
    /// handle, never the address of the loader's [`Instance`].
    static SEEN_INSTANCE: AtomicUsize = AtomicUsize::new(0);

    /// The messenger a stub driver was last asked to destroy.
    static SEEN_MESSENGER: AtomicU64 = AtomicU64::new(0);

    fn reset() {
        CREATED.store(0, Ordering::SeqCst);
        DESTROYED.store(0, Ordering::SeqCst);
        // Starts at 1 so that no stub ever hands back 0, which `from_handle`
        // reads as "no messenger" -- a generator that could produce it would
        // make this module's null check look correct for the wrong reason.
        NEXT_HANDLE.store(1, Ordering::SeqCst);
        SEEN_INSTANCE.store(0, Ordering::SeqCst);
        SEEN_MESSENGER.store(0, Ordering::SeqCst);
    }

    unsafe extern "C" fn stub_create(
        instance: Handle,
        _create_info: *const c_void,
        _allocator: *const c_void,
        out: *mut u64,
    ) -> VkResult {
        SEEN_INSTANCE.store(instance as usize, Ordering::SeqCst);
        CREATED.fetch_add(1, Ordering::SeqCst);
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the loader passes a live slot it owns.
        unsafe { out.write(handle) };
        VK_SUCCESS
    }

    unsafe extern "C" fn stub_create_failing(
        _instance: Handle,
        _create_info: *const c_void,
        _allocator: *const c_void,
        _out: *mut u64,
    ) -> VkResult {
        VK_ERROR_OUT_OF_HOST_MEMORY
    }

    unsafe extern "C" fn stub_destroy(instance: Handle, messenger: u64, _allocator: *const c_void) {
        SEEN_INSTANCE.store(instance as usize, Ordering::SeqCst);
        SEEN_MESSENGER.store(messenger, Ordering::SeqCst);
        DESTROYED.fetch_add(1, Ordering::SeqCst);
    }

    fn erase(f: *const ()) -> unsafe extern "C" fn() {
        // SAFETY: every caller passes a real `extern "C"` function of this
        // module, cast only to be cast straight back.
        unsafe { core::mem::transmute::<*const (), unsafe extern "C" fn()>(f) }
    }

    /// A driver that implements `VK_EXT_debug_utils`.
    unsafe extern "C" fn gipa_with_messenger(_instance: Handle, name: *const c_char) -> VoidFn {
        // SAFETY: the loader passes a NUL-terminated name.
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"vkCreateDebugUtilsMessengerEXT" => Some(erase(stub_create as *const ())),
            b"vkDestroyDebugUtilsMessengerEXT" => Some(erase(stub_destroy as *const ())),
            _ => None,
        }
    }

    /// A driver that does not -- the ordinary case, and not an error.
    unsafe extern "C" fn gipa_without_messenger(_instance: Handle, _name: *const c_char) -> VoidFn {
        None
    }

    /// A driver that offers the command and then fails it.
    unsafe extern "C" fn gipa_failing_messenger(_instance: Handle, name: *const c_char) -> VoidFn {
        // SAFETY: the loader passes a NUL-terminated name.
        match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"vkCreateDebugUtilsMessengerEXT" => Some(erase(stub_create_failing as *const ())),
            b"vkDestroyDebugUtilsMessengerEXT" => Some(erase(stub_destroy as *const ())),
            _ => None,
        }
    }

    fn registry_of(lookups: &[unsafe extern "C" fn(Handle, *const c_char) -> VoidFn]) -> Registry {
        let mut registry = Registry::new(CURRENT);
        for lookup in lookups {
            registry.admit(
                "stub",
                Entry {
                    get_instance_proc_addr: *lookup,
                    icd_get_instance_proc_addr: None,
                    get_physical_device_proc_addr: None,
                    negotiate: None,
                },
                DriverReply::Success {
                    reported: CURRENT.get(),
                },
            );
        }
        registry
    }

    /// Distinct, real, non-null addresses to stand in for driver instances.
    /// Taken from statics rather than made up so they carry provenance; no
    /// stub ever dereferences one.
    static DRIVER_INSTANCE_A: u8 = 0;
    static DRIVER_INSTANCE_B: u8 = 0;

    fn handle_a() -> Handle {
        (&raw const DRIVER_INSTANCE_A).cast::<c_void>().cast_mut()
    }

    fn handle_b() -> Handle {
        (&raw const DRIVER_INSTANCE_B).cast::<c_void>().cast_mut()
    }

    /// A null pointer of the type the C signatures take.
    fn nothing() -> *const c_void {
        core::ptr::null()
    }

    /// A loader instance spread over the first `n` drivers of a registry.
    fn instance_over(n: usize) -> Box<Instance> {
        let mut drivers = vec![DriverInstance {
            driver: 0,
            handle: handle_a(),
        }];
        if n > 1 {
            drivers.push(DriverInstance {
                driver: 1,
                handle: handle_b(),
            });
        }
        Instance::new(drivers)
    }

    #[test]
    fn every_driver_that_offers_the_command_gets_a_messenger() {
        let _order = STUB_LOCK.lock();
        reset();
        let registry = registry_of(&[gipa_with_messenger, gipa_with_messenger]);
        let instance = instance_over(2);

        // SAFETY: the drivers behind this instance are this registry's.
        let built =
            unsafe { create_across(&registry, &instance, nothing(), nothing()) }.expect("created");

        assert_eq!(built.drivers().len(), 2, "both drivers offered the command");
        assert_eq!(CREATED.load(Ordering::SeqCst), 2);
        // Distinct handles: a fan-out that called one driver twice, or copied
        // one answer into both slots, would pass a bare count.
        assert_ne!(
            built.drivers().first().expect("first").handle,
            built.drivers().get(1).expect("second").handle,
            "each driver's messenger is its own"
        );
    }

    #[test]
    fn a_driver_without_the_extension_is_not_a_failure() {
        let _order = STUB_LOCK.lock();
        reset();
        let registry = registry_of(&[gipa_with_messenger, gipa_without_messenger]);
        let instance = instance_over(2);

        // SAFETY: the drivers behind this instance are this registry's.
        let built =
            unsafe { create_across(&registry, &instance, nothing(), nothing()) }.expect("created");

        assert_eq!(
            built.drivers().len(),
            1,
            "the driver without the extension contributes nothing, and does not fail the call"
        );
        assert_eq!(CREATED.load(Ordering::SeqCst), 1);

        // The control: the same fan-out over two drivers that *do* offer it
        // builds two. Without this, a `create_across` that silently dropped
        // every driver would pass the assertion above.
        reset();
        let both = registry_of(&[gipa_with_messenger, gipa_with_messenger]);
        // SAFETY: as above.
        let built =
            unsafe { create_across(&both, &instance, nothing(), nothing()) }.expect("created");
        assert_eq!(built.drivers().len(), 2);
    }

    #[test]
    fn a_driver_that_fails_unwinds_the_ones_already_built() {
        let _order = STUB_LOCK.lock();
        reset();
        // Driver 0 succeeds, driver 1 offers the command and fails it.
        let registry = registry_of(&[gipa_with_messenger, gipa_failing_messenger]);
        let instance = instance_over(2);

        // SAFETY: the drivers behind this instance are this registry's.
        let result = unsafe { create_across(&registry, &instance, nothing(), nothing()) };

        assert_eq!(
            result.err(),
            Some(VK_ERROR_OUT_OF_HOST_MEMORY),
            "the failing driver's own error is what the application is told"
        );
        assert_eq!(CREATED.load(Ordering::SeqCst), 1, "driver 0 built one");
        assert_eq!(
            DESTROYED.load(Ordering::SeqCst),
            1,
            "and it was destroyed again, so the call left nothing behind"
        );

        // The control: with driver 1 succeeding instead, nothing is destroyed.
        // Without it, a `create_across` that destroyed on *every* path -- or a
        // stub that counted a destroy it never received -- would pass above.
        reset();
        let both = registry_of(&[gipa_with_messenger, gipa_with_messenger]);
        // SAFETY: as above.
        let built =
            unsafe { create_across(&both, &instance, nothing(), nothing()) }.expect("created");
        assert_eq!(built.drivers().len(), 2);
        assert_eq!(DESTROYED.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_driver_is_given_its_own_instance_and_its_own_messenger() {
        let _order = STUB_LOCK.lock();
        reset();
        let registry = registry_of(&[gipa_with_messenger]);
        let instance = instance_over(1);
        let loader_address = core::ptr::from_ref(&*instance) as usize;

        // SAFETY: the drivers behind this instance are this registry's.
        let built =
            unsafe { create_across(&registry, &instance, nothing(), nothing()) }.expect("created");
        assert_eq!(
            SEEN_INSTANCE.load(Ordering::SeqCst),
            handle_a() as usize,
            "the driver is handed the instance it created"
        );
        assert_ne!(
            SEEN_INSTANCE.load(Ordering::SeqCst),
            loader_address,
            "and never the loader's own object, which it would read as its own"
        );

        let driver_handle = built.drivers().first().expect("one").handle;
        let loader_handle = built.into_handle();
        // SAFETY: the handle was just produced by `into_handle`.
        let reclaimed = unsafe { from_handle(loader_handle) }.expect("round trip");
        // SAFETY: the entry is one `create_across` made and has not destroyed.
        unsafe { destroy_across(&registry, reclaimed.drivers(), nothing()) };

        assert_eq!(DESTROYED.load(Ordering::SeqCst), 1);
        assert_eq!(
            SEEN_MESSENGER.load(Ordering::SeqCst),
            driver_handle,
            "destruction hands back the driver's messenger"
        );
        assert_ne!(
            SEEN_MESSENGER.load(Ordering::SeqCst),
            loader_handle,
            "not the loader's, which is the address of its own box"
        );
    }

    #[test]
    fn no_driver_with_the_extension_means_no_entry_point() {
        let _order = STUB_LOCK.lock();
        reset();
        let none = registry_of(&[gipa_without_messenger, gipa_without_messenger]);
        let instance = instance_over(2);
        // SAFETY: the drivers behind this instance are this registry's.
        assert!(
            !unsafe { supported(&none, &instance) },
            "no driver implements it, so the loader must answer null"
        );

        // The control: one driver having it is enough.
        let some = registry_of(&[gipa_without_messenger, gipa_with_messenger]);
        // SAFETY: as above.
        assert!(unsafe { supported(&some, &instance) });
    }

    #[test]
    fn destroying_a_null_messenger_does_nothing() {
        let _order = STUB_LOCK.lock();
        reset();
        // SAFETY: zero is the documented null case.
        assert!(unsafe { from_handle(0) }.is_none());
        assert_eq!(DESTROYED.load(Ordering::SeqCst), 0);
    }
}
