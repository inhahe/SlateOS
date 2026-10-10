//! `merge.c`: `--merge` -- a hunk that does not apply where it says is
//! merged in rather than rejected: the stretch of the file it best matches
//! is found (`locate_merge`, with `bestmatch.h`), the hunk's old lines are
//! compared with it line by line (gnulib's `compareseq`, in
//! [`crate::diffseq`]), and what the hunk changes is applied wherever the
//! file still has the old lines -- and written between conflict markers,
//! `merge`'s or `diff3`'s, wherever it does not.
//!
//! Upstream leaves `compute_changes`' `too_expensive` uninitialized, so the
//! search's give-up point is whatever its stack held; here it is never
//! reached, which is what an unlimited value gives.

use crate::diffseq::{self, Seq};
use crate::util::{self, cstr, say};
use crate::{ConflictStyle, Ctx, Diff, Lin, OutState, Verbosity, skip_nl};

/// The hunk's old lines against the file's, as `compute_changes` hands
/// them to `compareseq`: a deleted old line marks `oldin[x]`, an inserted
/// file line `oldin[in + y - ymin]`.
struct Changes<'a> {
    cx: &'a mut Ctx,
    oldin: &'a mut Vec<u8>,
    in_: Lin,
    ymin: Lin,
}

impl Changes<'_> {
    fn mark(&mut self, i: Lin, c: u8) {
        if let Some(slot) = usize::try_from(i).ok().and_then(|i| self.oldin.get_mut(i)) {
            *slot = c;
        }
    }
}

#[allow(clippy::arithmetic_side_effects)]
impl Seq for Changes<'_> {
    fn equal(&mut self, x: Lin, y: Lin) -> bool {
        self.cx.context_matches_file(x, y)
    }
    fn note_delete(&mut self, x: Lin) {
        self.mark(x, b'-');
    }
    fn note_insert(&mut self, y: Lin) {
        let i = self.in_ + y - self.ymin;
        self.mark(i, b'+');
    }
}

/// `oldin[i]`.
fn at(oldin: &[u8], i: Lin) -> u8 {
    usize::try_from(i)
        .ok()
        .and_then(|i| oldin.get(i))
        .copied()
        .unwrap_or(0)
}

// Line numbers are `lin` arithmetic, as upstream's.
#[allow(clippy::arithmetic_side_effects)]
impl Ctx {
    /// `locate_merge`: where in the file the hunk best matches, allowing
    /// as many lines to be replaced as the hunk has context, nearest its
    /// expected place first; and how many file lines that match takes.
    fn locate_merge(&mut self) -> (Lin, Lin) {
        let first_guess = self.pch_first() + self.in_offset;
        let pat_lines = self.pch_ptrn_lines();
        let context_lines = self.count_context_lines();
        let max_where = self.input_lines - pat_lines + context_lines + 1;
        let min_where = self.last_frozen_line + 1;
        let mut max_pos_offset = max_where - first_guess;
        let mut max_neg_offset = first_guess - min_where;
        let max_offset = if max_pos_offset < max_neg_offset {
            max_neg_offset
        } else {
            max_pos_offset
        };
        let mut where_ = first_guess;
        let mut max_matched: Lin = 0;

        // Note: we need to preserve patch's property that it applies hunks
        // at the best match closest to their original position in the file.
        // It is common for hunks to apply equally well in several places in
        // a file. Applying at the first best match would be a lot easier.

        if context_lines != 0 {
            // Allow at most CONTEXT_LINES lines to be replaced (replacing
            // counts as insert + delete), and require the remaining MIN
            // lines to match.
            let mut max = 2 * context_lines;
            let mut min = pat_lines - context_lines;

            if self.debug & 1 != 0 {
                say(format!("locating merge: min={min} max={max} ").as_bytes());
            }

            // Hunks from the start or end of the file have less context.
            // Anchor them to the start or end, trying to make up for this
            // disadvantage.
            let offset = self.pch_suffix_context() - self.pch_prefix_context();
            if offset > 0 && self.pch_first() <= 1 {
                max_pos_offset = 0;
            }
            let match_until_eof = offset < 0;

            // Do not try lines <= 0.
            if first_guess <= max_neg_offset {
                max_neg_offset = first_guess - 1;
            }

            let mut offset: Lin = 0;
            while offset <= max_offset {
                let mut guesses = [None, None];
                if offset <= max_pos_offset {
                    guesses[0] = Some(first_guess + offset);
                }
                if 0 < offset && offset <= max_neg_offset {
                    guesses[1] = Some(first_guess - offset);
                }
                let mut done = false;
                for guess in guesses.into_iter().flatten() {
                    let want = if match_until_eof {
                        self.input_lines - guess + 1
                    } else {
                        min
                    };
                    let ylim = self.input_lines + 1;
                    let (changes, last) =
                        self.bestmatch(1, pat_lines + 1, guess, ylim, want, max, true);
                    let last = last.unwrap_or(-1);
                    if changes <= max && max_matched < last - guess {
                        max_matched = last - guess;
                        where_ = guess;
                        if changes == 0 {
                            done = true;
                            break;
                        }
                        min = last - guess;
                        max = changes - 1;
                    }
                }
                if done {
                    break;
                }
                offset += 1;
            }
            if self.debug & 1 != 0 {
                say(
                    format!("where={where_} matched={max_matched} changes={}\n", max + 1)
                        .as_bytes(),
                );
            }
        }

        // out:
        if where_ < min_where {
            where_ = min_where;
        }
        (where_, max_matched)
    }

