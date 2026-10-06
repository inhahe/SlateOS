//! The picker as tools see it -- a screen reader, a script
//! ([`guitk::widget::automation::Accessible`]): a dialog holding its search
//! field, its list of categories, its grid of characters and its skin
//! tones. Each is a part a tool can find by what it is called and act on as
//! its user would: a character pressed is picked, a category or a tone
//! chosen, the search's text set, the keyboard given, the grid scrolled.
//!
//! The boxes are the ones the picker is drawn and clicked in, read from the
//! same layout and the same scroll; a category or a character scrolled out
//! of its well has its box above or below the well, where it would be.

use guitk::frame::Rect;
use guitk::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

use crate::{
    CharPicker, CharPickerEvent, Layout, Part, SideRow, SkinTone, TITLE, TONES, Target,
    code_points, count_f32, tone_rect,
};

/// What tools call a skin tone's swatch.
fn tone_name(tone: Option<SkinTone>) -> String {
    tone.map_or_else(
        || "No skin tone".to_string(),
        |tone| {
            let name = tone.name();
            let mut chars = name.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
            format!("{}{} skin tone", first.unwrap_or_default(), chars.as_str())
        },
    )
}

/// How far `row` is below `first`, in rows: less than nothing above it.
fn rows_below(row: usize, first: usize) -> f32 {
    count_f32(row) - count_f32(first)
}

impl Accessible for CharPicker {
    type Part = Target;
    type Event = CharPickerEvent;

