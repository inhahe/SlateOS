//! gnulib's `diffseq.h`, as patch 2.7.6 bundles it: the Myers O(ND)
//! difference between two sequences, with Eggert's heuristics for large
//! ones, reporting each element deleted from the first or inserted from the
//! second. `--merge` runs it between a hunk's old lines and the stretch of
//! the file they were found at.
//!
//! The sequences are abstract: a [`Seq`] says whether two elements are
//! equal and records the edits, as the C's `XVECREF_YVECREF_EQUAL`,
//! `NOTE_DELETE` and `NOTE_INSERT` macros do.

use crate::Lin;

/// `OFFSET_MAX`.
const OFFSET_MAX: Lin = Lin::MAX;

/// `SNAKE_LIMIT`: snakes bigger than this are considered "big".
const SNAKE_LIMIT: Lin = 20;

/// The two sequences being compared, and where the edits go.
pub trait Seq {
    /// `XVECREF_YVECREF_EQUAL (ctxt, x, y)`.
    fn equal(&mut self, x: Lin, y: Lin) -> bool;
    /// `NOTE_DELETE (ctxt, x)`.
    fn note_delete(&mut self, x: Lin);
    /// `NOTE_INSERT (ctxt, y)`.
    fn note_insert(&mut self, y: Lin);
}

/// `struct context`'s search state: the furthest-reaching points along each
/// diagonal, forward and backward, indexed by diagonal plus `off`.
pub struct Context {
    fdiag: Vec<Lin>,
    bdiag: Vec<Lin>,
    off: Lin,
    /// `heuristic`: the `diff --speed-large-files` heuristic.
    pub heuristic: bool,
    /// `too_expensive`: the edit cost past which the search gives up and
    /// takes the best point so far.
    pub too_expensive: Lin,
}

/// `struct partition`.
struct Partition {
    xmid: Lin,
    ymid: Lin,
    lo_minimal: bool,
    hi_minimal: bool,
}

// Diagonal and cost arithmetic, bounded by the sequences' lengths as the C
// bounds it.
#[allow(clippy::arithmetic_side_effects)]
impl Context {
    /// The vectors for sequences ending at `xmax` and `ymax`: as
    /// `compute_changes` sizes them, `xmax + ymax + 3` diagonals each, from
    /// `-(ymax + 1)`.
    pub fn new(xmax: Lin, ymax: Lin) -> Self {
        let diags = usize::try_from(xmax + ymax + 3).unwrap_or(0);
        Self {
            fdiag: vec![0; diags],
            bdiag: vec![0; diags],
            off: ymax + 1,
            heuristic: true,
            too_expensive: OFFSET_MAX,
        }
    }

    fn at(&self, d: Lin) -> Option<usize> {
        usize::try_from(d + self.off).ok()
    }

    fn fd(&self, d: Lin) -> Lin {
        self.at(d)
            .and_then(|i| self.fdiag.get(i))
            .copied()
            .unwrap_or(-1)
    }

    fn set_fd(&mut self, d: Lin, v: Lin) {
        if let Some(slot) = self.at(d).and_then(|i| self.fdiag.get_mut(i)) {
            *slot = v;
        }
    }

    fn bd(&self, d: Lin) -> Lin {
        self.at(d)
            .and_then(|i| self.bdiag.get(i))
            .copied()
            .unwrap_or(OFFSET_MAX)
    }

    fn set_bd(&mut self, d: Lin, v: Lin) {
        if let Some(slot) = self.at(d).and_then(|i| self.bdiag.get_mut(i)) {
            *slot = v;
        }
    }
}

