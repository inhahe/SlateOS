//! DRM connector — a display output (HDMI, DP, VGA, virtual).
//!
//! A connector represents a physical or virtual display output.
//! Each connector reports its connection status, supported modes,
//! and which encoders can drive it.

extern crate alloc;
use alloc::vec::Vec;

use super::DrmObjectId;
use super::mode::DrmMode;

// ---------------------------------------------------------------------------
// Connector
// ---------------------------------------------------------------------------

/// Type of display connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorType {
    /// Virtual display (virtio-gpu, Limine framebuffer, headless).
    Virtual,
    /// HDMI output.
    Hdmi,
    /// DisplayPort output.
    DisplayPort,
    /// VGA (D-Sub 15-pin).
    Vga,
    /// LVDS (laptop panel).
    Lvds,
    /// DVI output.
    Dvi,
    /// Embedded DisplayPort (laptop panel).
    Edp,
}

impl ConnectorType {
    /// Human-readable name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Virtual => "Virtual",
            Self::Hdmi => "HDMI",
            Self::DisplayPort => "DP",
            Self::Vga => "VGA",
            Self::Lvds => "LVDS",
            Self::Dvi => "DVI",
            Self::Edp => "eDP",
        }
    }
}

/// Connection status of a connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorStatus {
    /// A display is plugged in and detected.
    Connected,
    /// No display detected on this output.
    Disconnected,
    /// Status cannot be determined (no hotplug detect).
    Unknown,
}

/// A DRM connector — one display output.
pub struct DrmConnector {
    /// Unique object ID.
    pub id: DrmObjectId,
    /// Physical connector type.
    pub connector_type: ConnectorType,
    /// Current connection status.
    pub status: ConnectorStatus,
    /// Modes supported by the connected display.
    ///
    /// Populated from EDID (real hardware) or from the host
    /// (virtio-gpu GET_DISPLAY_INFO).  For Limine, contains
    /// exactly one mode matching the boot resolution.
    pub modes: Vec<DrmMode>,
    /// Currently active encoder (if any).
    pub current_encoder: Option<DrmObjectId>,
    /// Encoders that can drive this connector.
    pub possible_encoders: Vec<DrmObjectId>,
}

impl DrmConnector {
    /// The preferred mode (first in the list, or first with PREFERRED flag).
    #[must_use]
    pub fn preferred_mode(&self) -> Option<&DrmMode> {
        self.modes.iter()
            .find(|m| m.flags == super::mode::DrmModeFlags::PREFERRED)
            .or_else(|| self.modes.first())
    }
}
