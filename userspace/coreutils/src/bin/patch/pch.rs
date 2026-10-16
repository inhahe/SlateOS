//! `pch.c`: reading the patch -- finding the next diff in it and what kind
//! it is (`intuit_diff_type`), deciding which file it is for, and reading it
//! one hunk at a time (`another_hunk`) into the arrays the rest of `patch`
//! applies: each line's text, and its control character (`-`, `+`, `!`,
//! ` `, `*` and `=` for the two header lines, `^` after the last).
//!
//! The patch is read whole at the start. A regular file is read from where
//! its descriptor stood (a patch on standard input need not start at its
//! beginning); anything else -- a pipe, a terminal -- is copied to a
//! temporary file first, as upstream does, so that it can be read again.
//! Positions in it are the file's own offsets, which is what the one message
//! that prints one ("Prereq: with multiple words at line N", where N is a
//! byte offset -- upstream's slip, kept) shows.
//!
//! Upstream's slips in reading hunks are kept too, since a patch that meets
//! one applies differently with and without them: a context line led by a
//! tab is stored without its last byte; under `-l` a one-space context line
//! is stored as a newline and a NUL; `pget_line`'s count of `- ` prefixes to
//! strip is not reset after a comment line; and "\ No newline at end of
//! file" is recognised only unindented.
//!
//! Line numbers are upstream's `lin`, and their arithmetic is upstream's,
//! bounded where upstream bounds it (the `LINENUM_MAX` tests); where upstream
//! does not, it wraps here as the C does in practice.

use crate::sys::{self, Stat, Timespec, errno, oflag};
use crate::util::{self, cstr, say};
use crate::{Ctx, Diff, INDEX, Lin, NEW, NONE, OLD, Output, TMP_ED, TMP_PAT, Verbosity};

/// `INITHUNKMAX`: the hunk arrays' first size.
const INITHUNKMAX: Lin = 125;
/// `LINENUM_MAX`.
const LINENUM_MAX: Lin = Lin::MAX;
/// `EDITOR_PROGRAM`.
const EDITOR_PROGRAM: &str = "ed";
/// `bufsize`'s first value: the line buffer, 8 KiB.
pub const INITIAL_BUFSIZE: usize = 8 * 1024;

/// `ISSPACE` in the C locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// `s[k]`, with the NUL that ends every C string past the end.
fn at(s: &[u8], k: usize) -> u8 {
    s.get(k).copied().unwrap_or(0)
}

/// `strncmp (s + k, lit, n) == 0` for a literal without NULs.
fn str_n_eq(s: &[u8], k: usize, lit: &[u8]) -> bool {
    lit.iter()
        .enumerate()
        .all(|(i, &c)| at(s, k.saturating_add(i)) == c)
}

/// The patch, and everything `pch.c` keeps about the part of it being read.
#[derive(Debug)]
pub struct Pch {
    /// The patch file's bytes from `base` on.
    data: Vec<u8>,
    /// The file offset `data` starts at.
    base: u64,
    /// `file_tell (pfp)`.
    pos: u64,
    /// 0 for existent and nonempty, 1 for existent and probably (but not
    /// necessarily) empty, 2 for nonexistent; `[OLD]`, `[NEW]`.
    p_says_nonexistent: [i32; 2],
    /// RFC 934 nesting level: how many `- ` to strip from each line.
    p_rfc934_nesting: i32,
    /// File names in the patch headers: old, new, index.
    p_name: [Option<Vec<u8>>; 3],
    /// The dangerous names already reported, two at most.
    invalid_names: Vec<Vec<u8>>,
    p_copy: [bool; 2],
    p_rename: [bool; 2],
    /// The headers' timestamps as text.
    p_timestr: [Option<Vec<u8>>; 2],
    /// The `index` line's object names.
    p_sha1: [Option<Vec<u8>>; 2],
    /// The file modes a git diff gives.
    p_mode: [u32; 2],
    /// The size of the patch file.
    p_filesize: u64,
    p_first: Lin,
    p_newfirst: Lin,
    p_ptrn_lines: Lin,
    p_repl_lines: Lin,
    /// The last line of the hunk; -1 with none.
    p_end: Lin,
    /// The most `p_end` may become.
    p_max: Lin,
    p_prefix_context: Lin,
    p_suffix_context: Lin,
    /// The line of the patch just read.
    p_input_line: Lin,
    /// The hunk's lines, each exactly its `p_len` bytes.
    p_line: Vec<Vec<u8>>,
    /// The hunk's control characters.
    p_char: Vec<u8>,
    /// The arrays' size; it only grows, and a hunk that reaches it is
    /// "unterminated".
    hunkmax: Lin,
    /// How far the patch is indented.
    p_indent: usize,
    p_strip_trailing_cr: bool,
    /// Whether `#` lines are text (inside an ed script) or comments.
    p_pass_comments_through: bool,
    /// Where to intuit next time, and its line number.
    p_base: u64,
    p_bline: Lin,
    /// Where intuit found a patch, and its line number.
    p_start: u64,
    p_sline: Lin,
    /// The line the current hunk began on.
    p_hunk_beg: Lin,
    /// The C function a hunk is in, as its header names it.
    p_c_function: Option<Vec<u8>>,
    p_git_diff: bool,
    /// The headers' timestamps; `sec == -1` for none.
    pub p_timestamp: [Timespec; 2],
}

impl Default for Pch {
    fn default() -> Self {
        let n = usize::try_from(INITHUNKMAX).unwrap_or(125);
        Self {
            data: Vec::new(),
            base: 0,
            pos: 0,
            p_says_nonexistent: [0, 0],
            p_rfc934_nesting: 0,
            p_name: [None, None, None],
            invalid_names: Vec::new(),
            p_copy: [false, false],
            p_rename: [false, false],
            p_timestr: [None, None],
            p_sha1: [None, None],
            p_mode: [0, 0],
            p_filesize: 0,
            p_first: 0,
            p_newfirst: 0,
            p_ptrn_lines: 0,
            p_repl_lines: 0,
            p_end: -1,
            p_max: 0,
            p_prefix_context: 0,
            p_suffix_context: 0,
            p_input_line: 0,
            p_line: vec![Vec::new(); n],
            p_char: vec![0; n],
            hunkmax: INITHUNKMAX,
            p_indent: 0,
            p_strip_trailing_cr: false,
            p_pass_comments_through: false,
            p_base: 0,
            p_bline: 0,
            p_start: 0,
            p_sline: 0,
            p_hunk_beg: 0,
            p_c_function: None,
            p_git_diff: false,
            p_timestamp: [Timespec { sec: -1, nsec: 0 }, Timespec { sec: -1, nsec: 0 }],
        }
    }
}

impl Pch {
    fn idx(i: Lin) -> Option<usize> {
        usize::try_from(i).ok()
    }

    /// `p_line[i]`, `p_len[i]` long.
    fn line(&self, i: Lin) -> &[u8] {
        Self::idx(i)
            .and_then(|i| self.p_line.get(i))
            .map_or(&[], Vec::as_slice)
    }

    fn set_line(&mut self, i: Lin, v: Vec<u8>) {
        if let Some(slot) = Self::idx(i).and_then(|i| self.p_line.get_mut(i)) {
            *slot = v;
        }
    }

    /// `p_Char[i]`.
    fn ch(&self, i: Lin) -> u8 {
        Self::idx(i)
            .and_then(|i| self.p_char.get(i))
            .copied()
            .unwrap_or(0)
    }

    fn set_ch(&mut self, i: Lin, c: u8) {
        if let Some(slot) = Self::idx(i).and_then(|i| self.p_char.get_mut(i)) {
            *slot = c;
        }
    }

    /// Line `i`'s text, control character and length, moved to `j`.
    fn copy_entry(&mut self, from: Lin, to: Lin) {
        let line = self.line(from).to_vec();
        let c = self.ch(from);
        self.set_line(to, line);
        self.set_ch(to, c);
    }

    /// `grow_hunkmax`: the arrays doubled; false when the memory cannot be
    /// had.
    fn grow_hunkmax(&mut self) -> bool {
        let Some(max) = self.hunkmax.checked_mul(2) else {
            return false;
        };
        let Some(n) = Self::idx(max) else {
            return false;
        };
        let more = n.saturating_sub(self.p_line.len());
        if self.p_line.try_reserve_exact(more).is_err()
            || self.p_char.try_reserve_exact(more).is_err()
        {
            return false;
        }
        self.hunkmax = max;
        self.p_line.resize(n, Vec::new());
        self.p_char.resize(n, 0);
        true
    }
}

/// `fetchmode`: the six octal digits a git header gives a mode in, then the
/// end of the line; 0 for anything else.
fn fetchmode(s: &[u8]) -> u32 {
    let mut k = 0usize;
    while is_space(at(s, k)) {
        k = k.saturating_add(1);
    }
    let start = k;
    let mut mode = 0u32;
    while k < start.saturating_add(6) {
        let c = at(s, k);
        if (b'0'..=b'7').contains(&c) {
            mode = (mode << 3).wrapping_add(u32::from(c.wrapping_sub(b'0')));
        } else {
            mode = 0;
            break;
        }
        k = k.saturating_add(1);
    }
    if at(s, k) == b'\r' {
        k = k.saturating_add(1);
    }
    if at(s, k) != b'\n' {
        mode = 0;
    }
    mode
}

/// `sha1_says_nonexistent`: 2 for an all-zero object name (no file), 1 for a
/// prefix of the empty blob's (an empty file), 0 otherwise.
fn sha1_says_nonexistent(sha1: &[u8]) -> i32 {
    const EMPTY_SHA1: &[u8] = b"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    if sha1.iter().all(|&c| c == b'0') {
        return 2;
    }
    i32::from(EMPTY_SHA1.starts_with(sha1))
}

/// `skip_hex_digits`: where the lowercase hex digits at `k` end, or `None`
/// when there are none.
fn skip_hex_digits(s: &[u8], k: usize) -> Option<usize> {
    let mut e = k;
    while matches!(at(s, e), b'0'..=b'9' | b'a'..=b'f') {
        e = e.saturating_add(1);
    }
    (e != k).then_some(e)
}

/// `skip_spaces`.
fn skip_spaces(s: &[u8], mut k: usize) -> usize {
    while is_space(at(s, k)) {
        k = k.saturating_add(1);
    }
    k
}

/// `get_ed_command_letter`: the command a line is, if it is one of the ed
/// commands a diff produces (`a`, `i`, `c`, `d`, and `s/.//`), else 0.
pub fn get_ed_command_letter(line: &[u8]) -> u8 {
    let mut p = 0usize;
    let mut pair = false;
    if is_digit(at(line, p)) {
        p = p.saturating_add(1);
        while is_digit(at(line, p)) {
            p = p.saturating_add(1);
        }
        if at(line, p) == b',' {
            p = p.saturating_add(1);
            if !is_digit(at(line, p)) {
                return 0;
            }
            p = p.saturating_add(1);
            while is_digit(at(line, p)) {
                p = p.saturating_add(1);
            }
            pair = true;
        }
    }
    let letter = at(line, p);
    p = p.saturating_add(1);
    match letter {
        b'a' | b'i' => {
            if pair {
                return 0;
            }
        }
        b'c' | b'd' => {}
        b's' => {
            if !str_n_eq(line, p, b"/.//") {
                return 0;
            }
            p = p.saturating_add(4);
        }
        _ => return 0,
    }
    while matches!(at(line, p), b' ' | b'\t') {
        p = p.saturating_add(1);
    }
    if at(line, p) == b'\n' { letter } else { 0 }
}

// Line numbers are `lin` arithmetic, as upstream's: see the module docs.
#[allow(clippy::arithmetic_side_effects)]
impl Ctx {
    // ------------------------------------------------------ the file ----

    /// `getc (pfp)`.
    fn getc(&mut self) -> Option<u8> {
        let i = usize::try_from(self.pch.pos.checked_sub(self.pch.base)?).ok()?;
        let c = *self.pch.data.get(i)?;
        self.pch.pos += 1;
        Some(c)
    }

