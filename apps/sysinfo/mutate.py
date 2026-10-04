"""Mutation test for System Information's unreadable-versus-empty categories.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27: each category keeps its read as a
`Result`, so a category that could not be read says why -- naming the path the
read used -- and one that was read and found nothing says "None found".  Both
used to draw an empty pane, and the one-value categories named
`/sys/hardware/...` paths nothing reads any more.  And a value nothing
reports -- a cache, the memory's speed and slots, the adapter's memory, a
refresh rate with no primary output -- is drawn as not reported, where it was
"0 KiB", "0 MHz", "0 / 0", "0 MiB" and "0 Hz".  The last rows, added
2026-10-04, cover the list of keys' hold on the pointer: a press with it up
puts it away and reaches nothing under it, and the wheel scrolls nothing it
covers.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

UNREADABLE = "an_unreadable_category_is_not_an_empty_one"
SUMMARY = "the_summary_invents_no_version_build_or_manufacturer"
ZERO = "a_value_nothing_reports_is_not_drawn_as_zero"

MUTATIONS = [
    (
        "an empty list draws an empty pane",
        "            Ok(list) if !list.is_empty() => Ok(list),",
        "            Ok(list) => Ok(list),",
        [UNREADABLE],
    ),
    (
        "an unreadable list draws an empty pane",
        "            Err(e) => Err(Self::unreadable(what, e)),",
        "            Err(_) => Ok(&[]),",
        [UNREADABLE],
    ),
    (
        "the reason names no path",
        '                format!("Not available — nothing on this system provides {path}")',
        '                String::from("Not available")',
        [UNREADABLE],
    ),
    (
        "the processor's reason names a path nothing reads",
        '            Err(e) => return Self::unreadable("Processor", e),',
        '            Err(_) => return vec![Property::new("Processor", "Not available — nothing on this system provides /sys/hardware/cpu")],',
        [UNREADABLE],
    ),
    (
        "the kernel version is invented again",
        "                    Ok(release) => release.as_str(),",
        '                    Ok(_) => "0.1.0-slateos",',
        [SUMMARY],
    ),
    (
        "the manufacturer is invented again",
        '            Property::new("System Manufacturer", Self::NOT_REPORTED),',
        '            Property::new("System Manufacturer", "SMBIOS: To Be Filled By O.E.M."),',
        [SUMMARY],
    ),
    (
        "an unreported cache is drawn as 0 KiB again",
        '            Property::new("L2 Cache", &Self::num_or_absent(cpu.l2_kb, "KiB")),',
        '            Property::new("L2 Cache", &format!("{} KiB", cpu.l2_kb.unwrap_or(0))),',
        [ZERO],
    ),
    (
        "an unreported speed is drawn as 0 MHz again",
        '            Property::new("Speed", &Self::num_or_absent(mem.speed_mhz, "MHz")),',
        '            Property::new("Speed", &format!("{} MHz", mem.speed_mhz.unwrap_or(0))),',
        [ZERO],
    ),
    (
        "unreported slots are drawn as 0 / 0 again",
        "                    _ => Self::NOT_REPORTED.to_string(),\n                },",
        '                    _ => String::from("0 / 0"),\n                },',
        [ZERO],
    ),
    (
        "a heading is drawn over no slots",
        "        if !mem.slots.is_empty() {",
        "        if true {",
        [ZERO],
    ),
    (
        "an unreported VRAM is drawn as 0 MiB again",
        "            || Self::NOT_REPORTED.to_string(),\n            |mb|",
        '            || String::from("0 MiB (0.0 GiB)"),\n            |mb|',
        [ZERO],
    ),
    (
        "the VRAM is scaled by a thousand",
        "f64::from(mb) / 1024.0",
        "f64::from(mb) / 1000.0",
        [ZERO],
    ),
    (
        "an empty name is drawn blank",
        "        Self::or_absent(Some(value).filter(|v| !v.is_empty()))",
        "        Self::or_absent(Some(value))",
        [ZERO],
    ),
    (
        "no primary output is called unreported",
        "                    Self::NO_PRIMARY\n                } else {",
        "                    Self::NOT_REPORTED\n                } else {",
        [ZERO],
    ),
    (
        "the refresh rate loses its unit",
        '|hz| format!("{hz} Hz")',
        '|hz| format!("{hz}")',
        [ZERO],
    ),
]

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

MUTATIONS += [
    # The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        "a press goes through the list of keys",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n"
        "                }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the list of keys away",
        "                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the list of keys leaves it up",
        "                    self.show_help = false;\n"
        "                    return EventResult::Consumed;\n",
        "                    return EventResult::Consumed;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the list of keys covers",
        "                MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

# The keys are plain and a Ctrl chord is not AltGr; the search box is the
# toolkit's field, edited by textline's editor (2026-10-04; lane C,
# c-e-a-theme-can-shape-the-controls).
CHORDS = "a_chord_is_no_key_here_and_types_nothing"
FIELD = "the_search_box_is_the_toolkits_field"

MUTATIONS += [
    (
        "a chord raises the list of keys",
        "        if key.key == Key::F1 && plain {\n",
        "        if key.key == Key::F1 {\n",
        [CHORDS],
    ),
    (
        "AltGr+E opens the export dialog",
        "            Key::E if ctrl => {\n",
        "            Key::E if key.modifiers.ctrl => {\n",
        [CHORDS],
    ),
    (
        "a chord works the window's keys",
        "            _ if !plain => return EventResult::Ignored,\n",
        "",
        [CHORDS],
    ),
    (
        "a chorded Escape leaves the search",
        "            Key::Escape if plain => {\n                self.search_focused = false;\n",
        "            Key::Escape => {\n                self.search_focused = false;\n",
        [CHORDS],
    ),
    (
        "the search box is never lit",
        "            hovered: self.search_hovered && !covered,\n",
        "            hovered: false,\n",
        [FIELD],
    ),
    (
        "the pointer over the search box is not noted",
        "                self.search_hovered = Self::toolbar_layout().search.contains(mouse.x, mouse.y);\n",
        "",
        [FIELD],
    ),
    (
        "the search box never has the keyboard's mark",
        "            focused: self.search_focused && !covered,\n",
        "            focused: false,\n",
        [FIELD],
    ),
    (
        "the search box keeps its mark under the list of keys",
        "            focused: self.search_focused && !covered,\n",
        "            focused: self.search_focused,\n",
        [FIELD],
    ),
    (
        "a search that finds nothing is not red",
        "            invalid: !self.search_text.is_empty() && self.search_all(&self.search_text).is_empty(),\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "a press leaves the caret where it was",
        "        self.search_editor.set_cursor(cursor);\n",
        "        let _ = cursor;\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "sysinfo-app", timeout=600, only=only))
