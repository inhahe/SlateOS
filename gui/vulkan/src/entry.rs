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
//! | `vkCreateDevice` | Exported. Creates the device on the one driver behind the physical device, and adopts the handle. |
//! | `vkGetDeviceProcAddr` | Exported. Answers for itself and `vkDestroyDevice`; forwards everything else to the driver. |
//! | `vkDestroyDevice` | Exported. Destroys the driver's device and frees the loader's record for it. |
//! | *every other device command* | **Not exported, and not a gap.** A `VkDevice` this loader returns is the driver's own, so `vkCmdDraw` and its several hundred siblings are found through the driver's `vkGetDeviceProcAddr` and called with the loader nowhere in the path. |
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

use crate::device::{self, Device, LoaderCommand, Lookup};
use crate::dispatch;
use crate::icd::CURRENT;
use crate::instance::{DriverInstance, Instance, PhysicalDevice, adopt_all, array_query, outcome};
use crate::physical::{self, Ask, Command};
use crate::registry::{Admission, Driver, Entry, Registry};
use crate::vk::{
    CreateDeviceFn, CreateInstanceFn, DestroyDeviceFn, DestroyInstanceFn,
    EnumerateDeviceExtensionPropertiesFn, EnumerateDeviceLayerPropertiesFn,
    EnumeratePhysicalDevicesFn, GetDeviceProcAddrFn, GetInstanceProcAddrFn,
    GetPhysicalDeviceFeaturesFn, GetPhysicalDeviceFormatPropertiesFn,
    GetPhysicalDeviceImageFormatPropertiesFn, GetPhysicalDeviceMemoryPropertiesFn,
    GetPhysicalDeviceProcAddrFn, GetPhysicalDevicePropertiesFn,
    GetPhysicalDeviceQueueFamilyPropertiesFn, GetPhysicalDeviceSparseImageFormatPropertiesFn,
    Handle, NegotiateFn, VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INITIALIZATION_FAILED, VK_SUCCESS,
    VkEnum, VkFlags, VkResult, VoidFn,
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
///
/// The rule for what belongs here is *what the handle dispatches through*, not
/// what level the command is named for. `vkCreateDevice` is in the table because
/// it is called on a `VkPhysicalDevice`, which points at this table.
/// `vkGetDeviceProcAddr` and `vkDestroyDevice` are not, because they are called
/// on a `VkDevice`, which points at a [`crate::device::Device`] record instead —
/// they are handed out directly by [`get_instance_proc_addr`], for the same
/// reason `vkCreateInstance` is.
///
/// By that rule the table is **exhaustive over the commands a
/// `VkPhysicalDevice` can be called with**, which is why the nine forwarding
/// trampolines of [`crate::physical`] are members rather than being reached
/// directly. The alternative — leaving them out because nothing dispatches
/// through the table by offset yet — would make the table a structure that
/// merely looks like the thing it is named after, which is the defect
/// `crate::device` declines to introduce on the device side. Thirteen entries,
/// every one of them read by [`get_instance_proc_addr`], is a different
/// proposition from the several hundred a device table would need.
#[repr(C)]
struct Table {
    get_instance_proc_addr: GetInstanceProcAddrFn,
    destroy_instance: DestroyInstanceFn,
    enumerate_physical_devices: EnumeratePhysicalDevicesFn,
    create_device: CreateDeviceFn,
    get_physical_device_properties: GetPhysicalDevicePropertiesFn,
    get_physical_device_features: GetPhysicalDeviceFeaturesFn,
    get_physical_device_memory_properties: GetPhysicalDeviceMemoryPropertiesFn,
    get_physical_device_queue_family_properties: GetPhysicalDeviceQueueFamilyPropertiesFn,
    get_physical_device_format_properties: GetPhysicalDeviceFormatPropertiesFn,
    get_physical_device_image_format_properties: GetPhysicalDeviceImageFormatPropertiesFn,
    get_physical_device_sparse_image_format_properties:
        GetPhysicalDeviceSparseImageFormatPropertiesFn,
    enumerate_device_extension_properties: EnumerateDeviceExtensionPropertiesFn,
    enumerate_device_layer_properties: EnumerateDeviceLayerPropertiesFn,
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
    create_device,
    get_physical_device_properties,
    get_physical_device_features,
    get_physical_device_memory_properties,
    get_physical_device_queue_family_properties,
    get_physical_device_format_properties,
    get_physical_device_image_format_properties,
    get_physical_device_sparse_image_format_properties,
    enumerate_device_extension_properties,
    enumerate_device_layer_properties,
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
            b"vkCreateDevice" => Some(erase(TABLE.create_device as *const ())),
            // Not reached through the table: these two are called on a
            // `VkDevice`, which dispatches through a `Device` record rather than
            // through `TABLE`. An application is still entitled to find them
            // here — `vkGetInstanceProcAddr` answers for device-level commands
            // as well, which is how a `VkDevice`'s first command is ever found.
            b"vkGetDeviceProcAddr" => Some(erase(get_device_proc_addr as *const ())),
            b"vkDestroyDevice" => Some(erase(destroy_device as *const ())),
            // The nine commands taking a `VkPhysicalDevice`, answered from one
            // place rather than nine arms so that the set stays exactly
            // [`crate::physical::Command`] and the compiler checks it is covered.
            _ => physical::lookup(name).map(physical_trampoline),
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
            let mut wrapper = PhysicalDevice::new(instance.driver, instance.handle, handle);
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

// ---------------------------------------------------------------------------
// Physical devices
// ---------------------------------------------------------------------------

/// The trampoline that answers for a physical-device command.
///
/// Kept as one exhaustive match rather than nine arms in
/// [`get_instance_proc_addr`] so that adding a [`Command`] is a compile error
/// until a function is named for it. Every arm reads [`TABLE`], because the
/// table is what a `VkPhysicalDevice` dispatches through and a second list of
/// the same nine pointers is a second list to get out of step.
fn physical_trampoline(command: Command) -> unsafe extern "C" fn() {
    let f = match command {
        Command::Properties => TABLE.get_physical_device_properties as *const (),
        Command::Features => TABLE.get_physical_device_features as *const (),
        Command::MemoryProperties => TABLE.get_physical_device_memory_properties as *const (),
        Command::QueueFamilyProperties => {
            TABLE.get_physical_device_queue_family_properties as *const ()
        }
        Command::FormatProperties => TABLE.get_physical_device_format_properties as *const (),
        Command::ImageFormatProperties => {
            TABLE.get_physical_device_image_format_properties as *const ()
        }
        Command::SparseImageFormatProperties => {
            TABLE.get_physical_device_sparse_image_format_properties as *const ()
        }
        Command::DeviceExtensionProperties => {
            TABLE.enumerate_device_extension_properties as *const ()
        }
        Command::DeviceLayerProperties => TABLE.enumerate_device_layer_properties as *const (),
    };
    // SAFETY: every arm is a function pointer cast in place, which is the whole
    // of `erase`'s contract.
    unsafe { erase(f) }
}

/// Unwrap an application's `VkPhysicalDevice` and find one of its driver's
/// commands, in the order [`crate::physical::ask`] prescribes.
///
/// Returns the driver's own physical-device handle alongside the pointer,
/// because the two are always needed together and separating them invites
/// calling a driver's function with the loader's handle — the mistake wrapping
/// exists to make possible.
///
/// # Safety
///
/// `physical_device` must be null or a `VkPhysicalDevice` this loader handed out
/// from `vkEnumeratePhysicalDevices` whose instance is still alive.
unsafe fn forward(
    physical_device: Handle,
    command: Command,
) -> Option<(Handle, unsafe extern "C" fn())> {
    if physical_device.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees this handle is one this loader handed out,
    // so it points at a live `PhysicalDevice` owned by an instance that has not
    // been destroyed.
    let physical: &PhysicalDevice = unsafe { &*physical_device.cast::<PhysicalDevice>() };

    let registry = DRIVERS.lock();
    // The device names a driver the registry no longer has — unreachable while
    // drivers are only ever added, and reported rather than assumed away for the
    // same reason as in `create_device`.
    let driver = registry.drivers().get(physical.driver())?;

    let name = command.name();
    let instance = physical.instance();
    let driver_lookup = driver.physical_device_proc_addr();

    let found = match physical::ask(driver_lookup.is_some()) {
        Ask::PhysicalDeviceThenInstance => driver_lookup.and_then(|lookup| {
            // SAFETY: `driver_lookup` is `Some` only for a driver that settled at
            // interface version 4 or above, which is what entitles the loader to
            // call this pointer at all; `instance` is that driver's own, and
            // `name` is a literal that outlives the call.
            unsafe { lookup(instance, name.as_ptr()) }
        }),
        // Nothing to try first: below version 4 the driver exports no such
        // pointer, and calling one it never exported is not a null check away.
        Ask::InstanceOnly => None,
    };

    let found = match found {
        Some(found) => found,
        // SAFETY: the driver's entry points are live for the process, and
        // `instance` is the instance this physical device was enumerated from.
        None => unsafe { instance_level(driver, instance, name) }?,
    };
    Some((physical.handle(), found))
}

// The nine trampolines. Each is the same three steps — unwrap, resolve, call
// with the driver's handle — and they are written out rather than generated
// because a macro would hide the one thing worth reading in each: that its
// signature matches the header.
//
// A driver that cannot supply one of these is not a Vulkan 1.0 driver, since all
// nine are core. There is still a decision to make about what happens, and it is
// the same one `driver_destroy_device` makes: the command does what it can and
// no more. Where there is a count to report, it is set to zero — a truthful "no
// queue families", which makes the application's `vkCreateDevice` fail for a
// reason it can act on. Where there is a `VkResult`,
// `VK_ERROR_INITIALIZATION_FAILED` is returned. Where there is neither, the
// caller's structure is left untouched, which is the only remaining option and
// is recorded here as noticed rather than overlooked.

/// `vkGetPhysicalDeviceProperties`.
///
/// # Safety
///
/// `physical_device` must be null or a `VkPhysicalDevice` this loader handed out
/// whose instance is still alive, and `out` must be a writable
/// `VkPhysicalDeviceProperties`.
#[unsafe(export_name = "vkGetPhysicalDeviceProperties")]
pub unsafe extern "C" fn get_physical_device_properties(physical_device: Handle, out: *mut c_void) {
    // SAFETY: forwarded from this function's contract.
    let Some((handle, found)) = (unsafe { forward(physical_device, Command::Properties) }) else {
        return;
    };
    // SAFETY: the driver returned this pointer for this command's name, which is
    // the contract that fixes its signature.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDevicePropertiesFn>(found)
    };
    // SAFETY: `handle` is the driver's own physical device and `out` is the
    // caller's, passed through unread.
    unsafe { call(handle, out) };
}

/// `vkGetPhysicalDeviceFeatures`.
///
/// # Safety
///
/// As [`get_physical_device_properties`], with `out` a
/// `VkPhysicalDeviceFeatures`.
#[unsafe(export_name = "vkGetPhysicalDeviceFeatures")]
pub unsafe extern "C" fn get_physical_device_features(physical_device: Handle, out: *mut c_void) {
    // SAFETY: forwarded from this function's contract.
    let Some((handle, found)) = (unsafe { forward(physical_device, Command::Features) }) else {
        return;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDeviceFeaturesFn>(found)
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, out) };
}

