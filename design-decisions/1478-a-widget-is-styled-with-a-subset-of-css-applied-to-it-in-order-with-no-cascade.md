## 1478. A widget is styled with a subset of CSS, applied to it in order -- no specificity, no `!important`

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a program can style its widgets by writing CSS -- the part of
CSS that makes sense for a widget: colours, fonts, margins, padding,
borders, rounded corners, sizes, opacity, shadows -- instead of setting each
field of a `Style` by hand. It can use CSS's units (`em`, `%`, `vw`, `mm`...),
`calc()`, the theme's colours by name (`var(--accent)`), and `:hover`,
`:focus` and the other states. What it leaves out is the part of CSS that
makes styles hard to predict: there is no specificity and no `!important`.
Styles apply in the order they are written, then the widget's own -- a
reader can tell what a widget looks like by reading down the sheet.

**Where:** `gui/toolkit/src/css.rs` and `css/` -- `token` (CSS Syntax Level
3's tokenizer, for the subset), `value` (lengths, `calc()`, colours), `decl`
(the properties and their shorthands); the computing, the widget tree's
side, selectors, transitions and positioning follow. Asked for by
`design.txt` ("css styles applied to various things like with Qt, but no
annoying css overrides unlike Qt") and `roadmap-detailed.md` §3.5 (*Styling
-- CSS Subset with Inheritance, No Cascade*).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **CSS's own syntax and values** for the subset | a language of the toolkit's own | Every programmer and theme author already knows it; colours, units and `calc()` mean what they mean everywhere. `design.txt` weighed both and kept the subset. | A reader may expect a property the subset does not have; each one not read is named in the warning. |
| **Rules apply in the order written, then the widget's own declarations; no specificity, no `!important`** | CSS's cascade | What `design.txt` asks: "no annoying css overrides". A later rule wins because it is later -- one fact to know, visible in the text. | A sheet that wants a general rule after a specific one must put the specific one last. |
| **Inheritance only of what CSS set** | inheriting the toolkit's defaults too | A tree with no CSS draws exactly as before: a label's size is its own unless a style sheet gave its parent one. | Inheritance stops at a widget whose parent was styled only in code. |
| **A length is a sum of amounts in each unit until it is drawn** | resolving at parse time | `calc(100% - 2em)` and `50%` need the widget's font and its container, known only where it is laid out. | A little more to carry than a number. |
| **`cm` and `mm` at CSS's 96 pixels to the inch, unless the display's size is known** | refusing them | The roadmap asks for true physical sizes from the display's EDID; until a program can learn the display's size (`TD-C-PROGRAMS-DRAW-AT-ONE-SCALE-WHATEVER-THE-DISPLAYS-IS`, C-Q34) they are CSS's reference sizes, and correct the day the size is passed in. | A millimetre is not one on a display far from 96 dpi. |
| **What cannot be drawn is refused, not approximated silently** | drawing the nearest thing | An elliptical corner, an inset shadow, a list of shadows, `justify`, italic text, `auto` margins: each is a warning naming what was not done, and the declaration is dropped. Dotted and dashed borders are the one approximation (drawn solid), said in the documentation. | A style written for a browser loses those parts. |
| **Lenient parsing with warnings** | failing the whole sheet | CSS's own rule -- a declaration that cannot be read is dropped and the rest kept -- and a program can show the warnings (the theme checker does) or log them. | A typo is a warning, not a stop. |
