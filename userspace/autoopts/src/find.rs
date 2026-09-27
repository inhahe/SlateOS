//! Finding options on the command line (`find.c`) and handling them
//! (`autoopts.c`, `init.c`): the immediate and regular passes, and the
//! lookup the rc-file reader shares with them.

use crate::charmap::strneqvcmp;
use crate::{Action, Callbacks, Exit, Options, pr, st};

/// `tSuccess`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Res {
    /// `SUCCESS`.
    Success,
    /// `FAILURE`: an error, reported unless errors are being ignored.
    Failure,
    /// `PROBLEM`: nothing more to do -- the end of the options, or an option
    /// that may not be preset.
    Problem,
}

/// `teOptType`: how the current option was named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptType {
    /// Not yet found.
    Undefined,
    /// By its flag character.
    Short,
    /// By its long name.
    Long,
}

/// `tOptState`: an option found but not yet handled.
#[derive(Clone, Debug)]
pub(crate) struct OSt {
    /// `pOD`: the descriptor's index.
    pub od: Option<usize>,
    /// `pzOptArg`.
    pub arg: Option<Vec<u8>>,
    /// How it is being set, and (once its argument is fetched) the
    /// descriptor's persistent bits.
    pub flags: u32,
    /// `optType`.
    pub typ: OptType,
}

impl OSt {
    /// `OPTSTATE_INITIALIZER(flags)`.
    pub(crate) fn new(flags: u32) -> Self {
        OSt {
            od: None,
            arg: None,
            flags,
            typ: OptType::Undefined,
        }
    }
}

/// `DO_IMMEDIATELY`: handled in the immediate pass.
fn do_immediately(f: u32) -> bool {
    f & (st::DISABLED | st::IMM) == st::IMM
        || f & (st::DISABLED | st::DISABLE_IMM) == (st::DISABLED | st::DISABLE_IMM)
}

/// `DO_NORMALLY`: handled in the regular pass.
fn do_normally(f: u32) -> bool {
    f & (st::DISABLED | st::IMM) == 0 || f & (st::DISABLED | st::DISABLE_IMM) == st::DISABLED
}

/// `DO_SECOND_TIME`: handled in both.
fn do_second_time(f: u32) -> bool {
    f & (st::DISABLED | st::TWICE) == st::TWICE
        || f & (st::DISABLED | st::DISABLE_TWICE) == (st::DISABLED | st::DISABLE_TWICE)
}

/// `name_buf`'s size in `opt_find_long`: a name this long is not a name.
const NAME_BUF: usize = 128;

/// Bytes joined, for a diagnostic built from pieces.
fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

impl Options {
    /// Whether errors are being reported (`OPTPROC_ERRSTOP`).
    pub(crate) fn errstop(&self) -> bool {
        self.set & pr::ERRSTOP != 0
    }

    /// `ao_initialize`: the presets, once.
    pub(crate) fn initialize(&mut self, cb: &mut dyn Callbacks) -> Result<bool, Exit> {
        if self.set & pr::INITDONE != 0 {
            return Ok(true);
        }
        self.set |= pr::INITDONE;
        if self.do_presets(cb)? != Res::Success {
            return Ok(false);
        }
        self.cur_idx = 1;
        self.cur_opt = None;
        Ok(true)
    }

    /// `do_presets`: the immediate pass, then the rc files unless
    /// `--no-load-opts` said not to.
    fn do_presets(&mut self, cb: &mut dyn Callbacks) -> Result<Res, Exit> {
        if self.immediate_opts(cb)? != Res::Success {
            return Ok(Res::Failure);
        }
        let load = self.prog.save_opts.saturating_add(1);
        let disabled = |o: &Options| {
            o.state
                .get(load)
                .is_some_and(|s| s.flags & st::DISABLED != 0)
        };
        if disabled(self) {
            return Ok(Res::Success);
        }
        self.set |= pr::PRESETTING;
        if !self.prog.home_list.is_empty() && !disabled(self) {
            let r = self.intern_file_load(cb);
            if r.is_err() {
                self.set &= !pr::PRESETTING;
            }
            r?;
        }
        self.set &= !pr::PRESETTING;
        Ok(Res::Success)
    }

