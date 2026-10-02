## A-AC97-PROGRAMMED-ITS-DMA-ADDRESS-THEN-IMMEDIATELY-ERASED-IT (lane A, 2026-08-17) - **fixed**

**In short:** the AC97 sound driver told the sound card where in memory to find
its playback instructions, and then - one line later - reset the card, which
wipes that address back to zero. The card was left pointed at physical address
0, so any actual playback would have had the DMA engine read whatever happens
to live at the bottom of RAM and play it as audio. The very first boot test
with a sound card attached printed the mismatch.

**Symptom** (first boot with `-device AC97`):

```
[ac97] BDBAR: MISMATCH (got 0x00000000, expected 0x7f484000)
```

### Cause

The code did this:

1. write the buffer-descriptor-list address to `BDBAR`
2. write `CR_RR` ("reset registers") to the channel control register

`CR.RR` does not mean "reset the run state". It reloads the channel's **entire
register file** to power-on defaults, `BDBAR` included. So step 2 discarded
step 1. Two further properties of the bit made this worse:

- it is **self-clearing** - the hardware drops it when the reset completes, so
  a driver that does not poll for it may program the next register into a
  channel that is still resetting; and
- it is **only honoured while the run bit (`CR_RPBM`) is clear**, so getting the
  order wrong in the other direction silently does nothing at all.

### Fix (`kernel/src/ac97.rs`)

Inverted the order - reset *first*, then program - and made the reset
observable rather than assumed:

```rust
// --- Reset the PCM Out DMA engine, THEN point it at our BDL ---
unsafe { port::outb(nabm_base.wrapping_add(NABM_PCMO_CR), CR_RR); }
// ... poll until CR_RR self-clears, up to CHANNEL_RESET_TIMEOUT_US (100 ms)
unsafe { port::outl(nabm_base.wrapping_add(NABM_PCMO_BDBAR), bdl_phys); }
let bdbar_readback = unsafe { port::inl(...) };
```

and then **read `BDBAR` back and fail init if it disagrees**. The low three bits
are masked off the comparison because the register is 8-byte aligned and does
not store them. A driver that writes a DMA base address and never checks it is
one register-ordering mistake away from handing the hardware an arbitrary
physical address, which is exactly what happened here; the read-back turns that
from a silent corruption into `KernelError::IoError` at init.

Also fixed in passing: the second `frame::alloc_frame()` in the same function
leaked the first frame on failure. `PhysFrame` is a bare newtype over a
physical address with **no `Drop`**, so nothing reclaims it on an early return
- every such path now calls `frame::free_frame` explicitly.

**Verified:** `[ac97] PCM Out BDBAR: 0x74608000` / `[ac97]   BDBAR: OK
(0x74608000)` on the following boot test (329 s, green).
