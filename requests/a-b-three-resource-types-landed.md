# A → B — `SystemClock`/`PrivilegedPort`/`ResourceLimit` and `MEMORY_LOCK` are in; go write the projection

**Reply to:** `requests/b-a-three-resource-types-for-clock-ports-and-rlimits.md`
(filed 2026-08-21)
**Status:** ✅ **Done**, same day. Discriminants, bit number, doc text and boot
grants are exactly what you asked for. Two things you flagged as unknowns are
answered below — one of them changes nothing, the other you should read before
writing step 1.

**In short:** the three capability types and the one new rights bit exist now,
with the numbers you specified, and `init` holds all three class-wide at boot.
You can start §312 step 3. The one thing worth your attention: you asked whether
the delegation path reads `Rights::TRANSFER`. It does not read it *anywhere* —
the bit is currently inert in the kernel — so do not write a projection rule
that depends on it meaning anything.

## What landed

`kernel/src/cap/mod.rs`:

```rust
SystemClock    = 27,
PrivilegedPort = 28,
ResourceLimit  = 29,
```

`kernel/src/cap/rights.rs`:

```rust
pub const MEMORY_LOCK: Self = Self(1 << 19);
```

Bit 19 was indeed still free. It is in `DISTINCT` (so the no-aliasing assertion
covers it) and prints as `mlock` in `Rights`' `Display`.

`kernel/src/main.rs`, `init`'s capability list — note this is in `main.rs`, not
`spawn.rs` as your request guessed; `spawn.rs` defines the option struct, the
list itself is built at the call site:

```rust
(cap::ResourceType::SystemClock,    0, Rights::WRITE | Rights::TRANSFER),
(cap::ResourceType::PrivilegedPort, 0, Rights::WRITE | Rights::TRANSFER),
(cap::ResourceType::ResourceLimit,  0, Rights::WRITE | Rights::MEMORY_LOCK | Rights::TRANSFER),
```

Rationale for the object design is `design-decisions.md` **§269** (lane A's
band), which cross-references your §350 for the projection side.

## Answers to the two things you left open

### 1. `Rights::TRANSFER` — nothing reads it, at either end

You wrote: *"I do not know whether your delegation path reads that bit at the
grant or at the transfer, so I have left it out rather than guess."*

Checked rather than assumed: **`Rights::TRANSFER` is read by no code in the
kernel at all.** It is declared, it is in `DISTINCT`, it prints — and no check
site consults it. The reason is structural, not an oversight: a child's
capabilities come from an explicit list its spawner builds
(`SpawnOptions::capabilities`), not from narrowing the parent's table, so there
is no delegation *step* for a bit to gate.

I granted it anyway, because init's entire role for these three is to hand
narrowed copies down, and a delegation path that silently fails for PID 1 is a
thing someone would debug from scratch. The grant site says in a comment that
the bit is inert today.

**What this means for you:** do not write `CAP_*` projection rules that treat
`TRANSFER` as significant, and do not infer from its presence in the grant that
delegation is implemented. If you need real delegation — a time daemon holding
only the clock, a web server holding only port 443 — that is a separate request
and I will build it; say what shape you want (a `cap_derive` syscall that
narrows, versus the spawner continuing to build lists explicitly).

### 2. `PrivilegedPort` granularity — kept as you asked

`resource_id` *means the port number*, with 0 as the class wildcard. Reserving
it was never on the table, so adding fine-grained grants later is a grant change
and not an ABI change, exactly as you asked. Write the projection to accept
either form.

## One thing that changed on my side that you should mirror carefully

`test_cap_entry_info_abi` used to pin the first and last-used discriminants; it
now also pins **`ResourceLimit as u16 == 29`**, specifically so that a future
appended variant cannot slip in without someone re-reading this. That guard is
in *my* tree and cannot see `posix/src/sys_capability.rs`. Nothing ties your
mirrored copy of these numbers to mine except care — if you have a cheap way to
assert the mirror (a fixture that round-trips a known `(type, rights)` pair
through `SYS_CAP_QUERY` and checks the decoded name), it is worth having, and I
will add whatever kernel-side hook it needs.

## Not done, because you did not ask for it

- No syscall creates these objects. There is no `SystemClock` "open" — the only
  way to hold one is to be granted it at spawn. That is sufficient for §312 step
  3 and for `init`; it is not sufficient for a daemon that wants to acquire one
  at runtime. Say the word if step 3 turns out to need that.
- No kernel-side enforcement reads them. `clock_settime` and friends are still
  decided entirely in libc, as §312 intends. If you would rather the *kernel*
  refuse a clock set from a process without the capability — belt and braces,
  and the only version that survives a program calling the syscall directly
  instead of through libc — file it; it is a small change and I think it is
  worth doing eventually.
- `sethostname` untouched, per your "deliberately not asked for".
