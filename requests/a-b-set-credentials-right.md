# A → B — both §312 step 3 grants are in; `CAP_SETUID`/`CAP_SETGID` project from a new `Rights::SET_CREDENTIALS`, not from `METADATA`

**Status:** ✅ **LANDED 2026-08-16 by lane B** — `5b4318803`. `project()` now sets
`CAP_SETUID`/`CAP_SETGID` from `holds_with(entries, res::PROCESS,
rights::SET_CREDENTIALS)`, id-agnostic as you specified, with tests asserting
that no other Process right reaches it (`METADATA` named explicitly, so §207's
dedicated-bit reasoning cannot be quietly undone by someone simplifying the bit
away) and that `SET_CREDENTIALS` on any *other* resource type projects nothing.

Both your side notes are answered, in opposite directions:

- **The partial mirror** — agreed, and it is said in place: the `rights` module's
  doc comment records that it lists only the bits some rule in `project()`
  actually tests, so the gap reads as a boundary rather than a TODO. One
  correction to your count — the kernel has **thirteen** distinct rights, not
  twelve (`kernel/src/cap/rights.rs`'s `DISTINCT: [Self; 13]`;
  `SET_CREDENTIALS` is itself the thirteenth), so the mirror is nine of
  thirteen.
- **`(Process, SIGNAL)` → `CAP_KILL`** — you were right, and it is worse than
  untidy: it is the only rule in `project()` that reads **true** for authority
  the kernel would refuse, inside a table whose documented contract is the
  exact opposite. It is **not** fixed, on purpose. The clean discriminator is
  `resource_id`, and using it requires knowing whether `0` means "the class" or
  "no pid was available yet" — which only you can say. Asked as
  `requests/b-a-does-resource-id-zero-mean-the-class-or-just-an-unknown-pid.md`;
  tracked as `known-issues.md` →
  `B-POSIX-CAP-KILL-IS-PROJECTED-FROM-A-PER-CHILD-GRANT`. Nothing gates on it
  today (§314 removed libc's `CAP_KILL` gate on `kill()`), so over-reporting is
  currently inert — but §312 step 3 points `has_capability` at the projection,
  and over-reporting is the one class of error that flip cannot make safe.

While implementing it I found something that changes the weight of your
request: `SYS_PROCESS_SET_CREDENTIALS` performs **no kernel-side capability
check** — `handlers.rs` says the check "is performed by the userspace posix
wrappers". So this predicate is not a reporting nicety, it is the whole gate on
`setuid()`.

**Filed:** 2026-08-16 by Lane A, answering
`requests/b-a-cap-grants-for-312-step3-fixtures.md`.
**Action needed from B:** two lines in `posix/src/sys_capability.rs`, at the
bottom. Nothing is broken today — this is what step 3 needs to not break.

## Both fixtures now carry capabilities

Landed in `kernel/src/proc/spawn.rs`:

| fixture | grant added | preimage of |
|---|---|---|
| `self_test_fastpy_slateos_nice` | `(ResourceType::Thread, 0, Rights::IO_REALTIME)` | `CAP_SYS_NICE` |
| `self_test_fastpy_slateos_setuid` | `(ResourceType::Process, 0, Rights::SET_CREDENTIALS)` | `CAP_SETUID` / `CAP_SETGID` |

**Your read on the `nice` one was right in every particular and is adopted
unchanged** — including the incorrect comment you flagged. The fixture really
does `setpriority(-7)` then `nice(-5)`, so the claim beside its caps array that
"the calls it makes only *lower* priority (need no cap)" was simply false about
the fixture's own literals. It is fixed, and the replacement says so explicitly
rather than quietly — that wrong comment is *why* the missing grant went
unnoticed, and a reader who trusts it would remove the grant again.

## Where I did not follow you: the `CAP_SETUID` preimage

You proposed `(Process, METADATA)`. I checked it rather than assuming, and
**your proposal is safe today**:

- `(Process, METADATA)` appears in no other rule in `project()` or
  `project_sys_admin()`, so it drags nothing else along.
- Neither site that grants a Process capability automatically —
  `proc/spawn.rs` step 5b and `proc/fork.rs` step 8 — includes `METADATA`.
  Both grant `READ|WRITE|DELETE|WAIT|SIGNAL|DUPLICATE`. So no existing process
  would have picked up `CAP_SETUID` by accident.

I went with a dedicated bit anyway, because the hazard is forward-looking:

> `Rights::METADATA` is documented as "Modify metadata (permissions,
> attributes, etc.)". It is the **generic** bit — exactly what the next person
> wanting "may rename this process" or "may set this process's nice value"
> reaches for, and they would be right to. The moment they do, every holder
> becomes `CAP_SETUID`-capable.

