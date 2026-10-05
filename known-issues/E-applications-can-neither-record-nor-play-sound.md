### [E] Applications can neither record nor play sound -- 2026-09-25
**Status:** OPEN -- blocked below lane E (traced 2026-09-26): nothing empties
the kernel's mixer into a device, and a native program cannot drive the PCM
device at all -- items 2 and 4 below, filed as
`requests/e-ad-no-application-can-reach-the-sound-device.md`. After those, lane
E's client crate (item 1). Capture: lane A (the kernel mixer has no input).

**In short:** a program here cannot make a sound or hear one. The recorder
cannot record, the music player and the metronome cannot play, and every one of
them says so. Nothing is broken in any of them: the path from an application
to a speaker, and from a microphone to an application, is what is missing.

**What exists.** The kernel speaks the Linux sound interface: `/dev/snd/pcmC0D0p`
(playback) and `/dev/snd/pcmC0D0c` (capture), driven by the usual ALSA ioctls
(`HW_PARAMS`, `PREPARE`, `WRITEI_FRAMES`/`READI_FRAMES`), in
`kernel/src/syscall/linux.rs` and `kernel/src/ipc/alsa_pcm.rs`. A playback
stream feeds `kernel/src/audio_mixer.rs`, whose documentation routes its
output to HDA, virtio-sound or AC97. A capture stream reads
**synthesised silence** -- `alsa_pcm_ioctl_readi` says so: the mixer is
output-only.

**What is missing.**
1. **No application opens `/dev/snd` at all.** There is no client crate, so
   each program would need its own ALSA ioctl bindings. The proper fix is one
   small crate in `apps/` (a `pcmout`: open, negotiate 48 kHz 16-bit stereo,
   write frames; on the host build, "no device") that `musicplayer`,
   `metronome`, `soundrecorder` and `videoplayer` share -- and a boot-test rung
   that plays a known buffer and reads back what the mixer produced, since
   nothing on the host can observe it.
2. **The mixer's output reaches no device** (traced 2026-09-26; this said
   "unverified" before). `audio_mixer::mix_output` has no caller outside its
   own file, and the HDA, AC97 and virtio-sound drivers play only their own
   test tones. A stream's ring (16 KiB, about 85 ms) fills once and stays
   full: every write after that is `EAGAIN`, and `POLLOUT` never fires, so a
   player waiting for room waits forever. Lane A's.
3. **Capture has no source.** Until the mixer (or a driver beside it) has an
   input, `READI_FRAMES` is silence, and a recorder that took it would record
   nothing while its level meter sat still -- the failure
   `TD-C-A-RECORDER-THAT-RAN-A-CLOCK-OVER-NO-AUDIO` describes. Lane A's.
4. **A native program cannot drive the device** (found 2026-09-26). A PCM
   descriptor is made only by the Linux-ABI open
   (`kernel/src/syscall/linux.rs::try_open_alsa_pcm`), and the ALSA controls
   exist only in `linux.rs::alsa_pcm_ioctl`. Every `apps/**` binary is a native
   program, whose `ioctl()` is `posix/src/ioctl.rs` -- which answers anything
   but the terminal requests with `ENOTTY`. So item 1's crate could not
   negotiate a format even with item 2 fixed. Lanes A and D: either the native
   open and `ioctl` reach the existing ALSA handlers (lane E's recommendation,
   since the kernel already implements that interface), or a native audio
   interface.

**Until then** the recorder refuses a take with the reason on screen, and is
useful only for what is already on disk (see the soundrecorder paragraph of
`TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
