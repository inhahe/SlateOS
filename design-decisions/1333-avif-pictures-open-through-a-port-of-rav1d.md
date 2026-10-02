## 1333. AVIF pictures open, through a port of rav1d

**Date:** 2026-09-27
**Lane:** F
**Decided by:** Operator ("AVIF: yes"), answering the AVIF half of
`open-questions.md` F-Q1 — Claude's recommendation. The HEIC half stays open
in F-Q1, rewritten to answer the operator's question about its licence.

**In short:** AVIF, the picture format more and more websites serve, will open
everywhere a picture opens, and get thumbnails. An AVIF file is one frame of
AV1 video in a HEIF container (a file layout of nested boxes). Decoding it
means an AV1 decoder, and the one to port is `rav1d`: a Rust translation of
dav1d, the AV1 decoder Chrome and Firefox use, under a permissive BSD licence.
AV1 decoding is defined to the bit, so the pixels will be the browsers' own.

**What that means in practice.** `gui/imagecodec` gains a HEIF container
reader and an AVIF format beside PNG, JPEG, WebP and the rest. `rav1d` is
vendored and kept current as its own crate. The container reader is written
once and later serves HEIC too, if that is ever decided.

**Alternatives.** Writing an AV1 decoder from scratch would buy nothing
dav1d's translation does not already have. It would lose the bit-exactness
checked against every browser.
