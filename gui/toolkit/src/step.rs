//! Moving a selection through a list, by index — and saying which way it
//! behaves at the ends.
//!
//! Four widgets had grown their own copy of the wrapping half: the menu bar
//! stepping across top-level menus and down a dropdown, the text view stepping
//! through search hits, the path bar through completions. Each copy was a few
//! lines of `if idx == 0 { len - 1 } else { idx - 1 }`, and each was written
//! where the bound it relies on had been established by some earlier statement
//! — which is how three of them ended up flagged by
//! `clippy::arithmetic_side_effects` for subtracting from a `usize` that a
//! reader, but not the compiler, could see was non-zero.
//!
//! Nine more widgets had grown the *clamping* half — `if i > 0 { i -= 1 }` and
//! `if i < len - 1 { i += 1 }` — and it took collecting them here to notice
//! that the two halves are not two spellings of one thing. They are two
//! different answers to a question the copies never asked out loud:
//!
//! > What happens when you press Down on the last row?
//!
//! The application launcher stops there. The Wi-Fi network list jumps back to
//! the first network. Both are defensible — a launcher's list is a ranking, so
//! running off the end of it means nothing, whereas a short menu of networks
//! is a ring you thumb through — but neither call site *says* which it meant,
//! so the answer was decided by whoever typed the loop. That is the same fault
//! this module was written to fix, one level up: not arithmetic without a
//! proof, but behaviour without a decision.
//!
//! So the policy is in the name. [`wrapping_after`] and [`clamped_after`] are
//! both here, neither is the default, and a call site has to pick one:
//!
//! ```
//! use guitk::step;
//!
//! // A ring of three: past the end is the beginning again.
//! assert_eq!(step::wrapping_after(3, 2), 0);
//! assert_eq!(step::wrapping_before(3, 0), 2);
//!
//! // A list of three: past the end is still the end.
//! assert_eq!(step::clamped_after(3, 2), 2);
//! assert_eq!(step::clamped_before(3, 0), 0);
//! ```
//!
//! Every function takes the list's length rather than the list, because callers
//! move through things that are not slices — a search-hit count, a completion
//! list held behind a lock — and because an index helper has no business
//! borrowing the collection it indexes.
//!
//! All four are total. The "list is not empty" condition lives inside the
//! expression that depends on it, so there is no bound to prove at the call
//! site and nothing for a fourteenth copy to get subtly different. An empty
//! list yields `0` — the only index that could be meaningful — and callers
//! that must distinguish "no selection" from "the first one" do so with an
//! `Option`, before they get here.

/// The index one place before `from`, wrapping round to the last.
///
/// `from` is assumed to be in range; an empty list yields `0`, the only index
/// that could be meaningful, and callers that care distinguish it beforehand.
#[must_use]
pub fn wrapping_before(len: usize, from: usize) -> usize {
    from.checked_sub(1).unwrap_or_else(|| len.saturating_sub(1))
}

/// The index one place after `from`, wrapping round to the first.
///
/// `from` is assumed to be in range; an empty list yields `0`.
#[must_use]
pub fn wrapping_after(len: usize, from: usize) -> usize {
    match from.checked_add(1) {
        Some(next) if next < len => next,
        _ => 0,
    }
}

/// The index one place before `from`, stopping at the first.
///
/// `from` is brought into range first, so a selection left over from a longer
/// list steps to a valid index rather than to another invalid one — the case
/// that bites when a list is filtered under a live selection, which is what
/// every one of these call sites does on every keystroke.
#[must_use]
pub fn clamped_before(len: usize, from: usize) -> usize {
    from.min(len.saturating_sub(1)).saturating_sub(1)
}

/// The index one place after `from`, stopping at the last.
///
/// An empty list yields `0`, matching the rest of this module; `from` beyond
/// the end comes back to the last index rather than advancing further past it.
#[must_use]
pub fn clamped_after(len: usize, from: usize) -> usize {
    from.saturating_add(1).min(len.saturating_sub(1))
}

/// The indices `0..len`, visited once each in cyclic order from `start`,
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

    use super::{clamped_after, clamped_before, indices, wrapping_after, wrapping_before};

    #[test]
    fn stepping_off_either_end_wraps_round() {
        assert_eq!(wrapping_before(3, 0), 2);
        assert_eq!(wrapping_before(3, 1), 0);
        assert_eq!(wrapping_after(3, 2), 0);
        assert_eq!(wrapping_after(3, 1), 2);
    }

    /// The other answer to the same question, which the copies gave without
    /// ever writing down that they were choosing.
    #[test]
    fn stepping_off_either_end_clamped_stays_where_it_is() {
        assert_eq!(clamped_before(3, 0), 0);
        assert_eq!(clamped_before(3, 1), 0);
        assert_eq!(clamped_after(3, 2), 2);
        assert_eq!(clamped_after(3, 1), 2);
    }

    #[test]
    fn a_single_entry_list_steps_to_itself() {
        for from in [0, 0] {
            assert_eq!(wrapping_before(1, from), 0);
            assert_eq!(wrapping_after(1, from), 0);
            assert_eq!(clamped_before(1, from), 0);
            assert_eq!(clamped_after(1, from), 0);
        }
    }

    /// An empty list has nowhere to step to, and must not underflow saying so.
    /// This is the case the hand-rolled `len - 1` copies got wrong: a list
    /// filtered down to nothing while a selection was live.
    #[test]
    fn an_empty_list_yields_zero_rather_than_underflowing() {
        assert_eq!(wrapping_before(0, 0), 0);
        assert_eq!(wrapping_after(0, 0), 0);
        assert_eq!(clamped_before(0, 0), 0);
        assert_eq!(clamped_after(0, 0), 0);
        assert_eq!(indices(0, 0, true).count(), 0);
        assert_eq!(indices(0, 5, false).count(), 0);
    }

    /// A selection held across a list that got shorter — the launcher's list
    /// re-filtering under a live cursor on every keystroke. The clamped step
    /// brings it back into range rather than stepping from a stale index.
    #[test]
    fn a_stale_index_from_a_longer_list_steps_back_into_range() {
        assert_eq!(clamped_after(3, 9), 2);
        assert_eq!(clamped_before(3, 9), 1);
        assert_eq!(clamped_before(3, 3), 1);
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
            assert_eq!(pair[1], wrapping_after(len, pair[0]));
        }
        for pair in backwards.windows(2) {
            assert_eq!(pair[1], wrapping_before(len, pair[0]));
        }
    }

    /// A start beyond the end is reduced into range rather than skipping the
    /// walk or panicking.
    #[test]
    fn an_out_of_range_start_is_brought_back_into_range() {
        let visited: Vec<usize> = indices(3, 7, true).collect();
        assert_eq!(visited, vec![1, 2, 0]);
    }

    /// In the middle of a list the two policies are the same function; they
    /// differ only at the two ends. Stated as a test so that a change to one
    /// that accidentally changes the other is caught here.
    #[test]
    fn the_two_policies_differ_only_at_the_ends() {
        let len = 6;
        for from in 1..len - 1 {
            assert_eq!(wrapping_before(len, from), clamped_before(len, from));
            assert_eq!(wrapping_after(len, from), clamped_after(len, from));
        }
        assert_ne!(wrapping_before(len, 0), clamped_before(len, 0));
        assert_ne!(wrapping_after(len, len - 1), clamped_after(len, len - 1));
    }
}
