## `TD-C-NOTES-CANNOT-TAG-OR-DELETE-A-NOTE` (lane C, 2026-09-17)

**In short:** `apps/notes` offers "Tagging system with tag-based filtering" in
its feature list, and there is no way to put a tag on a note. There is also no
way to delete one. Both operations are written, tested, and have no caller
outside the test module -- the same shape as `apps/photomanager`, which took a
session to wire up and had the same list of symptoms.

**Verified, not inferred:**

| Claim | State |
|---|---|
| tags on a note | `NotesApp::add_tag_to_note` and `remove_tag_from_note` have no production caller. The only other writer of `Note::tags` is `Note::add_tag`, reached from that method and from `seed_sample_content`, which is itself test-only. |
| tag filtering | `set_tag_filter` is the only writer of `active_tag_filter`, and has no production caller. |
| deleting a note | `delete_note` has no production caller. The only other route is `delete_notebook`'s cascade, which has no production caller either. |

**The trap, which is the transferable part.** The probe that found these also
flagged `pub fn search` as test-only, and search *works*. The application
filters through a different path -- `search_query` is written directly by the
key handler at lines 2029 and 2039 and read by `matches_search` -- so the
unreachable method is a second door to a room that already has one.

**An unreachable method does not imply an unreachable feature.** Going the
other way is safe (a feature with no reachable writer is genuinely dead), but
the method-level measurement is a *candidate list*, and each candidate has to
be chased to the feature before it means anything. I nearly filed "notes
cannot search", which is false.

**The method, for whoever picks this up.** For each `pub fn` on the app type,
count call sites before the `^mod tests` line. Note `^mod tests` and not the
first `#[cfg(test)]`: an item-level attribute appears hundreds of lines
earlier in several of these files, and cutting there hides most of the
application -- see the entry above about three checkers doing exactly that.

**The other seven, chased the same way.** All unreachable, each checked for an
alternative route rather than counted:

| Operation | The only writer, and where it is reached from |
|---|---|
| rename a notebook | `nb.name = ...` inside `rename_notebook`. No caller. |
| move a note | `note.notebook_id = ...` inside `move_note`. No caller. |
| retitle a note | `note.title = ...` inside `update_note_title`; the only other assignment is in `seed_sample_content`, which is test-only. |
| remove a checklist item | `checklist.remove` inside `Note::remove_checklist_item`, reached only from the app method of the same name. No caller. |
| resolve a wiki link | `resolve_links` and `build_backlinks`. No callers, and nothing else in the file mentions backlinks -- so `[[Note Title]]` is text that never becomes a link. |
| restore a version | see below. |

**Version history is the sharpest one, because it nearly works.** Versions are
genuinely recorded: `Note::set_content` snapshots the old text and it has a
production caller, so editing a note really does accumulate history. The
sidebar that lists them is drawn -- `render_version_sidebar`. And
`restore_version` has no caller, so the application shows you a history you
cannot restore from. Everything except the last click is built.

**Tally for the claims in the module doc.** Of fourteen listed features, four
are contradicted: tagging with tag-based filtering, wiki-style linking,
version history *with snapshot restore*, and notebook organisation to the
extent that a notebook cannot be renamed and a note cannot be moved between
notebooks. "Full-text search" is real. The rest were not examined.

**They are not nine oversights. The application has no mouse.**

`apps/notes` handles `Event::Key`, `Event::Resize` and `Event::CloseRequested`,
and nothing else. It imports no `MouseEvent`, no `MouseButton`, no
`MouseEventKind`; `grep -c "Event::Mouse\|MouseEvent"` is 0, and there is not
one hit-test function in the file. It draws a notebook sidebar, a note list
and an editor -- three panels, none of which can be clicked anywhere.

So every operation must be a keyboard shortcut, and the keyboard covers about
eight of them: Tab, Up/Down, `/` and Ctrl+F for search, Ctrl+S and S to save,
P to pin, V to favourite, B for bold, Escape/Enter, Backspace. The nine in the
table above have neither a key nor a click. That is the whole of the defect,
and it explains why the list reads like a cross-section of the application
rather than a set of related gaps.

**It is not a documented keyboard-only design.** There is no comment saying
so, the module doc advertises a "Multi-panel UI", and three mentions of a
shortcut in the whole of the production code is not a keyboard-driven
application either. It is an application with a mouse-shaped interface and no
mouse.

**So the repair is not nine wires.** It is a pointer layer -- hit-tests for
the three panels, mirroring the way `apps/photomanager` derives every
clickable rectangle from one function the renderer also reads -- and then the
nine operations have somewhere to hang. Adding nine more keyboard shortcuts
would reach them too, and would leave a program whose sidebar still does
nothing when clicked.

**PROGRESS 2026-09-17: seven have a route, and the pointer layer is done.**
All four panels answer a click -- the version panel, the note list, the
notebook tree and the tag cloud.

| Operation | Route |
|---|---|
| restore a version | click it in the version panel |
| delete a note | right-click the note |
| move a note to another notebook | right-click, Move to |
| tag a note | right-click, Add tag..., type, Enter |
| filter by tag | click the tag; click it again to clear |
| rename a notebook | right-click it, Rename |
| delete a notebook | right-click it, Delete |
| remove a tag | **none** |
| retitle a note | **none** |
| remove a checklist item | **none** |
| resolve a wiki link | **none** |

**The four that remain are not waiting on a hit test.** Every panel takes a
pointer now, so what is missing in each case is a control rather than a layer:

* *remove a tag* -- the chips are drawn in the sidebar for filtering, and a
  note's own tags are not drawn anywhere. There is nothing to click yet.
* *retitle a note* and *remove a checklist item* -- both are edits **inside**
  the editor, where the text already lives. A menu is the wrong shape for
  them; they want the editor to be editable in place.
* *resolve a wiki link* -- `[[Note Title]]` has to become clickable text
  within the editor's own content, which no other control here does.

So the cheap half is finished and the rest is editor work.

**A smaller bug found while reading the version panel for a hit-test.** It
computes how many rows fit with `(height - 30.0) / 24.0` and then advances
`vy` by `28.0` per row. At a 740-pixel panel that is 29 rows drawn 28 apart in
812 pixels, so the last few are drawn past the bottom of the panel they are
in. One of the two numbers is wrong and they should be one constant.
