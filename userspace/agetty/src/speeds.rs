//! `speedtab`: the line speeds agetty knows, with the `B*` code `termios`
//! stores for each -- Linux's on x86-64, which SlateOS's C library keeps.

/// One speed: bits per second, and its `speed_t` code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Speed {
    /// Bits per second.
    pub bps: u32,
    /// `B9600` and the rest.
    pub code: u32,
}

/// `speedtab[]`, in upstream's order -- what `--list-speeds` prints.
pub const SPEEDS: [Speed; 31] = [
    Speed { bps: 50, code: 0o1 },
    Speed { bps: 75, code: 0o2 },
    Speed {
        bps: 110,
        code: 0o3,
    },
    Speed {
        bps: 134,
        code: 0o4,
    },
    Speed {
        bps: 150,
        code: 0o5,
    },
    Speed {
        bps: 200,
        code: 0o6,
    },
    Speed {
        bps: 300,
        code: 0o7,
    },
    Speed {
        bps: 600,
        code: 0o10,
    },
    Speed {
        bps: 1200,
        code: 0o11,
    },
    Speed {
        bps: 1800,
        code: 0o12,
    },
    Speed {
        bps: 2400,
        code: 0o13,
    },
    Speed {
        bps: 4800,
        code: 0o14,
    },
    Speed {
        bps: 9600,
        code: 0o15,
    },
    Speed {
        bps: 19200,
        code: 0o16,
    },
    Speed {
        bps: 38400,
        code: 0o17,
    },
    Speed {
        bps: 57600,
        code: 0o10001,
    },
    Speed {
        bps: 115200,
        code: 0o10002,
    },
    Speed {
        bps: 230400,
        code: 0o10003,
    },
    Speed {
        bps: 460800,
        code: 0o10004,
    },
    Speed {
        bps: 500000,
        code: 0o10005,
    },
    Speed {
        bps: 576000,
        code: 0o10006,
    },
    Speed {
        bps: 921600,
        code: 0o10007,
    },
    Speed {
        bps: 1000000,
        code: 0o10010,
    },
    Speed {
        bps: 1152000,
        code: 0o10011,
    },
    Speed {
        bps: 1500000,
        code: 0o10012,
    },
    Speed {
        bps: 2000000,
        code: 0o10013,
    },
    Speed {
        bps: 2500000,
        code: 0o10014,
    },
    Speed {
        bps: 3000000,
        code: 0o10015,
    },
    Speed {
        bps: 3500000,
        code: 0o10016,
    },
    Speed {
        bps: 4000000,
        code: 0o10017,
    },
    Speed { bps: 0, code: 0 },
];

/// `bcode (s)`: the code of the speed `s` names, by `strtol` -- leading
/// space and a sign allowed, anything after the digits ignored -- or 0 when
/// it names none.
#[must_use]
pub fn bcode(s: &[u8]) -> u32 {
    // `errno || !end || end == s`: no digits, or `ERANGE`. What follows the
    // digits is not looked at.
    let Some(scanned) = ulstrutils::scan_integer(s, 10) else {
        return 0;
    };
    if scanned.saturated || scanned.negative {
        return 0;
    }
    SPEEDS
        .iter()
        .take_while(|sp| sp.bps != 0)
        .find(|sp| u128::from(sp.bps) == scanned.magnitude)
        .map_or(0, |sp| sp.code)
}

/// The bits per second of `code`, if the table has it -- `\b` in an issue
/// file.
#[must_use]
pub fn bps_of(code: u32) -> Option<u32> {
    SPEEDS
        .iter()
        .take_while(|sp| sp.bps != 0)
        .find(|sp| sp.code == code)
        .map(|sp| sp.bps)
}

/// `list_speeds ()`: every speed, right-aligned in ten columns.
#[must_use]
pub fn list() -> Vec<u8> {
    let mut out = Vec::new();
    for sp in SPEEDS.iter().take_while(|sp| sp.bps != 0) {
        out.extend_from_slice(format!("{:>10}\n", sp.bps).as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_speed_is_read_as_strtol_reads_it() {
        assert_eq!(bcode(b"9600"), 0o15);
        assert_eq!(bcode(b" 9600"), 0o15);
        assert_eq!(bcode(b"115200"), 0o10002);
        assert_eq!(bcode(b"9600x"), 0o15, "what follows the digits is ignored");
        assert_eq!(bcode(b"9601"), 0);
        assert_eq!(bcode(b"fast"), 0);
        assert_eq!(bcode(b""), 0);
    }

    #[test]
    fn the_list_is_the_table_right_aligned() {
        let list = list();
        assert!(list.starts_with(b"        50\n        75\n"));
        assert!(list.ends_with(b"   4000000\n"));
        assert_eq!(list.split(|&b| b == b'\n').count(), 31);
        assert_eq!(bps_of(0o15), Some(9600));
        assert_eq!(bps_of(0o20), None);
    }
}
