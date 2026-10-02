//! diffutils' `analyze.c` with gnulib's `diffseq.h`: which lines changed.
//!
//! The comparison proper is Myers' O(ND) algorithm with the middle-snake
//! split (`diag`/`compareseq`), run on equivalence classes rather than lines,
//! and only over the lines `discard_confusing_lines` left in: a line with no
//! match in the other file is marked changed beforehand, as is one that
//! matches too many lines to be worth anchoring on, when it sits among such
//! lines. Then `shift_boundaries` slides each run of changes as far down as
//! identical lines allow -- which is what makes GNU's choice among equally
//! short edit scripts the one it is -- and the flags become a script.
//!
//! Every index here is the algorithm's: diagonals within the band `diag`
//! keeps, lines within the vectors `discard_confusing_lines` built. Those
//! functions allow `indexing_slicing` and `arithmetic_side_effects` on that
//! ground: a broken invariant must stop the program, because a substituted
//! zero would not -- it would print a wrong diff, quietly, from a tool whose
//! whole output is a claim about two files.

use crate::io::{FileData, Lin};

/// `SNAKE_LIMIT`: a snake longer than this is "big".
const SNAKE_LIMIT: Lin = 20;

/// `struct change`: lines `line0 ..+deleted` of file 0 replaced by
/// `line1 ..+inserted` of file 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub line0: Lin,
    pub line1: Lin,
    pub deleted: Lin,
    pub inserted: Lin,
    /// `ignore`: every line of the change is ignorable (`-B`, `-I`).
    pub ignore: bool,
}

/// `struct context` from `diffseq.h`.
struct Context<'a> {
    xv: &'a [Lin],
    yv: &'a [Lin],
    /// `fdiag` and `bdiag`, indexed by diagonal plus `offset`.
    fdiag: Vec<Lin>,
    bdiag: Vec<Lin>,
    offset: Lin,
    heuristic: bool,
    too_expensive: Lin,
}

/// The snake from `(x, y)`: how many lines match going forward, short of
/// `xlim` and `ylim` -- upstream's `while (x < xlim && y < ylim &&
/// XREF_YREF_EQUAL (x, y))`, as one run over the two slices.
///
/// Most diagonals the search tries end at once, so the first pair is looked
/// at before the run is set up. Always inlined: it is the inner loop of the
/// comparison, run once per diagonal per edit step, and at the size-tuned
/// optimisation level the coreutils build at, a call here costs as much as
/// the snake itself.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::inline_always
)]
#[inline(always)]
fn snake_forward(xv: &[Lin], yv: &[Lin], x: Lin, y: Lin, xlim: Lin, ylim: Lin) -> Lin {
    if xlim <= x || ylim <= y || xv[x as usize] != yv[y as usize] {
        return 0;
    }
    let xs = &xv[x as usize + 1..xlim as usize];
    let ys = &yv[y as usize + 1..ylim as usize];
    1 + xs.iter().zip(ys).take_while(|(a, b)| a == b).count() as Lin
}

/// The snake back from `(x, y)`: how many lines before it match, down to
/// `xoff` and `yoff`. Inlined for [`snake_forward`]'s reason.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::inline_always
)]
#[inline(always)]
fn snake_backward(xv: &[Lin], yv: &[Lin], x: Lin, y: Lin, xoff: Lin, yoff: Lin) -> Lin {
    if x <= xoff || y <= yoff || xv[x as usize - 1] != yv[y as usize - 1] {
        return 0;
    }
    let xs = &xv[xoff as usize..x as usize - 1];
    let ys = &yv[yoff as usize..y as usize - 1];
    1 + xs
        .iter()
        .rev()
        .zip(ys.iter().rev())
        .take_while(|(a, b)| a == b)
        .count() as Lin
}

/// `struct partition`.
struct Partition {
    xmid: Lin,
    ymid: Lin,
    lo_minimal: bool,
    hi_minimal: bool,
}

