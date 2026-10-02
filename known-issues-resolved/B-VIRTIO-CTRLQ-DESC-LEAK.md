## B-VIRTIO-CTRLQ-DESC-LEAK — GPU and sound control commands never freed their descriptors (lane A, 2026-08-16)

**Status:** FIXED 2026-08-16, same change as B-VIRTIO-UNVALIDATED-USED-ID.

**In short.** Sending a command to the virtual GPU or sound card used up two
slots from a small fixed pool, and never gave them back. The pool holds 64.
The screen-update path sends two such commands per frame, so after about
sixteen screen updates the pool was empty and the display stopped updating —
with no error message, because the driver treated "no slots" as "try again
later" and simply never succeeded again.

### Where it was

`kernel/src/virtio/gpu.rs` — `send_ctrl_cmd` and `attach_backing`;
`kernel/src/virtio/sound.rs` — the three control-command helpers. All five had
the same shape:

```rust
dev.controlq.submit(&[ (req, len, 0), (resp, len, VRING_DESC_F_WRITE) ])?;
dev.transport.notify_queue(0);
loop {
    if dev.controlq.poll_used().is_some() { break; }   // <-- head discarded
    ...
}
```

`poll_used` returns the head descriptor index of the completed chain, and that
index is the only handle by which the chain can be returned to the free list.
Discarding it with `.is_some()` leaked the whole chain. `blk.rs` and `net.rs`
did call `free_chain`; the GPU and sound drivers did not, and nothing tied the
two halves together.

### Why it mattered more than it looks

The GPU init sequence issues a handful of commands, which is survivable. But
`flush_rect` and `flush_full` — the per-frame display update path — each call
`transfer_to_host_2d` **and** `resource_flush`, so a single screen update costs
four descriptors. At a queue size of 64 that is sixteen updates before
`submit` starts returning `WouldBlock` permanently. The control queue is only
rebuilt by a device reset, and nothing resets it on this path.

### Why it was not caught

The same reason as the entry above, from the other direction: the tests drive
a well-behaved QEMU device and check that a command *succeeds*. A leak does
not make any individual command fail — it makes the *seventeenth* one fail, and
no test issued seventeen.

### How it is fixed

Each site now binds the head out of `poll_used` and calls `free_chain` after
the response has been read. The response lives in the DMA control frame rather
than in the descriptors, so freeing first is safe.

The timeout paths deliberately **do not** free: a chain that timed out is still
owned by the device, which may yet write into it, so returning it to the
allocator would hand out a live DMA target. Those paths leak two descriptors
by design, which is the right trade — and the leak is now bounded by how often
a device times out rather than by how often it succeeds.

### Related, deliberately left alone

`net.rs`'s TX timeout path *does* free a timed-out chain, which is the opposite
choice. It is safe against the double-free it used to risk — a later completion
for that chain is now rejected by `poll_used`'s `is_allocated` check rather
than corrupting the free list — but it can still let the device read a
descriptor whose buffer has been reused. The proper fix is what `blk.rs` does:
reset the device on timeout (`recover_after_timeout`) rather than reclaiming
descriptors underneath it. Left as tech debt: changing it to leak instead
would trade a rare wrong-packet for a permanent queue drain on a flaky device,
and neither is right without the reset path.
