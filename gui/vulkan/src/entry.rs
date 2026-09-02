//! The Vulkan symbols this loader exports, and the global state behind them.
//!
//! Everything else in this crate is a decision taken on values. This module is
//! where those decisions meet the C ABI: it owns the process-wide driver
//! registry, exports the entry points an application links against, and holds
//! the dispatch table their addresses are handed out from.
//!
//! # What is exported, and what is deliberately not
//!
//! | Symbol | State |
//! |---|---|
//! | `vkGetInstanceProcAddr` | Exported. Answers for every command below and null for everything else. |
//! | `vkCreateInstance` | Exported. Fans out to every registered driver; see [`crate::instance`] for which failure the application is told about. |
//! | `vkDestroyInstance` | Exported. Fans back out and frees the loader's object. |
//! | `vkEnumeratePhysicalDevices` | Exported. Aggregates every driver's devices behind loader-owned handles. |
//! | `vkEnumerateInstanceExtensionProperties` | **Not exported.** Needs the union of every driver's extension list, de-duplicated. |
//! | `vkEnumerateInstanceLayerProperties` | **Not exported.** There are no layers, because loading one needs `dlopen`. |
//! | `vkEnumerateInstanceVersion` | **Not exported.** Its answer depends on the two above. |
//!
//! The three that are missing are missing *loudly*: an application that needs
//! them fails to link, which is a build error naming the symbol. The
//! alternative — exporting them and returning an empty list — is a program that
//! reports success for work it never did, and produces a bug report about the
//! driver rather than about the loader.
//!
//! `vkGetInstanceProcAddr` returning null for a command it does not implement
//! is not that: null is the answer the C API defines for "no such entry point",
//! and every caller already has to handle it.
//!
//! # Registration
//!
//! `vk_slateosRegisterDriver` is how a statically linked driver hands its entry
//! points over. It is the SlateOS-specific part of this file and the only
//! symbol here that is not from Khronos, which is why it carries a vendor
//! prefix rather than a `vk_icd` one — those names belong to the interface, not
//! to us. See the crate documentation for why registration is static at all.
//!
//! # The lock
//!
//! The registry is behind a spin lock rather than being written once at
//! startup, because "once at startup" is an assumption nothing in the type
//! system holds anyone to, and a driver registering while another thread is
//! part-way through `vkCreateInstance` would otherwise read a half-updated
//! `Vec`.
//!
//! The lock is held across the calls the loader makes *into* drivers. That is
//! deliberate — it is what makes the set of drivers a call fans out to a fixed
//! one — and it has one consequence worth stating: a driver that calls
//! `vk_slateosRegisterDriver` from inside its own `vkCreateInstance` deadlocks.
//! No sane driver does that, and the alternative (copying the driver list on
//! every call) buys nothing a real driver would notice.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::ffi::{CStr, c_char, c_void};
use core::ops::{Deref, DerefMut};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::icd::CURRENT;
use crate::instance::{DriverInstance, Instance, PhysicalDevice, adopt_all, array_query, outcome};
use crate::registry::{Admission, Driver, Entry, Registry};
use crate::vk::{
    CreateInstanceFn, DestroyInstanceFn, EnumeratePhysicalDevicesFn, GetInstanceProcAddrFn,
    GetPhysicalDeviceProcAddrFn, Handle, NegotiateFn, VK_ERROR_INCOMPATIBLE_DRIVER,
    VK_ERROR_INITIALIZATION_FAILED, VK_SUCCESS, VkResult, VoidFn,
};

// ---------------------------------------------------------------------------
// A lock
// ---------------------------------------------------------------------------

/// A spin lock, because this crate has no operating system underneath it.
///
/// `gui/vulkan` is `#![no_std]` with no dependencies, so there is no `Mutex` to
/// reach for. Spinning is the right shape here anyway: the critical sections
/// are driver registration, which happens a handful of times at startup, and
/// instance creation, which an application does once. Contention is close to
/// hypothetical, and a lock that blocks would need a scheduler this crate
/// cannot see.
struct SpinLock<T> {
    held: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: `lock` hands out access only to the thread that won the exchange, and
// does not release until the guard is dropped, so at most one reference to the
// value exists at any moment. `T: Send` is what makes handing that access to
// another thread sound; `T: Sync` is not required, because two threads never
// hold it at once.
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    const fn new(value: T) -> Self {
        Self {
            held: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    fn lock(&self) -> Guard<'_, T> {
        while self
            .held
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Guard { lock: self }
    }
}

/// Holds the lock for as long as it lives.
struct Guard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: this guard exists, so this thread holds the lock and no other
        // reference to the value is outstanding.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as above, and `&mut self` proves this is the only borrow of
        // the guard, hence of the value.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.held.store(false, Ordering::Release);
    }
}

/// Every driver linked into this image, in registration order.
///
/// Proposes [`CURRENT`] to each: this loader implements the whole interface it
/// knows about, and a driver that cannot go that high says so in the handshake.
static DRIVERS: SpinLock<Registry> = SpinLock::new(Registry::new(CURRENT));

// ---------------------------------------------------------------------------
// The dispatch table
// ---------------------------------------------------------------------------

