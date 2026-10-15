//! "Startup routines" that read what the user saved and typed: the rcfiles
//! (`/etc/toprc`, the personal one, `/etc/topdefaultrc`), their "other
//! filter" and inspect sections, the conversion of every older format, the
//! command line -- and `W`, which writes the personal rcfile back.
//!
//! The rcfile is read as upstream reads it, with `fgets` and `fscanf` on
//! one stream: [`CFile`] is that stream, its scanning glibc's (a directive
//! that fails leaves the input where glibc leaves it), so that a damaged or
//! hand-edited file is accepted or refused exactly as upstream would.

use std::ffi::OsString;

use getoptlong::{Opt, Program, Takes};

use crate::cfmt::Arg;
use crate::defs::{
    CVT_FLDMAX, DEF_DELAY, EU_MAXPFLGS, FLD_ON, FLD_ROWMAX, GROUPSMAX, LRGBUFSIZ, MONPIDMAX,
    OURPATHSZ, QSRT_NORMAL, RCF_EYECATCHER, RCF_PLUS_H, RCF_PLUS_J, RCF_VERSION_ID, RCF_XFORMED_ID,
    SCREENMAX, SHOW_CMDLIN, SHOW_CTIMES, SHOW_FOREST, SHOW_HICOLS, SHOW_IDLEPS, SHOW_JRNUMS,
    SHOW_TASKON, SK_EB, SK_KB, SK_PB, SMLBUFSIZ, SYS_RCDEFAULTS, SYS_RCRESTRICT, VIEW_CPUNOD,
    VIEW_CPUSUM, VIEW_NOBOLD, VIEW_STATES, W_MIN_COL,
};
use crate::fields::{fld_get, mlen, n_col};
use crate::fmt::TICS_AS_LAST;
use crate::term::c_str;
use crate::{IEnt, Top, insp, nls};

/// `Osel_delim_1_txt`, `Osel_delim_2_txt`.
pub const OSEL_DELIM_1: &[u8] = b"begin: saved other filter data -------------------\n";
pub const OSEL_DELIM_2: &[u8] = b"end  : saved other filter data -------------------\n";
/// `OSEL_FILTER`.
const OSEL_FILTER: &[u8] = b"filter=";
/// `E_CORES_ONLY`: the highest `core_types`.
const E_CORES_ONLY: i32 = 3;

/// One `fscanf` conversion's result.
#[derive(Clone, Debug, PartialEq)]
pub enum Got {
    Int(i32),
    Char(u8),
    Str(Vec<u8>),
    Float(f32),
}

impl Got {
    /// The `%d` value.
    #[must_use]
    pub fn int(&self) -> i32 {
        match self {
            Self::Int(v) => *v,
            _ => 0,
        }
    }
}

/// glibc's `isspace`, in every locale this tree has.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | 0x09..=0x0d)
}

/// A file read with stdio's `fgets` and `fscanf`, which share its
/// position.
#[derive(Clone, Debug)]
pub struct CFile {
    data: Vec<u8>,
    pos: usize,
    /// The decimal point `%f` reads: the locale's, as `fscanf` has it.
    radix: u8,
}

impl CFile {
    /// `fopen (path, "r")`, the file read whole.
    ///
    /// # Errors
    ///
    /// The file could not be opened or read.
    pub fn open(path: &[u8]) -> std::io::Result<Self> {
        if path.is_empty() {
            // "fopen() shall fail if pathname is an empty string."
            return Err(std::io::Error::from_raw_os_error(2));
        }
        let data = std::fs::read(quoting::os_from_bytes(path))?;
        Ok(Self::with_radix(&data, crate::cfmt::radix()))
    }

    /// A stream over `data`, read in the locale's numbers: `sscanf`'s.
    #[must_use]
    pub fn from_bytes(data: &[u8]) -> Self {
        Self::with_radix(data, crate::cfmt::radix())
    }

