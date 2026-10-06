//! The desktop shell as tools see it -- a screen reader, a script
//! ([`Accessible`]): the desktop's icons, the taskbar and what is open over
//! it.
//!
//! The icons are a list, each named by its label and said to be what it is
//! -- a folder, a program, the recycle bin -- the chosen ones marked; one
//! chosen is clicked, one pressed is double-clicked, so it opens.
//!
//! The taskbar holds the start button, a tile for each pinned program and
//! each window -- named by the program and by the window's title, the window
//! with the keyboard marked -- the tray's icons, each named by what its
//! program says of it, the chevron to the hidden ones, the speaker, the
//! bell, the clock (named by the time, described by the whole date) and
//! "Show desktop". While the start menu is open, it holds the search field,
//! the list of programs -- every row, a row scrolled out of the list with
//! its box above or below it -- the places and the power button and its
//! caret; while the power choices are open, they; while the volume flyout
//! is open, its level, its mute switch and "Audio settings…"; while the
//! notification pane is open, its parts (`notif_pane::accessible`).
//!
//! # As the user would
//!
//! A part pressed is clicked: the shell's own press and release at the
//! middle of the box it is drawn in, so the click does exactly what the
//! user's would -- a program started, a window raised, a menu opened -- and
//! answers the [`ShellAction`] the host carries out. A part the click would
//! not reach refuses: one covered by something drawn over it -- a menu, the
//! Run box, the notification pane's scrim -- and one a click would be spent
//! closing something else for, as a press away from an open menu or flyout
//! is. A row of the start menu or a notification scrolled out of sight is
//! scrolled into it first, as the user would; a notification's cross,
//! shown while the pointer is on its card, is pointed at before it is
//! pressed. The search's text, the volume's level, its mute and the
//! brightness are set as typing and the controls set them.

use guitk::widget::CheckState;
use guitk::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

use crate::notif_pane::PanePart;
use crate::{
    DesktopShell, Hit, MouseButton, MouseEvent, MouseEventKind, Rect, ShellAction, StartRow,
    StartShortcut, TaskbarSlot, click, icons, power, volume_flyout,
};

/// A part of the shell, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellPart {
    /// The whole screen the shell draws on.
    Desktop,
    /// The icons on the desktop.
    Icons,
    /// A desktop icon: chosen as a click chooses it, opened as a double
    /// click opens it.
    Icon(icons::IconId),
    /// The taskbar.
    Taskbar,
    /// The start menu, while it is open.
    StartMenu,
    /// The start menu's search field.
    StartSearch,
    /// The start menu's list of programs.
    StartList,
    /// The power choices, while they are open.
    PowerMenu,
    /// The volume flyout, while it is open.
    Volume,
    /// The flyout's level, 0 to 100.
    VolumeLevel,
    /// The flyout's mute switch.
    VolumeMute,
    /// The flyout's "Audio settings…".
    AudioSettings,
    /// A control the shell's hit test names, pressed as it is clicked.
    Control(Hit),
    /// A part of the notification pane, while it is open.
    Pane(PanePart),
}

/// An action's answer as a tool hears it: nothing for the host, or what the
/// host is to do.
fn answered(action: ShellAction) -> Option<ShellAction> {
    match action {
        ShellAction::Consumed | ShellAction::Pass => None,
        other => Some(other),
    }
}

/// A button node.
fn button(hit: Hit, name: impl Into<String>, bounds: Rect) -> Node<ShellPart> {
    Node::new(ShellPart::Control(hit), Role::Button, name, bounds)
}

/// A level from 0 to 100 a tool asked for, as a slider of whole steps
/// takes it: within its bounds, on a step. `value` is a number.
fn level_of(value: f64) -> u8 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=100 and rounded first"
    )]
    let level = value.clamp(0.0, 100.0).round() as u8;
    level
}

