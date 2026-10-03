## 529. A selection highlight may be a raised `surface0` only if everything drawn on it is full-strength text; otherwise it recedes to `mantle`

**Date:** 2026-08-24
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** When you pick an item in a list, the shell marks it by painting a
patch of colour behind it. Two of the shell's lists paint that patch a *lighter*
shade (a raised tile), and one now paints it a *darker* one (a shallow dent).
That looks like an inconsistency and it is not quite: in light mode, the shell's
"raised tile" shade is close enough to its grey text that a caption sitting on
it becomes hard to read. So the rule is that a raised tile is allowed only when
everything written on it is the strongest text colour; a row that also carries a
smaller, greyer second line has to use the dent instead. This entry records the
rule, because otherwise the next module to hit it will pick whichever looks
right in dark mode and ship an unreadable light mode.

**Where it came from.** Both the calendar (module 43) and the workspace picker
(module 44) hit the same wall from opposite sides. The wall is a property of the
Latte palette, not of either module: Latte's *surface* ladder sits much closer
to its *ink* ladder than Mocha's does, because Latte's surfaces are darkenings of
a near-white base while Mocha's are lightenings of a near-black one. Measured on
`surface0`:

| ink on `surface0` | Mocha | Latte |
|---|---|---|
| `text` | 8.69 | **5.17** |
| `subtext1` | 7.10 | **4.05** |
| `subtext0` | 5.65 | **3.40** |

Only `text` clears 4.5:1 in Latte. In Mocha every rung clears it comfortably, and
by a wide margin — which is exactly why a dark-mode-only review passes a design
that is broken for half the users.

**The two shapes, and why they diverge.**

| Site | What sits on the selection | Fill | Why |
|---|---|---|---|
| Calendar's selected day | one day number, at `text` | `surface0` (raised) | The only ink on it is full strength, so 5.17:1 in Latte is enough. A raised disc is the conventional selection mark and there is no reason to give it up. |
| Workspace picker's selected row | an icon, a name at `text`, **and** a window-count caption and a shortcut hint at a quieter rung | `mantle` (recessed) | The captions are drawn unconditionally, so they land on the selection fill *and* on the picker background. They must clear the floor on both. On `surface0` no quiet rung does. |

So the divergence is not two answers to one question; it is one rule producing
two answers because the two rows carry different content. Stating it as a rule is
what stops it looking arbitrary later.

**What was rejected.**

| Option | Why not |
|---|---|
| **Keep `surface0`, raise the captions to `text` when selected** | The selected row then has three inks at the same strength and no internal hierarchy — the caption stops reading as a caption exactly when the row is the one you are looking at. It also spreads a `if selected` into three more sites. |
| **Keep `surface0`, accept 4.05:1** | An exception in a legibility floor is how the floor stops being one — the same argument the palette itself makes at `LIGHT_SUBTEXT0`, where the published Catppuccin value was darkened rather than granted an exemption. |
| **Darken Latte's `surface0`** | It would fix this and break every place a surface is meant to read as a *surface*; the ladder's spacing is the palette's business, not this module's. |
| **Recess *every* selection, including the calendar's** | Consistency for its own sake, at the cost of the conventional raised-selection idiom in the one place it demonstrably works. A rule that fires only when it must is a smaller change than a rule that fires everywhere. |

**Cost, stated plainly.** The shell now spells "selected" two ways, and someone
reading only the code will see it as drift. That is the price of the rule and the
reason it is written down rather than left in two file-local comments. The
mitigation is that the condition is mechanical and checkable: *if any ink on the
selection fill is quieter than `text`, the fill may not be a `surface*` role.*
Each module's contrast test enumerates its own ink-on-fill pairings by hand, so a
future module that gets this wrong fails its own test rather than shipping.
(**Superseded in part by §530**: a hand-written pairing table catches a *role*
moving underneath it, but cannot catch a pairing nobody wrote down, and cannot
catch anything at all in the module it lives in. The pairings are now read out of
the rendered commands.)

**Consequence for what is left.** Five shell modules remain in part 2, and the
~2,258-constant application conversion after it. Every list, menu and picker in
that set has this same shape, so the rule above is the first thing to check when
converting one — before the mechanical `theme::X` → `p.x` pass, not after, since
finding it afterwards means redoing the fixture that pins the row.
