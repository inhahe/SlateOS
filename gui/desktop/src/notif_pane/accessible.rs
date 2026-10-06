//! The notification pane as tools see it -- held among the shell's own parts
//! by `crate::accessible`, which presses them as the user would.
//!
//! The header's links; the quick settings -- each switch named by what it
//! switches and ticked while it is on, out of use and saying why while it
//! cannot be used, and the volume and the brightness, each a slider from 0 to
//! 100 -- and below them the notifications, newest first: each card named by
//! its title and said to be from its program, when, whether it is unread,
//! and what it says, holding the cross that dismisses it. While the
//! programs' switches show in the notifications' place, each program's card
//! instead, named by the program and said to be how important it is and how
//! it is shown, holding its switch; and below them the way to Settings.
//!
//! Every box is where the pane draws the part and takes a press on it, from
//! the one layout the renderer and the hit test share, on the screen -- so a
//! press at the middle of a part's box ([`NotificationPane::press_point`]) is
//! a press on the part. A card scrolled out of the list has its box above or
//! below the list, where it would be drawn; [`NotificationPane::reveal`]
//! scrolls it in, as the user would before pressing it.

use guitk::frame::Rect;
use guitk::widget::CheckState;
use guitk::widget::automation::{Node, Role, Value};
use notifsettings::Importance;

use super::{
    APP_CARD_HEIGHT, FULL_SETTINGS_LABEL, HeaderLink, NOTIF_CARD_HEIGHT, NotificationPane,
    PANE_PADDING, PANE_WIDTH, QUICK_SETTINGS_HEIGHT, QuickSetting,
};

/// A part of the notification pane, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanePart {
    /// The pane.
    Pane,
    /// "Settings", in the header: each program's switch in the
    /// notifications' place.
    Settings,
    /// "Clear all", in the header: every notification gone.
    ClearAll,
    /// "Back", in the header: the notifications again.
    Back,
    /// The quick settings.
    QuickSettings,
    /// A quick setting's switch.
    Switch(QuickSetting),
    /// The volume, 0 to 100.
    Volume,
    /// The screen's brightness, 0 to 100.
    Brightness,
    /// The notifications, newest first.
    Notifications,
    /// The card of the notification with this id: pressed, it is read, and
    /// what it names is started.
    Card(u64),
    /// The cross that dismisses the notification with this id.
    Dismiss(u64),
    /// The programs that have sent notifications, each with its switch.
    Programs,
    /// The card of the `n`th program in the list.
    Program(usize),
    /// The `n`th program's switch: whether it may show notifications.
    ProgramSwitch(usize),
    /// "Open full notification settings…", below the programs.
    FullSettings,
}

impl HeaderLink {
    /// Its part, as tools name it.
    const fn part(self) -> PanePart {
        match self {
            Self::Settings => PanePart::Settings,
            Self::ClearAll => PanePart::ClearAll,
            Self::Back => PanePart::Back,
        }
    }
}

/// A box in the pane's own coordinates, on a screen where the pane's left
/// edge is at `left`: the pane's `y` is the screen's.
fn on_screen(rect: Rect, left: f32) -> Rect {
    Rect::new(rect.x + left, rect.y, rect.w, rect.h)
}

/// A box given as `(x, y, width, height)`.
fn rect_of((x, y, w, h): (f32, f32, f32, f32)) -> Rect {
    Rect::new(x, y, w, h)
}

/// A checkbox's tick for a switch that is `on`.
const fn ticked(on: bool) -> Value {
    Value::Check(if on {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    })
}

impl NotificationPane {
    /// Where the pane's left edge is drawn now, on a screen `screen_width`
    /// wide: off the right edge while it slides.
    fn left_edge(&self, screen_width: f32) -> f32 {
        screen_width - PANE_WIDTH * self.shown()
    }

    /// Where the list scrolled as it is puts its own `y`, in the pane.
    fn list_origin(&self) -> f32 {
        Self::list_start_y() - self.scroll_offset
    }

