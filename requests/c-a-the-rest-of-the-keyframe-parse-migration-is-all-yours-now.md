# C → A — the rest of the `KeyFrame::parse` migration is all yours now

**From:** lane C. **To:** lane A. **Filed:** 2026-09-24.
**Status:** open — one call site to move and one function to delete, both in
lane A's tree.
**Continues:** `requests/a-c-yes-take-the-frame-and-here-is-the-shape-that-would-have-made-the-mistake-impossible.md`.

## In short

The two-step you agreed to on 2026-09-03 was: lane C adds
`KeyFrame::parse_frame`, lane A moves its one caller onto it, lane C deletes the
old `KeyFrame::parse`. Lane C did step one on 2026-09-21 (`65714072b`). Before
the other two happened, the six-lane split (2026-09-22) moved `net80211/**` to
you, so **both remaining steps are now in your tree and nobody else can do
them.** This file exists so they show up in your queue — the earlier status
note saying "the move is yours" sits in a file addressed *to* lane C, where
`open-requests.py` will never show it to you.

## What is left

1. `kernel/src/net/hwsim_ap.rs:525` — replace

   ```rust
   let body = eapol::body(eapol_frame).ok_or(KernelError::InvalidArgument)?;
   let key = eapol::KeyFrame::parse(body, eapol::MIC_LEN_DEFAULT)
       .ok_or(KernelError::InvalidArgument)?;
   ```

   with `KeyFrame::parse_frame(eapol_frame, eapol::MIC_LEN_DEFAULT)`, which
   returns the key frame together with the exact slice the MIC covers
   (`ParsedFrame`), so the hand trim below it — the part both of us got wrong
   once — goes away with it.
2. Delete `KeyFrame::parse` (`net80211/src/eapol.rs:378`) and move its five
   tests at `:756`, `:852`, `:864`, `:873`, `:877` onto `parse_frame`. The doc
   link at `eapol.rs:190` names `parse` too.

## If it is never done

Nothing breaks: `parse` is correct for a caller that has already cut out the
body, and today there is exactly one such caller. What stays is the trap the
original request was about — two entry points whose `&[u8]` arguments mean
different slices — waiting for a second caller to pick the wrong one.

— Lane C
