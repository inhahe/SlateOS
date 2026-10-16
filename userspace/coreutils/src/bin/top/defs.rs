//! `top.h`: the sizes, keys, field numbers, window flags and defaults.
//!
//! Built as Ubuntu builds it: `--disable-modern-top`, so `ORIG_TOPDEFS` --
//! the original defaults (a three second delay, the summary's one `%Cpu(s)`
//! line, memory in MiB, the classic columns) -- and none of the other
//! `configure` switches that reach `top` (`BOOST_MEMORY`, `BOOST_PERCNT`,
//! `SIGNALS_LESS`).

use coreutils::procps::pids::Item;

/// `DEF_DELAY`, `ORIG_TOPDEFS`'s.
pub const DEF_DELAY: f32 = 3.0;
/// `MSG_USLEEP`: how long a message stays, in microseconds.
pub const MSG_USLEEP: u64 = 1_250_000;
/// `LIB_USLEEP`: the priming pause before the first frame's second reading.
pub const LIB_USLEEP: u64 = 100_000;
/// `MONPIDMAX`: how many `-p` PIDs.
pub const MONPIDMAX: usize = 20;
/// `W_MIN_COL`, `W_MIN_ROW`.
pub const W_MIN_COL: i32 = 3;
pub const W_MIN_ROW: i32 = 3;

pub const SCREENMAX: usize = 512;
pub const CAPBUFSIZ: usize = 32;
pub const CLRBUFSIZ: usize = 64;
pub const PFLAGSSIZ: usize = 128;
pub const SMLBUFSIZ: usize = 128;
pub const MEDBUFSIZ: usize = 256;
pub const LRGBUFSIZ: usize = 512;
pub const OURPATHSZ: usize = 1024;
pub const BIGBUFSIZ: usize = 2048;
pub const BOTBUFSIZ: usize = 16384;
pub const MAXBUFSIZ: usize = 1024 * 64 * 2;
pub const ROWMINSIZ: usize = SCREENMAX + 8 * (CAPBUFSIZ + CLRBUFSIZ);
pub const ROWMAXSIZ: usize = SCREENMAX + 16 * (CAPBUFSIZ + CLRBUFSIZ);
pub const READMINSZ: usize = 2048;
pub const FNDBUFSIZ: usize = MEDBUFSIZ;

/// `COLPADSTR`, `COLPADSIZ`, `COLPLUSCH`.
pub const COLPADSTR: &[u8] = b" ";
pub const COLPADSIZ: i32 = 1;
pub const COLPLUSCH: u8 = b'+';

// The keys `iokey` returns.
pub const KBD_ESC: i32 = 0x1b;
pub const KBD_SPACE: i32 = b' ' as i32;
pub const KBD_ENTER: i32 = b'\n' as i32;
pub const KBD_UP: i32 = 129;
pub const KBD_DOWN: i32 = 130;
pub const KBD_LEFT: i32 = 131;
pub const KBD_RIGHT: i32 = 132;
pub const KBD_PGUP: i32 = 133;
pub const KBD_PGDN: i32 = 134;
pub const KBD_HOME: i32 = 135;
pub const KBD_END: i32 = 136;
pub const KBD_BKSP: i32 = 137;
pub const KBD_INS: i32 = 138;
pub const KBD_DEL: i32 = 139;
pub const KBD_BTAB: i32 = 140;
pub const KBD_CTRL_E: i32 = 0x05;
pub const KBD_CTRL_G: i32 = 0x07;
pub const KBD_CTRL_I: i32 = 0x09;
pub const KBD_CTRL_K: i32 = 0x0b;
pub const KBD_CTRL_L: i32 = 0x0c;
pub const KBD_CTRL_N: i32 = 0x0e;
pub const KBD_CTRL_O: i32 = 0x0f;
pub const KBD_CTRL_P: i32 = 0x10;
pub const KBD_CTRL_R: i32 = 0x12;
pub const KBD_CTRL_U: i32 = 0x15;

/// `PROC_XTRA`: `Pseudo_row`'s "prime the pump" value.
pub const PROC_XTRA: i32 = -1;

