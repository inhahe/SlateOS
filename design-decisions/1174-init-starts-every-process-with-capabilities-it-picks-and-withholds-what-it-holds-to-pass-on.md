## 1174. Init starts every process with capabilities it picks: its class-wide grants, less the types it holds only to pass on, plus the service's `caps:`

**Date:** 2026-10-05
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a service listed in `/etc/startup.conf` can now ask for
permissions beyond the usual ones with `caps:`. The compositor's line will be
`caps:InputDevice/0/r,Service/0/w`: read the keyboard and mouse, and offer the
display service. The kernel lets a process hand on only a permission it holds
itself, so for the compositor to get the keyboard, init must hold it. Until
now every process init started got a copy of everything init held. Had init
held the keyboard, every service and every program started from init's console
would have held it too, and could have read every keystroke. So init no longer
starts anything that way. Each process gets init's general permissions minus
the ones init holds only to pass on, and a service gets one of those only when
its line names it. Nothing changes on today's image: init holds none of the
pass-on-only kinds yet. Lane A is asked to grant them.

| Option | What it means |
|---|---|
| **A. Init holds them and withholds them unless named** (chosen) | only the compositor reads the keyboard; nothing else init starts can |
| B. Keep 706's "no grant to init" | `caps:InputDevice` can never work: the kernel delegates only what the parent holds |
| C. Init holds them and keeps the old spawn | every process reads every keystroke: 706's reason for refusing the grant |

**Why A.** 706 refused to grant `InputDevice` to init because init's table
went wholesale to everything under it: the spawn call init used copies it, and
nothing can give a capability up. That is a fact about one spawn call, not
about init. `SYS_PROCESS_SPAWN_EX2` (559) makes the parent name each
capability the child gets, and init now starts every process through it --
services, and programs run from its console. Lane A's delegation rule settles
where the grant must sit: a child receives a capability only from a parent
holding that same id (`requests/a-b-the-two-fields-you-want-shipped-on-2026-08-22-as-spawn-ex2.md`).
The compositor's parent is init, so init must hold it. 706's other point
stands: the grant goes on the service's line, not on everything.

**What a process gets** (`services/init/src/lib.rs`, `child_caps`):
- init's grants on a whole class (`resource_id` 0), except the types
  `caps:` can name (`DELEGATED_TYPES`: `InputDevice`, `Service`);
- then what the service's `caps:` names. A name init does not hold makes the
  kernel refuse the spawn, and init says so on the console.

It no longer gets init's grants on single objects, above all the `Process`
capability the kernel gives init over each child. Those are init's handles on
its own children. With the old spawn, the second service got init's handle on
the first. Each service still gets the `Process` class grant, so this narrows
little today; narrowing each service to what it uses is a later change, made
through the same list.

**What changes today:** nothing a service can see. `/bin/ticker`, the only
entry, gets the same six capabilities: the kernel's log line for it now reads
"Delegated 6 of parent N's capability(ies)" where it read "Inherited 6".

**Revisit if:** init ever forks (a fork copies the whole table, withheld kinds
included); a `SYS_CAP_DROP` arrives; or a type that `caps:` must name is also
one every service needs. Then `DELEGATED_TYPES` would need to split into
"named only" and "nameable".
