## 1400. The taskbar's tiles are the Aero reference's, and a tooltip's delay wakes the desktop

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The taskbar's buttons were boxes of one width, each with a
picture and a name. They are now drawn as the default theme's reference draws
them: a pinned program is its picture alone, on a small square, and a window
is its picture and its title on a tile as wide as the title needs, up to 160
pixels, in faint glass with an edge. A pinned program no longer shows its name,
so resting the pointer on any tile names it -- and making that work showed that
tooltips never had on a desktop nobody was otherwise touching: nothing woke the
desktop when a tooltip's half-second delay ran out, so a tray icon's name
appeared only if something else happened to redraw, and stayed up after the
pointer left until something did again.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| A pinned tile | its program's picture, 30 pixels on a 36-pixel square, flat | its picture and name, as before | the reference's `aero-task`; and `design.txt` makes the name the option ("option to show app name along with app icon"), so the picture is the default |
| A window's tile width | as wide as its picture and title need, up to 160 (`width: auto; max-width: 160px`) | one width for all, as before | the reference's `is-labeled` tile; a short title no longer holds a long title's room. The price: a tile's width follows its title, so the tiles right of it move when it changes |
| When they do not fit | the widest windows give way first, all to one width, each down to a square; past that, every tile shares alike (`fit_tiles`) | shrinking every tile by the same fraction, as the reference's flexbox would | a short title is the last to be cut, which is the order a reader would give them up in |
| Tile corners | half the windows' radius | the windows' radius, as before; or the reference's fixed 4 | the reference's 4 at the default 8, and the setting still moves them at every step -- where 16 on a 36-pixel tile is a pill, and a fixed 4 ignores the setting |
| The glass | two fills, the upper half brighter, an edge in the bar's text colour and a highlight along the top | the reference's gradients | the renderer draws no gradients; two steps are the same glass |
| A pin's tooltip | "Terminal — pinned (click to open)" | the name alone | the reference's; it says what a click does, which is not what most taskbars' pins do (§885) |

### The tooltip's deadline

The session sleeps while nothing on screen moves (§812), and wakes for known
moments -- a clock's next minute, a slideshow's next picture. A tooltip's delay
was not one of them, and a tooltip becoming visible, or going, did not mark
anything to be redrawn. The unit tests never saw it, because they call the
clock by hand. Now the shell reports when a tooltip is due
(`DesktopShell::tooltip_due_in`, from the toolkit's `Tooltip::due_in`) and
whether one came, went or began waiting (`take_tooltip_changed`), and the
session wakes and repaints for both. A session test drives it through the real
input path.

### What is not done here

- ~~**Hover and pressed states**~~ -- the hover done the same day: the tile under
  the pointer lights in the accent's glass with a glow of it, over whatever
  state it was in, as the reference's stylesheet orders it; the bar is redrawn
  when the light moves. The reference gives its tiles no `:active` state (only
  the start orb has one), so there is no pressed one to add.
- ~~**The option to hide windows' titles**~~ -- done the same day, §1401.
- **A window asking for attention** -- the reference's amber `is-alert`: a window
  cannot yet say it wants attention; that is a field in the window list, lane
  F's protocol, asked for in `requests/c-f-a-window-cannot-ask-for-attention.md`.
- **The start orb** -- the reference's round, 64-pixel start button filled with a
  picture.
