# E -> C: a window rule the file cannot read is deleted by the next save

**From:** Lane E (`apps/settings`). **To:** Lane C (`gui/windowrules`).
**Filed:** 2026-10-10. **Status:** OPEN -- nothing is lost while it waits:
Settings' Window Rules page refuses every change while the file holds a rule
it cannot read, and says why. The cost is that a user with one typo in a
hand-written rule cannot use the page until they mend that rule or remove it.

**In short:** `windowrules::file::store` writes the rules it is given and
nothing else. `write` empties the `rules` block -- every name in it, readable
or not -- and writes the given rules back. So a program that saves the rules
it read deletes every rule the file could not read: a hand-written rule with
one misspelt key, which is exactly the rule `problems` exists to tell its
author about so they can mend it, is gone the moment they flip any switch on
the page.

## What happens now

1. `window-rules.yaml` holds two rules: `Terminal` (readable) and `Chat`,
   written by hand with `taskbar: flase`.
2. `load()` answers `Terminal`, and a problem: `Chat`, "`taskbar` is `true`
   or `false`, not \"flase\"". The desktop says so in a notification.
3. Settings shows both -- and the user turns `Terminal` off, which stores
   `[Terminal]`. `Chat` and everything the user wrote in it are gone, and the
   notification that would have reminded them of it stops, since the file no
   longer has the problem.

## What would fix it

`write` keeps each rule it cannot read, as the text its author wrote, where
it stands among the others -- or, if a place cannot be kept, below the rules
it writes: it is not applied, so its priority means nothing until it is
mended. Then `store` is safe to hand `load()`'s rules, changed or not.

A test that would hold it: a file with a readable rule and one that cannot
be read, stored with the readable one changed, still holds the other's text,
and `read` still reports it as the same problem.

When it lands, say so here: Settings drops its refusal (the page's check of
`problems` before a change) in the same change that reads this.

## And one small thing

The number of virtual desktops is a literal in `DesktopShell::new`
(`num_desktops: 4`), and a rule's `desktop` past it is dropped
(`rule_requests`). Settings' Desktop list offers desktops 1 to 4 to match it,
which is a second copy of the 4. A constant both can read -- in
`windowrules`, beside the rules that name a desktop -- would keep the two
together.
