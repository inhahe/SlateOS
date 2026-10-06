//! The colour dialog as tools see it -- a screen reader, a script
//! ([`Accessible`]): a dialog holding the saturation-and-brightness square
//! (its two axes as sliders), the hue and opacity bars, the colour as it is
//! now, the hex field, the eyedropper, the RGB and HSV tabs and the three
//! sliders of the one showing, the presets, the recent colours, and OK and
//! Cancel. Each is a part a tool can find by what it is called and act on as
//! its user would, answering the event a click on the same part answers.
//!
//! The boxes are the ones the dialog is drawn and clicked in, from the same
//! layout. While the eyedropper is picking from the screen, every part but
//! the eyedropper is out of use, as a click anywhere would end the pick.

use crate::color::Color;
use crate::frame::Rect;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

use super::{
    ALPHA_BAR_HEIGHT, CANCEL_LABEL, ColorPickerDialog, ColorPickerEvent, DIALOG_TITLE,
    DialogLayout, EYEDROPPER_SIZE, HUE_BAR_WIDTH, OK_LABEL, PRESET_COLORS, PREVIEW_SIZE,
    PickerMode, SECTION_LABEL_HEIGHT, SLIDER_TAB_SIZE, SWATCH_SIZE, SliderTab, color_to_hex_string,
    color_to_hex_string_alpha, hex_field_rect, hsv_to_rgb, parse_hex_color, rgb_to_hsv, scaled,
};

/// A part of the colour dialog, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorPart {
    /// The dialog itself.
    Dialog,
    /// The saturation-and-brightness square.
    Square,
    /// The square's across: saturation, 0 to 100.
    Saturation,
    /// The square's down: brightness, 0 to 100.
    Brightness,
    /// The hue bar, 0 to 360.
    Hue,
    /// The opacity bar, 0 to 255.
    Opacity,
    /// The colour as it is now, beside the one the dialog opened with.
    Preview,
    /// The hex field.
    Hex,
    /// The eyedropper, which picks a colour from the screen.
    Eyedropper,
    /// The tab of sliders, RGB or HSV.
    Tab(SliderTab),
    /// Slider 0, 1 or 2 of the tab showing.
    Row(u8),
    /// The preset colours.
    Presets,
    /// Preset colour `i`.
    Preset(usize),
    /// The colours chosen lately.
    Recent,
    /// Recent colour `i`.
    RecentColor(usize),
    /// OK: keep the colour.
    Ok,
    /// Cancel: back to the colour the dialog opened with.
    Cancel,
}

/// A colour's name to a tool: its hex code -- with its opacity where it is
/// not opaque.
fn color_name(color: Color) -> String {
    if color.a == u8::MAX {
        format!("#{}", color_to_hex_string(color))
    } else {
        format!("#{}", color_to_hex_string_alpha(color))
    }
}

/// A slider node: `part`, called `name`, at `bounds`, at `value` of
/// `min` to `max`.
fn slider(
    part: ColorPart,
    name: &str,
    bounds: Rect,
    value: f64,
    min: f64,
    max: f64,
) -> Node<ColorPart> {
    let mut node = Node::new(part, Role::Slider, name, bounds);
    node.value = Some(Value::Range { value, min, max });
    node
}

/// A swatch's box, its top-left corner `origin`.
fn swatch(origin: (f32, f32)) -> Rect {
    Rect::new(origin.0, origin.1, scaled(SWATCH_SIZE), scaled(SWATCH_SIZE))
}

/// What slider `row` of the tab `tab` is called, and how far it runs from
/// nought.
fn row_meaning(tab: SliderTab, row: u8) -> (&'static str, f64) {
    match (tab, row) {
        (SliderTab::Rgb, 0) => ("Red", 255.0),
        (SliderTab::Rgb, 1) => ("Green", 255.0),
        (SliderTab::Rgb, _) => ("Blue", 255.0),
        (SliderTab::Hsv, 0) => ("Hue", 360.0),
        (SliderTab::Hsv, 1) => ("Saturation", 100.0),
        (SliderTab::Hsv, _) => ("Value", 100.0),
    }
}

/// `value` as a byte: within a byte's range, rounded.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a byte's range and rounded first"
)]
fn byte(value: f64) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

/// `value` as the picker keeps hue, saturation and brightness.
#[allow(
    clippy::cast_possible_truncation,
    reason = "each caller clamps to 0..=360 first, well within an f32"
)]
fn narrow(value: f64) -> f32 {
    value as f32
}