/// The loader's instance dispatch table — what the first word of a
/// `VkInstance` or `VkPhysicalDevice` handed to the application points at.
///
/// A `VkPhysicalDevice` shares it: in Vulkan a physical device dispatches
/// through its instance's table rather than owning one.
#[repr(C)]
struct Table {
    get_instance_proc_addr: GetInstanceProcAddrFn,
    destroy_instance: DestroyInstanceFn,
    enumerate_physical_devices: EnumeratePhysicalDevicesFn,
}

/// The one table, for the lifetime of the process.
///
/// A `static` rather than an allocation because every safety comment that
/// installs it needs the table to outlive the object, and `'static` is the only
/// way to say that without tracking a lifetime through a raw pointer.
static TABLE: Table = Table {
    get_instance_proc_addr,
    destroy_instance,
    enumerate_physical_devices,
};

/// [`TABLE`]'s address, in the form the dispatch machinery takes.
fn table() -> *const c_void {
    (&raw const TABLE).cast::<c_void>()
}

/// A concrete function pointer as the `PFN_vkVoidFunction` the C API returns.
///
/// Every `GetProcAddr` in Vulkan does this: it hands back a pointer stripped of
/// its signature, and the caller — which asked by name and therefore knows the
/// signature — casts it back. The type system cannot help on either side of
/// that, so the loader's only real obligation is to never return a pointer
/// under a name that does not match it, which is why the table above and
/// [`get_instance_proc_addr`] are the same three entries.
///
/// Returns the bare pointer rather than the `Option` the C API uses, so that
/// the two questions stay separate: this function erases a signature, and its
/// caller decides whether the loader has the command at all. Wrapping here
/// would make every answer a `Some`, which is exactly what `None` has to be
/// able to contradict.
///
/// # Safety
///
/// `f` must be a function pointer that was cast to `*const ()`, not a data
/// pointer. Every call site below casts one in the same expression.
unsafe fn erase(f: *const ()) -> unsafe extern "C" fn() {
    // SAFETY: the caller guarantees `f` holds a function's address, so
    // transmuting it back to a function pointer of *some* signature restores
    // exactly what was cast in.
    unsafe { core::mem::transmute::<*const (), unsafe extern "C" fn()>(f) }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// `vk_slateosRegisterDriver` — hand this loader a statically linked driver.
///
/// Returns `VK_SUCCESS` if the driver was accepted, `VK_ERROR_INCOMPATIBLE_DRIVER`
/// if the interface-version handshake ruled it out, and
/// `VK_ERROR_INITIALIZATION_FAILED` if the arguments were unusable — a null or
/// non-UTF-8 name, or no `vkGetInstanceProcAddr`, which every driver has at
/// every interface version.
///
/// A rejected driver is not forgotten: the reason is kept in the registry, so
/// that "no Vulkan devices found" can later be answered with which driver was
/// turned away and why.
///
/// # Safety
///
/// `name` must be a NUL-terminated string that lives for the rest of the
/// process — it is stored, not copied. Every non-null function pointer must be
/// live, callable from any thread, and must remain so for the rest of the
/// process, since the loader keeps and calls them.
#[unsafe(export_name = "vk_slateosRegisterDriver")]
pub unsafe extern "C" fn register_driver(
    name: *const c_char,
    get_instance_proc_addr: Option<GetInstanceProcAddrFn>,
    icd_get_instance_proc_addr: Option<GetInstanceProcAddrFn>,
    get_physical_device_proc_addr: Option<GetPhysicalDeviceProcAddrFn>,
    negotiate: Option<NegotiateFn>,
) -> VkResult {
    if name.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    let Some(get_instance_proc_addr) = get_instance_proc_addr else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: the caller guarantees a NUL-terminated string that outlives the
    // process, which is what makes the `'static` this is bound to honest.
    let name: &'static CStr = unsafe { CStr::from_ptr(name) };
    let Ok(name) = name.to_str() else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };

    let entry = Entry {
        get_instance_proc_addr,
        icd_get_instance_proc_addr,
        get_physical_device_proc_addr,
        negotiate,
    };

    // SAFETY: the caller guarantees every pointer in `entry` is live and
    // callable, which is exactly what `register` requires of them.
    match unsafe { DRIVERS.lock().register(name, entry) } {
        Admission::Accepted(_) => VK_SUCCESS,
        Admission::Rejected(_) => VK_ERROR_INCOMPATIBLE_DRIVER,
    }
}

// ---------------------------------------------------------------------------
// Looking up an entry point
// ---------------------------------------------------------------------------