// `enum pflag`.
pub const EU_PID: usize = 0;
pub const EU_PPD: usize = 1;
pub const EU_UED: usize = 2;
pub const EU_UEN: usize = 3;
pub const EU_URD: usize = 4;
pub const EU_URN: usize = 5;
pub const EU_USD: usize = 6;
pub const EU_USN: usize = 7;
pub const EU_GID: usize = 8;
pub const EU_GRP: usize = 9;
pub const EU_PGD: usize = 10;
pub const EU_TTY: usize = 11;
pub const EU_TPG: usize = 12;
pub const EU_SID: usize = 13;
pub const EU_PRI: usize = 14;
pub const EU_NCE: usize = 15;
pub const EU_THD: usize = 16;
pub const EU_CPN: usize = 17;
pub const EU_CPU: usize = 18;
pub const EU_TME: usize = 19;
pub const EU_TM2: usize = 20;
pub const EU_MEM: usize = 21;
pub const EU_VRT: usize = 22;
pub const EU_SWP: usize = 23;
pub const EU_RES: usize = 24;
pub const EU_COD: usize = 25;
pub const EU_DAT: usize = 26;
pub const EU_SHR: usize = 27;
pub const EU_FL1: usize = 28;
pub const EU_FL2: usize = 29;
pub const EU_DRT: usize = 30;
pub const EU_STA: usize = 31;
pub const EU_CMD: usize = 32;
pub const EU_WCH: usize = 33;
pub const EU_FLG: usize = 34;
pub const EU_CGR: usize = 35;
pub const EU_SGD: usize = 36;
pub const EU_SGN: usize = 37;
pub const EU_TGD: usize = 38;
pub const EU_OOA: usize = 39;
pub const EU_OOM: usize = 40;
pub const EU_ENV: usize = 41;
pub const EU_FV1: usize = 42;
pub const EU_FV2: usize = 43;
pub const EU_USE: usize = 44;
pub const EU_NS1: usize = 45;
pub const EU_NS2: usize = 46;
pub const EU_NS3: usize = 47;
pub const EU_NS4: usize = 48;
pub const EU_NS5: usize = 49;
pub const EU_NS6: usize = 50;
pub const EU_LXC: usize = 51;
pub const EU_RZA: usize = 52;
pub const EU_RZF: usize = 53;
pub const EU_RZL: usize = 54;
pub const EU_RZS: usize = 55;
pub const EU_CGN: usize = 56;
pub const EU_NMA: usize = 57;
pub const EU_LID: usize = 58;
pub const EU_EXE: usize = 59;
pub const EU_RSS: usize = 60;
pub const EU_PSS: usize = 61;
pub const EU_PZA: usize = 62;
pub const EU_PZF: usize = 63;
pub const EU_PZS: usize = 64;
pub const EU_USS: usize = 65;
pub const EU_IRB: usize = 66;
pub const EU_IRO: usize = 67;
pub const EU_IWB: usize = 68;
pub const EU_IWO: usize = 69;
pub const EU_AGI: usize = 70;
pub const EU_AGN: usize = 71;
pub const EU_TM3: usize = 72;
pub const EU_TM4: usize = 73;
pub const EU_CUU: usize = 74;
pub const EU_CUC: usize = 75;
pub const EU_NS7: usize = 76;
pub const EU_NS8: usize = 77;
/// `EU_MAXPFLGS`: the number of fields.
pub const EU_MAXPFLGS: usize = 78;
/// `EU_XON` and `EU_XOF`: not fields; the sort column's highlight on and off.
pub const EU_XON: usize = 79;
pub const EU_XOF: usize = 80;

// The pseudo fields after `eu_LAST`, for library items with no column.
pub const EU_CMDLINE: usize = EU_MAXPFLGS;
pub const EU_TICS_ALL_C: usize = EU_MAXPFLGS + 1;
pub const EU_ID_FUID: usize = EU_MAXPFLGS + 2;
pub const EU_CMDLINE_V: usize = EU_MAXPFLGS + 3;
pub const EU_ENVIRON_V: usize = EU_MAXPFLGS + 4;
pub const EU_TREE_HID: usize = EU_MAXPFLGS + 5;
pub const EU_TREE_LVL: usize = EU_MAXPFLGS + 6;
pub const EU_TREE_ADD: usize = EU_MAXPFLGS + 7;
/// `eu_RESET`: from here on the slots are `PIDS_extra`.
pub const EU_RESET: usize = EU_TREE_HID;
/// `MAXTBL(Fieldstab)`.
pub const FIELDSTAB_LEN: usize = EU_MAXPFLGS + 8;

