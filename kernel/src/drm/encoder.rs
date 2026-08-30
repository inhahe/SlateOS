//! DRM encoder — signal conversion between CRTC and connector.
//!
//! An encoder converts the CRTC's digital pixel stream into the
//! electrical signaling required by the connector (TMDS for HDMI,
//! LVDS for laptop panels, etc.).
//!
//! For virtual displays (Limine, virtio-gpu) the encoder is a
//! passthrough — it exists only to complete the DRM object graph.

use super::DrmObjectId;

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Encoder type (signal encoding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderType {
    /// No encoding — virtual display.
    Virtual,
    /// TMDS (DVI, HDMI).
    Tmds,
    /// LVDS (laptop panels).
    Lvds,
    /// DisplayPort main link.
    DpMst,
    /// DAC (VGA analog).
    Dac,
}

/// A DRM encoder.
pub struct DrmEncoder {
    /// Unique object ID.
    pub id: DrmObjectId,
    /// Signal encoding type.
    pub encoder_type: EncoderType,
    /// Currently bound CRTC (if any).
    pub crtc: Option<DrmObjectId>,
    /// Bitmask of CRTCs this encoder can connect to.
    pub possible_crtcs: u32,
}
