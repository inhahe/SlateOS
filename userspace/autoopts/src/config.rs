//! Configuration files (`configfile.c`, `load.c`): the rc files named by the
//! home list, and the file `--load-opts` names.
//!
//! A file is read whole and scanned as one NUL-terminated string, entries
//! being cut out of it in place -- terminated, overwritten, cooked -- exactly
//! as the C does to its private mapping. Several of upstream's surprises
//! (listed in the crate docs) are consequences of those in-place edits, so
//! the buffer is edited the same way rather than parsed into something
//! tidier.
//!
//! Errors are never reported here: `OPTPROC_ERRSTOP` is cleared for the
//! length of a file, and what cannot be read or understood is skipped.

use std::fs;
use std::io;

use crate::charmap::{
    END_XML_TOKEN, LOAD_LINE_SKIP, LOWER_CASE, OPTION_NAME, VALUE_NAME, VAR_FIRST, WHITESPACE, at,
    brk, cstr, is, spn, spn_back, starts_with, strchr, strlen, strneqvcmp, strstr,
};
use crate::cook::{cook_xml_text, trim_quotes};
use crate::find::{OSt, Res};
use crate::{Callbacks, Exit, LoadMode, Options, bytes_os, getenv, pr, st};

/// Handling normal options: rc files forwards, then the command line.
pub(crate) const PROCESS: i32 = 1;
/// Handling immediate options: rc files backwards.
pub(crate) const PRESET: i32 = -1;
/// A file named by `--load-opts`.
pub(crate) const CALLED: i32 = 0;

/// `AG_PATH_MAX + 1`: the path buffers' size.
const PATH_BUF: usize = 4097;

/// What an XML-ish entry's value is, as its `type=` attribute says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValType {
    /// No value, or one that could not be understood.
    None,
    /// A string: the only kind that is cooked.
    String,
    /// Any other declared type.
    Other,
}

/// `text_mmap`: a regular file's contents, NUL-terminated, or `None` for one
/// that cannot be opened, is not a regular file, or is empty. Two NULs end
/// the buffer: `trim_xml_text` may overwrite the first.
fn load_text(path: &[u8]) -> Option<Vec<u8>> {
    let file = fs::File::open(bytes_os(path)).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    let mut text = Vec::new();
    io::Read::read_to_end(&mut io::Read::take(&file, meta.len()), &mut text).ok()?;
    if text.is_empty() {
        return None;
    }
    text.extend_from_slice(&[0, 0]);
    Some(text)
}

/// `direction_ok`: whether an entry is handled in this direction, by the
/// flags *of the entry's state* -- which carry no immediate bits, so that in
/// practice everything is handled forwards and nothing backwards.
fn direction_ok(f: u32, dir: i32) -> bool {
    if dir == CALLED {
        return true;
    }
    match f & (st::IMM | st::DISABLE_IMM) {
        0 => dir >= 0,
        st::IMM => {
            if dir < 0 {
                f & st::DISABLED != 0
            } else {
                f & st::DISABLED == 0
            }
        }
        st::DISABLE_IMM => {
            if dir < 0 {
                f & st::DISABLED == 0
            } else {
                f & st::DISABLED != 0
            }
        }
        _ => dir <= 0,
    }
}

/// `assemble_arg_val`: end the name at the first of ` \t\n:=` and return
/// where the value starts -- past whitespace, and past one `:` or `=` if the
/// name ended in whitespace.
fn assemble_arg_val(s: &mut [u8], txt: usize, mode: LoadMode) -> usize {
    let len = strlen(s, txt);
    let Some(end) = s
        .get(txt..txt.saturating_add(len))
        .and_then(|w| w.iter().position(|b| b" \t\n:=".contains(b)))
        .map(|p| txt.saturating_add(p))
    else {
        return txt.saturating_add(len);
    };
    if mode == LoadMode::Keep {
        if let Some(b) = s.get_mut(end) {
            *b = 0;
        }
        return end.saturating_add(1);
    }
    let space_break = is(at(s, end), WHITESPACE);
    if let Some(b) = s.get_mut(end) {
        *b = 0;
    }
    let mut p = spn(s, end.saturating_add(1), WHITESPACE);
    if space_break && matches!(at(s, p), b':' | b'=') {
        p = spn(s, p.saturating_add(1), WHITESPACE);
    }
    p
}

