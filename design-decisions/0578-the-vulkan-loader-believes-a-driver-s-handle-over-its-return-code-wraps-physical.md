## 578. The Vulkan loader believes a driver's handle over its return code, wraps physical devices but adopts instances, and does not export the three commands it cannot answer

**Date:** 2026-09-02

**Lane:** C

**Decided by:** Claude (autonomous)

**In short:** §577 built the Vulkan loader's *decisions* — which driver
version to settle on, and when it is safe to write to a handle. This entry
covers the layer that actually calls drivers: what happens when several
graphics drivers are installed and one of them misbehaves. Four choices,
each with a real case against it: a driver that says "I succeeded" but hands
back nothing is disbelieved; one driver returning a malformed object aborts
the whole call rather than being skipped; the loader puts its own small
object around each graphics card rather than writing into the driver's; and
three Vulkan functions the loader cannot yet answer honestly are left out of
the library entirely, so a program needing one fails to *build* instead of
being told there is nothing available.

**Background.** `vkCreateInstance` is the call an application makes to start
using Vulkan. With several drivers registered the loader has to make it once
per driver and then present the results as a single object. Khronos's
Loader–Driver Interface has exactly one normative rule about that fan-out:

> A loader **must** return **VK_ERROR_INCOMPATIBLE_DRIVER** if it fails to
> find and load a valid Vulkan driver on the system.

Everything else below is loader policy, and is labelled as such in the code,
because the document is silent on it — there is no stated rule for what to do
when one driver of three fails, and no stated error precedence.

**Decision 1 — a success code with a null handle is not believed.** A driver
that returns `>= VK_SUCCESS` and leaves the out-parameter null is recorded as
having failed with `VK_ERROR_INITIALIZATION_FAILED`.

*Alternative:* believe the return code, and put the null in the fan-out list.

*For disbelieving:* the two halves of the answer contradict each other and
one of them has to be discarded. Believing the code costs a null pointer in
the list of instances the loader calls through on every subsequent command —
a crash on the first use, in the loader, attributed to the loader.
Disbelieving costs at worst one usable driver on a machine whose driver is
already broken. The asymmetry is not close.

*Against:* a driver could in principle have a legitimate reason to report
success without a handle; none is described in the specification, and no such
driver is known.

**Decision 2 — one unstamped instance sinks the whole call.** If any driver
returns a `VkInstance` whose first word does not carry `ICD_LOADER_MAGIC`,
the loader destroys every instance it created and fails the call, rather than
dropping that one driver and continuing.

*Alternative:* skip the offending driver, keep the rest.

*For sinking:* the check is the only evidence the loader will ever have that
this driver understands the dispatchable-handle contract at all (§577,
decision 2). A driver that got the first word wrong is a driver whose objects
the loader must not write to — and it is about to be asked for physical
devices, queues and command buffers, every one of which needs the same
stamp. Continuing means trusting, for the rest of the process's life, a
driver that has already failed the one check the loader can perform.

*Against:* this is strictly harsher than the specification requires, and on a
machine with a good discrete GPU and one broken software rasteriser it turns
a working Vulkan into no Vulkan. That is the real cost, and it is accepted
because the alternative failure is a silent write into a stranger's struct.

**Decision 3 — instances are adopted, physical devices are wrapped.** The
`VkInstance` each driver returns has its dispatch word overwritten with the
loader's table, which is what the LDI describes:

> The loader will replace the first entry with a pointer to the dispatch
> table which is owned by the loader.

A `VkPhysicalDevice`, by contrast, is not touched: the loader allocates its
own small object holding the driver index and the driver's handle, and hands
*that* to the application.

*Alternative:* treat both the same way — adopt both, or wrap both.

*For the split:* they are asked different questions. An instance arrives back
at a loader entry point as the receiver of a call the loader is about to
forward, and the LDI sentence above says plainly what to do with it; adopting
also gives the loader its one opportunity to notice a driver that never
stamped, which is decision 2. A physical device arrives as an *argument*, and
with several drivers registered a bare `VkPhysicalDevice` is
un-attributable — nothing in it says who made it, and asking each driver in
turn is both a fan-out and a guess. The wrapper makes that a field read.
Wrapping also leaves the driver's own object entirely alone, so a driver that
keeps state in the word ahead of its physical devices is not disturbed.

*Against:* two mechanisms where one would do, and an allocation per physical
device. The allocation happens once per instance, not per frame.

**Decision 4 — `vkEnumerateInstanceExtensionProperties`,
`vkEnumerateInstanceLayerProperties` and `vkEnumerateInstanceVersion` are not
exported at all.** Not stubs returning empty lists — absent symbols. An
application that calls one fails to link, naming it.

*Alternative:* export all three, returning an empty extension list, an empty
layer list, and version 1.0.

*For omitting:* the extension enumeration's honest answer is the
de-duplicated union of every registered driver's list, which is real work
this loader has not done yet; layers need `dlopen`, which does not exist
here; and the version depends on both. An empty list is not a smaller
version of that answer, it is a different and false one — and it is false in
the direction applications act on, because "no extensions" is a legitimate
reply that a well-written program handles by degrading rather than by
reporting a problem. The resulting bug report is about the *driver*, filed by
a user whose machine has a perfectly good driver. This is the same defect
lane C spent 2026-09-01 filing against `userspace/`
(`requests/c-b-2288-userspace-tools-report-success-for-work-they-never-did.md`):
a tool reporting success for work it never did. A link error names the
missing thing at build time, to the one person who can act on it.

*Against:* it makes the loader unusable by any application that calls those
functions unconditionally — which is most of them — so the loader is not yet
a drop-in. That is accurate: it is not.

*Note on the boundary:* `vkGetInstanceProcAddr` returning null for a command
it does not implement is **not** the same thing and is done freely. Null is
the C API's defined answer to "do you have this?", so a null is information,
whereas an empty extension list is an assertion.

**A note on `clippy::vec_box`, and why it is suppressed here.** The physical
devices are held as `Vec<Box<PhysicalDevice>>`, and clippy rejects that as an
unnecessary indirection — `Vec<T>` is already on the heap. For an ordinary
collection it would be right. It is wrong here for a reason that is not
visible in the type: the *address* of each `PhysicalDevice` is the
`VkPhysicalDevice` handle the application is given, and Vulkan requires an
instance to report the same handles for its whole life. In a
`Vec<PhysicalDevice>` every handle is an interior pointer into one buffer, so
the next `push` — GPU hot-plug being the obvious future one — reallocates and
every handle the application still holds dangles. The `Box` decouples each
device's lifetime from the spine, which is what the C API already promises.

This is the same shape as §577's note about `unnecessary_wraps`: the lint's
suggested fix and the lint's premise are different claims, and here the
premise ("the extra allocation buys nothing") is false because what it buys
is address stability rather than storage. Suppressed at the three sites with
the reasoning in-line rather than crate-wide, so that a fourth
`Vec<Box<...>>` with no such contract behind it still gets caught. The
stub driver in the tests boxes for the same reason and says so: an unboxed
one would model a driver that relocates its physical devices, which no
conforming driver may do, and the loader would then be tested against a
driver it must never meet.

**Where it lives:** `gui/vulkan/src/entry.rs` (the exported symbols, the
process-wide registry and its spin lock, and the private `*_across` helpers
that take a `&Registry` so every one of these cases is reachable from a
registry a test builds) and `gui/vulkan/src/instance.rs` (`Instance`,
`PhysicalDevice`, `array_query`, `outcome`).
