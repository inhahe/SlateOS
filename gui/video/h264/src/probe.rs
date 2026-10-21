//! The debugging probes `tools/h264cmp.py` switches on, each by an
//! environment variable, read once per process: they are tested on every
//! macroblock and every edge, and on Windows each lookup scans the whole
//! environment block and allocates.

use std::sync::LazyLock;

/// Which probes are on. Each prints the lines its namesake in
/// `tools/ffdbg/ffdbg.patch` prints in FFmpeg, for `tools/h264cmp.py`.
pub(crate) struct Probes {
    /// `H264_MB`: each macroblock's samples summed once reconstructed.
    pub(crate) mb: bool,
    /// `H264_LF`: each macroblock's samples summed once its edges are filtered.
    pub(crate) lf: bool,
    /// `H264_MV`: each inter macroblock's references and vectors.
    pub(crate) mv: bool,
    /// `H264_REF`: each slice's reference lists.
    pub(crate) refs: bool,
    /// `H264_PIC`: the picture once a packet's slices are decoded and concealed.
    pub(crate) pic: bool,
    /// `H264_OUT`: each picture handed out.
    pub(crate) out: bool,
    /// `H264_ER`: when error concealment conceals anything.
    pub(crate) er: bool,
}

impl Probes {
    /// Whether any probe is on: its lines are worth reading only from one
    /// stream decoded at a time.
    pub(crate) fn any(&self) -> bool {
        self.mb || self.lf || self.mv || self.refs || self.pic || self.out || self.er
    }
}

/// The probes, as the environment had them when first asked.
pub(crate) static PROBES: LazyLock<Probes> = LazyLock::new(|| {
    let on = |name: &str| std::env::var_os(name).is_some();
    Probes {
        mb: on("H264_MB"),
        lf: on("H264_LF"),
        mv: on("H264_MV"),
        refs: on("H264_REF"),
        pic: on("H264_PIC"),
        out: on("H264_OUT"),
        er: on("H264_ER"),
    }
});
