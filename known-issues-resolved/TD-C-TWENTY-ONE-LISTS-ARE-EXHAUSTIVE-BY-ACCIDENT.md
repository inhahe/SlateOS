## `TD-C-TWENTY-ONE-LISTS-ARE-EXHAUSTIVE-BY-ACCIDENT` (lane C, 2026-09-18) -- **CLOSED 2026-09-22**

> **Closed by inverting the gate's default rather than by making twenty-one
> judgments.** `scripts/check-variant-lists.py` no longer decides what to check
> by name. Every list whose element type resolves to an enum is checked, and a
> list that is deliberately short is recorded in
> `scripts/variant-lists-partial.txt` with a reason. 112 lists now hold every
> variant, 16 are excused, 0 are out of step.
>
> **The entry's own "general form" was the fix**, and the reason to prefer it
> over twenty-one renames is that the two defaults fail differently.
> Name-scoping fails toward *silence*: a list that ought to be total and
> happens to be called `FLEET` or `VIEW_MODES` gets no check and nobody finds
> out. Opt-out fails toward *noise*: forgetting to record an exception is a
> failing gate that asks for the reason in writing. The crying-wolf cost the
> original design was avoiding is real, and it is paid once per list in a file
> where the reasons sit together and can be audited.
>
> **The check also changed from counting to naming**, which was not in the
> plan and is the more important half. Comparing lengths has a hole: `[A, A, C]`
> over `enum { A, B, C }` has three entries and three variants and is missing
> `B`. It also cannot say anything at all about an enum whose variants carry
> data -- `apps/sudoku`'s `KEYPAD` is fourteen entries over ten variants
> because `Target::Digit(1)` through `Digit(9)` are one variant. Naming the
> missing variants fixes both, and makes the failure message say *which* one
> fell out instead of only that the totals disagree.
>
> **What it found, which is why this was worth doing rather than filing.**
> `apps/settings`'s `DropdownId::FIXED` held 10 of 16. Its doc comment
> explained the absence of exactly one of the six, `NotifImportance` -- the
> other five, `RotationInterval`, `LoginBackground`, `UiFont`, `LockAfter` and
> `MonoFont`, were absent for no reason anybody had written down. Each was
> drawn, openable and handled, so nothing was broken; what was missing was the
> guarantee the list exists to give, since `test_every_dropdown_has_something_that_opens_it`
> walks `FIXED` and the file's own comment says "a dropdown excluded for being
> hard to reach is exactly the one it should be checking". Adding the five made
> the sweep fail three times in a row, each for a different missing
> precondition: the rotation interval needs a wallpaper *folder*, and the two
> font pickers need a non-empty family list, which a test never has. All three
> are now fixtures in the sweep's setup and all 237 settings tests pass.
>
> **The old count-based check could not have found them.** Ten of sixteen looks
> exactly like a subset that means it; only asking *which six* shows that five
> of them have no stated reason. That is the same lesson as
> `TD-C-NINETY-ONE-APPS-BIND-KEYS-AND-NAME-THEM-NOWHERE`: the instrument's
> question decides what it is able to notice, and a question about quantity
> cannot see a defect in identity.
>
> **One latent bug in the new keying, caught by a count that looked wrong.**
> The exceptions file was first keyed by `(path, name)`, and reported 17
> exclusions for 16 records. `apps/settings` has two constants called `FIXED`
> -- `DropdownId::FIXED` and `SliderId::FIXED` -- so one record silently
> excused both, and the second was exhaustive and wanted checking. The key is
> now `(path, Enum::NAME)`. A key that can match the wrong thing is precisely
> the silent hole this gate exists to close, so it is fitting that the gate's
> own bookkeeping had one.
>
> `userspace/coreutils/src/bin/ls.rs`'s `FILETYPE_INDICATORS` is lane B's and
> was not touched; it is recorded as a legitimate subset, since `Ind` also
> carries colour-control entries that are not file types. Lane B's
> `uname.rs:PRINT_ORDER` is exhaustive in fact and is now held to it --
> `requests/c-b-variant-lists-are-checked-by-type-not-by-name.md` says so.


**In short:** 21 arrays in `apps/` name every variant of their enum today, and
none of them is named as though it should stay that way. Nothing checks them,
so the day somebody adds a variant it is silently dropped from the list --
a dropdown missing an option, a game missing a difficulty -- with no error
anywhere and nothing to notice.

**Where this came from.** `scripts/check-variant-lists.py` checks every list
whose *name* claims totality (`ALL`, `ALL_*`, `EVERY_*`). Lane A pointed out
that this makes the gate's population a fact about what authors called things,
and asked what happens to a list that ought to be total and is called
`PRIMARY_COMMANDS`. Nothing happens to it, which is the problem -- and the
tool could not even show which lists those were: `subsets` was a counter that
was printed and never recorded, and the summary's "(--list says which)" was
true of the three unresolved skips and false of the thirty-six subsets. It
lists all three groups now.

**The flag that makes it actionable** is "exhaustive in fact, not named so":
declared length equals the variant count. That converts an unknowable question
-- did the author *mean* this to be total? -- into an observable one, and the
output separates the two kinds cleanly:

| | |
|---|---|
| genuine subsets | `PROMOTION_KINDS: [PieceKind; 4]` of 6 -- you cannot promote a pawn to a king; `ALWAYS_DRAWN: [Target; 6]` of 19; `FIXED: [DropdownId; 10]` of 16 |
| **total today, unnamed as such** | `FLEET: [ShipKind; 5]`, `LANGUAGES: [Language; 12]`, `VIEW_MODES: [ViewMode; 5]`, `COLOR_LABELS: [ColorLabel; 7]`, magnifier's `MODES`/`TRACKINGS`/`FILTERS`, four separate `[Difficulty; 3]` |

**Why it is filed rather than fixed.** The fix is one judgment per list, and it
is not "rename them all to `ALL_`": some are complete today by coincidence and
have no duty to remain so, and only somebody who knows the app can say which.
Renaming a list that *may* legitimately stay partial would replace a silent gap
with a false guarantee. Twenty-one judgments, each cheap, none mechanical.

**The general form** is the one worth carrying: a checker whose population is
chosen by a name inherits the vocabulary of whoever wrote the code, and the
cure is to define the population by something the type system fixes. For a key
list that is `const NAME: ... (&str, &str)`; for this, "an array of an enum's
variants, however named". Both tools were written the other way on the same
afternoon by two different authors -- see the fifteenth row of
`TD-C-SEVEN-WAYS-A-SEARCH-SAYS-NOTHING-AND-MEANS-NOTHING`.
