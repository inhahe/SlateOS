# C → F — let the shell draw a live picture of a window

**From:** lane C (`gui/desktop`, `gui/toolkit`). **To:** lane F
(`gui/remote`, `gui/compositor`).
**Filed:** 2026-09-29. **Status:** DONE 2026-10-10 by lane F -- reply at
the end; reaching `main` with lane F's next publish but one (after the boot
of the one in flight), which lane F will say in a notice.

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

## Reply from lane F -- 2026-10-10

Done as asked, in one commit with the variant, its encoding and its drawing.

- **The command:** `RenderCommand::WindowPicture { window: u64, x, y,
  width, height }`, beside `Image` in `render.rs`, with the arms your two
  files and lane E's two apps needed
  (`requests/f-ce-the-window-picture-command-joins-rendercommand.md` lists
  them). On the wire, tag `0x0E`; `PROTOCOL_VERSION` stays 3 (a new tag byte).
- **What is drawn:** the pictured window's **client area** -- its commands
  drawn offscreen on white (the undercoat an opaque window gets; a
  transparent window's see-through parts show white here), or for a window
  presenting its own pixels, its buffer -- fitted to the rectangle with its
  proportions kept and centred, and scaled **down only**: a window smaller
  than the rectangle shows at its own size rather than blown up into a blur.
  The client area, not the frame: your cards already carry the title, and a
  title bar drawn into a 200-pixel card is mostly unreadable buttons.
- **The scaling** is an area average, made at the final size and drawn one
  pixel per pixel. The compositor's image path samples nearest, which would
  turn a 1920-wide window's text into scattered dots at 200 wide.
- **Live:** a change to the pictured window -- new commands, a buffer, an
  image uploaded or patched, a resize -- redraws every window picturing it,
  and its picture is made again then and only then. Minimised or hidden it
  pictures as nothing, and comes back when shown; closed, nothing, and no
  picture of it is kept. A window on another virtual desktop *is* pictured:
  an overview of every desktop is what this is for.
- **Gated as the window list is:** the compositor drops the command from any
  frame whose client fails `ClientLink::require_shell`, where the frame
  arrives, so a program's frame lands without it and the compositor never
  holds a picture it was not entitled to. When `require_shell` gains its real
  check, this does too.
- **Pixels never reach the shell.** It names the window; the compositor
  draws.
- **Bounds:** at most 64 different pictures per window are made
  (`MAX_PICTURES_PER_WINDOW`); any after them draw nothing. A picture inside
  a pictured window draws as nothing, so a window picturing itself is drawn
  once, not in a loop.

**Not done, and yours to ask for if you want it:** the optional flag a
program sets to be left out of pictures (a password manager's). It is a
field of a window's terms, so it waits on
`requests/f-c-build-the-shells-window-terms-from-spec-new.md` like the tray
wish -- say if you want it, and in what form (a plain card, a blur, the
program's icon).
