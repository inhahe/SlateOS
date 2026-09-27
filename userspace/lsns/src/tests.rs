//! lsns's own logic, against upstream's rules; `scripts/lsns-diff.sh`
//! measures the whole program against util-linux 2.39.3 inside a user, PID
//! and mount namespace of its own.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests index what they built"
)]

use super::*;

/// A process in the namespaces `ids` (`LSNS_ID_*` order), with no parent or
/// owner namespaces known.
fn process(pid: i32, ppid: i32, ids: [u64; 8]) -> Process {
    Process {
        pid,
        ppid,
        ns_ids: ids,
        netnsid: NETNS_UNUSABLE,
        ..Process::default()
    }
}

/// An `Lsns` over `procs`, every type listed, the columns `cols`.
fn lsns_over(procs: Vec<Process>, cols: &[Col], tree: Tree) -> Lsns {
    Lsns {
        short: b"lsns".to_vec(),
        fltr_types: [true; 8],
        processes: procs,
        columns: cols.to_vec(),
        tree,
        ..Lsns::default()
    }
}

/// Whether `line` hangs, at any depth, under `ancestor`.
fn under(tab: &Table, line: LineId, ancestor: LineId) -> bool {
    line != ancestor && tab.line_is_ancestor(ancestor, line)
}

/// The namespace listed with `id`.
fn ns_by_id(ls: &Lsns, id: u64) -> &Namespace {
    ls.namespaces.iter().find(|n| n.id == id).unwrap()
}

#[test]
fn proc_stat_is_read_as_upstreams_two_sscanfs() {
    assert_eq!(parse_proc_stat(b"42 (bash) S 7 42 42 0 -1\n"), Ok((42, 7)));
    // The last `)` is the one after the name, whatever the name holds.
    assert_eq!(parse_proc_stat(b"5 (a) b) (c) R 1 5\n"), Ok((5, 1)));
    // `%d (` counts only the number: what follows it is not checked.
    assert_eq!(parse_proc_stat(b"12x) S 3"), Ok((12, 3)));
    // A name holding a newline ends the line before its `)`.
    assert_eq!(parse_proc_stat(b"9 (a\nb) S 1 9\n"), Err(EINVAL));
    assert_eq!(parse_proc_stat(b"(x) S 1"), Err(EINVAL));
    assert_eq!(parse_proc_stat(b"3 (x)"), Err(EINVAL));
    assert_eq!(parse_proc_stat(b"3 (x) S"), Err(EINVAL));
    assert_eq!(parse_proc_stat(b"3 (x)S-4"), Ok((3, -4)));
    // `%d` is read as a `long` and cut to an `int`.
    assert_eq!(parse_proc_stat(b"4294967297 (x) S 1"), Ok((1, 1)));
    assert_eq!(parse_proc_stat(b""), Err(EINVAL));
}

#[test]
fn an_nsfs_root_is_read_by_strtoumax() {
    assert_eq!(nsfs_root_ino(b"net:[4026532008]"), Some(4_026_532_008));
    assert_eq!(nsfs_root_ino(b"net:[]"), Some(0));
    assert_eq!(nsfs_root_ino(b"net:[ 7]"), Some(7));
    assert_eq!(nsfs_root_ino(b"net:[-1]"), Some(u64::MAX));
    assert_eq!(nsfs_root_ino(b"net:[18446744073709551616]"), None);
    assert_eq!(nsfs_root_ino(b"net:[12"), None);
    assert_eq!(nsfs_root_ino(b"net:[12]x"), Some(12));
    assert_eq!(nsfs_root_ino(b"/"), None);
    assert_eq!(nsfs_root_ino(b"a[b[3]"), None);
}

