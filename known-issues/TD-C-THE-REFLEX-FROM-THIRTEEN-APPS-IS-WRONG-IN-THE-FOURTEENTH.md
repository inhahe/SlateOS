## TD-C-THE-REFLEX-FROM-THIRTEEN-APPS-IS-WRONG-IN-THE-FOURTEENTH -- METHOD 2026-09-15

**In short:** fourteen applications got the same feature this week — a file
dialog — and in eight of them the thing that went wrong was **the habit formed
by the previous ones**. Not carelessness: each mistake was the correct answer
somewhere else, written confidently, and wrong here in a way that reads as
consistency. The failure gets *more* likely as a sweep progresses, which is the
opposite of what experience usually does.

### The eight

| app | the reflex | what was actually there |
|---|---|---|
| `kanban` | Ctrl+S saves | Ctrl+S was the search bar; a second `Key::S if ctrl` arm is unreachable |
| `kanban` | `WINDOW_WIDTH` | `INITIAL_WIDTH`, and a `u32` |
| `torrent` | `INITIAL_WIDTH` | `WINDOW_WIDTH` — the opposite guess, one app later |
| `kanban` | `Card::new(id, title)` | `Card::new(title)`; ids come from one `Id` type, not per-kind newtypes |
| `filesearch` | the picker is drawn in `render_commands` | it is drawn in the App-trait `render`, over the top |
| `automator` | Ctrl+O opens | `handle_key` refuses **every** modified key, deliberately, with a comment |
| `automator` | add `win_width`/`win_height` | the app already stores `self.size` in `render` |
| eight apps | one opener name | `open_file_dialog`, `open_save_dialog`, `open_folder_dialog`, `open_transfer_dialog` |

### Why it gets worse rather than better

After ten conversions the pattern is genuinely known — the routing, the
`Picked` arms, the truncation note, the `own > 0` control. That knowledge is
real and it is what makes the eleventh fast. It is also exactly what stops the
eleventh being *read*.

The `torrent` case is the clearest: I had just been corrected on `kanban`'s
constant name, and reached for the name `kanban` used, in an app that used the
other one. The correction did not generalise because there was nothing to
generalise — **the fact is per-app and there is no rule that predicts it.**

### The two that are worth more than the rest

`automator`'s modifier policy is the sharpest, because the reflex would not
have failed loudly. A `Ctrl+O` binding there compiles, runs, and is silently
discarded by a guard four hundred lines away that exists on purpose. Nothing
would have said so. And it would have contradicted a written position rather
than merely not working — which is worse, because the next reader would have
found two parts of one file disagreeing and no way to tell which was intended.

`filesearch` is the other: the test written like the other seven **failed
against correct code**, which is the good outcome. It cost one diagnosis and
taught where that app draws. The bad outcome is the same test passing for an
unrelated reason, which is what `fileassoc`'s scrim did.

### What actually works

Not "be careful". Two concrete habits, both cheap:

1. **Grep for the thing before using its name.** The constant, the opener, the
   render entry, the key. Every one of the eight above was one `grep` away, and
   the grep costs a second where the compile costs a minute and a wrong
   *runtime* assumption costs a merge.
2. **Read the app's own position before overriding it.** `automator`'s comment
   said what it thought about modifiers. `kanban`'s search binding was visible
   in its key table. Both were written down by somebody who had thought about
   it, and in both cases the convention I was carrying was younger than the
   position I was about to break.

The general form: **a convention is evidence about the codebase you learned it
in, and every app is a different codebase until you check.** The scanners in
`scripts/` all carry a version of that warning in their docstrings; this is the
same rule applied to the person rather than the tool.
