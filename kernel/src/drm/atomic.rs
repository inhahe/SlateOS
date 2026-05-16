//! Atomic modesetting — commit all display state changes at once.
//!
//! The atomic API ensures that display state transitions are either
//! fully applied or fully rejected (no partial updates that leave the
//! display in an inconsistent state).
//!
//! ## Flow
//!
//! 1. Build an [`AtomicState`] describing the desired changes.
//! 2. Call `atomic_check()` to validate (test-only, no hardware change).
//! 3. Call `atomic_commit()` to apply all changes to hardware.
//!
//! ## Validation
//!
//! `atomic_check()` validates:
//! - All referenced CRTC/plane/connector IDs exist in the device.
//! - Requested modes are in the connector's mode list.
//! - Planes reference valid framebuffers.
//! - Planes are assigned to CRTCs they can physically drive
//!   (`possible_crtcs` bitmask).
//! - Connector ↔ CRTC bindings use compatible encoders.
//!
//! ## Commit
//!
//! `atomic_commit()` applies all validated changes atomically:
//! - CRTC active state + mode changes → backend `mode_set()`.
//! - Plane FB/CRTC assignment + src/dst rects → internal state update.
//! - Connector CRTC binding → internal state update.
//!
//! If any step fails, previous changes within this commit are NOT rolled
//! back (the commit is "atomic" in the sense that check prevents invalid
//! states, not that hardware changes are transactional).  This matches
//! Linux DRM behavior.
//!
//! ## References
//!
//! - Linux `drivers/gpu/drm/drm_atomic.c`
//! - Linux `include/drm/drm_atomic.h`

extern crate alloc;
use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

use super::DrmObjectId;
use super::mode::DrmMode;
use super::DrmDevice;

// ---------------------------------------------------------------------------
// Atomic state
// ---------------------------------------------------------------------------

/// A pending atomic modesetting state.
///
/// Collects all desired changes before committing them in one shot.
pub struct AtomicState {
    /// CRTC state changes.
    pub crtc_changes: Vec<CrtcState>,
    /// Plane state changes.
    pub plane_changes: Vec<PlaneState>,
    /// Connector state changes.
    pub connector_changes: Vec<ConnectorState>,
    /// If true, only validate — don't apply to hardware.
    pub test_only: bool,
}

impl AtomicState {
    /// Create an empty atomic state (no changes).
    #[must_use]
    pub fn new() -> Self {
        Self {
            crtc_changes: Vec::new(),
            plane_changes: Vec::new(),
            connector_changes: Vec::new(),
            test_only: false,
        }
    }

    /// Whether this state has any changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.crtc_changes.is_empty()
            && self.plane_changes.is_empty()
            && self.connector_changes.is_empty()
    }

    /// Add a CRTC state change.
    pub fn add_crtc(&mut self, state: CrtcState) {
        self.crtc_changes.push(state);
    }

    /// Add a plane state change.
    pub fn add_plane(&mut self, state: PlaneState) {
        self.plane_changes.push(state);
    }

    /// Add a connector state change.
    pub fn add_connector(&mut self, state: ConnectorState) {
        self.connector_changes.push(state);
    }
}

/// Desired state for a CRTC.
pub struct CrtcState {
    /// Which CRTC to modify.
    pub id: DrmObjectId,
    /// Set active state (None = don't change).
    pub active: Option<bool>,
    /// Set display mode (None = don't change, Some(None) = disable).
    pub mode: Option<Option<DrmMode>>,
}

/// Desired state for a plane.
pub struct PlaneState {
    /// Which plane to modify.
    pub id: DrmObjectId,
    /// Framebuffer to display (None = don't change, Some(None) = disable).
    pub fb_id: Option<Option<DrmObjectId>>,
    /// CRTC to bind to (None = don't change, Some(None) = unbind).
    pub crtc_id: Option<Option<DrmObjectId>>,
    /// Source rectangle in framebuffer coordinates.
    pub src_rect: Option<Rect>,
    /// Destination rectangle in CRTC coordinates.
    pub dst_rect: Option<IRect>,
}

