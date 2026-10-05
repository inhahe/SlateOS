//! diffutils' `dir.c`: comparing two directories, name by name.

use crate::{Diff, Opts};

/// `dir_read`: the names in a directory, `.` and `..` and excluded names
/// left out. `None` for a nonexistent directory reads as empty.
pub fn dir_read(name: Option<&[u8]>, o: &Opts) -> std::io::Result<Vec<Vec<u8>>> {
    let Some(name) = name else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for entry in std::fs::read_dir(coreutils::quote::os_from_bytes(name))? {
        let entry = entry?;
        let n = coreutils::quote::os_bytes(&entry.file_name()).into_owned();
        if n == b"." || n == b".." {
            continue;
        }
        if o.excluded.excludes(&n) {
            continue;
        }
        names.push(n);
    }
    Ok(names)
}

/// `compare_names`: `strcoll`, or `strcasecoll` under
/// `--ignore-file-name-case`, falling back to `strcmp` on a tie unless case
/// is being ignored. In the C and C.UTF-8 locales collation is byte order.
pub fn compare_names(a: &[u8], b: &[u8], o: &Opts) -> std::cmp::Ordering {
    let collated = if o.ignore_file_name_case {
        casecoll(a, b)
    } else {
        a.cmp(b)
    };
    if collated != std::cmp::Ordering::Equal || o.ignore_file_name_case {
        return collated;
    }
    a.cmp(b)
}

/// `compare_names_for_qsort`: collation, ties broken by bytes.
fn compare_for_sort(a: &[u8], b: &[u8], o: &Opts) -> std::cmp::Ordering {
    let collated = if o.ignore_file_name_case {
        casecoll(a, b)
    } else {
        a.cmp(b)
    };
    collated.then_with(|| a.cmp(b))
}

/// gnulib's `strcasecoll` in a locale whose collation is byte order: the two
/// names compared with ASCII letters folded to lower case.
fn casecoll(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
}

/// `diff_dirs`: every name in either directory, in order, handed to
/// `compare_files`. Returns the worst status.
// Every index into `stat`, `names`, `nonexistent` and `lists` is 0 or 1.
#[allow(clippy::indexing_slicing)]
pub fn diff_dirs(d: &mut Diff, cmp: &crate::Comparison) -> i32 {
    let o = d.opts.clone();
    // `dir_loop`: a directory that is one of its own ancestors.
    let looping = |i: usize| {
        let mut p = cmp.parent;
        while let Some(par) = p {
            if crate::io::same_file(&par.stat[i], &cmp.stat[i]) {
                return true;
            }
            p = par.parent;
        }
        false
    };
    if (cmp.nonexistent[0] || looping(0)) && (cmp.nonexistent[1] || looping(1)) {
        let which = usize::from(cmp.nonexistent[0]);
        d.error(&[cmp.names[which].as_slice(), b": recursive directory loop"].concat());
        return crate::EXIT_TROUBLE;
    }

    let mut lists: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
    let mut val = crate::EXIT_SUCCESS;
    for i in 0..2 {
        let name = (!cmp.nonexistent[i]).then_some(cmp.names[i].as_slice());
        match dir_read(name, &o) {
            Ok(v) => lists[i] = v,
            Err(e) => {
                d.perror_with_name(&cmp.names[i], &e);
                val = crate::EXIT_TROUBLE;
            }
        }
    }
    if val != crate::EXIT_SUCCESS {
        return val;
    }
    for l in &mut lists {
        l.sort_by(|a, b| compare_for_sort(a, b, &o));
    }

    let [mut n0, mut n1] = lists;
    let mut i0 = 0usize;
    let mut i1 = 0usize;
    // `-S`: at the top level, skip the names before the starting file.
    if cmp.parent.is_none()
        && let Some(start) = o.starting_file.as_deref()
    {
        while n0
            .get(i0)
            .is_some_and(|n| compare_names(n, start, &o).is_lt())
        {
            i0 = i0.saturating_add(1);
        }
        while n1
            .get(i1)
            .is_some_and(|n| compare_names(n, start, &o).is_lt())
        {
            i1 = i1.saturating_add(1);
        }
    }

    while i0 < n0.len() || i1 < n1.len() {
        let nameorder = match (n0.get(i0), n1.get(i1)) {
            (None, _) => std::cmp::Ordering::Greater,
            (_, None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => compare_names(a, b, &o),
        };

        // Under --ignore-file-name-case, prefer an exact match if the run of
        // names that compare equal holds one.
        if nameorder.is_eq() && o.ignore_file_name_case {
            if let (Some(a), Some(b)) = (n0.get(i0).cloned(), n1.get(i1).cloned()) {
                let raw = a.cmp(&b);
                if raw.is_ne() {
                    let greater_side = usize::from(raw.is_lt());
                    let (lesser, li, greater_name) = if greater_side == 1 {
                        (&mut n0, i0, b)
                    } else {
                        (&mut n1, i1, a)
                    };
                    let mut p = li.saturating_add(1);
                    while let Some(name) = lesser.get(p) {
                        if !compare_names(name, &greater_name, &o).is_eq() {
                            break;
                        }
                        let c = name.as_slice().cmp(greater_name.as_slice());
                        if c.is_ge() {
                            if c.is_eq() {
                                // Move the exact match to the front of the run.
                                let m = lesser.remove(p);
                                lesser.insert(li, m);
                            }
                            break;
                        }
                        p = p.saturating_add(1);
                    }
                }
            }
        }

        let a = if nameorder.is_gt() {
            None
        } else {
            let n = n0.get(i0).cloned();
            i0 = i0.saturating_add(1);
            n
        };
        let b = if nameorder.is_lt() {
            None
        } else {
            let n = n1.get(i1).cloned();
            i1 = i1.saturating_add(1);
            n
        };
        let v = crate::compare_files(d, Some(cmp), a.as_deref(), b.as_deref());
        val = val.max(v);
    }
    val
}

/// `find_dir_file_pathname`: `DIR/FILE`, with FILE's name as the directory
/// spells it under `--ignore-file-name-case`.
pub fn find_dir_file_pathname(dir: &[u8], file: &[u8], o: &Opts) -> Vec<u8> {
    let mut matched = file.to_vec();
    if o.ignore_file_name_case
        && let Ok(names) = dir_read(Some(dir), o)
    {
        let mut found_inexact = false;
        for n in names {
            if compare_names(&n, file, o).is_eq() {
                if n == file {
                    matched = n;
                    break;
                }
                if !found_inexact {
                    matched = n;
                    found_inexact = true;
                }
            }
        }
    }
    coreutils::pathname::file_name_concat(dir, &matched)
}
