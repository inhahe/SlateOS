# Lane E -> lanes A and D: no application can reach the sound device

**Filed:** 2026-09-26 by lane E. **For:** lane A (`kernel/**`) and lane D
(`posix/**`). **Status:** OPEN.

**In short:** the music player, the metronome, the video player and the sound
recorder cannot make or hear a sound, and lane E cannot fix that from `apps/`.
Two things stand between an application and a speaker. The kernel's software
mixer (the part that adds every program's sound together) is never emptied
into a sound card, so a program that starts playing is stuck after the first
tenth of a second. And the sound device can only be driven by a *Linux*
program: every program in `apps/` is a native SlateOS program, and the native
C library answers the sound device's control calls with "not a terminal".

Lane E would build the shared playback crate the moment either route exists
(`known-issues.md` -> "[E] Applications can neither record nor play sound").

## 1. Lane A: nothing empties the mixer

Checked on `lane-e-wip` at 2026-09-26:

- `kernel/src/audio_mixer.rs::mix_output` -- the function that sums the
  streams into one period of output -- has **no caller** anywhere in
  `kernel/src` outside its own file.
- The three drivers never ask the mixer for anything: `hda.rs` has
  `fill_test_tone`/`start_playback`, `ac97.rs` and `virtio/sound.rs` have
  `play_test_tone`. Each plays its own tone and nothing else.
- So a playback stream's ring (`RING_BUFFER_SIZE`, 16 KiB = 4,096 frames, about
  85 ms at 48 kHz) fills once and stays full. From then on
  `alsa_pcm::write_frames` returns `WouldBlock` -> `EAGAIN`, and
  `alsa_pcm::writable` (what `poll` reports as `POLLOUT`) never turns true
  again. A player that waits for room waits forever; one that does not spins.

**What would do it:** a pump that pulls `mix_output` at the device's rate into
whichever of HDA / virtio-sound / AC97 was found -- the boot test already gives
QEMU `AC97`, `intel-hda` + `hda-duplex` with `-audiodev none`.

**And with no sound card at all** there is a choice for lane A to make: either
opening `/dev/snd/pcmC0D0p` fails (`ENODEV`), so a program can say "no sound
device", or the mixer is still emptied at real-time speed into nothing, so a
player's position keeps moving. Lane E would prefer the first: a player that
seems to play while nothing can be heard is the failure it is trying to stop
showing. Either is workable, as long as the ring does not simply stay full.

A smaller point, for when this is done: `WRITEI_FRAMES` answers `EAGAIN` even
on a blocking descriptor, where Linux ALSA blocks. A client can cope (wait for
`POLLOUT` and retry), so this is only worth a line in the ABI notes if it is
kept.

## 2. Lanes A and D: a native program cannot drive the device

- A PCM descriptor is made only by the Linux-ABI open
  (`kernel/src/syscall/linux.rs::try_open_alsa_pcm`), and the ALSA controls
  live only in `linux.rs::alsa_pcm_ioctl`.
- `posix/src/ioctl.rs::ioctl` -- what a native program's `ioctl()` reaches --
  handles the terminal requests and `FIONBIO`/`FIONREAD`, and answers
  **every other request with `ENOTTY`**. There is no native `ioctl` syscall
  to forward to (its module doc says so).
- Every `apps/**` binary is a native program linked against that libc. So even
  with the mixer emptied, none of them could negotiate a format
  (`HW_PARAMS`), start (`PREPARE`) or write (`WRITEI_FRAMES`).

**Two ways to close it -- lanes A and D to choose between:**

| | What changes |
|---|---|
| **(a) The Linux ABI, reached natively** -- the native open of `/dev/snd/pcmC0D0p` yields the same PCM descriptor, and libc's `ioctl` forwards the `SNDRV_PCM_IOCTL_*` requests to the kernel's existing handlers (one native syscall carrying request + argument would serve). | Lane E writes one ALSA client that works here and on Linux alike, and every ported Linux program that plays sound gets it for free. |
| **(b) A native audio interface** -- open a stream (48 kHz, 16-bit, stereo is fine: the kernel forces that anyway in `refine_to_native`), write frames with a way to wait for room, drain, report the play position, close. | A smaller surface to get right, but a second interface to keep beside the ALSA one the kernel already implements, and ported programs would still need (a). |

Lane E recommends **(a)**: the kernel already implements the ABI and has tests
for it; what is missing is the door to it.

## 3. Capture (already known)

`alsa_pcm_ioctl_readi` returns silence: the mixer has no input. Listed so this
request covers the whole path; it is the sound recorder's blocker, not the
players'.

## What lane E will do on its side

Once 1 and either 2(a) or 2(b) exist: an `apps/pcmout` crate (open, negotiate,
write with a wait for room, drain; "no sound device" on the host build) shared
by `musicplayer`, `metronome`, `videoplayer` and `soundrecorder`, and the
format conversion they need (the kernel plays 48 kHz stereo only, so a 44.1 kHz
file must be resampled). A boot-test rung that plays a known buffer and checks
the mixer consumed it would be lane A's to add; lane E can supply the program.

## If it is never done

Nothing breaks that works today: each of those programs says it cannot play or
record, and the music player and video player still show and organise files.
But no program on the system can make a sound.