/// `diag`: the midpoint of the shortest edit script for `[xoff, xlim)` and
/// `[yoff, ylim)`, whose first and last elements are known not to match.
#[allow(clippy::arithmetic_side_effects)]
fn diag<S: Seq>(
    s: &mut S,
    ctxt: &mut Context,
    xoff: Lin,
    xlim: Lin,
    yoff: Lin,
    ylim: Lin,
    find_minimal: bool,
) -> Partition {
    let dmin = xoff - ylim; // Minimum valid diagonal.
    let dmax = xlim - yoff; // Maximum valid diagonal.
    let fmid = xoff - yoff; // Center diagonal of top-down search.
    let bmid = xlim - ylim; // Center diagonal of bottom-up search.
    let mut fmin = fmid;
    let mut fmax = fmid; // Limits of top-down search.
    let mut bmin = bmid;
    let mut bmax = bmid; // Limits of bottom-up search.
    // True if southeast corner is on an odd diagonal with respect to the
    // northwest.
    let odd = (fmid - bmid) & 1 != 0;

    ctxt.set_fd(fmid, xoff);
    ctxt.set_bd(bmid, xlim);

    let mut c: Lin = 1;
    loop {
        let mut big_snake = false;

        // Extend the top-down search by an edit step in each diagonal.
        if fmin > dmin {
            fmin -= 1;
            ctxt.set_fd(fmin - 1, -1);
        } else {
            fmin += 1;
        }
        if fmax < dmax {
            fmax += 1;
            ctxt.set_fd(fmax + 1, -1);
        } else {
            fmax -= 1;
        }
        let mut d = fmax;
        while d >= fmin {
            let tlo = ctxt.fd(d - 1);
            let thi = ctxt.fd(d + 1);
            let x0 = if tlo < thi { thi } else { tlo + 1 };
            let mut x = x0;
            let mut y = x0 - d;
            while x < xlim && y < ylim && s.equal(x, y) {
                x += 1;
                y += 1;
            }
            if x - x0 > SNAKE_LIMIT {
                big_snake = true;
            }
            ctxt.set_fd(d, x);
            if odd && bmin <= d && d <= bmax && ctxt.bd(d) <= x {
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
            ctxt.set_bd(bmin - 1, OFFSET_MAX);
        } else {
            bmin += 1;
        }
        if bmax < dmax {
            bmax += 1;
            ctxt.set_bd(bmax + 1, OFFSET_MAX);
        } else {
            bmax -= 1;
        }
        let mut d = bmax;
        while d >= bmin {
            let tlo = ctxt.bd(d - 1);
            let thi = ctxt.bd(d + 1);
            let x0 = if tlo < thi { tlo } else { thi - 1 };
            let mut x = x0;
            let mut y = x0 - d;
            while xoff < x && yoff < y && s.equal(x - 1, y - 1) {
                x -= 1;
                y -= 1;
            }
            if x0 - x > SNAKE_LIMIT {
                big_snake = true;
            }
            ctxt.set_bd(d, x);
            if !odd && fmin <= d && d <= fmax && x <= ctxt.fd(d) {
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

        // Heuristic: check occasionally for a diagonal that has made lots
        // of progress compared with the edit distance. If we have any such,
        // find the one that has made the most progress and return it as if
        // it had succeeded.
        if 200 < c && big_snake && ctxt.heuristic {
            let mut part = Partition {
                xmid: 0,
                ymid: 0,
                lo_minimal: false,
                hi_minimal: false,
            };
            let mut best: Lin = 0;
            let mut d = fmax;
            while d >= fmin {
                let dd = d - fmid;
                let x = ctxt.fd(d);
                let y = x - d;
                let v = (x - xoff) * 2 - dd;
                if v > 12 * (c + dd.abs())
                    && v > best
                    && xoff + SNAKE_LIMIT <= x
                    && x < xlim
                    && yoff + SNAKE_LIMIT <= y
                    && y < ylim
                {
                    // We have a good enough best diagonal; now insist that
                    // it end with a significant snake.
                    let mut k: Lin = 1;
                    while s.equal(x - k, y - k) {
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
                part.lo_minimal = true;
                part.hi_minimal = false;
                return part;
            }

            best = 0;
            let mut d = bmax;
            while d >= bmin {
                let dd = d - bmid;
                let x = ctxt.bd(d);
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
                    while s.equal(x + k, y + k) {
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
                part.lo_minimal = false;
                part.hi_minimal = true;
                return part;
            }
        }

        // Heuristic: if we've gone well beyond the call of duty, give up and
        // report halfway between our best results so far.
        if c >= ctxt.too_expensive {
            // Find forward diagonal that maximizes X + Y.
            let mut fxybest: Lin = -1;
            let mut fxbest: Lin = 0;
            let mut d = fmax;
            while d >= fmin {
                let mut x = ctxt.fd(d).min(xlim);
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

            // Find backward diagonal that minimizes X + Y.
            let mut bxybest: Lin = OFFSET_MAX;
            let mut bxbest: Lin = 0;
            let mut d = bmax;
            while d >= bmin {
                let mut x = xoff.max(ctxt.bd(d));
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

            // Use the better of the two diagonals.
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

/// `compareseq`: the differences between `[xoff, xlim)` and `[yoff, ylim)`,
/// reported through `s`.
#[allow(clippy::arithmetic_side_effects)]
pub fn compareseq<S: Seq>(
    s: &mut S,
    ctxt: &mut Context,
    mut xoff: Lin,
    mut xlim: Lin,
    mut yoff: Lin,
    mut ylim: Lin,
    find_minimal: bool,
) {
    // Slide down the bottom initial diagonal.
    while xoff < xlim && yoff < ylim && s.equal(xoff, yoff) {
        xoff += 1;
        yoff += 1;
    }

    // Slide up the top initial diagonal.
    while xoff < xlim && yoff < ylim && s.equal(xlim - 1, ylim - 1) {
        xlim -= 1;
        ylim -= 1;
    }

    // Handle simple cases.
    if xoff == xlim {
        while yoff < ylim {
            s.note_insert(yoff);
            yoff += 1;
        }
    } else if yoff == ylim {
        while xoff < xlim {
            s.note_delete(xoff);
            xoff += 1;
        }
    } else {
        // Find a point of correspondence in the middle of the vectors.
        let part = diag(s, ctxt, xoff, xlim, yoff, ylim, find_minimal);

        // Use the partitions to split this problem into subproblems.
        compareseq(s, ctxt, xoff, part.xmid, yoff, part.ymid, part.lo_minimal);
        compareseq(s, ctxt, part.xmid, xlim, part.ymid, ylim, part.hi_minimal);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// Two byte strings, the edits marked as `NOTE_DELETE`/`NOTE_INSERT`
    /// would mark them.
    struct Strs {
        x: Vec<u8>,
        y: Vec<u8>,
        xmark: Vec<u8>,
        ymark: Vec<u8>,
    }

    impl Seq for Strs {
        fn equal(&mut self, x: Lin, y: Lin) -> bool {
            self.x[x as usize] == self.y[y as usize]
        }
        fn note_delete(&mut self, x: Lin) {
            self.xmark[x as usize] = b'-';
        }
        fn note_insert(&mut self, y: Lin) {
            self.ymark[y as usize] = b'+';
        }
    }

    fn marks(a: &str, b: &str) -> (String, String) {
        let mut s = Strs {
            x: a.as_bytes().to_vec(),
            y: b.as_bytes().to_vec(),
            xmark: vec![b' '; a.len()],
            ymark: vec![b' '; b.len()],
        };
        let mut ctxt = Context::new(a.len() as Lin, b.len() as Lin);
        compareseq(
            &mut s,
            &mut ctxt,
            0,
            a.len() as Lin,
            0,
            b.len() as Lin,
            false,
        );
        (
            String::from_utf8(s.xmark).unwrap(),
            String::from_utf8(s.ymark).unwrap(),
        )
    }

    #[test]
    fn edits_are_the_shortest_script() {
        assert_eq!(marks("abc", "abc"), ("   ".into(), "   ".into()));
        assert_eq!(marks("abc", "axc"), (" - ".into(), " + ".into()));
        assert_eq!(marks("abcd", "acd"), (" -  ".into(), "   ".into()));
        assert_eq!(marks("ac", "abc"), ("  ".into(), " + ".into()));
        assert_eq!(marks("", "ab"), (String::new(), "++".into()));
        // Myers' own example: ABCABBA to CBABAC takes five edits.
        let (x, y) = marks("ABCABBA", "CBABAC");
        let edits = x.matches('-').count() + y.matches('+').count();
        assert_eq!(edits, 5);
    }
}
