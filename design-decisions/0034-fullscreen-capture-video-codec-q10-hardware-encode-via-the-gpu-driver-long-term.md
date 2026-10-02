## 34. Fullscreen-capture video codec (Q10) — hardware encode via the GPU driver long-term (option C); defer the software-codec port near-term (option D); no stub encoder meanwhile

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q10; the operator deferred
to Claude's recommendation). The operator's words: *"Q10: I'll go with your
recommendation."* Claude's recommendation was **C long-term, D near-term**.

**The decision.** The proper home for the remote-desktop fullscreen capture
fallback (roadmap §4.5 — DMA-BUF/buffer-backed game/video surfaces with raw
pixels, not vector `RenderCommand`s) is **hardware video encode via the GPU
driver's encode engine**, which is hard-blocked on a GPU driver with an encode
engine (AMDGPU/i915, roadmap §4.x) that does not exist yet. So:
- **Near-term: defer the whole fallback** rather than build a software encoder
  hardware encode would later obsolete.
- **If** a software fallback is ever wanted before GPU encode lands, prefer
  **AV1 via `rav1e`** (royalty-free + Rust-native), not H.264/x264
  (patent/GPL friction).
- **No stub encoder meanwhile** (band-aid); the draw-command stream already
  covers the flat-shaded-desktop case.

**Rationale (both sides).** *For C/D:* avoids a soon-obsolete software-codec port;
matches real streaming architecture; keeps the royalty-free posture. *Against:*
fullscreen game/video remoting stays unsupported until GPU encode exists —
acceptable because the capture substrate is codec-agnostic (only the encoder
backend is blocked) and the desktop case already streams.

**Where it bites.** `gui/compositor` (fullscreen pixel capture + frame pacing + an
`Encoder` trait) and a future encoder crate; IPC extends
`CompositorRequest`/`CompositorResponse` alongside `StreamStart`/`StreamCapture`/
`StreamStop`. No code now — records the deferral + codec choice for when GPU
encode lands.
