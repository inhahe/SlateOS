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
//! # What is deliberately not here yet
//!
//! The exported symbols are `vkGetInstanceProcAddr`, `vkCreateInstance`,
//! `vkDestroyInstance` and `vkEnumeratePhysicalDevices`, plus SlateOS's own
//! `vk_slateosRegisterDriver`. That is the whole list, and the omissions are
//! omissions rather than stubs: `vkEnumerateInstanceExtensionProperties`,
//! `vkEnumerateInstanceLayerProperties` and `vkEnumerateInstanceVersion` are
//! *not exported at all*, so an application that needs one fails to link with
//! that symbol named.
//!
//! Exporting them to return an empty list would be the alternative, and it is
//! the defect this tree has been filing bugs about — a tool that reports
//! success for work it never did. A link error names the missing thing; an
//! empty extension list produces a bug report about the driver.
//!
//! Device-level Vulkan — `vkCreateDevice` and everything a `VkDevice`
//! dispatches — is the next layer, and needs a device dispatch table per driver
//! rather than the one instance-level table [`entry`] has today.

#![no_std]

extern crate alloc;

pub mod dispatch;
pub mod entry;
pub mod icd;
pub mod instance;
pub mod registry;
pub mod vk;