/// What a change of the colour answers: the colour now, as a drag does.
#[allow(
    clippy::unnecessary_wraps,
    reason = "an action's answer, given where the others are"
)]
fn changed(dialog: &ColorPickerDialog) -> Result<Option<ColorPickerEvent>, Refusal> {
    Ok(Some(ColorPickerEvent::Changed(
        dialog.picker.current_color(),
    )))
}

impl ColorPickerDialog {
    /// The value tools see on slider `row` of the tab showing.
    fn row_value(&self, row: u8) -> f64 {
        let hsv = self.picker.hsv;
        match self.slider_tab {
            SliderTab::Rgb => {
                let (r, g, b) = hsv_to_rgb(hsv);
                f64::from(match row {
                    0 => r,
                    1 => g,
                    _ => b,
                })
            }
            SliderTab::Hsv => match row {
                0 => f64::from(hsv.h),
                1 => f64::from(hsv.s) * 100.0,
                _ => f64::from(hsv.v) * 100.0,
            },
        }
    }

    /// Set slider `row` of the tab showing to `value`, within its range.
    fn set_row(&mut self, row: u8, value: f64) {
        let (_, max) = row_meaning(self.slider_tab, row);
        let value = value.clamp(0.0, max);
        match self.slider_tab {
            SliderTab::Rgb => {
                let (mut r, mut g, mut b) = hsv_to_rgb(self.picker.hsv);
                match row {
                    0 => r = byte(value),
                    1 => g = byte(value),
                    _ => b = byte(value),
                }
                self.picker.hsv = rgb_to_hsv(r, g, b);
            }
            SliderTab::Hsv => match row {
                0 => self.picker.hsv.h = narrow(value),
                1 => self.picker.hsv.s = narrow(value / 100.0),
                _ => self.picker.hsv.v = narrow(value / 100.0),
            },
        }
        self.picker.sync_hex_from_hsv();
    }
}

impl Accessible for ColorPickerDialog {
    type Part = ColorPart;
    type Event = ColorPickerEvent;

