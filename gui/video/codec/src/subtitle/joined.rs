//! Cues made whole again from the pieces a track's samples cut them into.
//!
//! MP4 carries WebVTT and TTML (ISO/IEC 14496-30) as samples, each a stretch
//! of time and what shows through it, so a cue showing through several
//! samples is cut into a piece in each -- and TTML's paragraphs are cut again
//! inside a sample, wherever what they show changes. [`Joined`] puts the
//! pieces back together: a piece that begins just as a cue showing the same
//! reaches its end goes on with it; any other begins a cue; a cue no piece
//! goes on from has ended. Cues are given in the order they began, those
//! beginning together in the order their first pieces came.
//!
//! **What it costs.** Each piece is matched to the cue it goes on with by a
//! hash of what it shows, among only the cues reaching its time, and every
//! cue waits for its turn in a heap: a logarithm of the cues open a piece,
//! however many a sample lists or however long a cue stays open while others
//! end. (Matching a piece against every cue showing, and sorting every cue
//! waiting after every sample, made a track of many cues -- many in one
//! sample, or one held open across thousands of samples -- quadratic.)

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::hash::Hash;

/// A cue made whole: from when its first piece began to when its last
/// ended, on the clock its pieces were given on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Whole<T, C> {
    pub start: T,
    pub end: T,
    pub cue: C,
}

/// An ended cue waiting for those begun before it, by its place: when it
/// began, then its place among those beginning together.
#[derive(Debug)]
struct Waiting<T, C> {
    start: T,
    order: u64,
    end: T,
    cue: C,
}

impl<T: Ord, C> PartialEq for Waiting<T, C> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<T: Ord, C> Eq for Waiting<T, C> {}

impl<T: Ord, C> PartialOrd for Waiting<T, C> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord, C> Ord for Waiting<T, C> {
    /// Every cue's `order` is its own, so no two are equal.
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.start, self.order).cmp(&(&other.start, other.order))
    }
}

/// A track's cues made whole again from their pieces.
#[derive(Debug)]
pub(crate) struct Joined<T, C> {
    /// The cues that may still go on, by the time they reach, then by what
    /// they show: each its place in the order cues began, and when it began.
    open: BTreeMap<T, HashMap<C, BTreeSet<(u64, T)>>>,
    /// The places, `(start, order)`, of the cues that may still go on: the
    /// first holds back every ended cue placed after it.
    places: BTreeSet<(T, u64)>,
    /// Ended cues waiting for those begun before them.
    ended: BinaryHeap<Reverse<Waiting<T, C>>>,
    /// The place the next cue to begin takes.
    next: u64,
}

impl<T, C> Default for Joined<T, C> {
    fn default() -> Self {
        Self {
            open: BTreeMap::new(),
            places: BTreeSet::new(),
            ended: BinaryHeap::new(),
            next: 0,
        }
    }
}

impl<T: Ord + Copy, C: Eq + Hash + Clone> Joined<T, C> {
    /// A sample ending at `end`, and its `pieces`, each `(from, to, cue)`
    /// inside it, in the order they begin -- those beginning together in the
    /// order the sample gives them, which is the order the cues they begin
    /// take. A piece beginning just as a cue showing the same reaches its
    /// end goes on with it -- each cue at most once, the first begun first
    /// -- and any other begins a cue; a cue reaching a time no piece goes on
    /// from has ended there, and one reaching `end` may yet go on into the
    /// next sample. Answers the cues ended that can now be given, in the
    /// order they began.
    pub(crate) fn sample(&mut self, end: T, pieces: Vec<(T, T, C)>) -> Vec<Whole<T, C>> {
        let mut pieces = pieces.into_iter().peekable();
        while let Some(&(from, _, _)) = pieces.peek() {
            // A cue reaching a time before this one can no longer go on: a
            // piece going on from it would have begun already.
            self.end_before(from);
            let mut reaching = self.open.remove(&from).unwrap_or_default();
            while let Some((_, to, cue)) = pieces.next_if(|piece| piece.0 == from) {
                let (order, start) = match reaching.get_mut(&cue).and_then(BTreeSet::pop_first) {
                    Some(going_on) => going_on,
                    None => {
                        let order = self.next;
                        self.next = self.next.saturating_add(1);
                        self.places.insert((from, order));
                        (order, from)
                    }
                };
                self.open
                    .entry(to)
                    .or_default()
                    .entry(cue)
                    .or_default()
                    .insert((order, start));
            }
            // What reached this time and did not go on ended at it.
            for (cue, cues) in reaching {
                self.ended_at(from, cue, cues);
            }
        }
        self.end_before(end);
        self.release()
    }