impl DesktopShell {
    /// Whether something open over everything takes every press wherever it
    /// lands, before any part of the shell under it could: a menu, the
    /// character picker, the Run box or a chooser, the overview, the tiling
    /// overlay, the list a shut down waits on, or a drag or a press under
    /// way -- as `handle_mouse_with` and `handle_press_with` hand a press.
    fn takes_every_press(&self) -> bool {
        self.char_picker.is_some()
            || self.field_menu.is_some()
            || self.wallpaper_move.is_some()
            || self.desktop_menu.is_visible()
            || self.widget_drag.is_some()
            || self.notification_menu.is_some()
            || self.taskbar_menu.is_some()
            || self.pin_menu.is_some()
            || self.tray_overflow_menu.is_some()
            || self.start_drag.is_some()
            || self.window_press.is_some()
            || self.pin_drag.is_some()
            || self.tray_drag.is_some()
            || self.chooser.is_some()
            || self.run_dialog.is_visible()
            || self.overview.visible
            || self.snap.is_overlay_visible()
            || self.ending_listing()
    }

    /// Whether a press at `(x, y)` on `hit` -- what the shell's hit test
    /// says is there -- would reach it: nothing drawn over it takes the
    /// press, and it is not spent closing something open elsewhere.
    fn reaches(&self, x: f32, y: f32, hit: Hit) -> bool {
        let under_pane =
            self.notifications.pane_state().is_visible() && !self.taskbar_rect().contains(x, y);
        !self.takes_every_press()
            && !under_pane
            && self.hit_test(x, y) == hit
            && self.closed_by_press(hit).is_none()
    }

    /// Where the control `hit` is drawn and clicked, if it is on screen.
    fn part_rect(&self, hit: Hit) -> Option<Rect> {
        match hit {
            Hit::StartButton => Some(self.start_button_rect()),
            Hit::TaskbarButton(_) | Hit::TaskbarPinned(_) => self
                .taskbar_slots()
                .into_iter()
                .zip(self.taskbar_layout())
                .find(|(slot, _)| match (*slot, hit) {
                    (TaskbarSlot::Window(id), Hit::TaskbarButton(wanted)) => id == wanted,
                    (TaskbarSlot::Pinned(pin), Hit::TaskbarPinned(wanted)) => pin == wanted,
                    _ => false,
                })
                .map(|(_, rect)| rect),
            Hit::TrayIcon(index) => self.tray_icon_rects().get(index).copied(),
            Hit::TrayOverflow => self.tray_overflow_rect(),
            Hit::VolumeIcon => Some(self.volume_icon_rect()),
            Hit::NotificationBell => Some(self.bell_rect()),
            Hit::Clock => Some(self.clock_rect()),
            Hit::ShowDesktop => Some(self.show_desktop_rect()),
            Hit::StartMenuEntry(index) if self.start_menu_open => {
                let row = index.checked_sub(self.start_menu_scroll)?;
                (row < self.start_menu_visible_rows()).then(|| self.start_menu_row_rect(row))
            }
            Hit::StartMenuShortcut(which) if self.start_menu_open => {
                Some(self.start_shortcut_rect(which))
            }
            Hit::PowerButton if self.start_menu_open => Some(self.power_button_rect()),
            Hit::PowerCaret if self.start_menu_open => Some(self.power_caret_rect()),
            Hit::PowerMenuEntry(row) if self.power_menu_open => {
                (row < self.power_menu_visible_rows()).then(|| self.power_menu_row_rect(row))
            }
            _ => None,
        }
    }

