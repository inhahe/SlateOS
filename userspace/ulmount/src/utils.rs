//! libmount's `utils.c` and the pieces of `lib/` it leans on: which
//! filesystem types are pseudo or network filesystems, `-t` type lists,
//! path comparison, and `NAME=value` tags.

/// `pseudofs[]`: the types with no real source device. Sorted, as upstream
/// keeps it for `bsearch`.
const PSEUDOFS: &[&[u8]] = &[
    b"anon_inodefs",
    b"apparmorfs",
    b"autofs",
    b"bdev",
    b"binder",
    b"binfmt_misc",
    b"bpf",
    b"cgroup",
    b"cgroup2",
    b"configfs",
    b"cpuset",
    b"debugfs",
    b"devfs",
    b"devpts",
    b"devtmpfs",
    b"dlmfs",
    b"dmabuf",
    b"drm",
    b"efivarfs",
    b"fuse",
    b"fuse.archivemount",
    b"fuse.avfsd",
    b"fuse.dumpfs",
    b"fuse.encfs",
    b"fuse.gvfs-fuse-daemon",
    b"fuse.gvfsd-fuse",
    b"fuse.lxcfs",
    b"fuse.rofiles-fuse",
    b"fuse.vmware-vmblock",
    b"fuse.xwmfs",
    b"fusectl",
    b"hugetlbfs",
    b"ipathfs",
    b"mqueue",
    b"nfsd",
    b"none",
    b"nsfs",
    b"overlay",
    b"pipefs",
    b"proc",
    b"pstore",
    b"ramfs",
    b"resctrl",
    b"rootfs",
    b"rpc_pipefs",
    b"securityfs",
    b"selinuxfs",
    b"smackfs",
    b"sockfs",
    b"spufs",
    b"sysfs",
    b"tmpfs",
    b"tracefs",
    b"vboxsf",
    b"virtiofs",
];

/// `mnt_fstype_is_pseudofs(type)`.
#[must_use]
pub fn fstype_is_pseudofs(ty: &[u8]) -> bool {
    PSEUDOFS.binary_search(&ty).is_ok()
}

/// `mnt_fstype_is_netfs(type)`.
#[must_use]
pub fn fstype_is_netfs(ty: &[u8]) -> bool {
    matches!(
        ty,
        b"cifs"
            | b"smb3"
            | b"smbfs"
            | b"afs"
            | b"ncpfs"
            | b"glusterfs"
            | b"fuse.curlftpfs"
            | b"fuse.sshfs"
    ) || ty.starts_with(b"nfs")
        || ty.starts_with(b"9p")
}

