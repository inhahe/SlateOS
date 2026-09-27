//! `table.c`'s sorting, `cell.c`'s `scols_cmpstr_cells`, and `list.h`'s
//! `list_sort`: a table's lines -- and in a tree, every line's children and
//! every group's -- ordered by a column's comparison function; and a table's
//! lines reordered so each is followed by its children.

use crate::{Cell, ColumnId, Error, LineId, Table};

/// What a comparison function is shown of a cell: its data, and the number
/// a program kept with it ([`Table::cell_set_userdata`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct CellView<'a> {
    data: Option<&'a [u8]>,
    userdata: Option<u64>,
}

impl<'a> CellView<'a> {
    /// `scols_cell_get_data`: the data, or nothing if none was set.
    #[must_use]
    pub fn data(&self) -> Option<&'a [u8]> {
        self.data
    }

    /// `scols_cell_get_userdata`: the number kept with the cell, if any.
    #[must_use]
    pub fn userdata(&self) -> Option<u64> {
        self.userdata
    }
}

/// A column's comparison function (`scols_column_set_cmpfunc`): negative,
/// zero or positive as the first cell sorts before, with, or after the
/// second.
pub type CmpFunc = fn(CellView<'_>, CellView<'_>) -> i32;

/// `scols_cmpstr_cells`: by the data, a cell with none first -- by
/// `strcoll`, which in the C and C.UTF-8 locales, the only ones this system
/// has, compares bytes.
#[must_use]
pub fn cmpstr_cells(a: CellView<'_>, b: CellView<'_>) -> i32 {
    match (a.data, b.data) {
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
        (Some(x), Some(y)) => match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        },
    }
}

/// `MAX_LIST_LENGTH_BITS`: `list_sort`'s levels of partial lists.
const MAX_LIST_LENGTH_BITS: usize = 20;

/// `merge`: two sorted runs into one, `a`'s first when they compare equal.
fn merge<F>(cmp: &mut F, a: Vec<LineId>, b: Vec<LineId>) -> Vec<LineId>
where
    F: FnMut(LineId, LineId) -> i32,
{
    let mut out = Vec::with_capacity(a.len().saturating_add(b.len()));
    let mut a = a.into_iter().peekable();
    let mut b = b.into_iter().peekable();
    while let (Some(&x), Some(&y)) = (a.peek(), b.peek()) {
        if cmp(x, y) <= 0 {
            out.push(x);
            a.next();
        } else {
            out.push(y);
            b.next();
        }
    }
    out.extend(a);
    out.extend(b);
    out
}

/// `list_sort`: upstream's bottom-up merge sort, step for step -- so that a
/// comparison function that is not a consistent order orders the lines as
/// upstream's does, and a consistent one as any stable sort would.
/// (`merge_and_restore_back_links` also calls the function on each line
/// with itself, and ignores the answer; those calls are not made.)
fn list_sort<F>(list: &mut Vec<LineId>, mut cmp: F)
where
    F: FnMut(LineId, LineId) -> i32,
{
    if list.is_empty() {
        return;
    }
    // Sorted partial lists; the last slot is a sentinel, never filled.
    let mut part: Vec<Option<Vec<LineId>>> = vec![None; MAX_LIST_LENGTH_BITS.saturating_add(1)];
    let mut max_lev = 0usize;
    for ln in std::mem::take(list) {
        let mut cur = vec![ln];
        let mut lev = 0usize;
        while let Some(p) = part.get_mut(lev).and_then(Option::take) {
            cur = merge(&mut cmp, p, cur);
            lev = lev.saturating_add(1);
        }
        if lev > max_lev {
            // "list passed to list_sort() too long for efficiency"
            if lev >= part.len().saturating_sub(1) {
                lev = lev.saturating_sub(1);
            }
            max_lev = lev;
        }
        if let Some(slot) = part.get_mut(lev) {
            *slot = Some(cur);
        }
    }
    let mut merged = Vec::new();
    for lev in 0..max_lev {
        if let Some(p) = part.get_mut(lev).and_then(Option::take) {
            merged = merge(&mut cmp, p, merged);
        }
    }
    let first = part.get_mut(max_lev).and_then(Option::take).unwrap_or_default();
    *list = merge(&mut cmp, first, merged);
}

