//! `output.c`: one line of output, and every column's print function.
//!
//! # How a line is laid out
//!
//! `show_one_proc` walks the columns keeping two positions: where the line
//! *is* (`actual`) and where it *should be* (`correct`, the sum of the widths
//! so far plus one space between columns). A right-justified column is
//! padded to its width; a value too wide for its column is printed whole and
//! pushes `actual` past `correct`, and the next column takes the difference
//! back from its own padding -- down to the one space that always separates
//! two columns. Only the last column is cut to fit the screen, and only the
//! columns that may run long (`args`, `comm`, `wchan`, ...) are cut at all;
//! a number is never cut.
//!
//! The screen is `screen_cols` wide, or a multiple of it if the columns'
//! widths add up to more (`active_cols`), so a line that does not fit is not
//! cut to nothing.
//!
//! Some arithmetic here is `unsigned`, as upstream's is, and wraps: when a
//! line has already overrun the screen, the room left for the last column is
//! not zero but "very large", and that column is printed whole. Only a line
//! that ends *exactly* at the screen's edge loses its last column.

use crate::formats::{self, Pr};
use crate::items::{self, Item, Stack, Val};
use crate::{Exit, FormatNode, OUTBUF_SIZE, Ps};
use coreutils::extfloat::{self, ExtF80, Spec};

/// `COLWID`: what `snprintf` writes at most, the NUL included.
const COLWID: usize = 240;
/// `SIGNAL_NAME_WIDTH`: `--signames`' minimum signal column.
const SIGNAL_NAME_WIDTH: i32 = 27;
/// `SPACE_AMOUNT`: the most padding one column can be given.
const SPACE_AMOUNT: i32 = 144;
/// `RLIM_INFINITY`.
const RLIM_INFINITY: u64 = u64::MAX;
/// `OUTBUF_SIZE` as a buffer length.
const OUTBUF: usize = 2 * 64 * 1024;

/// What a print function produced: the bytes it wrote (perhaps with a NUL
/// in them, which ends the text) and the number it returned -- a count of
/// screen cells, which is not always the count of bytes.
struct Cell {
    buf: Vec<u8>,
    amount: i64,
}

/// `snprintf (outbuf, COLWID, …)`: at most 239 bytes written, and the full
/// length returned.
fn snp(text: impl AsRef<[u8]>) -> Cell {
    let t = text.as_ref();
    Cell {
        buf: t
            .get(..t.len().min(COLWID - 1))
            .unwrap_or_default()
            .to_vec(),
        amount: i64::try_from(t.len()).unwrap_or(i64::MAX),
    }
}

