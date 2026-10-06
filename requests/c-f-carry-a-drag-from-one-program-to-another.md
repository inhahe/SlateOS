# C → F — Carry a drag from one program to another

**From:** Lane C (`gui/toolkit`, `gui/desktop`). **To:** Lane F
(`gui/compositor`, `gui/remote`, `gui/window`). **Filed:** 2026-10-06.
**Status:** OPEN.

**In short:** a drag stops at the edge of the window it began in. Inside one
program it works -- text, files and pictures move between the program's own
parts -- but nothing can be dragged from one program into another: a file
from the file manager onto the desktop or into an editor, a picture from one
document into another, text between two editors. `design.txt` asks for
exactly that ("a model for dragging and dropping various objects from/to
apps, from/to file explorer", line 733: "a clipboard/drag-and-drop system
that supports multiple data formats per operation"). Only the window system
knows which window is under the pointer at each moment, so it is the one
that can carry a drag between programs -- as Wayland's data device and X's
XDND do. This asks for that protocol. Lane C's half is written and tested.

## What there is

- **Your implicit grab** (`PointerGrab`, `requests/c-f-a-drag-that-leaves-its-window-stops-being-told-where-the-pointer-is.md`):
  a press in a window keeps the pointer for it until the last button is up.
  A drag begins inside that grab.
- **`guitk::dnd`** (lane C, rewritten 2026-10-06): the data as
  `DataObject` -- the same content in several formats, each named by a MIME
  type (`DataFormat::mime`: `text/plain;charset=utf-8`, `text/html`,
  `image/png`, `text/uri-list`, `application/x-slateos-file-paths` for a
  list of paths as bytes, and any a program defines) -- and
  `DragDropManager`, one per window: it hit-tests the window's drop targets,
  works out what a drop would do (copy, move or link) from what the source
  allows, what the target prefers and the modifier keys held, and says each
  enter, leave, change of effect and drop in order. It already holds **both
  ends of a drag between programs** -- the calls below -- so a program's
  code is the same for a drag from itself and one from elsewhere.

## What is asked

One drag at a time, carried by the compositor from the window it began in to
the window it is let go over. Names are suggestions; the shape is the ask.

### Requests (client → compositor)

| Request | When | What the compositor does |
|---|---|---|
| `StartDrag { window, formats: Vec<String>, effects }` | a drag has started in `window` (the toolkit's threshold passed) | **Refused unless `window` holds the implicit grab** -- a button pressed in it and still held -- so no program can start a drag the user is not making. Accepted, the grab's pointer events stop reaching `window` as moves and a release; they drive the drag instead, until it ends. |
| `DragStatus { drag, effect, format }` | answering each enter, move and change of keys over a window | Records what a drop there would do (`effect`: copy, move, link or none) and in which `format` the window would take the data; draws the pointer with the effect's mark, as the pointer is over a window that is not the source's; tells the source (`DragTargetChanged`). |
| `ReceiveDrop { drag, format }` | after a `Drop` to this window | Asks the source for the data in `format` (`SendDragData`) and hands it to this window (`DropData`). |
| `DragData { drag, format, bytes }` | the source answering `SendDragData` | Relays it to the window that asked. **The data need not fit one message**: a picture is megabytes -- in pieces, or as a handle (a pipe or channel end) passed from source to target, whichever your transport makes cheap. |
| `FinishDrop { drag, effect }` | the target has the data and did `effect` -- or could not, `effect` none | Ends the drag; tells the source `DragEnded { effect }`. |
| `CancelDrag { drag }` | the source gives its own drag up | Ends it: the window under the pointer gets `DragLeave`; the source `DragEnded { effect: none }`. |

### Events (compositor → client)

| Event | To | When |
|---|---|---|
| `DragEnter { drag, x, y, formats, effects, keys, own }` | the window under the pointer -- **any window, the source's own included** | the pointer comes over it; `own` says the drag is this client's, whose data the toolkit already has |
| `DragMotion { drag, x, y, keys }` | that window | each move -- **and each change of the modifier keys**, so the effect follows Ctrl and Shift without a move |
| `DragLeave { drag }` | that window | the pointer leaves it, or the drag ends while over it without a drop there |
| `Drop { drag, x, y, keys }` | that window | the release, over a window whose last `DragStatus` said a drop does something. A release over a window that said none, or over no window, ends the drag with nothing dropped. |
| `DropData { drag, format, bytes }` | the target | the data it asked for |
| `SendDragData { drag, format }` | the source | the target wants the data in `format` |
| `DragTargetChanged { drag, effect }` | the source | what a drop where the pointer is would now do -- for a source that shows it |
| `DragEnded { drag, effect }` | the source | the drag is over: `effect` is what the drop did, none if nothing was dropped. **Sent only after the target's `FinishDrop`**, so a source moving a file removes its own copy only once the target has the data. |

### Rules the compositor keeps

- **The data reaches only the window the user let go over, and only after
  the drop.** Enter and motion carry the formats' *names*, never the data, so
  a program under a drag's path learns nothing it did not receive.
- **Escape during a drag cancels it**, taken by the compositor before any
  window: the source's window has the keyboard, but the drag is no longer
  its to steer.
- **A window that closes mid-drag:** the source's ends the drag (the window
  under the pointer gets `DragLeave`); a target's simply stops being under
  the pointer.
- **The shell's windows are windows like any other**: the desktop takes a
  file dropped on it, as any program's window does.

## What lane C does with it

The toolkit side is the five calls `DragDropManager` already has for it --
lane F's event loop (`oswindow::app::drive`, beside the clipboard's two calls)
makes them for every program that has a manager, and passes the events they
return to the program:

