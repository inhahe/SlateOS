## 1452. A photo frame on the desktop shows the Pictures folder, a picture every half minute

**Date:** 2026-09-30 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** Right-click the desktop, "Add widget", "Photo frame": a frame
appears showing the pictures in your Pictures folder, one at a time, the
next every thirty seconds, in name order and round again. Each picture is
fitted inside the frame without being cut or stretched. A folder with no
pictures says so.

**What was there.** `WidgetKind::PhotoFrame` was named, sized and given a
picture in `widgets.rs`, and offered by nothing (`roadmap-detailed.md` →
*Widget support*: "defines five more ... that nothing offered").

**How it works:**

| | |
|---|---|
| The folder | the user's Pictures -- the folder the start menu's Pictures place opens, from the same definition (`DesktopShell::photo_frame_folder`); found once when the session starts -- or the one chosen for the frame with "Choose folder…" on its menu: the shell's chooser (the Run box's, generalised) in its folder mode, the folder kept with the layout (`widgets.yaml`, `folder`, in `pathcodec`'s form since a name need not be text); a new folder starts at its first picture |
| Which pictures | the files the wallpaper's slideshow would take (`wallpaper::is_picture`), listed as it lists them (`ShellSession::pictures_in`), sorted by name |
| The order | the next *name* after the last one chosen, round to the first: a picture added or deleted between two steps moves no other's turn |
| The pace | 30 s (`FRAME_INTERVAL_MS`), a gadget's -- slower than a whole-screen slideshow, since a frame is read in passing; a deadline, so an idle desktop still sleeps between pictures |
| The size | decoded to the frame's size on the decoding thread (`imagecodec::decode_scaled`, `Job::fit`), never at its own: a 24-megapixel photograph drawn 300 pixels wide would spend 90 MB of the upload budget on pixels nobody sees |
| The ids | bit 61 (`FRAME_PICTURE_TAG`), bit 62 clear: the desktop's surface holds the wallpaper's pictures, numbered from one, and the icons', tagged bit 62 with any bits below -- so bit 61 alone is not enough (`is_frame_picture`) |
| Between pictures | the old one stays until the next is up, and is released only after the frame that stops naming it is sent -- released before, a redraw meanwhile would show a hole |
| An empty folder | "No pictures in Pictures", and the last picture let go rather than left up from a folder since emptied |
| A picture that will not open | said, the previous picture kept, and the next step moves past it |

| Alternative | For | Against |
|---|---|---|
| **Pictures folder, name order** (chosen) | nothing to set up; the same order as the wallpaper's slideshow unshuffled | one folder for every frame until a frame can be pointed elsewhere |
| Shuffled | variety | two frames in step, and a picture repeated before another is seen |
| Decode at full size, scale when drawing | simpler | memory and decode time for pixels never shown |
| A timer per frame | independent | the widget layer already dates its widgets (`update_interval_ms`); a second clock is a second thing to arm |

**What choosing a folder found in the toolkit.** "Choose folder…" uses
the toolkit's folder mode (`FileDialog::select_folder`), as seven of lane
E's applications already did -- `archivemanager`, `filediff`,
`filesearch`, `mediaconvert`, `renamer`, `settings`, `soundrecorder`. Its
test, driving the chooser through the keys a user presses, found what
`known-issues.md` had warned an end-to-end use might
(`TD-C-THE-TOOLKIT-CAN-SELECT-A-FOLDER-AND-NO-APPLICATION-ASKS-IT-TO`):
opening a folder -- a double click, or Enter on it -- *chose* it, so the
picker could not be walked down a folder at a time; anything deeper than
the folders listed where it stood was reachable only by typing its path.
Opening a folder now opens it, in every mode; the Select button, or Enter
with nothing highlighted, chooses the folder highlighted or the one shown
-- as Windows' and GTK's folder pickers do. The seven applications share
the fix with nothing to change, since the toolkit's `FilePicker` lists a
folder opened as it lists any other
(`requests/c-e-the-folder-picker-opens-a-folder-now-rather-than-choosing-it.md`).

**Revisit if** users ask for shuffling or a slower pace.