/// `skip_unkn`: past an attribute that is not understood, to the next
/// token end; `None` at the end of the text.
fn skip_unkn(s: &[u8], i: usize) -> Option<usize> {
    let p = brk(s, i, END_XML_TOKEN);
    (at(s, p) != 0).then_some(p)
}

/// `parse_value`: a `type=NAME` attribute's value.
fn parse_value(s: &[u8], i: usize, typ: &mut ValType) -> Option<usize> {
    let mut p = i;
    let first = at(s, p);
    p = p.saturating_add(1);
    if first != b'=' {
        *typ = ValType::None;
        return skip_unkn(s, p);
    }
    let len = spn(s, p, OPTION_NAME).saturating_sub(p);
    let word = s.get(p..p.saturating_add(len)).unwrap_or_default();
    let known = matches!(
        word,
        b"string"
            | b"integer"
            | b"bool"
            | b"boolean"
            | b"keyword"
            | b"set"
            | b"set-membership"
            | b"nested"
            | b"hierarchy"
    );
    if len == 0 || !is(at(s, p.saturating_add(len)), END_XML_TOKEN) || !known {
        *typ = ValType::None;
        return skip_unkn(s, p.saturating_add(len));
    }
    *typ = if word == b"string" {
        ValType::String
    } else {
        ValType::Other
    };
    Some(p.saturating_add(len))
}

/// `parse_attrs`: an XML-ish entry's attributes, up to its `>` or `/`.
fn parse_attrs(s: &[u8], mut p: usize, mode: &mut LoadMode, typ: &mut ValType) -> Option<usize> {
    loop {
        let len = spn(s, p, LOWER_CASE).saturating_sub(p);
        let word = s.get(p..p.saturating_add(len)).unwrap_or_default();
        let set_mode = match word {
            b"type" => {
                p = parse_value(s, p.saturating_add(len), typ)?;
                None
            }
            // `words=` and `members=` apply to keyword and set-membership
            // options, which sharutils has none of; libopts skips them too.
            b"words" | b"members" => {
                p = skip_unkn(s, p.saturating_add(len))?;
                None
            }
            b"cooked" => Some(LoadMode::Cooked),
            b"uncooked" => Some(LoadMode::Uncooked),
            b"keep" => Some(LoadMode::Keep),
            _ => {
                *typ = ValType::None;
                return skip_unkn(s, p);
            }
        };
        if let Some(m) = set_mode {
            p = p.saturating_add(len);
            if !is(at(s, p), END_XML_TOKEN) {
                *typ = ValType::None;
                return skip_unkn(s, p);
            }
            *mode = m;
        }
        p = spn(s, p, WHITESPACE);
        match at(s, p) {
            b'/' => {
                *typ = ValType::None;
                return Some(p);
            }
            b'>' => return Some(p),
            _ => {}
        }
        if !is(at(s, p), LOWER_CASE) {
            return None;
        }
    }
}

/// `trim_xml_text`: find `</NAME>` after the value at `intxt`, end the value
/// there (trimming trailing whitespace unless uncooked), and return what
/// follows the closing tag. The value's first byte is overwritten with a
/// space first, as upstream does.
fn trim_xml_text(s: &mut [u8], intxt: usize, name: &[u8], mode: LoadMode) -> Option<usize> {
    let mut pat = b"</".to_vec();
    pat.extend_from_slice(name);
    pat.push(b'>');
    if let Some(b) = s.get_mut(intxt) {
        *b = b' ';
    }
    let etext = strstr(s, intxt, &pat)?;
    let result = etext.saturating_add(pat.len());
    let etext = if mode == LoadMode::Uncooked {
        etext
    } else {
        spn_back(s, intxt, etext, WHITESPACE)
    };
    if let Some(b) = s.get_mut(etext) {
        *b = 0;
    }
    Some(result)
}

