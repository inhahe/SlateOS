//! Window rules in the desktop shell: the settings panel, and the rules
//! themselves under the names the shell has always used.
//!
//! The rules, the engine that applies them and the file they are kept in
//! live in their own crate, `windowrules`, so that Settings can read and
//! write `window-rules.yaml` without linking the shell. What stays here is
//! the panel that draws them and the re-export below.
//!
//! A user defines rules that automatically apply window behavior
//! when a window first appears. Rules match by window title or by the program
//! the window belongs to (its [`app_id`](crate::ManagedWindow::app_id)), and
//! can express:
//!
//! - Initial position and size
//! - Virtual desktop assignment
//! - Always-on-top / always-on-bottom
//! - Start minimized / maximized / fullscreen
//! - Opacity / transparency
//! - Skip taskbar / skip alt-tab
//! - Minimise to the system tray (and, with start minimized, start there)
//! - Force-assign to specific monitor
//! - Custom title bar visibility
//!
//! Rules are evaluated in priority order; first match wins (unless the manager
//! is in [`EvalMode::MergeAll`], in which case all matching rules are merged).
//!
//! # What the shell can actually carry out today
//!
//! Sixteen of [`RuleActions`]'s eighteen fields: `skip_taskbar`,
//! `skip_alt_tab` and `to_tray` in the shell itself, and the rest through
//! requests only a shell may send about another program's window -- see
//! `DesktopShell::rule_requests`. The two that do nothing yet are
//! `target_monitor`, which waits on the compositor modelling more than one
//! display, and `no_decorations`, which waits on a request to take a frame
//! away. They are stored and shown rather than faked. See `known-issues.md`
//! `TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO`.

use appearance::{Edge, Palette, Surface, readable_on};
use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::scroll_window;
use guitk::style::CornerRadii;
use guitk::text;

// Rule-list column widths. Defined once because the header row and the rule
// rows both lay out against them, and two copies of a column width drift.
const COL_PRIORITY: f32 = 70.0;
const COL_NAME: f32 = 180.0;
const COL_MATCH: f32 = 200.0;
const COL_ACTIONS: f32 = 60.0;
const COL_HITS: f32 = 50.0;
const COL_STATUS: f32 = 60.0;
/// Room left between a column's text and the next column's edge.
const COL_GUTTER: f32 = 8.0;
/// Horizontal room the action-summary line gives up to the icon on its left
/// and the row's right margin.
const SUMMARY_INSET: f32 = 86.0;

// ============================================================================
// Colour
// ============================================================================
//
// Fourteen `const MOCHA_*: Color` declarations stood here — a private copy of
// the Catppuccin Mocha ladder, so this panel could not follow the user's mode
// or accent no matter what they chose. They are gone; `render` takes the
// resolved `&Palette` and every colour below is a role on it. See
// known-issues.md `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
//
// Two judgements the substitution had to make, since a role is not implied by
// a hex value:
//
// * **The rule row's cells are categorical, not accent.** Each cell of a rule
//   row is coloured by what it *means* — priority is red above 50, yellow
//   above 10, `subtext1` otherwise; the action count is green when there are
//   any and `overlay0` when there are none; status is green for ON and red
//   for OFF; the match expression is blue. Those are siblings in one row, and
//   a user who picks a pink accent is telling the shell what to highlight
//   with, not asking for pink "OFF" badges. They stay on the named hues. The
//   blue of the match column is the trap in that set: blue is the *default*
//   accent, so a blue with categorical siblings reads like an accent site and
//   is not one.
// * **The two chrome affordances are accent.** The "+ Add Rule" button is the
//   list view's primary action and the selected match-type chip is a
//   selection, so both are `p.accent` with `readable_on` labels. The editor's
//   Save button stays `p.green` — green is "confirm" against the Cancel
//   button's `surface2`, and it is the one hue no accent resolves to, so
//   there is no faithful reading of it as an accent site. That leaves a
//   primary action that follows the accent in one view and one that does not
//   in the other; the inconsistency predates the conversion and is recorded
//   in known-issues.md rather than designed away here, because converting a
//   module and redesigning it at the same time makes both unreviewable.

