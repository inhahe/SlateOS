//! The slice of the Vulkan C ABI the loader itself has to speak.
//!
//! This is not a Vulkan binding, and it is deliberately not on its way to
//! becoming one. It declares only the types that appear in the *loader's* own
//! signatures — the handles it passes through, the result codes it produces or
//! interprets, and the three function pointer shapes it calls on a driver. An
//! application's Vulkan header is the binding; this module is the small part
//! of it that the loader cannot avoid knowing.
//!
//! Keeping it small is the point. Every declaration here is a promise about
//! the memory layout of something another compiler produced, and a wrong
//! promise is a crash rather than a type error. Six declarations can be
//! checked against `vulkan_core.h` by reading; six hundred cannot.
//!
//! # Naming
//!
//! The C names are `VkInstance`, `PFN_vkGetInstanceProcAddr` and so on. Those
//! do not survive Rust's naming lints, and suppressing the lints crate-wide to
//! keep them would be a poor trade — the C name is a *fact to record*, not a
//! spelling to imitate. So each item takes a Rust name and states its C
//! counterpart in its documentation, which is where someone diffing against
//! the header will look anyway.

use core::ffi::{c_char, c_void};

/// `VkResult`. Zero is success, negative values are errors, positive values
/// are successes that carry information.
///
/// Declared as a plain `i32` rather than an enum on purpose: a driver may
/// return a code from an extension this loader has never heard of, and an
/// enum with a fixed set of variants makes that value undefined behaviour to
/// receive. The loader's job with an unrecognised code is to pass it along
/// unaltered, which requires being able to hold it.
pub type VkResult = i32;

/// `VK_SUCCESS`.
pub const VK_SUCCESS: VkResult = 0;

/// `VK_INCOMPLETE`. A *success*, not an error: the array the caller supplied
/// was too small, and as much as fitted was written.
///
/// It is positive, which is the whole reason [`crate::instance::outcome`]
/// treats "non-negative" rather than "equal to `VK_SUCCESS`" as success.
pub const VK_INCOMPLETE: VkResult = 5;

/// `VK_ERROR_OUT_OF_HOST_MEMORY`.
pub const VK_ERROR_OUT_OF_HOST_MEMORY: VkResult = -1;

/// `VK_ERROR_INITIALIZATION_FAILED`.
pub const VK_ERROR_INITIALIZATION_FAILED: VkResult = -3;

/// `VK_ERROR_INCOMPATIBLE_DRIVER`.
///
/// The loader sees this in two quite different roles, which is worth keeping
/// straight. From a driver's interface-version handshake it means "I cannot
/// work with a loader like you" and the correct response is to skip that
/// driver silently. Returned from `vkCreateInstance` to the *application* it
/// means "this machine has no driver that can serve you at all", which is a
/// real, reportable failure.
pub const VK_ERROR_INCOMPATIBLE_DRIVER: VkResult = -9;

/// `VkInstance`, and every other dispatchable handle, as an untyped address.
///
/// The loader passes these through far more often than it inspects them, and
/// the one thing it does inspect — the first word — is reached through
/// [`crate::dispatch`], which takes a `*mut c_void` anyway. Distinct newtypes
/// per handle kind would be worth having in a binding used by applications;
/// here they would mostly be cast away.
pub type Handle = *mut c_void;

/// `PFN_vkVoidFunction`: a function pointer of unknown signature, as returned
/// by the `GetProcAddr` family.
///
/// `Option` is what makes the null case representable without a raw pointer:
/// `None` is the null the C API returns for "no such entry point", and Rust's
/// null-pointer optimisation means the two have the same representation, so
/// this is an honest ABI match rather than a wrapper.
pub type VoidFn = Option<unsafe extern "C" fn()>;

/// `PFN_vkGetInstanceProcAddr`, and identically `vk_icdGetInstanceProcAddr`.
///
/// # Safety
///
/// `instance` must be null or a handle this driver produced, and `name` must
/// point to a NUL-terminated string that stays valid for the call.
pub type GetInstanceProcAddrFn =
    unsafe extern "C" fn(instance: Handle, name: *const c_char) -> VoidFn;

/// `PFN_vkGetPhysicalDeviceProcAddr`, the interface-version-4 addition
/// (`vk_icdGetPhysicalDeviceProcAddr`).
///
/// # Safety
///
/// As [`GetInstanceProcAddrFn`], and the driver must have settled on interface
/// version 4 or above — calling this on a driver that did not is a jump
/// through a pointer the driver never exported.
pub type GetPhysicalDeviceProcAddrFn =
    unsafe extern "C" fn(instance: Handle, name: *const c_char) -> VoidFn;

/// `PFN_vkCreateInstance`.
///
/// The two structure pointers are `*const c_void` rather than declared types
/// because the loader does not read either of them — it hands both to every
/// driver exactly as the application gave them. Declaring `VkInstanceCreateInfo`
/// here would mean promising a layout the loader never checks, which is the
/// kind of promise this module exists to avoid making.
///
/// # Safety
///
/// `create_info` must be a valid `VkInstanceCreateInfo*` for the callee,
/// `allocator` a valid `VkAllocationCallbacks*` or null, and `out` a writable
/// handle slot.
pub type CreateInstanceFn = unsafe extern "C" fn(
    create_info: *const c_void,
    allocator: *const c_void,
    out: *mut Handle,
) -> VkResult;

/// `PFN_vkDestroyInstance`.
///
/// # Safety
///
/// `instance` must be null or a handle the callee created and has not already
/// destroyed; `allocator` must match the one creation was given.
pub type DestroyInstanceFn = unsafe extern "C" fn(instance: Handle, allocator: *const c_void);

/// `PFN_vkEnumeratePhysicalDevices`.
///
/// # Safety
///
/// `instance` must be a handle the callee created, `count` a writable `u32`,
/// and `out` either null or an array of at least `*count` handles.
pub type EnumeratePhysicalDevicesFn =
    unsafe extern "C" fn(instance: Handle, count: *mut u32, out: *mut Handle) -> VkResult;

/// `PFN_vk_icdNegotiateLoaderICDInterfaceVersion`.
///
/// The loader writes the version it proposes into the `u32`, calls, and reads
/// back what the driver settled on. The same location carries both, which is
/// why [`crate::icd::settle`] takes the proposal and the reply as separate
/// values: once the call has returned, the proposal is gone.
///
/// # Safety
///
/// The `*mut u32` must point to a writable, initialised `u32` that the driver
/// may overwrite.
pub type NegotiateFn = unsafe extern "C" fn(version: *mut u32) -> VkResult;