    /// A stream over `data` whose `%f` reads `radix` as the decimal point.
    #[must_use]
    pub fn with_radix(data: &[u8], radix: u8) -> Self {
        Self {
            data: data.to_vec(),
            pos: 0,
            radix,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// `fgets (buf, size, fp)`: up to `size - 1` bytes, through a newline;
    /// `None` at the end of the file.
    pub fn fgets(&mut self, size: usize) -> Option<Vec<u8>> {
        let rest = self.data.get(self.pos..)?;
        if rest.is_empty() {
            return None;
        }
        let max = size.saturating_sub(1);
        let mut n = 0;
        while n < max {
            let Some(&b) = rest.get(n) else { break };
            n = n.saturating_add(1);
            if b == b'\n' {
                break;
            }
        }
        self.pos = self.pos.saturating_add(n);
        Some(rest.get(..n).unwrap_or_default().to_vec())
    }

    /// `fscanf (fp, fmt, ...)` for the directives the rcfile needs --
    /// whitespace, literals, `%d`, `%c`, `%s`, `%Ns`, `%*s` and `%f` -- the
    /// count glibc returns (-1 when the input ended before the first
    /// conversion), and the values converted, in order.
    pub fn scanf(&mut self, fmt: &[u8]) -> (i32, Vec<Got>) {
        let mut got = Vec::new();
        let mut assigned = 0i32;
        let mut converted_any = false;
        let mut i = 0usize;
        let eof_result = |converted_any: bool, assigned: i32| {
            if converted_any { assigned } else { -1 }
        };
        while let Some(&f) = fmt.get(i) {
            if is_space(f) {
                while self.peek().is_some_and(is_space) {
                    self.pos = self.pos.saturating_add(1);
                }
                i = i.saturating_add(1);
                continue;
            }
            if f != b'%' {
                match self.peek() {
                    None => return (eof_result(converted_any, assigned), got),
                    Some(c) if c == f => {
                        self.pos = self.pos.saturating_add(1);
                        i = i.saturating_add(1);
                        continue;
                    }
                    Some(_) => return (assigned, got),
                }
            }
            // A conversion.
            i = i.saturating_add(1);
            let mut suppress = false;
            if fmt.get(i) == Some(&b'*') {
                suppress = true;
                i = i.saturating_add(1);
            }
            let mut width = 0usize;
            while let Some(d) = fmt.get(i).filter(|d| d.is_ascii_digit()) {
                width = width
                    .saturating_mul(10)
                    .saturating_add(usize::from(d.wrapping_sub(b'0')));
                i = i.saturating_add(1);
            }
            let Some(&conv) = fmt.get(i) else {
                return (assigned, got);
            };
            i = i.saturating_add(1);
            if conv != b'c' {
                while self.peek().is_some_and(is_space) {
                    self.pos = self.pos.saturating_add(1);
                }
            }
            if self.peek().is_none() {
                return (eof_result(converted_any, assigned), got);
            }
            match conv {
                b'd' => {
                    let start = self.pos;
                    if matches!(self.peek(), Some(b'+' | b'-')) {
                        self.pos = self.pos.saturating_add(1);
                    }
                    let digits_from = self.pos;
                    while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                        self.pos = self.pos.saturating_add(1);
                    }
                    if self.pos == digits_from {
                        // The sign stays consumed; the byte after it is
                        // pushed back.
                        return (assigned, got);
                    }
                    let text = self.data.get(start..self.pos).unwrap_or_default();
                    let (v, _) = cstrtol::strtol(text, 10);
                    converted_any = true;
                    if !suppress {
                        got.push(Got::Int(i32::from_le_bytes(
                            v.to_le_bytes()[..4].try_into().unwrap_or([0; 4]),
                        )));
                        assigned = assigned.saturating_add(1);
                    }
                }
                b'c' => {
                    let c = self.peek().unwrap_or(0);
                    self.pos = self.pos.saturating_add(1);
                    converted_any = true;
                    if !suppress {
                        got.push(Got::Char(c));
                        assigned = assigned.saturating_add(1);
                    }
                }
                b's' => {
                    let max = if width == 0 { usize::MAX } else { width };
                    let start = self.pos;
                    while self.pos.saturating_sub(start) < max
                        && self.peek().is_some_and(|b| !is_space(b))
                    {
                        self.pos = self.pos.saturating_add(1);
                    }
                    converted_any = true;
                    if !suppress {
                        got.push(Got::Str(
                            self.data.get(start..self.pos).unwrap_or_default().to_vec(),
                        ));
                        assigned = assigned.saturating_add(1);
                    }
                }
                b'f' => {
                    let rest = self.data.get(self.pos..).unwrap_or_default();
                    let (value, consumed) = crate::fmt::strtof_in(rest, self.radix);
                    if consumed == 0 {
                        return (assigned, got);
                    }
                    self.pos = self.pos.saturating_add(consumed);
                    converted_any = true;
                    if !suppress {
                        got.push(Got::Float(value));
                        assigned = assigned.saturating_add(1);
                    }
                }
                _ => return (assigned, got),
            }
        }
        (assigned, got)
    }
}

/// `sscanf (s, fmt, ...)`: [`CFile::scanf`] over a string, which ends at
/// its first NUL.
#[must_use]
pub fn sscanf(s: &[u8], fmt: &[u8]) -> (i32, Vec<Got>) {
    CFile::from_bytes(c_str(s)).scanf(fmt)
}

/// `scat (buf, s)` into a C buffer of `LRGBUFSIZ` bytes: `s` appended after
/// the string the buffer holds.
fn scat_buf(buf: &mut [u8; LRGBUFSIZ], s: &[u8]) {
    let at = c_str(buf).len();
    for (k, &b) in s.iter().enumerate() {
        if let Some(slot) = buf.get_mut(at.saturating_add(k)) {
            *slot = b;
        }
    }
    if let Some(slot) = buf.get_mut(at.saturating_add(s.len())) {
        *slot = 0;
    }
}

/// A word written into a C buffer: its bytes, then a NUL; whatever lay
/// past them stays.
fn put_cstr(buf: &mut [u8; LRGBUFSIZ], s: &[u8]) {
    let n = s.len().min(LRGBUFSIZ - 1);
    for (slot, &b) in buf.iter_mut().zip(s.iter().take(n)) {
        *slot = b;
    }
    if let Some(slot) = buf.get_mut(n) {
        *slot = 0;
    }
}

impl Top {
    /// `cfg_xform (q, flds, defs)`: a 3.2.8 to 3.3.17 `fieldscur` (letters,
    /// or one byte per field) made the numbers this version keeps. `true`
    /// when the string cannot be read.
    fn cfg_xform(&mut self, w: usize, flds: &mut [u8; LRGBUFSIZ], defs: &[u8]) -> bool {
        const FLAGS_TAB: [(i32, i32); 5] = [
            (0x00_0001, VIEW_NOBOLD),
            (0x00_0008, SHOW_TASKON),
            (0x00_0010, QSRT_NORMAL),
            (0x00_0200, SHOW_HICOLS),
            (0x01_0000, 0),
        ];
        const NULL_FLDS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
        if self.rc.id == b'a' {
            let win = &mut self.winstk[w];
            let mut x = win.rc.winflags;
            win.rc.winflags = 0;
            for (old, new) in FLAGS_TAB {
                if x & old != 0 {
                    x &= !old;
                    win.rc.winflags |= new;
                }
            }
            win.rc.winflags |= x;

            let f = c_str(flds).len();
            if f >= CVT_FLDMAX {
                return true;
            }
            let src = crate::defs::former(crate::defs::CVT_FORMER);
            let mut dst = [0u8; crate::defs::PFLAGSSIZ];
            for (d, &s) in dst.iter_mut().zip(&src) {
                *d = s;
            }
            let p1 = c_str(flds).iter().position(|&b| b == b'[');
            if let Some(p) = p1 {
                if let Some(b) = flds.get_mut(p) {
                    *b = b'{';
                }
            }
            let p2 = c_str(flds).iter().position(|&b| b == b'\\');
            if let Some(p) = p2 {
                if let Some(b) = flds.get_mut(p) {
                    *b = b'|';
                }
            }
            for i in 0..f {
                let c = flds.get(i).copied().unwrap_or(0);
                let x = i32::from(c.to_ascii_lowercase()).wrapping_sub(i32::from(b'a'));
                if x < 0 || x >= CVT_FLDMAX as i32 {
                    return true;
                }
                let to = src
                    .get(usize::try_from(x).unwrap_or(0))
                    .copied()
                    .unwrap_or(0);
                let upper = c.is_ascii_uppercase();
                if let Some(d) = dst.get_mut(i) {
                    *d = if upper { to | 0x80 } else { to };
                }
            }
            if let Some(p) = p1 {
                if let Some(d) = dst.get_mut(p) {
                    *d |= 0x80;
                }
            }
            if let Some(p) = p2 {
                if let Some(d) = dst.get_mut(p) {
                    *d |= 0x80;
                }
            }

            // "next, we must adjust the old sort field enum ..."
            let x = self.winstk[w].rc.sortindx;
            let c = usize::try_from(x)
                .ok()
                .and_then(|i| NULL_FLDS.get(i))
                .copied()
                .unwrap_or(0);
            self.winstk[w].rc.sortindx = 0;
            let window = &flds[..CVT_FLDMAX];
            let found = window
                .iter()
                .position(|&b| b == c)
                .or_else(|| window.iter().position(|&b| b == c.to_ascii_uppercase()));
            if let Some(x) = found {
                let d = dst.get(x).copied().unwrap_or(0);
                self.winstk[w].rc.sortindx =
                    i32::from(d & 0x7f).wrapping_sub(crate::defs::FLD_OFFSET);
            }
            // "now we're in a 3.3.0 format (soon to be transformed) ..."
            let n = c_str(&dst).len();
            put_cstr(flds, dst.get(..n).unwrap_or_default());
        }

        // "lastly, let's attend to the 3.3.0 - 3.3.17 fieldcurs format ..."
        let x = defs.len();
        for i in 0..x {
            let b = flds.get(i).copied().unwrap_or(0);
            let mut f = i32::from(b & 0x7f) << 1;
            if b & 0x80 != 0 {
                f |= FLD_ON;
            }
            if let Some(slot) = self.winstk[w].rc.fieldscur.get_mut(i) {
                *slot = f;
            }
        }
        false
    }