#[test]
fn a_path_is_included_only_where_strstr_first_finds_it() {
    assert!(is_path_included(b"/run/netns/a", b"/run/netns/a", b','));
    assert!(is_path_included(b"/a,/b", b"/a", b','));
    assert!(is_path_included(b"/a,/b,/c", b"/b", b','));
    assert!(is_path_included(b"/a,/b", b"/b", b','));
    assert!(!is_path_included(b"/a,/b", b"/c", b','));
    // A prefix of a listed path is not that path.
    assert!(!is_path_included(b"/ab,/c", b"/a", b','));
    // The first occurrence is inside a longer path, so the listed one is
    // missed, as upstream misses it.
    assert!(!is_path_included(b"/x/a,/a", b"/a", b','));
    assert!(is_path_included(b"/a\n/b", b"/b", b'\n'));
    assert!(!is_path_included(b"/a\n/b", b"/b", b','));
}

#[test]
fn a_users_name_is_kept_unless_it_prints_as_nothing() {
    assert!(name_is_kept(b"root", true));
    assert!(name_is_kept(b"root", false));
    assert!(!name_is_kept(b"", true));
    assert!(!name_is_kept(b"a\x01b", true));
    assert!(!name_is_kept(b"a\x01b", false));
    assert!(name_is_kept("jos\u{e9}".as_bytes(), true));
    // A byte the locale has no character for is measured by `strlen`.
    assert!(name_is_kept("jos\u{e9}".as_bytes(), false));
    assert!(name_is_kept(b"\xff", true));
    // Only a combining mark: printable, but no width.
    assert!(!name_is_kept("\u{301}".as_bytes(), true));
}

#[test]
fn netnsid_text_is_a_number_unassigned_or_nothing() {
    assert_eq!(netnsid_text(0), Some(b"0".to_vec()));
    assert_eq!(netnsid_text(17), Some(b"17".to_vec()));
    assert_eq!(
        netnsid_text(NSID_NOT_ASSIGNED),
        Some(b"unassigned".to_vec())
    );
    assert_eq!(netnsid_text(NETNS_UNUSABLE), None);
}

#[test]
fn clone_types_map_to_lsns_types() {
    let pairs = [
        (0x0002_0000, "mnt"),
        (0x4000_0000, "net"),
        (0x2000_0000, "pid"),
        (0x0400_0000, "uts"),
        (0x0800_0000, "ipc"),
        (0x1000_0000, "user"),
        (0x0200_0000, "cgroup"),
        (0x0000_0080, "time"),
    ];
    for (clone, name) in pairs {
        assert_eq!(
            clone_type_to_lsns_type(clone).map(|t| NS_NAMES[t]),
            Some(name)
        );
    }
    assert_eq!(clone_type_to_lsns_type(0), None);
    assert_eq!(clone_type_to_lsns_type(0x0000_0100), None);
}

#[test]
fn column_names_match_without_case() {
    assert_eq!(column_name_to_id(b"ns", b"ns", b"lsns"), Some(Col::Ns));
    assert_eq!(
        column_name_to_id(b"NetNsId", b"NetNsId,x", b"lsns"),
        Some(Col::Netnsid)
    );
    assert_eq!(column_name_to_id(b"N", b"N", b"lsns"), None);
    assert_eq!(column_name_to_id(b"NSX", b"NSX", b"lsns"), None);
    // `INFOS` is in `Col`'s order, which `info` relies on.
    for (i, ci) in INFOS.iter().enumerate() {
        assert_eq!(ci.id as usize, i);
        assert_eq!(info(ci.id).name, ci.name);
    }
}

