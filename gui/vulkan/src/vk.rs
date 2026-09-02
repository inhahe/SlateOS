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
//! promise is a crash rather than a type error. A list this short can be
//! checked against `vulkan_core.h` by reading; a whole binding cannot.
//!
//! # Exactly one structure is declared here, and the rule is why
//!
//! Nearly every Vulkan structure the loader touches appears in these signatures
//! as `*const c_void` or `*mut c_void` — `VkInstanceCreateInfo`,
//! `VkDeviceCreateInfo`, `VkPhysicalDeviceProperties`, `VkQueueFamilyProperties`
//! and the rest. That is not laziness; it is the only part of this module that
//! is genuinely load-bearing.
//!
//! A wrong *function* signature is nearly always caught: the loader passes a
//! handle and some scalars and forwards, so getting one wrong means a wrong
//! argument count, which is a compile error at the call site or an immediate
//! crash. A wrong *structure* layout is caught by nothing. The caller writes
//! field `A`, the driver reads field `B`, and the result is a plausible wrong
//! answer — the failure mode this tree keeps filing bugs about. Since the
//! loader never reads a field of those structures, declaring one would buy
//! nothing and stake everything.
//!
//! The rule is therefore not "never declare one". It is:
//!
//! > **Declare a structure only when the loader must read a field of it, and
//! > argue the case where the reading happens.**
//!
//! [`ExtensionProperties`] is the only item that has met that bar, and
//! [`crate::global`] is where it is argued. The short version: the loader's
//! answer to `vkEnumerateInstanceExtensionProperties` is the de-duplicated union
//! of every driver's list, there is no instance yet and so no driver to forward
//! to, and de-duplicating means comparing extension *names* — which means
//! knowing where the name is. What makes it survivable is that it is two fields
//! with no `pNext` and no version-dependent tail, and that its size and
//! alignment are asserted at compile time, so the failure mode the paragraph
//! above describes is a build error here rather than a wrong answer at runtime.
//!
//! When the next command tempts you, the question to ask is not "would a struct
//! be convenient" but "does the loader have to read a field, and can the layout
//! be pinned by an assertion the compiler checks?" Two noes and one yes is not
//! enough.
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

/// `VK_ERROR_LAYER_NOT_PRESENT`.
///
/// The loader's answer when asked for the extensions belonging to a named
/// layer. There are no layers on SlateOS — loading one needs `dlopen`, which
/// returns null — so every name is absent, and saying so is more useful than
/// reporting an empty list for a layer that does not exist.
pub const VK_ERROR_LAYER_NOT_PRESENT: VkResult = -6;

/// `VK_ERROR_INCOMPATIBLE_DRIVER`.
///
/// The loader sees this in two quite different roles, which is worth keeping
/// straight. From a driver's interface-version handshake it means "I cannot
/// work with a loader like you" and the correct response is to skip that
/// driver silently. Returned from `vkCreateInstance` to the *application* it
/// means "this machine has no driver that can serve you at all", which is a
/// real, reportable failure.
pub const VK_ERROR_INCOMPATIBLE_DRIVER: VkResult = -9;

/// Any Vulkan enumeration — `VkFormat`, `VkImageType`, `VkImageTiling` and the
/// rest — as the loader passes it: a 32-bit value it does not interpret.
///
/// Signed because that is what a C `enum` is. Every Vulkan enumeration carries a
/// `..._MAX_ENUM = 0x7FFFFFFF` member for the express purpose of pinning its
/// underlying type to a 32-bit signed integer, so this is the header's own
/// choice rather than a guess. On every calling convention the loader targets it
/// occupies the same argument slot as the `u32` a flags word does; the
/// distinction is kept anyway, because "this parameter is an enumeration" is a
/// fact about the API that costs one word to record and cannot be recovered from
/// `u32`.
///
/// Deliberately not an enum on the Rust side, for the reason given on
/// [`VkResult`]: a driver may be asked about a format from an extension this
/// loader has never heard of, and receiving a value outside a fixed variant set
/// is undefined behaviour.
pub type VkEnum = i32;

/// Any Vulkan flags word — `VkImageUsageFlags`, `VkImageCreateFlags` — as the
/// loader passes it.
///
/// `VkFlags` is `typedef uint32_t VkFlags` in the header, so unlike [`VkEnum`]
/// there is nothing to infer.
pub type VkFlags = u32;

/// `VK_MAX_EXTENSION_NAME_SIZE`.
pub const MAX_EXTENSION_NAME_SIZE: usize = 256;

