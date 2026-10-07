//! FLAC's two CRCs (RFC 9639 §9.1.8, §9.3): CRC-8 with the polynomial 0x07
//! over a frame header, CRC-16 with 0x8005 over a whole frame -- both most
//! significant bit first, starting from 0, with no final xor.

/// CRC-8 of `data`.
pub(crate) fn crc8(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |c, &b| {
        let mut c = c ^ b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x07
            } else {
                c << 1
            };
        }
        c
    })
}

/// CRC-16 of `data`.
pub(crate) fn crc16(data: &[u8]) -> u16 {
    data.iter().fold(0u16, |c, &b| {
        let mut c = c ^ (u16::from(b) << 8);
        for _ in 0..8 {
            c = if c & 0x8000 != 0 {
                (c << 1) ^ 0x8005
            } else {
                c << 1
            };
        }
        c
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crcs_are_flacs() {
        // The check values of CRC-8/SMBUS and CRC-16/UMTS (both FLAC's).
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16(b"123456789"), 0xFEE8);
        assert_eq!(crc8(&[]), 0);
        assert_eq!(crc16(&[]), 0);
    }
}