/// `vkGetPhysicalDeviceMemoryProperties`.
///
/// # Safety
///
/// As [`get_physical_device_properties`], with `out` a
/// `VkPhysicalDeviceMemoryProperties`.
#[unsafe(export_name = "vkGetPhysicalDeviceMemoryProperties")]
pub unsafe extern "C" fn get_physical_device_memory_properties(
    physical_device: Handle,
    out: *mut c_void,
) {
    // SAFETY: forwarded from this function's contract.
    let Some((handle, found)) = (unsafe { forward(physical_device, Command::MemoryProperties) })
    else {
        return;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDeviceMemoryPropertiesFn>(found)
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, out) };
}

/// `vkGetPhysicalDeviceQueueFamilyProperties`.
///
/// The one command an application cannot do without: `vkCreateDevice` needs a
/// queue family index, and this is the only way to learn which indices exist.
///
/// # Safety
///
/// `physical_device` must be null or a `VkPhysicalDevice` this loader handed out
/// whose instance is still alive, `count` a writable `u32`, and `out` null or an
/// array of at least `*count` `VkQueueFamilyProperties`.
#[unsafe(export_name = "vkGetPhysicalDeviceQueueFamilyProperties")]
pub unsafe extern "C" fn get_physical_device_queue_family_properties(
    physical_device: Handle,
    count: *mut u32,
    out: *mut c_void,
) {
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { forward(physical_device, Command::QueueFamilyProperties) };
    let Some((handle, found)) = found else {
        // Zero rather than untouched: an application that reads a count it never
        // wrote gets whatever was on its stack and then reads that many
        // structures. Zero is a wrong answer it survives.
        if !count.is_null() {
            // SAFETY: checked non-null, and the caller guarantees it is writable.
            unsafe { *count = 0 };
        }
        return;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDeviceQueueFamilyPropertiesFn>(
            found,
        )
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, count, out) };
}