/// `enum scale_enum`.
pub const SK_KB: i32 = 0;
pub const SK_MB: i32 = 1;
pub const SK_PB: i32 = 4;
pub const SK_EB: i32 = 5;

/// `enum resize_states`: `Frames_signal`.
pub const BREAK_OFF: i32 = 0;
pub const BREAK_KBD: i32 = 1;
pub const BREAK_SIG: i32 = 2;
pub const BREAK_AUTOX: i32 = 3;
pub const BREAK_SCREEN: i32 = 4;

pub const GROUPSMAX: usize = 4;
pub const WINNAMSIZ: usize = 4;
pub const GRPNAMSIZ: usize = WINNAMSIZ + 2;

// The window flags.
pub const VIEW_CPUSUM: i32 = 0x0000_8000;
pub const VIEW_CPUNOD: i32 = 0x0040_0000;
pub const VIEW_LOADAV: i32 = 0x0000_4000;
pub const VIEW_STATES: i32 = 0x0000_2000;
pub const VIEW_MEMORY: i32 = 0x0000_1000;
pub const VIEW_NOBOLD: i32 = 0x0000_0008;
pub const VIEW_SCROLL: i32 = 0x0008_0000;
pub const SHOW_COLORS: i32 = 0x0000_0800;
pub const SHOW_HIBOLD: i32 = 0x0000_0400;
pub const SHOW_HICOLS: i32 = 0x0000_0200;
pub const SHOW_HIROWS: i32 = 0x0000_0100;
pub const SHOW_CMDLIN: i32 = 0x0000_0080;
pub const SHOW_CTIMES: i32 = 0x0000_0040;
pub const SHOW_IDLEPS: i32 = 0x0000_0020;
pub const SHOW_TASKON: i32 = 0x0000_0010;
pub const SHOW_FOREST: i32 = 0x0000_0002;
pub const QSRT_NORMAL: i32 = 0x0000_0004;
pub const SHOW_JRSTRS: i32 = 0x0004_0000;
pub const SHOW_JRNUMS: i32 = 0x0002_0000;
pub const NOPRINT_XXX: i32 = 0x0001_0000;
pub const EQUWINS_XXX: i32 = 0x0000_0001;

/// `DEF_WINFLGS`, `ORIG_TOPDEFS`'s.
pub const DEF_WINFLGS: i32 = VIEW_LOADAV
    | VIEW_STATES
    | VIEW_CPUSUM
    | VIEW_MEMORY
    | SHOW_HIBOLD
    | SHOW_HIROWS
    | SHOW_IDLEPS
    | SHOW_TASKON
    | SHOW_JRNUMS
    | QSRT_NORMAL;
/// `ALT_WINFLGS`, `ORIG_TOPDEFS`'s: the same.
pub const ALT_WINFLGS: i32 = DEF_WINFLGS;

/// `enum reflag_enum`.
pub const FLAGS_TOG: i32 = 0;
pub const FLAGS_SET: i32 = 1;
pub const FLAGS_OFF: i32 = 2;

/// `enum warn_enum`.
pub const WARN_ALT: i32 = 0;
pub const WARN_VIZ: i32 = 1;

/// `FLDon`.
pub const FLD_ON: i32 = 0x01;
/// `FLD_OFFSET`: `'%'`.
pub const FLD_OFFSET: i32 = b'%' as i32;
/// `FLD_ROWMAX`: `fieldscur` numbers per rcfile line.
pub const FLD_ROWMAX: usize = 20;

