### TD-C-A-KEY-NAME-TABLE-IS-PER-APP-AND-A-WRONG-ONE-FAILS-SILENTLY — 2026-09-04 — OPEN

**In short.** Several apps handle keys by name — their handler takes a string
like `"Enter"` or `"Down"` and matches on it. Wiring such an app to the
compositor needs a small table turning a real key into one of those names, and
**the names are not the same from one app to the next**: `apps/flashcards` spells
the return key `"Enter"`, `apps/habits` spells it `"Return"`. A table that
guesses wrong does not fail, warn, or log: the key simply does nothing, and
every existing test still passes because they call the handler with strings
directly and never go through the table.

**It has already happened once.** Converting `apps/flashcards` on 2026-09-04, the
table mapped the return key to `"Return"` while every match arm in that app says
`"Enter"`. Enter did nothing in the deck list — the app's primary action. It was
caught only because a new test pressed a real `Key::Enter` and asserted the view
changed. Without that test the conversion would have shipped, green.

**Why this is a category and not one mistake.** The trap has three properties
that make it likely to recur:

| | |
|---|---|
| the vocabulary is invisible | it exists only as string literals scattered through five `match` statements |
| the failure is silent | an unmatched name falls into `_ => {}`, which is also how the app ignores keys it genuinely does not want |
| the old tests cannot see it | they call `handle_key("Enter", …)` directly, below the table |

**Proper fix, in preference order.**

1. **Stop translating.** The handler should take the `KeyEvent`, as
   `apps/procexplorer` and `apps/reminders` do. Then the compiler checks the
   match, and there is no vocabulary to get wrong. This is the real fix and the
   only one that removes the category.
2. Failing that, **every conversion of a string-keyed app must include a test
   that presses each named key as a real `Key` and asserts the effect** — not a
   test that calls the handler with a string. That is what caught this one.
3. A checker could compare the names a crate's table produces against the string
   literals its `match` arms accept, and report names that no arm can ever
   receive. That is mechanical and would have found this without a test.

**Where it stands now.** `flashcards` is correct and covered. `habits` and
`finance` were checked by hand on 2026-09-04: `habits` genuinely uses
`"Return"`, and `finance` has no return-key action at all, so its mapping is
inert rather than wrong. The remaining string-keyed apps have not been converted
yet, and each is an opportunity to hit this again.
