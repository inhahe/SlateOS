//! `disk-utils/fdisk-list.c`: a device's geometry, its partition table and
//! its free space, printed for `fdisk -l`, fdisk's `p` and `F` commands and
//! `sfdisk --list`; and the `-o` column list.
//!
//! Every line but the tables goes through libfdisk's info dialog, as
//! upstream's `fdisk_info` calls do, so the program's callback prints it.

use std::io::BufRead;

use libfdisk::{Context, Partition, Table, fieldtype};
use quoting::escape_unprintable;
use smartcols::{FL_RIGHT, FL_TRUNC};
use ulclosestream::{warn, warnx};
use ulstrutils::{
    SIZE_DECIMAL_2DIGITS, SIZE_SUFFIX_3LETTER, SIZE_SUFFIX_SPACE, size_to_human_string,
};

use crate::Term;

/// `EACCES`.
const EACCES: i32 = 13;

/// `_PATH_PROC_PARTITIONS`.
const PATH_PROC_PARTITIONS: &str = "/proc/partitions";

/// A C string printed with `%s`: `(null)` for none.
fn s_or_null(s: Option<&[u8]>) -> Vec<u8> {
    s.map_or_else(|| b"(null)".to_vec(), <[u8]>::to_vec)
}

/// `fdisk_info (cxt, ...)` of text built from pieces.
fn info(cxt: &mut Context, parts: &[&[u8]]) {
    cxt.info(&parts.concat());
}

/// `list_disk_identifier (cxt)`.
pub fn list_disk_identifier(cxt: &mut Context) {
    if cxt.has_label() {
        let name = cxt.label_ref().map_or("", |l| l.get_name());
        info(cxt, &[b"Disklabel type: ", name.as_bytes()]);
    }
    if !cxt.is_details() {
        if let Ok(Some(id)) = cxt.get_disklabel_id() {
            info(cxt, &[b"Disk identifier: ", &id]);
        }
    }
}

/// `list_disk_geometry (cxt)`.
pub fn list_disk_geometry(cxt: &mut Context, term: &Term) {
    let nsectors = cxt.get_nsectors();
    let bytes = nsectors.wrapping_mul(cxt.get_sector_size());
    let strsz = size_to_human_string(
        SIZE_DECIMAL_2DIGITS | SIZE_SUFFIX_SPACE | SIZE_SUFFIX_3LETTER,
        bytes,
    );

    term.color_scheme_enable(b"header", ulcolors::BOLD);
    let dev = s_or_null(cxt.get_devname());
    info(
        cxt,
        &[
            b"Disk ",
            &dev,
            format!(": {strsz}, {bytes} bytes, {nsectors} sectors").as_bytes(),
        ],
    );
    term.color_disable();

    if let Some(model) = cxt.get_devmodel().map(<[u8]>::to_vec) {
        info(cxt, &[b"Disk model: ", &model]);
    }
    if cxt.label_ref().is_some_and(|lb| lb.require_geometry()) || cxt.use_cylinders() {
        let heads = i32::from_ne_bytes(cxt.get_geom_heads().to_ne_bytes());
        let m = format!(
            "Geometry: {heads} heads, {} sectors/track, {} cylinders",
            cxt.get_geom_sectors(),
            cxt.get_geom_cylinders()
        );
        cxt.info(m.as_bytes());
    }
    list_units(cxt);
    let m = format!(
        "I/O size (minimum/optimal): {} bytes / {} bytes",
        cxt.get_minimal_iosize(),
        cxt.get_optimal_iosize()
    );
    cxt.info(m.as_bytes());
    if cxt.get_alignment_offset() != 0 {
        let m = format!("Alignment offset: {} bytes", cxt.get_alignment_offset());
        cxt.info(m.as_bytes());
    }
    list_disk_identifier(cxt);
}

/// The `Units:` and `Sector size` lines both listings print.
fn list_units(cxt: &mut Context) {
    let ups = cxt.get_units_per_sector();
    let ss = cxt.get_sector_size();
    let m = format!(
        "Units: {} of {} * {} = {} bytes",
        cxt.get_unit(libfdisk::PLURAL),
        i32::from_ne_bytes(ups.to_ne_bytes()),
        i64::from_ne_bytes(ss.to_ne_bytes()),
        i64::from_ne_bytes(u64::from(ups).wrapping_mul(ss).to_ne_bytes())
    );
    cxt.info(m.as_bytes());
    let m = format!(
        "Sector size (logical/physical): {} bytes / {} bytes",
        cxt.get_sector_size(),
        cxt.get_physector_size()
    );
    cxt.info(m.as_bytes());
}

