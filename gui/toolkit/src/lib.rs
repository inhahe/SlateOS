#![deny(clippy::all)]
// `clippy::pedantic` is intentionally NOT denied yet — guitk is ~50k lines of
// widget code and a pedantic sweep produces >1200 warnings (most are doc/style
// items: `missing_panics_doc`, `must_use_candidate`, etc.). Tracked as a
// separate LIMITATION in `todo.txt`.

//! guitk — Slate OS GUI Toolkit Library
//!
//! Provides a widget library with a Flexbox/Grid-inspired layout engine,
//! a simple styling system, and event dispatch. Rendering-backend-agnostic:
//! produces a render tree of drawing primitives that any compositor backend
//! can consume.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! Widget Tree (declarative UI description)
//!     │
//!     ▼
//! Layout Engine (Flexbox/Grid algorithm → computed positions & sizes)
//!     │
//!     ▼
//! Render Tree (list of drawing primitives: rects, text, images)
//!     │
//!     ▼
//! Backend (compositor syscalls, framebuffer, etc.)
//! ```

pub mod color;
pub mod colorpicker;
pub mod context_ext;
pub mod dialog;
pub mod disabled;
pub mod dnd;
pub mod event;
pub mod filetypes;
pub mod fontdb;
pub mod grid;
pub mod layout;
pub mod menu;
pub mod menubar;
pub mod modal;
pub mod pathbar;
pub mod render;
pub mod scaling;
pub mod signal;
pub mod style;
pub mod svg;
pub mod table;
pub mod tabs;
pub mod text;
pub mod textview;
pub mod theme;
pub mod tree;
pub mod widget;

// Text-format escaping lives in `textfmt`, a dependency-free crate, because
// the components that most need it are headless and must not link a widget
// library. It is re-exported here under its original paths: `guitk::csv` and
// `guitk::escape` are what 137 applications already say, and moving the code
// is not a reason to touch all of them.
pub use textfmt::{csv, escape, fold, kv};

// Case-insensitive substring search lives in `textfind` for the same reason:
// the headless components search text too, and the applications that need it
// reach it through the toolkit they already depend on. Re-exported so a call
// site can say `guitk::textfind::matches(…)` without its crate growing a
// second path entry for a crate `guitk` already links.
pub use textfind;

pub use color::Color;
pub use event::{Event, KeyEvent, MouseButton, MouseEvent};
pub use layout::{Axis, FlexAlign, FlexDirection, FlexWrap, LayoutBox, Size};
pub use render::{RenderCommand, RenderTree};
pub use style::Style;
pub use widget::{Widget, WidgetId, WidgetTree};