    /// The box of `part` in the pane's own coordinates, where it is drawn
    /// with the list scrolled as it is -- `None` for a part the pane does not
    /// show now: a link of the other view, a notification or a program that
    /// is not there, a list that is not the one showing.
    fn part_box(&self, part: PanePart, height: f32) -> Option<Rect> {
        let list = Rect::new(
            0.0,
            Self::list_start_y(),
            PANE_WIDTH,
            (height - Self::list_start_y()).max(0.0),
        );
        match part {
            PanePart::Pane => Some(Rect::new(0.0, 0.0, PANE_WIDTH, height)),
            PanePart::Settings | PanePart::ClearAll | PanePart::Back => self
                .header_links()
                .iter()
                .find(|link| link.part() == part)
                .map(|&link| Self::header_link_rect(link)),
            PanePart::QuickSettings => Some(Rect::new(
                0.0,
                Self::qs_start_y(),
                PANE_WIDTH,
                QUICK_SETTINGS_HEIGHT,
            )),
            PanePart::Switch(setting) => {
                let pill = Self::qs_switch_rect(setting.index());
                Some(Rect::new(
                    pill.x,
                    pill.y + Self::qs_start_y(),
                    pill.w,
                    pill.h,
                ))
            }
            PanePart::Volume => Some(Self::qs_slider_placement(0, Self::qs_start_y()).hit()),
            PanePart::Brightness => Some(Self::qs_slider_placement(1, Self::qs_start_y()).hit()),
            PanePart::Notifications => (!self.show_settings).then_some(list),
            PanePart::Programs => self.show_settings.then_some(list),
            PanePart::Card(id) => {
                let top = self.card_top_of(id)?;
                Some(Rect::new(
                    PANE_PADDING,
                    self.list_origin() + top,
                    Self::card_width(),
                    NOTIF_CARD_HEIGHT,
                ))
            }
            PanePart::Dismiss(id) => {
                let top = self.card_top_of(id)?;
                Some(Self::dismiss_rect(self.list_origin() + top))
            }
            PanePart::Program(index) => {
                self.programs_index(index)?;
                Some(Rect::new(
                    PANE_PADDING,
                    self.list_origin() + Self::app_card_top(index),
                    Self::card_width(),
                    APP_CARD_HEIGHT,
                ))
            }
            PanePart::ProgramSwitch(index) => {
                self.programs_index(index)?;
                Some(rect_of(Self::app_toggle_rect(
                    self.list_origin() + Self::app_card_top(index),
                )))
            }
            PanePart::FullSettings => {
                if !self.show_settings {
                    return None;
                }
                let row = self.full_settings_rect();
                Some(Rect::new(row.x, self.list_origin() + row.y, row.w, row.h))
            }
        }
    }

    /// Where the card of notification `id` begins in the list, before
    /// scrolling -- `None` where there is no such notification, or the
    /// programs show in the notifications' place.
    fn card_top_of(&self, id: u64) -> Option<f32> {
        if self.show_settings {
            return None;
        }
        let index = self.notifications.iter().position(|n| n.id == id)?;
        self.card_tops().get(index).copied()
    }

    /// `index`, where the programs show and there is a program there.
    fn programs_index(&self, index: usize) -> Option<usize> {
        (self.show_settings && index < self.app_settings.len()).then_some(index)
    }

    /// Where on the screen a press lands on `part` -- the middle of its box
    /// -- on a screen `screen_width` wide where the pane is `height` tall:
    /// `None` for a part the pane does not show now, or one of the list's
    /// scrolled out of it or below the pane's foot ([`reveal`] it first).
    ///
    /// [`reveal`]: Self::reveal
    #[must_use]
    pub fn press_point(
        &self,
        part: PanePart,
        screen_width: f32,
        height: f32,
    ) -> Option<(f32, f32)> {
        if !self.state.is_visible() {
            return None;
        }
        let rect = self.part_box(part, height)?;
        let (x, y) = rect.centre();
        let in_list = matches!(
            part,
            PanePart::Card(_)
                | PanePart::Dismiss(_)
                | PanePart::Program(_)
                | PanePart::ProgramSwitch(_)
                | PanePart::FullSettings
        );
        if in_list && !(y >= Self::list_start_y() && y < height) {
            return None;
        }
        Some((x + self.left_edge(screen_width), y))
    }

    /// Scroll the list, in a pane `height` tall, so `part` -- a card, its
    /// cross, a program's card or switch, or the way to Settings -- is in it
    /// whole, as the user would before pressing it; nothing for any other
    /// part. As little as that takes: a part already in the list stays
    /// where it is.
    pub fn reveal(&mut self, part: PanePart, height: f32) {
        self.note_screen_height(height);
        let span = match part {
            PanePart::Card(id) | PanePart::Dismiss(id) => self
                .card_top_of(id)
                .map(|top| (top, top + NOTIF_CARD_HEIGHT)),
            PanePart::Program(index) | PanePart::ProgramSwitch(index) => {
                self.programs_index(index).map(|index| {
                    let top = Self::app_card_top(index);
                    (top, top + APP_CARD_HEIGHT)
                })
            }
            PanePart::FullSettings if self.show_settings => {
                let row = self.full_settings_rect();
                Some((row.y, row.bottom()))
            }
            _ => None,
        };
        let Some((top, bottom)) = span else {
            return;
        };
        let shown = self.list_height();
        if top < self.scroll_offset {
            self.scroll_offset = top;
        } else if bottom > self.scroll_offset + shown {
            // Its foot at the list's foot -- or, for a part taller than the
            // list, its top at the list's top, so what is pressed is shown.
            self.scroll_offset = (bottom - shown).min(top);
        }
        self.clamp_scroll();
    }