impl Table {
    /// The line's `n`th cell, as a comparison function sees it.
    fn cell_view(&self, ln: LineId, n: usize) -> CellView<'_> {
        let cell = self.line(ln).and_then(|l| l.cells.get(n));
        CellView {
            data: cell.and_then(Cell::data),
            userdata: cell.and_then(|c| c.userdata),
        }
    }

    /// `cells_cmp_wrapper_lines` and `cells_cmp_wrapper_children`: two lines
    /// compared by their cells of the column whose cells are the `n`th.
    fn cmp_lines(&self, cmp: CmpFunc, n: usize, a: LineId, b: LineId) -> i32 {
        cmp(self.cell_view(a, n), self.cell_view(b, n))
    }

    /// `sort_line_children`: `ln`'s children sorted, each's own first --
    /// and if `ln` is its group's first member, the group's children too.
    ///
    /// Upstream recurses without end on a line whose ancestry loops, until
    /// it runs out of stack; here the depth is bounded by the number of
    /// lines, which no loop-free tree reaches.
    fn sort_line_children(&mut self, ln: LineId, cmp: CmpFunc, n: usize, depth: usize) {
        if depth > self.lines.len() {
            return;
        }
        let deeper = depth.saturating_add(1);
        let children = self
            .line(ln)
            .map(|l| l.children.clone())
            .unwrap_or_default();
        if !children.is_empty() {
            for child in children {
                self.sort_line_children(child, cmp, n, deeper);
            }
            if let Some(mut list) = self.lines.get_mut(ln.0).map(|l| std::mem::take(&mut l.children)) {
                list_sort(&mut list, |a, b| self.cmp_lines(cmp, n, a, b));
                if let Some(line) = self.lines.get_mut(ln.0) {
                    line.children = list;
                }
            }
        }
        if self.is_first_group_member(ln)
            && let Some(gr) = self.line(ln).and_then(|l| l.group)
        {
            for child in self.group_children_of(ln) {
                self.sort_line_children(child, cmp, n, deeper);
            }
            if let Some(mut list) = self.groups.get_mut(gr.0).map(|g| std::mem::take(&mut g.children)) {
                list_sort(&mut list, |a, b| self.cmp_lines(cmp, n, a, b));
                if let Some(g) = self.groups.get_mut(gr.0) {
                    g.children = list;
                }
            }
        }
    }

    /// `__scols_sort_tree`: the children of every line in the table sorted.
    fn sort_tree(&mut self, cmp: CmpFunc, n: usize) {
        for ln in self.order.clone() {
            self.sort_line_children(ln, cmp, n, 0);
        }
    }

    /// `scols_sort_table(tb, cl)`: the table's lines ordered by `cl`'s
    /// comparison function -- by the column last sorted by, without one --
    /// and in a tree, every line's and group's children too. Lines that
    /// compare equal keep their order.
    ///
    /// Children are sorted to a depth of at most the number of lines, which
    /// only a loop reaches: a line its own ancestor, or a group member its
    /// own group's child. Upstream recurses through one until its stack runs
    /// out.
    ///
    /// # Errors
    ///
    /// No column is given and none was sorted by before, the column is not
    /// this table's, or it has no comparison function
    /// ([`Table::column_set_cmpfunc`]).
    pub fn sort(&mut self, cl: Option<ColumnId>) -> Result<(), Error> {
        let cl = cl.or(self.dflt_sort_column).ok_or(Error::Invalid)?;
        let (cmp, n) = self
            .column_ref(cl)
            .and_then(|c| c.cmpfunc.map(|f| (f, c.seqnum)))
            .ok_or(Error::Invalid)?;
        let mut order = std::mem::take(&mut self.order);
        list_sort(&mut order, |a, b| self.cmp_lines(cmp, n, a, b));
        self.order = order;
        if self.is_tree() {
            self.sort_tree(cmp, n);
        }
        self.dflt_sort_column = Some(cl);
        Ok(())
    }

    /// `move_line_and_children(ln, pre)`: `ln` put after `pre` in the table
    /// (where it is, without one), and its children after it, each followed
    /// by its own. The last line so placed.
    fn move_line_and_children(&mut self, ln: LineId, pre: Option<LineId>, depth: usize) -> LineId {
        if let Some(pre) = pre {
            if let Some(i) = self.order.iter().position(|&l| l == ln) {
                self.order.remove(i);
            }
            let at = self
                .order
                .iter()
                .position(|&l| l == pre)
                .map_or(self.order.len(), |i| i.saturating_add(1));
            self.order.insert(at, ln);
        }
        let mut last = ln;
        // Bounded as `sort_line_children` is: upstream recurses without end
        // through a loop.
        if depth <= self.lines.len() {
            let children = self
                .line(ln)
                .map(|l| l.children.clone())
                .unwrap_or_default();
            for child in children {
                last = self.move_line_and_children(child, Some(last), depth.saturating_add(1));
            }
        }
        last
    }

    /// `scols_sort_table_by_tree`: the table's lines reordered so each is
    /// followed by its children -- sorted first by the column [`Table::sort`]
    /// last sorted by, if it did.
    ///
    /// As upstream's, the lines are taken in table order while they are
    /// being moved: each time from the line that followed the previous one
    /// when that one was taken, wherever it has since been moved to.
    pub fn sort_by_tree(&mut self) {
        if let Some((cmp, n)) = self
            .dflt_sort_column
            .and_then(|cl| self.column_ref(cl))
            .and_then(|c| c.cmpfunc.map(|f| (f, c.seqnum)))
        {
            self.sort_tree(cmp, n);
        }
        let mut next = self.order.first().copied();
        while let Some(ln) = next {
            next = self
                .order
                .iter()
                .position(|&l| l == ln)
                .and_then(|i| self.order.get(i.saturating_add(1)))
                .copied();
            self.move_line_and_children(ln, None, 0);
        }
    }
}
