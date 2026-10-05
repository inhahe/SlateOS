//! A folder's methods as 7-Zip names them in a listing:
//! `CHandler::SetMethodToProp` of `CPP/7zip/Archive/7z/7zHandler.cpp`
//! (LZMA SDK 26.00, public domain), ported -- "LZMA2:24 BCJ",
//! "PPMD:o6:mem22", "BCJ2 LZMA:23 LZMA:20:lc0:lp2 LZMA:20:lc0:lp2".
//!
//! Each coder is its name and, for some, its settings; they are listed from
//! the folder's last coder to its first. 7-Zip builds the text backwards in
//! a 256-byte buffer and stops, with "... " in front, when the next coder
//! would not fit; the port keeps that limit.

use alloc::string::String;
use alloc::vec::Vec;

use crate::decode::method;
use crate::header::Coder;

/// `GetStringForSizeValue`: a power of two as its exponent, else a number
/// of megabytes, kilobytes or bytes with `m`, `k` or `b`.
fn size_value(val: u32) -> String {
    if val.is_power_of_two() {
        return alloc::format!("{}", val.trailing_zeros());
    }
    if val & ((1 << 20) - 1) == 0 {
        alloc::format!("{}m", val >> 20)
    } else if val & ((1 << 10) - 1) == 0 {
        alloc::format!("{}k", val >> 10)
    } else {
        alloc::format!("{val}b")
    }
}

/// `GetLzma2String`: the LZMA2 dictionary property as a power of two, or a
/// size in `k` or `m`.
fn lzma2_value(d: u8) -> String {
    if d > 40 {
        String::new()
    } else if d & 1 == 0 {
        alloc::format!("{}", u32::from(d >> 1) + 12)
    } else {
        // `d` is at most 40 here, so the shift below is at most 11.
        let mut d = u32::from(d >> 1).saturating_add(1);
        let mut c = 'k';
        if d >= 10 {
            c = 'm';
            d = d.saturating_sub(10);
        }
        alloc::format!("{}{c}", 3u32 << d)
    }
}

/// A method 7-Zip knows by name (`FindMethod` over its registered coders),
/// for the ids `SetMethodToProp` does not name itself.
fn registered_name(id: u64) -> Option<&'static str> {
    Some(match id {
        method::COPY => "Copy",
        method::DEFLATE => "Deflate",
        method::DEFLATE64 => "Deflate64",
        method::BZIP2 => "BZip2",
        method::PPC => "PPC",
        method::IA64 => "IA64",
        method::ARM => "ARM",
        method::ARMT => "ARMT",
        method::SPARC => "SPARC",
        0x02_0302 => "Swap2",
        0x02_0304 => "Swap4",
        _ => return None,
    })
}

/// `ConvertMethodIdToString`: an id in hex, two digits a byte.
fn hex_id(id: u64) -> String {
    let mut digits = Vec::new();
    let mut v = id;
    loop {
        for _ in 0..2 {
            let d = (v & 0xF) as u8;
            digits.push(if d < 10 {
                b'0'.saturating_add(d)
            } else {
                b'A'.saturating_add(d.saturating_sub(10))
            });
            v >>= 4;
        }
        if v == 0 {
            break;
        }
    }
    digits.iter().rev().map(|&b| char::from(b)).collect()
}

