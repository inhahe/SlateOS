//! The compiler's name lookups: `comp_hash.c`'s through the generated
//! tables ([`crate::captab`]), `name_match.c`'s on name fields, and
//! `parse_entry.c`'s `lookup_fullname`.
//!
//! Upstream finds a capability through a hash table whose chains it walks
//! comparing names -- whole names for terminfo, the first two bytes only for
//! termcap, where every name is two bytes. The generated tables record what
//! that finds for every name there is, so a lookup here is a search of them:
//! a termcap name is cut to its first two bytes first, since nothing past
//! them can change which entry upstream's walk stops at.

use crate::Kind;
use crate::captab::{self, Alias, Cap, UserCap};
use crate::names;

use super::MAX_NAME_SIZE;

/// The bytes of a termcap name that upstream's hash and comparison read:
/// `tcap_hash` and `compare_tcap_names` stop after two.
fn tcap_key(name: &[u8]) -> &[u8] {
    name.get(..name.len().min(2)).unwrap_or(name)
}

/// The name table of a syntax: `_nc_get_table (termcap)`.
#[must_use]
pub fn table(termcap: bool) -> &'static [Cap] {
    if termcap {
        &captab::CAP_TABLE
    } else {
        &captab::INFO_TABLE
    }
}

/// `_nc_find_entry (name, _nc_get_hash_table (termcap))`: the entry upstream's
/// hash finds for `name` -- for a name two capabilities share, the one its
/// chain gives first.
#[must_use]
pub fn find_entry(name: &[u8], termcap: bool) -> Option<&'static Cap> {
    let (find, key): (&[(&str, usize)], &[u8]) = if termcap {
        (&captab::CAP_FIND, tcap_key(name))
    } else {
        (&captab::INFO_FIND, name)
    };
    let at = find.binary_search_by(|(n, _)| n.as_bytes().cmp(key)).ok()?;
    let &(_, index) = find.get(at)?;
    table(termcap).get(index)
}

/// `_nc_find_type_entry (name, type, termcap)`: the entry of that name and
/// that type -- for termcap's `ML`, two strings, the one upstream's hash
/// chain gives first.
#[must_use]
pub fn find_type_entry(name: &[u8], kind: Kind, termcap: bool) -> Option<&'static Cap> {
    let (find, key): (&[(&str, Kind, usize)], &[u8]) = if termcap {
        (&captab::CAP_TYPE_FIND, tcap_key(name))
    } else {
        (&captab::INFO_TYPE_FIND, name)
    };
    let &(_, _, index) = find
        .iter()
        .find(|(n, k, _)| *k == kind && n.as_bytes() == key)?;
    table(termcap).get(index)
}

/// `_nc_find_user_entry (name)`: one of the user-definable capabilities
/// ncurses knows the types and parameters of.
#[must_use]
pub fn find_user_entry(name: &[u8]) -> Option<&'static UserCap> {
    captab::USER_TABLE
        .iter()
        .find(|u| u.name.as_bytes() == name)
}

/// `_nc_get_alias_table (termcap)`.
#[must_use]
pub fn aliases(termcap: bool) -> &'static [Alias] {
    if termcap {
        &captab::CAP_ALIASES
    } else {
        &captab::INFO_ALIASES
    }
}

/// `lookup_fullname (name)`: a capability by its long name (`bell` for
/// `bel`) -- the booleans' first, then the numbers', then the strings'.
#[must_use]
pub fn lookup_fullname(find: &[u8]) -> Option<&'static Cap> {
    let lists: [(Kind, &[&str]); 3] = [
        (Kind::Boolean, &names::BOOLFNAMES),
        (Kind::Number, &names::NUMFNAMES),
        (Kind::String, &names::STRFNAMES),
    ];
    for (kind, list) in lists {
        if let Some(count) = list.iter().position(|n| n.as_bytes() == find) {
            return captab::INFO_TABLE
                .iter()
                .find(|c| c.kind == kind && c.index == count);
        }
    }
    None
}

/// `_nc_first_name (names)`: the name field up to its first `|`, at most
/// `MAX_NAME_SIZE` bytes.
#[must_use]
pub fn first_name(names: &[u8]) -> Vec<u8> {
    names
        .iter()
        .take(MAX_NAME_SIZE)
        .take_while(|&&c| c != 0 && c != b'|')
        .copied()
        .collect()
}

