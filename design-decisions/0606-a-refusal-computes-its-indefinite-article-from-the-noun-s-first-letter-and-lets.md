## 606. A refusal computes its indefinite article from the noun's first letter, and lets the caller override it

**Date:** 2026-08-26
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When the shell cannot read a word, it says `` `zz' is not a
window id ``. The article was a literal `a` in the helper's format string,
so any noun beginning with a vowel came out as `` is not a element id `` —
visibly broken English in the one message whose whole job is to be believed.
It is now computed. The tricky part is that English picks the article by
*sound*, not spelling, so no letter rule can be right about "a UID"
(yoo-eye-dee) or "an hour"; those callers write the article into the noun
themselves and the helper stands aside.

**Why not just rename the offending nouns.** That was the first fix
attempted, and it worked for the case at hand — "x coordinate" became
"horizontal coordinate", which is a better noun anyway. But a survey found
four vowel-initial nouns already shipped ("UID", "alpha (0-255)", "element",
"inode ratio"), and renaming is a fix that must be re-applied by every future
author, silently, with no gate to catch a miss. The wording gate (§604) will
reject a self-test assertion that predicts the *correct* article while the
kernel prints the wrong one — which is how this was found — but only for
nouns some rung happens to assert on. Renaming treats instances; computing
treats the class.

**The escape hatch is the whole design.** `article_for` returns `""` when the
noun already starts with `"a "` or `"an "`. So a caller with a noun the
letter rule gets wrong passes `"a UID"` instead of `"UID"` and gets the right
sentence. This is deliberately not a lookup table of exceptions: a table
lives far from the call site, has to be found before it can be consulted, and
grows without bound. Putting the override in the argument puts it where the
person choosing the noun is already looking.

**What is knowingly wrong.** The rule is first-letter-is-a-vowel, which
mispredicts every noun whose *pronunciation* disagrees with its spelling —
consonant-sounding vowels ("a UID", "a one-shot") and vowel-sounding
consonants ("an hour", "an FD" — eff-dee). Shipping a heuristic known to be
wrong is only defensible because the failure is loud, local, and cheap: it
produces one ungrammatical word in one error message, visible to whoever
writes the noun, fixable in place by writing the article in. Compare the
alternative of a pronunciation dictionary in a kernel, which is not a
serious proposal.

**Cost of being wrong.** Cosmetic in both directions, but not free: the whole
argument for the §600 refusals is that a command that declines to act must be
*convincing* about it. A message that reads like a typo invites the reader to
assume the shell is broken rather than that their input was, which is the
opposite of what a refusal is for.

**Where it came from.** Rung 78 of `kshell::self_test`, which asserted
`` `zz' is not an element id `` and was rejected by the wording gate because
the kernel could only produce `` a element id ``.
