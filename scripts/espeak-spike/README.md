# espeak-spike — does eSpeak NG cross-compile for SlateOS?

The speech-output half of the roadmap's **`[E]` Speech input / speech output**
(§5.6; `design.txt`: "can do speech input, speech output — exceptions to 'no
AI'"). Written as the first step `roadmap-detailed.md`'s "Porting vs.
Reimplementing" policy asks for: try the port before writing a line.

**Result, 2026-09-24: eSpeak NG 1.52.0, unmodified, links against SlateOS's own
`libc.a` with zero undefined and zero duplicate symbols, on the first
attempt.** A 2,828,584-byte static x86-64 `ET_EXEC` (with debug info). That is
a link, not a run — see "What is not known yet".

## Why eSpeak NG

It is the mature implementation the porting policy points at: the formant
synthesizer the Linux screen readers speak through (Orca, via
speech-dispatcher), around 110 languages, and small — the program is under
3 MB and English needs about 2 MB of data. Formant synthesis sounds synthetic
next to a neural voice, and it is also what screen-reader users commonly choose
on purpose, because it stays intelligible at very high speaking rates and
starts speaking with almost no delay.

**Licence:** GPL-3.0-or-later (with Apache-2.0 and BSD-2 parts). The same
licence as the GNU bash, make and coreutils this tree already ships, and it is
built as its own program, so the licence stays with it; nothing links it into
a program of ours.

## Running it

From the worktree, in WSL:

```sh
wsl -d Ubuntu --exec bash scripts/espeak-spike/run.sh
```

`--exec`, not `--`: the latter hands the command line to a shell that re-parses
it, and a `$var` inside a `bash -c '…'` then vanishes.

Needs `cmake` in WSL (eSpeak NG 1.52 builds with CMake, not autotools) and the
pinned zig, which `scripts/lib/worktree.sh` fetches. The source is pinned there
too — `SLATE_ESPEAK_VERSION`/`SLATE_ESPEAK_SHA256`, with the two packagers'
attestations the pin rests on.

It publishes, for `scripts/create-ext4-rootfs.sh` to stage:

| artifact | what |
|---|---|
| `build/spike/espeak-ng-slateos.elf` | the program, relinked `-nostdlib` against `libc.a` |
| `build/spike/espeak-ng-data/` | its phoneme tables, voices and dictionaries, 18 MB for all languages |

The two are one artifact: the data is compiled by *this* build's own
`espeak-ng` (a zig-musl static binary, which runs under WSL), and they are
replaced together.

## How it is configured, and why

| option | why |
|---|---|
| `USE_LIBPCAUDIO=OFF` | no sound device: the program writes a WAV file (`-w FILE`) or raw samples to standard output (`--stdout`), and cannot open audio hardware at all |
| `USE_ASYNC=OFF` | the asynchronous API is a thread feeding pcaudiolib; with no audio device there is nothing to feed |
| `USE_MBROLA=OFF` | MBROLA is a separate, non-free diphone synthesizer |
| `USE_LIBSONIC=OFF` | sonic speeds speech past ~450 words a minute; an optional dependency this tree does not have |
| `USE_SPEECHPLAYER=OFF` | speechPlayer is C++ and optional; the Klatt formant synthesizer beneath it is the one eSpeak speaks with |
| `CMAKE_INSTALL_PREFIX=/usr` | compiles in `/usr/share/espeak-ng-data` as the data path, which is where the image is to stage it |

## What is not known yet

- **Whether it runs.** Linking proves nothing is *missing*; it does not prove
  that eSpeak's file handling, its memory use or its stdio behave on SlateOS.
  The test that would is a ring-3 rung like bash's and pkgconf's: run
  `espeak-ng -w /tmp/x.wav "…"` and check the file is a well-formed RIFF/WAVE
  of plausible length with sound in it. That rung is lane A's to write
  (`kernel/src/proc/spawn.rs`), and staging the program on the image is lane
  D's (`scripts/create-ext4-rootfs.sh`); both are asked for in `requests/`.
- **Playing it.** Nothing on SlateOS can make a sound from a userspace program
  yet. The kernel side of an ALSA-compatible playback device is in progress
  (`kernel/src/audio_alsa.rs`), and a speech service would feed it. Until then
  speech output is a file, which is also exactly what the rung can check.
