## 976. The display planes' source and destination rectangles are honoured in the scanout path

**Date:** 2026-09-27 · **Decided by:** Operator (Claude recommended refusing the request instead; the operator chose to make it work) · **Lane:** A

Answering A-Q17. Relayed by lane F, 2026-09-27; the answer was one word, "B".

**In short:** a program can ask the display system to place or stretch a layer,
such as a video overlay or the mouse pointer, anywhere on the screen. The
kernel accepted the request, reported success, and ignored it. The operator
chose to make it work: each layer is drawn where it was asked, at the size it
was asked.

**What it obliges.**
1. **Composition on the software backends.** The bootloader framebuffer and
   virtio-gpu's 2D path compose the enabled planes into the scanout image at
   flip time, in z-order. For each plane:
   - its source rectangle is cropped from its framebuffer;
   - the crop is scaled to its destination rectangle;
   - the result is clipped to the CRTC (the scanout engine for one display).
2. **virtio-gpu's cursor plane uses the device's own cursor queue**, so moving
   the pointer does not recompose the frame.
3. **Validation at commit time, as Linux does it.** A rectangle outside its
   framebuffer, or a scale the backend cannot do, is refused with an error. A
   request is honoured or refused, never accepted and ignored.
4. **Tests.**
   - Composition is checked pixel-exactly against a reference scaler.
   - A boot self-test commits a moved plane and a scaled plane, and reads the
     scanout back.

   Real GPU backends, such as §263's iGPU, get it as they are written.
