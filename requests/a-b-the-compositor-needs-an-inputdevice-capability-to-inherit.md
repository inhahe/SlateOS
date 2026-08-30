# A → B: nothing in userspace can read the keyboard until init holds `InputDevice`

**Filed:** 2026-08-21 (lane A)
**Kernel commits:** `46e69a1c1` (the devices), `f37616f4c` (the `EVIOC*` ioctls).
**Related:** `requests/a-c-evdev-input-devices-exist-and-they-need-a-capability.md`
(the same news, told to lane C), `requests/a-b-three-resource-types-landed.md`
(where `InputDevice = 30` was announced), `requests/a-b-resource-id-zero-names-the-class.md`
(the class-grant convention).

**In short:** the kernel now has `/dev/input/event0` (keyboard) and
`/dev/input/event1` (mouse) — real Linux evdev nodes that any input client can
`read(2)`. Opening them requires an `InputDevice` capability, on purpose: without
that gate, every keystroke on the machine, passwords included, is readable by
anything that can name a path. But **no userspace process on SlateOS holds that
capability today**, and capabilities can only be granted at spawn or inherited
from a parent — so as things stand the nodes exist and are unopenable from
userspace. This is a request for the grant to be made once, at the top of the
process tree that lane B owns.

## Why this lands on lane B

Capabilities reach a process by exactly two routes:

1. `SpawnOptions.capabilities` at `spawn_process` time — kernel-side, and lane A
   only uses it for its own ring-3 self-tests.
2. **Inheritance.** `fork` clones the parent's capability table wholesale
   (`kernel/src/proc/fork.rs`, "cloned capability table already carries the
   authority"), and `execve` does not clear it. So a child spawned the ordinary
   way holds everything its parent held.

There is a third thing that looks like a route and is not: `SYS_CAP_REQUEST`,
the ask-the-human broker. Its resource-type validation table
(`kernel/src/syscall/handlers.rs:6181`) enumerates types 1–15 explicitly and
rejects everything else, so a request for `InputDevice` (30) returns
`InvalidArgument`. That is arguably a bug on lane A's side and is noted below —
but do not design around it, because even if it were fixed it prompts a human,
which is not a thing the display server can do before there is a display server.

That leaves inheritance, and the root of the userspace process tree is
`init/` — lane B's.

## The ask

**Whatever process ends up being the compositor's ancestor should be spawned
holding `(ResourceType::InputDevice, resource_id = 0, Rights::READ)`.**

* `ResourceType::InputDevice` is **30** (`kernel/src/cap/mod.rs:359`).
* `resource_id = 0` is the class grant — it covers both devices, present and
  future. A specific minor (0 keyboard, 1 mouse) grants just that one device.
  The compositor wants the class.
* `Rights::READ` is bit 0. `WRITE` is not needed and should not be given; there
  is nothing to write to these devices.

Concretely, in the `SpawnOptions` for init (or for the service manager, if that
is the closer ancestor):

```rust
capabilities: &[(ResourceType::InputDevice, 0, Rights::READ)],
```

## The part lane A wants lane B's judgment on

**Inheritance is all-or-nothing, and there is no syscall to drop a capability.**
If init holds `InputDevice`, then every process init ever spawns — the shell,
every service, every user program launched from any of them — holds it too, and
none of them can give it up. The gate then protects the machine from nothing,
because the whole tree is inside it.

Lane A does not think that should block the grant: the alternative is that the
compositor cannot read the keyboard at all, and a gate that nothing is inside is
strictly no worse than today's `SYS_CONSOLE_READ_CHAR`, which has no gate. But
it does mean the grant should be made as **low** in the tree as lane B can
manage — on the compositor's own service entry rather than on init, if the
service manager has a per-service capability list or could grow one — so that
the eventual shape is one process with keyboard authority rather than all of
them.

Lane B knows the shape of `init/` and `services/` and lane A does not, so the
choice of *where* is lane B's call. If the answer is "init, for now, because
there is no per-service capability plumbing yet", that is a fine answer — say so
and lane A will treat the narrowing as a tracked item rather than a surprise.

If per-service capability plumbing would be useful and the blocker is the kernel
side, say what shape you want and lane A will build it; `SpawnOptions` is lane
A's and can grow whatever the service manager needs to express.

## Two smaller notes

**The `posix/src/sys_capability.rs` mirror does not need updating for this.**
That module says plainly that its `res` table is partial by design — only the
variants some predicate in `project` actually tests belong there. `InputDevice`
projects onto no Linux capability, so adding it would be noise. Lane A mentions
it only because the "mirror every new `ResourceType`" tripwire from
`requests/a-b-three-resource-types-landed.md` would otherwise read as though it
applied here.

**`SYS_CAP_REQUEST`'s type table stopping at 15 is lane A's to fix**, and lane A
is not fixing it silently as part of this: whether an arbitrary process should be
able to raise a system prompt asking the human for keyboard-tap authority is a
policy question, not an oversight to paper over. It is going to
`open-questions.md`. Nothing lane B does depends on the outcome.

## What lane A tested

`evdev::self_test` and `evdev_fd::self_test` cover the wire format, the
scancode→keycode tables, the per-client cursor, `SYN_DROPPED`, the ioctl
encode/decode round trip against the real Linux request literals, and the grab
battery. A ring-3 test (`proc::spawn::self_test_linux_evdev`) spawns two real
user processes — one granted the capability, which walks the whole `EVIOC*`
interrogation sequence from actual ring-3 `syscall` instructions, and one denied,
which must get `EACCES` from `open`. So the gate described above is proven to
exist and to bite, not assumed.
