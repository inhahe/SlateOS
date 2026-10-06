//! The font picker as tools see it -- a screen reader, a script
//! ([`crate::widget::automation::Accessible`]): a dialog holding the search
//! field, the list of families, the list of weights, the size field, the
//! list of sizes, a picture of the choice, and OK and Cancel. A tool finds
//! each by what it is called and acts on it as its user would: a family, a
//! weight or a size chosen is a choice the host may preview, a field's text
//! set searches or sizes, OK keeps the choice and Cancel turns it down --
//! each answering the event the host already acts on from a click.
//!
//! The boxes are the ones the dialog is drawn and clicked in: the same
//! layout, the same scroll, the same row boxes (`list_row_rect`).

use super::{
    FAMILY_ROW, FontPickerDialog, FontPickerEvent, Layout, Part, ROW, SIZES, Target, bar_width,
    list_row_rect, parse_size, rows_in, shown_rows, weight_name,
};
use crate::frame::Rect;
use crate::text::scaled;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A list node named `name` over `rect`, its rows `rows` -- their target,
/// name and whether each is the one chosen -- drawn from row `first`, each
/// `row_h` tall, short of a scrollbar column `bar` wide.
fn list_node(
    id: Target,
    name: &str,
    rect: Rect,
    (first, row_h, bar): (usize, f32, f32),
    rows: impl Iterator<Item = (Target, String, bool)>,
) -> Node<Target> {
    let mut list = Node::new(id, Role::List, name, rect);
    list.focusable = true;
    list.children = rows
        .enumerate()
        .map(|(i, (target, row_name, chosen))| {
            let mut item = Node::new(
                target,
                Role::ListItem,
                row_name,
                list_row_rect(rect, first, i, row_h, bar),
            );
            item.value = Some(Value::Chosen(chosen));
            item
        })
        .collect();
    list
}

impl Accessible for FontPickerDialog {
    type Part = Target;
    type Event = FontPickerEvent;

