//! The commands an application calls on a `VkPhysicalDevice`, and the price of
//! having wrapped one.
//!
//! # Why this module exists at all
//!
//! [`crate::device`] argues that a `VkDevice` should be the driver's own handle
//! with its dispatch word restamped, so that `vkCmdDraw` and its several hundred
//! siblings reach the driver with the loader nowhere in the call path. This
//! module is the other half of that argument, paid rather than avoided.
//!
//! A `VkPhysicalDevice` cannot be the driver's own handle. One
//! `vkCreateInstance` fans out to every installed driver, so the loader collects
//! physical devices from all of them into one list; a bare driver handle arriving
//! back at a loader entry point would be un-attributable, because with three
//! drivers registered there is nothing in the handle to say which one produced
//! it. So the loader **wraps**: the application's `VkPhysicalDevice` is a
//! [`crate::instance::PhysicalDevice`] the loader allocated, holding the driver's
//! real handle inside.
//!
//! The consequence is immediate and unavoidable: **every command taking a
//! `VkPhysicalDevice` must pass through the loader**, because the driver has
//! never seen the pointer the application holds. There is no forwarding by
//! default here and no way to arrange one — a command the loader has not named
//! is a command whose first argument nothing will unwrap. That is precisely the
//! cost `device` describes wrapping as having, stated there in the abstract and
//! here as nine functions.
//!
//! It is a bounded cost, which is why it is the right trade rather than merely
//! the forced one. Vulkan 1.0 has exactly ten physical-device commands, they are
//! called a handful of times during startup while an application chooses a GPU,
//! and none of them is on a per-frame path. The set the loader would have had to
//! trampoline had it wrapped devices instead is open-ended and per-frame.
//!
//! # What is in the table and what is not
//!
//! The ten commands are the nine in [`Command`] plus `vkCreateDevice`, and
//! `vkCreateDevice` is deliberately *not* a [`Command`]. This table is the
//! commands the loader **only forwards** — unwrap the handle, call the driver,
//! return whatever it says. `vkCreateDevice` does much more than that: it creates
//! loader state that outlives the call, and its own reasons for the order it does
//! things in. Listing it here would put a command with a body in a table whose
//! entries are defined by not having one.
//!
//! # Asking the driver: two entry points, in an order that matters
//!
//! A driver can be asked for a physical-device command two ways, and which are
//! available depends on the interface version it settled on in
//! [`crate::icd`]. From version 4 a driver may export
//! `vk_icdGetPhysicalDeviceProcAddr`; every driver has
//! `vk_icdGetInstanceProcAddr`.
//!
//! Where both exist the version-4 one is asked **first**, and this is not a
//! preference — it is the reason that entry point was added to the interface. An
//! extension can define a command name that exists at *device* level, at
//! *physical-device* level, or both. Asked through `vkGetInstanceProcAddr` a
//! driver returns a pointer either way, and the loader cannot tell from the
//! answer which kind it got — but the two need opposite treatment, since a
//! physical-device command's first argument must be unwrapped and a device
//! command's must not. `vk_icdGetPhysicalDeviceProcAddr` answers exactly one
//! question — "is this a physical-device command, and if so, here it is" — and
//! null means no.
//!
//! Falling back to `vkGetInstanceProcAddr` when it says no is required by the
//! Loader–Driver Interface, and is not merely defensive: a driver may route only
//! its *extension* physical-device commands through the version-4 entry point
//! and answer for the core ten through the instance one. Treating a null from
//! the first as final would lose `vkGetPhysicalDeviceProperties` on a driver
//! that is behaving correctly.
//!
//! [`Ask`] is that rule as a value, so that it can be stated once and tested,
//! rather than living as a comment inside nine near-identical trampolines.

use core::ffi::CStr;