    /// `config_insp (fp, buf, size)`: the rcfile's inspect entries -- or,
    /// without any, the three demonstration ones.
    fn config_insp(&mut self, fp: &mut CFile, first: Vec<u8>) {
        let mut buf = first;
        self.inspect.raw = Some(if buf.first() != Some(&b'\n') {
            b"\n".to_vec()
        } else {
            Vec::new()
        });
        let mut tab: Vec<IEnt> = Vec::new();
        loop {
            if buf.is_empty() {
                match fp.fgets(LRGBUFSIZ) {
                    Some(line) => buf = line,
                    None => break,
                }
            }
            if let Some(raw) = self.inspect.raw.as_mut() {
                raw.extend_from_slice(&buf);
            }
            if buf.first() == Some(&b'#') || buf.first() == Some(&b'\n') {
                buf.clear();
                continue;
            }
            // "part of this is used in a show_special() call, so let's
            // sanitize it"
            for b in &mut buf {
                if *b != b'\t' && *b != b'\n' && *b < b' ' {
                    *b = b'.';
                    self.rc_questions = true;
                }
            }
            let mut fields = buf
                .split(|&b| b == b'\t' || b == b'\n')
                .filter(|s| !s.is_empty());
            let (Some(s1), Some(s2), Some(s3)) = (fields.next(), fields.next(), fields.next())
            else {
                self.rc_questions = true;
                buf.clear();
                continue;
            };
            let func = match buf.first().map(u8::to_ascii_uppercase) {
                Some(b'F') => insp::Func::File,
                Some(b'P') => insp::Func::Pipe,
                _ => {
                    self.rc_questions = true;
                    buf.clear();
                    continue;
                }
            };
            tab.push(IEnt {
                func,
                typ: s1.to_vec(),
                name: s2.to_vec(),
                fmts: s3.to_vec(),
                caps: b"",
                fstr: Vec::new(),
                flen: 0,
            });
            buf.clear();
        }
        self.inspect.total = i32::try_from(tab.len()).unwrap_or(i32::MAX);
        self.inspect.tab = tab;
        if self.inspect.total == 0 {
            let sels = [
                nls::YINSP_DEMO01_TXT,
                nls::YINSP_DEMO02_TXT,
                nls::YINSP_DEMO03_TXT,
            ];
            self.inspect.demo = true;
            self.inspect.total = 3;
            self.inspect.tab = sels
                .iter()
                .map(|s| IEnt {
                    func: insp::Func::Demo,
                    typ: nls::YINSP_DEQTYP_TXT.as_bytes().to_vec(),
                    name: s.as_bytes().to_vec(),
                    fmts: nls::YINSP_DEQFMT_TXT.as_bytes().to_vec(),
                    caps: b"",
                    fstr: Vec::new(),
                    flen: 0,
                })
                .collect();
        }
    }

    /// `config_osel (fp, buf, size)`: the rcfile's "other filter" section.
    /// The line after it -- the first an inspect entry may be on -- is left
    /// in `buf`. A read at the end of the file leaves `buf` as it was, as
    /// `fgets` does: the line before stays there for the inspect entries to
    /// be read from, the section's last line among them.
    fn config_osel(&mut self, fp: &mut CFile, buf: &mut Vec<u8>) {
        let mut begun = false;
        loop {
            let Some(line) = fp.fgets(LRGBUFSIZ) else {
                return;
            };
            *buf = line;
            if buf.first() == Some(&b'\n') {
                continue;
            }
            let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
            if !begun && !has(buf, OSEL_DELIM_1) {
                return;
            }
            if !begun && has(buf, OSEL_DELIM_1) {
                begun = true;
                continue;
            }
            if begun && has(buf, OSEL_DELIM_2) {
                break;
            }
            let (n, v) = sscanf(buf, b"window #%d, osel_tot=%d\n");
            if n != 2 {
                self.rc_questions = true;
                return;
            }
            let wno = v.first().map_or(0, Got::int);
            let tot = v.get(1).map_or(0, Got::int);
            let Ok(wno) = usize::try_from(wno) else {
                self.rc_questions = true;
                return;
            };
            if wno >= GROUPSMAX || tot < 0 {
                self.rc_questions = true;
                return;
            }
            for _ in 0..tot {
                let Some(line) = fp.fgets(LRGBUFSIZ) else {
                    return;
                };
                *buf = line;
                let (n, v) = sscanf(buf, b"\ttype=%d,\tfilter=%*s\n");
                if 1 > n {
                    self.rc_questions = true;
                    return;
                }
                let ch = v.first().map_or(0, Got::int);
                let mut text = buf.clone();
                if let Some(nl) = text.iter().position(|&b| b == b'\n') {
                    text.truncate(nl);
                }
                let Some(at) = text
                    .windows(OSEL_FILTER.len())
                    .position(|w| w == OSEL_FILTER)
                else {
                    self.rc_questions = true;
                    return;
                };
                let glob = text
                    .get(at.saturating_add(OSEL_FILTER.len())..)
                    .unwrap_or_default()
                    .to_vec();
                if self.osel_add(wno, ch, &glob, false).is_some() {
                    self.rc_questions = true;
                    return;
                }
            }
        }
        // "let's prime that buf for the next guy..."
        if let Some(line) = fp.fgets(LRGBUFSIZ) {
            *buf = line;
        }
    }

