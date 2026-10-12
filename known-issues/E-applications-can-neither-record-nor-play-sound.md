### [E] Applications can neither record nor play sound -- 2026-09-25
**Status:** OPEN -- blocked on lane D (checked 2026-10-10): the C library
does not yet reach the sound device -- item 4, lane D's half of
`requests/e-ad-no-application-can-reach-the-sound-device.md`. Lane A's half is
on `main` (the mixer is emptied into a card, and the kernel's device calls
exist: items 2 and 4, design-decisions §1520), and the client crate of item 1
exists as lane C's `sound::pcm::Playback`. Capture: lane A (the kernel mixer
has no input). Until 2026-10-10 this said: blocked below lane E (traced
2026-09-26) -- nothing empties the kernel's mixer into a device, and a native
program cannot drive the PCM device at all; after those, lane E's client
crate.

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
   *2026-10-05: the crate exists, and is not lane E's to write -- lane C
   built it for the desktop's sounds as `sound::pcm::Playback` in
   `gui/sound` (`requests/c-e-the-sound-device-client-exists-in-gui-sound.md`),
   with WAV, Ogg Vorbis and Ogg Opus decoding and the conversion to the
   mixer's format; the boot-test rung exists as `audio_out`'s self-test
   (§1520). What a music player still wants is its streaming half -- decode
   as the sound is needed, and a resampler that carries its filter from one
   buffer to the next -- which lane C offers to add beside `convert`.
   Settings' System Sounds section plays through it already (2026-10-10).*
2. **The mixer's output reaches no device** (traced 2026-09-26; this said
   "unverified" before). `audio_mixer::mix_output` has no caller outside its
   own file, and the HDA, AC97 and virtio-sound drivers play only their own
   test tones. A stream's ring (16 KiB, about 85 ms) fills once and stays
   full: every write after that is `EAGAIN`, and `POLLOUT` never fires, so a
   player waiting for room waits forever. Lane A's.
   *Fixed 2026-10-02 by lane A (27b2ccfe0, §1520), on `main`: a kernel task
   keeps the first usable card fed from the mixer, 64 ms ahead.*
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
   *2026-10-02: the first, lane D's choice too. Lane A's half is on `main` --
   the device calls `SYS_DEVICE_OPEN` .. `SYS_DEVICE_CLOSE` (1119-1123),
   answering as the Linux device does. Lane D's half -- a descriptor kind for
   a device handle, `ioctl` passing the request through, `poll` asking the
   handle -- was not on `origin/lane-d` when checked on 2026-10-10.*

**Until then** the recorder refuses a take with the reason on screen, and is
useful only for what is already on disk (see the soundrecorder paragraph of
`TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`).
