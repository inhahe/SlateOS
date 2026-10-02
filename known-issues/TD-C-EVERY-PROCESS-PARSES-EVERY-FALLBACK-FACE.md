## TD-C-EVERY-PROCESS-PARSES-EVERY-FALLBACK-FACE

**Date:** 2026-09-26. **Lane:** C. **OPEN.**

**In short:** every program that draws text reads and keeps its own copy of
each fallback font -- about 7 MB on SlateOS as shipped (Noto Sans and Noto
Color Emoji), and tens of megabytes more for each writing system installed
(a CJK font is 15-20 MB). Twenty programs open is twenty copies. Nothing is
wrong on screen; it is memory.

**Where.** `guitk::text::install_fallback_faces` loads each family with
`FontDb::load`, which is `fs::read` then `osfont::Face::parse` -- a `Face`
owns its bytes. The process-wide cache calls it once, on first use of text
(`text::cache`). The UI and fixed-pitch faces have always worked this way
too; fallback faces are simply bigger and more of them.

**The proper fix.** A face backed by shared memory rather than a private
buffer: map the font file read-only (`Face::parse` over a mapping), so every
process's copy is the same physical pages -- which is what every other
desktop does. That is an `osfont` change (lane F: `Face` holding a borrowed
or mapped slice, not a `Vec<u8>`) plus a mapping primitive on SlateOS. The
alternative -- parsing a fallback face only when a character first needs it
-- saves the memory in a process that never draws an emoji, but moves a
multi-megabyte read into the middle of a frame, and needs `osfont` to take
faces lazily.

**Why it has not bitten.** The OS image carries no fonts yet
(`requests/f-cd-the-os-image-ships-no-fonts-...`), so today nothing is
loaded there at all.
