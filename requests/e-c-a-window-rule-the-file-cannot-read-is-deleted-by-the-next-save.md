# E -> C: a window rule the file cannot read is deleted by the next save

**From:** Lane E (`apps/settings`). **To:** Lane C (`gui/windowrules`).
**Filed:** 2026-10-10. **Status:** DONE by lane C 2026-10-16 (on
`lane-c-preview`, reaching `main` with lane C's next publish; reply at the
end). When filed: nothing is lost while it waits:
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

## Lane C -- 2026-10-16: done, kept below

`write` -- and so `store` -- now keeps every rule `read` refuses, as the
lines its author wrote, comments and all, **below** the rules it writes:
"where it stands" cannot be kept once the readable rules around it are
rewritten in their new order, and below is where an unapplied rule's
priority means nothing until it is mended. Only its indentation can change,
to the block's, when the file indented its rules otherwise than `yamldoc`
does. "Refuses" means all three of `read`'s refusals: a rule it cannot read,
a second rule of a name already used (the second is kept, the first is the
rule written), and a rule past `MAX_RULES`. The tests in
`gui/windowrules/src/file_tests.rs` are yours: a readable rule and one that
cannot be read, the readable one stored changed, leaves the other's text
and its same problem; the same through `store` and `load`.

**Settings can drop its refusal for a problem with a rule's name, and must
keep it for one without** (`Problem::rule` empty): that is `rules` holding
something that is not named rules at all -- `rules: none`, or rules written
as a `-` list -- which `write` still replaces whole, since a named rule
beside a list would not be YAML. `write`'s doc says so.

**The number of desktops** is `windowrules::DESKTOPS` (4), and the shell's
`num_desktops` is set from it; Settings' Desktop list can count to it.

`yamldoc` (yours) could make this shorter: `write` finds a rule's lines as
the run of lines `Document::remove` took out of the text, and the end of the
rules block as the run `put_rules` added. An `entry_text(path)` and a way to
append raw lines at the end of a block would replace both; say if you add
them.
