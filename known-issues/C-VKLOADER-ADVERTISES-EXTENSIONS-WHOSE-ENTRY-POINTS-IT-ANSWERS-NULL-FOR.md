## C-VKLOADER-ADVERTISES-EXTENSIONS-WHOSE-ENTRY-POINTS-IT-ANSWERS-NULL-FOR — PARTIALLY FIXED 2026-09-02 (lane C)

**Physical-device half FIXED 2026-09-02** — design-decisions.md §803,
`gui/vulkan/src/unknown.rs`. That is the WSI family and most other extension
commands, and it is the half that was blocking real applications.
**Instance-level half PARTLY FIXED 2026-09-22** — `VK_EXT_debug_utils`'
messenger pair is implemented (`gui/vulkan/src/messenger.rs`,
design-decisions.md §867); the surface family is still open and is blocked on a
prerequisite rather than a decision. See "What remains" at the bottom. The
original report follows unchanged.

**In short:** As of design-decisions.md §802 the Vulkan loader answers
"which optional add-ons are available?" with the union of every installed
driver's list. It has no way to hand out the *functions* those add-ons
consist of, so an application that asks the advertised question, gets
"yes", and then asks for the function gets nothing back. The loader tells
the truth about what exists and then cannot deliver it.

This is the exact failure shape §802 was written to argue against, created
by §802 itself, and it was not noticed while writing it — which is why it
is filed rather than quietly fixed in the same commit.

### What happens

`vkEnumerateInstanceExtensionProperties` now reports, say,
`VK_KHR_surface`, because a registered driver reported it. The application
enables it in `vkCreateInstance` — which succeeds, because the create-info
is passed through to the drivers untouched and at least one of them
supports it (§578: a partial success is a success). It then calls
`vkGetInstanceProcAddr(instance, "vkGetPhysicalDeviceSurfaceSupportKHR")`
and gets **null**.

`gui/vulkan/src/entry.rs`'s `get_instance_proc_addr` ends in
`physical::lookup(name).map(physical_trampoline)`, and
`gui/vulkan/src/physical.rs` matches exactly nine byte-string literals —
the core Vulkan 1.0 physical-device commands. Anything else is null. That
was correct and deliberate before §802 (`physical.rs`'s own test
`a_prefix_of_a_command_is_not_that_command` asserts
`vkGetPhysicalDeviceProperties2` is null); what changed is that the loader
now *advertises* the things it then declines to name.

Per the Vulkan specification this is a conformance failure: if an
extension is supported and enabled, `vkGetInstanceProcAddr` must return a
valid pointer for its commands.

### Why it cannot be fixed by forwarding

The reason is §801's, and it is the whole cost of wrapping: a
`VkPhysicalDevice` the loader hands out is a **loader-owned object the
driver has never seen**. Passing an unknown command straight through would
hand the driver a pointer to a `crate::instance::PhysicalDevice` where it
expects its own handle — memory corruption, not an error. The same is true
of `VkInstance`, which is likewise a loader object fanning out to several
drivers.

So every command needs its first argument unwrapped, and the loader has no
signature for a command it has never heard of.

### What the proper fix is

Two halves, and they are not equally hard:

- **Commands whose first argument is a `VkPhysicalDevice`** — the WSI
  family (`vkGetPhysicalDeviceSurfaceSupportKHR`,
  `…SurfaceCapabilitiesKHR`, `…SurfaceFormatsKHR`, `…PresentModesKHR`) and
  most other extension physical-device commands. These are fixable
  *generically*, with no signature knowledge at all: a naked trampoline
  that replaces argument 0 with the unwrapped driver handle and **tail-jumps**
  to the driver's function, leaving every other register and the whole
  stack untouched. This is what the Khronos loader does
  (`unknown_ext_chain_*.asm`), and it is the reason
  `vk_icdGetPhysicalDeviceProcAddr` exists in interface version 4 at all —
  it is the only way to ask a driver "is this unknown name a
  physical-device command?", and `gui/vulkan/src/physical.rs` already asks
  through it first. Needs `#[unsafe(naked)]` + `naked_asm!`, a per-slot
  table of (driver function, slot index), and a fixed pool of slots.

- **Commands whose first argument is a `VkInstance`** — these cannot be
  mechanised, because the loader must decide a *fan-out policy* per
  command (which driver answers? all of them? the first that succeeds?),
  and that is a decision, not a forwarding rule. The Khronos loader
  generates a trampoline per command from `vk.xml`. Ours would need the
  same, and it is the point at which "declare no Vulkan structures"
  finally becomes untenable.

### What to do in the meantime

Nothing silently. The gap is now stated in `gui/vulkan/src/global.rs`'s
module documentation next to the promise that creates it, so the next
reader of the union policy meets its limit in the same breath.

Do **not** "fix" this by narrowing the reported list to what the loader can
dispatch. Many instance extensions add no commands at all
(`VK_KHR_portability_enumeration` is a flag and nothing else), so
intersecting with the loader's dispatch table would deny an application
extensions that need no dispatching — and for the rest it would reproduce
exactly the empty-list stub §578 refused to write.

