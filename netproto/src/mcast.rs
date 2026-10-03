//! The host's side of multicast membership: which groups each socket is in,
//! which groups the host is in on their behalf, and which of those changes
//! must be announced to the network ([`crate::igmp`], [`crate::mld`]).
//!
//! A host belongs to a group while at least one of its sockets does. The
//! network hears about the group when the first socket joins (a report) and
//! when the last one leaves (a leave or done), and is told again whenever a
//! router asks ([`HostGroups::answer`]). Sockets in between are counted, not
//! announced.
//!
//! Fixed capacities, no allocation: [`SocketGroups`] holds a socket's groups
//! and its multicast send settings, [`HostGroups`] the host's groups with a
//! socket count each.

use crate::Ipv4Addr;
use crate::igmp;
use crate::ipv6::Ipv6Addr;
use crate::mld;

/// A multicast group of either family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// An IPv4 group (224.0.0.0/4).
    V4(Ipv4Addr),
    /// An IPv6 group (ff00::/8).
    V6(Ipv6Addr),
}

impl Group {
    /// Whether the address is a multicast address of its family.
    #[must_use]
    pub fn is_multicast(&self) -> bool {
        match self {
            Self::V4(a) => igmp::is_multicast(a),
            Self::V6(a) => mld::is_multicast(a),
        }
    }

    /// Whether membership of this group is announced at all: never the
    /// all-hosts group 224.0.0.1 (RFC 2236 §6) or ff02::1 (RFC 2710 §5), nor
    /// an IPv6 group of interface-local or reserved scope.
    #[must_use]
    pub fn reportable(&self) -> bool {
        match self {
            Self::V4(a) => igmp::is_multicast(a) && *a != igmp::ALL_HOSTS,
            Self::V6(a) => mld::reportable(a),
        }
    }
}

/// What a membership change requires the caller to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Announce {
    /// Nothing: another socket already holds the group, or still does, or
    /// the group is never announced.
    Nothing,
    /// A report (IGMPv2 Membership Report / MLD Listener Report) for the
    /// group: the host has just joined it.
    Report(Group),
    /// A leave (IGMP Leave Group / MLD Listener Done) for the group: the host
    /// has just left it.
    Leave(Group),
}

/// Why a join or leave was refused -- the errors Linux gives the same calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Not a multicast address (`EINVAL`).
    NotMulticast,
    /// Joining a group the socket is already in (`EADDRINUSE`).
    AlreadyIn,
    /// Leaving a group the socket is not in (`EADDRNOTAVAIL`).
    NotIn,
    /// No room for another group (`ENOBUFS`).
    Full,
}

/// One socket's multicast state: the groups it has joined, at most `N`, and
/// the settings its multicast sends use, with Linux's defaults.
#[derive(Debug, Clone, Copy)]
pub struct SocketGroups<const N: usize> {
    groups: [Option<Group>; N],
    /// IPv4 multicast TTL (`IP_MULTICAST_TTL`): 1 unless set; 0 keeps a
    /// datagram on this machine.
    pub ttl: u8,
    /// Whether IPv4 multicast sends loop back to this machine's members
    /// (`IP_MULTICAST_LOOP`): on unless cleared.
    pub loop4: bool,
    /// IPv6 multicast hop limit (`IPV6_MULTICAST_HOPS`): 1 unless set.
    pub hops: u8,
    /// Whether IPv6 multicast sends loop back (`IPV6_MULTICAST_LOOP`): on
    /// unless cleared.
    pub loop6: bool,
}

