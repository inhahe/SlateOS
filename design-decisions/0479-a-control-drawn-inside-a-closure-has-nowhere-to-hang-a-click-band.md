## §479 — A control drawn inside a closure has nowhere to hang a click band

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the Settings app offered six little pictures to choose an account
avatar from. All six were painted by one drawing call, so the part of the app
that answers "what is under the pointer?" saw one opaque blob rather than six
tiles — there was nowhere to register six click bands even in principle. The
first tile was hardcoded as the outlined one and nothing stored a choice. The
fix loops per tile, naming the tile geometry once so the drawn square and the
clickable square are the same expression. The decision worth recording is a
narrower one about *whose* picture the page edits, and a testing rule that this
lane had not previously written down.

### Why a closure is the wrong container for a control

`PageSink` has two implementations that matter: one paints, one hit-tests.
`draw(f)` hands `f` to the painter and *discards it unevaluated* in the
hit-tester. That is the right design — it is why a page is written once and
answers both questions — but it means anything a closure computes internally is
invisible to the click side by construction. Six tiles in one closure is not
"six controls that were forgotten"; it is **zero controls**, drawn.

So the shape of the fix is forced: one loop iteration per tile, with
`hit_rect` and `draw` called at the same offset from the same
`picture_tile_dx(idx)`. That is §475 again (a page drew rows and hit-tested
them separately), and it is now the fifth in this family — §475, §476, §477,
§478, §479.

### The call that had a real choice in it: which account gets edited

`SettingsState` has two notions of "an account we care about":

| | Means | Set by |
|---|---|---|
| `is_current` | the account signed in on this machine | the system |
| `selected_account` | the row the Accounts page has highlighted | clicking a row |

On a fresh state they are the same account, so either would pass a naive test.
They are chosen deliberately to be `is_current`, because the Login Options page
says "Choose a picture for **your** account". Editing
`user_accounts[selected_account]` would mean that browsing to Bob's row, then
switching to Login Options, silently retargets a control whose label still says
"your". A control whose meaning depends on where the user has been is a control
that cannot be labelled correctly.

The counter-argument is real and was weighed: an administrator plausibly *wants*
to set another user's picture, and `selected_account` is the obvious handle for
that. Rejected because it would be an admin feature wearing a personal-settings
label. When account administration exists, it belongs on the Accounts page next
to the row it edits — where `selected_account` is the right question — not
smuggled into a page about yourself.

### Two smaller calls

**`UserAccount::picture` is an index into `ACCOUNT_PICTURES`, not an icon
string.** One list then decides both what the grid offers and what the account
list draws; they cannot drift into offering different sets. The cost is that an
index can be out of range, which a string cannot — paid for by
`set_current_account_picture` refusing an out-of-range index outright, so the
stored field never names a picture that does not exist.

**No signed-in account draws as "nothing outlined", not as tile 0.** The
accessor returns `Option<usize>`. A machine with nobody signed in should not
claim a choice was made — which is the same instinct as §478's dimmed buttons:
do not let the UI assert something the state does not know.

### Testing note — two rules this lane had not written down

**A hit band is a rectangle; a probe at its centre tests a point.** Shifting
every band 12 px off its tile survived the first mutation run, because the bands
are as wide as the tiles and a small offset still covers the middle. Only a
click near an edge asks whether the clickable square *is* the square on screen.
Corner probes now, and this generalises to every control in the app.

**Collapsing the paint and the band into one expression buys correctness at the
price of one blind spot, and the price is right.** A wrong `picture_tile_dx`
moves both together, so no test that checks them against each other can see it.
The remedy is not to un-collapse them — that is the bug this family keeps
fixing — but to add one check against something with **no shared origin**:
`the_picture_tiles_stand_apart_inside_the_content_column` measures the painted
tiles against the window. Overlapping tiles (only the leftmost of a stack is
reachable) and a strip running off the content column both fail there. Same
move as `no_cell_is_drawn_over_the_chrome` in §477, and it is now the standard
companion to any single-expression collapse.
