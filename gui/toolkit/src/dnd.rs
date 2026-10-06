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
//! DragDropManager (Idle → Pending → Dragging ⇄ OverTarget → Drop/Cancel)
//!     │  (hit-tests registered targets, negotiates the effect)
//!     ▼
//! Target Widget
//!     (receives DataObject, picks preferred format)
//! ```
//!
//! # Usage
//!
//! 1. Register drop targets via [`DragDropManager::register_target`], each
//!    with the formats it takes and the effects it can have, in the order it
//!    prefers them.
//! 2. When the user presses on something that can be dragged, call
//!    [`DragDropManager::begin_drag`] with a populated [`DataObject`] and the
//!    effects the source allows.
//! 3. Feed pointer moves to [`DragDropManager::update_position`] and the
//!    keyboard's modifiers to [`DragDropManager::set_keys`]. Each returns the
//!    [`DragEvent`]s the change caused, in order: the drag starting once the
//!    pointer has gone [far enough](DragDropManager::with_threshold), a target
//!    left *before* the next is entered, and a target's effect changing under
//!    a key.
//! 4. On release, [`DragDropManager::end_drag`] drops the data on the target
//!    under the pointer, or cancels; Escape is [`DragDropManager::cancel`].
//!
//! # What a drop does
//!
//! Each drag has one [`DropEffect`] at a time, worked out by [`negotiate`]
//! from three parties: the effects the source allows (a read-only file can be
//! copied, not moved), the effects the target can have, in its order of
//! preference (a folder moves what is dropped on it, a document copies it in),
//! and the keys held, which ask for one effect by name as on every desktop
//! ([`DragKeys`]: Ctrl copies, Shift moves, Ctrl+Shift or Alt links). Keys
//! that ask for an effect the source or the target cannot have make the drop
//! do nothing -- a target shows "not allowed" -- rather than quietly doing
//! another effect than the one asked for: a user holding Ctrl to keep the
//! original must not find it moved.

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
    /// File paths, as bytes, separated by NUL ([`DataObject::with_files`]).
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
    /// Paths are **bytes**, separated by NUL.
    ///
    /// # Why bytes, and why NUL
    ///
    /// A SlateOS filename may hold every byte except `/` and NUL
    /// (`design.txt`), so neither half of the old signature worked: paths were
    /// `&str`, which cannot express such a name at all, and they were joined
    /// on `\n`, which is a *legal* character in one -- a file called
    /// `notes<LF>draft.txt` arrived as two paths that do not exist. NUL is the
    /// one byte that cannot occur, which is exactly what makes it the only
    /// unambiguous separator, and is why `find -print0` exists.
    ///
    /// # This is not a new decision
    ///
    /// It is the format `kernel/src/fs/clipboard.rs` already uses --
    /// `set_files(&[&[u8]])`, NUL-separated, `get_files() -> Vec<Vec<u8>>` --
    /// settled on 2026-09-07 in answer to lane C's own request
    /// `c-a-the-system-clipboards-file-list-cannot-carry-our-own-paths.md`.
    /// A clipboard and a drag are the same problem (a list of files crossing a
    /// process boundary) and two formats for it that disagreed would be a
    /// third place deciding what a path is.
    ///
    /// Returning raw bytes rather than `PathBuf` is the part that makes this
    /// sound everywhere. On SlateOS an `OsStr` is bytes and the conversion is
    /// free; on the Windows host it is WTF-8 and bytes arriving from another
    /// process are not necessarily valid, so a conversion here would be
    /// unsound exactly where the tests run. Handing back bytes leaves that to
    /// the caller, which knows its platform.
    pub fn with_files(paths: &[&[u8]]) -> Self {
        let mut obj = Self::new();
        let mut joined: Vec<u8> = Vec::new();
        for (i, path) in paths.iter().enumerate() {
            if i > 0 {
                joined.push(0);
            }
            joined.extend_from_slice(path);
        }
        obj.set_data(DataFormat::FilePaths, joined);
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

    /// Retrieves the file paths, as bytes.
    ///
    /// `None` when no `FilePaths` data is set. Empty runs are dropped, so a
    /// trailing separator does not produce a path with no name.
    ///
    /// No UTF-8 validation: see [`Self::with_files`]. The previous version
    /// validated the *whole* blob and returned `None` if any part of it
    /// failed, so a single awkward filename made the receiving application see
    /// nothing at all -- not "the odd file is missing" but "the drop did
    /// nothing", with no clue which file caused it.
    #[must_use]
    pub fn get_file_paths(&self) -> Option<Vec<&[u8]>> {
        self.get_data(&DataFormat::FilePaths)
            .map(|bytes| bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).collect())
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

/// What a drop does with the data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropEffect {
    /// Nothing: the drop is not allowed here, as things stand.
    None,
    /// The target gets a copy; the source keeps its own.
    Copy,
    /// The target gets the data and the source gives it up -- removes it
    /// once the drop is done.
    Move,
    /// The target makes a link or shortcut to the data, which stays where it
    /// is.
    Link,
}

