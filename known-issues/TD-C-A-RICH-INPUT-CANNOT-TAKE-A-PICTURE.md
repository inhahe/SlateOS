### [C] TD-C-A-RICH-INPUT-CANNOT-TAKE-A-PICTURE -- 2026-10-05

**Status:** OPEN

**In short:** `roadmap-detailed.md` §3.5 asks for a rich input "with
formatting and image paste". The toolkit's rich field (`guitk::richinput`)
takes formatting, but no picture: nothing can paste one, because the program's
clipboard (`guitk::clipboard`) holds text only, and the system's clipboard --
which a picture copied in another program would come from -- is not yet
reachable at all (`TD-C-NOTHING-CAN-ACTUALLY-COPY-AND-PASTE-BETWEEN-PROGRAMS`,
`open-questions.md` C-Q29).

**Where:** `gui/toolkit/src/richinput.rs` (`paste`) and `richinput/doc.rs`
(a document has characters and formats, and no object in the text).

**The proper fix:** once the clipboard carries more than text -- a
multi-format clipboard, `guitk::dnd::DataObject`'s formats being the shape it
already has inside a program -- a picture becomes an object in the document:
the object replacement character (U+FFFC) in the text, with the picture's id
and size kept beside it, laid out as a piece as tall as the picture, and
pasted, dragged in, copied and undone as any other stretch of the document is.