    fn automation(&self, width: f32, height: f32) -> Node<ColorPart> {
        let layout = self.layout(width, height);
        let hsv = self.picker.hsv;
        let color = self.picker.current_color();

        let square_box = Rect::new(layout.sv_x, layout.sv_y, layout.sv_size, layout.sv_size);
        let mut square = Node::new(
            ColorPart::Square,
            Role::Group,
            "Saturation and brightness",
            square_box,
        );
        square.children = vec![
            slider(
                ColorPart::Saturation,
                "Saturation",
                square_box,
                f64::from(hsv.s) * 100.0,
                0.0,
                100.0,
            ),
            slider(
                ColorPart::Brightness,
                "Brightness",
                square_box,
                f64::from(hsv.v) * 100.0,
                0.0,
                100.0,
            ),
        ];

        let mut children = vec![
            square,
            slider(
                ColorPart::Hue,
                "Hue",
                Rect::new(
                    layout.hue_x,
                    layout.sv_y,
                    scaled(HUE_BAR_WIDTH),
                    layout.sv_size,
                ),
                f64::from(hsv.h),
                0.0,
                360.0,
            ),
            slider(
                ColorPart::Opacity,
                "Opacity",
                Rect::new(
                    layout.sv_x,
                    layout.alpha_y,
                    layout.sv_size,
                    scaled(ALPHA_BAR_HEIGHT),
                ),
                f64::from(self.picker.alpha),
                0.0,
                255.0,
            ),
            self.preview_node(&layout, color),
            self.hex_node(&layout),
            self.eyedropper_node(&layout),
        ];
        children.extend([SliderTab::Rgb, SliderTab::Hsv].map(|tab| {
            let (w, h) = SLIDER_TAB_SIZE;
            let mut node = Node::new(
                ColorPart::Tab(tab),
                Role::RadioButton,
                match tab {
                    SliderTab::Rgb => "RGB",
                    SliderTab::Hsv => "HSV",
                },
                Rect::new(DialogLayout::slider_tab_x(tab), layout.slider_y, w, h),
            );
            node.value = Some(Value::Chosen(tab == self.slider_tab));
            node
        }));
        children.extend([0_u8, 1, 2].map(|row| {
            let (name, max) = row_meaning(self.slider_tab, row);
            slider(
                ColorPart::Row(row),
                name,
                layout.row_hold(row),
                self.row_value(row),
                0.0,
                max,
            )
        }));
        children.push(Self::presets_node(&layout, width, color));
        if !self.picker.recent_colors.is_empty() {
            children.push(self.recent_node(&layout, width));
        }
        children.push(Node::new(
            ColorPart::Ok,
            Role::Button,
            OK_LABEL,
            layout.ok_button(),
        ));
        children.push(Node::new(
            ColorPart::Cancel,
            Role::Button,
            CANCEL_LABEL,
            layout.cancel_button(),
        ));

        // While picking from the screen, the eyedropper alone is in use.
        if self.picker.mode == PickerMode::Eyedropper {
            for child in &mut children {
                disable_all_but_the_eyedropper(child);
            }
        }

        let mut root = Node::new(
            ColorPart::Dialog,
            Role::Dialog,
            DIALOG_TITLE,
            Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0)),
        );
        root.children = children;
        root
    }

    fn invoke(
        &mut self,
        part: &ColorPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<ColorPickerEvent>, Refusal> {
        let asked = action.name();
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: asked,
        };
        let value = match &action {
            Action::SetValue(value) if value.is_finite() => Some(*value),
            Action::SetValue(_) => return Err(Refusal::NotANumber),
            _ => None,
        };
        if self.picker.mode == PickerMode::Eyedropper {
            return match (part, &action) {
                (ColorPart::Eyedropper, Action::Press) => {
                    self.picker.cancel_eyedropper();
                    Ok(Some(ColorPickerEvent::EyedropperDeactivated))
                }
                (ColorPart::Eyedropper, _) => Err(not_for(Role::Button)),
                _ => Err(Refusal::Disabled),
            };
        }
        match (part, action) {
            (ColorPart::Saturation, _) => {
                let to = value.ok_or_else(|| not_for(Role::Slider))?;
                self.picker.hsv.s = narrow(to.clamp(0.0, 100.0) / 100.0);
                self.picker.sync_hex_from_hsv();
                changed(self)
            }
            (ColorPart::Brightness, _) => {
                let to = value.ok_or_else(|| not_for(Role::Slider))?;
                self.picker.hsv.v = narrow(to.clamp(0.0, 100.0) / 100.0);
                self.picker.sync_hex_from_hsv();
                changed(self)
            }
            (ColorPart::Hue, _) => {
                let to = value.ok_or_else(|| not_for(Role::Slider))?;
                self.picker.hsv.h = narrow(to.clamp(0.0, 360.0));
                self.picker.sync_hex_from_hsv();
                changed(self)
            }
            (ColorPart::Opacity, _) => {
                let to = value.ok_or_else(|| not_for(Role::Slider))?;
                self.picker.alpha = byte(to);
                changed(self)
            }
            (ColorPart::Row(row), _) => {
                if *row > 2 {
                    return Err(Refusal::NoSuchWidget);
                }
                let to = value.ok_or_else(|| not_for(Role::Slider))?;
                self.set_row(*row, to);
                changed(self)
            }
            (ColorPart::Hex, Action::SetText(text)) => {
                // As a code typed and kept: any form the field takes, the
                // opacity too where it is given.
                let code = text.trim().trim_start_matches('#');
                let color = parse_hex_color(code).ok_or(Refusal::NotANumber)?;
                self.picker.hsv = rgb_to_hsv(color.r, color.g, color.b);
                if code.len() == 8 {
                    self.picker.alpha = color.a;
                }
                self.picker.sync_hex_from_hsv();
                changed(self)
            }
            (ColorPart::Hex, Action::Focus) => {
                self.picker.hex_focused = true;
                Ok(None)
            }
            (ColorPart::Hex, _) => Err(not_for(Role::TextField)),
            (ColorPart::Eyedropper, Action::Press) => {
                self.picker.activate_eyedropper();
                Ok(Some(ColorPickerEvent::EyedropperActivated))
            }
            (ColorPart::Tab(tab), Action::Choose | Action::Press) => {
                self.slider_tab = *tab;
                Ok(None)
            }
            (ColorPart::Tab(_), _) => Err(not_for(Role::RadioButton)),
            (ColorPart::Preset(i), Action::Press | Action::Choose) => {
                let color = PRESET_COLORS
                    .get(*i)
                    .copied()
                    .ok_or(Refusal::NoSuchWidget)?;
                self.picker.set_color(color);
                Ok(Some(ColorPickerEvent::Changed(color)))
            }
            (ColorPart::Preset(_), _) => Err(not_for(Role::GridCell)),
            (ColorPart::RecentColor(i), Action::Press | Action::Choose) => {
                let color = self
                    .picker
                    .recent_colors
                    .get(*i)
                    .copied()
                    .ok_or(Refusal::NoSuchWidget)?;
                self.picker.set_color(color);
                Ok(Some(ColorPickerEvent::Changed(color)))
            }
            (ColorPart::RecentColor(_), _) => Err(not_for(Role::ListItem)),
            (ColorPart::Ok, Action::Press) => {
                // A code being typed is kept first, as a click on OK keeps it.
                if self.picker.hex_focused {
                    self.picker.commit_hex();
                }
                Ok(Some(self.confirm()))
            }
            (ColorPart::Cancel, Action::Press) => Ok(Some(self.cancel())),
            (ColorPart::Eyedropper | ColorPart::Ok | ColorPart::Cancel, _) => {
                Err(not_for(Role::Button))
            }
            (ColorPart::Dialog, _) => Err(not_for(Role::Dialog)),
            (ColorPart::Square, _) => Err(not_for(Role::Group)),
            (ColorPart::Preview, _) => Err(not_for(Role::Image)),
            (ColorPart::Presets, _) => Err(not_for(Role::Grid)),
            (ColorPart::Recent, _) => Err(not_for(Role::List)),
        }
    }
}