/// The modifier keys held during a drag, which ask for one effect by name as
/// on every desktop: Ctrl copies, Shift moves, and Ctrl+Shift links -- or
/// Alt, as Windows also has it. With none held the target chooses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DragKeys {
    /// A Ctrl key is down.
    pub ctrl: bool,
    /// A Shift key is down.
    pub shift: bool,
    /// An Alt key is down.
    pub alt: bool,
}

impl DragKeys {
    /// The effect these keys ask for, or `None` when they leave it to the
    /// target.
    #[must_use]
    pub fn asked(self) -> Option<DropEffect> {
        match (self.ctrl, self.shift, self.alt) {
            (true, true, _) | (_, _, true) => Some(DropEffect::Link),
            (true, false, false) => Some(DropEffect::Copy),
            (false, true, false) => Some(DropEffect::Move),
            (false, false, false) => None,
        }
    }
}

/// What a drop would do, given the effects the `source` allows, the effects
/// the `target` can have in the order it prefers them, and the `keys` held.
///
/// The effect the keys ask for when both sides can have it -- and
/// [`DropEffect::None`] when either cannot, rather than another effect than
/// the one asked for (see the module's docs). With no keys, the first effect
/// in the target's order that the source allows; `None` when there is none.
#[must_use]
pub fn negotiate(source: &[DropEffect], target: &[DropEffect], keys: DragKeys) -> DropEffect {
    let possible = |effect: &DropEffect| {
        *effect != DropEffect::None && source.contains(effect) && target.contains(effect)
    };
    match keys.asked() {
        Some(asked) if possible(&asked) => asked,
        Some(_) => DropEffect::None,
        None => target
            .iter()
            .copied()
            .find(|effect| possible(effect))
            .unwrap_or(DropEffect::None),
    }
}

/// Where a drag stands: [`DragDropManager::state`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragState {
    /// No drag: nothing pressed on a source.
    Idle,
    /// Pressed on a source, the pointer not yet gone far enough for a drag:
    /// a click, so far.
    Pending,
    /// Dragging, over nothing that takes the data.
    Dragging,
    /// Dragging over a target that takes the data.
    OverTarget {
        /// The target under the pointer.
        target_id: u64,
        /// What a drop there would do -- [`DropEffect::None`] when the
        /// source, the target and the keys held agree on nothing.
        effect: DropEffect,
    },
}

/// What a change to a drag caused, for the source and the targets to show:
/// returned in order by every call that can change a drag.
#[derive(Clone, Debug)]
pub enum DragEvent {
    /// The pointer went far enough from the press for a drag: from here on,
    /// letting go is a drop or a cancel, not a click.
    DragStart {
        /// The source the drag began on.
        source_id: u64,
    },
    /// The pointer moved during the drag.
    DragMove {
        /// Where it is now.
        x: f32,
        /// Where it is now.
        y: f32,
    },
    /// The pointer came over a target that takes the data. Every enter is
    /// followed by a [`DragLeave`](Self::DragLeave) or a
    /// [`Drop`](Self::Drop) on the same target before any other enter.
    DragEnter {
        /// The target entered.
        target_id: u64,
        /// The formats the data is offered in.
        formats: Vec<DataFormat>,
        /// What a drop here would do.
        effect: DropEffect,
    },
    /// What a drop on the target under the pointer would do changed, by a
    /// key pressed or let go, or the target's effects re-registered.
    EffectChanged {
        /// The target under the pointer.
        target_id: u64,
        /// What a drop there would do now.
        effect: DropEffect,
    },
    /// The pointer left a target, or the drag ended over it without a drop.
    DragLeave {
        /// The target left.
        target_id: u64,
    },
    /// The data was let go on a target, to do `effect` -- never
    /// [`DropEffect::None`].
    Drop {
        /// The target receiving the drop.
        target_id: u64,
        /// The data, given up by the drag.
        data: DataObject,
        /// What the drop does.
        effect: DropEffect,
    },
    /// The drag ended with no drop: let go over nothing that takes the data,
    /// or where the drop would do nothing, or given up (Escape).
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
    /// The effects a drop here can have, in the order the target prefers
    /// them: the first the source allows is what a drop does when no key
    /// asks for another ([`negotiate`]).
    pub allowed_effects: Vec<DropEffect>,
}

