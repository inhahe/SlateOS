## 1520. The sound card is fed by a polling pump in the kernel, and native programs reach it through a door that speaks the Linux device ABI

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous), on lane E's request and lane D's choice of its option (a) · **Lane:** A

**In short:** no program could make a sound: the kernel's mixer added every
program's audio together but nothing ever passed the sum to a sound card, so
a player stalled after its first tenth of a second; and a program built on
SlateOS's own C library could not drive the sound device at all
(`requests/e-ad-no-application-can-reach-the-sound-device.md`). Now a kernel
task (the "pump") keeps the card fed from the mixer, and a small family of
native calls -- the "device door" -- lets the C library open the sound
device and pass it exactly the requests a Linux program would, answered by
the kernel's existing Linux handlers.

**What changed:**
- `audio_out`: at boot the first usable card -- Intel HD Audio, else
  virtio-sound, else AC'97 at 48 kHz -- becomes the sink. A kernel task at
  real-time priority (4) wakes every 5 ms, reads how far the card's DMA engine
  has read its cyclic buffer, and refills it from the mixer to 64 ms ahead;
  for virtio-sound, which takes periods as queue messages, it keeps three in
  flight and refills each the device returns. The card runs only while a
  stream is open; two seconds after the last closes it stops, and the pump
  parks until a stream opens. With no usable card, opening a PCM or control
  node answers `ENODEV`.
- Blocking writes and drains: a blocking descriptor's `write`,
  `WRITEI_FRAMES` and `DRAIN` wait for the pump (interruptibly), as ALSA's
  do; a non-blocking `DRAIN` answers `EAGAIN` and reads `SETUP` once played
  out. `PAUSE` now pauses the stream in the mixer. A `poll` on a substream
  blocks on the pump's wake instead of re-scanning on a timer.
- The device door, `SYS_DEVICE_OPEN/IOCTL/READ/WRITE/CLOSE` (1119-1123):
  open a device node by path (handle, and its kind), then requests, reads and
  writes as on Linux. Every answer is the device ABI's -- a negated Linux
  errno on failure, not a kernel error code.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Poll the card's position every 5 ms (chosen)** | a kernel task wakes 200 times a second while sound plays, never while it does not | one loop serves HDA, AC'97 and virtio-sound alike; no interrupt plumbing (HDA's IRQ handler deliberately takes no lock, and a refill needs the device lock); a late tick costs nothing until it is 64 ms late | 200 wakeups a second while playing; latency of the 64 ms lead |
| B. Refill on the card's interrupt at each period's end | the card's interrupt drives the refill | no wakeups beyond the periods | the refill would run in, or be signalled from, interrupt context per driver; AC'97 has no IRQ wired at all; a second scheme for virtio-sound anyway |

| | What changes | For | Against |
|---|---|---|---|
| **Run the card only while a stream is open (chosen)** | idle machines do no audio work | no DMA traffic or wakeups at idle; the 2 s tail keeps track changes seamless | a first sound after idle starts the card (microseconds of register writes) |
| Run it always | -- | no start latency | a wakeup every 5 ms forever, for silence |

| | What changes | For | Against |
|---|---|---|---|
| **No card: `ENODEV` on open (chosen, lane E's preference)** | a program says "no sound device" | the user is told the truth | a program that would rather play into nothing must handle the error |
| Drain the mixer in real time into nothing | players appear to play | nothing to handle | a player seeming to play while nothing is heard is the failure lane E wanted to stop showing |

| | What changes | For | Against |
|---|---|---|---|
| **The door answers in Linux errnos (chosen)** | the C library passes answers straight to `errno` | the requests are the device's (Linux's) ABI, and so are its errors -- `ENOTTY`, `EBADFD`-class answers have no kernel error code; the Linux and native callers get the same answer from the same handler | the one native family whose negative values are not `KernelError` codes; documented on the block in `number.rs` |
| Kernel error codes, as every other native call | uniform convention | -- | lossy translation both ways, and a second error table for the C library |

| | What changes | For | Against |
|---|---|---|---|
| **One generic door, the kind passed with each call (chosen)** | `ioctl(kind, handle, request, arg, flags)` | lane D's ask -- "the same door would serve the next device"; the C library already keeps each descriptor's kind | a call that names the wrong kind for a handle is `EBADF` rather than impossible |
| Dedicated `SYS_AUDIO_*` calls | a typed native audio API | smaller surface | a second interface beside the ALSA one, and ported programs still need the ALSA one (lane E's option (b), which lanes D and E both declined) |

`O_NONBLOCK` travels as a per-call flag, as the native pipe calls choose
between their blocking and `TRY_` forms: it is the open file's, which the C
library keeps.

**Not done yet** (`known-issues.md`): capture still reads silence
(`A-SOUND-CAPTURE-HAS-NO-SOURCE`); an underrun is not reported as
`XRUN`/`EPIPE` (`A-PCM-UNDERRUN-IS-SILENT`).

**Revisit** when real hardware shows underruns at 5 ms / 64 ms (the
constants in `audio_out`), or when a second device wants the door.