impl Options {
    /// `intern_file_load`: each home-list entry's rc file, highest priority
    /// first in the presetting direction, then lowest first in the
    /// processing one.
    pub(crate) fn intern_file_load(&mut self, cb: &mut dyn Callbacks) -> Result<(), Exit> {
        let list = self.prog.home_list;
        if list.is_empty() {
            return Ok(());
        }
        let saved = self.set;
        self.set &= !pr::ERRSTOP;
        let mut idx: isize = isize::try_from(list.len())
            .unwrap_or(isize::MAX)
            .saturating_sub(1);
        let mut inc = PRESET;
        loop {
            if idx < 0 {
                inc = PROCESS;
                idx = 0;
            }
            let Some(path) = usize::try_from(idx).ok().and_then(|i| list.get(i)) else {
                break;
            };
            idx = idx.saturating_add(isize::try_from(inc).unwrap_or(1));
            let prog_path = self.prog_path.clone();
            let Some(mut fname) = make_path(PATH_BUF, path.as_bytes(), &prog_path) else {
                continue;
            };
            let Ok(meta) = fs::metadata(bytes_os(&fname)) else {
                continue;
            };
            if meta.is_dir() {
                let rc = self.prog.rc_name.as_bytes();
                if fname
                    .len()
                    .saturating_add(1)
                    .saturating_add(rc.len().saturating_add(1))
                    >= PATH_BUF
                {
                    continue;
                }
                if fname.last() != Some(&b'/') {
                    fname.push(b'/');
                }
                fname.extend_from_slice(rc);
            }
            let r = self.file_preset(&fname, inc, cb);
            if r.is_err() {
                self.set = saved;
                r?;
            }
            let load = self.prog.save_opts.saturating_add(1);
            let disabled = self
                .state
                .get(load)
                .is_some_and(|s| s.flags & st::DISABLED != 0);
            if disabled && inc < 0 {
                idx = idx.saturating_sub(isize::try_from(inc).unwrap_or(-1));
                inc = PROCESS;
            }
        }
        self.set = saved;
        Ok(())
    }

    /// `optionLoadOpt`: `--load-opts FILE`, read now -- unless the option is
    /// being disabled, which was dealt with in the immediate pass.
    pub(crate) fn load_opt(&mut self, od: usize, cb: &mut dyn Callbacks) -> Result<(), Exit> {
        let Some(state) = self.state.get(od) else {
            return Ok(());
        };
        if state.flags & (st::DISABLED | st::RESET) != 0 {
            return Ok(());
        }
        let name = state.arg.clone().unwrap_or_default();
        let err = match fs::metadata(bytes_os(&name)) {
            Ok(meta) if meta.is_file() => None,
            Ok(_) => Some(io::Error::from_raw_os_error(EINVAL)),
            Err(e) => Some(e),
        };
        if let Some(e) = err {
            if !self.errstop() {
                return Ok(());
            }
            return Err(self.fserr_exit(b"stat", &name, &e));
        }
        self.file_preset(&name, CALLED, cb)
    }

    /// `file_preset`: one configuration file, in direction `dir`.
    fn file_preset(&mut self, fname: &[u8], dir: i32, cb: &mut dyn Callbacks) -> Result<(), Exit> {
        let Some(mut text) = load_text(fname) else {
            return Ok(());
        };
        let saved = self.set;
        self.set &= !pr::ERRSTOP;
        let mut flags = st::PRESET;
        let mut dir = dir;
        if dir == CALLED {
            flags = st::DEFINED;
            dir = PROCESS;
        }
        if self.set & pr::PRESETTING == 0 {
            flags = st::SET;
        }
        let r = self.scan_file(&mut text, flags, dir, cb);
        self.set = saved;
        r
    }

    /// The loop of `file_preset`: entry after entry until the text ends or
    /// stops making sense.
    fn scan_file(
        &mut self,
        s: &mut [u8],
        flags: u32,
        dir: i32,
        cb: &mut dyn Callbacks,
    ) -> Result<(), Exit> {
        let mut o = OSt::new(flags);
        let mut pos = Some(0usize);
        while let Some(p) = pos {
            o.flags = flags;
            let p = spn(s, p, WHITESPACE);
            let c = at(s, p);
            if is(c, VAR_FIRST) {
                pos = self.handle_cfg(s, p, &mut o, dir, cb)?;
                continue;
            }
            match c {
                b'<' => {
                    let c1 = at(s, p.saturating_add(1));
                    pos = if is(c1, VAR_FIRST) {
                        self.handle_struct(s, p, &mut o, dir, cb)?
                    } else {
                        match c1 {
                            b'?' => self.handle_directive(s, p),
                            b'!' => strstr(s, p, b"-->").map(|e| e.saturating_add(3)),
                            b'/' => {
                                strchr(s, p.saturating_add(2), b'>').map(|e| e.saturating_add(1))
                            }
                            _ => None,
                        }
                    };
                    if pos.is_none() {
                        break;
                    }
                }
                b'[' => pos = self.handle_section(s, p),
                b'#' => pos = strchr(s, p.saturating_add(1), b'\n'),
                _ => break,
            }
        }
        Ok(())
    }

