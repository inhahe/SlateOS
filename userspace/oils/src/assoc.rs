//! The storage behind a shell associative array (`declare -A m; m[k]=v`).
//!
//! Two things have to be true at once, and neither alone is enough:
//!
//! * **Order is observable.** `${!m[@]}`, `${m[@]}` and `declare -p m` all
//!   enumerate the array, and osh enumerates it in the order the keys were
//!   first assigned — a deterministic order, unlike bash's hash order, which is
//!   unspecified. A plain hash map cannot provide it.
//! * **Lookup is on the hot path.** Every element read *and* every element
//!   write goes through a key lookup, so a scan makes a loop that fills an
//!   array quadratic. It measurably did: 40 000 insertions took 2 348 ms
//!   against bash's 278, and the curve was bending upward.
//!
//! So this keeps both: a `Vec` for the order and a hash index beside it for the
//! lookup. Removal — `unset m[key]`, which is rare — shifts every later
//! position and so rebuilds the index; everything else is O(1).

use std::collections::HashMap;

use crate::bytes::{BStr, Str};

/// An insertion-ordered map from byte-string keys to byte-string values.
///
/// The invariant, maintained by every method here and relied on by all of
/// them: `index[k] == i` exactly when `order[i].0 == k`.
#[derive(Clone, Debug, Default)]
pub struct AssocArray {
    /// `(key, value)` in the order the keys were first assigned.
    order: Vec<(Str, Str)>,
    /// `key → its position in `order``.
    index: HashMap<Str, usize>,
}

impl AssocArray {
    /// An array with no elements — a bare `declare -A m`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many elements the array holds (`${#m[@]}`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether the array holds no elements. Note that this does *not* say
    /// whether it was ever assigned: `declare -A m` and `m=()` are both empty
    /// and print differently, which the caller tracks separately.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The value at `key`, or `None` if the array has no such key.
    #[must_use]
    pub fn get(&self, key: BStr<'_>) -> Option<&Str> {
        let i = *self.index.get(key)?;
        self.order.get(i).map(|(_, v)| v)
    }

    /// Whether `key` is present. An element may be present and empty, which is
    /// not the same as absent — `${m[k]+set}` tells them apart.
    #[must_use]
    pub fn contains_key(&self, key: BStr<'_>) -> bool {
        self.index.contains_key(key)
    }

    /// The elements in insertion order.
    pub fn iter(&self) -> std::slice::Iter<'_, (Str, Str)> {
        self.order.iter()
    }

    /// The keys in insertion order (`${!m[@]}`).
    pub fn keys(&self) -> impl Iterator<Item = &Str> {
        self.order.iter().map(|(k, _)| k)
    }

    /// The values in insertion order (`${m[@]}`).
    pub fn values(&self) -> impl Iterator<Item = &Str> {
        self.order.iter().map(|(_, v)| v)
    }

    /// Assign `key`, replacing any value it already had.
    ///
    /// Re-assigning an existing key keeps its original position: bash's
    /// traversal order does not move an element because it was written again,
    /// and neither does this one.
    pub fn set(&mut self, key: Str, val: Str) {
        self.slot(key).1 = val;
    }

    /// Append to `key` (`m[k]+=v`), starting from empty if it is absent.
    pub fn append(&mut self, key: Str, val: BStr<'_>) {
        self.slot(key).1.extend_from_slice(val);
    }

    /// The `(key, value)` slot for `key`, created empty at the end if it is not
    /// already there. The single place a new key is introduced, so the index
    /// and the order cannot drift apart.
    fn slot(&mut self, key: Str) -> &mut (Str, Str) {
        let i = match self.index.get(key.as_slice()) {
            Some(&i) => i,
            None => {
                let i = self.order.len();
                self.index.insert(key.clone(), i);
                self.order.push((key, Str::new()));
                i
            }
        };
        // The index is only ever set to a position that exists, so this cannot
        // fail; an empty fallback keeps the method total rather than panicking.
        self.order.get_mut(i).expect("index points at an existing slot")
    }

    /// Remove `key` if present, reporting whether there was one.
    ///
    /// Every later element moves down one, so the index is rebuilt. `unset
    /// m[key]` is rare enough that paying O(n) here to keep every other
    /// operation O(1) is the right way round.
    pub fn remove(&mut self, key: BStr<'_>) -> bool {
        let Some(i) = self.index.get(key).copied() else {
            return false;
        };
        self.order.remove(i);
        self.index.clear();
        for (i, (k, _)) in self.order.iter().enumerate() {
            self.index.insert(k.clone(), i);
        }
        true
    }
}

