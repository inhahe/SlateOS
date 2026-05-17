//! Drag-and-drop data transfer module.
//!
//! Provides OLE-style multi-format data transfer for drag-and-drop operations
//! and inter-application data exchange. The design mirrors Windows OLE's
//! `IDataObject` concept: a source provides data in multiple formats, and
//! targets accept whichever format they understand.
//!
//! # Architecture
//!
//! ```text
//! Source Widget
//!     │  (populates DataObject with formats)
//!     ▼
//! DragDropManager (state machine: Idle → Dragging → OverTarget → Drop/Cancel)
//!     │  (hit-tests registered targets, negotiates formats)
//!     ▼
//! Target Widget
//!     (receives DataObject, picks preferred format)
//! ```
//!
//! # Usage
//!
//! 1. Register drop targets via [`DragDropManager::register_target`].
//! 2. When the user starts a drag, call [`DragDropManager::begin_drag`] with
//!    a populated [`DataObject`].
//! 3. Feed mouse moves to [`DragDropManager::update_position`], which returns
//!    [`DragEvent`]s for enter/leave transitions.
//! 4. On mouse release, call [`DragDropManager::end_drag`] to complete or
//!    cancel the drop.

/// Identifies a data format for transfer.
///
/// Standard formats cover common use cases (text, files, images, URLs).
/// Applications can define additional formats with [`DataFormat::Custom`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DataFormat {
    /// UTF-8 plain text.
    PlainText,
    /// Rich text (RTF-encoded).
    RichText,
    /// HTML fragment.
    Html,
    /// Newline-separated file paths.
    FilePaths,
    /// PNG-encoded image data.
    ImagePng,
    /// BMP-encoded image data.
    ImageBmp,
    /// A URL string.
    Url,
    /// Application-defined format identified by a string key.
    Custom(String),
}

/// A single piece of data in a specific format.
///
/// The raw bytes in `data` are interpreted according to the associated
/// [`DataFormat`]. For text-based formats, this is UTF-8 encoded text.
/// For image formats, this is the encoded image bytes.
#[derive(Clone, Debug)]
pub struct DataItem {
    /// The format this data is encoded in.
    pub format: DataFormat,
    /// Raw byte payload.
    pub data: Vec<u8>,
}

/// A data object that can provide data in multiple formats.
///
/// The source application populates this when starting a drag or copy
/// operation. It can hold the same logical content in multiple representations
/// (e.g., both `PlainText` and `Html`) so that targets can pick the richest
/// format they support.
#[derive(Clone, Debug, Default)]
pub struct DataObject {
    items: Vec<DataItem>,
}

impl DataObject {
    /// Creates an empty data object.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Creates a data object containing plain text.
    pub fn with_text(text: &str) -> Self {
        let mut obj = Self::new();
        obj.set_data(DataFormat::PlainText, text.as_bytes().to_vec());
        obj
    }

    /// Creates a data object containing file paths.
    ///
    /// Paths are stored as newline-separated UTF-8 strings.
    pub fn with_files(paths: &[&str]) -> Self {
        let mut obj = Self::new();
        let joined = paths.join("\n");
        obj.set_data(DataFormat::FilePaths, joined.into_bytes());
        obj
    }

    /// Sets data for a given format, replacing any existing data in that format.
    pub fn set_data(&mut self, format: DataFormat, data: Vec<u8>) {
        // Replace existing entry for this format if present.
        if let Some(item) = self.items.iter_mut().find(|i| i.format == format) {
            item.data = data;
        } else {
            self.items.push(DataItem { format, data });
        }
    }

    /// Retrieves raw data for a given format, if available.
    pub fn get_data(&self, format: &DataFormat) -> Option<&[u8]> {
        self.items
            .iter()
            .find(|i| &i.format == format)
            .map(|i| i.data.as_slice())
    }

    /// Returns true if data is available in the given format.
    pub fn has_format(&self, format: &DataFormat) -> bool {
        self.items.iter().any(|i| &i.format == format)
    }

    /// Returns all formats available in this data object.
    pub fn available_formats(&self) -> Vec<&DataFormat> {
        self.items.iter().map(|i| &i.format).collect()
    }

    /// Convenience: retrieves the plain text content as a string slice.
    ///
    /// Returns `None` if no `PlainText` data is set or if the bytes are not
    /// valid UTF-8.
    pub fn get_text(&self) -> Option<&str> {
        self.get_data(&DataFormat::PlainText)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
    }

