# C -> A, B, F -- the operator answered A-Q14, A-Q15, B-Q8, F-Q1 and F-Q2 in lane C's chat

**From:** Lane C. **To:** Lanes A, B and F.
**Filed:** 2026-09-27. **Status:** OPEN -- each addressee records its own
answers in its own way; this file is the relay's record, not a decision.

**In short:** on 2026-09-27 the operator answered several lanes' open
questions in one message to lane C. Lane C relayed each lane's part at once --
to lane A by message (acknowledged: "saved verbatim in lane A's plan file"), to
lanes B and F on the notice board, their sessions being down at the time. The
answers are copied below, verbatim, so they are on record in the tree.
Nothing here is lane C's to act on.

## Lane F

> F-Q2: do vp9. if possible, find or write hardware encoders and decoders for
> it (preferably find), and also have cpu fallback either way - multithreaded
> according to the number of cores in a user's system.
>
> F-Q1: AVIF: yes. HEIC: i don't really understand 'obliges you to let users
> replace that library with their own build', what could be the problem with
> that? seems like something trivial to implement.

F-Q1 is answered for AVIF; for HEIC the operator asks what, concretely, would
be hard about letting users replace the library.

## Lane A

> A-Q14: A
>
> A-Q15: I'd like to actual empirical tests, using real world conditions,
> including some extreme ones but not too unrealistic, to see what's worse -
> the slowness of A or the memory consumption of B. But I don't know if that's
> possible under QEMU because it runs more slowly than realtime. If it's not,
> then log it somewhere that we need to test this once the OS is tested on
> bare metal. Another possibility would be to have the OS support both A and B,
> and if B runs out of memory, then drop back to A, ideally with some
> connection shuffling so that the ones that need the speed of B the most get
> B, unless this possibility just isn't a good option.

## Lane B

> B-Q8: I want a way to ask the terminal how wide it will draw something as
> part of the protocol. It will be an addition purely for programs made for
> Slate OS, so it won't interfere with POSIX or whatever, is that fine?
> Regarding which table to keep, you say that bash and GNU tools will not be
> running on Slate OS, only reimplementations, so does that not mean that,
> regardlress of which we keep, nothing will show up improperly (because our
> own implementations will naturally know the correct table to use)? If
> that's the case, just do whichever looks the best, or whichever you want.
> Though one thing that concerns me is the decision to make the renderer obey
> the table rather than vice versa--what if the glyph it's trying to render
> doesn't agree with the table in the current font? Wouldn't you either get a
> glyph printed too wide or too narrow?

The operator asks two things back: whether the width-query addition to the
terminal protocol is fine, and the renderer-versus-table concern.

**Recorded by lane B (2026-09-27):** `design-decisions.md` §1042, which
quotes this answer and replies to both follow-ups; the width query is
`requests/b-c-the-terminal-should-answer-how-wide-it-will-draw-text.md`,
answered by lane E's terminal (`OSC 7730`).

## Lane C's own

The same message answered C-Q11, C-Q24, C-Q25 and C-Q26 (and, earlier the same
day, C-Q30); those are written up in `design-decisions.md` §1415-§1418 and in
`open-questions.md`'s resolved index.
