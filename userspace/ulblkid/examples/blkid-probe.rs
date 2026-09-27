//! The subject side of `scripts/blkid-diff.sh`: the port's answers about
//! each image named on the command line, printed exactly as
//! `scripts/blkid-probe.c` prints libblkid's -- see that file for the four
//! questions asked. A test instrument, not a utility: it lives in
//! `examples/` so that nothing installs it.

use std::io::Write;

use ulblkid::partitions::{Partlist, Parttable};
use ulblkid::{
    PARTS_ENTRY_DETAILS, PARTS_FORCE_GPT, PARTS_MAGIC, Probe, SUBLKS_BADCSUM, SUBLKS_FSINFO,
    SUBLKS_LABEL, SUBLKS_LABELRAW, SUBLKS_MAGIC, SUBLKS_SECTYPE, SUBLKS_TYPE, SUBLKS_USAGE,
    SUBLKS_UUID, SUBLKS_UUIDRAW, SUBLKS_VERSION,
};

/// Printable ASCII but the backslash as itself, everything else `\xHH`.
fn put_escaped(out: &mut Vec<u8>, d: &[u8]) {
    for &c in d {
        if (0x20..0x7f).contains(&c) && c != b'\\' {
            out.push(c);
        } else {
            out.extend_from_slice(format!("\\x{c:02x}").as_bytes());
        }
    }
}

/// A C string or `-` for none.
fn put_str(out: &mut Vec<u8>, s: Option<&[u8]>) {
    match s {
        Some(s) if !s.is_empty() => put_escaped(out, s),
        _ => out.push(b'-'),
    }
}

fn dump_values(out: &mut Vec<u8>, pr: &Probe) {
    for v in pr.values() {
        out.extend_from_slice(format!("  {} len={} ", v.name, v.len()).as_bytes());
        put_escaped(out, v.data());
        out.push(b'\n');
    }
}

fn open_probe(path: &[u8], badcsum: bool) -> Option<Probe> {
    let mut pr = Probe::from_filename(path).ok()?;
    pr.enable_superblocks(true);
    pr.set_superblocks_flags(
        SUBLKS_LABEL
            | SUBLKS_LABELRAW
            | SUBLKS_UUID
            | SUBLKS_UUIDRAW
            | SUBLKS_TYPE
            | SUBLKS_SECTYPE
            | SUBLKS_USAGE
            | SUBLKS_VERSION
            | SUBLKS_MAGIC
            | SUBLKS_FSINFO
            | if badcsum { SUBLKS_BADCSUM } else { 0 },
    );
    pr.enable_partitions(true);
    pr.set_partitions_flags(PARTS_ENTRY_DETAILS | PARTS_MAGIC);
    Some(pr)
}

fn dump_table(out: &mut Vec<u8>, ls: &Partlist, tab: Option<&Parttable>) {
    let Some(tab) = tab else {
        out.extend_from_slice(b"none");
        return;
    };
    out.extend_from_slice(b"type=");
    put_str(out, Some(tab.ty.as_bytes()));
    out.extend_from_slice(format!(" offset={} id=", tab.offset.cast_signed()).as_bytes());
    put_str(out, tab.id());
    let parent = tab
        .parent
        .and_then(|p| ls.partition(p.0))
        .map_or(0, |p| p.partno);
    out.extend_from_slice(format!(" parent={parent}").as_bytes());
}

fn dump_parts(out: &mut Vec<u8>, path: &[u8], flags: u32) {
    let Ok(mut pr) = Probe::from_filename(path) else {
        out.extend_from_slice(b"  open failed\n");
        return;
    };
    pr.enable_superblocks(false);
    pr.enable_partitions(true);
    pr.set_partitions_flags(flags);
    let Some(ls) = pr.partitions() else {
        out.extend_from_slice(b"  none\n");
        return;
    };
    out.extend_from_slice(b"  root ");
    dump_table(out, ls, ls.table().and_then(|t| ls.tab(t)));
    out.push(b'\n');
    for par in ls.partitions() {
        out.extend_from_slice(
            format!(
                "  part {} start={} size={} type=0x{:x} typestr=",
                par.partno,
                par.start.cast_signed(),
                par.size.cast_signed(),
                par.ty
            )
            .as_bytes(),
        );
        put_str(out, par.type_string());
        out.extend_from_slice(b" name=");
        put_str(out, par.name());
        out.extend_from_slice(b" uuid=");
        put_str(out, par.uuid());
        let kind = [
            if ls.is_primary(par) { 'P' } else { '-' },
            if ls.is_extended(par) { 'E' } else { '-' },
            if ls.is_logical(par) { 'L' } else { '-' },
        ];
        out.extend_from_slice(
            format!(
                " flags=0x{:x} kind={}{}{} table: ",
                par.flags, kind[0], kind[1], kind[2]
            )
            .as_bytes(),
        );
        dump_table(out, ls, ls.tab(par.tab));
        out.push(b'\n');
    }
}

fn main() {
    let mut out = Vec::new();
    for arg in std::env::args_os().skip(1) {
        let path = quoting::os_bytes(&arg).into_owned();
        let base = path
            .iter()
            .rposition(|&b| b == b'/')
            .and_then(|i| path.get(i.saturating_add(1)..))
            .unwrap_or(&path);
        out.extend_from_slice(b"# ");
        out.extend_from_slice(base);
        out.push(b'\n');

        for badcsum in [false, true] {
            let Some(mut pr) = open_probe(&path, badcsum) else {
                out.extend_from_slice(b"open failed\n");
                continue;
            };
            let rc = pr.do_safeprobe();
            let what = if badcsum { "badcsum" } else { "safeprobe" };
            out.extend_from_slice(format!("{what} rc={rc}\n").as_bytes());
            dump_values(&mut out, &pr);
        }

        if let Some(mut pr) = open_probe(&path, false) {
            let rc = pr.do_fullprobe();
            out.extend_from_slice(format!("fullprobe rc={rc}\n").as_bytes());
            dump_values(&mut out, &pr);
        }

        if let Some(mut pr) = open_probe(&path, false) {
            out.extend_from_slice(b"walk\n");
            for _ in 0..32 {
                let rc = pr.do_probe();
                out.extend_from_slice(format!(" step rc={rc}\n").as_bytes());
                if rc != 0 {
                    break;
                }
                dump_values(&mut out, &pr);
                let w = pr.do_wipe(true);
                out.extend_from_slice(format!(" wipe rc={w}\n").as_bytes());
            }
        }

        out.extend_from_slice(b"parts\n");
        dump_parts(&mut out, &path, 0);
        out.extend_from_slice(b"parts-gpt\n");
        dump_parts(&mut out, &path, PARTS_FORCE_GPT);
    }
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&out)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        std::process::exit(1);
    }
}
