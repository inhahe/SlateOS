//! Multi-window scene protocol — the compositor-level layer of the SlateOS
//! remote-desktop protocol.
//!
//! [`crate::encode_frame`] serialises a single window's draw commands. A real
//! desktop has many windows stacked in z-order, each redrawing independently;
//! this module wraps the per-window command codec into a *scene* frame that
//! carries the whole visible window set plus the metadata a remote viewer needs
//! to composite it: per-window geometry, opacity, stacking order, and the ids
//! of windows that disappeared since the previous frame.
//!
//! The per-window command body is encoded with the very same
//! [`crate::encode_frame`]/[`crate::decode_frame`] used for single-window
//! streaming — there is exactly one draw-command wire codec in this crate.
//!
//! ## Wire format
//!
//! ```text
//! magic    : [u8;4] = b"SCEN"
//! version  : u8     = SCENE_VERSION
//! flags    : u8     = 0 (reserved)
//! sequence : u64                       monotonically increasing frame number
//! disp_w   : u32                       viewer surface width
//! disp_h   : u32                       viewer surface height
//! n_remove : u32                       removed-window count
//!   [u64 ; n_remove]                   ids gone since the previous frame
//! n_win    : u32                       window count, bottom→top z-order
//!   per window:
//!     id      : u64
//!     x, y    : i32, i32               top-left (incl. decorations), LE
//!     w, h    : u32, u32               client size
//!     opacity : f32 (bits)             0.0..=1.0
//!     present : u8                     1 = a command frame follows, 0 = delta
//!     if present: <one inline ORDR frame — see [`crate::encode_frame`]>
//! ```
//!
//! ## Delta suppression
//!
//! A [`SceneSession`] fingerprints each window's encoded command frame. When a
//! window's commands are byte-identical to what the viewer already holds, the
//! window is emitted as `present = 0` (geometry only). [`apply_scene_frame`] on
//! the viewer side carries the prior commands forward for such windows, so a
//! static desktop streams as little more than its window rectangles.

use std::collections::BTreeMap;

use guitk::render::RenderTree;

use crate::{DecodeError, Reader};

/// Scene-frame magic: `b"SCEN"`.
pub const SCENE_MAGIC: [u8; 4] = *b"SCEN";

/// Scene protocol version. Bump on any incompatible layout change.
pub const SCENE_VERSION: u8 = 1;

/// Upper bound on the window count and removed-id count in a single scene
/// frame, to reject corrupt/hostile input before allocating.
pub const MAX_WINDOWS_PER_FRAME: u32 = 1 << 16;

/// Scene-frame header: magic + version + flags + sequence + dims + n_remove.
const SCENE_HEADER_LEN: usize = 4 + 1 + 1 + 8 + 4 + 4 + 4;

/// One window's contribution to a scene frame.
#[derive(Clone, Debug)]
pub struct SceneWindow {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    /// `Some` when the window's commands changed (or it is new to the session)
    /// and are forwarded in full; `None` is a delta meaning "reuse the commands
    /// you already have for this window".
    pub commands: Option<RenderTree>,
}

