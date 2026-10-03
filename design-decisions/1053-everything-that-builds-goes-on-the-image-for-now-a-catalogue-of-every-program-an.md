## 1053. Everything that builds goes on the image for now; a catalogue of every program, and the question of what the OS is for, follow

**Date:** 2026-09-27
**Lane:** B
**Decided by:** Operator (answering B-Q21: option A for now, and asking for a
catalogue of the programs, options for "what this OS is for", and a rule
that every program is recorded where everyone can find it). Relayed
verbatim through lane F's session.

**In short:** most programs we have written were built and tested but never
put on the disk image that boots, because together they did not fit. The
image is to grow and carry everything that builds. Then the operator wants a
list of every program with a line on what it does, followed by options for
what SlateOS is for -- each option with the programs it would drop -- so the
choice of what ships can be made deliberately. And programs must stop being
undiscoverable: a new program is recorded where every lane looks.

**The operator's answer, verbatim:**

> Go with A for now, but create a list of all the 276 programs along with a
> short description for each, and at the end, give me options for your
> question of "what this OS is for" and maybe with some implications as to
> which option implies which programs would be removed. Also, were you
> saying that these 276 programs are not even easily discoverable? We
> should have a rule somewhere that if you make a program, record it
> somewhere so everybody knows it exists.
> As for the second question, I guess there's no point in having both a
> fastpy and a Rust implementation of anything. Wait, yes there is. We may
> determine that the Rust implementation is better and make that the stock
> install, but the user may find Python much easier to edit. And vice
> versa, they may prefer Rust for some reason even if we think the Python
> version is better. Though another option is to keep the alternative
> versions in the repo but not included in the OS distribution.

**What follows:**
1. The image: raising `IMG_SIZE` and staging every binary that builds is the
   rootfs recipe's (lane D's), and is requested from them.
2. The catalogue: a generated list of every program the workspace builds,
   with a one-line description each, and at its end the options for what
   SlateOS is for, each with what it would drop. The options go into
   `open-questions.md` as a new question.
3. The rule: the catalogue is the place a program is recorded, and a gate
   checks every binary the workspace builds appears in it, so the rule
   cannot be forgotten.
4. On keeping both implementations: the operator's second paragraph is the
   answer `deferred-questions.md` DQ1 was waiting for in part -- both may be
   kept, the alternative possibly in the repository only -- and is copied
   there.
