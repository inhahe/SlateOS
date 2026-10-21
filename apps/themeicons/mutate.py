"""Mutation test for a program's theme icons.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

ONCE = "an_icon_drawn_is_uploaded_once_at_its_size"
NOTHING = "a_name_nothing_draws_uploads_nothing"
AGAIN = "another_appearance_drops_every_icon_and_draws_them_again"
OWN = "a_picture_of_the_programs_own_is_never_dropped"
TOLD = "an_icon_drawn_is_told_from_a_picture_of_the_programs_own"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "an icon is uploaded every frame",
        "            |id| registry.request(id).is_some() && held.insert(id),\n",
        "            |id| {\n                held.insert(id);\n                true\n            },\n",
        [ONCE],
    ),
    (
        "an icon is never uploaded",
        "            |id| registry.request(id).is_some() && held.insert(id),\n",
        "            |_id| false,\n",
        [ONCE, NOTHING, AGAIN, OWN],
    ),
    (
        "a picture of the program's own is held as an icon",
        "            |id| registry.request(id).is_some() && held.insert(id),\n",
        "            |id| held.insert(id),\n",
        [OWN],
    ),
    (
        "an icon is uploaded at its size squared",
        "                    stride: icon.size.saturating_mul(4),\n",
        "                    stride: icon.size.saturating_mul(icon.size),\n",
        [ONCE],
    ),
    (
        "the fallback is never drawn",
        "        self.registry.icon_or(name, fallback, px, color)\n",
        "        let _ = fallback;\n        self.registry.icon(name, px, color)\n",
        [NOTHING],
    ),
    (
        "uploads are handed over twice",
        "        std::mem::take(&mut self.changes)\n",
        "        self.changes.clone()\n",
        [ONCE, AGAIN],
    ),
    (
        "another theme keeps the old icons",
        "            self.theme = theme;\n            self.forget();\n",
        "            self.theme = theme;\n",
        [AGAIN],
    ),
    (
        "the same theme again drops every icon",
        "        if theme != self.theme {\n",
        "        if theme != self.theme || !self.held.is_empty() {\n",
        [AGAIN],
    ),
    (
        "forgetting drops nothing from the window",
        "            self.changes.push(ImageChange::Drop(id));\n",
        "            let _ = id;\n",
        [AGAIN],
    ),
    (
        "every picture is taken for an icon",
        "        self.registry.request(id).is_some()\n",
        "        let _ = id;\n        true\n",
        [TOLD],
    ),
    (
        "no picture is taken for an icon",
        "        self.registry.request(id).is_some()\n",
        "        let _ = id;\n        false\n",
        [TOLD],
    ),
    (
        "forgetting keeps the window's list",
        "        for id in std::mem::take(&mut self.held) {\n",
        "        for id in self.held.clone() {\n",
        [AGAIN],
    ),
]


if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "themeicons", timeout=600, only=only))
