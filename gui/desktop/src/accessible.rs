//! The desktop shell as tools see it -- a screen reader, a script
//! ([`Accessible`]): the desktop's icons, the taskbar and what is open over
//! it.
//!
//! The icons are a list, each named by its label and said to be what it is
//! -- a folder, a program, the recycle bin -- the chosen ones marked; one
//! chosen is clicked, one pressed is double-clicked, so it opens. The
//! widgets over them (`widgets::accessible`) are each a group named by its
//! title, holding what it shows -- a clock's time and date, a meter's
//! reading, a note's text, a frame's picture, the battery's charge; a note
//! given the keyboard is clicked, which opens it for writing, and its text
//! set is pasted over all of it.
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
//! calendar is open, its arrows, its title, "Today", its days or months and
//! the chosen day's events (`calendar::accessible`); while a window
//! switch is under way, the switcher's windows, the one it goes to chosen;
//! while the overview is open, its search, its windows' cards and close
//! buttons and its desktops' lanes (`overview::accessible`); while the
//! notification pane is open, its parts (`notif_pane::accessible`); and
//! every menu open over everything -- the desktop's, a notification's, a
//! tile's, a field's -- with its rows, and the submenu a row opened
//! (`guitk::menu::MenuPart`), a row chosen as a click chooses it. While
//! the tiling overlay is up, its zones, each named by where it is and
//! pressed as clicked -- the focused window tiled there -- and its picker's
//! layouts, chosen as the user chooses one: the pointer to the top edge,
//! onto the layout, pressed. While a shut down waits on programs still
//! open, its list: what it says, the programs, and "... anyway" and Cancel.
//! While it is up, the Run box (`run_dialog::accessible`): its line, typed
//! over and run, its suggestions, OK, Cancel, Browse... -- a line run
//! started, as after a click. While they are up, the file chooser
//! (`guitk::dialog::DialogTarget`), whose choice goes where it was put up
//! for -- the Run box's line, a folder frame -- and the character picker
//! over a field (`charpicker::Target`), whose pick is typed into the field:
//! each the component's own parts, where the shell draws it. And, only to be
//! read, the on-screen display while it is up (`osd::OsdPart`) -- a level, a
//! lock key, a track, each saying what it says -- and the card saying how to
//! move the wallpaper while it is being moved.
//!
//! # As the user would
//!
//! A part pressed is clicked: the shell's own press and release at the
//! middle of the box it is drawn in, so the click does exactly what the
//! user's would -- a program started, a window raised, a menu opened -- and
//! answers the [`ShellAction`] the host carries out. A part the click would
//! not reach refuses: one covered by something drawn over it -- a menu, the
//! Run box, the notification pane's scrim, the pop-ups in the corner, a
//! widget over an icon -- and one a click would be spent closing something
//! else for, as a press away from an open menu or flyout is. A row of the start menu or a notification scrolled out of sight is
//! scrolled into it first, as the user would; a notification's cross,
//! shown while the pointer is on its card, is pointed at before it is
//! pressed. The search's text, the volume's level, its mute and the
//! brightness are set as typing and the controls set them.

use guitk::dialog::DialogTarget;
use guitk::menu::{ContextMenu, MenuPart};
use guitk::widget::CheckState;
use guitk::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

use crate::calendar::{CalendarHit, CalendarPart, CalendarViewMode};
use crate::notif_pane::PanePart;
use crate::osd::OsdPart;
use crate::overview::{self, OverviewPart};
use crate::run_dialog::RunPart;
use crate::snap::{SnapLayoutPreset, ZoneId};
use crate::widgets::{NoteKey, WidgetInstanceId, WidgetPart};
use crate::{
    DesktopShell, Hit, MouseButton, MouseEvent, MouseEventKind, Rect, ShellAction, StartRow,
    StartShortcut, SwitchView, TaskbarSlot, TextRole, WindowId, click, icons, power, volume_flyout,
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
    /// A part of the widgets out on the desktop (`widgets::accessible`): a
    /// note's text set as typed into it, after a click on it.
    Widget(WidgetPart),
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
    /// A part of a menu the shell has open over everything.
    Menu(ShellMenu, MenuPart),
    /// A part of the calendar popup, while it is open, that is no control:
    /// the popup, its grid, the card of a day's events and the events on it.
    /// Its controls -- the arrows, the title, "Today", a day, a month -- are
    /// [`Control`](Self::Control)s, pressed as clicked.
    Calendar(CalendarPart),
    /// A part of the overview, while it is open: its search, its windows'
    /// cards and their close buttons, its desktops' lanes.
    Overview(OverviewPart),
    /// The window switcher, while a switch is shown as its strip.
    Switcher,
    /// A window in the switcher: chosen as Tab steps to it, pressed as
    /// letting go on it switches to it.
    SwitchTo(WindowId),
    /// A part of the character picker, while it is up over a field
    /// (`char_picker`): what is picked is typed into the field.
    CharPicker(charpicker::Target),
    /// A part of the file chooser, while it is up: what it chooses goes
    /// where it was put up for -- the Run box's line, a folder frame.
    Chooser(DialogTarget),
    /// A part of the Run box, while it is up: a line run is started, and
    /// Browse puts the chooser up.
    RunBox(RunPart),
    /// The tiling overlay, while it is up: its zones -- each a
    /// [`Control`](Self::Control), pressed as clicked, the focused window
    /// tiled into it -- and its layouts.
    Snap,
    /// The tiling overlay's layout picker, summoned by the pointer at the
    /// top edge.
    SnapLayouts,
    /// A layout in the picker: chosen as the user chooses it, the pointer
    /// brought to the top edge, onto it, and pressed.
    SnapLayout(SnapLayoutPreset),
    /// The list a shut down, restart or log out waits on, while it is up:
    /// what it says, its programs, and its two buttons -- each a
    /// [`Control`](Self::Control).
    Ending,
    /// The programs still open, on that list.
    EndingPrograms,
    /// A program still open, by its window.
    EndingProgram(WindowId),
    /// The notifications popped up in the corner above the taskbar, while
    /// any are.
    Toasts,
    /// One of them, by its notification's id: pressed, it is opened, as a
    /// click on it opens it.
    Toast(u64),
    /// Its close button: pressed, the pop-up goes and the notification
    /// stays in the pane, unread.
    ToastClose(u64),
    /// A part of the on-screen display -- the volume's or brightness's
    /// level, a lock key, a track -- while one is up: only to be read.
    Osd(OsdPart),
    /// The card saying how to move the wallpaper, while it is being moved:
    /// the keys and the pointer move it, Enter keeps it, Escape puts it back.
    WallpaperMove,
}

