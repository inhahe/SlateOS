## 577. The Vulkan loader's two ABI rules are encoded as types that refuse the wrong answer, not as integers the caller compares

**Date:** 2026-09-02

**Lane:** C

**Decided by:** Claude (autonomous)

**In short:** A Vulkan loader is the library an application calls instead of
calling a graphics driver directly; it finds the drivers on the machine and
stands between them and the app. Two of the rules it has to obey are the kind
where getting it wrong corrupts memory rather than producing an error message.
The decision is to express both rules as small types whose method names *are*
the rule — you ask "may this driver manage surfaces?" rather than comparing a
number against 3 — and to give the dangerous one no unchecked escape hatch at
all, so that the mistake cannot be written.

**The two rules.** Khronos's Loader–Driver Interface says (a) the loader and
each driver agree on an *interface version* between 0 and 7, and each version
number entitles either side to assume specific things; and (b) the handles
Vulkan hands out for instances, devices, queues and command buffers are
pointers whose **first machine word** is reserved for a dispatch-table
pointer. The driver stamps a magic value (`0x01CDC0DE`) into that word; the
loader checks the magic and then overwrites the word with its own table
pointer.

**Decision 1 — `InterfaceVersion` has no public integer.** The version is a
newtype with a private `u32` and seven predicates:
`exports_icd_get_instance_proc_addr` (≥1), `negotiates` (≥2),
`may_manage_surfaces` (≥3), `has_physical_device_proc_addr` (≥4),
`loader_validates_api_version` (≥5), `enumerates_adapters` (≥6),
`entry_points_may_be_unexported` (≥7).

*Alternative:* a plain `u32`, compared at each site — which is what the
reference loader does.

*For the newtype:* the boundary is written down once per capability instead
of at every call site, so a `>` where `>=` was meant is a single-line fix in a
single place rather than a hunt. The names also carry the knowledge: I had
memorised `vk_icdGetPhysicalDeviceProcAddr` as arriving at version 3, and the
spec says 4 — a plain `if v >= 3` at a call site would have shipped that
error silently, whereas `has_physical_device_proc_addr()` is a claim the test
`each_capability_appears_at_the_version_the_specification_says` can check on
both sides of every boundary. (That test exists specifically because of this
near-miss, and says so.)

*Against:* an extra layer between the loader and a number that is, after all,
just a number; and a caller who genuinely needs the integer — to log it, or
to pass it back over FFI — has to call `.get()`.

**Decision 2 — the dispatch word can only be written through a checked
function.** `dispatch::adopt` reads the word, returns
`Err(NotDispatchable { found })` if the magic is absent, and writes only
otherwise. There is deliberately **no `set_dispatch_unchecked`**.

*Alternative:* provide both, and use the unchecked one on the paths where the
object provably came from a driver that already stamped it.

*For check-only:* the failure mode of the unchecked version is not a wrong
value, it is a write of a pointer into the first eight bytes of whatever the
object actually is — usually a driver's private struct — which surfaces
arbitrarily far away, inside a stripped third-party shared library, as a
crash with no relationship to the loader. Every loader bug of this shape in
the wild is a *missing* check rather than a wrongly-failing one, so the
asymmetry is real: the cost of the check is a load and a compare on a path
that already performed a store, and the cost of skipping it is unbounded.
There is no hot loop here — handles are stamped at creation, not per frame.

*Against:* it forces an `unwrap`-shaped decision on callers who know the
magic is present, and it makes the loader marginally slower at handle
creation.

**Decision 3 — policy is separated from FFI.** `icd::settle` takes a
`DriverReply` value and returns a `Negotiation` value; it never calls a
function pointer. The actual call into a driver's
`vk_icdNegotiateLoaderICDInterfaceVersion` is a thin `unsafe` wrapper that
produces a `DriverReply`. This is what lets the clamping rules be tested at
all: `settle` is exercised with a driver that reports 9 (clamped to the
loader's offer), one that returns `VK_ERROR_INCOMPATIBLE_DRIVER` (skipped,
*not* surfaced to the application), and one that fails with some other code
(kept distinguishable from incompatibility). None of those cases can be
produced by a real driver on demand.

**Decision 4 — a driver's entry points are never handed out ungated.**
`registry::Driver` stores the `Entry` it was given but does not expose it.
Callers get `instance_proc_addr()` and `physical_device_proc_addr()`, which
consult the settled version first: a driver that *offered*
`vk_icdGetInstanceProcAddr` but settled at 0 is still called through the plain
one, because a version-0 driver exporting that name is a coincidence of
spelling rather than the version-1 contract, and the loader has no promise
about what it does.

*Alternative:* expose the `Entry` and let each call site check the version.

*For gating at the accessor:* this is the same argument as decision 1 one
level up. The check is the sort that is correct in every place it appears and
catastrophic in the one place it is forgotten, and forgetting it is a jump
through a pointer the driver never exported. A call site that cannot obtain
the ungated pointer cannot forget. The cost is that a future caller with a
legitimate reason to inspect the raw offer — a diagnostic dump, say — has to
have an accessor added for it, which is a good moment to ask why.

**A related note on fixtures.** The stub `GetProcAddr`s in the registry's
tests first answered unconditionally, and clippy's `unnecessary_wraps` flagged
the resulting always-`Some` return. Its suggested fix — drop the `Option` —
would have put the stub out of ABI agreement with `GetInstanceProcAddrFn`,
which is what makes it tempting to read the lint as a false positive and
silence it. It was not: a `GetProcAddr` that cannot fail does not model the
null return that is the whole reason `VoidFn` is an `Option`, so the lint was
describing a defect in the fixture. The stubs now match a name and answer null
otherwise. Recorded because "the lint's suggested fix is wrong" and "the lint
is wrong" look identical at the call site and are not the same thing.

**Where it lives:** `gui/vulkan/` — crate `vkloader`, modules `icd`,
`dispatch`, `registry` and `vk`. 70 tests across the crate. *(At the time this
entry was written no Vulkan entry points were exported at all, on purpose: a
loader that exports `vkCreateInstance` before it can dispatch it is a tool
that reports success for work it never did, which is the defect lane C spent
this same day filing against `userspace/` — see
`requests/c-b-2288-userspace-tools-report-success-for-work-they-never-did.md`.
Later that day the instance-level commands became dispatchable and were
exported; the same reasoning still keeps the three `vkEnumerateInstance*`
commands out of the library entirely. See §578, decision 4.)*

**Why drivers are registered statically rather than discovered.** A
conventional loader finds drivers by reading JSON manifests and `dlopen`ing
the library each names. `posix/src/dlfcn.rs`'s `dlopen` is a stub returning
null with the error `"dynamic linking not supported"`, so a loader built
around it would find nothing on every machine while looking like it worked.
Drivers therefore hand the loader their entry points directly. This is not a
placeholder: discovery answers only *which* drivers exist, and everything
after that — negotiating, stamping, dispatching — is identical either way,
which is why it can be built and tested now. When dynamic loading lands, the
addition is a discovery step producing the same registration records, plus a
manifest parser that belongs in `textfmt` rather than here (`apps/jsonviewer`
already carries a private JSON parser, and a second private copy is the
duplication `textfmt` exists to prevent).