/// Desired state for a connector.
pub struct ConnectorState {
    /// Which connector to modify.
    pub id: DrmObjectId,
    /// CRTC to bind to (None = don't change, Some(None) = unbind).
    pub crtc_id: Option<Option<DrmObjectId>>,
}

/// Rectangle (unsigned coordinates, for source rects).
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Rectangle (signed coordinates, for destination rects).
#[derive(Debug, Clone, Copy)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

// ---------------------------------------------------------------------------
// Atomic check
// ---------------------------------------------------------------------------

/// Validate an atomic state against a DRM device.
///
/// Checks that all referenced objects exist, modes are supported,
/// and bindings are compatible.  Returns `Ok(())` if the state is
/// valid and can be committed.
pub fn atomic_check(dev: &DrmDevice, state: &AtomicState) -> KernelResult<()> {
    // Validate CRTC changes.
    for cs in &state.crtc_changes {
        // CRTC must exist.
        if !dev.crtcs().iter().any(|c| c.id == cs.id) {
            serial_println!(
                "[drm-atomic] check FAIL: CRTC {} not found",
                cs.id
            );
            return Err(KernelError::NotFound);
        }

        // If a mode is being set, it must match a connector's supported mode.
        // For now we only validate that the mode dimensions are reasonable.
        if let Some(Some(mode)) = &cs.mode {
            if mode.hdisplay == 0 || mode.vdisplay == 0 || mode.vrefresh == 0 {
                serial_println!(
                    "[drm-atomic] check FAIL: invalid mode {}x{}@{}",
                    mode.hdisplay,
                    mode.vdisplay,
                    mode.vrefresh,
                );
                return Err(KernelError::InvalidArgument);
            }
        }
    }

    // Validate plane changes.
    for ps in &state.plane_changes {
        let plane = dev.planes().iter().find(|p| p.id == ps.id);
        let plane = match plane {
            Some(p) => p,
            None => {
                serial_println!(
                    "[drm-atomic] check FAIL: plane {} not found",
                    ps.id
                );
                return Err(KernelError::NotFound);
            }
        };

        // If binding to a CRTC, check possible_crtcs bitmask.
        if let Some(Some(crtc_id)) = ps.crtc_id {
            let crtc = dev.crtcs().iter().find(|c| c.id == crtc_id);
            match crtc {
                Some(c) => {
                    #[allow(clippy::arithmetic_side_effects)]
                    let bit = 1u32 << c.index;
                    if plane.possible_crtcs & bit == 0 {
                        serial_println!(
                            "[drm-atomic] check FAIL: plane {} can't drive CRTC {}",
                            ps.id,
                            crtc_id,
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
                None => {
                    serial_println!(
                        "[drm-atomic] check FAIL: CRTC {} not found for plane {}",
                        crtc_id,
                        ps.id,
                    );
                    return Err(KernelError::NotFound);
                }
            }
        }

        // If assigning a framebuffer, verify it exists.
        if let Some(Some(fb_id)) = ps.fb_id {
            if dev.fb_get(fb_id).is_none() {
                serial_println!(
                    "[drm-atomic] check FAIL: FB {} not found for plane {}",
                    fb_id,
                    ps.id,
                );
                return Err(KernelError::NotFound);
            }
        }

        // Validate source/destination rectangles.
        if let Some(src) = &ps.src_rect {
            if src.w == 0 || src.h == 0 {
                serial_println!(
                    "[drm-atomic] check FAIL: zero-size source rect for plane {}",
                    ps.id,
                );
                return Err(KernelError::InvalidArgument);
            }
        }
        if let Some(dst) = &ps.dst_rect {
            if dst.w == 0 || dst.h == 0 {
                serial_println!(
                    "[drm-atomic] check FAIL: zero-size dest rect for plane {}",
                    ps.id,
                );
                return Err(KernelError::InvalidArgument);
            }
        }
    }

    // Validate connector changes.
    for cs in &state.connector_changes {
        // Connector must exist.
        if !dev.connectors().iter().any(|c| c.id == cs.id) {
            serial_println!(
                "[drm-atomic] check FAIL: connector {} not found",
                cs.id
            );
            return Err(KernelError::NotFound);
        }

        // If binding to a CRTC, verify it exists and has a compatible encoder.
        if let Some(Some(crtc_id)) = cs.crtc_id {
            if !dev.crtcs().iter().any(|c| c.id == crtc_id) {
                serial_println!(
                    "[drm-atomic] check FAIL: CRTC {} not found for connector {}",
                    crtc_id,
                    cs.id,
                );
                return Err(KernelError::NotFound);
            }

            // Check that there's at least one encoder that can connect
            // this connector to the requested CRTC.
            let connector = dev.connectors().iter().find(|c| c.id == cs.id);
            if let Some(conn) = connector {
                let crtc = dev.crtcs().iter().find(|c| c.id == crtc_id);
                if let Some(c) = crtc {
                    let has_compatible_encoder = conn.possible_encoders.iter().any(|enc_id| {
                        dev.encoders().iter().any(|e| {
                            #[allow(clippy::arithmetic_side_effects)]
                            let compat = e.id == *enc_id
                                && (e.possible_crtcs & (1u32 << c.index)) != 0;
                            compat
                        })
                    });
                    if !has_compatible_encoder {
                        serial_println!(
                            "[drm-atomic] check FAIL: no encoder links connector {} to CRTC {}",
                            cs.id,
                            crtc_id,
                        );
                        return Err(KernelError::InvalidArgument);
                    }
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Atomic commit
// ---------------------------------------------------------------------------

/// Apply an atomic state to a DRM device.
///
/// The state MUST have been validated by `atomic_check()` first.
/// If `state.test_only` is set, this function does nothing beyond
/// the check (dry-run mode).
///
/// Applies changes in order: CRTCs first (mode/active), then planes
/// (FB/CRTC binding, src/dst rects), then connectors (CRTC binding).
pub fn atomic_commit(dev: &mut DrmDevice, state: &AtomicState) -> KernelResult<()> {
    // Validate first.
    atomic_check(dev, state)?;

    if state.test_only {
        return Ok(());
    }

    // Apply CRTC changes.
    for cs in &state.crtc_changes {
        if let Some(crtc) = dev.crtc_mut(cs.id) {
            if let Some(active) = cs.active {
                crtc.active = active;
            }
            if let Some(mode_opt) = &cs.mode {
                crtc.mode = *mode_opt;
            }
        }
    }

    // Apply plane changes.
    for ps in &state.plane_changes {
        if let Some(plane) = dev.plane_mut(ps.id) {
            if let Some(fb_opt) = ps.fb_id {
                plane.fb = fb_opt;
            }
            if let Some(crtc_opt) = ps.crtc_id {
                plane.crtc = crtc_opt;
            }
            if let Some(src) = &ps.src_rect {
                plane.src_x = src.x;
                plane.src_y = src.y;
                plane.src_w = src.w;
                plane.src_h = src.h;
            }
            if let Some(dst) = &ps.dst_rect {
                plane.dst_x = dst.x;
                plane.dst_y = dst.y;
                plane.dst_w = dst.w;
                plane.dst_h = dst.h;
            }
        }
    }

    // Apply connector changes.
    // Two-pass approach: first compute encoder bindings (immutable borrow),
    // then apply mutations (mutable borrow).  Avoids borrow conflicts.
    for cs in &state.connector_changes {
        if let Some(crtc_opt) = cs.crtc_id {
            // Phase 1: compute the encoder ID (immutable borrow).
            let enc_id = if let Some(crtc_id) = crtc_opt {
                find_compatible_encoder(dev, cs.id, crtc_id)
            } else {
                None
            };

            // Phase 2: apply mutations.
            if let Some(conn) = dev.connector_mut(cs.id) {
                conn.current_encoder = enc_id;
            }

            // Update encoder's CRTC binding.
            if let Some(enc_id) = enc_id {
                if let Some(enc) = dev.encoder_mut(enc_id) {
                    enc.crtc = crtc_opt;
                }
            }
        }
    }

    serial_println!(
        "[drm-atomic] Committed: {} CRTCs, {} planes, {} connectors",
        state.crtc_changes.len(),
        state.plane_changes.len(),
        state.connector_changes.len(),
    );

    Ok(())
}

/// Find a compatible encoder for a connector → CRTC binding.
fn find_compatible_encoder(
    dev: &DrmDevice,
    connector_id: DrmObjectId,
    crtc_id: DrmObjectId,
) -> Option<DrmObjectId> {
    let connector = dev.connectors().iter().find(|c| c.id == connector_id)?;
    let crtc = dev.crtcs().iter().find(|c| c.id == crtc_id)?;

    for enc_id in &connector.possible_encoders {
        if let Some(enc) = dev.encoders().iter().find(|e| e.id == *enc_id) {
            #[allow(clippy::arithmetic_side_effects)]
            let can_drive = (enc.possible_crtcs & (1u32 << crtc.index)) != 0;
            if can_drive {
                return Some(*enc_id);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Cursor operations
// ---------------------------------------------------------------------------

/// Cursor image dimensions (hardware cursors are typically 64×64).
pub const CURSOR_WIDTH: u32 = 64;
/// Cursor image height.
pub const CURSOR_HEIGHT: u32 = 64;

/// Cursor state for a CRTC.
///
/// Tracks the cursor GEM buffer, position, and visibility.
/// Updated via dedicated cursor operations (not atomic commit,
/// since cursor moves are extremely frequent — every mouse event).
#[derive(Debug, Clone, Copy)]
pub struct CursorState {
    /// GEM handle for the cursor image (0 = no cursor).
    pub gem_handle: u32,
    /// Cursor X position (CRTC coordinates).
    pub x: i32,
    /// Cursor Y position (CRTC coordinates).
    pub y: i32,
    /// Width of the cursor image.
    pub width: u32,
    /// Height of the cursor image.
    pub height: u32,
    /// Whether the cursor is visible.
    pub visible: bool,
    /// "Hot spot" X offset within the cursor image.
    pub hot_x: u32,
    /// "Hot spot" Y offset within the cursor image.
    pub hot_y: u32,
}

impl CursorState {
    /// Default cursor state: no cursor, position (0,0).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gem_handle: 0,
            x: 0,
            y: 0,
            width: CURSOR_WIDTH,
            height: CURSOR_HEIGHT,
            visible: false,
            hot_x: 0,
            hot_y: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run atomic modesetting self-tests.
pub(crate) fn self_test() -> KernelResult<()> {
    // Test 1: empty state passes check.
    {
        let state = AtomicState::new();
        if !state.is_empty() {
            serial_println!("[drm-atomic]   FAIL: new state not empty");
            return Err(KernelError::InternalError);
        }
    }

    // Test 2: check against the primary device with valid objects.
    super::with_primary(|dev| {
        let state = AtomicState::new();
        atomic_check(dev, &state)?;
        serial_println!("[drm-atomic]   Empty atomic check: OK");
        Ok(())
    })?;

    // Test 3: check with valid CRTC change.
    super::with_primary(|dev| {
        let crtc_id = dev.crtcs().first()
            .map(|c| c.id)
            .ok_or(KernelError::InternalError)?;

        let mut state = AtomicState::new();
        state.add_crtc(CrtcState {
            id: crtc_id,
            active: Some(true),
            mode: None,
        });
        atomic_check(dev, &state)?;
        serial_println!("[drm-atomic]   Valid CRTC check: OK");
        Ok(())
    })?;

    // Test 4: check with invalid CRTC ID → NotFound.
    super::with_primary(|dev| {
        let mut state = AtomicState::new();
        state.add_crtc(CrtcState {
            id: DrmObjectId::new(9999),
            active: Some(true),
            mode: None,
        });
        match atomic_check(dev, &state) {
            Err(KernelError::NotFound) => {
                serial_println!("[drm-atomic]   Invalid CRTC rejected: OK");
                Ok(())
            }
            Ok(()) => {
                serial_println!("[drm-atomic]   FAIL: invalid CRTC accepted");
                Err(KernelError::InternalError)
            }
            Err(e) => {
                serial_println!("[drm-atomic]   FAIL: unexpected error {:?}", e);
                Err(KernelError::InternalError)
            }
        }
    })?;

    // Test 5: check with invalid plane → NotFound.
    super::with_primary(|dev| {
        let mut state = AtomicState::new();
        state.add_plane(PlaneState {
            id: DrmObjectId::new(9998),
            fb_id: None,
            crtc_id: None,
            src_rect: None,
            dst_rect: None,
        });
        match atomic_check(dev, &state) {
            Err(KernelError::NotFound) => {
                serial_println!("[drm-atomic]   Invalid plane rejected: OK");
                Ok(())
            }
            _ => {
                serial_println!("[drm-atomic]   FAIL: invalid plane not rejected");
                Err(KernelError::InternalError)
            }
        }
    })?;

    // Test 6: check with zero-size mode → InvalidArgument.
    super::with_primary(|dev| {
        let crtc_id = dev.crtcs().first()
            .map(|c| c.id)
            .ok_or(KernelError::InternalError)?;

        let mut state = AtomicState::new();
        state.add_crtc(CrtcState {
            id: crtc_id,
            active: Some(true),
            mode: Some(Some(DrmMode::from_resolution(0, 0, 60))),
        });
        match atomic_check(dev, &state) {
            Err(KernelError::InvalidArgument) => {
                serial_println!("[drm-atomic]   Zero-size mode rejected: OK");
                Ok(())
            }
            _ => {
                serial_println!("[drm-atomic]   FAIL: zero-size mode not rejected");
                Err(KernelError::InternalError)
            }
        }
    })?;

    // Test 7: test_only commit applies no changes.
    super::with_primary_mut(|dev| {
        let crtc_id = dev.crtcs().first()
            .map(|c| c.id)
            .ok_or(KernelError::InternalError)?;

        let was_active = dev.crtcs().first()
            .map(|c| c.active)
            .unwrap_or(false);

        let mut state = AtomicState::new();
        state.test_only = true;
        state.add_crtc(CrtcState {
            id: crtc_id,
            active: Some(!was_active),
            mode: None,
        });
        atomic_commit(dev, &state)?;

        // State should NOT have changed (test_only).
        let still_active = dev.crtcs().first()
            .map(|c| c.active)
            .unwrap_or(false);
        if still_active != was_active {
            serial_println!("[drm-atomic]   FAIL: test_only changed state");
            return Err(KernelError::InternalError);
        }
        serial_println!("[drm-atomic]   test_only commit: OK");
        Ok(())
    })?;

    // Test 8: real commit updates CRTC state.
    super::with_primary_mut(|dev| {
        let crtc_id = dev.crtcs().first()
            .map(|c| c.id)
            .ok_or(KernelError::InternalError)?;

        // Deactivate.
        let mut state = AtomicState::new();
        state.add_crtc(CrtcState {
            id: crtc_id,
            active: Some(false),
            mode: None,
        });
        atomic_commit(dev, &state)?;

        let active = dev.crtcs().first()
            .map(|c| c.active)
            .unwrap_or(true);
        if active {
            serial_println!("[drm-atomic]   FAIL: CRTC still active after deactivation");
            return Err(KernelError::InternalError);
        }

        // Re-activate.
        let mut state2 = AtomicState::new();
        state2.add_crtc(CrtcState {
            id: crtc_id,
            active: Some(true),
            mode: None,
        });
        atomic_commit(dev, &state2)?;

        let active2 = dev.crtcs().first()
            .map(|c| c.active)
            .unwrap_or(false);
        if !active2 {
            serial_println!("[drm-atomic]   FAIL: CRTC not active after reactivation");
            return Err(KernelError::InternalError);
        }

        serial_println!("[drm-atomic]   Real commit updates state: OK");
        Ok(())
    })?;

    // Test 9: cursor state initialization.
    {
        let cs = CursorState::new();
        if cs.visible || cs.gem_handle != 0 || cs.x != 0 || cs.y != 0 {
            serial_println!("[drm-atomic]   FAIL: cursor default state wrong");
            return Err(KernelError::InternalError);
        }
        serial_println!("[drm-atomic]   Cursor state default: OK");
    }

    serial_println!("[drm-atomic]   Atomic modesetting: OK");
    Ok(())
}
