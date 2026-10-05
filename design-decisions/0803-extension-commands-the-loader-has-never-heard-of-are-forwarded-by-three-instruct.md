## 803. Extension commands the loader has never heard of are forwarded by three instructions of assembly, from a fixed pool of 128 slots that is never recycled

**Date:** 2026-09-02
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A graphics driver can offer optional add-ons ("extensions") with
extra functions in them. Our Vulkan loader sits between a program and the
drivers, and §802 made it honestly report every add-on any driver offers. That
immediately created a lie next to the truth: the program is told the add-on
exists, asks the loader "where is its function?", and the loader answers
*nowhere* — because it only knows how to hand over functions it was written to
know about, and an add-on's functions are by definition ones nobody told it
about. The fix is a piece of hand-written machine code that forwards a call to
the driver **without knowing what the call looks like**: it swaps one pointer
and jumps, leaving every other argument untouched wherever the caller left it.
The decisions recorded here are that this is done at all, that there is a fixed
supply of 128 such forwarders, that a forwarder once given to a function name
keeps it for the life of the program, and that a driver too old to be asked the
one question that makes this safe is simply not asked.

### Why a signature is not needed

The nine core commands that take a `VkPhysicalDevice` are each named in
`physical.rs` and each get a hand-written Rust trampoline with the right
argument list (§801). That is unavailable here: there is no argument list to
write, because the loader does not know the extension exists.

What makes it work anyway is a property of the calling conventions rather than
of Vulkan. Both conventions the loader targets pass the first pointer argument
in one register — `rdi` on System V, `rcx` on Windows x64 — and put every other
argument somewhere that does not depend on what argument 0 is. So:

```text
    mov rax, [arg0 + EXT_OFFSET]     ; the driver's table of extension entry points
    mov arg0, [arg0 + HANDLE_OFFSET] ; the driver's own VkPhysicalDevice
    jmp [rax + 8*SLOT]               ; tail call: control never comes back here
```

is correct for **every signature at once**. The stack is not touched, so stack
arguments, the return address and Windows' shadow space stay exactly as the
caller built them; the floating-point argument registers are not touched; and
because it jumps rather than calls, the driver returns straight to the
application and the return value needs no handling either. `rax` is the only
register clobbered, and it is caller-saved in both conventions and an argument
register in neither.

The order of the two loads is load-bearing: the table pointer must be read
*before* argument 0 is overwritten, because both words live in the object
argument 0 points at.

The one signature shape this would break on is a function returning a large
struct by value, where the ABI inserts a hidden result pointer as argument 0 and
shifts everything along. No Vulkan command does that — every output is written
through a caller-supplied pointer and every return value is a `VkResult`, a
handle, or nothing — so the exception is recorded rather than guarded against.

This cannot be expressed in Rust: there is no guaranteed tail call, and no way
to name "the arguments I was not told about". It is `#[unsafe(naked)]` plus
`naked_asm!`, and it is what the Khronos loader does for the same reason
(`unknown_ext_chain_*.asm`). It is also the reason
`vk_icdGetPhysicalDeviceProcAddr` exists in the Loader–Driver Interface at all.

**Consequence, accepted:** the loader is now architecture-specific. A
`compile_error!` on any target that is not x86-64 says so at the point of the
assembly, with what a port would have to write. A portable fallback was
considered and rejected as impossible rather than expensive: forwarding
arguments the compiler was never told about is exactly the thing a
high-level language cannot do.

### Why a fixed pool, and why a slot is never taken back

A forwarder must be a distinct *address* per command name, because the address
is all the application keeps — there is nowhere in the call to put a "which
command is this?" parameter. Distinct addresses that share one behaviour can
come from only two places: code generated at run time, or code generated once
per slot at compile time. Run-time code generation needs an executable
allocation the OS does not yet offer and would be a far larger surface; so the
pool is 128 entries of `tramp::<N>`, monomorphised at compile time.

| | Fixed pool of 128 | Recycle the least-recently-used slot | Generate code at run time |
|---|---|---|---|
| *What changes:* | the 129th distinct extension command is reported missing | the 129th works, and some earlier one silently becomes it | any number work |
| Cost | a hard ceiling | **a wrong function called by a correct program** | W^X mapping, a code cache, and an allocator the OS lacks |

Recycling was rejected outright, not traded off. Vulkan explicitly permits an
application to resolve a command once at startup and call the pointer for the
life of the process; a slot that were ever reassigned would turn one command
into another in a program that did nothing wrong, with no diagnostic anywhere.
Exhaustion instead returns null — which is the C API's own way of saying "no
such entry point", the answer the application already has to handle, and the
answer it was getting for *every* extension command until this change.

128 is a ceiling on *distinct extension commands that take a
`VkPhysicalDevice`*, not on extensions: the whole of WSI (`VK_KHR_surface`,
each platform surface extension, `VK_KHR_swapchain`) contributes a
single-figure number of them. Raising it is a one-line change to `SLOTS` and
the macro call beneath it.

