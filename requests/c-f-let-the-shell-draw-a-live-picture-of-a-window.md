# C → F — let the shell draw a live picture of a window

**From:** lane C (`gui/desktop`, `gui/toolkit`). **To:** lane F
(`gui/remote`, `gui/compositor`).
**Filed:** 2026-09-29. **Status:** open.

## In short

The Aero reference's taskbar shows a small live picture of a program's windows
when the pointer rests on its button, and "peeks" at a window -- shows it alone,
the others faded -- when the pointer rests on that picture. The overview and
the Alt-Tab switcher show window pictures too. The shell can draw none of it:
the display protocol has no way for one client to draw another client's
window, so the overview's cards are titled rectangles and the taskbar has no
preview at all. `gui/desktop/src/window_peek.rs` (2 150 lines, tested, on the
orphan baseline) is the preview popup, waiting for exactly this.

## What is asked

A draw command that shows a window's current content, scaled into a
rectangle -- say

```rust
RenderCommand::WindowPicture { window: u64, x: f32, y: f32, width: f32, height: f32 }
```

-- which the compositor draws from that window's latest frame, fitted to the
rectangle with its proportions kept, and draws again when that window changes,
so the picture is live the way the window is.

- **Gated as the window list is.** Honoured only in a frame from a client
  that passes `ClientLink::require_shell` -- the seam
  `SubscribeWindowList` already goes through -- and drawn as nothing in
  anyone else's. A picture of a window is a stronger read than its title
  (`known-issues.md` `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`), so it
  must never be more available than the title is. When `require_shell`
  gains its real capability check, this command gains it with no change here.
- **A window that is gone, minimised or unknown draws nothing** -- not an
  error: a preview raced by a window closing is ordinary.
- **Pixels never travel to the shell.** The compositor composes the picture;
  the shell only names the window. That keeps another program's screen out of
  the shell's memory, and costs nothing on the wire per frame.
- **Optional, and worth deciding with it:** a window flag its program sets
  to be left out of pictures (a password manager's), drawn as a plain card.

The command is a variant of `guitk::render::RenderCommand`, which is lane
C's file (`gui/toolkit/src/render.rs`), while its codec and its drawing are
lane F's. Lane C is content for lane F to add the variant together with its
encoding and its drawing, in one commit touching `render.rs` -- the order
§429 allows when one lane's addition would otherwise break another's
exhaustive matches. Or lane C adds it first, drawn as nothing, if lane F
prefers.

## What lane C does with it

- Wires `window_peek.rs`: the preview above a taskbar button after a
  moment's rest, one picture per window of the program, a click switching to
  it, a close button on each.
- Aero Peek: resting on a picture shows that window alone, the rest faded
  (the "Show desktop" strip's peek is the same effect for all windows).
- Pictures on the overview's cards and in Alt-Tab.

## If this is never done

The taskbar has no previews, and the overview and Alt-Tab keep titled
rectangles in their windows' proportions: usable, and short of the reference
(`roadmap-detailed.md` → *Aero-inspired theme* → *Taskbar*: "hover
thumbnails, Aero Peek-style preview on hover").
