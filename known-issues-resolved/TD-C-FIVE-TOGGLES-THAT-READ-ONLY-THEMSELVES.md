## `TD-C-FIVE-TOGGLES-THAT-READ-ONLY-THEMSELVES` (lane C, 2026-09-21) -- **CLOSED 2026-09-22**

> **Closed. All five, by two different routes -- and the split is the point.**
> Three of them wanted a reader and got one: `camera`'s `fullscreen_preview`
> now picks a `Layout::fullscreen()` (main.rs:1666), `logviewer`'s
> `wrap_lines` chooses wrapping over eliding in `message_lines`
> (main.rs:1520), and `videoplayer`'s `chapter_list_visible` draws a real
> chapter list over the tab bar (main.rs:3247). Two wanted **deletion**:
> `editor`'s `use_regex` and `settings`'s `checking_for_updates` are gone,
> each leaving a one-line tombstone where the field was (editor:1380,
> settings:4437) so the next reader learns why rather than re-adding it.
>
> **"Give it a reader" was not the fix for all five, and assuming it would
> be is the mistake this entry nearly made.** A toggle with no reader is a
> claim the program cannot keep; the repair is to make the claim true *or*
> to stop making it, and which one depends entirely on whether the feature
> is wanted. Wiring `checking_for_updates` to something would have meant
> inventing an update check; wiring `use_regex` would have meant writing a
> regex engine into a text editor's find bar. Deleting a control is a fix,
> not a retreat from one.
>
> **The fix to the checker described below was not made**, and that is
> deliberate rather than forgotten: `x.f = !x.f` still reads as a use to
> `scripts/check-fields-written-never-read.py`. Two of the five no longer
> exist and three now have genuine readers, so the gate would report
> nothing today either way -- it would be a change with no test that can
> fail. It is worth doing the next time a self-toggling field appears,
> which is the condition to act on, and the route that actually found
> these (asking what a key *did*) works regardless.

**In short:** five programs have a switch you can flip that nothing anywhere
looks at. `apps/editor`'s `Ctrl+E` is the clearest: it turns "use regular
expressions" on and off in the find bar, and the flag it sets has exactly three
mentions in the whole crate -- where it is declared, where it is initialised to
`false`, and where `Ctrl+E` inverts it. No search code consults it. Pressing the
key does nothing at all, and nothing on screen changes either, so there is not
even a wrong answer to notice.

**The five**, each the only live mention of the field being its own toggle:

| app | field | the control |
|---|---|---|
| `apps/editor` | `use_regex` | `Ctrl+E` in the find bar |
| `apps/camera` | `fullscreen_preview` | |
| `apps/logviewer` | `wrap_lines` | |
| `apps/settings` | `checking_for_updates` | |
| `apps/videoplayer` | `chapter_list_visible` | |

**Why the existing gate does not catch them, which is the part worth keeping.**
`scripts/check-fields-written-never-read.py` exists for exactly this defect and
reports `ok: no new write-only fields`. It is right by its own rule: a field
written as

```rust
self.find.use_regex = !self.find.use_regex;
```

**is read** -- by the expression that writes it. A boolean toggled against
itself reads itself once, and that single read is enough to look used. The
detector's own docstring records it learning to match any receiver so it could
see a test read; this is the same lesson one turn further on, where the read it
can see is one that means nothing.

**The fix to the checker** is to discount a read that appears on the
right-hand side of an assignment to the same path -- `x.f = !x.f` should count
as a write and not as a read. That is a small change and it is what turns these
five from a thing I noticed into a thing the tree reports.

**How they were found**, because the route matters: not by the gate and not by
reading the apps, but by the key-list programme. `apps/editor` showed five keys
the survey said were never named; tracing `Ctrl+E` to say what it *did* is what
turned up a flag with no reader. The key survey keeps surfacing defects that
are not about keys, because "what does this key do" is a question nobody had
been asking of these apps.

**If this is never done,** five settings keep lying: a user turns regex
searching on and gets literal matching, with the switch apparently accepted.
`apps/editor`'s is the worst of the five because the find bar also draws no
indicator, so both the setting and its failure are invisible.