#[test]
fn help_is_upstreams() {
    let text = String::from_utf8(usage(b"lsns")).unwrap();
    assert!(text.starts_with(
        "\nUsage:\n lsns [options] [<namespace>]\n\nList system namespaces.\n\nOptions:\n"
    ));
    // `USAGE_HELP_OPTIONS(24)`: each option padded to 24 columns. The option
    // fields are built from single words so that scripts/check-help-vs-parser.py,
    // which reads this file apart from the parser in main.rs, does not take
    // the expectation for a second, unparsed help text.
    let help_opt = format!(" {}, {}", "-h", "--help");
    let version_opt = format!(" {}, {}", "-V", "--version");
    let help = format!("\n{help_opt:<24}display this help\n{version_opt:<24}display version\n");
    assert!(text.contains(&help));
    // `" %11s  %s\n"` for each column.
    let ns = format!(
        "\nAvailable output columns:\n {:>11}  namespace identifier (inode number)\n",
        "NS"
    );
    assert!(text.contains(&ns));
    assert!(text.contains(&format!(
        "\n {:>11}  namespace ID as used by network subsystem\n",
        "NETNSID"
    )));
    assert!(text.ends_with("\nFor more details see lsns(8).\n"));
}

#[test]
fn long_options_map_to_upstreams_values() {
    assert_eq!(LONGS.len(), LONG_VALS.len());
    assert_eq!(option_to_longopt(i32::from(b'T')), Some("tree"));
    assert_eq!(option_to_longopt(OPT_OUTPUT_ALL), Some("output-all"));
    assert_eq!(option_to_longopt(i32::from(b'Z')), None);
}

#[test]
fn namespaces_are_read_from_processes_and_sorted() {
    // pid 1 is in mnt 30 and net 20; pid 5 (its child) in mnt 30 and net
    // 10; pid 3 in mnt 40 and net 10.
    let procs = vec![
        process(1, 0, [30, 20, 0, 0, 0, 0, 0, 0]),
        process(5, 1, [30, 10, 0, 0, 0, 0, 0, 0]),
        process(3, 1, [40, 10, 0, 0, 0, 0, 0, 0]),
    ];
    let mut ls = lsns_over(procs, &[Col::Ns], Tree::None);
    ls.read_namespaces();
    let ids: Vec<u64> = ls.order.iter().map(|&i| ls.namespaces[i].id).collect();
    assert_eq!(ids, vec![10, 20, 30, 40]);
    let net10 = ns_by_id(&ls, 10);
    assert_eq!(net10.nprocs, 2);
    // The lowest PID, not the first found.
    assert_eq!(ls.processes[net10.proc_.unwrap()].pid, 3);
    assert_eq!(net10.ty, ID_NET);
    assert_eq!(ns_by_id(&ls, 30).nprocs, 2);
    // Parents are linked whichever of the two was read first.
    assert_eq!(ls.processes[1].parent, Some(0));
    assert_eq!(ls.processes[2].parent, Some(0));
    assert_eq!(ls.processes[0].parent, None);
}

#[test]
fn the_process_tree_hangs_a_namespace_from_where_its_parent_was_last_shown() {
    let procs = vec![
        process(1, 0, [100, 200, 0, 0, 0, 0, 0, 0]),
        process(5, 1, [100, 300, 0, 0, 0, 0, 0, 0]),
        process(7, 5, [101, 300, 0, 0, 0, 0, 0, 0]),
    ];
    let mut ls = lsns_over(procs, &[Col::Ns, Col::Pid, Col::Command], Tree::Process);
    ls.read_namespaces();
    let tab = ls.namespaces_table();
    let lines: Vec<LineId> = tab.line_ids().collect();
    // mnt 100 (pid 1), mnt 101 (pid 7, whose parent was not shown yet),
    // net 200 (pid 1 again), net 300 (pid 5, under pid 1's latest line).
    assert_eq!(lines.len(), 4);
    let ns_col = tab.column(0).unwrap();
    let text = |ln: LineId| tab.line_column_data(ln, ns_col).unwrap().to_vec();
    assert_eq!(text(lines[0]), b"100");
    assert_eq!(text(lines[1]), b"101");
    assert_eq!(text(lines[2]), b"200");
    assert_eq!(text(lines[3]), b"300");
    assert!(under(&tab, lines[3], lines[2]));
    assert!(!under(&tab, lines[3], lines[0]));
    assert!(!under(&tab, lines[1], lines[0]));
    assert_eq!(
        tab.column_flags(tab.column(2).unwrap()).unwrap() & FL_TREE,
        FL_TREE
    );
}