/// `_nc_name_match (namelst, name, delim)`: whether `name` is one of the
/// names in `namelst`, whole, the names separated by any byte of `delim`.
///
/// Upstream's walk, step for step: it compares from where the last name
/// ended, so a name that matches the start of the list's next one does not
/// match.
#[must_use]
pub fn name_match(namelst: &[u8], name: &[u8], delim: &[u8]) -> bool {
    let at = |i: usize| namelst.get(i).copied().unwrap_or(0);
    let mut s = 0usize;
    while at(s) != 0 {
        let mut d = 0usize;
        while let Some(&c) = name.get(d) {
            if c == 0 || at(s) != c {
                break;
            }
            s = s.saturating_add(1);
            d = d.saturating_add(1);
        }
        let name_ended = name.get(d).is_none_or(|&c| c == 0);
        let mut code = true;
        while at(s) != 0 {
            if delim.contains(&at(s)) {
                break;
            }
            code = false;
            s = s.saturating_add(1);
        }
        if code && name_ended {
            return true;
        }
        // `*s++ == 0`.
        let ended = at(s) == 0;
        s = s.saturating_add(1);
        if ended {
            break;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn terminfo_names_are_found_whole() {
        let c = find_entry(b"cols", false).unwrap();
        assert_eq!((c.name, c.kind), ("cols", Kind::Number));
        assert!(find_entry(b"col", false).is_none());
        assert!(find_entry(b"colsx", false).is_none());
    }

    #[test]
    fn termcap_names_are_found_by_their_first_two_bytes() {
        let c = find_entry(b"co", true).unwrap();
        assert_eq!((c.name, c.kind), ("co", Kind::Number));
        assert_eq!(find_entry(b"cox", true), find_entry(b"co", true));
        assert!(find_entry(b"c", true).is_none());
    }

    #[test]
    fn a_shared_termcap_name_is_told_apart_by_type() {
        let n = find_type_entry(b"ma", Kind::Number, true).unwrap();
        let s = find_type_entry(b"ma", Kind::String, true).unwrap();
        assert_eq!((n.kind, s.kind), (Kind::Number, Kind::String));
    }

    #[test]
    fn every_name_and_type_is_found() {
        // Terminfo's names are one capability each.
        let t = table(false);
        for (i, a) in t.iter().enumerate() {
            assert!(
                t.iter().skip(i + 1).all(|b| b.name != a.name),
                "{} twice",
                a.name
            );
        }
        // Every name and type of either table is found as that type.
        for termcap in [false, true] {
            for c in table(termcap) {
                let f = find_type_entry(c.name.as_bytes(), c.kind, termcap).unwrap();
                assert_eq!((f.name, f.kind), (c.name, c.kind));
            }
        }
        // Termcap's two ML strings are one of them, as upstream's chain
        // gives.
        let ml: Vec<usize> = table(true)
            .iter()
            .filter(|c| c.name == "ML")
            .map(|c| c.index)
            .collect();
        assert_eq!(ml.len(), 2);
        let f = find_type_entry(b"ML", Kind::String, true).unwrap();
        assert!(ml.contains(&f.index));
        for (i, a) in captab::USER_TABLE.iter().enumerate() {
            assert!(
                captab::USER_TABLE
                    .iter()
                    .skip(i + 1)
                    .all(|b| b.name != a.name),
                "{} twice in the user table",
                a.name
            );
        }
    }

    #[test]
    fn full_names_find_their_capability() {
        let c = lookup_fullname(b"bell").unwrap();
        assert_eq!((c.name, c.kind), ("bel", Kind::String));
        let c = lookup_fullname(b"columns").unwrap();
        assert_eq!((c.name, c.kind), ("cols", Kind::Number));
        assert!(lookup_fullname(b"nonesuch").is_none());
    }

    #[test]
    fn names_match_only_whole() {
        assert!(name_match(b"xterm|xterm-color|X", b"xterm-color", b"|"));
        assert!(name_match(b"xterm|xterm-color|X", b"X", b"|"));
        assert!(!name_match(b"xterm-color|X", b"xterm", b"|"));
        assert!(!name_match(b"xterm", b"xterm-color", b"|"));
        assert!(!name_match(b"", b"", b"|"));
        // An empty name matches an empty field, as upstream's walk has it.
        assert!(name_match(b"|a", b"", b"|"));
        assert!(!name_match(b"a|", b"", b"|"));
    }

    #[test]
    fn the_first_name_stops_at_a_bar() {
        assert_eq!(first_name(b"vt100|dec vt100"), b"vt100");
        assert_eq!(first_name(b"solo"), b"solo");
    }
}