/// `vkGetInstanceProcAddr`.
///
/// Which commands are answerable depends on whether there is an instance, and
/// the rule is Vulkan's rather than this loader's: a *global* command such as
/// `vkCreateInstance` is looked up with a null instance, because there is no
/// instance yet, and an *instance-level* command is looked up with the instance
/// it will be called on. Asking for either through the wrong one is null.
///
/// `vkGetInstanceProcAddr` itself answers to both, which is what lets an
/// application bootstrap: it is the only symbol it needs to find by other means.
///
/// # Safety
///
/// `name` must be null or a NUL-terminated string valid for this call, and
/// `instance` must be null or a `VkInstance` this loader created and has not
/// destroyed.
#[unsafe(export_name = "vkGetInstanceProcAddr")]
pub unsafe extern "C" fn get_instance_proc_addr(instance: Handle, name: *const c_char) -> VoidFn {
    if name.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a NUL-terminated string valid for the call,
    // and the borrow does not outlive this function.
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();

    // SAFETY: every argument to `erase` below is a function pointer cast in
    // place, which is the whole of its contract.
    unsafe {
        if name == b"vkGetInstanceProcAddr" {
            return Some(erase(TABLE.get_instance_proc_addr as *const ()));
        }

        if instance.is_null() {
            return if name == b"vkCreateInstance" {
                // Not reached through the table: with no instance there is
                // nothing to dispatch through, which is the whole reason this
                // command is looked up with a null handle.
                Some(erase(create_instance as *const ()))
            } else {
                None
            };
        }

        match name {
            b"vkDestroyInstance" => Some(erase(TABLE.destroy_instance as *const ())),
            b"vkEnumeratePhysicalDevices" => {
                Some(erase(TABLE.enumerate_physical_devices as *const ()))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Creating and destroying an instance
// ---------------------------------------------------------------------------

/// Ask one driver for an instance.
///
/// Returns the result to record for that driver and the handle it produced —
/// null unless the result is a success.
///
/// # Safety
///
/// `driver`'s entry points must be live, and `create_info` and `allocator` must
/// be whatever the application passed, unaltered.
unsafe fn create_one(
    driver: &Driver,
    create_info: *const c_void,
    allocator: *const c_void,
) -> (VkResult, Handle) {
    let lookup = driver.instance_proc_addr();
    // SAFETY: the caller guarantees `lookup` is live; a null instance is the
    // correct handle for looking up a global command.
    let found = unsafe { lookup(ptr::null_mut(), c"vkCreateInstance".as_ptr()) };
    let Some(found) = found else {
        // A driver with no `vkCreateInstance` cannot serve anybody. Recorded as
        // an incompatibility rather than a failure, because that is what it is:
        // nothing went wrong, this driver simply cannot be used.
        return (VK_ERROR_INCOMPATIBLE_DRIVER, ptr::null_mut());
    };
    // SAFETY: the driver returned this pointer for the name `vkCreateInstance`,
    // which is the contract that fixes its signature.
    let create = unsafe { core::mem::transmute::<unsafe extern "C" fn(), CreateInstanceFn>(found) };

    let mut handle: Handle = ptr::null_mut();
    // SAFETY: `handle` is a writable slot that outlives the call, and the two
    // structure pointers are the application's own, passed through untouched.
    let result = unsafe { create(create_info, allocator, &raw mut handle) };

    if result >= VK_SUCCESS && handle.is_null() {
        // A driver that reports success and hands back nothing has not created
        // an instance. Believing both halves would put a null into the fan-out
        // list and crash on the first call through it; believing the handle
        // over the code costs at worst one usable driver on a broken system.
        return (VK_ERROR_INITIALIZATION_FAILED, ptr::null_mut());
    }
    (result, handle)
}

/// Tell one driver to destroy an instance it created.
///
/// # Safety
///
/// `handle` must be an instance `driver` created and has not destroyed, and
/// `allocator` must be the one creation was given.
unsafe fn destroy_one(driver: &Driver, handle: Handle, allocator: *const c_void) {
    let lookup = driver.instance_proc_addr();
    // SAFETY: the caller's guarantees cover `lookup` and `handle`.
    let found = unsafe { lookup(handle, c"vkDestroyInstance".as_ptr()) };
    let Some(found) = found else {
        // A driver that can create an instance and not destroy one leaks it.
        // There is nothing the loader can do about that and nowhere to report
        // it from — `vkDestroyInstance` returns void — so the leak is the
        // driver's, and this is the note saying it was noticed.
        return;
    };
    // SAFETY: the driver returned this pointer for the name
    // `vkDestroyInstance`, which is the contract that fixes its signature.
    let destroy =
        unsafe { core::mem::transmute::<unsafe extern "C" fn(), DestroyInstanceFn>(found) };
    // SAFETY: forwarded from this function's contract.
    unsafe { destroy(handle, allocator) };
}

/// Destroy every driver instance in `created`.
///
/// # Safety
///
/// As [`destroy_one`], for every element, and each `driver` index must address
/// `registry`.
unsafe fn destroy_across(
    registry: &Registry,
    created: &[DriverInstance],
    allocator: *const c_void,
) {
    for instance in created {
        let Some(driver) = registry.drivers().get(instance.driver) else {
            continue;
        };
        // SAFETY: forwarded from this function's contract.
        unsafe { destroy_one(driver, instance.handle, allocator) };
    }
}

/// `vkCreateInstance`, minus the C signature: build one loader instance over
/// every driver in `registry`, or report why not.
///
/// Kept separate from [`create_instance`] so that the fan-out can be driven
/// against a registry a test built, rather than against whatever drivers this
/// image happens to have been linked with.
///
/// # Safety
///
/// Every driver in `registry` must have live entry points, and `create_info`
/// and `allocator` must be the application's own.
unsafe fn create_across(
    registry: &Registry,
    create_info: *const c_void,
    allocator: *const c_void,
) -> Result<Box<Instance>, VkResult> {
    let mut attempts: Vec<VkResult> = Vec::with_capacity(registry.drivers().len());
    let mut created: Vec<DriverInstance> = Vec::new();

    for (index, driver) in registry.drivers().iter().enumerate() {
        // SAFETY: forwarded from this function's contract.
        let (result, handle) = unsafe { create_one(driver, create_info, allocator) };
        attempts.push(result);
        if result >= VK_SUCCESS {
            created.push(DriverInstance {
                driver: index,
                handle,
            });
        }
    }

    if let Err(code) = outcome(&attempts) {
        // `created` is empty here, because `outcome` reports an error only when
        // no driver succeeded. Written as an unwind rather than as an assertion
        // so that a later change to that policy cannot start leaking instances
        // silently.
        // SAFETY: every handle came from the driver at its recorded index.
        unsafe { destroy_across(registry, &created, allocator) };
        return Err(code);
    }

    let handles: Vec<Handle> = created.iter().map(|i| i.handle).collect();
    // SAFETY: each handle is a dispatchable object a driver just returned, so
    // it is live, aligned, and — if the driver follows the interface — stamped.
    if unsafe { adopt_all(&handles, table()) }.is_err() {
        // One driver returned an object without the loader magic, so the
        // interface contract it was accepted under does not hold. See
        // `crate::instance` for why the whole batch is refused rather than the
        // one driver: a partially adopted set has no correct next step.
        // SAFETY: as above.
        unsafe { destroy_across(registry, &created, allocator) };
        return Err(VK_ERROR_INITIALIZATION_FAILED);
    }

    let mut instance = Instance::new(created);
    // SAFETY: `TABLE` is a `static`, so it outlives every possible use.
    if unsafe { instance.install_table(table()) }.is_err() {
        // Unreachable: `Instance::new` stamps the object this just checked. Not
        // an `unwrap`, because the cost of handling it is three lines and the
        // cost of being wrong is a corrupted handle in the application's hands.
        // SAFETY: the instances are still the ones the drivers created.
        unsafe { destroy_across(registry, instance.drivers(), allocator) };
        return Err(VK_ERROR_INITIALIZATION_FAILED);
    }

    Ok(instance)
}

/// `vkCreateInstance`.
///
/// # Safety
///
/// `create_info` must be a valid `VkInstanceCreateInfo*`, `allocator` a valid
/// `VkAllocationCallbacks*` or null, and `out` a writable `VkInstance` slot.
#[unsafe(export_name = "vkCreateInstance")]
pub unsafe extern "C" fn create_instance(
    create_info: *const c_void,
    allocator: *const c_void,
    out: *mut Handle,
) -> VkResult {
    if out.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    let registry = DRIVERS.lock();
    // SAFETY: every registered driver's entry points were guaranteed live for
    // the process by whoever registered it, and the two structure pointers are
    // the caller's.
    match unsafe { create_across(&registry, create_info, allocator) } {
        Ok(instance) => {
            // SAFETY: `out` was checked non-null and the caller guarantees it
            // is writable. The `Box` is leaked on purpose: the application owns
            // it now, and `vkDestroyInstance` is what reclaims it.
            unsafe { *out = Box::into_raw(instance).cast::<c_void>() };
            VK_SUCCESS
        }
        Err(code) => code,
    }
}

/// `vkDestroyInstance`.
///
/// # Safety
///
/// `instance` must be null or a `VkInstance` this loader created and has not
/// already destroyed, and `allocator` must match the one creation was given.
#[unsafe(export_name = "vkDestroyInstance")]
pub unsafe extern "C" fn destroy_instance(instance: Handle, allocator: *const c_void) {
    if instance.is_null() {
        // Vulkan defines destroying a null handle as doing nothing, so that
        // teardown paths need not branch on how far setup got.
        return;
    }
    // SAFETY: the caller guarantees this is a handle `vkCreateInstance`
    // returned and has not destroyed, so it is the pointer that call leaked
    // from a `Box`, and reclaiming it here frees it exactly once. The physical
    // device wrappers it owns are freed with it.
    let instance: Box<Instance> = unsafe { Box::from_raw(instance.cast::<Instance>()) };
    let registry = DRIVERS.lock();
    // SAFETY: each driver instance is one that driver created, and this is the
    // only path that destroys it.
    unsafe { destroy_across(&registry, instance.drivers(), allocator) };
}

// ---------------------------------------------------------------------------
// Physical devices
// ---------------------------------------------------------------------------

/// Every physical device one driver reports for one of its instances.
///
/// A driver that cannot answer, or answers with an error, contributes nothing.
/// That is not a swallowed failure: a driver with no devices and a driver that
/// failed to list them are the same thing to an application, which either finds
/// a device it can use among the others or finds none at all — and the second
/// case is already reported, by `vkEnumeratePhysicalDevices` returning an empty
/// set.
///
/// # Safety
///
/// `driver`'s entry points must be live and `instance` must be an instance it
/// created.
unsafe fn devices_of(driver: &Driver, instance: Handle) -> Vec<Handle> {
    let lookup = driver.instance_proc_addr();
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { lookup(instance, c"vkEnumeratePhysicalDevices".as_ptr()) };
    let Some(found) = found else {
        return Vec::new();
    };
    // SAFETY: the driver returned this pointer for the name
    // `vkEnumeratePhysicalDevices`, which is the contract that fixes its
    // signature.
    let enumerate = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), EnumeratePhysicalDevicesFn>(found)
    };

    let mut count: u32 = 0;
    // SAFETY: `count` is a writable `u32` that outlives the call, and a null
    // array is how the C API asks for the count alone.
    if unsafe { enumerate(instance, &raw mut count, ptr::null_mut()) } < VK_SUCCESS {
        return Vec::new();
    }

    let mut handles: Vec<Handle> = vec![ptr::null_mut(); count as usize];
    // SAFETY: `handles` has room for `count` entries, which is what `count`
    // now says, and both pointers outlive the call.
    let result = unsafe { enumerate(instance, &raw mut count, handles.as_mut_ptr()) };
    if result < VK_SUCCESS {
        return Vec::new();
    }

    // The driver may have written fewer than it first said — devices can go
    // away between the two calls — and it reports that by lowering `count`.
    // Trusting the first number here would hand out uninitialised handles.
    handles.truncate(count as usize);
    handles.retain(|handle| !handle.is_null());
    handles
}

/// Wrap every driver's devices into loader-owned handles, in driver order.
///
/// # Safety
///
/// Every driver in `registry` must have live entry points, and each element of
/// `instances` must be an instance created by the driver at its recorded index.
// Each device is boxed because its address becomes the application's
// `VkPhysicalDevice` and must outlive every later change to the collection
// holding it; see the field docs on `Instance::physical_devices`.
#[allow(clippy::vec_box)]
unsafe fn enumerate_across(
    registry: &Registry,
    instances: &[DriverInstance],
) -> Result<Vec<Box<PhysicalDevice>>, VkResult> {
    let mut devices: Vec<Box<PhysicalDevice>> = Vec::new();
    for instance in instances {
        let Some(driver) = registry.drivers().get(instance.driver) else {
            continue;
        };
        // SAFETY: forwarded from this function's contract.
        for handle in unsafe { devices_of(driver, instance.handle) } {
            let mut wrapper = PhysicalDevice::new(instance.driver, handle);
            // SAFETY: `TABLE` is a `static`, so it outlives every use.
            if unsafe { wrapper.install_table(table()) }.is_err() {
                // Unreachable: `PhysicalDevice::new` stamps what this checks.
                // Reported rather than skipped, because a device quietly
                // missing from the list is the failure mode that gets
                // diagnosed as "the loader does not see my GPU".
                return Err(VK_ERROR_INITIALIZATION_FAILED);
            }
            devices.push(wrapper);
        }
    }
    Ok(devices)
}

/// `vkEnumeratePhysicalDevices`.
///
/// # Safety
///
/// `instance` must be a `VkInstance` this loader created and has not destroyed,
/// `count` a writable `u32`, and `out` either null or an array of at least
/// `*count` handles.
#[unsafe(export_name = "vkEnumeratePhysicalDevices")]
pub unsafe extern "C" fn enumerate_physical_devices(
    instance: Handle,
    count: *mut u32,
    out: *mut Handle,
) -> VkResult {
    if instance.is_null() || count.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SAFETY: the caller guarantees this handle came from this loader's
    // `vkCreateInstance`, so it points at a live `Instance` no one else holds.
    let instance: &mut Instance = unsafe { &mut *instance.cast::<Instance>() };

    if !instance.devices_enumerated() {
        let registry = DRIVERS.lock();
        // SAFETY: the driver indices in this instance came from this registry,
        // and every registered driver's entry points are live for the process.
        match unsafe { enumerate_across(&registry, instance.drivers()) } {
            Ok(devices) => instance.set_physical_devices(devices),
            Err(code) => return code,
        }
    }

    let available = instance.physical_devices().len();
    if out.is_null() {
        // SAFETY: `count` is the caller's writable `u32`.
        unsafe { *count = clamp_count(available) };
        return VK_SUCCESS;
    }

    // SAFETY: as above; the first call left the capacity here.
    let capacity = unsafe { *count } as usize;
    let (writable, result) = array_query(available, capacity);

    for (slot, device) in instance
        .physical_devices()
        .iter()
        .take(writable)
        .enumerate()
    {
        let handle = ptr::from_ref::<PhysicalDevice>(device)
            .cast_mut()
            .cast::<c_void>();
        // SAFETY: the caller guarantees `out` holds at least `capacity`
        // handles, and `slot < writable <= capacity`.
        unsafe { *out.add(slot) = handle };
    }

    // SAFETY: `count` is the caller's writable `u32`.
    unsafe { *count = clamp_count(writable) };
    result
}

/// A device count as the `u32` the C API reports it in.
///
/// Saturating rather than wrapping. Neither can happen — it would take four
/// billion GPUs — but of the two impossible answers, "more devices than fit in
/// the count" is the one that does not silently report a small number and hand
/// back a truncated list.
fn clamp_count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
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
        SpinLock, create_across, destroy_across, enumerate_across, get_instance_proc_addr, table,
    };
    use crate::dispatch::{ICD_LOADER_MAGIC, is_loader_magic};
    use crate::icd::{CURRENT, DriverReply};
    use crate::registry::{Entry, Registry};
    use crate::vk::{
        Handle, VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INITIALIZATION_FAILED, VK_INCOMPLETE,
        VK_SUCCESS, VkResult, VoidFn,
    };
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::ffi::{CStr, c_char, c_void};
    use core::ptr;
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Serialises the tests that touch [`LIVE`]. Nothing in the loader needs
    /// this; a shared counter across parallel tests does.
    static ORDER: SpinLock<()> = SpinLock::new(());

    /// Driver instances created and not yet destroyed, across all stub drivers.
    static LIVE: AtomicUsize = AtomicUsize::new(0);

    /// A stub driver's `VkPhysicalDevice`.
    #[repr(C)]
    struct FakeDevice {
        loader_data: usize,
    }

    /// A stub driver's `VkInstance`, owning its own devices so that their
    /// addresses are stable for as long as the instance is — which is what
    /// Vulkan promises an application about physical device handles.
    #[repr(C)]
    struct FakeInstance {
        loader_data: usize,
        // Boxed for the reason in the doc comment above: dropping the `Box` on
        // clippy's advice would make this stub model a driver that moves its
        // physical devices around, which no conforming driver may do — and the
        // loader would then be tested against a driver it must never meet.
        #[allow(clippy::vec_box)]
        devices: Vec<Box<FakeDevice>>,
    }

    unsafe fn make_instance(devices: usize, out: *mut Handle) -> VkResult {
        let instance = Box::new(FakeInstance {
            loader_data: ICD_LOADER_MAGIC as usize,
            devices: (0..devices)
                .map(|_| {
                    Box::new(FakeDevice {
                        loader_data: ICD_LOADER_MAGIC as usize,
                    })
                })
                .collect(),
        });
        LIVE.fetch_add(1, Ordering::SeqCst);
        unsafe { *out = Box::into_raw(instance).cast::<c_void>() };
        VK_SUCCESS
    }

    unsafe extern "C" fn create_with_one_device(
        _create_info: *const c_void,
        _allocator: *const c_void,
        out: *mut Handle,
    ) -> VkResult {
        unsafe { make_instance(1, out) }
    }

    unsafe extern "C" fn create_with_two_devices(
        _create_info: *const c_void,
        _allocator: *const c_void,
        out: *mut Handle,
    ) -> VkResult {
        unsafe { make_instance(2, out) }
    }

    unsafe extern "C" fn create_refusing(
        _create_info: *const c_void,
        _allocator: *const c_void,
        _out: *mut Handle,
    ) -> VkResult {
        VK_ERROR_INCOMPATIBLE_DRIVER
    }

    /// Reports success and writes nothing — the broken driver `create_one`
    /// exists to catch.
    unsafe extern "C" fn create_lying(
        _create_info: *const c_void,
        _allocator: *const c_void,
        _out: *mut Handle,
    ) -> VkResult {
        VK_SUCCESS
    }

    /// Returns an instance that was never stamped with the loader magic.
    unsafe extern "C" fn create_unstamped(
        _create_info: *const c_void,
        _allocator: *const c_void,
        out: *mut Handle,
    ) -> VkResult {
        let instance = Box::new(FakeInstance {
            loader_data: 0,
            devices: Vec::new(),
        });
        LIVE.fetch_add(1, Ordering::SeqCst);
        unsafe { *out = Box::into_raw(instance).cast::<c_void>() };
        VK_SUCCESS
    }

    unsafe extern "C" fn destroy(instance: Handle, _allocator: *const c_void) {
        drop(unsafe { Box::from_raw(instance.cast::<FakeInstance>()) });
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn enumerate(
        instance: Handle,
        count: *mut u32,
        out: *mut Handle,
    ) -> VkResult {
        let instance = unsafe { &*instance.cast::<FakeInstance>() };
        let available = instance.devices.len();
        if out.is_null() {
            unsafe { *count = available as u32 };
            return VK_SUCCESS;
        }
        let capacity = unsafe { *count } as usize;
        let writable = capacity.min(available);
        for (slot, device) in instance.devices.iter().take(writable).enumerate() {
            let handle = ptr::from_ref::<FakeDevice>(device)
                .cast_mut()
                .cast::<c_void>();
            unsafe { *out.add(slot) = handle };
        }
        unsafe { *count = writable as u32 };
        if writable < available {
            VK_INCOMPLETE
        } else {
            VK_SUCCESS
        }
    }

    /// The `vkGetInstanceProcAddr` every stub driver shares, parameterised by
    /// which `vkCreateInstance` it hands out.
    unsafe fn answer(name: *const c_char, create: *const ()) -> VoidFn {
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        let f: *const () = match name {
            b"vkCreateInstance" => create,
            b"vkDestroyInstance" => destroy as *const (),
            b"vkEnumeratePhysicalDevices" => enumerate as *const (),
            _ => return None,
        };
        Some(unsafe { core::mem::transmute::<*const (), unsafe extern "C" fn()>(f) })
    }

    unsafe extern "C" fn gipa_one_device(_instance: Handle, name: *const c_char) -> VoidFn {
        unsafe { answer(name, create_with_one_device as *const ()) }
    }

    unsafe extern "C" fn gipa_two_devices(_instance: Handle, name: *const c_char) -> VoidFn {
        unsafe { answer(name, create_with_two_devices as *const ()) }
    }

    unsafe extern "C" fn gipa_refusing(_instance: Handle, name: *const c_char) -> VoidFn {
        unsafe { answer(name, create_refusing as *const ()) }
    }

    unsafe extern "C" fn gipa_lying(_instance: Handle, name: *const c_char) -> VoidFn {
        unsafe { answer(name, create_lying as *const ()) }
    }

    unsafe extern "C" fn gipa_unstamped(_instance: Handle, name: *const c_char) -> VoidFn {
        unsafe { answer(name, create_unstamped as *const ()) }
    }

    /// Creates instances happily and has no `vkEnumeratePhysicalDevices`.
    unsafe extern "C" fn gipa_cannot_list(_instance: Handle, name: *const c_char) -> VoidFn {
        if unsafe { CStr::from_ptr(name) }.to_bytes() == b"vkEnumeratePhysicalDevices" {
            return None;
        }
        unsafe { answer(name, create_with_one_device as *const ()) }
    }

    /// A driver that exports nothing at all.
    unsafe extern "C" fn gipa_empty(_instance: Handle, _name: *const c_char) -> VoidFn {
        None
    }

    /// A registry holding the named stub drivers, all settled at [`CURRENT`].
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

    #[test]
    fn every_driver_that_succeeds_is_in_the_fan_out() {
        let _order = ORDER.lock();
        let registry = registry_of(&[gipa_one_device, gipa_two_devices]);
        let before = LIVE.load(Ordering::SeqCst);

        let instance = unsafe { create_across(&registry, ptr::null(), ptr::null()) }.unwrap();
        assert_eq!(instance.drivers().len(), 2);
        assert_eq!(LIVE.load(Ordering::SeqCst), before + 2);
        assert_eq!(
            instance.dispatch_word(),
            table() as usize,
            "the application would dispatch through the loader magic"
        );

        unsafe { destroy_across(&registry, instance.drivers(), ptr::null()) };
        assert_eq!(LIVE.load(Ordering::SeqCst), before);
    }

    #[test]
    fn a_refusing_driver_does_not_take_the_others_down_with_it() {
        // The rule with the most user-visible consequence, end to end this
        // time: the application still starts.
        let _order = ORDER.lock();
        let registry = registry_of(&[gipa_refusing, gipa_one_device]);
        let before = LIVE.load(Ordering::SeqCst);

        let instance = unsafe { create_across(&registry, ptr::null(), ptr::null()) }.unwrap();
        assert_eq!(instance.drivers().len(), 1);
        assert_eq!(
            instance.drivers()[0].driver,
            1,
            "the surviving instance was attributed to the driver that refused"
        );

        unsafe { destroy_across(&registry, instance.drivers(), ptr::null()) };
        assert_eq!(LIVE.load(Ordering::SeqCst), before);
    }

    #[test]
    fn every_driver_refusing_is_reported_as_incompatible() {
        let registry = registry_of(&[gipa_refusing, gipa_refusing]);
        let result = unsafe { create_across(&registry, ptr::null(), ptr::null()) };
        assert_eq!(result.err(), Some(VK_ERROR_INCOMPATIBLE_DRIVER));
    }

    #[test]
    fn a_registry_with_no_drivers_is_reported_as_incompatible() {
        // `LDP_LOADER_1`, reached through the whole fan-out rather than through
        // `outcome` alone.
        let registry = registry_of(&[]);
        let result = unsafe { create_across(&registry, ptr::null(), ptr::null()) };
        assert_eq!(result.err(), Some(VK_ERROR_INCOMPATIBLE_DRIVER));
    }

    #[test]
    fn a_driver_with_no_create_instance_is_skipped_not_believed() {
        let registry = registry_of(&[gipa_empty]);
        let result = unsafe { create_across(&registry, ptr::null(), ptr::null()) };
        assert_eq!(result.err(), Some(VK_ERROR_INCOMPATIBLE_DRIVER));
    }

    #[test]
    fn a_driver_that_reports_success_and_returns_nothing_is_not_believed() {
        // Believing both halves would put a null handle in the fan-out list.
        let registry = registry_of(&[gipa_lying]);
        let result = unsafe { create_across(&registry, ptr::null(), ptr::null()) };
        assert_eq!(
            result.err(),
            Some(VK_ERROR_INITIALIZATION_FAILED),
            "a null instance was accepted as a created one"
        );
    }

    #[test]
    fn an_instance_without_the_loader_magic_sinks_the_whole_call() {
        let _order = ORDER.lock();
        let registry = registry_of(&[gipa_one_device, gipa_unstamped]);
        let before = LIVE.load(Ordering::SeqCst);

        let result = unsafe { create_across(&registry, ptr::null(), ptr::null()) };
        assert_eq!(result.err(), Some(VK_ERROR_INITIALIZATION_FAILED));
        assert_eq!(
            LIVE.load(Ordering::SeqCst),
            before,
            "the instances created before the bad one were leaked"
        );
    }

    #[test]
    fn physical_devices_are_aggregated_across_drivers_in_order() {
        let _order = ORDER.lock();
        let registry = registry_of(&[gipa_one_device, gipa_two_devices]);
        let instance = unsafe { create_across(&registry, ptr::null(), ptr::null()) }.unwrap();

        let devices = unsafe { enumerate_across(&registry, instance.drivers()) }.unwrap();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].driver(), 0);
        assert_eq!(devices[1].driver(), 1);
        assert_eq!(devices[2].driver(), 1);
        for device in &devices {
            assert_eq!(device.dispatch_word(), table() as usize);
            assert!(
                !is_loader_magic(device.dispatch_word()),
                "the wrapper still carries the magic, so no table was installed"
            );
        }

        unsafe { destroy_across(&registry, instance.drivers(), ptr::null()) };
    }

    #[test]
    fn wrapping_a_device_leaves_the_drivers_own_handle_alone() {
        // The loader passes these back down to the driver; overwriting their
        // dispatch word would be writing into the driver's object.
        let _order = ORDER.lock();
        let registry = registry_of(&[gipa_two_devices]);
        let instance = unsafe { create_across(&registry, ptr::null(), ptr::null()) }.unwrap();

        let devices = unsafe { enumerate_across(&registry, instance.drivers()) }.unwrap();
        for device in &devices {
            let driver_object = unsafe { &*device.handle().cast::<FakeDevice>() };
            assert!(
                is_loader_magic(driver_object.loader_data),
                "the driver's own device object was overwritten"
            );
        }

        unsafe { destroy_across(&registry, instance.drivers(), ptr::null()) };
    }

    #[test]
    fn a_driver_that_cannot_list_devices_contributes_none_and_blocks_nobody() {
        // The instance it created is still real and still gets destroyed; it
        // simply appears in no device list.
        let _order = ORDER.lock();
        let registry = registry_of(&[gipa_cannot_list, gipa_two_devices]);
        let before = LIVE.load(Ordering::SeqCst);

        let instance = unsafe { create_across(&registry, ptr::null(), ptr::null()) }.unwrap();
        assert_eq!(instance.drivers().len(), 2);

        let devices = unsafe { enumerate_across(&registry, instance.drivers()) }.unwrap();
        assert_eq!(devices.len(), 2);
        assert!(
            devices.iter().all(|d| d.driver() == 1),
            "a device was attributed to the driver that cannot list any"
        );

        unsafe { destroy_across(&registry, instance.drivers(), ptr::null()) };
        assert_eq!(LIVE.load(Ordering::SeqCst), before);
    }

    fn lookup(instance: Handle, name: &CStr) -> VoidFn {
        unsafe { get_instance_proc_addr(instance, name.as_ptr()) }
    }

    /// An address that stands in for an instance. Never dereferenced, because
    /// `vkGetInstanceProcAddr` only tests it against null.
    fn an_instance() -> Handle {
        ptr::without_provenance_mut::<c_void>(0x1234_5678)
    }

    #[test]
    fn get_proc_addr_answers_for_itself_with_or_without_an_instance() {
        // The bootstrap: this is the only symbol an application can be required
        // to find by other means.
        assert!(lookup(ptr::null_mut(), c"vkGetInstanceProcAddr").is_some());
        assert!(lookup(an_instance(), c"vkGetInstanceProcAddr").is_some());
    }

    #[test]
    fn a_global_command_is_answerable_only_without_an_instance() {
        assert!(lookup(ptr::null_mut(), c"vkCreateInstance").is_some());
        assert!(
            lookup(an_instance(), c"vkCreateInstance").is_none(),
            "a global command answered through an instance"
        );
    }

    #[test]
    fn an_instance_command_is_answerable_only_with_an_instance() {
        assert!(lookup(an_instance(), c"vkDestroyInstance").is_some());
        assert!(lookup(an_instance(), c"vkEnumeratePhysicalDevices").is_some());
        assert!(
            lookup(ptr::null_mut(), c"vkDestroyInstance").is_none(),
            "an instance command answered with no instance"
        );
    }

    #[test]
    fn an_unimplemented_command_is_null_rather_than_a_wrong_pointer() {
        // Null is the C API's own answer for "no such entry point". Returning
        // anything else here is how a loader ships a jump to the wrong function.
        assert!(lookup(an_instance(), c"vkCreateDevice").is_none());
        assert!(lookup(ptr::null_mut(), c"vkEnumerateInstanceVersion").is_none());
        assert!(lookup(an_instance(), c"").is_none());
    }

    #[test]
    fn a_null_name_is_answered_rather_than_dereferenced() {
        assert!(unsafe { get_instance_proc_addr(ptr::null_mut(), ptr::null()) }.is_none());
    }
}
