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

SRC = Path(__file__).parent / "src"

OFF = "thumbnails_are_made_off_the_window"
EXIF = "a_photographs_exif_is_three_columns"
LATE = "exif_past_the_head_of_a_webp_is_found"

MAIN = [
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

COLUMNS = [
    (
        "the camera column reads nothing",
        "            ColumnId::CAMERA => text(exif().and_then(|e| e.camera())),",
        "            ColumnId::CAMERA => ColumnValue::Empty,",
        [EXIF],
    ),
    (
        "the date column keeps the file's colons",
        "            ColumnId::DATE_TAKEN => text(exif().and_then(|e| e.date_taken).map(|d| exif_date(&d))),",
        "            ColumnId::DATE_TAKEN => text(exif().and_then(|e| e.date_taken)),",
        [EXIF],
    ),
    (
        "a turn right is called a turn left",
        "        6 => \"Turned right\",",
        "        6 => \"Turned left\",",
        [EXIF],
    ),
    (
        "EXIF past the head of the file is never looked for",
        "    if !found.is_empty() || !anywhere || head.len() < IMAGE_HEAD_BYTES {",
        "    if true {",
        [LATE],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "columns.rs": COLUMNS,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "explorer", timeout=900, only=mine))
    raise SystemExit(worst)