/// `vkGetPhysicalDeviceFormatProperties`.
///
/// # Safety
///
/// As [`get_physical_device_properties`], with `out` a `VkFormatProperties`.
/// `format` is passed through unvalidated.
#[unsafe(export_name = "vkGetPhysicalDeviceFormatProperties")]
pub unsafe extern "C" fn get_physical_device_format_properties(
    physical_device: Handle,
    format: VkEnum,
    out: *mut c_void,
) {
    // SAFETY: forwarded from this function's contract.
    let Some((handle, found)) = (unsafe { forward(physical_device, Command::FormatProperties) })
    else {
        return;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDeviceFormatPropertiesFn>(found)
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, format, out) };
}

/// `vkGetPhysicalDeviceImageFormatProperties`.
///
/// # Safety
///
/// As [`get_physical_device_properties`], with `out` a
/// `VkImageFormatProperties`. The five scalars are passed through unvalidated.
#[unsafe(export_name = "vkGetPhysicalDeviceImageFormatProperties")]
pub unsafe extern "C" fn get_physical_device_image_format_properties(
    physical_device: Handle,
    format: VkEnum,
    image_type: VkEnum,
    tiling: VkEnum,
    usage: VkFlags,
    flags: VkFlags,
    out: *mut c_void,
) -> VkResult {
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { forward(physical_device, Command::ImageFormatProperties) };
    let Some((handle, found)) = found else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDeviceImageFormatPropertiesFn>(
            found,
        )
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, format, image_type, tiling, usage, flags, out) }
}

/// `vkGetPhysicalDeviceSparseImageFormatProperties`.
///
/// # Safety
///
/// As [`get_physical_device_queue_family_properties`], with `out` an array of
/// `VkSparseImageFormatProperties`. The four scalars are passed through
/// unvalidated.
#[unsafe(export_name = "vkGetPhysicalDeviceSparseImageFormatProperties")]
pub unsafe extern "C" fn get_physical_device_sparse_image_format_properties(
    physical_device: Handle,
    format: VkEnum,
    image_type: VkEnum,
    samples: VkFlags,
    usage: VkFlags,
    tiling: VkEnum,
    count: *mut u32,
    out: *mut c_void,
) {
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { forward(physical_device, Command::SparseImageFormatProperties) };
    let Some((handle, found)) = found else {
        // Zero, for the reason given in
        // `get_physical_device_queue_family_properties`.
        if !count.is_null() {
            // SAFETY: checked non-null, and the caller guarantees it is writable.
            unsafe { *count = 0 };
        }
        return;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), GetPhysicalDeviceSparseImageFormatPropertiesFn>(
            found,
        )
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe {
        call(
            handle, format, image_type, samples, usage, tiling, count, out,
        );
    };
}

/// `vkEnumerateDeviceExtensionProperties`.
///
/// Unlike the *instance* extension list, this one is a single driver's answer
/// about a single physical device, so there is nothing to merge and nothing to
/// decide — the driver's answer is the answer.
///
/// # Safety
///
/// `physical_device` must be null or a `VkPhysicalDevice` this loader handed out
/// whose instance is still alive, `layer_name` null or a NUL-terminated string
/// valid for the call, `count` a writable `u32`, and `out` null or an array of at
/// least `*count` `VkExtensionProperties`.
#[unsafe(export_name = "vkEnumerateDeviceExtensionProperties")]
pub unsafe extern "C" fn enumerate_device_extension_properties(
    physical_device: Handle,
    layer_name: *const c_char,
    count: *mut u32,
    out: *mut c_void,
) -> VkResult {
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { forward(physical_device, Command::DeviceExtensionProperties) };
    let Some((handle, found)) = found else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), EnumerateDeviceExtensionPropertiesFn>(found)
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, layer_name, count, out) }
}

/// `vkEnumerateDeviceLayerProperties`.
///
/// Deprecated by Vulkan — a device has no layers of its own any more — and
/// forwarded rather than answered with an empty list anyway. The loader does not
/// know what a driver will say, and inventing a zero here would be exactly the
/// "reports success for work it never did" failure this crate declines to commit
/// elsewhere.
///
/// # Safety
///
/// As [`get_physical_device_queue_family_properties`], with `out` an array of
/// `VkLayerProperties`.
#[unsafe(export_name = "vkEnumerateDeviceLayerProperties")]
pub unsafe extern "C" fn enumerate_device_layer_properties(
    physical_device: Handle,
    count: *mut u32,
    out: *mut c_void,
) -> VkResult {
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { forward(physical_device, Command::DeviceLayerProperties) };
    let Some((handle, found)) = found else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: as in `get_physical_device_properties`.
    let call = unsafe {
        core::mem::transmute::<unsafe extern "C" fn(), EnumerateDeviceLayerPropertiesFn>(found)
    };
    // SAFETY: as in `get_physical_device_properties`.
    unsafe { call(handle, count, out) }
}

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