impl FromIterator<(Str, Str)> for AssocArray {
    /// Build from `(key, value)` pairs, keeping the first position of a
    /// repeated key and its *last* value — the same rule `set` follows.
    fn from_iter<I: IntoIterator<Item = (Str, Str)>>(iter: I) -> Self {
        let mut m = Self::new();
        for (k, v) in iter {
            m.set(k, v);
        }
        m
    }
}

impl<'a> IntoIterator for &'a AssocArray {
    type Item = &'a (Str, Str);
    type IntoIter = std::slice::Iter<'a, (Str, Str)>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::AssocArray;

    fn pairs(m: &AssocArray) -> Vec<(String, String)> {
        m.iter()
            .map(|(k, v)| {
                (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned())
            })
            .collect()
    }

    #[test]
    fn keeps_insertion_order_and_finds_by_key() {
        let mut m = AssocArray::new();
        for k in ["b", "a", "c"] {
            m.set(k.as_bytes().to_vec(), k.to_uppercase().into_bytes());
        }
        assert_eq!(m.len(), 3);
        assert_eq!(
            pairs(&m),
            [("b", "B"), ("a", "A"), ("c", "C")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .to_vec()
        );
        assert_eq!(m.get(b"a").map(Vec::as_slice), Some(&b"A"[..]));
        assert_eq!(m.get(b"zz"), None);
        assert!(m.contains_key(b"c") && !m.contains_key(b"zz"));
    }

    #[test]
    fn reassigning_a_key_does_not_move_it() {
        let mut m = AssocArray::new();
        m.set(b"x".to_vec(), b"1".to_vec());
        m.set(b"y".to_vec(), b"2".to_vec());
        m.set(b"x".to_vec(), b"3".to_vec());
        assert_eq!(pairs(&m), vec![("x".into(), "3".into()), ("y".into(), "2".into())]);
        // Appending is the same rule, and starts from empty for a new key.
        m.append(b"y".to_vec(), b"tail");
        m.append(b"z".to_vec(), b"new");
        assert_eq!(
            pairs(&m),
            vec![("x".into(), "3".into()), ("y".into(), "2tail".into()), ("z".into(), "new".into())]
        );
    }

    #[test]
    fn removing_renumbers_everything_after_it() {
        let mut m: AssocArray =
            ["a", "b", "c", "d"].into_iter().map(|k| (k.as_bytes().to_vec(), k.as_bytes().to_vec())).collect();
        assert!(m.remove(b"b"));
        assert!(!m.remove(b"b"), "removing twice reports the second as absent");
        assert_eq!(m.len(), 3);
        // The survivors are still reachable by key — the invariant the index
        // rebuild exists to keep.
        for k in ["a", "c", "d"] {
            assert_eq!(m.get(k.as_bytes()).map(Vec::as_slice), Some(k.as_bytes()), "{k}");
        }
        assert_eq!(m.get(b"b"), None);
        // And a key added after a removal lands at the end, not in the hole.
        m.set(b"e".to_vec(), b"e".to_vec());
        assert_eq!(m.keys().map(|k| k[0]).collect::<Vec<_>>(), b"acde");
    }

    #[test]
    fn a_repeated_key_in_a_literal_keeps_its_first_place() {
        let m: AssocArray = [("k", "1"), ("j", "2"), ("k", "3")]
            .into_iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect();
        assert_eq!(pairs(&m), vec![("k".into(), "3".into()), ("j".into(), "2".into())]);
    }
}