    /// `configs_file (fp, name, delay)`: one rcfile read into `Rc` and the
    /// windows; the complaint, when it is damaged.
    fn configs_file(&mut self, fp: &mut CFile, name: &[u8], delay: &mut f32) -> Option<Vec<u8>> {
        let mut fbuf = [0u8; LRGBUFSIZ];
        let p = Self::fmtmk(nls::RC_BAD_FILES_FMT, &[Arg::S(name)]);
        // "ignore eyecatcher"
        if let Some(line) = fp.fgets(LRGBUFSIZ) {
            put_cstr(&mut fbuf, &line);
        }
        let (n, v) =
            fp.scanf(b"Id:%c, Mode_altscr=%d, Mode_irixps=%d, Delay_time=%d.%d, Curwin=%d\n");
        // Each conversion is stored as it is made, before a later one fails.
        if let Some(Got::Char(c)) = v.first() {
            self.rc.id = *c;
        }
        if let Some(g) = v.get(1) {
            self.rc.mode_altscr = g.int();
        }
        if let Some(g) = v.get(2) {
            self.rc.mode_irixps = g.int();
        }
        if n != 6 {
            return Some(p);
        }
        let tmp_whole = v.get(3).map_or(0, Got::int);
        let tmp_fract = v.get(4).map_or(0, Got::int);
        let i = v.get(5).map_or(0, Got::int);
        if self.rc.id < b'a' || self.rc.id > RCF_VERSION_ID {
            return Some(p);
        }
        if b"bcde".contains(&self.rc.id) {
            return Some(p);
        }
        if !(0..=1).contains(&self.rc.mode_altscr) || !(0..=1).contains(&self.rc.mode_irixps) {
            return Some(p);
        }
        if tmp_whole < 0 {
            return Some(p);
        }
        let Ok(cw) = usize::try_from(i) else {
            return Some(p);
        };
        if cw >= GROUPSMAX {
            return Some(p);
        }
        self.curwin = cw;
        // "this may be ugly, but it keeps us locale independent..."
        *delay =
            crate::c_float(f64::from(tmp_whole)) + crate::c_float(f64::from(tmp_fract)) / 1000.0;

        let def_flds = [
            crate::defs::former(crate::defs::DEF_FORMER),
            crate::defs::former(crate::defs::JOB_FORMER),
            crate::defs::former(crate::defs::MEM_FORMER),
            crate::defs::former(crate::defs::USR_FORMER),
        ];
        for (wi, defs) in def_flds.iter().enumerate() {
            let p = Self::fmtmk(
                nls::RC_BAD_ENTRY_FMT,
                &[
                    Arg::I(i64::try_from(wi).unwrap_or(0).saturating_add(1)),
                    Arg::S(name),
                ],
            );
            let (n, v) = fp.scanf(b"%3s\tfieldscur=");
            if let Some(Got::Str(s)) = v.first() {
                self.winstk[wi].rc.winname = s.clone();
            }
            if n != 1 {
                return Some(p);
            }
            if self.rc.id < RCF_XFORMED_ID {
                let (_, v) = fp.scanf(b"%s\n");
                if let Some(Got::Str(s)) = v.first() {
                    put_cstr(&mut fbuf, s);
                }
            } else {
                // Upstream reads numbers until one fails, however many; the
                // table here holds as many as upstream's does.
                let mut j = 0usize;
                loop {
                    let (n, v) = fp.scanf(b"%d");
                    if n != 1 {
                        break;
                    }
                    if let Some(slot) = self.winstk[wi].rc.fieldscur.get_mut(j) {
                        *slot = v.first().map_or(0, Got::int);
                    }
                    j = j.saturating_add(1);
                }
            }

            let (n, v) = fp.scanf(
                b"\twinflags=%d, sortindx=%d, maxtasks=%d, graph_cpus=%d, graph_mems=%d, double_up=%d, combine_cpus=%d, core_types=%d\n",
            );
            {
                let rc = &mut self.winstk[wi].rc;
                let slots: [&mut i32; 8] = [
                    &mut rc.winflags,
                    &mut rc.sortindx,
                    &mut rc.maxtasks,
                    &mut rc.graph_cpus,
                    &mut rc.graph_mems,
                    &mut rc.double_up,
                    &mut rc.combine_cpus,
                    &mut rc.core_types,
                ];
                for (slot, g) in slots.into_iter().zip(&v) {
                    *slot = g.int();
                }
            }
            if 3 > n {
                return Some(p);
            }
            let rc = &self.winstk[wi].rc;
            if rc.sortindx < 0 || rc.sortindx >= EU_MAXPFLGS as i32 {
                return Some(p);
            }
            if rc.maxtasks < 0
                || !(0..=2).contains(&rc.graph_cpus)
                || !(0..=2).contains(&rc.graph_mems)
                || rc.double_up < 0
                || rc.double_up >= crate::fields::ADJOIN_LIMIT
                || rc.combine_cpus < 0
                || !(0..=E_CORES_ONLY).contains(&rc.core_types)
            {
                return Some(p);
            }

            let (n, v) = fp.scanf(b"\tsummclr=%d, msgsclr=%d, headclr=%d, taskclr=%d\n");
            {
                let rc = &mut self.winstk[wi].rc;
                let slots: [&mut i32; 4] = [
                    &mut rc.summclr,
                    &mut rc.msgsclr,
                    &mut rc.headclr,
                    &mut rc.taskclr,
                ];
                for (slot, g) in slots.into_iter().zip(&v) {
                    *slot = g.int();
                }
            }
            if n != 4 {
                return Some(p);
            }
            let rc = &self.winstk[wi].rc;
            if [rc.summclr, rc.msgsclr, rc.headclr, rc.taskclr]
                .iter()
                .any(|c| !(0..=255).contains(c))
            {
                return Some(p);
            }

            // The fall-through of upstream's `switch (Rc.id)`.
            let id = self.rc.id;
            if id <= b'f' {
                self.winstk[wi].set(SHOW_JRNUMS);
            }
            if id <= b'g' && id > b'a' {
                scat_buf(&mut fbuf, RCF_PLUS_H);
            }
            if id <= b'h' {
                self.winstk[wi].rc.graph_cpus = 0;
                self.winstk[wi].rc.graph_mems = 0;
                self.rc.summ_mscale = SK_KB;
                self.rc.task_mscale = SK_KB;
            }
            if id <= b'i' {
                if id > b'a' {
                    scat_buf(&mut fbuf, RCF_PLUS_J);
                }
                self.winstk[wi].rc.double_up = 0;
                self.winstk[wi].rc.combine_cpus = 0;
            }
            if id <= b'j' {
                if self.cfg_xform(wi, &mut fbuf, defs) {
                    return Some(p);
                }
                self.rc.tics_scaled = 0;
            }
            let cur = &self.winstk[wi].rc.fieldscur;
            if mlen(cur) < EU_MAXPFLGS {
                return Some(p);
            }
            for &v in cur.iter().take(EU_MAXPFLGS) {
                let f = fld_get(v);
                if f >= EU_MAXPFLGS as i32 || f < 0 {
                    return Some(p);
                }
            }
            // "ensure there's been no manual alteration of fieldscur"
            for (n, &v) in cur.iter().enumerate().take(EU_MAXPFLGS) {
                if crate::fields::first_pos(cur, v) != Some(n) {
                    return Some(p);
                }
            }
        }

        // "any new addition(s) last, for older rcfiles compatibility..."
        let (_, v) = fp.scanf(
            b"Fixed_widest=%d, Summ_mscale=%d, Task_mscale=%d, Zero_suppress=%d, Tics_scaled=%d\n",
        );
        {
            let rc = &mut self.rc;
            let slots: [&mut i32; 5] = [
                &mut rc.fixed_widest,
                &mut rc.summ_mscale,
                &mut rc.task_mscale,
                &mut rc.zero_suppress,
                &mut rc.tics_scaled,
            ];
            for (slot, g) in slots.into_iter().zip(&v) {
                *slot = g.int();
            }
        }
        if self.rc.fixed_widest < -1 || self.rc.fixed_widest > SCREENMAX as i32 {
            self.rc.fixed_widest = 0;
        }
        if self.rc.summ_mscale < 0 || self.rc.summ_mscale > SK_EB {
            self.rc.summ_mscale = 0;
        }
        if self.rc.task_mscale < 0 || self.rc.task_mscale > SK_PB {
            self.rc.task_mscale = 0;
        }
        if !(0..=1).contains(&self.rc.zero_suppress) {
            self.rc.zero_suppress = 0;
        }
        if self.rc.tics_scaled < 0 || self.rc.tics_scaled > TICS_AS_LAST {
            self.rc.tics_scaled = 0;
        }

        if self.rc.id != RCF_VERSION_ID {
            self.rc_compatibility = true;
        }

        let mut buf = Vec::new();
        self.config_osel(fp, &mut buf);
        self.config_insp(fp, buf);
        None
    }

