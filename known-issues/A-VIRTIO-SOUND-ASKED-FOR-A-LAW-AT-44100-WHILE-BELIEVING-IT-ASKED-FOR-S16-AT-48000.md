## A-VIRTIO-SOUND-ASKED-FOR-A-LAW-AT-44100-WHILE-BELIEVING-IT-ASKED-FOR-S16-AT-48000 (lane A, 2026-08-17) - **fixed**

**In short:** once the virtio sound card could be reached at all (see the entry
above), the very next thing it did was refuse to play. The driver names the
audio format and the sample rate by number, and all three of the numbers it
used were wrong: it asked for **A-law** while believing it asked for
**16-bit PCM**, and for **44100 Hz** while believing it asked for **48000 Hz**.
The card rejected the format outright. Had it not, the tone would have played
at the wrong pitch.

**Symptom** (second boot with `-device virtio-sound-pci`):

```
[virtio-snd] SET_PARAMS for stream 0 failed: status 0x8002
[virtio-snd]   Short tone playback: IoError (non-fatal)
```
plus, from QEMU itself:
```
qemu-system-x86_64.exe: Stream format is not supported.
```

`0x8002` is `VIRTIO_SND_S_NOT_SUPP`.

### Cause

`kernel/src/virtio/sound.rs` carried three hand-picked constants:

| Constant | Was | Should be | What the wrong value actually means |
|---|---|---|---|
| `VIRTIO_SND_PCM_FMT_S16` | 2 | **5** | 2 = `A_LAW` |
| `VIRTIO_SND_PCM_RATE_44100` | 5 | **6** | 5 = 32000 Hz |
| `VIRTIO_SND_PCM_RATE_48000` | 6 | **7** | 6 = 44100 Hz |

These are **dense ordinals** in virtio 1.2 §5.14.6.6.1 - the list starts at 0
and has no gaps - and each value is used two different ways: it is the number
written into `set_params.format`, *and* it is the bit index into the `formats` /
`rates` masks the device advertises. That dual use is what made the bug
survivable in review: a wrong ordinal is wrong in both places at once, so any
check that compares our request against our own constants agrees with itself
perfectly.

The device's own answer proves the corrected numbering, which is worth
recording because it is a check anyone can repeat from the boot log:

```
[virtio-snd]   Stream 0: dir=OUT ch=1-2 fmts=0xe0078 rates=0x3fff
```

`0xe0078` = bits 3,4,5,6,17,18,19 = S8, U8, S16, U16, S32, U32, FLOAT - exactly
the set QEMU's virtio-sound implements, and only under the corrected numbering.
Under the old numbering bit 2 (the `S16` we were sending) is *clear* in that
very mask, so the driver was printing its own refutation and discarding it.

### Fix

Two parts, because fixing only the first would leave the next such mistake just
as invisible:

1. **The complete format and rate tables are now spelled out** (`mod fmt`,
   `mod rate`), all 25 formats and all 14 rates, even though one of each is
   used. A lone constant with a spec citation beside it is indistinguishable
   from a correct one at a glance; a contiguous run starting at 0 that can be
   read off against the spec's own list is not. This is the actual defence -
   the values were not mistyped, they were *guessed*, and only a form that
   makes guessing visible prevents a re-guess.
2. **`set_params` now validates the request against the advertised masks
   before sending it.** The driver was already reading `formats`, `rates`,
   `channels_min` and `channels_max` per stream and printing them, then
   throwing them away; they are now kept in `StreamCaps` and checked. A bad
   request fails with the field and the value named:

   ```
   [virtio-snd] ERROR: stream 0 does not accept format ordinal 2
                (device advertises formats 0xe0078)
   ```

   instead of `status 0x8002`, which names no field, no value, and is the same
   four bytes whether the format, the rate or the channel count was at fault.

A stream with no recorded capabilities is deliberately **not** rejected - a
device may expose more streams than `MAX_STREAMS`, and refusing to drive one we
merely failed to record would turn a missing optimisation into a failure. In
that case the request goes out and the device arbitrates, as before.

### The generalisation

This is the third bug in two hours found by the same method and unfindable by
any other: attach the hardware and read what it says back. Note in particular
that the refuting evidence was **already in the boot log** before the fix - the
driver printed the capability mask that contradicted its own constants. Reading
a value into a log message is not the same as checking it; where a device tells
you what it accepts, check the request against it rather than printing both and
trusting the constant.
