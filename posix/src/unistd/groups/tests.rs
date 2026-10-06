use super::*;

/// What a sink was left holding.
#[derive(Default)]
struct Got(Vec<GidT>);

impl GroupSink for Got {
    fn restart(&mut self) {
        self.0.clear();
    }
    fn gid(&mut self, gid: GidT) {
        self.0.push(gid);
    }
}

/// `text` read whole: the groups of its last `Groups:` line, or `None`
/// where that line is missing or malformed.
fn read_all(text: &[u8]) -> Option<Vec<GidT>> {
    let mut got = Got::default();
    let mut line = GroupsLine::new();
    line.feed(text, &mut got);
    line.finish(&mut got).then_some(got.0)
}

/// `text` as a source for [`getgroups_with`], read as the target reads
/// `/proc/self/status`.
fn source(text: &'static [u8]) -> impl FnOnce(&mut dyn GroupSink) -> Result<(), i32> {
    move |sink| {
        let mut line = GroupsLine::new();
        line.feed(text, sink);
        if line.finish(sink) {
            Ok(())
        } else {
            Err(errno::EIO)
        }
    }
}

/// The shape the kernel writes (`build_pid_status`), trimmed.
const STATUS: &[u8] = b"Name:\tsh\nUmask:\t0022\nState:\tR (running)\nTgid:\t7\n\
Uid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nGroups:\t30 10 20\n\
VmSize:\t1024 kB\nThreads:\t1\n";

#[test]
fn the_kernels_line_is_read_in_its_order() {
    assert_eq!(read_all(STATUS), Some(vec![30, 10, 20]));
}

/// Linux writes a space after each gid, the last too.
#[test]
fn linuxs_trailing_space_is_no_gid() {
    assert_eq!(read_all(b"Groups:\t4 24 27 \n"), Some(vec![4, 24, 27]));
}

#[test]
fn an_empty_line_is_no_groups() {
    assert_eq!(
        read_all(b"Name:\tsh\nGroups:\t\nVmSize:\t1 kB\n"),
        Some(vec![])
    );
    assert_eq!(read_all(b"Groups:\n"), Some(vec![]));
}

#[test]
fn the_extremes_of_a_gid() {
    assert_eq!(
        read_all(b"Groups:\t0 4294967295\n"),
        Some(vec![0, u32::MAX])
    );
    assert_eq!(read_all(b"Groups:\t4294967296\n"), None, "past u32::MAX");
    assert_eq!(read_all(b"Groups:\t99999999999999999999\n"), None);
}

/// The name is the first line, raw: a newline in it starts a line of the
/// name's choosing. The kernel's own `Groups:` comes later, and wins.
#[test]
fn a_line_forged_in_the_name_is_overruled_by_the_kernels() {
    let text = b"Name:\tx\nGroups:\t0 27\nUmask:\t0022\nUid:\t1000\t1000\t1000\t1000\n\
Gid:\t1000\t1000\t1000\t1000\nGroups:\t100\nThreads:\t1\n";
    assert_eq!(read_all(text), Some(vec![100]));
    // Forged after the key too, with the separator the kernel uses.
    let text = b"Name:\ta\nGroups:\t0\nUmask:\t0022\nGroups:\t\nThreads:\t1\n";
    assert_eq!(
        read_all(text),
        Some(vec![]),
        "an empty real list stays empty"
    );
}

/// A malformed forgery is not the last line, so it spoils nothing.
#[test]
fn a_malformed_line_before_the_last_is_forgotten() {
    let text = b"Name:\tx\nGroups:\tzz 9\nUmask:\t0022\nGroups:\t5\n";
    assert_eq!(read_all(text), Some(vec![5]));
}

#[test]
fn a_malformed_last_line_is_no_answer() {
    assert_eq!(read_all(b"Groups:\t5 x\n"), None);
    assert_eq!(read_all(b"Groups:\t5,6\n"), None);
    assert_eq!(read_all(b"Groups:\t-1\n"), None);
    assert_eq!(read_all(b"Groups:\t5\nGroups:\t6 ?\n"), None);
}

