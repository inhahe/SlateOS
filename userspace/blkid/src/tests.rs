//! Unit tests of the pieces that do not print; what the program prints is
//! `scripts/blkid-cli-diff.sh`'s to compare with upstream's.

use super::*;

#[test]
fn usage_lists_are_upstreams() {
    let mut flag = FLTR_ONLYIN;
    assert_eq!(
        list_to_usage(b"filesystem,raid", &mut flag, b"blkid"),
        Ok(USAGE_FILESYSTEM | USAGE_RAID)
    );
    assert_eq!(flag, FLTR_ONLYIN);
    // "no" inverts; a prefix of the word is enough, as strncmp checks.
    assert_eq!(
        list_to_usage(b"nocryptoX", &mut flag, b"blkid"),
        Ok(USAGE_CRYPTO)
    );
    assert_eq!(flag, FLTR_NOTIN);
    let mut flag = FLTR_ONLYIN;
    assert_eq!(
        list_to_usage(b"other,bogus", &mut flag, b"blkid"),
        Err(BLKID_EXIT_OTHER)
    );
    assert_eq!(flag, 0);
    assert_eq!(
        list_to_usage(b"no", &mut FLTR_ONLYIN.clone(), b"blkid"),
        Err(BLKID_EXIT_OTHER)
    );
}

#[test]
fn type_lists_are_upstreams() {
    let mut flag = FLTR_ONLYIN;
    assert_eq!(
        list_to_types(b"vfat,ext3,", &mut flag, b"blkid"),
        Ok(vec![b"vfat".to_vec(), b"ext3".to_vec(), Vec::new()])
    );
    assert_eq!(
        list_to_types(b"novfat", &mut flag, b"blkid"),
        Ok(vec![b"vfat".to_vec()])
    );
    assert_eq!(flag, FLTR_NOTIN);
    // A second list after a "no" one loses its first two bytes, as upstream's
    // does.
    assert_eq!(
        list_to_types(b"ext4", &mut flag, b"blkid"),
        Ok(vec![b"t4".to_vec()])
    );
    assert_eq!(
        list_to_types(b"no", &mut flag, b"blkid"),
        Err(BLKID_EXIT_OTHER)
    );
}

#[test]
fn hint_errnos_are_what_upstream_leaves() {
    // Does not parse: whatever was there.
    assert_eq!(hint_errno(b"name=", 2), 2);
    assert_eq!(hint_errno(b"name=\"open", 0), 0);
    // Parses, but is no number: strtoumax cleared it.
    assert_eq!(hint_errno(b"name=x1", 2), 0);
    // Too large.
    assert_eq!(hint_errno(b"name=99999999999999999999999", 0), ERANGE);
}

#[test]
fn options_map_to_their_long_names() {
    assert_eq!(option_to_longopt(i32::from(b'n')), Some("match-types"));
    assert_eq!(option_to_longopt(i32::from(b'u')), Some("usages"));
    assert_eq!(option_to_longopt(i32::from(b'w')), None);
    assert_eq!(LONGS.len(), LONG_VALS.len());
}

#[test]
fn encoding_keeps_what_fit() {
    assert_eq!(encoded(b"My Disk", 265), b"My\\x20Disk".to_vec());
    // Too long for the buffer: the part written.
    let long = vec![b' '; 100];
    let e = encoded(&long, 265);
    assert!(e.len() < 265);
    assert!(e.starts_with(b"\\x20\\x20"));
}

#[test]
fn the_usage_names_the_program() {
    let u = String::from_utf8(usage(b"blkid")).unwrap_or_default();
    assert!(u.starts_with("\nUsage:\n blkid --label <label> | --uuid <uuid>\n"));
    assert!(u.ends_with("\nFor more details see blkid(8).\n"));
}
