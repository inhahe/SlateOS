//! `calculate.c`: every column's width. Off a terminal each column is as wide
//! as its widest cell (or its header); on one, columns are then shrunk to the
//! terminal -- the least typical first, by statistics over their cells -- or
//! the spare room handed out.
//!
//! Two subtractions in upstream's reduction are unsigned and can wrap (a
//! column narrower than the three cells stage 6 takes from the worst column,
//! and `reduce_to_68` taking more than a column has). The branch each one
//! decides is taken here exactly as upstream takes it; only the arithmetic
//! saturates, where upstream's would leave a column billions of cells wide.

use crate::print::Buf;
use crate::{LineId, Table, WStat, mbs};

/// `sqrtroot`: upstream's Newton iteration, kept rather than `f64::sqrt`
/// because its last bit can differ, and the sort order depends on it.
fn sqrtroot(num: f64) -> f64 {
    let mut tmp = 0.0_f64;
    let mut sq = num / 2.0;
    #[allow(
        clippy::float_cmp,
        reason = "upstream iterates until two successive values are identical"
    )]
    while sq != tmp {
        tmp = sq;
        // `(num / tmp + tmp) / 2`: `midpoint` computes exactly that for any
        // operand below `f64::MAX / 2`, which a squared cell width is.
        sq = f64::midpoint(num / tmp, tmp);
    }
    sq
}

/// `(size_t) x`, as gcc compiles it for x86-64: `cvttsd2si` below 2^63,
/// and above it the same after subtracting 2^63, with the top bit put back.
///
/// C leaves the conversion undefined outside `size_t`'s range, and this is
/// what upstream's binary does there -- which a width hint from `column
/// --table-column width=...` can reach: infinity and anything from 2^64 up
/// convert to 0 (the hint is then ignored), NaN to 2^63, and a negative
/// number wraps (-1 is `SIZE_MAX`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "this is the conversion being reproduced, bit for bit"
)]
fn to_size(x: f64) -> usize {
    /// `cvttsd2si`: truncation, or the "integer indefinite" `i64::MIN` for
    /// NaN and anything outside `i64`.
    fn cvttsd2si(x: f64) -> i64 {
        const TWO63: f64 = 9_223_372_036_854_775_808.0;
        if x.is_nan() || !(-TWO63..TWO63).contains(&x) {
            i64::MIN
        } else {
            x as i64
        }
    }
    const TWO63: f64 = 9_223_372_036_854_775_808.0;
    // `comisd x, 2^63; jnb`: NaN compares unordered and takes the first way.
    let bits = if x >= TWO63 {
        (cvttsd2si(x - TWO63) as u64) ^ (1 << 63)
    } else {
        cvttsd2si(x) as u64
    };
    bits as usize
}

/// `(double) n`.
#[allow(
    clippy::cast_precision_loss,
    reason = "widths are far below 2^52; this is C's own conversion"
)]
fn to_f64(n: usize) -> f64 {
    n as f64
}

impl Table {
    /// The width a cell or header takes: encoded, unless encoding is off.
    fn text_width(&self, data: &[u8]) -> usize {
        if self.no_encode {
            mbs::mbs_width(data, self.utf8)
        } else {
            mbs::mbs_safe_width(data, self.utf8)
        }
    }

    /// `scols_wrapnl_chunksize`: the width of the widest newline-separated
    /// piece.
    fn wrapnl_chunksize(&self, data: &[u8]) -> usize {
        mbs::c_str(data)
            .split(|&b| b == b'\n')
            .map(|piece| self.text_width(piece))
            .max()
            .unwrap_or(0)
    }

    /// `count_cell_width`.
    fn count_cell_width(&mut self, ln: LineId, cl: usize, buf: &mut Buf) {
        self.cell_to_buffer(ln, cl, buf);
        let customwrap = self
            .columns
            .get(cl)
            .is_some_and(crate::Column::is_customwrap);
        let len = if customwrap {
            self.wrapnl_chunksize(&buf.data)
        } else {
            self.text_width(&buf.data)
        };
        let treewidth = buf.safe_pointer_width(self.utf8);
        if let Some(cell) = self.cell_mut(ln, cl) {
            cell.width = len;
        }
        if let Some(col) = self.columns.get_mut(cl) {
            col.wstat.width_max = col.wstat.width_max.max(len);
            if col.is_tree() {
                col.width_treeart = col.width_treeart.max(treewidth);
            }
        }
    }