/// Ask one driver for an entry point by name, through its instance.
///
/// # Safety
///
/// `driver`'s entry points must be live, and `instance` must be an instance that
/// driver created and has not destroyed.
unsafe fn instance_level(driver: &Driver, instance: Handle, name: &CStr) -> VoidFn {
    let lookup = driver.instance_proc_addr();
    // SAFETY: the caller guarantees `lookup` is live and `instance` is the
    // driver's own; `name` is a literal that outlives the call.
    unsafe { lookup(instance, name.as_ptr()) }
}

/// Tell a driver to destroy a device it created.
///
/// The lookup goes through the device's own `vkGetDeviceProcAddr` rather than
/// its instance's. That is where a device-level command belongs, and it is also
/// the only lookup the loader is certain to still have: the record kept for a
/// device holds this function and not the instance it came from.
///
/// # Safety
///
/// `device_proc_addr` must be the driver's lookup for `device`, `device` must be
/// a device that driver created and has not destroyed, and `allocator` must be
/// the one creation was given.
unsafe fn driver_destroy_device(
    device_proc_addr: GetDeviceProcAddrFn,
    device: Handle,
    allocator: *const c_void,
) {
    // SAFETY: forwarded from this function's contract.
    let found = unsafe { device_proc_addr(device, c"vkDestroyDevice".as_ptr()) };
    let Some(found) = found else {
        // A driver that can create a device and not destroy one leaks it. There
        // is nowhere to report that from — `vkDestroyDevice` returns void — so
        // the leak is the driver's, and this is the note saying it was noticed.
        // The same case, for the same reason, as in `destroy_one`.
        return;
    };
    // SAFETY: the driver returned this pointer for the name `vkDestroyDevice`,
    // which is the contract that fixes its signature.
    let destroy = unsafe { core::mem::transmute::<unsafe extern "C" fn(), DestroyDeviceFn>(found) };
    // SAFETY: forwarded from this function's contract.
    unsafe { destroy(device, allocator) };
}

/// The [`Device`] record a loader-created `VkDevice`'s dispatch word points at.
///
/// # Safety
///
/// `device` must be a `VkDevice` this loader's [`create_device`] returned and
/// that [`destroy_device`] has not since reclaimed.
unsafe fn record_of<'a>(device: Handle) -> &'a Device {
    // SAFETY: forwarded from this function's contract — a device this loader
    // returned is a dispatchable object whose first word it wrote itself.
    let word = unsafe { dispatch::loader_data(device) };
    // SAFETY: `create_device` wrote the address of a leaked `Box<Device>` into
    // that word, and only `destroy_device` reclaims it, so it is live here.
    unsafe { &*(word as *const Device) }
}

/// `vkCreateDevice`.
///
/// The device handed back is the **driver's own**, not a wrapper: the loader
/// adopts it by replacing its dispatch word. That is what lets a device command
/// the loader does not export reach the driver with nothing in between, and it
/// is argued for in [`crate::device`].
///
/// # Safety
///
/// `physical_device` must be a `VkPhysicalDevice` this loader handed out from
/// `vkEnumeratePhysicalDevices` whose instance is still alive, `create_info` a
/// valid `VkDeviceCreateInfo*`, `allocator` a valid `VkAllocationCallbacks*` or
/// null, and `out` a writable `VkDevice` slot.
#[unsafe(export_name = "vkCreateDevice")]
pub unsafe extern "C" fn create_device(
    physical_device: Handle,
    create_info: *const c_void,
    allocator: *const c_void,
    out: *mut Handle,
) -> VkResult {
    if physical_device.is_null() || out.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    // SAFETY: the caller guarantees this handle is one this loader handed out,
    // so it points at a live `PhysicalDevice` owned by an instance that has not
    // been destroyed.
    let physical: &PhysicalDevice = unsafe { &*physical_device.cast::<PhysicalDevice>() };

    let registry = DRIVERS.lock();
    let Some(driver) = registry.drivers().get(physical.driver()) else {
        // The device names a driver the registry no longer has. Unreachable
        // while drivers are only ever added, and reported rather than assumed
        // away because the alternative is an index into the wrong driver.
        return VK_ERROR_INITIALIZATION_FAILED;
    };

    // Resolved *before* the device is created, not after. A driver with no
    // `vkGetDeviceProcAddr` cannot serve a device at all, and finding that out
    // first means the refusal has nothing to unwind — whereas discovering it
    // afterwards would leave a live device to destroy through the very lookup
    // function that turned out to be missing.
    // SAFETY: the driver's entry points are live for the process, and
    // `physical.instance()` is the instance this device was enumerated from,
    // which is what an instance-level lookup has to be asked with.
    let found = unsafe { instance_level(driver, physical.instance(), c"vkGetDeviceProcAddr") };
    let Some(found) = found else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: the driver returned this pointer for the name
    // `vkGetDeviceProcAddr`, which is the contract that fixes its signature.
    let device_proc_addr =
        unsafe { core::mem::transmute::<unsafe extern "C" fn(), GetDeviceProcAddrFn>(found) };

    // SAFETY: as above.
    let found = unsafe { instance_level(driver, physical.instance(), c"vkCreateDevice") };
    let Some(found) = found else {
        return VK_ERROR_INITIALIZATION_FAILED;
    };
    // SAFETY: the driver returned this pointer for the name `vkCreateDevice`,
    // which is the contract that fixes its signature.
    let create = unsafe { core::mem::transmute::<unsafe extern "C" fn(), CreateDeviceFn>(found) };

    let mut handle: Handle = ptr::null_mut();
    // SAFETY: `physical.handle()` is the driver's own physical device, the two
    // structure pointers are the application's passed through unread, and
    // `handle` is a writable slot that outlives the call.
    let result = unsafe { create(physical.handle(), create_info, allocator, &raw mut handle) };
    if result < VK_SUCCESS {
        return result;
    }
    if handle.is_null() {
        // Success with no handle, treated exactly as `create_one` treats it:
        // believing the code over the handle would hand the application a null
        // `VkDevice` that crashes on its first use.
        return VK_ERROR_INITIALIZATION_FAILED;
    }

    let mut record = Device::new(physical.driver(), device_proc_addr);
    // SAFETY: `handle` is a dispatchable object the driver just returned, so it
    // is live, aligned, and — if the driver follows the interface — stamped. The
    // record outlives every use of the handle: it is leaked below and reclaimed
    // only by `destroy_device`.
    if unsafe { dispatch::adopt(handle, record.as_dispatch_target()) }.is_err() {
        // The driver returned a device without the loader magic, so the
        // interface contract it was admitted under does not hold. Destroyed
        // rather than handed back: a handle the loader could not dispatch
        // through is one the application cannot use, and leaving it alive would
        // leak a device on a path that reports failure.
        // SAFETY: `handle` is the device this call just created, and
        // `device_proc_addr` is that driver's own lookup for it.
        unsafe { driver_destroy_device(device_proc_addr, handle, allocator) };
        return VK_ERROR_INITIALIZATION_FAILED;
    }

    // SAFETY: `out` was checked non-null and the caller guarantees it is
    // writable. The application's `VkDevice` is the driver's own handle, which
    // is the whole point of adopting rather than wrapping.
    unsafe { *out = handle };
    // Leaked on purpose: the device's dispatch word now points at this record,
    // so it must outlive the handle, and `destroy_device` is what reclaims it.
    core::mem::forget(record);
    VK_SUCCESS
}

