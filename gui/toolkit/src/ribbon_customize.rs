//! The dialog for a ribbon's changes: every command on the left, every tab
//! -- its groups, and their commands -- on the right, and buttons between
//! and under them. The same changes a right-click on the ribbon offers, for
//! a user who wants to see them all at once: which tabs are shown, in what
//! order, and which commands each group holds.
//!
//! Every change goes through the ribbon's own methods (`add_to_group`,
//! `remove_from_group`, `hide_tab`, `move_tab`, `reset_customization`), so
//! the dialog holds nothing of the ribbon's but what is chosen in it.

use super::{
    CommandId, CommandSink, CornerRadii, FontWeightHint, Key, KeyEvent, MouseButton, MouseEvent,
    MouseEventKind, Palette, Rect, RenderCommand, Ribbon, RibbonEvent, TextOverflow, fill, line,
    stroke, text, text_top,
};

/// The dialog's size, when the window has room for it.
const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 440.0;
/// Between the dialog's edge and what is in it.
const PAD: f32 = 12.0;
/// The title's band.
const TITLE_HEIGHT: f32 = 30.0;
/// A row of either list.
pub(super) const ROW: f32 = 22.0;
/// The column of buttons between the lists.
const MIDDLE: f32 = 96.0;
/// A button.
const BUTTON_HEIGHT: f32 = 26.0;
/// How far a group's row, and a command's, sit in from its tab's.
const INDENT: f32 = 16.0;
/// A tab row's check box.
const CHECK: f32 = 12.0;
/// Text in the dialog.
const SIZE: f32 = 12.0;

/// What is chosen in the right-hand list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeRow {
    /// A tab, by its id.
    Tab(String),
    /// A group: its tab's id and its own.
    Group(String, String),
    /// A command in a group: tab id, group id, command.
    Command(String, String, CommandId),
}

/// Which list the arrow keys move in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The commands, on the left.
    Commands,
    /// The tabs, on the right.
    Tabs,
}

/// The dialog's own state: what is chosen, and where each list is scrolled
/// to. Everything else it shows is read from the ribbon as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Customize {
    /// Which list the arrow keys move in.
    pub focus: Focus,
    /// The command chosen on the left.
    pub command: Option<CommandId>,
    /// What is chosen on the right.
    pub chosen: Option<TreeRow>,
    /// The first row of each list shown: left, right.
    top: (usize, usize),
    /// What a button pressed and not yet released is.
    pressed: Option<Button>,
}

/// A button of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Put the command chosen on the left into the group chosen on the right.
    Add,
    /// Take the command chosen on the right out of its group.
    Remove,
    /// Move the tab chosen one place up -- left, on the ribbon.
    Up,
    /// Move it one place down.
    Down,
    /// Undo every change.
    Reset,
    /// Close the dialog.
    Close,
}

impl Button {
    /// Every button the dialog has.
    pub const ALL: [Self; 6] = [
        Self::Add,
        Self::Remove,
        Self::Up,
        Self::Down,
        Self::Reset,
        Self::Close,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Add => "Add \u{203a}",
            Self::Remove => "\u{2039} Remove",
            Self::Up => "Move up",
            Self::Down => "Move down",
            Self::Reset => "Undo all changes",
            Self::Close => "Close",
        }
    }
}

/// Where the dialog's parts are.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogSlots {
    /// The whole window, dimmed behind the dialog.
    pub scrim: Rect,
    /// The dialog.
    pub rect: Rect,
    /// The box of commands, on the left.
    pub commands: Rect,
    /// The command rows shown: which command, where.
    pub command_rows: Vec<(CommandId, Rect)>,
    /// The box of tabs, groups and commands, on the right.
    pub tree: Rect,
    /// The rows shown on the right: what, where, and a tab's check box.
    pub tree_rows: Vec<(TreeRow, Rect, Option<Rect>)>,
    /// Each button, where.
    pub buttons: Vec<(Button, Rect)>,
}

impl Customize {
    /// A dialog with nothing chosen, the commands' list having the keys.
    #[must_use]
    pub fn new() -> Self {
        Self {
            focus: Focus::Commands,
            command: None,
            chosen: None,
            top: (0, 0),
            pressed: None,
        }
    }
}