pub use windowrules::{
    EvalMode, InitialState, MatchCriteria, PositionSpec, RuleActions, RuleId, SizeSpec, WindowRule,
    WindowRulesManager,
};

/// What to tell the user about rules in their file that cannot be read: a
/// notice's title and body, or `None` when there is nothing to tell.
///
/// # A rule that cannot be read is said, not skipped in silence
///
/// Settings writes only rules it can read, so what meets this is a hand edit
/// -- and a typo in a hand edit that quietly does nothing is the failure that
/// most needs a voice. The session says it on every read that finds it, the
/// first of the session included, so a problem left in the file is said again
/// each session rather than once and never again. Each problem goes to the
/// error stream as well, which has every one where the notice lists a few.
#[must_use]
pub fn problems_notice(problems: &[windowrules::file::Problem]) -> Option<(String, String)> {
    /// More than this are counted rather than listed: a notice is read at a
    /// glance, and a toast shows three lines of it.
    const LISTED: usize = 3;
    let title = match problems {
        [] => return None,
        [one] if one.rule.is_empty() => "No window rule is applied".to_owned(),
        [_] => "A window rule is not applied".to_owned(),
        many => format!("{} window rules are not applied", many.len()),
    };
    let mut lines: Vec<String> = problems
        .iter()
        .take(LISTED)
        .map(|problem| {
            if problem.rule.is_empty() {
                problem.what.clone()
            } else {
                // Debug-quoted, as `Problem`'s own text has it, so a name with
                // a hidden character in it shows the character.
                format!("{:?}: {}", problem.rule, problem.what)
            }
        })
        .collect();
    let unlisted = problems.len().saturating_sub(LISTED);
    if unlisted > 0 {
        lines.push(format!("and {unlisted} more"));
    }
    Some((title, lines.join("\n")))
}

// ============================================================================
// Settings UI model
// ============================================================================

/// Which section of the rules settings UI is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesSettingsTab {
    RuleList,
    EditRule,
    CreateRule,
}

/// State for the rules settings panel.
pub struct RulesSettingsUI {
    pub active_tab: RulesSettingsTab,
    pub selected_rule_idx: usize,
    pub scroll_offset: usize,
    pub editing_name: String,
    /// Which [`MatchCriteria`] the editor is building: 0=TitleExact,
    /// 1=TitleContains, 2=AppId, 3=Any. Indices into `criteria_labels` in
    /// `render`, and the reason the value box is hidden for 3 alone.
    pub editing_criteria_type: usize,
    pub editing_criteria_value: String,
    pub editing_priority: i32,
    pub visible_rules: usize,
}

impl RulesSettingsUI {
    pub fn new() -> Self {
        Self {
            active_tab: RulesSettingsTab::RuleList,
            selected_rule_idx: 0,
            scroll_offset: 0,
            editing_name: String::new(),
            editing_criteria_type: 0,
            editing_criteria_value: String::new(),
            editing_priority: 0,
            visible_rules: 10,
        }
    }