    /// `file_tell (pfp)`.
    fn tell(&self) -> u64 {
        self.pch.pos
    }

    /// `Fseek (pfp, pos, SEEK_SET)`.
    fn seek(&mut self, pos: u64) {
        self.pch.pos = pos;
    }

    /// `re_patch`: ready to look for the next patch.
    pub fn re_patch(&mut self) {
        let p = &mut self.pch;
        p.p_first = 0;
        p.p_newfirst = 0;
        p.p_ptrn_lines = 0;
        p.p_repl_lines = 0;
        p.p_end = -1;
        p.p_max = 0;
        p.p_indent = 0;
        p.p_strip_trailing_cr = false;
    }

    /// `open_patch_file`: the patch read in, from `filename` or standard
    /// input.
    pub fn open_patch_file(&mut self, filename: Option<&[u8]>) {
        let from_stdin = filename.is_none_or(|f| f.is_empty() || f == b"-");
        let fd = if from_stdin {
            0
        } else {
            let name = filename.unwrap_or_default();
            match sys::open_at(sys::AT_FDCWD, name, oflag::RDONLY, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    let mut m = b"Can't open patch file ".to_vec();
                    m.extend_from_slice(&self.q(name));
                    self.pfatal(&m, e);
                }
            }
        };
        let st = match sys::fstat_fd(fd) {
            Ok(st) => st,
            Err(e) => self.pfatal(b"fstat", e),
        };
        let here = if st.is_reg() {
            sys::lseek_fd(fd, 0, sys::SEEK_CUR).ok()
        } else {
            None
        };
        let (file_pos, filesize) = if let Some(pos) = here {
            // Read where the descriptor stands on; the file stays open, as
            // upstream's stream does.
            let mut data = Vec::new();
            if read_all(fd, &mut data).is_err() {
                self.read_fatal();
            }
            self.pch.data = data;
            let pos = u64::try_from(pos).unwrap_or(0);
            self.pch.base = pos;
            (pos, u64::try_from(st.size).unwrap_or(0))
        } else {
            let (name, made) = self.make_tempfile(b'p', None, oflag::RDWR, 0);
            self.tmppat.name = Some(name.clone());
            let tfd = match made {
                Ok(tfd) => tfd,
                Err(e) => {
                    // Unquoted, as upstream has it here.
                    let mut m = b"Can't create temporary file ".to_vec();
                    m.extend_from_slice(&name);
                    self.pfatal(&m, e);
                }
            };
            self.tmppat.needs_removal = true;
            util::register_temp(TMP_PAT, &name);
            let mut data = Vec::new();
            if read_all(fd, &mut data).is_err() {
                self.read_fatal();
            }
            if coreutils::stdfd::write_all(tfd, &data).is_err() {
                self.write_fatal();
            }
            // `fclose (read_pfp)`: the stream it came on is done with.
            if sys::close_fd(fd).is_err() {
                self.read_fatal();
            }
            let size = u64::try_from(data.len()).unwrap_or(u64::MAX);
            self.pch.data = data;
            self.pch.base = 0;
            (0, size)
        };
        self.pch.p_filesize = filesize;
        self.next_intuit_at(file_pos, 1);
    }

    // --------------------------------------------------- the next patch ----

    /// `maybe_reverse`: whether the patch looks reversed for `name` -- it
    /// would create a file that exists, or delete or empty out one that is
    /// not there or empty -- and, if it does, asked.
    fn maybe_reverse(&mut self, name: &[u8], nonexistent: bool, is_empty: bool) -> bool {
        let says = |i: bool| self.pch_says_nonexistent(i);
        let looks_reversed = i32::from(!is_empty) < says(self.reverse ^ is_empty);

        // Allow to create and delete empty files when we know that they are
        // empty: in the "diff --git" format, we know that from the index
        // header.
        if is_empty
            && says(self.reverse ^ nonexistent) == 1
            && says(!self.reverse ^ nonexistent) == 2
        {
            return false;
        }

        if looks_reversed {
            let mut m = format!(
                "The next patch{} would {} the file ",
                if self.reverse { ", when reversed," } else { "" },
                if nonexistent {
                    "delete"
                } else if is_empty {
                    "empty out"
                } else {
                    "create"
                }
            )
            .into_bytes();
            m.extend_from_slice(&self.q(name));
            m.extend_from_slice(
                format!(
                    ",\nwhich {}!",
                    if nonexistent {
                        "does not exist"
                    } else if is_empty {
                        "is already empty"
                    } else {
                        "already exists"
                    }
                )
                .as_bytes(),
            );
            if self.ok_to_reverse(&m) {
                self.reverse = !self.reverse;
            }
        }
        looks_reversed
    }

    /// `there_is_another_patch`: whether the rest of the patch file holds a
    /// diff; if it does, the file it is for, asked for when it cannot be
    /// told.
    pub fn there_is_another_patch(&mut self, need_header: bool, file_type: &mut u32) -> bool {
        let verbose = self.verbosity == Verbosity::Verbose;
        if self.pch.p_base != 0 && self.pch.p_base >= self.pch.p_filesize {
            if verbose {
                say(b"done\n");
            }
            return false;
        }
        if verbose {
            say(b"Hmm...");
        }
        self.diff_type = self.intuit_diff_type(need_header, file_type);
        if self.diff_type == Diff::No {
            if verbose {
                say(if self.pch.p_base != 0 {
                    b"  Ignoring the trailing garbage.\ndone\n".as_slice()
                } else {
                    b"  I can't seem to find a patch in there anywhere.\n".as_slice()
                });
            }
            if self.pch.p_base == 0 && self.pch.p_filesize != 0 {
                self.fatal(b"Only garbage was found in the patch input.");
            }
            return false;
        }
        if self.skip_rest_of_patch {
            let start = self.pch.p_start;
            self.seek(start);
            self.pch.p_input_line = self.pch.p_sline - 1;
            return true;
        }
        if verbose {
            let what = match self.diff_type {
                Diff::Uni => "a unified diff",
                Diff::Context => "a context diff",
                Diff::NewContext => "a new-style context diff",
                Diff::Normal => "a normal diff",
                Diff::GitBinary => "a git binary diff",
                _ => "an ed script",
            };
            say(format!(
                "  {}ooks like {what} to me...\n",
                if self.pch.p_base == 0 {
                    "L"
                } else {
                    "The next patch l"
                }
            )
            .as_bytes());
        }

        if self.no_strip_trailing_cr {
            self.pch.p_strip_trailing_cr = false;
        }

        if self.verbosity != Verbosity::Silent {
            let indent = self.pch.p_indent;
            if indent != 0 {
                say(format!(
                    "(Patch is indented {indent} space{}.)\n",
                    if indent == 1 { "" } else { "s" }
                )
                .as_bytes());
            }
            if self.pch.p_strip_trailing_cr {
                say(b"(Stripping trailing CRs from patch; use --binary to disable.)\n");
            }
            if self.inname.is_none() {
                say(format!(
                    "can't find file to patch at input line {}\n",
                    self.pch.p_sline
                )
                .as_bytes());
                if self.diff_type != Diff::Ed && self.diff_type != Diff::Normal {
                    say(if self.strippath == -1 {
                        b"Perhaps you should have used the -p or --strip option?\n".as_slice()
                    } else {
                        b"Perhaps you used the wrong -p or --strip option?\n".as_slice()
                    });
                }
            }
        }

        let (start, sline) = (self.pch.p_start, self.pch.p_sline);
        self.skip_to(start, sline);
        while self.inname.is_none() {
            if self.force || self.batch {
                say(b"No file to patch.  Skipping patch.\n");
                self.skip_rest_of_patch = true;
                return true;
            }
            self.ask(b"File to patch: ");
            let typed = cstr(&self.buf).to_vec();
            if typed.len() > 1 && typed.last() == Some(&b'\n') {
                let name = typed.get(..typed.len() - 1).unwrap_or_default().to_vec();
                match self.stat_file(&name) {
                    Ok(st) => {
                        self.instat = st;
                        self.inerrno = 0;
                        self.inname = Some(name);
                        self.invc = -1;
                    }
                    Err(e) => {
                        self.inerrno = e;
                        // `perror (inname)`.
                        let mut m = name.clone();
                        m.extend_from_slice(b": ");
                        m.extend_from_slice(
                            coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(e))
                                .as_bytes(),
                        );
                        m.push(b'\n');
                        util::print_stderr(&m);
                    }
                }
            }
            if self.inname.is_none() {
                self.ask(b"Skip this patch? [y] ");
                if self.answer() != b'n' {
                    if self.verbosity != Verbosity::Silent {
                        say(b"Skipping patch.\n");
                    }
                    self.skip_rest_of_patch = true;
                    return true;
                }
            }
        }
        true
    }

    /// `name_is_valid`: whether a name may be patched -- relative, with no
    /// `..` -- unless the working directory is `/`. Each refused name is
    /// reported once.
    fn name_is_valid(&mut self, name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < 2 {
            let Some(seen) = self.pch.invalid_names.get(i) else {
                break;
            };
            if seen.as_slice() == name {
                return false;
            }
            i += 1;
        }
        let mut is_valid = util::filename_is_safe(name);
        // Allow any filename if we are in the filesystem root.
        if !is_valid && self.cwd_is_root(name) {
            is_valid = true;
        }
        if !is_valid {
            let mut m = b"Ignoring potentially dangerous file name ".to_vec();
            m.extend_from_slice(&self.q(name));
            m.push(b'\n');
            say(&m);
            if i < 2 {
                self.pch.invalid_names.push(name.to_vec());
            }
        }
        is_valid
    }

    /// `intuit_diff_type`: what kind of diff the rest of the patch file
    /// holds, where it starts, its file names, modes and timestamps -- and
    /// from those, the file to patch.
    // Every index is a nametype -- OLD, NEW or INDEX, below 3 -- into a
    // three-element array, or OLD or NEW into a two-element one.
    #[allow(clippy::indexing_slicing)]
    fn intuit_diff_type(&mut self, mut need_header: bool, p_file_type: &mut u32) -> Diff {
        let mut this_line: u64 = 0;
        let mut first_command_line: Option<u64> = None;
        let mut first_ed_command_letter = 0u8;
        let mut fcl_line: Lin = 0;
        let mut this_is_a_command = false;
        let mut stars_this_line = false;
        let mut extended_headers = false;
        let mut st = [Stat::default(); 3];
        let mut stat_errno = [0i32; 3];
        let mut version_controlled = [-1i32; 3];
        let mut indent = 0usize;

        self.pch.p_name = [None, None, None];
        self.pch.invalid_names.clear();
        self.pch.p_timestr = [None, None];
        self.pch.p_sha1 = [None, None];
        self.pch.p_git_diff = false;
        self.pch.p_mode = [0, 0];
        self.pch.p_copy = [false, false];
        self.pch.p_rename = [false, false];

        // Ed and normal format patches don't have filename headers.
        if self.diff_type == Diff::Ed || self.diff_type == Diff::Normal {
            need_header = false;
        }

        self.pch.p_rfc934_nesting = 0;
        self.pch.p_timestamp[OLD].sec = -1;
        self.pch.p_timestamp[NEW].sec = -1;
        self.pch.p_says_nonexistent = [0, 0];
        let base = self.pch.p_base;
        self.seek(base);
        self.pch.p_input_line = self.pch.p_bline - 1;

        let retval = 'scan: loop {
            let previous_line = this_line;
            let last_line_was_command = this_is_a_command;
            let stars_last_line = stars_this_line;
            let indent_last_line = indent;

            indent = 0;
            this_line = self.tell();
            let chars_read = self.pget_line(0, 0, false, false);
            if chars_read == 0 {
                if first_ed_command_letter != 0 {
                    // Nothing but deletes!?
                    self.pch.p_start = first_command_line.unwrap_or(0);
                    self.pch.p_sline = fcl_line;
                    break 'scan Diff::Ed;
                }
                self.pch.p_start = this_line;
                self.pch.p_sline = self.pch.p_input_line;
                if extended_headers {
                    // Patch contains no hunks; any diff type will do.
                    break 'scan Diff::Uni;
                }
                return Diff::No;
            }
            let line = self.buf.clone();
            let b = |k: usize| at(&line, k);
            let strip_trailing_cr = chars_read >= 2 && b(chars_read - 2) == b'\r';
            let mut s = 0usize;
            while matches!(b(s), b' ' | b'\t' | b'X') {
                if b(s) == b'\t' {
                    indent = (indent + 8) & !7;
                } else {
                    indent += 1;
                }
                s += 1;
            }
            if is_digit(b(s)) {
                let mut t = s + 1;
                while is_digit(b(t)) || b(t) == b',' {
                    t += 1;
                }
                if matches!(b(t), b'd' | b'c' | b'a') {
                    t += 1;
                    while is_digit(b(t)) || b(t) == b',' {
                        t += 1;
                    }
                    while matches!(b(t), b' ' | b'\t') {
                        t += 1;
                    }
                    if b(t) == b'\r' {
                        t += 1;
                    }
                    this_is_a_command = b(t) == b'\n';
                }
            }
            if !need_header && first_command_line.is_none() {
                let ed_command_letter = get_ed_command_letter(line.get(s..).unwrap_or_default());
                if ed_command_letter != 0 || this_is_a_command {
                    first_command_line = Some(this_line);
                    first_ed_command_letter = ed_command_letter;
                    fcl_line = self.pch.p_input_line;
                    self.pch.p_indent = indent; // assume this for now
                    self.pch.p_strip_trailing_cr = strip_trailing_cr;
                }
            }
            let rest = |k: usize| line.get(k..).unwrap_or_default();
            let strip = self.strippath;
            if !stars_last_line && str_n_eq(&line, s, b"*** ") {
                let (mut name, mut ts, mut stamp) = self.take_old_header();
                self.fetchname(
                    rest(s + 4),
                    strip,
                    &mut name,
                    Some(&mut ts),
                    Some(&mut stamp),
                );
                self.put_old_header(name, ts, stamp);
                need_header = false;
            } else if str_n_eq(&line, s, b"+++ ") {
                // Swap with NEW below.
                let (mut name, mut ts, mut stamp) = self.take_old_header();
                self.fetchname(
                    rest(s + 4),
                    strip,
                    &mut name,
                    Some(&mut ts),
                    Some(&mut stamp),
                );
                self.put_old_header(name, ts, stamp);
                need_header = false;
                self.pch.p_strip_trailing_cr = strip_trailing_cr;
            } else if str_n_eq(&line, s, b"Index:") {
                let mut name = self.pch.p_name[INDEX].take();
                self.fetchname(rest(s + 6), strip, &mut name, None, None);
                self.pch.p_name[INDEX] = name;
                need_header = false;
                self.pch.p_strip_trailing_cr = strip_trailing_cr;
            } else if str_n_eq(&line, s, b"Prereq:") {
                let mut t = s + 7;
                while is_space(b(t)) {
                    t += 1;
                }
                let revision = t;
                while b(t) != 0 {
                    if is_space(b(t)) {
                        let mut u = t + 1;
                        while is_space(b(u)) {
                            u += 1;
                        }
                        if b(u) != 0 {
                            // The byte offset of the line, not its number:
                            // upstream's slip.
                            say(format!(
                                "Prereq: with multiple words at line {this_line} of patch\n"
                            )
                            .as_bytes());
                        }
                        break;
                    }
                    t += 1;
                }
                self.revision =
                    (t != revision).then(|| line.get(revision..t).unwrap_or_default().to_vec());
            } else if str_n_eq(&line, s, b"diff --git ") {
                if extended_headers {
                    self.pch.p_start = this_line;
                    self.pch.p_sline = self.pch.p_input_line;
                    // Patch contains no hunks; any diff type will do.
                    break 'scan Diff::Uni;
                }
                self.pch.p_name[OLD] = None;
                self.pch.p_name[NEW] = None;
                let (old, u) = Self::parse_name(rest(s + 11), strip);
                let u = s + 11 + u;
                let mut ok = false;
                if let Some(old) = old
                    && is_space(b(u))
                {
                    let (new, v) = Self::parse_name(rest(u), strip);
                    if let Some(new) = new
                        && b(skip_spaces(&line, u + v)) == 0
                    {
                        self.pch.p_name[OLD] = Some(old);
                        self.pch.p_name[NEW] = Some(new);
                        ok = true;
                    }
                }
                if !ok {
                    self.pch.p_name[OLD] = None;
                    self.pch.p_name[NEW] = None;
                }
                self.pch.p_git_diff = true;
                need_header = false;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"index ") {
                if let Some(u) = skip_hex_digits(&line, s + 6)
                    && b(u) == b'.'
                    && b(u + 1) == b'.'
                    && let Some(v) = skip_hex_digits(&line, u + 2)
                    && (b(v) == 0 || is_space(b(v)))
                {
                    let old = line.get(s + 6..u).unwrap_or_default().to_vec();
                    let new = line.get(u + 2..v).unwrap_or_default().to_vec();
                    self.pch.p_says_nonexistent[OLD] = sha1_says_nonexistent(&old);
                    self.pch.p_says_nonexistent[NEW] = sha1_says_nonexistent(&new);
                    self.pch.p_sha1 = [Some(old), Some(new)];
                    let v = skip_spaces(&line, v);
                    if b(v) != 0 {
                        let mode = fetchmode(rest(v));
                        self.pch.p_mode = [mode, mode];
                    }
                    extended_headers = true;
                }
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"old mode ") {
                self.pch.p_mode[OLD] = fetchmode(rest(s + 9));
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"new mode ") {
                self.pch.p_mode[NEW] = fetchmode(rest(s + 9));
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"deleted file mode ") {
                self.pch.p_mode[OLD] = fetchmode(rest(s + 18));
                self.pch.p_says_nonexistent[NEW] = 2;
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"new file mode ") {
                self.pch.p_mode[NEW] = fetchmode(rest(s + 14));
                self.pch.p_says_nonexistent[OLD] = 2;
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"rename from ") {
                // Git leaves out the prefix in the file name in this header,
                // so we can only ignore the file name.
                self.pch.p_rename[OLD] = true;
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"rename to ") {
                self.pch.p_rename[NEW] = true;
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"copy from ") {
                self.pch.p_copy[OLD] = true;
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"copy to ") {
                self.pch.p_copy[NEW] = true;
                extended_headers = true;
            } else if self.pch.p_git_diff && str_n_eq(&line, s, b"GIT binary patch") {
                self.pch.p_start = this_line;
                self.pch.p_sline = self.pch.p_input_line;
                break 'scan Diff::GitBinary;
            } else {
                let mut t = s;
                while b(t) == b'-' && b(t + 1) == b' ' {
                    t += 2;
                }
                if str_n_eq(&line, t, b"--- ") {
                    let mut timestamp = Timespec { sec: -1, nsec: 0 };
                    let mut name = self.pch.p_name[NEW].take();
                    let mut ts = self.pch.p_timestr[NEW].take();
                    self.fetchname(
                        rest(t + 4),
                        strip,
                        &mut name,
                        Some(&mut ts),
                        Some(&mut timestamp),
                    );
                    self.pch.p_name[NEW] = name;
                    self.pch.p_timestr[NEW] = ts;
                    need_header = false;
                    if timestamp.sec != -1 {
                        self.pch.p_timestamp[NEW] = timestamp;
                        self.pch.p_rfc934_nesting = i32::try_from((t - s) >> 1).unwrap_or(i32::MAX);
                    }
                    self.pch.p_strip_trailing_cr = strip_trailing_cr;
                }
            }
            if need_header {
                continue;
            }
            if (self.diff_type == Diff::No || self.diff_type == Diff::Ed)
                && let Some(fcl) = first_command_line
                && cstr(rest(s)) == b".\n"
            {
                self.pch.p_start = fcl;
                self.pch.p_sline = fcl_line;
                break 'scan Diff::Ed;
            }
            if (self.diff_type == Diff::No || self.diff_type == Diff::Uni)
                && str_n_eq(&line, s, b"@@ -")
            {
                // 'p_name', 'p_timestr', and 'p_timestamp' are backwards;
                // swap them.
                let p = &mut self.pch;
                p.p_timestamp.swap(OLD, NEW);
                p.p_name.swap(OLD, NEW);
                p.p_timestr.swap(OLD, NEW);

                let mut s = s + 4;
                if b(s) == b'0' && !is_digit(b(s + 1)) {
                    p.p_says_nonexistent[OLD] = 1 + i32::from(p.p_timestamp[OLD].sec == 0);
                }
                while s < line.len() && b(s) != b' ' && b(s) != b'\n' {
                    s += 1;
                }
                while b(s) == b' ' {
                    s += 1;
                }
                if b(s) == b'+' && b(s + 1) == b'0' && !is_digit(b(s + 2)) {
                    p.p_says_nonexistent[NEW] = 1 + i32::from(p.p_timestamp[NEW].sec == 0);
                }
                p.p_indent = indent;
                p.p_start = this_line;
                p.p_sline = p.p_input_line;
                // (Upstream's "missing header for unified diff" warning
                // follows here, but needs `need_header`, which is false by
                // now: it is never printed.)
                break 'scan Diff::Uni;
            }
            stars_this_line = str_n_eq(&line, s, b"********");
            if matches!(self.diff_type, Diff::No | Diff::Context | Diff::NewContext)
                && stars_last_line
                && indent_last_line == indent
                && str_n_eq(&line, s, b"*** ")
            {
                let mut s = s + 4;
                if b(s) == b'0' && !is_digit(b(s + 1)) {
                    self.pch.p_says_nonexistent[OLD] =
                        1 + i32::from(self.pch.p_timestamp[OLD].sec == 0);
                }
                // If this is a new context diff the character just before
                // the newline is a '*'.
                while s < line.len() && b(s) != b'\n' {
                    s += 1;
                }
                self.pch.p_indent = indent;
                self.pch.p_strip_trailing_cr = strip_trailing_cr;
                self.pch.p_start = previous_line;
                self.pch.p_sline = self.pch.p_input_line - 1;
                let retval = if s > 0 && b(s - 1) == b'*' {
                    Diff::NewContext
                } else {
                    Diff::Context
                };

                // Scan the first hunk to see whether the file contents
                // appear to have been deleted.
                let saved_p_base = self.pch.p_base;
                let saved_p_bline = self.pch.p_bline;
                self.seek(previous_line);
                self.pch.p_input_line -= 2;
                if self.another_hunk(retval, false) != 0
                    && self.pch.p_repl_lines == 0
                    && self.pch.p_newfirst == 1
                {
                    self.pch.p_says_nonexistent[NEW] =
                        1 + i32::from(self.pch.p_timestamp[NEW].sec == 0);
                }
                self.next_intuit_at(saved_p_base, saved_p_bline);
                // (Its "missing header for context diff" is never printed,
                // for the reason above.)
                break 'scan retval;
            }
            if (self.diff_type == Diff::No || self.diff_type == Diff::Normal)
                && last_line_was_command
                && (str_n_eq(&line, s, b"< ") || str_n_eq(&line, s, b"> "))
            {
                self.pch.p_start = previous_line;
                self.pch.p_sline = self.pch.p_input_line - 1;
                self.pch.p_indent = indent;
                break 'scan Diff::Normal;
            }
        };

        // scan_exit:

        // The old, new, or old and new file types may be defined. When both
        // file types are defined, make sure they are the same, or else
        // assume we do not know the file type.
        let mut file_type = self.pch.p_mode[OLD] & sys::S_IFMT;
        if file_type != 0 {
            let new_file_type = self.pch.p_mode[NEW] & sys::S_IFMT;
            if new_file_type != 0 && file_type != new_file_type {
                file_type = 0;
            }
        } else {
            file_type = self.pch.p_mode[NEW] & sys::S_IFMT;
            if file_type == 0 {
                file_type = sys::S_IFREG;
            }
        }
        *p_file_type = file_type;

        // To intuit 'inname', the name of the file to patch, use the
        // algorithm specified by POSIX 1003.1-2001 XCU lines 25680-26599
        // (with some modifications if posixly_correct is zero).
        let mut i = NONE;

        if self.inname.is_none() {
            let mut i0 = NONE;

            if !self.posixly_correct
                && (self.pch.p_name[OLD].is_some() || self.pch.p_name[NEW].is_some())
                && self.pch.p_name[INDEX].is_some()
            {
                self.pch.p_name[INDEX] = None;
            }

            i = OLD;
            while i <= INDEX {
                if let Some(name) = self.pch.p_name[i].clone() {
                    if i0 != NONE && self.pch.p_name[i0].as_deref() == Some(name.as_slice()) {
                        // It's the same name as before; reuse stat results.
                        stat_errno[i] = stat_errno[i0];
                        if stat_errno[i] == 0 {
                            st[i] = st[i0];
                        }
                    } else {
                        match self.stat_file(&name) {
                            Ok(s) => {
                                st[i] = s;
                                stat_errno[i] = 0;
                            }
                            Err(e) => stat_errno[i] = e,
                        }
                        if stat_errno[i] == 0 {
                            if self.lookup_file_id(&st[i]) == crate::FileIdType::DeleteLater {
                                stat_errno[i] = errno::ENOENT;
                            } else if self.posixly_correct && self.name_is_valid(&name) {
                                break;
                            }
                        }
                    }
                    i0 = i;
                }
                i += 1;
            }

            if !self.posixly_correct {
                // The best of all existing files.
                i = self.best_name(&stat_errno);

                if i == NONE && self.patch_get != 0 {
                    let mut nope = NONE;
                    i = OLD;
                    while i <= INDEX {
                        if let Some(name) = self.pch.p_name[i].clone() {
                            let readonly = self
                                .outfile
                                .as_deref()
                                .is_some_and(|o| o != name.as_slice());
                            if nope == NONE
                                || self.pch.p_name[nope].as_deref() != Some(name.as_slice())
                            {
                                let found = self.version_controller(&name, readonly, None);
                                version_controlled[i] = i32::from(found.is_some());
                                if let Some((cs, getbuf, _diffbuf)) = found {
                                    if self.version_get(
                                        &name, cs, false, readonly, &getbuf, &mut st[i],
                                    ) {
                                        stat_errno[i] = 0;
                                    } else {
                                        version_controlled[i] = 0;
                                    }
                                    if stat_errno[i] == 0 {
                                        break;
                                    }
                                }
                            }
                            nope = i;
                        }
                        i += 1;
                    }
                }

                if i0 != NONE && (i == NONE || st[i].mode & sys::S_IFMT == file_type) {
                    let which = if i == NONE { i0 } else { i };
                    let name = self.pch.p_name[which].clone().unwrap_or_default();
                    let is_empty = i == NONE || st[i].size == 0;
                    if self.maybe_reverse(&name, i == NONE, is_empty) && i == NONE {
                        i = i0;
                    }
                }

                if i == NONE && self.pch.p_says_nonexistent[usize::from(self.reverse)] != 0 {
                    let mut newdirs = [0i32; 3];
                    let mut newdirs_min = i32::MAX;
                    let mut distance_from_minimum = [0i32; 3];

                    for k in OLD..=INDEX {
                        if let Some(name) = self.pch.p_name[k].clone() {
                            newdirs[k] = self.prefix_components(&name, false)
                                - self.prefix_components(&name, true);
                            if newdirs[k] < newdirs_min {
                                newdirs_min = newdirs[k];
                            }
                        }
                    }
                    for k in OLD..=INDEX {
                        if self.pch.p_name[k].is_some() {
                            distance_from_minimum[k] = newdirs[k] - newdirs_min;
                        }
                    }
                    // The best of the filenames which create the fewest
                    // directories.
                    i = self.best_name(&distance_from_minimum);
                }
            }
        }

        if (self.pch_rename() || self.pch_copy()) && self.inname.is_none() {
            let r = usize::from(self.reverse);
            let nr = usize::from(!self.reverse);
            let valid = (i == OLD || i == NEW)
                && match (self.pch.p_name[r].clone(), self.pch.p_name[nr].clone()) {
                    (Some(a), Some(b)) => self.name_is_valid(&a) && self.name_is_valid(&b),
                    _ => false,
                };
            if !valid {
                say(format!(
                    "Cannot {} file without two valid file names\n",
                    if self.pch_rename() { "rename" } else { "copy" }
                )
                .as_bytes());
                self.skip_rest_of_patch = true;
            }
        }

        if i == NONE {
            if let Some(inname) = self.inname.clone() {
                match self.stat_file(&inname) {
                    Ok(s) => {
                        self.instat = s;
                        self.inerrno = 0;
                    }
                    Err(e) => self.inerrno = e,
                }
                if self.inerrno != 0 || self.instat.mode & sys::S_IFMT == file_type {
                    let nonexistent = self.inerrno != 0;
                    let is_empty = nonexistent || self.instat.size == 0;
                    self.maybe_reverse(&inname, nonexistent, is_empty);
                }
            } else {
                self.inerrno = -1;
            }
        } else {
            self.inname = self.pch.p_name[i].clone();
            self.inerrno = stat_errno[i];
            self.invc = version_controlled[i];
            self.instat = st[i];
        }

        retval
    }

    /// `p_name[OLD]`, `p_timestr[OLD]` and `p_timestamp[OLD]`, taken for
    /// `fetchname` to replace.
    fn take_old_header(&mut self) -> (Option<Vec<u8>>, Option<Vec<u8>>, Timespec) {
        (
            self.pch.p_name[OLD].take(),
            self.pch.p_timestr[OLD].take(),
            self.pch.p_timestamp[OLD],
        )
    }

    fn put_old_header(&mut self, name: Option<Vec<u8>>, ts: Option<Vec<u8>>, stamp: Timespec) {
        self.pch.p_name[OLD] = name;
        self.pch.p_timestr[OLD] = ts;
        self.pch.p_timestamp[OLD] = stamp;
    }

    /// `prefix_components`: how many components precede the last in
    /// `filename`; with `checkdirs`, only those that are directories, from
    /// the first.
    fn prefix_components(&mut self, filename: &[u8], checkdirs: bool) -> i32 {
        let f = cstr(filename);
        let mut count = 0i32;
        if !f.is_empty() {
            let mut k = 1usize;
            while k < f.len() {
                if at(f, k) == b'/' && at(f, k - 1) != b'/' {
                    if checkdirs {
                        let prefix = f.get(..k).unwrap_or_default();
                        if !self.safe.stat(prefix).is_ok_and(|st| st.is_dir()) {
                            break;
                        }
                    }
                    count += 1;
                }
                k += 1;
            }
        }
        count
    }

    /// `best_name`: the best of the old, new and index names -- the fewest
    /// prefix components, then the shortest base name, then the shortest
    /// name, then the first -- ignoring those `ignore` marks. As upstream,
    /// the minima are not reset when a name has fewer components than the
    /// one before, so a later name may win none of the three tests, and no
    /// name be chosen at all.
    // As `intuit_diff_type`: nametype indexes into three-element arrays.
    #[allow(clippy::indexing_slicing)]
    fn best_name(&mut self, ignore: &[i32; 3]) -> usize {
        let mut components = [0i32; 3];
        let mut components_min = i32::MAX;
        let mut basename_len = [0usize; 3];
        let mut basename_len_min = usize::MAX;
        let mut len = [0usize; 3];
        let mut len_min = usize::MAX;

        for i in OLD..=INDEX {
            let Some(name) = self.pch.p_name[i].clone() else {
                continue;
            };
            if ignore[i] != 0 {
                continue;
            }
            // Take the names with the fewest prefix components.
            components[i] = self.prefix_components(&name, false);
            if components_min < components[i] {
                continue;
            }
            components_min = components[i];

            // Of those, take the names with the shortest basename -- as
            // upstream measures it, `base_len (name)`: the whole name's
            // length less any trailing slashes.
            basename_len[i] = util::base_len(&name);
            if basename_len_min < basename_len[i] {
                continue;
            }
            basename_len_min = basename_len[i];

            // Of those, take the shortest names.
            len[i] = name.len();
            if len_min < len[i] {
                continue;
            }
            len_min = len[i];
        }

        // Of those, take the first name.
        let mut i = OLD;
        while i <= INDEX {
            if let Some(name) = self.pch.p_name[i].clone()
                && ignore[i] == 0
                && self.name_is_valid(&name)
                && components[i] == components_min
                && basename_len[i] == basename_len_min
                && len[i] == len_min
            {
                break;
            }
            i += 1;
        }
        i
    }

    /// `next_intuit_at`: where this patch ends, to start again from.
    fn next_intuit_at(&mut self, file_pos: u64, file_line: Lin) {
        self.pch.p_base = file_pos;
        self.pch.p_bline = file_line;
    }

    /// `skip_to`: on to where the diff starts -- showing what came before it
    /// when verbose or when the file to patch is not known.
    fn skip_to(&mut self, file_pos: u64, file_line: Lin) {
        let p_base = self.pch.p_base;
        if (self.verbosity == Verbosity::Verbose || self.inname.is_none()) && p_base < file_pos {
            self.seek(p_base);
            say(b"The text leading up to this was:\n--------------------------\n");
            let mut out = Vec::new();
            while self.tell() < file_pos {
                out.push(b'|');
                loop {
                    let Some(c) = self.getc() else {
                        util::print_stdout(&out);
                        self.read_fatal();
                    };
                    out.push(c);
                    if c == b'\n' {
                        break;
                    }
                }
            }
            util::print_stdout(&out);
            say(b"--------------------------\n");
        } else {
            self.seek(file_pos);
        }
        self.pch.p_input_line = file_line - 1;
    }

    /// `malformed`.
    fn malformed(&mut self) -> ! {
        let mut m = format!("malformed patch at line {}: ", self.pch.p_input_line).into_bytes();
        m.extend_from_slice(cstr(&self.buf));
        self.fatal(&m);
    }

    /// `scan_linenum`: the number at `buf[s0]`, and where it ends.
    fn scan_linenum(&mut self, s0: usize) -> (Lin, usize) {
        let mut s = s0;
        let mut n: Lin = 0;
        let mut overflow = false;
        while is_digit(at(&self.buf, s)) {
            let new_n = n
                .wrapping_mul(10)
                .wrapping_add(Lin::from(at(&self.buf, s) - b'0'));
            overflow |= new_n / 10 != n;
            n = new_n;
            s += 1;
        }
        if s == s0 {
            let mut m =
                format!("missing line number at line {}: ", self.pch.p_input_line).into_bytes();
            m.extend_from_slice(cstr(&self.buf));
            self.fatal(&m);
        }
        if overflow {
            let mut m = b"line number ".to_vec();
            m.extend_from_slice(self.buf.get(s0..s).unwrap_or_default());
            m.extend_from_slice(
                format!(" is too large at line {}: ", self.pch.p_input_line).as_bytes(),
            );
            m.extend_from_slice(cstr(&self.buf));
            self.fatal(&m);
        }
        (n, s)
    }

    /// `grow_hunkmax`, or gnulib's `xalloc_die` when the arrays cannot
    /// grow: a hunk whose line numbers ask for more lines than memory holds.
    fn grow_hunkmax(&mut self) {
        if !self.pch.grow_hunkmax() {
            let mut m = self.program_name.clone();
            m.extend_from_slice(b": memory exhausted\n");
            util::print_stderr(&m);
            self.exit(2);
        }
    }

    /// `buf` replaced, as upstream's `sprintf (buf, ...)` and `strcpy`.
    fn set_buf(&mut self, text: &[u8]) {
        self.buf.clear();
        self.buf.extend_from_slice(text);
    }

    /// `chars_read -= (cond && incomplete_line ())`.
    fn less_incomplete(&mut self, chars_read: usize, cond: bool) -> usize {
        if cond && self.incomplete_line() {
            chars_read.saturating_sub(1)
        } else {
            chars_read
        }
    }

    // ---------------------------------------------------------- hunks ----

    /// `another_hunk`: the next hunk read into the arrays; 1 if there was
    /// one, 0 if not.
    pub fn another_hunk(&mut self, difftype: Diff, rev: bool) -> i32 {
        self.pch.p_end = -1;
        self.pch.p_c_function = None;
        self.pch.p_max = self.pch.hunkmax; // gets reduced when --- found

        let found = match difftype {
            Diff::Context | Diff::NewContext => self.context_hunk(difftype),
            Diff::Uni => self.unified_hunk(),
            _ => self.normal_hunk(),
        };
        if !found {
            return 0;
        }
        if rev {
            // Upstream says "Not enough memory to swap next hunk!" when
            // this fails, which a Rust allocation cannot.
            self.pch_swap();
        }
        let end = self.pch.p_end;
        self.pch.set_ch(end + 1, b'^'); // add a stopper for apply_hunk
        if self.debug & 2 != 0 {
            self.dump_hunk();
        }
        1
    }

    /// The `debug & 2` listing of the hunk, on standard error.
    fn dump_hunk(&mut self) {
        let mut i: Lin = 0;
        while i <= self.pch.p_end + 1 {
            let c = self.pch.ch(i);
            let mut m = format!("{i} ").into_bytes();
            m.push(c);
            match c {
                b'*' => m.extend_from_slice(
                    format!(" {},{}\n", self.pch.p_first, self.pch.p_ptrn_lines).as_bytes(),
                ),
                b'=' => m.extend_from_slice(
                    format!(" {},{}\n", self.pch.p_newfirst, self.pch.p_repl_lines).as_bytes(),
                ),
                b'^' => m.push(b'\n'),
                _ => m.extend_from_slice(b" |"),
            }
            util::print_stderr(&m);
            if !matches!(c, b'*' | b'=' | b'^') {
                let line = self.pch.line(i).to_vec();
                if line.is_empty() {
                    // `fwrite` of nothing reports failure.
                    self.pfatal(b"write error", 0);
                }
                util::print_stderr(&line);
            }
            i += 1;
        }
    }

    /// A context diff's hunk (old or new style).
    fn context_hunk(&mut self, mut difftype: Diff) -> bool {
        let line_beginning = self.tell();
        let mut repl_beginning: Lin = 0; // index of --- line
        let mut fillcnt: Lin = 0; // #lines of missing ptrn or repl
        let mut fillsrc: Lin = 0; // index of first line to copy
        let mut filldst: Lin = 0; // index of first missing line
        let mut ptrn_spaces_eaten = false; // ptrn was slightly misformed
        let mut some_context = false; // (perhaps internal) context seen
        let mut repl_could_be_missing = true;
        let mut ptrn_missing = false; // The pattern was missing.
        let mut repl_missing = false; // Likewise for replacement.
        let mut repl_backtrack_position: u64 = 0; // file pos of first repl line
        let mut repl_patch_line: Lin = 0; // input line number for same
        let mut repl_context: Lin = 0; // context for same
        let mut ptrn_prefix_context: Lin = -1; // lines in pattern prefix context
        let mut ptrn_suffix_context: Lin = -1; // lines in pattern suffix context
        let mut repl_prefix_context: Lin = -1; // lines in replac. prefix context
        let mut ptrn_copiable: Lin = 0; // # of copiable lines in ptrn
        let mut repl_copiable: Lin = 0; // Likewise for replacement.
        let mut context: Lin = 0;

        let chars_read = self.get_line();
        if chars_read <= 8 || !str_n_eq(&self.buf, 0, b"********") {
            let line = self.pch.p_input_line;
            self.next_intuit_at(line_beginning, line);
            return false;
        }
        let mut s = 0usize;
        while at(&self.buf, s) == b'*' {
            s += 1;
        }
        if at(&self.buf, s) == b' ' {
            let start = s;
            while s < self.buf.len() && at(&self.buf, s) != b'\n' {
                s += 1;
            }
            self.pch.p_c_function = Some(cstr(self.buf.get(start..s).unwrap_or_default()).to_vec());
        }
        self.pch.p_hunk_beg = self.pch.p_input_line + 1;

        'hunk: while self.pch.p_end < self.pch.p_max {
            let mut chars_read = self.get_line();
            if chars_read == 0 {
                if repl_beginning != 0 && repl_could_be_missing {
                    repl_missing = true;
                    break 'hunk;
                }
                if self.pch.p_max - self.pch.p_end < 4 {
                    self.set_buf(b"  \n"); // assume blank lines got chopped
                    chars_read = 3;
                } else {
                    self.fatal(b"unexpected end of file in patch");
                }
            }
            self.pch.p_end += 1;
            let p_end = self.pch.p_end;
            if p_end == self.pch.hunkmax {
                let mut m = format!(
                    "unterminated hunk starting at line {}; giving up at line {}: ",
                    self.pch.p_hunk_beg, self.pch.p_input_line
                )
                .into_bytes();
                m.extend_from_slice(cstr(&self.buf));
                self.fatal(&m);
            }
            let first = at(&self.buf, 0);
            self.pch.set_ch(p_end, first);
            self.pch.set_line(p_end, Vec::new());
            let mut change_line = false;
            match first {
                b'*' => {
                    if str_n_eq(&self.buf, 0, b"********") {
                        if repl_beginning != 0 && repl_could_be_missing {
                            repl_missing = true;
                            break 'hunk;
                        }
                        let m = format!("unexpected end of hunk at line {}", self.pch.p_input_line);
                        self.fatal(m.as_bytes());
                    }
                    if p_end != 0 {
                        if repl_beginning != 0 && repl_could_be_missing {
                            repl_missing = true;
                            break 'hunk;
                        }
                        let mut m = format!("unexpected '***' at line {}: ", self.pch.p_input_line)
                            .into_bytes();
                        m.extend_from_slice(cstr(&self.buf));
                        self.fatal(&m);
                    }
                    context = 0;
                    self.pch.set_line(p_end, cstr(&self.buf).to_vec());
                    let mut s = 0usize;
                    while at(&self.buf, s) != 0 && !is_digit(at(&self.buf, s)) {
                        s += 1;
                    }
                    if str_n_eq(&self.buf, s, b"0,0") {
                        // `remove_prefix (s, 2)`.
                        self.buf.drain(s..s + 2);
                    }
                    let (first, s) = self.scan_linenum(s);
                    self.pch.p_first = first;
                    if at(&self.buf, s) == b',' {
                        let mut s = s;
                        while at(&self.buf, s) != 0 && !is_digit(at(&self.buf, s)) {
                            s += 1;
                        }
                        let (n, _) = self.scan_linenum(s);
                        self.pch.p_ptrn_lines = n.wrapping_add(1 - self.pch.p_first);
                        if self.pch.p_ptrn_lines < 0 {
                            self.malformed();
                        }
                    } else if self.pch.p_first != 0 {
                        self.pch.p_ptrn_lines = 1;
                    } else {
                        self.pch.p_ptrn_lines = 0;
                        self.pch.p_first = 1;
                    }
                    if self.pch.p_first >= LINENUM_MAX - self.pch.p_ptrn_lines
                        || self.pch.p_ptrn_lines >= LINENUM_MAX - 6
                    {
                        self.malformed();
                    }
                    self.pch.p_max = self.pch.p_ptrn_lines + 6; // we need this much at least
                    while self.pch.p_max + 1 >= self.pch.hunkmax {
                        self.grow_hunkmax();
                    }
                    self.pch.p_max = self.pch.hunkmax;
                }
                b'-' if at(&self.buf, 1) == b'-' => {
                    if ptrn_prefix_context == -1 {
                        ptrn_prefix_context = context;
                    }
                    ptrn_suffix_context = context;
                    let blank = Lin::from(p_end > 0 && self.pch.ch(p_end - 1) == b'\n');
                    if repl_beginning != 0
                        || p_end <= 0
                        || p_end != self.pch.p_ptrn_lines + 1 + blank
                    {
                        if p_end == 1 {
                            // 'Old' lines were omitted. Set up to fill them
                            // in from 'new' context lines.
                            ptrn_missing = true;
                            self.pch.p_end = self.pch.p_ptrn_lines + 1;
                            ptrn_prefix_context = -1;
                            ptrn_suffix_context = -1;
                            fillsrc = self.pch.p_end + 1;
                            filldst = 1;
                            fillcnt = self.pch.p_ptrn_lines;
                        } else if repl_beginning == 0 {
                            let m = format!(
                                "{} '---' at line {}; check line numbers at line {}",
                                if p_end <= self.pch.p_ptrn_lines {
                                    "Premature"
                                } else {
                                    "Overdue"
                                },
                                self.pch.p_input_line,
                                self.pch.p_hunk_beg
                            );
                            self.fatal(m.as_bytes());
                        } else if !repl_could_be_missing {
                            let m = format!(
                                "duplicate '---' at line {}; check line numbers at line {}",
                                self.pch.p_input_line,
                                self.pch.p_hunk_beg + repl_beginning
                            );
                            self.fatal(m.as_bytes());
                        } else {
                            repl_missing = true;
                            break 'hunk;
                        }
                    }
                    let p_end = self.pch.p_end;
                    repl_beginning = p_end;
                    repl_backtrack_position = self.tell();
                    repl_patch_line = self.pch.p_input_line;
                    repl_context = context;
                    self.pch.set_line(p_end, cstr(&self.buf).to_vec());
                    self.pch.set_ch(p_end, b'=');
                    let mut s = 0usize;
                    while at(&self.buf, s) != 0 && !is_digit(at(&self.buf, s)) {
                        s += 1;
                    }
                    let (newfirst, mut s) = self.scan_linenum(s);
                    self.pch.p_newfirst = newfirst;
                    if at(&self.buf, s) == b',' {
                        loop {
                            s += 1;
                            if at(&self.buf, s) == 0 {
                                self.malformed();
                            }
                            if is_digit(at(&self.buf, s)) {
                                break;
                            }
                        }
                        let (n, _) = self.scan_linenum(s);
                        self.pch.p_repl_lines = n.wrapping_add(1 - self.pch.p_newfirst);
                        if self.pch.p_repl_lines < 0 {
                            self.malformed();
                        }
                    } else if self.pch.p_newfirst != 0 {
                        self.pch.p_repl_lines = 1;
                    } else {
                        self.pch.p_repl_lines = 0;
                        self.pch.p_newfirst = 1;
                    }
                    if self.pch.p_newfirst >= LINENUM_MAX - self.pch.p_repl_lines
                        || self.pch.p_repl_lines >= LINENUM_MAX - p_end
                    {
                        self.malformed();
                    }
                    self.pch.p_max = self.pch.p_repl_lines + p_end;
                    while self.pch.p_max + 1 >= self.pch.hunkmax {
                        self.grow_hunkmax();
                    }
                    // `p_prefix_context` is the previous hunk's, as upstream
                    // reads it here.
                    if self.pch.p_repl_lines != ptrn_copiable
                        && (self.pch.p_prefix_context != 0
                            || context != 0
                            || self.pch.p_repl_lines != 1)
                    {
                        repl_could_be_missing = false;
                    }
                    context = 0;
                }
                b'-' => change_line = true,
                b'+' | b'!' => {
                    repl_could_be_missing = false;
                    change_line = true;
                }
                b'\t' | b'\n' => {
                    // Assume spaces got eaten.
                    if first == b'\t' {
                        chars_read -= 1;
                    }
                    if repl_beginning != 0
                        && repl_could_be_missing
                        && (!ptrn_spaces_eaten || difftype == Diff::NewContext)
                    {
                        repl_missing = true;
                        break 'hunk;
                    }
                    let last = if repl_beginning != 0 {
                        self.pch.p_max
                    } else {
                        self.pch.p_ptrn_lines
                    };
                    chars_read = self.less_incomplete(chars_read, chars_read > 1 && p_end == last);
                    // From `buf`, not past the tab: upstream's slip, which
                    // leaves a tab-led line one byte short.
                    let text = self.buf.get(..chars_read).unwrap_or_default().to_vec();
                    self.pch.set_line(p_end, text);
                    if p_end != self.pch.p_ptrn_lines + 1 {
                        ptrn_spaces_eaten |= repl_beginning != 0;
                        some_context = true;
                        context += 1;
                        if repl_beginning != 0 {
                            repl_copiable += 1;
                        } else {
                            ptrn_copiable += 1;
                        }
                        self.pch.set_ch(p_end, b' ');
                    }
                }
                b' ' => {
                    let mut s = 1usize;
                    chars_read -= 1;
                    if at(&self.buf, s) == b'\n' && self.canonicalize_ws {
                        // `strcpy (s, "\n"); chars_read = 2;`: the newline,
                        // and the NUL after it, as upstream has it.
                        self.buf.truncate(s);
                        self.buf.extend_from_slice(b"\n\0");
                        chars_read = 2;
                    }
                    if matches!(at(&self.buf, s), b' ' | b'\t') {
                        s += 1;
                        chars_read -= 1;
                    } else if repl_beginning != 0 && repl_could_be_missing {
                        repl_missing = true;
                        break 'hunk;
                    }
                    some_context = true;
                    context += 1;
                    if repl_beginning != 0 {
                        repl_copiable += 1;
                    } else {
                        ptrn_copiable += 1;
                    }
                    let last = if repl_beginning != 0 {
                        self.pch.p_max
                    } else {
                        self.pch.p_ptrn_lines
                    };
                    chars_read = self.less_incomplete(chars_read, chars_read > 1 && p_end == last);
                    let text = self.buf.get(s..s + chars_read).unwrap_or_default().to_vec();
                    self.pch.set_line(p_end, text);
                }
                _ => {
                    if repl_beginning != 0 && repl_could_be_missing {
                        repl_missing = true;
                        break 'hunk;
                    }
                    self.malformed();
                }
            }
            if change_line {
                let mut s = 1usize;
                chars_read -= 1;
                if at(&self.buf, s) == b'\n' && self.canonicalize_ws {
                    self.buf.truncate(s);
                    self.buf.extend_from_slice(b" \n");
                    chars_read = 2;
                }
                if matches!(at(&self.buf, s), b' ' | b'\t') {
                    s += 1;
                    chars_read -= 1;
                } else if repl_beginning != 0 && repl_could_be_missing {
                    repl_missing = true;
                    break 'hunk;
                }
                if repl_beginning == 0 {
                    if ptrn_prefix_context == -1 {
                        ptrn_prefix_context = context;
                    }
                } else if repl_prefix_context == -1 {
                    repl_prefix_context = context;
                }
                let last = if repl_beginning != 0 {
                    self.pch.p_max
                } else {
                    self.pch.p_ptrn_lines
                };
                chars_read = self.less_incomplete(chars_read, chars_read > 1 && p_end == last);
                let text = self.buf.get(s..s + chars_read).unwrap_or_default().to_vec();
                self.pch.set_line(p_end, text);
                context = 0;
            }
        }

        // hunk_done:
        if self.pch.p_end >= 0 && repl_beginning == 0 {
            let m = format!("no '---' found in patch at line {}", self.pch.p_hunk_beg);
            self.fatal(m.as_bytes());
        }

        if repl_missing {
            // Reset state back to just after ---.
            self.pch.p_input_line = repl_patch_line;
            context = repl_context;
            self.seek(repl_backtrack_position);

            // Redundant 'new' context lines were omitted - set up to fill
            // them in from the old file context.
            fillsrc = 1;
            filldst = repl_beginning + 1;
            fillcnt = self.pch.p_repl_lines;
            self.pch.p_end = self.pch.p_max;
        } else if !ptrn_missing && ptrn_copiable != repl_copiable {
            let m = format!("context mangled in hunk at line {}", self.pch.p_hunk_beg);
            self.fatal(m.as_bytes());
        } else if !some_context && fillcnt == 1 {
            // The first hunk was a null hunk with no context and we were
            // expecting one line -- fix it up.
            while filldst < self.pch.p_end {
                self.pch.copy_entry(filldst + 1, filldst);
                filldst += 1;
            }
            self.pch.p_end -= 1;
            self.pch.p_first += 1; // do append rather than insert
            fillcnt = 0;
            self.pch.p_ptrn_lines = 0;
        }

        self.pch.p_prefix_context = if repl_prefix_context == -1
            || (ptrn_prefix_context != -1 && ptrn_prefix_context < repl_prefix_context)
        {
            ptrn_prefix_context
        } else {
            repl_prefix_context
        };
        self.pch.p_suffix_context = if ptrn_suffix_context != -1 && ptrn_suffix_context < context {
            ptrn_suffix_context
        } else {
            context
        };
        if self.pch.p_prefix_context == -1 || self.pch.p_suffix_context == -1 {
            self.mangled_replacement();
        }

        if difftype == Diff::Context
            && (fillcnt != 0
                || (self.pch.p_first > 1
                    && self.pch.p_prefix_context + self.pch.p_suffix_context < ptrn_copiable))
        {
            if self.verbosity == Verbosity::Verbose {
                say(b"(Fascinating -- this is really a new-style context diff but without\nthe telltale extra asterisks on the *** line that usually indicate\nthe new style...)\n");
            }
            difftype = Diff::NewContext;
            self.diff_type = difftype;
        }

        // If there were omitted context lines, fill them in now.
        if fillcnt != 0 {
            while fillcnt > 0 {
                fillcnt -= 1;
                while fillsrc <= self.pch.p_end
                    && fillsrc != repl_beginning
                    && self.pch.ch(fillsrc) != b' '
                {
                    fillsrc += 1;
                }
                if self.pch.p_end < fillsrc || fillsrc == repl_beginning {
                    self.mangled_replacement();
                }
                self.pch.copy_entry(fillsrc, filldst);
                fillsrc += 1;
                filldst += 1;
            }
            while fillsrc <= self.pch.p_end && fillsrc != repl_beginning {
                if self.pch.ch(fillsrc) == b' ' {
                    self.mangled_replacement();
                }
                fillsrc += 1;
            }
            if self.debug & 64 != 0 {
                util::print_stdout(
                    format!(
                        "fillsrc {fillsrc}, filldst {filldst}, rb {repl_beginning}, e+1 {}\n",
                        self.pch.p_end + 1
                    )
                    .as_bytes(),
                );
            }
        }
        true
    }

    /// "replacement text or line numbers mangled in hunk at line N".
    fn mangled_replacement(&mut self) -> ! {
        let m = format!(
            "replacement text or line numbers mangled in hunk at line {}",
            self.pch.p_hunk_beg
        );
        self.fatal(m.as_bytes());
    }

    /// A unified diff's hunk, turned into the context form the rest of
    /// `patch` works with.
    fn unified_hunk(&mut self) -> bool {
        let line_beginning = self.tell();
        let mut context: Lin = 0;

        let chars_read = self.get_line();
        if chars_read <= 4 || !str_n_eq(&self.buf, 0, b"@@ -") {
            let line = self.pch.p_input_line;
            self.next_intuit_at(line_beginning, line);
            return false;
        }
        let (first, mut s) = self.scan_linenum(4);
        self.pch.p_first = first;
        if at(&self.buf, s) == b',' {
            let (n, e) = self.scan_linenum(s + 1);
            self.pch.p_ptrn_lines = n;
            s = e;
        } else {
            self.pch.p_ptrn_lines = 1;
        }
        if self.pch.p_first >= LINENUM_MAX - self.pch.p_ptrn_lines {
            self.malformed();
        }
        if at(&self.buf, s) == b' ' {
            s += 1;
        }
        if at(&self.buf, s) != b'+' {
            self.malformed();
        }
        let (newfirst, e) = self.scan_linenum(s + 1);
        self.pch.p_newfirst = newfirst;
        s = e;
        if at(&self.buf, s) == b',' {
            let (n, e) = self.scan_linenum(s + 1);
            self.pch.p_repl_lines = n;
            s = e;
        } else {
            self.pch.p_repl_lines = 1;
        }
        if self.pch.p_newfirst >= LINENUM_MAX - self.pch.p_repl_lines {
            self.malformed();
        }
        if at(&self.buf, s) == b' ' {
            s += 1;
        }
        let c = at(&self.buf, s);
        s += 1;
        if c != b'@' {
            self.malformed();
        }
        let c = at(&self.buf, s);
        s += 1;
        if c == b'@' && at(&self.buf, s) == b' ' {
            let start = s;
            while s < self.buf.len() && at(&self.buf, s) != b'\n' {
                s += 1;
            }
            self.pch.p_c_function = Some(cstr(self.buf.get(start..s).unwrap_or_default()).to_vec());
        }
        if self.pch.p_ptrn_lines == 0 {
            self.pch.p_first += 1; // do append rather than insert
        }
        if self.pch.p_repl_lines == 0 {
            self.pch.p_newfirst += 1;
        }
        if self.pch.p_ptrn_lines >= LINENUM_MAX - (self.pch.p_repl_lines + 1) {
            self.malformed();
        }
        self.pch.p_max = self.pch.p_ptrn_lines + self.pch.p_repl_lines + 1;
        while self.pch.p_max + 1 >= self.pch.hunkmax {
            self.grow_hunkmax();
        }
        let mut fillsrc: Lin = 1;
        let mut filldst: Lin = fillsrc + self.pch.p_ptrn_lines;
        self.pch.p_end = filldst + self.pch.p_repl_lines;
        let head = format!(
            "*** {},{} ****\n",
            self.pch.p_first,
            self.pch.p_first + self.pch.p_ptrn_lines - 1
        );
        self.set_buf(head.as_bytes());
        self.pch.set_line(0, head.into_bytes());
        self.pch.set_ch(0, b'*');
        let head = format!(
            "--- {},{} ----\n",
            self.pch.p_newfirst,
            self.pch.p_newfirst + self.pch.p_repl_lines - 1
        );
        self.set_buf(head.as_bytes());
        self.pch.set_line(filldst, head.into_bytes());
        self.pch.set_ch(filldst, b'=');
        filldst += 1;
        self.pch.p_prefix_context = -1;
        self.pch.p_hunk_beg = self.pch.p_input_line + 1;
        while fillsrc <= self.pch.p_ptrn_lines || filldst <= self.pch.p_end {
            let mut chars_read = self.get_line();
            if chars_read == 0 {
                if self.pch.p_max - filldst < 3 {
                    self.set_buf(b" \n"); // assume blank lines got chopped
                    chars_read = 2;
                } else {
                    self.fatal(b"unexpected end of file in patch");
                }
            }
            let first = at(&self.buf, 0);
            let (mut ch, text) = if first == b'\t' || first == b'\n' {
                // Assume the space got eaten.
                (
                    b' ',
                    self.buf.get(..chars_read).unwrap_or_default().to_vec(),
                )
            } else {
                chars_read -= 1;
                (
                    first,
                    self.buf.get(1..=chars_read).unwrap_or_default().to_vec(),
                )
            };
            if ch == b'=' {
                ch = b' ';
            }
            match ch {
                b'-' => {
                    if fillsrc > self.pch.p_ptrn_lines {
                        self.pch.p_end = filldst - 1;
                        self.malformed();
                    }
                    let cond = fillsrc == self.pch.p_ptrn_lines;
                    chars_read = self.less_incomplete(chars_read, cond);
                    self.pch.set_ch(fillsrc, ch);
                    self.pch
                        .set_line(fillsrc, text.get(..chars_read).unwrap_or_default().to_vec());
                    fillsrc += 1;
                }
                b' ' | b'+' => {
                    let mut text = text;
                    if ch == b' ' {
                        if fillsrc > self.pch.p_ptrn_lines {
                            self.pch.p_end = fillsrc - 1;
                            self.malformed();
                        }
                        context += 1;
                        let cond = fillsrc == self.pch.p_ptrn_lines;
                        chars_read = self.less_incomplete(chars_read, cond);
                        text.truncate(chars_read);
                        self.pch.set_ch(fillsrc, ch);
                        self.pch.set_line(fillsrc, text.clone());
                        fillsrc += 1;
                    }
                    if filldst > self.pch.p_end {
                        self.pch.p_end = fillsrc - 1;
                        self.malformed();
                    }
                    let cond = filldst == self.pch.p_end;
                    chars_read = self.less_incomplete(chars_read, cond);
                    text.truncate(chars_read);
                    self.pch.set_ch(filldst, ch);
                    self.pch.set_line(filldst, text);
                    filldst += 1;
                }
                _ => {
                    self.pch.p_end = filldst;
                    self.malformed();
                }
            }
            if ch != b' ' {
                if self.pch.p_prefix_context == -1 {
                    self.pch.p_prefix_context = context;
                }
                context = 0;
            }
        }
        if self.pch.p_prefix_context == -1 {
            self.malformed();
        }
        self.pch.p_suffix_context = context;
        true
    }

    /// A normal diff's hunk, faked up as a context one.
    fn normal_hunk(&mut self) -> bool {
        let line_beginning = self.tell();
        self.pch.p_prefix_context = 0;
        self.pch.p_suffix_context = 0;
        let chars_read = self.get_line();
        if chars_read == 0 || !is_digit(at(&self.buf, 0)) {
            let line = self.pch.p_input_line;
            self.next_intuit_at(line_beginning, line);
            return false;
        }
        let (first, mut s) = self.scan_linenum(0);
        self.pch.p_first = first;
        if at(&self.buf, s) == b',' {
            let (n, e) = self.scan_linenum(s + 1);
            s = e;
            self.pch.p_ptrn_lines = n.wrapping_add(1_i64.wrapping_sub(self.pch.p_first));
        } else {
            self.pch.p_ptrn_lines = Lin::from(at(&self.buf, s) != b'a');
        }
        if self.pch.p_first >= LINENUM_MAX.wrapping_sub(self.pch.p_ptrn_lines) {
            self.malformed();
        }
        let hunk_type = at(&self.buf, s);
        if hunk_type == b'a' {
            self.pch.p_first += 1; // do append rather than insert
        }
        let (mut min, e) = self.scan_linenum(s + 1);
        s = e;
        let max = if at(&self.buf, s) == b',' {
            self.scan_linenum(s + 1).0
        } else {
            min
        };
        if min > max || max.wrapping_sub(min) == LINENUM_MAX {
            self.malformed();
        }
        if hunk_type == b'd' {
            min = min.wrapping_add(1);
        }
        self.pch.p_newfirst = min;
        self.pch.p_repl_lines = max.wrapping_sub(min).wrapping_add(1);
        if self.pch.p_newfirst >= LINENUM_MAX.wrapping_sub(self.pch.p_repl_lines) {
            self.malformed();
        }
        if self.pch.p_ptrn_lines >= LINENUM_MAX.wrapping_sub(self.pch.p_repl_lines.wrapping_add(1))
        {
            self.malformed();
        }
        self.pch.p_end = self
            .pch
            .p_ptrn_lines
            .wrapping_add(self.pch.p_repl_lines)
            .wrapping_add(1);
        while self.pch.p_end.wrapping_add(1) >= self.pch.hunkmax {
            self.grow_hunkmax();
        }
        let head = format!(
            "*** {},{}\n",
            self.pch.p_first,
            self.pch
                .p_first
                .wrapping_add(self.pch.p_ptrn_lines)
                .wrapping_sub(1)
        );
        self.set_buf(head.as_bytes());
        self.pch.set_line(0, head.into_bytes());
        self.pch.set_ch(0, b'*');
        let mut i: Lin = 1;
        while i <= self.pch.p_ptrn_lines {
            let chars_read = self.get_line();
            if chars_read == 0 {
                let m = format!(
                    "unexpected end of file in patch at line {}",
                    self.pch.p_input_line
                );
                self.fatal(m.as_bytes());
            }
            if at(&self.buf, 0) != b'<' || !matches!(at(&self.buf, 1), b' ' | b'\t') {
                let m = format!(
                    "'<' followed by space or tab expected at line {} of patch",
                    self.pch.p_input_line
                );
                self.fatal(m.as_bytes());
            }
            let n = self.less_incomplete(chars_read, i == self.pch.p_ptrn_lines);
            let n = n.saturating_sub(2);
            self.pch
                .set_line(i, self.buf.get(2..2 + n).unwrap_or_default().to_vec());
            self.pch.set_ch(i, b'-');
            i += 1;
        }
        if hunk_type == b'c' {
            let chars_read = self.get_line();
            if chars_read == 0 {
                let m = format!(
                    "unexpected end of file in patch at line {}",
                    self.pch.p_input_line
                );
                self.fatal(m.as_bytes());
            }
            if at(&self.buf, 0) != b'-' {
                let m = format!("'---' expected at line {} of patch", self.pch.p_input_line);
                self.fatal(m.as_bytes());
            }
        }
        let head = format!("--- {min},{max}\n");
        self.set_buf(head.as_bytes());
        self.pch.set_line(i, head.into_bytes());
        self.pch.set_ch(i, b'=');
        i += 1;
        while i <= self.pch.p_end {
            let chars_read = self.get_line();
            if chars_read == 0 {
                let m = format!(
                    "unexpected end of file in patch at line {}",
                    self.pch.p_input_line
                );
                self.fatal(m.as_bytes());
            }
            if at(&self.buf, 0) != b'>' || !matches!(at(&self.buf, 1), b' ' | b'\t') {
                let m = format!(
                    "'>' followed by space or tab expected at line {} of patch",
                    self.pch.p_input_line
                );
                self.fatal(m.as_bytes());
            }
            let n = self.less_incomplete(chars_read, i == self.pch.p_end);
            let n = n.saturating_sub(2);
            self.pch
                .set_line(i, self.buf.get(2..2 + n).unwrap_or_default().to_vec());
            self.pch.set_ch(i, b'+');
            i += 1;
        }
        true
    }

    /// `get_line`: the next line of the hunk, read as the patch is
    /// indented, nested and line-ended.
    fn get_line(&mut self) -> usize {
        let p = &self.pch;
        let (indent, nesting, cr, comments) = (
            p.p_indent,
            p.p_rfc934_nesting,
            p.p_strip_trailing_cr,
            p.p_pass_comments_through,
        );
        self.pget_line(indent, nesting, cr, comments)
    }

    /// `pget_line`: a line of the patch into `buf`, up to `indent` columns
    /// of leading white space stripped, then up to `rfc934_nesting` leading
    /// `- `, then a trailing CR if asked; `#` lines skipped as comments
    /// unless passed through. A partial line at the end is not a line: it
    /// is reported, and 0 returned as at the end.
    fn pget_line(
        &mut self,
        indent: usize,
        mut rfc934_nesting: i32,
        strip_trailing_cr: bool,
        pass_comments_through: bool,
    ) -> usize {
        loop {
            let mut i = 0usize;
            let mut c;
            loop {
                let Some(ch) = self.getc() else {
                    return 0;
                };
                c = ch;
                if indent <= i {
                    break;
                }
                if c == b' ' || c == b'X' {
                    i += 1;
                } else if c == b'\t' {
                    i = (i + 8) & !7;
                } else {
                    break;
                }
            }

            self.buf.clear();
            // The count is not reset for the next line when this one turns
            // out to be a comment: upstream's.
            while c == b'-' && {
                rfc934_nesting -= 1;
                rfc934_nesting >= 0
            } {
                let Some(ch) = self.getc() else {
                    return self.ends_in_middle_of_line();
                };
                c = ch;
                if c != b' ' {
                    self.buf.push(b'-');
                    break;
                }
                let Some(ch) = self.getc() else {
                    return self.ends_in_middle_of_line();
                };
                c = ch;
            }

            loop {
                self.buf.push(c);
                if c == b'\n' {
                    break;
                }
                let Some(ch) = self.getc() else {
                    return self.ends_in_middle_of_line();
                };
                c = ch;
            }
            while self.bufsize <= self.buf.len() {
                self.bufsize = self.bufsize.saturating_mul(2);
            }

            self.pch.p_input_line += 1;
            if self.buf.first() != Some(&b'#') || pass_comments_through {
                break;
            }
        }

        let mut i = self.buf.len();
        if strip_trailing_cr && i >= 2 && self.buf.get(i - 2) == Some(&b'\r') {
            if let Some(cr) = self.buf.get_mut(i - 2) {
                *cr = b'\n';
            }
            i -= 1;
            self.buf.truncate(i);
        }
        i
    }

    /// `patch_ends_in_middle_of_line`.
    fn ends_in_middle_of_line(&mut self) -> usize {
        say(b"patch unexpectedly ends in middle of line\n");
        0
    }

    /// `incomplete_line`: whether the next line is "\ No newline at end of
    /// file" (or anything else led by a backslash), and if so, past it --
    /// read raw, without the indentation and nesting `get_line` strips, and
    /// not counted as a line.
    fn incomplete_line(&mut self) -> bool {
        let line_beginning = self.tell();
        if self.getc() == Some(b'\\') {
            while !matches!(self.getc(), Some(b'\n') | None) {}
            true
        } else {
            self.seek(line_beginning);
            false
        }
    }

    /// `pch_swap`: the old and new halves of the hunk exchanged.
    pub fn pch_swap(&mut self) -> bool {
        let p = &mut self.pch;
        std::mem::swap(&mut p.p_first, &mut p.p_newfirst);

        // Make a scratch copy.
        let n = Pch::idx(p.hunkmax).unwrap_or(0);
        let tp_line = std::mem::replace(&mut p.p_line, vec![Vec::new(); n]);
        let tp_char = std::mem::replace(&mut p.p_char, vec![0; n]);
        let tline = |i: Lin| {
            Pch::idx(i)
                .and_then(|i| tp_line.get(i))
                .cloned()
                .unwrap_or_default()
        };
        let tch = |i: Lin| {
            Pch::idx(i)
                .and_then(|i| tp_char.get(i))
                .copied()
                .unwrap_or(0)
        };

        // Now turn the new into the old.
        let mut i = p.p_ptrn_lines + 1;
        let mut blankline = false;
        if tch(i) == b'\n' {
            // Account for possible blank line.
            blankline = true;
            i += 1;
        }
        let mut n: Lin = 0;
        while i <= p.p_end {
            p.set_line(n, tline(i));
            let c = tch(i);
            p.set_ch(n, if c == b'+' { b'-' } else { c });
            i += 1;
            n += 1;
        }
        if blankline {
            let i = p.p_ptrn_lines + 1;
            p.set_line(n, tline(i));
            p.set_ch(n, tch(i));
            n += 1;
        }
        p.set_ch(0, b'*');
        let mut head = p.line(0).to_vec();
        let len = cstr(&head).len();
        for c in head.iter_mut().take(len) {
            if *c == b'-' {
                *c = b'*';
            }
        }
        p.set_line(0, head);

        // Now turn the old into the new.
        let mut head = tline(0);
        let len = cstr(&head).len();
        for c in head.iter_mut().take(len) {
            if *c == b'*' {
                *c = b'-';
            }
        }
        let mut i: Lin = 0;
        while n <= p.p_end {
            let (line, c) = if i == 0 {
                (head.clone(), b'=')
            } else {
                (tline(i), tch(i))
            };
            p.set_line(n, line);
            p.set_ch(n, if c == b'-' { b'+' } else { c });
            i += 1;
            n += 1;
        }
        std::mem::swap(&mut p.p_ptrn_lines, &mut p.p_repl_lines);
        let end = p.p_end;
        p.set_ch(end + 1, b'^');
        true
    }

    // ------------------------------------------------------ accessors ----

    /// `pch_says_nonexistent`: 1 for empty, 2 for nonexistent.
    pub fn pch_says_nonexistent(&self, which: bool) -> i32 {
        let [old, new] = self.pch.p_says_nonexistent;
        if which { new } else { old }
    }

    /// `pch_name`.
    pub fn pch_name(&self, which: usize) -> Option<&[u8]> {
        self.pch.p_name.get(which).and_then(|n| n.as_deref())
    }

    /// `pch_copy`: a git diff that copies a file.
    pub fn pch_copy(&self) -> bool {
        self.pch.p_copy[OLD] && self.pch.p_copy[NEW]
    }

    /// `pch_rename`: a git diff that renames a file.
    pub fn pch_rename(&self) -> bool {
        self.pch.p_rename[OLD] && self.pch.p_rename[NEW]
    }

    pub fn pch_first(&self) -> Lin {
        self.pch.p_first
    }

    pub fn pch_ptrn_lines(&self) -> Lin {
        self.pch.p_ptrn_lines
    }

    pub fn pch_newfirst(&self) -> Lin {
        self.pch.p_newfirst
    }

    pub fn pch_repl_lines(&self) -> Lin {
        self.pch.p_repl_lines
    }

    pub fn pch_end(&self) -> Lin {
        self.pch.p_end
    }

    pub fn pch_prefix_context(&self) -> Lin {
        self.pch.p_prefix_context
    }

    pub fn pch_suffix_context(&self) -> Lin {
        self.pch.p_suffix_context
    }

    /// `pch_line_len`.
    pub fn pch_line_len(&self, line: Lin) -> usize {
        self.pch.line(line).len()
    }

    /// `pch_char`: `-`, `+`, `!`, ` `, `*`, `=`, `^` -- or `\n` for an
    /// empty line in a context hunk, which belongs to neither side.
    pub fn pch_char(&self, line: Lin) -> u8 {
        self.pch.ch(line)
    }

    /// `pfetch`: line `line` of the hunk, `pch_line_len` bytes.
    pub fn pfetch(&self, line: Lin) -> &[u8] {
        self.pch.line(line)
    }

    pub fn pch_hunk_beg(&self) -> Lin {
        self.pch.p_hunk_beg
    }

    pub fn pch_c_function(&self) -> Option<&[u8]> {
        self.pch.p_c_function.as_deref()
    }

    pub fn pch_git_diff(&self) -> bool {
        self.pch.p_git_diff
    }

    pub fn pch_timestr(&self, which: bool) -> Option<&[u8]> {
        let [old, new] = &self.pch.p_timestr;
        if which {
            new.as_deref()
        } else {
            old.as_deref()
        }
    }

    pub fn pch_mode(&self, which: bool) -> u32 {
        let [old, new] = self.pch.p_mode;
        if which { new } else { old }
    }

    pub fn pch_timestamp(&self, which: bool) -> Timespec {
        let [old, new] = self.pch.p_timestamp;
        if which { new } else { old }
    }

    // ------------------------------------------------------- ed scripts ----

    /// `do_ed_script`: the ed script that starts here, handed to `ed` to
    /// apply to `outname` -- a copy of `inname` -- and, with `-o`, the
    /// result copied to `ofp`. Debian's form: the script goes to a temporary
    /// file, read by `ed` as its standard input, so that a bad command ends
    /// `ed` rather than having the next line taken for one
    /// (CVE-2018-1000156), and a missing input file is allowed.
    pub fn do_ed_script(&mut self, inname: &[u8], outname: &[u8], ofp: Option<&mut Output>) {
        let mut script: Option<(Vec<u8>, i32)> = None;
        if !self.dry_run && !self.skip_rest_of_patch {
            let (name, made) = self.make_tempfile(b'e', None, oflag::RDWR, 0);
            self.tmped.name = Some(name.clone());
            let fd = match made {
                Ok(fd) => fd,
                Err(e) => {
                    let mut m = b"Can't create temporary file ".to_vec();
                    m.extend_from_slice(&self.q(&name));
                    self.pfatal(&m, e);
                }
            };
            self.tmped.needs_removal = true;
            util::register_temp(TMP_ED, &name);
            script = Some((Vec::new(), fd));
        }

        loop {
            let beginning_of_this_line = self.tell();
            let chars_read = self.get_line();
            if chars_read == 0 {
                let line = self.pch.p_input_line;
                self.next_intuit_at(beginning_of_this_line, line);
                break;
            }
            let ed_command_letter = get_ed_command_letter(&self.buf);
            if ed_command_letter == 0 {
                let line = self.pch.p_input_line;
                self.next_intuit_at(beginning_of_this_line, line);
                break;
            }
            if let Some((text, _)) = script.as_mut() {
                text.extend_from_slice(self.buf.get(..chars_read).unwrap_or_default());
            }
            if ed_command_letter != b'd' && ed_command_letter != b's' {
                self.pch.p_pass_comments_through = true;
                loop {
                    let chars_read = self.get_line();
                    if chars_read == 0 {
                        break;
                    }
                    if let Some((text, _)) = script.as_mut() {
                        text.extend_from_slice(self.buf.get(..chars_read).unwrap_or_default());
                    }
                    if chars_read == 2 && cstr(&self.buf) == b".\n" {
                        break;
                    }
                }
                self.pch.p_pass_comments_through = false;
            }
        }
        let Some((mut text, tmpfd)) = script else {
            return;
        };
        text.extend_from_slice(b"w\nq\n");
        if coreutils::stdfd::write_all(tmpfd, &text).is_err() {
            self.write_fatal();
        }
        if let Err(e) = sys::lseek_fd(tmpfd, 0, sys::SEEK_SET) {
            let mut m = b"Can't rewind to the beginning of file ".to_vec();
            let name = self.tmped.name.clone().unwrap_or_default();
            m.extend_from_slice(&self.q(&name));
            self.pfatal(&m, e);
        }

        if !self.dry_run && !self.skip_rest_of_patch {
            let exclusive = if self.tmpout.needs_removal {
                0
            } else {
                oflag::EXCL
            };
            self.tmpout.needs_removal = true;
            if self.inerrno != errno::ENOENT {
                let mode = self.instat.mode;
                self.copy_file(inname, outname, None, exclusive, mode, true);
            }
            if !self.run_editor(tmpfd, outname) {
                let m = format!("{EDITOR_PROGRAM} FAILED");
                self.fatal(m.as_bytes());
            }
        }

        // `fclose (tmpfp)`: the script stays until cleanup removes it.
        let _ = sys::close_fd(tmpfd);

        if let Some(ofp) = ofp {
            let fd = match sys::open_at(sys::AT_FDCWD, outname, oflag::RDONLY, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    let mut m = b"can't open '".to_vec();
                    m.extend_from_slice(outname);
                    m.push(b'\'');
                    self.pfatal(&m, e);
                }
            };
            let mut data = Vec::new();
            let read = read_all(fd, &mut data);
            if std::io::Write::write_all(&mut ofp.w, &data).is_err() {
                self.write_fatal();
            }
            if read.is_err() || sys::close_fd(fd).is_err() {
                self.read_fatal();
            }
        }
    }

    /// `ed - OUTNAME`, the script on its standard input: whether it ran and
    /// exited 0. Upstream forks, and its child asserts that `outname` does
    /// not begin with `!` or `-` -- which `ed` would take for a command or an
    /// option -- before it execs; the assertion's message and failure are
    /// reproduced.
    fn run_editor(&mut self, tmpfd: i32, outname: &[u8]) -> bool {
        if matches!(outname.first(), Some(b'!' | b'-')) {
            // glibc's assertion message, under `__progname`.
            let mut m = util::base_name(&self.program_name);
            m.extend_from_slice(
                b": pch.c:2471: do_ed_script: Assertion `outname[0] != '!' && outname[0] != '-'' failed.\n",
            );
            util::print_stderr(&m);
            return false;
        }
        let Ok(stdin_fd) = sys::dup_fd(tmpfd) else {
            return false;
        };
        let stdin = util::file_from_fd(stdin_fd);
        std::process::Command::new(EDITOR_PROGRAM)
            .arg("-")
            .arg(coreutils::quote::os_from_bytes(outname))
            .stdin(stdin)
            .status()
            .is_ok_and(|s| s.success())
    }

    // -------------------------------------------------------- formats ----

    /// `pch_normalize`: `!` lines rewritten as `-` and `+` for the unified
    /// format, or runs of `-` then `+` as `!` for the context one.
    pub fn pch_normalize(&mut self, format: Diff) {
        let p = &mut self.pch;
        let mut old: Lin = 1;
        let mut new = p.p_ptrn_lines + 1;

        while p.ch(new) == b'=' || p.ch(new) == b'\n' {
            new += 1;
        }

        if format == Diff::Uni {
            // Convert '!' markers into '-' and '+' as defined by the Unified
            // Format.
            while old <= p.p_ptrn_lines {
                if p.ch(old) == b'!' {
                    p.set_ch(old, b'-');
                }
                old += 1;
            }
            while new <= p.p_end {
                if p.ch(new) == b'!' {
                    p.set_ch(new, b'+');
                }
                new += 1;
            }
        } else {
            // Convert '-' and '+' markers which are part of a group into '!'
            // as defined by the Context Format.
            while old <= p.p_ptrn_lines {
                if p.ch(old) == b'-' {
                    if new <= p.p_end && p.ch(new) == b'+' {
                        loop {
                            p.set_ch(old, b'!');
                            old += 1;
                            if !(old <= p.p_ptrn_lines && p.ch(old) == b'-') {
                                break;
                            }
                        }
                        loop {
                            p.set_ch(new, b'!');
                            new += 1;
                            if !(new <= p.p_end && p.ch(new) == b'+') {
                                break;
                            }
                        }
                    } else {
                        loop {
                            old += 1;
                            if !(old <= p.p_ptrn_lines && p.ch(old) == b'-') {
                                break;
                            }
                        }
                    }
                } else if new <= p.p_end && p.ch(new) == b'+' {
                    loop {
                        new += 1;
                        if !(new <= p.p_end && p.ch(new) == b'+') {
                            break;
                        }
                    }
                } else {
                    old += 1;
                    new += 1;
                }
            }
        }
    }
}

