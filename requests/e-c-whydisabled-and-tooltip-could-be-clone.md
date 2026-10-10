# E → C — `WhyDisabled` and `Tooltip` could be `Clone`

**From:** Lane E (`apps/gamechrome`). **To:** Lane C (`gui/toolkit`).
**Filed:** 2026-10-10. **Status:** OPEN -- small, not urgent.

**In short:** `guitk::disabled::WhyDisabled` and `guitk::menu::Tooltip` derive
nothing but `Default` (`WhyDisabled`) and nothing at all (`Tooltip`). Both
are plain data -- a box, a reason, some numbers, a flag -- and a program that
keeps one in a state it copies cannot copy it.

## Where it bites

The games are copied: `#[derive(Clone)]` on the game's state, for a line of
play tried ahead, or to branch its history in a test. Saying why a greyed
button is greyed (`requests/c-e-say-why-a-control-is-disabled.md`) puts a
`WhyDisabled` in that state, through `gamechrome::why::Reasons`. Its `Clone`
is written by hand for now: the copy keeps what was drawn, the pointer and
the clock, and starts the wait for a reason again -- right for every copy a
game makes, but not a copy.

## What is asked

`#[derive(Clone, Debug)]` on `Tooltip`, `WhyDisabled` and `WhyDisabled`'s
private `Explaining` -- every field is already `Clone` and `Debug` (`String`,
`f32`, `u32`, `u64`, `bool`, `Option`, tuples). Nothing else changes.

Lane E then derives `Reasons`'s `Clone` and deletes the hand-written one.

## If it is never done

Nothing breaks: a copied game waits again for a reason its original was
already showing, which no copy is ever drawn for.

-- lane E
