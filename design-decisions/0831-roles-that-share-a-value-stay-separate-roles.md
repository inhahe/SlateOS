## 831. Roles that share a value stay separate roles

**Date:** 2026-09-11
**Lane:** C
**Decided by:** Operator (Claude had framed the shared value as an open question; the operator pointed out it need not be one)

**In short:** two of the palette's text colours are now the same colour. The
question was whether to merge them into one. The answer is no — keep both
names, let them hold the same value. Changing one of them back later is then a
one-line edit instead of a hunt through a thousand places that say the wrong
name.

**The operator's argument, which is simply correct.** A palette role is a level
of indirection. The expensive thing is never the constant, it is the call sites:
`subtext0` is named at **1,087** places and `subtext1` at **161**. Merging them
means rewriting 161 call sites now *and* losing the record of which places meant
"the quieter one" — so a later split would have to reconstruct that by reading
all 1,248. Keeping both names costs one unused-looking constant and makes the
reversal a single line.

**What I verified before agreeing, because it could have cut the other way.**
If the two roles had ever been visibly different, equalising them would be
destroying real information. They never have been:

| | separation |
|---|---|
| upstream Latte, `#6C6F85` vs `#5C5F77` | 1.27 |
| Mocha (dark mode), `#A6ADC8` vs `#BAC2DE` | 1.26 |
| after §826, `#3D3D3F` vs `#373739` | 1.10 |

Every one of those is at or below the 1.30 that reads as "barely distinct", and
the last is "one colour". So the values were already effectively equal in both
modes and always had been; making it explicit changes nothing anyone could see.

**The caveat that follows from the same fact, and it bounds the future work.**
Because the two were never distinguishable, *nobody has ever been able to check
a call site by looking at it*. The 1,087/161 split records what each author
believed the role meant, never corrected by eye. So the reversibility this
decision buys is real but not total: a future split gets the palette change for
free, and should still review the **161** `subtext1` sites — the smaller set —
rather than trusting them. That is a bounded afternoon, not an audit of 1,248.

**The corollary matters more than the decision.** The same reasoning applies to
the accent, which §829 gave `subtext1`'s value. `accent` stays its own role too.
That is what makes the open link-versus-caption question cheap: if an underline
turns out not to be enough to separate something you can click from something
you can only read, giving the accent back a value of its own is one line, and
every one of its call sites is already labelled `accent` rather than having been
rewritten to say `subtext1`.

**Stated as a rule, since it will come up again:** when two roles converge on
one value, change the *values* and leave the *names* alone. Deleting a role is
only correct when the distinction itself is wrong, not when the current palette
happens not to use it.
