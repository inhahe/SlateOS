"""Mutation test for where Disk Cleanup looks in the user's home.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the three categories that live in a home
looked in `home/user` for everybody, the recycle bin category looked in another
desktop's `~/.local/share/Trash`, and the thumbnail cache in a folder nothing
writes.  They look in the user's own home now, at SlateOS's recycle bin
(`~/.recycle`, read through `recyclebin`) and at `gui/thumbs`' cache.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

BIN = "test_scanner_scan_recycle_bin"
NOT_TRASH = "the_recycle_bin_is_not_another_desktops_trash_folder"
THUMBS = "test_scanner_scan_thumbnail_cache"
WHOSE = "the_home_categories_look_in_the_users_own_home"
PLACED = "home_is_placed_under_the_scan_base"
NO_HOME = "without_a_home_the_home_categories_find_nothing"
BOX = "a_categorys_box_is_the_toolkits_check_box"

MUTATIONS = [
    (
        "every home is home/user again",
        "        let mut dir = Path::new(base_path).join(self.home.as_ref()?);",
        "        let _ = self.home.as_ref()?;\n        let mut dir = Path::new(base_path).join(\"home/user\");",
        [WHOSE],
    ),
    (
        "no home is a guessed one",
        "        let mut dir = Path::new(base_path).join(self.home.as_ref()?);",
        "        let mut dir = Path::new(base_path).join(self.home.clone().unwrap_or_else(|| PathBuf::from(\"home/user\")));",
        [NO_HOME],
    ),
    (
        "a home that climbs is followed",
        "            Component::ParentDir | Component::Prefix(_) => return None,",
        "            Component::ParentDir => under.push(\"..\"),\n            Component::Prefix(_) => return None,",
        [PLACED],
    ),
    (
        "a relative home is placed anyway",
        "    if !home.has_root() {\n        return None;\n    }",
        "",
        [PLACED],
    ),
    (
        "the bin is looked for in another desktop's trash",
        "        let Some(dir) = self.in_home(base_path, &[\".recycle\"]) else {",
        "        let Some(dir) = self.in_home(base_path, &[\".local\", \"share\", \"Trash\"]) else {",
        [BIN],
    ),
    (
        "a recycled thing is shown by its folder in the bin",
        "                    .with_shown_as(format!(\"{}{from}\", entry.display_name())),",
        "                    .with_shown_as(path.shown().to_string()),",
        [BIN],
    ),
    (
        "the thumbnail cache is looked for where nothing writes",
        "        let Some(cache_dir) = self.in_home(base_path, &[\".cache\", \"thumbs\"]) else {",
        "        let Some(cache_dir) = self.in_home(base_path, &[\".cache\", \"thumbnails\"]) else {",
        [THUMBS],
    ),
    (
        "the bin's folder is not a place deletion may happen",
        "        // Recorded whether or not anything is found, as `collect` does.\n        self.roots.push(dir.clone());",
        "",
        [BIN],
    ),
    # -- the toolkit's check box (c-e-the-toolkit-has-switches-checkboxes-...)
    (
        "a category's box is drawn the same wherever the pointer is",
        "            hovered: self.hovered_row == Some(index),",
        "            hovered: false,",
        [BOX],
    ),
    (
        "the pointer is not followed",
        "                let over = self.row_at(&self.layout(), mouse.x, mouse.y);",
        "                let over = None::<usize>;",
        [BOX],
    ),
    (
        "the pointer leaving the window leaves its row lit",
        "                return if self.hovered_row.take().is_some() {",
        "                return if self.hovered_row.is_some() {",
        [BOX],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    sys.exit(sweep(SRC, MUTATIONS, "diskcleanup", timeout=900, only=only))