    /// Render the rules settings panel.
    pub fn render(
        &self,
        p: &Palette,
        manager: &WindowRulesManager,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background panel.
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: p.base,
            corner_radii: CornerRadii::all(8.0),
        });

        // Title bar.
        p.push_surface(&mut cmds, x, y, w, 40.0, 0.0, Surface::Strip(Edge::Bottom));
        cmds.push(RenderCommand::Text {
            x: x + 16.0,
            y: y + 12.0,
            text: "Window Rules".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Rule count badge.
        let count_text = format!(
            "{} rules ({} active)",
            manager.total_rule_count(),
            manager.active_rule_count(),
        );
        cmds.push(RenderCommand::Text {
            x: x + w - 200.0,
            y: y + 14.0,
            text: count_text,
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Eval mode indicator.
        let mode_text = match manager.eval_mode() {
            EvalMode::FirstMatch => "Mode: First Match",
            EvalMode::MergeAll => "Mode: Merge All",
        };
        cmds.push(RenderCommand::Text {
            x: x + w - 200.0,
            y: y + 28.0,
            text: mode_text.to_string(),
            font_size: 10.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        match self.active_tab {
            RulesSettingsTab::RuleList => {
                self.render_rule_list(p, &mut cmds, manager, x, y + 44.0, w, h - 44.0);
            }
            RulesSettingsTab::EditRule | RulesSettingsTab::CreateRule => {
                self.render_rule_editor(p, &mut cmds, x, y + 44.0, w, h - 44.0);
            }
        }

        cmds
    }

    fn render_rule_list(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        manager: &WindowRulesManager,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
    ) {
        let rules = manager.rules();
        let row_h = 48.0;

        // Column headers. The widths come from the shared constants rather
        // than being written out again here: the row loop below advances by
        // the same figures, and two copies of a column width drift.
        let headers = [
            ("Priority", COL_PRIORITY),
            ("Name", COL_NAME),
            ("Match", COL_MATCH),
            ("Actions", COL_ACTIONS),
            ("Hits", COL_HITS),
            ("Status", COL_STATUS),
        ];
        let mut hx = x + 8.0;
        for (label, col_w) in &headers {
            cmds.push(RenderCommand::Text {
                x: hx,
                y: y + 4.0,
                text: label.to_string(),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            hx += col_w;
        }

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: x + 4.0,
            y1: y + 22.0,
            x2: x + w - 4.0,
            y2: y + 22.0,
            color: p.surface1,
            width: 1.0,
        });

        // Rule rows. `scroll_offset` is a public field that nothing clamps, so
        // it can name a row past the end of a list that has since shrunk — which
        // used to be an `end - start` underflow here. The shared window is now
        // the only copy of that arithmetic (`touchpad.rs` and `bluetooth.rs`
        // each had their own, differently broken), and it goes one better than
        // the local fix did: a stale offset shows the last page rather than a
        // blank list.
        let window =
            scroll_window::visible_count(rules.len(), self.visible_rules, self.scroll_offset);
        let visible = rules.get(window.start..window.end()).unwrap_or_default();
        for (row, rule) in visible.iter().enumerate() {
            let i = window.start.saturating_add(row);
            let ry = y + 26.0 + (row as f32) * row_h;
            let selected = i == self.selected_rule_idx;

            // Row background.
            if selected {
                p.push_surface(
                    cmds,
                    x + 4.0,
                    ry,
                    w - 8.0,
                    row_h - 4.0,
                    4.0,
                    Surface::Selected,
                );
            }

            let mut cx = x + 8.0;

            // Priority.
            let priority_color = if rule.priority > 50 {
                p.red
            } else if rule.priority > 10 {
                p.yellow
            } else {
                p.subtext1
            };
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{}", rule.priority),
                font_size: 12.0,
                color: priority_color,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += COL_PRIORITY;

            // Name. These cells carry no `max_width`, so the elision is the
            // only thing keeping a long rule name -- which the user types --
            // out of the next column. Cut it to the column it occupies.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: text::elide(
                    &rule.name,
                    COL_NAME - COL_GUTTER,
                    "...",
                    12.0,
                    FontWeightHint::Regular,
                ),
                font_size: 12.0,
                color: if rule.enabled { p.text } else { p.overlay0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += COL_NAME;

            // Match criteria.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: text::elide(
                    &rule.criteria.description(),
                    COL_MATCH - COL_GUTTER,
                    "...",
                    11.0,
                    FontWeightHint::Regular,
                ),
                font_size: 11.0,
                color: p.ink(p.blue),
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += COL_MATCH;

            // Action count.
            let ac = rule.actions.active_count();
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{} act.", ac),
                font_size: 11.0,
                color: if ac > 0 { p.ink(p.green) } else { p.subtext0 },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += 60.0;

            // Match count.
            cmds.push(RenderCommand::Text {
                x: cx,
                y: ry + 8.0,
                text: format!("{}", rule.match_count),
                font_size: 11.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cx += 50.0;

            // Status.
            let (status_text, status_color) = if rule.enabled {
                ("ON", p.green)
            } else {
                ("OFF", p.red)
            };
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: ry + 6.0,
                width: 32.0,
                height: 18.0,
                // The badge is the status hue at a fifth strength. Alpha is
                // how a role becomes a wash, so this is still the role and
                // the two-mode sweep still recognises it — it compares RGB
                // and ignores alpha for exactly this shape.
                color: Color::rgba(status_color.r, status_color.g, status_color.b, 51),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: cx + 6.0,
                y: ry + 8.0,
                text: status_text.to_string(),
                font_size: 10.0,
                color: status_color,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // One-shot indicator.
            if rule.one_shot {
                cmds.push(RenderCommand::Text {
                    x: cx + 40.0,
                    y: ry + 8.0,
                    text: "1x".to_string(),
                    font_size: 9.0,
                    color: p.ink(p.peach),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }

            // Second row: action summary.
            let summary = rule.actions.summary();
            if !summary.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: x + 78.0,
                    y: ry + 26.0,
                    // Spans the rest of the row rather than a fixed column, so
                    // it is elided to the room actually left beside the icon.
                    text: text::elide(
                        &summary,
                        (w - SUMMARY_INSET).max(0.0),
                        "...",
                        10.0,
                        FontWeightHint::Regular,
                    ),
                    font_size: 10.0,
                    color: p.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
        }

        // "Add Rule" button area.
        let btn_y = y + 26.0 + (visible.len() as f32) * row_h + 8.0;
        cmds.push(RenderCommand::FillRect {
            x: x + 8.0,
            y: btn_y,
            width: 100.0,
            height: 28.0,
            color: p.accent,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 24.0,
            y: btn_y + 7.0,
            text: "+ Add Rule".to_string(),
            font_size: 12.0,
            color: readable_on(p.accent),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    fn render_rule_editor(
        &self,
        p: &Palette,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        _h: f32,
    ) {
        let label_x = x + 16.0;
        let input_x = x + 140.0;
        let input_w = w - 170.0;
        let mut cy = y + 12.0;

        let title = if self.active_tab == RulesSettingsTab::CreateRule {
            "Create New Rule"
        } else {
            "Edit Rule"
        };
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy,
            text: title.to_string(),
            font_size: 14.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 30.0;

        // Name field.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Name:".to_string(),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        p.push_surface(cmds, input_x, cy, input_w, 24.0, 4.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: input_x + 8.0,
            y: cy + 5.0,
            text: if self.editing_name.is_empty() {
                "Enter rule name...".to_string()
            } else {
                self.editing_name.clone()
            },
            font_size: 12.0,
            color: if self.editing_name.is_empty() {
                p.subtext0
            } else {
                p.text
            },
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 36.0;

        // Match type selector.
        let criteria_labels = ["Title (exact)", "Title (contains)", "Application", "Any"];
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Match:".to_string(),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        for (i, label) in criteria_labels.iter().enumerate() {
            let bx = input_x + (i as f32) * 110.0;
            let selected = i == self.editing_criteria_type;
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: cy,
                width: 105.0,
                height: 24.0,
                color: if selected { p.accent } else { p.surface0 },
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: cy + 6.0,
                text: label.to_string(),
                font_size: 10.0,
                color: if selected {
                    readable_on(p.accent)
                } else {
                    p.text
                },
                font_weight: if selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
        cy += 36.0;

        // Match value (unless "Any", which is the last chip and takes none).
        if self.editing_criteria_type < criteria_labels.len().saturating_sub(1) {
            cmds.push(RenderCommand::Text {
                x: label_x,
                y: cy + 4.0,
                text: "Value:".to_string(),
                font_size: 12.0,
                color: p.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            p.push_surface(cmds, input_x, cy, input_w, 24.0, 4.0, Surface::Card);
            cmds.push(RenderCommand::Text {
                x: input_x + 8.0,
                y: cy + 5.0,
                text: if self.editing_criteria_value.is_empty() {
                    "Enter match value...".to_string()
                } else {
                    self.editing_criteria_value.clone()
                },
                font_size: 12.0,
                color: if self.editing_criteria_value.is_empty() {
                    p.subtext0
                } else {
                    p.text
                },
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += 36.0;
        }

        // Priority.
        cmds.push(RenderCommand::Text {
            x: label_x,
            y: cy + 4.0,
            text: "Priority:".to_string(),
            font_size: 12.0,
            color: p.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        p.push_surface(cmds, input_x, cy, 80.0, 24.0, 4.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: input_x + 8.0,
            y: cy + 5.0,
            text: format!("{}", self.editing_priority),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        cy += 40.0;

        // Save / Cancel buttons.
        cmds.push(RenderCommand::FillRect {
            x: input_x,
            y: cy,
            width: 80.0,
            height: 28.0,
            color: p.green,
            corner_radii: CornerRadii::all(6.0),
        });
        cmds.push(RenderCommand::Text {
            x: input_x + 20.0,
            y: cy + 7.0,
            text: "Save".to_string(),
            font_size: 12.0,
            color: readable_on(p.green),
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        p.push_surface(cmds, input_x + 92.0, cy, 80.0, 28.0, 6.0, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: input_x + 108.0,
            y: cy + 7.0,
            text: "Cancel".to_string(),
            font_size: 12.0,
            color: p.text,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

impl Default for RulesSettingsUI {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helpers
// ============================================================================

// `truncate_string` lived here: it compared `s.len()` (bytes) against a
// character budget, and each of its three call sites passed a budget picked
// independently of the column the text was drawn in. Replaced by
// `text::elide`, which measures against the actual column width. See
// known-issues.md TD-APPS-ESTIMATE-TEXT-WIDTH.

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
    use appearance::palette_check;

    /// The palette the tests that predate the conversion render against.
    ///
    /// Dark, because those tests were written when this module could only be
    /// dark and they assert about geometry and text rather than colour. The
    /// colour tests below build their own palettes.
    fn test_palette() -> Palette {
        Palette::for_mode(false)
    }

    /// A rule set that reaches every branch of the row painter.
    ///
    /// The colour of a rule row is a function of the *rule*, not of the UI
    /// state, so a sweep driven only by `RulesSettingsUI` fields would render
    /// perhaps half the hues in the module and call the other half converted
    /// without ever drawing them. These five rules between them take all three
    /// steps of the priority ladder, both sides of `enabled`, both sides of
    /// "has any actions", the one-shot badge and a non-empty action summary.
    fn every_branch_rule_set() -> WindowRulesManager {
        let mut mgr = WindowRulesManager::empty();

        // Priority above 50: red. Has actions: green. Non-empty summary.
        let mut hot = WindowRule::new(0, "hot", MatchCriteria::TitleExact("A".to_string()));
        hot.priority = 99;
        hot.actions.always_on_top = Some(true);
        hot.match_count = 7;
        assert!(mgr.add_rule(hot).is_some());

        // Priority above 10: yellow. One-shot: the peach badge.
        let mut warm = WindowRule::new(0, "warm", MatchCriteria::TitleContains("b".to_string()));
        warm.priority = 20;
        warm.one_shot = true;
        assert!(mgr.add_rule(warm).is_some());

        // Low priority: subtext1. No actions: overlay0, and no summary line.
        let mut cool = WindowRule::new(0, "cool", MatchCriteria::AppId("c".to_string()));
        cool.priority = 1;
        assert!(mgr.add_rule(cool).is_some());

        // Disabled: an overlay0 name and the red OFF badge.
        let mut off = WindowRule::new(0, "off", MatchCriteria::AppId("d".to_string()));
        off.enabled = false;
        assert!(mgr.add_rule(off).is_some());

        let mut any = WindowRule::new(0, "any", MatchCriteria::Any);
        any.actions.opacity = Some(0.5);
        any.actions.skip_taskbar = Some(true);
        assert!(mgr.add_rule(any).is_some());

        mgr
    }

    fn no_rules() -> WindowRulesManager {
        WindowRulesManager::empty()
    }

    fn wound_ui(
        tab: RulesSettingsTab,
        criteria_type: usize,
        filled: bool,
        selected_rule_idx: usize,
    ) -> RulesSettingsUI {
        let mut ui = RulesSettingsUI::new();
        ui.active_tab = tab;
        ui.editing_criteria_type = criteria_type;
        ui.selected_rule_idx = selected_rule_idx;
        if filled {
            ui.editing_name = "a rule".to_string();
            ui.editing_criteria_value = "firefox".to_string();
            ui.editing_priority = 42;
        }
        ui
    }

    #[test]
    fn a_rule_name_stays_inside_its_column() {
        // These cells carry no `max_width`, so nothing downstream will save a
        // name that overruns -- it draws straight over the Match column. The
        // old character budget could not prevent that, because 24 characters
        // is a different width for every 24 characters.
        let mut mgr = WindowRulesManager::new();
        for (i, name) in [
            "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW",
            "a rule name with accents ééééééééééééééé",
            "short",
        ]
        .iter()
        .enumerate()
        {
            mgr.add_rule(WindowRule::new(i as RuleId, name, MatchCriteria::Any));
        }
        let ui = RulesSettingsUI::new();
        let mut cmds = Vec::new();
        ui.render_rule_list(&test_palette(), &mut cmds, &mgr, 0.0, 0.0, 900.0, 600.0);

        let mut checked = 0;
        for cmd in &cmds {
            if let RenderCommand::Text {
                text,
                font_size,
                font_weight,
                ..
            } = cmd
                && (*font_size - 12.0).abs() < 0.01
                && text != "Priority"
            {
                let w = guitk::text::measure(text, *font_size, *font_weight);
                assert!(
                    w <= COL_NAME - COL_GUTTER + 0.01,
                    "rule name {text:?} measures {w}, wider than its {} px column",
                    COL_NAME - COL_GUTTER
                );
                checked += 1;
            }
        }
        // Without this the test passes on a render that drew no names at all.
        assert!(checked >= 3, "expected three rule names, checked {checked}");
    }

    #[test]
    fn a_short_rule_name_is_not_elided() {
        let mut mgr = WindowRulesManager::new();
        mgr.add_rule(WindowRule::new(1, "short", MatchCriteria::Any));
        let ui = RulesSettingsUI::new();
        let mut cmds = Vec::new();
        ui.render_rule_list(&test_palette(), &mut cmds, &mgr, 0.0, 0.0, 900.0, 600.0);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "short"
            )),
            "a name that fits its column should be drawn whole"
        );
    }

    #[test]
    fn test_ui_creation() {
        let ui = RulesSettingsUI::new();
        assert_eq!(ui.active_tab, RulesSettingsTab::RuleList);
        assert_eq!(ui.selected_rule_idx, 0);
    }

    #[test]
    fn test_ui_render_no_panic() {
        let mgr = WindowRulesManager::new();
        let ui = RulesSettingsUI::new();
        let cmds = ui.render(&test_palette(), &mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_edit_tab() {
        let mgr = WindowRulesManager::new();
        let mut ui = RulesSettingsUI::new();
        ui.active_tab = RulesSettingsTab::EditRule;
        let cmds = ui.render(&test_palette(), &mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_ui_render_create_tab() {
        let mgr = WindowRulesManager::new();
        let mut ui = RulesSettingsUI::new();
        ui.active_tab = RulesSettingsTab::CreateRule;
        let cmds = ui.render(&test_palette(), &mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty());
    }

    #[test]
    fn scrolling_past_the_end_of_a_shrunken_rule_list_shows_the_last_page() {
        // `scroll_offset` is public and nothing clamps it, so a list that
        // shrinks under a scrolled view leaves it pointing past the end. That
        // used to be an `end - start` underflow — a panic in the shell's render
        // path, reachable from removing rules while scrolled down. The first fix
        // made it render an empty list; sharing `scroll_window` with the other
        // panels makes it render the last page instead, which is what the user
        // scrolled down to look at.
        //
        // This test was previously named "renders nothing" but only asserted
        // that *something* drew, so it would have passed either way. It now
        // names the row it expects to see.
        let mut mgr = WindowRulesManager::empty();
        mgr.add_rule(WindowRule::new(0, "only", MatchCriteria::Any));

        let mut ui = RulesSettingsUI::new();
        ui.scroll_offset = 50;
        let cmds = ui.render(&test_palette(), &mgr, 0.0, 0.0, 800.0, 600.0);
        assert!(!cmds.is_empty(), "the chrome still draws");
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                RenderCommand::Text { text, .. } if text == "only"
            )),
            "the one surviving rule should be pulled back into view"
        );
    }

    /// Every colour the panel paints is a role on the palette it was handed.
    ///
    /// Rendered in both modes; the light render is the one that does the work,
    /// because the fourteen deleted `MOCHA_*` constants were Catppuccin Mocha
    /// values and none of them is a member of Latte. A leftover therefore names
    /// itself in the light pass rather than hiding behind a value that happens
    /// to be right in dark mode.
    ///
    /// `derived` is empty even though the status badge is a wash: the sweep
    /// compares roles on RGB alone, so `p.green` at alpha 51 is still `p.green`
    /// and a *Mocha* green at alpha 51 is still not a Latte role. Declaring the
    /// wash as derived would have blinded the sweep to exactly the literal it
    /// exists to catch.
    #[test]
    fn every_colour_the_panel_draws_comes_from_its_palette() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for populated in [true, false] {
                for mode in [EvalMode::FirstMatch, EvalMode::MergeAll] {
                    let mut mgr = if populated {
                        every_branch_rule_set()
                    } else {
                        no_rules()
                    };
                    mgr.set_eval_mode(mode);
                    for tab in [
                        RulesSettingsTab::RuleList,
                        RulesSettingsTab::EditRule,
                        RulesSettingsTab::CreateRule,
                    ] {
                        // 0..=4 covers every match-type chip as the selected
                        // one, and 4 ("Any") is also the branch that hides the
                        // value field entirely.
                        for criteria_type in 0..=4 {
                            for filled in [false, true] {
                                // Index 0 selects a visible row; 3 selects a
                                // different one, so neither the selected nor
                                // the unselected background goes undrawn.
                                for selected in [0_usize, 3] {
                                    let ui = wound_ui(tab, criteria_type, filled, selected);
                                    let cmds = ui.render(&p, &mgr, 0.0, 0.0, 900.0, 600.0);
                                    assert!(!cmds.is_empty(), "the panel always draws its frame");
                                    // The selected chip is lettered for the
                                    // accent, the save button for green.
                                    palette_check::assert_drawn_from(
                                        &p,
                                        &cmds,
                                        &[readable_on(p.accent), readable_on(p.green)],
                                        "window_rules",
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// A rule row's colours say what the rule *is*, so they do not move when
    /// the user changes the accent.
    ///
    /// The sweep above cannot make this check. A role is a member of *both*
    /// palettes, so a row converted to `p.accent` instead of `p.green` passes
    /// the light-mode sweep exactly as it passes the dark one — the sweep
    /// answers "was this converted at all", never "was it converted to the
    /// right thing".
    ///
    /// The match-expression column is this module's trap: it is blue, and blue
    /// is the *default* accent, so it reads like an accent site at a glance. It
    /// is not one — it is a sibling of the priority red, the action-count green
    /// and the OFF red in the same row, and a user who picks a pink accent is
    /// saying what to highlight with, not asking for pink "OFF" badges.
    #[test]
    fn a_rule_rows_colours_do_not_follow_the_accent() {
        let mgr = every_branch_rule_set();
        let ui = wound_ui(RulesSettingsTab::RuleList, 0, true, 0);
        let render = |accent: Color| {
            let mut p = Palette::for_mode(false);
            p.accent = accent;
            ui.render(&p, &mgr, 0.0, 0.0, 900.0, 600.0)
        };
        let blue = render(appearance::BLUE);
        let mauve = render(appearance::MAUVE);

        // Everything in the list view except the "+ Add Rule" button: the row
        // cells, the status pills, the headers and the chrome.
        let rows = |cmds: &[RenderCommand]| -> Vec<Color> {
            cmds.iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, color, .. } if text != "+ Add Rule" => Some(*color),
                    RenderCommand::FillRect {
                        width: 32.0, color, ..
                    } => Some(*color),
                    _ => None,
                })
                .collect()
        };
        assert!(
            rows(&blue).len() > 20,
            "the fixture drew {} row colours, too few to be measuring the rows",
            rows(&blue).len()
        );
        assert_eq!(
            rows(&blue),
            rows(&mauve),
            "a rule row's colours are categorical; none of them may move with \
             the accent"
        );

        // The negative half. Without it a module that ignored the accent
        // everywhere -- the very bug this conversion exists to fix -- would
        // pass the assertion above while measuring nothing at all.
        let add_button = |cmds: &[RenderCommand]| -> Vec<Color> {
            cmds.iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        width: 100.0,
                        height: 28.0,
                        color,
                        ..
                    } => Some(*color),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            add_button(&blue).len(),
            1,
            "the Add Rule button is drawn once"
        );
        assert_ne!(
            add_button(&blue),
            add_button(&mauve),
            "the list view's primary action is an accent site and must follow \
             the accent"
        );
    }

    /// Save is green because green means confirm, not because blue was the
    /// accent — so it stays green when the accent changes.
    ///
    /// Green is the one hue no accent resolves to, which is what makes this a
    /// decidable question rather than a matter of taste: reading Save as an
    /// accent site would have changed its colour under the *default* accent,
    /// and no faithful conversion does that. The chips above it are selections
    /// and do follow the accent, which is this test's negative half.
    #[test]
    fn the_editors_save_button_does_not_follow_the_accent() {
        let mgr = every_branch_rule_set();
        let ui = wound_ui(RulesSettingsTab::EditRule, 1, true, 0);
        let render = |accent: Color| {
            let mut p = Palette::for_mode(false);
            p.accent = accent;
            ui.render(&p, &mgr, 0.0, 0.0, 900.0, 600.0)
        };
        let blue = render(appearance::BLUE);
        let mauve = render(appearance::MAUVE);

        // Save and Cancel: the two 80x28 buttons at the foot of the form.
        let buttons = |cmds: &[RenderCommand]| -> Vec<Color> {
            cmds.iter()
                .filter_map(appearance::painted_rect)
                .filter(|(_, _, w, h, _)| (*w - 80.0).abs() < 0.01 && (*h - 28.0).abs() < 0.01)
                .map(|t| t.4)
                .collect()
        };
        assert_eq!(buttons(&blue).len(), 2, "Save and Cancel both draw");
        assert_eq!(
            buttons(&blue),
            buttons(&mauve),
            "confirm-green and the neutral Cancel are not accent sites"
        );

        let chips = |cmds: &[RenderCommand]| -> Vec<Color> {
            cmds.iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect {
                        width: 105.0,
                        color,
                        ..
                    } => Some(*color),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(chips(&blue).len(), 4, "one chip per match type");
        assert_ne!(
            chips(&blue),
            chips(&mauve),
            "the selected match-type chip is a selection and must follow the \
             accent"
        );
    }

    #[test]
    fn the_panel_has_a_default() {
        let ui = RulesSettingsUI::default();
        assert_eq!(ui.active_tab, RulesSettingsTab::RuleList);
    }

    /// **What a notice says about rules that cannot be read**: nothing for
    /// none; the rule's name and why for one; for many, how many, the first
    /// three and how many more; and the file's own trouble when it is the
    /// rules as a whole.
    #[test]
    fn a_notice_names_the_rules_that_cannot_be_read() {
        use windowrules::file::Problem;
        let problem = |rule: &str| Problem {
            rule: rule.to_owned(),
            what: format!("{rule} is wrong"),
        };
        assert_eq!(problems_notice(&[]), None);

        let (title, body) = problems_notice(&[problem("typo")]).unwrap();
        assert_eq!(title, "A window rule is not applied");
        assert_eq!(body, "\"typo\": typo is wrong");

        let whole = Problem {
            rule: String::new(),
            what: "`rules` holds rules".to_owned(),
        };
        let (title, body) = problems_notice(&[whole]).unwrap();
        assert_eq!(title, "No window rule is applied");
        assert_eq!(body, "`rules` holds rules");

        let five: Vec<Problem> = ["a", "b", "c", "d", "e"].map(problem).into();
        let (title, body) = problems_notice(&five).unwrap();
        assert_eq!(title, "5 window rules are not applied");
        assert_eq!(
            body.lines().collect::<Vec<_>>(),
            [
                "\"a\": a is wrong",
                "\"b\": b is wrong",
                "\"c\": c is wrong",
                "and 2 more"
            ]
        );
        // Three are listed whole, with nothing left to count.
        let (title, body) = problems_notice(&five[..3]).unwrap();
        assert_eq!(title, "3 window rules are not applied");
        assert_eq!(body.lines().count(), 3);
        // A hidden character in a name is shown, not drawn as nothing.
        let (_, body) = problems_notice(&[problem("a\u{200b}b")]).unwrap();
        assert!(body.starts_with("\"a\\u{200b}b\""), "{body}");
    }
}
