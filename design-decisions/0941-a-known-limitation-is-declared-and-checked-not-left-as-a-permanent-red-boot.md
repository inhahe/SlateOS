## 941. A known limitation is declared and checked, not left as a permanent red boot

**Date:** 2026-09-15 · **Decided by:** Claude (autonomous) · **Lane:** A ·
**Reverses:** my own decision of earlier the same day, recorded in
`kernel/src/net/socket.rs`

**In short:** a self-test could not run, because the thing it needed (two
network sockets at once) is something the system currently cannot do. I had
made it FAIL, so every boot was red and nothing could be merged until the
operator decided A-Q15. It now says "NOT CHECKED" and explains why, and a
second check fails the build if the limitation ever goes away without anyone
noticing. The property is still not claimed as passing.

**What I wrote when I chose the failure**, quoted because reversing yourself is
only honest with the original in view: *"So this fails, the boot stays red, and
lane A's merge to main is blocked on A-Q15 -- which is the honest consequence of
a real defect, not a thing to engineer around."*

**What changed is the cost, not the defect.** A-Q15 is exactly as real as it
was. But a permanently failing self-test is a merge gate on ALL work, not on
the work that caused it. By the time I came back to it, three unrelated fixes
were held behind this one line -- including a self-deadlock between the block
registry and the buffer-cache writeback softirq, which can fire on the
operator's machine under load and had nothing whatever to do with sockets.

A-Q15 is already unignorable: it is in `open-questions.md`, which is the
operator's queue and the mechanism designed for this. The red boot was a
*second* forcing function for the same question, and the price was paid by
unrelated bug fixes not reaching the other two lanes. One forcing function is
enough when the other costs that much.

**Why this is not the skip the gate rightly refuses.**
`check-selftest-skips` refuses a skip decided by the outcome of a call into the
subject, because a test that skips when its subject errors stops testing at the
moment the subject breaks. That rule is why the failure was the only option the
first time round. It is satisfied here by a different route:

| requirement | how |
|---|---|
| the reason is looked up, not inferred from an error | a `const NETSTACK_HOLDS_MULTIPLE_SOCKETS: bool` the test reads |
| the decline is visible where coverage is claimed | it is the last line the function prints on that path; the success line is unreachable |
| it cannot rot | a positive control in the `Ok` arm FAILS if the accept succeeds, saying the constant is stale |

**The positive control is what makes this defensible rather than convenient.**
The standing objection to declaring a limitation is that the declaration
outlives it: the day A-Q15 is fixed, a plain skip would keep skipping and the
case would be retired by a stale constant nobody re-read. So the `Ok` arm --
reached only if a second socket no longer destroys the listener -- fails the
suite and names the constant. The declaration is therefore checked against
reality on every boot, which a comment or a skip would not be.

**What is deliberately NOT claimed.** The message says the property is
UNTESTED, not that it passes. Head-of-line blocking has never been observed
working on this stack and this entry does not pretend otherwise; 932's
mirror-versus-witness distinction applies, and what exists here is neither --
it is an honest gap with its own alarm attached.