### An unclaimed slot aborts rather than holding null

A machine can have three drivers where only one implements an extension. The
table is per driver, so the other two have an entry for that slot with nothing
to put in it. Leaving it null would make an application's mistake — calling an
extension command on a device whose driver lacks it, without checking first —
into a jump through address zero. Leaving it holding a *neighbour's* pointer
would be worse: another driver's code called with this driver's handle.

It holds a stub that panics with a sentence naming what happened. The
application error is the same error either way; what differs is entirely how
the resulting bug report reads, and the cost of the better one is one word per
slot.

### The version-4 gate is a memory-safety rule, not a preference

**Only a name a driver answered through `vk_icdGetPhysicalDeviceProcAddr` may be
given a slot.** `physical::Ask` legitimately falls back to the driver's
`vkGetInstanceProcAddr` for the nine *known* commands (§801). Doing the same
here would be memory corruption.

The reason is that `vkGetInstanceProcAddr` answers for *device-level* commands
too, and its answer does not say which kind it gave. An extension may define
the same name at device level; a device command reached through a trampoline
would have argument 0 — a `VkDevice` the loader hands back untouched, and which
is the *driver's* object, not a loader wrapper — read as if it had the
wrapper's layout, and two words pulled out of the middle of it and jumped
through. The version-4 entry point exists precisely to answer "is this a
physical-device command?" and nothing else.

So a driver that settled below interface version 4 contributes nothing to this
path, even when it does export the symbol — `Driver::physical_device_proc_addr`
gates on **entitlement, not on the pointer being non-null** (§577). That is a
real limitation, and it is the right one: such a driver cannot be asked the
question safely, so it is not asked, and its extension commands stay
unreachable rather than becoming unsafe. There is a dedicated test for it,
because the failure it prevents is silent.

### Resolution is redone on every lookup, and a slot is spent only after a yes

Every `vkGetInstanceProcAddr` for an unknown name re-asks **all** the
instance's drivers and rewrites their table entries. Caching the fact that a
name had been resolved would leave a driver registered later reachable through
a slot whose entry nobody ever filled. Re-asking costs a handful of
`GetProcAddr` calls on a path an application walks at startup, which is not a
hot path in any application that exists.

The order within one lookup matters too: every driver is asked *first*, and the
slot is assigned only if at least one said yes. A name nobody implements
therefore costs nothing permanent — which is what stops an application probing
for extensions it does not have from exhausting a pool that is never reclaimed.

### Where the two pieces of state live

- The **name-to-slot map** is on `Registry`, not a second `static` in `entry.rs`.
  It is written in the same breath as the per-driver tables and under the same
  lock; two locks over two halves of one operation is a deadlock waiting for an
  ordering mistake.
- The **per-driver table** is a `Box` on `Driver`. Its *address* is copied into
  every physical-device wrapper that driver produced, and `Driver` itself lives
  in a `Vec` that moves when the next driver registers. A box's contents do not
  move when the box does; that is the entire reason for the indirection.

Entries are `AtomicUsize` for the *writing* side. The reading side is a `mov`
inside a naked function, outside Rust's memory model entirely. What makes that
sound is that x86-64 does not reorder a load ahead of a prior store from the
same thread, that a table entry is only ever written while the registry lock is
held, and that the application obtains the trampoline's address only after that
lock is released. The atomics keep the writing side defined and record the
intent.

### The offsets the assembly indexes with are derived, not written twice

`PhysicalDevice::HANDLE_OFFSET` and `EXT_OFFSET` are `offset_of!` constants
consumed by `naked_asm!`'s `const` operands. The alternative — two literals in
the assembly — is a hand-maintained copy of a struct layout, and reordering the
fields would not fail to compile; it would forward calls through the wrong two
words. Deriving them means the layout and the code that indexes it move
together.

### The shape to remember

The crash that came out of this change was not in any of it. It was a test
fixture, `an_instance()`, that returned the fabricated address `0x1234_5678`
with a comment justifying it: `vkGetInstanceProcAddr` never dereferences the
instance handle. That was true when it was written, and the new fallback —
whose entire job is to ask the instance's drivers a question — made it false.
The whole test binary died with an access violation in a test at the far end of
the file from the change.

**A fixture justified by "nobody looks inside this" stops being a fixture the
day somebody looks**, and the failure surfaces nowhere near the edit. The
justification is the thing that ages, not the value.

**Where it is:** `gui/vulkan/src/unknown.rs` (the whole module: `Slots`,
`Table`, `unresolved`, `tramp`, the 128-entry `POOL`); `unknown_across` and
`unknown_physical_device_command` in `gui/vulkan/src/entry.rs`, plus the new
tail of the instance-level match in `get_instance_proc_addr`; the `ext` field,
`HANDLE_OFFSET` and `EXT_OFFSET` on `PhysicalDevice` in
`gui/vulkan/src/instance.rs`; `Driver::ext` and `Registry::slot_for` in
`gui/vulkan/src/registry.rs`.
