## 801. Wrapping a `VkPhysicalDevice` costs one hand-written forwarder per command, and the loader writes all nine

**Date:** 2026-09-02
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Before a program can draw anything it has to pick a graphics card
and ask that card what it can do — how much memory, which image formats, and
above all which *queue families* it has (a queue family is a group of the card's
work-submission slots; you must name one to create a device at all). Our loader
answered "no such command" to every one of those questions, because they were
simply not implemented. The consequence is the part worth remembering: the
device layer recorded in §800 was finished, tested and green, and **no
conforming application could reach it**, because the one command that reports
queue families was missing and `vkCreateDevice` cannot be called without a queue
family index. This entry adds the nine missing commands, and records why they
have to be written out one at a time when §800's several hundred device commands
did not.

Governs `gui/vulkan/src/physical.rs` and the physical-device half of
`gui/vulkan/src/entry.rs`.

### 1. The bill for wrapping, arriving

§800 chose to *adopt* the driver's `VkDevice` — hand the application the
driver's own handle with only its dispatch word restamped — rather than *wrap*
it, and gave the cost of wrapping as the reason. A wrapped handle is a pointer
the driver has never seen, so every command taking it must pass through the
loader; and to pass through the loader it must be *named* by the loader, which
means a command invented after the loader was written cannot work at all.

Physical devices are wrapped, and for a reason §800 also gives: one
`vkCreateInstance` fans out to every installed driver, so the loader merges
several drivers' physical devices into one list, and a bare driver handle coming
back would be un-attributable — with three drivers registered there is nothing
in the handle to say which produced it. That decision was taken when physical
devices were first enumerated. This entry is the invoice.

**It is still the right trade, and the numbers are the argument.** Vulkan 1.0
has exactly ten commands taking a `VkPhysicalDevice`. They are called a handful
of times while an application chooses a GPU and never again; none is on a
per-frame path; and the set is closed, because Vulkan 1.0 is finished. The set
the loader would have had to write had it wrapped devices instead is open-ended,
per-frame, and unknowable in advance. Ten bounded startup calls against several
hundred unbounded drawing calls is not a close question — but it is a real cost,
and pretending a decision is free is how the second half of it goes unbuilt for
a day, which is exactly what happened here.

### 2. A version-4 driver is asked through its version-4 entry point *first*, and the instance one is still asked after

A driver can be asked for a physical-device command two ways. Every driver
exports `vk_icdGetInstanceProcAddr`; from interface version 4 a driver may also
export `vk_icdGetPhysicalDeviceProcAddr`. The rule adopted is: ask the
version-4 one first if it exists, and if it answers null, ask the instance one.

**Why the version-4 one first.** An extension may define a command name that
exists at device level, at physical-device level, or at both. Asked through
`vkGetInstanceProcAddr` a driver returns a pointer either way, and the answer
carries no clue which kind it is — yet the two need opposite handling, since a
physical-device command's first argument must be unwrapped and a device
command's must not. `vk_icdGetPhysicalDeviceProcAddr` answers exactly one
question — "is this a physical-device command, and if so, here it is" — and null
means no. That disambiguation is the entire reason the entry point was added to
the interface, and preferring it is using the interface as designed.

**Why the fallback is not merely defensive.** It is required by the
Loader–Driver Interface, and there is a real case behind the requirement: a
driver may route only its *extension* physical-device commands through the
version-4 entry point and answer for the core ten through the instance one.
Treating a null from the first as final would lose `vkGetPhysicalDeviceProperties`
on a driver that is behaving correctly.

**What was rejected.** "Instance lookup only, it is simpler and works for all ten
core commands" — true today and wrong the first time an extension command is
added, which is the same defect as a hard-coded list of known commands. And
"version-4 only where available", which drops conforming drivers on the floor.

This finally gives `Driver::physical_device_proc_addr` a caller. It has been
exposed, version-gated and unit-tested since the registry was written, and until
now nothing in the tree read it — a gate correctly implemented and guarding
nothing.

### 3. All nine go in the instance dispatch table