impl Default for Customize {
    fn default() -> Self {
        Self::new()
    }
}

/// Every row of the right-hand list, in order: each tab -- the ordinary ones
/// in the user's order, hidden ones too, then the contextual ones -- and
/// under a shown tab its groups, and under each group its commands.
fn tree(ribbon: &Ribbon) -> Vec<(TreeRow, String, bool)> {
    let mut ordinary: Vec<usize> = (0..ribbon.tabs.len())
        .filter(|&i| ribbon.tabs.get(i).is_some_and(|t| t.context.is_none()))
        .collect();
    ordinary.sort_by_key(|&i| ribbon.order_of(i));
    let contextual =
        (0..ribbon.tabs.len()).filter(|&i| ribbon.tabs.get(i).is_some_and(|t| t.context.is_some()));
    let mut rows = Vec::new();
    for i in ordinary.into_iter().chain(contextual) {
        let Some(tab) = ribbon.tabs.get(i) else {
            continue;
        };
        let hidden = ribbon.custom.hidden.contains(&tab.id);
        rows.push((TreeRow::Tab(tab.id.clone()), tab.label.clone(), hidden));
        if hidden {
            continue;
        }
        for group in &tab.groups {
            rows.push((
                TreeRow::Group(tab.id.clone(), group.id.clone()),
                group.label.clone(),
                false,
            ));
            for control in &group.controls {
                let command = control.command();
                rows.push((
                    TreeRow::Command(tab.id.clone(), group.id.clone(), command.id),
                    command.label.clone(),
                    false,
                ));
            }
        }
    }
    rows
}

/// How many rows a list box `h` high shows.
fn rows_in(h: f32) -> usize {
    (h / ROW).floor().max(0.0) as usize
}

/// Where everything is, the dialog centred in `viewport`.
pub(super) fn lay(ribbon: &Ribbon, dialog: &Customize, viewport: (f32, f32)) -> DialogSlots {
    let (vw, vh) = viewport;
    let w = WIDTH.min((vw - 2.0 * PAD).max(0.0));
    let h = HEIGHT.min((vh - 2.0 * PAD).max(0.0));
    let rect = Rect::new((vw - w) / 2.0, (vh - h) / 2.0, w, h);
    let list_top = rect.y + TITLE_HEIGHT + PAD;
    let list_height = (rect.bottom() - PAD - BUTTON_HEIGHT - PAD - list_top).max(0.0);
    let list_width = ((w - 4.0 * PAD - MIDDLE) / 2.0).max(0.0);
    let commands = Rect::new(rect.x + PAD, list_top, list_width, list_height);
    let middle_x = commands.right() + PAD;
    let tree_box = Rect::new(middle_x + MIDDLE + PAD, list_top, list_width, list_height);
    let shown = rows_in(list_height);

    let command_rows = ribbon
        .catalog()
        .into_iter()
        .skip(dialog.top.0)
        .take(shown)
        .enumerate()
        .map(|(n, c)| {
            (
                c.id,
                Rect::new(commands.x, commands.y + n as f32 * ROW, commands.w, ROW),
            )
        })
        .collect();
    let tree_rows = tree(ribbon)
        .into_iter()
        .skip(dialog.top.1)
        .take(shown)
        .enumerate()
        .map(|(n, (row, _, _))| {
            let indent = match row {
                TreeRow::Tab(_) => 0.0,
                TreeRow::Group(..) => INDENT,
                TreeRow::Command(..) => 2.0 * INDENT,
            };
            let r = Rect::new(tree_box.x, tree_box.y + n as f32 * ROW, tree_box.w, ROW);
            let check = matches!(row, TreeRow::Tab(_))
                .then(|| Rect::new(r.x + 6.0 + indent, r.y + (ROW - CHECK) / 2.0, CHECK, CHECK));
            (row, r, check)
        })
        .collect();

    let middle_top = list_top + (list_height - 2.0 * BUTTON_HEIGHT - PAD) / 2.0;
    let under = rect.bottom() - PAD - BUTTON_HEIGHT;
    let mut buttons = vec![
        (
            Button::Add,
            Rect::new(middle_x, middle_top, MIDDLE, BUTTON_HEIGHT),
        ),
        (
            Button::Remove,
            Rect::new(
                middle_x,
                middle_top + BUTTON_HEIGHT + PAD,
                MIDDLE,
                BUTTON_HEIGHT,
            ),
        ),
    ];
    // Under the tabs, the ones that act on a tab; at the far right, Close.
    let mut x = tree_box.x;
    for button in [Button::Up, Button::Down] {
        buttons.push((button, Rect::new(x, under, 80.0, BUTTON_HEIGHT)));
        x += 80.0 + PAD / 2.0;
    }
    buttons.push((
        Button::Reset,
        Rect::new(rect.x + PAD, under, 130.0, BUTTON_HEIGHT),
    ));
    buttons.push((
        Button::Close,
        Rect::new(rect.right() - PAD - 80.0, under, 80.0, BUTTON_HEIGHT),
    ));
    DialogSlots {
        scrim: Rect::new(0.0, 0.0, vw, vh),
        rect,
        commands,
        command_rows,
        tree: tree_box,
        tree_rows,
        buttons,
    }
}

