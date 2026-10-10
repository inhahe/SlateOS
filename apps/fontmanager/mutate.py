"""Mutation test for the font manager.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The program was rewritten on 2026-10-10 over the machine's real fonts
(known issue E-the-font-manager-lists-invented-fonts-and-installs-
nothing), and these rows with it: the list and its filters, the search,
the keys, the panel, the preview's pictures, installing and removing, and
the shortcut card's hold on the keys and the pointer.

Not rows:
- the 128 MB cap on a font file read to install (`MAX_FONT_BYTES`), which a
  test would have to write a file that size to reach;
- the folder holding ninety-nine files of one name (`NAME_TRIES`), likewise.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

# --- the window (main.rs) ---------------------------------------------------
W_LIST = "the_list_is_the_machines_fonts"
W_FILTERS = "the_sidebar_filters_the_list"
W_SEARCH = "the_search_box_finds_fonts_by_name"
W_KEYS = "the_keys_and_the_pointer_choose_a_font"
W_PANEL = "the_panel_says_what_the_font_is"
W_PREVIEW = "the_font_chosen_is_shown_in_its_own_letters"
W_INSTALL = "a_font_file_is_installed_from_the_picker"
W_REMOVE = "your_font_is_removed_after_asking"
W_SCROLL = "a_long_list_scrolls"
W_CARD = "the_card_is_modal"

# (name, old, new, [tests that must fail])
MAIN = [
    (
        "the Yours filter shows every font",
        "            Self::Yours => family.has_own(),\n",
        "            Self::Yours => true,\n",
        [W_FILTERS],
    ),
    (
        "the fixed-pitch filter shows every font",
        "            Self::FixedPitch => family.is_fixed_pitch(),\n",
        "            Self::FixedPitch => true,\n",
        [W_FILTERS],
    ),
    (
        "the search is in one case",
        "        let query = self.search.text().trim().to_lowercase();\n",
        "        let query = self.search.text().trim().to_owned();\n",
        [W_SEARCH],
    ),
    (
        "the choice is left on a font not shown",
        "        if self.chosen_index().is_none() {\n            self.selected = self.visible().first().map(|f| f.name.clone());\n        }\n",
        "",
        [W_FILTERS],
    ),
    (
        "Ctrl+F does not reach the search",
        "                Key::F => {\n                    self.search_focused = true;\n",
        "                Key::F => {\n                    self.search_focused = false;\n",
        [W_SEARCH],
    ),
    (
        "Escape does not leave the search",
        "                self.search_focused = false;\n                return EventResult::Consumed;\n",
        "                return EventResult::Consumed;\n",
        [W_SEARCH],
    ),
    (
        "Escape does not empty the search",
        "            Key::Escape if !self.search.text().is_empty() => {\n                self.search.clear();\n",
        "            Key::Escape if !self.search.text().is_empty() => {\n",
        [W_SEARCH],
    ),
    (
        "a press elsewhere leaves the search the keyboard",
        "        self.search_focused = layout.search.contains(x, y);\n",
        "        self.search_focused = self.search_focused || layout.search.contains(x, y);\n",
        [W_SEARCH],
    ),
    (
        "Down goes to the first",
        "            Key::Down => at.map_or(Some(0), |at| at.checked_add(1).filter(|n| *n < count)),\n",
        "            Key::Down => at.map_or(Some(0), |_at| Some(0)),\n",
        [W_KEYS],
    ),
    (
        "End goes nowhere",
        "            Key::End => count.checked_sub(1),\n",
        "            Key::End => None,\n",
        [W_KEYS],
    ),
    (
        "Home goes nowhere",
        "            Key::Home => (count > 0).then_some(0),\n",
        "            Key::Home => None,\n",
        [W_KEYS],
    ),
    (
        "a press on a row chooses nothing",
        "        if let Some((_, index)) = layout.rows.iter().find(|(r, _)| r.contains(x, y)) {\n            self.choose(*index);\n",
        "        if let Some((_, index)) = layout.rows.iter().find(|(r, _)| r.contains(x, y)) {\n            let _ = index;\n",
        [W_KEYS],
    ),
    (
        "a font chosen above the view is left off screen",
        "        if top < self.list_scroll {\n            self.list_scroll = top;\n",
        "        if top < self.list_scroll {\n",
        [W_SCROLL],
    ),
    (
        "the wheel does not scroll the list",
        "                self.list_scroll += guitk::wheel::pixels(dy, ROW_HEIGHT);\n",
        "                self.list_scroll += 0.0 * guitk::wheel::pixels(dy, ROW_HEIGHT);\n",
        [W_SCROLL],
    ),
    (
        "the list scrolls past its last row",
        "        let most = (rows * ROW_HEIGHT - self.list_rect().h).max(0.0);\n",
        "        let most = rows * ROW_HEIGHT;\n",
        [W_SCROLL],
    ),
    (
        "the count is not said",
        "                        \"{count} font famil{} installed.\",\n",
        "                        \"{count} famil{} installed.\",\n",
        [W_LIST],
    ),
    (
        "Remove is offered for the system's fonts",
        "            .filter(|family| family.has_own())\n            .map(|_| self.remove_button());\n",
        "            .map(|_| self.remove_button());\n",
        [W_PANEL],
    ),
    (
        "the styles run together",
        "            (\"Styles\", family.style_names().join(\", \")),\n",
        "            (\"Styles\", family.style_names().join(\" \")),\n",
        [W_PANEL],
    ),
    (
        "the system's fonts are not said to be",
        "            \"The system's\"\n        };\n",
        "            \"Shared\"\n        };\n",
        [W_PANEL],
    ),
    (
        "the system's are asked about removing",
        "        let files = family.own_files();\n        if files.is_empty() {\n            return;\n        }\n",
        "        let files = family.own_files();\n",
        [W_REMOVE],
    ),
    (
        "Escape removes",
        "                Key::Escape if plain => self.confirm = None,\n",
        "                Key::Escape if plain => self.remove_confirmed(),\n",
        [W_REMOVE],
    ),
    (
        "Enter keeps",
        "                Key::Enter if plain => self.remove_confirmed(),\n",
        "                Key::Enter if plain => self.confirm = None,\n",
        [W_REMOVE],
    ),
    (
        "Keep it removes",
        "                } else if keep.contains(x, y) {\n                    self.confirm = None;\n",
        "                } else if keep.contains(x, y) {\n                    self.remove_confirmed();\n",
        [W_REMOVE],
    ),
    (
        "Remove keeps",
        "                if remove.contains(x, y) {\n                    self.remove_confirmed();\n",
        "                if remove.contains(x, y) {\n                    self.confirm = None;\n",
        [W_REMOVE],
    ),
    (
        "the font installed is not chosen",
        "                self.selected = Some(installed.family.clone());\n",
        "",
        [W_INSTALL],
    ),
    (
        "an install is said as a failure",
        "                    failed: false,\n                }\n            }\n            Err(e) => Message {\n                text: format!(\"{} was not installed: {e}.\", file_name(path)),\n",
        "                    failed: true,\n                }\n            }\n            Err(e) => Message {\n                text: format!(\"{} was not installed: {e}.\", file_name(path)),\n",
        [W_INSTALL],
    ),
    (
        "a refused install is said as done",
        "                text: format!(\"{} was not installed: {e}.\", file_name(path)),\n                failed: true,\n",
        "                text: format!(\"{} was not installed: {e}.\", file_name(path)),\n                failed: false,\n",
        [W_INSTALL],
    ),
    (
        "Ctrl+O puts no picker up",
        "                Key::O => {\n                    self.open_install_dialog();\n",
        "                Key::O => {\n",
        [W_INSTALL],
    ),
    (
        "the Install button puts no picker up",
        "        if layout.install.contains(x, y) {\n            self.open_install_dialog();\n",
        "        if layout.install.contains(x, y) {\n",
        [W_INSTALL],
    ),
    (
        "the file chosen in the picker is not installed",
        "                self.install(&path);\n",
        "                let _ = &path;\n",
        [W_INSTALL],
    ),
    (
        "the same pictures are made again",
        "        if key == self.previews.key {\n            return;\n        }\n",
        "",
        [W_PREVIEW],
    ),
    (
        "the last font's pictures are kept",
        "            self.previews.changes.push(ImageChange::Drop(id));\n",
        "            let _ = id;\n",
        [W_PREVIEW],
    ),
    (
        "the pictures keep the old panel's colour",
        "            .map(|family| (family.name.clone(), background, ink, width));\n",
        "            .map(|family| (family.name.clone(), 0, ink, width));\n",
        [W_PREVIEW],
    ),
    (
        "the frame draws a picture it did not upload",
        "                image_id: *id,\n",
        "                image_id: id.wrapping_add(1),\n",
        [W_PREVIEW],
    ),
    (
        "a press goes through the shortcut card",
        "            if matches!(mouse.kind, MouseEventKind::Press(_)) {\n                self.show_help = false;\n                return EventResult::Consumed;\n            }\n",
        "            if matches!(mouse.kind, MouseEventKind::Press(_)) {\n                self.show_help = false;\n            }\n",
        [W_CARD],
    ),
    (
        "a key goes under the shortcut card",
        "            if plain && matches!(key.key, Key::Escape | Key::Enter) {\n                self.show_help = false;\n            }\n            return EventResult::Consumed;\n",
        "            if plain && matches!(key.key, Key::Escape | Key::Enter) {\n                self.show_help = false;\n            }\n",
        [W_CARD],
    ),
]

# --- the fonts (library.rs) -------------------------------------------------
L_LISTED = "the_fonts_listed_are_the_ones_in_the_folders"
L_INSTALL = "a_font_file_is_installed_into_your_fonts_folder"
L_SAME_NAME = "a_second_file_of_the_same_name_is_written_beside_the_first"
L_TWICE = "the_same_family_and_style_is_not_installed_twice"
L_REFUSED = "what_is_not_a_font_is_refused_and_nothing_is_written"
L_REMOVE = "your_fonts_are_removed_and_the_systems_are_not"
L_SHARING = "a_file_another_family_shares_is_named"
L_STYLES = "styles_are_named_as_people_name_them"

LIBRARY = [
    (
        "a font in the user's folder is not theirs",
        "                        own: own.is_some_and(|own| face.path.starts_with(own)),\n",
        "                        own: own.is_some_and(|own| face.path.starts_with(own) && own.as_os_str().is_empty()),\n",
        [L_LISTED],
    ),
    (
        "a name that is not a font's is read",
        "            .filter(|_| is_font_name(from))\n",
        "            .filter(|_| !from.as_os_str().is_empty())\n",
        [L_REFUSED],
    ),
    (
        "what is not a font is said to have no name",
        "        let face = Face::parse(read.bytes.clone()).map_err(|_| InstallError::NotAFont)?;\n",
        "        let face = Face::parse(read.bytes.clone()).map_err(|_| InstallError::NoFamilyName)?;\n",
        [L_REFUSED],
    ),
    (
        "a font with no family name is installed",
        "            .ok_or(InstallError::NoFamilyName)?;\n",
        "            .unwrap_or_default();\n",
        [L_REFUSED],
    ),
    (
        "the same family and style is installed twice",
        "        if already {\n",
        "        if already && names.is_empty() {\n",
        [L_TWICE],
    ),
    (
        "the system's font is counted as the user's",
        "                    .any(|face| face.own && face.style == style)\n",
        "                    .any(|face| face.style == style)\n",
        [L_TWICE],
    ),
    (
        "another style is counted as the same",
        "                    .any(|face| face.own && face.style == style)\n",
        "                    .any(|face| face.own)\n",
        [L_SAME_NAME],
    ),
    (
        "the fonts folder is not made",
        "        std::fs::create_dir_all(&own).map_err(InstallError::Write)?;\n",
        "",
        [L_INSTALL],
    ),
    (
        "the fonts are not read again after an install",
        "        self.rescan();\n        Ok(Installed {\n",
        "        Ok(Installed {\n",
        [L_INSTALL],
    ),
    (
        "every name tried is the first",
        "        if n > 1 {\n",
        "        if n > NAME_TRIES {\n",
        [L_SAME_NAME],
    ),
    (
        "a name taken stops the install",
        "            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}\n",
        "",
        [L_SAME_NAME],
    ),
    (
        "the system's fonts are removed",
        "        if files.is_empty() {\n            return Err(RemoveError::SystemFonts);\n        }\n",
        "",
        [L_REMOVE],
    ),
    (
        "removing deletes nothing",
        "            match std::fs::remove_file(&file) {\n",
        "            match Ok::<(), io::Error>(()) {\n",
        [L_REMOVE],
    ),
    (
        "the fonts are not read again after a removal",
        "        self.rescan();\n        match failed {\n",
        "        match failed {\n",
        [L_REMOVE],
    ),
    (
        "a shared file's other family is not named",
        "            .filter(|other| !other.name.eq_ignore_ascii_case(family))\n",
        "            .filter(|other| other.name.eq_ignore_ascii_case(family))\n",
        [L_SHARING],
    ),
    (
        "regular is never said",
        "    if weight != \"Regular\" || (width.is_none() && !style.italic) {\n",
        "    if weight != \"Regular\" {\n",
        [L_STYLES],
    ),
    (
        "italic is never said",
        "        words.push(\"Italic\");\n",
        "",
        [L_STYLES],
    ),
]

# --- the preview (preview.rs) -----------------------------------------------
P_GLYPHS = "a_line_is_drawn_in_the_familys_own_glyphs"
P_TALL = "the_picture_is_one_line_tall"

PREVIEW = [
    (
        "nothing is drawn",
        "    font.draw_text(text, &mut target, PAD, PAD + ascent);\n",
        "    let _ = (&mut target, text);\n",
        [P_GLYPHS],
    ),
    (
        "the picture is drawn on nothing",
        "    let mut pixels = vec![background; count];\n",
        "    let mut pixels = vec![0; count];\n",
        [P_GLYPHS],
    ),
    (
        "the picture leaves out the descent",
        "    let tall = (ascent + descent + 2.0 * PAD).ceil();\n",
        "    let tall = (ascent + 2.0 * PAD).ceil();\n",
        [P_TALL],
    ),
    (
        "a picture is made for no width",
        "    if !tall.is_finite() || tall < 1.0 || width == 0 {\n",
        "    if !tall.is_finite() || tall < 1.0 {\n",
        [P_TALL],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "library.rs": LIBRARY,
    "preview.rs": PREVIEW,
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
        worst = max(worst, sweep(SRC / file, rows, "fontmanager", timeout=600, only=mine))
    raise SystemExit(worst)
