## `TD-C-A-PRINTED-KEY-LIST-IS-A-SECOND-COPY` -- **FIXED 2026-09-18** (lane C)

**In short:** Five apps print a list of their keys on screen. That list is a
second copy of something the key handler already knows, and two copies of a
fact drift apart -- `apps/rssreader` once shipped an overlay of twenty-one
shortcuts of which about four worked. Only two of the five checked the list
against the handler, and **both did it through a third copy**. All five now
read the printed label itself, so there are two copies and a test that reads
both.

**Where each app stood before this.**

| App | Prints | Checked? |
|---|---|---|
| `minesweeper` | six keys, in a footer | that the footer was **complete** -- nothing about whether it was **true** |
| `wordsearch` | two tables, footer | six keys named **by hand** in the test; a seventh row would not have been checked |
| `mixer` | two tables, footer | both directions, but through a local `fn keys_named(label)` -- a match arm per label |
| `rssreader` | twenty-one rows, `?` overlay | a 21-arm `probe(action) -> Event`, first key of each row only |
| `slides` | twenty rows, `?` overlay | a 40-line `event_for(spec)` table, written the same day |

**The third copy is the part worth naming.** A printed list and a handler are
two copies, and the cure is a test that reads both. But a test that reads the
list and then *names the keys itself* has added a third, which drifts from the
other two and fails to catch either. Four of the five had exactly that, and
each wrote it independently -- `"Left/Right" => vec![Key::Left, Key::Right]`
appears in `mixer` and, in different words, in `rssreader`, `wordsearch` and
`slides`. Nobody was being careless: the check is obvious and the parser is
forty dull lines, so each author wrote the forty lines again.

**The fix is `guitk::shortcut::keystrokes(label)`**, which turns a printed
label back into the keystrokes it names -- `"Ctrl+PageUp / Ctrl+PageDown"` into
two strokes -- and refuses by name what it cannot read. It lives in the toolkit
because the *labels* are not app-specific: `Esc` versus `Escape` is not a fact
about a mixer. Each app keeps only what is genuinely its own, which is the set
of states to try the key in.

**Two things this turned up that were not visible from any one app.**

`rssreader`'s probe checked only the first key of each row, so the `Down` in
`"J / Down"`, the `Enter` in `"R / Enter"` and the `Shift+Tab` in
`"Tab / Shift+Tab"` were advertised to users and never once pressed by a test.
They all work. **Nothing knew that**, which is a different state from working.

And the new splitter dropped the second `/` of `rssreader`'s `"Ctrl+F / /"`,
where the slash is both the separator and a key. That is the failure this
whole entry is about, reproduced inside the fix for it: the guard test would
have checked one key where it meant to check two, **and passed**. A `/` with
nothing but blanks before it is now the key. The corpus test that should have
caught it was reading four apps' labels and not the fifth -- the eighth way a
search says nothing, "one file was read and the crate was not", in its
list-of-apps form.

**The property to assert is "some reachable state answers this key",** not
"this key is taken right now". The `slides` guard failed on its first run, on
its first row, and the app was right: `Left` at the first slide returns
`Ignored` on purpose, as do `1` for a view already open, `Ctrl+V` with nothing
copied, `C` on a number whose flags are missing, and `Esc` with no mark open.
**Declining from its own arm is answering.** The defect to catch is a row that
falls through to the catch-all in *every* state, so each guard offers its keys
to a small set of prepared states -- three decks, three boards, two grids --
and requires one of them to take it.

**Both new guards were mutation-checked**, since a guard test that has only
ever been green is a decoration: one row retyped to a key the program does not
answer (`B` to plain `C` in slides, `F` to `Q` in minesweeper), and each failed
naming the row. The first `slides` failure had been about *state* rather than a
missing handler, which is exactly why the deliberate one was worth running.

**2026-09-18, later: there is a third thing that can disagree, and it is the
box.** `apps/rssreader` -- the app this entry is named after -- was drawing
twenty of its twenty-one rows. The overlay's height was a hand-picked `520.0`
with a `break` when the rows ran past the bottom, so it drew as many as fitted
and stopped. **The row it dropped was `ShowHelp`: the overlay did not list the
key that closes it**, and had not since it was written.

Neither existing check could see it. The guard test reads the list against the
key handler, and *both of those were right* -- every one of the twenty-one keys
works and every one is in the list. The overlay's own size was a third
quantity, agreeing with neither, and the only thing that can catch a third
quantity is a reader that looks at the screen. So the pair of tests each app
needs is not one test done twice; it is:

| | Reads | Catches |
|---|---|---|
| `every_advertised_key_does_something` | the list against the handler | a row nothing answers |
| `the_shortcut_list_reaches_the_window` | the list against the *screen* | a row nothing draws |

rssreader had the first and not the second, because it already had an overlay
when the pattern was written and I checked the thing that was new rather than
the thing that was old. **The app that taught me a list and a handler drift
apart was itself silently dropping a row**, and the fix is that
`guitk::shortcut::render_card` computes its height from `rows.len()` instead of
being told a number -- the same cure as the description column, one line up.

**What is still open is the other half**, tracked in
`TD-C-KEYS-THAT-WORK-AND-NOTHING-MENTIONS`: five apps print no list at all, so
there is nothing to check. A list that is absent cannot be false, and is still
a user who cannot find the key.