    /// `bestmatch` (patch's `bestmatch.h`): the fewest changes that turn
    /// the hunk's old lines `[xoff, xlim)` into file lines from `yoff`, at
    /// most `max` of them and keeping at least `min` lines matched; `max + 1`
    /// if there is no such. With `py`, the file lines may be a prefix of
    /// `[yoff, ylim)`, and where the match ended is returned too.
    #[allow(clippy::too_many_arguments)]
    fn bestmatch(
        &mut self,
        mut xoff: Lin,
        xlim: Lin,
        mut yoff: Lin,
        ylim: Lin,
        mut min: Lin,
        max: Lin,
        py: bool,
    ) -> (Lin, Option<Lin>) {
        let dmin = xoff - ylim; // Minimum valid diagonal.
        let dmax = xlim - yoff; // Maximum valid diagonal.
        let fmid = xoff - yoff; // Center diagonal.
        let mut fmin = fmid;
        let mut fmax = fmid;
        let mut ymax: Lin = -1;
        let size = usize::try_from(2 * max + 3).unwrap_or(0);
        let mut v: Vec<Lin> = vec![0; size];
        let idx = |d: Lin| usize::try_from(d - fmid + max + 1).ok();
        let fd = |v: &Vec<Lin>, d: Lin| idx(d).and_then(|i| v.get(i)).copied().unwrap_or(-1);
        let set = |v: &mut Vec<Lin>, d: Lin, x: Lin| {
            if let Some(slot) = idx(d).and_then(|i| v.get_mut(i)) {
                *slot = x;
            }
        };

        // The number of elements matched must be at least MIN: with
        // x_skipped = (c + delta) / 2, that is x + y - c >= fmid + 2 * min.
        let fmid_plus_2_min;
        if min != 0 {
            fmid_plus_2_min = fmid + 2 * min;
            min += yoff;
            if min > ylim {
                return (max + 1, None);
            }
        } else {
            fmid_plus_2_min = 0; // disable this check
        }
        if !py {
            min = ylim;
        }

        // Handle the exact-match case.
        while xoff < xlim && yoff < ylim && self.context_matches_file(xoff, yoff) {
            xoff += 1;
            yoff += 1;
        }
        let mut c: Lin;
        if xoff == xlim && yoff >= min && xoff + yoff >= fmid_plus_2_min {
            ymax = yoff;
            c = 0;
        } else {
            set(&mut v, fmid, xoff);
            c = 1;
            'search: while c <= max {
                if fmin > dmin {
                    fmin -= 1;
                    set(&mut v, fmin - 1, -1);
                } else {
                    fmin += 1;
                }
                if fmax < dmax {
                    fmax += 1;
                    set(&mut v, fmax + 1, -1);
                } else {
                    fmax -= 1;
                }
                let mut d = fmax;
                while d >= fmin {
                    let mut x = if fd(&v, d - 1) < fd(&v, d + 1) {
                        fd(&v, d + 1)
                    } else {
                        fd(&v, d - 1) + 1
                    };
                    let mut y = x - d;
                    while x < xlim && y < ylim && self.context_matches_file(x, y) {
                        x += 1;
                        y += 1;
                    }
                    set(&mut v, d, x);
                    if x == xlim && y >= min && x + y - c >= fmid_plus_2_min {
                        if ymax < y {
                            ymax = y;
                        }
                        if y == ylim {
                            break 'search;
                        }
                    }
                    d -= 2;
                }
                if ymax != -1 {
                    break 'search;
                }
                c += 1;
            }
        }
        (c, py.then_some(ymax))
    }

    /// `print_linerange`.
    fn print_linerange(from: Lin, to: Lin) -> String {
        if to <= from {
            format!("{from}")
        } else {
            format!("{from}-{to}")
        }
    }

    /// `merge_result`: one more piece of "Hunk #N merged at A-B, NOT MERGED
    /// at C." -- or, with no `what`, the end of the line.
    fn merge_result(
        &mut self,
        first_result: &mut bool,
        hunk: i32,
        what: Option<&'static str>,
        from: Lin,
        to: Lin,
    ) {
        let mut m = match what {
            Some(w) if *first_result => {
                self.last_what = Some(w);
                format!("Hunk #{hunk} {w} at ")
            }
            None => {
                if !*first_result {
                    util::print_stdout(b".\n");
                    self.last_what = None;
                }
                return;
            }
            Some(w) if self.last_what == Some(w) => ",".to_string(),
            Some(w) => format!(", {w} at "),
        };
        m.push_str(&Self::print_linerange(
            from + self.out_offset,
            to + self.out_offset,
        ));
        util::print_stdout(m.as_bytes());
        *first_result = false;
    }

    /// `merge_hunk`: the hunk merged in at `where_` (or, with 0, wherever it
    /// matches best); false when the output would be garbled.
    pub fn merge_hunk(
        &mut self,
        hunk: i32,
        outstate: &mut OutState,
        mut where_: Lin,
        somefailed: &mut bool,
    ) -> bool {
        let mut first_result = true;
        let mut old: Lin = 1;
        let mut firstold = self.pch_ptrn_lines();
        let mut new = firstold + 1;
        let mut firstnew;

        // Convert '!' markers into '-' and '+' to simplify things here.
        self.pch_normalize(Diff::Uni);

        while self.pch_char(new) == b'=' || self.pch_char(new) == b'\n' {
            new += 1;
        }

        let applies_cleanly;
        let matched;
        if where_ != 0 {
            applies_cleanly = true;
            matched = self.pch_ptrn_lines();
        } else {
            let (w, m) = self.locate_merge();
            where_ = w;
            matched = m;
            applies_cleanly = false;
        }

        let mut in_ = firstold + 2;
        let mut oldin = vec![b' '; usize::try_from(in_ + matched + 1).unwrap_or(0)];
        if let Some(c) = oldin.first_mut() {
            *c = b'*';
        }
        if let Some(c) = usize::try_from(in_ - 1).ok().and_then(|i| oldin.get_mut(i)) {
            *c = b'=';
        }
        if let Some(c) = usize::try_from(in_ + matched)
            .ok()
            .and_then(|i| oldin.get_mut(i))
        {
            *c = b'^';
        }
        self.compute_changes(old, in_ - 1, where_, where_ + matched, &mut oldin, in_);

        if self.debug & 2 != 0 {
            let mut m = b"\n".to_vec();
            let mut n: Lin = 0;
            while n <= in_ + matched {
                m.extend_from_slice(format!("{n} ").as_bytes());
                m.push(at(&oldin, n));
                if n == 0 {
                    m.extend_from_slice(
                        format!(" {},{}\n", self.pch_first(), self.pch_ptrn_lines()).as_bytes(),
                    );
                } else if n <= firstold {
                    m.extend_from_slice(b" |");
                    m.extend_from_slice(cstr(self.pfetch(n)));
                } else if n == in_ - 1 {
                    m.extend_from_slice(format!(" {where_},{matched}\n").as_bytes());
                } else if n >= in_ && n < in_ + matched {
                    m.extend_from_slice(b" |");
                    let line = self.ifetch(where_ + n - in_, false).to_vec();
                    m.extend_from_slice(cstr(&line));
                } else {
                    m.push(b'\n');
                }
                n += 1;
            }
            util::print_stderr(&m);
        }

        if self.last_frozen_line < where_ - 1 && !self.copy_till(outstate, where_ - 1) {
            return false;
        }

        loop {
            firstold = old;
            firstnew = new;
            let mut firstin = in_;
            let pc = |cx: &Self, i: Lin| cx.pch_char(i);

            // Each arm either moves on (`continue`), ends the merge
            // (`break`), or falls into the conflict below (upstream's `goto
            // conflict`).
            if pc(self, old) == b'-' || pc(self, new) == b'+' {
                let mut conflict = false;
                while pc(self, old) == b'-' {
                    if at(&oldin, old) == b'-' || at(&oldin, in_) == b'+' {
                        conflict = true;
                        break;
                    } else if at(&oldin, old) == b' ' {
                        in_ += 1;
                    }
                    old += 1;
                }
                if !conflict && (at(&oldin, old) == b'-' || at(&oldin, in_) == b'+') {
                    conflict = true;
                }
                if !conflict {
                    while pc(self, new) == b'+' {
                        new += 1;
                    }
                    let lines = new - firstnew;
                    if self.verbosity == Verbosity::Verbose
                        || (self.verbosity != Verbosity::Silent && !applies_cleanly)
                    {
                        self.merge_result(
                            &mut first_result,
                            hunk,
                            Some("merged"),
                            where_,
                            where_ + lines - 1,
                        );
                    }
                    self.last_frozen_line += old - firstold;
                    where_ += old - firstold;
                    self.out_offset += new - firstnew;

                    if firstnew < new {
                        while firstnew < new {
                            outstate.after_newline = self.out_write_line(outstate, firstnew);
                            firstnew += 1;
                        }
                        outstate.zero_output = false;
                    }
                    continue;
                }
            } else if pc(self, old) == b' ' {
                if at(&oldin, old) == b'-' {
                    let mut conflict = false;
                    while pc(self, old) == b' ' {
                        if at(&oldin, old) != b'-' {
                            break;
                        }
                        if pc(self, new) == b'+' {
                            conflict = true;
                            break;
                        }
                        old += 1;
                        new += 1;
                    }
                    if !conflict && pc(self, old) != b'-' && pc(self, new) != b'+' {
                        continue;
                    }
                } else if at(&oldin, in_) == b'+' {
                    while at(&oldin, in_) == b'+' {
                        in_ += 1;
                    }
                    // Take these lines from the input file.
                    where_ += in_ - firstin;
                    if !self.copy_till(outstate, where_ - 1) {
                        return false;
                    }
                    continue;
                } else if at(&oldin, old) == b' ' {
                    while pc(self, old) == b' '
                        && at(&oldin, old) == b' '
                        && pc(self, new) == b' '
                        && at(&oldin, in_) == b' '
                    {
                        old += 1;
                        new += 1;
                        in_ += 1;
                    }
                    // Take these lines from the input file.
                    where_ += in_ - firstin;
                    if !self.copy_till(outstate, where_ - 1) {
                        return false;
                    }
                    continue;
                } else {
                    continue;
                }
            } else {
                // Nothing more left to merge.
                break;
            }

            // conflict:
            // Find the end of the conflict.
            loop {
                if pc(self, old) == b'-' {
                    while at(&oldin, in_) == b'+' {
                        in_ += 1;
                    }
                    if at(&oldin, old) == b' ' {
                        in_ += 1;
                    }
                    old += 1;
                } else if at(&oldin, old) == b'-' {
                    while pc(self, new) == b'+' {
                        new += 1;
                    }
                    if pc(self, old) == b' ' {
                        new += 1;
                    }
                    old += 1;
                } else if pc(self, new) == b'+' {
                    while pc(self, new) == b'+' {
                        new += 1;
                    }
                } else if at(&oldin, in_) == b'+' {
                    while at(&oldin, in_) == b'+' {
                        in_ += 1;
                    }
                } else {
                    break;
                }
            }

            // Output common prefix lines.
            let mut lastwhere = where_;
            while firstin < in_ && firstnew < new && self.context_matches_file(firstnew, lastwhere)
            {
                firstin += 1;
                firstnew += 1;
                lastwhere += 1;
            }
            let already_applied = firstin == in_ && firstnew == new;
            if already_applied {
                self.merge_result(
                    &mut first_result,
                    hunk,
                    Some("already applied"),
                    where_,
                    lastwhere - 1,
                );
            }
            if self.conflict_style == ConflictStyle::Diff3 {
                let common_prefix = lastwhere - where_;
                // Forget about common prefix lines.
                firstin -= common_prefix;
                firstnew -= common_prefix;
                lastwhere -= common_prefix;
            }
            if where_ != lastwhere {
                where_ = lastwhere;
                if !self.copy_till(outstate, where_ - 1) {
                    return false;
                }
            }

            if !already_applied {
                let mut common_suffix: Lin = 0;

                if self.conflict_style == ConflictStyle::Merge {
                    // Remember common suffix lines.
                    let mut lastwhere = where_ + (in_ - firstin);
                    while firstin < in_
                        && firstnew < new
                        && self.context_matches_file(new - 1, lastwhere - 1)
                    {
                        in_ -= 1;
                        new -= 1;
                        lastwhere -= 1;
                        common_suffix += 1;
                    }
                }

                let mut lines = 3 + (in_ - firstin) + (new - firstnew);
                if self.conflict_style == ConflictStyle::Diff3 {
                    lines += 1 + (old - firstold);
                }
                self.merge_result(
                    &mut first_result,
                    hunk,
                    Some("NOT MERGED"),
                    where_,
                    where_ + lines - 1,
                );
                self.out_offset += lines - (in_ - firstin);

                let m = skip_nl(outstate.after_newline, b"\n<<<<<<<\n").to_vec();
                self.out_write(outstate, &m);
                outstate.after_newline = true;
                if firstin < in_ {
                    where_ += in_ - firstin;
                    if !self.copy_till(outstate, where_ - 1) {
                        return false;
                    }
                }

                if self.conflict_style == ConflictStyle::Diff3 {
                    let m = skip_nl(outstate.after_newline, b"\n|||||||\n").to_vec();
                    self.out_write(outstate, &m);
                    outstate.after_newline = true;
                    while firstold < old {
                        outstate.after_newline = self.out_write_line(outstate, firstold);
                        firstold += 1;
                    }
                }

                let m = skip_nl(outstate.after_newline, b"\n=======\n").to_vec();
                self.out_write(outstate, &m);
                outstate.after_newline = true;
                while firstnew < new {
                    outstate.after_newline = self.out_write_line(outstate, firstnew);
                    firstnew += 1;
                }
                let m = skip_nl(outstate.after_newline, b"\n>>>>>>>\n").to_vec();
                self.out_write(outstate, &m);
                outstate.after_newline = true;
                outstate.zero_output = false;

                // Output common suffix lines.
                if common_suffix != 0 {
                    where_ += common_suffix;
                    if !self.copy_till(outstate, where_ - 1) {
                        return false;
                    }
                    in_ += common_suffix;
                    new += common_suffix;
                }
                *somefailed = true;
            }
        }
        self.merge_result(&mut first_result, 0, None, 0, 0);
        true
    }

    /// `count_context_lines`: the hunk's old lines that are context.
    fn count_context_lines(&self) -> Lin {
        let lastold = self.pch_ptrn_lines();
        let mut context: Lin = 0;
        let mut old: Lin = 1;
        while old <= lastold {
            if self.pch_char(old) == b' ' {
                context += 1;
            }
            old += 1;
        }
        context
    }

    /// `context_matches_file`: whether hunk line `old` is file line
    /// `where_` -- never past the end of the file.
    pub fn context_matches_file(&mut self, old: Lin, where_: Lin) -> bool {
        let line = self.ifetch(where_, false).to_vec();
        if line.is_empty() {
            return false;
        }
        let pat = self.pfetch(old);
        if self.canonicalize_ws {
            crate::similar(pat, &line)
        } else {
            line.as_slice() == pat
        }
    }

    /// `compute_changes`: the hunk's old lines `[xmin, xmax)` compared with
    /// the file's `[ymin, ymax)`, the edits marked in `oldin`.
    fn compute_changes(
        &mut self,
        xmin: Lin,
        xmax: Lin,
        ymin: Lin,
        ymax: Lin,
        oldin: &mut Vec<u8>,
        in_: Lin,
    ) {
        let mut ctxt = diffseq::Context::new(xmax, ymax);
        let mut changes = Changes {
            cx: self,
            oldin,
            in_,
            ymin,
        };
        diffseq::compareseq(&mut changes, &mut ctxt, xmin, xmax, ymin, ymax, false);
    }
}
