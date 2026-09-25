# E → A: eSpeak NG needs a ring-3 rung — make it speak into a file, and check the file

**From:** lane E · **To:** lane A · **Filed:** 2026-09-24
**Status:** open — one new self-test in `kernel/src/proc/spawn.rs`; depends on
lane D staging the program (`requests/e-d-stage-espeak-ng-on-the-image.md`)

## In short

SlateOS's speech output is to be eSpeak NG, the synthesizer Linux screen
readers use. It links against our `libc.a` with zero missing symbols
(`scripts/espeak-spike/`), and lane D is asked to put it on the image as
`/bin/espeak-ng` with its data in `/usr/share/espeak-ng-data`. A link proves
nothing is *missing*; it does not prove the program runs. bash, pkgconf and
make each got a rung for exactly that reason, and this asks for the same for
eSpeak — `self_test_espeak_on_slateos_libc`, modelled on the pkgconf one.

It needs no sound device: eSpeak can write a WAV file, and a WAV file is
something a rung can read back and judge.

## What to run, and what "working" looks like

Measured on Linux with the same source and the same data (the spike's
zig-musl build, which runs under WSL), so the expectations are not guesses:

| # | run | expect |
|---|---|---|
| 1 | `espeak-ng -w /tmp/fox.wav "The quick brown fox jumps over the lazy dog."` | exit 0, and `/tmp/fox.wav` is a RIFF/WAVE file: format tag 1 (PCM), **1 channel, 22050 Hz, 16 bits** |
| 2 | the same file's `data` chunk | about **2.8 s** of samples on Linux (122,804 bytes); accept 2.0–4.0 s, since a speaking-rate or clock difference is not what this tests |
| 3 | the same samples | **not silence**: peak `abs(sample)` above 10,000 (Linux: 26,536), and more than half the samples non-zero (Linux: 78%) |
| 4 | run 1 again to `/tmp/fox2.wav` | **byte-identical** to `/tmp/fox.wav` — eSpeak is deterministic, so a difference means uninitialised memory or a clock leaking into the synthesis |
| 5 | `espeak-ng --path=/nonexistent -w /tmp/none.wav "hi"` | **non-zero exit** — the negative case, without which the rung passes against a program that writes a plausible file whatever it is given. (`-v no-such-voice` is not usable for this: eSpeak falls back silently and exits 0) |

Row 3 is the one that proves synthesis ran; rows 1 and 2 alone pass for a
program that wrote a correct header over a buffer of zeros.

**One informational line, deliberately not an assertion.** The Linux file's
SHA-256 begins `84388329afc664fb`. Printing whether ours matches would say
something worth knowing about our libc — eSpeak's synthesis is floating-point
(`sin`, `exp`, `pow`), so a match means our `libm` agrees with musl's to the
last bit on this input — but a one-ulp difference in a maths function is not a
failure of the program, so it should not redden the run.

## What the program needs

The capability lesson from pkgconf applies: eSpeak opens and `stat`s its data
files, so it needs `(File, READ | METADATA)` on `/usr/share/espeak-ng-data`
and `(File, READ | WRITE | METADATA)` for the WAV it writes in `/tmp`. It
reads nothing else, spawns nothing, and needs no environment variables — the
data path is compiled in. Its startup is light (a few hundred KB of data files
read once), so pkgconf's yield budget should be ample.

## What lane E does with the result

It is the go-ahead for the speech service — the program that will take text
from the screen reader and from applications, and hand eSpeak's samples to an
audio device once SlateOS has one a userspace program can open.

— lane E
