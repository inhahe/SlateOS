## 1609. The compositor draws every program at the display's scale; an element may carry its own density

**Date:** 2026-10-12 &middot; **Decided by:** Operator (Claude recommended A; the operator asked whether B is the better design long-run, what other systems do, and whether a program could choose per element; after lane A's answers, the operator chose A) &middot; **Lane:** C, with F

**In short:** on a display set to 150% or 200%, the desktop's own parts grow
but every program's contents stay at 100%
(`known-issues/TD-C-PROGRAMS-DRAW-AT-ONE-SCALE-WHATEVER-THE-DISPLAYS-IS.md`).
Programs hand the compositor a list of shapes and text, so the enlarging can
happen there, once, for every program (A), or in each program (B)
(`open-questions.md` C-Q34). The operator chose A, with the per-element
control they asked about: text and shapes scaled by the compositor, an image
able to say it is already at a higher density, and a surface able to ask for
the screen's real pixels.

**The answers, verbatim.** In `operator-answers/2026-10-09-open-questions-answers.txt`:

> C-Q34: Is B a better option if we don't worry about the fact that we have
> to change two hundred existing programs? Because I think having the best
> option in the long run is more important. What do other OSs do? As for "so
> a program that wants every physical pixel must say so," shouldn't it be able
> to say to scale or not scale any give screen element? So, for example, text
> could still be automatically scaled while an image it displays isn't.

And after lane A's answers, in chat (`operator-answers/2026-10-09-chat-to-lane-a.txt`) and in `open-questions/answers.2.txt`:

> C-Q34: A

**Lane A's answers, on which the operator chose:**

- **A is the better design on its merits, not only the cheaper one.** Layout
  is resolution-independent by construction; the pointer is mapped once; a
  window moving between displays of different scales simply redraws; and
  since programs hand over drawing lists, text and shapes stay sharp at the
  screen's resolution. B is Win32's model and the source of Windows'
  half-scaled programs -- WPF and WinUI moved to device-independent units.
- **Other systems:** macOS lays out in points and draws `@2x` images 1:1,
  rendering fractional scales at 2x and downsampling; Android lays out in dp;
  browsers in CSS pixels, with `srcset` images and a canvas sized by
  `devicePixelRatio`. All lay out in logical units with the system
  multiplying.
- **Per element, yes -- the browser model:** an image element carries its
  own density, so a 2x picture for a logical rectangle is drawn 1:1 at 200%;
  and a surface can declare physical pixels (a game, a video, a photo shown
  at 100%).

**What it obliges:** the compositor draws each program's list at the
display's scale, divides the pointer before it reaches the program, takes an
image's density from the image, and lets a surface ask for physical pixels
(lane F, told 2026-10-12). The toolkit's text size (§1474) stays separate: it
enlarges text and what is laid out round it, on any display. The desktop
draws its own surfaces as programs do once the compositor scales them, and
stops scaling them itself (lane C).
