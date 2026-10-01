//! util-linux's `lib/match.c`: whether a filesystem type is in a list of
//! types, as `mount -t`, `findmnt -t`, `fsck -t` and `wipefs -t` take one.

/// `strncasecmp(a, b, n) == 0` over C strings: at most `n` bytes, ASCII
/// case ignored, stopping at a NUL (bytes past a slice's end read as NUL).
fn strncaseeq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if !x.eq_ignore_ascii_case(&y) {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

/// `match_fstype(type, pattern)`: whether `ty` is in a comma-separated list
/// of types, compared without case; a leading `no` inverts the whole list,
/// and a `no` before one item excludes that one (`nofoo,bar` means
/// `nofoo,nobar`). No type and no pattern match; a type without a pattern
/// does not.
#[must_use]
pub fn match_fstype(ty: Option<&[u8]>, pattern: Option<&[u8]>) -> bool {
    let (Some(ty), Some(pattern)) = (ty, pattern) else {
        return ty.is_none() && pattern.is_none();
    };
    let (no, pattern) = match pattern.strip_prefix(b"no") {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let len = ty.len();
    let at = |p: &[u8], i: usize| p.get(i).copied().unwrap_or(0);
    let mut p = pattern;
    loop {
        if p.starts_with(b"no") && strncaseeq(p.get(2..).unwrap_or_default(), ty, len) {
            let end = at(p, len.saturating_add(2));
            if end == 0 || end == b',' {
                return false;
            }
        }
        if strncaseeq(p, ty, len) {
            let end = at(p, len);
            if end == 0 || end == b',' {
                return !no;
            }
        }
        match p.iter().position(|&b| b == b',') {
            Some(i) => p = p.get(i.saturating_add(1)..).unwrap_or_default(),
            None => break,
        }
    }
    no
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_lists_match_as_upstream() {
        let m = |t: &str, p: &str| match_fstype(Some(t.as_bytes()), Some(p.as_bytes()));
        assert!(m("ext4", "ext4"));
        assert!(m("ext4", "xfs,EXT4"));
        assert!(!m("ext4", "noext4"));
        assert!(m("ext4", "noxfs"));
        assert!(!m("ext4", "noxfs,ext4"));
        assert!(!m("ext4", "xfs,noext4"));
        assert!(!m("ext4", "ext"));
        assert!(!m("ext", "ext4"));
        assert!(match_fstype(None, None));
        assert!(!match_fstype(Some(b"ext4"), None));
    }
}
