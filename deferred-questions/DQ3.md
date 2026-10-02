## DQ3 (was C-Q18) — When fullscreen is shown without being copied, what happens to the mouse pointer over it? — ANSWERED 2026-09-27, `design-decisions.md` §1334

*The operator answered it before the trigger fired, and answered the future
case too: the pointer is always shown -- drawn on the presenter's copy today,
and by the display's hardware cursor plane once a screen is shown without
copying. Lane F wrote it up as §1334. Kept here for its history, and so
the number is not reissued; there is nothing left to decide.*

*(Was `open-questions.md` C-Q18, filed by lane C on 2026-09-13. Deferred
2026-09-25 by lane C at lane F's request,
`requests/f-c-c-q18s-premise-changed-the-pointer-is-drawn-over-fullscreen-at-no-cost.md`;
lane F's reasoning is in `design-decisions.md` under the pointer's design --
search for `POINTER_OVER_DIRECT_SCANOUT`.)*

**In short:** the question was what the mouse pointer should do over
fullscreen video and games, because drawing a pointer seemed to mean giving up
the shortcut that makes fullscreen cheap. Lane F has since built the pointer as
a layer laid over the picture at the moment it is shown -- the way a graphics
chip's cursor plane works -- and every way SlateOS puts a picture on a screen
today copies a fullscreen picture anyway, so drawing the pointer onto that copy
costs nothing. The pointer is drawn everywhere, fullscreen included, and a game
that wants no pointer asks for a hidden one over its own window. There is
nothing to trade, so nothing to decide.

**Trigger:** a presenter that shows a fullscreen window's buffer directly --
without copying it (true direct scanout, or a graphics chip's own planes) --
exists. Then the pointer can no longer ride on the copy, and the question below
is real again: software pointer over a copied frame, no pointer over
fullscreen, or the chip's cursor plane.

**The question as it was put, for when it returns** (the options are unchanged;
only their costs moved):

> ## C-Q18 — [C] Nothing draws the mouse pointer. When we start, what happens to fullscreen video and games? — Status: OPEN
>
> **In short:** SlateOS does not draw a mouse pointer. On the development
> machine you see Windows' arrow, borrowed from the host; on real hardware there
> would be no pointer at all. The system already works out *which* pointer to
> show — an I-beam over text, arrows on a window edge — and then draws none of
> them. Starting to draw one is straightforward except in a single case:
> fullscreen video and games currently take a shortcut that skips drawing
> altogether, and a pointer cannot be painted on top of a frame that is never
> painted. What you are choosing is what happens in that case.
>
> **Glossary.** *Direct scanout* is the shortcut: when one window covers the
> whole screen and is fully opaque, its picture is handed to the display exactly
> as the program drew it, with no copying. It is what makes fullscreen video and
> games cheap. A *hardware cursor* is a pointer the graphics chip draws for
> itself, from a small image the display controller holds separately — it costs
> nothing per frame and does not disturb the picture underneath.
>
> **Where it bites:** `gui/compositor/src/lib.rs` — `compose_frame`'s
> `direct_scanout_window` bypass, and `cursor_shape`, which is computed on every
> pointer move and read by two tests.
>
> **What is already true, measured 2026-09-13:** `CursorShape` has ten members
> and the compositor picks the right one continuously. Across all three
> presenters there is exactly one line of cursor code — `LoadCursorW(IDC_ARROW)`
> in the Windows host window class — so the shape is chosen and discarded. The
> kernel has cursor-plane support (`kernel/src/drm/`); the compositor's DRM
> presenter never reaches for it.
>
> **The options**
>
> **A. Draw the pointer in software, always — fullscreen loses its shortcut.**
> *What changes:* the pointer appears everywhere, and fullscreen video and games
> go back to being composited frame by frame.
> For: one code path, correct on every backend, and the pointer is never missing.
> Against: it spends a measured performance feature on a 32×32 image. The
> shortcut exists because copying a 4K frame is expensive, and this would pay
> that cost on every frame of every film.
>
> **B. Draw it in software, except over fullscreen content.**
> *What changes:* the pointer appears everywhere except on top of a fullscreen
> video or game, where it disappears.
> For: keeps the shortcut, and for games it is arguably *right* — a game hides
> the pointer itself. Costs nothing new.
> Against: a fullscreen video player with on-screen controls becomes unusable,
> because you cannot see what you are pointing at. "The pointer vanishes
> sometimes" is a hard thing for a user to form a rule about.
>
> **C. Ask the graphics chip to draw it, with software as the fallback.**
> *What changes:* the same as A from the user's side — a pointer that is always
> there — with fullscreen keeping its shortcut on real hardware.
> For: it is what the hardware is for, and it is the only option where nothing
> is given up. The kernel already has the plane support.
> Against: the most work by a wide margin, and the development host has no such
> plane, so the software path has to exist anyway and B or A is what a developer
> would see. Two paths mean the one you test is not the one that ships.
>
> **My recommendation: C, built as B first.** The software renderer is needed
> either way — it is the fallback, and it is what the dev host will use — so the
> first commit is the same under all three answers. The question is only what
> happens when it meets a fullscreen window, and B is a safe place to stand while
> the hardware path is built, because it is the one answer that gives nothing up
> today. What I would not do is A: spending direct scanout permanently, to solve
> a case that C solves properly, is the kind of trade that is easy to make and
> hard to take back.
>
> **If it is never answered:** there is no pointer on real hardware and Windows'
> arrow on the development host, which is also what makes the current state easy
> to miss. Three settings stay inert — `cursor_size`, `cursor_scheme` and the
> whole `CursorShape` vocabulary — and every accessibility question about pointer
> size stays unanswerable. Nothing degrades with time; it simply does not exist.
