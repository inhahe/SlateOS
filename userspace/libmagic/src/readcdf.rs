//! libmagic's `readcdf.c`: describing a Composite Document File from its
//! summary information -- the application that wrote it, its title, author,
//! dates -- or, failing that, from the names of the streams in it ("CDFV2
//! Microsoft Word"), or, failing that too, from what went wrong reading it.

use crate::buffer::Buffer;
use crate::cdf::{
    CDF_CLIPBOARD, CDF_DIR_TYPE_USER_STORAGE, CDF_DIR_TYPE_USER_STREAM, CDF_DOUBLE, CDF_FILETIME, CDF_FLOAT,
    CDF_LENGTH32_STRING, CDF_LENGTH32_WSTRING, CDF_NULL, CDF_PROPERTY_NAME_OF_APPLICATION, CDF_SIGNED16,
    CDF_SIGNED32, CDF_UNSIGNED32, Cdf, Dir, ESRCH, Header, Info, Prop, Sat, Stream, print_elapsed_time,
    print_property_name, unpack_summary_info,
};
use crate::funcs::Ms;
use crate::magic::{MAGIC_APPLE, MAGIC_EXTENSION, MAGIC_MIME, MAGIC_MIME_TYPE};
use crate::printf::Arg;

/// `NOTMIME`.
fn notmime(ms: &Ms) -> bool {
    ms.flags & MAGIC_MIME == 0
}

/// `app2mime`: what an application name says the document is.
const APP2MIME: &[(&str, &str)] = &[
    ("Word", "msword"),
    ("Excel", "vnd.ms-excel"),
    ("Powerpoint", "vnd.ms-powerpoint"),
    ("Crystal Reports", "x-rpt"),
    ("Advanced Installer", "vnd.ms-msi"),
    ("InstallShield", "vnd.ms-msi"),
    ("Microsoft Patch Compiler", "vnd.ms-msi"),
    ("NAnt", "vnd.ms-msi"),
    ("Windows Installer", "vnd.ms-msi"),
];

/// `name2mime` and `name2desc`: what a stream name says it is.
const NAME2MIME: &[(&str, &str)] = &[
    ("Book", "vnd.ms-excel"),
    ("Workbook", "vnd.ms-excel"),
    ("WordDocument", "msword"),
    ("PowerPoint", "vnd.ms-powerpoint"),
    ("DigitalSignature", "vnd.ms-msi"),
];
const NAME2DESC: &[(&str, &str)] = &[
    ("Book", "Microsoft Excel"),
    ("Workbook", "Microsoft Excel"),
    ("WordDocument", "Microsoft Word"),
    ("PowerPoint", "Microsoft PowerPoint"),
    ("DigitalSignature", "Microsoft Installer"),
];

/// `clsid2mime` and `clsid2desc`: the root storage's class.
const MSI_CLSID: [u64; 2] = [0x0000_0000_000c_1084, 0x4600_0000_0000_00c0];

fn clsid_to_mime(clsid: [u64; 2], desc: bool) -> Option<&'static str> {
    (clsid == MSI_CLSID).then_some(if desc { "MSI Installer" } else { "x-msi" })
}

