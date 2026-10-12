### A-PCM-UNDERRUN-IS-SILENT -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- a difference from Linux ALSA, found with the
pump.

**In short:** when a playing program does not write fast enough and its
stream runs dry, the card plays silence and the stream stays `RUNNING`. On
Linux the stream goes to `XRUN` and the next write fails with `EPIPE`, which
is how a player learns it stuttered and re-prepares. Here a player never
learns it; nothing breaks, but a stutter is invisible to it.

**Where:** `kernel/src/audio_mixer.rs` (`mix_output` reads what a ring has
and does not say a running stream came up short); `kernel/src/ipc/alsa_pcm.rs`
(no `XRUN` transition).

**Proper fix:** the mixer marks a stream that had fewer frames than a period
asked of it while its substream was `RUNNING` (not draining, not paused);
`alsa_pcm` reads the mark and moves the substream to `STATE_XRUN`, where a
write answers `EPIPE` until `PREPARE` -- as `snd_pcm_lib_write` does.