impl DropTarget {
    /// Returns true if the point (px, py) lies within this target's bounds.
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// A drag under way: [`DragDropManager`]'s whole state between a press and
/// its release.
#[derive(Debug)]
struct Drag {
    /// What is dragged.
    data: DataObject,
    /// The source it was pressed on.
    source_id: u64,
    /// The effects the source allows, kept for the whole drag: every target
    /// entered is negotiated against these.
    allowed: Vec<DropEffect>,
    /// Where it was pressed.
    start: (f32, f32),
    /// Where the pointer is.
    at: (f32, f32),
    /// Whether the pointer has gone past the threshold: until then a press.
    started: bool,
    /// The target under the pointer that takes the data, and what a drop on
    /// it would do.
    over: Option<(u64, DropEffect)>,
}

/// Manages the drag-and-drop lifecycle.
///
/// Tracks the drag, the registered drop targets and the keys held, and
/// produces [`DragEvent`]s as the drag progresses. The manager enforces a
/// minimum movement threshold to prevent accidental drags from simple clicks.
pub struct DragDropManager {
    /// The drag under way, from the press to the release.
    drag: Option<Drag>,
    /// Minimum pixels the pointer must move before a press is a drag.
    drag_threshold: f32,
    /// Registered drop targets, the last registered on top.
    targets: Vec<DropTarget>,
    /// The modifier keys held, as last told.
    keys: DragKeys,
}

impl DragDropManager {
    /// Creates a new manager with a default drag threshold of 5 pixels.
    #[must_use]
    pub fn new() -> Self {
        Self::with_threshold(5.0)
    }

    /// Creates a new manager with a custom drag threshold.
    #[must_use]
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            drag: None,
            drag_threshold: threshold,
            targets: Vec::new(),
            keys: DragKeys::default(),
        }
    }

    /// Registers a drop target. If a target with the same ID already exists,
    /// it is replaced. A drag under way sees the change at its next move.
    pub fn register_target(&mut self, target: DropTarget) {
        self.unregister_target(target.id);
        self.targets.push(target);
    }

    /// Removes a drop target by ID. A drag over it is told it left at its
    /// next move, or when it ends.
    pub fn unregister_target(&mut self, id: u64) {
        self.targets.retain(|t| t.id != id);
    }

    /// A press on the source `source_id` at (`x`, `y`) that may become a
    /// drag of `data`, which the source allows to have the effects `allowed`.
    ///
    /// Not a drag until the pointer moves past the threshold: a click on
    /// something draggable must stay a click. A drag already under way --
    /// a caller's slip, as a press cannot come before the last release -- is
    /// cancelled first, and its events returned.
    pub fn begin_drag(
        &mut self,
        source_id: u64,
        x: f32,
        y: f32,
        data: DataObject,
        allowed: Vec<DropEffect>,
    ) -> Vec<DragEvent> {
        let ended = self.cancel();
        self.drag = Some(Drag {
            data,
            source_id,
            allowed,
            start: (x, y),
            at: (x, y),
            started: false,
            over: None,
        });
        ended
    }

    /// The pointer moved to (`x`, `y`): what that changed, in order -- the
    /// drag starting, the target left, the target entered or its effect
    /// changed, and the move itself.
    pub fn update_position(&mut self, x: f32, y: f32) -> Vec<DragEvent> {
        let mut events = Vec::new();
        let Some(drag) = self.drag.as_mut() else {
            return events;
        };
        drag.at = (x, y);
        if !drag.started {
            let (dx, dy) = (x - drag.start.0, y - drag.start.1);
            if dx * dx + dy * dy < self.drag_threshold * self.drag_threshold {
                return events;
            }
            drag.started = true;
            events.push(DragEvent::DragStart {
                source_id: drag.source_id,
            });
        }
        Self::retarget(drag, &self.targets, self.keys, &mut events);
        events.push(DragEvent::DragMove { x, y });
        events
    }

    /// The modifier keys held are now `keys`: what that changed -- at most
    /// the effect a drop on the target under the pointer would have.
    pub fn set_keys(&mut self, keys: DragKeys) -> Vec<DragEvent> {
        self.keys = keys;
        let mut events = Vec::new();
        if let Some(drag) = self.drag.as_mut()
            && drag.started
        {
            Self::retarget(drag, &self.targets, keys, &mut events);
        }
        events
    }

    /// The pointer was let go at (`x`, `y`): the data dropped on the target
    /// there, if a drop there does something, and the drag cancelled
    /// otherwise -- after whatever the move to (`x`, `y`) changed. Nothing at
    /// all for a press that never became a drag: that was a click.
    pub fn end_drag(&mut self, x: f32, y: f32) -> Vec<DragEvent> {
        let mut events = self.update_position(x, y);
        let Some(drag) = self.drag.take() else {
            return events;
        };
        if !drag.started {
            return events;
        }
        match drag.over {
            Some((target_id, effect)) if effect != DropEffect::None => {
                events.push(DragEvent::Drop {
                    target_id,
                    data: drag.data,
                    effect,
                });
            }
            Some((target_id, _)) => {
                events.push(DragEvent::DragLeave { target_id });
                events.push(DragEvent::DragCancelled);
            }
            None => events.push(DragEvent::DragCancelled),
        }
        events
    }

    /// Give the drag up (Escape): the target under the pointer is left and
    /// the drag cancelled. Nothing for a press that never became a drag, or
    /// with no drag at all.
    pub fn cancel(&mut self) -> Vec<DragEvent> {
        let Some(drag) = self.drag.take() else {
            return Vec::new();
        };
        if !drag.started {
            return Vec::new();
        }
        let mut events = Vec::new();
        if let Some((target_id, _)) = drag.over {
            events.push(DragEvent::DragLeave { target_id });
        }
        events.push(DragEvent::DragCancelled);
        events
    }

    /// Whether a drag is under way: pressed and moved past the threshold. A
    /// press that has not moved that far is [`DragState::Pending`] and not a
    /// drag yet.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|drag| drag.started)
    }

    /// Where the drag stands.
    #[must_use]
    pub fn state(&self) -> DragState {
        match &self.drag {
            None => DragState::Idle,
            Some(drag) if !drag.started => DragState::Pending,
            Some(Drag {
                over: Some((target_id, effect)),
                ..
            }) => DragState::OverTarget {
                target_id: *target_id,
                effect: *effect,
            },
            Some(_) => DragState::Dragging,
        }
    }

    /// What is being dragged, from the press to the release.
    #[must_use]
    pub fn data(&self) -> Option<&DataObject> {
        self.drag.as_ref().map(|drag| &drag.data)
    }

    /// The source the drag began on.
    #[must_use]
    pub fn source(&self) -> Option<u64> {
        self.drag.as_ref().map(|drag| drag.source_id)
    }

    /// Where the pointer is in the drag.
    #[must_use]
    pub fn position(&self) -> Option<(f32, f32)> {
        self.drag.as_ref().map(|drag| drag.at)
    }

    /// The modifier keys held, as last told.
    #[must_use]
    pub fn keys(&self) -> DragKeys {
        self.keys
    }

    /// The target at the drag's position that takes its data, and what a
    /// drop there would do -- compared with the one it was over, and the
    /// difference said in `events`: a leave before an enter, or the effect
    /// changed.
    ///
    /// The topmost target at the point decides. One that does not take the
    /// data hides any under it, as a window hides the windows behind it: a
    /// drop must land on what the user sees under the pointer.
    fn retarget(
        drag: &mut Drag,
        targets: &[DropTarget],
        keys: DragKeys,
        events: &mut Vec<DragEvent>,
    ) {
        let (x, y) = drag.at;
        let now = targets
            .iter()
            .rev()
            .find(|target| target.contains(x, y))
            .filter(|target| {
                target
                    .accepted_formats
                    .iter()
                    .any(|format| drag.data.has_format(format))
            })
            .map(|target| {
                (
                    target.id,
                    negotiate(&drag.allowed, &target.allowed_effects, keys),
                )
            });
        match (drag.over, now) {
            (Some((was, before)), Some((target_id, effect))) if was == target_id => {
                if before != effect {
                    events.push(DragEvent::EffectChanged { target_id, effect });
                }
            }
            (was, now) => {
                if let Some((target_id, _)) = was {
                    events.push(DragEvent::DragLeave { target_id });
                }
                if let Some((target_id, effect)) = now {
                    events.push(DragEvent::DragEnter {
                        target_id,
                        formats: drag.data.available_formats().into_iter().cloned().collect(),
                        effect,
                    });
                }
            }
        }
        drag.over = now;
    }
}

