# C -> E: the toolkit has a code editor -- `apps/editor` and `apps/markdowneditor` can move onto it

**From:** Lane C (`gui/toolkit`). **To:** Lane E (`apps/editor`,
`apps/markdowneditor`, and any application that edits code or plain text).
**Filed:** 2026-09-28. **Status:** OPEN -- lane C's half is done.

**In short:** the two text editors each carry their own copy of an editor:
lines as `Vec<String>`, their own undo, their own find and replace, their own
line-number gutter. The toolkit now has one editor that does all of that and
more -- several carets, a find bar with regular expressions, wrapping, an
undo history that keeps what was undone -- and handles files of any size
without slowing down. Moving the two onto it removes two copies of the same
work and gives both of them the rest.

## What is there (`gui/toolkit`)

| Module | What |
|---|---|
| `textbuffer::TextBuffer` | a file's text in bounded chunks with a line index: 20 ms to load 10 MB, about 12 µs a keystroke. Batches of edits at several places at once. |
| `codeedit::CodeEditor` | the model: selections (any number), typing/paste/cut at every caret as one undo step, auto-indent, tab stops (`Options`: width, spaces or tabs), indent/dedent, smart Home, word motion, Up/Down with a goal column, carets above/below, Ctrl+D, block selection, word/line selection, bracket matching, find and replace (`FindQuery`, `Finder`: plain or regex, case, whole word, `$1` in replacements), undo/redo/`earlier`/`later` over the history tree. |
| `codeview::CodeView` | the view: `set_bounds`, `set_focused`, `draw(sink, palette)`, `handle_key(key)`, `handle_mouse(event, modifiers)`, `paste(text)`; a gutter (`ViewOptions::line_numbers`), wrapping (`ViewOptions::wrap`), the find bar (Ctrl+F / Ctrl+H), the scrollbar. |

## Moving an editor onto it

- Hold a `CodeView` where the editor held its lines; build it with
  `CodeView::new(CodeEditor::from_text(&contents))`, and save
  `view.editor().text()`.
- Forward keys to `handle_key` and pointer events to `handle_mouse` with the
  modifiers your window holds (mouse events carry none). Draw with `draw`.
- The clipboard is the program's (`guitk::clipboard`, since 2026-09-30),
  shared with every text field: Ctrl+C, Ctrl+X and Ctrl+V use it without
  you, and `CodeViewEvent::Copy(text)` / `Cut(text)` say what went on it.
  (`CodeViewEvent::Paste`, which asked you to paste, is gone.)
- A right-click on the text (added 2026-10-01): put up a menu of
  `view.edit_menu()` where it landed -- Undo, Redo, Cut, Copy, Paste,
  Delete, Select all, each greyed when it would do nothing -- and hand the
  chosen row's id to `view.edit_command(id)`, which answers as the key that
  does the same would (`None` for an id that is not one of its rows, so
  rows of your own can follow them). As with the keys, Cut and Copy with
  nothing selected take the caret's line. The same pattern as every text
  field's: `requests/c-e-text-fields-have-a-right-click-menu.md`.
- `CodeViewEvent::Changed` is the moment to mark the document modified.
- A file with `\r\n` line endings keeps its `\r`s as text; convert on open and
  back on save if the editor offers a line-ending setting.

## Colouring the code (added 2026-09-28)

Syntax highlighting is there now, and it is tree-sitter's (design-decisions
§1437): the `syntax` crate (`gui/syntax`) has Ada, Bash, C, C++, CSS, diffs, Dockerfiles, DTD, Go, HTML, INI, Java, JavaScript,
JSON, linker scripts, Lua, Makefiles, Markdown, Nushell, PowerShell, Python, Rust, SQL, TOML, tree-sitter queries, TypeScript, TSX, XML and YAML so far, each
grammar passing its authors' own test corpus. A language inside another is coloured as itself: a Markdown code
fence in the language its info string names, front matter as YAML.

- Add `syntax = { path = "../../gui/syntax" }` to the editor's `Cargo.toml`.
- On opening a file: `syntax::Language::for_file(path)`, or for a script
  with no telling extension `syntax::Language::for_first_line(first_line)`;
  then `view.set_highlighter(Some(Box::new(language.highlighter()?)))`.
  `syntax::Language::all()` lists the languages for a "Language" menu.
- In the event loop: while `view.has_work()`, call `view.work()` and draw
  again. That is only ever the first parse of a large file -- an edit's
  re-parse happens inside the key press -- and it arrives a frame at a time,
  never freezing the window.
- The colours are the theme's (`syntax` / `syntax-light` in `theme.yaml`,
  one per kind: keyword, string, comment, function, type, ...), held to the
  text contrast floor. Drop the editors' own highlight colours for these, so
  a theme colours code everywhere at once.
- A highlighter of your own (a log viewer colouring levels) implements
  `guitk::highlight::Highlighter` and plugs into the same view.

What the editors have that the widget does not yet: the markdown editor's
preview, which is the application's; and languages not here yet -- ask
for the ones the editors need first, and they come next.