**Where it is:** `get_instance_proc_addr` in `gui/vulkan/src/entry.rs`;
the nine-name match in `gui/vulkan/src/physical.rs`; the promise in
`enumerate_instance_extension_properties` and `gui/vulkan/src/global.rs`.

### What was fixed, 2026-09-02

The first bullet above, exactly as described. `gui/vulkan/src/unknown.rs` adds a
fixed pool of 128 `#[unsafe(naked)]` trampolines; `get_instance_proc_addr` now
falls through to `unknown_across`, which asks every driver behind the instance
through `vk_icdGetPhysicalDeviceProcAddr`, assigns the name a slot if any of
them answered, records each driver's function in that driver's own table, and
hands back the slot's trampoline. Full reasoning in design-decisions.md §803.

An application enabling `VK_KHR_surface` now gets a working
`vkGetPhysicalDeviceSurfaceSupportKHR` and its siblings.

Two limitations came with it and are deliberate, not oversights:

- **Drivers below interface version 4 contribute nothing to this path**, even
  when they do export the symbol. `vkGetInstanceProcAddr` answers for
  device-level commands too and does not say which kind it gave, so using it
  here would eventually hand a trampoline a `VkDevice` and read two words out
  of the middle of the driver's own object. There is no safe way to ask such a
  driver the question, so it is not asked.
- **The pool is 128 distinct physical-device extension command names**, never
  recycled. The 129th is reported missing rather than stealing an earlier
  slot, because a reassigned slot would silently turn one command into another
  in an application that did nothing wrong. Raising the ceiling is a one-line
  change if a real machine ever approaches it; the whole of WSI is a
  single-figure number of names.

### What remains

**The instance-level half is now partly closed.** `VK_EXT_debug_utils`' two
commands — `vkCreateDebugUtilsMessengerEXT` and
`vkDestroyDebugUtilsMessengerEXT` — are implemented in
`gui/vulkan/src/messenger.rs` and answered from `get_instance_proc_addr`, with
a fan-out policy recorded as design-decisions.md §867: every driver that offers
the command gets a messenger, a driver that does not implement the extension is
not a failure, and a driver that offers it and then fails unwinds the whole
call. The loader answers **null** when no driver implements it, rather than
handing back a pointer that could only fail — which is this entry's own defect,
in the one direction it had not yet been fixed.

**What is left is the surface family, and it is blocked on more than a policy.**
The original text below said the remaining work needed a per-command fan-out
policy and a generated trampoline per command. That is true and it is not the
gate. Two things are:

1. **There is no driver to fan out to.** `posix::dlfcn::dlopen` returns null on
   SlateOS, so the registry takes drivers by registration rather than
   discovery — and `vk_slateosRegisterDriver` has no caller anywhere in the
   tree outside this crate's own tests. Every ICD that exists is a stub in
   `#[cfg(test)]`. That was equally true when the physical-device half was
   built, and it did not block that work, because a trampoline is
   *driver-agnostic*: one mechanism forwards every signature, so it is correct
   before any driver exists. A fan-out policy is not — it is a statement about
   what one specific command means when several drivers could answer, and you
   cannot write it for `vkCreateXlibSurfaceKHR` without deciding what a surface
   *is* on this operating system.
2. **There is no surface concept to decide about.** SlateOS has no X11 or
   Win32; the display belongs to the compositor in `gui/`. So the platform
   surface command this loader would implement is not one of the ones in
   `vk.xml` — it is one we would be inventing, together with the driver-side
   contract for it. That is a graphics-stack design task, not a loader task,
   and it is downstream of the Mesa port the roadmap already has blocked.

**The distinction worth keeping** is between the two reasons a command can be
unforwardable. `vkCreateDebugUtilsMessengerEXT` was blocked on a *decision*,
and a decision can be made at any time — which is why it is now done. The
surface commands are blocked on a *prerequisite*, and no amount of deciding
moves them. Reading the original text below, a reader would have started
designing a policy table and discovered the emptiness only after building it.

The Khronos loader generates one trampoline per command from `vk.xml`. Ours
would need the same for the surface family, and it is the point at which
"declare no Vulkan structures" (§802) finally becomes untenable — a
surface-creation command takes a structure the loader has to read to know which
platform it is for. Notably the messenger pair did **not** force that: its
create-info pointer is passed to every driver untouched, so the loader never
reads a field. That is a property of that command rather than a reprieve.

#### The original text, unchanged

A command whose first argument is a `VkInstance` still cannot be forwarded,
because the loader holds several driver instances behind one handle and must
decide *per command* which of them answers — a policy, not a forwarding rule,
and not something a trampoline can encode. `vkCreateDebugUtilsMessengerEXT`,
`vkDestroySurfaceKHR` and the platform `vkCreate*SurfaceKHR` calls are in this
group, so WSI is reachable but not yet complete.
