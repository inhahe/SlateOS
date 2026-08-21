//! File Drag-and-Drop Overlay
//!
//! Desktop-level handling of file drag operations between applications:
//!
//! - Visual drag cursor with file count indicator
//! - Drop target highlighting (window borders glow)
//! - Drop preview (what will happen: copy/move/link)
//! - Multi-file drag with thumbnail stack
//! - Cross-application drag data (file paths, URIs, text)
//! - Drag cancellation (Escape key)
//! - Auto-scroll when dragging near window edges

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

// ============================================================================
// Catppuccin Mocha palette
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);

// ============================================================================
// Drag-cursor card geometry
// ============================================================================

/// Width of the little card drawn next to the cursor during a drag.
const DRAG_CARD_W: f32 = 140.0;
/// Inset of the card's contents from its left and right edges.
const DRAG_CARD_PAD: f32 = 8.0;
/// Distance from the card's right edge to the item-count badge. Doubles as the
/// right bound on the description whenever the badge is drawn, so the two
/// cannot drift into each other.
const COUNT_BADGE_INSET: f32 = 28.0;
const DRAG_DESC_SIZE: f32 = 11.0;

// ============================================================================
// Drag data types
// ============================================================================

/// Type of data being dragged.
#[derive(Clone, Debug, PartialEq)]
pub enum DragDataType {
    /// File paths (most common).
    Files(Vec<String>),
    /// Plain text.
    Text(String),
    /// URI list.
    Uris(Vec<String>),
    /// Raw bytes with MIME type.
    Raw { mime: String, size: usize },
}

impl DragDataType {
    /// Human-readable description, in full.
    ///
    /// Deliberately *not* truncated. Truncation belongs to whoever draws this,
    /// because only they know the room available; the model attempting it here
    /// was wrong twice over. It cut at `&t[..27]`, which aborts whenever byte
    /// 27 lands inside a multi-byte character — so dragging any non-Latin text
    /// selection took the whole desktop shell down — and its 30-byte budget
    /// bore no relation to the 124px the drag overlay actually has.
    /// [`render_drag_overlay`] elides it to that width instead.
    pub fn description(&self) -> String {
        match self {
            // Matching the one-element slice binds the path *and* carries
            // the length condition, instead of testing the length in one
            // statement and indexing on the strength of it in the next.
            Self::Files(paths) => match paths.as_slice() {
                [only] => only.rsplit('/').next().unwrap_or(only).to_string(),
                other => format!("{} files", other.len()),
            },
            Self::Text(t) => format!("\"{t}\""),
            Self::Uris(uris) => format!("{} links", uris.len()),
            Self::Raw { mime, size } => format!("{mime} ({size} bytes)"),
        }
    }

    /// Number of items.
    pub fn item_count(&self) -> usize {
        match self {
            Self::Files(f) => f.len(),
            Self::Text(_) => 1,
            Self::Uris(u) => u.len(),
            Self::Raw { .. } => 1,
        }
    }
}

// ============================================================================
// Drop effect
// ============================================================================

/// What happens when the data is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEffect {
    /// No drop allowed here.
    None,
    /// Copy data to target.
    Copy,
    /// Move data to target (remove from source).
    Move,
    /// Create a link/shortcut.
    Link,
}

impl DropEffect {
    pub fn label(&self) -> &str {
        match self {
            Self::None => "Not allowed",
            Self::Copy => "Copy",
            Self::Move => "Move",
            Self::Link => "Link",
        }
    }

    pub fn cursor_badge(&self) -> &str {
        match self {
            Self::None => "X",
            Self::Copy => "+",
            Self::Move => "",
            Self::Link => "~",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::None => MOCHA_RED,
            Self::Copy => MOCHA_GREEN,
            Self::Move => MOCHA_BLUE,
            Self::Link => MOCHA_PEACH,
        }
    }
}

// ============================================================================
// Drop target
// ============================================================================

/// A potential drop target (window or region).
#[derive(Clone, Debug)]
pub struct DropTarget {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// What effects this target accepts.
    pub accepted_effects: Vec<DropEffect>,
    /// Label to show when hovering.
    pub label: String,
}