/// `strncasecmp(a, b, n) == 0` over C strings: the first `n` bytes agree
/// ignoring ASCII case, a string shorter than `n` agreeing only if the
/// other ends at the same place.
fn strncaseeq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if !x.eq_ignore_ascii_case(&y) {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

/// `match_fstype(type, pattern)` from `lib/match.c`: whether `ty` is in a
/// comma-separated list of types, compared without case; a leading `no`
/// inverts the whole list, and a `no` before one item excludes that one
/// (`nofoo,bar` means `nofoo,nobar`).
#[must_use]
pub fn match_fstype(ty: Option<&[u8]>, pattern: Option<&[u8]>) -> bool {
    let (Some(ty), Some(pattern)) = (ty, pattern) else {
        return ty.is_none() && pattern.is_none();
    };
    let (no, pattern) = match pattern.strip_prefix(b"no") {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let len = ty.len();
    let at = |p: &[u8], i: usize| p.get(i).copied().unwrap_or(0);
    let mut p = pattern;
    loop {
        if p.starts_with(b"no") && strncaseeq(p.get(2..).unwrap_or_default(), ty, len) {
            let end = at(p, len.saturating_add(2));
            if end == 0 || end == b',' {
                return false;
            }
        }
        if strncaseeq(p, ty, len) {
            let end = at(p, len);
            if end == 0 || end == b',' {
                return !no;
            }
        }
        match p.iter().position(|&b| b == b',') {
            Some(i) => p = p.get(i.saturating_add(1)..).unwrap_or_default(),
            None => break,
        }
    }
    no
}

/// `next_path_segment(str, &sz)`: the next `/name` of a path, repeated
/// slashes before it skipped; `None` at the end.
fn next_path_segment(s: &[u8]) -> Option<(usize, usize)> {
    let mut start = 0usize;
    while s.get(start) == Some(&b'/') && s.get(start.saturating_add(1)) == Some(&b'/') {
        start = start.saturating_add(1);
    }
    if s.get(start).is_none_or(|&b| b == 0) {
        return None;
    }
    let mut end = start.saturating_add(1);
    while s.get(end).is_some_and(|&b| b != 0 && b != b'/') {
        end = end.saturating_add(1);
    }
    Some((start, end))
}

/// `streq_paths(a, b)`: the same path, repeated slashes and a trailing one
/// aside.
#[must_use]
pub fn streq_paths(a: Option<&[u8]>, b: Option<&[u8]>) -> bool {
    let (Some(mut a), Some(mut b)) = (a, b) else {
        return false;
    };
    loop {
        let sa = next_path_segment(a);
        let sb = next_path_segment(b);
        let a_sz = sa.map_or(0, |(s, e)| e.saturating_sub(s));
        let b_sz = sb.map_or(0, |(s, e)| e.saturating_sub(s));
        if a_sz.saturating_add(b_sz) == 0 {
            return true;
        }
        fn seg(p: &[u8], r: Option<(usize, usize)>) -> Option<&[u8]> {
            r.and_then(|(s, e)| p.get(s..e))
        }
        let (a_seg, b_seg) = (seg(a, sa), seg(b, sb));
        if a_sz.saturating_add(b_sz) == 1
            && (a_seg.is_some_and(|x| x.first() == Some(&b'/'))
                || b_seg.is_some_and(|x| x.first() == Some(&b'/')))
        {
            return true;
        }
        let (Some(x), Some(y)) = (a_seg, b_seg) else {
            return false;
        };
        if x != y {
            return false;
        }
        a = a.get(sa.map_or(0, |(_, e)| e)..).unwrap_or_default();
        b = b.get(sb.map_or(0, |(_, e)| e)..).unwrap_or_default();
    }
}

/// `mnt_valid_tagname(name)`.
#[must_use]
pub fn valid_tagname(name: &[u8]) -> bool {
    matches!(
        name,
        b"ID" | b"UUID" | b"LABEL" | b"PARTUUID" | b"PARTLABEL"
    )
}

/// `blkid_parse_tag_string(token, &type, &value)`: `NAME=value`, a value in
/// matching quotes unquoted up to the *last* such quote; `None` without an
/// `=`, with an empty value, or with an unclosed quote.
#[must_use]
pub fn parse_tag_string(token: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let token = crate::c_str(token);
    let eq = token.iter().position(|&b| b == b'=')?;
    let name = token.get(..eq)?.to_vec();
    let mut value = token.get(eq.saturating_add(1)..)?;
    if let Some(&q) = value.first()
        && (q == b'"' || q == b'\'')
    {
        let rest = value.get(1..).unwrap_or_default();
        let close = rest.iter().rposition(|&b| b == q)?;
        value = rest.get(..close).unwrap_or_default();
    }
    if value.is_empty() {
        return None;
    }
    Some((name, value.to_vec()))
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests index what they built"
)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_and_network_types() {
        assert!(fstype_is_pseudofs(b"proc"));
        assert!(fstype_is_pseudofs(b"fuse.lxcfs"));
        assert!(!fstype_is_pseudofs(b"ext4"));
        assert!(fstype_is_netfs(b"nfs4"));
        assert!(fstype_is_netfs(b"9p"));
        assert!(!fstype_is_netfs(b"ext4"));
        assert!(PSEUDOFS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn type_lists_match_as_upstream() {
        let m = |t: &str, p: &str| match_fstype(Some(t.as_bytes()), Some(p.as_bytes()));
        assert!(m("ext4", "ext4"));
        assert!(m("ext4", "xfs,EXT4"));
        assert!(!m("ext4", "noext4"));
        assert!(m("ext4", "noxfs"));
        assert!(!m("ext4", "noxfs,ext4"));
        assert!(!m("ext4", "xfs,noext4"));
        assert!(!m("ext4", "ext"));
        assert!(!m("ext", "ext4"));
        assert!(match_fstype(None, None));
        assert!(!match_fstype(Some(b"ext4"), None));
    }

    #[test]
    fn paths_compare_as_upstream() {
        let e = |a: &str, b: &str| streq_paths(Some(a.as_bytes()), Some(b.as_bytes()));
        assert!(e("/mnt", "/mnt/"));
        assert!(e("//mnt", "/mnt"));
        assert!(e("/mnt//x", "/mnt/x"));
        assert!(!e("/mnt/x", "/mnt/y"));
        assert!(e("/", "/"));
        assert!(!streq_paths(None, Some(b"/")));
    }

    #[test]
    fn tags_parse_as_blkid() {
        assert_eq!(
            parse_tag_string(b"LABEL=root"),
            Some((b"LABEL".to_vec(), b"root".to_vec()))
        );
        assert_eq!(
            parse_tag_string(b"UUID=\"a b\""),
            Some((b"UUID".to_vec(), b"a b".to_vec()))
        );
        assert_eq!(
            parse_tag_string(b"LABEL=\"a\"b\""),
            Some((b"LABEL".to_vec(), b"a\"b".to_vec()))
        );
        assert_eq!(parse_tag_string(b"LABEL="), None);
        assert_eq!(parse_tag_string(b"LABEL=\"x"), None);
        assert_eq!(parse_tag_string(b"/dev/sda1"), None);
    }
}