    fn automation(&self, width: f32, height: f32) -> Node<Target> {
        let layout = Layout::new(width, height);
        let searching = !self.search.text().trim().is_empty();

        let mut search = Node::new(
            Target::Search,
            Role::TextField,
            crate::SEARCH_HINT,
            layout.search,
        );
        search.value = Some(Value::Text(self.search.text().to_string()));
        search.focused = self.focus == Part::Search;
        search.focusable = true;

        let mut list = Node::new(Target::Sidebar, Role::List, "Categories", layout.sidebar);
        list.focused = self.focus == Part::Categories;
        list.focusable = true;
        let view = self.side_view_in(layout.sidebar);
        for (row, side_row) in self.side.iter().enumerate() {
            let SideRow::Category(category) = side_row else {
                continue;
            };
            let at = self.side_row_rect(layout.sidebar, &view, row);
            let mut item = Node::new(Target::Category(row), Role::ListItem, category.name(), at);
            item.value = Some(Value::Chosen(!searching && view.selected() == Some(row)));
            list.children.push(item);
        }

        let cells = layout.cells();
        let grid_name = if searching {
            "Search results"
        } else {
            self.category.name()
        };
        let mut grid = Node::new(Target::Grid, Role::Grid, grid_name, layout.grid);
        grid.focused = self.focus == Part::Grid;
        grid.focusable = true;
        let first_row = self.grid_first(&cells);
        for (i, cell) in self.shown.iter().enumerate() {
            let shown = cell.toned(self.tone);
            let text = shown.text();
            let at = Rect::new(
                cells.area.x + count_f32(cells.column_of(i)) * cells.width,
                cells.area.y + rows_below(cells.row_of(i), first_row) * cells.height,
                cells.width,
                cells.height,
            );
            let name = shown
                .name()
                .map_or_else(|| code_points(&text), str::to_string);
            let mut node = Node::new(Target::Cell(i), Role::GridCell, name, at);
            node.description = Some(code_points(&text));
            node.value = Some(Value::Text(text));
            node.focused = self.focus == Part::Grid && self.cursor == Some(i);
            node.focusable = true;
            grid.children.push(node);
        }

        let mut tones = Node::new(Target::Tones, Role::Group, "Skin tone", layout.tones);
        tones.focused = self.focus == Part::Tones;
        tones.focusable = true;
        for (slot, &tone) in TONES.iter().enumerate() {
            let mut swatch = Node::new(
                Target::Tone(tone),
                Role::RadioButton,
                tone_name(tone),
                tone_rect(layout.tones, slot),
            );
            swatch.value = Some(Value::Chosen(tone == self.tone));
            tones.children.push(swatch);
        }

        let mut root = Node::new(
            Target::Picker,
            Role::Dialog,
            TITLE,
            Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0)),
        );
        root.children = vec![search, list, grid, tones];
        root
    }

    fn invoke(
        &mut self,
        part: &Target,
        action: Action,
        width: f32,
        height: f32,
    ) -> Result<Option<CharPickerEvent>, Refusal> {
        let layout = Layout::new(width, height);
        self.fit(&layout);
        let cells = layout.cells();
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (part, &action) {
            (Target::Search, Action::SetText(text)) => {
                self.search.set_text(text);
                self.refresh();
            }
            (Target::Search, Action::Focus) => self.focus = Part::Search,
            (Target::Search, _) => return Err(not_for(Role::TextField)),
            (Target::Sidebar, Action::Focus) => self.focus = Part::Categories,
            (Target::Sidebar, _) => return Err(not_for(Role::List)),
            (Target::Category(row), Action::Choose | Action::Press) => {
                if !matches!(self.side.get(*row), Some(SideRow::Category(_))) {
                    return Err(Refusal::NoSuchWidget);
                }
                self.choose(*row);
            }
            (Target::Category(_), _) => return Err(not_for(Role::ListItem)),
            (Target::Grid, Action::Focus) => self.enter_grid(&cells),
            (Target::Grid, Action::ScrollTo { y, .. }) => {
                if !y.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                let rows_down = (y / cells.height).floor();
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a finite count of rows, at least nought, floored"
                )]
                let rows_down = if rows_down > 0.0 {
                    rows_down as usize
                } else {
                    0
                };
                let last_first = cells
                    .rows(self.shown.len())
                    .saturating_sub(cells.rows_shown);
                self.first_row = rows_down.min(last_first);
            }
            (Target::Grid, _) => return Err(not_for(Role::Grid)),
            (Target::Cell(i), Action::Press) => {
                // A pick, as a click on it is: the host inserts it.
                return self.pick(*i).map(Some).ok_or(Refusal::NoSuchWidget);
            }
            (Target::Cell(i), Action::Focus) => {
                if *i >= self.shown.len() {
                    return Err(Refusal::NoSuchWidget);
                }
                self.focus = Part::Grid;
                self.move_cursor(*i, &cells);
            }
            (Target::Cell(_), _) => return Err(not_for(Role::GridCell)),
            (Target::Tones, Action::Focus) => self.focus = Part::Tones,
            (Target::Tones, _) => return Err(not_for(Role::Group)),
            (Target::Tone(tone), Action::Choose | Action::Press) => self.tone = *tone,
            (Target::Tone(_), _) => return Err(not_for(Role::RadioButton)),
            (Target::Picker, _) => return Err(not_for(Role::Dialog)),
            // A scrollbar is the grid's or the list's, not a part of its own.
            (Target::Track(_) | Target::Thumb(_), _) => return Err(Refusal::NoSuchWidget),
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )]

    use super::*;
    use guitk::palette::Palette;
    use guitk::widget::automation::Query;

    const W: f32 = 560.0;
    const H: f32 = 460.0;

    fn part(picker: &CharPicker, target: &Target) -> Node<Target> {
        picker
            .automation(W, H)
            .walk()
            .find(|n| n.id == *target)
            .cloned()
            .unwrap_or_else(|| panic!("no {target:?}"))
    }

    /// **The picker is a dialog of four parts, each named and holding what
    /// it shows** -- the search's text, the categories with the one chosen,
    /// the characters by name and code point, the tones with the one chosen.
    #[test]
    fn the_picker_shows_tools_its_parts() {
        let picker = CharPicker::new();
        let tree = picker.automation(W, H);
        assert_eq!((tree.role, tree.name.as_str()), (Role::Dialog, TITLE));
        let roles: Vec<Role> = tree.children.iter().map(|n| n.role).collect();
        assert_eq!(
            roles,
            [Role::TextField, Role::List, Role::Grid, Role::Group]
        );

        let search = part(&picker, &Target::Search);
        assert_eq!(search.name, crate::SEARCH_HINT);
        assert_eq!(search.value, Some(Value::Text(String::new())));
        assert!(search.focused, "the keyboard starts in the search");

        let list = &tree.children[1];
        assert!(list.children.iter().all(|n| n.role == Role::ListItem));
        let chosen: Vec<&str> = list
            .children
            .iter()
            .filter(|n| n.value == Some(Value::Chosen(true)))
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(chosen, ["Smileys & Emotion"]);
        assert!(
            list.children.iter().all(|n| n.name != "Emoji"),
            "no headings"
        );

        let grid = &tree.children[2];
        assert_eq!(grid.name, "Smileys & Emotion");
        let first = &grid.children[0];
        assert_eq!(first.role, Role::GridCell);
        assert_eq!(first.name, "grinning face");
        assert_eq!(first.description.as_deref(), Some("U+1F600"));
        assert_eq!(first.value, Some(Value::Text("\u{1F600}".into())));

        let tones = &tree.children[3];
        let names: Vec<&str> = tones.children.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "No skin tone",
                "Light skin tone",
                "Medium-light skin tone",
                "Medium skin tone",
                "Medium-dark skin tone",
                "Dark skin tone"
            ]
        );
        assert_eq!(tones.children[0].value, Some(Value::Chosen(true)));
    }

    /// **A part's box is where the picker draws and hits it**: the box tools
    /// are told for every category, cell and tone on screen is the one the
    /// frame records for it.
    #[test]
    fn a_parts_box_is_where_it_is_drawn() {
        let mut picker = CharPicker::new();
        picker
            .invoke(&Target::Grid, Action::ScrollTo { x: 0.0, y: 200.0 }, W, H)
            .unwrap();
        let frame = picker.frame(&Palette::for_mode(false), W, H);
        let tree = picker.automation(W, H);
        let mut checked = 0;
        for node in tree.walk() {
            if !matches!(
                node.id,
                Target::Category(_) | Target::Cell(_) | Target::Tone(_)
            ) {
                continue;
            }
            if let Some(drawn) = frame.rect_of(|t| *t == node.id) {
                let b = node.bounds;
                assert!(
                    (drawn.x - b.x).abs() < 0.01
                        && (drawn.y - b.y).abs() < 0.01
                        && (drawn.w - b.w).abs() < 0.01
                        && (drawn.h - b.h).abs() < 0.01,
                    "{:?}: told {b:?}, drawn {drawn:?}",
                    node.id
                );
                checked += 1;
            }
        }
        assert!(checked > 50, "{checked}");
        // A cell scrolled above the grid is told above it.
        let above = part(&picker, &Target::Cell(0));
        assert!(above.bounds.y < tree.children[2].bounds.y);
    }

    /// **A tool searches, picks and chooses as the user would**: the search's
    /// text set searches; a cell pressed is picked, the event its host
    /// inserts; a tone chosen tones the emoji; a category chosen shows it.
    #[test]
    fn a_tool_acts_as_the_user_would() {
        let mut picker = CharPicker::new();
        picker
            .invoke(&Target::Search, Action::SetText("waving hand".into()), W, H)
            .unwrap();
        let tree = picker.automation(W, H);
        assert_eq!(tree.children[2].name, "Search results");
        let wave = Query {
            name: Some("waving hand".into()),
            ..Query::default()
        }
        .find_in(&tree);
        assert_eq!(wave, [Target::Cell(0)]);
        picker
            .invoke(&Target::Tone(Some(SkinTone::Dark)), Action::Choose, W, H)
            .unwrap();
        assert_eq!(
            picker.invoke(&Target::Cell(0), Action::Press, W, H),
            Ok(Some(CharPickerEvent::Picked("\u{1F44B}\u{1F3FF}".into())))
        );
        assert_eq!(picker.recent(), ["\u{1F44B}"]);

        let flags = picker
            .side
            .iter()
            .position(|r| matches!(r, SideRow::Category(c) if c.name() == "Flags"))
            .unwrap();
        picker
            .invoke(&Target::Category(flags), Action::Choose, W, H)
            .unwrap();
        assert_eq!(picker.category().name(), "Flags");
        assert_eq!(picker.search_text(), "", "a category replaces a search");

        picker
            .invoke(&Target::Cell(3), Action::Focus, W, H)
            .unwrap();
        assert_eq!(picker.focus(), Part::Grid);
        assert_eq!(picker.cursor(), Some(3));
        assert!(part(&picker, &Target::Cell(3)).focused);
    }

    /// **What a part is not for is refused, and changes nothing.**
    #[test]
    fn what_a_part_is_not_for_is_refused() {
        let mut picker = CharPicker::new();
        assert_eq!(
            picker.invoke(&Target::Cell(0), Action::SetText("x".into()), W, H),
            Err(Refusal::NotApplicable {
                role: Role::GridCell,
                action: "set the text of"
            })
        );
        assert_eq!(
            picker.invoke(&Target::Cell(99_999), Action::Press, W, H),
            Err(Refusal::NoSuchWidget)
        );
        assert_eq!(
            picker.invoke(&Target::Category(1), Action::Choose, W, H),
            Err(Refusal::NoSuchWidget),
            "row 1 is a heading"
        );
        assert_eq!(
            picker.invoke(
                &Target::Grid,
                Action::ScrollTo {
                    x: 0.0,
                    y: f32::NAN
                },
                W,
                H
            ),
            Err(Refusal::NotANumber)
        );
        assert!(matches!(
            picker.invoke(&Target::Picker, Action::Press, W, H),
            Err(Refusal::NotApplicable {
                role: Role::Dialog,
                ..
            })
        ));
        assert!(picker.recent().is_empty());
        assert_eq!(picker.category(), crate::Category::Emoji(0));
    }
}
