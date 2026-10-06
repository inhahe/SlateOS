//! The overview as tools see it -- held among the shell's own parts by
//! `crate::accessible`, which presses them as the user would.
//!
//! The search field, holding what is typed; the windows, each card named by
//! its window's title, said to be focused, minimised or left out by the
//! search, lit as the arrow keys and the pointer light it, and holding its
//! close button; and, while every desktop shows, each desktop's lane, named
//! by the desktop, said to be the current one, chosen while the wheel has
//! chosen it, and holding its windows' cards.
//!
//! Every box is the one the overview draws and takes a press in
//! ([`hit_test`]): a press at a part's [`press_point`] presses that part.

use guitk::widget::automation::{Node, Role, Value};

use super::{
    LaneLayout, OverviewConfig, OverviewHit, OverviewMode, OverviewState, ThumbnailLayout,
    close_rect, content_area, hit_test, lane_layout, overview_layout, search_bar_rect,
};
use crate::Rect;

/// A part of the overview, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverviewPart {
    /// The overview.
    Overview,
    /// Its search field: what is typed in it finds windows by their titles.
    Search,
    /// The windows it shows -- or, while every desktop shows, the desktops.
    Windows,
    /// A desktop's lane, by the desktop's number: pressed off its cards, a
    /// switch to the desktop.
    Lane(u32),
    /// A window's card, by the window's id: pressed, a switch to the window.
    Card(u64),
    /// A window's close button, by the window's id: drawn while the pointer
    /// is on its card.
    Close(u64),
}

/// The overview as tools see it on a `screen_w` × `screen_h` display.
#[must_use]
pub fn automation(
    state: &OverviewState,
    config: &OverviewConfig,
    screen_w: f32,
    screen_h: f32,
) -> Node<OverviewPart> {
    let mut root = Node::new(
        OverviewPart::Overview,
        Role::Dialog,
        "Overview",
        Rect::new(0.0, 0.0, screen_w, screen_h),
    );
    root.description = Some(
        match state.mode {
            OverviewMode::AllWindows => "this desktop's windows",
            OverviewMode::AllDesktops => "every desktop",
            OverviewMode::RecentApps => "recent windows",
        }
        .to_owned(),
    );
    root.shown = state.visible;
    let mut search = Node::new(
        OverviewPart::Search,
        Role::TextField,
        "Search windows",
        search_bar_rect(screen_w),
    );
    search.value = Some(Value::Text(state.search_query.clone()));
    // Typing goes to it whenever the overview is open.
    search.focused = true;
    search.focusable = true;
    root.children.push(search);

    let layouts = overview_layout(state, config, screen_w, screen_h);
    let (bx, by, bw, bh) = content_area(screen_w, screen_h);
    let area = Rect::new(bx, by, bw, bh);
    let windows = if state.mode == OverviewMode::AllDesktops {
        let mut desktops = Node::new(OverviewPart::Windows, Role::List, "Desktops", area);
        let lanes = lane_layout(state, config, screen_w, screen_h);
        for lane in &state.lanes {
            let Some(rect) = lanes
                .iter()
                .find(|placed| placed.desktop_id == lane.desktop_id)
                .map(|placed| placed.rect)
            else {
                continue;
            };
            let mut node = Node::new(
                OverviewPart::Lane(lane.desktop_id),
                Role::ListItem,
                lane.name.clone(),
                rect,
            );
            node.description = lane.is_current.then(|| "the current desktop".to_owned());
            node.value = Some(Value::Chosen(
                state.selected_desktop == Some(lane.desktop_id),
            ));
            node.children.extend(
                layouts
                    .iter()
                    .filter(|layout| layout.desktop_id == lane.desktop_id)
                    .map(|layout| card_node(state, layout)),
            );
            desktops.children.push(node);
        }
        desktops
    } else {
        let mut list = Node::new(OverviewPart::Windows, Role::List, "Windows", area);
        list.children
            .extend(layouts.iter().map(|layout| card_node(state, layout)));
        list
    };
    root.children.push(windows);
    root
}

/// A window's card, and its close button.
fn card_node(state: &OverviewState, layout: &ThumbnailLayout) -> Node<OverviewPart> {
    let lit = state.hovered_window == Some(layout.window_id);
    let mut card = Node::new(
        OverviewPart::Card(layout.window_id),
        Role::ListItem,
        layout.title.clone(),
        layout.card_rect(lit),
    );
    let mut said = Vec::new();
    if layout.is_focused {
        said.push("focused");
    }
    if layout.is_minimized {
        said.push("minimised");
    }
    if !state.search_query.is_empty() && !state.search_results.contains(&layout.window_id) {
        said.push("not found by the search");
    }
    card.description = (!said.is_empty()).then(|| said.join(", "));
    card.value = Some(Value::Chosen(lit));
    card.focused = lit;
    card.focusable = true;
    // Where it is drawn once the card is lit: a press on it lights the card
    // first, as the pointer reaching it does.
    card.children.push(Node::new(
        OverviewPart::Close(layout.window_id),
        Role::Button,
        "Close",
        close_rect(layout.card_rect(true)),
    ));
    card
}

/// Where a press lands on `part` -- a card, its close button (which takes a
/// press only while its card is lit), or a lane off its cards -- on a
/// `screen_w` × `screen_h` display: `None` for a part not shown, or one with
/// nowhere a press would reach it.
#[must_use]
pub fn press_point(
    state: &OverviewState,
    config: &OverviewConfig,
    screen_w: f32,
    screen_h: f32,
    part: OverviewPart,
) -> Option<(f32, f32)> {
    if !state.visible {
        return None;
    }
    let layouts = overview_layout(state, config, screen_w, screen_h);
    let lanes = lane_layout(state, config, screen_w, screen_h);
    let reaches = |(x, y): (f32, f32), wanted: OverviewHit| {
        (hit_test(state, &layouts, &lanes, x, y) == Some(wanted)).then_some((x, y))
    };
    match part {
        OverviewPart::Card(id) => {
            let layout = layouts.iter().find(|layout| layout.window_id == id)?;
            let lit = state.hovered_window == Some(id);
            reaches(layout.card_rect(lit).centre(), OverviewHit::Card(id))
        }
        OverviewPart::Close(id) => {
            let layout = layouts.iter().find(|layout| layout.window_id == id)?;
            reaches(
                close_rect(layout.card_rect(true)).centre(),
                OverviewHit::Close(id),
            )
        }
        OverviewPart::Lane(desktop) => {
            let lane: &LaneLayout = lanes.iter().find(|lane| lane.desktop_id == desktop)?;
            // Its top-left corner, where its label row begins: no card is
            // drawn there.
            reaches(
                (lane.rect.x + 1.0, lane.rect.y + 1.0),
                OverviewHit::Lane(desktop),
            )
        }
        OverviewPart::Overview | OverviewPart::Search | OverviewPart::Windows => None,
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
