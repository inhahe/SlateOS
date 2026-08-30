//! `<linux/sockios.h>` — `ioctl(2)` numbers for sockets.
//!
//! The `SIOC*` ioctls predate netlink and are still how `ifconfig`,
//! `route`, `arp`, and lots of legacy code configure interfaces.
//! All the numbers live in the 0x89xx range so they don't collide
//! with file-system ioctls.

// ---------------------------------------------------------------------------
// Interface configuration ioctls (`SIOCGIF*` / `SIOCSIF*`)
// ---------------------------------------------------------------------------

pub const SIOCGIFNAME: u32 = 0x8910;
pub const SIOCSIFLINK: u32 = 0x8911;
pub const SIOCGIFCONF: u32 = 0x8912;
pub const SIOCGIFFLAGS: u32 = 0x8913;
pub const SIOCSIFFLAGS: u32 = 0x8914;
pub const SIOCGIFADDR: u32 = 0x8915;
pub const SIOCSIFADDR: u32 = 0x8916;
pub const SIOCGIFDSTADDR: u32 = 0x8917;
pub const SIOCSIFDSTADDR: u32 = 0x8918;
pub const SIOCGIFBRDADDR: u32 = 0x8919;
pub const SIOCSIFBRDADDR: u32 = 0x891A;
pub const SIOCGIFNETMASK: u32 = 0x891B;
pub const SIOCSIFNETMASK: u32 = 0x891C;
pub const SIOCGIFMETRIC: u32 = 0x891D;
pub const SIOCSIFMETRIC: u32 = 0x891E;
pub const SIOCGIFMEM: u32 = 0x891F;
pub const SIOCSIFMEM: u32 = 0x8920;
pub const SIOCGIFMTU: u32 = 0x8921;
pub const SIOCSIFMTU: u32 = 0x8922;
pub const SIOCSIFNAME: u32 = 0x8923;
pub const SIOCSIFHWADDR: u32 = 0x8924;
pub const SIOCGIFENCAP: u32 = 0x8925;
pub const SIOCSIFENCAP: u32 = 0x8926;
pub const SIOCGIFHWADDR: u32 = 0x8927;
pub const SIOCGIFSLAVE: u32 = 0x8929;
pub const SIOCSIFSLAVE: u32 = 0x8930;
pub const SIOCADDMULTI: u32 = 0x8931;
pub const SIOCDELMULTI: u32 = 0x8932;
pub const SIOCGIFINDEX: u32 = 0x8933;

// ---------------------------------------------------------------------------
// Routing-table ioctls
// ---------------------------------------------------------------------------

pub const SIOCADDRT: u32 = 0x890B;
pub const SIOCDELRT: u32 = 0x890C;

// ---------------------------------------------------------------------------
// ARP-cache ioctls
// ---------------------------------------------------------------------------

pub const SIOCDARP: u32 = 0x8953;
pub const SIOCGARP: u32 = 0x8954;
pub const SIOCSARP: u32 = 0x8955;

// ---------------------------------------------------------------------------
// Generic socket info
// ---------------------------------------------------------------------------

pub const FIONREAD: u32 = 0x541B;
pub const TIOCOUTQ: u32 = 0x5411;
pub const SIOCGSTAMP: u32 = 0x8906;
pub const SIOCGSTAMPNS: u32 = 0x8907;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifaddr_ioctls_in_0x89xx_range() {
        let i = [
            SIOCGIFNAME,
            SIOCGIFCONF,
            SIOCGIFFLAGS,
            SIOCSIFFLAGS,
            SIOCGIFADDR,
            SIOCSIFADDR,
            SIOCGIFDSTADDR,
            SIOCSIFDSTADDR,
            SIOCGIFBRDADDR,
            SIOCSIFBRDADDR,
            SIOCGIFNETMASK,
            SIOCSIFNETMASK,
            SIOCGIFMTU,
            SIOCSIFMTU,
            SIOCGIFHWADDR,
            SIOCGIFINDEX,
        ];
        for &v in i.iter() {
            assert_eq!(v & 0xFF00, 0x8900);
        }
    }

    #[test]
    fn test_get_set_pairs_adjacent() {
        // Each get/set pair sits on adjacent ioctl numbers.
        assert_eq!(SIOCSIFFLAGS, SIOCGIFFLAGS + 1);
        assert_eq!(SIOCSIFADDR, SIOCGIFADDR + 1);
        assert_eq!(SIOCSIFDSTADDR, SIOCGIFDSTADDR + 1);
        assert_eq!(SIOCSIFBRDADDR, SIOCGIFBRDADDR + 1);
        assert_eq!(SIOCSIFNETMASK, SIOCGIFNETMASK + 1);
        assert_eq!(SIOCSIFMTU, SIOCGIFMTU + 1);
        assert_eq!(SIOCSIFMETRIC, SIOCGIFMETRIC + 1);
        assert_eq!(SIOCSIFMEM, SIOCGIFMEM + 1);
        assert_eq!(SIOCSIFENCAP, SIOCGIFENCAP + 1);
    }

    #[test]
    fn test_route_ioctls_adjacent() {
        // SIOCADDRT and SIOCDELRT are adjacent.
        assert_eq!(SIOCDELRT, SIOCADDRT + 1);
    }

    #[test]
    fn test_arp_ioctls_consecutive() {
        // Three ARP-cache ioctls form a dense block 0x8953..0x8955.
        assert_eq!(SIOCGARP, SIOCDARP + 1);
        assert_eq!(SIOCSARP, SIOCGARP + 1);
    }

    #[test]
    fn test_fionread_and_tiocoutq_share_ttybase() {
        // FIONREAD and TIOCOUTQ live in the tty ioctl namespace (0x54xx)
        // because they apply to all character devices, not just sockets.
        assert_eq!(FIONREAD & 0xFF00, 0x5400);
        assert_eq!(TIOCOUTQ & 0xFF00, 0x5400);
    }

    #[test]
    fn test_socket_timestamp_ioctls() {
        // SIOCGSTAMP and SIOCGSTAMPNS live just below the SIOCG/S
        // block at 0x8906/0x8907.
        assert_eq!(SIOCGSTAMP, 0x8906);
        assert_eq!(SIOCGSTAMPNS, 0x8907);
        assert_eq!(SIOCGSTAMPNS, SIOCGSTAMP + 1);
    }
}
