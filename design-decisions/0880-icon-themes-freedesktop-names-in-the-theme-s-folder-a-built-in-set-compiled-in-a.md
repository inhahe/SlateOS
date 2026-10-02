## 880. Icon themes: freedesktop names in the theme's folder, a built-in set compiled in, and the shell uploads each icon once before the frame that names it

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The desktop drew no icons -- its desktop icons are emoji
glyphs, which a font without emoji shows as boxes, and everything else was
words. There is now an icon theme: SVG files a theme can carry, found by
standard names, tinted to the colours around them, with a built-in set drawn
for this desktop. The start menu's places and its power button are the first
to use them.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| How is an icon named? | the freedesktop Icon Naming Specification (`folder-documents`, `utilities-terminal`), with its fallback of dropping the last `-part` | names of our own | every icon set in the world is drawn to these names, so a set made for another desktop drops in, and a program asking for an icon uses a name its author already knows. |
| Where does a theme keep its icons? | `<theme>/icons/<name>.svg`, beside `theme.yaml`; a folder of icons alone is an icon pack | a separate icon-theme directory tree (`/usr/share/icons`) | one place for everything a theme is, found through the colour themes' two roots with the user's first; `theme.icons` chooses the icons apart from `theme.colors`, which is the mix-and-match the roadmap asks for. |
| Sizes | one scalable SVG per name, drawn at the size asked | per-size PNG directories | the renderer draws SVG at any size with supersampling; per-size bitmaps would multiply the files a theme author has to make by the number of scales. |
| Colour | `currentColor` becomes the colour the icon is drawn in; other colours are kept | fixed-colour icons only | a monochrome set then follows the theme, the accent, and light or dark mode with no file per colour. |
| A theme without an icon | its shorter name, then the built-in icon, compiled in | draw nothing | a theme that draws a few icons, or no theme installed at all, still has every icon. |
| Untrusted files | over 256 KiB, not text, not an SVG this renderer reads, or drawing nothing: passed over for the next place to look; at most 512 px square | read what is there | a theme is data from strangers, and an icon is a few hundred bytes. |
| How the shell gets an icon to the compositor | the shell names an icon by a deterministic image id -- the request's hash under a reserved tag -- and the session, before submitting a tree, renders and uploads each id that surface has not been sent | rasterise in the shell and ship pixels in the tree | a tree is commands, not pixels, and the compositor already stores uploaded images per window; an icon goes up once and is drawn every frame by id. A change of appearance drops them all, and the next frames send the new ones. |

### What is not done here

- The taskbar and the tray still draw words; they move onto the icons next.
  (The desktop's icons did the same day: each type is its own picture in its
  own hue, and a dragged icon's ghost is the same picture faded -- an icon
  drawn in a translucent colour is drawn translucent.)
- Programs have no icon of their own: which icon is a program's is the
  application registry's to say (`open-questions.md`).
- A change to an icon file while its theme stays chosen is not noticed until
  the settings change, as with colour themes.
- What of SVG an icon may use is the toolkit renderer's subset: shapes and
  paths, fill and stroke with their joins, caps and fill rules, presentation
  attributes and (since later the same day) the `style` attribute, which is
  how Inkscape and Breeze write them. A `<style>` sheet's rules, `<use>`,
  gradients and clip paths are not drawn -- an icon relying on them draws
  without them.
