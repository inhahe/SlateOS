## `TD-C-AUTHORING-APPS-THAT-CANNOT-AUTHOR` -- **ALL THREE FIXED 2026-09-18** (lane C)

**In short:** Three of our content-creation programs cannot create content.
You can add slides, notes and diagram nodes; you cannot put a word in any of
them. Each has its whole periphery built -- themes, notebooks, templates,
versions, export -- and is missing the one act it exists for.

| App | What it can do | What it cannot |
|---|---|---|
| `slides` | add slides, four shapes, images; themes, transitions, sorter view, undo, export -- **and, since 2026-09-18, type** | ~~**type anything.**~~ *(fixed)* Zero assignments to `.text` in the crate, tests included; every element born "New Text" / "Presentation Title"; deck permanently "Untitled Presentation" |
| `notes` | notebooks, tags, versions, search, export to text/Markdown/HTML -- **and, since 2026-09-18, make and write a note** | ~~**make a note.**~~ *(fixed)* `create_note`, `update_note_title`, `update_note_content` all callerless; both `notes.push` sites are inside the unreachable creators; there is no import. It exports notes it cannot create |
| `diagram` | insert canned flowcharts and org charts, move and connect nodes, export SVG/JSON -- **and, since 2026-09-18, label a node or an edge** | ~~**label anything.**~~ *(fixed)* `set_node_label` and `set_edge_label` callerless, **zero** typing sites in the crate. The only writer of `.label` besides them is `add_template_node`, so every box says what the template said |

**`diagram` is the one that shows what the class costs.** Its 32 reachable
`add_template_node` calls build a flowchart reading Start → Process →
Decision? → Action A → Action B → End, and an org chart of "CEO" and "VP Eng".
Those labels are not placeholders; they are **somebody else's example**, and
they are permanent. A user's diagram of their own system is always a diagram
of ours.

**Why a sweep for unreachable operations does not find this.** That sweep asks
which *written* functions have no caller. Here the functions are missing
outright -- nobody wrote `set_element_text` for slides, so there is nothing
callerless to report -- or, in `diagram`'s case, they exist and the sweep does
report them, but as two entries among fifteen, indistinguishable from
`next_slide` (a redundant duplicate) and `light` (an unused theme
constructor). **The periphery being thorough is what hides it:** 90 tests in
slides, four panels and a version history in notes, templates and layers and
alignment guides in diagram.

**The question that finds it**, and it has to be asked per-app because the
answer is never generic: *what is this program for, and can a user do that?*
For these three it is one sentence each -- put words on a slide, write a note,
say what the box means.

**A caution against the obvious fix.** The repair is a text-entry mode in each,
on the pattern `apps/markdowneditor` and `apps/rssreader` now use, and all
three already have the accessor it needs (`element_by_id_mut`,
`update_note_content`, `set_node_label`). But adding one and stopping is how
this happened: every one of these apps looks finished from the inside because
everything *around* the hole is built. The test worth writing is not "the key
sets the field" but **"a user can produce the artifact the program is named
after"** -- type into a new note, save it, and read it back.

**All three fixed the same day**, each with an end-to-end test that asks for
the artifact rather than for the field: `a_user_can_make_a_note_and_write_in_it`,
`a_user_can_put_words_on_a_slide`, `a_user_can_label_a_node`.

**Every one of the three end-to-end tests failed first, on something a unit
test of the key handler would have passed.** `notes` needed a notebook
invented for the first note, because the app starts with none and
`create_note` takes an id. `slides` produced "New TextHi", because seeding the
buffer from the box is right for an edit and wrong for a box still holding its
prompt. `diagram` drew "New" on the canvas and "Old" in the properties panel
at the same moment -- one value disagreeing with itself on one screen. **The
assertion that catches all three is the same one: ask for the artifact and
read what comes out.**

**The near-miss in `diagram` is the one to remember.** `Backspace` is bound
there to *delete the selection*, so a typo while naming a box would have
deleted the box, with the undo stack the only record. The mode taking the
keyboard first is what prevents it, and
`backspace_while_labelling_does_not_delete_the_node` is what keeps it
prevented -- the same shape as `rssreader`'s digits and `spreadsheet`'s bare
`T`. **A text mode is not finished when it accepts text; it is finished when
it stops the keys underneath it.**

**Related.** `TD-C-A-PRESENTATION-EDITOR-THAT-CANNOT-TYPE`,
`TD-C-NOTES-CANNOT-MAKE-A-NOTE`, and the method note
`TD-C-SEVEN-WAYS-A-SEARCH-SAYS-NOTHING-AND-MEANS-NOTHING` -- particularly its
corollary, since each row here is stated as *nothing writes this* rather than
*this function has no caller*, which is the only form that survives a second
implementation path.