impl Default for DragDropManager {
    fn default() -> Self {
        Self::new()
    }
}

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
        clippy::arithmetic_side_effects,
        clippy::float_cmp
    )]

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

    /// A filename containing a newline survives the round trip.
    ///
    /// The bug this separator change fixes. A newline is legal in a name here,
    /// so joining on one split a single real file into two paths that do not
    /// exist -- and the failure would have surfaced as "no such file" for a
    /// file plainly visible on screen.
    #[test]
    fn a_name_containing_a_newline_is_one_path_not_two() {
        let obj = DataObject::with_files(&[b"/home/user/notes\ndraft.txt"]);
        let paths = obj.get_file_paths().expect("file paths");
        assert_eq!(paths.len(), 1, "a legal filename was split: {paths:?}");
        assert_eq!(paths[0], b"/home/user/notes\ndraft.txt");
    }

    #[test]
    fn data_object_with_files() {
        let obj = DataObject::with_files(&[b"/home/user/doc.txt", b"/tmp/image.png"]);
        assert!(obj.has_format(&DataFormat::FilePaths));
        let paths = obj.get_file_paths().expect("should have file paths");
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], b"/home/user/doc.txt");
        assert_eq!(paths[1], b"/tmp/image.png");

        // Round trip: what comes out joins back to what went in.
        let rejoined: Vec<u8> = paths.join(&0u8);
        assert_eq!(
            obj.get_data(&DataFormat::FilePaths),
            Some(rejoined.as_slice())
        );
    }

    /// A name that is not text survives the drag.
    ///
    /// The reason the signature is bytes. Such a name is legal here --
    /// `design.txt` allows every byte but `/` and NUL -- and the old `&str`
    /// could not express one at all.
    #[test]
    fn a_path_that_is_not_utf8_survives() {
        // 0xFF is not valid UTF-8 in any position.
        let odd: &[u8] = b"/home/user/report\xFF.txt";
        // No runtime check that this is really undecodable: it is a literal,
        // so the compiler knows, and clippy says so outright if it ever
        // becomes valid UTF-8. A guard the compiler can prove is a guard
        // better placed in the compiler.

        let obj = DataObject::with_files(&[odd]);
        let paths = obj.get_file_paths().expect("file paths");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], odd, "the path was altered in transit");
    }

    /// One undecodable name does not lose the others.
    ///
    /// The sharp edge of the old version: it validated the WHOLE blob as UTF-8
    /// and returned `None` if any part failed, so a single awkward filename
    /// made the drop do nothing at all, with no indication which file was
    /// responsible.
    #[test]
    fn one_odd_name_does_not_lose_the_rest() {
        let obj = DataObject::with_files(&[
            b"/home/user/a.txt",
            b"/home/user/b\xFF.txt",
            b"/home/user/c.txt",
        ]);
        let paths = obj.get_file_paths().expect("file paths");
        assert_eq!(
            paths.len(),
            3,
            "an undecodable name took the whole selection with it: {paths:?}"
        );
        assert_eq!(paths[0], b"/home/user/a.txt");
        assert_eq!(paths[2], b"/home/user/c.txt");
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
        assert_eq!(
            obj.get_data(&DataFormat::Html),
            Some(b"<b>bold</b>".as_slice())
        );
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
        mgr.register_target(text_target(1, 10.0, 10.0, &[DropEffect::Copy]));
        assert_eq!(mgr.targets.len(), 1);

        mgr.unregister_target(1);
        assert_eq!(mgr.targets.len(), 0);
    }

    /// A target `id` 50 pixels square at (`x`, `y`), taking text, its
    /// effects `effects` in the order it prefers them.
    fn text_target(id: u64, x: f32, y: f32, effects: &[DropEffect]) -> DropTarget {
        DropTarget {
            id,
            x,
            y,
            width: 50.0,
            height: 50.0,
            accepted_formats: vec![DataFormat::PlainText],
            allowed_effects: effects.to_vec(),
        }
    }

    /// Every effect, for a source or a target with no opinion.
    const ALL: [DropEffect; 3] = [DropEffect::Copy, DropEffect::Move, DropEffect::Link];

    /// The events, each as a word and its target or effect, so a whole
    /// sequence can be compared at once.
    fn said(events: &[DragEvent]) -> Vec<String> {
        events
            .iter()
            .map(|event| match event {
                DragEvent::DragStart { source_id } => format!("start {source_id}"),
                DragEvent::DragMove { .. } => "move".to_owned(),
                DragEvent::DragEnter {
                    target_id, effect, ..
                } => format!("enter {target_id} {effect:?}"),
                DragEvent::EffectChanged { target_id, effect } => {
                    format!("effect {target_id} {effect:?}")
                }
                DragEvent::DragLeave { target_id } => format!("leave {target_id}"),
                DragEvent::Drop {
                    target_id, effect, ..
                } => format!("drop {target_id} {effect:?}"),
                DragEvent::DragCancelled => "cancelled".to_owned(),
            })
            .collect()
    }

    /// A manager with the targets `targets`, a drag of text begun on source
    /// 7 at (-100, -100), clear of every target, that the source allows to
    /// have `allowed`, and moved past the threshold, to (-99, -99).
    fn dragging(targets: Vec<DropTarget>, allowed: &[DropEffect]) -> DragDropManager {
        let mut mgr = DragDropManager::with_threshold(1.0);
        for target in targets {
            mgr.register_target(target);
        }
        let ended = mgr.begin_drag(
            7,
            -100.0,
            -100.0,
            DataObject::with_text("dragged"),
            allowed.to_vec(),
        );
        assert!(ended.is_empty());
        assert_eq!(
            said(&mgr.update_position(-99.0, -99.0)),
            ["start 7", "move"]
        );
        mgr
    }

    /// **A press that does not go past the threshold is a click**: nothing
    /// is said, it is not a drag, and letting go says nothing either.
    #[test]
    fn drag_manager_threshold_prevents_accidental_drag() {
        let mut mgr = DragDropManager::with_threshold(10.0);
        let ended = mgr.begin_drag(
            1,
            50.0,
            50.0,
            DataObject::with_text("drag me"),
            vec![DropEffect::Copy],
        );
        assert!(ended.is_empty());
        assert_eq!(mgr.state(), DragState::Pending);

        assert!(mgr.update_position(53.0, 52.0).is_empty());
        assert!(!mgr.is_dragging());
        assert_eq!(mgr.state(), DragState::Pending);
        assert_eq!(mgr.position(), Some((53.0, 52.0)));

        assert!(mgr.end_drag(53.0, 52.0).is_empty());
        assert_eq!(mgr.state(), DragState::Idle);
        assert!(mgr.data().is_none());

        // Moved exactly the threshold -- 6 across and 8 down is 10 -- it is
        // a drag.
        let ended = mgr.begin_drag(2, 0.0, 0.0, DataObject::with_text("x"), ALL.to_vec());
        assert!(ended.is_empty());
        assert_eq!(said(&mgr.update_position(6.0, 8.0)), ["start 2", "move"]);
    }

    /// **A drag starts, enters its target and is dropped there**, with the
    /// target's first effect the source allows.
    #[test]
    fn drag_manager_full_lifecycle_with_drop() {
        let mut mgr = DragDropManager::with_threshold(3.0);
        mgr.register_target(DropTarget {
            id: 42,
            x: 100.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
            accepted_formats: vec![DataFormat::PlainText, DataFormat::Html],
            allowed_effects: vec![DropEffect::Copy, DropEffect::Move],
        });
        let ended = mgr.begin_drag(
            1,
            50.0,
            50.0,
            DataObject::with_text("hello"),
            vec![DropEffect::Copy, DropEffect::Move],
        );
        assert!(ended.is_empty());
        assert_eq!(mgr.source(), Some(1));

        assert_eq!(said(&mgr.update_position(60.0, 60.0)), ["start 1", "move"]);
        assert!(mgr.is_dragging());
        assert_eq!(mgr.state(), DragState::Dragging);

        let events = mgr.update_position(150.0, 150.0);
        assert_eq!(said(&events), ["enter 42 Copy", "move"]);
        let DragEvent::DragEnter { formats, .. } = &events[0] else {
            panic!("expected an enter: {events:?}");
        };
        assert_eq!(formats, &[DataFormat::PlainText]);
        assert_eq!(
            mgr.state(),
            DragState::OverTarget {
                target_id: 42,
                effect: DropEffect::Copy
            }
        );

        let events = mgr.end_drag(150.0, 150.0);
        assert_eq!(said(&events), ["move", "drop 42 Copy"]);
        let Some(DragEvent::Drop { data, .. }) = events.last() else {
            panic!("expected a drop: {events:?}");
        };
        assert_eq!(data.get_text(), Some("hello"));
        assert!(!mgr.is_dragging());
        assert_eq!(mgr.state(), DragState::Idle);
    }

    /// **Moving straight from one target to the next leaves the first before
    /// entering the second** -- which the manager once did not say at all,
    /// so the first kept its highlight for the rest of the drag.
    #[test]
    fn moving_between_targets_leaves_one_before_entering_the_next() {
        let mut mgr = dragging(
            vec![
                text_target(1, 0.0, 0.0, &ALL),
                text_target(2, 50.0, 0.0, &ALL),
            ],
            &ALL,
        );
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Copy", "move"]
        );
        assert_eq!(
            said(&mgr.update_position(60.0, 10.0)),
            ["leave 1", "enter 2 Copy", "move"]
        );
        assert_eq!(said(&mgr.update_position(61.0, 10.0)), ["move"]);
        assert_eq!(said(&mgr.update_position(200.0, 10.0)), ["leave 2", "move"]);
    }

    /// **The source's effects hold for the whole drag**: a source that only
    /// moves is moved onto a target that prefers to copy, after leaving
    /// another target on the way -- the manager once forgot what the source
    /// allowed on the way out of a target and offered every effect after.
    #[test]
    fn the_sources_effects_hold_for_the_whole_drag() {
        let mut mgr = dragging(
            vec![
                text_target(1, 0.0, 0.0, &[DropEffect::Move]),
                text_target(2, 100.0, 0.0, &[DropEffect::Copy, DropEffect::Move]),
            ],
            &[DropEffect::Move],
        );
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Move", "move"]
        );
        assert_eq!(said(&mgr.update_position(75.0, 10.0)), ["leave 1", "move"]);
        assert_eq!(
            said(&mgr.update_position(110.0, 10.0)),
            ["enter 2 Move", "move"]
        );
        assert_eq!(said(&mgr.end_drag(110.0, 10.0)), ["move", "drop 2 Move"]);
    }

    /// **Keys ask for an effect**, and letting go of them gives the choice
    /// back to the target.
    #[test]
    fn keys_ask_for_an_effect() {
        let mut mgr = dragging(
            vec![text_target(
                1,
                0.0,
                0.0,
                &[DropEffect::Move, DropEffect::Copy, DropEffect::Link],
            )],
            &ALL,
        );
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Move", "move"]
        );
        let keys = |ctrl, shift, alt| DragKeys { ctrl, shift, alt };
        assert_eq!(
            said(&mgr.set_keys(keys(true, false, false))),
            ["effect 1 Copy"]
        );
        assert_eq!(
            said(&mgr.set_keys(keys(false, true, false))),
            ["effect 1 Move"]
        );
        assert_eq!(
            said(&mgr.set_keys(keys(true, true, false))),
            ["effect 1 Link"]
        );
        assert!(
            mgr.set_keys(keys(false, false, true)).is_empty(),
            "Alt links too: no change"
        );
        assert_eq!(mgr.keys(), keys(false, false, true));
        assert_eq!(
            said(&mgr.set_keys(keys(false, false, false))),
            ["effect 1 Move"]
        );
        assert_eq!(
            said(&mgr.set_keys(keys(true, false, false))),
            ["effect 1 Copy"]
        );
        assert_eq!(said(&mgr.end_drag(10.0, 10.0)), ["move", "drop 1 Copy"]);
    }

    /// **Keys held before the drag count from its first target**, and keys
    /// with no drag change nothing.
    #[test]
    fn keys_held_before_the_drag_count_from_its_first_target() {
        let mut mgr = DragDropManager::with_threshold(1.0);
        mgr.register_target(text_target(
            1,
            0.0,
            0.0,
            &[DropEffect::Move, DropEffect::Copy],
        ));
        let ctrl = DragKeys {
            ctrl: true,
            ..DragKeys::default()
        };
        assert!(mgr.set_keys(ctrl).is_empty());
        let ended = mgr.begin_drag(3, 0.0, 0.0, DataObject::with_text("x"), ALL.to_vec());
        assert!(ended.is_empty());
        assert!(mgr.set_keys(ctrl).is_empty(), "a press is not a drag yet");
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["start 3", "enter 1 Copy", "move"]
        );
    }

    /// **Keys asking for what cannot be done make the drop do nothing**,
    /// rather than another effect than the one asked for: the target hears
    /// it is left, and the drag is cancelled.
    #[test]
    fn keys_asking_for_what_cannot_be_done_drop_nothing() {
        let mut mgr = dragging(
            vec![text_target(
                1,
                0.0,
                0.0,
                &[DropEffect::Move, DropEffect::Copy],
            )],
            &[DropEffect::Copy, DropEffect::Move],
        );
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Move", "move"]
        );
        let link = DragKeys {
            ctrl: true,
            shift: true,
            alt: false,
        };
        assert_eq!(said(&mgr.set_keys(link)), ["effect 1 None"]);
        assert_eq!(
            mgr.state(),
            DragState::OverTarget {
                target_id: 1,
                effect: DropEffect::None
            }
        );
        assert_eq!(
            said(&mgr.end_drag(10.0, 10.0)),
            ["move", "leave 1", "cancelled"]
        );
    }

    /// **A target whose effects the source does not share is entered and
    /// takes no drop**: it can show "not allowed" while the pointer is on it.
    #[test]
    fn a_target_sharing_no_effect_takes_no_drop() {
        let mut mgr = dragging(
            vec![text_target(1, 0.0, 0.0, &[DropEffect::Move])],
            &[DropEffect::Copy],
        );
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 None", "move"]
        );
        assert_eq!(
            said(&mgr.end_drag(10.0, 10.0)),
            ["move", "leave 1", "cancelled"]
        );
    }

    #[test]
    fn drag_manager_cancel() {
        let mut mgr = DragDropManager::with_threshold(3.0);
        let ended = mgr.begin_drag(
            1,
            0.0,
            0.0,
            DataObject::with_text("cancel me"),
            vec![DropEffect::Move],
        );
        assert!(ended.is_empty());
        assert_eq!(said(&mgr.update_position(20.0, 20.0)), ["start 1", "move"]);
        assert_eq!(said(&mgr.cancel()), ["cancelled"]);
        assert!(!mgr.is_dragging());
        assert_eq!(mgr.state(), DragState::Idle);
    }

    /// **Escape over a target leaves it, then cancels**; a press that never
    /// became a drag, or no drag at all, cancels nothing.
    #[test]
    fn a_cancel_over_a_target_leaves_it() {
        let mut mgr = dragging(vec![text_target(1, 0.0, 0.0, &ALL)], &ALL);
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Copy", "move"]
        );
        assert_eq!(said(&mgr.cancel()), ["leave 1", "cancelled"]);
        assert!(mgr.cancel().is_empty(), "nothing left to cancel");

        let ended = mgr.begin_drag(2, 0.0, 0.0, DataObject::with_text("x"), ALL.to_vec());
        assert!(ended.is_empty());
        assert!(mgr.cancel().is_empty(), "a press, not a drag");
        assert_eq!(mgr.state(), DragState::Idle);
    }

    #[test]
    fn drag_manager_cancel_when_idle() {
        let mut mgr = DragDropManager::new();
        assert!(mgr.cancel().is_empty());
        assert!(mgr.end_drag(0.0, 0.0).is_empty());
        assert!(mgr.update_position(5.0, 5.0).is_empty());
    }

    #[test]
    fn drag_manager_leave_target_on_move_out() {
        let mut mgr = dragging(
            vec![text_target(10, 50.0, 50.0, &[DropEffect::Copy])],
            &[DropEffect::Copy],
        );
        assert_eq!(
            said(&mgr.update_position(60.0, 60.0)),
            ["enter 10 Copy", "move"]
        );
        assert_eq!(
            said(&mgr.update_position(200.0, 200.0)),
            ["leave 10", "move"]
        );
        assert_eq!(mgr.state(), DragState::Dragging);
    }

    /// **The topmost target decides**: one that does not take the data hides
    /// a target under it that would, and a drop there does nothing.
    #[test]
    fn drag_manager_target_rejects_incompatible_format() {
        let mut mgr = DragDropManager::with_threshold(1.0);
        mgr.register_target(text_target(19, 0.0, 0.0, &ALL));
        // On top of it, a target that only takes images.
        mgr.register_target(DropTarget {
            id: 20,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            accepted_formats: vec![DataFormat::ImagePng],
            allowed_effects: vec![DropEffect::Copy],
        });
        let ended = mgr.begin_drag(
            1,
            50.0,
            50.0,
            DataObject::with_text("no images here"),
            vec![DropEffect::Copy],
        );
        assert!(ended.is_empty());
        assert_eq!(said(&mgr.update_position(10.0, 10.0)), ["start 1", "move"]);
        assert_eq!(mgr.state(), DragState::Dragging);
        assert_eq!(said(&mgr.end_drag(10.0, 10.0)), ["move", "cancelled"]);
    }

    /// **A target taken away under the drag is left** at the next move, and
    /// one put back with other effects says its new effect.
    #[test]
    fn a_target_changed_under_the_drag_says_so() {
        let mut mgr = dragging(vec![text_target(1, 0.0, 0.0, &[DropEffect::Copy])], &ALL);
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Copy", "move"]
        );
        mgr.register_target(text_target(1, 0.0, 0.0, &[DropEffect::Link]));
        assert_eq!(
            said(&mgr.update_position(11.0, 10.0)),
            ["effect 1 Link", "move"]
        );
        mgr.unregister_target(1);
        assert_eq!(said(&mgr.update_position(12.0, 10.0)), ["leave 1", "move"]);
        // Taken away and never moved over again: the release says so.
        mgr.register_target(text_target(1, 0.0, 0.0, &[DropEffect::Copy]));
        assert_eq!(
            said(&mgr.update_position(13.0, 10.0)),
            ["enter 1 Copy", "move"]
        );
        mgr.unregister_target(1);
        assert_eq!(
            said(&mgr.end_drag(13.0, 10.0)),
            ["leave 1", "move", "cancelled"]
        );
    }

    /// **A release far from its press, with no move between, is a drag** --
    /// the release's position counts, not the last move's.
    #[test]
    fn a_release_far_from_the_press_is_a_drop() {
        let mut mgr = DragDropManager::with_threshold(5.0);
        mgr.register_target(text_target(4, 100.0, 100.0, &[DropEffect::Copy]));
        let ended = mgr.begin_drag(9, 0.0, 0.0, DataObject::with_text("x"), ALL.to_vec());
        assert!(ended.is_empty());
        assert_eq!(
            said(&mgr.end_drag(110.0, 110.0)),
            ["start 9", "enter 4 Copy", "move", "drop 4 Copy"]
        );
    }

    /// **A press during a drag ends the drag first**, and says so.
    #[test]
    fn a_press_during_a_drag_ends_it_first() {
        let mut mgr = dragging(vec![text_target(1, 0.0, 0.0, &ALL)], &ALL);
        assert_eq!(
            said(&mgr.update_position(10.0, 10.0)),
            ["enter 1 Copy", "move"]
        );
        let ended = mgr.begin_drag(8, 10.0, 10.0, DataObject::with_text("y"), ALL.to_vec());
        assert_eq!(said(&ended), ["leave 1", "cancelled"]);
        assert_eq!(mgr.source(), Some(8));
        assert_eq!(mgr.state(), DragState::Pending);
    }

    /// **The effect is negotiated in the target's order, within what the
    /// source allows, unless keys ask** -- and `None` in either list is not
    /// an effect to choose.
    #[test]
    fn the_effect_is_negotiated_as_documented() {
        use DropEffect::{Copy, Link, Move, None};
        let no = DragKeys::default();
        let ctrl = DragKeys { ctrl: true, ..no };
        let shift = DragKeys { shift: true, ..no };
        let alt = DragKeys { alt: true, ..no };
        assert_eq!(negotiate(&ALL, &[Move, Copy], no), Move);
        assert_eq!(negotiate(&ALL, &[Link, Move], no), Link);
        assert_eq!(negotiate(&[Copy], &[Move, Copy], no), Copy);
        assert_eq!(negotiate(&[Copy], &[Move], no), None);
        assert_eq!(negotiate(&[None, Copy], &[None, Copy], no), Copy);
        assert_eq!(negotiate(&[None], &[None], no), None);
        assert_eq!(negotiate(&ALL, &[], no), None);
        assert_eq!(negotiate(&ALL, &[Move, Copy], ctrl), Copy);
        assert_eq!(negotiate(&ALL, &[Copy, Move], shift), Move);
        assert_eq!(negotiate(&ALL, &[Copy, Link], alt), Link);
        assert_eq!(negotiate(&[Move], &[Move, Copy], ctrl), None);
        assert_eq!(negotiate(&ALL, &[Move], ctrl), None);

        // The keys' names, every combination.
        for (ctrl, shift, alt, asked) in [
            (false, false, false, Option::None),
            (true, false, false, Some(Copy)),
            (false, true, false, Some(Move)),
            (true, true, false, Some(Link)),
            (false, false, true, Some(Link)),
            (true, false, true, Some(Link)),
            (false, true, true, Some(Link)),
            (true, true, true, Some(Link)),
        ] {
            assert_eq!(
                DragKeys { ctrl, shift, alt }.asked(),
                asked,
                "{ctrl} {shift} {alt}"
            );
        }
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
