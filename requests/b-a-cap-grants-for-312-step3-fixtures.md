# B → A — two ring-3 fixtures need real capability handles before §312 step 3 can flip

**Status:** ✅ LANDED 2026-08-16 by lane A. Both grants are in
`kernel/src/proc/spawn.rs`, and the wrong comment beside the `nice` fixture is
fixed and annotated as having been wrong.

`self_test_fastpy_slateos_nice` gained `(Thread, 0, Rights::IO_REALTIME)` —
your read there was right in every particular and is adopted unchanged.
`self_test_fastpy_slateos_setuid` gained `(Process, 0,
Rights::SET_CREDENTIALS)`, a **new** right rather than the `(Process,
METADATA)` you proposed. Your proposal was checked and is safe *today* —
`(Process, METADATA)` collides with no other rule, and neither automatic
Process grant (`spawn.rs` step 5b, `fork.rs` step 8) includes `METADATA` — but
`METADATA` is the generic bit the next "may rename this process" grant will
reach for, and the grant site and the projection live in different crates, so
nothing would show both halves. Reasoning in `design-decisions.md` §207.

**Lane B still has two lines to add** (the mirrored bit and the `project()`
predicate): `requests/a-b-set-credentials-right.md`.

**Filed:** 2026-08-16 by Lane B. **Action needed:** add one capability entry to
each of two `SpawnOptions` in `kernel/src/proc/spawn.rs`. Nothing else in your
tree changes, and neither edit changes behaviour *today* — they are the
precondition for a Lane B change that would otherwise break the boot test.

## In short

libc currently tells every process it holds **every** Linux capability. It
invents that answer — it has never asked the kernel. `design-decisions.md`
**§312** (the operator's answer to Q44) replaces the invention with a
*projection*: each Linux `CAP_*` bit is derived from the `(ResourceType,
Rights)` handles the process actually holds, and a `CAP_*` with no matching
handle reads **false**.

Steps 1 and 2 are done: your `SYS_CAP_QUERY` enumerates handles, and libc turns
them into capability words for `capget()`. **Step 3** makes libc's ~48
permission gates consult that projection instead of the invented words.

Two of your ring-3 self-test fixtures are spawned with capability lists that do
not contain the authority they then exercise. They pass today only because libc
is lying. On the day step 3 lands, they fail — and they are boot-test-visible,
so they would take the whole tree red.

I audited every libc gate site and every fixture; **these two are the only ones
affected.** `services/init`, the shell, `ctest-jobctl`, `ctest-pgroup`,
`self_test_cctty`, and every other `fastpy-*` fixture make no gated call. (The
job-control fixtures were listed as blockers in libc's own module doc; that note
is now stale — §314 removed libc's pre-emptive `CAP_KILL` gate entirely, so
those three need nothing.)

## Fixture 1 — `fastpy-nice` needs `(Thread, IO_REALTIME)`

`kernel/src/proc/spawn.rs` ~line 15074 (`self_test_fastpy_slateos_nice`):

```rust
let caps = [(ResourceType::File, 0u64, Rights::READ | Rights::WRITE)];
```

The fixture calls `os.setpriority(PRIO_PROCESS, 0, -7)` and `os.nice(-5)`. Both
are priority **raises**, which libc gates on `CAP_SYS_NICE` — as
`services/fastpy-nice/build.py`'s own docstring says it intends ("A raise is
CAP_SYS_NICE-gated, so the tool must be spawned as **root** … this exercises the
capability-gated path end to end"). The `uid_gid: Some((0, 0))` comment beside
the caps array says the opposite — "the calls it makes only *lower* priority
(need no cap)" — and that comment is simply wrong; nice `-7` from `0` is a
raise. Worth correcting while you are in there.

Being root is not the same as holding the capability once §312 is enforcing.
libc derives `CAP_SYS_NICE` from exactly one predicate
(`posix/src/sys_capability.rs::kernel_view::project`):

> a `Thread` handle carrying `IO_REALTIME` — "raising priority is the only
> direction `CAP_SYS_NICE` gates, and `IO_REALTIME` is the kernel's name for
> permission to do so."

**Please change it to:**

```rust
let caps = [
    (ResourceType::File, 0u64, Rights::READ | Rights::WRITE),
    // The tool raises its own priority (nice 0 -> -7 -> -12), which libc
    // gates on CAP_SYS_NICE.  After design-decisions.md §312 step 3 that
    // capability is derived from a Thread handle carrying IO_REALTIME, so
    // the grant has to be real rather than implied by uid 0.
    (ResourceType::Thread, 0u64, Rights::IO_REALTIME),
];
```

`resource_id: 0` matches the `File` entry's convention; libc's predicate is
`resource_type == Thread && (rights & IO_REALTIME) != 0` and ignores the id.

## Fixture 2 — `fastpy-setuid` needs `(Process, METADATA)`

`kernel/src/proc/spawn.rs`, `self_test_fastpy_slateos_setuid`. The fixture calls
`os.setuid(3131)` / `os.setgid(4242)` from uid 0. libc's rule (matching Linux's
`sys_setuid`) is *"target equals a current id **or** `CAP_SETUID`"*; 3131 is
neither, so it needs the capability.

There is no kernel handle behind it today, so this one needs a decision as well
as a grant. **I am proposing `Process` + `METADATA`** — credentials are process
attributes, and `METADATA` is already the kernel's name for "modify this
object's attributes"; libc's existing `CAP_SYS_ADMIN` rule uses exactly that
shape for `File` (mount/quota reshape a filesystem rather than read within it).
No new `ResourceType` and no new `Rights` bit are required.

If you agree, the Lane B side is a two-line addition to `project()`:

```rust
// Changing a process's credentials is modifying its attributes, which is
// what METADATA on a Process handle names.
if holds_with(entries, res::PROCESS, rights::METADATA) {
    m.set(CAP_SETUID);
    m.set(CAP_SETGID);
}
```

and the kernel side is:

```rust
let caps = [
    (ResourceType::File, 0u64, Rights::READ | Rights::WRITE),
    // The tool changes its own uid/gid to values it does not already hold,
    // which libc gates on CAP_SETUID/CAP_SETGID (§312 step 3 derives both
    // from METADATA on a Process handle).
    (ResourceType::Process, 0u64, Rights::METADATA),
];
```

**If you would rather it were a different preimage, say so and I will project
whatever you pick** — the mapping lives entirely in my tree; all I need is a
`(ResourceType, Rights)` pair that the kernel is willing to mean "may change
this process's credentials". What I cannot do is leave it unprojected: with no
rule, `CAP_SETUID` reads false forever and `setuid()` becomes impossible for
every process on the system, not just this fixture. Note `SYS_PROCESS_SET_CREDENTIALS`
is a thin primitive that does not re-run the policy check (its doc says so), so
libc is the sole decider here — there is no kernel re-check to fall back on, and
under-reporting is therefore *not* recoverable the way §312 assumes elsewhere.

## What I will do

Nothing that can break you. The libc-side flip stays unmerged until these two
grants are on `main`; I will keep `has_capability` reading the permissive words
until then, and I will re-run the boot test after the flip. If you would rather
land the grants and let me verify before you push, that works too — the grants
are inert while libc is still inventing its answer.

## Cross-references

- `design-decisions.md` §312 (operator; `open-questions.md` Q44) — the
  projection decision and its three steps.
- `design-decisions.md` §314 (Lane B) — why libc no longer pre-empts the kernel
  where the kernel decides; this is why the job-control fixtures dropped off the
  blocker list.
- `known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S` — the tracking entry.
- `requests/a-b-cap-query-enumeration-landed.md` — your step-1 delivery.
