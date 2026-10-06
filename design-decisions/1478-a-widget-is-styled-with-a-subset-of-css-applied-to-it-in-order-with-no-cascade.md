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
| **Selectors: a widget's kind, `.class`, `#name`, states, and `>` for a child** | CSS's full selector language | What the roadmap keeps. The descendant combinator (a space) is dropped with the sibling ones: `A B` reaches any depth, and a rule that styles more than its author can see is the "annoying override" `design.txt` names. | A sheet names each level it means. |
| **A widget's states in its own block: `&:hover { ... }`** | states only in a style sheet | A program styling one widget inline can give it a hover without a sheet; `&` is CSS Nesting's spelling, so the text reads as CSS. | -- |
| **The program's `Style` kept apart; CSS computed over it at every layout** (`Widget::look`) | CSS writing into `Style` | A state's style comes off when the state does, and a field the program changes is taken up at the next layout -- neither could be told from a style CSS had already written over. | A style is computed per widget per layout; a tree with no CSS computes none. |
| **A styled tree lays itself out again when the pointer or a key changes a widget's state** | restyling only colours | `:hover { padding: 8px }` changes the widget's size, which only a layout can follow. The states are compared before and after each event, so a pointer moving within one widget lays out nothing, and a tree with no CSS is not laid out any more often than before. | A styled tree is laid out each time the pointer crosses from one widget to another -- small trees, cheap. |
| **`width` and `height` are the border box's** (`box-sizing: border-box`) | CSS's default content box | The toolkit's sizes and limits are border boxes already; a width that grew with its padding would be a second meaning of the same number. | A style written for a content-box page draws a little smaller. |
| **A `font-family` list draws in the first family of it installed here; generic names are the user's faces** (`serif`, `sans-serif`, `system-ui` the UI face, `monospace` the fixed-pitch one) | a face per generic name | The user chose the desktop's faces; a program asking for "serif" gets the user's text face, not a serif the user never picked. A name with no font here is passed over, as CSS does, and a list with none drawn here is the UI face. Until a render tree can name a family (`FontFamily::Named`, `requests/c-f-text-in-a-family-the-drawing-names.md`), every name is passed over. | A program cannot ask for "a serif face" without naming one. |
| **A widget measures in its family by a scope, not by every call naming it** (`text::in_family` round its sizing, drawing and event handling; `PushFont` round its drawing where the family changes) | passing the family to each of the toolkit's measuring functions | Every component (button, field, text area ...) measures through functions that name no family; a scope makes all of them follow the style at once, and the same scope round drawing and events keeps a caret where the face puts it. A child that inherits its parent's family is not pushed again. | A thread-local the measuring functions read: a caller measuring for a widget outside its scope measures in the UI face. |