impl<const N: usize> Default for SocketGroups<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SocketGroups<N> {
    /// No groups, Linux's defaults: TTL and hop limit 1, both loops on.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            groups: [None; N],
            ttl: 1,
            loop4: true,
            hops: 1,
            loop6: true,
        }
    }

    /// Whether the socket is in `group`.
    #[must_use]
    pub fn contains(&self, group: &Group) -> bool {
        self.groups.iter().flatten().any(|g| g == group)
    }

    /// Add `group` to the socket's set.
    ///
    /// # Errors
    ///
    /// [`Refused::NotMulticast`], [`Refused::AlreadyIn`], [`Refused::Full`].
    pub fn join(&mut self, group: Group) -> Result<(), Refused> {
        if !group.is_multicast() {
            return Err(Refused::NotMulticast);
        }
        if self.contains(&group) {
            return Err(Refused::AlreadyIn);
        }
        let slot = self
            .groups
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Refused::Full)?;
        *slot = Some(group);
        Ok(())
    }

    /// Remove `group` from the socket's set.
    ///
    /// # Errors
    ///
    /// [`Refused::NotIn`].
    pub fn leave(&mut self, group: &Group) -> Result<(), Refused> {
        let slot = self
            .groups
            .iter_mut()
            .find(|s| s.as_ref() == Some(group))
            .ok_or(Refused::NotIn)?;
        *slot = None;
        Ok(())
    }

    /// The socket's groups, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = Group> + '_ {
        self.groups.iter().flatten().copied()
    }
}

/// The host's groups, at most `M`, each with how many sockets are in it.
#[derive(Debug, Clone, Copy)]
pub struct HostGroups<const M: usize> {
    entries: [Option<(Group, u16)>; M],
}

