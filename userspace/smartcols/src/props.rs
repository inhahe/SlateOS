//! `scols_column_set_properties`: a column described by a string, as
//! `column --table-column` takes it -- `name=SIZE,right,json=number`.
//!
//! Upstream's parsing is kept with its three flaws, because each changes
//! what a user sees:
//!
//! * **Names are matched as abbreviations**, by `strncmp` over the given
//!   name's length and in a fixed order: `t` is `trunc`, `r` is `right`,
//!   `n` is `noextremes` (even as `n=X`), `w` is `wrap` (even as `w=5`).
//! * **`noextremes` sets `SCOLS_FL_STRICTWIDTH`**, not `SCOLS_FL_NOEXTREMES`.
//! * **`width=` tests an `errno` nothing cleared**, and compares where the
//!   number ended with where the *next item* starts. So `width=` refuses
//!   the whole string (`-EINVAL`) whenever `errno` is already set -- which in
//!   upstream's `column` it usually is, by the terminal-size probe on a
//!   pipe, or by locale loading -- and whenever it is the last item and its
//!   number runs to the end of the string. The refusal is immediate: flags
//!   gathered before it are dropped, while a `name=` before it has already
//!   been applied. The caller passes its model of `errno` (see `column`'s
//!   port for where upstream's comes from), and gets back what `strtod`
//!   left in it.

use crate::{
    ColumnId, Error, FL_HIDDEN, FL_RIGHT, FL_STRICTWIDTH, FL_TREE, FL_TRUNC, FL_WRAP, JsonType,
    Table,
};
use ulstrutils::{strtod, ul_optstr_next};

/// `ERANGE`, on Linux and in the SlateOS C library.
const ERANGE: i32 = 34;

/// `strncmp(a, b, n) == 0`, with the end of either slice as C's NUL.
fn strncmp_eq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let ca = a.get(i).copied().unwrap_or(0);
        let cb = b.get(i).copied().unwrap_or(0);
        if ca != cb {
            return false;
        }
        if ca == 0 {
            return true;
        }
    }
    true
}

