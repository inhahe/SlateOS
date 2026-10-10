## TD-C-PROGRAMS-DRAW-AT-ONE-SCALE-WHATEVER-THE-DISPLAYS-IS (lane C, 2026-10-05) — OPEN

**Status:** OPEN -- waiting on the operator's answer to C-Q34 (where the
scale is applied), which decides whose fix it is.

**In short:** with a display's scale set above 100% (`display.scaling_percent`
in `appearance.yaml`, the Display page in Settings), the desktop's own
surfaces and the window frames grow and every program's contents do not:
their text, controls and pictures are drawn at the size they would have on a
100% display.

**Where:**
- `gui/compositor/src/lib.rs`: each display has a `scale_factor`, applied to
  the window frames (`Window::scale_factor`, `scale_dimension`) and reported
  to programs (`GetDisplayInfo`); what a program draws is drawn as written.
- `gui/window/src/lib.rs`: `Connection::display_info` asks for it; the
  application event loop (`app.rs`) never does, and nothing tells a program
  the scale changed.
- `gui/toolkit/src/scaling.rs`: `set_global_scale` is set by the desktop
  alone and read by nothing; the rest of the module (logical and physical
  pixels, `ScaleContext`) has no caller in a program.
- `apps/**`: no program reads the scale (`scale_factor`, `display_info`,
  `scaling_percent`: no match, 2026-10-05).

**To reproduce:** set `display: scaling_percent: 200` in `appearance.yaml`,
start any program: its text is the size it is at 100%, while the taskbar's
and the title bar's are doubled.

**The fix:** one of two, chosen in C-Q34. (A) The compositor draws each
program's render list at its window's scale -- programs lay out in logical
pixels, the compositor multiplies, and divides pointer coordinates before
handing them over; every program is right at once, and text stays sharp
because it is drawn from the list, not stretched (lane F). (B) The toolkit
learns the scale per thread, as it learned the text size (design-decisions
§1474), and each program multiplies its own layout (lanes C and E, program by
program). Recommended: A.