#[test]
fn the_owner_tree_shows_an_owner_before_what_it_owns() {
    // user 50 owns user 60 (its child), mnt 100 and net 200; pid 2 is in
    // user 60.
    let mut p1 = process(1, 0, [100, 200, 0, 0, 0, 50, 0, 0]);
    p1.ns_oids = [50, 50, 0, 0, 0, 0, 0, 0];
    let mut p2 = process(2, 1, [100, 200, 0, 0, 0, 60, 0, 0]);
    p2.ns_oids = [50, 50, 0, 0, 0, 50, 0, 0];
    p2.ns_pids = [0, 0, 0, 0, 0, 50, 0, 0];
    let mut ls = lsns_over(vec![p1, p2], &[Col::Ns, Col::Type], Tree::Owner);
    ls.read_namespaces();
    assert_eq!(
        ns_by_id(&ls, 60)
            .related_ns
            .owner
            .map(|i| ls.namespaces[i].id),
        Some(50)
    );
    assert_eq!(
        ns_by_id(&ls, 60)
            .related_ns
            .parent
            .map(|i| ls.namespaces[i].id),
        Some(50)
    );
    assert_eq!(ns_by_id(&ls, 50).related_ns, Rela::default());
    let tab = ls.namespaces_table();
    let lines: Vec<LineId> = tab.line_ids().collect();
    assert_eq!(lines.len(), 4);
    for &child in &lines[1..] {
        assert!(under(&tab, child, lines[0]));
    }
    // The NS column is the tree, and not right-aligned.
    let flags = tab.column_flags(tab.column(0).unwrap()).unwrap();
    assert_eq!(flags & (FL_TREE | FL_RIGHT), FL_TREE);
}

#[test]
fn an_orphan_without_a_process_keeps_no_relation() {
    // A namespace whose owner is not listed and which no process is in --
    // a persistent one of a filtered-out owner -- is where upstream
    // dereferences a missing process.
    let mut ls = lsns_over(Vec::new(), &[Col::Ns], Tree::Owner);
    let ns = ls.add_namespace(ID_NET, 200, 0, 50);
    ls.read_related_namespaces();
    assert_eq!(ls.namespaces[ns].related_ns, Rela::default());
    let tab = ls.namespaces_table();
    assert_eq!(tab.nlines(), 1);
}

#[test]
fn filters_by_task_and_persistence() {
    let procs = vec![
        process(1, 0, [100, 0, 0, 0, 0, 0, 0, 0]),
        process(2, 1, [101, 0, 0, 0, 0, 0, 0, 0]),
    ];
    let mut ls = lsns_over(procs.clone(), &[Col::Ns], Tree::None);
    ls.read_namespaces();
    ls.fltr_pid = 2;
    assert_eq!(ls.namespaces_table().nlines(), 1);

    let mut ls = lsns_over(procs, &[Col::Ns], Tree::None);
    ls.read_namespaces();
    ls.add_namespace(ID_NET, 7, 0, 0);
    ls.persist = true;
    let tab = ls.namespaces_table();
    assert_eq!(tab.nlines(), 1);
    let ln = tab.line_ids().next().unwrap();
    assert_eq!(
        tab.line_column_data(ln, tab.column(0).unwrap()),
        Some(&b"7"[..])
    );
}

