## A-PROC-VERSION-AND-LOADAVG-DOUBLE-FREE-A-256-BYTE-SLOT

**Found:** 2026-09-01, by `fs::conformance` on its first live boots.
**Status:** **CLOSED** 2026-09-01 by `e24ce4ad6`, confirmed by boot
`bv39hyr6h`: `[syshealth] Tests complete: 24 passed, 0 failed`, and the only
`DOUBLE-FREE detected!` line left in the serial log is the deliberate one from
`poison_self_test` Test 3. The new Test 5 — the regression for this exact shape
— prints `Magic constant in live data: OK (not a double-free)`. That satisfies
the closing criterion at the foot of this entry in full, including its stricter
half: no diagnostic instrumentation is compiled in, because the free-trace was
deleted along with the fix rather than left in place.

**It was not a double-free at all** — it was a false positive in the
allocator's own detector. Everything below the "What it actually was" section is
the diagnosis as it stood *before* the cause was found, kept because the wrong
turns are the useful part; where it contradicts that section, that section wins.

### What it actually was (2026-09-01, final)

`POISON_MAGIC` was a **fixed constant**, so the allocator leaves a copy of it on
the kernel stack every time `poison_free` or `check_poison` runs. `TaskInfo` —
DWARF says size 176 — puts `stack_used: Option<usize>` at offset 0 with its
payload at bytes **8..16**, which is exactly where the signature lives. That
field is `None` for the idle task, so those 8 bytes are *never written*, and the
struct was `memcpy`'d into the heap still carrying `FE ED FA CE FE ED FA CE` —
the allocator's own magic — off the stack. `gen_loadavg`'s single free then read
it back as proof of a previous one.

Corroborated by every other byte in the slot: `id=0`, `name="idle"`,
`name_len=4`, and `len=1 cap=1` (one task exists at that point in boot, so
`collect()` allocates once and never grows).

Nothing in `procfs`, `sched`, or the slab allocator was wrong. The fix is in the
detector: the signature is now `POISON_MAGIC_BASE ^ slot_addr`, 8 bytes at
offsets 8..16, so a leaked copy can only match the slot it came from — see
`design-decisions.md` §672 and `poison_self_test` Test 5, which reproduces this
exact shape. The free-trace instrumentation is removed: it was built to identify
the *first* free, and there was only ever one.

Two corrections to what is written below:

- **"Two files double-free" was one bug, not two, and now zero.** `/proc/version`
  was a `HeapWatch` labelling artefact — it names whichever path is under
  inspection when the global counter moves, and `readdir` generates every
  entry's content.
- **The unexplained 61-byte-string-in-a-256-byte-class mismatch is explained.**
  The block was never the version string; it is a capacity-1 `Vec<TaskInfo>`,
  and `TaskInfo` is 176 bytes.

The `/proc/heapinfo` readdir-vs-stat size disagreement is a **separate** bug and
is not fixed by this — it persisted in boot `bv39hyr6h`, which had zero heap
violations and still reported `2507 clause(s) held, 1 broke`. Cause:
`gen_heapinfo` prints `slab_allocs`/`total_allocs`/etc., which the harness's own
allocations increment between the `readdir` and the `stat`, moving a decimal
digit and changing the byte length. **The defect was in the harness, not in
procfs** — both sizes were true when they were taken, and the clause asserted
that the file held still without ever establishing that it had. Fixed in
`707defe2b` by bracketing the stat between two listings and voiding, rather than
passing, a comparison whose two readings saw different states; see
`design-decisions.md` §673.

**In short:** reading two files under `/proc` — `version` and `loadavg` — each
hands the same 256-byte block of memory back to the allocator twice. The
allocator notices, refuses to reuse the block (so nothing is corrupted), and
bumps a counter. That counter is global, so a *completely different* check
three subsystems away — `syshealth`'s "Heap safety" line — then reports
`2 violation(s)`, and `kshell::self_test` rung 21 panics the kernel on it. The
panic names none of the above.

### How it presents

