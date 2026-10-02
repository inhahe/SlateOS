## `TD-C-THE-REDRAW-SIGNAL-WAS-A-THIRD-LIST` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/jsonviewer` decides whether to draw a frame by comparing a
snapshot of its state before and after each event. Three kinds of change were
missing from that snapshot, so the program made them and then did not repaint:
**expanding a node**, **scrolling the raw or diff view**, and **editing the
document** -- deleting a node or committing a new value. The work happened and
the screen kept the old picture.

**The shape.** `handle_key` returns nothing, so the snapshot *is* the answer to
"did anything happen"; `handle_event` returns `Consumed` exactly when the
snapshot moved. That makes the snapshot a third list beside the state and the
renderer, and it drifts from both the same way a printed key list drifts from
its handler. A field nobody added is a change nobody can see.

**The comment was already right, again.** The snapshot's own comment read
"Scrolling and expanding are document state, and a wheel event that moved the
view has to read as a redraw" -- above a tuple containing neither a scroll
offset nor anything about expansion. That is the second time today a comment in
this tree stated an invariant the code beneath it did not keep; the first was
`apps/explorer`'s `icon_columns`, whose doc said it must not disagree with the
renderer while it did. **A doc comment stating an invariant is not an
invariant.**

**How it was found: by a test for something else.** The guard test for the new
shortcut overlay presses every key the list advertises and asserts the program
answers. `Enter` -- "expand or collapse" -- answered nothing, because expansion
was invisible to the snapshot. Then `Delete` for the same reason once edit mode
was on. Neither key was suspected of anything; the overlay work simply pressed
every key in the app with an honest definition of "answered", and three
redraw bugs fell out. **A guard test written for discoverability turned out to
be a redraw audit**, because both questions reduce to "does this key do
anything a user can perceive".

**The fixes, each at the level the fact lives at:**

| Change | Signal |
|---|---|
| expanding a node | the *count* of expanded paths -- one event toggles at most one, so the count always moves, and cloning a `Vec<Vec<PathSegment>>` per keystroke buys nothing |
| scrolling | the three scroll offsets, which are `f32` and compare fine |
| editing | a `revision` counter bumped in `invalidate_caches`, the one choke point every content change already passes through |

The revision counter is the one worth explaining: `input.len()` would have been
cheaper and wrong, because editing a `1` to a `2` changes no length. An exact
O(1) counter at a choke point beats a cheap proxy that is right about most
edits.

**`apps/flashcards` had it too, and worse.** Checked immediately on the
strength of the pattern, and its snapshot held `study_session.is_some()` --
whether a session *exists*, which is true from the first card to the last. So
`Space` set `flipped`, every field compared equal, and **the answer stayed
hidden**: in a flashcards program, the one interaction it is for. Cycling the
tag filter and shuffling the deck were invisible for the same reason. Found by
reading the snapshot rather than by using the app, which is the whole argument
for treating this as a class rather than three bugs.

Its snapshot is a struct now as well -- clippy refused the eleven-element tuple
outright, which is the tooling reaching the same conclusion by a different
road.

**`apps/finance` had it too. `apps/photomanager` does not have the pattern at
all** -- its `thumb_fingerprint` decides when to re-queue thumbnails, which is
a different question, and it has no redraw gate. So the class is **three apps,
not four**, and all three are now fixed.

finance's gap was `search_query`. Typing is safe there -- that path answers
`Consumed` outright, ahead of the comparison -- but `Backspace` comes through
`handle_key`, changes only the query, and answered `Ignored`. The search bar
draws the query with a caret after it, so **the deleted character stayed on
screen**. One half of an edit repainting and the other half not is worse than
neither repainting, because it reads as the key having failed.

**All three snapshots are structs now**, and each arrived there by a different
road: `jsonviewer` because Rust implements `PartialEq` for tuples only up to
twelve and it needed sixteen, `flashcards` because clippy refused eleven, and
`finance` because seven fields was the last legible size and the fix made it
eight. Three independent signals that a positional list of heterogeneous state
is the wrong shape -- and a reader adding a field to one has no way to check
they put it in the right place, which is how all three came to be missing one.

**A near-miss worth recording, because it is the fifteenth shape again.** While
auditing finance I ran `grep -n search_query ... | head -6`, saw only a `clear`
and a `pop`, and was a sentence away from filing "the search box can never
contain anything -- it is decorative". The writer is at line 903,
`search_query.push_str(text)`, seventh in the list. **The `head` truncated the
evidence and I read the truncation as the answer**, which is shape 14 with
`head` where that row has `tail`. The same command with no limit settled it. The cheap way to check them is the
programme already running: give each one the shortcut overlay and its guard
test, and any field missing from its snapshot shows up as a key that the list
advertises and the program says it did not answer. **The discoverability sweep
doubles as a redraw audit on exactly the apps that need one**, which is a
better reason to prioritise those three than their key counts.

**The snapshot is a struct now, not a sixteen-tuple.** It had to become one --
Rust implements `PartialEq` for tuples up to twelve -- but it was past readable
well before it was past legal: `(usize, String, usize, bool, bool, bool,
String, usize, ...)` gives somebody adding a field no way to check they put it
in the right place, which is precisely how three fields came to be missing.
