//! Two more of `lib/strutils.c`'s parsers: `ul_optstr_next`, which splits
//! a mount-style options string (`trunc,json=number,name="A B"`) -- the
//! form libsmartcols' column properties take -- and `parse_range`, which
//! reads `N-M` (as `column --table-hide 2-4` does).
//!
//! Both are kept exactly, quirks included, because what they accept is
//! what a user can type: `parse_range` takes a number followed by anything
//! but `-` or `:` as the range of that one number (`5x` is `5-5`), and
//! `ul_optstr_next` keeps a value's quotes (`name="A B"` names a column
//! `"A B"`, quotes and all).

use crate::{NumErr, scan_integer};

/// `ul_optstr_next`'s `-EINVAL`: an item that ends before it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptstrInvalid;

/// One `name[=value]` item of an options string, by indices into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptstrItem {
    /// Where the name starts.
    pub name: usize,
    /// Its length: up to the first `=`, or the whole item.
    pub namesz: usize,
    /// Where the value starts -- after the `=` -- if there is one.
    pub value: Option<usize>,
    /// Its length, to the end of the item.
    pub valsz: usize,
}

/// `ul_optstr_next(&optstr, &name, &namesz, &value, &valsz)`: the item that
/// starts at `*pos` in `s`, with `*pos` moved past it and its comma.
///
/// Upstream's scan: leading commas are skipped; a `"` opens or closes a
/// quoted stretch, in which `,` and `=` are plain bytes (and whose quotes
/// stay part of the name or value); the name ends at the first `=` after
/// its first byte; a comma after a backslash does not end the item. A
/// quote left open runs to the end of the string, and the item is then
/// never returned: the string is at its end.
///
/// # Errors
///
/// Upstream's `-EINVAL`, for an item that ends before it starts -- which
/// its own scan never produces, and which is kept so the result reads as
/// upstream's does.
pub fn ul_optstr_next(s: &[u8], pos: &mut usize) -> Result<Option<OptstrItem>, OptstrInvalid> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut optstr0 = *pos;
    while at(optstr0) == b',' {
        optstr0 = optstr0.saturating_add(1);
    }
    let mut start: Option<usize> = None;
    let mut sep: Option<usize> = None;
    let mut open_quote = false;
    let mut p = optstr0;
    while at(p) != 0 {
        let begin = *start.get_or_insert(p);
        let c = at(p);
        if c == b'"' {
            open_quote = !open_quote;
        }
        if open_quote {
            p = p.saturating_add(1);
            continue;
        }
        if sep.is_none() && p > begin && c == b'=' {
            sep = Some(p);
        }
        let stop = if c == b',' && (p == optstr0 || at(p.saturating_sub(1)) != b'\\') {
            Some(p)
        } else if at(p.saturating_add(1)) == 0 {
            Some(p.saturating_add(1))
        } else {
            None
        };
        let Some(stop) = stop else {
            p = p.saturating_add(1);
            continue;
        };
        if stop <= begin {
            return Err(OptstrInvalid);
        }
        *pos = if at(stop) == 0 {
            stop
        } else {
            stop.saturating_add(1)
        };
        return Ok(Some(match sep {
            Some(sep) => OptstrItem {
                name: begin,
                namesz: sep.saturating_sub(begin),
                value: Some(sep.saturating_add(1)),
                valsz: stop.saturating_sub(sep).saturating_sub(1),
            },
            None => OptstrItem {
                name: begin,
                namesz: stop.saturating_sub(begin),
                value: None,
                valsz: 0,
            },
        }));
    }
    Ok(None)
}

/// `strtol(s, &end, 10)` as `parse_range` uses it: the value and where the
/// digits end, or the refusal -- no conversion, or `ERANGE`.
fn strtol(s: &[u8]) -> Result<(i64, usize), NumErr> {
    let sc = scan_integer(s, 10).ok_or(NumErr::Invalid)?;
    let limit: u128 = if sc.negative {
        1u128 << 63
    } else {
        (1u128 << 63).saturating_sub(1)
    };
    if sc.saturated || sc.magnitude > limit {
        return Err(NumErr::Range);
    }
    let magnitude = i128::try_from(sc.magnitude).map_err(|_| NumErr::Range)?;
    let value = if sc.negative {
        magnitude.checked_neg().ok_or(NumErr::Range)?
    } else {
        magnitude
    };
    Ok((i64::try_from(value).map_err(|_| NumErr::Range)?, sc.end))
}

