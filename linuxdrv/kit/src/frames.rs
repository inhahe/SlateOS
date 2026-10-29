//! A served network card's ring (design-decisions 1589), the driver's side,
//! as far as it is not system calls: what the network stack submits,
//! checked and sorted -- frames to send, slots to receive into -- and kept
//! in the order it came until the card sends or receives. `nicport` (on the
//! driver process's own target) drives it.
//!
//! The stack is another process, so nothing it submits is trusted: a
//! window outside the data area, a frame too short to have a header or too
//! long for the card, or an operation a card has no use for, is refused --
//! completed at once with an error -- and never reaches the C half.

use netipc::ring::{OP_RECV, OP_SEND, Sqe};

/// An Ethernet header: the least a frame can be.
pub const ETH_HLEN: u32 = 14;
/// What a VLAN tag adds to a frame's header.
pub const VLAN_HLEN: u32 = 4;

/// The errnos refusals and failures complete with, negated (Linux's numbers,
/// as the ring's completions carry them).
pub const EINVAL: i32 = 22;
pub const EMSGSIZE: i32 = 90;
pub const EOPNOTSUPP: i32 = 95;
pub const ENOBUFS: i32 = 105;

/// A submission taken: its token, and its window in the data area -- the
/// frame to send, or the slot to receive into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub user_data: u64,
    pub off: u32,
    pub len: u32,
}

/// A submission, sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorted {
    /// A frame to send.
    Send(Window),
    /// A slot to receive a frame into.
    Recv(Window),
    /// Refused: to be completed at once with its token and this negative
    /// errno.
    Refused(u64, i32),
}

/// The longest frame a card whose frames carry `mtu` bytes after the header
/// sends: header, VLAN tag and payload.
#[must_use]
pub fn max_frame(mtu: u32) -> u32 {
    mtu.saturating_add(ETH_HLEN).saturating_add(VLAN_HLEN)
}

/// `sqe` sorted, against a data area of `area` bytes and a card carrying
/// `mtu` bytes a frame. A receive slot may be any length that holds a
/// header: a frame longer than the slot it would go to is dropped, the slot
/// kept.
#[must_use]
pub fn sort(sqe: &Sqe, area: usize, mtu: u32) -> Sorted {
    let w = Window {
        user_data: sqe.user_data,
        off: sqe.data_off,
        len: sqe.data_len,
    };
    let inside = usize::try_from(w.off)
        .ok()
        .zip(usize::try_from(w.len).ok())
        .and_then(|(off, len)| off.checked_add(len))
        .is_some_and(|end| end <= area);
    let refuse = |errno: i32| Sorted::Refused(w.user_data, errno.saturating_neg());
    match sqe.op {
        OP_SEND | OP_RECV if !inside || w.len < ETH_HLEN => refuse(EINVAL),
        OP_SEND if w.len > max_frame(mtu) => refuse(EMSGSIZE),
        OP_SEND => Sorted::Send(w),
        OP_RECV => Sorted::Recv(w),
        _ => refuse(EOPNOTSUPP),
    }
}

/// Windows in the order they came, `N` at most.
pub struct Queue<const N: usize> {
    items: [Window; N],
    head: usize,
    len: usize,
}

impl<const N: usize> Default for Queue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Queue<N> {
    #[must_use]
    pub const fn new() -> Self {
        Queue {
            items: [Window {
                user_data: 0,
                off: 0,
                len: 0,
            }; N],
            head: 0,
            len: 0,
        }
    }

    /// `w` added at the back: `false`, nothing changed, when the queue is
    /// full.
    pub fn push(&mut self, w: Window) -> bool {
        if self.len >= N {
            return false;
        }
        // head < N and len < N, so the sum does not overflow.
        let at = self.head.wrapping_add(self.len).checked_rem(N).unwrap_or(0);
        let Some(slot) = self.items.get_mut(at) else {
            return false;
        };
        *slot = w;
        self.len = self.len.wrapping_add(1);
        true
    }

    /// The window at the front.
    #[must_use]
    pub fn front(&self) -> Option<Window> {
        if self.len == 0 {
            return None;
        }
        self.items.get(self.head).copied()
    }

    /// The window at the front, taken.
    pub fn pop(&mut self) -> Option<Window> {
        let w = self.front()?;
        self.head = self.head.wrapping_add(1).checked_rem(N).unwrap_or(0);
        self.len = self.len.wrapping_sub(1);
        Some(w)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqe(op: u8, off: u32, len: u32) -> Sqe {
        Sqe {
            op,
            conn_id: 0,
            data_off: off,
            data_len: len,
            user_data: 77,
            aux: 0,
        }
    }

    #[test]
    fn a_frame_and_a_slot_inside_the_area_are_taken() {
        let w = Window {
            user_data: 77,
            off: 2048,
            len: 60,
        };
        assert_eq!(sort(&sqe(OP_SEND, 2048, 60), 4096, 1500), Sorted::Send(w));
        assert_eq!(sort(&sqe(OP_RECV, 2048, 60), 4096, 1500), Sorted::Recv(w));
        let last = sqe(OP_SEND, 4096 - 1518, 1518);
        assert!(
            matches!(sort(&last, 4096, 1500), Sorted::Send(_)),
            "ends at the area's end"
        );
    }

    #[test]
    fn what_a_card_cannot_take_is_refused() {
        let cases = [
            (sqe(OP_SEND, 4000, 200), -EINVAL, "past the area"),
            (sqe(OP_RECV, 4000, 200), -EINVAL, "a slot past the area"),
            (sqe(OP_SEND, u32::MAX, 20), -EINVAL, "an end that overflows"),
            (
                sqe(OP_SEND, 0, ETH_HLEN - 1),
                -EINVAL,
                "no room for a header",
            ),
            (
                sqe(OP_RECV, 0, ETH_HLEN - 1),
                -EINVAL,
                "a slot with no room for one",
            ),
            (
                sqe(OP_SEND, 0, max_frame(1500) + 1),
                -EMSGSIZE,
                "longer than the card sends",
            ),
            (sqe(0x01, 0, 64), -EOPNOTSUPP, "not a card's operation"),
        ];
        for (s, errno, what) in cases {
            assert_eq!(sort(&s, 4096, 1500), Sorted::Refused(77, errno), "{what}");
        }
        assert!(
            matches!(
                sort(&sqe(OP_SEND, 0, max_frame(1500)), 4096, 1500),
                Sorted::Send(_)
            ),
            "the longest frame, VLAN tag and all"
        );
    }

    #[test]
    fn a_queue_keeps_its_order_and_its_bound_and_wraps() {
        let w = |n: u64| Window {
            user_data: n,
            off: 0,
            len: 14,
        };
        let mut q = Queue::<3>::new();
        assert_eq!(q.pop(), None);
        for round in 0..4u64 {
            for i in 0..3 {
                assert!(q.push(w(round * 10 + i)));
            }
            assert!(!q.push(w(99)), "full");
            assert_eq!(q.len(), 3);
            assert_eq!(q.front(), Some(w(round * 10)));
            for i in 0..3 {
                assert_eq!(q.pop(), Some(w(round * 10 + i)));
            }
            assert!(q.is_empty());
            // Leave the head somewhere else for the next round.
            assert!(q.push(w(5)));
            assert_eq!(q.pop(), Some(w(5)));
        }
    }
}