    /// `handle_cfg`: a `name value` line.
    fn handle_cfg(
        &mut self,
        s: &mut [u8],
        name: usize,
        o: &mut OSt,
        dir: i32,
        cb: &mut dyn Callbacks,
    ) -> Result<Option<usize>, Exit> {
        let mut txt = name.saturating_add(1);
        let Some(mut end) = strchr(s, txt, b'\n') else {
            return Ok(Some(txt.saturating_add(strlen(s, txt))));
        };
        txt = spn(s, txt, VALUE_NAME);
        txt = spn(s, txt, WHITESPACE);
        let mut name_only = txt > end;
        if !name_only {
            if matches!(at(s, txt), b'=' | b':') {
                txt = spn(s, txt.saturating_add(1), WHITESPACE);
                name_only = txt > end;
            } else if !is(at(s, txt.saturating_sub(1)), WHITESPACE) {
                return Ok(None);
            }
        }
        if name_only {
            if let Some(b) = s.get_mut(end) {
                *b = 0;
            }
            self.load_opt_line(s, name, o, dir, LoadMode::Uncooked, cb)?;
            return Ok(Some(end.saturating_add(1)));
        }
        let next = if at(s, end.saturating_sub(1)) == b'\\' {
            // Meant to join the next line on; it starts copying *at* the
            // newline, so all it does is drop the backslash.
            let mut d = end.saturating_sub(1);
            let mut src = end;
            loop {
                let mut ch = at(s, src);
                src = src.saturating_add(1);
                match ch {
                    0 | b'\n' => {
                        if let Some(b) = s.get_mut(d) {
                            *b = 0;
                        }
                        break if ch == 0 { None } else { Some(src) };
                    }
                    b'\\' => {
                        if at(s, src) == b'\n' {
                            ch = b'\n';
                            src = src.saturating_add(1);
                        }
                    }
                    _ => {}
                }
                if let Some(b) = s.get_mut(d) {
                    *b = ch;
                }
                d = d.saturating_add(1);
            }
        } else {
            if let Some(b) = s.get_mut(end) {
                *b = 0;
            }
            end = end.saturating_add(1);
            Some(end)
        };
        self.load_opt_line(s, name, o, dir, LoadMode::Uncooked, cb)?;
        Ok(next)
    }

    /// `handle_struct`: `<name>value</name>`, `<name/>`, or either with
    /// attributes.
    fn handle_struct(
        &mut self,
        s: &mut [u8],
        lt: usize,
        o: &mut OSt,
        dir: i32,
        cb: &mut dyn Callbacks,
    ) -> Result<Option<usize>, Exit> {
        let mut mode = self.load_mode;
        let mut typ = ValType::String;
        let name = lt.saturating_add(1);
        let mut txt = spn(s, name, VALUE_NAME);
        let nul_point = txt;
        let mut self_closing = false;
        match at(s, txt) {
            b' ' | b'\t' => {
                let p = spn(s, txt, WHITESPACE);
                let Some(p) = parse_attrs(s, p, &mut mode, &mut typ) else {
                    return Ok(None);
                };
                txt = p;
                match at(s, txt) {
                    b'>' => {}
                    b'/' => self_closing = true,
                    _ => return Ok(None),
                }
            }
            b'/' => self_closing = true,
            b'>' => {}
            _ => return Ok(strchr(s, txt, b'>').map(|e| e.saturating_add(1))),
        }
        if self_closing {
            if at(s, txt.saturating_add(1)) != b'>' {
                return Ok(None);
            }
            if let Some(b) = s.get_mut(txt) {
                *b = 0;
            }
            self.load_opt_line(s, name, o, dir, mode, cb)?;
            return Ok(Some(txt.saturating_add(2)));
        }
        if let Some(b) = s.get_mut(nul_point) {
            *b = 0;
        }
        let data = txt.saturating_add(1);
        let tag = cstr(s, name);
        let Some(after) = trim_xml_text(s, data, &tag, mode) else {
            return Ok(None);
        };
        for b in s.get_mut(nul_point..data).unwrap_or_default() {
            *b = b' ';
        }
        if typ == ValType::String && mode == LoadMode::Cooked {
            cook_xml_text(s, data);
        }
        self.load_opt_line(s, name, o, dir, mode, cb)?;
        Ok(Some(after))
    }

