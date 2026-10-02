//! NetBSD's `fmtcheck(3)`, as file 5.45 carries it in `src/fmtcheck.c`: does a
//! format take the same arguments, in the same order, as a known-good one?
//!
//! libmagic asks it of every description before printing a value through it,
//! and prints with the known-good default instead when the answer is no. The
//! answer is NetBSD's, quirks included: `%G` and `%F` are not floating point
//! conversions to it (its list is `eEfg`), so a description written with one is
//! refused, and `'` is not a flag.

/// What one directive takes (`EFT`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Eft {
    Start,
    Int,
    Long,
    Quad,
    ShortPointer,
    IntPointer,
    LongPointer,
    QuadPointer,
    Double,
    LongDouble,
    String,
    Width,
    Precision,
    Done,
    Unknown,
}

/// `get_next_format_from_precision`: the length modifier and the conversion.
/// Leaves `*pf` at the conversion character.
fn from_precision(f: &[u8], pf: &mut usize) -> Eft {
    let at = |i: usize| f.get(i).copied().unwrap_or(0);
    let mut i = *pf;
    let (mut sh, mut lg, mut quad, mut longdouble) = (false, false, false, false);
    match at(i) {
        b'h' => {
            i += 1;
            sh = true;
        }
        b'l' => {
            i += 1;
            if at(i) == 0 {
                *pf = i;
                return Eft::Unknown;
            }
            if at(i) == b'l' {
                i += 1;
                quad = true;
            } else {
                lg = true;
            }
        }
        b'q' => {
            i += 1;
            quad = true;
        }
        b'L' => {
            i += 1;
            longdouble = true;
        }
        _ => {}
    }
    *pf = i;
    let c = at(i);
    if c == 0 {
        return Eft::Unknown;
    }
    let any = sh || lg || quad || longdouble;
    if b"diouxX".contains(&c) {
        return if longdouble {
            Eft::Unknown
        } else if lg {
            Eft::Long
        } else if quad {
            Eft::Quad
        } else {
            Eft::Int
        };
    }
    if c == b'n' {
        return if longdouble {
            Eft::Unknown
        } else if sh {
            Eft::ShortPointer
        } else if lg {
            Eft::LongPointer
        } else if quad {
            Eft::QuadPointer
        } else {
            Eft::IntPointer
        };
    }
    if b"DOU".contains(&c) {
        return if any { Eft::Unknown } else { Eft::Long };
    }
    if b"eEfg".contains(&c) {
        return if longdouble {
            Eft::LongDouble
        } else if sh || lg || quad {
            Eft::Unknown
        } else {
            Eft::Double
        };
    }
    if c == b'c' {
        return if any { Eft::Unknown } else { Eft::Int };
    }
    if c == b's' {
        return if any { Eft::Unknown } else { Eft::String };
    }
    if c == b'p' {
        return if any { Eft::Unknown } else { Eft::Long };
    }
    Eft::Unknown
}

/// `get_next_format_from_width`: an optional precision, then the rest.
fn from_width(f: &[u8], pf: &mut usize) -> Eft {
    let at = |i: usize| f.get(i).copied().unwrap_or(0);
    let mut i = *pf;
    if at(i) == b'.' {
        i += 1;
        if at(i) == b'*' {
            *pf = i;
            return Eft::Precision;
        }
        // Eat any precision; an empty one is allowed.
        while at(i).is_ascii_digit() {
            i += 1;
        }
        if at(i) == 0 {
            *pf = i;
            return Eft::Unknown;
        }
    }
    *pf = i;
    from_precision(f, pf)
}

/// `get_next_format`: the next directive's argument, after `eft`.
fn next_format(f: &[u8], pf: &mut usize, eft: Eft) -> Eft {
    let at = |i: usize| f.get(i).copied().unwrap_or(0);
    if eft == Eft::Width {
        *pf += 1;
        return from_width(f, pf);
    }
    if eft == Eft::Precision {
        *pf += 1;
        return from_precision(f, pf);
    }
    let mut i = *pf;
    loop {
        // `strchr(f, '%')`.
        match f.get(i..).and_then(|rest| rest.iter().position(|&c| c == b'%')) {
            None => {
                *pf = f.len();
                return Eft::Done;
            }
            Some(k) => i += k,
        }
        i += 1;
        if at(i) == 0 {
            *pf = i;
            return Eft::Unknown;
        }
        if at(i) != b'%' {
            break;
        }
        i += 1;
    }
    // Eat any of the flags.
    while at(i) != 0 && b"#0- +".contains(&at(i)) {
        i += 1;
    }
    if at(i) == b'*' {
        *pf = i;
        return Eft::Width;
    }
    // Eat any width.
    while at(i).is_ascii_digit() {
        i += 1;
    }
    if at(i) == 0 {
        *pf = i;
        return Eft::Unknown;
    }
    *pf = i;
    from_width(f, pf)
}

/// `fmtcheck(f1, f2)`: `f1` when every directive it has takes what `f2`'s
/// takes in the same position, else `f2`. Both are C strings: they end at
/// their first NUL.
#[must_use]
pub fn fmtcheck<'a>(f1: &'a [u8], f2: &'a [u8]) -> &'a [u8] {
    let f1c = cut(f1);
    let f2c = cut(f2);
    let (mut p1, mut p2) = (0usize, 0usize);
    let (mut t1, mut t2) = (Eft::Start, Eft::Start);
    loop {
        t1 = next_format(f1c, &mut p1, t1);
        if t1 == Eft::Done {
            return f1;
        }
        if t1 == Eft::Unknown {
            return f2;
        }
        t2 = next_format(f2c, &mut p2, t2);
        if t1 != t2 {
            return f2;
        }
    }
}

fn cut(s: &[u8]) -> &[u8] {
    s.get(..s.iter().position(|&c| c == 0).unwrap_or(s.len()))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(desc: &str, def: &str) -> bool {
        std::ptr::eq(fmtcheck(desc.as_bytes(), def.as_bytes()), desc.as_bytes())
    }

    #[test]
    fn compatible_formats_are_kept() {
        assert!(ok("version %d", "%d"));
        assert!(ok("%x", "%u"));
        assert!(ok("%c", "%d"));
        assert!(ok("%-10.3s", "%s"));
        assert!(ok("%#llx", "%llu"));
        assert!(ok("no format at all", "%s"));
        assert!(ok("100%% sure %d", "%d"));
        assert!(ok("%.2f", "%g"));
    }

    #[test]
    fn netbsds_refusals_are_kept_too() {
        assert!(!ok("%s", "%d"));
        assert!(!ok("%d %d", "%d"));
        assert!(!ok("%lld", "%d"));
        assert!(!ok("%G", "%g"));
        assert!(!ok("%F", "%g"));
        assert!(!ok("%'d", "%d"));
        assert!(!ok("trailing %", "%d"));
    }
}