/// The group a right-hand row names: its own, or its command's.
fn target_group(chosen: Option<&TreeRow>) -> Option<(&str, &str)> {
    match chosen? {
        TreeRow::Group(tab, group) | TreeRow::Command(tab, group, _) => {
            Some((tab.as_str(), group.as_str()))
        }
        TreeRow::Tab(_) => None,
    }
}

/// Whether `button` can be pressed now.
pub(super) fn enabled(ribbon: &Ribbon, dialog: &Customize, button: Button) -> bool {
    match button {
        Button::Add => {
            let (Some(id), Some((tab, group))) =
                (dialog.command, target_group(dialog.chosen.as_ref()))
            else {
                return false;
            };
            ribbon
                .shown_group(tab, group)
                .is_some_and(|g| !g.controls.iter().any(|c| c.command().id == id))
        }
        Button::Remove => matches!(dialog.chosen, Some(TreeRow::Command(..))),
        Button::Up | Button::Down => match &dialog.chosen {
            Some(TreeRow::Tab(id)) => ribbon.move_target(id, button == Button::Up).is_some(),
            _ => false,
        },
        Button::Reset => ribbon.customized(),
        Button::Close => true,
    }
}

impl Ribbon {
    /// Open the dialog for the user's changes, closing whatever else is
    /// open.
    pub fn open_customize(&mut self) {
        self.close();
        self.tips = None;
        self.customize = Some(Customize::new());
    }

    /// Whether the dialog for the user's changes is open.
    #[must_use]
    pub fn customize_open(&self) -> bool {
        self.customize.is_some()
    }

    /// Press `button` of the open dialog.
    fn press_button(&mut self, button: Button) -> RibbonEvent {
        let Some(dialog) = self.customize.clone() else {
            return RibbonEvent::Ignored;
        };
        if !enabled(self, &dialog, button) {
            return RibbonEvent::Handled;
        }
        let changed = match button {
            Button::Add => match (dialog.command, target_group(dialog.chosen.as_ref())) {
                (Some(id), Some((tab, group))) => {
                    let (tab, group) = (tab.to_owned(), group.to_owned());
                    let added = self.add_to_group(&tab, &group, id);
                    if added {
                        self.choose_row(Some(TreeRow::Command(tab, group, id)));
                    }
                    added
                }
                _ => false,
            },
            Button::Remove => match &dialog.chosen {
                Some(TreeRow::Command(tab, group, id)) => {
                    let removed = self.remove_from_group(tab, group, *id);
                    if removed {
                        self.choose_row(Some(TreeRow::Group(tab.clone(), group.clone())));
                    }
                    removed
                }
                _ => false,
            },
            Button::Up | Button::Down => match &dialog.chosen {
                Some(TreeRow::Tab(id)) => self.move_tab(id, button == Button::Up),
                _ => false,
            },
            Button::Reset => {
                self.reset_customization();
                self.choose_row(None);
                true
            }
            Button::Close => {
                self.customize = None;
                return RibbonEvent::Handled;
            }
        };
        if changed {
            RibbonEvent::Customized
        } else {
            RibbonEvent::Handled
        }
    }

