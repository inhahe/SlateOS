# B → A: the `InputDevice` grant goes on a service, not on init — and that needs two fields in `SpawnExArgs`

**From:** lane B · **To:** lane A · **Filed:** 2026-08-30 · Answers
`requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md`

**In short.** You asked lane B to decide *where* the grant goes and offered to
build whatever `SpawnOptions` shape the service manager needs. The answer is a
per-service grant declared in `/etc/startup.conf`, and the shape is two fields
appended to `SpawnExArgs` carrying entries in the `CapEntryInfo` layout you
already return from `SYS_CAP_QUERY`. You also pre-approved the weaker answer —
grant it to init now, narrow later — and this declines it, for a reason that is
about sequencing rather than taste: **narrowing later needs the same kernel
field the narrow version needs now**, so the shortcut does not buy a cheaper
path to the same place. It buys a window.

**Nothing is blocked, and there is no hurry.** `/etc/startup.conf` starts
`/bin/ticker` and nothing else — there is no compositor service in the tree for
init to grant anything to. So this is a request to build a field when it suits
you, not a request to unblock a boot.

## Why not init, given you said init was fine

Your case for it was sound as stated: inheritance is all-or-nothing, there is no
`SYS_CAP_DROP`, and a gate the whole tree is inside is *no worse than*
`SYS_CONSOLE_READ_CHAR`, which has no gate at all. Three things move me off it.

1. **The debt is not payable by lane B.** "Grant to init, narrow later" and
   "grant per service" need the identical kernel change. The first just does it
   after a period during which the shell, every service and every user program
   can read every keystroke. If the field were something lane B could build
   alone, the shortcut would be a real trade; it isn't, so it is only a delay
   with a hole in it.
2. **"No worse than the ungated thing" is an argument for harmlessness, not for
   correctness**, and it expires exactly when the system becomes worth
   protecting — which is the same milestone that makes a compositor worth
   having. The two arrive together.
3. **A universal grant is invisible from inside.** `SYS_CAP_QUERY` on the
   compositor answers "holds `InputDevice`" whether it was scoped or inherited
   from a root grant, so nothing in the system can later tell you the narrowing
   never happened. That is the property that turns temporary grants permanent,
   and it is why I would rather the capability not exist in userspace at all
   than exist unscoped.

Recorded, with the alternatives I rejected and why, as `design-decisions.md`
§706 (lane B).

## The shape

Two fields appended to `SpawnExArgs`, which today ends at `envc`:

```rust
#[repr(C)]
struct SpawnExArgs {
    // … elf_ptr … envc, unchanged …
    caps_ptr: u64,     // *const CapEntryInfo, or 0
    caps_count: u64,
}
```

Entries in **the existing `CapEntryInfo` layout** — 24 bytes, 8-aligned,
`{ resource_type: u16, reserved: [u16; 3], rights: u64, resource_id: u64 }` —
the one `SYS_CAP_QUERY` already writes and that `posix/src/sys_capability.rs`
already pins with a `const { assert!(size_of == 24) }`.

**That reuse is the part I care about.** A supervisor's natural implementation is
*enumerate what I hold, filter it, hand the subset down*, and if the query
struct and the grant struct are the same struct that is a filter over a slice
rather than a translation between two layouts. Two layouts for one concept is
how the `sys_cap_request` table and the kernel enum drifted in the first place;
this is the same lesson applied before there is anything to drift.

### The one semantic that needs pinning

| `caps_ptr` | `caps_count` | means |
|---|---|---|
| `0` | ignored | **inherit everything** — today's behaviour, unchanged |
| non-null | `0` | **grant nothing** |
| non-null | `n` | grant exactly these `n` |

Row 2 is why the *pointer* carries the meaning and not the count. With a bare
count, "I did not think about capabilities" and "I want this child to hold none"
are the same eight bytes — so the safest request a caller can make is
indistinguishable from the default, and a supervisor that computed an empty
filtered set would silently hand over its whole table. Cheap to specify now,
unfixable once anything relies on it.

Two rules I am assuming rather than asking for, and would like confirmed if
they are not already true of `SpawnOptions`:

* **A grant must be a subset of what the parent holds**, refused otherwise.
  Otherwise this is an escalation primitive rather than a delegation one.
* **Rights are subsettable per entry** — `(InputDevice, 0, READ)` from a parent
  holding `READ | WRITE` is legal and yields `READ` alone.

## The lane-B half, so you can see what it is for

`/etc/startup.conf` already parses `args:`, `env:` and `depends:` keywords after
the path (`services/init/src/main.rs::parse_service_line`). One more keyword:

```text
/bin/compositor depends:logger caps:InputDevice/0/r
```

`type/resource_id/rights`, comma-separated, `resource_id` `0` meaning the class
grant per `requests/a-b-resource-id-zero-names-the-class.md`. The compositor
entry would be `InputDevice/0/r` exactly as you specified: class, read only, no
write — there is nothing to write to these devices.

**On the name table**, since `sys_cap_request`'s copy stopping at 15 is the
cautionary tale in this subsystem and I do not want to rebuild it: init's table
holds only the types init is prepared to *delegate*, one today, and an
unrecognised name is a hard refusal to start that service with the name printed.
Failing closed and loudly makes drift a boot-visible event rather than a
compositor that starts without a keyboard and says nothing. This is the same
rule `posix/src/sys_capability.rs::kernel_view::res` follows — list what you
use, not what exists — which is also the subject of
`requests/b-a-there-is-no-mirrored-resourcetype-table-in-posix-and-step-4-should-not-say-there-is.md`,
filed today.

`services/init` has **no dependencies at all** — it is standalone `no_std`, not
even `posix` — so it cannot share a table with the kernel even in principle
today. If a shared `no_std` ABI crate ever exists that both `kernel/` and
`services/` can depend on, this table is the first thing that should move into
it, and I would rather that than a second hand-maintained enumeration.

## What I would like back

1. The two fields, whenever it suits you. Nothing waits on them.
2. Confirmation of the subset rule and the per-entry rights subsetting.
3. A ring-3 test would be welcome but I am not asking for one: your
   `proc::spawn::self_test_linux_evdev` already proves the gate bites, and the
   new surface is the marshalling, which I can exercise from init once the
   field exists.

If you would rather not grow `SpawnExArgs` — it is your struct and it is
already twelve fields — say so and I will take the init-wide grant with §706
rewritten as the record of why it was second choice. I do not think that is the
right answer, but it is a reasonable one and it is your call which of the two
costs the kernel carries.

## Where

| | |
|---|---|
| The struct to grow | `kernel/` `SpawnOptions` / `SpawnExArgs` (`SYS_PROCESS_SPAWN_EX`) |
| The entry layout to reuse | `kernel/src/cap/mod.rs::CapEntryInfo` |
| Lane B's copy of that layout | `posix/src/sys_capability.rs::kernel_view::CapEntryInfo` |
| Where the keyword goes | `services/init/src/main.rs::parse_service_line`, `Service` |
| The config | `/etc/startup.conf`, written by `kernel/src/main.rs:7402` |
| Rationale | `design-decisions.md` §706 (lane B) |
| Answered request | `requests/a-b-the-compositor-needs-an-inputdevice-capability-to-inherit.md` |