    /// Convenience: retrieves file paths as a vector of string slices.
    ///
    /// Returns `None` if no `FilePaths` data is set or if the bytes are not
    /// valid UTF-8. Individual paths are split on newlines.
    pub fn get_file_paths(&self) -> Option<Vec<&str>> {
        self.get_data(&DataFormat::FilePaths)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .map(|s| s.split('\n').filter(|p| !p.is_empty()).collect())
    }

    /// Convenience: retrieves the URL content as a string slice.
    ///
    /// Returns `None` if no `Url` data is set or if the bytes are not valid
    /// UTF-8.
    pub fn get_url(&self) -> Option<&str> {
        self.get_data(&DataFormat::Url)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
    }
}

/// The allowed drop effects, indicating what operation the drop performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEffect {
    /// No drop allowed.
    None,
    /// Data will be copied to the target.
    Copy,
    /// Data will be moved (source deletes after drop).
    Move,
    /// A link/shortcut to the data will be created.
    Link,
}

/// Current state of a drag operation.
///
/// The state machine transitions:
/// `Idle` -> `Dragging` -> `OverTarget` -> `Idle` (on drop or cancel)
#[derive(Clone, Debug)]
pub enum DragState {
    /// No drag in progress.
    Idle,
    /// Drag started, mouse button held, not yet over a valid target.
    Dragging {
        /// The data being dragged.
        data: DataObject,
        /// Widget ID of the drag source.
        source_id: u64,
        /// Effects the source allows.
        allowed_effects: Vec<DropEffect>,
        /// X coordinate where drag started.
        start_x: f32,
        /// Y coordinate where drag started.
        start_y: f32,
        /// Current X coordinate of the pointer.
        current_x: f32,
        /// Current Y coordinate of the pointer.
        current_y: f32,
    },
    /// Pointer is over a valid drop target.
    OverTarget {
        /// The data being dragged.
        data: DataObject,
        /// Widget ID of the drag source.
        source_id: u64,
        /// Widget ID of the current drop target.
        target_id: u64,
        /// The negotiated drop effect.
        effect: DropEffect,
        /// Current X coordinate of the pointer.
        current_x: f32,
        /// Current Y coordinate of the pointer.
        current_y: f32,
    },
}

/// Events emitted during drag-and-drop.
///
/// The UI layer uses these to update visual feedback (cursor changes,
/// highlight states, etc.).
#[derive(Clone, Debug)]
pub enum DragEvent {
    /// Drag started from a source widget.
    DragStart {
        /// Widget ID of the drag source.
        source_id: u64,
        /// The data being dragged.
        data: DataObject,
        /// Effects the source allows.
        allowed: Vec<DropEffect>,
    },
    /// Mouse moved during drag.
    DragMove {
        /// Current X coordinate.
        x: f32,
        /// Current Y coordinate.
        y: f32,
    },
    /// Pointer entered a potential drop target.
    DragEnter {
        /// Widget ID of the target entered.
        target_id: u64,
        /// Formats available in the dragged data.
        formats: Vec<DataFormat>,
    },
    /// Pointer left a drop target.
    DragLeave {
        /// Widget ID of the target left.
        target_id: u64,
    },
    /// Data was dropped on a target.
    Drop {
        /// Widget ID of the target receiving the drop.
        target_id: u64,
        /// The transferred data.
        data: DataObject,
        /// The operation that was performed.
        effect: DropEffect,
    },
    /// Drag cancelled (Escape pressed or dropped on invalid area).
    DragCancelled,
}

/// A registered drop target area.
///
/// Drop targets define a rectangular region that can accept dragged data in
/// specific formats with specific effects.
#[derive(Clone, Debug)]
pub struct DropTarget {
    /// Unique identifier for this target.
    pub id: u64,
    /// X coordinate of the target's bounding box (top-left).
    pub x: f32,
    /// Y coordinate of the target's bounding box (top-left).
    pub y: f32,
    /// Width of the target's bounding box.
    pub width: f32,
    /// Height of the target's bounding box.
    pub height: f32,
    /// Formats this target can accept.
    pub accepted_formats: Vec<DataFormat>,
    /// Effects this target supports.
    pub allowed_effects: Vec<DropEffect>,
}