    /// `handle_directive`: `<?program NAME>` and `<?auto-options FLAGS>`;
    /// any other `<?...>` is skipped.
    fn handle_directive(&mut self, s: &[u8], lt: usize) -> Option<usize> {
        let after = lt.saturating_add(2);
        for (word, is_program) in [(&b"program"[..], true), (&b"auto-options"[..], false)] {
            if starts_with(s, after, word)
                && !is(at(s, after.saturating_add(word.len())), VALUE_NAME)
            {
                let at_end = after.saturating_add(word.len());
                return if is_program {
                    self.program_directive(s, at_end)
                } else {
                    self.aoflags_directive(s, at_end)
                };
            }
        }
        strchr(s, after, b'>').map(|e| e.saturating_add(1))
    }

    /// `aoflags_directive`: `AUTOOPTS_USAGE` words, from the file.
    fn aoflags_directive(&mut self, s: &[u8], txt: usize) -> Option<usize> {
        let pz = spn(s, txt.saturating_add(1), WHITESPACE);
        let end = strchr(s, pz, b'>')?;
        let words = s.get(pz..end).unwrap_or_default().to_vec();
        self.set_usage_flags(Some(&words));
        Some(end.saturating_add(1))
    }

    /// `program_directive`: skip to the section for this program. Only the
    /// first directive's name is compared: the search for the next one lands
    /// on its `<`, and the name is then read from the `?` after it.
    fn program_directive(&mut self, s: &[u8], txt: usize) -> Option<usize> {
        let name = self.prog_name.clone();
        let mut txt = txt;
        loop {
            txt = spn(s, txt.saturating_add(1), WHITESPACE);
            let tail = s.get(txt..).unwrap_or_default();
            if strneqvcmp(tail, &name, name.len()) == 0
                && is(at(s, txt.saturating_add(name.len())), END_XML_TOKEN)
            {
                txt = txt.saturating_add(name.len());
                break;
            }
            txt = strstr(s, txt, b"<?program")?;
        }
        loop {
            let c = at(s, txt);
            if c == 0 {
                return None;
            }
            txt = txt.saturating_add(1);
            if c == b'>' {
                return Some(txt);
            }
        }
    }

    /// `handle_section`: `[NAME]` sections; another program's is skipped, up
    /// to this program's or the end.
    fn handle_section(&mut self, s: &[u8], lb: usize) -> Option<usize> {
        let upper = self.prog.upper.as_bytes();
        let len = upper.len();
        if starts_with(s, lb.saturating_add(1), upper)
            && at(s, lb.saturating_add(len).saturating_add(1)) == b']'
        {
            return strchr(s, lb.saturating_add(len).saturating_add(2), b'\n');
        }
        if len > 16 {
            return None;
        }
        let mut z = b"[".to_vec();
        z.extend_from_slice(upper);
        z.push(b']');
        let found = strstr(s, lb, &z)?;
        strchr(s, found, b'\n')
    }

    /// `load_opt_line`: one entry, `name` through its NUL, as an option.
    fn load_opt_line(
        &mut self,
        s: &mut [u8],
        line: usize,
        o: &mut OSt,
        dir: i32,
        mode: LoadMode,
        cb: &mut dyn Callbacks,
    ) -> Result<(), Exit> {
        let line = spn(s, line, LOAD_LINE_SKIP);
        let arg = assemble_arg_val(s, line, mode);
        if is(at(s, line.saturating_add(1)), OPTION_NAME) {
            let name = cstr(s, line);
            if self.opt_find_long(&name, o)? != Res::Success {
                return Ok(());
            }
        } else if self.opt_find_short(at(s, line), o)? != Res::Success {
            return Ok(());
        }
        if dir != CALLED && o.flags & st::NO_INIT != 0 {
            return Ok(());
        }
        trim_quotes(s, arg);
        let value = cstr(s, arg);
        o.arg = Some(value);
        if !direction_ok(o.flags, dir) {
            return Ok(());
        }
        let od = o.od.unwrap_or(0);
        let flags = self.state.get(od).map_or(0, |x| x.flags);
        let empty = o.arg.as_ref().is_none_or(Vec::is_empty);
        if flags & st::ARG_TYPE_MASK == 0 {
            if !empty {
                return Ok(());
            }
            o.arg = None;
        } else if flags & st::ARG_OPTIONAL != 0 {
            if empty {
                o.arg = None;
            }
        } else if empty {
            o.arg = Some(Vec::new());
        }
        let saved = self.load_mode;
        self.load_mode = mode;
        let r = self.handle_opt(o, cb);
        self.load_mode = saved;
        r.map(|_| ())
    }

