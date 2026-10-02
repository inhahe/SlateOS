## §299 — A gate's trigger is part of its rule: the usage-status checker fires on the category, not on the word `Usage`

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the shell has a build-time check that a command which prints an
error message must also tell the caller it failed. It found those messages by
searching for the word `Usage:`, so a command that scolded the user with
`Unknown mode: banana` instead passed the check while still reporting success —
49 of them did. The check now looks for the *kind* of message rather than one
word of it, and all 49 are fixed. What is being decided here is where that line
is drawn: which words count, and why the answer is a structural rule rather
than a longer list of exceptions.

**The shape of the mistake, for the third time.** §296 already moved this
checker's *walk* from a syntactic pattern to a semantic property — from "a
`Usage:` print followed by `return;`" to "a diagnostic that can be reached and
then left without a non-zero `set_exit`". That fixed the 87 sites the original
sweep's `return;`-shaped pattern could not see. But the *trigger* — how a line
gets nominated as a diagnostic in the first place — was left as the literal
string `Usage:`, which is the wording the first bug happened to have. So the
checker was complete over messages containing that word, and silent about every
diagnostic worded any other way. `cmd_quota`'s catch-all said `Use:` and
passed. Widening it turned up 49 more, in 29 further functions.

A gate has two halves and both are its rule. Making the walk semantic while
leaving the trigger lexical does not remove the blind spot; it moves it one
level down, to a place that is harder to notice precisely because the visible
half now looks principled.

**What "the category" was taken to be.** The five openings that fire the
trigger are `Usage`, `Unknown`, `Unrecognised`/`Unrecognized`, `Invalid`, and
`Use:`. The category they are meant to stand for is *the shell naming
something the user typed back at them as wrong* — which is exactly the
condition under which a caller needs a non-zero status, because it is exactly
when the command did not do what it was asked.

| Option | Against |
|---|---|
| Keep `Usage:` and add words as bugs are found | This *is* the bug, iterated. Each addition is keyed to the last miss, and the list is only ever complete over the diagnostics someone has already tripped over. |
| Trigger on *any* `shell_println!` in a block that can return without a status | Fires on every report, every progress line and every success message in the shell — thousands of sites, no signal. The rule would be deleted within a week. |
| **Five category words, anchored to the start of the message** | Still a word list, and a diagnostic worded `"no such mode: banana"` escapes it. Accepted: it is a far larger net than one word, and the residue is recorded below rather than being pretended away. |
| Classify by call-site semantics (does the block correspond to a rejected input?) | That is the `check-query-status.py` problem, and that checker needed a hand-written `ALLOWED` for two sites out of a tree this size. Doing it for *every* diagnostic is a much larger classifier with a much larger error budget, and its errors are silent. |

**Two structural exclusions, chosen over allowlist entries.** Widening the
trigger produced exactly two false positives, and both are killed by structure:

* **The word must start the message.** `vlan stats` prints
  `"  Unknown drops:    {}"` — a *field label*, indented inside its report.
  A diagnostic starts at the beginning of the string. (`Usage` keeps its
  leading-whitespace tolerance, because `cmd_memcg` has an indented one that
  already sits in `ALLOWED`, and removing the tolerance would silently drop a
  site the gate is currently watching.)
* **The words are matched with `\b`.** `thumbcache` prints
  `"Invalidated {} entries for: {}"` on *success*; without the word boundary
  that is an `Invalid` anything.

The reason to prefer structure here is that an `ALLOWED` entry is checked once,
by the person adding it, and then never again — whereas a structural rule keeps
applying to sites nobody has written yet. Two entries would have covered
today's tree; the next indented `Unknown` field label would have been reported,
and the temptation would be to add a third entry rather than to notice the
pattern. The allowlist is for sites that are genuinely exceptions, not for
categories the trigger should never have caught.

**Why the sweep imported the checker instead of restating its rule.** The 49
fixes were applied by a scratch script that loaded `check-usage-status.py` and
took the site list from the checker's own output, rather than re-implementing
"find the diagnostics" a second time. §297's rule: two descriptions of one set,
maintained separately, differ silently — and here the difference would have
been invisible, because a site the sweep missed and the gate found would look
like a *new* bug, while a site the sweep fixed and the gate did not care about
would look like nothing at all. The script also refused any shape it did not
recognise and printed the refusal, rather than doing its best: one site
(`cmd_fstrim`'s `Unknown mode`, whose print is a block's tail expression with no
trailing semicolon) came out as `MANUAL` and was fixed by hand. A mechanical
pass that guesses at an unfamiliar shape is how the mirror-image bug gets
written. The script was deleted after use; the gate is what persists.

**What is still not covered, stated rather than implied.** A diagnostic that
opens with none of the five words — `"no such mode: banana"`, `"cannot parse
…"`, `"expected a number"` — is not nominated, and the gate will pass it.
That residue is real and this decision does not close it; what it closes is the
much larger class where the shell says `Unknown X` and then claims success. If
a fourth instance of this defect appears, the question to ask is not "which
word do we add" but whether the trigger should become a property of the *block*
(does this branch correspond to input the command rejected?) rather than of the
*string* — with the `check-query-status.py` experience as the estimate of what
that costs.
