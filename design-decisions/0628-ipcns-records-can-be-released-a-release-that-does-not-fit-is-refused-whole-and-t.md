## 628. `ipcns` records can be released, a release that does not fit is refused whole, and the global totals became live counts

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `/proc/ipcns` and the `ipcns` shell command report how much shared
memory, how many semaphore sets and how many message queues each IPC namespace
holds. Every way of changing those numbers only ever *added* to them. There was
no way to say "that segment is gone" and no way to correct a number typed
wrongly — re-entering it correctly added the right value underneath the wrong
one — so the only repair for one mistyped character was to destroy the whole
namespace and re-enter every other row in it. This adds the missing half:
`release_shm`/`release_sem`/`release_msg`, reachable from the shell as
`ipcns shm del …`. Two judgement calls came with it, both recorded below: a
release that does not fit is **refused whole** rather than clamped, and the
`del` size is **mandatory** where the `add` size has a default.

### The defect

`record_shm` is, in full:

```rust
ns.shm_segments += 1;
ns.shm_bytes += bytes;
state.total_shm += 1;
```

`record_sem` and `record_msg` are the same shape. The module published nothing
between those three and `destroy_ns`, which removes the entire namespace. So the
set of operations was: add a row, or throw away the ledger.

That is a modelling error rather than a missing feature. The columns are named
`shm_segments` and `shm_bytes` — quantities a namespace *holds*, not events that
*happened to* it. A quantity that can only rise is not a quantity; it is an
event counter wearing a quantity's name, and everything downstream reads it as
the former. `/proc/ipcns` prints it under a header that also carries
`Namespaces:`, which was live, directly above a table that was also live.

### Why refuse a release that does not fit, rather than clamp it

The obvious implementation is `saturating_sub` on both columns. It is one line
shorter, it never returns an error, and it is wrong in a way that is hard to
see, because each row has **two** counters that must stay consistent: a count of
segments and a total of their sizes.

Consider a namespace holding one 4096-byte segment, and a caller that releases
"a segment of 8192 bytes" — a plausible mistake, since the caller has to
remember the size it recorded.

| | `saturating_sub` | refuse whole |
|---|---|---|
| Returns | `Ok(())` | `Err(InvalidArgument)` |
| Leaves | `shm=0(0 B)` | `shm=1(4096 B)` |
| Caller learns | nothing | the amount was wrong |

The clamped row is not merely inaccurate — it is *plausible*, and it reports
success, so nothing will ever prompt anyone to look at it again. Worse variants
are reachable the same way: clamping the count but not the bytes gives
`shm=0(4096 B)`, a namespace with no segments and 4096 bytes inside them, which
no sequence of real events can produce.

So both halves are checked before either is touched, and a release that would
underflow either changes nothing at all. This is the same rule the operand
handling in the shell arms follows, one level down: **a number that cannot be
right is refused, not rounded into one that looks right.** `ipcns::self_test`
case 7 pins it by reading the columns back after a refused release — a
`saturating_sub` implementation passes the `Err` check and fails there.

The cost is that callers must track the size they recorded. That is real, and it
is the right price: the alternative is an API where a caller that has *lost*
track can still call the function and be told it worked.

### Why the global totals became live, and `ops` did not

`total_shm`/`total_sem`/`total_msg` were cumulative-ever, and `destroy_ns` did
not touch them. The old self-test asserted the consequence directly:
`(nss, shm, sem, msg) == (0, 1, 1, 1)` — zero namespaces, one shm segment. That
is `/proc/ipcns` printing `Namespaces: 0  SHM: 1` above an empty table.

Two numbers under one heading meant two different things, and nothing in the
output said which was which. They are now the column sums of the table beneath
them: `release_*` decrements them and `destroy_ns` returns the destroyed
namespace's rows. The subtraction in `destroy_ns` is exact rather than
saturating, on purpose — each global is the sum of the column it totals, so the
row being removed cannot exceed it, and a `saturating_sub` there would absorb a
skew instead of letting it surface.

`ops` stays cumulative. It counts state accesses including failed ones, it is
labelled `Ops:` rather than named after a resource, and a monotonic counter is
what a reader expects from it.

### Why the shell verb is mandatory

The grammar changed from `ipcns shm <ns_id> [bytes]` to
`ipcns shm add|del <ns_id> …`, and the verb is required: `ipcns shm 1 4096` now
refuses instead of recording. Defaulting it to `add` would preserve every
existing command line, and was rejected — `add` and `del` are opposites, and
choosing between opposites from no information is exactly the defect §607 and
its successors have been removing from this shell one command at a time.

The break is loud and total (every affected line errors immediately, naming the
two words it wanted), it is confined to a diagnostic command, and the only
callers in the tree are `kshell::self_test` rung 91, which is updated here.

### Why `del`'s size is mandatory where `add`'s is not

`add`'s bracketed `[bytes]` keeps its documented default of 4096 (§607).
`del`'s is required. The asymmetry looks inconsistent and is deliberate:

- `add`'s default is a **documented size** — a page. Getting it means recording
  a segment of a size the synopsis names.
- `del`'s default would be a **guess at what to take away**. Getting it wrong
  destroys a record rather than inventing one, and the caller who omitted the
  word is precisely the caller who does not know what the right value is.

A guessed undo is the same class of defect as a guessed operand, applied to the
repair rather than the mistake. The usage block says so in one line, because
the asymmetry will otherwise read as an oversight:

```text
  (a `del' size is mandatory: guessing what to remove is not an undo)
```

### What this makes possible that was not

`kshell::self_test` rung 91 now asserts the sequence a person would actually
type after a typo — `shm del <ns> <wrong>`, `shm add <ns> <right>` — and pins
that it leaves `shm=1(1024 B)`: one segment, the corrected size. Before the
inverse existed that sequence had no first line, and the same correction left
two segments whose byte total was the sum of the mistake and the fix.

### How to reverse

Delete `release_shm`/`release_sem`/`release_msg` and the `add|del` verb
(restoring `parts` indices 1 and 2 in the three arms and dropping
`ipcns_verb`); restore `destroy_ns` to `remove(idx)` with no subtraction; drop
cases 6 and 7 from `ipcns::self_test`, the global-total assertions added to
cases 8 and 10, the `release_*` lines added to case 9, and the second half of
rung 91. Nothing outside this module, those shell arms and `gen_ipcns`'s header
reads any of it, and `gen_ipcns` needs no change either way — its format string
is unchanged; only the meaning of three of its numbers is.