    /// `immediate_opts`: the first pass over the command line, handling only
    /// the immediate options -- but finding, and so reporting errors in,
    /// every one.
    fn immediate_opts(&mut self, cb: &mut dyn Callbacks) -> Result<Res, Exit> {
        self.set |= pr::IMMEDIATE;
        self.cur_idx = 1;
        self.cur_opt = None;
        let res = loop {
            let mut o = OSt::new(st::PRESET);
            match self.next_opt(&mut o)? {
                Res::Failure => break Res::Failure,
                Res::Problem => {
                    self.set &= !pr::IMMEDIATE;
                    return Ok(Res::Success);
                }
                Res::Success => {}
            }
            if !do_immediately(o.flags) {
                continue;
            }
            if self.handle_opt(&o, cb)? != Res::Success {
                break Res::Success;
            }
        };
        if self.errstop() {
            return Err(self.option_usage(1));
        }
        self.set &= !pr::IMMEDIATE;
        Ok(res)
    }

    /// `regular_opts`: the second pass, handling everything that is not
    /// immediate.
    pub(crate) fn regular_opts(&mut self, cb: &mut dyn Callbacks) -> Result<Res, Exit> {
        loop {
            let mut o = OSt::new(st::DEFINED);
            match self.next_opt(&mut o)? {
                Res::Failure => break,
                Res::Problem => return Ok(Res::Success),
                Res::Success => {}
            }
            if !do_normally(o.flags) {
                if !do_second_time(o.flags) {
                    continue;
                }
                if let Some(s) = o.od.and_then(|i| self.state.get_mut(i)) {
                    s.occ = s.occ.wrapping_sub(1);
                }
            }
            if self.handle_opt(&o, cb)? != Res::Success {
                break;
            }
        }
        if self.errstop() {
            return Err(self.option_usage(1));
        }
        Ok(Res::Failure)
    }

    /// `next_opt`: find the next option and fetch its argument, without
    /// handling it.
    pub(crate) fn next_opt(&mut self, o: &mut OSt) -> Result<Res, Exit> {
        let res = self.find_opt(o)?;
        if res != Res::Success {
            return Ok(res);
        }
        let Some(od) = o.od else {
            return Ok(Res::Failure);
        };
        if o.flags & st::DEFINED != 0
            && self
                .state
                .get(od)
                .is_some_and(|s| s.flags & st::NO_COMMAND != 0)
        {
            let name = self.desc_name(od);
            ulclosestream::stderr_write(&cat(&[b"'", name, b"' is not a command line option.\n"]));
            return Ok(Res::Failure);
        }
        Ok(self.get_opt_arg(o))
    }

