//! DRM connector — a display output (HDMI, DP, VGA, virtual).
//!
//! A connector represents a physical or virtual display output.
//! Each connector reports its connection status, supported modes,
//! and which encoders can drive it.

extern crate alloc;
use alloc::vec::Vec;

use super::DrmObjectId;
use super::edid::EdidInfo;
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
    /// Parsed EDID information (if available).
    ///
    /// Present when the display provided valid EDID data via DDC/I2C,
    /// virtio-gpu GET_EDID, or firmware.  `None` for virtual displays
    /// (Limine framebuffer) or when EDID is unavailable.
    pub edid: Option<EdidInfo>,
}

impl DrmConnector {
    /// The preferred mode (first in the list, or first with PREFERRED flag).
    #[must_use]
    pub fn preferred_mode(&self) -> Option<&DrmMode> {
        self.modes.iter()
            .find(|m| m.flags == super::mode::DrmModeFlags::PREFERRED)
            .or_else(|| self.modes.first())
    }

    /// Update the connector's mode list and metadata from raw EDID data.
    ///
    /// Parses the EDID block(s), extracts supported modes, and replaces
    /// the connector's current mode list.  Also stores the parsed EDID
    /// info for manufacturer/monitor name queries.
    ///
    /// Returns the number of modes extracted, or an error if the EDID
    /// data is malformed.
    pub fn update_from_edid(&mut self, edid_data: &[u8]) -> crate::error::KernelResult<usize> {
        let info = super::edid::parse(edid_data)?;
        let mode_count = info.modes.len();
        self.modes = info.modes.clone();
        self.edid = Some(info);
        if !self.modes.is_empty() {
            self.status = ConnectorStatus::Connected;
        }
        Ok(mode_count)
    }

    /// Whether this connector has valid EDID data.
    #[must_use]
    pub fn has_edid(&self) -> bool {
        self.edid.is_some()
    }

    /// The monitor name from EDID, or `None` if unavailable.
    #[must_use]
    pub fn monitor_name(&self) -> Option<&[u8]> {
        self.edid.as_ref().map(|e| e.name_str())
    }
}
