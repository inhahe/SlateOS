//! Wrapping movement through a list, by index.
//!
//! Four widgets had grown their own copy of this: the menu bar stepping across
//! top-level menus and down a dropdown, the text view stepping through search
//! hits, the path bar through completions. Each copy was a few lines of
//! `if idx == 0 { len - 1 } else { idx - 1 }`, and each was written where the
//! bound it relies on had been established by some earlier statement — which is
//! how three of them ended up flagged by `clippy::arithmetic_side_effects` for
//! subtracting from a `usize` that a reader, but not the compiler, could see was
//! non-zero.
//!
//! Written here once, the wrap is total: the "list is not empty" condition lives
//! inside the expression that depends on it, so there is no bound to prove at the
//! call site and nothing for a fifth copy to get subtly different.
//!
//! Every function takes the list's length rather than the list, because callers
//! move through things that are not slices — a search-hit count, a completion
//! list held behind a lock — and because an index helper has no business
//! borrowing the collection it indexes.

/// The index one place before `from`, wrapping round to the last.
///
/// `from` is assumed to be in range; an empty list yields `0`, the only index
/// that could be meaningful, and callers that care distinguish it beforehand.
#[must_use]
pub fn before(len: usize, from: usize) -> usize {
    from.checked_sub(1).unwrap_or_else(|| len.saturating_sub(1))
}

/// The index one place after `from`, wrapping round to the first.
///
/// `from` is assumed to be in range; an empty list yields `0`.
#[must_use]
pub fn after(len: usize, from: usize) -> usize {
    match from.checked_add(1) {
        Some(next) if next < len => next,
        _ => 0,
    }
}

/// The byte offsets `0..len`, visited once each in cyclic order from `start`,
/// going forwards or backwards.
///
/// For searching a list from a position — the next selectable menu row, the next
/// matching completion — where the search must consider every entry and then
/// stop. The wrap is modular arithmetic on `usize`, so an index cannot leave the
/// range in the first place; a signed cursor stepped off the end and pushed back
/// into range by a later `if` is out of range in between, and the proof that it
/// is back in range by the time it is used sits in a different statement from
/// the use.
///
/// `step` is always below `len`, so the `saturating_sub` is exact; `checked_rem`
/// carries the "`len` is not zero" condition into the expression that depends on
/// it rather than leaving it behind in a guard.
pub fn indices(len: usize, start: usize, forward: bool) -> impl Iterator<Item = usize> {
    let start = start.checked_rem(len).unwrap_or(0);
    (0..len).filter_map(move |step| {
        let delta = if forward {
            step
        } else {
            len.saturating_sub(step)
        };
        start.saturating_add(delta).checked_rem(len)
    })
}

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::{after, before, indices};

    #[test]
    fn stepping_off_either_end_wraps_round() {
        assert_eq!(before(3, 0), 2);
        assert_eq!(before(3, 1), 0);
        assert_eq!(after(3, 2), 0);
        assert_eq!(after(3, 1), 2);
    }

    #[test]
    fn a_single_entry_list_steps_to_itself() {
        assert_eq!(before(1, 0), 0);
        assert_eq!(after(1, 0), 0);
    }

    /// An empty list has nowhere to step to, and must not underflow saying so.
    #[test]
    fn an_empty_list_yields_zero_rather_than_underflowing() {
        assert_eq!(before(0, 0), 0);
        assert_eq!(after(0, 0), 0);
        assert_eq!(indices(0, 0, true).count(), 0);
        assert_eq!(indices(0, 5, false).count(), 0);
    }

    /// The walk must reach every index exactly once and then stop, whichever
    /// index it starts from and whichever way it goes.
    #[test]
    fn the_walk_covers_every_index_exactly_once() {
        for len in 1..8_usize {
            for start in 0..len {
                for forward in [true, false] {
                    let visited: Vec<usize> = indices(len, start, forward).collect();
                    let mut distinct = visited.clone();
                    distinct.sort_unstable();
                    distinct.dedup();
                    assert_eq!(visited.len(), len, "len={len} start={start}");
                    assert_eq!(visited[0], start, "the walk begins where it was told");
                    assert_eq!(distinct.len(), len, "len={len} start={start} fwd={forward}");
                }
            }
        }
    }

    /// Consecutive steps of the walk agree with the single-step helpers.
    #[test]
    fn the_walk_steps_the_same_way_the_single_step_helpers_do() {
        let len = 5;
        let forwards: Vec<usize> = indices(len, 2, true).collect();
        let backwards: Vec<usize> = indices(len, 2, false).collect();
        for pair in forwards.windows(2) {
            assert_eq!(pair[1], after(len, pair[0]));
        }
        for pair in backwards.windows(2) {
            assert_eq!(pair[1], before(len, pair[0]));
        }
    }

    /// A start beyond the end is reduced into range rather than skipping the
    /// walk or panicking.
    #[test]
    fn an_out_of_range_start_is_brought_back_into_range() {
        let visited: Vec<usize> = indices(3, 7, true).collect();
        assert_eq!(visited, vec![1, 2, 0]);
    }
}