/// A full streamed frame: the visible window set in bottom→top z-order plus the
/// ids that disappeared since the previous frame.
#[derive(Clone, Debug)]
pub struct SceneFrame {
    pub sequence: u64,
    pub display_width: u32,
    pub display_height: u32,
    /// Bottom-to-top z-order (last entry is topmost).
    pub windows: Vec<SceneWindow>,
    /// Window ids present last frame but gone now — the viewer drops them.
    pub removed: Vec<u64>,
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encode a scene frame to its wire representation.
#[must_use]
pub fn encode_scene_frame(frame: &SceneFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(SCENE_HEADER_LEN + frame.windows.len() * 48);
    out.extend_from_slice(&SCENE_MAGIC);
    out.push(SCENE_VERSION);
    out.push(0); // flags
    crate::write_u64(&mut out, frame.sequence);
    crate::write_u32(&mut out, frame.display_width);
    crate::write_u32(&mut out, frame.display_height);

    // Counts saturate to u32::MAX on overflow rather than silently truncating;
    // the decode side rejects any count above MAX_WINDOWS_PER_FRAME anyway.
    crate::write_u32(&mut out, u32::try_from(frame.removed.len()).unwrap_or(u32::MAX));
    for &id in &frame.removed {
        crate::write_u64(&mut out, id);
    }

    crate::write_u32(&mut out, u32::try_from(frame.windows.len()).unwrap_or(u32::MAX));
    for win in &frame.windows {
        crate::write_u64(&mut out, win.id);
        // Window coordinates are signed; encode the raw two's-complement bits.
        crate::write_u32(&mut out, win.x.cast_unsigned());
        crate::write_u32(&mut out, win.y.cast_unsigned());
        crate::write_u32(&mut out, win.width);
        crate::write_u32(&mut out, win.height);
        crate::write_f32(&mut out, win.opacity);
        match &win.commands {
            Some(tree) => {
                out.push(1);
                // Reuse the single-window command codec verbatim.
                crate::encode_frame(tree, &mut out);
            }
            None => out.push(0),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decode a scene frame from its wire representation.
pub fn decode_scene_frame(input: &[u8]) -> Result<SceneFrame, DecodeError> {
    let mut r = Reader::new(input);
    r.need(SCENE_HEADER_LEN)?;
    let magic = [r.buf[0], r.buf[1], r.buf[2], r.buf[3]];
    if magic != SCENE_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    r.pos = 4;
    let ver = r.read_u8()?;
    if ver != SCENE_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let sequence = r.read_u64()?;
    let display_width = r.read_u32()?;
    let display_height = r.read_u32()?;

    let n_remove = r.read_u32()?;
    if n_remove > MAX_WINDOWS_PER_FRAME {
        return Err(DecodeError::TooManyWindows(n_remove));
    }
    let mut removed = Vec::with_capacity(n_remove as usize);
    for _ in 0..n_remove {
        removed.push(r.read_u64()?);
    }

    let n_win = r.read_u32()?;
    if n_win > MAX_WINDOWS_PER_FRAME {
        return Err(DecodeError::TooManyWindows(n_win));
    }
    let mut windows = Vec::with_capacity(n_win as usize);
    for _ in 0..n_win {
        let id = r.read_u64()?;
        let x = r.read_u32()?.cast_signed();
        let y = r.read_u32()?.cast_signed();
        let width = r.read_u32()?;
        let height = r.read_u32()?;
        let opacity = r.read_f32()?;
        let present = r.read_u8()?;
        let commands = match present {
            0 => None,
            1 => {
                // Decode one inline ORDR frame from the remaining bytes and
                // advance our cursor by however many it consumed.
                let rest = r.buf.get(r.pos..).ok_or(DecodeError::UnexpectedEof)?;
                let (tree, consumed) = crate::decode_frame(rest)?;
                r.pos += consumed;
                Some(tree)
            }
            other => return Err(DecodeError::BadTag(other)),
        };
        windows.push(SceneWindow {
            id,
            x,
            y,
            width,
            height,
            opacity,
            commands,
        });
    }

    Ok(SceneFrame {
        sequence,
        display_width,
        display_height,
        windows,
        removed,
    })
}

// ---------------------------------------------------------------------------
// Session — stateful delta tracking
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit hash. Cheap, allocation-free fingerprint for change detection
/// only (not cryptographic): a collision would merely suppress one window's
/// redraw for one frame, which the next genuine content change corrects.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// A window's current state, supplied to [`SceneSession::build_frame`] in
/// bottom-to-top z-order. Borrows the live command list to avoid a copy when
/// the window is unchanged (the common case).
pub struct WindowSnapshot<'a> {
    pub id: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: f32,
    pub commands: &'a RenderTree,
}

/// Tracks what one remote viewer already holds, so successive frames forward a
/// window's commands only when its fingerprint changes (geometry-only deltas
/// otherwise).
#[derive(Clone, Debug, Default)]
pub struct SceneSession {
    next_sequence: u64,
    /// window id → fingerprint of the last forwarded command frame.
    sent: BTreeMap<u64, u64>,
}

impl SceneSession {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The sequence number the next [`build_frame`](Self::build_frame) stamps.
    #[must_use]
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Build the next [`SceneFrame`] from the current visible window set
    /// (bottom-to-top z-order). A window's commands are included only when its
    /// fingerprint differs from what this session last forwarded; otherwise it
    /// is a geometry-only delta. Windows present in a previous frame but absent
    /// now are reported in [`SceneFrame::removed`].
    pub fn build_frame(
        &mut self,
        display_width: u32,
        display_height: u32,
        windows: &[WindowSnapshot<'_>],
    ) -> SceneFrame {
        let mut out_windows = Vec::with_capacity(windows.len());
        let mut still_present: BTreeMap<u64, u64> = BTreeMap::new();

        for snap in windows {
            let blob = crate::encode_frame_to_vec(snap.commands);
            let fp = fnv1a_64(&blob);
            let changed = self.sent.get(&snap.id) != Some(&fp);
            let commands = if changed {
                Some(snap.commands.clone())
            } else {
                None
            };
            still_present.insert(snap.id, fp);
            out_windows.push(SceneWindow {
                id: snap.id,
                x: snap.x,
                y: snap.y,
                width: snap.width,
                height: snap.height,
                opacity: snap.opacity,
                commands,
            });
        }

        let removed: Vec<u64> = self
            .sent
            .keys()
            .filter(|id| !still_present.contains_key(id))
            .copied()
            .collect();

        self.sent = still_present;
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        SceneFrame {
            sequence,
            display_width,
            display_height,
            windows: out_windows,
            removed,
        }
    }

    /// Forget all tracked state. The next frame re-sends every window's commands
    /// in full — use when a viewer (re)connects.
    pub fn reset(&mut self) {
        self.sent.clear();
    }
}

/// Reconstruct the full per-window command set from a decoded frame, carrying
/// the previous frame's commands forward for delta (`None`) windows. Returned
/// keyed by window id; `removed` ids simply never appear in the result.
#[must_use]
pub fn apply_scene_frame(
    prev: &BTreeMap<u64, RenderTree>,
    frame: &SceneFrame,
) -> BTreeMap<u64, RenderTree> {
    let mut next: BTreeMap<u64, RenderTree> = BTreeMap::new();
    for win in &frame.windows {
        let tree = match &win.commands {
            Some(t) => t.clone(),
            None => prev.get(&win.id).cloned().unwrap_or_default(),
        };
        next.insert(win.id, tree);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use guitk::color::Color;
    use guitk::render::{FontWeightHint, RenderCommand};
    use guitk::style::CornerRadii;

    fn sample_tree() -> RenderTree {
        RenderTree {
            commands: vec![
                RenderCommand::FillRect {
                    x: 1.5,
                    y: 2.5,
                    width: 100.0,
                    height: 50.0,
                    color: Color::rgba(10, 20, 30, 255),
                    corner_radii: CornerRadii::all(4.0),
                },
                RenderCommand::Text {
                    x: 5.0,
                    y: 6.0,
                    text: "héllo 🦀".to_string(),
                    color: Color::rgba(255, 255, 255, 255),
                    font_size: 14.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(120.0),
                },
            ],
        }
    }

    fn assert_tree_eq(a: &RenderTree, b: &RenderTree) {
        assert_eq!(a.commands.len(), b.commands.len());
        for (ca, cb) in a.commands.iter().zip(b.commands.iter()) {
            assert_eq!(format!("{ca:?}"), format!("{cb:?}"));
        }
    }

    #[test]
    fn scene_frame_round_trips() {
        let frame = SceneFrame {
            sequence: 42,
            display_width: 1920,
            display_height: 1080,
            windows: vec![
                SceneWindow {
                    id: 1,
                    x: -10,
                    y: 20,
                    width: 640,
                    height: 480,
                    opacity: 0.75,
                    commands: Some(sample_tree()),
                },
                SceneWindow {
                    id: 2,
                    x: 100,
                    y: 200,
                    width: 300,
                    height: 150,
                    opacity: 1.0,
                    commands: None,
                },
            ],
            removed: vec![7, 9],
        };
        let bytes = encode_scene_frame(&frame);
        let back = decode_scene_frame(&bytes).expect("decode");
        assert_eq!(back.sequence, 42);
        assert_eq!(back.display_width, 1920);
        assert_eq!(back.display_height, 1080);
        assert_eq!(back.removed, vec![7, 9]);
        assert_eq!(back.windows.len(), 2);
        assert_eq!(back.windows[0].id, 1);
        assert_eq!(back.windows[0].x, -10);
        assert_eq!(back.windows[0].y, 20);
        assert!((back.windows[0].opacity - 0.75).abs() < f32::EPSILON);
        assert_tree_eq(
            back.windows[0].commands.as_ref().unwrap(),
            &sample_tree(),
        );
        assert!(back.windows[1].commands.is_none());
    }

    #[test]
    fn bad_magic_rejected() {
        let mut bytes = encode_scene_frame(&SceneFrame {
            sequence: 0,
            display_width: 1,
            display_height: 1,
            windows: vec![],
            removed: vec![],
        });
        bytes[0] ^= 0xFF;
        assert!(matches!(
            decode_scene_frame(&bytes),
            Err(DecodeError::BadMagic)
        ));
    }

    #[test]
    fn truncated_rejected() {
        let bytes = encode_scene_frame(&SceneFrame {
            sequence: 1,
            display_width: 1,
            display_height: 1,
            windows: vec![SceneWindow {
                id: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: Some(sample_tree()),
            }],
            removed: vec![],
        });
        assert!(decode_scene_frame(&bytes[..bytes.len() - 3]).is_err());
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut bytes = encode_scene_frame(&SceneFrame {
            sequence: 0,
            display_width: 1,
            display_height: 1,
            windows: vec![],
            removed: vec![],
        });
        bytes[4] = 0xFE; // version byte after the 4-byte magic
        assert!(matches!(
            decode_scene_frame(&bytes),
            Err(DecodeError::UnsupportedVersion(0xFE))
        ));
    }

    #[test]
    fn session_suppresses_unchanged_then_resends_on_change() {
        let mut session = SceneSession::new();
        let tree_a = sample_tree();
        let snaps_a = vec![WindowSnapshot {
            id: 5,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            opacity: 1.0,
            commands: &tree_a,
        }];

        let f0 = session.build_frame(800, 600, &snaps_a);
        assert_eq!(f0.sequence, 0);
        assert!(f0.windows[0].commands.is_some());

        let f1 = session.build_frame(800, 600, &snaps_a);
        assert_eq!(f1.sequence, 1);
        assert!(f1.windows[0].commands.is_none());

        // Change the content → resend.
        let tree_b = RenderTree {
            commands: vec![RenderCommand::FillRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                color: Color::rgba(9, 9, 9, 255),
                corner_radii: CornerRadii::ZERO,
            }],
        };
        let snaps_b = vec![WindowSnapshot {
            id: 5,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            opacity: 1.0,
            commands: &tree_b,
        }];
        let f2 = session.build_frame(800, 600, &snaps_b);
        assert!(f2.windows[0].commands.is_some());
    }

    #[test]
    fn session_reports_removed_windows() {
        let mut session = SceneSession::new();
        let tree = sample_tree();
        let two = vec![
            WindowSnapshot {
                id: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: &tree,
            },
            WindowSnapshot {
                id: 2,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                opacity: 1.0,
                commands: &tree,
            },
        ];
        session.build_frame(10, 10, &two);

        let one = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &tree,
        }];
        let f = session.build_frame(10, 10, &one);
        assert_eq!(f.removed, vec![2]);
    }

    #[test]
    fn apply_scene_frame_carries_forward_deltas() {
        let mut session = SceneSession::new();
        let tree = sample_tree();
        let snaps = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &tree,
        }];

        let f0 = session.build_frame(10, 10, &snaps);
        let v0 = apply_scene_frame(&BTreeMap::new(), &f0);
        assert_eq!(v0.get(&1).map(|t| t.commands.len()), Some(tree.commands.len()));

        let f1 = session.build_frame(10, 10, &snaps);
        assert!(f1.windows[0].commands.is_none());
        let v1 = apply_scene_frame(&v0, &f1);
        assert_tree_eq(v1.get(&1).unwrap(), &tree);
    }

    #[test]
    fn reset_forces_full_resend() {
        let mut session = SceneSession::new();
        let tree = sample_tree();
        let snaps = vec![WindowSnapshot {
            id: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            opacity: 1.0,
            commands: &tree,
        }];
        session.build_frame(10, 10, &snaps);
        session.reset();
        let f = session.build_frame(10, 10, &snaps);
        assert!(f.windows[0].commands.is_some());
    }
}