/// `strcasestr` in the C locale.
fn strcasestr(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// `cdf_app_to_mime`: the first pattern found in `vbuf`, case aside.
fn app_to_mime(vbuf: &[u8], nv: &[(&str, &'static str)]) -> Option<&'static str> {
    let vbuf = crate::cstd::cstr(vbuf);
    nv.iter()
        .find(|(pat, _)| strcasestr(vbuf, pat.as_bytes()))
        .map(|&(_, mime)| mime)
}

/// `cdf_file_property_info`: each property, in order; -1 at a type it does
/// not know (which every vector is).
fn file_property_info(ms: &mut Ms, cdf: &mut Cdf<'_>, sst: &Stream, info: &[Prop], root: Option<&Dir>) -> i32 {
    let mut str_: Option<&'static str> = None;
    if !notmime(ms) {
        if let Some(r) = root {
            str_ = clsid_to_mime(r.storage_uuid, false);
        }
    }
    let tab = sst.bytes();
    for p in info {
        let buf = print_property_name(p.id);
        let nm = Arg::Str(&buf);
        let r = match p.typ {
            CDF_NULL | CDF_CLIPBOARD => 0,
            #[allow(clippy::cast_possible_truncation)]
            CDF_SIGNED16 => {
                if notmime(ms) {
                    // `%hd` of the `int` the short promotes to.
                    let v = i32::from(p.val as u16 as i16);
                    ms.printf(b", %s: %hd", &[nm, Arg::I32(v as u32)])
                } else {
                    0
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            CDF_SIGNED32 | CDF_UNSIGNED32 => {
                if notmime(ms) {
                    let f: &[u8] = if p.typ == CDF_SIGNED32 { b", %s: %d" } else { b", %s: %u" };
                    ms.printf(f, &[nm, Arg::I32(p.val as u32)])
                } else {
                    0
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            CDF_FLOAT => {
                if notmime(ms) {
                    let v = f32::from_bits(p.val as u32);
                    ms.printf(b", %s: %g", &[nm, Arg::F64(f64::from(v))])
                } else {
                    0
                }
            }
            CDF_DOUBLE => {
                if notmime(ms) {
                    ms.printf(b", %s: %g", &[nm, Arg::F64(f64::from_bits(p.val))])
                } else {
                    0
                }
            }
            CDF_LENGTH32_STRING | CDF_LENGTH32_WSTRING => {
                // `int len = s_len`: a length past `INT_MAX` is negative.
                #[allow(clippy::cast_possible_wrap)]
                let mut len = p.str_len as i32;
                if len > 1 {
                    let k = if p.typ == CDF_LENGTH32_WSTRING { 2 } else { 1 };
                    let mut vbuf: Vec<u8> = Vec::new();
                    #[allow(clippy::cast_sign_loss)]
                    let end = p.str_off + len as usize;
                    let mut s = p.str_off;
                    while s < end && vbuf.len() < 1024 {
                        let more = len != 0;
                        len = len.wrapping_sub(1);
                        if !more {
                            break;
                        }
                        let c = tab.get(s).copied().unwrap_or(0);
                        if c == 0 {
                            break;
                        }
                        if crate::cstd::isprint(c) {
                            vbuf.push(c);
                        }
                        s += k;
                    }
                    if vbuf.len() == 1024 {
                        vbuf.pop();
                    }
                    if notmime(ms) {
                        if vbuf.is_empty() {
                            0
                        } else {
                            ms.printf(b", %s: %s", &[nm, Arg::Str(&vbuf)])
                        }
                    } else {
                        if str_.is_none() && p.id == CDF_PROPERTY_NAME_OF_APPLICATION {
                            str_ = app_to_mime(&vbuf, APP2MIME);
                        }
                        0
                    }
                } else {
                    0
                }
            }
            CDF_FILETIME => {
                #[allow(clippy::cast_possible_wrap)]
                let tp = p.val as i64;
                if tp == 0 {
                    0
                } else if tp < 1_000_000_000_000_000 {
                    let tbuf = print_elapsed_time(tp);
                    if notmime(ms) {
                        ms.printf(b", %s: %s", &[nm, Arg::Str(&tbuf)])
                    } else {
                        0
                    }
                } else {
                    // A time `mktime` refuses is -1, the second before 1970,
                    // and leaves EINVAL.
                    let sec = crate::cdf_time::cdf_timestamp_to_timespec(tp).unwrap_or_else(|| {
                        cdf.errno = crate::cdf::EFTYPE;
                        -1
                    });
                    let c = crate::cdf_time::cdf_ctime(sec);
                    let c = c.split(|&b| b == b'\n').next().unwrap_or_default();
                    if notmime(ms) {
                        ms.printf(b", %s: %s", &[nm, Arg::Str(c)])
                    } else {
                        0
                    }
                }
            }
            _ => return -1,
        };
        if r == -1 {
            return -1;
        }
    }
    if ms.flags & MAGIC_MIME_TYPE != 0 {
        let Some(s) = str_ else {
            return 0;
        };
        if ms.printf(b"application/%s", &[Arg::Str(s.as_bytes())]) == -1 {
            return -1;
        }
    }
    1
}

/// `cdf_file_summary_info`.
fn file_summary_info(ms: &mut Ms, cdf: &mut Cdf<'_>, sst: &Stream, root: Option<&Dir>) -> i32 {
    let Some((si, info)) = unpack_summary_info(cdf, sst) else {
        return -1;
    };
    if notmime(ms) {
        if ms.print_str(b"Composite Document File V2 Document") == -1 {
            return -1;
        }
        let endian: &[u8] = if si.byte_order == 0xfffe { b"Little" } else { b"Big" };
        if ms.printf(b", %s Endian", &[Arg::Str(endian)]) == -1 {
            return -2;
        }
        let lo = u32::from(si.os_version & 0xff);
        let hi = u32::from(si.os_version) >> 8;
        let r = match si.os {
            2 => ms.printf(b", Os: Windows, Version %d.%d", &[Arg::I32(lo), Arg::I32(hi)]),
            1 => ms.printf(b", Os: MacOS, Version %d.%d", &[Arg::I32(hi), Arg::I32(lo)]),
            os => ms.printf(
                b", Os %d, Version: %d.%d",
                &[Arg::I32(u32::from(os)), Arg::I32(lo), Arg::I32(hi)],
            ),
        };
        if r == -1 {
            return -2;
        }
        if let Some(r) = root {
            if let Some(s) = clsid_to_mime(r.storage_uuid, true) {
                if ms.printf(b", %s", &[Arg::Str(s.as_bytes())]) == -1 {
                    return -2;
                }
            }
        }
    }
    let m = file_property_info(ms, cdf, sst, &info, root);
    if m == -1 { -2 } else { m }
}

/// `cdf_check_summary_info`.
///
/// Upstream ends with a fall back to the Thumbs.db catalog, taken when the
/// answer so far is not positive -- which it always is by then (the property
/// printer gives 1, or 0 only under `--mime-type`, which then prints a type
/// and sets 1). That branch cannot run, and is not here.
fn check_summary_info(ms: &mut Ms, cdf: &mut Cdf<'_>, scn: &Stream, dir: &[Dir], root: Option<&Dir>, expn: &mut &'static str) -> i32 {
    let i = file_summary_info(ms, cdf, scn, root);
    if i < 0 {
        *expn = "Can't expand summary_info";
        return i;
    }
    if i == 1 {
        return i;
    }
    let mut str_: Option<&'static str> = None;
    for d in dir {
        if str_.is_some() {
            break;
        }
        // `(char)` of each of the 32 code units -- a C string that, with no
        // NUL among them, upstream reads on past the array; here it ends.
        #[allow(clippy::cast_possible_truncation)]
        let name: Vec<u8> = d.name.iter().map(|&c| c as u8).collect();
        str_ = app_to_mime(&name, if notmime(ms) { NAME2DESC } else { NAME2MIME });
    }
    let mut i = i;
    if notmime(ms) {
        if let Some(s) = str_ {
            if ms.print_str(s.as_bytes()) == -1 {
                return -1;
            }
            i = 1;
        }
    } else if ms.flags & MAGIC_MIME_TYPE != 0 {
        let s = str_.unwrap_or("vnd.ms-office");
        if ms.printf(b"application/%s", &[Arg::Str(s.as_bytes())]) == -1 {
            return -1;
        }
        i = 1;
    }
    i
}

/// `sectioninfo[]`: stream names that say what a document is.
struct SInfo {
    name: &'static str,
    mime: &'static str,
    sections: &'static [(&'static [u8], u8)],
}

const SECTIONINFO: &[SInfo] = &[
    SInfo {
        name: "Encrypted",
        mime: "encrypted",
        sections: &[(b"EncryptedPackage", CDF_DIR_TYPE_USER_STREAM), (b"EncryptedSummary", CDF_DIR_TYPE_USER_STREAM)],
    },
    SInfo {
        name: "QuickBooks",
        mime: "quickbooks",
        sections: &[(b"mfbu_header", CDF_DIR_TYPE_USER_STREAM)],
    },
    SInfo {
        name: "Microsoft Excel",
        mime: "vnd.ms-excel",
        sections: &[(b"Book", CDF_DIR_TYPE_USER_STREAM), (b"Workbook", CDF_DIR_TYPE_USER_STREAM)],
    },
    SInfo {
        name: "Microsoft Word",
        mime: "msword",
        sections: &[(b"WordDocument", CDF_DIR_TYPE_USER_STREAM)],
    },
    SInfo {
        name: "Microsoft PowerPoint",
        mime: "vnd.ms-powerpoint",
        sections: &[(b"PowerPoint", CDF_DIR_TYPE_USER_STREAM)],
    },
    SInfo {
        name: "Microsoft Outlook Message",
        mime: "vnd.ms-outlook",
        sections: &[
            (b"__properties_version1.0", CDF_DIR_TYPE_USER_STREAM),
            (b"__recip_version1.0_#00000000", CDF_DIR_TYPE_USER_STORAGE),
        ],
    },
];

/// `cdf_file_dir_info`.
fn file_dir_info(ms: &mut Ms, cdf: &mut Cdf<'_>, dir: &[Dir]) -> i32 {
    for si in SECTIONINFO {
        let found = si.sections.iter().any(|&(name, typ)| cdf.find_stream(dir, name, typ) > 0);
        if !found {
            continue;
        }
        if notmime(ms) {
            if ms.printf(b"CDFV2 %s", &[Arg::Str(si.name.as_bytes())]) == -1 {
                return -1;
            }
        } else if ms.flags & MAGIC_MIME_TYPE != 0 && ms.printf(b"application/%s", &[Arg::Str(si.mime.as_bytes())]) == -1 {
            return -1;
        }
        return 1;
    }
    -1
}

/// The parts of the container read so far: what `file_trycdf` frees on its
/// way out.
struct Parts {
    h: Header,
    sat: Sat,
    ssat: Sat,
    dir: Vec<Dir>,
    sst: Stream,
    root: Option<usize>,
}

/// `file_trycdf`'s reading, from the SAT on: `Ok` with the value it reaches
/// `out0` with (and what went wrong, if nothing described the file), `Err`
/// with one it returns at once.
fn read_parts(ms: &mut Ms, cdf: &mut Cdf<'_>, h: Header, expn: &mut &'static str) -> Result<i32, i32> {
    let Some(sat) = cdf.read_sat(&h) else {
        *expn = "Can't read SAT";
        return Ok(-1);
    };
    let Some(ssat) = cdf.read_ssat(&h, &sat) else {
        *expn = "Can't read SSAT";
        return Ok(-1);
    };
    let Some(dir) = cdf.read_dir(&h, &sat) else {
        *expn = "Can't read directory";
        return Ok(-1);
    };
    let mut sst = Stream::default();
    let (i, root) = cdf.read_short_stream(&h, &sat, &dir, &mut sst);
    if i == -1 {
        *expn = "Cannot read short stream";
        return Ok(-1);
    }
    let p = Parts {
        h,
        sat,
        ssat,
        dir,
        sst,
        root,
    };
    describe(ms, cdf, &p, expn)
}

/// The rest of `file_trycdf`: an HWP file, the summary information, the
/// document summary information, the stream names.
fn describe(ms: &mut Ms, cdf: &mut Cdf<'_>, p: &Parts, expn: &mut &'static str) -> Result<i32, i32> {
    let root = p.root.and_then(|r| p.dir.get(r));
    let mut scn = Stream::default();
    if cdf.read_user_stream(&p.h, &p.sat, &p.ssat, &p.sst, &p.dir, b"FileHeader", &mut scn) != -1 {
        const HWP5_SIGNATURE: &[u8] = b"HWP Document File";
        if scn.len.wrapping_mul(scn.ss) >= HWP5_SIGNATURE.len() && scn.bytes().starts_with(HWP5_SIGNATURE) {
            if notmime(ms) {
                if ms.print_str(b"Hancom HWP (Hangul Word Processor) file, version 5.0") == -1 {
                    return Err(-1);
                }
            } else if ms.flags & MAGIC_MIME_TYPE != 0 && ms.print_str(b"application/x-hwp") == -1 {
                return Err(-1);
            }
            return Ok(1);
        }
        scn.zero();
    }

    let mut i = cdf.read_user_stream(&p.h, &p.sat, &p.ssat, &p.sst, &p.dir, b"\x05SummaryInformation", &mut scn);
    if i == -1 {
        if cdf.errno != ESRCH {
            *expn = "Cannot read summary info";
        }
    } else {
        i = check_summary_info(ms, cdf, &scn, &p.dir, root, expn);
        scn.zero();
    }
    if i <= 0 {
        i = cdf.read_user_stream(&p.h, &p.sat, &p.ssat, &p.sst, &p.dir, b"\x05DocumentSummaryInformation", &mut scn);
        if i == -1 {
            if cdf.errno != ESRCH {
                *expn = "Cannot read summary info";
            }
        } else {
            i = check_summary_info(ms, cdf, &scn, &p.dir, root, expn);
        }
    }
    if i <= 0 {
        i = file_dir_info(ms, cdf, &p.dir);
        if i < 0 {
            *expn = "Cannot read section info";
        }
    }
    Ok(i)
}

/// `file_trycdf`: 1 (or another nonzero value) when the file was a CDF and
/// was described.
pub fn file_trycdf(ms: &mut Ms, b: &Buffer<'_>) -> i32 {
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let mut cdf = Cdf {
        info: Info {
            fd: b.fd,
            buf: b.fbuf,
        },
        errno: ms.errno,
    };
    let Some(h) = cdf.read_header() else {
        ms.errno = cdf.errno;
        return 0;
    };
    let mut expn: &'static str = "";
    let i = read_parts(ms, &mut cdf, h, &mut expn);
    ms.errno = cdf.errno;
    let i = match i {
        Ok(i) => i,
        Err(r) => return r,
    };
    // "If we handled it already, return".
    if i != -1 {
        return i;
    }
    // "Provide a default handler".
    if notmime(ms) {
        if ms.print_str(b"Composite Document File V2 Document") == -1 {
            return -1;
        }
        if !expn.is_empty() && ms.printf(b", %s", &[Arg::Str(expn.as_bytes())]) == -1 {
            return -1;
        }
    } else if ms.flags & MAGIC_MIME_TYPE != 0 && ms.print_str(b"application/x-ole-storage") == -1 {
        return -1;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_names_match_case_insensitively() {
        assert_eq!(app_to_mime(b"Microsoft Office Word\0junk", APP2MIME), Some("msword"));
        assert_eq!(app_to_mime(b"microsoft excel", APP2MIME), Some("vnd.ms-excel"));
        assert_eq!(app_to_mime(b"Writer", APP2MIME), None);
        // The first pattern in the table wins.
        assert_eq!(app_to_mime(b"Excel and Word", APP2MIME), Some("msword"));
    }

    #[test]
    fn the_msi_class_is_known() {
        assert_eq!(clsid_to_mime(MSI_CLSID, true), Some("MSI Installer"));
        assert_eq!(clsid_to_mime([0, 0], false), None);
    }
}
