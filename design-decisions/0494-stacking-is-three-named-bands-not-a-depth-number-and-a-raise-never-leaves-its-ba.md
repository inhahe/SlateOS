## §494 — Stacking is three named bands, not a depth number, and a raise never leaves its band

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A desktop has three kinds of surface: a wallpaper that belongs
behind everything, ordinary application windows, and the shell's own furniture
— taskbar, start menu, popups — that belongs in front of everything. Until now
the compositor had one flat pile and every window that was clicked went to the
very top of it, so a taskbar would have been an ordinary window that disappeared
behind the first application the user clicked on. The fix is to give each window
a *band* it is created in and can never leave; clicking still raises a window,
but only as far as the top of its own band.

**The alternative, and why not:** the obvious cheaper design is an integer depth
on each window — wallpaper 0, applications 100, taskbar 1000 — sorted at
composite time. It is less code and needs no new type. It was rejected on two
counts.

The first is that a depth number does not survive a raise. Raising means "put
this in front of its peers", which with numbers means picking a new number
larger than theirs, and there is no such number that is also smaller than the
next band's. Every scheme that fixes this — renumbering the band on each raise,
reserving gaps, using floats — is re-deriving a sorted list, badly, in a
representation that cannot express the constraint the list exists to enforce.

The second is that an open-ended depth encodes policy nobody wrote down. Each
new surface picks a number, and the numbers only mean something relative to
numbers chosen elsewhere in the tree; the next person adding an on-screen
display has to read every other surface's constant to find out what to type.
Three named roles say the same thing in a form the compiler checks, and there
are exactly three because three is what a desktop actually distinguishes.

**The representation, and the invariant it rests on:** `z_stack` stays one flat
`Vec<WindowId>`, kept partitioned by band in ascending order. It is not three
stacks, because roughly ten places already walk `z_stack` — the occlusion cull,
input hit-testing, damage, scene capture, direct-scanout eligibility — and every
one of them is correct as written against a single bottom-to-top list. Three
stacks would make each of those chain three iterators in the right order, which
is ten new chances to get the order wrong for one saved invariant.

The invariant is instead concentrated in one place: `stack_insertion_index`
*counts* the windows in bands at or below a given one, which is the correct
insertion point exactly while the stack is sorted, and `raise_within_layer` is
the only thing that ever inserts. `the_stack_stays_partitioned_by_band_under_
arbitrary_raises` re-checks the sortedness after every raise of every window
over three rounds, because a counting function silently returns a plausible
wrong answer the moment its precondition lapses.

**A consequence worth naming: focus is confined but not banded.** Clicking an
application still focuses it even though it cannot rise above the taskbar — the
band restricts stacking only. The one place the band does reach focus is
*automatic* refocus when a window closes: that now picks the topmost remaining
window at or below the closed window's band, because the old "topmost remaining
window" would have meant closing an application hands the keyboard to the
taskbar, which is in front of everything by construction and is never what the
user was about to type into.

**Compatibility:** `Layer::Normal` is the `Default` and what `WindowSpec::new`
sets, so every existing caller is unchanged — with all windows in one band the
insertion index is always the length of the stack, which is the `push` the code
did before. The 194 pre-existing compositor tests passed untouched, which is the
evidence for that claim rather than an argument for it. On the wire the layer is
one trailing byte on `CreateWindow`, and an unrecognised value is a decode error
rather than a default: a peer whose taskbar we silently filed under
"application" would give the user a broken desktop instead of a refused
connection.

**Reversible:** yes. Deleting the field returns the flat stack; nothing outside
`raise_within_layer`, `stack_insertion_index` and the `CreateWindow` codec knows
the concept exists.
