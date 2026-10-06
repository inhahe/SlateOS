#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use super::*;
use crate::colorpicker::PRESET_COLORS;
use crate::widget::automation::Query;

const W: f32 = 400.0;
const H: f32 = 600.0;

/// The node of `part`, in `dialog` drawn `W` by `H`.
fn node(dialog: &ColorPickerDialog, part: ColorPart) -> Node<ColorPart> {
    dialog
        .automation(W, H)
        .walk()
        .find(|node| node.id == part)
        .unwrap_or_else(|| panic!("no {part:?}"))
        .clone()
}

/// The value a slider node holds.
fn range(node: &Node<ColorPart>) -> (f64, f64, f64) {
    match node.value {
        Some(Value::Range { value, min, max }) => (value, min, max),
        ref other => panic!("{:?} holds {other:?}", node.id),
    }
}

/// **The dialog shows tools every part**: its square's two axes, the hue
/// and opacity bars, the colour, the hex field, the eyedropper, the two tabs
/// and the three sliders of the one showing, the presets, OK and Cancel --
/// each called what a user calls it, and found by that name.
#[test]
fn the_dialog_shows_tools_every_part() {
    let dialog = ColorPickerDialog::new(Color::rgb(255, 0, 0));
    let tree = dialog.automation(W, H);
    assert_eq!(tree.role, Role::Dialog);
    assert_eq!(tree.name, DIALOG_TITLE);
    let names: Vec<(Role, String)> = tree
        .children
        .iter()
        .map(|node| (node.role, node.name.clone()))
        .collect();
    let want = [
        (Role::Group, "Saturation and brightness"),
        (Role::Slider, "Hue"),
        (Role::Slider, "Opacity"),
        (Role::Image, "Color"),
        (Role::TextField, "Hex"),
        (Role::Button, "Eyedropper"),
        (Role::RadioButton, "RGB"),
        (Role::RadioButton, "HSV"),
        (Role::Slider, "Red"),
        (Role::Slider, "Green"),
        (Role::Slider, "Blue"),
        (Role::Grid, "Presets"),
        (Role::Button, "OK"),
        (Role::Button, "Cancel"),
    ];
    assert_eq!(
        names,
        want.map(|(role, name)| (role, name.to_owned())),
        "no recent colours yet: no list of them"
    );
    let red = node(&dialog, ColorPart::Row(0));
    assert_eq!(range(&red), (255.0, 0.0, 255.0));
    assert_eq!(
        node(&dialog, ColorPart::Preview).value,
        Some(Value::Text("#FF0000".to_owned()))
    );
    let ok = Query {
        role: Some(Role::Button),
        name: Some("ok".to_owned()),
        ..Query::default()
    };
    assert_eq!(ok.find_in(&tree), [ColorPart::Ok]);
    let presets = node(&dialog, ColorPart::Presets);
    assert_eq!(presets.children.len(), PRESET_COLORS.len());
    for part in tree.walk() {
        assert!(part.enabled, "{:?} in use", part.id);
    }
}

/// **A part's box is where it is drawn and clicked**: a click in the middle
/// of the OK node is a confirmation, of a preset's the preset's colour, of
/// the tab's the tab.
#[test]
fn a_parts_box_is_where_it_is_clicked() {
    let middle = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let press = |x, y| crate::event::MouseEvent {
        x,
        y,
        kind: crate::event::MouseEventKind::Press(crate::event::MouseButton::Left),
    };

    let mut dialog = ColorPickerDialog::new(Color::rgb(1, 2, 3));
    let (x, y) = middle(node(&dialog, ColorPart::Preset(3)).bounds);
    assert_eq!(
        dialog.handle_mouse(&press(x, y), W, H),
        Some(ColorPickerEvent::Changed(PRESET_COLORS[3]))
    );
    let (x, y) = middle(node(&dialog, ColorPart::Tab(SliderTab::Hsv)).bounds);
    dialog.handle_mouse(&press(x, y), W, H);
    assert_eq!(dialog.slider_tab(), SliderTab::Hsv);
    let (x, y) = middle(node(&dialog, ColorPart::Ok).bounds);
    assert!(matches!(
        dialog.handle_mouse(&press(x, y), W, H),
        Some(ColorPickerEvent::Confirmed(_))
    ));
}

/// **The sliders are set as a user sets them**: the square's two axes and
/// the hue in their own measures, the opacity in bytes, a tab's sliders in
/// the tab's -- each change answered as a drag's is, held to its range, and
/// a value that is no number refused.
#[test]
fn the_sliders_are_set_as_a_user_sets_them() {
    let mut dialog = ColorPickerDialog::new(Color::rgb(255, 0, 0));
    let set = |dialog: &mut ColorPickerDialog, part, value| {
        dialog.invoke(&part, Action::SetValue(value), W, H)
    };
    assert_eq!(
        set(&mut dialog, ColorPart::Hue, 120.0),
        Ok(Some(ColorPickerEvent::Changed(Color::rgb(0, 255, 0))))
    );
    assert_eq!(dialog.picker().hex_input(), "00FF00", "the field follows");
    set(&mut dialog, ColorPart::Brightness, 50.0).unwrap();
    let (_, g, _) = hsv_to_rgb(dialog.picker().hsv());
    assert!((i32::from(g) - 128).abs() <= 1, "green at half: {g}");
    set(&mut dialog, ColorPart::Saturation, 0.0).unwrap();
    assert_eq!(range(&node(&dialog, ColorPart::Saturation)).0, 0.0);
    set(&mut dialog, ColorPart::Opacity, 300.0).unwrap();
    assert_eq!(dialog.picker().alpha(), 255, "held to its range");
    set(&mut dialog, ColorPart::Opacity, 64.0).unwrap();
    assert_eq!(dialog.current_color().a, 64);

    let mut dialog = ColorPickerDialog::new(Color::rgb(0, 0, 0));
    set(&mut dialog, ColorPart::Row(2), 200.0).unwrap();
    assert_eq!(dialog.current_color(), Color::rgb(0, 0, 200), "blue");
    dialog
        .invoke(&ColorPart::Tab(SliderTab::Hsv), Action::Choose, W, H)
        .unwrap();
    assert_eq!(node(&dialog, ColorPart::Row(0)).name, "Hue");
    assert_eq!(range(&node(&dialog, ColorPart::Row(1))).2, 100.0);
    set(&mut dialog, ColorPart::Row(2), 100.0).unwrap();
    assert_eq!(
        dialog.current_color(),
        Color::rgb(0, 0, 255),
        "value at full"
    );

    assert_eq!(
        set(&mut dialog, ColorPart::Hue, f64::NAN),
        Err(Refusal::NotANumber)
    );
    assert_eq!(
        set(&mut dialog, ColorPart::Row(3), 1.0),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        dialog.invoke(&ColorPart::Hue, Action::Press, W, H),
        Err(Refusal::NotApplicable {
            role: Role::Slider,
            action: "press"
        })
    );
}

