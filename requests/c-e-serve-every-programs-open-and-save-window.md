# C → E — Serve every program's Open and Save window from the file explorer

**From:** Lane C (`gui/filechooser`, `gui/toolkit`). **To:** Lane E
(`apps/explorer`, and the applications that open and save files).
**Filed:** 2026-10-05. **Status:** OPEN.

**In short:** the operator decided (`design-decisions.md` §1415, answering
C-Q30) that the file explorer shows every program's Open and Save window, and
that a program is told only the file chosen. Lane C's half is built: a
program can now *ask* (`filechooser::Picker`), and there is a library for the
explorer to *answer* with (`filechooser::service`). Two things are lane E's:
the explorer answering, and the applications asking. Until both happen, every
program goes on drawing the toolkit's own dialog, as it does today -- nothing
breaks, and nothing improves.

## 1. The explorer serves `org.slateos.FileChooser`

What it is asked, and how to answer, are in `gui/filechooser` (§1463):

```rust
// once, at start, on SlateOS
let listener = filechooser::service::register()?;   // ChannelListener
// for each connection the listener accepts:
let mut asked = filechooser::service::Asked::read(conn, filechooser::service::REQUEST_PATIENCE)?;
let request = asked.request();      // mode, owner window, title, start, name, filters, filter
// ... show the window, in "choose a file" form ...
asked.answer(&filechooser::Reply::Chosen { path, filter })?;   // or Reply::Cancelled
```

- **The window**: the explorer's own views (columns, thumbnails, layouts) in
  a "choose" form: Open/Save (or Select Folder) and Cancel, the request's
  filters as the type list with `request.filter` first, `request.start` as
  the folder (empty: your choice), `request.name` in the name field when
  saving. `request.title` is the program's own title when it gave one;
  otherwise yours by mode -- with the program's name, which
  `asked.connection().peer_cred()` gives you (the kernel's answer, not the
  program's).
- **The answer** is final: when saving, add the filter's extension if the
  name has none (as `guitk::dialog::FileDialog::confirm` does) and ask about
  overwriting *before* answering. `Asked::answer` refuses a path that is not
  absolute and a filter the request did not offer.
- **Above the program**: `request.owner` is the asking window as the
  compositor numbers it. Lane F is asked to let a window be kept above
  another (`requests/c-f-the-file-choosers-window-belongs-to-the-program-that-asked.md`);
  pass it there when that lands.
- **A program that gives up** (closes its picker, or exits) hangs up:
  `asked.is_abandoned()` turns true. Take the window down then.
- **Who starts the explorer as a service** is open: a resident process the
  session starts at login, or one started on the first request. Tell me which
  you want, and whether the desktop session (lane C) should start it.

## 2. The applications ask

Every application holding a `guitk::dialog::FilePicker` moves to
`filechooser::Picker`, which has the same calls (`open_to_read`,
`open_to_write`, `put_up`, `handle`, `render`, `close`, `is_open`,
`is_saving`) and two more:

- `Picker::new().with_waker(event_loop.waker()?.unwrap())` -- so the
  explorer's answer wakes the program.
- `picker.poll()` when the loop wakes (`Dispatch::Woken` under
  `run_batched`): `Picked::Chose(path)` / `Picked::Cancelled` when the answer
  has come, `Picked::Handled` (draw again) if the explorer failed and the
  toolkit's dialog is up instead, `Picked::Ignored` otherwise.
- `picker.set_owner(window_id)` before asking, so the explorer's window can
  be kept above the right window.

Where nothing serves the name -- every development host, and SlateOS until
part 1 lands -- `Picker` draws the toolkit's dialog exactly as `FilePicker`
does, so the move can happen before part 1, one application at a time.

## Tests that would hold it

- The explorer: a request in each mode answered with what the user chose;
  Cancel answered as `Reply::Cancelled`; a program that hangs up taking the
  window down. `filechooser`'s own tests use a stand-in transport
  (`gui/filechooser/src/service_tests.rs`) that you can copy.
- An application: its Open runs through `Picker` against a stand-in chooser
  (`Picker::with_connect`; `gui/filechooser/src/client_tests.rs` has one) and
  opens the path answered.

## If this is never done

Programs keep drawing their own dialog and reading folders to fill it, as
today; the security §1415 is for -- a program handed only the file chosen --
does not arrive.