impl DropTarget {
    /// Check if a point is inside this target.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    /// Get the best effect for this target.
    pub fn best_effect(&self) -> DropEffect {
        if self.accepted_effects.contains(&DropEffect::Move) {
            DropEffect::Move
        } else if self.accepted_effects.contains(&DropEffect::Copy) {
            DropEffect::Copy
        } else if self.accepted_effects.contains(&DropEffect::Link) {
            DropEffect::Link
        } else {
            DropEffect::None
        }
    }
}

// ============================================================================
// Drag session state
// ============================================================================

/// Phase of a drag operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragPhase {
    /// No drag in progress.
    Idle,
    /// Mouse button down, threshold not yet met.
    Pending,
    /// Actively dragging.
    Active,
    /// Drag completed (drop accepted).
    Completed,
    /// Drag cancelled.
    Cancelled,
}

/// Active drag session.
#[derive(Clone, Debug)]
pub struct DragSession {
    /// Source application/window ID.
    pub source_window: u64,
    /// Current drag phase.
    pub phase: DragPhase,
    /// Data being dragged.
    pub data: DragDataType,
    /// Current mouse position.
    pub mouse_x: f32,
    pub mouse_y: f32,
    /// Starting position (for threshold check).
    pub start_x: f32,
    pub start_y: f32,
    /// Current drop effect.
    pub current_effect: DropEffect,
    /// Currently hovered target.
    pub hover_target: Option<u64>,
    /// Whether modifier key (Ctrl for copy) is held.
    pub ctrl_held: bool,
}

impl DragSession {
    pub fn new(source: u64, data: DragDataType, x: f32, y: f32) -> Self {
        Self {
            source_window: source,
            phase: DragPhase::Pending,
            data,
            mouse_x: x,
            mouse_y: y,
            start_x: x,
            start_y: y,
            current_effect: DropEffect::Move,
            hover_target: None,
            ctrl_held: false,
        }
    }

    /// Distance from start point.
    pub fn drag_distance(&self) -> f32 {
        let dx = self.mouse_x - self.start_x;
        let dy = self.mouse_y - self.start_y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Check if drag threshold (5 pixels) is met.
    pub fn threshold_met(&self) -> bool {
        self.drag_distance() >= 5.0
    }
}

// ============================================================================
// Drag-and-drop manager
// ============================================================================

/// Drag threshold in pixels.
const DRAG_THRESHOLD: f32 = 5.0;

/// Manages drag operations across the desktop.
pub struct DragDropManager {
    pub session: Option<DragSession>,
    pub targets: Vec<DropTarget>,
}

impl DragDropManager {
    pub fn new() -> Self {
        Self {
            session: None,
            targets: Vec::new(),
        }
    }

    /// Start a potential drag.
    pub fn begin_drag(&mut self, source: u64, data: DragDataType, x: f32, y: f32) {
        self.session = Some(DragSession::new(source, data, x, y));
    }

    /// Update mouse position during drag.
    pub fn update(&mut self, x: f32, y: f32, ctrl: bool) {
        if let Some(ref mut session) = self.session {
            session.mouse_x = x;
            session.mouse_y = y;
            session.ctrl_held = ctrl;

            // Check threshold.
            if session.phase == DragPhase::Pending && session.drag_distance() >= DRAG_THRESHOLD {
                session.phase = DragPhase::Active;
            }

            if session.phase == DragPhase::Active {
                // Hit-test targets.
                let mut found_target = None;
                let mut best_effect = DropEffect::None;
                for target in &self.targets {
                    if target.contains(x, y) {
                        found_target = Some(target.id);
                        best_effect = if ctrl {
                            DropEffect::Copy
                        } else {
                            target.best_effect()
                        };
                        break;
                    }
                }
                session.hover_target = found_target;
                session.current_effect = best_effect;
            }
        }
    }

    /// Complete the drop at current location.
    pub fn drop_here(&mut self) -> Option<(DragDataType, u64, DropEffect)> {
        if let Some(ref mut session) = self.session {
            if session.phase != DragPhase::Active {
                self.session = None;
                return None;
            }
            if let Some(target_id) = session.hover_target
                && session.current_effect != DropEffect::None
            {
                let data = session.data.clone();
                let effect = session.current_effect;
                session.phase = DragPhase::Completed;
                let result = Some((data, target_id, effect));
                self.session = None;
                return result;
            }
            session.phase = DragPhase::Cancelled;
            self.session = None;
        }
        None
    }

