# Fuzzing lane F's decoders

Coverage-guided fuzzing (libFuzzer, through `cargo fuzz`) of everything in
lane F that reads a file a stranger made: every picture format `imagecodec`
opens, every video and sound file `videocodec` plays, the subtitles it reads
(SubRip, ASS, SSA, WebVTT; MP4's timed text; Blu-ray's, DVD's and DVB's
pictures), the Matroska and MP4 demuxers on their own, and the VP8 and VP9
decoders on raw packets. A decoder here must return an error for any input
it cannot use. A panic, a hang or a runaway
allocation is a bug: the program that opened the file goes down with it,
whether that is a thumbnailer, the image viewer or the video player.

The unit tests and fixture suites hold each decoder to its reference on the
inputs they name. This harness asks about the inputs nobody named.

## Running it

In WSL, with a nightly toolchain (`rustup toolchain install nightly`) and
`cargo install cargo-fuzz --locked`:

    python3 setup.py "/mnt/e/visual studio projects/os-lane-f"
    bash run.sh 1200        # seconds per target; all eight in parallel

`setup.py` does three things:

- It copies the decoder crates and every crate they reach by `path` into
  `~/fuzz/tree`, under a workspace root that holds only this workspace's
  lints. cargo-fuzz needs nightly and a workspace of its own, and the real
  one has neither to spare.
- It installs the harness (`harness.toml`, `fuzz_targets/`, and each
  target's dictionary of the tokens its parsers read, `NAME.dict`) as
  `~/fuzz/tree/fuzz`.
- It builds seed corpora from the crates' own test fixtures. The VP8 and VP9
  packet targets take their seeds from the test vectors, remuxed to runs of
  length-prefixed packets with `ffmpeg`; the subtitle target from
  `videocodec`'s subtitle fixtures -- text, timed text, PGS and VobSub
  pictures -- with `subtitles.dict`'s markup -- ASS override tags, SRT and
  WebVTT tags, cue settings -- PGS's segment headers, VobSub's control
  commands, and DVB's segment headers and pixel codes for its mutations; the
  MP4 target from the `mp4` crate's
  fixtures, ffmpeg's files and the hand-written ones that exercise each
  table, with `mp4.dict`'s box types and the counts at the edges of FFmpeg's
  arithmetic.

`run.sh` runs each target at low priority for the given time. It builds with
`-O` and no debug assertions, as a release build runs, so anything it finds
also happens in what ships. Each run has a 4 GB memory limit and a 20-second
limit per input, so a hang or a runaway allocation counts as a finding.
`image` and `video` reach `rav1d`'s `unsafe` code, and run under the address
sanitizer; the six all-safe targets run without it, at about twice the
speed. A finding lands in `~/fuzz/tree/fuzz/artifacts/<target>/`.

## When it finds something

Reproduce with `cargo +nightly fuzz run -O <target> <artifact>`, then fix the
crate in the lane F worktree. Copy the artifact into that crate's tests as a
regression test (each crate's `tests/` already holds damaged files of its
own), and run `setup.py` again before the next round so the fuzzer sees the
fix.

## What it has found

| Round | Per target | Inputs run | Found |
|---|---|---|---|
| 2026-10-05 | 20 minutes, all six with the address sanitizer | image 78k, video 19k, sound 34k, matroska 418k, vp8 38k, vp9 16k | nothing |
| 2026-10-05 | 30 minutes, `subtitles` alone (its PGS and VobSub readers new), no sanitizer | subtitles 392k (stopped at 12 minutes by the find) | an MP4 timed-text track claiming 3.9 billion samples of one size: `mp4` asked for 46 GB and was killed (fixed: design-decisions §1364; `mp4/tests/data/found_tx3g_claims_billions.mp4`) |

That round reported one input as slow: a 640x480 AVIF taking over 10
seconds, with six fuzzers sharing a busy machine. On its own it ran in 3.9
seconds with the sanitizer, and in 0.35 seconds without it, for three
decodes. That is the sanitizer's cost inside `rav1d`, not the decoder's.

A round longer than twenty minutes is still to run. It loads every core
WSL has, so run it when no boot test is waiting for the machine.