#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
impl Context<'_> {
    #[inline]
    fn eq(&self, x: Lin, y: Lin) -> bool {
        self.xv[x as usize] == self.yv[y as usize]
    }
    #[inline]
    fn fd(&self, d: Lin) -> Lin {
        self.fdiag[(d + self.offset) as usize]
    }
    #[inline]
    fn set_fd(&mut self, d: Lin, v: Lin) {
        let i = (d + self.offset) as usize;
        self.fdiag[i] = v;
    }
    #[inline]
    fn bd(&self, d: Lin) -> Lin {
        self.bdiag[(d + self.offset) as usize]
    }
    #[inline]
    fn set_bd(&mut self, d: Lin, v: Lin) {
        let i = (d + self.offset) as usize;
        self.bdiag[i] = v;
    }

    /// `diag`: the midpoint of the shortest edit script for
    /// `[xoff, xlim) x [yoff, ylim)`, or a good guess at one when finding it
    /// would cost too much.
    fn diag(
        &mut self,
        xoff: Lin,
        xlim: Lin,
        yoff: Lin,
        ylim: Lin,
        find_minimal: bool,
    ) -> Partition {
        let dmin = xoff - ylim;
        let dmax = xlim - yoff;
        let fmid = xoff - yoff;
        let bmid = xlim - ylim;
        let mut fmin = fmid;
        let mut fmax = fmid;
        let mut bmin = bmid;
        let mut bmax = bmid;
        let odd = (fmid - bmid) & 1 != 0;

        self.set_fd(fmid, xoff);
        self.set_bd(bmid, xlim);

        let mut c: Lin = 1;
        loop {
            let mut big_snake = false;

            // Extend the top-down search by an edit step in each diagonal.
            if fmin > dmin {
                fmin -= 1;
                self.set_fd(fmin - 1, -1);
            } else {
                fmin += 1;
            }
            if fmax < dmax {
                fmax += 1;
                self.set_fd(fmax + 1, -1);
            } else {
                fmax -= 1;
            }
            // The vectors are indexed directly in the two scans below: they
            // are the comparison's inner loop.
            let (xv, yv, off) = (self.xv, self.yv, self.offset);
            let (fd, bd) = (&mut self.fdiag, &mut self.bdiag);
            let mut d = fmax;
            while d >= fmin {
                let k = (d + off) as usize;
                let thi = fd[k + 1];
                let tlo = fd[k - 1];
                let x0 = if tlo < thi { thi } else { tlo + 1 };
                let run = snake_forward(xv, yv, x0, x0 - d, xlim, ylim);
                let x = x0 + run;
                let y = x0 - d + run;
                if x - x0 > SNAKE_LIMIT {
                    big_snake = true;
                }
                fd[k] = x;
                if odd && bmin <= d && d <= bmax && bd[k] <= x {
                    return Partition {
                        xmid: x,
                        ymid: y,
                        lo_minimal: true,
                        hi_minimal: true,
                    };
                }
                d -= 2;
            }

            // Similarly extend the bottom-up search.
            if bmin > dmin {
                bmin -= 1;
                self.set_bd(bmin - 1, Lin::MAX);
            } else {
                bmin += 1;
            }
            if bmax < dmax {
                bmax += 1;
                self.set_bd(bmax + 1, Lin::MAX);
            } else {
                bmax -= 1;
            }
            let (fd, bd) = (&mut self.fdiag, &mut self.bdiag);
            let mut d = bmax;
            while d >= bmin {
                let k = (d + off) as usize;
                let thi = bd[k + 1];
                let tlo = bd[k - 1];
                let x0 = if tlo < thi { tlo } else { thi - 1 };
                let run = snake_backward(xv, yv, x0, x0 - d, xoff, yoff);
                let x = x0 - run;
                let y = x0 - d - run;
                if x0 - x > SNAKE_LIMIT {
                    big_snake = true;
                }
                bd[k] = x;
                if !odd && fmin <= d && d <= fmax && x <= fd[k] {
                    return Partition {
                        xmid: x,
                        ymid: y,
                        lo_minimal: true,
                        hi_minimal: true,
                    };
                }
                d -= 2;
            }

            if find_minimal {
                c += 1;
                continue;
            }

            // Heuristic: a diagonal that has made lots of progress compared
            // with the edit distance, ending in a big snake, is taken as if
            // the search had succeeded there.
            if 200 < c && big_snake && self.heuristic {
                let mut best: Lin = 0;
                let mut part = Partition {
                    xmid: 0,
                    ymid: 0,
                    lo_minimal: true,
                    hi_minimal: false,
                };
                let mut d = fmax;
                while d >= fmin {
                    let dd = d - fmid;
                    let x = self.fd(d);
                    let y = x - d;
                    let v = (x - xoff) * 2 - dd;
                    if v > 12 * (c + dd.abs())
                        && v > best
                        && xoff + SNAKE_LIMIT <= x
                        && x < xlim
                        && yoff + SNAKE_LIMIT <= y
                        && y < ylim
                    {
                        // Insist that it end with a significant snake.
                        let mut k: Lin = 1;
                        while self.eq(x - k, y - k) {
                            if k == SNAKE_LIMIT {
                                best = v;
                                part.xmid = x;
                                part.ymid = y;
                                break;
                            }
                            k += 1;
                        }
                    }
                    d -= 2;
                }
                if best > 0 {
                    return part;
                }

                let mut best: Lin = 0;
                let mut part = Partition {
                    xmid: 0,
                    ymid: 0,
                    lo_minimal: false,
                    hi_minimal: true,
                };
                let mut d = bmax;
                while d >= bmin {
                    let dd = d - bmid;
                    let x = self.bd(d);
                    let y = x - d;
                    let v = (xlim - x) * 2 + dd;
                    if v > 12 * (c + dd.abs())
                        && v > best
                        && xoff < x
                        && x <= xlim - SNAKE_LIMIT
                        && yoff < y
                        && y <= ylim - SNAKE_LIMIT
                    {
                        let mut k: Lin = 0;
                        while self.eq(x + k, y + k) {
                            if k == SNAKE_LIMIT - 1 {
                                best = v;
                                part.xmid = x;
                                part.ymid = y;
                                break;
                            }
                            k += 1;
                        }
                    }
                    d -= 2;
                }
                if best > 0 {
                    return part;
                }
            }

            // Gone well beyond the call of duty: report halfway between the
            // best results so far.
            if c >= self.too_expensive {
                let mut fxybest: Lin = -1;
                let mut fxbest: Lin = 0;
                let mut d = fmax;
                while d >= fmin {
                    let mut x = self.fd(d).min(xlim);
                    let mut y = x - d;
                    if ylim < y {
                        x = ylim + d;
                        y = ylim;
                    }
                    if fxybest < x + y {
                        fxybest = x + y;
                        fxbest = x;
                    }
                    d -= 2;
                }
                let mut bxybest = Lin::MAX;
                let mut bxbest: Lin = 0;
                let mut d = bmax;
                while d >= bmin {
                    let mut x = xoff.max(self.bd(d));
                    let mut y = x - d;
                    if y < yoff {
                        x = yoff + d;
                        y = yoff;
                    }
                    if x + y < bxybest {
                        bxybest = x + y;
                        bxbest = x;
                    }
                    d -= 2;
                }
                return if (xlim + ylim) - bxybest < fxybest - (xoff + yoff) {
                    Partition {
                        xmid: fxbest,
                        ymid: fxybest - fxbest,
                        lo_minimal: true,
                        hi_minimal: false,
                    }
                } else {
                    Partition {
                        xmid: bxbest,
                        ymid: bxybest - bxbest,
                        lo_minimal: false,
                        hi_minimal: true,
                    }
                };
            }
            c += 1;
        }
    }

    /// `compareseq`: mark the changes between `[xoff, xlim)` and
    /// `[yoff, ylim)`, which as wholes are known to correspond.
    fn compareseq(
        &mut self,
        mut xoff: Lin,
        mut xlim: Lin,
        mut yoff: Lin,
        mut ylim: Lin,
        mut find_minimal: bool,
        files: &mut [FileData; 2],
    ) {
        loop {
            // Slide down the bottom initial diagonal.
            let run = snake_forward(self.xv, self.yv, xoff, yoff, xlim, ylim);
            xoff += run;
            yoff += run;
            // Slide up the top initial diagonal.
            let run = snake_backward(self.xv, self.yv, xlim, ylim, xoff, yoff);
            xlim -= run;
            ylim -= run;
            // The simple cases.
            if xoff == xlim {
                while yoff < ylim {
                    let real = files[1].realindexes[yoff as usize];
                    files[1].changed.set(real, true);
                    yoff += 1;
                }
                return;
            }
            if yoff == ylim {
                while xoff < xlim {
                    let real = files[0].realindexes[xoff as usize];
                    files[0].changed.set(real, true);
                    xoff += 1;
                }
                return;
            }

            let part = self.diag(xoff, xlim, yoff, ylim, find_minimal);

            // Recurse on the smaller half, iterate on the other.
            let (lo, hi);
            if (xlim + ylim) - (part.xmid + part.ymid) < (part.xmid + part.ymid) - (xoff + yoff) {
                lo = (part.xmid, xlim, part.ymid, ylim, part.hi_minimal);
                hi = (xoff, part.xmid, yoff, part.ymid, part.lo_minimal);
            } else {
                lo = (xoff, part.xmid, yoff, part.ymid, part.lo_minimal);
                hi = (part.xmid, xlim, part.ymid, ylim, part.hi_minimal);
            }
            self.compareseq(lo.0, lo.1, lo.2, lo.3, lo.4, files);
            (xoff, xlim, yoff, ylim, find_minimal) = hi;
        }
    }
}