/// **The hex field takes a code in any form it takes typed** -- with or
/// without its `#`, three digits or eight -- and refuses what is no colour.
#[test]
fn the_hex_field_takes_a_code() {
    let mut dialog = ColorPickerDialog::new(Color::rgb(0, 0, 0));
    let text = |dialog: &mut ColorPickerDialog, code: &str| {
        dialog.invoke(&ColorPart::Hex, Action::SetText(code.to_owned()), W, H)
    };
    assert_eq!(
        text(&mut dialog, "#00f"),
        Ok(Some(ColorPickerEvent::Changed(Color::rgb(0, 0, 255))))
    );
    assert_eq!(
        node(&dialog, ColorPart::Hex).value,
        Some(Value::Text("0000FF".to_owned()))
    );
    text(&mut dialog, "FF000080").unwrap();
    assert_eq!(dialog.current_color(), Color::rgba(0xFF, 0, 0, 0x80));
    assert_eq!(
        node(&dialog, ColorPart::Preview).value,
        Some(Value::Text("#FF000080".to_owned())),
        "a colour not opaque says so"
    );
    let before = dialog.current_color();
    assert_eq!(text(&mut dialog, "#zz0000"), Err(Refusal::NotANumber));
    assert_eq!(dialog.current_color(), before, "nothing changed");
    dialog.invoke(&ColorPart::Hex, Action::Focus, W, H).unwrap();
    assert!(node(&dialog, ColorPart::Hex).focused);
}

/// **A preset or a recent colour pressed is the colour**; OK keeps it among
/// the recent, which then appear as a list; Cancel goes back.
#[test]
fn presets_recent_ok_and_cancel() {
    let mut dialog = ColorPickerDialog::new(Color::rgb(9, 9, 9));
    assert_eq!(
        dialog.invoke(&ColorPart::Preset(8), Action::Press, W, H),
        Ok(Some(ColorPickerEvent::Changed(PRESET_COLORS[8])))
    );
    assert_eq!(
        node(&dialog, ColorPart::Preset(8)).value,
        Some(Value::Chosen(true))
    );
    assert_eq!(
        dialog.invoke(&ColorPart::Preset(PRESET_COLORS.len()), Action::Press, W, H),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        dialog.invoke(&ColorPart::Ok, Action::Press, W, H),
        Ok(Some(ColorPickerEvent::Confirmed(PRESET_COLORS[8])))
    );
    let recent = node(&dialog, ColorPart::Recent);
    assert_eq!(recent.role, Role::List);
    assert_eq!(recent.children.len(), 1);
    assert_eq!(recent.children[0].name, color_name(PRESET_COLORS[8]));

    dialog.picker_mut().set_color(Color::rgb(1, 1, 1));
    assert_eq!(
        dialog.invoke(&ColorPart::RecentColor(0), Action::Press, W, H),
        Ok(Some(ColorPickerEvent::Changed(PRESET_COLORS[8])))
    );
    assert_eq!(
        dialog.invoke(&ColorPart::RecentColor(1), Action::Press, W, H),
        Err(Refusal::NoSuchWidget)
    );
    assert_eq!(
        dialog.invoke(&ColorPart::Cancel, Action::Press, W, H),
        Ok(Some(ColorPickerEvent::Cancelled))
    );
    assert_eq!(dialog.current_color(), Color::rgb(9, 9, 9));
}

/// **While the eyedropper picks, it alone is in use**: every other part
/// says so and refuses, and pressing it again stops the pick.
#[test]
fn while_the_eyedropper_picks_it_alone_is_in_use() {
    let mut dialog = ColorPickerDialog::new(Color::rgb(9, 9, 9));
    assert_eq!(
        dialog.invoke(&ColorPart::Eyedropper, Action::Press, W, H),
        Ok(Some(ColorPickerEvent::EyedropperActivated))
    );
    let tree = dialog.automation(W, H);
    assert!(
        tree.enabled,
        "the dialog itself, which holds the eyedropper"
    );
    for part in tree.children.iter().flat_map(Node::walk) {
        assert_eq!(
            part.enabled,
            part.id == ColorPart::Eyedropper,
            "{:?}",
            part.id
        );
    }
    assert_eq!(
        dialog.invoke(&ColorPart::Ok, Action::Press, W, H),
        Err(Refusal::Disabled)
    );
    assert_eq!(
        dialog.invoke(&ColorPart::Eyedropper, Action::Press, W, H),
        Ok(Some(ColorPickerEvent::EyedropperDeactivated))
    );
    assert!(node(&dialog, ColorPart::Ok).enabled);
}