    /// `configs_path (fmts, ...)`: the rcfile's name, if it fits.
    fn configs_path(&mut self, path: Vec<u8>) -> usize {
        if path.is_empty() || path.len() >= OURPATHSZ {
            self.rc_name.clear();
            return 0;
        }
        let n = path.len();
        self.rc_name = path;
        n
    }

    /// `configs_reads`: the restrictions file, then the personal rcfile --
    /// the legacy name or the XDG one -- or else the system defaults.
    pub fn configs_reads(&mut self) {
        let mut tmp_delay = DEF_DELAY;

        if let Ok(mut fp) = CFile::open(SYS_RCRESTRICT.as_bytes())
            && fp.fgets(SMLBUFSIZ).is_some()
        {
            self.secure_mode = true;
            if let Some(line) = fp.fgets(SMLBUFSIZ)
                && let (1, v) = sscanf(&line, b"%f")
                && let Some(Got::Float(f)) = v.first()
            {
                self.rc.delay_time = *f;
            }
        }

        self.rc_name.clear();
        let env_bytes = |name: &str| std::env::var_os(name).map(OsString::into_encoded_bytes);
        let mut p_home = env_bytes("HOME");
        if p_home.as_ref().is_none_or(|h| h.first() != Some(&b'/')) {
            let db = pwdb::Db::load();
            p_home = db
                .user_by_uid(libcall::process::getuid())
                .map(|u| u.dir.clone())
                .filter(|h| h.first() == Some(&b'/'));
        }
        if let Some(home) = p_home.as_ref() {
            let path = [home.as_slice(), b"/.", self.myname.as_slice(), b"rc"].concat();
            self.configs_path(path);
        }

        // A file that does not open is `fopen`'s NULL, which the code below
        // answers with the XDG name and then the defaults.
        let mut fp = CFile::open(&self.rc_name).ok();
        let mut system_default = false;
        if fp.is_none() {
            let mut p = env_bytes("XDG_CONFIG_HOME");
            if p.as_ref().is_none_or(|x| x.first() != Some(&b'/')) {
                match p_home.as_ref() {
                    None => system_default = true,
                    Some(home) => {
                        let cfg = [home.as_slice(), b"/.config"].concat();
                        // Its failure -- most often, that it is there already -- shows when
                        // the rcfile is opened.
                        let _ = mkdir_0700(&cfg);
                        p = Some(cfg);
                    }
                }
            }
            if !system_default {
                let base = p.unwrap_or_default();
                if self.configs_path([base.as_slice(), b"/procps"].concat()) == 0 {
                    system_default = true;
                } else {
                    let dir = self.rc_name.clone();
                    // As above: what matters is whether the file opens.
                    let _ = mkdir_0700(&dir);
                    let path =
                        [base.as_slice(), b"/procps/", self.myname.as_slice(), b"rc"].concat();
                    if self.configs_path(path) == 0 {
                        system_default = true;
                    } else {
                        // Nothing there is the defaults, as upstream's NULL is.
                        fp = CFile::open(&self.rc_name).ok();
                    }
                }
            }
        }

        let complaint = match (fp, system_default) {
            (Some(mut f), false) => {
                let name = self.rc_name.clone();
                self.configs_file(&mut f, &name, &mut tmp_delay)
            }
            _ => match CFile::open(SYS_RCDEFAULTS.as_bytes()) {
                Ok(mut f) => self.configs_file(&mut f, SYS_RCDEFAULTS.as_bytes(), &mut tmp_delay),
                Err(_) => None,
            },
        };
        if let Some(p) = complaint {
            self.error_exit(&p);
        }

        if libcall::process::getuid() == 0 {
            self.secure_mode = false;
        }
        if !self.secure_mode {
            self.rc.delay_time = tmp_delay;
        }
    }

