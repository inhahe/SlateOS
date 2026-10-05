### [A] The byte count I used as proof was anti-correlated with the truth -- 2026-09-17

**Status:** OPEN

The strongest single piece of evidence this session produced for its own
recurring lesson, and it is against me.

I wired `binfmt` so something would finally report to it, and confirmed the
fix by watching `/proc/binfmt` grow from **49 to 104 bytes** -- the only
signal available, since the procfs self-test prints sizes and not content.
I wrote that the disclosure was "confirmed live" on that basis.

Then I added a report that prints the counts, because a byte count cannot
distinguish "a format row exists" from "anything records into it". It said
`0 format(s), 0 load(s)` -- the registration was being wiped by binfmt's own
self-test, which re-opens its table empty by design. After moving the
registration to after that dispatch:

```
[binfmt] 1 format(s), 11 load(s), 0 error(s)
```

And the three byte counts, in order:

| state | `/proc/binfmt` |
|---|---|
| no wiring at all | 49 bytes |
| wiring present, registration wiped -- **broken** | **104 bytes** |
| working: 1 format, 11 loads | **51 bytes** |

**So the number I cited as proof was the broken state, and the working state
is two bytes off the unwired one.** The proxy did not merely fail to
distinguish the cases; it moved in the *opposite direction* from the property
it was standing in for. Had I trusted it, I would have shipped a statistics
module that records nothing and a commit message asserting it works -- and
the next person to read `/proc/binfmt` would have found a plausible, empty
file.

**Why it went the wrong way**, since a rule needs a mechanism and not just an
anecdote: the broken state prints an empty-table form, and the working state
prints one compact format row. Size tracked the *shape* of the output, which
has no fixed relationship to whether a counter is being incremented. That is
the whole of it -- a proxy is only evidence if you can state why it moves
with the thing, and I never did.

**What this does not say.** Byte counts were the right instrument for the
dd-945 `/proc` disclosures earlier the same day: there the question was "is
this string being served", growth of the exact length of the added sentence
answers it, and an unchanged control (`devpower` at 362 both sides) made it
specific. Same instrument, different question, opposite verdict on its
fitness. The error was not using a proxy; it was not asking what the proxy
was a proxy *for*.