pub const SYS_RCRESTRICT: &str = "/etc/toprc";
pub const SYS_RCDEFAULTS: &str = "/etc/topdefaultrc";
pub const RCF_EYECATCHER: &[u8] = b"Config File (Linux processes with windows)\n";
pub const RCF_PLUS_H: &[u8] = b"\\]^_`abcdefghij";
pub const RCF_PLUS_J: &[u8] = b"klmnopqrstuvwxyz";
pub const RCF_XFORMED_ID: u8 = b'k';
pub const RCF_VERSION_ID: u8 = b'k';

/// The fields of the old (3.2.8 to 3.3.17) rcfile formats, for `cfg_xform`:
/// `DEF_FORMER` (`ORIG_TOPDEFS`'s), `JOB_FORMER`, `MEM_FORMER`, `USR_FORMER`
/// and `CVT_FORMER`, each followed by `RCF_PLUS_H` and `RCF_PLUS_J`.
pub const DEF_FORMER: &[u8] =
    b"\xa5\xa8\xb3\xb4\xbb\xbd\xc0\xc4\xb7\xba\xb9\xc5&')*+,-./012568<>?ABCFGHIJKLMNOPQRSTUVWXYZ[";
pub const JOB_FORMER: &[u8] =
    b"\xa5\xa6\xb9\xb7\xba(\xb3\xb4\xc4\xbb\xbd@<\xa7\xc5)*+,-./012568>?ABCFGHIJKLMNOPQRSTUVWXYZ[";
pub const MEM_FORMER: &[u8] =
    b"\xa5\xba\xbb<\xbd\xbe\xbf\xc0\xc1MBN\xc3D34\xb7\xc5&'()*+,-./0125689FGHIJKLOPQRSTUVWXYZ[";
pub const USR_FORMER: &[u8] =
    b"\xa5\xa6\xa7\xa8\xaa\xb0\xb9\xb7\xba\xc4\xc5)+,-./1234568;<=>?@ABCFGHIJKLMNOPQRSTUVWXYZ[";
pub const CVT_FORMER: &[u8] = b"%&*'(-0346789:;<=>?@ACDEFGML)+,./125BHIJKNOPQRSTUVWXYZ[";
/// `CVT_FLDMAX`.
pub const CVT_FLDMAX: usize = 28;

/// One of the `*_FORMER` strings with its two suffixes, as C's adjacent
/// literals join them.
#[must_use]
pub fn former(head: &[u8]) -> Vec<u8> {
    [head, RCF_PLUS_H, RCF_PLUS_J].concat()
}