    /// The pane as tools see it, on a screen `screen_width` wide where the
    /// pane is `height` tall -- the height its caller draws it at.
    #[must_use]
    pub fn automation(&self, screen_width: f32, height: f32) -> Node<PanePart> {
        let left = self.left_edge(screen_width);
        let place = |part: PanePart| {
            self.part_box(part, height)
                .map_or(Rect::new(left, 0.0, 0.0, 0.0), |rect| on_screen(rect, left))
        };
        let title = if self.show_settings {
            "Notification Settings"
        } else {
            "Notifications"
        };
        let mut pane = Node::new(PanePart::Pane, Role::Dialog, title, place(PanePart::Pane));
        for &link in self.header_links() {
            pane.children.push(Node::new(
                link.part(),
                Role::Button,
                link.label(),
                place(link.part()),
            ));
        }
        pane.children.push(self.quick_settings_node(&place));
        pane.children.push(if self.show_settings {
            self.programs_node(&place)
        } else {
            self.notifications_node(&place)
        });
        pane
    }

    /// The quick settings: the switches, then the two levels.
    fn quick_settings_node(&self, place: &impl Fn(PanePart) -> Rect) -> Node<PanePart> {
        let mut block = Node::new(
            PanePart::QuickSettings,
            Role::Group,
            "Quick Settings",
            place(PanePart::QuickSettings),
        );
        for &setting in QuickSetting::all() {
            let part = PanePart::Switch(setting);
            let mut switch = Node::new(part, Role::CheckBox, setting.label(), place(part));
            switch.value = Some(ticked(self.quick_settings.get(setting)));
            let why = self.unavailable(setting);
            switch.enabled = why.is_none();
            switch.description = why.map(str::to_owned);
            block.children.push(switch);
        }
        for (slot, part, name) in [
            (0, PanePart::Volume, "Volume"),
            (1, PanePart::Brightness, "Brightness"),
        ] {
            let mut level = Node::new(part, Role::Slider, name, place(part));
            let fixed = self.fixed.get(slot).copied().flatten();
            // No level where the pane shows none: the card could not be
            // read, so the number it holds says nothing.
            level.value = fixed.is_none_or(|f| f.level_shown).then(|| Value::Range {
                value: f64::from(self.qs_level(slot)),
                min: 0.0,
                max: 100.0,
            });
            level.enabled = fixed.is_none();
            level.description = fixed.map(|f| f.why.to_owned());
            level.focusable = true;
            block.children.push(level);
        }
        block
    }

    /// The notifications, newest first, each with its cross.
    fn notifications_node(&self, place: &impl Fn(PanePart) -> Rect) -> Node<PanePart> {
        let mut list = Node::new(
            PanePart::Notifications,
            Role::List,
            "Notifications",
            place(PanePart::Notifications),
        );
        for notif in &self.notifications {
            let part = PanePart::Card(notif.id);
            let name = if notif.title.is_empty() {
                notif.app_name.clone()
            } else {
                notif.title.clone()
            };
            let mut card = Node::new(part, Role::ListItem, name, place(part));
            let mut said = vec![
                notif.app_name.clone(),
                self.format_relative_time(notif.timestamp),
            ];
            if !notif.read {
                said.push("unread".to_owned());
            }
            let mut description = said.join(", ");
            if !notif.body.is_empty() {
                description.push_str(". ");
                description.push_str(&notif.body);
            }
            card.description = Some(description);
            let cross = PanePart::Dismiss(notif.id);
            card.children
                .push(Node::new(cross, Role::Button, "Dismiss", place(cross)));
            list.children.push(card);
        }
        list
    }

    /// The programs, each with its switch, and the way to Settings.
    fn programs_node(&self, place: &impl Fn(PanePart) -> Rect) -> Node<PanePart> {
        let mut list = Node::new(
            PanePart::Programs,
            Role::List,
            "Per-App Settings",
            place(PanePart::Programs),
        );
        for (index, app) in self.app_settings.iter().enumerate() {
            let part = PanePart::Program(index);
            let mut card = Node::new(part, Role::ListItem, app.app_name.clone(), place(part));
            let on = app.importance != Importance::Silent;
            let mut said = vec![app.importance.label()];
            if app.sound {
                said.push("sound");
            }
            if app.banner {
                said.push("banner");
            }
            card.description = Some(said.join(", "));
            let switch = PanePart::ProgramSwitch(index);
            let mut pill = Node::new(
                switch,
                Role::CheckBox,
                format!("Notifications from {}", app.app_name),
                place(switch),
            );
            pill.value = Some(ticked(on));
            card.children.push(pill);
            list.children.push(card);
        }
        list.children.push(Node::new(
            PanePart::FullSettings,
            Role::Button,
            FULL_SETTINGS_LABEL,
            place(PanePart::FullSettings),
        ));
        list
    }

    /// Put the screen's brightness at `level`, clamped to `0..=100`, as its
    /// slider does -- nothing while it cannot be changed here. Its caller
    /// puts it on the screen (`DesktopShell::write_brightness`).
    pub const fn set_brightness(&mut self, level: u8) {
        if self.fixed[1].is_none() {
            self.quick_settings.brightness = if level > 100 { 100 } else { level };
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