/// Everything left on `fd`, appended to `out`.
fn read_all(fd: i32, out: &mut Vec<u8>) -> Result<(), ()> {
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        match coreutils::stdfd::read(fd, &mut chunk) {
            Ok(0) => return Ok(()),
            Ok(n) => out.extend_from_slice(chunk.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn ed_commands_are_the_ones_diff_writes() {
        assert_eq!(get_ed_command_letter(b"5a\n"), b'a');
        assert_eq!(get_ed_command_letter(b"5,7d\n"), b'd');
        assert_eq!(get_ed_command_letter(b"5,7c \t\n"), b'c');
        assert_eq!(get_ed_command_letter(b"5,7a\n"), 0, "a pair cannot append");
        assert_eq!(get_ed_command_letter(b"s/.//\n"), b's');
        assert_eq!(get_ed_command_letter(b"s/x//\n"), 0);
        assert_eq!(get_ed_command_letter(b"5,\n"), 0);
        assert_eq!(get_ed_command_letter(b"w\n"), 0);
        assert_eq!(get_ed_command_letter(b"5d"), 0, "no newline");
    }

    #[test]
    fn modes_are_six_octal_digits_then_the_end_of_the_line() {
        assert_eq!(fetchmode(b" 100644\n"), 0o100644);
        assert_eq!(fetchmode(b"100755\r\n"), 0o100755);
        assert_eq!(fetchmode(b"100644 x\n"), 0);
        assert_eq!(fetchmode(b"10064\n"), 0);
        assert_eq!(fetchmode(b"100648\n"), 0);
    }

    #[test]
    fn object_names_say_whether_the_file_is_there() {
        assert_eq!(sha1_says_nonexistent(b"0000000"), 2);
        assert_eq!(sha1_says_nonexistent(b"e69de29"), 1);
        assert_eq!(
            sha1_says_nonexistent(b"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
            1
        );
        assert_eq!(
            sha1_says_nonexistent(b"e69de29bb2d1d6434b8b29ae775ad8c2e48c53911"),
            0
        );
        assert_eq!(sha1_says_nonexistent(b"abc1234"), 0);
    }

    #[test]
    fn hex_digits_are_lowercase_only() {
        assert_eq!(skip_hex_digits(b"index 1a2b..3c", 6), Some(10));
        assert_eq!(skip_hex_digits(b"index ..", 6), None);
        assert_eq!(skip_hex_digits(b"index ABC", 6), None);
    }
}
