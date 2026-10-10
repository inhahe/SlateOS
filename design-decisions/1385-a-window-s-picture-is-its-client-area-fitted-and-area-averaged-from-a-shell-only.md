## 1385. A window's picture is its client area, fitted and area-averaged, drawn only from a shell

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous), within the shape lane C asked for in
`requests/c-f-let-the-shell-draw-a-live-picture-of-a-window.md`.

**In short:** the shell can now show a small live picture of any window --
the taskbar's preview when the pointer rests on a button, Aero Peek, the
overview's cards, Alt-Tab -- by naming the window in a draw command; the
compositor draws the picture and keeps it current. This records how the
picture is made and who may ask for one.

### The choices

| Question | Decided | Alternative | Why |
|---|---|---|---|
| What is pictured | The client area -- what the program draws | The whole window, frame and title bar too (what Windows' thumbnails show) | The shell's cards already carry the title, and a title bar scaled into a 200-pixel card is unreadable buttons. Making the client area is also one render of the window's commands, where the frame would mean drawing the decorations away from the screen too. |
| How it is scaled | An area average to the exact size it is drawn at, then drawn one pixel per pixel | The compositor's image path, which samples the nearest pixel | A 1920-wide window drawn 200 wide keeps one pixel in ten of each line: its text becomes scattered dots. The average keeps a line of text a grey line, as every high-quality downscale does. |
| Larger rectangle than window | Drawn at its own size, centred | Scaled up to fill | A blown-up small window is a blur that looks broken; the card's space around it does not. |
| When it is made again | When the pictured window's content changes (a per-window revision: commands, buffer, images, size) | Every frame it is drawn | A picture costs a render of the window and an average over every pixel; a static window's picture is made once. A video's is made once per frame of the video, which is what live means. |
| Who may ask | A client passing `require_shell`, the window list's own check; the command is dropped from anyone else's frame where it arrives | Checked when drawn | Checked at arrival, the compositor never holds a picture a program was not entitled to, and the draw path needs no notion of who sent what. A picture of a window is a stronger read than its title, so it must never be more available. |
| A window gone, minimised or hidden | Pictured as nothing | Its last frame | The shell is told of all three in the window list and can show what it likes; a stale picture of a closed window would be a lie. A window on another desktop *is* pictured: an overview of every desktop is the point. |
| Bounds | 64 different pictures per viewer; a picture inside a picture draws nothing | None | A frame can name millions of pictures, each one a render to make and keep; and a window picturing itself must not loop. |

A transparent window's see-through parts are pictured as white -- the
undercoat an opaque window gets -- because the compositor composites
opaquely and keeps no coverage to cut them out with. A blank card corner is
the cost; the alternative was a coverage channel through the whole software
renderer for one effect.

### Revisit if

A program needs to be left out of pictures (a password manager): that is a
flag in its window's terms, waiting on lane C's change to how the shell
builds them (`requests/f-c-build-the-shells-window-terms-from-spec-new.md`).