/// A table of `columns` -- name, width hint and flags each -- made for
/// printing: its header coloured as upstream colours it (not yet: the
/// `smartcols` port draws no colour, see known-issues
/// `TD-B-SMARTCOLS-DRAWS-NO-COLOUR`).
fn new_table(columns: &[(&[u8], f64, u32)]) -> smartcols::Table {
    let mut out = smartcols::Table::new();
    for &(name, whint, flags) in columns {
        out.new_column(name, whint, flags);
    }
    out
}

/// `scols_print_table (out)`, to `term`.
fn print_table(term: &Term, out: &mut smartcols::Table) {
    let mut text = Vec::new();
    // A table this program built: nothing in it can be refused.
    let _ = out.print_into(&mut text);
    term.write(&text);
}

/// The text of field `id` of `pa`, or none.
fn field_data(cxt: &mut Context, pa: &mut Partition, id: i32) -> Option<Vec<u8>> {
    cxt.partition_to_string(pa, id).ok().flatten()
}

/// `list_disklabel (cxt)`: the label's own lines, the partition table, and
/// what is wrong with it.
pub fn list_disklabel(cxt: &mut Context, term: &Term, fields: &Fields) {
    // The label's own lines, through the info dialog.
    cxt.list_disklabel();

    let mut tb = Table::new();
    if cxt.get_partitions(&mut tb) != 0 || tb.get_nents() == 0 {
        return;
    }
    let Some(ids) = init_fields(cxt, term, fields) else {
        return;
    };
    let Some(lb) = cxt.label_ref() else { return };

    let mut cols = Vec::new();
    for &id in &ids {
        let Some(field) = lb.get_field(id) else {
            continue;
        };
        let mut fl = 0;
        if field.is_number() {
            fl |= FL_RIGHT;
        }
        if field.get_id() == fieldtype::TYPE {
            fl |= FL_TRUNC;
        }
        cols.push((field.get_name().as_bytes(), field.get_width(), fl));
    }
    let mut out = new_table(&cols);

    for pa in tb.iter_mut() {
        let Ok(ln) = out.new_line(None) else {
            warn_failed(cxt, b"failed to allocate output line");
            return;
        };
        for (i, &id) in ids.iter().enumerate() {
            let Some(data) = field_data(cxt, pa, id) else {
                continue;
            };
            if out.line_refer_data(ln, i, &data).is_err() {
                warn_failed(cxt, b"failed to add output data");
                return;
            }
        }
    }

    if out.nlines() > 0 {
        // Just a line break.
        cxt.info(b"");
        print_table(term, &mut out);
    }

    // What is wrong with it.
    let mut post = false;
    for pa in tb.iter() {
        if !pa.has_start() {
            continue;
        }
        if !cxt.lba_is_phy_aligned(pa.start) {
            if !post {
                cxt.info(b"");
            }
            let m = format!(
                "Partition {} does not start on physical sector boundary.",
                pa.partno.wrapping_add(1)
            );
            cxt.warnx(m.as_bytes());
            post = true;
        }
        if cxt.partition_has_wipe(pa) {
            if !post {
                cxt.info(b"");
            }
            let m = format!(
                "Filesystem/RAID signature on partition {} will be wiped.",
                pa.partno.wrapping_add(1)
            );
            cxt.info(m.as_bytes());
            post = true;
        }
    }
    if tb.wrong_order() {
        if !post {
            cxt.info(b"");
        }
        cxt.info(b"Partition table entries are not in disk order.");
    }
}

/// `fdisk_warn (cxt, msg)` for an allocation upstream could not make.
fn warn_failed(cxt: &mut Context, msg: &[u8]) {
    cxt.warn(libfdisk::errno::ENOMEM, msg);
}

/// `list_freespace (cxt)`.
pub fn list_freespace(cxt: &mut Context, term: &Term) {
    const COLS: [(&[u8], i32); 4] = [
        (b"Start", fieldtype::START),
        (b"End", fieldtype::END),
        (b"Sectors", fieldtype::SECTORS),
        (b"Size", fieldtype::SIZE),
    ];
    let mut tb = Table::new();
    if cxt.get_freespaces(&mut tb) != 0 {
        return;
    }
    let cols: Vec<(&[u8], f64, u32)> = COLS
        .iter()
        .map(|&(name, _)| (name, 5.0, FL_RIGHT))
        .collect();
    let mut out = new_table(&cols);

    let mut sumsize: u64 = 0;
    for pa in tb.iter_mut() {
        let Ok(ln) = out.new_line(None) else {
            warn_failed(cxt, b"failed to allocate output line");
            return;
        };
        for (i, &(_, id)) in COLS.iter().enumerate() {
            let Some(data) = field_data(cxt, pa, id) else {
                continue;
            };
            if out.line_refer_data(ln, i, &data).is_err() {
                warn_failed(cxt, b"failed to add output data");
                return;
            }
        }
        if pa.has_size() {
            sumsize = sumsize.wrapping_add(pa.size);
        }
    }

    let bytes = sumsize.wrapping_mul(cxt.get_sector_size());
    let strsz = size_to_human_string(
        SIZE_DECIMAL_2DIGITS | SIZE_SUFFIX_SPACE | SIZE_SUFFIX_3LETTER,
        bytes,
    );
    term.color_scheme_enable(b"header", ulcolors::BOLD);
    let dev = s_or_null(cxt.get_devname());
    info(
        cxt,
        &[
            b"Unpartitioned space ",
            &dev,
            format!(": {strsz}, {bytes} bytes, {sumsize} sectors").as_bytes(),
        ],
    );
    term.color_disable();
    list_units(cxt);

    if out.nlines() > 0 {
        cxt.info(b"");
        print_table(term, &mut out);
    }
}