impl DropTarget {
    /// Returns true if the point (px, py) lies within this target's bounds.
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// Manages the drag-and-drop lifecycle.
///
/// Tracks drag state, registered drop targets, and produces [`DragEvent`]s
/// as the drag progresses. The manager enforces a minimum movement threshold
/// to prevent accidental drags from simple clicks.
pub struct DragDropManager {
    /// Current state of the drag operation.
    state: DragState,
    /// Minimum pixels the pointer must move before a drag is recognized.
    drag_threshold: f32,
    /// Whether the threshold has been exceeded for the current drag.
    threshold_met: bool,
    /// Registered drop targets.
    targets: Vec<DropTarget>,
}

impl DragDropManager {
    /// Creates a new manager with a default drag threshold of 5 pixels.
    pub fn new() -> Self {
        Self {
            state: DragState::Idle,
            drag_threshold: 5.0,
            threshold_met: false,
            targets: Vec::new(),
        }
    }

    /// Creates a new manager with a custom drag threshold.
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            state: DragState::Idle,
            drag_threshold: threshold,
            threshold_met: false,
            targets: Vec::new(),
        }
    }

    /// Registers a drop target. If a target with the same ID already exists,
    /// it is replaced.
    pub fn register_target(&mut self, target: DropTarget) {
        self.unregister_target(target.id);
        self.targets.push(target);
    }

    /// Removes a drop target by ID.
    pub fn unregister_target(&mut self, id: u64) {
        self.targets.retain(|t| t.id != id);
    }

    /// Begin a potential drag operation.
    ///
    /// The drag will not be considered active until the pointer moves beyond
    /// the configured [`drag_threshold`](Self::drag_threshold). This prevents
    /// accidental drags from simple clicks.
    pub fn begin_drag(
        &mut self,
        source_id: u64,
        x: f32,
        y: f32,
        data: DataObject,
        allowed: Vec<DropEffect>,
    ) {
        self.threshold_met = false;
        self.state = DragState::Dragging {
            data,
            source_id,
            allowed_effects: allowed,
            start_x: x,
            start_y: y,
            current_x: x,
            current_y: y,
        };
    }

    /// Update drag position (called on mouse move).
    ///
    /// Returns a [`DragEvent`] when the drag crosses a target boundary
    /// (enter/leave) or when the threshold is first exceeded (drag start).
    pub fn update_position(&mut self, x: f32, y: f32) -> Option<DragEvent> {
        match &self.state {
            DragState::Idle => None,
            DragState::Dragging { start_x, start_y, .. } => {
                let dx = x - start_x;
                let dy = y - start_y;
                let distance_sq = dx * dx + dy * dy;
                let threshold_sq = self.drag_threshold * self.drag_threshold;

                // Check if threshold has been met.
                if !self.threshold_met {
                    if distance_sq < threshold_sq {
                        // Update position but don't emit events yet.
                        if let DragState::Dragging {
                            current_x,
                            current_y,
                            ..
                        } = &mut self.state
                        {
                            *current_x = x;
                            *current_y = y;
                        }
                        return None;
                    }
                    self.threshold_met = true;
                }

                // Threshold exceeded — check for target entry.
                let target_hit = self.target_at_position(x, y);

                if let Some(target) = target_hit {
                    let target_id = target.id;
                    // Extract data from current state to build new state.
                    let (data, source_id, allowed_effects) =
                        if let DragState::Dragging {
                            data,
                            source_id,
                            allowed_effects,
                            ..
                        } = &self.state
                        {
                            (data.clone(), *source_id, allowed_effects.clone())
                        } else {
                            return None;
                        };

                    if self.target_accepts_data(target_id, &data) {
                        let effect = self.negotiate_effect(&allowed_effects, target_id);
                        let formats = data.available_formats().into_iter().cloned().collect();

                        self.state = DragState::OverTarget {
                            data,
                            source_id,
                            target_id,
                            effect,
                            current_x: x,
                            current_y: y,
                        };

                        return Some(DragEvent::DragEnter {
                            target_id,
                            formats,
                        });
                    }
                }

                // Not over a target — stay in Dragging state, update position.
                if let DragState::Dragging {
                    current_x,
                    current_y,
                    ..
                } = &mut self.state
                {
                    *current_x = x;
                    *current_y = y;
                }

                if self.threshold_met {
                    Some(DragEvent::DragMove { x, y })
                } else {
                    None
                }
            }
            DragState::OverTarget {
                target_id,
                data,
                source_id,
                ..
            } => {
                let prev_target_id = *target_id;
                let data_clone = data.clone();
                let source_id_val = *source_id;

                // Check if still over the same target.
                let still_over = self
                    .targets
                    .iter()
                    .find(|t| t.id == prev_target_id)
                    .is_some_and(|t| t.contains(x, y));

                if still_over {
                    // Update position within the same target.
                    if let DragState::OverTarget {
                        current_x,
                        current_y,
                        ..
                    } = &mut self.state
                    {
                        *current_x = x;
                        *current_y = y;
                    }
                    Some(DragEvent::DragMove { x, y })
                } else {
                    // Left the target — check if entering a new one.
                    let new_target = self.target_at_position(x, y);

                    if let Some(new_tgt) = new_target {
                        let new_id = new_tgt.id;
                        if self.target_accepts_data(new_id, &data_clone) {
                            let allowed = self.collect_allowed_from_dragging(&data_clone, source_id_val);
                            let effect = self.negotiate_effect(&allowed, new_id);
                            let formats = data_clone.available_formats().into_iter().cloned().collect();

                            self.state = DragState::OverTarget {
                                data: data_clone,
                                source_id: source_id_val,
                                target_id: new_id,
                                effect,
                                current_x: x,
                                current_y: y,
                            };

                            // Emit leave for old, but we return enter for new.
                            // In a real system we'd queue both; here we prioritize enter.
                            return Some(DragEvent::DragEnter {
                                target_id: new_id,
                                formats,
                            });
                        }
                    }

                    // Fell off all targets — revert to Dragging.
                    self.state = DragState::Dragging {
                        data: data_clone,
                        source_id: source_id_val,
                        allowed_effects: Vec::new(),
                        start_x: x,
                        start_y: y,
                        current_x: x,
                        current_y: y,
                    };

                    Some(DragEvent::DragLeave {
                        target_id: prev_target_id,
                    })
                }
            }
        }
    }

    /// Complete the drag (mouse released).
    ///
    /// If the pointer is over a valid target, a [`DragEvent::Drop`] is
    /// returned. Otherwise, [`DragEvent::DragCancelled`] is returned.
    pub fn end_drag(&mut self) -> Option<DragEvent> {
        let event = match &self.state {
            DragState::Idle => None,
            DragState::Dragging { .. } => {
                // Not over a valid target — cancel.
                if self.threshold_met {
                    Some(DragEvent::DragCancelled)
                } else {
                    // Threshold never met — this was just a click, not a drag.
                    None
                }
            }
            DragState::OverTarget {
                data,
                target_id,
                effect,
                ..
            } => Some(DragEvent::Drop {
                target_id: *target_id,
                data: data.clone(),
                effect: *effect,
            }),
        };

        self.state = DragState::Idle;
        self.threshold_met = false;
        event
    }

    /// Cancel the drag operation.
    ///
    /// Returns [`DragEvent::DragCancelled`] if a drag was in progress,
    /// or `None` if already idle.
    pub fn cancel(&mut self) -> Option<DragEvent> {
        let event = match &self.state {
            DragState::Idle => None,
            _ => Some(DragEvent::DragCancelled),
        };
        self.state = DragState::Idle;
        self.threshold_met = false;
        event
    }

    /// Returns true if a drag operation is currently in progress.
    pub fn is_dragging(&self) -> bool {
        !matches!(self.state, DragState::Idle)
    }

    /// Returns the current drag state.
    pub fn state(&self) -> &DragState {
        &self.state
    }

    /// Hit-test: find the drop target at the given position.
    fn target_at_position(&self, x: f32, y: f32) -> Option<&DropTarget> {
        // Return the last registered target that contains the point
        // (later registrations are "on top").
        self.targets.iter().rev().find(|t| t.contains(x, y))
    }

    /// Check if a target accepts any of the dragged data's formats.
    fn target_accepts_data(&self, target_id: u64, data: &DataObject) -> bool {
        let target = match self.targets.iter().find(|t| t.id == target_id) {
            Some(t) => t,
            None => return false,
        };
        Self::formats_overlap(&target.accepted_formats, data)
    }

    /// Returns true if the target's accepted formats overlap with the data's
    /// available formats.
    fn formats_overlap(accepted: &[DataFormat], data: &DataObject) -> bool {
        accepted.iter().any(|f| data.has_format(f))
    }

    /// Negotiate the best drop effect between source and target.
    ///
    /// Prefers Copy > Move > Link > None, intersecting what the source
    /// allows with what the target supports.
    fn negotiate_effect(&self, source_allowed: &[DropEffect], target_id: u64) -> DropEffect {
        let target = match self.targets.iter().find(|t| t.id == target_id) {
            Some(t) => t,
            None => return DropEffect::None,
        };

        // Priority order for negotiation.
        let priority = [DropEffect::Copy, DropEffect::Move, DropEffect::Link];

        for effect in &priority {
            if source_allowed.contains(effect) && target.allowed_effects.contains(effect) {
                return *effect;
            }
        }

        DropEffect::None
    }

    /// Helper to collect allowed effects when transitioning back to OverTarget
    /// from a data clone.
    fn collect_allowed_from_dragging(
        &self,
        _data: &DataObject,
        _source_id: u64,
    ) -> Vec<DropEffect> {
        // Default to all effects; the negotiate step will narrow it down.
        vec![DropEffect::Copy, DropEffect::Move, DropEffect::Link]
    }
}

