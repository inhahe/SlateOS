//! What a picture carries besides its samples: FFmpeg's frame side data
//! (`AVFrameSideData`), as its H.264 decoder attaches it from the SEI
//! messages that came with the picture (`ff_h2645_sei_to_frame`,
//! `h264_export_frame_props`).

/// A fraction, `num / den` (`AVRational`).
pub type Rational = (i32, i32);

/// One piece of side data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideData {
    /// How the picture packs two views of a stereo pair
    /// (`AV_FRAME_DATA_STEREO3D`), from a frame packing arrangement.
    Stereo3d(Stereo3d),
    /// How to rotate and flip the picture to show it, as FFmpeg's 3x3
    /// display matrix: 16.16 fixed point, but the last column 2.30
    /// (`AV_FRAME_DATA_DISPLAYMATRIX`).
    DisplayMatrix([i32; 9]),
    /// ATSC A/53 closed captions: `cc_data` triplets
    /// (`AV_FRAME_DATA_A53_CC`).
    A53Captions(Vec<u8>),
    /// A message of unregistered user data: its UUID, then its payload
    /// (`AV_FRAME_DATA_SEI_UNREGISTERED`).
    Unregistered(Vec<u8>),
    /// The ambient viewing environment the content is for
    /// (`AV_FRAME_DATA_AMBIENT_VIEWING_ENVIRONMENT`).
    AmbientViewing {
        /// In cd/m2.
        illuminance: Rational,
        /// CIE 1931 x and y.
        light_x: Rational,
        light_y: Rational,
    },
    /// The display the content was mastered on
    /// (`AV_FRAME_DATA_MASTERING_DISPLAY_METADATA`).
    MasteringDisplay(MasteringDisplay),
    /// How bright the content gets, in cd/m2
    /// (`AV_FRAME_DATA_CONTENT_LIGHT_LEVEL`).
    ContentLight { max_cll: u32, max_fall: u32 },
    /// An active format description (`AV_FRAME_DATA_AFD`).
    Afd(u8),
    /// The data of an LCEVC enhancement layer (`AV_FRAME_DATA_LCEVC`).
    Lcevc(Vec<u8>),
    /// SMPTE 12-M time codes: the count, then each in SMPTE's binary form
    /// (`AV_FRAME_DATA_S12M_TIMECODE`).
    S12mTimecode(Vec<u32>),
}

/// A stereo pair's packing (`AVStereo3D`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stereo3d {
    pub kind: Stereo3dType,
    /// The views are the other way round (`AV_STEREO3D_FLAG_INVERT`).
    pub inverted: bool,
    /// Which view a picture of a frame-sequential pair is.
    pub view: Stereo3dView,
}

/// How the two views are packed (`AVStereo3DType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stereo3dType {
    /// One view only (`AV_STEREO3D_2D`).
    TwoD,
    SideBySide,
    TopBottom,
    /// One picture each, alternately (`AV_STEREO3D_FRAMESEQUENCE`).
    FrameSequence,
    Checkerboard,
    SideBySideQuincunx,
    /// Alternate rows (`AV_STEREO3D_LINES`).
    Lines,
    /// Alternate columns (`AV_STEREO3D_COLUMNS`).
    Columns,
}

/// Which view (`AVStereo3DView`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stereo3dView {
    /// Both, packed (`AV_STEREO3D_VIEW_PACKED`).
    #[default]
    Packed,
    Left,
    Right,
}

/// A mastering display (`AVMasteringDisplayMetadata`): its primaries and
/// white point as CIE 1931 x and y, each over 50000, and its luminance in
/// cd/m2 over 10000. Each half is there only where FFmpeg finds it in
/// range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasteringDisplay {
    /// Red, green, blue: `[x, y]` each.
    pub display_primaries: [[Rational; 2]; 3],
    pub white_point: [Rational; 2],
    pub min_luminance: Rational,
    pub max_luminance: Rational,
    pub has_primaries: bool,
    pub has_luminance: bool,
}
