## 1334. The mouse pointer is always shown: drawn on the presenter's copy today, by the display's cursor plane when a screen is shown without copying

**Date:** 2026-09-27
**Lane:** F
**Decided by:** Operator ("Claude's recommendation"), answering
`open-questions.md` C-Q18, which lane C filed and lane F acts on; lane F's
code since 2026-09-24 (§1301) already drew the pointer everywhere. The
recommendation was C, built as B first: the display controller's hardware
cursor, with software as the fallback.

**In short:** the mouse pointer never disappears, including over fullscreen
video and games (unless the program itself hides it over its own window).
Today every way SlateOS shows a picture copies it to the screen, and the
pointer is painted onto that copy for free. When a future display path shows
a fullscreen program's picture without copying it, the pointer moves to the
graphics chip's own cursor layer (the kernel's `SYS_DRM_CURSOR_SET`/`MOVE`).
The software pointer stays as the fallback wherever there is no such layer.
Neither costs a fullscreen program its shortcut.

**Pointers that show on light and on dark.** The operator also asked for a
pointer set visible on both light and dark scenes. Every pointer already has a
body and an outline of the opposite shade, and the outline is the body grown by
a pixel or two, so it stays exactly around it at every size. The Default scheme
is a white body with a black outline (Windows' convention); Inverted is a black
body with a white outline (the Mac's). Both show on either background. The
operator also named, as an alternative, a pointer XORed with what is behind it.
That is not built, because the outlined schemes already do what it is for.

**How to reverse.** The policy is one condition in the compositor's
`Server::show` and one per presenter.
