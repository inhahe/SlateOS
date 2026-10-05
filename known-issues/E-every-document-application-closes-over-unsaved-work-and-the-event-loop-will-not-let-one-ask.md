### [E] Every document application closes over unsaved work, and the event loop will not let one ask -- 2026-09-25
**Status:** FIXED (lane E, 2026-09-25/26), on lane F's `Response::KeepOpen` (b82f06a11): the text editor, the markdown editor, the hex editor and the JSON viewer first, then slides, sticky notes, paint, the diagram editor, the whiteboard and the spreadsheet -- see `[E] Document applications closed over unsaved work, and the hex editor and the JSON viewer could not save at all` below -- and notes, contacts, kanban and snippets, which kept nothing at all (`[E] Notes, contacts, snippets and kanban keep nothing`). This status said OPEN for slides and sticky notes after both were fixed.

**In short:** click a window's X and every change since the last save is gone,
without a question, in the text editor, the markdown editor, the hex editor and
the JSON viewer. An untitled document -- which auto-save never touches -- is
lost whole. The markdown editor has had an "Unsaved changes: Save / Don't save /
Cancel" dialog since today, and it still loses the work in a real window,
because `oswindow` closes the window on a close request whatever the
application answers.

**Where.** `gui/window/src/lib.rs`, `EventLoop::run_batched`: `if verdict ==
EventResponse::Exit || requested_close` -- the loop stops on `CloseRequested`
regardless of the verdict, by design ("A title-bar X that does nothing is worse
than an application that quits when it would rather not have"). Behind it, each
application answers `Exit` without looking: `apps/editor/src/input.rs`
(`Event::CloseRequested => Response::Exit`), `apps/hexeditor/src/main.rs` and
`apps/jsonviewer/src/main.rs` (`if matches!(event, Event::CloseRequested) {
return Response::Exit; }`). `apps/markdowneditor` asks (`App::request_quit`),
and is overruled.

**How to see it.** Open a file in `apps/editor`, type a character, close the
window. The file is unchanged and the character is gone.

**`apps/slides` joins the list (2026-09-25).** It can save a deck now and
knows when one has unsaved changes (the window bar's `*`, and the question
before Open), so its close is the same loss; it answers
`Event::CloseRequested` with `Response::Exit`, and asks nothing, until the
loop lets it.

**The proper fix** is in two halves. Lane F's: an explicit "not yet" answer to
a close request (the request proposes `Response::KeepOpen`, keeping today's
close-anyway default for every application that does not use it). Lane E's:
each document application raises the markdown editor's dialog -- which
already exists, tested, with pointer and keys -- and answers `KeepOpen` while
it is up. Until the first half lands, the markdown editor at least saves every
document that has a file when auto-save is on, since auto-save is the user
having said "save for me" and the close is the last chance to.
