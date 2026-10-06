## 1463. A program asks the file explorer for its Open and Save window through `filechooser`

**Date:** 2026-10-05 &middot; **Decided by:** Claude (operator-approved scope:
the direction is the operator's, §1415; how lane C builds its part is
Claude's) &middot; **Lane:** C

**In short:** §1415 decided that the file explorer shows every program's Open
and Save window, so that a program never reads your folders itself. This is
how a program asks and hears back. A small library, `filechooser`, takes the
place of the toolkit's file picker in a program -- the same calls -- and
sends the request to a system service the explorer runs, getting back the
path of the file chosen. Where nothing serves the request (a development
machine; a SlateOS whose explorer does not serve it yet) the program draws
the toolkit's own dialog exactly as it does today. Until lane E's explorer
serves the requests and programs move onto the library, nothing a user sees
changes.

**What was built** (`gui/filechooser`):

- **The protocol** (`protocol`): one request and one reply per connection to
  the service `org.slateos.FileChooser`. The request says what to choose (a
  file to open, a place and name to save to, a folder), which window is
  asking, an optional title, where to start, the name to offer, the file type
  filters and which is first. The reply is the path chosen -- absolute, any
  extension the filter adds already on it, overwriting already agreed to --
  or "cancelled". Paths and names travel as their bytes, never decoded. Every
  field is bounded both ways, and a frame declaring more than the bound is
  refused before it is read.
- **The program's side** (`Picker`): `FilePicker`'s calls. The request is
  sent on the calling thread, so a missing explorer is known at once and the
  toolkit's dialog drawn at once; a worker thread waits for the answer and
  wakes the program's event loop with `oswindow::EventLoop::waker`; the
  program collects the answer with `poll()` when woken, or at its next
  event. While the explorer has the request, the program's window is behind
  a dialog: its keys and clicks are taken and do nothing, except Escape,
  which gives up asking. If the explorer fails part way -- exits, or answers
  something that is not an answer -- the toolkit's dialog comes up with the
  same settings: the user asked to choose a file, and still gets to.
- **The explorer's side** (`service`): `Asked::read` waits for a fresh
  connection's request (five seconds at most: a program that connects and
  says nothing cannot hold the chooser), `Asked::answer` checks the answer
  against the request and sends it, `Asked::is_abandoned` says whether the
  program has given up.

**The choices, and what was not done:**

| Question | Chosen | Not chosen, and why |
|---|---|---|
| Where the code lives | A crate of its own, `gui/filechooser` | In `guitk::dialog`: the transport is `guiremote`'s, and `guiremote` depends on the toolkit, so the toolkit cannot depend on it. In `oswindow`: that is lane F's crate, and choosing a file is not the window system's business |
| How the answer reaches the program | A worker thread waits on the connection and wakes the event loop with the waker it already has | The compositor relaying the answer as an event: puts the file chooser in the display protocol. A way for the event loop to wait on a second handle: lane F's to build, and the waker already does the job |
| Where the request is sent from | The calling thread; only the wait is on the worker | All of it on the worker: a missing explorer would be found only after a wake, and the toolkit's dialog would appear a moment late |
| A chooser that fails part way | The toolkit's dialog comes up | Treating it as "cancelled": the user asked to choose a file |
| The program's window while the explorer has it | Its keys and clicks are taken; Escape gives up | Letting them through: the program could close the document it is in the middle of saving |

**The other lanes' parts**, asked for the same day:

| Part | Lane | Request |
|---|---|---|
| The explorer serves `org.slateos.FileChooser`: shows its window in the request's mode, above the asking window, and answers | E | `requests/c-e-serve-every-programs-open-and-save-window.md` |
| Programs move from `guitk::dialog::FilePicker` to `filechooser::Picker`, and call `poll()` when woken | E | the same request |
| The chooser's window is kept above the window that asked, and belongs to it | F | `requests/c-f-the-file-choosers-window-belongs-to-the-program-that-asked.md` |
| A program is handed the chosen file open, not its name | A, D | `requests/c-ad-hand-a-program-the-file-it-chose-not-its-name.md` |

**Known limit:** whichever process registers `org.slateos.FileChooser` first
answers every program; registering a service name is a capability that is
not yet narrowed to one name (lane F's note to lane A, 2026-10-03). Until it
is, a process holding that capability could answer in the explorer's place.