    fn automation(&self, width: f32, height: f32) -> Node<Target> {
        let layout = Layout::new(width, height);
        let finished = self.finished();

        let mut search = Node::new(Target::Search, Role::TextField, "Search", layout.search);
        search.value = Some(Value::Text(self.search.text().to_string()));

        let family_h = scaled(FAMILY_ROW);
        let len = self.shown.len();
        let capacity = rows_in(layout.families.h, family_h);
        let first = shown_rows(self.family_view, len, capacity).start;
        let selected = self.family_view.selected();
        let families = list_node(
            Target::FamilyList,
            "Family",
            layout.families,
            (first, family_h, bar_width(layout.families, len, capacity)),
            self.shown.iter().enumerate().filter_map(|(i, &f)| {
                let name = self.families.get(f)?;
                Some((Target::Family(i), name.clone(), selected == Some(i)))
            }),
        );

        let row_h = scaled(ROW);
        let weights = list_node(
            Target::WeightList,
            "Weight",
            layout.weights,
            (0, row_h, 0.0),
            self.weights().iter().map(|&w| {
                (
                    Target::Weight(w),
                    weight_name(w).to_string(),
                    self.choice.weight == w,
                )
            }),
        );

        let mut size_field = Node::new(
            Target::SizeField,
            Role::TextField,
            "Size",
            layout.size_field,
        );
        size_field.value = Some(Value::Text(self.size_text.text().to_string()));

        let capacity = rows_in(layout.sizes.h, row_h);
        let first = shown_rows(self.size_view, SIZES.len(), capacity).start;
        let sizes = list_node(
            Target::SizeList,
            "Size list",
            layout.sizes,
            (first, row_h, bar_width(layout.sizes, SIZES.len(), capacity)),
            SIZES
                .iter()
                .enumerate()
                .map(|(i, &px)| (Target::Size(i), px.to_string(), self.choice.size == px)),
        );

        let ok = Node::new(Target::Ok, Role::Button, "OK", layout.ok);
        let cancel = Node::new(Target::Cancel, Role::Button, "Cancel", layout.cancel);

        let mut parts = vec![search, families, weights, size_field, sizes, ok, cancel];
        let focus = [
            Part::Search,
            Part::Families,
            Part::Weights,
            Part::SizeField,
            Part::Sizes,
            Part::Ok,
            Part::Cancel,
        ];
        for (node, part) in parts.iter_mut().zip(focus) {
            node.focused = !finished && self.focus == part;
            node.focusable = !finished;
            node.enabled = !finished;
        }

        let mut root = Node::new(
            Target::Dialog,
            Role::Dialog,
            "Font",
            Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0)),
        );
        root.description = Some(format!(
            "{}, {}, {} pixels",
            self.choice.family,
            weight_name(self.choice.weight),
            self.choice.size
        ));
        root.enabled = !finished;
        root.children = parts;
        root
    }

    fn invoke(
        &mut self,
        part: &Target,
        action: Action,
        width: f32,
        height: f32,
    ) -> Result<Option<FontPickerEvent>, Refusal> {
        if self.finished() {
            return Err(Refusal::Disabled);
        }
        self.fit(&Layout::new(width, height));
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        let event = match (part, &action) {
            (Target::Search, Action::SetText(text)) => {
                self.focus = Part::Search;
                self.search.set_text(text);
                self.searched()
            }
            (Target::SizeField, Action::SetText(text)) => {
                self.focus = Part::SizeField;
                self.size_text.set_text(text);
                parse_size(text).and_then(|px| self.pick_size(px, false))
            }
            (Target::Family(i), Action::Choose | Action::Press) => {
                if *i >= self.shown.len() {
                    return Err(Refusal::NoSuchWidget);
                }
                self.focus = Part::Families;
                self.pick_family(*i)
            }
            (Target::Weight(weight), Action::Choose | Action::Press) => {
                if !self.weights().contains(weight) {
                    return Err(Refusal::NoSuchWidget);
                }
                self.focus = Part::Weights;
                self.pick_weight(*weight)
            }
            (Target::Size(i), Action::Choose | Action::Press) => {
                let px = *SIZES.get(*i).ok_or(Refusal::NoSuchWidget)?;
                self.focus = Part::Sizes;
                self.pick_size(px, true)
            }
            (Target::Ok, Action::Press) => Some(self.confirm()),
            (Target::Cancel, Action::Press) => Some(self.cancel()),
            (target, Action::Focus) => {
                self.focus = match target {
                    Target::Search => Part::Search,
                    Target::FamilyList | Target::Family(_) => Part::Families,
                    Target::WeightList | Target::Weight(_) => Part::Weights,
                    Target::SizeField => Part::SizeField,
                    Target::SizeList | Target::Size(_) => Part::Sizes,
                    Target::Ok => Part::Ok,
                    Target::Cancel => Part::Cancel,
                    Target::Dialog | Target::Track(_) | Target::Thumb(_) => {
                        return Err(not_for(Role::Dialog));
                    }
                };
                self.entered();
                None
            }
            (Target::Search | Target::SizeField, _) => return Err(not_for(Role::TextField)),
            (Target::FamilyList | Target::WeightList | Target::SizeList, _) => {
                return Err(not_for(Role::List));
            }
            (Target::Family(_) | Target::Weight(_) | Target::Size(_), _) => {
                return Err(not_for(Role::ListItem));
            }
            (Target::Ok | Target::Cancel, _) => return Err(not_for(Role::Button)),
            (Target::Dialog, _) => return Err(not_for(Role::Dialog)),
            // A scrollbar is its list's, not a part of its own.
            (Target::Track(_) | Target::Thumb(_), _) => return Err(Refusal::NoSuchWidget),
        };
        Ok(event)
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

    use super::super::FontChoice;
    use super::*;
    use crate::palette::Palette;
    use crate::render::FontWeightHint;
    use crate::widget::automation::Query;

    const W: f32 = 560.0;
    const H: f32 = 440.0;

    fn families() -> Vec<String> {
        (0..40).map(|i| format!("Family {i:02}")).collect()
    }

    fn picker() -> FontPickerDialog {
        FontPickerDialog::over(
            FontChoice::new("Family 03", FontWeightHint::Regular, 13),
            families(),
            |name| name.ends_with('3'),
        )
    }

    /// **The dialog is its parts, each named and holding what it shows** --
    /// the families with the one chosen, the weights the family can be drawn
    /// at, the sizes, the fields' text, the choice described.
    #[test]
    fn the_dialog_shows_tools_its_parts() {
        let p = picker();
        let tree = p.automation(W, H);
        assert_eq!((tree.role, tree.name.as_str()), (Role::Dialog, "Font"));
        assert_eq!(
            tree.description.as_deref(),
            Some("Family 03, Regular, 13 pixels")
        );
        let roles: Vec<Role> = tree.children.iter().map(|n| n.role).collect();
        assert_eq!(
            roles,
            [
                Role::TextField,
                Role::List,
                Role::List,
                Role::TextField,
                Role::List,
                Role::Button,
                Role::Button
            ]
        );
        let families = &tree.children[1];
        assert_eq!(families.children.len(), 40);
        let chosen: Vec<&str> = families
            .children
            .iter()
            .filter(|n| n.value == Some(Value::Chosen(true)))
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(chosen, ["Family 03"]);
        let weights: Vec<&str> = tree.children[2]
            .children
            .iter()
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(weights, ["Regular", "Bold"], "Family 03 has a bold face");
        assert_eq!(
            tree.children[3].value,
            Some(Value::Text("13".into())),
            "the size field"
        );
        assert!(
            tree.children[0].focused,
            "the keyboard starts in the search"
        );
    }

    /// **A part's box is where the dialog draws and hits it.**
    #[test]
    fn a_parts_box_is_where_it_is_drawn() {
        let p = picker();
        let frame = p.frame(&Palette::for_mode(false), W, H);
        let tree = p.automation(W, H);
        let mut checked = 0;
        for node in tree.walk() {
            if let Some(drawn) = frame.rect_of(|t| *t == node.id) {
                // A row cut by its list's well is hit only where it shows.
                if drawn.h < node.bounds.h - 0.01 {
                    continue;
                }
                assert!(
                    (drawn.x - node.bounds.x).abs() < 0.01
                        && (drawn.y - node.bounds.y).abs() < 0.01
                        && (drawn.w - node.bounds.w).abs() < 0.01
                        && (drawn.h - node.bounds.h).abs() < 0.01,
                    "{:?}: told {:?}, drawn {drawn:?}",
                    node.id,
                    node.bounds
                );
                checked += 1;
            }
        }
        assert!(checked > 20, "{checked}");
    }

    /// **A tool chooses as the user would, and the host hears what it hears
    /// from a click**: a family chosen is a change to preview, a size typed
    /// too, OK keeps the choice -- and then the dialog takes nothing more.
    #[test]
    fn a_tool_chooses_as_the_user_would() {
        let mut p = picker();
        let tree = p.automation(W, H);
        let found = Query {
            name: Some("Family 13".into()),
            ..Query::default()
        }
        .find_in(&tree);
        assert_eq!(found, [Target::Family(13)]);
        let changed = p.invoke(&Target::Family(13), Action::Choose, W, H).unwrap();
        assert!(matches!(changed, Some(FontPickerEvent::Changed(c)) if c.family == "Family 13"));
        p.invoke(&Target::Weight(FontWeightHint::Bold), Action::Choose, W, H)
            .unwrap();
        assert_eq!(p.choice().weight, FontWeightHint::Bold);
        let sized = p
            .invoke(&Target::SizeField, Action::SetText("20".into()), W, H)
            .unwrap();
        assert!(matches!(sized, Some(FontPickerEvent::Changed(c)) if c.size == 20));
        p.invoke(&Target::Search, Action::SetText("family 2".into()), W, H)
            .unwrap();
        assert!(p.shown().all(|f| f.starts_with("Family 2")));
        let kept = p.invoke(&Target::Ok, Action::Press, W, H).unwrap();
        assert!(matches!(kept, Some(FontPickerEvent::Confirmed(_))));
        assert_eq!(
            p.invoke(&Target::Cancel, Action::Press, W, H),
            Err(Refusal::Disabled),
            "a dialog done with takes nothing more"
        );
        assert!(!p.automation(W, H).enabled);
    }

    /// **What a part is not for is refused, and changes nothing.**
    #[test]
    fn what_a_part_is_not_for_is_refused() {
        let mut p = picker();
        assert_eq!(
            p.invoke(&Target::Family(99), Action::Choose, W, H),
            Err(Refusal::NoSuchWidget)
        );
        assert!(matches!(
            p.invoke(&Target::Ok, Action::SetText("x".into()), W, H),
            Err(Refusal::NotApplicable {
                role: Role::Button,
                ..
            })
        ));
        // Family 00 has no bold face, so bold is no weight it offers.
        p.invoke(&Target::Family(0), Action::Choose, W, H).unwrap();
        assert_eq!(
            p.invoke(&Target::Weight(FontWeightHint::Bold), Action::Choose, W, H),
            Err(Refusal::NoSuchWidget)
        );
        p.invoke(&Target::Cancel, Action::Focus, W, H).unwrap();
        assert_eq!(p.focus(), Part::Cancel);
        assert!(!p.is_confirmed() && !p.is_cancelled());
    }
}
