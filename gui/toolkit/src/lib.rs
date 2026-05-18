//! guitk — OurOS GUI Toolkit Library
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
pub mod dialog;
pub mod dnd;
pub mod event;
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
pub mod tabs;
pub mod textview;
pub mod theme;
pub mod tree;
pub mod widget;

pub use color::Color;
pub use event::{Event, KeyEvent, MouseButton, MouseEvent};
pub use layout::{Axis, FlexAlign, FlexDirection, FlexWrap, LayoutBox, Size};
pub use render::{RenderCommand, RenderTree};
pub use style::Style;
pub use widget::{Widget, WidgetId, WidgetTree};