/// A Vulkan 1.0 command the loader forwards to a driver after unwrapping the
/// `VkPhysicalDevice` it was called on.
///
/// `vkCreateDevice` is not a member; see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// `vkGetPhysicalDeviceProperties`.
    Properties,
    /// `vkGetPhysicalDeviceFeatures`.
    Features,
    /// `vkGetPhysicalDeviceMemoryProperties`.
    MemoryProperties,
    /// `vkGetPhysicalDeviceQueueFamilyProperties`.
    QueueFamilyProperties,
    /// `vkGetPhysicalDeviceFormatProperties`.
    FormatProperties,
    /// `vkGetPhysicalDeviceImageFormatProperties`.
    ImageFormatProperties,
    /// `vkGetPhysicalDeviceSparseImageFormatProperties`.
    SparseImageFormatProperties,
    /// `vkEnumerateDeviceExtensionProperties`.
    DeviceExtensionProperties,
    /// `vkEnumerateDeviceLayerProperties`.
    DeviceLayerProperties,
}

/// Every [`Command`], so that the set can be iterated rather than restated.
///
/// Exists for the round-trip test: a name misspelt in [`lookup`] and nowhere
/// else would make a core command permanently unreachable, and would look
/// exactly like a driver that does not implement it. Iterating the set is what
/// turns that from a silent wrong answer into a failing assertion.
pub const ALL: [Command; 9] = [
    Command::Properties,
    Command::Features,
    Command::MemoryProperties,
    Command::QueueFamilyProperties,
    Command::FormatProperties,
    Command::ImageFormatProperties,
    Command::SparseImageFormatProperties,
    Command::DeviceExtensionProperties,
    Command::DeviceLayerProperties,
];

impl Command {
    /// The name this command is known to Vulkan by.
    ///
    /// A `CStr`, because the only thing the loader does with it is hand it to a
    /// driver's `GetProcAddr`, which takes a NUL-terminated string.
    #[must_use]
    pub const fn name(self) -> &'static CStr {
        match self {
            Self::Properties => c"vkGetPhysicalDeviceProperties",
            Self::Features => c"vkGetPhysicalDeviceFeatures",
            Self::MemoryProperties => c"vkGetPhysicalDeviceMemoryProperties",
            Self::QueueFamilyProperties => c"vkGetPhysicalDeviceQueueFamilyProperties",
            Self::FormatProperties => c"vkGetPhysicalDeviceFormatProperties",
            Self::ImageFormatProperties => c"vkGetPhysicalDeviceImageFormatProperties",
            Self::SparseImageFormatProperties => c"vkGetPhysicalDeviceSparseImageFormatProperties",
            Self::DeviceExtensionProperties => c"vkEnumerateDeviceExtensionProperties",
            Self::DeviceLayerProperties => c"vkEnumerateDeviceLayerProperties",
        }
    }
}

/// Which [`Command`] a name is, if it is one of them.
///
/// `None` is not "the driver will answer this" — unlike [`crate::device::lookup`],
/// where an unrecognised name is forwarded. Here `None` means the loader has no
/// trampoline for the name and so cannot offer it at all, because the handle it
/// would be called with is one only the loader can unwrap. The difference between
/// the two modules on this exact point is the difference between wrapping a
/// handle and adopting one.
#[must_use]
pub fn lookup(name: &[u8]) -> Option<Command> {
    match name {
        b"vkGetPhysicalDeviceProperties" => Some(Command::Properties),
        b"vkGetPhysicalDeviceFeatures" => Some(Command::Features),
        b"vkGetPhysicalDeviceMemoryProperties" => Some(Command::MemoryProperties),
        b"vkGetPhysicalDeviceQueueFamilyProperties" => Some(Command::QueueFamilyProperties),
        b"vkGetPhysicalDeviceFormatProperties" => Some(Command::FormatProperties),
        b"vkGetPhysicalDeviceImageFormatProperties" => Some(Command::ImageFormatProperties),
        b"vkGetPhysicalDeviceSparseImageFormatProperties" => {
            Some(Command::SparseImageFormatProperties)
        }
        b"vkEnumerateDeviceExtensionProperties" => Some(Command::DeviceExtensionProperties),
        b"vkEnumerateDeviceLayerProperties" => Some(Command::DeviceLayerProperties),
        _ => None,
    }
}