/// `discard_confusing_lines`: mark lines with no match in the other file as
/// changed, and those matching too many lines too, where they sit among
/// such lines; build the vectors of what is left.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn discard_confusing_lines(files: &mut [FileData; 2], minimal: bool) {
    let equiv_max = files[0].equiv_max.max(1) as usize;
    let mut equiv_count = [vec![0 as Lin; equiv_max], vec![0 as Lin; equiv_max]];
    for f in 0..2 {
        for &e in &files[f].equivs {
            equiv_count[f][e as usize] += 1;
        }
    }

    let mut discarded: [Vec<u8>; 2] = [
        vec![0; files[0].buffered_lines as usize],
        vec![0; files[1].buffered_lines as usize],
    ];

    // Mark each line that matches no line of the other file; mark one that
    // matches many as provisionally discardable.
    for f in 0..2 {
        let end = files[f].buffered_lines as usize;
        let counts = &equiv_count[1 - f];
        let equivs = &files[f].equivs;
        let mut many: usize = 5;
        let mut tem = end / 64;
        // MANY is 5 times the approximate square root of the line count.
        loop {
            tem >>= 2;
            if tem == 0 {
                break;
            }
            many *= 2;
        }
        for (d, &e) in discarded[f].iter_mut().zip(equivs).take(end) {
            if e == 0 {
                continue;
            }
            let nmatch = counts[e as usize];
            if nmatch == 0 {
                *d = 1;
            } else if nmatch as usize > many {
                *d = 2;
            }
        }
    }

    // Discard the provisional lines only within a run of discardables with
    // nonprovisionals at its ends.
    for discards in &mut discarded {
        let end = discards.len() as Lin;
        let mut i: Lin = 0;
        while i < end {
            if discards[i as usize] == 2 {
                discards[i as usize] = 0;
            } else if discards[i as usize] != 0 {
                // A nonprovisional discard: find the end of its run.
                let mut provisional: Lin = 0;
                let mut j = i;
                while j < end {
                    if discards[j as usize] == 0 {
                        break;
                    }
                    if discards[j as usize] == 2 {
                        provisional += 1;
                    }
                    j += 1;
                }
                // Cancel provisional discards at the end, shrinking the run.
                while j > i && discards[(j - 1) as usize] == 2 {
                    j -= 1;
                    discards[j as usize] = 0;
                    provisional -= 1;
                }
                let length = j - i;

                if provisional * 4 > length {
                    // A quarter of the run provisional: cancel them all.
                    while j > i {
                        j -= 1;
                        if discards[j as usize] == 2 {
                            discards[j as usize] = 0;
                        }
                    }
                } else {
                    // MINIMUM is approximately the square root of LENGTH/4.
                    let mut minimum: Lin = 1;
                    let mut tem = length >> 2;
                    loop {
                        tem >>= 2;
                        if tem <= 0 {
                            break;
                        }
                        minimum <<= 1;
                    }
                    minimum += 1;

                    // Cancel any subrun of MINIMUM or more provisionals.
                    let mut consec: Lin = 0;
                    let mut jj: Lin = 0;
                    while jj < length {
                        if discards[(i + jj) as usize] != 2 {
                            consec = 0;
                        } else {
                            consec += 1;
                            if minimum == consec {
                                // Back up to the start of the subrun.
                                jj -= consec;
                            } else if minimum < consec {
                                discards[(i + jj) as usize] = 0;
                            }
                        }
                        jj += 1;
                    }

                    // From the start of the run, cancel provisionals until
                    // three nonprovisionals in a row, or the first
                    // nonprovisional at least 8 lines in.
                    let mut consec: Lin = 0;
                    let mut jj: Lin = 0;
                    while jj < length {
                        let k = (i + jj) as usize;
                        if jj >= 8 && discards[k] == 1 {
                            break;
                        }
                        if discards[k] == 2 {
                            consec = 0;
                            discards[k] = 0;
                        } else if discards[k] == 0 {
                            consec = 0;
                        } else {
                            consec += 1;
                        }
                        if consec == 3 {
                            break;
                        }
                        jj += 1;
                    }

                    // I advances to the last line of the run.
                    i += length - 1;

                    // Likewise from the end.
                    let mut consec: Lin = 0;
                    let mut jj: Lin = 0;
                    while jj < length {
                        let k = (i - jj) as usize;
                        if jj >= 8 && discards[k] == 1 {
                            break;
                        }
                        if discards[k] == 2 {
                            consec = 0;
                            discards[k] = 0;
                        } else if discards[k] == 0 {
                            consec = 0;
                        } else {
                            consec += 1;
                        }
                        if consec == 3 {
                            break;
                        }
                        jj += 1;
                    }
                }
            }
            i += 1;
        }
    }

    // Actually discard the lines.
    for (file, discards) in files.iter_mut().zip(&discarded) {
        let end = file.buffered_lines as usize;
        let mut undiscarded = Vec::with_capacity(end);
        let mut realindexes = Vec::with_capacity(end);
        for (i, (&d, &e)) in discards.iter().zip(&file.equivs).take(end).enumerate() {
            if minimal || d == 0 {
                undiscarded.push(e);
                realindexes.push(i as Lin);
            } else {
                file.changed.set(i as Lin, true);
            }
        }
        file.nondiscarded_lines = undiscarded.len() as Lin;
        file.undiscarded = undiscarded;
        file.realindexes = realindexes;
    }
}