impl<const M: usize> Default for HostGroups<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const M: usize> HostGroups<M> {
    /// No groups.
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: [None; M] }
    }

    /// How many sockets are in `group` (0 when the host is not).
    #[must_use]
    pub fn count(&self, group: &Group) -> u16 {
        self.entries
            .iter()
            .flatten()
            .find(|(g, _)| g == group)
            .map_or(0, |&(_, n)| n)
    }

    /// Whether any socket is in `group`: whether a datagram addressed to it
    /// is for this host.
    #[must_use]
    pub fn contains(&self, group: &Group) -> bool {
        self.count(group) > 0
    }

    /// A socket joined `group`: count it, and say whether the host has just
    /// joined (a report to send).
    ///
    /// # Errors
    ///
    /// [`Refused::NotMulticast`]; [`Refused::Full`] when the host is in `M`
    /// groups already and this is a new one.
    pub fn join(&mut self, group: Group) -> Result<Announce, Refused> {
        if !group.is_multicast() {
            return Err(Refused::NotMulticast);
        }
        if let Some((_, n)) = self.entries.iter_mut().flatten().find(|(g, _)| *g == group) {
            *n = n.saturating_add(1);
            return Ok(Announce::Nothing);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|s| s.is_none())
            .ok_or(Refused::Full)?;
        *slot = Some((group, 1));
        Ok(if group.reportable() {
            Announce::Report(group)
        } else {
            Announce::Nothing
        })
    }

    /// A socket left `group`: uncount it, and say whether the host has just
    /// left (a leave to send). Leaving a group no socket is in is
    /// [`Announce::Nothing`]: the socket's own set ([`SocketGroups::leave`])
    /// is what refuses that.
    pub fn leave(&mut self, group: &Group) -> Announce {
        for slot in &mut self.entries {
            if let Some((g, n)) = slot
                && g == group
            {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    *slot = None;
                    return if group.reportable() {
                        Announce::Leave(*group)
                    } else {
                        Announce::Nothing
                    };
                }
                return Announce::Nothing;
            }
        }
        Announce::Nothing
    }

    /// The groups a router's query asks this host to report: every
    /// announced group of the query's family for a general query (`about`
    /// is `None`), or `about` alone when the host is in it.
    pub fn answer(&self, about: Option<Group>, v6: bool) -> impl Iterator<Item = Group> + '_ {
        self.entries.iter().flatten().filter_map(move |&(g, _)| {
            let family = matches!(g, Group::V6(_)) == v6;
            let asked = about.is_none_or(|a| a == g);
            (family && asked && g.reportable()).then_some(g)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MDNS4: Group = Group::V4([224, 0, 0, 251]);
    const SSDP4: Group = Group::V4([239, 255, 255, 250]);
    const MDNS6: Group = Group::V6([0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFB]);

    #[test]
    fn a_socket_set_joins_leaves_and_refuses_as_linux_does() {
        let mut s: SocketGroups<2> = SocketGroups::new();
        assert_eq!((s.ttl, s.loop4, s.hops, s.loop6), (1, true, 1, true));
        assert_eq!(s.join(MDNS4), Ok(()));
        assert_eq!(s.join(MDNS4), Err(Refused::AlreadyIn));
        assert_eq!(s.join(Group::V4([10, 0, 0, 1])), Err(Refused::NotMulticast));
        assert_eq!(s.join(MDNS6), Ok(()));
        assert_eq!(s.join(SSDP4), Err(Refused::Full));
        assert!(s.contains(&MDNS4) && s.contains(&MDNS6));
        assert_eq!(s.leave(&SSDP4), Err(Refused::NotIn));
        assert_eq!(s.leave(&MDNS4), Ok(()));
        assert!(!s.contains(&MDNS4));
        assert_eq!(s.join(SSDP4), Ok(()));
        assert_eq!(s.iter().count(), 2);
    }

    #[test]
    fn the_host_announces_the_first_join_and_the_last_leave() {
        let mut h: HostGroups<4> = HostGroups::new();
        assert_eq!(h.join(MDNS4), Ok(Announce::Report(MDNS4)));
        assert_eq!(h.join(MDNS4), Ok(Announce::Nothing));
        assert_eq!(h.count(&MDNS4), 2);
        assert_eq!(h.leave(&MDNS4), Announce::Nothing);
        assert!(h.contains(&MDNS4));
        assert_eq!(h.leave(&MDNS4), Announce::Leave(MDNS4));
        assert!(!h.contains(&MDNS4));
        // Leaving again is not an announcement.
        assert_eq!(h.leave(&MDNS4), Announce::Nothing);
    }

    #[test]
    fn all_hosts_and_all_nodes_are_counted_but_never_announced() {
        let mut h: HostGroups<4> = HostGroups::new();
        let all_hosts = Group::V4(igmp::ALL_HOSTS);
        let all_nodes = Group::V6(crate::icmpv6::ALL_NODES_LINK_LOCAL);
        assert_eq!(h.join(all_hosts), Ok(Announce::Nothing));
        assert_eq!(h.join(all_nodes), Ok(Announce::Nothing));
        assert!(h.contains(&all_hosts) && h.contains(&all_nodes));
        assert_eq!(h.leave(&all_hosts), Announce::Nothing);
        assert_eq!(h.answer(None, false).count(), 0);
    }

    #[test]
    fn a_full_host_refuses_a_new_group_but_counts_a_known_one() {
        let mut h: HostGroups<1> = HostGroups::new();
        assert_eq!(h.join(MDNS4), Ok(Announce::Report(MDNS4)));
        assert_eq!(h.join(SSDP4), Err(Refused::Full));
        assert_eq!(h.join(MDNS4), Ok(Announce::Nothing));
        assert_eq!(
            h.join(Group::V6([
                0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1
            ])),
            Err(Refused::NotMulticast)
        );
    }

    #[test]
    fn a_query_is_answered_with_the_groups_it_asks_about() {
        let mut h: HostGroups<4> = HostGroups::new();
        for g in [MDNS4, SSDP4, MDNS6] {
            h.join(g).unwrap();
        }
        let general4: std::vec::Vec<Group> = h.answer(None, false).collect();
        assert_eq!(general4.len(), 2);
        assert!(general4.contains(&MDNS4) && general4.contains(&SSDP4));
        let general6: std::vec::Vec<Group> = h.answer(None, true).collect();
        assert_eq!(general6, std::vec![MDNS6]);
        let specific: std::vec::Vec<Group> = h.answer(Some(SSDP4), false).collect();
        assert_eq!(specific, std::vec![SSDP4]);
        // A group the host is not in is not answered.
        assert_eq!(h.answer(Some(Group::V4([239, 1, 1, 1])), false).count(), 0);
    }
}