/// `next_proc_partition (&f)`: the whole disks `/proc/partitions` lists, by
/// their canonical device paths, CD-ROMs aside. Upstream reads it 128 bytes
/// of a line at a time; its names are shorter than that.
#[must_use]
pub fn proc_partitions(short: &[u8]) -> Vec<Vec<u8>> {
    let f = match std::fs::File::open(PATH_PROC_PARTITIONS) {
        Ok(f) => f,
        Err(e) => {
            warn(short, &format!("cannot open {PATH_PROC_PARTITIONS}"), &e);
            return Vec::new();
        }
    };
    let mut devs = Vec::new();
    for line in std::io::BufReader::new(f).split(b'\n') {
        let Ok(line) = line else { break };
        // " %*d %*d %*d %128[^\n ]"
        let mut words = line
            .split(|&c| c == b' ' || c == b'\t')
            .filter(|w| !w.is_empty());
        let numbers = words.by_ref().take(3).filter(|w| is_int(w)).count();
        let Some(name) = words.next().filter(|_| numbers == 3) else {
            continue;
        };
        let name = name.get(..name.len().min(128)).unwrap_or(name);
        let devno = ulsysfs::devname_to_devno(name);
        if devno == 0 {
            continue;
        }
        if ulsysfs::devno_is_dm_private(devno).0 || ulsysfs::devno_is_wholedisk(devno) != Some(true)
        {
            continue;
        }
        let Some(path) = ulsysfs::devno_to_devpath(devno) else {
            continue;
        };
        let Some(cn) = ulsysfs::canonicalize::canonicalize_path(&path) else {
            continue;
        };
        if !is_ide_cdrom_or_tape(&cn) {
            devs.push(cn);
        }
    }
    devs
}

/// Whether `w` is what `%d` reads whole: an optional sign and digits.
fn is_int(w: &[u8]) -> bool {
    let digits = w
        .strip_prefix(b"-")
        .or_else(|| w.strip_prefix(b"+"))
        .unwrap_or(w);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// `is_ide_cdrom_or_tape (device)`: whether the device answers as a CD-ROM.
fn is_ide_cdrom_or_tape(device: &[u8]) -> bool {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        /// `O_NONBLOCK`.
        const O_NONBLOCK: i32 = 0o4000;
        opts.custom_flags(O_NONBLOCK);
    }
    let Ok(f) = opts.open(ulsysfs::path_of(device)) else {
        return false;
    };
    ulblkid::blkdev::ioctl_val(&f, ulblkid::blkdev::CDROM_GET_CAPABILITY, 0).is_ok_and(|r| r != 0)
}

/// `print_device_pt (cxt, device, warnme, verify, separator)`: a device's
/// geometry and table; -1 when it cannot be opened.
pub fn print_device_pt(
    cxt: &mut Context,
    term: &Term,
    fields: &Fields,
    device: &[u8],
    warnme: bool,
    verify: bool,
    separator: bool,
) -> i32 {
    let rc = cxt.assign_device(device, true);
    if rc != 0 {
        if warnme || rc == -EACCES {
            let e = std::io::Error::from_raw_os_error(rc.wrapping_neg());
            warn(
                &term.short,
                &format!("cannot open {}", escape_unprintable(device)),
                &e,
            );
        }
        return -1;
    }
    if separator {
        term.write(b"\n\n");
    }
    list_disk_geometry(cxt, term);
    if cxt.has_label() {
        list_disklabel(cxt, term, fields);
        if verify {
            cxt.verify_disklabel();
        }
    }
    // Opened read-only: nothing to sync.
    let _ = cxt.deassign_device(true);
    0
}