/// `VkExtensionProperties` — the only Vulkan structure this module declares.
///
/// The rule that permits it, and the argument that it is the only thing meeting
/// that rule, are in this module's documentation and in [`crate::global`]. In
/// one line: `vkEnumerateInstanceExtensionProperties` has no instance and so no
/// driver to forward to, its answer is a union that must be de-duplicated, and
/// de-duplication compares names.
///
/// `extensionName` is declared `[u8; 256]` rather than `[c_char; 256]`. The two
/// are layout-identical, and the byte array drops a signedness question that
/// bears on nothing: the loader compares these bytes and never interprets them
/// as characters, which is also what this tree's rule about data crossing the
/// OS boundary already asks for.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExtensionProperties {
    /// `extensionName`. NUL-terminated; the bytes after the NUL are
    /// unspecified, which is why a reader must stop at it rather than compare
    /// the whole array.
    pub extension_name: [u8; MAX_EXTENSION_NAME_SIZE],
    /// `specVersion` — the version of the extension itself, which has nothing
    /// to do with the Vulkan API version.
    pub spec_version: u32,
}

// The layout is the entire risk this one declaration takes on, so it is checked
// rather than trusted. `char[256]` at offset 0 followed by a `uint32_t` is 260
// bytes at alignment 4; a mistake is a build failure here rather than a driver
// reading the wrong field at runtime. This assertion is what makes the
// declaration a bounded risk instead of the open-ended one the module
// documentation refuses to take.
const _: () = assert!(core::mem::size_of::<ExtensionProperties>() == 260);
const _: () = assert!(core::mem::align_of::<ExtensionProperties>() == 4);

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

/// `PFN_vkEnumerateInstanceExtensionProperties`.
///
/// Takes no handle, which is the point of it: an application asks this before it
/// has an instance, and the loader asks each *driver* the same way, through that
/// driver's `vk_icdGetInstanceProcAddr` with a null instance.
///
/// This is the one signature in the module naming a declared structure rather
/// than `*mut c_void`, because it is the one command whose answer the loader has
/// to read rather than pass along. See [`ExtensionProperties`].
///
/// # Safety
///
/// `layer_name` must be null or a NUL-terminated string, `count` a writable
/// `u32`, and `out` either null or an array of at least `*count` records.
pub type EnumerateInstanceExtensionPropertiesFn = unsafe extern "C" fn(
    layer_name: *const c_char,
    count: *mut u32,
    out: *mut ExtensionProperties,
) -> VkResult;

/// `PFN_vkEnumeratePhysicalDevices`.
///
/// # Safety
///
/// `instance` must be a handle the callee created, `count` a writable `u32`,
/// and `out` either null or an array of at least `*count` handles.
pub type EnumeratePhysicalDevicesFn =
    unsafe extern "C" fn(instance: Handle, count: *mut u32, out: *mut Handle) -> VkResult;

/// `PFN_vkGetDeviceProcAddr`.
///
/// The same shape as [`GetInstanceProcAddrFn`], and deliberately a separate
/// name rather than an alias. The two are not interchangeable: what may be
/// asked of each differs, and — the part that matters here — the pointer this
/// one returns is specific to the `device` it was asked with, whereas an
/// instance-level pointer serves every device that instance has. A single type
/// would make passing one where the other belongs a silent success.
///
/// # Safety
///
/// `device` must be null or a `VkDevice` the callee created, and `name` must
/// point to a NUL-terminated string that stays valid for the call.
pub type GetDeviceProcAddrFn = unsafe extern "C" fn(device: Handle, name: *const c_char) -> VoidFn;

/// `PFN_vkCreateDevice`.
///
/// `create_info` is a `VkDeviceCreateInfo*` and is passed through unread, for
/// the reason given on [`CreateInstanceFn`].
///
/// # Safety
///
/// `physical_device` must be a `VkPhysicalDevice` the callee reported,
/// `create_info` a valid `VkDeviceCreateInfo*`, `allocator` a valid
/// `VkAllocationCallbacks*` or null, and `out` a writable handle slot.
pub type CreateDeviceFn = unsafe extern "C" fn(
    physical_device: Handle,
    create_info: *const c_void,
    allocator: *const c_void,
    out: *mut Handle,
) -> VkResult;

/// `PFN_vkDestroyDevice`.
///
/// # Safety
///
/// `device` must be null or a handle the callee created and has not already
/// destroyed; `allocator` must match the one creation was given.
pub type DestroyDeviceFn = unsafe extern "C" fn(device: Handle, allocator: *const c_void);