    /// Choose `row` on the right, keeping the dialog open -- the methods the
    /// buttons call close what the ribbon has open, which the dialog is not.
    fn choose_row(&mut self, row: Option<TreeRow>) {
        if let Some(dialog) = self.customize.as_mut() {
            dialog.chosen = row;
        }
    }

    /// Show or hide the tab `id`, from its check box.
    fn toggle_tab(&mut self, id: &str) -> RibbonEvent {
        let hidden = self.custom.hidden.contains(id);
        if self.hide_tab(id, !hidden) {
            RibbonEvent::Customized
        } else {
            RibbonEvent::Handled
        }
    }

    /// A pointer event while the dialog is open: all of it the dialog's --
    /// a press past it does nothing, as past any modal dialog.
    pub(super) fn customize_mouse(
        &mut self,
        layout: &super::RibbonLayout,
        event: &MouseEvent,
    ) -> RibbonEvent {
        let Some(slots) = layout.dialog.as_ref() else {
            return RibbonEvent::Handled;
        };
        let (x, y) = (event.x, event.y);
        match &event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some((button, _)) = slots.buttons.iter().find(|(_, r)| r.contains(x, y)) {
                    if let Some(dialog) = self.customize.as_mut() {
                        dialog.pressed = Some(*button);
                    }
                    return RibbonEvent::Handled;
                }
                if let Some((id, _)) = slots.command_rows.iter().find(|(_, r)| r.contains(x, y)) {
                    if let Some(dialog) = self.customize.as_mut() {
                        dialog.command = Some(*id);
                        dialog.focus = Focus::Commands;
                    }
                    return RibbonEvent::Handled;
                }
                if let Some((row, _, check)) =
                    slots.tree_rows.iter().find(|(_, r, _)| r.contains(x, y))
                {
                    let row = row.clone();
                    if let Some(dialog) = self.customize.as_mut() {
                        dialog.chosen = Some(row.clone());
                        dialog.focus = Focus::Tabs;
                    }
                    if let (TreeRow::Tab(id), Some(check)) = (&row, check)
                        && check.contains(x, y)
                    {
                        return self.toggle_tab(id);
                    }
                }
                RibbonEvent::Handled
            }
            MouseEventKind::Release(MouseButton::Left) => {
                let pressed = self.customize.as_mut().and_then(|d| d.pressed.take());
                match pressed {
                    Some(button)
                        if slots
                            .buttons
                            .iter()
                            .any(|(b, r)| *b == button && r.contains(x, y)) =>
                    {
                        self.press_button(button)
                    }
                    _ => RibbonEvent::Handled,
                }
            }
            // A double click on a command adds it, on the left; takes it out
            // of its group, on the right.
            MouseEventKind::DoubleClick(MouseButton::Left) => {
                if slots.command_rows.iter().any(|(_, r)| r.contains(x, y)) {
                    return self.press_button(Button::Add);
                }
                if slots
                    .tree_rows
                    .iter()
                    .any(|(row, r, _)| r.contains(x, y) && matches!(row, TreeRow::Command(..)))
                {
                    return self.press_button(Button::Remove);
                }
                RibbonEvent::Handled
            }
            MouseEventKind::Scroll { dy, .. } => {
                let shown = rows_in(slots.commands.h);
                let over_commands = slots.commands.contains(x, y);
                let total = if over_commands {
                    self.catalog().len()
                } else {
                    tree(self).len()
                };
                if let Some(dialog) = self.customize.as_mut() {
                    let top = if over_commands {
                        &mut dialog.top.0
                    } else {
                        &mut dialog.top.1
                    };
                    let most = total.saturating_sub(shown);
                    *top = if *dy > 0.0 {
                        top.saturating_sub(3)
                    } else {
                        top.saturating_add(3).min(most)
                    };
                }
                RibbonEvent::Handled
            }
            _ => RibbonEvent::Handled,
        }
    }

    /// A key while the dialog is open: the arrows move in the list that has
    /// the keys, Tab moves between the two, Space shows or hides a tab,
    /// Enter adds -- or takes out -- the command chosen, Escape closes.
    pub(super) fn customize_key(
        &mut self,
        layout: &super::RibbonLayout,
        key: &KeyEvent,
    ) -> RibbonEvent {
        let shown = layout
            .dialog
            .as_ref()
            .map_or(1, |slots| rows_in(slots.commands.h).max(1));
        let Some(dialog) = self.customize.clone() else {
            return RibbonEvent::Ignored;
        };
        match key.key {
            Key::Escape => {
                self.customize = None;
                RibbonEvent::Handled
            }
            Key::Tab => {
                if let Some(d) = self.customize.as_mut() {
                    d.focus = match d.focus {
                        Focus::Commands => Focus::Tabs,
                        Focus::Tabs => Focus::Commands,
                    };
                }
                RibbonEvent::Handled
            }
            Key::Up | Key::Down => {
                let down = key.key == Key::Down;
                match dialog.focus {
                    Focus::Commands => {
                        let ids: Vec<CommandId> = self.catalog().iter().map(|c| c.id).collect();
                        let at = dialog
                            .command
                            .and_then(|id| ids.iter().position(|&c| c == id));
                        let next = step(at, ids.len(), down);
                        if let Some(d) = self.customize.as_mut() {
                            d.command = next.and_then(|n| ids.get(n).copied());
                            if let Some(n) = next {
                                d.top.0 = reveal(d.top.0, n, shown);
                            }
                        }
                    }
                    Focus::Tabs => {
                        let rows: Vec<TreeRow> =
                            tree(self).into_iter().map(|(r, _, _)| r).collect();
                        let at = dialog
                            .chosen
                            .as_ref()
                            .and_then(|c| rows.iter().position(|r| r == c));
                        let next = step(at, rows.len(), down);
                        if let Some(d) = self.customize.as_mut() {
                            d.chosen = next.and_then(|n| rows.get(n).cloned());
                            if let Some(n) = next {
                                d.top.1 = reveal(d.top.1, n, shown);
                            }
                        }
                    }
                }
                RibbonEvent::Handled
            }
            Key::Space => match &dialog.chosen {
                Some(TreeRow::Tab(id)) if dialog.focus == Focus::Tabs => self.toggle_tab(id),
                _ => RibbonEvent::Handled,
            },
            Key::Enter => match dialog.focus {
                Focus::Commands => self.press_button(Button::Add),
                Focus::Tabs => self.press_button(Button::Remove),
            },
            _ => RibbonEvent::Handled,
        }
    }
}

