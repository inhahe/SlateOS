//! `parser.c`: the command line, in three syntaxes and two passes.
//!
//! Each argument is classified by its first bytes (`arg_type`): a dash and a
//! letter is Unix (SysV) options, a bare letter is BSD options, two dashes
//! and a letter is a GNU long option, and a number (or `-N`, `+N`) begins a
//! list of PIDs (process groups, sessions) running to the end of the line.
//!
//! When anything in the first pass fails -- an option, the format, the sort,
//! the selection -- everything is reset and the line is read again with
//! `force_bsd`, which reads every Unix-looking argument as BSD and fails on
//! a bare BSD one ("way bad") unless the personality is BSD. If that fails
//! too, the first pass's error is printed, with the short usage, and `ps`
//! exits 1.

use crate::{Exit, Ps, SelVal, SelectionNode, ff, fm, per, sel, ss, tf};
use coreutils::procps::scanf;

/// `arg_type`'s classes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arg {
    Gnu,
    End,
    Pgrp,
    Sysv,
    Pid,
    Bsd,
    Fail,
    Sess,
}

/// A base-0 number as `strtoul` and `strtol` scan it: its magnitude, sign,
/// whether the magnitude overflowed, and how many bytes it took (0 for no
/// number). Base 0 is `0x` hex, a leading `0` octal, otherwise decimal; `0x`
/// with no hex digit after it reads as the `0`.
fn scan_base0(s: &[u8]) -> (u64, bool, bool, usize) {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let mut i = s.iter().take_while(|&&b| scanf::is_space(b)).count();
    let neg = at(i) == b'-';
    if matches!(at(i), b'-' | b'+') {
        i = i.saturating_add(1);
    }
    let (base, start): (u64, usize) = if at(i) == b'0'
        && matches!(at(i.saturating_add(1)), b'x' | b'X')
        && at(i.saturating_add(2)).is_ascii_hexdigit()
    {
        (16, i.saturating_add(2))
    } else if at(i) == b'0' {
        (8, i)
    } else {
        (10, i)
    };
    let digit = |c: u8| -> Option<u64> {
        let d = match c {
            b'0'..=b'9' => c.wrapping_sub(b'0'),
            b'a'..=b'f' => c.wrapping_sub(b'a').wrapping_add(10),
            b'A'..=b'F' => c.wrapping_sub(b'A').wrapping_add(10),
            _ => return None,
        };
        (u64::from(d) < base).then_some(u64::from(d))
    };
    let mut j = start;
    let mut v: u64 = 0;
    let mut overflow = false;
    while let Some(d) = digit(at(j)) {
        match v.checked_mul(base).and_then(|x| x.checked_add(d)) {
            Some(x) => v = x,
            None => overflow = true,
        }
        j = j.saturating_add(1);
    }
    if j == start {
        return (0, false, false, 0);
    }
    (v, neg, overflow, j)
}

/// `strtoul (s, &end, 0)`: the value and how many bytes it took. A minus
/// sign negates in `unsigned long`; a magnitude past `ULONG_MAX` saturates.
#[must_use]
pub fn strtoul_base0(s: &[u8]) -> (u64, usize) {
    let (mag, neg, overflow, used) = scan_base0(s);
    let v = if overflow {
        u64::MAX
    } else if neg {
        mag.wrapping_neg()
    } else {
        mag
    };
    (v, used)
}

/// `strtol (s, &end, 0)`, for `--cols` and `COLUMNS`: saturating at
/// `LONG_MIN`/`LONG_MAX`.
#[must_use]
pub fn strtol_base0(s: &[u8]) -> (i64, usize) {
    let (mag, neg, overflow, used) = scan_base0(s);
    let v = if neg {
        if overflow || mag > i64::MIN.unsigned_abs() {
            i64::MIN
        } else {
            0i64.wrapping_sub_unsigned(mag)
        }
    } else if overflow || mag > i64::MAX.unsigned_abs() {
        i64::MAX
    } else {
        i64::try_from(mag).unwrap_or(i64::MAX)
    };
    (v, used)
}

/// `arg_type`.
fn arg_type(s: &[u8]) -> Arg {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let c = at(0);
    if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
        return Arg::Bsd;
    }
    if c.is_ascii_digit() {
        return Arg::Pid;
    }
    if c == b'+' {
        return Arg::Sess;
    }
    if c != b'-' {
        return Arg::Fail;
    }
    let c = at(1);
    if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
        return Arg::Sysv;
    }
    if c.is_ascii_digit() {
        return Arg::Pgrp;
    }
    if c != b'-' {
        return Arg::Fail;
    }
    let c = at(2);
    if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
        return Arg::Gnu;
    }
    if c == 0 {
        return Arg::End;
    }
    Arg::Fail
}

