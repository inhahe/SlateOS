## E-Q1 — [E] Understanding speech needs a data file bigger than the space left on the system disk. Which engine, and does its data ship with the system or install later? — Status: OPEN (raised 2026-09-24)

**In short:** SlateOS is meant to understand speech — dictation and voice
commands — as well as to speak. Every speech-recognition engine accurate enough
to be worth having needs a large data file (a "model": what speech sounds like,
learned from recordings), 40 to 150 MB for English. The disk image SlateOS
installs from has about 40 MB free today. So two answers are needed: which
engine to adopt, and whether its model is part of every installed system (the
image grows) or something a user installs when they first turn dictation on.

Speaking, the other half, is not affected: its engine, eSpeak NG, needs under
1 MB for English and is being brought up now (`roadmap.md` §5.6).

### Which engine

All three run entirely on the machine; none sends audio anywhere.

| | engine | English model | how well it hears | how it feels to use |
|---|---|---|---|---|
| **A** | whisper.cpp (MIT licence) | 75 MB ("tiny") or 142 MB ("base"); larger ones exist, to 1.5 GB | best by a wide margin, and it adds punctuation and capitals itself | text appears a second or two after you pause, a sentence at a time |
| **B** | Vosk (Apache licence) | 40–50 MB | good, noticeably behind A | words appear while you are still speaking |
| **C** | PocketSphinx (BSD licence) | about 30 MB | poor at free dictation; fine for a short list of fixed commands | words appear while you are still speaking |

- **A** — *What changes:* dictating a paragraph gives a punctuated paragraph at each pause, with few mistakes.
- **B** — *What changes:* words stream in live, with more mistakes and no punctuation.
- **C** — *What changes:* "open mail"-style commands work; dictating prose is frustrating.

### Where its model lives

| | | *What changes* |
|---|---|---|
| **1** | in the system image, which grows from 384 MB to 512 MB | dictation works the first time it is switched on, on every install, with no network |
| **2** | an optional package, offered when dictation is first switched on | the base system stays small; first use needs the package, from the network or the install media |

### Recommendation

**A with 2:** whisper.cpp, with its "base" English model (142 MB) as an
optional package offered the first time dictation is turned on, and the
other-language models the same way. Accuracy is what makes dictation usable at
all, and most people never turn it on, so they should not carry 142 MB for it.
If you would rather it were always there, **A with 1** and the 75 MB "tiny"
model is the compact version of the same choice.

**Feasibility, measured 2026-09-25:** whisper.cpp 1.9.4 links against our C
library and the C++ runtime we use with nothing missing, on the first attempt
(a quick link test, not yet a committed recipe). So A is possible today; this
question is only about which engine is right, and where its model lives. It
does not yet say how fast it runs: the virtual machine the tests boot in has
none of the wide arithmetic instructions whisper.cpp uses on real hardware, so
it will be much slower there than on a real computer.

### If never answered

Nothing breaks. Speech output goes ahead without it; speech input stays
unbuilt. Nothing gets worse with time.

**Where it bites:** `roadmap.md` §5.6 `[E] Speech input / speech output`. Option
1 is lane D's image recipe (`scripts/create-ext4-rootfs.sh`); option 2 is lane
B's package manager (`userspace/pkg`).