```
[fsconform] Running cross-backend FileMeta conformance...
[heap] DOUBLE-FREE detected! slot=0xffff80007e3a1900, class=256
[fsconform] FAIL heap:/proc/version — the allocator's corruption counters moved …
[heap] DOUBLE-FREE detected! slot=0xffff80007e3a1500, class=256
[fsconform] FAIL heap:/proc/loadavg — …
…
       [FAIL] Heap safety: 2 violation(s)
!!! KERNEL PANIC !!! kernel\src\kshell.rs:11139
```

Fully deterministic: **the same two slot addresses across separate boots**, and
exactly two events over 496 inspected objects, so it is not allocation-volume
dependent. The two slots are `0x400` apart — four class-256 slots — i.e. the
same slab page.

`/proc/heapinfo`'s "readdir and stat disagree about a regular file's size"
failure is *collateral*, not a third bug: heapinfo's text reports these very
counters, so the count going 0→2 between its `readdir` and its `stat` changes
the text's length. It is expected to disappear with the fix, and if it does not
it needs its own entry.

### What has been ruled out (do not re-derive these)

- **It is not a false positive from stale poison.** The theory was that a slot
  recycled without re-poisoning keeps `POISON_MAGIC` at bytes 8..11, so a later
  free misreads it. It cannot happen: every slab allocation goes through
  `pcpu_slab_alloc` or `slab_alloc`, both of which call `poison_alloc`, which
  fills the **whole** slot with `ALLOC_POISON` (`0xCD`) and destroys the magic.
- **It is not the owner's own bytes colliding with the magic.** That needs bytes
  8..11 to be `FE ED FA CE`; both payloads are ASCII text there.
- **It is not `realloc`.** The allocator supplies no `realloc` override, so `Vec`
  growth uses the default alloc/copy/dealloc, which frees the old block once.

So it is a real double-free.

### What makes it hard, and what was built for it

Neither generator is at fault on inspection, and the two share no code:
`gen_version` is a `format!` of three `const &str`s plus `into_bytes`;
`gen_loadavg` calls `sched::task_list()` and formats. The 61-byte version string
does not obviously want a 256-byte class either, which suggests the doubly-freed
block may not be the text at all.

Two pieces of permanent diagnostic machinery came out of this and should be kept
regardless of the fix:

- `fs::conformance`'s `HeapWatch` samples the corruption counters per inspected
  path, which is what turned "somewhere in ~200 `/proc` generators" into two
  named files.
- `poison_free` now walks the frame chain on detection (`866ffbcb2`), so the
  report names the freeing call site. Resolve the printed addresses with
  `python scripts/symbolize.py --log build/serial-test.txt`.

### What the frames said (2026-09-01)

Sixteen frames (`a9599b35e`) resolved it. **Neither event is `/proc/version`.**
Both are the same drop, in the same function, at the same instruction:

```
0..7   poison_free / slab_dealloc / dealloc / __rust_dealloc /
       Global::deallocate / RawVecInner::deallocate / RawVec::drop /
       drop_in_place<RawVec<sched::TaskInfo>>
8      drop_in_place<Vec<sched::TaskInfo>>
9      procfs::gen_loadavg+0x3e6          <-- both events, same offset
10     procfs::generate
11     ProcFs::readdir::{{closure}}   (event 1)
       ProcFs::stat                   (event 2)
```

Two consequences that overturn the sections above:

- **The `/proc/version` label was a red herring.** `HeapWatch` names whichever
  path was under inspection when the global counter moved; the counter moved
  because `readdir` walks the whole directory and calls `generate` for
  *`loadavg`* while `version` happened to be the entry being sampled. There is
  one bug, not two, and `gen_version` is not involved at all.
- **The doubly-freed block is the `Vec<TaskInfo>` from `sched::task_list()`**,
  not any generated text — which is why 256 bytes never fitted the 61-byte
  version string. `TaskInfo` is ~160 bytes, so a 256-byte class is a `Vec` of
  capacity 1: the buffer `collect()` allocates first and frees again when it
  grows to capacity 2.

`gen_loadavg` therefore double-frees **on every call**, which also explains why
twelve prior boots were green: nothing read `/proc/loadavg` during boot until
`fs::conformance` walked `/proc`. The harness exposes the bug; it does not
create it.