/// A coder's name and settings, as `SetMethodToProp` names one; `None` for a
/// method it leaves to `FindMethod`.
fn named(coder: &Coder) -> Option<(&'static str, String)> {
    let p = &coder.props;
    let le32 = |at: usize| -> Option<u32> {
        p.get(at..at.saturating_add(4))
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    };
    Some(match coder.method {
        method::LZMA => {
            let mut s = String::new();
            if let (5, Some(dic), Some(&d)) = (p.len(), le32(1), p.first()) {
                s = size_value(dic);
                if d != 0x5D {
                    let lc = d % 9;
                    let pb = d / 9 / 5;
                    let lp = d / 9 % 5;
                    if lc != 3 {
                        s.push_str(&alloc::format!(":lc{lc}"));
                    }
                    if lp != 0 {
                        s.push_str(&alloc::format!(":lp{lp}"));
                    }
                    if pb != 2 {
                        s.push_str(&alloc::format!(":pb{pb}"));
                    }
                }
            }
            ("LZMA", s)
        }
        method::LZMA2 => (
            "LZMA2",
            match p.as_slice() {
                &[d] => lzma2_value(d),
                _ => String::new(),
            },
        ),
        method::PPMD => (
            "PPMD",
            match (p.len(), p.first(), le32(1)) {
                (5, Some(&order), Some(mem)) => alloc::format!("o{order}:mem{}", size_value(mem)),
                _ => String::new(),
            },
        ),
        method::DELTA => (
            "Delta",
            match p.as_slice() {
                &[d] => alloc::format!("{}", u32::from(d) + 1),
                _ => String::new(),
            },
        ),
        method::ARM64 | method::RISCV => (
            if coder.method == method::ARM64 {
                "ARM64"
            } else {
                "RISCV"
            },
            match (p.len(), le32(0)) {
                (4, Some(pc)) => alloc::format!("{pc}"),
                _ => String::new(),
            },
        ),
        method::BCJ2 => ("BCJ2", String::new()),
        method::BCJ => ("BCJ", String::new()),
        method::AES => (
            "7zAES",
            p.first()
                .map_or_else(String::new, |&b| alloc::format!("{}", b & 0x3F)),
        ),
        _ => return None,
    })
}

/// `SetMethodToProp`: the folder's method text.
pub(crate) fn folder_method(coders: &[Coder]) -> String {
    const SIZE: usize = 256;
    // The text, built from its end: `pos` is where it starts.
    let mut pos = SIZE - 1;
    let mut parts: Vec<String> = Vec::new();
    let mut done = 0usize;
    for (i, coder) in coders.iter().enumerate() {
        if pos < 32 {
            break;
        }
        let space = usize::from(i != 0);
        let text = match named(coder) {
            Some((name, s)) => {
                // The name, ":" and the settings if there are any, and a
                // space before all but the first coder.
                let total = name
                    .len()
                    .saturating_add(s.len())
                    .saturating_add(usize::from(!s.is_empty()))
                    .saturating_add(space);
                if total.saturating_add(5) >= pos {
                    break;
                }
                pos = pos.saturating_sub(total);
                if s.is_empty() {
                    String::from(name)
                } else {
                    alloc::format!("{name}:{s}")
                }
            }
            None => {
                let name = registered_name(coder.method)
                    .map_or_else(|| hex_id(coder.method), String::from);
                let total = name.len().saturating_add(space);
                if registered_name(coder.method).is_some() && total.saturating_add(5) > pos {
                    break;
                }
                pos = pos.saturating_sub(total);
                name
            }
        };
        parts.push(text);
        done = done.saturating_add(1);
    }
    parts.reverse();
    let mut out = parts.join(" ");
    if done != coders.len() && pos >= 4 {
        out.insert_str(0, "... ");
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn sizes_are_written_as_7zip_writes_them() {
        assert_eq!(size_value(1 << 24), "24");
        assert_eq!(size_value(3 << 20), "3m");
        assert_eq!(size_value(3 << 10), "3k");
        assert_eq!(size_value(1000), "1000b");
        assert_eq!(lzma2_value(16), "20");
        // 3 << 19 is 1536 KiB, 3 << 20 three MiB, 3 << 12 twelve KiB.
        assert_eq!(lzma2_value(17), "1536k");
        assert_eq!(lzma2_value(19), "3m");
        assert_eq!(lzma2_value(3), "12k");
        assert_eq!(lzma2_value(41), "");
        assert_eq!(hex_id(0x21), "21");
        assert_eq!(hex_id(0x03_0101), "030101");
        assert_eq!(hex_id(0), "00");
    }
}