// ---------------------------------------------------------------------------
// The physical-device commands
// ---------------------------------------------------------------------------
//
// These nine, plus `vkCreateDevice` above, are every Vulkan 1.0 command whose
// first parameter is a `VkPhysicalDevice`. The loader has to name each one
// because it *wraps* physical devices: an application's `VkPhysicalDevice` is
// a loader object, and the driver has never seen it, so each of these commands
// needs a trampoline that substitutes the driver's handle before forwarding.
// That cost is the mirror image of what adopting the driver's `VkDevice` avoids
// for the several hundred device commands, and is argued for in
// `crate::physical`.
//
// Every structure parameter below is `c_void`. See the module documentation:
// none of these are read by the loader, and declaring their layouts is the one
// mistake this module exists to not make.

/// `PFN_vkGetPhysicalDeviceProperties`.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported, and `out` a writable
/// `VkPhysicalDeviceProperties`.
pub type GetPhysicalDevicePropertiesFn =
    unsafe extern "C" fn(physical_device: Handle, out: *mut c_void);

/// `PFN_vkGetPhysicalDeviceFeatures`.
///
/// # Safety
///
/// As [`GetPhysicalDevicePropertiesFn`], with `out` a `VkPhysicalDeviceFeatures`.
pub type GetPhysicalDeviceFeaturesFn =
    unsafe extern "C" fn(physical_device: Handle, out: *mut c_void);

/// `PFN_vkGetPhysicalDeviceMemoryProperties`.
///
/// # Safety
///
/// As [`GetPhysicalDevicePropertiesFn`], with `out` a
/// `VkPhysicalDeviceMemoryProperties`.
pub type GetPhysicalDeviceMemoryPropertiesFn =
    unsafe extern "C" fn(physical_device: Handle, out: *mut c_void);

/// `PFN_vkGetPhysicalDeviceQueueFamilyProperties`.
///
/// The count-then-array shape Vulkan uses everywhere, and the one an application
/// cannot avoid: a queue family index is required to create a device, and this
/// is the only command that reports what families exist.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported, `count` a writable
/// `u32`, and `out` either null or an array of at least `*count`
/// `VkQueueFamilyProperties`.
pub type GetPhysicalDeviceQueueFamilyPropertiesFn =
    unsafe extern "C" fn(physical_device: Handle, count: *mut u32, out: *mut c_void);

/// `PFN_vkGetPhysicalDeviceFormatProperties`.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported and `out` a writable
/// `VkFormatProperties`. `format` is not validated by the loader.
pub type GetPhysicalDeviceFormatPropertiesFn =
    unsafe extern "C" fn(physical_device: Handle, format: VkEnum, out: *mut c_void);

/// `PFN_vkGetPhysicalDeviceImageFormatProperties`.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported and `out` a writable
/// `VkImageFormatProperties`. The five scalars are passed through unvalidated.
pub type GetPhysicalDeviceImageFormatPropertiesFn = unsafe extern "C" fn(
    physical_device: Handle,
    format: VkEnum,
    image_type: VkEnum,
    tiling: VkEnum,
    usage: VkFlags,
    flags: VkFlags,
    out: *mut c_void,
) -> VkResult;

/// `PFN_vkGetPhysicalDeviceSparseImageFormatProperties`.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported, `count` a writable
/// `u32`, and `out` either null or an array of at least `*count`
/// `VkSparseImageFormatProperties`.
pub type GetPhysicalDeviceSparseImageFormatPropertiesFn = unsafe extern "C" fn(
    physical_device: Handle,
    format: VkEnum,
    image_type: VkEnum,
    samples: VkFlags,
    usage: VkFlags,
    tiling: VkEnum,
    count: *mut u32,
    out: *mut c_void,
);

/// `PFN_vkEnumerateDeviceExtensionProperties`.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported, `layer_name` null or
/// a NUL-terminated string valid for the call, `count` a writable `u32`, and
/// `out` either null or an array of at least `*count` `VkExtensionProperties`.
pub type EnumerateDeviceExtensionPropertiesFn = unsafe extern "C" fn(
    physical_device: Handle,
    layer_name: *const c_char,
    count: *mut u32,
    out: *mut c_void,
) -> VkResult;

/// `PFN_vkEnumerateDeviceLayerProperties`.
///
/// Deprecated by Vulkan — device layers no longer exist — but still a command a
/// conforming loader answers, and still one only the driver can answer for.
///
/// # Safety
///
/// `physical_device` must be a handle the callee reported, `count` a writable
/// `u32`, and `out` either null or an array of at least `*count`
/// `VkLayerProperties`.
pub type EnumerateDeviceLayerPropertiesFn =
    unsafe extern "C" fn(physical_device: Handle, count: *mut u32, out: *mut c_void) -> VkResult;

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