    /// Where row `index` of the start menu's list is: its row on screen, or
    /// above or below the list for one scrolled out of it.
    fn start_row_box(&self, index: usize) -> Rect {
        let first = self.start_menu_row_rect(0);
        let rows_down = |n: usize| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a list's length, far below where f32 loses whole numbers"
            )]
            let n = n as f32;
            n * first.h
        };
        let y = if index >= self.start_menu_scroll {
            first.y + rows_down(index.saturating_sub(self.start_menu_scroll))
        } else {
            first.y - rows_down(self.start_menu_scroll.saturating_sub(index))
        };
        Rect::new(first.x, y, first.w, first.h)
    }

    /// Scroll the start menu's list so row `index` shows, as the user would
    /// before clicking it.
    fn reveal_start_row(&mut self, index: usize) {
        let shown = self.start_menu_visible_rows().max(1);
        if index < self.start_menu_scroll {
            self.start_menu_scroll = index;
        } else if index >= self.start_menu_scroll.saturating_add(shown) {
            self.start_menu_scroll = index.saturating_add(1).saturating_sub(shown);
        }
    }

    /// Click the control `hit`: the shell's own press and release at the
    /// middle of its box. Refused where it is not on screen, or where the
    /// click would not reach it ([`reaches`](Self::reaches)).
    fn click_part(&mut self, hit: Hit) -> Result<Option<ShellAction>, Refusal> {
        if let Hit::StartMenuEntry(index) = hit {
            if !self.start_menu_open || index >= self.start_menu_rows().len() {
                return Err(Refusal::NoSuchWidget);
            }
            // Before scrolling: a refused action changes nothing.
            if self.takes_every_press() || self.closed_by_press(hit).is_some() {
                return Err(Refusal::Hidden);
            }
            self.reveal_start_row(index);
        }
        let rect = self.part_rect(hit).ok_or(Refusal::NoSuchWidget)?;
        let (x, y) = rect.centre();
        if !self.reaches(x, y, hit) {
            return Err(Refusal::Hidden);
        }
        Ok(self.click_at(x, y))
    }

    /// The shell's own press and release at `(x, y)`, answering what the
    /// host is to do: a control answers the press or the release -- a tile
    /// acts when the button comes up, so a drag of it is not a click.
    fn click_at(&mut self, x: f32, y: f32) -> Option<ShellAction> {
        let pressed = self.handle_mouse(&click(x, y));
        let released = self.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        });
        answered(released).or_else(|| answered(pressed))
    }

    /// The desktop's icons, each named by its label and said to be what it
    /// is, the chosen ones marked.
    fn icons_node(&self) -> Node<ShellPart> {
        let (width, height) = self.viewport();
        let mut list = Node::new(
            ShellPart::Icons,
            Role::List,
            "Desktop icons",
            Rect::new(0.0, 0.0, width, height),
        );
        for id in self.icons.icon_ids() {
            let (Some(icon), Some(bounds)) = (self.icons.get_icon(id), self.icons.icon_rect(id))
            else {
                continue;
            };
            let mut node = Node::new(
                ShellPart::Icon(id),
                Role::ListItem,
                icon.label.clone(),
                bounds,
            );
            node.description = Some(
                match icon.icon_type {
                    icons::IconType::Folder => "folder",
                    icons::IconType::File => "file",
                    icons::IconType::Shortcut => "shortcut",
                    icons::IconType::Drive => "drive",
                    icons::IconType::RecycleBin => "recycle bin",
                    icons::IconType::Computer => "computer",
                    icons::IconType::Document => "document",
                    icons::IconType::Image => "picture",
                    icons::IconType::Executable => "program",
                }
                .to_owned(),
            );
            node.value = Some(Value::Chosen(icon.selected));
            node.focused = icon.selected;
            node.focusable = true;
            list.children.push(node);
        }
        list
    }

    /// Click the desktop icon `id` -- and, where `open`, click it again as a
    /// double click: the shell's own presses, releases and double click at
    /// the middle of its cell. Refused where something is drawn over it.
    fn click_icon(
        &mut self,
        id: icons::IconId,
        open: bool,
    ) -> Result<Option<ShellAction>, Refusal> {
        let rect = self.icons.icon_rect(id).ok_or(Refusal::NoSuchWidget)?;
        let (x, y) = rect.centre();
        if !self.reaches(x, y, Hit::Desktop) || self.icons.icon_at(x, y) != Some(id) {
            return Err(Refusal::Hidden);
        }
        let release = MouseEvent {
            x,
            y,
            kind: MouseEventKind::Release(MouseButton::Left),
        };
        let mut said = answered(self.handle_mouse(&click(x, y)));
        said = answered(self.handle_mouse(&release)).or(said);
        if open {
            said = answered(self.handle_mouse(&MouseEvent {
                x,
                y,
                kind: MouseEventKind::DoubleClick(MouseButton::Left),
            }))
            .or(said);
            said = answered(self.handle_mouse(&release)).or(said);
        }
        Ok(said)
    }

    /// The taskbar's parts, left to right.
    fn taskbar_node(&self) -> Node<ShellPart> {
        let mut bar = Node::new(
            ShellPart::Taskbar,
            Role::Group,
            "Taskbar",
            self.taskbar_rect(),
        );
        let mut start = button(Hit::StartButton, "Start", self.start_button_rect());
        start.value = Some(Value::Chosen(self.start_menu_open));
        bar.children.push(start);
        for (slot, rect) in self.taskbar_slots().into_iter().zip(self.taskbar_layout()) {
            bar.children.push(match slot {
                TaskbarSlot::Pinned(pin) => {
                    let name = self
                        .taskbar
                        .pinned_apps()
                        .get(pin)
                        .map_or_else(String::new, |app| app.display_name.clone());
                    let mut node = button(Hit::TaskbarPinned(pin), name, rect);
                    node.description = Some("pinned: starts the program".to_owned());
                    node
                }
                TaskbarSlot::Window(id) => {
                    let title = self
                        .windows
                        .get(&id)
                        .map_or_else(String::new, |window| window.title.clone());
                    let mut node = button(Hit::TaskbarButton(id), title, rect);
                    node.value = Some(Value::Chosen(self.focused_window == Some(id)));
                    node
                }
            });
        }
        for (index, (icon, rect)) in self
            .ordered_tray_icons()
            .into_iter()
            .zip(self.tray_icon_rects())
            .enumerate()
        {
            let name = if icon.tooltip.is_empty() {
                "Notification area icon".to_owned()
            } else {
                icon.tooltip.clone()
            };
            bar.children.push(button(Hit::TrayIcon(index), name, rect));
        }
        if let Some(rect) = self.tray_overflow_rect() {
            bar.children
                .push(button(Hit::TrayOverflow, "Show hidden icons", rect));
        }
        let mut speaker = button(Hit::VolumeIcon, "Volume", self.volume_icon_rect());
        speaker.description = Some(self.volume_tooltip());
        speaker.value = Some(Value::Chosen(self.volume_flyout.is_visible()));
        bar.children.push(speaker);
        let mut bell = button(Hit::NotificationBell, "Notifications", self.bell_rect());
        let unread = self.notifications.unread_count();
        bell.description = Some(match unread {
            0 => "none unread".to_owned(),
            1 => "1 unread".to_owned(),
            n => format!("{n} unread"),
        });
        bar.children.push(bell);
        let now = Self::unix_now();
        let (time, _) = self.clock_lines_at(now);
        let mut clock = button(Hit::Clock, time, self.clock_rect());
        clock.description = Some(self.clock_tooltip_at(now));
        bar.children.push(clock);
        bar.children.push(button(
            Hit::ShowDesktop,
            "Show desktop",
            self.show_desktop_rect(),
        ));
        bar
    }

    /// The open start menu's parts: the search, the list, the places and the
    /// power button.
    fn start_menu_node(&self) -> Node<ShellPart> {
        let mut menu = Node::new(
            ShellPart::StartMenu,
            Role::Dialog,
            "Start menu",
            self.start_menu_rect(),
        );
        let mut search = Node::new(
            ShellPart::StartSearch,
            Role::TextField,
            "Search",
            self.start_search_rect(),
        );
        search.value = Some(Value::Text(self.start_query.text().to_owned()));
        search.focused = true;
        search.focusable = true;
        menu.children.push(search);

        let shown = self.start_menu_visible_rows();
        let list_box = if shown == 0 {
            self.start_menu_rect()
        } else {
            let first = self.start_menu_row_rect(0);
            let last = self.start_menu_row_rect(shown.saturating_sub(1));
            Rect::new(first.x, first.y, first.w, last.y + last.h - first.y)
        };
        let mut list = Node::new(ShellPart::StartList, Role::List, "Programs", list_box);
        for (index, row) in self.start_menu_rows().into_iter().enumerate() {
            let part = ShellPart::Control(Hit::StartMenuEntry(index));
            let bounds = self.start_row_box(index);
            let mut node = match row {
                StartRow::Program { entry, .. } => {
                    let mut node = Node::new(part, Role::ListItem, entry.name.clone(), bounds);
                    node.description =
                        (!entry.description.is_empty()).then(|| entry.description.clone());
                    node
                }
                StartRow::Action { entry, action } => {
                    let mut node = Node::new(part, Role::ListItem, action.name.clone(), bounds);
                    node.description = Some(entry.name.clone());
                    node
                }
                StartRow::Folder { folder, open } => {
                    let mut node = Node::new(part, Role::ListItem, folder.label(), bounds);
                    node.description =
                        Some(if open { "folder, open" } else { "folder" }.to_owned());
                    node
                }
                StartRow::Section(section) => Node::new(part, Role::Label, section.label(), bounds),
            };
            node.focused = self.start_selected == Some(index);
            node.focusable = node.role == Role::ListItem;
            list.children.push(node);
        }
        menu.children.push(list);

        for which in StartShortcut::ALL {
            menu.children.push(button(
                Hit::StartMenuShortcut(*which),
                which.label(),
                self.start_shortcut_rect(*which),
            ));
        }
        menu.children.push(button(
            Hit::PowerButton,
            power::PowerChoice::ShutDown.label(),
            self.power_button_rect(),
        ));
        let mut caret = button(Hit::PowerCaret, "Power options", self.power_caret_rect());
        caret.value = Some(Value::Chosen(self.power_menu_open));
        menu.children.push(caret);
        menu
    }

    /// The open power choices.
    fn power_menu_node(&self) -> Node<ShellPart> {
        let mut menu = Node::new(
            ShellPart::PowerMenu,
            Role::List,
            "Power options",
            self.power_menu_rect(),
        );
        for (row, choice) in power::PowerChoice::ALL
            .iter()
            .enumerate()
            .take(self.power_menu_visible_rows())
        {
            menu.children.push(Node::new(
                ShellPart::Control(Hit::PowerMenuEntry(row)),
                Role::ListItem,
                choice.label(),
                self.power_menu_row_rect(row),
            ));
        }
        menu
    }

    /// Do `action` to `part` of the open notification pane, as the user
    /// would: a level set as its slider sets it, anything else pressed.
    fn invoke_pane(
        &mut self,
        part: PanePart,
        action: Action,
    ) -> Result<Option<ShellAction>, Refusal> {
        if !self.notifications.pane_state().is_visible() {
            return Err(Refusal::NoSuchWidget);
        }
        let (width, _) = self.viewport();
        let height = self.notification_pane_height();
        // The pane says what the part is now, and whether it can be used:
        // a card dismissed is no part, a switch with no radio is out of use.
        let (role, enabled) = self
            .notifications
            .automation(width, height)
            .walk()
            .find(|node| node.id == part)
            .map(|node| (node.role, node.enabled))
            .ok_or(Refusal::NoSuchWidget)?;
        // A level to set, or `None` for a press.
        let level = match (part, &action) {
            (PanePart::Volume | PanePart::Brightness, Action::SetValue(value)) => Some(*value),
            (PanePart::Switch(_) | PanePart::ProgramSwitch(_), Action::Toggle | Action::Press)
            | (
                PanePart::Settings
                | PanePart::ClearAll
                | PanePart::Back
                | PanePart::FullSettings
                | PanePart::Card(_)
                | PanePart::Dismiss(_),
                Action::Press,
            ) => None,
            _ => {
                return Err(Refusal::NotApplicable {
                    role,
                    action: action.name(),
                });
            }
        };
        if self.takes_every_press() {
            return Err(Refusal::Hidden);
        }
        if level.is_some_and(|value| !value.is_finite()) {
            return Err(Refusal::NotANumber);
        }
        if !enabled {
            return Err(Refusal::Disabled);
        }
        let Some(value) = level else {
            return self.press_pane(part, width, height);
        };
        if part == PanePart::Volume {
            self.apply_volume_action(volume_flyout::Action::Level(level_of(value)));
        } else {
            self.notifications.set_brightness(level_of(value));
            self.write_brightness();
        }
        Ok(None)
    }

    /// Press `part` of the open notification pane: scrolled into the list
    /// first if it is a card's or a program's, pointed at first if it is a
    /// card's cross -- which is shown while the pointer is on its card --
    /// then the shell's own press and release at the middle of its box.
    fn press_pane(
        &mut self,
        part: PanePart,
        width: f32,
        height: f32,
    ) -> Result<Option<ShellAction>, Refusal> {
        self.notifications.reveal(part, height);
        let (x, y) = self
            .notifications
            .press_point(part, width, height)
            .ok_or(Refusal::Hidden)?;
        if let PanePart::Dismiss(_) = part {
            // Where the pointer is, not something a tool answers to.
            let _pointed = self.handle_mouse(&MouseEvent {
                x,
                y,
                kind: MouseEventKind::Move,
            });
        }
        Ok(self.click_at(x, y))
    }

    /// The open volume flyout: its level, its mute switch and its way to
    /// Settings -- the first two out of use while the card is out of reach.
    fn volume_node(&self) -> Node<ShellPart> {
        let layout = self.volume_flyout_layout();
        let reachable = self.volume.out_of_reach().is_none();
        let mut flyout = Node::new(ShellPart::Volume, Role::Group, "Volume", layout.panel);
        flyout.description = self.volume.out_of_reach().map(str::to_owned);
        let mut level = Node::new(
            ShellPart::VolumeLevel,
            Role::Slider,
            "Volume",
            layout.slider.hit(),
        );
        level.value = Some(Value::Range {
            value: f64::from(self.notifications.volume()),
            min: 0.0,
            max: 100.0,
        });
        level.enabled = reachable;
        level.focusable = true;
        let mut mute = Node::new(ShellPart::VolumeMute, Role::CheckBox, "Mute", layout.mute);
        mute.value = Some(Value::Check(if self.notifications.is_muted() {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }));
        mute.enabled = reachable;
        flyout.children = vec![
            level,
            mute,
            Node::new(
                ShellPart::AudioSettings,
                Role::Button,
                volume_flyout::SETTINGS_LABEL,
                layout.settings,
            ),
        ];
        flyout
    }
}