    /// The track's end: every cue still open has ended where it reached.
    /// Answers every cue not yet given, in the order they began.
    pub(crate) fn finish(&mut self) -> Vec<Whole<T, C>> {
        while let Some((end, reached)) = self.open.pop_first() {
            for (cue, cues) in reached {
                self.ended_at(end, cue, cues);
            }
        }
        self.release()
    }

    /// Everything forgotten: after a seek, the samples read next are from
    /// elsewhere in the track.
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Every cue reaching a time before `time` ended where it reached.
    fn end_before(&mut self, time: T) {
        while let Some(entry) = self.open.first_entry() {
            if *entry.key() >= time {
                break;
            }
            let (end, reached) = entry.remove_entry();
            for (cue, cues) in reached {
                self.ended_at(end, cue, cues);
            }
        }
    }

    /// `cues`, each showing `cue`, ended at `end`: waiting for their turn.
    fn ended_at(&mut self, end: T, cue: C, cues: BTreeSet<(u64, T)>) {
        for (order, start) in cues {
            self.places.remove(&(start, order));
            self.ended.push(Reverse(Waiting {
                start,
                order,
                end,
                cue: cue.clone(),
            }));
        }
    }

    /// The ended cues placed before every cue still open, in order.
    fn release(&mut self) -> Vec<Whole<T, C>> {
        let first_open = self.places.first().copied();
        let mut out = Vec::new();
        while let Some(Reverse(next)) = self.ended.peek() {
            if first_open.is_some_and(|first| (next.start, next.order) >= first) {
                break;
            }
            let Some(Reverse(w)) = self.ended.pop() else {
                break;
            };
            out.push(Whole {
                start: w.start,
                end: w.end,
                cue: w.cue,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "a test: a failure should be loud"
    )]

    use super::*;

    fn w(start: i64, end: i64, cue: &'static str) -> Whole<i64, &'static str> {
        Whole { start, end, cue }
    }

    /// Pieces each the whole sample, as WebVTT's samples give them.
    fn whole_sample(
        joined: &mut Joined<i64, &'static str>,
        start: i64,
        end: i64,
        cues: &[&'static str],
    ) -> Vec<Whole<i64, &'static str>> {
        joined.sample(end, cues.iter().map(|&c| (start, end, c)).collect())
    }

    #[test]
    fn a_cue_in_samples_one_after_another_is_one_cue() {
        let mut j = Joined::default();
        assert!(whole_sample(&mut j, 0, 10, &["a"]).is_empty());
        assert!(whole_sample(&mut j, 10, 20, &["a"]).is_empty());
        assert_eq!(whole_sample(&mut j, 20, 30, &[]), [w(0, 20, "a")]);
        assert!(j.finish().is_empty());
    }

    #[test]
    fn a_cue_after_a_gap_is_another_cue() {
        let mut j = Joined::default();
        whole_sample(&mut j, 0, 10, &["a"]);
        // The next sample begins later: nothing reaches its start.
        assert_eq!(whole_sample(&mut j, 15, 20, &["a"]), [w(0, 10, "a")]);
        assert_eq!(j.finish(), [w(15, 20, "a")]);
    }

    #[test]
    fn an_ended_cue_waits_for_one_begun_before_it() {
        let mut j = Joined::default();
        whole_sample(&mut j, 0, 10, &["long"]);
        whole_sample(&mut j, 10, 20, &["long", "short"]);
        // `short` has ended, but `long` began first and still shows.
        assert!(whole_sample(&mut j, 20, 30, &["long"]).is_empty());
        assert_eq!(
            whole_sample(&mut j, 30, 40, &[]),
            [w(0, 30, "long"), w(10, 20, "short")]
        );
    }

    #[test]
    fn two_cues_alike_are_two_cues_each_going_on_once() {
        let mut j = Joined::default();
        whole_sample(&mut j, 0, 10, &["x", "x"]);
        whole_sample(&mut j, 10, 20, &["x", "x"]);
        // One of them ends: the first begun goes on.
        whole_sample(&mut j, 20, 30, &["x"]);
        assert_eq!(j.finish(), [w(0, 30, "x"), w(0, 20, "x")]);
    }

    #[test]
    fn cues_beginning_together_keep_the_order_they_came_in() {
        let mut j = Joined::default();
        whole_sample(&mut j, 0, 10, &["b", "a", "c"]);
        assert_eq!(j.finish(), [w(0, 10, "b"), w(0, 10, "a"), w(0, 10, "c")]);
    }

    #[test]
    fn pieces_inside_a_sample_join_where_they_meet() {
        let mut j = Joined::default();
        // Within one sample: `a` from 0 to 4, again from 4 to 6 (another
        // paragraph saying the same), `b` from 2 to 10.
        let out = j.sample(10, vec![(0, 4, "a"), (2, 10, "b"), (4, 6, "a")]);
        assert_eq!(out, [w(0, 6, "a")]);
        // `b` reaches the sample's end, and goes on into the next -- to end
        // inside it, and be given with it.
        assert_eq!(j.sample(20, vec![(10, 12, "b")]), [w(2, 12, "b")]);
        assert!(j.finish().is_empty());
    }

    #[test]
    fn a_seek_forgets_everything() {
        let mut j = Joined::default();
        whole_sample(&mut j, 0, 10, &["a"]);
        j.clear();
        assert!(whole_sample(&mut j, 10, 20, &["a"]).is_empty());
        assert_eq!(j.finish(), [w(10, 20, "a")]);
    }

    /// The joining as it was first written: every cue showing matched
    /// against every cue a sample lists, the waiting sorted each sample --
    /// the reference the quicker one is held to.
    #[derive(Default)]
    struct Reference {
        showing: Vec<(u64, Whole<i64, u8>)>,
        ended: Vec<(u64, Whole<i64, u8>)>,
        next: u64,
    }

    impl Reference {
        fn sample(&mut self, start: i64, end: i64, cues: &[u8]) -> Vec<Whole<i64, u8>> {
            let mut going_on = vec![false; self.showing.len()];
            let mut begun = Vec::new();
            for &cue in cues {
                let found = self
                    .showing
                    .iter()
                    .zip(&going_on)
                    .position(|((_, s), &taken)| !taken && s.end == start && s.cue == cue);
                match found {
                    Some(i) => going_on[i] = true,
                    None => begun.push(cue),
                }
            }
            let mut still = Vec::new();
            for ((order, whole), on) in self.showing.drain(..).zip(going_on) {
                if on {
                    still.push((order, Whole { end, ..whole }));
                } else {
                    self.ended.push((order, whole));
                }
            }
            for cue in begun {
                still.push((self.next, Whole { start, end, cue }));
                self.next += 1;
            }
            self.showing = still;
            self.release()
        }

        fn finish(&mut self) -> Vec<Whole<i64, u8>> {
            self.ended.append(&mut self.showing);
            self.release()
        }

        fn release(&mut self) -> Vec<Whole<i64, u8>> {
            let place = |(order, w): &(u64, Whole<i64, u8>)| (w.start, *order);
            self.ended.sort_by_key(place);
            let first = self.showing.iter().map(place).min();
            let ready = self
                .ended
                .iter()
                .take_while(|e| first.is_none_or(|f| place(e) < f))
                .count();
            self.ended.drain(..ready).map(|(_, w)| w).collect()
        }
    }

    /// A small generator of numbers, the same every run.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    #[test]
    fn whole_samples_join_as_the_first_joining_did() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..2000 {
            let (mut quick, mut reference) = (Joined::default(), Reference::default());
            let (mut got, mut want) = (Vec::new(), Vec::new());
            let mut at = 0;
            for _ in 0..rng.below(12) {
                // Now and then a gap between samples.
                let start = at + i64::try_from(rng.below(4) / 3).unwrap();
                let end = start + 1 + i64::try_from(rng.below(3)).unwrap();
                let cues: Vec<u8> = (0..rng.below(5))
                    .map(|_| u8::try_from(rng.below(3)).unwrap())
                    .collect();
                got.extend(quick.sample(end, cues.iter().map(|&c| (start, end, c)).collect()));
                want.extend(reference.sample(start, end, &cues));
                at = end;
            }
            got.extend(quick.finish());
            want.extend(reference.finish());
            assert_eq!(got, want);
        }
    }

    #[test]
    fn many_cues_and_a_long_one_cost_no_more_than_their_pieces() {
        // A cue held open across a hundred thousand samples while each of
        // them ends another, and a sample of a hundred thousand cues: each
        // quadratic for the first joining.
        let mut j: Joined<i64, u32> = Joined::default();
        let mut given = 0usize;
        for i in 0..100_000i64 {
            let short = u32::try_from(i).unwrap() + 1;
            given += j
                .sample(i + 1, vec![(i, i + 1, 0), (i, i + 1, short)])
                .len();
        }
        let many: Vec<(i64, i64, u32)> = (0..100_000u32).map(|c| (100_000, 100_001, c)).collect();
        given += j.sample(100_001, many).len();
        given += j.finish().len();
        assert_eq!(given, 1 + 100_000 + 100_000 - 1);
    }
}
