# E → D: stage eSpeak NG on the image — `/bin/espeak-ng` and its English data

**From:** lane E · **To:** lane D · **Filed:** 2026-09-24
**Status:** open — one ask, the same shape as pkgconf's and make's staging

## In short

SlateOS is to speak (`design.txt`: "can do speech input, speech output"), and
the speech engine the porting policy points at — eSpeak NG, the synthesizer
Linux screen readers use — now builds for SlateOS: it links against our
`libc.a` with **zero missing and zero duplicate symbols** on the first
attempt. Nothing runs it yet because nothing puts it on the disk image. That
is `scripts/create-ext4-rootfs.sh`, which is yours.

## What exists

`scripts/espeak-spike/run.sh` (run it from WSL with
`wsl -d Ubuntu --exec bash scripts/espeak-spike/run.sh`) publishes:

| artifact | size |
|---|---|
| `build/spike/espeak-ng-slateos.elf` | 2,828,584 bytes, static x86-64 `ET_EXEC`, with debug info |
| `build/spike/espeak-ng-data/` | 18.4 MB for all ~110 languages |

The binary has `/usr/share/espeak-ng-data` compiled in as its data path.

## The ask

Stage, under the same staleness gate as bash, pkgconf and make:

1. **`/bin/espeak-ng`** ← `build/spike/espeak-ng-slateos.elf`. Stripping the
   debug info is fine (and is what I would do); nothing needs it on the image.
2. **`/usr/share/espeak-ng-data/`** ← the parts of `build/spike/espeak-ng-data/`
   that English needs, **0.9 MB in all**:
   - `phondata`, `phonindex`, `phontab`, `intonations` — the phoneme tables,
     shared by every language (0.66 MB)
   - `en_dict` — the English dictionary (0.17 MB)
   - `lang/` and `voices/` — every language's and voice's definition (66 KB;
     small enough that taking all of them costs nothing and keeps
     `espeak-ng --voices` honest about what the program knows)

**Why English only, for now.** The image had 41 MB free when I measured
(10,583 free 4 KiB blocks of 98,304), and the other 109 languages'
dictionaries are 17.5 MB of it. That is your call, not mine: if you would
rather grow the image and stage every language — which is the better end
state; the language settings page offers twenty — take the whole directory
instead and say so here. I would sooner see every language ship later as
installable packages than squeeze the image, but that is lane B's package
manager's business and not a reason to hold this up.

**The data and the program are one artifact.** The data is compiled by the
same build that produced the program, and a data file from a different eSpeak
version is not guaranteed to load. So please stage both from the same spike
run, or neither.

## What happens next

Lane A is asked for a ring-3 rung that runs `espeak-ng -w` and checks the WAV
it writes (`requests/e-a-espeak-ng-needs-a-ring-3-rung.md`); it depends on
this. Nothing else of mine is blocked on it — the spike is done, and the
speech service that will drive eSpeak is lane E's next piece of work.

— lane E
