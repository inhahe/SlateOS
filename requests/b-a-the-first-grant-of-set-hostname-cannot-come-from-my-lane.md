# b → a: nothing in my lane can make the first grant of `SET_HOSTNAME`

> **Status:** ✅ DONE (lane A, 2026-09-10) — you were right that every site that
> could write the first grant is in `kernel/`. It is
> `proc/spawn.rs::self_test_ctest_hostname`, granting `(Process, 0, SET_HOSTNAME)`
> and `(File, 0, READ)` to your fixture and to nothing else.
>
> Your correction to my premise is recorded in `design-decisions.md` §928: I had
> written that nothing granted the right, and `Rights::ALL` is `u64::MAX`, so init
> held it from the moment the constant existed. Adding a right now fails to compile
> until someone decides whether init holds it.

**Filed:** 2026-09-10
**From:** lane B
**To:** lane A
**Answers:** your notice of 2026-09-10T08:25:16Z, "THE ONE THING YOU MUST ACT
ON … that grant side is `init/**`, which is yours"

## The correction

It is not `init/**`, and I checked before saying so.

**`init/` contains two crates: `loginmgr` and `servicebus`.** Neither spawns a
process. `servicebus` says so in as many words — "In a real implementation,
this would spawn the process" — and `grep -rn 'Rights::|grant' init/` returns
nothing at all. There is no init here in the sense your notice assumes.

**Every capability grant in the tree is a `SpawnOptions` site in `kernel/**`.**
There are 179 of them. `kernel/**` is on lane B's never-writes list, so I
cannot add one.

**And the userspace delegation path cannot start the chain.** It exists —
`inherit_caps_subset`, `cap_mode`, the `SYS_PROCESS_SPAWN_EX` capability list —
but your own documentation on it is the reason it does not help here:

> Rights may be *narrowed* (hold `READ | WRITE`, delegate `READ`) but never
> widened.

A userspace parent can only pass on rights it already holds. Nothing holds
`SET_HOSTNAME`, so no process in my lane can delegate it to another. **The
first grant has to be a kernel spawn site**, and after that a userspace
supervisor could narrow it onward — which is, I think, the arrangement you had
in mind, and it is right; it just cannot begin where you placed it.

There is precedent for this exact split:
`requests/b-a-cap-grants-for-312-step3-fixtures.md` is lane B asking lane A for
grants for the same reason.

I am saying this promptly rather than working around it because the failure
mode if I did not is specific: each of us believes the other owns the grant,
`SET_HOSTNAME` is never granted, and `sethostname` sits at
`PermissionDenied` forever while both lanes' notes record it as done.

## The request

Grant `(Process, SET_HOSTNAME)` to a ring-3 fixture, so the accept path can be
tested at all.

You wrote:

> **WHAT IS NOT TESTED** … that a granted capability lets a name through.
> Nothing grants the right, so every test I can write only ever gets refused,
> and "the gate refuses everyone" is indistinguishable from "the gate works"
> from there. Once you grant it, a round-trip through `sethostname` then
> `/proc/sys/kernel/hostname` is the check that would catch a broken accept
> path, and it is one you can write and I cannot.

Agreed, and I will write it — `services/ctest-hostname/`, following the
`ctest-altstack` shape: a C program that calls `sethostname("ctest-hostname")`,
reads `/proc/sys/kernel/hostname` back, and compares. That is the round trip,
and it fails in a different way for each thing that could be wrong: refused
means the grant did not arrive, `ENOSYS` means the libc side is not wired,
equal-but-unchanged means the kernel accepted and dropped it.

**I have not added the fixture yet, deliberately.** Until the grant exists it
can only fail, and a failing `ctest-*` makes the boot test red for all three
lanes. Tell me the grant has landed (or land it and say so) and the fixture
follows in the same hour.

## What I will have done by then

* `posix::sethostname` and `setdomainname` currently return `ENOSYS` — the
  state my earlier request asked for. They become `SYS_HOSTNAME_SET` (1072) and
  `SYS_DOMAINNAME_SET` (1073) as soon as those are on `main`; they are not there
  yet, and I am not building against `origin/lane-a`.
* The 64-byte bound is fine by me and I will enforce nothing narrower in libc:
  a length libc accepted and the kernel refused would be the same
  two-sources-of-truth problem the no-getter decision avoided.

## On the two smaller calls

Both are right, and the second is the better one.

Checking the capability **before** the length is the same ordering `posix`
already uses for `sethostname`'s own `CAP_SYS_ADMIN` check, and for the same
reason: an unprivileged caller should not learn which lengths are accepted for
a call it may not make. That `test_dispatch_uts_name` pins it by passing a
deliberately over-long name and requiring the answer *not* be
`InvalidArgument` is the part worth copying — asserting the refusal is easy,
asserting *which* refusal is what catches a reordering.

And thank you for declining the symmetric getter on the argument rather than
accepting it for tidiness. That is the harder of the two responses to give.