`entry.rs`'s `Table` is what the first word of a `VkInstance` or
`VkPhysicalDevice` points at, and its stated membership rule is *what the handle
dispatches through*, not what level the command is named for. A
`VkPhysicalDevice` dispatches through this table, so by that rule the table is
exhaustive over the commands one can be called with, and the nine belong in it.

The alternative was to leave them out on the grounds that nothing dispatches
through the table by offset yet, so the entries would be unread. That is the
argument §800 makes *against* a device dispatch table — and it does not carry
over, because these nine are read: `vkGetInstanceProcAddr` returns them from the
table. Thirteen entries every one of which is read is a different proposition
from several hundred that none are. Leaving them out would have produced the
defect §800 names: a structure that looks like a working dispatch table right up
until someone relies on it.

### 4. `vkCreateDevice` is a physical-device command and is deliberately not in the table of them

`physical::Command` is the commands the loader **only forwards** — unwrap the
handle, call the driver, return what it says. `vkCreateDevice` does much more:
it creates loader state that outlives the call, and has its own ordering
requirements (§800). Listing it alongside nine entries whose defining property
is having no body would make the table mean two things.

### 5. What a driver that cannot supply one of these gets

All nine are core Vulkan 1.0, so a driver missing one is not a Vulkan 1.0
driver. It is still a case with a decision in it, and the decision follows what
`driver_destroy_device` already does: the command does what it can and no more.

| Shape of the command | What happens |
|---|---|
| Returns `VkResult` | `VK_ERROR_INITIALIZATION_FAILED` — a failure the application can act on. |
| Returns void, has a count out-parameter | The count is set to **zero**. |
| Returns void, has neither | The caller's structure is left untouched. |

The middle row is the one worth arguing. Leaving the count untouched would hand
the application whatever was on its stack and then have it read that many
structures out of an array that size — an arbitrarily large read from a garbage
count. Zero is a wrong answer that is survivable and legible: "this card has no
queue families" makes the application's own `vkCreateDevice` fail next, at a
point where it can report something.

The last row is the residue. There is nowhere to report from — the command
returns void and has no count — and the loader does not know the structure's
size, so it cannot even zero it. It is recorded here as noticed rather than
overlooked.

### The loader still declares no Vulkan structure layout, and that is the invariant rather than the tally

`vk.rs` used to justify itself by counting: "six declarations can be checked
against `vulkan_core.h` by reading; six hundred cannot." Nine commands later the
count is no longer the point, so the module now states the rule the count was
standing in for: **every structure is `*const c_void` or `*mut c_void`, and none
is declared.**

The asymmetry is why. A wrong *function* signature is nearly always caught,
because the loader passes a handle and some scalars and forwards — getting one
wrong means a wrong argument count, which is a compile error or an immediate
crash. A wrong *structure layout* is caught by nothing: the caller writes field
`A`, the driver reads field `B`, and out comes a plausible wrong answer. Since
the loader never reads a field of any of them, declaring one would buy nothing
and stake everything.

Two scalar aliases were added — `VkEnum` (signed, because every Vulkan
enumeration carries a `..._MAX_ENUM = 0x7FFFFFFF` member for the express purpose
of pinning it to a 32-bit signed integer) and `VkFlags` (`uint32_t` in the
header). They record which parameters are enumerations and which are flag words,
a fact `u32` everywhere would have thrown away for nothing gained.

### The shape to remember

The bug this entry fixes was not a wrong answer anywhere. Every test passed;
`device.rs` and its twelve tests were correct in isolation and would have stayed
correct forever. What was wrong was that nothing could *get there* — the layer
below did not export the one command needed to supply an argument the layer
above requires. A subsystem can be complete, tested, green and inert, and no
test that stays inside it will say so. The check that would have caught it is
the one asked from outside: *what sequence of calls does a real application
make, and does every step in it exist?*

**Where it is:** `gui/vulkan/src/physical.rs` (the command set, the ask-order
rule, and the argument for both); `physical_trampoline`, `forward`, and the nine
exported commands from `get_physical_device_properties` to
`enumerate_device_layer_properties` in `gui/vulkan/src/entry.rs`, along with the
nine new `Table` fields; `VkEnum`, `VkFlags` and the nine `PFN_` types in
`gui/vulkan/src/vk.rs`.