/// The row after -- or before -- `at` in a list of `count`, held to its
/// ends; the first row when nothing was chosen.
fn step(at: Option<usize>, count: usize, down: bool) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(match at {
        None => 0,
        Some(at) if down => at.saturating_add(1).min(last),
        Some(at) => at.saturating_sub(1),
    })
}

/// The first row to show so that row `row` is in view, `shown` rows at a
/// time, from `top`.
fn reveal(top: usize, row: usize, shown: usize) -> usize {
    if row < top {
        row
    } else if row >= top.saturating_add(shown) {
        row.saturating_add(1).saturating_sub(shown)
    } else {
        top
    }
}

/// Draw the dialog over the window: the scrim, the dialog, its two lists and
/// its buttons.
pub(super) fn draw<S: CommandSink + ?Sized>(
    sink: &mut S,
    p: &Palette,
    ribbon: &Ribbon,
    dialog: &Customize,
    slots: &DialogSlots,
) {
    fill(sink, slots.scrim, p.scrim(), CornerRadii::ZERO);
    sink.emit(RenderCommand::BoxShadow {
        x: slots.rect.x,
        y: slots.rect.y,
        width: slots.rect.w,
        height: slots.rect.h,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 16.0,
        spread: 0.0,
        color: p.shadow(),
        corner_radii: CornerRadii::all(6.0),
    });
    fill(sink, slots.rect, p.base, CornerRadii::all(6.0));
    stroke(sink, slots.rect, p.border, 1.0);
    sink.emit(RenderCommand::Text {
        x: slots.rect.x + PAD,
        y: text_top(slots.rect.y, TITLE_HEIGHT + PAD / 2.0, SIZE + 2.0),
        text: "Customize the ribbon".to_owned(),
        color: p.text,
        font_size: SIZE + 2.0,
        font_weight: FontWeightHint::Bold,
        max_width: Some(slots.rect.w - 2.0 * PAD),
        overflow: TextOverflow::Ellipsis,
    });

    for list in [slots.commands, slots.tree] {
        fill(sink, list, p.mantle, CornerRadii::ZERO);
        stroke(sink, list, p.border, 1.0);
    }
    let catalog = ribbon.catalog();
    for (id, r) in &slots.command_rows {
        let Some(command) = catalog.iter().find(|c| c.id == *id) else {
            continue;
        };
        if dialog.command == Some(*id) {
            chosen_ground(sink, p, *r, dialog.focus == Focus::Commands);
        }
        row_text(sink, r.x + 6.0, *r, &command.label, p.text, false);
    }
    let labels: Vec<(TreeRow, String, bool)> = tree(ribbon);
    for (row, r, check) in &slots.tree_rows {
        let Some((_, label, hidden)) = labels.iter().find(|(t, _, _)| t == row) else {
            continue;
        };
        if dialog.chosen.as_ref() == Some(row) {
            chosen_ground(sink, p, *r, dialog.focus == Focus::Tabs);
        }
        let (indent, bold) = match row {
            TreeRow::Tab(_) => (0.0, true),
            TreeRow::Group(..) => (INDENT, true),
            TreeRow::Command(..) => (2.0 * INDENT, false),
        };
        let mut x = r.x + 6.0 + indent;
        if let Some(check) = check {
            stroke(sink, *check, p.border, 1.0);
            if !hidden {
                fill(sink, *check, p.accent, CornerRadii::all(2.0));
                // A tick: two strokes.
                line(
                    sink,
                    check.x + 2.5,
                    check.y + 6.0,
                    check.x + 5.0,
                    check.y + 9.0,
                    p.on_accent(),
                );
                line(
                    sink,
                    check.x + 5.0,
                    check.y + 9.0,
                    check.x + 9.5,
                    check.y + 3.0,
                    p.on_accent(),
                );
            }
            x = check.right() + 6.0;
        }
        let ink = if *hidden { p.subtext0 } else { p.text };
        row_text(sink, x, *r, label, ink, bold);
    }

    for (button, r) in &slots.buttons {
        let on = enabled(ribbon, dialog, *button);
        let lit = on && dialog.pressed == Some(*button);
        fill(
            sink,
            *r,
            if lit { p.surface2 } else { p.surface0 },
            CornerRadii::all(4.0),
        );
        stroke(sink, *r, p.border, 1.0);
        let label = button.label();
        let w = text::measure(label, SIZE, FontWeightHint::Regular);
        sink.emit(RenderCommand::Text {
            x: r.x + ((r.w - w) / 2.0).max(4.0),
            y: text_top(r.y, r.h, SIZE),
            text: label.to_owned(),
            color: if on { p.text } else { p.overlay0 },
            font_size: SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: Some(r.w - 8.0),
            overflow: TextOverflow::Ellipsis,
        });
    }
}

/// The ground of a chosen row: the selection's, stronger in the list that
/// has the keys.
fn chosen_ground<S: CommandSink + ?Sized>(sink: &mut S, p: &Palette, r: Rect, focused: bool) {
    fill(sink, r, p.selection_fill(), CornerRadii::ZERO);
    if focused {
        stroke(sink, r, p.selection_border(), 1.0);
    }
}

fn row_text<S: CommandSink + ?Sized>(
    sink: &mut S,
    x: f32,
    r: Rect,
    words: &str,
    color: crate::color::Color,
    bold: bool,
) {
    sink.emit(RenderCommand::Text {
        x,
        y: text_top(r.y, r.h, SIZE),
        text: words.to_owned(),
        color,
        font_size: SIZE,
        font_weight: if bold {
            FontWeightHint::Bold
        } else {
            FontWeightHint::Regular
        },
        max_width: Some((r.right() - x - 4.0).max(1.0)),
        overflow: TextOverflow::Ellipsis,
    });
}

#[cfg(test)]
pub(super) fn tree_for_tests(ribbon: &Ribbon) -> Vec<TreeRow> {
    tree(ribbon).into_iter().map(|(row, _, _)| row).collect()
}
