//! findmnt's own logic, against upstream's rules; `scripts/findmnt-diff.sh`
//! measures the whole program against util-linux 2.39.3.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests index what they built"
)]

use super::*;
use ulmount::fs::MNT_FS_KERNEL;
use ulmount::tab::Fmt;

fn fs(id: i32, parent: i32, src: &str, target: &str, fstype: &str) -> Fs {
    let mut f = Fs {
        id,
        parent,
        target: Some(target.as_bytes().to_vec()),
        root: Some(b"/".to_vec()),
        vfs_optstr: Some(b"rw".to_vec()),
        fs_optstr: Some(b"rw".to_vec()),
        optstr: Some(b"rw".to_vec()),
        flags: MNT_FS_KERNEL,
        ..Fs::default()
    };
    f.set_source(Some(src.as_bytes().to_vec()));
    f.set_fstype(Some(fstype.as_bytes().to_vec()));
    f
}

fn kernel_table() -> MntTable {
    MntTable {
        fmt: Fmt::Mountinfo,
        ents: vec![
            fs(21, 1, "/dev/sda1", "/", "ext4"),
            fs(22, 21, "proc", "/proc", "proc"),
            fs(23, 21, "tmpfs", "/run", "tmpfs"),
            fs(24, 23, "tmpfs", "/run/user", "tmpfs"),
            fs(25, 21, "/dev/sdb1", "/home", "ext4"),
        ],
    }
}

fn nocache() -> Findmnt {
    let mut f = Findmnt::new(b"findmnt".to_vec());
    f.flags |= FL_NOCACHE;
    f
}

#[test]
fn a_devno_source_is_scanned_as_percent_d() {
    assert_eq!(scan_majmin_signed(b"8:1"), Some((8, 1)));
    assert_eq!(scan_majmin_signed(b" 8: 1"), Some((8, 1)));
    assert_eq!(scan_majmin_signed(b"-1:0"), Some((-1, 0)));
    assert_eq!(scan_majmin_signed(b"4294967297:1"), Some((1, 1)));
    assert_eq!(scan_majmin_signed(b"/dev/sda"), None);
    assert_eq!(scan_majmin_signed(b"8 :1"), None);
}

#[test]
fn a_source_match_is_a_devno_or_a_source() {
    let mut f = nocache();
    f.set_source_match(b"8:1");
    assert_eq!(f.matches.majmin_devno, Some(makedev(8, 1)));
    assert!(f.flags & FL_NOSWAPMATCH != 0);
    assert_eq!(f.matches.source, None);
    let mut f = nocache();
    f.set_source_match(b"/dev/sda1");
    assert_eq!(f.matches.source.as_deref(), Some(&b"/dev/sda1"[..]));
    assert_eq!(f.flags & FL_NOSWAPMATCH, 0);
}

#[test]
fn filters_and_their_inversion() {
    let tb = kernel_table();
    let mut f = nocache();
    f.matches.fstype = Some(b"tmpfs".to_vec());
    let hits: Vec<usize> = (0..tb.nents()).filter(|&i| f.match_func(&tb, i)).collect();
    assert_eq!(hits, vec![2, 3]);
    f.flags |= FL_INVERT;
    let hits: Vec<usize> = (0..tb.nents()).filter(|&i| f.match_func(&tb, i)).collect();
    assert_eq!(hits, vec![0, 1, 4]);
}

#[test]
fn df_keeps_tmpfs_and_drops_other_pseudo_filesystems() {
    let tb = kernel_table();
    let mut f = nocache();
    f.flags |= FL_DF;
    let hits: Vec<usize> = (0..tb.nents()).filter(|&i| f.match_func(&tb, i)).collect();
    assert_eq!(hits, vec![0, 2, 3, 4]);
}

#[test]
fn a_lone_operand_is_tried_as_a_source_then_as_a_target() {
    let tb = kernel_table();
    let mut f = nocache();
    f.set_source_match(b"/home");
    let mut itr = Iter::new(Direction::Forward);
    assert_eq!(f.get_next_fs(&tb, &mut itr), Some(4));
    assert_eq!(f.matches.target.as_deref(), Some(&b"/home"[..]));
    assert_eq!(f.matches.source, None);
}

#[test]
fn the_tree_follows_parent_ids_and_the_list_follows_the_file() {
    let tb = kernel_table();
    let mut f = nocache();
    f.flags |= FL_TREE;
    f.columns = vec![Col::Target];
    let mut table = Table::new();
    table.set_termforce(smartcols::TermForce::Never);
    table.enable_ascii(true);
    let id = table.new_column(b"TARGET", 0.3, SCOLS_FL_TREE);
    let mut lines = Vec::new();
    f.create_treenode(&mut table, &[id], &tb, None, None, &mut lines)
        .unwrap();
    assert_eq!(lines, vec![0, 1, 2, 3, 4]);
    let text = String::from_utf8(table.print().unwrap()).unwrap();
    assert_eq!(text, "TARGET\n/\n|-/proc\n|-/run\n| `-/run/user\n`-/home\n");
}