/// `shift_boundaries`: slide each run of changes back while the line before
/// it matches its last line, merging with earlier runs, then forward while
/// its first line matches the line after it -- and back again to line up
/// with a run in the other file if it can.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn shift_boundaries(files: &mut [FileData; 2]) {
    for f in 0..2 {
        let (this, other) = if f == 0 {
            let (a, b) = files.split_at_mut(1);
            (&mut a[0], &b[0])
        } else {
            let (a, b) = files.split_at_mut(1);
            (&mut b[0], &a[0])
        };
        let other_changed = &other.changed;
        let equivs = &this.equivs;
        let changed = &mut this.changed;
        let i_end = this.buffered_lines;
        let mut i: Lin = 0;
        let mut j: Lin = 0;

        loop {
            // Scan forwards to the next run of changes, tracking the
            // corresponding point in the other file.
            while i < i_end && !changed.get(i) {
                while other_changed.get(j) {
                    j += 1;
                }
                j += 1;
                i += 1;
            }
            if i == i_end {
                break;
            }
            let mut start = i;

            // Find the end of this run.
            i += 1;
            while changed.get(i) {
                i += 1;
            }
            while other_changed.get(j) {
                j += 1;
            }

            let mut corresponding;
            loop {
                let runlength = i - start;

                // Move the run back while the previous unchanged line matches
                // its last changed one; this merges with earlier runs.
                while start != 0 && equivs[(start - 1) as usize] == equivs[(i - 1) as usize] {
                    start -= 1;
                    changed.set(start, true);
                    i -= 1;
                    changed.set(i, false);
                    while changed.get(start - 1) {
                        start -= 1;
                    }
                    j -= 1;
                    while other_changed.get(j) {
                        j -= 1;
                    }
                }

                // CORRESPONDING: the end of the run, at the last point where
                // it corresponds to a run in the other file; I_END for none.
                corresponding = if other_changed.get(j - 1) { i } else { i_end };

                // Move the run forward while its first line matches the
                // following unchanged one; this merges with later runs.
                while i != i_end && equivs[start as usize] == equivs[i as usize] {
                    changed.set(start, false);
                    start += 1;
                    changed.set(i, true);
                    i += 1;
                    while changed.get(i) {
                        i += 1;
                    }
                    j += 1;
                    while other_changed.get(j) {
                        j += 1;
                        corresponding = i;
                    }
                }

                if runlength == i - start {
                    break;
                }
            }

            // Move the fully merged run back to a corresponding run in the
            // other file, if there is one.
            while corresponding < i {
                start -= 1;
                changed.set(start, true);
                i -= 1;
                changed.set(i, false);
                j -= 1;
                while other_changed.get(j) {
                    j -= 1;
                }
            }
        }
    }
}