#[test]
fn a_text_without_the_line_is_no_answer() {
    assert_eq!(read_all(b"Name:\tsh\nUmask:\t0022\n"), None);
    assert_eq!(read_all(b""), None);
    assert_eq!(read_all(b"Group:\t5\nGroupsX:\t5\n"), None);
}

/// The key counts only at the start of a line.
#[test]
fn the_key_starts_a_line() {
    assert_eq!(read_all(b"Name:\tGroups:\t9\nGroups:\t1\n"), Some(vec![1]));
    assert_eq!(read_all(b"XGroups:\t9\nGroups:\t1\n"), Some(vec![1]));
    assert_eq!(read_all(b"\n\nGroups:\t2\n"), Some(vec![2]));
    // A key cut short by a newline starts the next line afresh.
    assert_eq!(read_all(b"Grou\nGroups:\t3\n"), Some(vec![3]));
}

/// The last line may end with the text instead of a newline.
#[test]
fn the_text_may_end_inside_the_list() {
    assert_eq!(read_all(b"Groups:\t1 2"), Some(vec![1, 2]));
    assert_eq!(read_all(b"Groups:"), Some(vec![]));
}

/// The text comes in reads of any size, cut anywhere.
#[test]
fn pieces_of_any_size_read_as_the_whole() {
    let text = b"Name:\tx\nGroups:\t0 27\nUmask:\t0022\nGroups:\t4294967295 12 7 \nThreads:\t1\n";
    let whole = read_all(text);
    assert_eq!(whole, Some(vec![u32::MAX, 12, 7]));
    for size in 1..=text.len() {
        let mut got = Got::default();
        let mut line = GroupsLine::new();
        for piece in text.chunks(size) {
            line.feed(piece, &mut got);
        }
        assert!(line.finish(&mut got), "pieces of {size}");
        assert_eq!(Some(got.0), whole, "pieces of {size}");
    }
}

/// The reader takes the file in whatever reads it gives, and a read that
/// fails part-way is no answer, whatever came before it.
#[test]
fn read_status_takes_any_reads_and_fails_with_a_failed_one() {
    const FORGED: &[u8] = b"Name:\tx\nGroups:\t0 27\nUmask:\t0022\nGroups:\t100 50\nThreads:\t1\n";
    for size in [1, 3, 7, 64, 2048] {
        let mut rest = FORGED;
        let mut got = Got::default();
        let read = read_status(
            |buf| {
                let n = rest.len().min(size).min(buf.len());
                let (now, later) = rest.split_at(n);
                buf[..n].copy_from_slice(now);
                rest = later;
                Some(n)
            },
            &mut got,
        );
        assert_eq!(read, Ok(()), "reads of {size}");
        assert_eq!(got.0, vec![100, 50], "reads of {size}");
    }
    let mut reads = 0;
    let mut got = Got::default();
    let read = read_status(
        |buf| {
            reads += 1;
            if reads > 1 {
                return None;
            }
            buf[..12].copy_from_slice(b"Groups:\t5 6\n");
            Some(12)
        },
        &mut got,
    );
    assert_eq!(read, Err(errno::EIO), "a whole line, then a failed read");
}

// -- getgroups --

/// A source that must not be asked: the answer is due before it.
#[allow(clippy::panic)] // the test's point: this must not be called
fn never(_: &mut dyn GroupSink) -> Result<(), i32> {
    panic!("the source was asked");
}

/// What `getgroups_with` returned, and the errno it left (cleared first).
fn call(
    size: i32,
    list: *mut GidT,
    read: impl FnOnce(&mut dyn GroupSink) -> Result<(), i32>,
) -> (i32, i32) {
    errno::set_errno(0);
    let r = getgroups_with(size, list, read);
    (r, errno::get_errno())
}

#[test]
fn a_negative_size_is_einval_before_anything_is_read() {
    for size in [-1, -5, i32::MIN] {
        assert_eq!(
            call(size, core::ptr::null_mut(), never),
            (-1, errno::EINVAL)
        );
    }
}

