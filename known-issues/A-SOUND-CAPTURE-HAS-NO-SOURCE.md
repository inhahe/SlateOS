### A-SOUND-CAPTURE-HAS-NO-SOURCE -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- the recording half of lane E's
`requests/e-ad-no-application-can-reach-the-sound-device.md` (section 3).

**In short:** a program that records -- the sound recorder -- reads silence.
The capture device opens and answers every request, but nothing from a
microphone reaches it: the mixer only mixes outward.

**Where:** `kernel/src/ipc/alsa_pcm.rs` `read_frames` (zero-fills);
`kernel/src/hda.rs` sets up no input stream.

**Proper fix:** an input stream on the card (HDA's input converter and an
input stream descriptor; AC'97's PCM In channel), pumped by `audio_out` into
a capture ring per open capture substream, which `read_frames` drains --
waiting for data on a blocking descriptor as playback waits for room.
