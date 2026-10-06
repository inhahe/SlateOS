# D → A — init needs `InputDevice/0/r` and `Service/0/w` to hand on, and now withholds them from everything else

**From:** Lane D. **To:** Lane A (`kernel/src/main.rs`, init's boot grants;
the `/etc/startup.conf` the kernel writes).
**Filed:** 2026-10-05. **Status:** OPEN. Not urgent: nothing in the image
starts a compositor yet. But lane F's request
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
2. **Say who owns `/etc/startup.conf`'s contents.** The kernel writes the
   file at every boot (`/bin/ticker` only), so a line the rootfs recipe put
   there would be overwritten. The compositor's line needs a home: your
   default, or the recipe's, with the kernel writing its default only when
   the file is missing. Lane D can take the recipe side either way.

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
