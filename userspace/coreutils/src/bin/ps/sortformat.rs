//! `sortformat.c`: what `-o`, `-O`, `o`, `O`, `k`, `--format` and `--sort`
//! say, turned into columns and sort keys -- and, when none of them is given,
//! the format the other options imply.
//!
//! Each such option is only *recorded* while the command line is parsed
//! (`defer_sf_option`) and read afterwards, oldest first, so that `-O` can
//! insist on coming first and BSD `O` can be tried as a sort order and then
//! as a format. The quirks kept from upstream, because they show:
//!
//! * `format_parse`'s error message is written once into a static buffer and
//!   never cleared, so a second unknown specifier is reported with the
//!   first's name.
//! * A format with a `%` in it that does not parse as specifiers is tried
//!   again as an AIX format (`%p %a`), keeping whatever columns the failed
//!   attempt had already made.
//! * In BSD `O`'s short sort keys, only the first key and any key with an
//!   explicit `+` or `-` is sorted on; the rest are recorded with a
//!   direction of 0, which the sort refuses.

use crate::formats::{self, AIX, Pr};
use crate::items::{ASCEND, DESCEND};
use crate::{FormatNode, Ps, SortNode, ff, fm, per, tf};

/// `SF_U_O`: `-O`.
pub const SF_U_UPPER_O: i32 = 1;
/// `SF_U_o`: `-o`.
pub const SF_U_LOWER_O: i32 = 2;
/// `SF_B_O`: BSD `O`.
pub const SF_B_UPPER_O: i32 = 3;
/// `SF_B_o`: BSD `o`.
pub const SF_B_LOWER_O: i32 = 4;
/// `SF_B_m`: BSD `m`, as a sort on `pmem`.
pub const SF_B_M: i32 = 5;
/// `SF_G_sort`: `--sort` and BSD `k`.
pub const SF_G_SORT: i32 = 6;
/// `SF_G_format`: `--format`.
pub const SF_G_FORMAT: i32 = 7;

/// One recorded option: `sf_node`.
#[derive(Clone, Debug)]
pub struct SfNode {
    pub sf: Vec<u8>,
    pub sf_code: i32,
    /// Its columns, in display order.
    pub f_cooked: Vec<FormatNode>,
    /// Its sort keys, front first (the last written is first).
    pub s_cooked: Vec<SortNode>,
}

impl Ps {
    /// `do_one_spec`: the columns one specifier names -- one, or a macro's
    /// several, each with `override` as its header if given.
    pub fn do_one_spec(&mut self, spec: &[u8], over: Option<&[u8]>) -> Option<Vec<FormatNode>> {
        if let Some(fs) = formats::search_format(spec) {
            let w1 = if fs.flags & formats::PIDMAX != 0 {
                let pid_len = i32::try_from(coreutils::procps::sysinfo::pid_length(&self.root))
                    .unwrap_or(i32::MAX);
                let head = i32::try_from(fs.head.len()).unwrap_or(i32::MAX);
                pid_len.max(head)
            } else {
                fs.width
            };
            let (width, name) = match over {
                Some(o) => (
                    w1.max(i32::try_from(o.len()).unwrap_or(i32::MAX)),
                    o.to_vec(),
                ),
                None => (w1, fs.head.to_vec()),
            };
            return Some(vec![FormatNode {
                name,
                pr: Some(fs.pr),
                width,
                vendor: fs.vendor,
                flags: fs.flags,
            }]);
        }
        let head = formats::search_macro(spec)?;
        let mut list = Vec::new();
        let mut walk = head;
        while !walk.is_empty() {
            let dist = walk
                .iter()
                .position(|&b| b == b',' || b == b' ')
                .unwrap_or(walk.len());
            let word = walk.get(..dist).unwrap_or_default();
            // "Call self, assume success": every word is a specifier.
            list.extend(self.do_one_spec(word, over).unwrap_or_default());
            walk = walk.get(dist..).unwrap_or_default();
            if !walk.is_empty() {
                walk = walk.get(1..).unwrap_or_default();
            }
        }
        Some(list)
    }

