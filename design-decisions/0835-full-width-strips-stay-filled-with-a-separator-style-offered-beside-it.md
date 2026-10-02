## 835. Full-width strips stay filled, with a separator style offered beside it

**Date:** 2026-09-12
**Lane:** C
**Decided by:** Operator (answering C-Q14; Claude recommended A, the operator chose B as the default and asked for A as an option)

**In short:** a toolbar or a status bar keeps the pale band it has today. The
alternative — no band, just a hairline along one edge — becomes something a user
can switch on, rather than the default or a discarded idea.

**Why B as the default.** A strip that spans the window reads as a band rather
than as a box; window chrome is a different material from content, and it is
reasonable for it to look like one. It is also what ships today, so 211 draw
sites need no change to keep working.

**Why A survives as an option rather than being dropped.** It is what most
desktops do and it is the lighter treatment, and the cost of keeping it is one
`Surface` member and one arm in the decision point — which is what that type
exists for. Dropping it would mean the question gets re-asked the first time
somebody finds the bands heavy.

**The shape it takes.** `Surface::Strip` carries which edge faces the content,
because a toolbar's separator sits along its bottom and a status bar's along its
top, and the drawing code cannot infer that from the rectangle. A
`StripStyle` setting chooses `Filled` or `Separator`. It is a separate setting
from `SurfaceStyle` rather than a third variant of it, because the two are
orthogonal: someone may want outlined boxes with banded chrome, or filled cards
with hairline chrome, and folding them into one enum would offer four
combinations as two.

**What it costs, stated because it is the objection to A.** A third way of
drawing a surface. `SurfacePaint` grows a `separator` field that is `None` for
every other kind, and the tests that assert a box is "filled or outlined" have to
learn a third answer. That is the price of the option and the operator took it
knowingly.
