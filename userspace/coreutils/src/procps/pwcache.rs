//! procps-ng 4.0.4's `library/pwcache.c`: user and group names by number.
//!
//! What `ps` prints in a `USER` or `GROUP` column. The rule is upstream's: the
//! name the password (or group) database gives, unless there is none or it is
//! 33 bytes or longer (`P_G_SZ`, the size of the cache's buffer), in which
//! case the number in decimal.
//!
//! Upstream asks `getpwuid`/`getgrgid`; on SlateOS that is a stub (see
//! [`pwdb`]), so the files are read here by `pwdb`, which parses them as
//! glibc's `files` backend does. A harness that compares against upstream
//! therefore pins glibc to that backend, so both read the same file.

use std::collections::HashMap;

/// `P_G_SZ`: a name of this many bytes or more does not fit the cache entry.
const P_G_SZ: usize = 33;

/// The cache: each number is looked up once.
pub struct Pwcache {
    db: pwdb::Db,
    users: HashMap<u32, Vec<u8>>,
    groups: HashMap<u32, Vec<u8>>,
}

impl Pwcache {
    /// A cache over `/etc/passwd` and `/etc/group`.
    #[must_use]
    pub fn load() -> Self {
        Self::with_db(pwdb::Db::load())
    }

    /// A cache over a database already read.
    #[must_use]
    pub fn with_db(db: pwdb::Db) -> Self {
        Self {
            db,
            users: HashMap::new(),
            groups: HashMap::new(),
        }
    }

    /// `pwcache_get_user`.
    pub fn user(&mut self, uid: u32) -> Vec<u8> {
        if let Some(name) = self.users.get(&uid) {
            return name.clone();
        }
        let name = match self.db.user_by_uid(uid) {
            Some(u) if u.name.len() < P_G_SZ => u.name.clone(),
            _ => uid.to_string().into_bytes(),
        };
        self.users.insert(uid, name.clone());
        name
    }

    /// `pwcache_get_group`.
    pub fn group(&mut self, gid: u32) -> Vec<u8> {
        if let Some(name) = self.groups.get(&gid) {
            return name.clone();
        }
        let name = match self.db.group_by_gid(gid) {
            Some(g) if g.name.len() < P_G_SZ => g.name.clone(),
            _ => gid.to_string().into_bytes(),
        };
        self.groups.insert(gid, name.clone());
        name
    }

    /// `getpwnam`: the uid of a user name, for `ps -u NAME`.
    #[must_use]
    pub fn uid_by_name(&self, name: &[u8]) -> Option<u32> {
        self.db.user_by_name(name).map(|u| u.uid)
    }

    /// `getgrnam`: the gid of a group name, for `ps -G NAME`.
    #[must_use]
    pub fn gid_by_name(&self, name: &[u8]) -> Option<u32> {
        self.db.group_by_name(name).map(|g| g.gid)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cache() -> Pwcache {
        let passwd = b"root:x:0:0:root:/root:/bin/sh\n\
                       alice:x:1000:1000::/home/alice:/bin/sh\n\
                       a234567890123456789012345678901234:x:1001:1001::/:/bin/sh\n\
                       a23456789012345678901234567890123:x:1002:1002::/:/bin/sh\n\
                       a2345678901234567890123456789012:x:1003:1003::/:/bin/sh\n\
                       alice2:x:1000:1000::/:/bin/sh\n";
        let group = b"root:x:0:\nstaff:x:50:alice\n";
        Pwcache::with_db(pwdb::Db::from_bytes(passwd, group))
    }

    #[test]
    fn names_by_number_with_the_first_entry_winning() {
        let mut c = cache();
        assert_eq!(c.user(0), b"root");
        assert_eq!(c.user(1000), b"alice");
        assert_eq!(c.group(50), b"staff");
    }

    #[test]
    fn a_missing_or_overlong_name_is_the_number() {
        let mut c = cache();
        assert_eq!(c.user(4242), b"4242");
        assert_eq!(c.group(4242), b"4242");
        // 34 and 33 bytes do not fit; 32 is the longest name kept.
        assert_eq!(c.user(1001), b"1001");
        assert_eq!(c.user(1002), b"1002");
        assert_eq!(c.user(1003), b"a2345678901234567890123456789012");
    }

    #[test]
    fn names_back_to_numbers() {
        let c = cache();
        assert_eq!(c.uid_by_name(b"alice"), Some(1000));
        assert_eq!(c.gid_by_name(b"staff"), Some(50));
        assert_eq!(c.uid_by_name(b"nobody"), None);
    }
}