#[test]
fn a_zero_size_asks_how_many_and_touches_nothing() {
    let mut list = [77 as GidT; 4];
    assert_eq!(call(0, list.as_mut_ptr(), source(STATUS)), (3, 0));
    assert_eq!(list, [77; 4]);
    assert_eq!(call(0, core::ptr::null_mut(), source(STATUS)), (3, 0));
}

/// Linux sorts the list when it is installed; the native kernel keeps it as
/// given, so the copy is sorted.
#[test]
fn the_copy_is_in_ascending_order() {
    let mut list = [77 as GidT; 5];
    assert_eq!(call(3, list.as_mut_ptr(), source(STATUS)), (3, 0));
    assert_eq!(list[..3], [10, 20, 30]);
    assert_eq!(call(5, list.as_mut_ptr(), source(STATUS)), (3, 0));
    assert_eq!(list, [10, 20, 30, 77, 77], "past the groups, untouched");
}

#[test]
fn too_small_a_list_is_einval() {
    let mut list = [77 as GidT; 2];
    assert_eq!(
        call(2, list.as_mut_ptr(), source(STATUS)),
        (-1, errno::EINVAL)
    );
    assert_eq!(
        call(1, list.as_mut_ptr(), source(STATUS)),
        (-1, errno::EINVAL)
    );
}

/// A NULL list is EFAULT only where there is something to copy, and a size
/// too small is EINVAL first, as Linux checks it before the copy.
#[test]
fn a_null_list_is_efault_only_with_groups_to_copy() {
    let null = core::ptr::null_mut();
    assert_eq!(call(5, null, source(STATUS)), (-1, errno::EFAULT));
    assert_eq!(call(3, null, source(STATUS)), (-1, errno::EFAULT));
    assert_eq!(call(2, null, source(STATUS)), (-1, errno::EINVAL));
    assert_eq!(call(5, null, source(b"Groups:\t\n")), (0, 0));
}

#[test]
fn a_list_that_cannot_be_read_is_eio_not_none() {
    let mut list = [77 as GidT; 4];
    const NO_LINE: &[u8] = b"Name:\tsh\nUmask:\t0022\n";
    const BAD_LINE: &[u8] = b"Groups:\t1 x\n";
    for text in [NO_LINE, BAD_LINE] {
        assert_eq!(
            call(0, core::ptr::null_mut(), source(text)),
            (-1, errno::EIO)
        );
        assert_eq!(call(4, list.as_mut_ptr(), source(text)), (-1, errno::EIO));
    }
    let failing = |_: &mut dyn GroupSink| Err(errno::EIO);
    assert_eq!(call(4, list.as_mut_ptr(), failing), (-1, errno::EIO));
}

#[test]
fn success_leaves_errno_alone() {
    let mut list = [0 as GidT; 4];
    errno::set_errno(31);
    assert_eq!(getgroups_with(4, list.as_mut_ptr(), source(STATUS)), 3);
    assert_eq!(errno::get_errno(), 31);
}

#[test]
fn a_forged_line_reaches_neither_the_count_nor_the_copy() {
    const FORGED: &[u8] =
        b"Name:\tx\nGroups:\t0 27 4 5 6\nUmask:\t0022\nGroups:\t100 50\nThreads:\t1\n";
    assert_eq!(call(0, core::ptr::null_mut(), source(FORGED)), (2, 0));
    let mut list = [77 as GidT; 2];
    assert_eq!(
        call(2, list.as_mut_ptr(), source(FORGED)),
        (2, 0),
        "the forgery's 5 do not count"
    );
    assert_eq!(list, [50, 100]);
}

// -- the host --

/// Nothing can install a group on the host, so its list is empty, and the
/// public calls agree.
#[test]
fn the_hosts_list_is_empty() {
    let mut got = Got(vec![9]);
    assert_eq!(supplementary_groups(&mut got), Ok(()));
    assert_eq!(
        got.0,
        vec![],
        "the stand-in's line restarts the list, empty"
    );
    assert!(!in_supplementary_groups(0));
    assert!(!in_supplementary_groups(super::super::getegid()));
    assert_eq!(super::super::getgroups(0, core::ptr::null_mut()), 0);
}