/// What a list item parser returns: the value, or the error text.
type ItemResult = Result<SelVal, &'static str>;

impl Ps {
    /// The argument being parsed, `ps_argv[thisarg]`.
    fn arg(&self, i: usize) -> Vec<u8> {
        self.args.get(i).cloned().unwrap_or_default()
    }

    /// `get_opt_arg`: the rest of this argument after the option letter at
    /// `flag`, or the next argument if this one ends there and the next is
    /// not empty.
    fn get_opt_arg(&mut self, cur: &[u8], flag: usize) -> Option<Vec<u8>> {
        let rest = cur.get(flag.saturating_add(1)..).unwrap_or_default();
        if !rest.is_empty() {
            return Some(rest.to_vec());
        }
        if self.thisarg.saturating_add(2) > self.args.len() {
            return None;
        }
        let next = self.arg(self.thisarg.saturating_add(1));
        if next.is_empty() {
            return None;
        }
        self.thisarg = self.thisarg.saturating_add(1);
        Some(next)
    }

    /// `parse_pid`.
    fn parse_pid(s: &[u8]) -> ItemResult {
        let (num, used) = strtoul_base0(s);
        if used != s.len() {
            return Err("process ID list syntax error");
        }
        if num < 1 || num > 0x7fff_ffff {
            return Err("process ID out of range");
        }
        Ok(SelVal {
            num,
            cmd: Vec::new(),
        })
    }

    /// `parse_uid`.
    fn parse_uid(&mut self, s: &[u8]) -> ItemResult {
        let (mut num, used) = strtoul_base0(s);
        if used != s.len() {
            match self.pw().uid_by_name(s) {
                Some(uid) => num = u64::from(uid),
                None => {
                    if !self.negate_selection {
                        return Err("user name does not exist");
                    }
                    num = u64::MAX;
                }
            }
        }
        if !self.negate_selection && num > 0xffff_fffe {
            return Err("user ID out of range");
        }
        Ok(SelVal {
            num: num & 0xffff_ffff,
            cmd: Vec::new(),
        })
    }

    /// `parse_gid`.
    fn parse_gid(&mut self, s: &[u8]) -> ItemResult {
        let (mut num, used) = strtoul_base0(s);
        if used != s.len() {
            match self.pw().gid_by_name(s) {
                Some(gid) => num = u64::from(gid),
                None => {
                    if !self.negate_selection {
                        return Err("group name does not exist");
                    }
                    num = u64::MAX;
                }
            }
        }
        if !self.negate_selection && num > 0xffff_fffe {
            return Err("group ID out of range");
        }
        Ok(SelVal {
            num: num & 0xffff_ffff,
            cmd: Vec::new(),
        })
    }

    /// `parse_cmd`: the first 63 bytes. It cannot fail.
    fn parse_cmd(s: &[u8]) -> SelVal {
        SelVal {
            num: 0,
            cmd: s.get(..s.len().min(63)).unwrap_or_default().to_vec(),
        }
    }

    /// `parse_tty`: a terminal by path or by short name.
    fn parse_tty(s: &[u8]) -> ItemResult {
        let found = if s.first() == Some(&b'/') {
            match char_dev(s) {
                Some(v) => v,
                None => return Err("TTY could not be found"),
            }
        } else {
            let tries: [Vec<u8>; 5] = [
                [b"/dev/pts/".as_slice(), s].concat(),
                [b"/dev/".as_slice(), s].concat(),
                [b"/dev/tty".as_slice(), s].concat(),
                [b"/dev/pty".as_slice(), s].concat(),
                [b"/dev/".as_slice(), s, b"nsole"].concat(),
            ];
            match tries.iter().find_map(|p| char_dev(p)) {
                Some(v) => v,
                None => {
                    if s == b"-" || s == b"?" {
                        return Ok(SelVal::default());
                    }
                    if s.len() == 1 && char_dev(s).is_some() {
                        return Ok(SelVal::default());
                    }
                    return Err("TTY could not be found");
                }
            }
        };
        match found {
            (true, rdev) => Ok(SelVal {
                num: rdev,
                cmd: Vec::new(),
            }),
            (false, _) => Err("list member was not a TTY"),
        }
    }

