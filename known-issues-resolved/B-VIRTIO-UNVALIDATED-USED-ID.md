## B-VIRTIO-UNVALIDATED-USED-ID — a device picks the index into its own descriptor table (lane A, 2026-08-16)

**Status:** FIXED 2026-08-16 by the SPARK component in `kernel/ada/` — see the
"How it is fixed" section at the end. Recorded in full anyway, because the
shape of this bug is the argument for `design.txt`'s Ada/SPARK lane and will
recur in every other queue-based driver we write.

**In short.** A virtio device tells us which request it just finished by
writing a number into shared memory. We took that number and used it, without
checking it, as an index into an array — so the device, not us, chose which
kernel memory we read and wrote. A malicious or merely buggy device could pick
a number that lands far outside the array.

### What the code did

`kernel/src/virtio/queue.rs:283` reads a descriptor by raw pointer arithmetic:

```rust
fn desc(&self, idx: u16) -> &VirtqDesc {
    // SAFETY: idx is within 0..queue_size (ensured by alloc_desc).
    unsafe { &*(self.virt_base.add(idx as usize * 16) as *const VirtqDesc) }
}
```

**That SAFETY comment is false, and its falseness is the bug.** `alloc_desc`
is not the only producer of `idx`. `free_chain(head)` (line 255) takes `head`
straight from the caller, and every caller gets it from `poll_used()`, which
reads it out of the **used ring** — a structure the *device* writes:

| Call site | Value passed |
|---|---|
| `blk.rs:362`, `:491`, `:568` | `completed_head` from `poll_used()` |
| `net.rs:361`, `:367` | `head_idx` from `poll_used()` |
| `net.rs:355` | same value into `desc_phys_addr()`, also unchecked |
| `sound.rs:716` | `head` from `poll_used()` |
| `net.rs:497`, `:504` | `completed_head` from `poll_used()` |

`u16` is not a bound. The queue frame is 16 KiB and a descriptor is 16 bytes,
so a legitimate index is at most 255 for a 256-entry queue. A device reporting
`0xFFFF` yields `virt_base + 0xFFFF * 16` = **1 MiB past the start of the
frame** — a read *and* a write (`free_desc` at line 245 writes `desc.next` and
`desc.flags`) at an address the device chose. In the HHDM that address is
mapped, so there is no fault to catch it; it silently corrupts whatever is
there.

### Three distinct failures, not one

1. **Out of range.** As above — an index beyond `queue_size`.
2. **In range but not allocated.** A device may name a descriptor we already
   freed. `free_chain` frees it again, pushing it onto the free list twice;
   `alloc_desc` then hands the same descriptor to two live requests, which
   then DMA two different buffers to one address.
3. **A cycle.** `free_chain`'s `loop` (line 257) is unbounded and terminates
   only when it finds a descriptor whose `VRING_DESC_F_NEXT` is clear. The
   `next` links it follows live in `desc.next` — **inside the device-visible
   table**. A device that writes a ring of descriptors pointing at each other
   hangs that CPU forever inside a kernel loop, with no timeout and nothing to
   preempt it.

Failure 3 is the one that shows the structural mistake rather than a missing
check: the free list itself was stored where the device could edit it. No
amount of validating `head` fixes that, because the *links* are attacker data
too.

### Why it was not caught

Every existing virtio test drives a QEMU device that behaves. The bug needs a
device that lies, and we have no such harness — the tests confirm the driver
works, which was never in doubt. This is the general hazard with a hostile-
input bug: the test suite's silence is not evidence.

### How it is fixed

`kernel/ada/src/virtqueue_descriptors.ad[sb]` moves the index arithmetic into
SPARK and moves the chain links out of device-visible memory:

* The links live in a private array in kernel-only memory. The descriptor
  table becomes a write-only rendering of state whose authoritative copy the
  device cannot reach — which is what answers failure 3 at the root.
* `vqd_free_chain` answers all three failures explicitly and returns
  `Freed = 0` for each, having changed nothing: it validates the entire chain
  before freeing any of it, so a rejection is total rather than leaving half a
  chain on the free list.
* The walk is a bounded `for` loop over `1 .. Size`, so termination is
  structural. A cycle cannot hang the kernel even if one were somehow built.
* `gnatprove` discharges 106/106 checks — no overflow, no index outside its
  range, on every path for every input. Because the exported operations take
  raw `U16` rather than a constrained subtype, "the device sent nonsense" is
  not a special case; it is the same proof.

Note the deliberate choice **not** to give the prover an invariant to lean on:
sizes are clamped when read, not constrained when written. An invariant is a
promise about what that package writes, and in a kernel an unrelated wild
write can land in the middle of those arrays — a proof resting on the
invariant would be sound about the code and wrong about the machine. See the
body's notes.

### How `queue.rs` uses it (completed 2026-08-16)

The rewire is done — the driver now uses the proved component rather than
merely coexisting with it:

* `Virtqueue` owns a slot in the Ada pool (`queue_id`), claimed at `new` and
  released on `Drop`. `Drop` resets the slot first, so a stale index arriving
  after a device goes away is answered as invalid rather than against whichever
  driver claims the slot next.
* The free list is **gone from the descriptor table**. `new` no longer builds a
  `next` chain, and `free_desc` no longer writes one. `desc.next` is now
  written only when a chain is submitted, and read only by the device.
* `alloc_desc` is `vqd_allocate`; `free_chain` is `vqd_free_chain` and returns
  the number freed, logging a warning on the 0 that means "rejected".
* `submit` records chain topology with `vqd_link` / and rolls the whole chain
  back if any step is refused.
* **`poll_used` validates at the boundary.** `elem.id` is checked for width
  (a `u32` id of `0x1_0000` would otherwise truncate onto descriptor 0, a
  plausible-looking index for a completion that never happened) and then
  against `vqd_is_allocated`; a failure is logged and the completion dropped.
  Validating here rather than in each driver is deliberate: there are ten call
  sites across four drivers, and every one of them immediately uses the value
  to look something up.
* `desc` / `desc_mut` are bounds-checked and return `Option`. The false SAFETY
  comment quoted above is gone — the check at the dereference is now what makes
  the claim true, rather than an assertion about callers that was never
  enforced. `desc_phys_addr` returns `Option<u64>` for the same reason.

**A flaw found in the SPARK component itself while wiring this up**, worth
recording because it is the failure mode proof does *not* catch. `Allocate`
handled a damaged free-list link by setting `Head := 0` while leaving
`Free_Cnt` positive — and its own comment claimed this "truncates the free list
rather than aiming it somewhere." It did not truncate; it aimed at descriptor
0. The next `Allocate` would hand out descriptor 0 while it was still
allocated, aliasing two chains onto one descriptor. `gnatprove` was perfectly
happy: every array access was in range, which is all absence-of-run-time-errors
asserts. The proof bounds the *indices*, not the *meaning*. Fixed by making
truncation real (`Free_Cnt := 0`), so the queue reports exhausted until
`Initialize` or `Reset` rebuilds the list; re-proved at 106/106.