impl Default for DragDropManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_object_new_is_empty() {
        let obj = DataObject::new();
        assert!(obj.available_formats().is_empty());
        assert!(!obj.has_format(&DataFormat::PlainText));
    }

    #[test]
    fn data_object_with_text() {
        let obj = DataObject::with_text("hello world");
        assert!(obj.has_format(&DataFormat::PlainText));
        assert_eq!(obj.get_text(), Some("hello world"));
        assert!(!obj.has_format(&DataFormat::Html));
    }

    #[test]
    fn data_object_with_files() {
        let obj = DataObject::with_files(&["/home/user/doc.txt", "/tmp/image.png"]);
        assert!(obj.has_format(&DataFormat::FilePaths));
        let paths = obj.get_file_paths().expect("should have file paths");
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], "/home/user/doc.txt");
        assert_eq!(paths[1], "/tmp/image.png");
    }

    #[test]
    fn data_object_multi_format() {
        let mut obj = DataObject::new();
        obj.set_data(DataFormat::PlainText, b"plain".to_vec());
        obj.set_data(DataFormat::Html, b"<b>bold</b>".to_vec());
        obj.set_data(DataFormat::Url, b"https://example.com".to_vec());

        assert_eq!(obj.available_formats().len(), 3);
        assert_eq!(obj.get_text(), Some("plain"));
        assert_eq!(obj.get_url(), Some("https://example.com"));
        assert_eq!(obj.get_data(&DataFormat::Html), Some(b"<b>bold</b>".as_slice()));
    }

    #[test]
    fn data_object_set_data_replaces_existing() {
        let mut obj = DataObject::with_text("first");
        obj.set_data(DataFormat::PlainText, b"second".to_vec());
        assert_eq!(obj.get_text(), Some("second"));
        // Should still be only one item.
        assert_eq!(obj.available_formats().len(), 1);
    }

    #[test]
    fn data_object_get_data_missing_format() {
        let obj = DataObject::with_text("hello");
        assert_eq!(obj.get_data(&DataFormat::ImagePng), None);
        assert_eq!(obj.get_url(), None);
        assert_eq!(obj.get_file_paths(), None);
    }

    #[test]
    fn drag_manager_register_and_unregister_target() {
        let mut mgr = DragDropManager::new();
        let target = DropTarget {
            id: 1,
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
            accepted_formats: vec![DataFormat::PlainText],
            allowed_effects: vec![DropEffect::Copy],
        };
        mgr.register_target(target);
        assert_eq!(mgr.targets.len(), 1);

        mgr.unregister_target(1);
        assert_eq!(mgr.targets.len(), 0);
    }

    #[test]
    fn drag_manager_threshold_prevents_accidental_drag() {
        let mut mgr = DragDropManager::with_threshold(10.0);
        let data = DataObject::with_text("drag me");

        mgr.begin_drag(1, 50.0, 50.0, data, vec![DropEffect::Copy]);
        assert!(mgr.is_dragging());

        // Small movement within threshold — no event emitted.
        let event = mgr.update_position(53.0, 52.0);
        assert!(event.is_none());

        // End drag without exceeding threshold — no cancel event (was just a click).
        let event = mgr.end_drag();
        assert!(event.is_none());
        assert!(!mgr.is_dragging());
    }

    #[test]
    fn drag_manager_full_lifecycle_with_drop() {
        let mut mgr = DragDropManager::with_threshold(3.0);

        // Register a target.
        mgr.register_target(DropTarget {
            id: 42,
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
            accepted_formats: vec![DataFormat::PlainText, DataFormat::Html],
            allowed_effects: vec![DropEffect::Copy, DropEffect::Move],
        });

        // Start drag.
        let data = DataObject::with_text("hello");
        mgr.begin_drag(1, 50.0, 50.0, data, vec![DropEffect::Copy, DropEffect::Move]);

        // Move past threshold but not over target.
        let event = mgr.update_position(60.0, 60.0);
        assert!(event.is_some());

        // Move over the target.
        let event = mgr.update_position(150.0, 150.0);
        assert!(event.is_some());
        if let Some(DragEvent::DragEnter { target_id, .. }) = event {
            assert_eq!(target_id, 42);
        } else {
            panic!("expected DragEnter event");
        }

        // Drop.
        let event = mgr.end_drag();
        assert!(event.is_some());
        if let Some(DragEvent::Drop {
            target_id, effect, ..
        }) = event
        {
            assert_eq!(target_id, 42);
            assert_eq!(effect, DropEffect::Copy);
        } else {
            panic!("expected Drop event");
        }

        assert!(!mgr.is_dragging());
    }

    #[test]
    fn drag_manager_cancel() {
        let mut mgr = DragDropManager::with_threshold(3.0);
        let data = DataObject::with_text("cancel me");
        mgr.begin_drag(1, 0.0, 0.0, data, vec![DropEffect::Move]);

        // Move past threshold.
        let _ = mgr.update_position(20.0, 20.0);

        let event = mgr.cancel();
        assert!(matches!(event, Some(DragEvent::DragCancelled)));
        assert!(!mgr.is_dragging());
    }

    #[test]
    fn drag_manager_cancel_when_idle() {
        let mut mgr = DragDropManager::new();
        let event = mgr.cancel();
        assert!(event.is_none());
    }

    #[test]
    fn drag_manager_leave_target_on_move_out() {
        let mut mgr = DragDropManager::with_threshold(1.0);

        mgr.register_target(DropTarget {
            id: 10,
            x: 50.0,
            y: 50.0,
            width: 50.0,
            height: 50.0,
            accepted_formats: vec![DataFormat::PlainText],
            allowed_effects: vec![DropEffect::Copy],
        });

        let data = DataObject::with_text("test");
        mgr.begin_drag(1, 0.0, 0.0, data, vec![DropEffect::Copy]);

        // Move past threshold.
        let _ = mgr.update_position(10.0, 10.0);

        // Move into target.
        let event = mgr.update_position(60.0, 60.0);
        assert!(matches!(event, Some(DragEvent::DragEnter { .. })));

        // Move out of target.
        let event = mgr.update_position(200.0, 200.0);
        assert!(matches!(event, Some(DragEvent::DragLeave { target_id: 10 })));
    }

    #[test]
    fn drag_manager_target_rejects_incompatible_format() {
        let mut mgr = DragDropManager::with_threshold(1.0);

        // Target only accepts images.
        mgr.register_target(DropTarget {
            id: 20,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            accepted_formats: vec![DataFormat::ImagePng],
            allowed_effects: vec![DropEffect::Copy],
        });

        // Dragging text — target should not accept.
        let data = DataObject::with_text("no images here");
        mgr.begin_drag(1, 50.0, 50.0, data, vec![DropEffect::Copy]);

        // Move past threshold and over target.
        let event = mgr.update_position(60.0, 60.0);
        // Should get DragMove, not DragEnter, because the target doesn't
        // accept the format.
        assert!(matches!(event, Some(DragEvent::DragMove { .. })));

        // End drag — should cancel since no valid target.
        let event = mgr.end_drag();
        assert!(matches!(event, Some(DragEvent::DragCancelled)));
    }

    #[test]
    fn drop_target_hit_test() {
        let target = DropTarget {
            id: 1,
            x: 10.0,
            y: 20.0,
            width: 50.0,
            height: 30.0,
            accepted_formats: vec![DataFormat::PlainText],
            allowed_effects: vec![DropEffect::Copy],
        };

        // Inside bounds.
        assert!(target.contains(10.0, 20.0));
        assert!(target.contains(35.0, 35.0));
        assert!(target.contains(59.9, 49.9));

        // Outside bounds.
        assert!(!target.contains(9.9, 20.0));
        assert!(!target.contains(10.0, 19.9));
        assert!(!target.contains(60.0, 35.0));
        assert!(!target.contains(35.0, 50.0));
    }

    #[test]
    fn data_object_custom_format() {
        let mut obj = DataObject::new();
        let custom = DataFormat::Custom(String::from("application/x-my-widget"));
        obj.set_data(custom.clone(), vec![1, 2, 3, 4]);

        assert!(obj.has_format(&custom));
        assert_eq!(obj.get_data(&custom), Some([1u8, 2, 3, 4].as_slice()));
        assert!(!obj.has_format(&DataFormat::Custom(String::from("other/format"))));
    }
}