/// How a driver is asked for a physical-device command.
///
/// The reasoning is in the module documentation; this is that reasoning as a
/// value so it is written once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Ask `vk_icdGetPhysicalDeviceProcAddr`, and on null ask
    /// `vk_icdGetInstanceProcAddr`. Both answers count; the first is preferred
    /// because only it distinguishes a physical-device command from a device
    /// command of the same name.
    PhysicalDeviceThenInstance,
    /// Ask `vk_icdGetInstanceProcAddr` and nothing else, because the driver
    /// settled below interface version 4 and exports no other lookup.
    InstanceOnly,
}

/// The [`Ask`] for a driver, given whether it offers
/// `vk_icdGetPhysicalDeviceProcAddr`.
///
/// The argument is deliberately the *availability of the pointer* rather than the
/// interface version. [`crate::registry::Driver::physical_device_proc_addr`]
/// already applies the version gate — it returns `None` for a driver below
/// version 4 even if one was supplied — and re-deciding the same thing from the
/// version here would be a second place for the gate to be got wrong.
#[must_use]
pub const fn ask(has_physical_device_proc_addr: bool) -> Ask {
    if has_physical_device_proc_addr {
        Ask::PhysicalDeviceThenInstance
    } else {
        Ask::InstanceOnly
    }
}

#[cfg(test)]
mod tests {
    use super::{ALL, Ask, Command, ask, lookup};

    #[test]
    fn every_command_is_found_under_its_own_name() {
        // The point of `ALL`: a name misspelt in exactly one of the two places
        // would otherwise be indistinguishable from a driver that does not
        // implement the command.
        for command in ALL {
            assert_eq!(
                lookup(command.name().to_bytes()),
                Some(command),
                "{command:?} is not reachable by the name it reports",
            );
        }
    }

    #[test]
    fn no_two_commands_share_a_name() {
        for (i, one) in ALL.iter().enumerate() {
            for two in ALL.iter().skip(i + 1) {
                assert_ne!(one.name(), two.name(), "{one:?} and {two:?} collide");
            }
        }
    }

    #[test]
    fn the_command_an_application_needs_to_choose_a_queue_family_is_present() {
        // Named on its own because it is the one whose absence is fatal rather
        // than merely limiting: `vkCreateDevice` requires a queue family index,
        // and this is the only command that reports which indices exist. Without
        // it the whole device layer is unreachable, which is the state this
        // module was written to end.
        assert_eq!(
            lookup(b"vkGetPhysicalDeviceQueueFamilyProperties"),
            Some(Command::QueueFamilyProperties),
        );
    }

    #[test]
    fn create_device_is_not_in_this_table() {
        // It is a physical-device command, and it is answered elsewhere, because
        // this table is the commands that are *only* forwarded.
        assert_eq!(lookup(b"vkCreateDevice"), None);
    }

    #[test]
    fn a_device_command_is_not_a_physical_device_command() {
        // `None` here means "the loader cannot offer this on a physical device",
        // not "ask the driver" — the distinction the module documentation draws
        // against `device::lookup`.
        assert_eq!(lookup(b"vkCmdDraw"), None);
        assert_eq!(lookup(b"vkDestroyDevice"), None);
    }

    #[test]
    fn a_prefix_of_a_command_is_not_that_command() {
        assert_eq!(lookup(b"vkGetPhysicalDeviceProperti"), None);
        assert_eq!(lookup(b"vkGetPhysicalDeviceProperties2"), None);
        assert_eq!(lookup(b""), None);
    }

    #[test]
    fn a_version_four_driver_is_asked_through_the_version_four_entry_point_first() {
        assert_eq!(ask(true), Ask::PhysicalDeviceThenInstance);
    }

    #[test]
    fn a_driver_without_the_version_four_entry_point_is_asked_only_once() {
        // Not "asked through the version-four entry point and expected to
        // return null" — the pointer does not exist, and calling it would be a
        // jump through something the driver never exported.
        assert_eq!(ask(false), Ask::InstanceOnly);
    }
}