#[test]
fn data_is_formatted_as_upstream_formats_it() {
    let mut f = nocache();
    let mut e = fs(30, 21, "/dev/sda2", "/mnt", "ext4");
    e.devno = makedev(8, 2);
    e.root = Some(b"/sub".to_vec());
    e.opt_fields = Some(b"shared:1 master:2".to_vec());
    assert_eq!(f.get_data(&e, Col::Majmin).unwrap(), b"  8:2  ");
    assert_eq!(f.get_data(&e, Col::Source).unwrap(), b"/dev/sda2[/sub]");
    assert_eq!(f.get_data(&e, Col::Propagation).unwrap(), b"shared,slave");
    assert_eq!(f.get_data(&e, Col::Id).unwrap(), b"30");
    assert_eq!(f.get_data(&e, Col::Freq), None);
    f.flags |= FL_RAW | FL_NOFSROOT;
    assert_eq!(f.get_data(&e, Col::Majmin).unwrap(), b"8:2");
    assert_eq!(f.get_data(&e, Col::Source).unwrap(), b"/dev/sda2");
    // An fstab entry has FREQ and PASSNO, and no propagation.
    let mut t = fs(0, 0, "UUID=x", "/", "ext4");
    t.flags = 0;
    t.passno = 1;
    assert_eq!(f.get_data(&t, Col::Passno).unwrap(), b"1");
    assert_eq!(f.get_data(&t, Col::Propagation), None);
    assert_eq!(f.get_data(&t, Col::Uuid).unwrap(), b"x");
}

#[test]
fn every_column_is_in_help_in_enum_order() {
    let text = String::from_utf8(usage(b"findmnt")).unwrap();
    let listed: Vec<&str> = text
        .split("Available output columns:\n")
        .nth(1)
        .unwrap()
        .lines()
        .take_while(|l| !l.is_empty())
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    let names: Vec<&str> = INFOS.iter().map(|i| i.name).collect();
    assert_eq!(listed, names);
    for (n, i) in INFOS.iter().enumerate() {
        assert_eq!(i.id as usize, n);
    }
}

#[test]
fn column_names_are_matched_whole_and_in_any_case() {
    assert_eq!(
        column_name_to_id(b"target", b"target", b"findmnt"),
        Some(Col::Target)
    );
    assert_eq!(
        column_name_to_id(b"USE%", b"USE%", b"findmnt"),
        Some(Col::Useperc)
    );
    assert_eq!(column_name_to_id(b"TARG", b"TARG", b"findmnt"), None);
    assert_eq!(
        poll_action_name_to_id(b"MoUnT", b"MoUnT", b"findmnt"),
        Some(Change::Mount)
    );
    assert_eq!(poll_action_name_to_id(b"mou", b"mou", b"findmnt"), None);
}

#[test]
fn the_exclusive_option_rows_keep_upstreams_early_exit() {
    let run = |opts: &[u8]| {
        let mut st = [0i32; 9];
        opts.iter().find_map(|&c| {
            ulstrutils::err_exclusive_options(
                i32::from(c),
                &EXCL,
                &mut st,
                option_to_longopt,
                b"findmnt",
            )
        })
    };
    // `-m` is checked only against the rows before `{p,x}`, so `-m -p` passes.
    assert_eq!(run(b"mp"), None);
    assert_eq!(run(b"pm"), None);
    assert_eq!(
        run(b"ps").unwrap(),
        "findmnt: mutually exclusive arguments: --mtab --poll --fstab\n"
    );
    assert_eq!(
        run(b"Cc").unwrap(),
        "findmnt: mutually exclusive arguments: --nocanonicalize --canonicalize\n"
    );
}

#[test]
fn the_proc_filesystems_parse_is_upstreams() {
    let chunks = verify_test_hooks::fgets(b"nodev\tsysfs\n\text4\n", 80);
    assert_eq!(chunks, vec![&b"nodev\tsysfs\n"[..], b"\text4\n"]);
    let long = vec![b'a'; 200];
    assert_eq!(verify_test_hooks::fgets(&long, 80).len(), 3);
}

/// What the tests reach of `verify`.
mod verify_test_hooks {
    pub(super) fn fgets(text: &[u8], size: usize) -> Vec<&[u8]> {
        super::verify::fgets_lines_for_tests(text, size)
    }
}