    /// `count_column_deviation`: the mean width over every line -- an
    /// integer division, as upstream's is -- and the sample deviation.
    fn count_column_deviation(&mut self, cl: usize) {
        let widths: Vec<usize> = (0..self.lines.len())
            .map(|ln| self.cell(LineId(ln), cl).map_or(0, |c| c.width))
            .collect();
        let n = widths.len();
        let Some(col) = self.columns.get_mut(cl) else {
            return;
        };
        let st = &mut col.wstat;
        let sum = widths.iter().fold(0usize, |a, &w| a.saturating_add(w));
        if let Some(avg) = sum.checked_div(n) {
            st.width_avg = to_f64(avg);
        }
        if n > 1 {
            for &w in &widths {
                let diff = to_f64(w) - st.width_avg;
                st.width_sqr_sum += diff * diff;
            }
            let variance = st.width_sqr_sum / to_f64(n.saturating_sub(1));
            st.width_deviation = sqrtroot(variance);
        }
    }

    /// `count_column_width`.
    fn count_column_width(&mut self, cl: usize, buf: &mut Buf) {
        let is_last = self.is_last_column(cl);
        let (maxout, is_term, termwidth) = (self.maxout, self.is_term, self.termwidth);
        let header = self
            .columns
            .get(cl)
            .and_then(|c| c.header.data().map(<[u8]>::to_vec));
        let header_width = header.as_deref().map(|h| self.text_width(h));
        let Some(col) = self.columns.get_mut(cl) else {
            return;
        };
        col.width = 0;
        col.wstat = WStat::default();
        if col.width_hint < 1.0 && maxout && is_term {
            let mut min = to_size(col.width_hint * to_f64(termwidth));
            if !is_last {
                min = min.saturating_sub(1);
            }
            col.wstat.width_min = min;
        }
        let no_header = header_width.is_none();
        if let Some(len) = header_width {
            col.wstat.width_min = col.wstat.width_min.max(len);
        }
        if col.wstat.width_min == 0 {
            col.wstat.width_min = 1;
        }
        // A tree is measured as it is walked, so a line that no root leads
        // to -- one whose ancestry loops -- is not measured, as it is not
        // printed; a list, line by line.
        let lines: Vec<LineId> = if self.is_tree() {
            self.walk_order()
        } else {
            (0..self.lines.len()).map(LineId).collect()
        };
        for ln in lines {
            self.count_cell_width(ln, cl, buf);
        }
        let Some(col) = self.columns.get_mut(cl) else {
            return;
        };
        let st = col.wstat;
        col.width = st.width_max;
        let hint = to_size(col.width_hint);
        if col.width < st.width_min && !col.is_strict_width() {
            col.width = st.width_min;
        } else if col.width_hint >= 1.0 && col.width < hint && st.width_min < hint {
            col.width = hint;
        }
        // A column with neither header nor data takes no room at all.
        if st.width_max == 0 && no_header && st.width_min == 1 && col.width <= 1 {
            col.width = 0;
            col.wstat.width_min = 0;
        }
    }

    /// `reduce_to_68`: toward the mean plus one deviation.
    fn reduce_to_68(&mut self, cl: usize, wanted: usize) {
        let Some(col) = self.columns.get_mut(cl) else {
            return;
        };
        let st = col.wstat;
        if st.width_deviation < 1.0 {
            return;
        }
        let new = to_size(st.width_avg + st.width_deviation).max(st.width_min);
        if col.width.wrapping_sub(new) > wanted {
            col.width = col.width.saturating_sub(wanted);
        } else {
            col.width = new;
        }
    }