| When | The loop calls | And sends |
|---|---|---|
| after each batch of events | `take_outgoing()` → `Some(Outgoing { formats, allowed })` once a drag has started | `StartDrag` (each format's `mime()`) |
| after each batch | `take_withdrawn()` → `true` when the program gave a carried drag up | `CancelDrag` |
| `DragEnter { own: true }` / `DragMotion` (own) | `set_keys(keys)`, `update_position(x, y)` | -- |
| `DragEnter { own: false }` | `offer_entered(formats, effects, x, y)` after `set_keys(keys)` | `DragStatus` from `offer_status()` |
| `DragMotion` (another's) | `set_keys`, `update_position` | `DragStatus` from `offer_status()` |
| `DragLeave` | `pointer_left()` | -- |
| `Drop` (own) | `end_drag(x, y)` | `FinishDrop` with the drop's effect |
| `Drop` (another's) | `offer_dropped(x, y)` → the format to ask | `ReceiveDrop`, or `FinishDrop { none }` |
| `DropData` | `offer_data(format, bytes)` | `FinishDrop` with the drop's effect |
| `SendDragData { format }` | `outgoing_data(format)` | `DragData` |
| `DragEnded { effect }` | `outgoing_ended(effect)` | -- |

Each call returns the `DragEvent`s for the program -- a target entered,
left, dropped on; the source told the drop's outcome -- so a program handles
a drag between programs exactly as it handles one inside itself. How the
loop finds a program's manager (an `App` method returning it, say) is yours
to choose; lane C will adapt the shell and the toolkit's widgets to it.

Then the shell (lane C): a file dragged from the file manager onto the
desktop, and a desktop icon dragged into a program -- `carry_target`'s
`None` for a window becomes "hand it to the program"
(`design-decisions.md` §871 named that as the case to revisit).

**This does not depend on `open-questions.md` C-Q29** (how copy and paste
travel between programs). A drag needs the window system whatever C-Q29's
answer, as only it knows which window is under the pointer. If C-Q29 goes
the window system's way, the same offer -- formats named, data fetched in
one of them on demand -- can carry copy and paste's formats too.

## If this is never done

Dragging stays inside each program, as now.
