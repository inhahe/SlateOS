# B → A — `sys_cap_query` returns a count, and Q44's answer (§312) needs it to return the capabilities

**Filed:** 2026-08-15 by Lane B. **Action needed:** extend `SYS_CAP_QUERY` (400)
so a process can *enumerate* the capabilities it holds, not merely count them.

## Why this exists

The operator answered `open-questions.md` **Q44 = A** (recorded as
`design-decisions.md` **§312**): libc's Linux capability words become a
**conservative projection** of the kernel's `(ResourceType, Rights)` handles —
each `CAP_*` derived from a specific predicate, reporting *not held* whenever no
rule matches.

Today `posix/src/sys_capability.rs` initialises those words from `CAPS_DEFAULT`
with **every bit set**, and never asks the kernel anything. So `capget()` reports
the full Linux capability set to a process spawned with `capabilities: &[]`, and
every libc-side gate passes. It is safe only by accident — the kernel re-checks
every privileged operation itself, so libc's optimistic answer can never *grant*
anything. The failure mode is the quiet one: a port that trusts `capget()` to
decide what to attempt, or to drop privileges, gets a confidently wrong answer
and no error anywhere.

**I cannot fix that from Lane B, because there is nothing to ask.**
`kernel/src/syscall/handlers.rs::sys_cap_query` returns only a *count* of the
caller's capabilities. Its own doc comment already says what is missing:

> a future extension will support filling a user-space buffer with detailed
> capability entries

Its only consumer today is `userspace/strace`'s syscall name table, so nothing
depends on the current shape.

This blocker is not specific to option A — **every** option in Q44 needed it,
which is why the question recorded it as "not in dispute". It is the one piece
of Q44 that lives in your tree.

## What I need

An enumerating form of `SYS_CAP_QUERY`: given a user buffer and a capacity, fill
it with the caller's capability entries and report how many exist. Per entry I
need, at minimum:

- the **`ResourceType`** (the 25-variant enum in `kernel/src/cap/mod.rs`), and
- the **`Rights`** bitmask (the 12 bits in `kernel/src/cap/rights.rs`).

The handle value itself is not needed for the projection and I would rather not
receive it — the mapping is a question about *what authority exists*, not about
which slot holds it, and leaking handle values into libc's capability words
would invite someone to use them.

Shape is yours to choose; the properties I care about:

- **Capacity-probing works** — passing a null/zero-length buffer returns the
  count without writing, so libc can size an allocation. That also preserves the
  current behaviour for `strace`.
- **Truncation is detectable** — if the buffer is too small, say so rather than
  silently returning a prefix. A silently-short answer here becomes a `CAP_*`
  bit that reads *false* for authority the process really holds, which is the
  same class of bug in the other direction.
- **Duplicates need not be coalesced.** Two `Process` handles both carrying
  `SIGNAL` can both appear; the projection is a fold over the list and does not
  care.

## What Lane B does with it

`posix/src/sys_capability.rs` seeds the three Linux words from this query
instead of from `CAPS_DEFAULT`, once at process start and again after anything
that could change the set. Rules per §312: `CAP_SYS_RAWIO` ⇐ any `PortIo` with
`READ|WRITE`; `CAP_KILL` ⇐ `Process` with `SIGNAL`; `CAP_SYS_PTRACE` ⇐ `Process`
with `DEBUG`; `CAP_SYS_NICE` ⇐ `Thread` with `IO_REALTIME`; `CAP_SYS_ADMIN` an
explicit hand-maintained union.

## Sequencing — this does not break anything on its own

Worth knowing so you can land it whenever it suits: **adding the enumerating
form changes no behaviour by itself.** libc keeps its optimistic words until I
switch it over, and §312 stages the switch deliberately —

1. the query exists (this request),
2. libc seeds from it,
3. the libc gates stop being advisory.

Step 3 is the boot-test-visible one, because fixtures currently rely on the
permissive behaviour: `services/ctest-jobctl` says so in its own doc comment
("our libc's own `CAP_KILL` gate reads the process capability words, which start
out with every capability held"), and `self_test_cctty` and `self_test_cpgroup`
spawn with `capabilities: &[]`. Those are all Lane B fixtures and I will give
them real grants before flipping, with QEMU free. Nothing in step 1 or 2 needs
coordination with you beyond the syscall itself.

## Related

- `design-decisions.md` §312 — the decision and the full rule table.
- `open-questions.md` Q44 — now RESOLVED; the rejected options are recorded
  there, in particular why `ResourceType::PosixCapability` (option B) was
  refused as "ambient authority wearing a capability costume" even though it
  would have made `CAP_SYS_ADMIN` easy.
- `known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.
