"""Mutation test for the diagram editor's own file, and asking before a diagram
is lost.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

It could export an SVG, or a JSON that kept the shapes and dropped their
colours, borders, fonts, arrowheads, layers and groups, for an importer that
did not exist -- and nothing it wrote could be opened again.  Nothing recorded
unsaved changes, and the window closed over them.  The table covers the file
it has now and the question; the rest of the suite predates them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

ROUND = "a_diagram_saved_and_opened_again_is_the_same_diagram"
REFUSED = "a_file_that_is_not_a_diagram_is_refused"
PARTIAL = "what_is_not_understood_is_left_out_and_the_rest_read"
KEYS = "ctrl_s_saves_over_the_diagrams_own_file_once_it_has_one"
CLOSE = "closing_or_opening_over_unsaved_changes_asks"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a save writes nothing",
        "        match safeio::write_str_atomically(path, &self.diagram_document().to_text()) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [ROUND, KEYS],
    ),
    (
        "the file loses a box's fill",
        '            doc.set_str(&at("fill"), &colour_hex(node.fill_color));\n',
        "",
        [ROUND],
    ),
    (
        "the file loses the arrowheads",
        '            doc.set_str(&at("end"), edge.end_arrow.label());\n',
        "",
        [ROUND],
    ),
    (
        "the file loses the groups",
        "        for (i, group) in self.groups.iter().enumerate() {",
        "        for (i, group) in self.groups.iter().enumerate().take(0) {",
        [ROUND],
    ),
    (
        "a hidden layer comes back visible",
        '        layer.visible = doc.get_bool(&at("visible")).unwrap_or(true);',
        "        layer.visible = true;",
        [ROUND],
    ),
    (
        "a later format is half-read",
        "        Some(v) if v > DIAGRAM_FORMAT => {",
        "        Some(v) if v > DIAGRAM_FORMAT + 100 => {",
        [REFUSED],
    ),
    (
        "clashing ids are taken as they come",
        "    if seen.insert(id) {",
        "    if seen.insert(id) || true {",
        [REFUSED],
    ),
    (
        "a file too big to read whole is read in part",
        "        if read.truncated {",
        "        if false {",
        [REFUSED],
    ),
    (
        "an arrow to a missing box is read",
        "        if !nodes.iter().any(|n| n.id == from) || !nodes.iter().any(|n| n.id == to) {\n"
        "            left_out = left_out.saturating_add(1);\n"
        "            continue;\n"
        "        }\n",
        "",
        [PARTIAL],
    ),
    (
        "what was left out is not said",
        "                if left_out == 0 {",
        "                if true {",
        [PARTIAL],
    ),
    (
        "new boxes take the ids of opened ones",
        "                self.id_gen = IdGen::new(highest.saturating_add(1));",
        "                self.id_gen = IdGen::new(1);",
        ["new_things_do_not_take_the_ids_of_opened_ones"],
    ),
    (
        "Ctrl+S asks where every time",
        "            Some(path) => self.last_save = Some(self.write_native(&path)),",
        "            Some(_) => self.ask_where_to_save(PickerFor::Save),",
        [KEYS],
    ),
    (
        "an export counts as a save",
        "            PickerFor::Export => self.write_diagram(path),",
        "            PickerFor::Export => self.write_native(path),",
        ["an_export_is_not_a_save"],
    ),
    (
        "a change does not mark the diagram",
        "        self.undo.save(snap);\n        self.dirty = true;\n",
        "        self.undo.save(snap);\n",
        [KEYS, CLOSE],
    ),
    (
        "a name left as it was counts as a change",
        "        if self.find_node(id).is_some_and(|n| n.label == label) {\n"
        "            return;\n"
        "        }\n",
        "",
        ["a_name_left_as_it_was_is_no_change"],
    ),
    (
        "the window closes over unsaved changes",
        "            }\n        }\n        if !self.dirty {\n            return true;\n        }",
        "            }\n        }\n        if true {\n            return true;\n        }",
        [CLOSE],
    ),
    (
        "the question is drawn into a window the loop has closed",
        "            } else {\n                Response::KeepOpen\n            };",
        "            } else {\n                Response::Redraw\n            };",
        [CLOSE],
    ),
    (
        "keys reach the canvas under the question",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && matches!(event, Event::Key(_) | Event::Mouse(_))",
        "        if let Some(question) = self.question.as_mut()\n"
        "            && false",
        [CLOSE],
    ),
    (
        "Ctrl+O opens over unsaved changes",
        "                self.unless_unsaved(Pending::Open);",
        "                self.go_on(Pending::Open);",
        [CLOSE],
    ),
    (
        "what a save did is drawn nowhere",
        "        if let Some(note) = &self.last_save {",
        "        if let Some(note) = None::<&String> {",
        ["the_status_bar_says_what_the_last_save_did"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "diagram", timeout=900, only=only))
