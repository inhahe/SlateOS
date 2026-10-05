## 800. The Vulkan loader hands back the driver's own `VkDevice`, keeps one record per device rather than per driver, and forwards every device command it has not heard of

**Date:** 2026-09-02
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A Vulkan "device" is the handle a program uses to actually draw —
it gets one from a graphics driver and then makes thousands of calls a second
through it. Our loader sits between the program and the driver, and had to
decide how much of itself to leave in that path. It leaves none: the handle the
program receives *is* the driver's handle, so drawing calls go straight to the
driver. The loader keeps a small private record for each device, off to one
side, and only intercepts the two calls it genuinely has to.

Three decisions, taken together because each is the reason the next one is
available. They govern `gui/vulkan/src/device.rs` and the device half of
`gui/vulkan/src/entry.rs`.

### 1. Adopt the driver's handle, do not wrap it

The layer below this one wraps. `vkEnumeratePhysicalDevices` hands the
application a `PhysicalDevice` the *loader* allocated, holding the driver's real
handle inside, because one `vkCreateInstance` fans out to every installed driver
and a bare physical-device handle would be un-attributable — with three drivers
registered there is nothing in the handle itself to say which one produced it.

A device does not have that problem. An application creates a device *from a
particular physical device*, that physical device belongs to exactly one driver,
and so does the device. There is nothing to fan out to and nothing to choose
between, which means the loader can take the driver's `VkDevice` and stamp its
own record into the dispatch word — offset 0 of every dispatchable Vulkan handle,
which the Loader–Driver Interface gives the loader outright — rather than
allocating something new around it.

**The alternative and what it costs.** Wrapping would work and would be
consistent with the physical-device layer. It would also mean that every device
command the application ever calls arrives at the loader holding a pointer the
driver has never seen, so the loader would have to substitute the real handle
and forward — for every command, forever, including commands invented after this
loader was written, which it therefore could not name. That is not a
micro-optimisation being traded away; it is the loader inserting itself into a
path that runs thousands of times per frame, and it would require the loader to
export a trampoline for the entire device API in order to work at all.

Adopting means the pointer the application holds is already the one the driver
wants, so `vkCmdDraw` and its several hundred siblings reach the driver **with
the loader nowhere in the call path**. That separation is the reason Vulkan has
device-level dispatch distinct from instance-level dispatch at all, and a loader
that wrapped devices would throw it away for symmetry with a layer whose problem
it does not share.

**What is given up:** the loader can no longer tell a `VkDevice` of its own from
one it never saw by looking at the pointer, because there is nothing of the
loader's in the pointer. It reads the dispatch word instead, which is a load
rather than a search, and is what the word is for.

### 2. One record per device, not one per driver

Tempting to share: the driver index and the driver's `vkGetDeviceProcAddr` are
both per-driver facts, so two devices from the same driver look like they could
point at the same record.

They cannot. `vkGetDeviceProcAddr` is specified to return a pointer *specific to
the device it was asked about*. Two devices from one driver may legitimately be
given different function pointers for the same command — that is precisely how a
driver specialises a command for one device without a runtime test on every
call, which is the optimisation device-level dispatch exists to permit. A shared
record would be a place to cache those pointers under the wrong key: correct
today, because nothing is cached yet, and silently wrong the first time
something is.

So the record is per device and the sharing question never arises. The cost is
one small allocation per device, which is a per-device cost paid once against a
per-call cost paid forever.

This corrects a guess `gui/vulkan/src/lib.rs` had been carrying in prose since
the instance layer landed — it said a device dispatch table was needed "per
driver". The wrong version is left in that file, named as wrong, rather than
quietly deleted, because it is the version that sounds right.

### 3. Forward by default; two named exceptions

The loader's `vkGetDeviceProcAddr` answers for exactly two names —
`vkGetDeviceProcAddr` itself and `vkDestroyDevice` — and forwards everything else
to the driver, whose answer, including "no such command", is the answer.

There is deliberately **no list of device commands to accept**. Such a list would
be the commands known when the loader was written, and every extension a driver
supports that this loader has never heard of would be missing from it — the
application would get null for a command that exists. Forwarding by default is
what makes an unknown command work.

The two exceptions are exceptions because the loader has state riding on them:

| Command | Why the loader must own it |
|---|---|
| `vkGetDeviceProcAddr` | Handing back the driver's own would mean every later lookup made through it skipped the row below, and the loader would never hear about a device being destroyed. |
| `vkDestroyDevice` | Destroying a device also has to free the loader's record, which the driver knows nothing about. The driver's own, reached directly, would free the driver's object and leak the loader's — and leave a freed handle whose dispatch word still points at it. |

### The record holds what is used and nothing else

A loader's device dispatch table is conventionally an array of every device
command's function pointer, so the loader's exported `vkCmdDraw` can read a fixed
offset and jump. This loader exports no device commands — that is the point of
decision 1 — so such a table would be a few hundred slots that nothing reads: a
structure that looks like a working dispatch table right up until someone relies
on it, which is the same defect this tree has been filing bugs about elsewhere
(a tool reporting success for work it never did).

The record therefore holds two fields: which driver, and that driver's
`vkGetDeviceProcAddr` for this device. When the loader does start exporting
device commands, each one adds its pointer and its trampoline together, and the
field is read the moment it exists.

**Ordering consequence, worth stating because it is easy to get backwards.**
`vkDestroyDevice` must read the dispatch word *before* the driver destroys the
device, because that word lives inside the driver's object; and it must free the
loader's record *after*, because until the driver's call returns the device's
dispatch word still points at it. Creation has the mirror of this: the driver's
`vkGetDeviceProcAddr` is resolved *before* `vkCreateDevice` is called, so that a
driver which cannot supply one is refused while there is still nothing to unwind
— rather than after, leaving a live device to be destroyed through the very
lookup function that turned out to be missing.

**Where it is:** `gui/vulkan/src/device.rs` (the record, the lookup rule, and the
argument for all of the above); `create_device`, `get_device_proc_addr`,
`destroy_device`, `driver_destroy_device` and `record_of` in
`gui/vulkan/src/entry.rs`; `GetDeviceProcAddrFn`, `CreateDeviceFn` and
`DestroyDeviceFn` in `gui/vulkan/src/vk.rs`. `PhysicalDevice` gained an
`instance` field in `gui/vulkan/src/instance.rs` for a reason that falls out of
decision 1: `vkCreateDevice` is handed a physical device and nothing else, but
the driver's `vkCreateDevice` is an instance-level command and cannot be looked
up with a null handle, and the loader does not track its live instances, so
without that field it could not find the instance the device was enumerated
from.