    /// `parse_args (argc, argv)`: the command line, over what the rcfile
    /// said.
    pub fn parse_args(&mut self, argv: &[Vec<u8>]) {
        const LONGS: [(&str, Takes); 18] = [
            ("batch-mode", Takes::Nothing),
            ("cmdline-toggle", Takes::Nothing),
            ("delay", Takes::Required),
            ("scale-summary-mem", Takes::Required),
            ("scale-task-mem", Takes::Required),
            ("threads-show", Takes::Nothing),
            ("help", Takes::Nothing),
            ("idle-toggle", Takes::Nothing),
            ("iterations", Takes::Required),
            ("list-fields", Takes::Nothing),
            ("sort-override", Takes::Required),
            ("pid", Takes::Required),
            ("accum-time-toggle", Takes::Nothing),
            ("secure-mode", Takes::Nothing),
            ("filter-any-user", Takes::Required),
            ("filter-only-euser", Takes::Required),
            ("version", Takes::Nothing),
            ("width", Takes::Optional),
        ];
        const LONG_ONE: (&str, Takes) = ("single-cpu-toggle", Takes::Nothing);
        let short_of = |name: &str| -> u8 {
            match name {
                "batch-mode" => b'b',
                "cmdline-toggle" => b'c',
                "delay" => b'd',
                "scale-summary-mem" => b'E',
                "scale-task-mem" => b'e',
                "threads-show" => b'H',
                "help" => b'h',
                "idle-toggle" => b'i',
                "iterations" => b'n',
                "list-fields" => b'O',
                "sort-override" => b'o',
                "pid" => b'p',
                "accum-time-toggle" => b'S',
                "secure-mode" => b's',
                "filter-any-user" => b'U',
                "filter-only-euser" => b'u',
                "version" => b'V',
                "width" => b'w',
                _ => b'1',
            }
        };
        let mut longs: Vec<(&str, Takes)> = LONGS.to_vec();
        longs.push(LONG_ONE);

        let me = argv.first().cloned().unwrap_or_default();
        let words: Vec<OsString> = argv
            .iter()
            .skip(1)
            .map(|w| quoting::os_from_bytes(w))
            .collect();
        let program = Program::new("top", 1);
        let mut parser = program.parse(&words, "bcd:E:e:Hhin:Oo:p:SsU:u:Vw::1", &longs);
        let mut tmp_delay = f32::MAX;
        let mut first_operand: Option<Vec<u8>> = None;

        while let Some(item) = parser.next() {
            let (ch, optarg) = match item {
                Ok(Opt::Short(c, v)) => (c, v.map(OsString::into_encoded_bytes)),
                Ok(Opt::Long(name, v)) => (short_of(name), v.map(OsString::into_encoded_bytes)),
                Ok(Opt::Operand(w)) => {
                    // Upstream's `getopt_long` permutes these to the end and
                    // complains of the first once it is done.
                    if first_operand.is_none() {
                        first_operand = Some(w.clone().into_encoded_bytes());
                    }
                    continue;
                }
                Err(e) => {
                    // glibc's own words, into the buffered `stderr`: out
                    // once the program ends.
                    let line = [me.as_slice(), b": ", e.sentence.as_bytes(), b"\n"].concat();
                    self.out.err(&line);
                    self.bye_bye(Some(b""));
                }
            };

            // GETOPTFIX: a value in the next word for any option, `=` before
            // one, and the word after an empty one.
            let mut cp = optarg;
            if cp.is_none() {
                let at = parser.optind();
                if let Some(next) = words.get(at)
                    && next.as_encoded_bytes().first() != Some(&b'-')
                    && let Some(w) = parser.take_word()
                {
                    cp = Some(w.into_encoded_bytes());
                }
            }
            if let Some(c) = cp.as_mut() {
                if c.first() == Some(&b'=') {
                    c.remove(0);
                }
                if c.is_empty() {
                    match parser.take_word() {
                        Some(w) => *c = w.into_encoded_bytes(),
                        None => {
                            let msg = Self::fmtmk(nls::MISSING_ARGS_FMT, &[Arg::C(ch)]);
                            self.error_exit(&msg);
                        }
                    }
                }
            }
            let arg = cp.clone().unwrap_or_default();

            match ch {
                b'1' => {
                    let w = self.cwm();
                    if w.chk(VIEW_CPUNOD) {
                        w.off(VIEW_CPUSUM);
                    } else {
                        w.tog(VIEW_CPUSUM);
                    }
                    w.off(VIEW_CPUNOD);
                    w.set(VIEW_STATES);
                }
                b'b' => self.batch = true,
                b'c' => self.cwm().tog(SHOW_CMDLIN),
                b'd' => {
                    match crate::fmt::mkfloat(&arg, false, self.radix) {
                        Some(v) => tmp_delay = v,
                        None => {
                            let msg = Self::fmtmk(nls::BAD_DELAYINT_FMT, &[Arg::S(&arg)]);
                            self.error_exit(&msg);
                        }
                    }
                    if 0.0 > tmp_delay {
                        self.error_exit(nls::DELAY_BADARG_TXT.as_bytes());
                    }
                    continue;
                }
                b'E' | b'e' => {
                    let get: &[u8] = if ch == b'E' { b"kmgtpe" } else { b"kmgtp" };
                    let c = arg.first().copied().unwrap_or(0).to_ascii_lowercase();
                    // `strchr` finds the terminator too.
                    let got = if c == 0 {
                        Some(get.len())
                    } else {
                        get.iter().position(|&g| g == c)
                    };
                    let Some(got) = got.filter(|_| arg.len() <= 1) else {
                        let msg = Self::fmtmk(nls::BAD_MEMSCALE_FMT, &[Arg::S(&arg)]);
                        self.error_exit(&msg);
                    };
                    let v = i32::try_from(got).unwrap_or(0);
                    if ch == b'E' {
                        self.rc.summ_mscale = v;
                    } else {
                        self.rc.task_mscale = v;
                    }
                    continue;
                }
                b'H' => self.thread_mode = true,
                b'h' => {
                    let text = Self::fmtmk(nls::HELP_CMDLINE_FMT, &[Arg::S(&self.myname)]);
                    self.out.raw(&text);
                    self.out.raw(b"\n");
                    self.bye_bye(None);
                }
                b'i' => {
                    let w = self.cwm();
                    w.tog(SHOW_IDLEPS);
                    w.rc.maxtasks = 0;
                }
                b'n' => {
                    match crate::fmt::mkfloat(&arg, true, self.radix) {
                        Some(v) if v >= 1.0 => {
                            // `(int) tmp`, below `INT_MAX` by `mkfloat`.
                            #[allow(clippy::cast_possible_truncation)]
                            let n = v as i32;
                            self.loops = n;
                        }
                        _ => {
                            let msg = Self::fmtmk(nls::BAD_NITERATE_FMT, &[Arg::S(&arg)]);
                            self.error_exit(&msg);
                        }
                    }
                    continue;
                }
                b'O' => {
                    for i in 0..EU_MAXPFLGS {
                        self.out.raw(n_col(i32::try_from(i).unwrap_or(0)));
                        self.out.raw(b"\n");
                    }
                    self.bye_bye(None);
                }
                b'o' => {
                    let mut c = arg.as_slice();
                    if c.first() == Some(&b'+') {
                        self.cwm().set(QSRT_NORMAL);
                        c = c.get(1..).unwrap_or_default();
                    } else if c.first() == Some(&b'-') {
                        self.cwm().off(QSRT_NORMAL);
                        c = c.get(1..).unwrap_or_default();
                    }
                    let Some(i) =
                        (0..EU_MAXPFLGS).find(|&i| n_col(i32::try_from(i).unwrap_or(0)) == c)
                    else {
                        let msg = Self::fmtmk(nls::XTRA_BADFLDS_FMT, &[Arg::S(c)]);
                        self.error_exit(&msg);
                    };
                    self.cwm().off(SHOW_FOREST);
                    self.cwm().rc.sortindx = i32::try_from(i).unwrap_or(0);
                    continue;
                }
                b'p' => {
                    if self.cw().usrseltyp != 0 {
                        self.error_exit(nls::SELECT_CLASH_TXT.as_bytes());
                    }
                    let mut c: &[u8] = &arg;
                    loop {
                        if self.monpidsidx >= MONPIDMAX as i32 {
                            let msg = Self::fmtmk(
                                nls::LIMIT_EXCEED_FMT,
                                &[Arg::I(i64::try_from(MONPIDMAX).unwrap_or(0))],
                            );
                            self.error_exit(&msg);
                        }
                        let (n, v) = sscanf(c, b"%d");
                        if n != 1 || c.iter().any(|b| b"+-.".contains(b)) {
                            let msg = Self::fmtmk(nls::BAD_MON_PIDS_FMT, &[Arg::S(c)]);
                            self.error_exit(&msg);
                        }
                        let mut pid = v.first().map_or(0, Got::int);
                        if pid == 0 {
                            pid = i32::try_from(std::process::id()).unwrap_or(0);
                        }
                        let n = usize::try_from(self.monpidsidx).unwrap_or(0);
                        if !self.monpids.iter().take(n).any(|&p| p == pid) {
                            if let Some(slot) = self.monpids.get_mut(n) {
                                *slot = pid;
                            }
                            self.monpidsidx = self.monpidsidx.saturating_add(1);
                        }
                        let Some(comma) = c.iter().position(|&b| b == b',') else {
                            break;
                        };
                        c = c.get(comma.saturating_add(1)..).unwrap_or_default();
                        if c.is_empty() {
                            break;
                        }
                    }
                    continue;
                }
                b'S' => self.cwm().tog(SHOW_CTIMES),
                b's' => self.secure_mode = true,
                b'U' | b'u' => {
                    if self.monpidsidx != 0 || self.cw().usrseltyp != 0 {
                        self.error_exit(nls::SELECT_CLASH_TXT.as_bytes());
                    }
                    let w = self.curwin;
                    if let Some(errmsg) = self.user_certify(w, &arg, ch) {
                        self.error_exit(errmsg.as_bytes());
                    }
                    continue;
                }
                b'V' => {
                    let text = Self::fmtmk(
                        nls::VERSION_OPTS_FMT,
                        &[
                            Arg::S(&self.myname),
                            Arg::S(crate::PACKAGE_STRING.as_bytes()),
                        ],
                    );
                    self.out.raw(&text);
                    self.out.raw(b"\n");
                    self.bye_bye(None);
                }
                b'w' => {
                    let mut tmp = -1.0f32;
                    if let Some(c) = cp.as_ref() {
                        match crate::fmt::mkfloat(c, true, self.radix) {
                            Some(v) if v >= W_MIN_COL as f32 && v <= SCREENMAX as f32 => tmp = v,
                            _ => {
                                let msg = Self::fmtmk(nls::BAD_WIDTHARG_FMT, &[Arg::S(c)]);
                                self.error_exit(&msg);
                            }
                        }
                    }
                    // `(int) tmp`: -1, or a width already checked.
                    #[allow(clippy::cast_possible_truncation)]
                    let width = tmp as i32;
                    self.width_mode = width;
                    continue;
                }
                _ => self.bye_bye(Some(b"")),
            }
            if let Some(c) = cp {
                let msg = Self::fmtmk(nls::UNKNOWN_OPTS_FMT, &[Arg::S(&c)]);
                self.error_exit(&msg);
            }
        }

        if let Some(w) = first_operand {
            let msg = Self::fmtmk(nls::UNKNOWN_OPTS_FMT, &[Arg::S(&w)]);
            self.error_exit(&msg);
        }

        if f32::MAX > tmp_delay {
            if self.secure_mode {
                self.error_exit(nls::DELAY_SECURE_TXT.as_bytes());
            }
            self.rc.delay_time = tmp_delay;
        }
    }