/// `DEF_FIELDS`, `ORIG_TOPDEFS`'s.
pub const DEF_FIELDS: [i32; 100] = [
    75, 81, 103, 105, 119, 123, 129, 137, 111, 117, 115, 139, 76, 78, 82, 84, 86, 88, 90, 92, 94,
    96, 98, 100, 106, 108, 112, 120, 124, 126, 130, 132, 134, 140, 142, 144, 146, 148, 150, 152,
    154, 156, 158, 160, 162, 164, 166, 168, 170, 172, 174, 176, 178, 180, 182, 184, 186, 188, 190,
    192, 194, 196, 198, 200, 202, 204, 206, 208, 210, 212, 214, 216, 218, 220, 222, 224, 226, 228,
    230, 232, 234, 236, 238, 240, 242, 244, 246, 248, 250, 252, 254, 256, 258, 260, 262, 264, 266,
    268, 270, 272,
];
/// `JOB_FIELDS`.
pub const JOB_FIELDS: [i32; 100] = [
    75, 77, 115, 111, 117, 80, 103, 105, 137, 119, 123, 128, 120, 79, 139, 82, 84, 86, 88, 90, 92,
    94, 96, 98, 100, 106, 108, 112, 124, 126, 130, 132, 134, 140, 142, 144, 146, 148, 150, 152,
    154, 156, 158, 160, 162, 164, 166, 168, 170, 172, 174, 176, 178, 180, 182, 184, 186, 188, 190,
    192, 194, 196, 198, 200, 202, 204, 206, 208, 210, 212, 214, 216, 218, 220, 222, 224, 226, 228,
    230, 232, 234, 236, 238, 240, 242, 244, 246, 248, 250, 252, 254, 256, 258, 260, 262, 264, 266,
    268, 270, 272,
];
/// `MEM_FIELDS`.
pub const MEM_FIELDS: [i32; 100] = [
    75, 117, 119, 120, 123, 125, 127, 129, 131, 154, 132, 156, 135, 136, 102, 104, 111, 139, 76,
    78, 80, 82, 84, 86, 88, 90, 92, 94, 96, 98, 100, 106, 108, 112, 114, 140, 142, 144, 146, 148,
    150, 152, 158, 160, 162, 164, 166, 168, 170, 172, 174, 176, 178, 180, 182, 184, 186, 188, 190,
    192, 194, 196, 198, 200, 202, 204, 206, 208, 210, 212, 214, 216, 218, 220, 222, 224, 226, 228,
    230, 232, 234, 236, 238, 240, 242, 244, 246, 248, 250, 252, 254, 256, 258, 260, 262, 264, 266,
    268, 270, 272,
];
/// `USR_FIELDS`.
pub const USR_FIELDS: [i32; 100] = [
    75, 77, 79, 81, 85, 97, 115, 111, 117, 137, 139, 82, 86, 88, 90, 92, 94, 98, 100, 102, 104,
    106, 108, 112, 118, 120, 122, 124, 126, 128, 130, 132, 134, 140, 142, 144, 146, 148, 150, 152,
    154, 156, 158, 160, 162, 164, 166, 168, 170, 172, 174, 176, 178, 180, 182, 184, 186, 188, 190,
    192, 194, 196, 198, 200, 202, 204, 206, 208, 210, 212, 214, 216, 218, 220, 222, 224, 226, 228,
    230, 232, 234, 236, 238, 240, 242, 244, 246, 248, 250, 252, 254, 256, 258, 260, 262, 264, 266,
    268, 270, 272,
];

/// `COLOR_*` from `<curses.h>`.
pub const COLOR_RED: i32 = 1;
pub const COLOR_GREEN: i32 = 2;
pub const COLOR_YELLOW: i32 = 3;
pub const COLOR_BLUE: i32 = 4;
pub const COLOR_MAGENTA: i32 = 5;
pub const COLOR_CYAN: i32 = 6;
pub const COLOR_WHITE: i32 = 7;

/// `RCW_t`: the window part of the rcfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rcw {
    pub sortindx: i32,
    pub winflags: i32,
    pub maxtasks: i32,
    pub graph_cpus: i32,
    pub graph_mems: i32,
    pub double_up: i32,
    pub combine_cpus: i32,
    pub core_types: i32,
    pub summclr: i32,
    pub msgsclr: i32,
    pub headclr: i32,
    pub taskclr: i32,
    /// At most three bytes of name.
    pub winname: Vec<u8>,
    /// `fieldscur`: [`PFLAGSSIZ`] numbers, ended by the first 0.
    pub fieldscur: [i32; PFLAGSSIZ],
}

/// `RCF_t`: the rcfile.
#[derive(Clone, Debug, PartialEq)]
pub struct Rcf {
    pub id: u8,
    pub mode_altscr: i32,
    pub mode_irixps: i32,
    pub delay_time: f32,
    pub win_index: i32,
    pub win: [Rcw; GROUPSMAX],
    pub fixed_widest: i32,
    pub summ_mscale: i32,
    pub task_mscale: i32,
    pub zero_suppress: i32,
    pub tics_scaled: i32,
}

/// A fields list, padded with zeros to [`PFLAGSSIZ`].
fn fields(list: &[i32; 100]) -> [i32; PFLAGSSIZ] {
    let mut out = [0i32; PFLAGSSIZ];
    for (o, v) in out.iter_mut().zip(list.iter()) {
        *o = *v;
    }
    out
}

/// One window's defaults.
#[allow(clippy::too_many_arguments)]
fn rcw(
    sortindx: usize,
    winflags: i32,
    double_up: i32,
    clrs: [i32; 4],
    name: &[u8],
    flds: &[i32; 100],
) -> Rcw {
    Rcw {
        sortindx: i32::try_from(sortindx).unwrap_or(0),
        winflags,
        maxtasks: 0,
        graph_cpus: 0,
        graph_mems: 0,
        double_up,
        combine_cpus: 0,
        core_types: 0,
        summclr: clrs[0],
        msgsclr: clrs[1],
        headclr: clrs[2],
        taskclr: clrs[3],
        winname: name.to_vec(),
        fieldscur: fields(flds),
    }
}