impl Accessible for DesktopShell {
    type Part = ShellPart;
    type Event = ShellAction;

    fn automation(&self, _width: f32, _height: f32) -> Node<ShellPart> {
        let (width, height) = self.viewport();
        let mut root = Node::new(
            ShellPart::Desktop,
            Role::Group,
            "Desktop",
            Rect::new(0.0, 0.0, width, height),
        );
        root.children.push(self.icons_node());
        root.children.push(self.taskbar_node());
        if self.start_menu_open {
            root.children.push(self.start_menu_node());
        }
        if self.power_menu_open {
            root.children.push(self.power_menu_node());
        }
        if self.volume_flyout.is_visible() {
            root.children.push(self.volume_node());
        }
        if self.notifications.pane_state().is_visible() {
            root.children.push(
                self.notifications
                    .automation(width, self.notification_pane_height())
                    .map(&ShellPart::Pane),
            );
        }
        root
    }

    fn invoke(
        &mut self,
        part: &ShellPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<ShellAction>, Refusal> {
        let asked = action.name();
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: asked,
        };
        match (*part, action) {
            (ShellPart::Icon(id), Action::Choose | Action::Focus) => self.click_icon(id, false),
            (ShellPart::Icon(id), Action::Press) => self.click_icon(id, true),
            (ShellPart::Icon(_), _) => Err(not_for(Role::ListItem)),
            (ShellPart::Icons, _) => Err(not_for(Role::List)),
            (
                ShellPart::Control(hit @ Hit::StartMenuEntry(index)),
                Action::Press | Action::Choose,
            ) => {
                if matches!(
                    self.start_menu_rows().get(index),
                    Some(StartRow::Section(_))
                ) {
                    return Err(not_for(Role::Label));
                }
                self.click_part(hit)
            }
            (ShellPart::Control(hit @ Hit::PowerMenuEntry(_)), Action::Press | Action::Choose)
            | (ShellPart::Control(hit), Action::Press) => self.click_part(hit),
            (ShellPart::Control(Hit::PowerMenuEntry(_)), _) => Err(not_for(Role::ListItem)),
            (ShellPart::Control(Hit::StartMenuEntry(_)), _) => Err(not_for(Role::ListItem)),
            (ShellPart::Control(_), _) => Err(not_for(Role::Button)),
            (ShellPart::StartSearch, Action::SetText(text)) => {
                if !self.start_menu_open {
                    return Err(Refusal::NoSuchWidget);
                }
                if self.takes_every_press() {
                    return Err(Refusal::Hidden);
                }
                self.start_query.set_text(&text);
                self.search_changed();
                Ok(None)
            }
            (ShellPart::StartSearch, Action::Focus) => {
                // It has the keyboard whenever the menu is open.
                if !self.start_menu_open {
                    return Err(Refusal::NoSuchWidget);
                }
                if self.takes_every_press() {
                    return Err(Refusal::Hidden);
                }
                Ok(None)
            }
            (ShellPart::StartSearch, _) => Err(not_for(Role::TextField)),
            (ShellPart::VolumeLevel, Action::SetValue(value)) => {
                if !self.volume_flyout.is_visible() {
                    return Err(Refusal::NoSuchWidget);
                }
                if self.takes_every_press() {
                    return Err(Refusal::Hidden);
                }
                if !value.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                if self.volume.out_of_reach().is_some() {
                    return Err(Refusal::Disabled);
                }
                self.apply_volume_action(volume_flyout::Action::Level(level_of(value)));
                Ok(None)
            }
            (ShellPart::VolumeLevel, _) => Err(not_for(Role::Slider)),
            (ShellPart::VolumeMute, Action::Toggle | Action::Press) => {
                if !self.volume_flyout.is_visible() {
                    return Err(Refusal::NoSuchWidget);
                }
                if self.takes_every_press() {
                    return Err(Refusal::Hidden);
                }
                if self.volume.out_of_reach().is_some() {
                    return Err(Refusal::Disabled);
                }
                self.apply_volume_action(volume_flyout::Action::ToggleMute);
                Ok(None)
            }
            (ShellPart::VolumeMute, _) => Err(not_for(Role::CheckBox)),
            (ShellPart::AudioSettings, Action::Press) => {
                if !self.volume_flyout.is_visible() {
                    return Err(Refusal::NoSuchWidget);
                }
                let (x, y) = self.volume_flyout_layout().settings.centre();
                if !self.reaches(x, y, Hit::VolumeFlyout) {
                    return Err(Refusal::Hidden);
                }
                Ok(self.click_at(x, y))
            }
            (ShellPart::AudioSettings, _) => Err(not_for(Role::Button)),
            (ShellPart::Pane(part), action) => self.invoke_pane(part, action),
            (ShellPart::StartList | ShellPart::PowerMenu, _) => Err(not_for(Role::List)),
            (ShellPart::StartMenu, _) => Err(not_for(Role::Dialog)),
            (ShellPart::Desktop | ShellPart::Taskbar | ShellPart::Volume, _) => {
                Err(not_for(Role::Group))
            }
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
