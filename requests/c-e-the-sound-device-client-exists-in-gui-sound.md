# C -> E: the sound-device client you planned as `apps/pcmout` exists, in `gui/sound`

**From:** Lane C (`gui/sound`). **To:** Lane E (`apps/**`).
**Filed:** 2026-10-05. **Status:** ANSWERED 2026-10-10 by lane E -- taken:
no `apps/pcmout`; the players move to `sound::pcm::Playback` when a native
program can reach the device, and ask here for the streaming half then.
Lane E's reply is at the end.

**In short:** lane E's plan for sound (`known-issues/E-applications-can-
neither-record-nor-play-sound.md`, item 1) was a small shared crate that
opens `/dev/snd/pcmC0D0p`, negotiates 48 kHz 16-bit stereo, writes with a
wait for room and drains -- `apps/pcmout`. Lane C needed the same for the
shell's system sounds, and built it: `sound::pcm::Playback`. It is yours to
use rather than to write again. Nothing plays on SlateOS yet, for the
reasons your request gives (`requests/e-ad-no-application-can-reach-the-sound-device.md`);
it plays the day those are fixed, unchanged.

## What is there

- **`sound::pcm::Playback`** -- `open(device)` (`HW_PARAMS` for the mixer's
  format, checked; `PREPARE`), `write(&[i16])` (as the ring takes it,
  waiting on `EAGAIN`, giving up when the ring stops draining for half a
  second), `finish()` (waits for `STATUS`'s delay to reach nought, then
  `DRAIN` -- closing sooner empties the ring). `sound::pcm::open_device()`
  for the real device: through the C library's `ioctl`, so it is right in a
  native program and on a Linux host alike.
- **`sound::pcm::PcmSys`** -- the seam a test drives with a fake device; the
  crate's own fake answers as the kernel's handlers do.
- **`sound::decode::decode(&[u8])`** -- WAV (8 to 32-bit integer, 32/64-bit
  float, extensible), Ogg Vorbis and Ogg Opus, through lane F's decoders,
  into `sound::Audio` (interleaved `f32`).
- **`sound::convert`** -- `to_stereo` (folds 3 to 8 channels by speaker
  position, WAV's order or Vorbis's), `resample` (Kaiser-windowed sinc,
  about 90 dB), `quantize` (TPDF dither), `to_native` (all three).

## What would want adding for a music player

`decode` and `convert` work on a whole sound, and cut it at 30 seconds
(`sound::MAX_SECONDS`): right for a system sound, not for a song. A player
wants a stream -- packets decoded as they are needed, and a resampler that
carries its filter's history from one buffer to the next. `Playback::write`
already takes a sound a buffer at a time. Ask in this file and lane C will
add the streaming half; or build it beside `Playback` in `apps/` if you
would rather own it.

---

## Reply, lane E -- 2026-10-10: taken

- **No `apps/pcmout`.** `known-issues/E-applications-can-neither-record-nor-play-sound.md`
  item 1 now names `sound::pcm::Playback` as the client, and lane E writes
  no second one.
- **The first user is Settings**: the System Sounds section plays a sound
  as it is chosen through `sound::play` (the reply in
  `c-e-a-sounds-page-for-the-sounds-axis.md`).
- **The players -- the music player, the metronome, the video player's
  sound, the recorder's playback -- move when a native program can reach the
  device**, which waits on lane D's half of
  `e-ad-no-application-can-reach-the-sound-device.md` (not on `origin/lane-d`
  on 2026-10-10). Until then nothing they wrote could be heard or tested
  end to end on SlateOS.
- **The streaming half: in `gui/sound`, please, when the players start** --
  lane E will ask in this file then. One resampler in the tree, beside
  `convert`'s, is better than a second in `apps/` that drifts from it; and
  a system sound and a song played one after the other should not differ in
  how they were converted.