impl Table {
    /// `scols_column_set_properties(cl, opts)`: `trunc`, `tree`, `right`,
    /// `strictwidth`, `noextremes`, `hidden` and `wrap` gathered into the
    /// column's flags -- set, replacing what it had, once the whole string
    /// is read -- and `json=TYPE`, `width=HINT`, `color=COLOR` and
    /// `name=NAME` applied as they come. Anything else is ignored.
    ///
    /// `errno` is the caller's `errno`, read by `width=` and updated with
    /// its `strtod`'s `ERANGE`. Colours are not ported: `color=` is read
    /// and ignored, as a table printed without colours ignores it.
    ///
    /// # Errors
    ///
    /// The column is not this table's; or a `width=` that upstream refuses
    /// (see the module docs), after which nothing more of `opts` applies.
    pub fn column_set_properties(
        &mut self,
        cl: ColumnId,
        opts: &[u8],
        errno: &mut i32,
    ) -> Result<(), Error> {
        if self.column_flags(cl).is_none() {
            return Err(Error::Invalid);
        }
        let mut pos = 0usize;
        let mut flags = 0u32;
        while let Ok(Some(item)) = ul_optstr_next(opts, &mut pos) {
            let name = opts.get(item.name..).unwrap_or_default();
            let is = |word: &[u8]| strncmp_eq(name, word, item.namesz);
            let value = item.value.map(|v| (v, opts.get(v..).unwrap_or_default()));
            if is(b"trunc") {
                flags |= FL_TRUNC;
            } else if is(b"tree") {
                flags |= FL_TREE;
            } else if is(b"right") {
                flags |= FL_RIGHT;
            } else if is(b"strictwidth") || is(b"noextremes") {
                flags |= FL_STRICTWIDTH;
            } else if is(b"hidden") {
                flags |= FL_HIDDEN;
            } else if is(b"wrap") {
                flags |= FL_WRAP;
            } else if let Some((_, text)) = value
                && is(b"json")
            {
                let of = |word: &[u8]| strncmp_eq(text, word, item.valsz);
                let ty = if of(b"string") {
                    Some(JsonType::String)
                } else if of(b"number") {
                    Some(JsonType::Number)
                } else if of(b"array-string") {
                    Some(JsonType::ArrayString)
                } else if of(b"array-number") {
                    Some(JsonType::ArrayNumber)
                } else if of(b"boolean") {
                    Some(JsonType::Boolean)
                } else {
                    None
                };
                if let Some(ty) = ty {
                    self.column_set_json_type(cl, ty)?;
                }
            } else if let Some((start, text)) = value
                && is(b"width")
            {
                // `strtod(value, &end)` reads past the item, into the rest of
                // the string, as the value is not terminated where it ends.
                let number = strtod(text);
                if number.erange {
                    *errno = ERANGE;
                }
                let end = start.saturating_add(number.end);
                if *errno != 0 || pos == end {
                    return Err(Error::Invalid);
                }
                self.column_set_whint(cl, number.value)?;
            } else if value.is_some() && is(b"color") {
                // Read, and without colours nothing to do.
            } else if let Some((_, text)) = value
                && is(b"name")
            {
                let name = text.get(..item.valsz).unwrap_or(text);
                self.column_set_name(cl, Some(name))?;
            }
        }
        if flags != 0 {
            self.column_set_flags(cl, flags)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn column_with(opts: &str, errno: i32) -> (Table, ColumnId, Result<(), Error>, i32) {
        let mut tb = Table::new();
        let cl = tb.new_unnamed_column(0.0, 0);
        let mut errno = errno;
        let rc = tb.column_set_properties(cl, opts.as_bytes(), &mut errno);
        (tb, cl, rc, errno)
    }

    #[test]
    fn flags_and_names_are_applied() {
        let (tb, cl, rc, _) = column_with("name=SIZE,right,trunc", 0);
        assert_eq!(rc, Ok(()));
        assert_eq!(tb.column_name(cl), Some(&b"SIZE"[..]));
        assert_eq!(tb.column_flags(cl), Some(FL_RIGHT | FL_TRUNC));
    }

    #[test]
    fn names_are_abbreviations_in_upstreams_order() {
        assert_eq!(
            column_with("t", 0).0.column_flags(ColumnId(0)),
            Some(FL_TRUNC)
        );
        assert_eq!(
            column_with("tre", 0).0.column_flags(ColumnId(0)),
            Some(FL_TREE)
        );
        assert_eq!(
            column_with("r", 0).0.column_flags(ColumnId(0)),
            Some(FL_RIGHT)
        );
        // `w=5` is `wrap`, not `width`; `n=X` is `noextremes`, not `name`.
        assert_eq!(
            column_with("w=5", 0).0.column_flags(ColumnId(0)),
            Some(FL_WRAP)
        );
        let (tb, cl, _, _) = column_with("n=X", 0);
        assert_eq!(tb.column_flags(cl), Some(FL_STRICTWIDTH));
        assert_eq!(tb.column_name(cl), None);
        // Longer than the property is no match.
        assert_eq!(
            column_with("truncate", 0).0.column_flags(ColumnId(0)),
            Some(0)
        );
    }

    #[test]
    fn noextremes_is_strictwidth() {
        let (tb, cl, _, _) = column_with("noextremes", 0);
        assert_eq!(tb.column_flags(cl), Some(FL_STRICTWIDTH));
    }

    #[test]
    fn width_is_refused_on_a_stale_errno_or_as_the_last_item() {
        // With errno clear and an item after it, the hint is set.
        let (tb, cl, rc, _) = column_with("width=5,right", 0);
        assert_eq!(rc, Ok(()));
        assert_eq!(tb.columns[0].width_hint.to_bits(), 5.0f64.to_bits());
        assert_eq!(tb.column_flags(cl), Some(FL_RIGHT));
        // Any errno refuses it, and the flags gathered so far are lost; a
        // name before it stays.
        let (tb, cl, rc, _) = column_with("name=A,right,width=5,x", 25);
        assert_eq!(rc, Err(Error::Invalid));
        assert_eq!(tb.column_flags(cl), Some(0));
        assert_eq!(tb.column_name(cl), Some(&b"A"[..]));
        // Last, its number ends where the next item would start.
        assert_eq!(column_with("right,width=5", 0).2, Err(Error::Invalid));
        // An empty value, last: nothing read, and the end is the value.
        assert_eq!(column_with("width=", 0).2, Err(Error::Invalid));
        // Not a number: the hint is 0 and all is well.
        let (tb, _, rc, _) = column_with("width=abc,right", 0);
        assert_eq!(rc, Ok(()));
        assert_eq!(tb.columns[0].width_hint.to_bits(), 0.0f64.to_bits());
        // Out of range: strtod's ERANGE, left in errno.
        let (_, _, rc, errno) = column_with("width=1e999,right", 0);
        assert_eq!((rc, errno), (Err(Error::Invalid), ERANGE));
    }

    #[test]
    fn json_types_by_abbreviation() {
        let (tb, _, _, _) = column_with("json=num", 0);
        assert_eq!(tb.columns[0].json_type, JsonType::Number);
        let (tb, _, _, _) = column_with("json=a", 0);
        assert_eq!(tb.columns[0].json_type, JsonType::ArrayString);
        let (tb, _, _, _) = column_with("json=", 0);
        assert_eq!(tb.columns[0].json_type, JsonType::String);
        let (tb, _, _, _) = column_with("json=bool", 0);
        assert_eq!(tb.columns[0].json_type, JsonType::Boolean);
        let (tb, _, _, _) = column_with("json=xyz", 0);
        assert_eq!(tb.columns[0].json_type, JsonType::String);
    }

    #[test]
    fn a_quoted_name_keeps_its_quotes() {
        let (tb, cl, _, _) = column_with(r#"name="A,B""#, 0);
        assert_eq!(tb.column_name(cl), Some(&b"\"A,B\""[..]));
    }
}