/// `DEF_RCFILE`, `ORIG_TOPDEFS`'s.
#[must_use]
pub fn def_rcfile() -> Rcf {
    Rcf {
        id: RCF_VERSION_ID,
        mode_altscr: 0,
        mode_irixps: 1,
        delay_time: DEF_DELAY,
        win_index: 0,
        win: [
            rcw(
                EU_CPU,
                DEF_WINFLGS,
                1,
                [COLOR_RED, COLOR_RED, COLOR_YELLOW, COLOR_RED],
                b"Def",
                &DEF_FIELDS,
            ),
            rcw(
                EU_PID,
                ALT_WINFLGS,
                0,
                [COLOR_CYAN, COLOR_CYAN, COLOR_WHITE, COLOR_CYAN],
                b"Job",
                &JOB_FIELDS,
            ),
            rcw(
                EU_MEM,
                ALT_WINFLGS,
                0,
                [COLOR_MAGENTA, COLOR_MAGENTA, COLOR_BLUE, COLOR_MAGENTA],
                b"Mem",
                &MEM_FIELDS,
            ),
            rcw(
                EU_UEN,
                ALT_WINFLGS,
                0,
                [COLOR_YELLOW, COLOR_YELLOW, COLOR_GREEN, COLOR_YELLOW],
                b"Usr",
                &USR_FIELDS,
            ),
        ],
        fixed_widest: 0,
        summ_mscale: SK_MB,
        task_mscale: SK_KB,
        zero_suppress: 0,
        tics_scaled: 0,
    }
}

/// A `Fieldstab` entry: width (-1 a variable width, 0 set once at start),
/// scale target (-1 none), the window flag that justifies it, and the
/// library item behind it.
#[derive(Clone, Copy, Debug)]
pub struct Field {
    pub width: i32,
    pub scale: i32,
    pub align: i32,
    pub item: Item,
}

/// `A_left` and `A_right`: the flags that toggle each column's justification.
const A_LEFT: i32 = SHOW_JRSTRS;
const A_RIGHT: i32 = SHOW_JRNUMS;