/// `vkGetDeviceProcAddr`.
///
/// Two commands are answered by the loader and everything else by the driver;
/// which, and why those two, is [`crate::device::lookup`].
///
/// # Safety
///
/// `device` must be null or a `VkDevice` this loader created and has not
/// destroyed, and `name` must be null or a NUL-terminated string valid for this
/// call.
#[unsafe(export_name = "vkGetDeviceProcAddr")]
pub unsafe extern "C" fn get_device_proc_addr(device: Handle, name: *const c_char) -> VoidFn {
    if device.is_null() || name.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a NUL-terminated string valid for the call,
    // and the borrow does not outlive this function.
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();

    match device::lookup(bytes) {
        // SAFETY: a function pointer cast in place, which is `erase`'s contract.
        Lookup::Loader(LoaderCommand::GetDeviceProcAddr) => {
            Some(unsafe { erase(get_device_proc_addr as *const ()) })
        }
        // SAFETY: as above.
        Lookup::Loader(LoaderCommand::DestroyDevice) => {
            Some(unsafe { erase(destroy_device as *const ()) })
        }
        Lookup::Driver => {
            // SAFETY: the caller guarantees `device` came from this loader's
            // `vkCreateDevice`, which is `record_of`'s contract.
            let record = unsafe { record_of(device) };
            // SAFETY: the record holds this driver's own lookup for this very
            // device, and `name` is the caller's string, still valid.
            unsafe { (record.driver_get_device_proc_addr())(device, name) }
        }
    }
}