    /// `fserr_exit`: "PROG error ERRNO (REASON) calling OP for 'NAME'", exit 1.
    pub(crate) fn fserr_exit(&self, op: &[u8], fname: &[u8], err: &io::Error) -> Exit {
        let mut msg = self.prog_name.clone();
        msg.extend_from_slice(
            format!(
                " error {} ({}) calling ",
                err.raw_os_error().unwrap_or(0),
                errmsg::strerror(err)
            )
            .as_bytes(),
        );
        msg.extend_from_slice(op);
        msg.extend_from_slice(b" for ");
        msg.extend_from_slice(&crate::shown_in_quotes(fname));
        msg.push(b'\n');
        ulclosestream::stderr_write(&msg);
        Exit(1)
    }
}

/// `EINVAL`, on Linux and in the SlateOS C library.
const EINVAL: i32 = 22;

/// `optionMakePath`: a home-list entry as a real path. `$NAME...` takes the
/// variable's value; `$$` is the program's directory; `$@` would be the
/// package data directory, which sharutils does not have. The result is
/// canonical -- and a name that does not exist is not one.
pub(crate) fn make_path(buf_size: usize, fname: &[u8], prog_path: &[u8]) -> Option<Vec<u8>> {
    if fname.is_empty() || fname.len() >= buf_size {
        return None;
    }
    let raw = if fname.first() != Some(&b'$') {
        fname.to_vec()
    } else {
        match fname.get(1) {
            None => return None,
            Some(b'$') => add_prog_path(buf_size, fname, prog_path)?,
            Some(b'@') => return None,
            Some(_) => add_env_val(buf_size, fname)?,
        }
    };
    let real = fs::canonicalize(bytes_os(&raw)).ok()?;
    #[cfg_attr(unix, allow(unused_mut))]
    let mut real = crate::os_bytes(real.as_os_str());
    // The Windows development host canonicalises to a `\\?\` verbatim path,
    // in which `/` is not a separator -- and the rc file's name is joined on
    // with one. The plain form names the same file.
    #[cfg(not(unix))]
    if let Some(plain) = real.strip_prefix(br"\\?\") {
        real = plain.to_vec();
    }
    (real.len() < buf_size).then_some(real)
}

/// `add_prog_path`: `$$` or `$$/rest`, relative to the program's directory.
fn add_prog_path(buf_size: usize, fname: &[u8], prog_path: &[u8]) -> Option<Vec<u8>> {
    let skip = match fname.get(2) {
        Some(b'/') => 3,
        None => 2,
        Some(_) => return None,
    };
    let found;
    let path: &[u8] = if prog_path.contains(&b'/') {
        prog_path
    } else {
        let search = getenv(b"PATH");
        found = crate::pathfind::pathfind(search.as_deref(), prog_path)?;
        &found
    };
    let slash = path.iter().rposition(|&b| b == b'/')?;
    let rest = fname.get(skip..).unwrap_or_default();
    if slash.saturating_add(1).saturating_add(rest.len()) >= buf_size {
        return None;
    }
    let mut out = path.get(..=slash)?.to_vec();
    out.extend_from_slice(rest);
    Some(out)
}

/// `add_env_val`: `$NAME` and whatever follows the name.
fn add_env_val(buf_size: usize, fname: &[u8]) -> Option<Vec<u8>> {
    let name_len = fname
        .iter()
        .skip(1)
        .take_while(|&&b| is(b, VALUE_NAME))
        .count();
    if name_len == 0 {
        return None;
    }
    let var = fname.get(1..name_len.saturating_add(1))?;
    let value = getenv(var)?;
    let rest = fname.get(name_len.saturating_add(1)..).unwrap_or_default();
    if value.len().saturating_add(1).saturating_add(rest.len()) >= buf_size {
        return None;
    }
    let mut out = value;
    out.extend_from_slice(rest);
    Some(out)
}
