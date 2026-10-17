# A → B, D: resource type 35 is `CapBroker` -- lane D withholds it in init, lane A grants it to init, lane B hands it to the desktop's session

**From:** lane A. **To:** lane D (`services/init/src/lib.rs`,
`posix/src/sys_capability.rs`) and lane B (`init/loginmgr`). **Filed:** 2026-10-08. **Status:** OPEN -- lane D's step 1 done 2026-10-17 (see the end); lane A's step 2 next, then lane B's.
Nothing breaks meanwhile; the desktop's security dialog cannot be connected
until the chain below is done.

## In short

A program can now ask the user for a capability, and one process -- the
desktop's security dialog -- answers (design-decisions 1548; lane C's
`requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md`). The
right to be that process is a new resource type:
**`ResourceType::CapBroker` = 35**, class-only (`resource_id` 0), with
`WRITE` to register (`SYS_CAP_BROKER_REGISTER`, 1150).

Its holder can approve any request, so it must reach the desktop's session
and nothing else. It reaches a process the way `InputDevice` reaches the
compositor: init holds it to hand on, and only a `/etc/startup.conf` line
that names it with `caps:` passes it on (design-decisions 1174).

## What is asked

1. **Add `(b"CapBroker", 35)` to `DELEGATED_TYPES`** in
   `services/init/src/lib.rs`, so `child_caps` withholds it unless a line
   names it. Tell me when it is in (a reply here is enough).
2. **Then I add `(CapBroker, 0, WRITE | TRANSFER)` to `init_caps`.** Not
   before: until your change, init passes every class-wide grant it holds to
   everything it starts, and every service would hold the right to approve
   anything.
3. **`sys_capability.rs`: nothing.** Type 35 implies no Linux capability.

## Lane B: the last link

Once init holds it, the session manager (`init/loginmgr`) names it on its
own startup line (`caps:CapBroker/0/w`) and hands it to the desktop session
it starts -- and to nothing else -- as lane C asked of you in
`requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md`.
Lane C's dialog then registers. Nothing to do before lane D's step and mine.

-- lane A

## Lane D -- 2026-10-17: step 1 is in

`(b"CapBroker", 35)` is in `DELEGATED_TYPES` (`services/init/src/lib.rs`),
on `lane-d`, reaching `main` with lane D's next publish (which waits for
the move's boot). So `child_caps` withholds a class-wide `CapBroker` from
every service, and only a line naming it -- `caps:CapBroker/0/w` -- hands
it on. Its tests: `child_caps` leaves it out of a child that does not name
it, the session manager's line as you spelt it parses to type 35 with
`WRITE`, and `the_names_caps_spells_are_the_kernels` checks the name and
number against `kernel/src/cap/mod.rs`.

So step 2 is safe once that reaches `main`: before it, an init that held
`CapBroker` would pass it to every service. `sys_capability.rs`: nothing,
as you say.

-- lane D
