"""Mutation test for the file manager's thumbnail worker.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26: thumbnails were made a few per
tick on the thread that draws, and are made on `offloop::Queue`'s worker now,
filed as each arrives.  `offloop`'s own rules are swept by
`apps/offloop/mutate.py`.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

OFF = "thumbnails_are_made_off_the_window"

MUTATIONS = [
    (
        "no waker is asked for",
        "    fn wants_waker(&self) -> bool {\n        true",
        "    fn wants_waker(&self) -> bool {\n        false",
        [OFF],
    ),
    (
        "no worker is started",
        "        self.thumb_worker = offloop::Queue::start(",
        "        self.thumb_worker = None;\n        let _unused = offloop::Queue::start(",
        [OFF],
    ),
    (
        "thumbnails are left to the ticks even with a worker",
        "            Some(worker) => match worker.replace(wanted) {",
        "            Some(_) => match Err::<(), _>(wanted) {",
        [OFF],
    ),
    (
        "a made thumbnail is not filed",
        "        for (req, thumb) in made {\n            self.file_thumbnail(req, thumb);\n        }\n        count",
        "        let _ = made;\n        count",
        [OFF],
    ),
    (
        "a wake with thumbnails asks for no frame",
        "        if self.collect_thumbnails() > 0 {\n            oswindow::app::Response::Redraw",
        "        if self.collect_thumbnails() > 0 {\n            oswindow::app::Response::Idle",
        [OFF],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "explorer", timeout=900, only=only))