    /// `O_wrap`: `-O`'s columns between `pid` and the default trailer.
    fn o_wrap(&mut self, idx: usize, otype: u8) -> Result<(), &'static str> {
        let trailer: &[u8] = if otype == b'b' {
            b"END_BSD"
        } else {
            b"END_SYS5"
        };
        let pid = self
            .do_one_spec(b"pid", None)
            .ok_or("seriously crashing: goodbye cruel world")?;
        let tail = self
            .do_one_spec(trailer, None)
            .ok_or("seriously crashing: goodbye cruel world")?;
        if let Some(node) = self.sf_list.get_mut(idx) {
            let user = std::mem::take(&mut node.f_cooked);
            node.f_cooked = pid.into_iter().chain(user).chain(tail).collect();
        }
        Ok(())
    }

    /// `aix_format_parse`: `%x` descriptors and literal text.
    fn aix_format_parse(&mut self, sf: &[u8]) -> Result<Vec<FormatNode>, &'static str> {
        // The checking state machine, including its counting of the byte
        // after each descriptor as the start of a text item.
        let at = |k: usize| sf.get(k).copied().unwrap_or(0);
        let mut items = 0usize;
        let mut w = 0usize;
        let mut c = at(w);
        w = w.saturating_add(1);
        enum St {
            Initial,
            GetMore,
            GetDesc,
        }
        let mut st = St::Initial;
        loop {
            match st {
                St::Initial => {
                    if c == b'%' {
                        st = St::GetDesc;
                        continue;
                    }
                    if c == 0 {
                        break;
                    }
                    items = items.saturating_add(1);
                    st = St::GetMore;
                }
                St::GetMore => {
                    c = at(w);
                    w = w.saturating_add(1);
                    if c == b'%' {
                        st = St::GetDesc;
                    } else if c == b' ' {
                        st = St::GetMore;
                    } else if c != 0 {
                        return Err("improper AIX field descriptor");
                    } else {
                        break;
                    }
                }
                St::GetDesc => {
                    items = items.saturating_add(1);
                    c = at(w);
                    w = w.saturating_add(1);
                    if c != 0 && c != b' ' {
                        st = St::Initial;
                    } else {
                        return Err("missing AIX field descriptor");
                    }
                }
            }
        }

        let mut out = Vec::new();
        let mut walk = sf;
        for _ in 0..items {
            if walk.first() == Some(&b'%') {
                walk = walk.get(1..).unwrap_or_default();
                let d = walk.first().copied().unwrap_or(0);
                if d == b'%' {
                    return Err("missing AIX field descriptor");
                }
                walk = walk.get(1..).unwrap_or_default();
                let (spec, head) = formats::search_aix(d).ok_or("unknown AIX field descriptor")?;
                let nodes = self
                    .do_one_spec(spec, Some(head))
                    .ok_or("AIX field descriptor processing bug")?;
                out.extend(nodes);
            } else {
                let len = walk.iter().position(|&b| b == b'%').unwrap_or(walk.len());
                let text = walk.get(..len).unwrap_or_default().to_vec();
                walk = walk.get(len..).unwrap_or_default();
                out.push(FormatNode {
                    width: i32::try_from(text.len()).unwrap_or(i32::MAX),
                    name: text,
                    pr: None,
                    vendor: AIX,
                    flags: formats::ET,
                });
            }
        }
        self.already_parsed_format = true;
        Ok(out)
    }

    /// `format_parse`: a list of specifiers, each perhaps with `=HEADER` and
    /// `:WIDTH`, into columns. On failure the columns made so far are kept
    /// (`f_cooked` is upstream's own list), and a format with a `%` in it is
    /// tried as an AIX format.
    fn format_parse(&mut self, sf: &[u8], f_cooked: &mut Vec<FormatNode>) -> Result<(), Vec<u8>> {
        let result = self.format_parse_inner(sf, f_cooked);
        let Err(err) = result else {
            self.already_parsed_format = true;
            return Ok(());
        };
        if sf.contains(&b'%') {
            return match self.aix_format_parse(sf) {
                Ok(nodes) => {
                    f_cooked.extend(nodes);
                    Ok(())
                }
                Err(e) => Err(e.as_bytes().to_vec()),
            };
        }
        Err(err)
    }

    fn format_parse_inner(
        &mut self,
        sf: &[u8],
        f_cooked: &mut Vec<FormatNode>,
    ) -> Result<(), Vec<u8>> {
        let improper = || b"improper format list".to_vec();
        let mut buf = sf.to_vec();
        let mut need_item = true;
        let mut items = 0usize;
        if buf.is_empty() {
            return Err(improper());
        }
        for &c in &buf {
            match c {
                b' ' | b',' | b'\t' | b'\n' => {
                    if need_item {
                        return Err(improper());
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
        if items == 0 {
            return Err(b"empty format list".to_vec());
        }
        // One trailing delimiter is allowed: it is cut.
        if need_item {
            buf.pop();
        }
        let mut walk: Option<&[u8]> = Some(&buf);
        while items > 0 {
            items = items.saturating_sub(1);
            let Some(w) = walk else {
                return Err(b"please report this bug".to_vec());
            };
            let sep = w
                .iter()
                .position(|&b| matches!(b, b' ' | b',' | b'\t' | b'\n'));
            // With items left, the separator ends this one; the last item
            // runs to the end, separators and all (a header may hold them).
            let (mut item, next) = match sep {
                Some(at) if items > 0 => (
                    w.get(..at).unwrap_or_default(),
                    Some(w.get(at.saturating_add(1)..).unwrap_or_default()),
                ),
                Some(at) => (w, Some(w.get(at.saturating_add(1)..).unwrap_or_default())),
                None => (w, None),
            };
            let mut header: Option<&[u8]> = None;
            if let Some(eq) = item.iter().position(|&b| b == b'=') {
                header = item.get(eq.saturating_add(1)..);
                item = item.get(..eq).unwrap_or_default();
            }
            let mut width: Option<&[u8]> = None;
            if let Some(colon) = item.iter().position(|&b| b == b':') {
                let digits = item.get(colon.saturating_add(1)..).unwrap_or_default();
                let bad = digits.is_empty()
                    || !digits.iter().all(u8::is_ascii_digit)
                    || digits.first() == Some(&b'0')
                    || coreutils::procps::scanf::atoi(digits) <= 0;
                if bad {
                    return Err(b"column widths must be unsigned decimal numbers".to_vec());
                }
                width = Some(digits);
                item = item.get(..colon).unwrap_or_default();
            }
            let Some(mut nodes) = self.do_one_spec(item, header) else {
                if self.errbuf.is_none() {
                    let mut msg = b"unknown user-defined format specifier \"".to_vec();
                    msg.extend_from_slice(item);
                    msg.push(b'"');
                    msg.truncate(79);
                    self.errbuf = Some(msg);
                }
                return Err(self.errbuf.clone().unwrap_or_default());
            };
            if let Some(digits) = width {
                if nodes.len() > 1 {
                    return Err(
                        b"can not set width for a macro (multi-column) format specifier".to_vec(),
                    );
                }
                if let Some(n) = nodes.first_mut() {
                    n.width = coreutils::procps::scanf::atoi(digits);
                }
            }
            f_cooked.extend(nodes);
            walk = match (sep, next) {
                (Some(_), Some(n)) if items > 0 => Some(n),
                (Some(_), Some(n)) => Some(n),
                _ => None,
            };
        }
        Ok(())
    }

    /// `do_one_sort_spec`: `[+|-]specifier`.
    fn do_one_sort_spec(spec: &[u8]) -> Option<SortNode> {
        let (reverse, name) = match spec.first() {
            Some(b'-') => (DESCEND, spec.get(1..).unwrap_or_default()),
            Some(b'+') => (ASCEND, spec.get(1..).unwrap_or_default()),
            _ => (ASCEND, spec),
        };
        let fs = formats::search_format(name)?;
        Some(SortNode {
            sr: fs.sr,
            xe: fs.pr,
            reverse,
        })
    }

    /// `long_sort_parse`: `--sort`'s list, keys prepended as written.
    fn long_sort_parse(&mut self, idx: usize) -> Result<(), &'static str> {
        let Some(sf) = self.sf_list.get(idx).map(|n| n.sf.clone()) else {
            return Ok(());
        };
        let mut buf = sf;
        let mut need_item = true;
        let mut items = 0usize;
        if buf.is_empty() {
            return Err("improper sort list");
        }
        for &c in &buf {
            match c {
                b' ' | b',' | b'\t' | b'\n' => {
                    if need_item {
                        return Err("improper sort list");
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
        if items == 0 {
            return Err("empty sort list");
        }
        if need_item {
            buf.pop();
        }
        let mut walk: &[u8] = &buf;
        let mut keys = Vec::new();
        for _ in 0..items {
            let sep = walk
                .iter()
                .position(|&b| matches!(b, b' ' | b',' | b'\t' | b'\n'));
            let item = match sep {
                Some(at) => walk.get(..at).unwrap_or_default(),
                None => walk,
            };
            let node = Self::do_one_sort_spec(item).ok_or("unknown sort specifier")?;
            keys.insert(0, node);
            walk = match sep {
                Some(at) => walk.get(at.saturating_add(1)..).unwrap_or_default(),
                None => &[],
            };
        }
        if let Some(node) = self.sf_list.get_mut(idx) {
            let mut all = keys;
            all.append(&mut node.s_cooked);
            node.s_cooked = all;
        }
        self.already_parsed_sort = true;
        Ok(())
    }

    /// `verify_short_sort`.
    fn verify_short_sort(&self, arg: &[u8]) -> Result<(), &'static str> {
        const ALL: &[u8] = b"CGJKMNPRSTUcfgjkmnoprstuvy+-";
        if !arg.iter().all(|c| ALL.contains(c)) {
            return Err("bad sorting code");
        }
        let mut seen = [false; 256];
        for (i, &c) in arg.iter().enumerate() {
            match c {
                b'+' | b'-' => {
                    let next = arg.get(i.saturating_add(1)).copied().unwrap_or(0);
                    if next == 0 || next == b'+' || next == b'-' {
                        return Err("bad sorting code");
                    }
                }
                _ => {
                    if c == b'P' && self.forest_type != 0 {
                        return Err("PPID sort and forest output conflict");
                    }
                    let slot = seen.get_mut(usize::from(c)).ok_or("bad sorting code")?;
                    if *slot {
                        return Err("bad sorting code");
                    }
                    *slot = true;
                }
            }
        }
        Ok(())
    }

    /// `short_sort_parse`: BSD `O`'s letters. Its error is ignored by its
    /// caller, so an unknown letter just ends the list.
    fn short_sort_parse(&mut self, idx: usize) {
        let Some(sf) = self.sf_list.get(idx).map(|n| n.sf.clone()) else {
            return;
        };
        let mut direction = ASCEND;
        let mut keys: Vec<SortNode> = Vec::new();
        for &c in &sf {
            match c {
                b'+' => direction = ASCEND,
                b'-' => direction = DESCEND,
                _ => {
                    let Some(spec) = formats::search_shortsort(c) else {
                        break;
                    };
                    let Some(mut node) = Self::do_one_sort_spec(spec) else {
                        break;
                    };
                    node.reverse = direction;
                    keys.insert(0, node);
                    direction = 0;
                }
            }
        }
        if let Some(node) = self.sf_list.get_mut(idx) {
            keys.append(&mut node.s_cooked);
            node.s_cooked = keys;
        }
        self.already_parsed_sort = true;
    }

    /// Add `cooked` to option `idx`'s columns.
    fn add_cooked(&mut self, idx: usize, cooked: Vec<FormatNode>) {
        if let Some(n) = self.sf_list.get_mut(idx) {
            n.f_cooked.extend(cooked);
        }
    }

    /// `format_parse` on option `idx`'s text, its columns kept on the option
    /// whether or not it succeeds.
    fn format_parse_node(&mut self, idx: usize, sf: &[u8]) -> Result<(), Vec<u8>> {
        let mut cooked = Vec::new();
        let r = self.format_parse(sf, &mut cooked);
        self.add_cooked(idx, cooked);
        r
    }

    /// `parse_O_option`: one recorded option, after every older one.
    fn parse_o_option(&mut self, idx: usize) -> Result<(), Vec<u8>> {
        if idx.saturating_add(1) < self.sf_list.len() {
            self.parse_o_option(idx.saturating_add(1))?;
        }
        let Some((sf, code)) = self.sf_list.get(idx).map(|n| (n.sf.clone(), n.sf_code)) else {
            return Ok(());
        };
        match code {
            SF_B_LOWER_O | SF_G_FORMAT | SF_U_LOWER_O => {
                self.format_parse_node(idx, &sf)?;
                self.already_parsed_format = true;
                Ok(())
            }
            SF_U_UPPER_O => {
                if self.already_parsed_format {
                    return Err("option -O can not follow other format options".into());
                }
                self.format_parse_node(idx, &sf)?;
                self.already_parsed_format = true;
                self.o_wrap(idx, b'u').map_err(Vec::from)
            }
            SF_B_UPPER_O => {
                let err = if self.have_gnu_sort || self.already_parsed_sort {
                    Err("multiple sort options")
                } else {
                    self.verify_short_sort(&sf)
                };
                let Err(err) = err else {
                    self.short_sort_parse(idx);
                    self.already_parsed_sort = true;
                    return Ok(());
                };
                if self.already_parsed_format {
                    return Err("option O is neither first format nor sort order".into());
                }
                if self.format_parse_node(idx, &sf).is_ok() {
                    self.already_parsed_format = true;
                    return self.o_wrap(idx, b'b').map_err(Vec::from);
                }
                Err(err.into())
            }
            SF_G_SORT | SF_B_M => {
                let r = if self.already_parsed_sort {
                    Err("multiple sort options")
                } else {
                    self.long_sort_parse(idx)
                };
                self.already_parsed_sort = true;
                r.map_err(Vec::from)
            }
            _ => Err("please report this bug".into()),
        }
    }

    /// `defer_sf_option`: record a format or sort option for later.
    pub fn defer_sf_option(&mut self, arg: &[u8], source: i32) {
        self.sf_list.insert(
            0,
            SfNode {
                sf: arg.to_vec(),
                sf_code: source,
                f_cooked: Vec::new(),
                s_cooked: Vec::new(),
            },
        );
        if source == SF_G_SORT {
            self.have_gnu_sort = true;
        }
    }

    /// `reset_sortformat`.
    pub fn reset_sortformat(&mut self) {
        self.sf_list.clear();
        self.format_list.clear();
        self.sort_list.clear();
        self.have_gnu_sort = false;
        self.already_parsed_sort = false;
        self.already_parsed_format = false;
    }

    /// `fmt_add_after`: insert `node` after the first column named `find`.
    fn fmt_add_after(&mut self, find: &[u8], node: FormatNode) -> bool {
        match self.format_list.iter().position(|n| n.name == find) {
            Some(at) => {
                self.format_list.insert(at.saturating_add(1), node);
                true
            }
            None => false,
        }
    }

    /// `fmt_delete`: remove the first column named `find`.
    fn fmt_delete(&mut self, find: &[u8]) -> bool {
        match self.format_list.iter().position(|n| n.name == find) {
            Some(at) => {
                self.format_list.remove(at);
                true
            }
            None => false,
        }
    }

    /// One column by name, for the built-in lists (always found).
    fn spec1(&mut self, name: &[u8]) -> FormatNode {
        self.do_one_spec(name, None)
            .and_then(|v| v.into_iter().next())
            .unwrap_or(FormatNode {
                name: name.to_vec(),
                pr: Some(Pr::Nop),
                width: 1,
                vendor: 0,
                flags: formats::AN | formats::RIGHT,
            })
    }

    /// `PUSH`: a column onto the front of the list being built backwards.
    fn push(&mut self, name: &[u8]) {
        let n = self.spec1(name);
        self.format_list.insert(0, n);
    }

    /// `generate_sysv_list`: the Unix default format, built backwards.
    fn generate_sysv_list(&mut self) -> Result<(), &'static str> {
        let flags = self.format_flags;
        let mods = self.format_modifiers;
        if mods & fm::Y != 0 && flags & ff::UL == 0 {
            return Err("modifier -y without format -l makes no sense");
        }
        if self.prefer_bsd_defaults {
            if flags != 0 {
                self.push(b"cmd");
            } else {
                self.push(b"args");
            }
            self.push(b"bsdtime");
            if flags & ff::UL == 0 {
                self.push(b"stat");
            }
        } else {
            if flags & ff::UF != 0 {
                self.push(b"cmd");
            } else {
                self.push(b"ucmd");
            }
            self.push(b"time");
        }
        self.push(b"tname");
        if flags & ff::UF != 0 {
            self.push(b"stime");
        }
        if mods & fm::F != 0 {
            if mods & fm::P == 0 {
                self.push(b"psr");
            }
            if !(flags & ff::UL != 0 && mods & fm::Y != 0) {
                self.push(b"rss");
            }
        }
        if flags & ff::UL != 0 {
            self.push(b"wchan");
        }
        if flags & ff::UL != 0 && mods & fm::Y == 0 && self.personality & per::IRIX_L != 0 {
            self.push(b"sgi_rss");
            self.format_list.insert(
                0,
                FormatNode {
                    name: b":".to_vec(),
                    pr: None,
                    width: 1,
                    vendor: AIX,
                    flags: formats::ET,
                },
            );
        }
        if mods & fm::F != 0 || flags & ff::UL != 0 {
            self.push(b"sz");
        }
        if flags & ff::UL != 0 {
            if mods & fm::Y != 0 {
                self.push(b"rss");
            } else if self.personality & (per::ZAP_ADDR | per::IRIX_L) != 0 {
                self.push(b"sgi_p");
            } else {
                self.push(b"addr_1");
            }
        }
        if mods & fm::C != 0 {
            self.push(b"pri");
            self.push(b"class");
        } else if flags & ff::UL != 0 {
            self.push(b"ni");
            if self.personality & per::IRIX_L != 0 {
                self.push(b"priority");
            } else {
                self.push(b"opri");
            }
        }
        if self.thread_flags & tf::U_L != 0 && flags & ff::UF != 0 {
            self.push(b"nlwp");
        }
        if flags & (ff::UF | ff::UL) != 0 && mods & fm::C == 0 {
            self.push(b"c");
        }
        if mods & fm::P != 0 {
            self.push(b"psr");
        }
        if self.thread_flags & tf::U_L != 0 {
            self.push(b"lwp");
        }
        if mods & fm::J != 0 {
            self.push(b"sid");
            self.push(b"pgid");
        }
        if flags & (ff::UF | ff::UL) != 0 {
            self.push(b"ppid");
        }
        if self.thread_flags & tf::U_T != 0 {
            self.push(b"spid");
        }
        self.push(b"pid");
        if flags & ff::UF != 0 {
            if self.personality & per::SANE_USER != 0 {
                self.push(b"user");
            } else {
                self.push(b"uid_hack");
            }
        } else if flags & ff::UL != 0 {
            self.push(b"uid");
        }
        if flags & ff::UL != 0 {
            self.push(b"s");
            if mods & fm::Y == 0 {
                self.push(b"f");
            }
        }
        if mods & fm::M != 0 {
            self.push(b"label");
        }
        Ok(())
    }

    /// `process_sf_options`: every recorded option read, merged into
    /// `format_list` and `sort_list`, and the default format built if
    /// nothing gave one.
    ///
    /// # Errors
    ///
    /// The message to print: most are fixed text, but an unknown specifier's
    /// names it, in whatever bytes it was given.
    #[allow(clippy::too_many_lines)]
    pub fn process_sf_options(&mut self) -> Result<(), Vec<u8>> {
        if !self.sf_list.is_empty() {
            self.parse_o_option(0)?;
        }
        if !self.format_list.is_empty() {
            return Err("bug: must reset the list first".into());
        }
        // Oldest option's columns first.
        let mut cols = Vec::new();
        for node in self.sf_list.iter_mut().rev() {
            cols.append(&mut node.f_cooked);
        }
        self.format_list = cols;
        // Each option's keys go in front of the newer options' keys.
        let mut keys: Vec<SortNode> = Vec::new();
        for node in &mut self.sf_list {
            let mut mine = std::mem::take(&mut node.s_cooked);
            mine.append(&mut keys);
            keys = mine;
        }
        let mut sorted = keys;
        sorted.append(&mut self.sort_list);
        self.sort_list = sorted;

        if !self.sort_list.is_empty() && self.thread_flags & tf::NO_SORT != 0 {
            return Err("tell <procps@freelists.org> what you expected".into());
        }

        if self.format_flags == 0 && self.format_modifiers == 0 && self.format_list.is_empty() {
            let env = crate::env_bytes("PS_FORMAT").filter(|v| !v.is_empty());
            if let Some(tmp) = env {
                if self.thread_flags & tf::MUST_USE != 0 {
                    return Err(
                        "tell <procps@freelists.org> what you want (-L/-T, -m/m/H, and $PS_FORMAT)"
                            .into(),
                    );
                }
                let mut cooked = Vec::new();
                match self.format_parse(&tmp, &mut cooked) {
                    Ok(()) => {
                        self.format_list = cooked;
                        return Ok(());
                    }
                    Err(e) => {
                        let mut msg = b"warning: $PS_FORMAT ignored. (".to_vec();
                        msg.extend_from_slice(&e);
                        msg.extend_from_slice(b")\n");
                        self.eprint(&msg);
                    }
                }
            }
        }

        if !self.format_list.is_empty() {
            if self.format_flags != 0 {
                return Err("conflicting format options".into());
            }
            if self.format_modifiers != 0 {
                return Err("can not use output modifiers with user-defined output".into());
            }
            if self.thread_flags & tf::MUST_USE != 0 {
                return Err("-L/-T with H/m/-m and -o/-O/o/O is nonsense".into());
            }
            return Ok(());
        }

        let spec: Option<&'static [u8]> = match self.format_flags {
            0 => None,
            f if f == ff::UF | ff::UL => self.sysv_fl_format,
            f if f == ff::UF => self.sysv_f_format,
            f if f == ff::UL => self.sysv_l_format,
            f if f == ff::UJ => self.sysv_j_format,
            f if f == ff::UJ | ff::UL => Some(b"RD_lj"),
            f if f == ff::UJ | ff::UF => Some(b"RD_fj"),
            f if f == ff::BJ => Some(self.bsd_j_format),
            f if f == ff::BL => Some(self.bsd_l_format),
            f if f == ff::BS => Some(self.bsd_s_format),
            f if f == ff::BU => Some(self.bsd_u_format),
            f if f == ff::BV => Some(self.bsd_v_format),
            f if f == ff::LX => Some(b"OL_X"),
            f if f == ff::LM => Some(b"OL_m"),
            f if f == ff::FC => Some(b"FLASK_context"),
            _ => return Err("conflicting format options".into()),
        };
        let Some(spec) = spec else {
            return self.generate_sysv_list().map_err(Vec::from);
        };
        self.format_list = self.do_one_spec(spec, None).unwrap_or_default();

        if self.format_modifiers & fm::J != 0 {
            let n = self.spec1(b"pgid");
            if !self.fmt_add_after(b"PPID", n.clone()) && !self.fmt_add_after(b"PID", n) {
                return Err("internal error: no PID or PPID for -j option".into());
            }
            let n = self.spec1(b"sid");
            if !self.fmt_add_after(b"PGID", n) {
                return Err("lost my PGID".into());
            }
        }
        if self.format_modifiers & fm::Y != 0 {
            self.fmt_delete(b"F");
            let n = self.spec1(b"rss");
            if self.fmt_add_after(b"ADDR", n) {
                self.fmt_delete(b"ADDR");
            }
        }
        if self.format_modifiers & fm::C != 0 {
            for name in [&b"%CPU"[..], b"CPU", b"CP", b"C", b"NI"] {
                self.fmt_delete(name);
            }
            let n = self.spec1(b"class");
            if !self.fmt_add_after(b"PRI", n) {
                return Err("internal error: no PRI for -c option".into());
            }
            self.fmt_delete(b"PRI");
            let n = self.spec1(b"pri");
            if !self.fmt_add_after(b"CLS", n) {
                return Err("lost my CLS".into());
            }
        }
        if self.thread_flags & tf::U_T != 0 {
            let n = self.spec1(b"spid");
            if !self.fmt_add_after(b"PID", n) && self.thread_flags & tf::MUST_USE != 0 {
                return Err("-T with H/-m/m but no PID for SPID to follow".into());
            }
        }
        if self.thread_flags & tf::U_L != 0 {
            let n = self.spec1(b"lwp");
            let added = [&b"SID"[..], b"SESS", b"PGID", b"PGRP", b"PPID", b"PID"]
                .into_iter()
                .any(|after| self.fmt_add_after(after, n.clone()));
            if !added && self.thread_flags & tf::MUST_USE != 0 {
                return Err("-L with H/-m/m but no PID/PGID/SID/SESS for NLWP to follow".into());
            }
            let n = self.spec1(b"nlwp");
            self.fmt_add_after(b"%CPU", n);
        }
        if self.format_modifiers & fm::M != 0 {
            let n = self.spec1(b"label");
            self.format_list.insert(0, n);
        }
        if self.personality & per::ZAP_ADDR != 0 && self.format_flags & ff::UL != 0 {
            let n = self.spec1(b"sgi_p");
            if self.fmt_add_after(b"ADDR", n) {
                self.fmt_delete(b"ADDR");
            }
        }
        if self.personality & per::SANE_USER != 0 && self.format_flags & ff::UF != 0 {
            let n = self.spec1(b"user");
            if self.fmt_add_after(b"UID", n) {
                self.fmt_delete(b"UID");
            }
        }
        Ok(())
    }
}