/// A menu the shell opens over everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellMenu {
    /// A text field's, offering what its keys do: over the Run box, the
    /// start menu's search, a rename or a note.
    Field,
    /// The desktop's, or a desktop icon's.
    Desktop,
    /// A notification's.
    Notification,
    /// The taskbar's.
    Taskbar,
    /// A taskbar tile's: a pinned program's, or a window's.
    Tile,
    /// The tray's hidden icons.
    HiddenIcons,
}

impl ShellMenu {
    /// Every menu, in the order a press reaches them (`handle_mouse_inner`):
    /// the first open one takes it.
    const IN_PRESS_ORDER: [Self; 6] = [
        Self::Field,
        Self::Desktop,
        Self::Notification,
        Self::Taskbar,
        Self::Tile,
        Self::HiddenIcons,
    ];

    /// What tools call it.
    const fn name(self) -> &'static str {
        match self {
            Self::Field => "Text",
            Self::Desktop => "Desktop",
            Self::Notification => "Notification",
            Self::Taskbar => "Taskbar",
            Self::Tile => "Taskbar button",
            Self::HiddenIcons => "Hidden icons",
        }
    }
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
    /// Whether something the shell hands a press to before its file chooser
    /// takes every press wherever it lands: the character picker, a menu, a
    /// wallpaper being moved, or a drag or a press under way -- as
    /// `handle_mouse_with` asks them, in that order.
    fn ahead_of_chooser(&self) -> bool {
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
    }

    /// Whether something open over everything takes every press wherever it
    /// lands, before any part of the shell under it could: what is ahead of
    /// the chooser ([`ahead_of_chooser`](Self::ahead_of_chooser)), the
    /// chooser, the Run box, the overview, the tiling overlay, or the list a
    /// shut down waits on -- as `handle_mouse_with` and `handle_press_with`
    /// hand a press.
    fn takes_every_press(&self) -> bool {
        self.ahead_of_chooser()
            || self.chooser.is_some()
            || self.run_dialog.is_visible()
            || self.overview.visible
            || self.snap.is_overlay_visible()
            || self.ending_listing()
    }

    /// The file chooser as tools see it, where the shell draws it, while it
    /// is up.
    fn chooser_node(&self) -> Option<Node<ShellPart>> {
        let dialog = self.chooser.as_ref()?;
        let (x, y, width, height) = self.chooser_rect();
        Some(
            dialog
                .automation(width, height)
                .map(&ShellPart::Chooser)
                .translated(x, y),
        )
    }

    /// `action` on `target` of the file chooser, as the chooser's own click
    /// or key does it -- and what the shell does after one: a file chosen
    /// goes where the chooser was put up for, and the chooser comes down, as
    /// it does on Cancel.
    fn invoke_chooser(
        &mut self,
        target: DialogTarget,
        action: Action,
    ) -> Result<Option<ShellAction>, Refusal> {
        if self.chooser.is_none() {
            return Err(Refusal::NoSuchWidget);
        }
        if self.ahead_of_chooser() {
            return Err(Refusal::Hidden);
        }
        let (_, _, width, height) = self.chooser_rect();
        let dialog = self.chooser.as_mut().ok_or(Refusal::NoSuchWidget)?;
        if let Some(answer) = dialog.invoke(&target, action, width, height)? {
            self.apply_chooser_action(answer);
        }
        Ok(None)
    }

    /// `action` on `part` of the Run box, as the box's own click or key does
    /// it -- and what the shell does after one, from the same code: a line
    /// run is started (`run_request`), Browse puts the chooser up over it.
    /// Refused under the chooser, and under anything ahead of that.
    fn invoke_run_box(
        &mut self,
        part: RunPart,
        action: Action,
    ) -> Result<Option<ShellAction>, Refusal> {
        if !self.run_dialog.is_visible() {
            return Err(Refusal::NoSuchWidget);
        }
        if self.ahead_of_chooser() || self.chooser.is_some() {
            return Err(Refusal::Hidden);
        }
        let said = self
            .run_dialog
            .invoke(&part, action, 0.0, 0.0)?
            .unwrap_or_default();
        // The first, as after a press (`handle_mouse_with`): one action
        // reaches one button, and only OK or Enter runs a line.
        let request = self.act_on_run_dialog(said).into_iter().next();
        Ok(request
            .and_then(|request| self.run_request(request))
            .map(ShellAction::Launch))
    }

    /// Whether something the shell hands a press to before its own surfaces
    /// -- the overview, the list a shut down waits on, the tiling overlay --
    /// takes it: what is ahead of the chooser, the chooser, the Run box, or
    /// the notification pane's scrim, as `handle_mouse_with` asks them.
    fn ahead_of_own_surfaces(&self) -> bool {
        self.ahead_of_chooser()
            || self.chooser.is_some()
            || self.run_dialog.is_visible()
            || self.notifications.pane_state().is_visible()
    }

    /// The tiling overlay, while it is up: its zones, each named by where it
    /// is, and the layouts of its picker -- the active one chosen, all of
    /// them shown only while the picker is.
    fn snap_node(&self) -> Option<Node<ShellPart>> {
        if !self.snap.is_overlay_visible() {
            return None;
        }
        let area = self.snap.work_area();
        let mut overlay = Node::new(
            ShellPart::Snap,
            Role::Group,
            "Snap layout",
            Rect::new(area.x, area.y, area.width, area.height),
        );
        let active = self.snap.active_preset();
        overlay.description = Some(active.label().to_owned());
        for zone in &self.snap.layout().zones {
            overlay.children.push(button(
                Hit::SnapZone(zone.id),
                zone.label,
                Rect::new(zone.x, zone.y, zone.width, zone.height),
            ));
        }
        let (x, y, w, h) = self.snap.picker_rect();
        let mut picker = Node::new(
            ShellPart::SnapLayouts,
            Role::List,
            "Layouts",
            Rect::new(x, y, w, h),
        );
        picker.shown = self.snap.is_picker_visible();
        for &preset in SnapLayoutPreset::all() {
            let Some((tx, ty, size)) = self.snap.thumbnail_rect(preset) else {
                continue;
            };
            let mut layout = Node::new(
                ShellPart::SnapLayout(preset),
                Role::ListItem,
                preset.label(),
                Rect::new(tx, ty, size, size),
            );
            layout.value = Some(Value::Chosen(preset == active));
            layout.shown = picker.shown;
            picker.children.push(layout);
        }
        overlay.children.push(picker);
        Some(overlay)
    }

    /// A press on the tiling overlay's zone `zone` as the user's click at
    /// its middle: the focused window tiled into it.
    fn press_zone(&mut self, zone: ZoneId) -> Result<Option<ShellAction>, Refusal> {
        if !self.snap.is_overlay_visible() {
            return Err(Refusal::NoSuchWidget);
        }
        if self.ahead_of_own_surfaces() || self.overview.visible || self.ending_listing() {
            return Err(Refusal::Hidden);
        }
        let at = self.snap.zone_by_id(zone).ok_or(Refusal::NoSuchWidget)?;
        let (x, y) = (at.x + at.width / 2.0, at.y + at.height / 2.0);
        // Under the open picker, a click would choose a layout instead.
        if self.snap.picker_hit(x, y) {
            return Err(Refusal::Hidden);
        }
        Ok(self.click_at(x, y))
    }

    /// Choose `preset` in the tiling overlay's picker as the user does: the
    /// pointer to the top edge, which brings the picker; onto the layout's
    /// thumbnail; pressed.
    fn choose_layout(&mut self, preset: SnapLayoutPreset) -> Result<Option<ShellAction>, Refusal> {
        if !self.snap.is_overlay_visible() {
            return Err(Refusal::NoSuchWidget);
        }
        if self.ahead_of_own_surfaces() || self.overview.visible || self.ending_listing() {
            return Err(Refusal::Hidden);
        }
        let (x, y, size) = self
            .snap
            .thumbnail_rect(preset)
            .ok_or(Refusal::NoSuchWidget)?;
        let area = self.snap.work_area();
        self.point_at(area.x + area.width / 2.0, area.y + 1.0);
        let (cx, cy) = (x + size / 2.0, y + size / 2.0);
        self.point_at(cx, cy);
        Ok(self.click_at(cx, cy))
    }

    /// The list a shut down waits on, while it is up: named by what it says
    /// of how many are open and described by the rest; the programs still
    /// open, each named as the list names it; and its two buttons.
    fn ending_node(&self) -> Option<Node<ShellPart>> {
        let ending = self.ending.filter(|ending| ending.listing)?;
        let mut dialog = Node::new(
            ShellPart::Ending,
            Role::Dialog,
            self.ending_title(),
            self.ending_panel_rect(),
        );
        dialog.description = Some(Self::ending_lines(ending.choice).join(" "));
        let rows = self.ending_rows();
        let bounds = match (rows.first(), rows.last()) {
            (Some((_, first)), Some((_, last))) => {
                Rect::new(first.x, first.y, first.w, last.bottom() - first.y)
            }
            _ => Rect::default(),
        };
        let mut list = Node::new(
            ShellPart::EndingPrograms,
            Role::List,
            "Programs still open",
            bounds,
        );
        let more = self.windows.len().saturating_sub(rows.len());
        list.description = (more > 0).then(|| format!("and {more} more"));
        for (window, row) in &rows {
            list.children.push(Node::new(
                ShellPart::EndingProgram(window.id),
                Role::ListItem,
                self.ending_name(window),
                *row,
            ));
        }
        dialog.children.push(list);
        let (anyway, cancel) = self.ending_button_rects();
        dialog.children.push(button(
            Hit::EndingAnyway,
            format!("{} anyway", ending.choice.label()),
            anyway,
        ));
        dialog
            .children
            .push(button(Hit::EndingCancel, "Cancel", cancel));
        Some(dialog)
    }

    /// A press on the shut-down list's `hit` button, as the user's click at
    /// its middle: going ahead, or not.
    fn press_ending_button(&mut self, hit: Hit) -> Result<Option<ShellAction>, Refusal> {
        if !self.ending_listing() {
            return Err(Refusal::NoSuchWidget);
        }
        if self.ahead_of_own_surfaces() || self.overview.visible {
            return Err(Refusal::Hidden);
        }
        let (anyway, cancel) = self.ending_button_rects();
        let (x, y) = if hit == Hit::EndingAnyway {
            anyway.centre()
        } else {
            cancel.centre()
        };
        Ok(self.click_at(x, y))
    }

    /// The notifications popped up in the corner, top to bottom, while any
    /// are: each named and described as its card in the pane is -- by its
    /// title, else its program, and by its program and what it says -- with
    /// its close button. One whose notification has left the pane is
    /// leaving, and is left out.
    fn toasts_node(&self) -> Option<Node<ShellPart>> {
        let placed = self.toasts.placed();
        let bounds = placed.iter().map(|p| p.rect).reduce(Rect::union)?;
        let mut list = Node::new(ShellPart::Toasts, Role::List, "New notifications", bounds);
        let filed = self.notifications.notifications();
        for toast in placed {
            let Some(notif) = filed.iter().find(|n| n.id == toast.id) else {
                continue;
            };
            let name = if notif.title.is_empty() {
                notif.app_name.clone()
            } else {
                notif.title.clone()
            };
            let mut item = Node::new(ShellPart::Toast(toast.id), Role::ListItem, name, toast.rect);
            let mut description = notif.app_name.clone();
            if !notif.body.is_empty() {
                description.push_str(". ");
                description.push_str(&notif.body);
            }
            item.description = Some(description);
            item.children.push(Node::new(
                ShellPart::ToastClose(toast.id),
                Role::Button,
                "Close",
                toast.close,
            ));
            list.children.push(item);
        }
        Some(list)
    }

    /// Press the pop-up `id` -- or, `close`, its close button -- as the
    /// user's press on it: on the pop-ups' own surface, at the middle of
    /// what is pressed, where it is drawn on the screen now. A press a menu
    /// opened from a pop-up would take -- it only closes the menu -- or one
    /// on a pop-up still sliding in off the screen's edge is refused.
    fn press_toast(&mut self, id: u64, close: bool) -> Result<Option<ShellAction>, Refusal> {
        self.sync_toast_place();
        let toast = self
            .toasts
            .placed()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(Refusal::NoSuchWidget)?;
        if self.notification_menu.is_some() {
            return Err(Refusal::Hidden);
        }
        let (x, y) = if close {
            toast.close.centre()
        } else {
            // Its middle, or nearer its left where the close button would
            // take the middle of a short one.
            let (cx, cy) = toast.rect.centre();
            if toast.close.contains(cx, cy) {
                (toast.rect.x + toast.rect.w / 4.0, cy)
            } else {
                (cx, cy)
            }
        };
        #[allow(
            clippy::cast_precision_loss,
            reason = "a display dimension is exact in f32 for every size hardware produces"
        )]
        let screen = self.screen_width as f32;
        if !(0.0..screen).contains(&x) {
            return Err(Refusal::Hidden);
        }
        Ok(answered(self.handle_toast_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })))
    }

    /// Whether a press at `(x, y)` on `hit` -- what the shell's hit test
    /// says is there -- would reach it: nothing drawn over it takes the
    /// press, and it is not spent closing something open elsewhere.
    ///
    /// The notification pane is the second: while it is open, a press on
    /// any of the shell's parts but its bell never reaches the part -- above
    /// the taskbar the pane or its backdrop takes the press, and on the bar
    /// the press is spent closing it -- so `closed_by_press`, which says so
    /// of every part but the bell, answers for both.
    fn reaches(&self, x: f32, y: f32, hit: Hit) -> bool {
        !self.takes_every_press()
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
            Hit::CalendarControl(control) => {
                let (x, y) = self.calendar_origin();
                self.calendar
                    .control_rect(control, x, y, self.calendar_scale())
            }
            _ => None,
        }
    }

    /// Where a press lands on `part` of the open overview, or why there is
    /// nowhere.
    fn overview_point(&self, part: OverviewPart) -> Result<(f32, f32), Refusal> {
        let (width, height) = self.viewport();
        overview::accessible::press_point(
            &self.overview,
            &self.overview_config,
            width,
            height,
            part,
        )
        .ok_or(Refusal::Hidden)
    }

    /// The pointer moved to `(x, y)`: what lights a card, as the pointer
    /// does.
    fn point_at(&mut self, x: f32, y: f32) {
        // Where the pointer is is nothing a tool answers to.
        let _moved = self.handle_mouse(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Move,
        });
    }

    /// Do `action` to `part` of the open overview, as the user would: a
    /// card lit as the pointer lights it and pressed as clicked; a close
    /// button pressed with its card lit first, as the pointer reaching it
    /// lights it; a lane pressed off its cards; the search's text set as
    /// typing sets it.
    fn invoke_overview(
        &mut self,
        part: OverviewPart,
        action: &Action,
    ) -> Result<Option<ShellAction>, Refusal> {
        if !self.overview.visible {
            return Err(Refusal::NoSuchWidget);
        }
        let (width, height) = self.viewport();
        let role =
            overview::accessible::automation(&self.overview, &self.overview_config, width, height)
                .walk()
                .find(|node| node.id == part)
                .map(|node| node.role)
                .ok_or(Refusal::NoSuchWidget)?;
        match (part, action) {
            (OverviewPart::Search, Action::SetText(text)) => {
                self.overview.search_query.clone_from(text);
                self.overview.update_search();
                Ok(None)
            }
            // It has the keys whenever the overview is open.
            (OverviewPart::Search, Action::Focus) => Ok(None),
            (OverviewPart::Card(_), Action::Choose | Action::Focus) => {
                let (x, y) = self.overview_point(part)?;
                self.point_at(x, y);
                Ok(None)
            }
            (OverviewPart::Card(_) | OverviewPart::Lane(_), Action::Press) => {
                let (x, y) = self.overview_point(part)?;
                Ok(self.click_at(x, y))
            }
            (OverviewPart::Close(id), Action::Press) => {
                let (cx, cy) = self.overview_point(OverviewPart::Card(id))?;
                self.point_at(cx, cy);
                let (x, y) = self.overview_point(part)?;
                Ok(self.click_at(x, y))
            }
            _ => Err(Refusal::NotApplicable {
                role,
                action: action.name(),
            }),
        }
    }

    /// Whether a switch is under way and shown as the switcher's strip --
    /// one shown in the overview is the overview's to draw.
    fn switcher_shown(&self) -> bool {
        self.alt_tab_active && self.alt_tab_view == SwitchView::Switcher
    }

    /// The window switcher: each window the switch can go to, by its title,
    /// the one it goes to chosen -- each boxed in its cell on the page of the
    /// strip that shows it, as the strip is drawn once it is chosen.
    fn switcher_node(&self) -> Node<ShellPart> {
        let windows = self.switcher_windows();
        let size = self.font_size(TextRole::Body);
        let title_width = |index: usize| {
            windows
                .get(index)
                .map_or(0.0, |window| guitk::text::width(&window.title, size))
        };
        let shown = self.switcher_layout(
            windows.len(),
            self.alt_tab_index,
            title_width(self.alt_tab_index),
        );
        let mut list = Node::new(
            ShellPart::Switcher,
            Role::List,
            "Switch windows",
            shown.panel,
        );
        for (index, window) in windows.iter().enumerate() {
            let page = self.switcher_layout(windows.len(), index, title_width(index));
            let bounds = page
                .cells
                .iter()
                .find(|(at, _)| *at == index)
                .map_or(page.panel, |(_, cell)| *cell);
            let name = if window.title.is_empty() {
                window.app_id.clone()
            } else {
                window.title.clone()
            };
            let mut item = Node::new(ShellPart::SwitchTo(window.id), Role::ListItem, name, bounds);
            item.value = Some(Value::Chosen(index == self.alt_tab_index));
            item.focused = index == self.alt_tab_index;
            item.focusable = true;
            list.children.push(item);
        }
        list
    }

    /// Choose the window `id` in the switch under way, as Tab steps to it --
    /// and, where `go`, end the switch on it, as letting go does: the window
    /// raised is the host's to ask the compositor for.
    fn switch_to(&mut self, id: WindowId, go: bool) -> Result<Option<ShellAction>, Refusal> {
        if !self.switcher_shown() {
            return Err(Refusal::NoSuchWidget);
        }
        let index = self
            .switcher_windows()
            .iter()
            .position(|window| window.id == id)
            .ok_or(Refusal::NoSuchWidget)?;
        self.alt_tab_index = index;
        if !go {
            return Ok(None);
        }
        Ok(self.finish_alt_tab().map(ShellAction::Control))
    }

    /// Whether the day in cell `index` of the month the calendar shows is
    /// the one chosen.
    fn calendar_day_chosen(&self, index: usize) -> bool {
        self.calendar.mode == CalendarViewMode::Month
            && self
                .calendar
                .generate_grid()
                .get(index)
                .is_some_and(|cell| {
                    self.calendar.selected_date == Some((cell.year, cell.month, cell.day))
                })
    }

    /// The open calendar popup: its controls named as the shell's own, its
    /// other parts as the calendar's.
    fn calendar_node(&self) -> Node<ShellPart> {
        let (x, y) = self.calendar_origin();
        self.calendar
            .automation(x, y, self.calendar_scale(), &self.events)
            .map(&|part| match part {
                CalendarPart::Control(control) => ShellPart::Control(Hit::CalendarControl(control)),
                other => ShellPart::Calendar(other),
            })
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

    /// Whether a press at `(x, y)` on the desktop itself would reach what is
    /// there -- an icon, a widget: nothing the shell draws over the desktop
    /// takes it ([`reaches`](Self::reaches)), and the pop-ups' surface, which
    /// takes every press inside it (`handle_toast_mouse`), is not over it.
    fn desktop_reaches(&self, x: f32, y: f32) -> bool {
        self.reaches(x, y, Hit::Desktop)
            && !self
                .toasts
                .extent()
                .is_some_and(|stack| stack.contains(x, y))
    }

    /// Click the desktop icon `id` -- and, where `open`, click it again as a
    /// double click: the shell's own presses, releases and double click at
    /// the middle of its cell. Refused where something is drawn over it -- a
    /// widget among them, which takes a press on it.
    fn click_icon(
        &mut self,
        id: icons::IconId,
        open: bool,
    ) -> Result<Option<ShellAction>, Refusal> {
        let rect = self.icons.icon_rect(id).ok_or(Refusal::NoSuchWidget)?;
        let (x, y) = rect.centre();
        if !self.desktop_reaches(x, y)
            || self.widgets.hit_test(x, y).is_some()
            || self.icons.icon_at(x, y) != Some(id)
        {
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

    /// Do `action` to `part` of the widgets, as the user would: a note given
    /// the keyboard with a click on it, which opens it for writing, and its
    /// text set as a paste over all of it, the note opened so first -- one
    /// already open is not clicked again, which would move its caret.
    /// Nothing else on a widget is used.
    fn invoke_widget(
        &mut self,
        part: WidgetPart,
        action: Action,
    ) -> Result<Option<ShellAction>, Refusal> {
        let role = self
            .widgets
            .automation(&self.live_readings())
            .and_then(|tree| {
                tree.walk()
                    .find(|node| node.id == part)
                    .map(|node| node.role)
            })
            .ok_or(Refusal::NoSuchWidget)?;
        let not_for = |action: &Action| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        let WidgetPart::Note(id) = part else {
            return Err(not_for(&action));
        };
        let text = match action {
            Action::Focus => None,
            Action::SetText(text) => Some(text),
            other => return Err(not_for(&other)),
        };
        self.open_note(id)?;
        if let Some(text) = text
            && self.widgets.note_set_text(&text) == NoteKey::Changed
        {
            // A note's words are saved at every change, as typing's are.
            self.widgets_dirty = true;
        }
        Ok(None)
    }

    /// Open the note `id` for writing as the user does, with a click on the
    /// middle of its writing area -- unless it is open already. Refused
    /// where the click would not reach it: under something drawn over it, or
    /// spent closing something open elsewhere.
    fn open_note(&mut self, id: WidgetInstanceId) -> Result<(), Refusal> {
        let (x, y, width, height) = self.widgets.content_rect(id).ok_or(Refusal::NoSuchWidget)?;
        let (cx, cy) = (x + width / 2.0, y + height / 2.0);
        if !self.desktop_reaches(cx, cy) || self.widgets.note_body_at(cx, cy) != Some(id) {
            return Err(Refusal::Hidden);
        }
        if self.widgets.writing_note() == Some(id) {
            return Ok(());
        }
        // A press on a note asks the host for nothing.
        let _asked = self.click_at(cx, cy);
        // Should the shell ever hand the press to something the checks above
        // do not see, the note is not open, and that is not hidden from the
        // tool that asked.
        if self.widgets.writing_note() == Some(id) {
            Ok(())
        } else {
            Err(Refusal::Hidden)
        }
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

    /// The menu `which`, while it is open.
    fn open_menu(&self, which: ShellMenu) -> Option<&ContextMenu> {
        match which {
            ShellMenu::Field => self.field_menu.as_ref().map(|(menu, _)| menu),
            ShellMenu::Desktop => Some(&self.desktop_menu),
            ShellMenu::Notification => self.notification_menu.as_ref().map(|(menu, _)| menu),
            ShellMenu::Taskbar => self.taskbar_menu.as_ref(),
            ShellMenu::Tile => self.pin_menu.as_ref().map(|(menu, _)| menu),
            ShellMenu::HiddenIcons => self.tray_overflow_menu.as_ref().map(|(menu, _)| menu),
        }
        .filter(|menu| menu.is_visible())
    }

    /// The menu `which`, while it is open, to scroll a row of it into view.
    fn open_menu_mut(&mut self, which: ShellMenu) -> Option<&mut ContextMenu> {
        match which {
            ShellMenu::Field => self.field_menu.as_mut().map(|(menu, _)| menu),
            ShellMenu::Desktop => Some(&mut self.desktop_menu),
            ShellMenu::Notification => self.notification_menu.as_mut().map(|(menu, _)| menu),
            ShellMenu::Taskbar => self.taskbar_menu.as_mut(),
            ShellMenu::Tile => self.pin_menu.as_mut().map(|(menu, _)| menu),
            ShellMenu::HiddenIcons => self.tray_overflow_menu.as_mut().map(|(menu, _)| menu),
        }
        .filter(|menu| menu.is_visible())
    }

    /// The menu a press would reach, of those open: the first in the order
    /// `handle_mouse_inner` asks them -- `None` where none is open, or where
    /// something it asks first would take the press: the character picker,
    /// the wallpaper being moved, a widget being dragged.
    fn menu_on_top(&self) -> Option<ShellMenu> {
        let first = ShellMenu::IN_PRESS_ORDER
            .into_iter()
            .find(|&which| self.open_menu(which).is_some())?;
        let gesture = self.wallpaper_move.is_some() || self.widget_drag.is_some();
        (self.char_picker.is_none() && (first == ShellMenu::Field || !gesture)).then_some(first)
    }

    /// Do `action` to `part` of the open menu `which`, as the user would: a
    /// row clicked at its middle -- scrolled into the menu first, if it was
    /// out of it -- through the shell's own press, so the shell acts on the
    /// row as it does on a click. A check is toggled by choosing it.
    fn invoke_menu(
        &mut self,
        which: ShellMenu,
        part: MenuPart,
        action: &Action,
    ) -> Result<Option<ShellAction>, Refusal> {
        let menu = self.open_menu(which).ok_or(Refusal::NoSuchWidget)?;
        let (role, value) = menu
            .automation(0.0, 0.0)
            .walk()
            .find(|node| node.id == part)
            .map(|node| (node.role, node.value.clone()))
            .ok_or(Refusal::NoSuchWidget)?;
        let row = match (part, action) {
            (MenuPart::Item(id), Action::Press) => Some(id),
            (MenuPart::Item(id), Action::Toggle) if matches!(value, Some(Value::Check(_))) => {
                Some(id)
            }
            _ => None,
        };
        let Some(id) = row else {
            return Err(Refusal::NotApplicable {
                role,
                action: action.name(),
            });
        };
        if self.menu_on_top() != Some(which) {
            return Err(Refusal::Hidden);
        }
        let (x, y) = self
            .open_menu_mut(which)
            .ok_or(Refusal::NoSuchWidget)?
            .press_point(id)?;
        Ok(self.click_at(x, y))
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
        // Drawn over the icons, as they are drawn.
        if let Some(widgets) = self.widgets.automation(&self.live_readings()) {
            root.children.push(widgets.map(&ShellPart::Widget));
        }
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
        if self.calendar.visible {
            root.children.push(self.calendar_node());
        }
        if self.switcher_shown() {
            root.children.push(self.switcher_node());
        }
        if self.overview.visible {
            root.children.push(
                overview::accessible::automation(
                    &self.overview,
                    &self.overview_config,
                    width,
                    height,
                )
                .map(&ShellPart::Overview),
            );
        }
        if self.notifications.pane_state().is_visible() {
            root.children.push(
                self.notifications
                    .automation(width, self.notification_pane_height())
                    .map(&ShellPart::Pane),
            );
        }
        if let Some(snap) = self.snap_node() {
            root.children.push(snap);
        }
        if let Some(ending) = self.ending_node() {
            root.children.push(ending);
        }
        if self.run_dialog.is_visible() {
            root.children.push(
                self.run_dialog
                    .automation(width, height)
                    .map(&ShellPart::RunBox),
            );
        }
        if let Some(chooser) = self.chooser_node() {
            root.children.push(chooser);
        }
        // The pop-ups on their own surface over the shell's; a menu opened
        // from one is over them, and takes the press first.
        if let Some(toasts) = self.toasts_node() {
            root.children.push(toasts);
        }
        // The on-screen display, on the overlay surface, which takes no
        // press: what a key just did, to be read.
        if let Some(osd) = self.osd.automation() {
            root.children.push(osd.map(&ShellPart::Osd));
        }
        // How to move the wallpaper, while it is being moved: the keys and
        // the pointer do it, so the card is only to be read.
        if self.wallpaper_move.is_some() {
            let [(title, _), rest @ ..] = Self::WALLPAPER_MOVE_LINES;
            let mut card = Node::new(
                ShellPart::WallpaperMove,
                Role::Dialog,
                title,
                self.wallpaper_move_card(),
            );
            card.description = Some(
                rest.iter()
                    .map(|(words, _)| *words)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            root.children.push(card);
        }
        // The menus last, as they are drawn over everything else -- the one
        // a press reaches first last of all.
        for which in ShellMenu::IN_PRESS_ORDER.into_iter().rev() {
            if let Some(menu) = self.open_menu(which) {
                let mut node = menu
                    .automation(width, height)
                    .map(&|part| ShellPart::Menu(which, part));
                which.name().clone_into(&mut node.name);
                root.children.push(node);
            }
        }
        // And over even a field's menu, the picker it opened.
        if let Some(picker) = self.char_picker_node() {
            root.children.push(picker.map(&ShellPart::CharPicker));
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
            (ShellPart::Widget(part), action) => self.invoke_widget(part, action),
            // The tiling overlay's and the shut-down list's parts are the
            // surfaces' own, asked for a press before the shell's others.
            (ShellPart::Control(Hit::SnapZone(zone)), Action::Press) => self.press_zone(zone),
            (ShellPart::Control(hit @ (Hit::EndingAnyway | Hit::EndingCancel)), Action::Press) => {
                self.press_ending_button(hit)
            }
            (ShellPart::SnapLayout(preset), Action::Choose | Action::Press) => {
                self.choose_layout(preset)
            }
            (ShellPart::SnapLayout(_) | ShellPart::EndingProgram(_), _) => {
                Err(not_for(Role::ListItem))
            }
            (ShellPart::SnapLayouts | ShellPart::EndingPrograms, _) => Err(not_for(Role::List)),
            (ShellPart::Snap, _) => Err(not_for(Role::Group)),
            (ShellPart::Ending, _) => Err(not_for(Role::Dialog)),
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
            // A day already chosen stays chosen: a click on it would clear it,
            // and choosing is not clearing.
            (ShellPart::Control(Hit::CalendarControl(CalendarHit::Day(index))), Action::Choose)
                if self.calendar.visible && self.calendar_day_chosen(index) =>
            {
                if self.takes_every_press() {
                    Err(Refusal::Hidden)
                } else {
                    Ok(None)
                }
            }
            (ShellPart::Control(hit @ Hit::PowerMenuEntry(_)), Action::Press | Action::Choose)
            | (
                ShellPart::Control(
                    hit @ Hit::CalendarControl(CalendarHit::Day(_) | CalendarHit::Month(_)),
                ),
                Action::Choose,
            )
            | (ShellPart::Control(hit), Action::Press) => self.click_part(hit),
            (ShellPart::Control(Hit::PowerMenuEntry(_)), _) => Err(not_for(Role::ListItem)),
            (ShellPart::Control(Hit::StartMenuEntry(_)), _) => Err(not_for(Role::ListItem)),
            (
                ShellPart::Control(Hit::CalendarControl(
                    CalendarHit::Day(_) | CalendarHit::Month(_),
                )),
                _,
            ) => Err(not_for(Role::GridCell)),
            (ShellPart::Control(_), _) => Err(not_for(Role::Button)),
            (ShellPart::Overview(part), action) => self.invoke_overview(part, &action),
            (ShellPart::SwitchTo(id), Action::Choose | Action::Focus) => self.switch_to(id, false),
            (ShellPart::SwitchTo(id), Action::Press) => self.switch_to(id, true),
            (ShellPart::SwitchTo(_), _) => Err(not_for(Role::ListItem)),
            (ShellPart::Switcher, _) => Err(not_for(Role::List)),
            (ShellPart::Calendar(part), _) => Err(not_for(match part {
                CalendarPart::Popup => Role::Dialog,
                CalendarPart::Grid => Role::Grid,
                CalendarPart::Events => Role::List,
                CalendarPart::Event(_) => Role::ListItem,
                CalendarPart::Control(_) => Role::Button,
            })),
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
            (ShellPart::Menu(which, part), action) => self.invoke_menu(which, part, &action),
            (ShellPart::Chooser(target), action) => self.invoke_chooser(target, action),
            (ShellPart::RunBox(part), action) => self.invoke_run_box(part, action),
            (ShellPart::CharPicker(target), action) => {
                self.invoke_char_picker(target, action).map(answered)
            }
            (ShellPart::Toast(id), Action::Press) => self.press_toast(id, false),
            (ShellPart::ToastClose(id), Action::Press) => self.press_toast(id, true),
            (ShellPart::Toast(_), _) => Err(not_for(Role::ListItem)),
            (ShellPart::ToastClose(_), _) => Err(not_for(Role::Button)),
            // Only to be read: what the role of the part is, or that it went.
            (ShellPart::Osd(part), _) => Err(self
                .osd
                .automation()
                .and_then(|tree| {
                    tree.walk()
                        .find(|node| node.id == part)
                        .map(|node| node.role)
                })
                .map_or(Refusal::NoSuchWidget, not_for)),
            (ShellPart::WallpaperMove, _) => {
                if self.wallpaper_move.is_none() {
                    return Err(Refusal::NoSuchWidget);
                }
                Err(not_for(Role::Dialog))
            }
            (ShellPart::StartList | ShellPart::PowerMenu | ShellPart::Toasts, _) => {
                Err(not_for(Role::List))
            }
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