    /// `write_rcfile` (`W`): the personal rcfile, after asking whether to
    /// overwrite a damaged or older one.
    pub fn write_rcfile(&mut self) {
        if self.rc_questions {
            self.show_pmt(nls::XTRA_WARNCFG_TXT.as_bytes());
            let k = self.iokey(crate::term::IOKEY_ONCE);
            if i32::from(b'y') != tolower_key(k) {
                return;
            }
            self.rc_questions = false;
        }
        if self.rc_compatibility {
            self.show_pmt(nls::XTRA_WARNOLD_TXT.as_bytes());
            let k = self.iokey(crate::term::IOKEY_ONCE);
            if i32::from(b'y') != tolower_key(k) {
                return;
            }
            self.rc_compatibility = false;
        }

        let mut text: Vec<u8> = Vec::new();
        text.extend_from_slice(&self.myname);
        text.extend_from_slice(b"'s ");
        text.extend_from_slice(RCF_EYECATCHER);
        let d = self.rc.delay_time;
        // `(int) d` and `(int)((d - (int) d) * 1000)`, in `float`.
        #[allow(clippy::cast_possible_truncation)]
        let whole = d as i32;
        #[allow(clippy::cast_possible_truncation)]
        let fract = ((d - whole as f32) * 1000.0) as i32;
        text.extend_from_slice(
            format!(
                "Id:{}, Mode_altscr={}, Mode_irixps={}, Delay_time={}.{}, Curwin={}\n",
                char::from(RCF_VERSION_ID),
                self.rc.mode_altscr,
                self.rc.mode_irixps,
                whole,
                fract,
                self.curwin
            )
            .as_bytes(),
        );
        for w in &self.winstk {
            let n = mlen(&w.rc.fieldscur);
            text.extend_from_slice(&w.rc.winname);
            text.extend_from_slice(b"\tfieldscur=");
            for j in 0..n {
                if j != 0 && j % FLD_ROWMAX == 0 && j < n {
                    text.extend_from_slice(b"\n\t\t  ");
                }
                text.extend_from_slice(
                    format!("{:4} ", w.rc.fieldscur.get(j).copied().unwrap_or(0)).as_bytes(),
                );
            }
            text.push(b'\n');
            let r = &w.rc;
            text.extend_from_slice(
                format!(
                    "\twinflags={}, sortindx={}, maxtasks={}, graph_cpus={}, graph_mems={}, double_up={}, combine_cpus={}, core_types={}\n",
                    r.winflags, r.sortindx, r.maxtasks, r.graph_cpus, r.graph_mems, r.double_up,
                    r.combine_cpus, r.core_types
                )
                .as_bytes(),
            );
            text.extend_from_slice(
                format!(
                    "\tsummclr={}, msgsclr={}, headclr={}, taskclr={}\n",
                    r.summclr, r.msgsclr, r.headclr, r.taskclr
                )
                .as_bytes(),
            );
        }
        text.extend_from_slice(
            format!(
                "Fixed_widest={}, Summ_mscale={}, Task_mscale={}, Zero_suppress={}, Tics_scaled={}\n",
                self.rc.fixed_widest,
                self.rc.summ_mscale,
                self.rc.task_mscale,
                self.rc.zero_suppress,
                self.rc.tics_scaled
            )
            .as_bytes(),
        );
        if self.winstk.iter().any(|w| !w.osel.is_empty()) {
            text.push(b'\n');
            text.extend_from_slice(OSEL_DELIM_1);
            for (i, w) in self.winstk.iter().enumerate() {
                if !w.osel.is_empty() {
                    text.extend_from_slice(
                        format!("window #{}, osel_tot={}\n", i, w.osel_tot()).as_bytes(),
                    );
                    for o in &w.osel {
                        text.extend_from_slice(format!("\ttype={},\tfilter=", o.typ).as_bytes());
                        text.extend_from_slice(&o.raw);
                        text.push(b'\n');
                    }
                }
            }
            text.extend_from_slice(OSEL_DELIM_2);
        }
        if let Some(raw) = self.inspect.raw.as_ref()
            && raw.as_slice() != b"\n"
        {
            text.extend_from_slice(raw);
        }

        let path = quoting::os_from_bytes(&self.rc_name);
        let result = if self.rc_name.is_empty() {
            Err(std::io::Error::from_raw_os_error(2))
        } else {
            std::fs::write(&path, &text)
        };
        match result {
            Err(e) => {
                let msg = Self::fmtmk(
                    nls::FAIL_RC_OPEN_FMT,
                    &[
                        Arg::S(&self.rc_name.clone()),
                        Arg::S(&crate::strerror_of(&e)),
                    ],
                );
                self.show_msg(&msg);
            }
            Ok(()) => {
                let msg = Self::fmtmk(nls::WRITE_RCFILE_FMT, &[Arg::S(&self.rc_name.clone())]);
                self.show_msg(&msg);
            }
        }
    }
}

/// `tolower (iokey (IOKEY_ONCE))`: ASCII letters folded, everything else
/// as it was.
fn tolower_key(k: i32) -> i32 {
    if (i32::from(b'A')..=i32::from(b'Z')).contains(&k) {
        k | 0x20
    } else {
        k
    }
}

/// `mkdir (path, 0700)`, its result ignored as upstream ignores it.
fn mkdir_0700(path: &[u8]) -> std::io::Result<()> {
    let os = quoting::os_from_bytes(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(os)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(os)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn scanf_counts_conversions_and_stops_at_a_mismatch() {
        let mut f =
            CFile::from_bytes(b"Id:k, Mode_altscr=0, Mode_irixps=1, Delay_time=3.0, Curwin=0\nX");
        let (n, v) =
            f.scanf(b"Id:%c, Mode_altscr=%d, Mode_irixps=%d, Delay_time=%d.%d, Curwin=%d\n");
        assert_eq!(n, 6);
        assert_eq!(v[0], Got::Char(b'k'));
        assert_eq!(v[3], Got::Int(3));
        // The trailing blank directive took the newline; the X is next.
        assert_eq!(f.fgets(16), Some(b"X".to_vec()));
    }

    #[test]
    fn a_failed_number_leaves_the_byte_that_stopped_it() {
        let mut f = CFile::from_bytes(b"  75 81\n\twinflags=1");
        assert_eq!(f.scanf(b"%d").0, 1);
        assert_eq!(f.scanf(b"%d").0, 1);
        assert_eq!(f.scanf(b"%d").0, 0);
        let (n, v) = f.scanf(b"\twinflags=%d");
        assert_eq!((n, v[0].clone()), (1, Got::Int(1)));
        assert_eq!(f.scanf(b"%d").0, -1);
    }

    #[test]
    fn fgets_stops_at_size_or_newline() {
        let mut f = CFile::from_bytes(b"abcdef\ngh");
        assert_eq!(f.fgets(4), Some(b"abc".to_vec()));
        assert_eq!(f.fgets(16), Some(b"def\n".to_vec()));
        assert_eq!(f.fgets(16), Some(b"gh".to_vec()));
        assert_eq!(f.fgets(16), None);
    }

    #[test]
    fn a_width_bounds_a_string() {
        let (n, v) = sscanf(b"Defx\tfieldscur=", b"%3s\tfieldscur=");
        assert_eq!(n, 1);
        assert_eq!(v[0], Got::Str(b"Def".to_vec()));
    }
}
