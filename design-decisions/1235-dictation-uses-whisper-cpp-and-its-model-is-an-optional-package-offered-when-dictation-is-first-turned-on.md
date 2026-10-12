## 1235. Dictation uses whisper.cpp, and its model is an optional package offered when dictation is first turned on

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** E

Answering E-Q1 with option **A with 2**. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("E-Q1: Claude's
recommendation").

**In short:** when SlateOS understands speech -- dictation and voice commands --
it will use whisper.cpp, the most accurate of the engines that run entirely on
the machine. Its English data file (142 MB, the "base" model) is not part of
every installed system: it is offered as a package the first time someone turns
dictation on, and other languages' models the same way. People who never dictate
never carry it.

**The alternatives not taken:** Vosk (words appear while you speak, but with
more mistakes and no punctuation) and PocketSphinx (fine for a short list of
commands, poor at prose); and putting the model in the system image, which would
have grown it from 384 MB to 512 MB for a feature most people never switch on.

**What it obliges.**

- whisper.cpp built against our C library and C++ runtime, as a library in the
  sysroot: lane D's (the toolchain and the image recipe). It linked on the first
  attempt on 2026-09-25, as a quick test, not yet a committed recipe.
- The model as a package the package manager can fetch or install from the
  install media: lane B's package manager, once the port exists.
- The dictation front end -- the switch, the first-use offer, the text going
  into the focused program -- is lane E's, under `roadmap.md` §5.6 `[E] Speech
  input / speech output`.

Speaking (eSpeak NG, under 1 MB for English) is unaffected and goes ahead on its
own.