/// `Fieldstab`, as compiled: [`crate::fields`] widens and rescales it at
/// run time.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn fieldstab() -> [Field; FIELDSTAB_LEN] {
    use Item as P;
    let f = |width: i32, scale: i32, align: i32, item: Item| Field {
        width,
        scale,
        align,
        item,
    };
    [
        f(0, -1, A_RIGHT, P::IdPid),
        f(0, -1, A_RIGHT, P::IdPpid),
        f(5, -1, A_RIGHT, P::IdEuid),
        f(8, -1, A_LEFT, P::IdEuser),
        f(5, -1, A_RIGHT, P::IdRuid),
        f(8, -1, A_LEFT, P::IdRuser),
        f(5, -1, A_RIGHT, P::IdSuid),
        f(8, -1, A_LEFT, P::IdSuser),
        f(5, -1, A_RIGHT, P::IdEgid),
        f(8, -1, A_LEFT, P::IdEgroup),
        f(0, -1, A_RIGHT, P::IdPgrp),
        f(8, -1, A_LEFT, P::TtyName),
        f(0, -1, A_RIGHT, P::IdTpgid),
        f(0, -1, A_RIGHT, P::IdSession),
        f(3, -1, A_RIGHT, P::Priority),
        f(3, -1, A_RIGHT, P::Nice),
        f(3, -1, A_RIGHT, P::Nlwp),
        f(0, -1, A_RIGHT, P::Processor),
        f(5, -1, A_RIGHT, P::TicsAllDelta),
        f(6, -1, A_RIGHT, P::TicsAll),
        f(9, -1, A_RIGHT, P::TicsAll),
        f(5, -1, A_RIGHT, P::MemRes),
        f(7, SK_KB, A_RIGHT, P::MemVirt),
        f(6, SK_KB, A_RIGHT, P::VmSwap),
        f(6, SK_KB, A_RIGHT, P::MemRes),
        f(6, SK_KB, A_RIGHT, P::MemCode),
        f(7, SK_KB, A_RIGHT, P::MemData),
        f(6, SK_KB, A_RIGHT, P::MemShr),
        f(4, -1, A_RIGHT, P::FltMaj),
        f(4, -1, A_RIGHT, P::FltMin),
        f(4, -1, A_RIGHT, P::Noop),
        f(1, -1, A_RIGHT, P::State),
        f(-1, -1, A_LEFT, P::Cmd),
        f(10, -1, A_LEFT, P::WchanName),
        f(8, -1, A_LEFT, P::Flags),
        f(-1, -1, A_LEFT, P::Cgroup),
        f(-1, -1, A_LEFT, P::Supgids),
        f(-1, -1, A_LEFT, P::Supgroups),
        f(0, -1, A_RIGHT, P::IdTgid),
        f(5, -1, A_RIGHT, P::OomAdj),
        f(4, -1, A_RIGHT, P::OomScore),
        f(-1, -1, A_LEFT, P::Environ),
        f(3, -1, A_RIGHT, P::FltMajDelta),
        f(3, -1, A_RIGHT, P::FltMinDelta),
        f(6, SK_KB, A_RIGHT, P::VmUsed),
        f(10, -1, A_RIGHT, P::NsIpc),
        f(10, -1, A_RIGHT, P::NsMnt),
        f(10, -1, A_RIGHT, P::NsNet),
        f(10, -1, A_RIGHT, P::NsPid),
        f(10, -1, A_RIGHT, P::NsUser),
        f(10, -1, A_RIGHT, P::NsUts),
        f(8, -1, A_LEFT, P::Lxcname),
        f(6, SK_KB, A_RIGHT, P::VmRssAnon),
        f(6, SK_KB, A_RIGHT, P::VmRssFile),
        f(6, SK_KB, A_RIGHT, P::VmRssLocked),
        f(6, SK_KB, A_RIGHT, P::VmRssShared),
        f(-1, -1, A_LEFT, P::Cgname),
        f(0, -1, A_RIGHT, P::ProcessorNode),
        f(5, -1, A_RIGHT, P::IdLogin),
        f(-1, -1, A_LEFT, P::Exe),
        f(6, SK_KB, A_RIGHT, P::SmapRss),
        f(6, SK_KB, A_RIGHT, P::SmapPss),
        f(6, SK_KB, A_RIGHT, P::SmapPssAnon),
        f(6, SK_KB, A_RIGHT, P::SmapPssFile),
        f(6, SK_KB, A_RIGHT, P::SmapPssShmem),
        f(6, SK_KB, A_RIGHT, P::SmapPrvTotal),
        f(6, -1, A_RIGHT, P::IoReadBytes),
        f(5, -1, A_RIGHT, P::IoReadOps),
        f(6, -1, A_RIGHT, P::IoWriteBytes),
        f(5, -1, A_RIGHT, P::IoWriteOps),
        f(5, -1, A_RIGHT, P::AutogrpId),
        f(4, -1, A_RIGHT, P::AutogrpNice),
        f(7, -1, A_RIGHT, P::TicsBegan),
        f(7, -1, A_RIGHT, P::TimeElapsed),
        f(6, -1, A_RIGHT, P::Utilization),
        f(7, -1, A_RIGHT, P::UtilizationC),
        f(10, -1, A_RIGHT, P::NsCgroup),
        f(10, -1, A_RIGHT, P::NsTime),
        // The pseudo fields: library items with no column of their own.
        f(-1, -1, -1, P::Cmdline),
        f(-1, -1, -1, P::TicsAllC),
        f(-1, -1, -1, P::IdFuid),
        f(-1, -1, -1, P::CmdlineV),
        f(-1, -1, -1, P::EnvironV),
        f(-1, -1, -1, P::Extra),
        f(-1, -1, -1, P::Extra),
        f(-1, -1, -1, P::Extra),
    ]
}