#[test]
fn a_namespaces_processes_nest_only_within_it() {
    // pid 1 and 4 share mnt 100; 9 (child of 4) is in mnt 101; 12 (child
    // of 9) is back in mnt 100.
    let procs = vec![
        process(1, 0, [100, 0, 0, 0, 0, 0, 0, 0]),
        process(4, 1, [100, 0, 0, 0, 0, 0, 0, 0]),
        process(9, 4, [101, 0, 0, 0, 0, 0, 0, 0]),
        process(12, 9, [100, 0, 0, 0, 0, 0, 0, 0]),
    ];
    let mut ls = lsns_over(procs, &[Col::Pid, Col::Ppid, Col::Command], Tree::Process);
    ls.read_namespaces();
    let mnt = ls.get_namespace(100).unwrap();
    let tab = ls.namespace_processes_table(mnt);
    let lines: Vec<LineId> = tab.line_ids().collect();
    assert_eq!(lines.len(), 3);
    let pid = tab.column(0).unwrap();
    let pids: Vec<&[u8]> = lines
        .iter()
        .map(|&l| tab.line_column_data(l, pid).unwrap())
        .collect();
    assert_eq!(pids, vec![&b"1"[..], b"4", b"12"]);
    assert!(under(&tab, lines[1], lines[0]));
    // 12's parent is not in the namespace, so it starts a tree of its own.
    assert!(!under(&tab, lines[2], lines[0]));
}

#[test]
fn nsfs_mount_points_are_joined_once_each() {
    use ulmount::fs::Fs;
    let mount = |root: &str, target: &str| {
        let mut f = Fs {
            root: Some(root.as_bytes().to_vec()),
            target: Some(target.as_bytes().to_vec()),
            ..Fs::default()
        };
        f.set_fstype(Some(b"nsfs".to_vec()));
        f
    };
    let mut ls = lsns_over(Vec::new(), &[Col::Nsfs], Tree::None);
    ls.tab.ents = vec![
        mount("net:[7]", "/run/netns/a"),
        mount("net:[8]", "/run/netns/b"),
        mount("net:[7]", "/run/netns/c"),
        mount("net:[7]", "/run/netns/a"),
    ];
    let ns = Namespace {
        id: 7,
        ty: ID_NET,
        ..Namespace::default()
    };
    assert_eq!(
        ls.nsfs_text(&ns, b'\n'),
        Some(b"/run/netns/a\n/run/netns/c".to_vec())
    );
    assert_eq!(
        ls.nsfs_text(&ns, b','),
        Some(b"/run/netns/a,/run/netns/c".to_vec())
    );
    let other = Namespace {
        id: 9,
        ty: ID_NET,
        ..Namespace::default()
    };
    assert_eq!(ls.nsfs_text(&other, b','), None);
}

#[test]
fn list_and_tree_are_never_refused_together() {
    // Upstream's `{ 'l','T' }` row is out of ASCII order, and the scan
    // stops at the first row that starts past the option: `-T` is never
    // seen, so neither order is refused.
    let check = |opts: &[u8]| {
        let mut st = [0i32; 3];
        opts.iter().any(|&c| {
            ulstrutils::err_exclusive_options(
                i32::from(c),
                &EXCL,
                &mut st,
                option_to_longopt,
                b"lsns",
            )
            .is_some()
        })
    };
    assert!(!check(b"lT"));
    assert!(!check(b"Tl"));
    assert!(check(b"Jr"));
    assert!(check(b"pP"));
}

#[test]
fn options_are_refused_as_upstream_refuses_them() {
    let run_args = |args: &[&str]| {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        let mut out = Stdout::new(1);
        let rc = run(&argv, b"lsns", &mut out);
        out.discard();
        rc
    };
    assert_eq!(run_args(&["lsns", "-t", "bogus"]), 1);
    assert_eq!(run_args(&["lsns", "-J", "-r"]), 1);
    assert_eq!(run_args(&["lsns", "--tree=sideways"]), 1);
    assert_eq!(run_args(&["lsns", "-p", "x"]), 1);
    assert_eq!(run_args(&["lsns", "-p", "1", "4026531840"]), 1);
    assert_eq!(run_args(&["lsns", "-o", ""]), 1);
    assert_eq!(run_args(&["lsns", "-o", "NS,BOGUS"]), 1);
    assert_eq!(run_args(&["lsns", "notanumber"]), 1);
    assert_eq!(run_args(&["lsns", "--bogus"]), 1);
}
