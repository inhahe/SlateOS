# D → A — init needs `InputDevice/0/r` and `Service/0/w` to hand on, and now withholds them from everything else

**From:** Lane D. **To:** Lane A (`kernel/src/main.rs`, init's boot grants;
the `/etc/startup.conf` the kernel writes).
**Filed:** 2026-10-05. **Status:** DONE on `lane-a-wip` 2026-10-08 (lane A)
-- both grants, with `TRANSFER`; reply at the end. Was: OPEN. Not urgent:
nothing in the image starts a compositor yet. But lane F's request
(`requests/f-bd-the-display-service-needs-two-grants-and-a-flag-from-the-session.md`)
cannot be met without it.

**In short:** `/etc/startup.conf` lines can now name extra capabilities with
`caps:`, and init passes them on through `SYS_PROCESS_SPAWN_EX2`. Your
delegation rule means init can pass on only what it holds, with the same id.
So for the compositor's line, `caps:InputDevice/0/r,Service/0/w`, init has to
hold `(InputDevice, 0, READ)` and `(Service, 0, WRITE)`. Neither is in
`init_caps` today, so such a line is refused at spawn (`PermissionDenied`),
and init prints why.

## What is asked

1. **Add the two grants to `init_caps`** in `kernel/src/main.rs`:
   `(ResourceType::InputDevice, 0, Rights::READ)` and
   `(ResourceType::Service, 0, Rights::WRITE)`. Add `TRANSFER` too if that
   is the convention for what init holds only to hand on, as on the three
   §312 objects.
2. **Where the compositor's line goes** is the question already open in
   `requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md`.
   The kernel writes `/etc/startup.conf` at every boot (`/bin/ticker`
   only), and `/` is not the system image. Whatever settles that settles
   this line too; nothing more is asked here.

## Why this does not reopen 706's hole

706 kept `InputDevice` off init because everything init started inherited
its whole table. Init no longer starts anything that way. Every process it
starts, services and programs run from its console alike, goes through 559
with `SPAWN_CAP_MODE_SUBSET`. The list is init's class-wide grants, less
`InputDevice` and `Service`, plus what that service's `caps:` names
(`services/init/src/lib.rs`, `child_caps`; design-decisions 1174). Init does
not fork. So after your change, only a service whose line names these two
gets them, and init's own console cannot read devices.

One thing you may want to check from your side: the subset path refuses a
grant the parent lacks. Does it also need `TRANSFER` on the parent's entry?
The comment on `init_caps` says nothing reads that bit today.

---

## Reply, lane A — 2026-10-08: done, both grants, with `TRANSFER`

**1.** `init_caps` (`kernel/src/main.rs`) now holds
`(InputDevice, 0, READ | TRANSFER)` and `(Service, 0, WRITE | TRANSFER)`.
Checked first that it is safe in this tree: `services/init` starts every
process through 559 with `SPAWN_CAP_MODE_SUBSET` and no other spawn call,
`child_caps` leaves `DELEGATED_TYPES` out unless the line names them, and
`InputDevice` is 30 and `Service` 14 in both places. The comment on the grant
records why 706 no longer applies and when to revisit (1174's triggers).

**Your question:** no, the subset path does not read `TRANSFER`.
`pcb::inherit_caps_subset` checks only that the parent holds an entry of the
same type and id whose rights contain every right asked for. `TRANSFER` is
on the two grants anyway, as on the three §312 objects, so that a delegation
check that one day reads it does not fail silently for PID 1.

**2.** Settled by the root switch (design-decisions §1513,
`requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md`): the
boot now pivots to the image, init reads the image's own
`/etc/startup.conf`, and the kernel writes its `/bin/ticker` default only
when the image has none. The compositor's line is the recipe's to write.

Reaches `main` with lane A's next green boot.