    /// `parse_list`: a comma-, space- or tab-separated list, every item
    /// through `kind`, pushed onto the front of the selection list (whose
    /// type the caller then sets).
    fn parse_list(&mut self, arg: &[u8], kind: i32) -> Result<(), &'static str> {
        let mut need_item = true;
        let mut items = 0usize;
        // The terminating NUL is never examined: the loop stops at it.
        for &c in arg {
            match c {
                b' ' | b',' | b'\t' => {
                    if need_item {
                        return Err("improper list");
                    }
                    need_item = true;
                }
                _ => {
                    if need_item {
                        items = items.saturating_add(1);
                    }
                    need_item = false;
                }
            }
        }
        if arg.is_empty() || need_item {
            return Err("improper list");
        }
        let mut u = Vec::with_capacity(items);
        let mut walk = arg;
        for _ in 0..items {
            let (item, rest) = match walk.iter().position(|&b| matches!(b, b' ' | b',' | b'\t')) {
                Some(at) => (
                    walk.get(..at).unwrap_or_default(),
                    walk.get(at.saturating_add(1)..).unwrap_or_default(),
                ),
                None => (walk, &[][..]),
            };
            let v = match kind {
                sel::PID | sel::PID_QUICK | sel::SESS | sel::PPID | sel::PGRP => {
                    Self::parse_pid(item)?
                }
                sel::EUID | sel::RUID => self.parse_uid(item)?,
                sel::EGID | sel::RGID => self.parse_gid(item)?,
                sel::TTY => Self::parse_tty(item)?,
                _ => Self::parse_cmd(item),
            };
            u.push(v);
            walk = rest;
        }
        self.selection_list
            .insert(0, SelectionNode { typecode: kind, u });
        Ok(())
    }

    /// A list option: parse `arg` into a list of type `kind`.
    fn list_option(
        &mut self,
        arg: Option<Vec<u8>>,
        kind: i32,
        missing: &'static str,
    ) -> Result<(), &'static str> {
        let arg = arg.ok_or(missing)?;
        self.parse_list(&arg, kind)
    }

    /// `exclusive`: the option must be the only argument, exactly as spelled.
    fn exclusive(&self, x: &[u8], msg: &'static str) -> Result<(), &'static str> {
        if self.args.len() != 2 || self.args.get(1).map(Vec::as_slice) != Some(x) {
            return Err(msg);
        }
        Ok(())
    }

    /// Our tty on a one-element list: BSD `T`, and `t` with no list after it.
    fn push_our_tty(&mut self) {
        let tty = u64::from(u32::from_le_bytes(self.cached_tty.to_le_bytes()));
        self.selection_list.insert(
            0,
            SelectionNode {
                typecode: sel::TTY,
                u: vec![SelVal {
                    num: tty,
                    cmd: Vec::new(),
                }],
            },
        );
    }

    /// `parse_sysv_option`. `Ok(Some(exit))` asks `ps` to exit (`-V`).
    #[allow(clippy::too_many_lines)]
    fn parse_sysv_option(&mut self) -> Result<Option<Exit>, &'static str> {
        let cur = self.arg(self.thisarg);
        let mut i = 1usize;
        while let Some(&c) = cur.get(i) {
            match c {
                b'A' | b'e' => self.all_processes = true,
                b'C' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::COMM, "list of command names must follow -C")?;
                    return Ok(None);
                }
                b'D' => {
                    // Upstream `break`s here rather than returning, so when
                    // the format is the rest of this argument (`-D%H`), the
                    // loop goes on to read that same text as options.
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("date format must follow -D")?;
                    self.lstart_format = Some(a);
                }
                b'F' => {
                    self.format_modifiers |= fm::F;
                    self.format_flags |= ff::UF;
                    self.unix_f_option = true;
                }
                b'G' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::RGID, "list of real groups must follow -G")?;
                    return Ok(None);
                }
                b'H' => self.forest_type = b'u',
                b'L' => self.thread_flags |= tf::U_L,
                b'M' | b'Z' => self.format_modifiers |= fm::M,
                b'N' => self.negate_selection = true,
                b'O' => {
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("format or sort specification must follow -O")?;
                    self.defer_sf_option(&a, crate::sortformat::SF_U_UPPER_O);
                    return Ok(None);
                }
                b'P' => self.format_modifiers |= fm::P,
                b'T' => self.thread_flags |= tf::U_T,
                b'U' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::RUID, "list of real users must follow -U")?;
                    return Ok(None);
                }
                b'V' => {
                    self.exclusive(b"-V", "the option is exclusive: -V")?;
                    self.display_version();
                    return Ok(Some(Exit::Status(0)));
                }
                b'a' => self.simple_select |= ss::U_A,
                b'c' => self.format_modifiers |= fm::C,
                b'd' => self.simple_select |= ss::U_D,
                b'f' => {
                    self.format_flags |= ff::UF;
                    self.unix_f_option = true;
                }
                b'g' => {
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("list of session leaders OR effective group names must follow -g")?;
                    if self.parse_list(&a, sel::SESS).is_ok() {
                        return Ok(None);
                    }
                    if self.parse_list(&a, sel::EGID).is_ok() {
                        return Ok(None);
                    }
                    return Err("list of session leaders OR effective group IDs was invalid");
                }
                b'j' => {
                    if self.sysv_j_format.is_some() {
                        self.format_flags |= ff::UJ;
                    } else {
                        self.format_modifiers |= fm::J;
                    }
                }
                b'l' => self.format_flags |= ff::UL,
                b'm' => self.thread_flags |= tf::U_M,
                b'o' => {
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("format specification must follow -o")?;
                    self.defer_sf_option(&a, crate::sortformat::SF_U_LOWER_O);
                    return Ok(None);
                }
                b'p' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::PID, "list of process IDs must follow -p")?;
                    return Ok(None);
                }
                b'q' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::PID_QUICK, "List of process IDs must follow -q.")?;
                    return Ok(None);
                }
                b's' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::SESS, "list of session IDs must follow -s")?;
                    return Ok(None);
                }
                b't' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(
                        a,
                        sel::TTY,
                        "list of terminals (pty, tty...) must follow -t",
                    )?;
                    return Ok(None);
                }
                b'u' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::EUID, "list of users must follow -u")?;
                    return Ok(None);
                }
                b'w' => self.w_count = self.w_count.saturating_add(1),
                b'x' => {
                    if self.personality & per::SVR4_X != 0 {
                        self.format_modifiers |= fm::Y;
                    } else if self.personality & per::HPUX_X != 0 {
                        self.w_count = self.w_count.saturating_add(2);
                        self.unix_f_option = true;
                    } else {
                        return Err("must set personality to get -x option");
                    }
                }
                b'y' => self.format_modifiers |= fm::Y,
                b'-' => return Err("embedded '-' among SysV options makes no sense"),
                _ => return Err("unsupported SysV option"),
            }
            i = i.saturating_add(1);
        }
        Ok(None)
    }

    /// `parse_bsd_option`.
    #[allow(clippy::too_many_lines)]
    fn parse_bsd_option(&mut self) -> Result<Option<Exit>, &'static str> {
        let cur = self.arg(self.thisarg);
        let mut i = if cur.first() == Some(&b'-') {
            if !self.force_bsd {
                return Err("cannot happen - problem #1");
            }
            1
        } else {
            if self.personality & per::FORCE_BSD != 0 {
                if !self.force_bsd {
                    return Err("cannot happen - problem #2");
                }
            } else if self.force_bsd {
                return Err("second chance parse failed, not BSD or SysV");
            }
            0
        };
        while let Some(&c) = cur.get(i) {
            match c {
                b'0'..=b'9' => {
                    let a = cur.get(i..).unwrap_or_default().to_vec();
                    self.parse_list(&a, sel::PID)?;
                    return Ok(None);
                }
                b'H' => self.thread_flags |= tf::B_H,
                b'L' => {
                    self.exclusive(b"L", "the option is exclusive: L")?;
                    self.print_format_specifiers();
                    return Ok(Some(Exit::Status(0)));
                }
                b'M' => self.thread_flags |= tf::B_M,
                b'O' => {
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("format or sort specification must follow O")?;
                    self.defer_sf_option(&a, crate::sortformat::SF_B_UPPER_O);
                    return Ok(None);
                }
                b'S' => self.include_dead_children = true,
                b'T' => self.push_our_tty(),
                b'U' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::EUID, "list of users must follow U")?;
                    return Ok(None);
                }
                b'V' => {
                    self.exclusive(b"V", "the option is exclusive: V")?;
                    self.display_version();
                    return Ok(Some(Exit::Status(0)));
                }
                b'W' => return Err("obsolete W option not supported (you have a /dev/drum?)"),
                b'X' => self.format_flags |= ff::LX,
                b'Z' => self.format_modifiers |= fm::M,
                b'a' => self.simple_select |= ss::B_A,
                b'c' => self.bsd_c_option = true,
                b'e' => self.bsd_e_option = true,
                b'f' => self.forest_type = b'b',
                b'g' => self.simple_select |= ss::B_G,
                b'h' => {
                    if self.header_type != 0 {
                        return Err("only one heading option may be specified");
                    }
                    self.header_type = if self.personality & per::BSD_H != 0 {
                        crate::HEAD_MULTI
                    } else {
                        crate::HEAD_NONE
                    };
                }
                b'j' => self.format_flags |= ff::BJ,
                b'k' => {
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("long sort specification must follow 'k'")?;
                    self.defer_sf_option(&a, crate::sortformat::SF_G_SORT);
                    return Ok(None);
                }
                b'l' => self.format_flags |= ff::BL,
                b'm' => {
                    if self.personality & per::OLD_M != 0 {
                        self.format_flags |= ff::LM;
                    } else if self.personality & per::BSD_M != 0 {
                        self.defer_sf_option(b"pmem", crate::sortformat::SF_B_M);
                    } else {
                        self.thread_flags |= tf::B_M;
                    }
                }
                b'n' => {
                    self.wchan_is_number = true;
                    self.user_is_number = true;
                }
                b'o' => {
                    let a = self
                        .get_opt_arg(&cur, i)
                        .ok_or("format specification must follow o")?;
                    self.defer_sf_option(&a, crate::sortformat::SF_B_LOWER_O);
                    return Ok(None);
                }
                b'p' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::PID, "list of process IDs must follow p")?;
                    return Ok(None);
                }
                b'q' => {
                    let a = self.get_opt_arg(&cur, i);
                    self.list_option(a, sel::PID_QUICK, "List of process IDs must follow q.")?;
                    return Ok(None);
                }
                b'r' => self.running_only = true,
                b's' => self.format_flags |= ff::BS,
                b't' => {
                    match self.get_opt_arg(&cur, i) {
                        None => self.push_our_tty(),
                        Some(a) => self.parse_list(&a, sel::TTY)?,
                    }
                    return Ok(None);
                }
                b'u' => self.format_flags |= ff::BU,
                b'v' => self.format_flags |= ff::BV,
                b'w' => self.w_count = self.w_count.saturating_add(1),
                b'x' => self.simple_select |= ss::B_X,
                b'-' => return Err("embedded '-' among BSD options makes no sense"),
                _ => return Err("unsupported option (BSD syntax)"),
            }
            i = i.saturating_add(1);
        }
        Ok(None)
    }

    /// `grab_gnu_arg`: after `=` or `:` in the same argument, or the next
    /// argument if this one ends at the name and the next is not empty.
    fn grab_gnu_arg(&mut self, rest: &[u8]) -> Option<Vec<u8>> {
        match rest.first() {
            Some(b'=' | b':') => {
                let v = rest.get(1..).unwrap_or_default();
                (!v.is_empty()).then(|| v.to_vec())
            }
            None => {
                if self.thisarg.saturating_add(2) > self.args.len() {
                    return None;
                }
                let next = self.arg(self.thisarg.saturating_add(1));
                if next.is_empty() {
                    return None;
                }
                self.thisarg = self.thisarg.saturating_add(1);
                Some(next)
            }
            Some(_) => None,
        }
    }

    /// `parse_gnu_option`.
    #[allow(clippy::too_many_lines)]
    fn parse_gnu_option(&mut self) -> Result<Option<Exit>, &'static str> {
        let cur = self.arg(self.thisarg);
        let s = cur.get(2..).unwrap_or_default();
        let sl = s
            .iter()
            .position(|&b| b == b':' || b == b'=')
            .unwrap_or(s.len());
        if sl > 15 {
            return Err("unknown gnu long option");
        }
        let name = s.get(..sl).unwrap_or_default();
        let rest = s.get(sl..).unwrap_or_default();
        let no_arg = |msg: &'static str| if rest.is_empty() { Ok(()) } else { Err(msg) };
        match name {
            b"Group" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::RGID, "list of real groups must follow --Group")?;
            }
            b"User" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::RUID, "list of real users must follow --User")?;
            }
            b"cols" | b"width" | b"columns" => {
                if let Some(a) = self.grab_gnu_arg(rest)
                    && !a.is_empty()
                {
                    let (t, used) = strtol_base0(&a);
                    if used == a.len() && t > 0 && t < 2_000_000_000 {
                        self.screen_cols = i32::try_from(t).unwrap_or(0);
                        return Ok(None);
                    }
                }
                return Err("number of columns must follow --cols, --width, or --columns");
            }
            b"cumulative" => {
                no_arg("option --cumulative does not take an argument")?;
                self.include_dead_children = true;
            }
            b"date-format" => {
                let a = self
                    .grab_gnu_arg(rest)
                    .ok_or("date format must follow --date-format")?;
                self.lstart_format = Some(a);
            }
            b"deselect" => {
                no_arg("option --deselect does not take an argument")?;
                self.negate_selection = true;
            }
            b"no-header" | b"no-headers" | b"no-heading" | b"no-headings" | b"noheader"
            | b"noheaders" | b"noheading" | b"noheadings" => {
                no_arg("option --no-heading does not take an argument")?;
                if self.header_type != 0 {
                    return Err("only one heading option may be specified");
                }
                self.header_type = crate::HEAD_NONE;
            }
            b"header" | b"headers" | b"heading" | b"headings" => {
                no_arg("option --heading does not take an argument")?;
                if self.header_type != 0 {
                    return Err("only one heading option may be specified");
                }
                self.header_type = crate::HEAD_MULTI;
            }
            b"forest" => {
                no_arg("option --forest does not take an argument")?;
                self.forest_type = b'g';
            }
            b"format" => {
                let a = self
                    .grab_gnu_arg(rest)
                    .ok_or("format specification must follow --format")?;
                self.defer_sf_option(&a, crate::sortformat::SF_G_FORMAT);
            }
            b"group" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::EGID, "list of effective groups must follow --group")?;
            }
            b"help" => {
                let a = self.grab_gnu_arg(rest);
                return Ok(Some(self.do_help(a.as_deref(), 0)));
            }
            b"info" => {
                self.exclusive(b"--info", "the option is exclusive: --info")?;
                self.self_info();
                return Ok(Some(Exit::Status(0)));
            }
            b"pid" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::PID, "list of process IDs must follow --pid")?;
            }
            b"quick-pid" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(
                    a,
                    sel::PID_QUICK,
                    "List of process IDs must follow --quick-pid.",
                )?;
            }
            b"ppid" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::PPID, "list of process IDs must follow --ppid")?;
            }
            b"rows" | b"lines" => {
                if let Some(a) = self.grab_gnu_arg(rest)
                    && !a.is_empty()
                {
                    let (t, used) = strtol_base0(&a);
                    if used == a.len() && t > 0 && t < 2_000_000_000 {
                        self.screen_rows = i32::try_from(t).unwrap_or(0);
                        return Ok(None);
                    }
                }
                return Err("number of rows must follow --rows or --lines");
            }
            b"sid" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::SESS, "some sid thing(s) must follow --sid")?;
            }
            b"signames" => self.signal_names = true,
            b"sort" => {
                let a = self
                    .grab_gnu_arg(rest)
                    .ok_or("long sort specification must follow --sort")?;
                self.defer_sf_option(&a, crate::sortformat::SF_G_SORT);
            }
            b"tty" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::TTY, "list of ttys must follow --tty")?;
            }
            b"user" => {
                let a = self.grab_gnu_arg(rest);
                self.list_option(a, sel::EUID, "list of effective users must follow --user")?;
            }
            b"version" => {
                self.exclusive(b"--version", "the option is exclusive: --version")?;
                self.display_version();
                return Ok(Some(Exit::Status(0)));
            }
            b"context" => self.format_flags |= ff::FC,
            _ => return Err("unknown gnu long option"),
        }
        Ok(None)
    }

    /// `parse_trailing_pids`: every argument from here on is a PID, `-PGRP`
    /// or `+SESSION`.
    fn parse_trailing_pids(&mut self) -> Result<(), &'static str> {
        let start = self.thisarg;
        self.thisarg = self.args.len().saturating_sub(1);
        let mut pids = Vec::new();
        let mut grps = Vec::new();
        let mut sids = Vec::new();
        for i in start..self.args.len() {
            let data = self.arg(i);
            match data.first() {
                Some(b'-') => grps.push(Self::parse_pid(data.get(1..).unwrap_or_default())?),
                Some(b'+') => sids.push(Self::parse_pid(data.get(1..).unwrap_or_default())?),
                _ => pids.push(Self::parse_pid(&data)?),
            }
        }
        if !pids.is_empty() {
            self.selection_list.insert(
                0,
                SelectionNode {
                    typecode: sel::PID,
                    u: pids,
                },
            );
        }
        if !grps.is_empty() {
            self.selection_list.insert(
                0,
                SelectionNode {
                    typecode: sel::PGRP,
                    u: grps,
                },
            );
        }
        if !sids.is_empty() {
            self.selection_list.insert(
                0,
                SelectionNode {
                    typecode: sel::SESS,
                    u: sids,
                },
            );
        }
        Ok(())
    }

    /// `parse_all_options`. `Ok(Some(exit))` is an option that ends `ps`.
    fn parse_all_options(&mut self) -> Result<Option<Exit>, &'static str> {
        loop {
            self.thisarg = self.thisarg.saturating_add(1);
            if self.thisarg >= self.args.len() {
                return Ok(None);
            }
            let at = arg_type(&self.arg(self.thisarg));
            let r = match at {
                Arg::Gnu => self.parse_gnu_option()?,
                Arg::Sysv if !self.force_bsd => self.parse_sysv_option()?,
                Arg::Sysv => {
                    self.prefer_bsd_defaults = true;
                    self.parse_bsd_option()?
                }
                Arg::Bsd => {
                    if self.force_bsd && self.personality & per::FORCE_BSD == 0 {
                        return Err("way bad");
                    }
                    self.prefer_bsd_defaults = true;
                    self.parse_bsd_option()?
                }
                Arg::Pgrp | Arg::Sess | Arg::Pid => {
                    self.prefer_bsd_defaults = true;
                    self.parse_trailing_pids()?;
                    None
                }
                Arg::End | Arg::Fail => return Err("garbage option"),
            };
            if r.is_some() {
                return Ok(r);
            }
        }
    }

    /// `choose_dimensions`.
    fn choose_dimensions(&mut self) {
        if self.w_count != 0 && self.screen_cols < 132 {
            self.screen_cols = 132;
        }
        if self.w_count > 1 {
            self.screen_cols = crate::OUTBUF_SIZE;
        }
    }

    /// `thread_option_check`.
    fn thread_option_check(&mut self) -> Result<(), &'static str> {
        if self.thread_flags == 0 {
            self.thread_flags = tf::SHOW_PROC;
            return Ok(());
        }
        if self.forest_type != 0 {
            return Err("thread display conflicts with forest display");
        }
        let t = self.thread_flags;
        if t & tf::B_H != 0 && t & (tf::B_M | tf::U_M) != 0 {
            return Err("thread flags conflict; can't use H with m or -m");
        }
        if t & tf::B_M != 0 && t & tf::U_M != 0 {
            return Err("thread flags conflict; can't use both m and -m");
        }
        if t & tf::U_L != 0 && t & tf::U_T != 0 {
            return Err("thread flags conflict; can't use both -L and -T");
        }
        if t & tf::B_H != 0 {
            self.thread_flags |= tf::SHOW_PROC | tf::LOOSE_TASKS;
        }
        if t & (tf::B_M | tf::U_M) != 0 {
            self.thread_flags |= tf::SHOW_PROC | tf::SHOW_TASK | tf::SHOW_BOTH;
        }
        if t & (tf::U_T | tf::U_L) != 0 {
            if t & (tf::B_M | tf::U_M | tf::B_H) != 0 {
                self.thread_flags |= tf::MUST_USE;
            } else {
                self.thread_flags |= tf::SHOW_TASK;
            }
        }
        Ok(())
    }

    /// One pass over the arguments and everything that checks them.
    fn one_pass(&mut self) -> Result<Option<Exit>, Vec<u8>> {
        if let Some(exit) = self.parse_all_options()? {
            return Ok(Some(exit));
        }
        self.thread_option_check()?;
        self.process_sf_options()?;
        self.select_bits_setup()?;
        Ok(None)
    }

    /// `arg_parse`.
    pub fn arg_parse(&mut self) -> Result<(), Exit> {
        self.thisarg = 0;
        let first = if self.personality & per::FORCE_BSD != 0 {
            Err(Vec::new())
        } else {
            self.one_pass()
        };
        let err = match first {
            Ok(None) => {
                self.choose_dimensions();
                return Ok(());
            }
            Ok(Some(exit)) => return Err(exit),
            Err(e) => e,
        };

        self.reset_global()?;
        self.w_count = 0;
        self.reset_sortformat();
        self.format_flags = 0;
        self.thisarg = 0;
        self.force_bsd = true;
        self.prefer_bsd_defaults = true;
        if (per::OLD_M | per::BSD_M) & self.personality == 0 {
            self.personality |= per::OLD_M;
        }
        let err2 = match self.one_pass() {
            Ok(None) => {
                self.choose_dimensions();
                return Ok(());
            }
            Ok(Some(exit)) => return Err(exit),
            Err(e) => e,
        };
        self.w_count = 0;
        let shown = if self.personality & per::FORCE_BSD != 0 {
            err2
        } else {
            err
        };
        let mut msg = b"error: ".to_vec();
        msg.extend_from_slice(&shown);
        msg.push(b'\n');
        self.eprint(&msg);
        Err(self.do_help(None, 1))
    }

    /// `arg_check_conflicts`: `-q`'s restrictions.
    pub fn arg_check_conflicts(&mut self) -> Result<(), Exit> {
        let quick = self
            .selection_list
            .iter()
            .filter(|n| n.typecode == sel::PID_QUICK)
            .count();
        let len = self.selection_list.len();
        let fail = |ps: &mut Self, msg: &[u8]| {
            ps.eprint(msg);
            Err(Exit::Status(1))
        };
        if quick > 1 {
            return fail(self, b"q/-q/--quick-pid can only be used once.\n");
        }
        if quick > 0 && len > quick {
            return fail(
                self,
                b"q/-q/--quick-pid cannot be combined with other selection options.\n",
            );
        }
        if quick > 0 && self.forest_type != 0 {
            return fail(
                self,
                b"q/-q/--quick-pid cannot be used together with forest type listings.\n",
            );
        }
        if quick > 0 && !self.sort_list.is_empty() {
            return fail(
                self,
                b"q/-q,--quick-pid cannot be used together with sort options.\n",
            );
        }
        if quick > 0 && self.negate_selection {
            return fail(
                self,
                b"q/-q/--quick-pid cannot be used together with negation switches.\n",
            );
        }
        Ok(())
    }

    /// `display_ps_version`.
    fn display_version(&mut self) {
        let mut line = self.myname.clone();
        line.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
        self.out.extend_from_slice(&line);
    }
}

