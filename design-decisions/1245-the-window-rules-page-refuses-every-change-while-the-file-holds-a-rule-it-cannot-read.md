## 1245. The Window Rules page refuses every change while the file holds a rule it cannot read

**Date:** 2026-10-10 · **Decided by:** Claude (autonomous) · **Lane:** E

**In short:** Settings' new Window Rules page saves the rules with lane C's
`windowrules::file::store`, which writes the rules it is given and nothing
else. A rule someone wrote by hand with a typo -- `taskbar: flase` -- cannot
be read, so it is not among the rules the page holds, and the first change on
the page (flipping any switch) would delete it from the file. So while the
file has such a rule, the page lists it under "Not Applied" with why, and
refuses every change -- each control dimmed, saying that saving would delete
the rule -- until the user mends it in the file or presses "Remove them". A
user with one typo cannot use the page until they deal with it; nobody loses
a rule they wrote without being told.

### What was chosen

`rules_blocked()` is the one test: true while `windowrules::file::load`
reports a problem. Every control that would save is drawn dimmed with its
reason (`Press::Cannot`, `named_unavailable_row`), and `store_rules` -- the
line every change passes -- refuses as well, so a change cannot reach the
file by a way round the controls. "Remove them" clears the problems and
stores the rules that can be read: the one save that drops the unreadable
rules, taken on purpose. `requests/e-c-a-window-rule-the-file-cannot-read-is-deleted-by-the-next-save.md`
asks lane C for `store` to keep them; when it does, the refusal goes.

### Alternatives

| | What changes | For | Against |
|---|---|---|---|
| **A. Refuse every change, say why, offer "Remove them" (chosen)** | the page is read-only while the file has an unreadable rule | nothing is lost without the user choosing it; one rule, one line to lift when lane C's fix lands | one typo makes the whole page unusable until it is dealt with |
| B. Save anyway, and say what was dropped | changes work; the unreadable rules vanish from the file, and the page says so after | the page always works | the user's own text is deleted by a click on something unrelated -- the failure the desktop's "a rule is not applied" notice exists to prevent, and after the save that notice stops, since the file no longer has the problem |
| C. Ask before each save: "this will delete Chat" | each change asks | the user decides each time | a question on every switch, about a rule they may not remember writing; and a "yes" is the same loss as B |
| D. Settings writes the file itself, keeping the unreadable rules' text | changes work and nothing is lost | the best behaviour today | a second writer of lane C's format, in another lane's program -- two writers drift, and the format's owner is the right place for "keep what you cannot read" |

D's behaviour is the right end state, in the crate that owns the format,
which is what the request asks for. Until then A loses nothing, at the cost
of the user mending a typo first.