/// `build_script`: the flags as an edit script, in file order.
pub fn build_script(files: &[FileData; 2]) -> Vec<Change> {
    let c0 = &files[0].changed;
    let c1 = &files[1].changed;
    let mut i0 = files[0].buffered_lines;
    let mut i1 = files[1].buffered_lines;
    let mut script = Vec::new();
    while i0 >= 0 || i1 >= 0 {
        if c0.get(i0.saturating_sub(1)) || c1.get(i1.saturating_sub(1)) {
            let line0 = i0;
            let line1 = i1;
            while c0.get(i0.saturating_sub(1)) {
                i0 = i0.saturating_sub(1);
            }
            while c1.get(i1.saturating_sub(1)) {
                i1 = i1.saturating_sub(1);
            }
            script.push(Change {
                line0: i0,
                line1: i1,
                deleted: line0.saturating_sub(i0),
                inserted: line1.saturating_sub(i1),
                ignore: false,
            });
        }
        i0 = i0.saturating_sub(1);
        i1 = i1.saturating_sub(1);
    }
    // Built from the end, consing onto the front: reverse to file order.
    script.reverse();
    script
}

/// `build_reverse_script`: the same, last change first, for `-e`.
pub fn build_reverse_script(files: &[FileData; 2]) -> Vec<Change> {
    let c0 = &files[0].changed;
    let c1 = &files[1].changed;
    let len0 = files[0].buffered_lines;
    let len1 = files[1].buffered_lines;
    let mut i0: Lin = 0;
    let mut i1: Lin = 0;
    let mut script = Vec::new();
    while i0 < len0 || i1 < len1 {
        if c0.get(i0) || c1.get(i1) {
            let line0 = i0;
            let line1 = i1;
            while c0.get(i0) {
                i0 = i0.saturating_add(1);
            }
            while c1.get(i1) {
                i1 = i1.saturating_add(1);
            }
            script.push(Change {
                line0,
                line1,
                deleted: i0.saturating_sub(line0),
                inserted: i1.saturating_sub(line1),
                ignore: false,
            });
        }
        i0 = i0.saturating_add(1);
        i1 = i1.saturating_add(1);
    }
    script.reverse();
    script
}