/// `stat` on a path: whether it is a character device, and its device
/// number -- or `None` if `stat` fails.
#[cfg(unix)]
fn char_dev(path: &[u8]) -> Option<(bool, u64)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta =
        std::fs::metadata(std::path::Path::new(&coreutils::quote::os_from_bytes(path))).ok()?;
    Some((meta.file_type().is_char_device(), meta.rdev()))
}

/// The host build has no devices.
#[cfg(not(unix))]
fn char_dev(_path: &[u8]) -> Option<(bool, u64)> {
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn base_zero_reads_hex_octal_and_decimal() {
        assert_eq!(strtoul_base0(b"0x1f"), (31, 4));
        assert_eq!(strtoul_base0(b"010"), (8, 3));
        assert_eq!(strtoul_base0(b"08"), (0, 1));
        assert_eq!(strtoul_base0(b"0x"), (0, 1));
        assert_eq!(strtoul_base0(b"42abc"), (42, 2));
        assert_eq!(strtoul_base0(b"-1"), (u64::MAX, 2));
        assert_eq!(strtoul_base0(b"abc"), (0, 0));
        assert_eq!(strtol_base0(b"-5"), (-5, 2));
        assert_eq!(strtol_base0(b"0x10"), (16, 4));
    }

    #[test]
    fn arguments_are_classified_by_their_first_bytes() {
        assert!(arg_type(b"aux") == Arg::Bsd);
        assert!(arg_type(b"-ef") == Arg::Sysv);
        assert!(arg_type(b"--sort") == Arg::Gnu);
        assert!(arg_type(b"--") == Arg::End);
        assert!(arg_type(b"-5") == Arg::Pgrp);
        assert!(arg_type(b"5") == Arg::Pid);
        assert!(arg_type(b"+5") == Arg::Sess);
        assert!(arg_type(b"") == Arg::Fail);
        assert!(arg_type(b"-") == Arg::Fail);
        assert!(arg_type(b"--5") == Arg::Fail);
        assert!(arg_type(b"_") == Arg::Fail);
    }

    #[test]
    fn pids_are_checked_as_upstream_checks_them() {
        assert_eq!(Ps::parse_pid(b"12").unwrap().num, 12);
        assert_eq!(Ps::parse_pid(b"0x10").unwrap().num, 16);
        assert_eq!(Ps::parse_pid(b"0").unwrap_err(), "process ID out of range");
        assert_eq!(
            Ps::parse_pid(b"2147483648").unwrap_err(),
            "process ID out of range"
        );
        assert_eq!(
            Ps::parse_pid(b"12a").unwrap_err(),
            "process ID list syntax error"
        );
        assert_eq!(Ps::parse_cmd(&[b'x'; 80]).cmd.len(), 63);
    }
}
