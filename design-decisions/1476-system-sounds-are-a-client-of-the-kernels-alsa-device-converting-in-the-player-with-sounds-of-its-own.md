## 1476. System sounds are a client of the kernel's ALSA device, converting in the player, with sounds of its own

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a new crate, `gui/sound`, plays a short sound -- a chime when a
message arrives, a crumple when the recycle bin is emptied -- through the
kernel's sound device, the way a Linux program plays through ALSA. It reads
WAV and Ogg (Vorbis or Opus) files, turns them into the one format the
kernel's mixer takes, and makes sounds of its own for a theme that has none,
so a new system is not silent. It cannot be heard on SlateOS yet: native
programs have no way to the device and the kernel's mixer is not yet emptied
into a sound card, both the kernel's to add; it plays the day they are.

**Where:** `gui/sound/src/` -- `pcm.rs` (the device protocol, and the device
through the C library), `convert.rs` (channels, rate, samples), `decode.rs`
and `wav.rs` (files), `builtin.rs` (the synthesized sounds), `lib.rs`
(`play`, `play_on`, `render`). Asked for by `roadmap-detailed.md` §3.4
(*System sounds*).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Each program plays through the device itself** | a sound server process that programs send sounds to | The kernel mixes every stream already (`kernel/src/audio_mixer.rs`); a server would be a second mixer in front of the first, and one more process to be running before anything can make a sound. | Each program carries the decoders it uses. |
| **The player converts** -- fold to stereo, resample to 48 kHz, dither to 16 bits | asking the kernel to | The kernel takes exactly 48 kHz, 16-bit, stereo, and answers any other request with that (`refine_to_native`); a conversion stage in the kernel is not there, and is no improvement on doing it where the samples are. | A 44.1 kHz file costs a resample on every play -- a few milliseconds for a second of sound. |
| **Sounds synthesized here** for a theme without its own | sound files shipped on the image | No recordings to choose, license or package; a fresh system has a sound for every common event; a theme that ships its own replaces them event by event. | They are chimes and clicks, not a sound designer's work. |
| **The C library's `ioctl`**, not the system call | a raw `syscall` with Linux's number, as the compositor's DRM code does | Which table a `syscall` reaches is the process's (`AbiMode`), and in a native program Linux's `ioctl` is `clock_adjtime`. Through the C library it is right in either kind of process, and fails cleanly where the device is not reachable. Lane F's raw calls are written up in `requests/c-f-linux-syscall-numbers-in-a-native-program-reach-other-calls.md`. | -- |
| **At most four at once; a fifth is not played** | a queue | An event's sound is for its moment: a queue would ring out long after it. | A burst of events plays some of its sounds. |
| **A ring that stops draining is given up on** after 500 ms | waiting on it | With no sound card behind the mixer -- SlateOS today -- the ring fills once and stays full; a player that waited would wait forever. | A stalled device costs half a second of a thread before the sound is dropped. |

**What is not done.** Choosing a sound for an event -- the sound theme
(`theme.sounds`), the user's on/off and volume -- is the appearance crate's,
and playing them at the shell's events is the desktop's; both follow. And
the device itself is the kernel's: `requests/e-ad-no-application-can-reach-the-sound-device.md`.