/// The ASCII "printable" of `isprint` in the C and C.UTF-8 locales.
fn isprint(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// One UTF-8 character at the start of `s`, as glibc's `mbrtowc` reads it:
/// `Some((c, len))`, or `None` for an invalid or incomplete sequence.
fn mbrtowc(s: &[u8]) -> Option<(char, usize)> {
    let first = *s.first()?;
    let len = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let text = std::str::from_utf8(s.get(..len)?).ok()?;
    text.chars().next().map(|c| (c, len))
}

/// `output.c`'s `escape_str`: `src` (a C string; `None` is NULL) made safe
/// to print into a buffer of `bufsize` bytes, using at most `*maxcells`
/// screen cells, which it reduces by what it used. Returns the bytes.
fn escape_str(src: Option<&[u8]>, bufsize: i64, maxcells: &mut i32, utf8: bool) -> Vec<u8> {
    if bufsize <= 0 || bufsize >= i64::from(i32::MAX) || *maxcells == i32::MAX || *maxcells <= 0 {
        return Vec::new();
    }
    let src = coreutils::procps::readproc::cstr(src.unwrap_or_default());
    let mut out = Vec::new();
    let mut cells: i32 = 0;
    let mut bytes: i64 = 0;
    if utf8 {
        let mut i = 0usize;
        loop {
            if cells >= *maxcells || bytes.saturating_add(1) >= bufsize {
                break;
            }
            let rest = src.get(i..).unwrap_or_default();
            if rest.is_empty() {
                break;
            }
            match mbrtowc(rest) {
                None => {
                    out.push(b'?');
                    i = i.saturating_add(1);
                    cells = cells.saturating_add(1);
                    bytes = bytes.saturating_add(1);
                }
                Some((_, 1)) => {
                    let c = rest.first().copied().unwrap_or(b'?');
                    out.push(if isprint(c) { c } else { b'?' });
                    i = i.saturating_add(1);
                    cells = cells.saturating_add(1);
                    bytes = bytes.saturating_add(1);
                }
                Some((c, len)) => match charwidth::char_width(c) {
                    None => {
                        out.push(b'?');
                        i = i.saturating_add(len);
                        cells = cells.saturating_add(1);
                        bytes = bytes.saturating_add(1);
                    }
                    Some(w) => {
                        let w = i32::try_from(w).unwrap_or(i32::MAX);
                        let l = i64::try_from(len).unwrap_or(i64::MAX);
                        if w > maxcells.saturating_sub(cells)
                            || l >= bufsize.saturating_sub(bytes.saturating_add(1))
                        {
                            break;
                        }
                        out.extend_from_slice(rest.get(..len).unwrap_or_default());
                        i = i.saturating_add(len);
                        bytes = bytes.saturating_add(l);
                        if w > 0 {
                            cells = cells.saturating_add(w);
                        }
                    }
                },
            }
        }
    } else {
        let limit = bufsize.min(i64::from(*maxcells).saturating_add(1));
        for &c in src {
            if cells >= *maxcells || bytes.saturating_add(1) >= limit {
                break;
            }
            out.push(match c {
                0x20..=0x7e => c,
                0x80..=0xff => b'?',
                _ => b'.',
            });
            cells = cells.saturating_add(1);
            bytes = bytes.saturating_add(1);
        }
    }
    *maxcells = maxcells.saturating_sub(cells);
    out
}

/// `escaped_copy`: `src` as it is, cut to `*maxroom` bytes.
fn escaped_copy(src: &[u8], bufsize: i64, maxroom: &mut i32) -> Vec<u8> {
    if bufsize <= 0 || bufsize >= i64::from(i32::MAX) || *maxroom == i32::MAX || *maxroom <= 0 {
        return Vec::new();
    }
    let bufsize = bufsize.min(i64::from(*maxroom).saturating_add(1));
    let src = coreutils::procps::readproc::cstr(src);
    let n = src
        .len()
        .min(usize::try_from(bufsize.saturating_sub(1)).unwrap_or(0));
    *maxroom = maxroom.saturating_sub(i32::try_from(n).unwrap_or(i32::MAX));
    src.get(..n).unwrap_or_default().to_vec()
}

/// `%.2d`-style zero padding to two digits.
fn d2(v: u32) -> String {
    format!("{v:02}")
}

/// `unsigned` arithmetic on `unsigned` and `int`: the low 32 bits.
fn u32_of(v: i64) -> u32 {
    let b = v.to_le_bytes();
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// C's `(int)` of an `unsigned`.
fn i32_of(v: u32) -> i32 {
    i32::from_le_bytes(v.to_le_bytes())
}

/// C's `(long)` of an `unsigned long`.
fn i64_of(v: u64) -> i64 {
    i64::from_le_bytes(v.to_le_bytes())
}

impl Ps {
    /// `init_output`: the clock, and the screen's real width.
    pub fn init_output(&mut self) {
        self.seconds_since_1970 = now();
        self.check_header_width();
    }

    /// `check_header_width`: how wide the columns are together, and how many
    /// screens' widths that takes; and whether signal masks get 16 digits.
    fn check_header_width(&mut self) {
        let mut total: u32 = 0;
        let mut was_normal: u32 = 0;
        let mut sigs: u32 = 0;
        let n = self.format_list.len();
        for idx in 0..n {
            let has_next = idx.saturating_add(1) < n;
            let signal_names = self.signal_names;
            let Some(node) = self.format_list.get_mut(idx) else {
                continue;
            };
            let w = u32::from_le_bytes(node.width.to_le_bytes());
            match node.flags & formats::JUST_MASK {
                formats::SIGNAL => {
                    sigs = sigs.wrapping_add(1);
                    if signal_names {
                        if node.width < SIGNAL_NAME_WIDTH {
                            node.width = SIGNAL_NAME_WIDTH;
                        }
                        node.flags = formats::UNLIMITED;
                        let w = u32::from_le_bytes(node.width.to_le_bytes());
                        total = total.wrapping_add(if has_next { w } else { 3 });
                    } else {
                        total = total.wrapping_add(w);
                    }
                    total = total.wrapping_add(was_normal);
                    was_normal = 1;
                }
                formats::UNLIMITED => {
                    total = total.wrapping_add(if has_next { w } else { 3 });
                    total = total.wrapping_add(was_normal);
                    was_normal = 1;
                }
                0 => {
                    total = total.wrapping_add(w);
                    was_normal = 0;
                }
                _ => {
                    total = total.wrapping_add(w);
                    total = total.wrapping_add(was_normal);
                    was_normal = 1;
                }
            }
        }
        let cols = u32::from_le_bytes(self.screen_cols.to_le_bytes());
        let half = u32::try_from(OUTBUF_SIZE / 2).unwrap_or(0);
        let mut i: u32 = 0;
        loop {
            i = i.wrapping_add(1);
            self.active_cols = cols.wrapping_mul(i);
            if self.active_cols >= total {
                break;
            }
            if cols.wrapping_mul(i) >= half {
                break;
            }
        }
        self.wide_signals = total.wrapping_add(sigs.wrapping_mul(7)) <= self.active_cols;
    }

    /// `show_one_proc`: one line -- a process's, or the header (`p` `None`).
    #[allow(clippy::too_many_lines)]
    pub fn show_one_proc(&mut self, p: Option<&Stack>, fmt: &[FormatNode]) -> Result<(), Exit> {
        if p.is_some() {
            self.lines_to_next_header = self.lines_to_next_header.wrapping_sub(1);
            if self.lines_to_next_header == 0 {
                self.lines_to_next_header = self.header_gap;
                self.show_one_proc(None, fmt)?;
            }
        }
        self.did_stuff = true;
        if self.active_cols > u32::try_from(OUTBUF_SIZE).unwrap_or(0) {
            self.eprint(b"fix bigness error\n");
        }
        let outbuf_max = u32::try_from(OUTBUF_SIZE - 1).unwrap_or(0);
        let mut correct: i32 = 0;
        let mut actual: i32 = 0;
        let mut dospace: i32 = 0;
        let mut k = 0usize;
        while let Some(node) = fmt.get(k) {
            let has_next = fmt.get(k.saturating_add(1)).is_some();
            let mut legit = 0;
            let tmpspace = if has_next {
                self.max_rightward = u32::from_le_bytes(node.width.to_le_bytes());
                0
            } else {
                let mut ts = correct.wrapping_sub(actual);
                if ts < 1 {
                    ts = dospace;
                    self.max_rightward = self
                        .active_cols
                        .wrapping_sub(u32::from_le_bytes(actual.to_le_bytes()))
                        .wrapping_sub(u32::from_le_bytes(ts.to_le_bytes()));
                } else {
                    let m = if correct > actual { correct } else { actual };
                    self.max_rightward = self
                        .active_cols
                        .wrapping_sub(u32::from_le_bytes(m.to_le_bytes()));
                }
                ts
            };
            // `max_rightward <= 0` on an `unsigned` is `== 0`, where nothing
            // needs doing; only the upper clamp acts.
            if self.max_rightward >= u32::try_from(OUTBUF_SIZE).unwrap_or(0) {
                self.max_rightward = outbuf_max;
            }
            self.max_leftward = u32::from_le_bytes(
                node.width
                    .wrapping_add(actual)
                    .wrapping_sub(correct)
                    .to_le_bytes(),
            );
            if self.max_leftward >= u32::try_from(OUTBUF_SIZE).unwrap_or(0) {
                self.max_leftward = outbuf_max;
            }

            let mut cell = match (p, node.pr) {
                (Some(stack), Some(pr)) => self.render(pr, stack)?,
                _ => Cell {
                    buf: node.name.clone(),
                    amount: i64::try_from(node.name.len()).unwrap_or(i64::MAX),
                },
            };
            if cell.amount < 0 {
                cell.amount = 0;
                cell.buf.clear();
            } else if cell.amount >= i64::from(OUTBUF_SIZE) {
                cell.amount = i64::from(OUTBUF_SIZE - 1);
                cell.buf.truncate(OUTBUF - 1);
            }
            let amount = i32::try_from(cell.amount).unwrap_or(i32::MAX);
            let room_gone = self
                .active_cols
                .wrapping_sub(u32::from_le_bytes(actual.to_le_bytes()))
                .wrapping_sub(u32::from_le_bytes(tmpspace.to_le_bytes()))
                == 0;
            let leftpad = match node.flags & formats::JUST_MASK {
                0 | formats::LEFT => 0,
                formats::RIGHT => node.width.wrapping_sub(amount).max(0),
                formats::SIGNAL => {
                    let pad = if self.wide_signals {
                        legit = 7;
                        16i32.wrapping_sub(amount)
                    } else {
                        9i32.wrapping_sub(amount)
                    };
                    pad.max(0)
                }
                formats::USER => {
                    if self.user_is_number {
                        node.width.wrapping_sub(amount).max(0)
                    } else {
                        0
                    }
                }
                formats::WCHAN => {
                    if self.wchan_is_number {
                        node.width.wrapping_sub(amount).max(0)
                    } else {
                        if room_gone {
                            cell.buf.truncate(1);
                        }
                        0
                    }
                }
                formats::UNLIMITED => {
                    if room_gone {
                        cell.buf.truncate(1);
                    }
                    0
                }
                _ => {
                    self.eprint(b"bad alignment code\n");
                    0
                }
            };
            let mut space = correct.wrapping_sub(actual).wrapping_add(leftpad);
            if space < 1 {
                space = dospace;
            }
            if space > SPACE_AMOUNT {
                space = SPACE_AMOUNT;
            }
            // `sz = strlen (outbuf)`: the text ends at its first NUL. Each
            // column is written as it is made, as upstream's `fwrite`s are,
            // so a column that fails (no boot time, no `MemTotal`) leaves the
            // ones before it on the line above its message.
            let sz = cell
                .buf
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(cell.buf.len());
            self.out.extend(std::iter::repeat_n(
                b' ',
                usize::try_from(space).unwrap_or(0),
            ));
            self.out
                .extend_from_slice(cell.buf.get(..sz).unwrap_or_default());
            if !has_next {
                self.out.push(b'\n');
                break;
            }
            actual = actual.wrapping_add(space).wrapping_add(amount);
            correct = correct.wrapping_add(node.width).wrapping_add(legit);
            let next_pr = fmt.get(k.saturating_add(1)).and_then(|n| n.pr);
            if node.pr.is_some() && next_pr.is_some() {
                correct = correct.wrapping_add(1);
                dospace = 1;
            } else {
                dospace = 0;
            }
            k = k.saturating_add(1);
        }
        if self.out.len() >= 1 << 16 {
            self.flush();
        }
        Ok(())
    }

    /// `rSv`, typed.
    fn sv(&self, item: Item, p: &Stack) -> Val {
        self.rsv(item, p).clone()
    }

    /// `forest_helper`: the tree drawing before a command name.
    fn forest_helper(&self) -> Vec<u8> {
        let mut rightward = if self.max_rightward < u32::try_from(OUTBUF_SIZE).unwrap_or(0) {
            i64::from(self.max_rightward)
        } else {
            i64::from(OUTBUF_SIZE - 1)
        };
        let prefix = coreutils::procps::readproc::cstr(&self.forest_prefix);
        let mut out = Vec::new();
        let step: i64 = if self.forest_type == b'u' { 2 } else { 4 };
        for &c in prefix {
            if rightward < step {
                break;
            }
            let piece: &[u8] = if self.forest_type == b'u' {
                b"  "
            } else {
                match c {
                    b'L' | b'+' => b" \\_ ",
                    b'|' => b" |  ",
                    _ => b"    ",
                }
            };
            out.extend_from_slice(piece);
            rightward = rightward.saturating_sub(step);
        }
        out
    }

    /// `pr_args` and `pr_comm`: a command, after the forest drawing, with the
    /// environment after it under BSD `e`.
    fn command_cell(&self, p: &Stack, long: bool) -> Cell {
        let max = self.max_rightward;
        let mut rightward = i32_of(max);
        let mut out = self.forest_helper();
        let fh = i32::try_from(out.len()).unwrap_or(0);
        rightward = rightward.wrapping_sub(fh);
        let src = if long {
            self.sv(Item::Cmdline, p)
        } else {
            self.sv(Item::Cmd, p)
        };
        let room = |out: &Vec<u8>| {
            i64::from(OUTBUF_SIZE).saturating_sub(i64::try_from(out.len()).unwrap_or(0))
        };
        let r = room(&out);
        out.extend(escape_str(src.str(), r, &mut rightward, self.utf8));
        if self.bsd_e_option && rightward > 1 {
            let e = self.sv(Item::Environ, p);
            let text = e.str().unwrap_or_default();
            if text != b"-" {
                out.push(b' ');
                rightward = rightward.wrapping_sub(1);
                let r = room(&out);
                out.extend(escape_str(Some(text), r, &mut rightward, self.utf8));
            }
        }
        Cell {
            buf: out,
            amount: i64::from(i32_of(
                max.wrapping_sub(u32::from_le_bytes(rightward.to_le_bytes())),
            )),
        }
    }

    /// An escaped string column with no forest drawing: `pr_cgname`,
    /// `pr_cgroup`, `pr_supgrp`, `pr_exe`.
    fn escaped_cell(&self, v: &Val) -> Cell {
        let max = self.max_rightward;
        let mut rightward = i32_of(max);
        let buf = escape_str(v.str(), i64::from(OUTBUF_SIZE), &mut rightward, self.utf8);
        Cell {
            buf,
            amount: i64::from(i32_of(
                max.wrapping_sub(u32::from_le_bytes(rightward.to_le_bytes())),
            )),
        }
    }

    /// `do_pr_name`: a user or group name, cut with a `+` when it does not
    /// fit, or the number under BSD `n` or when cutting would split a
    /// character.
    fn name_cell(&self, name: Option<&[u8]>, id: u32) -> Cell {
        if !self.user_is_number {
            let mut rightward = OUTBUF_SIZE;
            let mut buf = escape_str(name, i64::from(OUTBUF_SIZE), &mut rightward, self.utf8);
            let len = OUTBUF_SIZE.wrapping_sub(rightward);
            if i64::from(len) <= i64::from(self.max_rightward) {
                return Cell {
                    buf,
                    amount: i64::from(len),
                };
            }
            let mr = usize::try_from(self.max_rightward).unwrap_or(0);
            // `(unsigned) outbuf[max_rightward-1] < 127`: a signed `char`, so
            // any byte of a multi-byte character fails.
            if mr >= 1 && buf.get(mr.saturating_sub(1)).is_some_and(|&b| b < 127) {
                buf.truncate(mr.saturating_sub(1));
                buf.push(b'+');
                return Cell {
                    buf,
                    amount: i64::try_from(mr).unwrap_or(0),
                };
            }
        }
        snp(id.to_string())
    }

    /// `help_pr_sig`: a signal mask as hexadecimal -- 16 digits when the
    /// screen is wide enough, else the last 8 with `<` if anything was cut --
    /// or, under `--signames`, by name.
    fn sig_cell(&self, v: &Val) -> Cell {
        let sig = v.str().unwrap_or_default();
        if self.signal_names {
            let names = crate::signames::print_signame(sig, i64::from(self.max_rightward));
            if !names.is_empty() {
                let n = i64::try_from(names.len()).unwrap_or(0);
                return Cell {
                    buf: names,
                    amount: n,
                };
            }
        }
        let len = sig.len();
        if self.wide_signals {
            if len > 8 {
                return snp(sig);
            }
            return snp([b"00000000".as_slice(), sig].concat());
        }
        let zeros = sig.iter().take_while(|&&b| b == b'0').count();
        if len.saturating_sub(zeros) > 8 {
            return snp([
                b"<".as_slice(),
                sig.get(len.saturating_sub(8)..).unwrap_or_default(),
            ]
            .concat());
        }
        if len < 8 {
            let pad = b"00000000".get(len..).unwrap_or_default();
            return snp([pad, sig].concat());
        }
        snp(sig.get(len.saturating_sub(8)..).unwrap_or_default())
    }

    /// `boot_time ()`, or upstream's fatal "Unable to get system boot time".
    fn boot_time(&mut self) -> Result<u32, Exit> {
        match self.sys.boot_time() {
            Some(b) => Ok(b),
            None => {
                self.error(b"Unable to get system boot time");
                Err(Exit::Status(1))
            }
        }
    }

    /// When a process started, in seconds since 1970: `boot_time () +
    /// TICS_BEGAN / Hertz`, in `unsigned long long` and then `time_t`.
    fn start_time(&mut self, p: &Stack) -> Result<i64, Exit> {
        let boot = u64::from(self.boot_time()?);
        let hz = u64::try_from(self.hertz).unwrap_or(100).max(1);
        let began = self.sv(Item::TicsBegan, p).ul();
        Ok(i64_of(
            boot.wrapping_add(began.checked_div(hz).unwrap_or(0)),
        ))
    }

    /// `ctime (&t)`: `asctime` of the local time, or `None` where glibc's
    /// `localtime` fails.
    fn ctime(&self, t: i64) -> Option<Vec<u8>> {
        self.zone.localtime_r(t)?;
        let tm = self.zone.localtime(t, 0);
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let day = DAYS
            .get(usize::try_from(tm.wday).unwrap_or(7))
            .copied()
            .unwrap_or("???");
        let mon = MONTHS
            .get(usize::try_from(tm.month.saturating_sub(1)).unwrap_or(12))
            .copied()
            .unwrap_or("???");
        Some(
            format!(
                "{day} {mon}{:3} {}:{}:{} {}\n",
                tm.day,
                d2(tm.hour),
                d2(tm.minute),
                d2(tm.second),
                tm.year
            )
            .into_bytes(),
        )
    }

    /// glibc's `strftime (outbuf, COLWID, fmt, tm)`: the text, or nothing if
    /// it does not fit in 240 bytes with its NUL.
    fn strftime_cell(&self, fmt: &[u8], t: i64) -> Cell {
        if self.zone.localtime_r(t).is_none() {
            return Cell {
                buf: Vec::new(),
                amount: 0,
            };
        }
        let tm = self.zone.localtime(t, 0);
        let text = localtime::strftime(fmt, &tm);
        if text.is_empty() || text.len() >= COLWID {
            return Cell {
                buf: Vec::new(),
                amount: 0,
            };
        }
        let n = i64::try_from(text.len()).unwrap_or(0);
        Cell {
            buf: text,
            amount: n,
        }
    }

    /// The `[dd-]hh:mm:ss` family: `t` broken into days, hours, minutes and
    /// seconds as `unsigned`s.
    fn dhms(t: u64) -> (u32, u32, u32, u32) {
        let ss = u32::try_from(t % 60).unwrap_or(0);
        let t = t / 60;
        let mm = u32::try_from(t % 60).unwrap_or(0);
        let t = t / 60;
        let hh = u32::try_from(t % 24).unwrap_or(0);
        let dd = u32_of(i64_of(t / 24));
        (dd, hh, mm, ss)
    }

    /// The `%cpu`-family's CPU time and elapsed ticks.
    fn cpu_and_ticks(&self, p: &Stack) -> (u64, u64) {
        let total = if self.include_dead_children {
            self.sv(Item::TicsAllC, p).ul()
        } else {
            self.sv(Item::TicsAll, p).ul()
        };
        let hz = items::dbl(u64::try_from(self.hertz).unwrap_or(100));
        let jiffies = items::cvt_u64(self.sv(Item::TimeElapsed, p).real() * hz);
        (total, jiffies)
    }

    /// The print functions.
    #[allow(clippy::too_many_lines)]
    fn render(&mut self, pr: Pr, p: &Stack) -> Result<Cell, Exit> {
        use Item as I;
        let s_int = |ps: &Self, i: Item| ps.sv(i, p).s_int();
        let ul = |ps: &Self, i: Item| ps.sv(i, p).ul();
        Ok(match pr {
            Pr::Nop => snp("-"),
            Pr::Args => self.command_cell(p, !self.bsd_c_option),
            Pr::Comm => self.command_cell(p, self.unix_f_option),
            Pr::Cgname => self.escaped_cell(&self.sv(I::Cgname, p)),
            Pr::Cgroup => self.escaped_cell(&self.sv(I::Cgroup, p)),
            Pr::Fname => {
                let max = self.max_rightward;
                let mut rightward = i32_of(max);
                let mut out = self.forest_helper();
                rightward = rightward.wrapping_sub(i32::try_from(out.len()).unwrap_or(0));
                if rightward > 8 {
                    rightward = 8;
                }
                let room =
                    i64::from(OUTBUF_SIZE).saturating_sub(i64::try_from(out.len()).unwrap_or(0));
                out.extend(escape_str(
                    self.sv(I::Cmd, p).str(),
                    room,
                    &mut rightward,
                    self.utf8,
                ));
                Cell {
                    buf: out,
                    amount: i64::from(i32_of(
                        max.wrapping_sub(u32::from_le_bytes(rightward.to_le_bytes())),
                    )),
                }
            }
            Pr::Etime => {
                let (dd, hh, mm, ss) =
                    Self::dhms(items::cvt_u64(self.sv(I::TimeElapsed, p).real()));
                let mut s = String::new();
                if dd != 0 {
                    s.push_str(&format!("{dd}-"));
                }
                if dd != 0 || hh != 0 {
                    s.push_str(&format!("{hh:02}:"));
                }
                s.push_str(&format!("{mm:02}:{ss:02}"));
                let n = i64::try_from(s.len()).unwrap_or(0);
                Cell {
                    buf: s.into_bytes(),
                    amount: n,
                }
            }
            Pr::Etimes => snp(items::cvt_u32(self.sv(I::TimeElapsed, p).real()).to_string()),
            Pr::C => {
                let (total, jiffies) = self.cpu_and_ticks(p);
                let mut pcpu = u32_of(i64_of(
                    total.wrapping_mul(100).checked_div(jiffies).unwrap_or(0),
                ));
                if pcpu > 99 {
                    pcpu = 99;
                }
                snp(format!("{pcpu:2}"))
            }
            Pr::Pcpu => {
                let (total, jiffies) = self.cpu_and_ticks(p);
                let pcpu = u32_of(i64_of(
                    total.wrapping_mul(1000).checked_div(jiffies).unwrap_or(0),
                ));
                if pcpu > 999 {
                    snp(format!("{}", pcpu / 10))
                } else {
                    snp(format!("{}.{}", pcpu / 10, pcpu % 10))
                }
            }
            Pr::Cp => {
                let (total, jiffies) = self.cpu_and_ticks(p);
                let mut pcpu = u32_of(i64_of(
                    total.wrapping_mul(1000).checked_div(jiffies).unwrap_or(0),
                ));
                if pcpu > 999 {
                    pcpu = 999;
                }
                snp(format!("{pcpu:3}"))
            }
            Pr::Pgid => snp(u32_of(i64::from(s_int(self, I::IdPgrp))).to_string()),
            Pr::Ppid => snp(u32_of(i64::from(s_int(self, I::IdPpid))).to_string()),
            Pr::Time => {
                let (dd, hh, mm, ss) = Self::dhms(items::cvt_u64(self.sv(I::TimeAll, p).real()));
                let mut s = String::new();
                if dd != 0 {
                    s.push_str(&format!("{dd}-"));
                }
                s.push_str(&format!("{hh:02}:{mm:02}:{ss:02}"));
                let n = i64::try_from(s.len()).unwrap_or(0);
                Cell {
                    buf: s.into_bytes(),
                    amount: n,
                }
            }
            Pr::Times => snp(items::cvt_u64(self.sv(I::TimeAll, p).real()).to_string()),
            Pr::Vsz => snp(ul(self, I::VmSize).to_string()),
            Pr::Priority => snp(s_int(self, I::Priority).to_string()),
            Pr::Opri => snp(60i32.wrapping_add(s_int(self, I::Priority)).to_string()),
            Pr::PriFoo => snp(s_int(self, I::Priority).wrapping_sub(20).to_string()),
            Pr::PriBar => snp(s_int(self, I::Priority).wrapping_add(1).to_string()),
            Pr::PriBaz => snp(s_int(self, I::Priority).wrapping_add(100).to_string()),
            Pr::Pri => snp(39i32.wrapping_sub(s_int(self, I::Priority)).to_string()),
            Pr::PriApi => snp((-1i32).wrapping_sub(s_int(self, I::Priority)).to_string()),
            Pr::Nice => {
                let class = s_int(self, I::SchedClass);
                if class != 0 && class != 3 && class != -1 {
                    snp("-")
                } else {
                    snp(s_int(self, I::Nice).to_string())
                }
            }
            Pr::OomAdj => snp(s_int(self, I::OomAdj).to_string()),
            Pr::Oom => snp(s_int(self, I::OomScore).to_string()),
            Pr::Class => snp(match s_int(self, I::SchedClass) {
                -1 => "-",
                0 => "TS",
                1 => "FF",
                2 => "RR",
                3 => "B",
                4 => "ISO",
                5 => "IDL",
                6 => "DLN",
                7 => "#7",
                8 => "#8",
                9 => "#9",
                _ => "?",
            }),
            Pr::Rtprio => {
                let class = s_int(self, I::SchedClass);
                if class == 0 || class == -1 {
                    snp("-")
                } else {
                    snp(s_int(self, I::PriorityRt).to_string())
                }
            }
            Pr::Sched => {
                let class = s_int(self, I::SchedClass);
                if class == -1 {
                    snp("-")
                } else {
                    snp(class.to_string())
                }
            }
            Pr::Wchan => {
                let v = self.sv(I::WchanName, p);
                let w = coreutils::procps::readproc::cstr(v.str().unwrap_or_default());
                let len = w
                    .len()
                    .min(usize::try_from(self.max_rightward).unwrap_or(0));
                Cell {
                    buf: w.get(..len).unwrap_or_default().to_vec(),
                    amount: i64::try_from(len).unwrap_or(0),
                }
            }
            Pr::Tty4 => snp(self.sv(I::TtyNumber, p).str().unwrap_or(b"(null)")),
            Pr::Tty8 => snp(self.sv(I::TtyName, p).str().unwrap_or(b"(null)")),
            Pr::Stat => {
                let mut out = vec![self.sv(I::State, p).s_ch()];
                let nice = s_int(self, I::Nice);
                if nice < 0 {
                    out.push(b'<');
                }
                if nice > 0 {
                    out.push(b'N');
                }
                if ul(self, I::VmRssLocked) != 0 {
                    out.push(b'L');
                }
                if s_int(self, I::IdSession) == s_int(self, I::IdTgid) {
                    out.push(b's');
                }
                if s_int(self, I::Nlwp) > 1 {
                    out.push(b'l');
                }
                if s_int(self, I::IdPgrp) == s_int(self, I::IdTpgid) {
                    out.push(b'+');
                }
                let n = i64::try_from(out.len()).unwrap_or(0);
                Cell {
                    buf: out,
                    amount: n,
                }
            }
            Pr::S => Cell {
                buf: vec![self.sv(I::State, p).s_ch()],
                amount: 1,
            },
            Pr::Flag => snp(format!("{:o}", u32_of(i64_of(ul(self, I::Flags) >> 6)) & 7)),
            Pr::Stackp => snp(format!("{:016x}", ul(self, I::AddrStackStart))),
            Pr::Esp => snp(format!("{:016x}", ul(self, I::AddrCurrEsp))),
            Pr::Eip => snp(format!("{:016x}", ul(self, I::AddrCurrEip))),
            Pr::Bsdtime => {
                let t = if self.include_dead_children {
                    ul(self, I::TicsAllC)
                } else {
                    ul(self, I::TicsAll)
                };
                let hz = u64::try_from(self.hertz).unwrap_or(100).max(1);
                let u = u32_of(i64_of(t.checked_div(hz).unwrap_or(0)));
                snp(format!("{:3}:{:02}", u / 60, u % 60))
            }
            Pr::Bsdstart => {
                let start = self.start_time(p)?;
                let ago = self.seconds_since_1970.wrapping_sub(start).max(0);
                let text = self.ctime(start).unwrap_or_default();
                let from = if ago > 3600 * 24 { 4 } else { 10 };
                let mut buf = text.get(from..).unwrap_or_default().to_vec();
                buf.truncate(COLWID - 1);
                buf.resize(buf.len().max(6), 0);
                buf.truncate(6);
                Cell { buf, amount: 6 }
            }
            Pr::Sz => {
                let div = u64::try_from(self.page_size / 1024).unwrap_or(1).max(1);
                snp(ul(self, I::VmSize)
                    .checked_div(div)
                    .unwrap_or(0)
                    .to_string())
            }
            Pr::Dsiz | Pr::Drs => {
                let v = ul(self, I::VsizeBytes);
                let n: i64 = if v != 0 {
                    i64_of(
                        v.wrapping_sub(ul(self, I::AddrCodeEnd))
                            .wrapping_add(ul(self, I::AddrCodeStart))
                            >> 10,
                    )
                } else {
                    0
                };
                snp(n.to_string())
            }
            Pr::Tsiz | Pr::Trs => {
                let v = ul(self, I::VsizeBytes);
                let n: i64 = if v != 0 {
                    i64_of(ul(self, I::AddrCodeEnd).wrapping_sub(ul(self, I::AddrCodeStart)) >> 10)
                } else {
                    0
                };
                snp(n.to_string())
            }
            Pr::Swapable => snp(ul(self, I::VmData)
                .wrapping_add(ul(self, I::VmStack))
                .to_string()),
            Pr::Size => snp(ul(self, I::VsizeBytes).to_string()),
            Pr::Minflt => {
                let f = if self.include_dead_children {
                    ul(self, I::FltMinC)
                } else {
                    ul(self, I::FltMin)
                };
                snp(f.to_string())
            }
            Pr::Majflt => {
                let f = if self.include_dead_children {
                    ul(self, I::FltMajC)
                } else {
                    ul(self, I::FltMaj)
                };
                snp(f.to_string())
            }
            Pr::Lim => {
                let v = ul(self, I::RssRlim);
                if v == RLIM_INFINITY {
                    Cell {
                        buf: b"xx".to_vec(),
                        amount: 2,
                    }
                } else {
                    snp(format!("{:5}", v >> 10))
                }
            }
            Pr::Psr => snp(s_int(self, I::Processor).to_string()),
            Pr::Pss => snp(ul(self, I::SmapPss).to_string()),
            Pr::Numa => snp(s_int(self, I::ProcessorNode).to_string()),
            Pr::Rss => snp(ul(self, I::VmRss).to_string()),
            Pr::Pmem => {
                let total = match self.sys.memory_total() {
                    Some(t) => t,
                    None => {
                        self.error(b"Unable to get total memory");
                        return Err(Exit::Status(1));
                    }
                };
                // A `MemTotal` of 0 divides by zero upstream (SIGFPE); here
                // the share of nothing is nothing.
                let mut pmem = ul(self, I::VmRss)
                    .wrapping_mul(1000)
                    .checked_div(total)
                    .unwrap_or(0);
                if pmem > 999 {
                    pmem = 999;
                }
                snp(format!(
                    "{:2}.{}",
                    u32_of(i64_of(pmem / 10)),
                    u32_of(i64_of(pmem % 10))
                ))
            }
            Pr::Lstart => {
                let t = self.start_time(p)?;
                let fmt = self
                    .lstart_format
                    .clone()
                    .unwrap_or_else(|| b"%a %b %e %H:%M:%S %Y".to_vec());
                self.strftime_cell(&fmt, t)
            }
            Pr::Stime => {
                let now = self.seconds_since_1970;
                if self.zone.localtime_r(now).is_none() {
                    return Ok(Cell {
                        buf: Vec::new(),
                        amount: 0,
                    });
                }
                let our = self.zone.localtime(now, 0);
                let t = self.start_time(p)?;
                if self.zone.localtime_r(t).is_none() {
                    return Ok(Cell {
                        buf: Vec::new(),
                        amount: 0,
                    });
                }
                let proc_tm = self.zone.localtime(t, 0);
                let mut fmt: &[u8] = b"%H:%M";
                if our.yday != proc_tm.yday {
                    fmt = b"%b%d";
                }
                if our.year != proc_tm.year {
                    fmt = b"%Y";
                }
                self.strftime_cell(fmt, t)
            }
            Pr::Start => {
                let t = self.start_time(p)?;
                let mut text = self.ctime(t).unwrap_or_default();
                if text.get(8) == Some(&b' ') {
                    if let Some(c) = text.get_mut(8) {
                        *c = b'0';
                    }
                }
                if text.get(11) == Some(&b' ') {
                    if let Some(c) = text.get_mut(11) {
                        *c = b'0';
                    }
                }
                let recent = u64::from_le_bytes(t.to_le_bytes()).wrapping_add(60 * 60 * 24)
                    > u64::from_le_bytes(self.seconds_since_1970.to_le_bytes());
                if recent {
                    let part = text.get(11..).unwrap_or_default();
                    let part = part.get(..part.len().min(8)).unwrap_or_default();
                    snp(format!("{:>8}", String::from_utf8_lossy(part)))
                } else {
                    let part = text.get(4..).unwrap_or_default();
                    let part = part.get(..part.len().min(6)).unwrap_or_default();
                    snp(format!("  {:>6}", String::from_utf8_lossy(part)))
                }
            }
            Pr::Tsig => self.sig_cell(&self.sv(I::Sigpending, p)),
            Pr::Sig => self.sig_cell(&self.sv(I::Signals, p)),
            Pr::Sigmask => self.sig_cell(&self.sv(I::Sigblocked, p)),
            Pr::Sigignore => self.sig_cell(&self.sv(I::Sigignore, p)),
            Pr::Sigcatch => self.sig_cell(&self.sv(I::Sigcatch, p)),
            Pr::Uss => snp(ul(self, I::SmapPrvTotal).to_string()),
            Pr::Egid => snp(i32_of(self.sv(I::IdEgid, p).u_int()).to_string()),
            Pr::Rgid => snp(i32_of(self.sv(I::IdRgid, p).u_int()).to_string()),
            Pr::Sgid => snp(i32_of(self.sv(I::IdSgid, p).u_int()).to_string()),
            Pr::Fgid => snp(i32_of(self.sv(I::IdFgid, p).u_int()).to_string()),
            Pr::Euid => snp(i32_of(self.sv(I::IdEuid, p).u_int()).to_string()),
            Pr::Ruid => snp(i32_of(self.sv(I::IdRuid, p).u_int()).to_string()),
            Pr::Suid => snp(i32_of(self.sv(I::IdSuid, p).u_int()).to_string()),
            Pr::Fuid => snp(i32_of(self.sv(I::IdFuid, p).u_int()).to_string()),
            Pr::Luid => {
                let v = s_int(self, I::IdLogin);
                if v == -1 {
                    snp("-")
                } else {
                    snp(v.to_string())
                }
            }
            Pr::Ruser => {
                self.name_cell(self.sv(I::IdRuser, p).str(), self.sv(I::IdRuid, p).u_int())
            }
            Pr::Euser => {
                self.name_cell(self.sv(I::IdEuser, p).str(), self.sv(I::IdEuid, p).u_int())
            }
            Pr::Fuser => {
                self.name_cell(self.sv(I::IdFuser, p).str(), self.sv(I::IdFuid, p).u_int())
            }
            Pr::Suser => {
                self.name_cell(self.sv(I::IdSuser, p).str(), self.sv(I::IdSuid, p).u_int())
            }
            Pr::Egroup => {
                self.name_cell(self.sv(I::IdEgroup, p).str(), self.sv(I::IdEgid, p).u_int())
            }
            Pr::Rgroup => {
                self.name_cell(self.sv(I::IdRgroup, p).str(), self.sv(I::IdRgid, p).u_int())
            }
            Pr::Fgroup => {
                self.name_cell(self.sv(I::IdFgroup, p).str(), self.sv(I::IdFgid, p).u_int())
            }
            Pr::Sgroup => {
                self.name_cell(self.sv(I::IdSgroup, p).str(), self.sv(I::IdSgid, p).u_int())
            }
            Pr::Rbytes => snp(ul(self, I::IoReadBytes).to_string()),
            Pr::Rchars => snp(ul(self, I::IoReadChars).to_string()),
            Pr::Rops => snp(ul(self, I::IoReadOps).to_string()),
            Pr::Wbytes => snp(ul(self, I::IoWriteBytes).to_string()),
            Pr::Wcbytes => snp(ul(self, I::IoWriteCbytes).to_string()),
            Pr::Wchars => snp(ul(self, I::IoWriteChars).to_string()),
            Pr::Wops => snp(ul(self, I::IoWriteOps).to_string()),
            Pr::Procs => snp(s_int(self, I::IdTgid).to_string()),
            Pr::Tasks => snp(s_int(self, I::IdPid).to_string()),
            Pr::Nlwp => snp(s_int(self, I::Nlwp).to_string()),
            Pr::Sess => snp(s_int(self, I::IdSession).to_string()),
            Pr::Supgid => {
                let max = self.max_rightward;
                let mut rightward = i32_of(max);
                let v = self.sv(I::Supgids, p);
                let buf = escaped_copy(
                    v.str().unwrap_or_default(),
                    i64::from(OUTBUF_SIZE),
                    &mut rightward,
                );
                Cell {
                    buf,
                    amount: i64::from(i32_of(
                        max.wrapping_sub(u32::from_le_bytes(rightward.to_le_bytes())),
                    )),
                }
            }
            Pr::Supgrp => self.escaped_cell(&self.sv(I::Supgroups, p)),
            Pr::Tpgid => snp(s_int(self, I::IdTpgid).to_string()),
            Pr::SgiP => {
                if self.sv(I::State, p).s_ch() == b'R' {
                    snp(u32::from_le_bytes(s_int(self, I::Processor).to_le_bytes()).to_string())
                } else {
                    snp("*")
                }
            }
            Pr::Exe => self.escaped_cell(&self.sv(I::Exe, p)),
            Pr::Utilization | Pr::UtilizationC => {
                let item = if pr == Pr::Utilization {
                    I::Utilization
                } else {
                    I::UtilizationC
                };
                let mut cu = self.sv(item, p).real();
                if cu > 99.0 {
                    cu = 99.999;
                }
                let mut spec = Spec::fixed(3);
                spec.hash = true;
                snp(extfloat::render(&spec, ExtF80::from_f64(cu)))
            }
            Pr::SdUnit => snp(self.sv(I::SdUnit, p).str().unwrap_or(b"(null)")),
            Pr::SdSession => snp(self.sv(I::SdSess, p).str().unwrap_or(b"(null)")),
            Pr::SdOuid => snp(self.sv(I::SdOuid, p).str().unwrap_or(b"(null)")),
            Pr::SdMachine => snp(self.sv(I::SdMach, p).str().unwrap_or(b"(null)")),
            Pr::SdUunit => snp(self.sv(I::SdUunit, p).str().unwrap_or(b"(null)")),
            Pr::SdSeat => snp(self.sv(I::SdSeat, p).str().unwrap_or(b"(null)")),
            Pr::SdSlice => snp(self.sv(I::SdSlice, p).str().unwrap_or(b"(null)")),
            Pr::Cgroupns
            | Pr::Ipcns
            | Pr::Mntns
            | Pr::Netns
            | Pr::Pidns
            | Pr::Timens
            | Pr::Userns
            | Pr::Utsns => {
                let item = match pr {
                    Pr::Cgroupns => I::NsCgroup,
                    Pr::Ipcns => I::NsIpc,
                    Pr::Mntns => I::NsMnt,
                    Pr::Netns => I::NsNet,
                    Pr::Pidns => I::NsPid,
                    Pr::Timens => I::NsTime,
                    Pr::Userns => I::NsUser,
                    _ => I::NsUts,
                };
                let v = ul(self, item);
                if v != 0 { snp(v.to_string()) } else { snp("-") }
            }
            Pr::Lxcname => snp(self.sv(I::Lxcname, p).str().unwrap_or(b"(null)")),
            Pr::Context => self.context_cell(s_int(self, I::IdTgid)),
            Pr::Agid => snp(s_int(self, I::AutogrpId).to_string()),
            Pr::Agnice => snp(s_int(self, I::AutogrpNice).to_string()),
            Pr::TUnlimited | Pr::TUnlimited2 => {
                let vals: &[&[u8]] = if pr == Pr::TUnlimited {
                    &[b"[123456789-12345] <defunct>", b"ps", b"123456789-123456"]
                } else {
                    &[
                        b"unlimited",
                        b"[123456789-12345] <defunct>",
                        b"ps",
                        b"123456789-123456",
                    ]
                };
                let v = test_val(vals, self.lines_to_next_header);
                let cap = usize::try_from(self.max_rightward).unwrap_or(0);
                let buf = v.get(..v.len().min(cap)).unwrap_or_default().to_vec();
                let n = i64::try_from(buf.len()).unwrap_or(0);
                Cell { buf, amount: n }
            }
            Pr::TRight => snp(test_val(
                &[b"999-23:59:59", b"99-23:59:59", b"9-23:59:59", b"59:59"],
                self.lines_to_next_header,
            )),
            Pr::TRight2 => snp(test_val(
                &[b"999-23:59:59", b"99-23:59:59", b"9-23:59:59"],
                self.lines_to_next_header,
            )),
            Pr::TLeft => snp(test_val(
                &[
                    b"tty7",
                    b"pts/9999",
                    b"iseries/vtty42",
                    b"ttySMX0",
                    b"3270/tty4",
                ],
                self.lines_to_next_header,
            )),
            Pr::TLeft2 => snp(test_val(
                &[b"tty7", b"pts/9999", b"ttySMX0", b"3270/tty4"],
                self.lines_to_next_header,
            )),
        })
    }

    /// `pr_context`: the SELinux label, read from `/proc/<pid>/attr/current`
    /// (there is no libselinux to ask): its printable prefix, or `-`.
    fn context_cell(&self, tgid: i32) -> Cell {
        use std::io::Read;
        let path = self.root.join(format!("{tgid}/attr/current"));
        if let Ok(mut f) = std::fs::File::open(path) {
            let mut buf = vec![0u8; OUTBUF - 1];
            if let Ok(n) = f.read(&mut buf)
                && n > 0
            {
                buf.truncate(n);
                let len = buf.iter().take_while(|&&b| isprint(b)).count();
                if len > 0 {
                    buf.truncate(len);
                    return Cell {
                        buf,
                        amount: i64::try_from(len).unwrap_or(0),
                    };
                }
            }
        }
        Cell {
            buf: b"-".to_vec(),
            amount: 1,
        }
    }
}

/// The test columns' `vals[lines_to_next_header % N]`, with C's unsigned
/// remainder.
fn test_val<'a>(vals: &[&'a [u8]], lines: i32) -> &'a [u8] {
    let n = u32::try_from(vals.len()).unwrap_or(1).max(1);
    let k = u32::from_le_bytes(lines.to_le_bytes())
        .checked_rem(n)
        .unwrap_or(0);
    vals.get(usize::try_from(k).unwrap_or(0))
        .copied()
        .unwrap_or_default()
}

