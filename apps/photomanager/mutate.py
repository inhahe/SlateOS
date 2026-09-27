"""Mutation test for the photo manager's photograph loader.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: the selected photograph was
decoded inside `render`, freezing the window for as long as that took; it is
decoded on `offloop`'s worker now, and put up when the worker wakes the
window.  `offloop`'s own rule -- only the newest request is answered -- is
swept by `apps/offloop/mutate.py`.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

OFF = "the_selected_photograph_is_decoded_off_the_window"
GONE = "a_photograph_deselected_while_decoding_is_not_put_up"
FAILS = "a_photograph_that_fails_off_the_window_says_why"
NOT_A_PICTURE = "a_file_that_is_not_a_picture_says_why_instead_of_staying_blank"
THUMBS = "the_grids_thumbnails_are_made_off_the_window"

MUTATIONS = [
    (
        "the photograph is decoded on the window even with a loader",
        "            Some(loader) => loader.ask((pid, path)),",
        "            Some(_) => Err::<offloop::Ticket, _>((pid, path)),",
        [OFF],
    ),
    (
        "no waker is asked for",
        "    fn wants_waker(&self) -> bool {\n        true",
        "    fn wants_waker(&self) -> bool {\n        false",
        [OFF],
    ),
    (
        "no loader is started",
        "        self.picture_loader = offloop::Latest::start(",
        "        self.picture_loader = None;\n        let _unused = offloop::Latest::start(",
        [OFF],
    ),
    (
        "a decoded photograph is not put up",
        "        self.show_picture(pid, decoded);\n        true",
        "        let _ = (pid, decoded);\n        true",
        [OFF],
    ),
    (
        "a decoded photograph is not drawn",
        "        self.show_picture(pid, decoded);\n        true",
        "        self.show_picture(pid, decoded);\n        false",
        [OFF],
    ),
    (
        "a photograph no longer selected is put up",
        "        if self.picture_for != Some(pid) {\n            return false;\n        }",
        "",
        [GONE],
    ),
    (
        "a photograph that will not decode says nothing",
        "            Err(why) => {\n                self.picture_error = Some(why);\n                return;",
        "            Err(why) => {\n                let _ = why;\n                return;",
        [FAILS, NOT_A_PICTURE],
    ),
    # -- the grid's thumbnails ------------------------------------------------
    (
        "thumbnails are left to the frame even with a worker",
        "            Some(worker) => match worker.replace(wanted) {",
        "            Some(_) => match Err::<(), _>(wanted) {",
        [THUMBS],
    ),
    (
        "no thumbnail worker is started",
        "        self.thumb_worker = offloop::Queue::start(",
        "        self.thumb_worker = None;\n        let _unused = offloop::Queue::start(",
        [THUMBS],
    ),
    (
        "a made thumbnail is not filed",
        "        for (req, thumb) in made {\n            self.file_thumbnail(req, thumb);\n        }\n        any",
        "        let _ = made;\n        any",
        [THUMBS],
    ),
    (
        "a wake with thumbnails asks for no frame",
        "        if self.take_decoded_picture() || thumbnails {",
        "        if self.take_decoded_picture() {",
        [THUMBS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "photomanager", timeout=600, only=only))