/// `vkDestroyDevice`.
///
/// # Safety
///
/// `device` must be null or a `VkDevice` this loader created and has not already
/// destroyed, and `allocator` must match the one creation was given.
#[unsafe(export_name = "vkDestroyDevice")]
pub unsafe extern "C" fn destroy_device(device: Handle, allocator: *const c_void) {
    if device.is_null() {
        // Vulkan defines destroying a null handle as doing nothing, so that
        // teardown paths need not branch on how far setup got.
        return;
    }
    // The dispatch word lives inside the driver's own object, so it has to be
    // read while that object is still alive — which is why the record is taken
    // back here and the driver is told afterwards, rather than the other way
    // round.
    // SAFETY: the caller guarantees this device came from this loader's
    // `vkCreateDevice`, so its first word holds the address that call leaked
    // from a `Box`. Reclaiming it here frees it exactly once.
    let word = unsafe { dispatch::loader_data(device) };
    // SAFETY: as above.
    let record: Box<Device> = unsafe { Box::from_raw(word as *mut Device) };

    // SAFETY: the record's lookup is this driver's own for this device, and the
    // caller guarantees the device is live and the allocator matches.
    unsafe { driver_destroy_device(record.driver_get_device_proc_addr(), device, allocator) };
    // Freed only now: the driver's `vkDestroyDevice` runs first, and until it
    // returns the device's dispatch word still points here.
    drop(record);
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
        SpinLock, create_across, create_device, create_instance, destroy_across, destroy_device,
        destroy_instance, enumerate_across, enumerate_physical_devices, get_device_proc_addr,
        get_instance_proc_addr, get_physical_device_format_properties,
        get_physical_device_queue_family_properties, register_driver, table,
    };
    use crate::dispatch::{ICD_LOADER_MAGIC, is_loader_magic, loader_data};
    use crate::icd::{CURRENT, DriverReply};
    use crate::instance::PhysicalDevice;
    use crate::registry::{Entry, Registry};
    use crate::vk::{
        Handle, VK_ERROR_INCOMPATIBLE_DRIVER, VK_ERROR_INITIALIZATION_FAILED, VK_INCOMPLETE,
        VK_SUCCESS, VkEnum, VkResult, VoidFn,
    };
    use alloc::boxed::Box;
    use alloc::vec;
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

    // -----------------------------------------------------------------------
    // A stub driver that can make devices, not just instances
    // -----------------------------------------------------------------------

    /// Devices created and not yet destroyed, across all stub drivers.
    static DEVICES_LIVE: AtomicUsize = AtomicUsize::new(0);

    /// A stub driver's `VkDevice`. Dispatchable, so the loader may adopt it.
    #[repr(C)]
    struct FakeGpu {
        loader_data: usize,
    }

    unsafe extern "C" fn create_gpu(
        _physical: Handle,
        _create_info: *const c_void,
        _allocator: *const c_void,
        out: *mut Handle,
    ) -> VkResult {
        let gpu = Box::new(FakeGpu {
            loader_data: ICD_LOADER_MAGIC as usize,
        });
        DEVICES_LIVE.fetch_add(1, Ordering::SeqCst);
        unsafe { *out = Box::into_raw(gpu).cast::<c_void>() };
        VK_SUCCESS
    }

    unsafe extern "C" fn destroy_gpu(device: Handle, _allocator: *const c_void) {
        drop(unsafe { Box::from_raw(device.cast::<FakeGpu>()) });
        DEVICES_LIVE.fetch_sub(1, Ordering::SeqCst);
    }

    /// Stands in for a device command this loader has never heard of. Never
    /// called — the test proves it can be *found*, which is the property the
    /// loader is responsible for.
    unsafe extern "C" fn an_extension_command() {}

    /// The stub driver's `vkGetDeviceProcAddr`.
    unsafe extern "C" fn gdpa(_device: Handle, name: *const c_char) -> VoidFn {
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        let f: *const () = match name {
            b"vkDestroyDevice" => destroy_gpu as *const (),
            b"vkCmdDrawInventedNextYear" => an_extension_command as *const (),
            _ => return None,
        };
        Some(unsafe { core::mem::transmute::<*const (), unsafe extern "C" fn()>(f) })
    }

    // -----------------------------------------------------------------------
    // Stub drivers that answer physical-device commands
    // -----------------------------------------------------------------------

    /// Which entry point the loader last found a physical-device command
    /// through.
    ///
    /// A driver may be asked two ways, and the answer looks identical either
    /// way — a function pointer — so the order [`crate::physical::ask`]
    /// prescribes is unobservable from outside unless the stub says which route
    /// the question arrived by. This is that.
    static VIA: AtomicUsize = AtomicUsize::new(0);
    const VIA_NOTHING: usize = 0;
    const VIA_PHYSICAL_DEVICE_PROC_ADDR: usize = 1;
    const VIA_INSTANCE_PROC_ADDR: usize = 2;

    /// The `VkPhysicalDevice` a stub driver's command was last called with,
    /// as an address.
    ///
    /// The point of the whole module under test: this must be the *driver's*
    /// handle, never the loader's wrapper.
    static SEEN: AtomicUsize = AtomicUsize::new(0);

    /// How many queue families every stub driver claims.
    const STUB_QUEUE_FAMILIES: u32 = 3;

    /// A stub's `vkGetPhysicalDeviceQueueFamilyProperties`.
    ///
    /// Only the count form is modelled. Writing the array would mean agreeing on
    /// the layout of `VkQueueFamilyProperties`, and `crate::vk` declares no
    /// Vulkan structure on purpose — a stub that declared one would be testing
    /// the loader against a promise the loader does not make.
    unsafe extern "C" fn gpu_queue_families(physical: Handle, count: *mut u32, _out: *mut c_void) {
        SEEN.store(physical as usize, Ordering::SeqCst);
        unsafe { *count = STUB_QUEUE_FAMILIES };
    }

    /// A stub's `vkGetPhysicalDeviceFormatProperties`.
    ///
    /// Echoes `format` into the caller's first four bytes. A handle arriving
    /// intact only proves the loader substituted one pointer; echoing a scalar
    /// proves the *rest* of the signature survived the trampoline too, which is
    /// the part a wrong `PFN_` declaration would break.
    unsafe extern "C" fn gpu_format_properties(physical: Handle, format: VkEnum, out: *mut c_void) {
        SEEN.store(physical as usize, Ordering::SeqCst);
        unsafe { *out.cast::<VkEnum>() = format };
    }

    /// A version-4 driver's `vk_icdGetPhysicalDeviceProcAddr`.
    ///
    /// Answers for one command and is deliberately null for
    /// `vkGetPhysicalDeviceFormatProperties`, which its `vkGetInstanceProcAddr`
    /// *does* answer — modelling the driver the Loader–Driver Interface's
    /// fallback rule exists for, which routes only some of its physical-device
    /// commands through this entry point.
    unsafe extern "C" fn gpdpa_with_devices(_instance: Handle, name: *const c_char) -> VoidFn {
        let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
        if bytes != b"vkGetPhysicalDeviceQueueFamilyProperties" {
            return None;
        }
        VIA.store(VIA_PHYSICAL_DEVICE_PROC_ADDR, Ordering::SeqCst);
        Some(unsafe {
            core::mem::transmute::<*const (), unsafe extern "C" fn()>(
                gpu_queue_families as *const (),
            )
        })
    }

    /// Settles at whatever the loader proposed, by succeeding without writing —
    /// which [`crate::icd::settle`] treats as agreement. That is
    /// [`CURRENT`], so this driver is entitled to its
    /// `vk_icdGetPhysicalDeviceProcAddr`.
    unsafe extern "C" fn negotiate_at_the_loaders_version(_version: *mut u32) -> VkResult {
        VK_SUCCESS
    }

    /// A driver that makes devices as well as instances.
    unsafe extern "C" fn gipa_with_devices(_instance: Handle, name: *const c_char) -> VoidFn {
        let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
        let f: *const () = match bytes {
            b"vkCreateDevice" => create_gpu as *const (),
            b"vkGetDeviceProcAddr" => gdpa as *const (),
            b"vkGetPhysicalDeviceFormatProperties" => {
                VIA.store(VIA_INSTANCE_PROC_ADDR, Ordering::SeqCst);
                gpu_format_properties as *const ()
            }
            // Everything else is the shared stub's answer, which is what gives
            // this driver an instance and a physical device to start from.
            _ => return unsafe { answer(name, create_with_one_device as *const ()) },
        };
        Some(unsafe { core::mem::transmute::<*const (), unsafe extern "C" fn()>(f) })
    }

    /// A driver below interface version 4 that answers a physical-device
    /// command. Its `vkGetInstanceProcAddr` is the only route there is.
    unsafe extern "C" fn gipa_physical_only(_instance: Handle, name: *const c_char) -> VoidFn {
        if unsafe { CStr::from_ptr(name) }.to_bytes() == b"vkGetPhysicalDeviceQueueFamilyProperties"
        {
            VIA.store(VIA_INSTANCE_PROC_ADDR, Ordering::SeqCst);
            return Some(unsafe {
                core::mem::transmute::<*const (), unsafe extern "C" fn()>(
                    gpu_queue_families as *const (),
                )
            });
        }
        unsafe { answer(name, create_with_one_device as *const ()) }
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

    /// The device lifetime, driven entirely through the symbols an application
    /// links against rather than through the inner functions the tests above
    /// use. Nothing else in this file exercises the global registry, so this is
    /// also the only test of `vk_slateosRegisterDriver` reaching it.
    ///
    /// Both drivers are registered in one test on purpose. They go into
    /// process-wide state that no test tears down, so two tests each registering
    /// their own would see each other's devices in a `vkEnumeratePhysicalDevices`
    /// whose order across tests nothing fixes.
    #[test]
    fn a_device_lives_and_dies_through_the_exported_symbols() {
        let _order = ORDER.lock();
        let instances_before = LIVE.load(Ordering::SeqCst);
        let devices_before = DEVICES_LIVE.load(Ordering::SeqCst);

        // SAFETY: both names are string literals, so `'static`, and every
        // function pointer is an item in this module and lives for the process.
        unsafe {
            // Settles at `CURRENT`, so its `vk_icdGetPhysicalDeviceProcAddr` is
            // one the loader is entitled to call.
            assert_eq!(
                register_driver(
                    c"stub-with-devices".as_ptr(),
                    Some(gipa_with_devices),
                    None,
                    Some(gpdpa_with_devices),
                    Some(negotiate_at_the_loaders_version)
                ),
                VK_SUCCESS
            );
            // Registered second, and used below for two opposite cases: it has an
            // instance and a physical device but no `vkGetDeviceProcAddr`, so no
            // device can be made from it, and it answers no physical-device
            // command either.
            assert_eq!(
                register_driver(
                    c"stub-without-devices".as_ptr(),
                    Some(gipa_one_device),
                    None,
                    None,
                    None
                ),
                VK_SUCCESS
            );
            // Third: no handshake, so it settles below version 4 and has no
            // `vk_icdGetPhysicalDeviceProcAddr` to be asked through.
            assert_eq!(
                register_driver(
                    c"stub-physical-only".as_ptr(),
                    Some(gipa_physical_only),
                    None,
                    None,
                    None
                ),
                VK_SUCCESS
            );
        }

        let mut instance: Handle = ptr::null_mut();
        // SAFETY: `instance` is a writable slot, and this loader reads neither
        // structure pointer.
        assert_eq!(
            unsafe { create_instance(ptr::null(), ptr::null(), &raw mut instance) },
            VK_SUCCESS
        );
        assert_eq!(LIVE.load(Ordering::SeqCst), instances_before + 3);

        let mut count: u32 = 0;
        // SAFETY: the instance is this loader's own and `count` is writable; a
        // null array is how the C API asks for the count alone.
        assert_eq!(
            unsafe { enumerate_physical_devices(instance, &raw mut count, ptr::null_mut()) },
            VK_SUCCESS
        );
        assert_eq!(count, 3, "one physical device per registered stub driver");

        let mut handles: Vec<Handle> = vec![ptr::null_mut(); count as usize];
        // SAFETY: `handles` has room for `count` entries, which is what `count`
        // says.
        assert_eq!(
            unsafe { enumerate_physical_devices(instance, &raw mut count, handles.as_mut_ptr()) },
            VK_SUCCESS
        );

        // ---- What a real application does next, and could not do before ----
        //
        // Having a list of GPUs, it asks each what it can do, and above all how
        // many queue families it has, because `vkCreateDevice` below cannot be
        // called without naming one. Every assertion in this block answered null
        // until `physical` existed, which is what made the device half of this
        // test unreachable in practice while passing in the suite.

        // The driver's own handle, which is what its commands must be called
        // with — never the wrapper the application holds.
        // SAFETY: `handles[0]` is a `PhysicalDevice` this loader just handed out
        // and the instance owning it is alive.
        let drivers_own = unsafe { &*handles[0].cast::<PhysicalDevice>() }.handle();
        assert_ne!(
            drivers_own as usize, handles[0] as usize,
            "the fixture is not testing anything: wrapper and inner handle coincide"
        );

        VIA.store(VIA_NOTHING, Ordering::SeqCst);
        SEEN.store(0, Ordering::SeqCst);
        let mut families: u32 = 0;
        // SAFETY: `handles[0]` is this loader's, `families` is writable, and a
        // null array is how the C API asks for the count alone.
        unsafe {
            get_physical_device_queue_family_properties(
                handles[0],
                &raw mut families,
                ptr::null_mut(),
            );
        }
        assert_eq!(
            families, STUB_QUEUE_FAMILIES,
            "the count never reached the driver"
        );
        assert_eq!(
            SEEN.load(Ordering::SeqCst),
            drivers_own as usize,
            "the driver was called with the loader's wrapper instead of its own handle"
        );
        assert_eq!(
            VIA.load(Ordering::SeqCst),
            VIA_PHYSICAL_DEVICE_PROC_ADDR,
            "a version-4 driver was not asked through its version-4 entry point first"
        );

        // The same driver, a command its version-4 entry point answers null for.
        // The Loader–Driver Interface requires falling back to the instance
        // lookup, and treating that null as final would lose the command on a
        // driver that is behaving correctly.
        VIA.store(VIA_NOTHING, Ordering::SeqCst);
        SEEN.store(0, Ordering::SeqCst);
        let mut echoed: VkEnum = 0;
        let format: VkEnum = 37;
        // SAFETY: `handles[0]` is this loader's and `echoed` is a writable
        // `VkEnum`, which is what this stub writes through `out`.
        unsafe {
            get_physical_device_format_properties(
                handles[0],
                format,
                (&raw mut echoed).cast::<c_void>(),
            );
        }
        assert_eq!(
            echoed, format,
            "a scalar argument did not survive the trampoline"
        );
        assert_eq!(SEEN.load(Ordering::SeqCst), drivers_own as usize);
        assert_eq!(
            VIA.load(Ordering::SeqCst),
            VIA_INSTANCE_PROC_ADDR,
            "the loader gave up when the version-4 entry point answered null"
        );

        // The third driver settled below version 4, so there is only one route
        // to it and the loader must take it.
        VIA.store(VIA_NOTHING, Ordering::SeqCst);
        let mut families: u32 = 0;
        // SAFETY: as for `handles[0]`.
        unsafe {
            get_physical_device_queue_family_properties(
                handles[2],
                &raw mut families,
                ptr::null_mut(),
            );
        }
        assert_eq!(families, STUB_QUEUE_FAMILIES);
        assert_eq!(VIA.load(Ordering::SeqCst), VIA_INSTANCE_PROC_ADDR);

        // The second driver answers no physical-device command at all — not a
        // conforming Vulkan 1.0 driver. The count is set to zero rather than
        // left holding whatever the caller's stack did, because an application
        // that then reads that many structures reads an arbitrary amount of
        // memory.
        let mut families: u32 = 0xDEAD_BEEF;
        // SAFETY: as for `handles[0]`.
        unsafe {
            get_physical_device_queue_family_properties(
                handles[1],
                &raw mut families,
                ptr::null_mut(),
            );
        }
        assert_eq!(
            families, 0,
            "a driver with no such command left the caller's count untouched"
        );

        let mut device: Handle = ptr::null_mut();
        // SAFETY: `handles[0]` is a physical device this loader just handed out,
        // its instance is alive, and `device` is a writable slot.
        assert_eq!(
            unsafe { create_device(handles[0], ptr::null(), ptr::null(), &raw mut device) },
            VK_SUCCESS
        );
        assert_eq!(DEVICES_LIVE.load(Ordering::SeqCst), devices_before + 1);
        assert!(!device.is_null());

        // Adopted rather than wrapped: the handle the application holds is the
        // driver's own, with the loader's record in its dispatch word where the
        // driver left the magic.
        // SAFETY: the device is live and this crate wrote that word itself.
        assert!(
            !is_loader_magic(unsafe { loader_data(device) }),
            "the loader never claimed the device's dispatch word"
        );

        // SAFETY: `device` came from this loader and every name is a literal.
        unsafe {
            assert!(
                get_device_proc_addr(device, c"vkGetDeviceProcAddr".as_ptr()).is_some(),
                "the loader failed to answer for its own lookup"
            );
            assert!(
                get_device_proc_addr(device, c"vkDestroyDevice".as_ptr()).is_some(),
                "the loader failed to answer for the command that frees its record"
            );
            // The property the whole design is for: a command written after this
            // loader was still reaches the driver.
            assert!(
                get_device_proc_addr(device, c"vkCmdDrawInventedNextYear".as_ptr()).is_some(),
                "an extension command was refused instead of forwarded"
            );
            // And the driver's null is the answer, not a guess of the loader's.
            assert!(get_device_proc_addr(device, c"vkCmdDraw".as_ptr()).is_none());
        }

        // The second driver has a physical device but no way to make a device
        // from it. Refused before anything is created, so there is nothing left
        // behind to count.
        let mut refused: Handle = ptr::null_mut();
        // SAFETY: as for `handles[0]`.
        assert_eq!(
            unsafe { create_device(handles[1], ptr::null(), ptr::null(), &raw mut refused) },
            VK_ERROR_INITIALIZATION_FAILED
        );
        assert!(refused.is_null());
        assert_eq!(
            DEVICES_LIVE.load(Ordering::SeqCst),
            devices_before + 1,
            "the refused call created a device anyway"
        );

        // SAFETY: the device is this loader's, has not been destroyed, and was
        // created with the same (null) allocator.
        unsafe { destroy_device(device, ptr::null()) };
        assert_eq!(
            DEVICES_LIVE.load(Ordering::SeqCst),
            devices_before,
            "the driver was not told to destroy its device"
        );

        // SAFETY: as above, for the instance.
        unsafe { destroy_instance(instance, ptr::null()) };
        assert_eq!(LIVE.load(Ordering::SeqCst), instances_before);
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
    fn the_device_level_commands_are_findable_through_an_instance() {
        // An application has to reach a `VkDevice`'s first command somehow, and
        // `vkGetInstanceProcAddr` is the only lookup it has before it owns one.
        // A loader that answered null here would leave `vkCreateDevice`
        // unreachable and every device it could have made unbuildable.
        assert!(lookup(an_instance(), c"vkCreateDevice").is_some());
        assert!(lookup(an_instance(), c"vkGetDeviceProcAddr").is_some());
        assert!(lookup(an_instance(), c"vkDestroyDevice").is_some());
        assert!(
            lookup(ptr::null_mut(), c"vkCreateDevice").is_none(),
            "a command needing a physical device was answered with no instance"
        );
    }

    #[test]
    fn every_physical_device_command_is_findable_through_an_instance() {
        // The check that was missing, and whose absence is the whole of the bug
        // the physical-device half of this module fixes. Each of these is core
        // Vulkan 1.0 and each was answered null — including the one that reports
        // queue families, without which `vkCreateDevice` cannot be called at
        // all, so the device layer below was complete, tested and unreachable.
        //
        // Iterating `physical::ALL` rather than listing nine names is what makes
        // this a check on the *set*: a command added to the enum and forgotten
        // in `get_instance_proc_addr` fails here rather than shipping.
        for command in crate::physical::ALL {
            assert!(
                lookup(an_instance(), command.name()).is_some(),
                "{command:?} is not reachable through vkGetInstanceProcAddr",
            );
            assert!(
                lookup(ptr::null_mut(), command.name()).is_none(),
                "{command:?} was answered with no instance",
            );
        }
    }

    #[test]
    fn an_unimplemented_command_is_null_rather_than_a_wrong_pointer() {
        // Null is the C API's own answer for "no such entry point". Returning
        // anything else here is how a loader ships a jump to the wrong function.
        // The three instance-enumeration commands are the loader's real
        // omissions; `vkCmdDraw` is here for a different reason — it is a device
        // command, and device commands are the driver's to answer through
        // `vkGetDeviceProcAddr`, not this loader's.
        assert!(lookup(ptr::null_mut(), c"vkEnumerateInstanceVersion").is_none());
        assert!(lookup(an_instance(), c"vkEnumerateInstanceLayerProperties").is_none());
        assert!(lookup(an_instance(), c"vkCmdDraw").is_none());
        assert!(lookup(an_instance(), c"").is_none());
    }

    #[test]
    fn a_null_name_is_answered_rather_than_dereferenced() {
        assert!(unsafe { get_instance_proc_addr(ptr::null_mut(), ptr::null()) }.is_none());
    }
}
