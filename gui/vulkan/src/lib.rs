//! The SlateOS Vulkan loader.
//!
//! # What a loader is for
//!
//! An application that wants to draw with a GPU does not talk to a graphics
//! driver directly. It links against one library — this one — and calls
//! `vkCreateInstance`. The loader's job is to find every graphics driver
//! installed on the machine, ask each one what it can do, and then stand
//! between the application and whichever driver ends up being used, so that
//! the application's code is the same whether the machine has an AMD card, an
//! Intel integrated GPU, a software rasteriser, or three of them at once.
//!
//! In Vulkan's vocabulary a driver is an **ICD** — an Installable Client
//! Driver. The rules governing what a loader may assume about an ICD, and
//! what an ICD may assume about a loader, are written down by Khronos as the
//! *Loader–Driver Interface*. Two of those rules are load-bearing enough that
//! getting them wrong corrupts memory rather than merely failing, and they are
//! what this crate implements first:
//!
//! | Module | The rule it encodes |
//! |---|---|
//! | [`icd`] | Which interface version the loader and a driver settle on, and what each version entitles either side to assume. |
//! | [`dispatch`] | The layout of a *dispatchable handle* — the first word of every `VkInstance`, `VkDevice`, `VkQueue` and `VkCommandBuffer` — and the check that must precede writing to it. |
//! | [`registry`] | The drivers this loader knows about: the handshake with each, the version it settled on, and — kept rather than discarded — the ones that were rejected and why. |
//! | [`instance`] | What one `vkCreateInstance` means across several drivers: which failure the application is told about, and the loader's own dispatchable instance and physical-device objects. |
//! | [`device`] | What one `vkCreateDevice` means when exactly one driver is behind it: the record a device's dispatch word points at, and which of the two device-level commands the loader must answer itself. |
//! | [`physical`] | The other side of that coin: the commands a wrapped `VkPhysicalDevice` forces the loader to name one by one, and the order a driver is asked for them in. |
//! | [`global`] | The three commands asked with no handle at all, before an instance exists — so the loader has to answer them itself rather than forward them. |
//! | [`unknown`] | The physical-device commands the loader has never heard of: how one piece of code forwards a signature nobody told it about. |
//! | [`entry`] | The exported symbols, the process-wide driver registry, and the dispatch table their addresses come from. |
//! | [`vk`] | The few Vulkan C types the loader's own signatures cannot avoid naming. Not a binding, and not becoming one. |
//!
//! Every module above [`entry`] is pure: it decides things, and something else
//! does the FFI. That split is deliberate and is argued for in each module's
//! own documentation. The short version is that the parts of a loader that get
//! this wrong are the parts that are hard to test, so this crate keeps the
//! policy in functions that take values and return values, and confines the
//! raw-pointer work to a thin layer around them — which is [`entry`], and is
//! why it is the only module that names a `static`.
//!
//! # Why drivers are registered rather than discovered
//!
//! On a conventional system the loader finds drivers by reading JSON manifest
//! files out of a handful of directories and `dlopen`ing the shared library
//! each manifest names. SlateOS cannot do that yet: `posix::dlfcn::dlopen` is
//! a stub that always returns null and sets the error string `"dynamic
//! linking not supported"`. A loader written around `dlopen` would therefore
//! find nothing on every machine, and — worse — would look like a loader that
//! works, right up until someone installed a driver.
//!
//! So drivers are **registered statically**: a driver is linked into the
//! image and hands the loader its entry points directly. This is not a
//! placeholder for the manifest scanner; it is the mechanism a manifest
//! scanner would eventually feed. Discovery answers *which* drivers exist,
//! and that is the only question `dlopen` is needed for. Everything after
//! that — negotiating a version, stamping handles, dispatching calls — is
//! identical either way, which is why it can be built and tested now.
//!
//! When dynamic loading arrives, the addition is a discovery step that
//! produces the same registration records this crate already consumes, plus
//! a JSON parser for the manifests. That parser belongs in `textfmt`, not
//! here: `apps/jsonviewer` already carries a private one, and a second
//! private copy is exactly the duplication `textfmt` exists to prevent.
//!
//! # A deliberate omission, and the day it stopped being one
//!
//! `vkEnumerateInstanceExtensionProperties`, `vkEnumerateInstanceLayerProperties`
//! and `vkEnumerateInstanceVersion` were for a while *not exported at all*, so
//! that an application needing one failed to link with the symbol named. The
//! alternative — exporting them to return an empty list — is the defect this
//! tree keeps filing bugs about: a tool reporting success for work it never did.
//! A link error names the missing thing; an empty extension list produces a bug
//! report about the driver.
//!
//! That argument had an expiry date, and [`global`] is where it ran out. It only
//! ever justified the omission *while there was no honest answer*, and there now
//! is one for each: the union of the drivers' extension lists, an empty layer
//! list because loading a layer needs `dlopen`, and Vulkan 1.0 because that is
//! what this loader implements. The distinction that matters, and is easy to
//! lose: an empty list computed from a real registry is a correct answer; an
//! empty list returned without looking is a lie that happens to be short.
//!
//! The habit worth taking from it is to write the closing condition next to the
//! omission. Nothing in the tree was watching for the moment the reason stopped
//! applying.
//!
//! Device-level Vulkan is the layer [`device`] adds, and building it corrected
//! a guess this paragraph used to state as fact. It said a device dispatch
//! table was needed *per driver*, on the reasoning that the driver's
//! `vkGetDeviceProcAddr` is a per-driver fact and two devices from one driver
//! could share a record. They cannot: `vkGetDeviceProcAddr` returns a pointer
//! specific to the device it was asked about, which is the whole point of
//! device-level dispatch, so the record is **per device**. It is recorded here
//! rather than quietly deleted because the wrong version is the one that sounds
//! right.
//!
//! What is exported at that level is `vkCreateDevice`, `vkGetDeviceProcAddr`
//! and `vkDestroyDevice` — and no device commands at all. A `VkDevice` this
//! loader returns *is* the driver's, so `vkCmdDraw` and its several hundred
//! siblings are reached through the driver's own `vkGetDeviceProcAddr` with the
//! loader nowhere in the call path, which is the arrangement Vulkan separates
//! device-level dispatch in order to allow.
//!
//! # The asymmetry that makes both halves work
//!
//! Handing back the driver's own `VkDevice` is what lets the device half export
//! three symbols and cover an open-ended API. The instance half cannot do the
//! same trick, and [`physical`] is the bill: because a `VkPhysicalDevice` is a
//! loader object the driver has never seen, **every** command taking one must be
//! named and trampolined, and there are ten of them — `vkCreateDevice` plus the
//! nine of [`physical::Command`].
//!
//! Building them was not a rounding-out exercise. Until they existed the loader
//! answered null for `vkGetPhysicalDeviceQueueFamilyProperties`, which is the
//! only command that reports which queue families a GPU has, and
//! `vkCreateDevice` cannot be called without a queue family index. The device
//! layer above was therefore *unreachable by a conforming application* despite
//! being complete and tested — a subsystem correct in isolation and inert in
//! place. That is worth remembering as a shape: the tests all passed.
//!
//! Naming ten commands is affordable. Naming every physical-device command any
//! extension will ever define is not, and [`global`] made that bill due the day
//! it started reporting the drivers' extensions honestly: an application told
//! `VK_KHR_surface` exists asks for an entry point [`physical`] has never heard
//! of. [`unknown`] is the escape, and it is a narrow one — three instructions of
//! assembly that swap argument 0 and *jump*, which forwards every signature at
//! once because neither calling convention lets the first argument's identity
//! affect where the others live. It costs the loader its portability, which is
//! the honest price of the wrapping [`physical`] is the bill for.
//!
//! It does not extend upward. A command taking a `VkInstance` needs a per-command
//! *fan-out policy* — which of several drivers answers — and a policy is not
//! something three instructions can carry. That half is still open, and it is
//! filed rather than hidden.

#![no_std]

extern crate alloc;

pub mod device;
pub mod dispatch;
pub mod entry;
pub mod global;
pub mod icd;
pub mod instance;
pub mod physical;
pub mod registry;
pub mod unknown;
pub mod vk;