    /// A descriptor's long name, as bytes.
    pub(crate) fn desc_name(&self, od: usize) -> &'static [u8] {
        self.prog.descs.get(od).map_or(b"", |d| d.name.as_bytes())
    }

    /// The word `pzCurOpt` is in, from its offset on (`None` for NULL).
    fn cur_text(&self) -> Option<&[u8]> {
        let (w, off) = self.cur_opt?;
        self.args.get(w).map(|a| a.get(off..).unwrap_or_default())
    }

    /// `find_opt`: the option at the current position.
    fn find_opt(&mut self, o: &mut OSt) -> Result<Res, Exit> {
        if let Some(&c) = self.cur_text().and_then(<[u8]>::first) {
            return self.opt_find_short(c, o);
        }
        if self.cur_idx >= self.args.len() {
            return Ok(Res::Problem);
        }
        let w = self.cur_idx;
        let word = self.args.get(w).cloned().unwrap_or_default();
        // `*(pzCurOpt++) != '-'`: the pointer moves on even for an operand.
        self.cur_opt = Some((w, 1));
        if word.first() != Some(&b'-') {
            return Ok(Res::Problem);
        }
        if word.len() == 1 {
            return Ok(Res::Problem);
        }
        self.cur_idx = self.cur_idx.saturating_add(1);
        if word.get(1) == Some(&b'-') {
            self.cur_opt = Some((w, 2));
            if word.len() == 2 {
                return Ok(Res::Problem);
            }
            if self.set & pr::LONGOPT == 0 {
                let msg = cat(&[
                    &self.prog_path,
                    b": illegal option -- ",
                    &crate::shown(&word),
                    b"\n",
                ]);
                ulclosestream::stderr_write(&msg);
                return Ok(Res::Failure);
            }
            return self.opt_find_long(word.get(2..).unwrap_or_default(), o);
        }
        if self.set & pr::SHORTOPT != 0 {
            return self.opt_find_short(word.get(1).copied().unwrap_or(0), o);
        }
        self.opt_find_long(word.get(1..).unwrap_or_default(), o)
    }

    /// `opt_find_long`: the option `text` names, with an `=value` glued on
    /// or not.
    pub(crate) fn opt_find_long(&mut self, text: &[u8], o: &mut OSt) -> Result<Res, Exit> {
        // `parse_opt`: the name ends at `=`, and 128 bytes of it is too many.
        let eq = text.iter().position(|&b| b == b'=');
        let name_end = eq.unwrap_or(text.len());
        let (name, glued, too_long) = if name_end >= NAME_BUF {
            (text, None, true)
        } else {
            let name = text.get(..name_end).unwrap_or_default();
            let glued = eq.map(|e| text.get(e.saturating_add(1)..).unwrap_or_default().to_vec());
            (name, glued, false)
        };
        if too_long || name.len() <= 1 {
            if !self.errstop() {
                return Ok(Res::Failure);
            }
            let msg = cat(&[
                &self.prog_name,
                b": invalid option name: ",
                &crate::shown(name),
                b"\n",
            ]);
            ulclosestream::stderr_write(&msg);
            return Err(self.option_usage(1));
        }
        let (ct, idx, disable) = self.opt_match_ct(name);
        match ct {
            1 => self.opt_set(glued, idx, disable, o),
            0 => {
                if !self.errstop() {
                    return Ok(Res::Failure);
                }
                let msg = cat(&[
                    &self.prog_path,
                    b": illegal option -- ",
                    &crate::shown(name),
                    b"\n",
                ]);
                ulclosestream::stderr_write(&msg);
                Err(self.option_usage(1))
            }
            _ => {
                if !self.errstop() {
                    return Ok(Res::Failure);
                }
                let mut msg = cat(&[
                    &self.prog_path,
                    b": ambiguous option name: ",
                    &crate::shown(name),
                ]);
                msg.extend_from_slice(format!(" (matches {ct} options)\n").as_bytes());
                ulclosestream::stderr_write(&msg);
                if ct <= 4 {
                    self.opt_ambiguities(name);
                }
                Err(self.option_usage(1))
            }
        }
    }

    /// `opt_match_ct`: how many options `name` could mean, the last (or the
    /// exact) one's index, and whether a disablement name matched.
    fn opt_match_ct(&self, name: &[u8]) -> (usize, usize, bool) {
        let len = name.len();
        let mut ct = 0usize;
        let mut idx = 0usize;
        let mut disable = false;
        for (i, d) in self.prog.descs.iter().enumerate() {
            let flags = self.state.get(i).map_or(d.flags, |s| s.flags);
            if flags & st::IMMUTABLE_MASK != 0 && flags != (st::OMITTED | st::NO_INIT) {
                continue;
            }
            if strneqvcmp(name, d.name.as_bytes(), len) == 0 {
                if d.name.len() == len {
                    return (1, i, disable);
                }
            } else if let Some(dn) = d.disable_name
                && strneqvcmp(name, dn.as_bytes(), len) == 0
            {
                disable = true;
                if dn.len() == len {
                    return (1, i, disable);
                }
            } else {
                continue;
            }
            idx = i;
            ct = ct.saturating_add(1);
        }
        (ct, idx, disable)
    }

    /// `opt_ambiguities`: the options an ambiguous name matches.
    fn opt_ambiguities(&self, name: &[u8]) {
        ulclosestream::stderr_write(b"  The following options match:\n");
        for d in self.prog.descs {
            if strneqvcmp(name, d.name.as_bytes(), name.len()) == 0 {
                ulclosestream::stderr_write(&cat(&[b"  --", d.name.as_bytes(), b"\n"]));
            } else if let Some(dn) = d.disable_name
                && strneqvcmp(name, dn.as_bytes(), name.len()) == 0
            {
                ulclosestream::stderr_write(&cat(&[b"  --", dn.as_bytes(), b"\n"]));
            }
        }
    }

    /// `opt_set`: the long name matched option `idx`.
    fn opt_set(
        &mut self,
        glued: Option<Vec<u8>>,
        idx: usize,
        disable: bool,
        o: &mut OSt,
    ) -> Result<Res, Exit> {
        let flags = self.state.get(idx).map_or(0, |s| s.flags);
        if flags & st::IMMUTABLE_MASK != 0 {
            if !self.errstop() {
                return Ok(Res::Failure);
            }
            self.disabled_error(&self.prog_name.clone(), idx);
            return Err(self.option_usage(1));
        }
        if disable {
            o.flags |= st::DISABLED;
        }
        o.od = Some(idx);
        o.arg = glued;
        o.typ = OptType::Long;
        Ok(Res::Success)
    }

    /// `zDisabledErr`: a compiled-out option was named.
    fn disabled_error(&self, who: &[u8], idx: usize) {
        let d = self.prog.descs.get(idx);
        let mut msg = cat(&[
            who,
            b": The '",
            self.desc_name(idx),
            b"' option has been disabled.",
        ]);
        if let Some(d) = d
            && !d.text.is_empty()
        {
            msg.extend_from_slice(b" -- ");
            msg.extend_from_slice(d.text.as_bytes());
        }
        msg.push(b'\n');
        ulclosestream::stderr_write(&msg);
    }

    /// `opt_find_short`: the option whose flag character is `c`.
    pub(crate) fn opt_find_short(&mut self, c: u8, o: &mut OSt) -> Result<Res, Exit> {
        for (i, d) in self.prog.descs.iter().enumerate() {
            if d.value != c {
                continue;
            }
            let flags = self.state.get(i).map_or(d.flags, |s| s.flags);
            if flags & st::IMMUTABLE_MASK != 0 {
                if flags == (st::OMITTED | st::NO_INIT) {
                    if !self.errstop() {
                        return Ok(Res::Failure);
                    }
                    self.disabled_error(&self.prog_path.clone(), i);
                    return Err(self.option_usage(1));
                }
                break;
            }
            o.od = Some(i);
            o.typ = OptType::Short;
            return Ok(Res::Success);
        }
        if !self.errstop() {
            return Ok(Res::Failure);
        }
        let msg = cat(&[
            &self.prog_path,
            b": illegal option -- ",
            &crate::shown(&[c]),
            b"\n",
        ]);
        ulclosestream::stderr_write(&msg);
        Err(self.option_usage(1))
    }

    /// `get_opt_arg`: fetch the argument the option takes, if any.
    fn get_opt_arg(&mut self, o: &mut OSt) -> Res {
        let od = o.od.unwrap_or(0);
        o.flags |= self.state.get(od).map_or(0, |s| s.flags) & st::PERSISTENT_MASK;
        if o.flags & st::DISABLED != 0 || o.flags & st::ARG_TYPE_MASK == 0 {
            return self.get_opt_arg_none(o);
        }
        if o.flags & st::ARG_OPTIONAL != 0 {
            return self.get_opt_arg_may(o);
        }
        self.get_opt_arg_must(o)
    }

    /// Advance `pzCurOpt` by one byte within its word.
    fn cur_advance(&mut self) {
        if let Some((w, off)) = self.cur_opt {
            self.cur_opt = Some((w, off.saturating_add(1)));
        }
    }

    /// `get_opt_arg_must`: the argument is the rest of this word, or the
    /// next word whatever it is.
    fn get_opt_arg_must(&mut self, o: &mut OSt) -> Res {
        match o.typ {
            OptType::Short => {
                self.cur_advance();
                match self.cur_text() {
                    Some(rest) if !rest.is_empty() => o.arg = Some(rest.to_vec()),
                    _ => {
                        o.arg = self.args.get(self.cur_idx).cloned();
                        self.cur_idx = self.cur_idx.saturating_add(1);
                    }
                }
            }
            OptType::Long => {
                if o.arg.is_none() {
                    o.arg = self.args.get(self.cur_idx).cloned();
                    self.cur_idx = self.cur_idx.saturating_add(1);
                }
            }
            OptType::Undefined => {}
        }
        if self.cur_idx > self.args.len() {
            let od = o.od.unwrap_or(0);
            let msg = cat(&[
                &self.prog_path,
                b": The '",
                self.desc_name(od),
                b"' option requires an argument.\n",
            ]);
            ulclosestream::stderr_write(&msg);
            return Res::Failure;
        }
        self.cur_opt = None;
        Res::Success
    }

    /// The next word, as an optional argument: taken unless there is none or
    /// it starts with `-`.
    fn lookahead_arg(&mut self) -> Option<Vec<u8>> {
        match self.args.get(self.cur_idx) {
            Some(next) if next.first() != Some(&b'-') => {
                let next = next.clone();
                self.cur_idx = self.cur_idx.saturating_add(1);
                Some(next)
            }
            _ => None,
        }
    }

    /// `get_opt_arg_may`: an optional argument.
    fn get_opt_arg_may(&mut self, o: &mut OSt) -> Res {
        match o.typ {
            OptType::Short => {
                self.cur_advance();
                match self.cur_text() {
                    Some(rest) if !rest.is_empty() => o.arg = Some(rest.to_vec()),
                    _ => o.arg = self.lookahead_arg(),
                }
            }
            OptType::Long => {
                if o.arg.is_none() {
                    o.arg = self.lookahead_arg();
                }
            }
            OptType::Undefined => {}
        }
        self.cur_opt = None;
        Res::Success
    }

    /// `get_opt_arg_none`: no argument, and a long option may not be given
    /// one.
    fn get_opt_arg_none(&mut self, o: &mut OSt) -> Res {
        if o.typ == OptType::Short {
            self.cur_advance();
        } else if o.arg.is_some() {
            let od = o.od.unwrap_or(0);
            let msg = cat(&[
                &self.prog_path,
                b": The '",
                self.desc_name(od),
                b"' option cannot have an argument.\n",
            ]);
            ulclosestream::stderr_write(&msg);
            return Res::Failure;
        } else {
            self.cur_opt = None;
        }
        Res::Success
    }

    /// `handle_opt`: record the option and run its procedure.
    pub(crate) fn handle_opt(&mut self, o: &OSt, cb: &mut dyn Callbacks) -> Result<Res, Exit> {
        let Some(od) = o.od else {
            return Ok(Res::Failure);
        };
        let Some(state) = self.state.get_mut(od) else {
            return Ok(Res::Failure);
        };
        state.arg.clone_from(&o.arg);
        if self.set & pr::PRESETTING != 0 && state.flags & st::NO_INIT != 0 {
            return Ok(Res::Problem);
        }
        state.flags &= st::PERSISTENT_MASK;
        state.flags |= o.flags & !st::PERSISTENT_MASK;
        if state.flags & st::DEFINED != 0 {
            state.occ = state.occ.saturating_add(1);
            let max = self.prog.descs.get(od).map_or(1, |d| d.max);
            if state.occ > u32::from(max) {
                return self.too_many(od);
            }
        }
        match self.prog.descs.get(od).map_or(Action::None, |d| d.action) {
            Action::None => {}
            Action::PrintVersion => return Err(self.print_version(od)),
            Action::Usage => return Err(self.option_usage(0)),
            Action::PagedUsage => return Err(self.paged_usage()),
            Action::LoadOpt => self.load_opt(od, cb)?,
            Action::User => cb.option(self, od)?,
        }
        Ok(Res::Success)
    }

    /// `too_many_occurrences`.
    fn too_many(&mut self, od: usize) -> Result<Res, Exit> {
        if !self.errstop() {
            return Ok(Res::Failure);
        }
        let mut msg = cat(&[&self.prog_name, b" error:  only "]);
        ulclosestream::stderr_write(&msg);
        let max = self.prog.descs.get(od).map_or(1, |d| d.max);
        msg = if max > 1 {
            let mut m = format!("{max} ").into_bytes();
            m.extend_from_slice(self.desc_name(od));
            m.extend_from_slice(b" options allowed\n");
            m
        } else {
            cat(&[b"one ", self.desc_name(od), b" option allowed\n"])
        };
        ulclosestream::stderr_write(&msg);
        Err(self.option_usage(1))
    }
}
