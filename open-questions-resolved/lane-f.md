## Resolved — lane F

- F-Q2 Remote desktop's video fallback: which video format? — resolved
  2026-09-27 (1332): **VP9**, with hardware encoders and decoders where they
  can be found, and a software fallback threaded across every core.
- F-Q1 iPhone photos (HEIC): may SlateOS include a patented decoder? —
  resolved 2026-10-09 (1370): **B**, a separate one-click install; the base
  system carries no HEVC code (HEVC video follows it). Its AVIF half was §1333.
- F-Q3 Screenshots: how does a program get permission to read the screen? —
  resolved 2026-10-09 (1371): **C with A**, the person's key press or pick is
  the permission; a program capturing on its own schedule asks, with the
  operator's three choices (once, for its run, always), "always" kept in
  A-Q26's grant store (§1568).
- F-Q4 AVIF at half a browser's speed: assembly or Rust? — resolved
  2026-10-09 (1372): **A**, rav1d's hand-written assembly, built with NASM.
- F-Q5 Newer processor instructions behind run-time checks? — resolved
  2026-10-09 (1373): **A**; the operator's note (drop the plain copies once
  SlateOS targets x86-64-v3) is in `deferred-questions/`.
- F-Q6 Remote desktop: how does another computer prove it may connect? —
  resolved 2026-10-09 (1374): **B**, a PIN pairing as Chrome Remote Desktop
  does; turning it on also sets up a dynamic DNS name, the router's port and
  a certificate, and the viewer runs in a browser on any screen size.
- F-Q7 The MP4 reader translated from FFmpeg: keep it? — resolved 2026-10-09
  (1375): **A**, kept, under FFmpeg's LGPL; not tied to D-Q6.
- F-Q8 H.264 video: include a decoder, from which code? — resolved
  2026-10-09 (1376): **B**, FFmpeg's decoder translated, included.
- F-Q9 AAC sound: include a decoder, from which code? — resolved 2026-10-09
  (1377): **B**, FFmpeg's fixed-point decoder translated, included.
- F-Q10 Colour management: add it, copying whose? — resolved 2026-10-09
  (1378): **A**, Chrome's: skcms for pictures, Chrome's HDR handling for
  video; Little CMS later, for printers.

(C-Q18, answered for lane F's code, is §1334; lane C files its index line
when it retires the entry.)

