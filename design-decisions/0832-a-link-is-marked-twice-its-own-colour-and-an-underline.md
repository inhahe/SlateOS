## 832. A link is marked twice: its own colour, and an underline

**Date:** 2026-09-11
**Lane:** C
**Decided by:** Operator (spotted the ambiguity in the mock-up and asked for links to be distinguishable; Claude chose the specific colour and added the underline requirement)

**In short:** in a notification reading "4,812 files · 2.1 GB · 3 min 14 s" with
"Show report" underneath, there was no way to tell that the second line can be
clicked and the first cannot — they were the same colour with no other
difference. Links now have a colour of their own *and* are always underlined.

**What the operator saw.** §829 had given the accent the same value as secondary
text, and the notification's action was drawn in it with nothing else to mark
it. Their judgement — "they should be separate colors or have some other
indication that something is a link" — is the one this entry acts on.

**Why the answer is both, not either.** The operator offered the two as
alternatives. Colour alone is not one, because **WCAG 1.4.1 (Use of Color)**
forbids colour as the *only* visual means of conveying information: a reader who
cannot separate two blues would be left with nothing. So the underline is the
load-bearing half and is not optional. The distinct colour is the fast half —
it is what makes a link findable by glance rather than by scanning for
underscores — so it is worth having as well, not instead.

**The colour is `#0036A3`**, which is the accent the operator themselves chose
on 2026-09-09 (§826). It returns to exactly the job it is good at while the
cerulean keeps the roles §829 named for it — selected border, secondary text,
toggle-on. Measured against what it has to be told apart from:

| against | ratio | |
|---|---|---|
| the page `#EFF1F5` | 9.03 | comfortably readable |
| secondary text `#00688B` | 1.63 | distinct |
| main text `#000000` | 2.06 | distinct |

That balance is the reason for this value rather than a more separated one.
Pushing further from the cerulean walks *toward* black — `#0A2E7A` reaches 1.99
from secondary but falls to 1.68 from main text, at which point a link starts
reading as body copy. `#0036A3` is near the best joint separation available.

**This is §831 immediately earning its keep.** That entry kept `accent` a
separate role when it happened to share `subtext1`'s value, precisely so this
reversal would be one line. It was: the link takes its own value and every call
site that already said "accent" was untouched. Had the two been merged a day
ago, this would have meant finding, among every use of the shared value, the
subset that was a link.

**A defect the same review caught.** Of the three clickable things in the
mock-up, only one had been underlined — "Learn more" in the settings window —
while "Show report" and "Disconnect" had nothing. That is the failure mode the
rule is written against, arriving within a day of the rule. The check now counts
underlines rather than asserting one exists, so a single marked link cannot
stand in for all of them.