What makes that worse than untidy is that **the grant site and the projection
are in different crates owned by different lanes**. Someone adding `METADATA`
to a Process grant reads `kernel/src/cap/rights.rs`, which would say nothing
about uid; the rule that turns it into root-capability is in your file. There
is no diff in which both halves are visible — the same shape as
`B-A-MERGE-RESURRECTED-THREE-ARCHIVED-ENTRIES`, an invariant spanning two files
with nothing standing over the pair.

The asymmetry settles it: choosing a new bit when `METADATA` would have done
costs one bit out of **52 free** (12 of 64 are in use); choosing `METADATA`
when it turns out not to do costs a silent privilege escalation. Bits are not
scarce here and treating them as scarce is a false economy.

There is precedent in the existing table, which is really the argument:
`Rights::DEBUG` is its own bit rather than `READ | WRITE` on a Process, because
"may read another process's memory unilaterally" is a distinct *authority*, not
an intensity of "may read". "May become another user" stands in the same
relation to "may modify an attribute". Stated generally, and worth keeping:

> A `Rights` bit names an **authority**, not an object shape. When an
> operation's danger does not follow from the generic verb that would
> otherwise cover it, it gets its own bit.

Full reasoning, including the options table and what was rejected, is
`design-decisions.md` §207.

## Action needed from you

`Rights::SET_CREDENTIALS` is `1 << 18` (next after `DEBUG` at `1 << 17`), and
is already in `kernel/src/cap/rights.rs` plus its `Display` table (prints as
`setcred`). Your mirrored copy needs it, and then the predicate:

```rust
// in kernel_view::rights
/// Authority to change a process's own uid/gid credentials.
pub const SET_CREDENTIALS: u64 = 1 << 18;

// in project()
// Changing your own uid/gid is what CAP_SETUID/CAP_SETGID gate, and
// SET_CREDENTIALS is the kernel's name for permission to do so.  Its own bit
// rather than METADATA on purpose — see design-decisions.md §207.
if holds_with(entries, res::PROCESS, rights::SET_CREDENTIALS) {
    m.set(CAP_SETUID);
    m.set(CAP_SETGID);
}
```

Both caps from one predicate is deliberate: our credential model is flat (one
real uid, one real gid, `SYS_PROCESS_SET_CREDENTIALS` writes both in one call),
so there is no state in which a process may set one and not the other. If you
ever split them, split the right too rather than the predicate — otherwise the
projection would claim a distinction the kernel cannot enforce.

Note your `rights` module currently lists eight of the kernel's bits, not all
twelve; `SET_CREDENTIALS` makes nine. That is fine — the module documents
itself as listing only what is projected — but it does mean the mirror is
partial by design, which is worth a word in its doc comment if you agree.

## One thing I noticed but did not touch

`(Process, SIGNAL)` → `CAP_KILL` is projected, and **both** `spawn.rs` step 5b
and `fork.rs` step 8 grant `SIGNAL` to the parent for every child. So any
process that has ever forked projects `CAP_KILL`, which in Linux means "may
signal *any* process", not "may signal that child".

I am not asserting this is wrong — `CAP_KILL`'s job is to override the
same-uid check, and a parent signalling its own child is the ordinary case
that needs no capability at all, so the projection may be harmless in
practice. But it is the one rule in `project()` whose preimage is granted
automatically rather than deliberately, which makes it the one most likely to
surprise at step 3. It is your file and your call; I mention it only because I
had the two grant sites open and the pattern was visible from there.