/// `time (NULL)`.
fn now() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => i64::try_from(e.duration().as_secs())
            .ok()
            .and_then(i64::checked_neg)
            .unwrap_or(i64::MIN),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn utf8_escaping_counts_cells_and_replaces_controls() {
        let mut cells = 10;
        assert_eq!(escape_str(Some(b"ab\x01c"), 100, &mut cells, true), b"ab?c");
        assert_eq!(cells, 6);
        // A wide character takes two cells and must fit whole.
        let mut cells = 3;
        assert_eq!(
            escape_str(Some("a\u{4e00}\u{4e00}".as_bytes()), 100, &mut cells, true),
            "a\u{4e00}".as_bytes()
        );
        assert_eq!(cells, 0);
        // Invalid bytes are one `?` each.
        let mut cells = 10;
        assert_eq!(escape_str(Some(b"\xff\xfe"), 100, &mut cells, true), b"??");
        // NULL is nothing.
        let mut cells = 10;
        assert_eq!(escape_str(None, 100, &mut cells, true), b"");
        // No room at all.
        let mut cells = 0;
        assert_eq!(escape_str(Some(b"abc"), 100, &mut cells, true), b"");
    }

    #[test]
    fn eight_bit_escaping_uses_the_code_table() {
        let mut cells = 10;
        assert_eq!(
            escape_str(Some(b"a\x01\x7f\xe9"), 100, &mut cells, false),
            b"a..?"
        );
        let mut cells = 2;
        assert_eq!(escape_str(Some(b"abcdef"), 100, &mut cells, false), b"ab");
    }

    #[test]
    fn copy_is_cut_to_the_room_left() {
        let mut room = 3;
        assert_eq!(escaped_copy(b"10,20,30", 100, &mut room), b"10,");
        assert_eq!(room, 0);
    }

    #[test]
    fn snprintf_is_cut_but_counts_everything() {
        let c = snp(vec![b'x'; 300]);
        assert_eq!(c.buf.len(), 239);
        assert_eq!(c.amount, 300);
    }
}
