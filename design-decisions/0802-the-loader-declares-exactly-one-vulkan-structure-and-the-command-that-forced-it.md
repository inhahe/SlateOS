## 802. The loader declares exactly one Vulkan structure, and the command that forced it is the one it cannot forward

**Date:** 2026-09-02
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Three Vulkan commands can be called before a program has anything
to call them on: "which version of Vulkan is this?", "which optional add-ons
are available?" and "which debugging layers are installed?" Our loader did not
export any of them, deliberately — §578 argued that a command answering "none"
without having looked is worse than a missing symbol, because the missing
symbol names itself and the false "none" gets reported as a driver bug. That
argument expires the moment an honest answer exists, and it now does for all
three. Getting there required breaking a rule the loader had held since it was
written: it now declares the memory layout of exactly one Vulkan data
structure, where before it declared none. This entry is about why that one is
different, and about two policy choices with no specification behind them.

Governs `gui/vulkan/src/global.rs`, the structure declaration in
`gui/vulkan/src/vk.rs`, and the three exported commands in
`gui/vulkan/src/entry.rs`.

### 1. Why these three could not simply be forwarded

Every other command this loader exports has a driver to hand it to. These do
not: they are asked with a **null instance**, before any driver has been
selected, which is the whole reason they exist — an application uses them to
decide what to put in the `vkCreateInstance` it has not made yet.

So the loader must *answer* rather than forward, and each answer is a decision:

| Command | The loader's answer | Where it comes from |
|---|---|---|
| `vkEnumerateInstanceVersion` | Vulkan 1.0 | What this loader implements — not what any driver implements, which is a different question asked of a `VkPhysicalDevice`. |
| `vkEnumerateInstanceLayerProperties` | an empty list | There is no mechanism by which a layer could exist: loading one needs `dlopen`, which returns null on SlateOS. |
| `vkEnumerateInstanceExtensionProperties` | the de-duplicated union of every registered driver's list | Every driver is asked, because any of them might serve the extension later. |

The middle row is the one that looks like the stub §578 refused to write, and
the difference is worth stating because it is easy to lose: **an empty list
computed from a real registry is a correct answer; an empty list returned
without looking is a lie that happens to be short.** What changed is not the
answer but that the answer now has a reason attached, and the reason is a fact
about the machine rather than about the loader's completeness.

The version is the same shape of argument in reverse. Reporting 1.1 would be
the *generous* lie: an application seeing 1.1 is entitled to ask for
`vkGetPhysicalDeviceProperties2` and would get null.

### 2. The invariant that broke, and why it broke here and nowhere else

`vk.rs` has said since §801 that **every Vulkan structure is `*const c_void` or
`*mut c_void`, and none is declared** — with the asymmetry as the argument: a
wrong function signature means a wrong argument count, which is a compile error
or an immediate crash, while a wrong structure layout is caught by nothing,
because the caller writes field `A`, the driver reads field `B`, and out comes
a plausible wrong answer. That section also named the moment to revisit it:
*when a command needs the loader to look inside a structure.*

`vkEnumerateInstanceExtensionProperties` is that command, and it is not a near
miss. Its answer is a union across drivers, a union must be de-duplicated, and
de-duplication compares extension **names** — so the loader has to know where
the name is. Comparing whole records instead does not work: two drivers
reporting the same extension at different `specVersion`s produce different
bytes and must still collapse to one entry. There is no forwarding to fall back
on, because there is nothing to forward to.

`VkExtensionProperties` is therefore declared, and these are the properties
that make it a tolerable exception rather than the first of many:

- **Two fields**, one a fixed-size byte array whose length is itself part of the
  ABI (`VK_MAX_EXTENSION_NAME_SIZE`, 256). No `pNext`, no version-dependent
  tail, no enumeration whose width could be wrong.
- **Checked, not trusted.** `const _: () = assert!(size_of == 260)` and the
  matching alignment assertion make a layout mistake a build failure rather
  than a driver reading the wrong field at runtime. This is exactly the check
  the original argument said a structure declaration cannot have — and it can,
  for a structure this simple, which is the whole of why this one is allowed.
- **The name is declared `[u8; 256]`, not `[c_char; 256]`.** Layout-identical,
  and it removes a sign question that has no bearing on anything: the loader
  compares these bytes, it does not interpret them as characters. It is also
  what the tree's rule about OS-boundary data already says.

**What was rejected.** "Three loose constants — offset 0, length 256, stride
260 — rather than a struct" is the same declaration wearing a disguise, and a
worse one, because the compiler checks a `#[repr(C)]` layout and does not check
three integers. And "do not export the command" leaves an application unable to
discover any instance extension, which is the state the WSI work will walk
straight into.

The rule the invariant becomes is not "never declare one" but **"declare one
only when the loader must read a field, and argue it at the point of use."**

### 3. Union, not intersection

An extension one driver offers is reported even when another does not.

Intersecting would let a driver the application was never going to use veto a
capability its actual GPU has — a machine with a discrete card and a software
rasteriser would report whatever the rasteriser lacks as unavailable. That is
the same failure §578's "a partial success is a success" rule exists to avoid,
and the consistency is not decorative: because one driver declining does not
sink `vkCreateInstance`, an application that enables an extension only some
drivers have still gets an instance.

### 4. The lowest `specVersion`, not the highest

When two drivers offer the same extension at different versions, the reported
version is the **minimum among the drivers that have it** — a driver that does
not report the extension at all contributes nothing rather than counting as
zero, which would collapse every version to 0 the moment one driver was silent.

The argument is not which number is more accurate; both are true of some
driver. It is which way the mistake falls:

| If the loader reports | And the app is served by a weaker driver | Cost |
|---|---|---|
| the minimum | it skips a feature that was in fact available | a missed optimisation |
| the maximum | it calls an entry point that is not there, **having asked first and been told yes** | a crash after doing everything right |

Under-promising fails safe and over-promising does not, and that settles it.
The Loader–Driver Interface says nothing about either this or §3, so both are
recorded as policy — the same distinction `instance.rs` draws, and blurring it
is how a plausible invention becomes a citation later.

### 5. Ordering is first-seen, not sorted

The list is small and an application looks names up rather than scanning, so
sorting would buy nothing. First-seen order keeps the answer stable between
runs and makes a log legible: the extensions appear in registration order, so
which driver contributed what can be read off directly.

### The shape to remember

§578 wrote down a reason for *not* building something, and the reason had a
built-in expiry date that was not written next to it: "there is no honest
answer yet" stops being true the moment there is one, and nothing in the tree
was watching for that moment. The omission outlived its justification by two
days — not long, but the mechanism does not have a length limit. A deliberate
gap should carry the condition that closes it, or it becomes a permanent one
defended by an argument that no longer applies.

**Where it is:** `gui/vulkan/src/global.rs` (the three answers, the merge
policy and both of its arguments); `ExtensionProperties`,
`MAX_EXTENSION_NAME_SIZE` and the layout assertions in `gui/vulkan/src/vk.rs`,
along with the rewritten invariant section; `enumerate_instance_version`,
`enumerate_instance_layer_properties` and
`enumerate_instance_extension_properties` in `gui/vulkan/src/entry.rs`.
