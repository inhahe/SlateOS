## 1432. Checkboxes and radio buttons: a three-state box goes from "default" to "yes", and a radio group can be emptied only if it says so

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit now draws checkboxes and radio buttons itself
(`guitk::checkbox`, `guitk::radio`), and three things about how they behave
had to be chosen. A checkbox that can be yes, no or "use the default" steps
default, yes, no when clicked -- so the first click on a box left at the
default says yes. A radio group can be put back to "nothing chosen" by
clicking the chosen option again, but only a group that asks for that; in an
ordinary group -- light or dark, a paper size -- the click does nothing,
because "nothing" is not an answer there. And Space ticks a checkbox but Enter
does not, because in a dialog Enter presses the button that finishes it.

**1. The three-state order.** `design.txt`: "tristate checkboxes -- good for
yes/no/default, 'default' is useful for cascading option overrides".

| Option | From "default", a click gives |
|---|---|
| **Unchecked, partly, checked** (chosen; Qt's) | yes |
| Unchecked, checked, partly (Win32's `BS_AUTO3STATE`) | no |

A click on a checkbox means *yes* everywhere else, and an override box starts
at "default", so the first click should say yes. The other kind of partly-set
box -- a parent summarising children that disagree -- is not a three-state box
at all: its "partly" is shown, never chosen, and a click sets or clears it
(`Mode::TwoState`, `CheckState::toggled`, the rule the tree view already
uses).

**2. Going back to no radio choice.** `design.txt` asked for "a way for the
user to go back to having no radio button selected", and doubted the only way
it could think of: "clicking again on the currently selected one, which isn't
a very good way".

| Option | What a click on the chosen option does |
|---|---|
| **Clears it, in a group that opts in** (chosen) | nothing in an ordinary group; clears the choice in one built with `deselectable(true)` |
| Always clears it | every group can be emptied by accident, including ones where empty means "no value" |
| Never | a group with a meaningful "none" -- a filter, "any" -- needs a separate reset control |

Clicking again is the only gesture that needs no extra control, and it is
harmless exactly where emptiness means something. Where it does not, a click
that quietly emptied the group would leave a setting with no value and no
sign of why. In an opted-in group Space on the chosen option clears it too, so
the keyboard can do what the pointer can. *If the operator wants a different
gesture -- a small clear button, say -- it replaces this one without touching
the groups that do not opt in.*

**3. Space, not Enter, flips a checkbox.** Enter in a dialog presses its
default button; a checkbox that took Enter would stop the dialog closing from
the keyboard. The switch (`guitk::switch`) takes both, because a switch
stands alone on a settings row and acts at once, where a checkbox is usually
one field of a form.

**Where it bites:** `gui/toolkit/src/checkbox.rs`, `gui/toolkit/src/radio.rs`.
No program uses them yet; lane E's are asked to in
`requests/c-e-the-toolkit-has-switches-checkboxes-radio-buttons-and-drop-downs.md`.