    /// `reduce_column`: one column's share of stage `stage`; `Err` when there
    /// are no stages left.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's switch with its fallthroughs, kept whole to be read against it"
    )]
    fn reduce_column(
        &mut self,
        cl: usize,
        width: &mut usize,
        stage: u32,
        nth: usize,
    ) -> Result<(), ()> {
        let termwidth = self.termwidth;
        // "No more stages" -- asked first, where upstream asks it last. Its
        // early returns come before the stage switch, so when every column
        // is hidden or already at its minimum and the table still does not
        // fit, no call ever reaches the switch, `stage` climbs, and upstream
        // never finishes. Asking first ends the reduction there instead, and
        // the table prints as wide as it is; in every case upstream finishes,
        // the answer is the same, since the columns it visits before the one
        // that reaches its `default:` change nothing.
        if stage > 6 {
            return Err(());
        }
        let Some(col) = self.columns.get(cl) else {
            return Ok(());
        };
        if termwidth >= *width
            || col.is_hidden()
            || col.width == col.wstat.width_min
            || col.width == 0
            || (col.is_tree() && *width <= col.width_treeart)
        {
            return Ok(());
        }
        let org_width = col.width;
        let wanted = width.saturating_sub(termwidth);
        let st = col.wstat;
        let is_trunc = col.is_trunc() || (col.is_wrap() && !col.is_customwrap());
        let noextremes = col.is_noextremes();
        let hint = col.width_hint;
        let wide_spread = |divisor: f64| st.width_deviation >= st.width_avg / divisor;
        match stage {
            0 => {
                if (is_trunc || noextremes) && nth == 0 {
                    self.reduce_to_68(cl, wanted);
                }
            }
            1 | 2 => {
                if (stage == 2 || wide_spread(2.0)) && noextremes {
                    self.reduce_to_68(cl, wanted);
                }
            }
            3 | 4 => {
                if (stage == 4 || wide_spread(2.0))
                    && is_trunc
                    && hint > 0.0
                    && hint < 1.0
                    && org_width >= to_size(hint * to_f64(termwidth))
                {
                    self.reduce_to_68(cl, wanted);
                }
            }
            5 | 6 => {
                if (stage == 6 || wide_spread(2.2)) && (is_trunc || noextremes) {
                    // Columns are visited worst first: the worst loses more.
                    let reduce = if nth == 0 { 3 } else { 1 };
                    let reduce = reduce.min(org_width.saturating_sub(st.width_min));
                    if let Some(col) = self.columns.get_mut(cl) {
                        col.width = col.width.saturating_sub(reduce);
                    }
                }
            }
            _ => return Err(()),
        }
        if let Some(col) = self.columns.get_mut(cl) {
            if col.width == 0 {
                col.flags |= crate::FL_HIDDEN;
            }
            *width = width.saturating_sub(org_width.saturating_sub(col.width));
        }
        Ok(())
    }

    /// The deviation order: `avg + 3 * deviation`, least first.
    fn deviation_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.columns.len()).collect();
        let key = |i: usize| {
            self.columns
                .get(i)
                .map_or(0.0, |c| c.wstat.width_avg + 3.0 * c.wstat.width_deviation)
        };
        order.sort_by(|&a, &b| {
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        order
    }

    /// `__scols_calculate`.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's one function, kept whole to be read against it"
    )]
    pub(crate) fn calculate(&mut self, buf: &mut Buf) {
        self.is_dummy_print = true;
        let colsepsz = self.text_width(&self.colsep());
        let (mut width, mut width_min) = (0usize, 0usize);

        for cl in 0..self.columns.len() {
            if self.columns.get(cl).is_some_and(crate::Column::is_hidden) {
                continue;
            }
            self.count_column_width(cl, buf);
            let sep = if self.is_last_column(cl) { 0 } else { colsepsz };
            if let Some(col) = self.columns.get(cl) {
                width = width.saturating_add(col.width).saturating_add(sep);
                width_min = width_min
                    .saturating_add(col.wstat.width_min)
                    .saturating_add(sep);
            }
        }

        if !self.is_term {
            self.is_dummy_print = false;
            return;
        }

        let termwidth = self.termwidth;
        // "Be paranoid": one pass taking a cell from each minimum.
        if width_min > termwidth && self.maxout {
            for col in &mut self.columns {
                if width_min <= termwidth {
                    break;
                }
                if col.is_hidden() {
                    continue;
                }
                width_min = width_min.saturating_sub(1);
                col.wstat.width_min = col.wstat.width_min.saturating_sub(1);
            }
        }

        let mut ignore_extremes = 0usize;
        for cl in 0..self.columns.len() {
            self.count_column_deviation(cl);
            if self
                .columns
                .get(cl)
                .is_some_and(crate::Column::is_noextremes)
            {
                ignore_extremes = ignore_extremes.saturating_add(1);
            }
        }

        // Remembered before any sorting: the last column of the list, hidden
        // or not.
        let last_cl = self.columns.len().saturating_sub(1);
        let mut order: Vec<usize> = (0..self.columns.len()).collect();
        let mut sorted = false;

        // Reduce, stage by stage, until it fits or no stage is left.
        let mut stage = 0u32;
        while width > termwidth {
            let org_width = width;
            if !sorted {
                order = self.deviation_order();
                sorted = true;
            }
            let mut failed = false;
            for (nth, &cl) in order.iter().rev().enumerate() {
                if width <= termwidth {
                    break;
                }
                if self.reduce_column(cl, &mut width, stage, nth).is_err() {
                    failed = true;
                    break;
                }
            }
            if failed {
                break;
            }
            if org_width == width {
                stage = stage.saturating_add(1);
            }
        }

        // Enlarge.
        if width < termwidth {
            if ignore_extremes > 0 {
                // Nothing reads `sorted` after this: the columns go back to
                // their own order on the way out either way.
                if !sorted {
                    order = self.deviation_order();
                }
                for &cl in order.iter().rev() {
                    let Some(col) = self.columns.get_mut(cl) else {
                        continue;
                    };
                    if !col.is_noextremes() || col.is_hidden() {
                        continue;
                    }
                    if col.wstat.width_min == 0 && col.width == 0 {
                        continue;
                    }
                    let mut add = termwidth.saturating_sub(width);
                    if add > 0
                        && col.wstat.width_max > 0
                        && col.width.saturating_add(add) > col.wstat.width_max
                    {
                        // Upstream's unsigned arithmetic: a column already
                        // wider than its widest cell is brought back to it.
                        add = col.wstat.width_max.wrapping_sub(col.width);
                    }
                    if add == 0 {
                        continue;
                    }
                    col.width = col.width.wrapping_add(add);
                    width = width.wrapping_add(add);
                    if width == termwidth {
                        break;
                    }
                }
            }
            if width < termwidth && self.maxout {
                let visible = order
                    .iter()
                    .any(|&cl| self.columns.get(cl).is_some_and(|c| !c.is_hidden()));
                while visible && width < termwidth {
                    for &cl in order.iter().rev() {
                        let Some(col) = self.columns.get_mut(cl) else {
                            continue;
                        };
                        if col.is_hidden() {
                            continue;
                        }
                        col.width = col.width.saturating_add(1);
                        width = width.saturating_add(1);
                        if width == termwidth {
                            break;
                        }
                    }
                }
            } else if width < termwidth
                && let Some(col) = self.columns.get_mut(last_cl)
                && !col.is_right()
            {
                col.width = col.width.saturating_add(termwidth.saturating_sub(width));
                width = termwidth;
            }
        }

        // `nowrap`: cut the last columns, or hide them, so the line fits.
        if self.no_wrap && width > termwidth {
            for col in self.columns.iter_mut().rev() {
                if col.is_hidden() {
                    continue;
                }
                if width <= termwidth {
                    break;
                }
                if width.saturating_sub(col.width) < termwidth {
                    let r = width.saturating_sub(termwidth);
                    col.flags |= crate::FL_TRUNC;
                    col.width = col.width.saturating_sub(r);
                    width = width.saturating_sub(r);
                } else {
                    col.flags |= crate::FL_HIDDEN;
                    width = width.saturating_sub(col.width.saturating_add(colsepsz));
                }
            }
        }
        self.is_dummy_print = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstreams_square_root_is_a_square_root() {
        assert!((sqrtroot(16.0) - 4.0).abs() < 1e-12);
        assert!(sqrtroot(0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_double_becomes_a_size_as_gcc_converts_it() {
        assert_eq!(to_size(2.9), 2);
        assert_eq!(to_size(0.5), 0);
        assert_eq!(to_size(-0.5), 0);
        assert_eq!(to_size(1e19), 10_000_000_000_000_000_000);
        // Outside size_t, x86-64's answers.
        assert_eq!(to_size(-1.0), usize::MAX);
        assert_eq!(to_size(f64::NAN), 1 << 63);
        assert_eq!(to_size(f64::INFINITY), 0);
        assert_eq!(to_size(1e30), 0);
        assert_eq!(to_size(f64::NEG_INFINITY), 1 << 63);
    }
}