`sched::task_list()` and `gen_loadavg` both read clean, and every slab
alloc/dealloc path has now been audited (including both `slab_dealloc` call
sites in `GlobalAlloc::dealloc`, the quarantine-eviction one being default-off).
Since `POISON_MAGIC` survives to the second free, and `poison_alloc` would have
destroyed it, no allocation of that slot intervened: the `Vec` that
`gen_loadavg` drops points at memory freed earlier and never handed back out.

### Recording the first free

Done in `3ac226bfa`, along the lines sketched below. `poison_free` writes 8
return addresses into bytes 12..76 of every freed slot in a class large enough
to hold them, and prints them as `first-free frame N` on detection;
`check_poison` scans for `FREE_POISON` from byte 76 instead of 12 on those
classes. `gen_loadavg` also prints its `Vec`'s pointer/len/capacity, which says
directly whether the `Vec` was born over already-freed memory. Both are
temporary and come out with the fix.

The original sketch is kept because the constraint in its last sentence is the
part that is easy to get wrong:

> Escalate to recording the **first** free's return addresses in the slot's own
> poison payload (bytes 12..44, which currently hold `FREE_POISON`), and print
> them on detection. "Who freed it the first time?" is the harder and more
> useful half, and the slot is already being written at exactly that moment.
> Note that `check_poison` verifies the `FREE_POISON` fill on allocation, so it
> must be taught to skip whatever range the provenance record occupies —
> otherwise the provenance itself reads as a use-after-free.

### It stopped reproducing, which is not the same as being fixed (2026-09-01)

The boot carrying both instruments (`3ac226bfa`) **did not reproduce it at
all**. `syshealth` went to 24 passed / 0 failed, the `kshell` rung-21 panic
disappeared, and the boot test passed. The `gen_loadavg` print shows the very
slot that had been reported double-freed on the two preceding boots being used
cleanly:

```
[fsconform] Running cross-backend FileMeta conformance...
[loadavg-dbg] tasks ptr=0xffff80007e3a1900 len=1 cap=1 elem=176
[loadavg-dbg] tasks ptr=0xffff80007e3a1800 len=1 cap=1 elem=176
```

Note `len=1 cap=1`: at that point in boot the scheduler holds one task, so
`collect()` allocates once and never grows. **The "buffer freed by a growth
`realloc`" explanation in the section above is therefore wrong** — there is no
growth. The `Vec` is a single 176-byte element in a 256-byte class, allocated
once and dropped once, and it still managed to be freed twice.

That combination is what makes this serious rather than solved. A double-free
of a `Vec` that is allocated exactly once, and that disappears when a serial
write is inserted between the allocation and the drop, is not a defect in
`gen_loadavg` — the function does not have enough moving parts to hold one. It
points instead at the slot being handed to two owners in the first place, which
the earlier "magic survives, so no allocation intervened" argument does **not**
rule out: that argument constrains what happens *between* the two frees, and
aliasing happens *before* both of them.

Do not close this on the strength of a green boot. The next boot (`1d96d8b58`)
removes the `gen_loadavg` print and keeps the heap-side trace, which splits the
two variables:

| Outcome | Reading |
|---|---|
| Double-free returns | The print was the perturbation; the trace now names the first freer, which is the open question. |
| Stays away | The perturbation is on the allocator side — the per-free backtrace walk changes the timing of every free — and the bug is a race, not a `procfs` defect. |

**Trigger for closing:** a boot whose `syshealth` "Heap safety" line reads
`[PASS]` with `fs::conformance` running **and with no diagnostic
instrumentation compiled in**, and where `/proc/heapinfo` no longer disagrees
with itself about its size. A green boot that still carries the free-trace does
not count — the instrument is itself a suspect.

**Resolution of that trigger (2026-09-01):** the discriminating boot ran and the
double-free returned, which settled the table above on the first row — the print
*was* the perturbation. What the trace then showed was not a first freer but
live object data, and decoding it against DWARF gave the cause recorded at the
top of this entry. The free-trace has since been deleted, so the "no diagnostic
instrumentation compiled in" half of the trigger is now satisfied by
construction; what remains is one green boot on `e24ce4ad6`, and the separate
`/proc/heapinfo` size bug.
