## §262 — Modern AMD graphics is out of scope until there is hardware to run it on; a driver nobody has ever started is worse than an honest gap

**Date:** 2026-08-21
**Decided by:** Operator (Claude recommended this option)
**Lane:** A

**In short:** The plan asks for a driver for modern AMD graphics cards. There is
no way to run one: the emulator we test in doesn't imitate any recent AMD card,
and this PC has an NVIDIA card. Such a driver could be written from
documentation but never once started — no picture, no error, no clue which of
its many steps was wrong. The decision is to say plainly that this OS drives
modern AMD cards through the generic fallback only, and to revisit it if
hardware ever appears. The old-AMD driver that *does* run is unaffected and
stays.

### The operator's own framing, recorded because it is the shape of the revisit

The operator's answer was "a for now, maybe someday c, but i can't afford it
right now", and then raised a third possibility worth writing down rather than
discarding: write the driver blind but **refuse to claim it works** — state
explicitly in the documentation that it was written without any hardware to test
on, and invite users (or an LLM on their machine) to debug it and send patches.

That variant is not option B, and the difference matters: B's fatal defect is
the *claim*, not the code. A roadmap that ticks "AMD support" on the strength of
code nobody has run is a claim we would have to un-make. A file that says "this
is untested, here is the documentation it was written from" makes no false
claim.

It was still not adopted, for the reason the operator themselves named: the
failure mode is a black screen, and "they'd have to figure out how to fix a
blank screen … but i guess we'd have the same problem". A contributor debugging
a modern AMD bring-up has to get through firmware upload and a long power-up
sequence with no output channel at all — that is not a task a patch-sized
contribution absorbs. If bare-metal boot lands (see §263), the calculus changes,
because then a contributor at least has serial output.

### What is settled and not in question

The old-card driver is done and running (§217). Its timing arithmetic is shared
with the newer chips, so none of it is wasted whichever way this goes. Writing
it immediately caught a genuine bug a never-run driver would have kept forever.

### If this is revisited

The trigger is hardware: a spare modern AMD card fitted to this machine, or the
Linux-host passthrough setup described in Q50 option D. Until then roadmap §3.1
records the gap rather than a promise.