/// `print_device_freespace (cxt, device, warnme, separator)`.
pub fn print_device_freespace(
    cxt: &mut Context,
    term: &Term,
    device: &[u8],
    warnme: bool,
    separator: bool,
) -> i32 {
    let rc = cxt.assign_device(device, true);
    if rc != 0 {
        if warnme || rc == -EACCES {
            let e = std::io::Error::from_raw_os_error(rc.wrapping_neg());
            warn(
                &term.short,
                &format!("cannot open {}", escape_unprintable(device)),
                &e,
            );
        }
        return -1;
    }
    if separator {
        term.write(b"\n\n");
    }
    list_freespace(cxt, term);
    // Opened read-only: nothing to sync.
    let _ = cxt.deassign_device(true);
    0
}

/// `print_all_devices_pt (cxt, verify)`.
pub fn print_all_devices_pt(cxt: &mut Context, term: &Term, fields: &Fields, verify: bool) {
    for (i, dev) in proc_partitions(&term.short).iter().enumerate() {
        print_device_pt(cxt, term, fields, dev, false, verify, i > 0);
    }
}

/// `print_all_devices_freespace (cxt)`.
pub fn print_all_devices_freespace(cxt: &mut Context, term: &Term) {
    for (i, dev) in proc_partitions(&term.short).iter().enumerate() {
        print_device_freespace(cxt, term, dev, false, i > 0);
    }
}

/// `list_available_columns (out)`: each label's column names, wrapped to
/// the terminal, for a usage text.
pub fn list_available_columns(out: &mut Vec<u8>) {
    let cxt = Context::new();
    let termwidth = smartcols::tty::terminal_dimension()
        .0
        .filter(|&w| w > 0)
        .unwrap_or(80);
    out.extend_from_slice(b"\nAvailable output columns:\n");
    for lb in cxt.labels() {
        // The label's name and separators.
        let mut width = 6usize;
        out.extend_from_slice(format!(" {}:", lb.get_name()).as_bytes());
        for i in 1..fieldtype::NFIELDS {
            let Some(name) = lb.get_field(i).map(libfdisk::Field::get_name) else {
                continue;
            };
            let len = name.len().wrapping_add(1);
            if width.wrapping_add(len) > termwidth {
                out.extend_from_slice(b"\n     ");
                width = 6;
            }
            out.extend_from_slice(format!(" {name}").as_bytes());
            width = width.wrapping_add(len);
        }
        out.push(b'\n');
    }
}

/// `WIPEMODE_AUTO`: wipe what a new table would hide when the dialog is
/// interactive.
pub const WIPEMODE_AUTO: i32 = 0;
/// `WIPEMODE_NEVER`.
pub const WIPEMODE_NEVER: i32 = 1;
/// `WIPEMODE_ALWAYS`.
pub const WIPEMODE_ALWAYS: i32 = 2;

/// `wipemode_from_string (str)`: `auto`, `never` or `always`, in any case.
#[must_use]
pub fn wipemode_from_string(s: &[u8]) -> Option<i32> {
    [
        (b"auto".as_slice(), WIPEMODE_AUTO),
        (b"never", WIPEMODE_NEVER),
        (b"always", WIPEMODE_ALWAYS),
    ]
    .into_iter()
    .find(|(name, _)| s.eq_ignore_ascii_case(name))
    .map(|(_, mode)| mode)
}

/// `fields_string`: the `-o` list, kept for every label the program meets.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    /// The list as given, or none for the label's defaults.
    pub string: Option<Vec<u8>>,
}

/// `init_fields (cxt, NULL, &n)`: the columns to print for the current
/// label -- its defaults, extended or replaced by the `-o` list -- or none
/// without a label. A name the label has no column for ends the program,
/// as upstream's `exit (EXIT_FAILURE)` does.
///
/// Upstream keeps the list it built for the label it built it for, but
/// forgets that label on the way out, so builds it again every time; this
/// builds it every time.
#[must_use]
pub fn init_fields(cxt: &Context, term: &Term, fields: &Fields) -> Option<Vec<i32>> {
    let lb = cxt.label_ref()?;
    let mut ids = cxt.label_get_fields_ids().ok()?;
    if let Some(list) = fields.string.as_deref() {
        let lname = lb.get_name();
        let added = ulstrutils::string_add_to_idarray(
            list,
            &mut ids,
            usize::try_from(fieldtype::NFIELDS)
                .unwrap_or(0)
                .saturating_mul(2),
            |name, _| {
                let id = lb.get_field_by_name(name).map(libfdisk::Field::get_id);
                if id.is_none() {
                    warnx(
                        &term.short,
                        &format!("{lname} unknown column: {}", escape_unprintable(name)),
                    );
                }
                id
            },
        );
        if added.is_err() {
            term.exit(1);
        }
    }
    Some(ids)
}
