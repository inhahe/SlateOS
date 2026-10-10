## 1378. Colour management copies Chrome: skcms for pictures' profiles, Chrome's HDR handling for video

**Date:** 2026-10-09
**Lane:** F
**Decided by:** Operator ("F-Q10: A", `operator-answers/2026-10-09-open-questions-answers.2.txt`), answering `open-questions/F-Q10.md`, after: "A sounds like the best option, but I do wonder if the specs *should* have asked to drive printers' colors, and in that case, if Firefox's version could be worked to provide a reference for testing?" (`operator-answers/2026-10-09-open-questions-answers.txt`) — Claude's recommendation. Lane A answered the question in chat: A for screens now; Little CMS (lcms2) when the print path is built, since printers' colours need printer (CMYK) profiles, and Firefox leaves printer colour to the system, so it cannot be the reference there.

**In short:** pictures and videos that say which colours their numbers mean --
iPhone photos (Display P3), some cameras' (Adobe RGB), HDR video (BT.2020,
with PQ or HLG light) -- will be converted to the screen's colours before
they are shown, as Chrome converts them, pixel for pixel. Today every pixel
is shown as if it were sRGB, so P3 photos look dull and HDR video dim and
grey.

**Decision.**
- **Pictures' colour profiles**: port *skcms*, Skia's colour library (BSD,
  about 3,000 lines of C), as Chrome uses it: the ICC profiles PNG, JPEG,
  WebP, AVIF and icon files carry, converted to the screen's colour space.
- **HDR and wide-gamut video**: Chrome's handling -- its tone mapping of PQ
  and HLG to the screen, and its conversion of BT.2020's colours -- reproduced
  and checked against Chrome's pixels, as every decoder here is held to its
  reference.
- **The target** is sRGB until a screen reports its own colours (lane A's
  display driver reading the monitor's description); then the compositor's
  colour management (`known-issues/F-video-is-shown-without-colour-management.md`).
- **Printing**, when it is built (roadmap-detailed: CUPS or an equivalent),
  uses Little CMS for printer profiles; not part of this.

**Rationale.** Every decoder in `gui/imagecodec` and `gui/video` is held to
Chrome's or libavif's pixels, so a Chrome-matching colour step keeps one
standard throughout and every result testable against a reference; skcms is
small, deterministic and well tested, and Chrome's HDR handling is the one
most people see.

**What it changes in work under way.** Lane F's HDR video work (October 2026)
had chosen, autonomously, ITU-R BT.2408's EETF on maxRGB for HDR to SDR; that
choice is superseded before it was committed. The stream's HDR metadata
(transfer, mastering display, content light level), which Chrome's handling
also reads, is kept; the mapping is rebuilt to Chrome's.

**Alternatives.** B, Little CMS (lcms2): the most complete, colours slightly
different from Chrome's; C, Firefox's qcms; D, leave it.

**What it asks, and where it will be done.** `roadmap.md` (lane F): colour
management -- skcms for pictures, Chrome's HDR handling for video, the
compositor's ownership of the screen's colour space once screens report
theirs.

**How to reverse.** Each conversion sits behind its decoder's output step; a
different library replaces it there.