impl ColorPickerDialog {
    /// The colour as it is now, and the one the dialog opened with.
    fn preview_node(&self, layout: &DialogLayout, color: Color) -> Node<ColorPart> {
        let mut preview = Node::new(
            ColorPart::Preview,
            Role::Image,
            "Color",
            Rect::new(
                layout.right_x,
                layout.sv_y + scaled(12.0),
                scaled(PREVIEW_SIZE),
                scaled(PREVIEW_SIZE),
            ),
        );
        preview.value = Some(Value::Text(color_name(color)));
        preview.description = Some(format!("was {}", color_name(self.picker.original)));
        preview
    }

    /// The hex field: what is typed in it, and whether it has the keyboard.
    fn hex_node(&self, layout: &DialogLayout) -> Node<ColorPart> {
        let mut hex = Node::new(
            ColorPart::Hex,
            Role::TextField,
            "Hex",
            hex_field_rect(layout.right_x, layout.hex_y, layout.right_width),
        );
        hex.value = Some(Value::Text(self.picker.hex_input.clone()));
        hex.focused = self.picker.hex_focused;
        hex.focusable = true;
        hex
    }

    /// The eyedropper, saying whether it is picking.
    fn eyedropper_node(&self, layout: &DialogLayout) -> Node<ColorPart> {
        let (w, h) = EYEDROPPER_SIZE;
        let mut eyedropper = Node::new(
            ColorPart::Eyedropper,
            Role::Button,
            "Eyedropper",
            Rect::new(layout.right_x, layout.eye_y, w, h),
        );
        eyedropper.description = Some(
            if self.picker.mode == PickerMode::Eyedropper {
                "Picking a color from the screen; press again to stop"
            } else {
                "Pick a color from the screen"
            }
            .to_owned(),
        );
        eyedropper
    }

    /// The presets, each a cell named by its code, the one that is the
    /// colour now chosen.
    fn presets_node(layout: &DialogLayout, width: f32, color: Color) -> Node<ColorPart> {
        let mut presets = Node::new(
            ColorPart::Presets,
            Role::Grid,
            "Presets",
            Rect::new(
                layout.sv_x,
                layout.preset_y,
                (width - layout.sv_x * 2.0).max(0.0),
                (layout.recent_y - layout.preset_y).max(0.0),
            ),
        );
        for (i, preset) in PRESET_COLORS.iter().enumerate() {
            let mut cell = Node::new(
                ColorPart::Preset(i),
                Role::GridCell,
                color_name(*preset),
                swatch(layout.preset_swatch(i)),
            );
            cell.value = Some(Value::Chosen(
                (preset.r, preset.g, preset.b) == (color.r, color.g, color.b),
            ));
            presets.children.push(cell);
        }
        presets
    }

    /// The colours chosen lately, newest first.
    fn recent_node(&self, layout: &DialogLayout, width: f32) -> Node<ColorPart> {
        let mut recent = Node::new(
            ColorPart::Recent,
            Role::List,
            "Recent colors",
            Rect::new(
                layout.sv_x,
                layout.recent_y,
                (width - layout.sv_x * 2.0).max(0.0),
                scaled(SECTION_LABEL_HEIGHT) + scaled(SWATCH_SIZE),
            ),
        );
        for (i, used) in self.picker.recent_colors.iter().enumerate() {
            recent.children.push(Node::new(
                ColorPart::RecentColor(i),
                Role::ListItem,
                color_name(*used),
                swatch(layout.recent_swatch(i)),
            ));
        }
        recent
    }
}

/// `node` and all under it out of use, unless it is the eyedropper.
fn disable_all_but_the_eyedropper(node: &mut Node<ColorPart>) {
    if node.id != ColorPart::Eyedropper {
        node.enabled = false;
    }
    for child in &mut node.children {
        disable_all_but_the_eyedropper(child);
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