/// The comparison proper: discard, compare, shift. `minimal` is `-d`,
/// `heuristic` is `-H`.
#[allow(clippy::arithmetic_side_effects)]
pub fn compare(files: &mut [FileData; 2], minimal: bool, heuristic: bool) {
    files[0].changed = crate::io::Flags::new(files[0].buffered_lines);
    files[1].changed = crate::io::Flags::new(files[1].buffered_lines);
    discard_confusing_lines(files, minimal);

    let n0 = files[0].nondiscarded_lines;
    let n1 = files[1].nondiscarded_lines;
    let mut diags = n0 + n1 + 3;
    let size = usize::try_from(diags).unwrap_or(0);
    // TOO_EXPENSIVE: about the square root of the input size, at least 4096.
    let mut too_expensive: Lin = 1;
    while diags != 0 {
        too_expensive <<= 1;
        diags >>= 2;
    }
    // Taken, not borrowed: the comparison marks lines in `files` as it goes.
    // Nothing reads them after it.
    let xv = std::mem::take(&mut files[0].undiscarded);
    let yv = std::mem::take(&mut files[1].undiscarded);
    let mut ctxt = Context {
        xv: &xv,
        yv: &yv,
        fdiag: vec![0; size],
        bdiag: vec![0; size],
        offset: n1 + 1,
        heuristic,
        too_expensive: too_expensive.max(4096),
    };
    ctxt.compareseq(0, n0, 0, n1, minimal, files);
    shift_boundaries(files);
}