/// `parse_range(str, &lower, &upper, def)`: `[[<low>]:][<high>]` or
/// `[[<low>]-][<high>]`, as `(lower, upper)`.
///
/// Upstream's forms: `:N` is `(def, N)`; `M` alone is `(M, M)` -- and so
/// is `M` followed by anything but `-` or `:`, which nothing checks
/// (`5x` is `(5, 5)`); `M:` is `(M, def)`; `M-N` and `M:N` are `(M, N)`.
/// Each number is `strtol`'s, then stored in an `int`: truncated, as C
/// truncates it.
///
/// # Errors
///
/// Upstream's `-1`, whose causes are told apart here as `strtol`'s
/// `errno` would: [`NumErr::Range`] for a number `long` cannot hold,
/// [`NumErr::Invalid`] for no number where one is needed, or anything after
/// the second.
#[allow(
    clippy::cast_possible_truncation,
    reason = "upstream stores strtol's long in an int, which gcc truncates; this is that conversion"
)]
pub fn parse_range(s: &[u8], def: i32) -> Result<(i32, i32), NumErr> {
    let whole = |t: &[u8]| -> Result<i32, NumErr> {
        match strtol(t)? {
            (v, end) if end == t.len() => Ok(v as i32),
            _ => Err(NumErr::Invalid),
        }
    };
    if let Some(rest) = s.strip_prefix(b":") {
        return Ok((def, whole(rest)?));
    }
    let (v, end) = strtol(s)?;
    let lower = v as i32;
    let after = s.get(end..).unwrap_or_default();
    if after == b":" {
        return Ok((lower, def));
    }
    match after.split_first() {
        Some((b'-' | b':', rest)) => Ok((lower, whole(rest)?)),
        _ => Ok((lower, lower)),
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

    /// Every item of `s` as `(name, value)`.
    fn items(s: &[u8]) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        let mut pos = 0;
        let mut out = Vec::new();
        while let Ok(Some(it)) = ul_optstr_next(s, &mut pos) {
            let name = s[it.name..it.name + it.namesz].to_vec();
            let value = it.value.map(|v| s[v..v + it.valsz].to_vec());
            out.push((name, value));
        }
        out
    }

    fn item(name: &str, value: Option<&str>) -> (Vec<u8>, Option<Vec<u8>>) {
        (
            name.as_bytes().to_vec(),
            value.map(|v| v.as_bytes().to_vec()),
        )
    }

    #[test]
    fn items_split_at_commas_and_names_at_the_first_equals() {
        assert_eq!(
            items(b"trunc,json=number,,name=a=b"),
            vec![
                item("trunc", None),
                item("json", Some("number")),
                item("name", Some("a=b")),
            ]
        );
        assert_eq!(items(b",,,right,"), vec![item("right", None)]);
        assert_eq!(items(b""), vec![]);
        // `=` as an item's first byte is part of its name.
        assert_eq!(items(b"=x"), vec![item("=x", None)]);
        assert_eq!(items(b"width="), vec![item("width", Some(""))]);
    }

    #[test]
    fn quotes_protect_commas_and_are_kept() {
        assert_eq!(
            items(br#"name="A,B",right"#),
            vec![item("name", Some("\"A,B\"")), item("right", None)]
        );
        // A quote left open swallows the rest, which is then never returned.
        assert_eq!(items(br#"right,name="A,B"#), vec![item("right", None)]);
        // A comma after a backslash does not end the item.
        assert_eq!(
            items(br"name=a\,b,x"),
            vec![item("name", Some(r"a\,b")), item("x", None)]
        );
    }

    #[test]
    fn the_position_after_the_last_item_is_the_end() {
        let s = b"a,width=5";
        let mut pos = 0;
        ul_optstr_next(s, &mut pos).unwrap();
        assert_eq!(pos, 2);
        ul_optstr_next(s, &mut pos).unwrap();
        assert_eq!(pos, s.len());
        assert_eq!(ul_optstr_next(s, &mut pos), Ok(None));
    }

    #[test]
    fn ranges_as_parse_range_reads_them() {
        assert_eq!(parse_range(b"2-4", 0), Ok((2, 4)));
        assert_eq!(parse_range(b"2:4", 0), Ok((2, 4)));
        assert_eq!(parse_range(b"-1", 0), Ok((-1, -1)));
        assert_eq!(parse_range(b"-3--1", 0), Ok((-3, -1)));
        assert_eq!(parse_range(b":5", 7), Ok((7, 5)));
        assert_eq!(parse_range(b"3:", 7), Ok((3, 7)));
        // Nothing checks what follows a lone number.
        assert_eq!(parse_range(b"5x-3", 0), Ok((5, 5)));
        assert_eq!(parse_range(b"2-", 0), Err(NumErr::Invalid));
        assert_eq!(parse_range(b"2-3x", 0), Err(NumErr::Invalid));
        assert_eq!(parse_range(b"a-b", 0), Err(NumErr::Invalid));
        assert_eq!(
            parse_range(b"99999999999999999999-1", 0),
            Err(NumErr::Range)
        );
        // A long stored in an int.
        assert_eq!(parse_range(b"4294967297-4294967298", 0), Ok((1, 2)));
    }
}