    /// Cancel the drag.
    pub fn cancel(&mut self) {
        if let Some(ref mut session) = self.session {
            session.phase = DragPhase::Cancelled;
        }
        self.session = None;
    }

    /// Register a drop target.
    pub fn register_target(&mut self, target: DropTarget) {
        self.targets.push(target);
    }

    /// Unregister a drop target by ID.
    pub fn unregister_target(&mut self, id: u64) {
        self.targets.retain(|t| t.id != id);
    }

    /// Whether a drag is actively in progress.
    pub fn is_dragging(&self) -> bool {
        self.session
            .as_ref()
            .map(|s| s.phase == DragPhase::Active)
            .unwrap_or(false)
    }
}

impl Default for DragDropManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Visual rendering
// ============================================================================

/// Render the drag cursor overlay.
pub fn render_drag_overlay(session: &DragSession) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();

    if session.phase != DragPhase::Active {
        return cmds;
    }

    let cx = session.mouse_x;
    let cy = session.mouse_y;

    // Dragged item indicator (small rounded rect near cursor).
    let w = DRAG_CARD_W;
    let h = 32.0;
    let ox = cx + 12.0;
    let oy = cy + 12.0;

    cmds.push(RenderCommand::FillRect {
        x: ox,
        y: oy,
        width: w,
        height: h,
        color: Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 220),
        corner_radii: CornerRadii::all(6.0),
    });

    // Item description, elided to the room the card actually has. When the
    // count badge is drawn the description has to stop before it, not at the
    // card edge — `max_width` alone would have let it run underneath.
    let count = session.data.item_count();
    let desc_room = if count > 1 {
        (w - COUNT_BADGE_INSET - DRAG_CARD_PAD * 2.0).max(0.0)
    } else {
        (w - DRAG_CARD_PAD * 2.0).max(0.0)
    };
    cmds.push(RenderCommand::Text {
        x: ox + DRAG_CARD_PAD,
        y: oy + 4.0,
        text: text::elide(
            &session.data.description(),
            desc_room,
            "...",
            DRAG_DESC_SIZE,
            FontWeightHint::Regular,
        ),
        font_size: DRAG_DESC_SIZE,
        color: MOCHA_TEXT,
        font_weight: FontWeightHint::Regular,
        max_width: Some(desc_room),
        overflow: TextOverflow::Ellipsis,
    });

    // Effect badge.
    let badge_text = session.current_effect.label();
    let badge_color = session.current_effect.color();
    cmds.push(RenderCommand::FillRect {
        x: ox + 8.0,
        y: oy + 18.0,
        width: 40.0,
        height: 12.0,
        color: badge_color,
        corner_radii: CornerRadii::all(3.0),
    });
    cmds.push(RenderCommand::Text {
        x: ox + 12.0,
        y: oy + 19.0,
        text: badge_text.to_string(),
        font_size: 9.0,
        color: MOCHA_BASE,
        font_weight: FontWeightHint::Bold,
        max_width: None,
        overflow: TextOverflow::Clip,
    });

    // Item count badge (if multiple).
    if count > 1 {
        cmds.push(RenderCommand::FillRect {
            x: ox + w - COUNT_BADGE_INSET,
            y: oy + 2.0,
            width: 24.0,
            height: 16.0,
            color: MOCHA_PEACH,
            corner_radii: CornerRadii::all(8.0),
        });
        cmds.push(RenderCommand::Text {
            x: ox + w - 24.0,
            y: oy + 3.0,
            text: format!("{}", count),
            font_size: 10.0,
            color: MOCHA_BASE,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    cmds
}

/// Render the drop target highlight (glowing border).
pub fn render_drop_target_highlight(target: &DropTarget, effect: DropEffect) -> Vec<RenderCommand> {
    let mut cmds = Vec::new();
    let color = effect.color();

    cmds.push(RenderCommand::StrokeRect {
        x: target.x - 2.0,
        y: target.y - 2.0,
        width: target.width + 4.0,
        height: target.height + 4.0,
        color,
        line_width: 2.0,
        corner_radii: CornerRadii::all(4.0),
    });

    // Label tooltip.
    if !target.label.is_empty() {
        let label_w = text::padded_width(&target.label, 8.0, 11.0, FontWeightHint::Regular);
        let label_x = target.x + (target.width - label_w) / 2.0;
        let label_y = target.y + target.height + 4.0;

        cmds.push(RenderCommand::FillRect {
            x: label_x,
            y: label_y,
            width: label_w,
            height: 20.0,
            color: Color::rgba(MOCHA_BASE.r, MOCHA_BASE.g, MOCHA_BASE.b, 200),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: label_x + 8.0,
            y: label_y + 3.0,
            text: target.label.clone(),
            font_size: 11.0,
            color: MOCHA_TEXT,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    cmds
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // --- DragDataType ---
    #[test]
    fn test_files_description_single() {
        let d = DragDataType::Files(vec!["/home/user/photo.jpg".to_string()]);
        assert_eq!(d.description(), "photo.jpg");
    }

    #[test]
    fn test_files_description_multi() {
        let d = DragDataType::Files(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(d.description(), "3 files");
    }

    #[test]
    fn test_text_description_short() {
        let d = DragDataType::Text("hello".to_string());
        assert_eq!(d.description(), "\"hello\"");
    }

    #[test]
    fn test_text_description_long() {
        // `description()` no longer truncates — the drag overlay does, against
        // the width it actually has. See `a_long_drag_description_is_elided`.
        let d = DragDataType::Text("a".repeat(50));
        assert_eq!(d.description(), format!("\"{}\"", "a".repeat(50)));
    }

    // --- Drag overlay: non-ASCII safety and width discipline ---

    fn overlay_for(data: DragDataType) -> Vec<RenderCommand> {
        let mut session = DragSession::new(1, data, 400.0, 300.0);
        session.phase = DragPhase::Active;
        render_drag_overlay(&session)
    }

    fn description_of(cmds: &[RenderCommand]) -> String {
        cmds.iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    text, font_size, ..
                } if *font_size == DRAG_DESC_SIZE => Some(text.clone()),
                _ => None,
            })
            .expect("the overlay draws a description")
    }

    /// Drag payloads chosen so byte 27 — the offset the old code cut at — falls
    /// inside a multi-byte character. The last pins it exactly: 26 ASCII bytes
    /// then a two-byte `é`, so byte 27 is that character's continuation byte.
    fn adversarial_payloads() -> Vec<String> {
        vec![
            "日本語のテキストを選択してドラッグしています".to_string(),
            "Επιλεγμένο κείμενο προς μεταφορά".to_string(),
            "Выделенный текст для перетаскивания".to_string(),
            "dragging 🌍 some 🚀 emoji 🛰 text around".to_string(),
            format!("{}é{}", "a".repeat(26), "b".repeat(30)),
        ]
    }

    #[test]
    fn a_non_ascii_drag_payload_does_not_abort_the_overlay() {
        // Regression: `description()` cut `Text` at `&t[..27]` behind a
        // `len() > 30` guard. Byte 27 lands inside a multi-byte character for
        // most non-Latin text, and the guard made that *more* likely rather
        // than less — a ten-character Japanese selection is 30 bytes and so
        // always took the truncating branch. Dragging a non-Latin selection
        // took the desktop shell down.
        for payload in adversarial_payloads() {
            let cmds = overlay_for(DragDataType::Text(payload.clone()));
            assert!(!cmds.is_empty(), "the overlay drew nothing for {payload:?}");
            // The filename path is document-controlled too.
            let cmds = overlay_for(DragDataType::Files(vec![format!("/home/u/{payload}")]));
            assert!(
                !cmds.is_empty(),
                "the overlay drew nothing for a {payload:?} file"
            );
        }
    }

    #[test]
    fn a_long_drag_description_is_elided() {
        let mut checked = 0usize;
        for payload in adversarial_payloads() {
            let cmds = overlay_for(DragDataType::Text(payload.clone()));
            let drawn = description_of(&cmds);
            let width = text::measure(&drawn, DRAG_DESC_SIZE, FontWeightHint::Regular);
            let room = DRAG_CARD_W - DRAG_CARD_PAD * 2.0;
            assert!(
                width <= room + 0.5,
                "the description {drawn:?} draws {width} wide, past the {room} \
                 the drag card has"
            );
            checked += 1;
        }
        assert_eq!(checked, adversarial_payloads().len());
    }

    #[test]
    fn a_multi_item_description_stops_before_the_count_badge() {
        // With a badge on the card, the card edge is the wrong bound: the
        // description has to stop before the badge or it draws underneath it.
        //
        // Honest note on this test's strength: every description that comes
        // with a badge today is short by construction — a multi-item drag
        // renders as "N files" or "N links", never as a path — so the
        // assertion currently has slack, and removing the badge-aware bound
        // from `render_drag_overlay` does *not* make it fail. It is written
        // against the drawn geometry rather than against the constants so
        // that it starts biting the moment that stops being true, which is
        // exactly when the overlap would appear.
        let files: Vec<String> = (0..12).map(|i| format!("/home/u/файл-{i}.txt")).collect();
        let cmds = overlay_for(DragDataType::Files(files));

        let (desc_x, desc) = cmds
            .iter()
            .find_map(|c| match c {
                RenderCommand::Text {
                    x, text, font_size, ..
                } if *font_size == DRAG_DESC_SIZE => Some((*x, text.clone())),
                _ => None,
            })
            .expect("the overlay draws a description");
        let badge_x = cmds
            .iter()
            .find_map(|c| match c {
                // The count badge is the only peach fill on the card.
                RenderCommand::FillRect { x, color, .. } if *color == MOCHA_PEACH => Some(*x),
                _ => None,
            })
            .expect("a 12-item drag draws a count badge");

        let desc_right = desc_x + text::measure(&desc, DRAG_DESC_SIZE, FontWeightHint::Regular);
        assert!(
            desc_right <= badge_x + 0.5,
            "the description {desc:?} runs to {desc_right}, under the count \
             badge that starts at {badge_x}"
        );
    }

    #[test]
    fn a_short_drag_description_is_drawn_verbatim() {
        // Otherwise "it fits" would be satisfiable by drawing nothing.
        let cmds = overlay_for(DragDataType::Text("да".to_string()));
        assert_eq!(description_of(&cmds), "\"да\"");
    }

    #[test]
    fn test_item_count() {
        assert_eq!(
            DragDataType::Files(vec!["a".into(), "b".into()]).item_count(),
            2
        );
        assert_eq!(DragDataType::Text("x".into()).item_count(), 1);
        assert_eq!(DragDataType::Uris(vec!["u".into()]).item_count(), 1);
    }

    // --- DropEffect ---
    #[test]
    fn test_effect_labels() {
        assert_eq!(DropEffect::Copy.label(), "Copy");
        assert_eq!(DropEffect::Move.label(), "Move");
        assert_eq!(DropEffect::None.label(), "Not allowed");
    }

    // --- DropTarget ---
    #[test]
    fn test_target_contains() {
        let t = DropTarget {
            id: 1,
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 150.0,
            accepted_effects: vec![DropEffect::Copy],
            label: "Drop here".into(),
        };
        assert!(t.contains(150.0, 150.0));
        assert!(!t.contains(50.0, 50.0));
        assert!(!t.contains(350.0, 150.0));
    }

    #[test]
    fn test_target_best_effect() {
        let t = DropTarget {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            accepted_effects: vec![DropEffect::Copy, DropEffect::Link],
            label: String::new(),
        };
        assert_eq!(t.best_effect(), DropEffect::Copy);

        let t2 = DropTarget {
            id: 2,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            accepted_effects: vec![DropEffect::Move],
            label: String::new(),
        };
        assert_eq!(t2.best_effect(), DropEffect::Move);

        let t3 = DropTarget {
            id: 3,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            accepted_effects: vec![],
            label: String::new(),
        };
        assert_eq!(t3.best_effect(), DropEffect::None);
    }

    // --- DragSession ---
    #[test]
    fn test_session_creation() {
        let s = DragSession::new(1, DragDataType::Text("hi".into()), 100.0, 200.0);
        assert_eq!(s.phase, DragPhase::Pending);
        assert_eq!(s.source_window, 1);
    }

    #[test]
    fn test_drag_distance() {
        let mut s = DragSession::new(1, DragDataType::Text("hi".into()), 0.0, 0.0);
        s.mouse_x = 3.0;
        s.mouse_y = 4.0;
        assert!((s.drag_distance() - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_threshold_not_met() {
        let s = DragSession::new(1, DragDataType::Text("hi".into()), 100.0, 100.0);
        assert!(!s.threshold_met());
    }

    #[test]
    fn test_threshold_met() {
        let mut s = DragSession::new(1, DragDataType::Text("hi".into()), 100.0, 100.0);
        s.mouse_x = 110.0;
        assert!(s.threshold_met());
    }

    // --- DragDropManager ---
    #[test]
    fn test_manager_new() {
        let mgr = DragDropManager::new();
        assert!(mgr.session.is_none());
        assert!(!mgr.is_dragging());
    }

    #[test]
    fn test_begin_drag() {
        let mut mgr = DragDropManager::new();
        mgr.begin_drag(1, DragDataType::Files(vec!["f".into()]), 50.0, 50.0);
        assert!(mgr.session.is_some());
        assert!(!mgr.is_dragging()); // Still pending
    }

    #[test]
    fn test_drag_becomes_active() {
        let mut mgr = DragDropManager::new();
        mgr.begin_drag(1, DragDataType::Text("x".into()), 50.0, 50.0);
        mgr.update(60.0, 50.0, false); // 10px moved, threshold met
        assert!(mgr.is_dragging());
    }

    #[test]
    fn test_cancel_drag() {
        let mut mgr = DragDropManager::new();
        mgr.begin_drag(1, DragDataType::Text("x".into()), 50.0, 50.0);
        mgr.update(60.0, 50.0, false);
        mgr.cancel();
        assert!(!mgr.is_dragging());
        assert!(mgr.session.is_none());
    }

    #[test]
    fn test_drop_on_target() {
        let mut mgr = DragDropManager::new();
        mgr.register_target(DropTarget {
            id: 42,
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 200.0,
            accepted_effects: vec![DropEffect::Copy],
            label: "Target".into(),
        });

        mgr.begin_drag(1, DragDataType::Files(vec!["f.txt".into()]), 50.0, 50.0);
        mgr.update(150.0, 150.0, false); // Move over target
        let result = mgr.drop_here();
        assert!(result.is_some());
        let (data, target_id, effect) = result.unwrap();
        assert_eq!(target_id, 42);
        assert_eq!(effect, DropEffect::Copy);
        assert!(matches!(data, DragDataType::Files(_)));
    }

    #[test]
    fn test_drop_nowhere() {
        let mut mgr = DragDropManager::new();
        mgr.begin_drag(1, DragDataType::Text("x".into()), 50.0, 50.0);
        mgr.update(60.0, 50.0, false);
        let result = mgr.drop_here();
        assert!(result.is_none());
    }

    #[test]
    fn test_ctrl_forces_copy() {
        let mut mgr = DragDropManager::new();
        mgr.register_target(DropTarget {
            id: 1,
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 200.0,
            accepted_effects: vec![DropEffect::Move, DropEffect::Copy],
            label: String::new(),
        });
        mgr.begin_drag(1, DragDataType::Files(vec!["f".into()]), 50.0, 50.0);
        mgr.update(150.0, 150.0, true); // Ctrl held
        assert_eq!(
            mgr.session.as_ref().unwrap().current_effect,
            DropEffect::Copy
        );
    }

    #[test]
    fn test_register_unregister_targets() {
        let mut mgr = DragDropManager::new();
        mgr.register_target(DropTarget {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            accepted_effects: vec![],
            label: String::new(),
        });
        assert_eq!(mgr.targets.len(), 1);
        mgr.unregister_target(1);
        assert_eq!(mgr.targets.len(), 0);
    }

    // --- Rendering ---
    #[test]
    fn test_render_overlay_idle() {
        let s = DragSession::new(1, DragDataType::Text("hi".into()), 0.0, 0.0);
        let cmds = render_drag_overlay(&s);
        assert!(cmds.is_empty()); // Not active
    }

    #[test]
    fn test_render_overlay_active() {
        let mut s = DragSession::new(
            1,
            DragDataType::Files(vec!["a".into(), "b".into()]),
            0.0,
            0.0,
        );
        s.phase = DragPhase::Active;
        s.mouse_x = 100.0;
        s.mouse_y = 100.0;
        let cmds = render_drag_overlay(&s);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_target_highlight() {
        let t = DropTarget {
            id: 1,
            x: 50.0,
            y: 50.0,
            width: 200.0,
            height: 150.0,
            accepted_effects: vec![DropEffect::Copy],
            label: "Drop files here".into(),
        };
        let cmds = render_drop_target_highlight(&t, DropEffect::Copy);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_default_trait() {
        let _ = DragDropManager::default();
    }
}
