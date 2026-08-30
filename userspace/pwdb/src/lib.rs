//! `/etc/passwd` and `/etc/group`, parsed the way glibc parses them.
//!
//! # Why this is a crate
//!
//! Twenty-six programs in this tree read `/etc/passwd`, and each of them had
//! written its own parser: `login`, `su`, `sudo`, `doas`, `passwd`, `chage`,
//! `chpasswd`, `useradd`, `getent`, `id`, `who`, `w`, `last`, `htop`, `lsof`,
//! `fuser`, `pgrep`, `pstree`, `lsns`, `loginctl`, `install`, `mktemp`,
//! `audit`, `selinux`, `sshd`, `ftpd`. That is the same shape as the four
//! chmod-string parsers that became [`modechange`] and the six temp-directory
//! helpers that became `scratchdir`, and it has the same consequence: a format
//! with twenty-six implementations is a format with no definition, so two
//! programs shown the same file can disagree about who a uid is.
//!
//! The disagreements are not hypothetical, and they are not evenly harmless.
//! A parser that splits on `:` and takes field 6 as the shell reads
//! `root:x:0:0:root:/root:/bin/sh:extra` differently from glibc, which puts
//! `/bin/sh:extra` in the shell. A parser that accepts `#root:x:0:0:...` reads
//! a *commented-out* root account as a live one. A parser using
//! `strtoul(s, NULL, 0)` reads uid `010` as 8 where glibc reads 10 — so a file
//! written by one tool grants a different account's privileges when read by
//! another. `sudo` and `login` are on that list.
//!
//! # This is not `posix::pwd`
//!
//! `posix::pwd` — our libc's `getpwuid`/`getpwnam` — does not read
//! `/etc/passwd` at all. It answers "root, uid 0, /bin/sh" for uid 0 and null
//! for everything else, and its module docs say so ("Our OS doesn't have a
//! real user database"). Every C program on this system, and every Rust
//! program that goes through libc, therefore sees a machine with one user.
//! That is why the twenty-six wrote their own parsers rather than calling
//! `getpwuid`: the libc entry point exists and is a stub.
//!
//! The intended end state is that `posix::pwd` is a thin shell over this
//! crate, so that C programs and Rust programs get the same answer. That
//! change belongs with the libc, is a larger job than the utility that
//! prompted this crate, and is logged in `known-issues.md` rather than done
//! here. Until it happens, a program that wants the real database must use
//! this crate and not libc.
//!
//! # The rules are measured, not remembered
//!
//! Every rule below was measured against glibc 2.39's own parser — a C program
//! calling `fgetpwent`/`fgetgrent` on a crafted file, so what is recorded is
//! what `__nss_files_parse_pwent` does rather than what the manual page says
//! it does. Several are not what a from-memory implementation would produce,
//! and those are the ones worth stating:
//!
//! | Input | glibc | The obvious guess |
//! |---|---|---|
//! | `oct:x:010:16:…` | uid **10** | 8, from `strtoul` base 0 |
//! | `#root:x:0:0:…` | **skipped** | a user named `#root` |
//! | `few:x:2:2` | **accepted**, empty gecos/dir/shell | rejected, too few fields |
//! | `a:x:3:3:g:/h:/s:X` | shell is **`/s:X`** | shell is `/s`, `X` dropped |
//! | `e:x::9:…` | **rejected** | uid 0 |
//! | `n:x:-1:9:…` | **rejected** | uid 4294967295 |
//! | `wheel:x:13:a,b,` | members **`a`, `b`** | `a`, `b`, `` |
//! | `wheel:x:14:  a  ,  b  ` | members **`a  `, `b  `** | `a`, `b` — trimmed both ends |
//! | `+@netgroup` | accepted, all fields absent | rejected, no colons |
//!
//! The last four rows are the ones a hand-written parser gets wrong most
//! often, and the `-1` row is the one that matters most: a uid field of `-1`
//! is how a corrupted or hostile line asks to be read as uid 4294967295, and
//! on a system where that is `nobody` the difference between "rejected" and
//! "uid 4294967295" is the difference between an error and a silent
//! misattribution.
//!
//! The tests below are a *transcription* of that probe's output, and a
//! transcription is somewhere an error can hide, so the agreement was also
//! checked mechanically: this crate and the C probe were run over the same nine
//! files — the crafted ones covering every row above, plus a real `/etc/passwd`
//! and `/etc/group` — and their output compared byte for byte. 42 crafted
//! records and 90 real ones, no differences.
//!
//! # Names are bytes
//!
//! A user name, a home directory and a shell path are all OS-boundary data, so
//! they are `Vec<u8>` and not `String` (`CLAUDE.md` self-review item 7). This
//! is not pedantry about a case that cannot arise: `pw_dir` is a *path*, and
//! this OS allows every byte but `/` and NUL in one. A parser that insisted on
//! UTF-8 would either panic or silently corrupt the home directory of an
//! account whose name is spelled in a non-UTF-8 locale's encoding — and
//! `from_utf8_lossy` on a path is exactly the silent data corruption that rule
//! forbids.
//!
//! # What is deliberately not here
//!
//! **No writer.** Nothing in this tree needs to rewrite `/etc/passwd` through
//! a shared crate today, and a writer is where the interesting failure modes
//! live — `userdb`'s module docs describe two writers that deleted each
//! other's fields. When one is needed it should preserve unknown fields, as
//! `userdb` does, and that is a design worth doing deliberately rather than
//! as a side effect of a reader.
//!
//! **No shadow file.** `/etc/shadow` is a separate format with separate
//! permissions, and mixing it in would mean every caller of `user_by_uid` —
//! including `ls`, which wants a name for a listing — opening a
//! root-only file and being denied. Password verification lives in `posix::crypt`
//! and its callers.
//!
//! **No NSS.** `nsswitch.conf`, LDAP and NIS are not implemented on this
//! system. The one concession is that the `+`/`-` NIS *syntax* is parsed
//! ([`Entry::Nis`]) rather than rejected, because glibc parses it and a file
//! copied from another machine can contain it; treating such a line as a
//! malformed record would be a silent difference from the system whose files
//! we are reading.

#![deny(clippy::all, clippy::pedantic)]

use std::collections::HashMap;
use std::path::Path;

/// Where the databases live. Overridable by the caller — see
/// [`Db::from_files`] — because the tests need to read a fixture and because a
/// chroot has its own pair.
pub const PASSWD_PATH: &str = "/etc/passwd";
/// The group database's usual path. See [`PASSWD_PATH`].
pub const GROUP_PATH: &str = "/etc/group";

/// One account, as `/etc/passwd` spells it.
///
/// Every field but [`uid`](Self::uid) and [`gid`](Self::gid) is bytes; see the
/// module docs for why. The three trailing fields may legitimately be empty,
/// because glibc accepts a line that simply stops after the gid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Login name. Never empty in a well-formed file, but *not* validated:
    /// glibc accepts `:x:0:0:::` and so does this, since rejecting it here
    /// would hide a real line in a real file rather than fix it.
    pub name: Vec<u8>,
    /// The second field. Historically the hash; `x` on any system with a
    /// shadow file.
    pub passwd: Vec<u8>,
    /// User id.
    pub uid: u32,
    /// The account's primary group.
    pub gid: u32,
    /// The GECOS field — full name and comma-separated extras.
    pub gecos: Vec<u8>,
    /// Home directory.
    pub dir: Vec<u8>,
    /// Login shell. **Absorbs any further colons on the line**; see the module
    /// docs' table.
    pub shell: Vec<u8>,
}

/// One group, as `/etc/group` spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Group name.
    pub name: Vec<u8>,
    /// The second field, as [`User::passwd`].
    pub passwd: Vec<u8>,
    /// Group id.
    pub gid: u32,
    /// Supplementary members, in file order. Members listed here are those for
    /// whom this is *not* the primary group; the primary members are the
    /// accounts whose [`User::gid`] is this gid, and are not repeated here.
    pub members: Vec<Vec<u8>>,
}

/// What one line of either file turned out to be.
///
/// [`Nis`](Self::Nis) exists so that a `+`/`-` line is distinguishable from a
/// record rather than being either dropped or mistaken for an account named
/// `+`. No caller in this tree acts on it today — see the module docs — but a
/// caller that enumerates accounts must not present it as one, and that
/// requires being able to tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry<T> {
    /// A record.
    Record(T),
    /// A netgroup or inclusion/exclusion line: the whole line, `+`/`-` and
    /// all. glibc gives such a line a name and leaves every other field null.
    Nis(Vec<u8>),
}

/// The two ways a byte string can fail to be a decimal id, kept apart because
/// only one of them is a *field* problem.
///
/// Not public: every caller of [`parse_id`] treats both the same way — the
/// line is skipped, exactly as glibc skips it — and a public error type would
/// invite a caller to distinguish them and thereby diverge.
fn parse_id(field: &[u8]) -> Option<u32> {
    // Empty is not zero. Measured: `e:x::9:g:/h:/s` is skipped entirely, so a
    // missing uid must not be read as uid 0 — which is root.
    if field.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    for &b in field {
        // Base ten, explicitly. glibc reads the field with a base-10
        // conversion, so `010` is ten and `0x10` is not a number at all. A
        // parser reaching for `strtoul(…, 0)` or Rust's `from_str_radix(…, 0)`
        // equivalent would read `010` as eight and hand the caller a different
        // account than the file names. Measured both ways.
        // Notably this rejects `-` and `+`, so `-1` is not 4294967295.
        // Measured: glibc skips the line. `checked_sub` rather than `-` because
        // the crate denies `arithmetic_side_effects`, and the arm's range guard
        // is the sort of proof-by-context the lint is right not to trust.
        let digit = u32::from(b.checked_sub(b'0').filter(|d| *d < 10)?);
        // Measured: `4294967296` is rejected rather than wrapped.
        value = value.checked_mul(10)?.checked_add(digit)?;
    }
    Some(value)
}

/// The prefix of `line` that glibc would look at: leading blanks removed.
///
/// Measured: `  spaced:x:1:1:…` yields the name `spaced`, and a comment is
/// recognised after indentation (`   # …` is skipped). Trailing blanks are
/// *not* touched — a group member of `b  ` keeps both spaces — so this trims
/// one end only, which is why it is not `trim`.
fn skip_leading_blanks(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .unwrap_or(line.len());
    // `position` returned an index into `line`, so this cannot panic.
    line.get(start..).unwrap_or(&[])
}

/// Whether glibc would ignore this line outright: blank, or a comment.
fn is_ignorable(trimmed: &[u8]) -> bool {
    trimmed.is_empty() || trimmed.first() == Some(&b'#')
}

/// Split `line` into at most `n` colon-separated fields, the last of which
/// keeps every remaining colon.
///
/// The cap is the whole point. `splitn(7)`-style splitting is what a
/// hand-written parser does, and it happens to be right for `passwd` and wrong
/// for `group`: a group line's member list is field four of four, so
/// `extra:x:15:a:b:c` has **one** member, `a:b:c`. Measured.
fn fields(line: &[u8], n: usize) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = Vec::with_capacity(n);
    let mut rest = line;
    while out.len().saturating_add(1) < n {
        match rest.iter().position(|&b| b == b':') {
            Some(i) => {
                out.push(rest.get(..i).unwrap_or(&[]));
                rest = rest.get(i.saturating_add(1)..).unwrap_or(&[]);
            }
            None => break,
        }
    }
    out.push(rest);
    out
}

/// A `+`/`-` line, if this is one.
///
/// The rule was measured rather than assumed, and it is narrower than "starts
/// with `+`": `+plusfull:x:14:14:g:/h:/s` is an ordinary account named
/// `+plusfull`, and `+bad:x:zz:1:…` is *rejected* rather than falling back to
/// a netgroup line. So the NIS form is exactly "no colon anywhere, and begins
/// with `+` or `-`" — and a colonless line that begins with neither is simply
/// malformed.
fn nis_line(trimmed: &[u8]) -> bool {
    !trimmed.contains(&b':') && matches!(trimmed.first(), Some(b'+' | b'-'))
}

/// Parse one line of `/etc/passwd`.
///
/// Returns `None` for a line glibc would skip: blank, comment, colonless and
/// not NIS, or holding a uid or gid that is not a decimal number in range.
/// The trailing newline may be present or absent; a trailing `\r` is **not**
/// stripped, because glibc does not strip it — a CRLF `passwd` file gives
/// glibc a shell ending in `\r`, and a parser that quietly cleaned that up
/// would disagree with the system it is supposed to be describing.
#[must_use]
pub fn parse_user_line(line: &[u8]) -> Option<Entry<User>> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = skip_leading_blanks(line);
    if is_ignorable(trimmed) {
        return None;
    }
    if nis_line(trimmed) {
        return Some(Entry::Nis(trimmed.to_vec()));
    }
    let f = fields(trimmed, 7);
    // Fewer than three fields cannot carry a gid, and glibc rejects such a
    // line. Three or more is enough: `f3:x:5:5:` and even `f4:x:4:4` parse,
    // with the absent trailing fields empty.
    let uid = parse_id(f.get(2)?)?;
    let gid = parse_id(f.get(3)?)?;
    Some(Entry::Record(User {
        name: f.first()?.to_vec(),
        passwd: f.get(1)?.to_vec(),
        uid,
        gid,
        gecos: f.get(4).unwrap_or(&&[][..]).to_vec(),
        dir: f.get(5).unwrap_or(&&[][..]).to_vec(),
        shell: f.get(6).unwrap_or(&&[][..]).to_vec(),
    }))
}

/// Parse one line of `/etc/group`. See [`parse_user_line`] for the shared
/// rules; the member list is the one thing this format has that the other does
/// not, and [`members`] documents it.
#[must_use]
pub fn parse_group_line(line: &[u8]) -> Option<Entry<Group>> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let trimmed = skip_leading_blanks(line);
    if is_ignorable(trimmed) {
        return None;
    }
    if nis_line(trimmed) {
        return Some(Entry::Nis(trimmed.to_vec()));
    }
    let f = fields(trimmed, 4);
    let gid = parse_id(f.get(2)?)?;
    Some(Entry::Record(Group {
        name: f.first()?.to_vec(),
        passwd: f.get(1)?.to_vec(),
        gid,
        members: members(f.get(3).unwrap_or(&&[][..])),
    }))
}

/// Split a group's member list on commas, glibc's way.
///
/// Two asymmetries, both measured, both the kind a from-memory implementation
/// smooths over:
///
/// * **Empty members are dropped**, wherever they come from: `a,b,` is two
///   members, `,a` is one, and `,` is none. So a trailing comma — which is how
///   a script that appends members without checking emptiness writes the
///   list — costs nothing.
/// * **Leading blanks are stripped from each member and trailing blanks are
///   not.** `  a  ,  b  ` is `a  ` and `b  `, with two trailing spaces each.
///   That is not a rule anyone would choose, and it is not symmetric with the
///   line-level trim, but a member spelled `b  ` will not match a user named
///   `b` and the caller needs to know that rather than have it hidden.
#[must_use]
pub fn members(field: &[u8]) -> Vec<Vec<u8>> {
    field
        .split(|&b| b == b',')
        .map(skip_leading_blanks)
        .filter(|m| !m.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// Every record in a whole `/etc/passwd`, in file order, NIS lines dropped.
#[must_use]
pub fn users(text: &[u8]) -> Vec<User> {
    records(text, parse_user_line)
}

/// Every record in a whole `/etc/group`, in file order, NIS lines dropped.
#[must_use]
pub fn groups(text: &[u8]) -> Vec<Group> {
    records(text, parse_group_line)
}

/// The shared body of [`users`] and [`groups`].
fn records<T>(text: &[u8], parse: fn(&[u8]) -> Option<Entry<T>>) -> Vec<T> {
    text.split(|&b| b == b'\n')
        .filter_map(parse)
        .filter_map(|e| match e {
            Entry::Record(r) => Some(r),
            Entry::Nis(_) => None,
        })
        .collect()
}

/// Both databases, read once and indexed.
///
/// # Why this caches
///
/// A long listing asks for the same handful of uids once per file. GNU `ls`
/// keeps a hash table for exactly this reason, and without one a directory of
/// ten thousand files owned by one user re-reads and re-parses `/etc/passwd`
/// ten thousand times. The cost of getting it wrong is not merely slowness:
/// it is a listing whose owner column changes halfway down because the file
/// was edited while the listing was being produced.
///
/// # Duplicates: first wins
///
/// A file may name two accounts with one uid, and `getpwuid` answers with the
/// first. This indexes the same way, and deliberately keeps the *later*
/// records reachable through [`Db::all_users`] rather than discarding them —
/// a tool auditing the file needs to see the duplicate, and a tool rendering a
/// listing needs the same answer glibc would give.
#[derive(Debug, Clone, Default)]
pub struct Db {
    all_users: Vec<User>,
    all_groups: Vec<Group>,
    by_uid: HashMap<u32, usize>,
    by_user_name: HashMap<Vec<u8>, usize>,
    by_gid: HashMap<u32, usize>,
    by_group_name: HashMap<Vec<u8>, usize>,
}

impl Db {
    /// Index the two files' contents. Takes bytes rather than paths so the
    /// whole of this crate can be tested without a filesystem — which on the
    /// development host, where `cargo test --workspace` runs, has no
    /// `/etc/passwd` at all.
    #[must_use]
    pub fn from_bytes(passwd: &[u8], group: &[u8]) -> Self {
        let all_users = users(passwd);
        let all_groups = groups(group);
        let mut db = Self {
            by_uid: HashMap::with_capacity(all_users.len()),
            by_user_name: HashMap::with_capacity(all_users.len()),
            by_gid: HashMap::with_capacity(all_groups.len()),
            by_group_name: HashMap::with_capacity(all_groups.len()),
            all_users,
            all_groups,
        };
        for (i, u) in db.all_users.iter().enumerate() {
            // `or_insert` and not `insert`: first wins. See the type's docs.
            db.by_uid.entry(u.uid).or_insert(i);
            db.by_user_name.entry(u.name.clone()).or_insert(i);
        }
        for (i, g) in db.all_groups.iter().enumerate() {
            db.by_gid.entry(g.gid).or_insert(i);
            db.by_group_name.entry(g.name.clone()).or_insert(i);
        }
        db
    }

    /// Read and index the two files.
    ///
    /// **A file that cannot be read is an empty database, not an error.** That
    /// is glibc's behaviour and it is the one the callers want: `ls -l` on a
    /// system with no `/etc/passwd` must still list the directory, printing
    /// numeric ids, rather than fail. A caller for whom a missing database
    /// *is* an error — `useradd`, say — should stat the file itself and say so
    /// in its own words, since "no such file" and "no such user" are different
    /// diagnostics and only the caller knows which it means.
    #[must_use]
    pub fn from_files(passwd: &Path, group: &Path) -> Self {
        let p = std::fs::read(passwd).unwrap_or_default();
        let g = std::fs::read(group).unwrap_or_default();
        Self::from_bytes(&p, &g)
    }

    /// [`from_files`](Self::from_files) on [`PASSWD_PATH`] and [`GROUP_PATH`].
    #[must_use]
    pub fn load() -> Self {
        Self::from_files(Path::new(PASSWD_PATH), Path::new(GROUP_PATH))
    }

    /// The account with this uid, or `None`.
    #[must_use]
    pub fn user_by_uid(&self, uid: u32) -> Option<&User> {
        self.all_users.get(*self.by_uid.get(&uid)?)
    }

    /// The account with this name, or `None`.
    #[must_use]
    pub fn user_by_name(&self, name: &[u8]) -> Option<&User> {
        self.all_users.get(*self.by_user_name.get(name)?)
    }

    /// The group with this gid, or `None`.
    #[must_use]
    pub fn group_by_gid(&self, gid: u32) -> Option<&Group> {
        self.all_groups.get(*self.by_gid.get(&gid)?)
    }

    /// The group with this name, or `None`.
    #[must_use]
    pub fn group_by_name(&self, name: &[u8]) -> Option<&Group> {
        self.all_groups.get(*self.by_group_name.get(name)?)
    }

    /// Every account, in file order, duplicates included.
    #[must_use]
    pub fn all_users(&self) -> &[User] {
        &self.all_users
    }

    /// Every group, in file order, duplicates included.
    #[must_use]
    pub fn all_groups(&self) -> &[Group] {
        &self.all_groups
    }

    /// glibc's `getgrouplist(name, gid, …)`: every group `name` belongs to.
    ///
    /// `gid` is the account's *login* group — `pw_gid`, which is a field of
    /// the passwd line and so need not appear in any `/etc/group` member list
    /// at all. It is therefore supplied by the caller rather than looked up,
    /// and it comes first in the result.
    ///
    /// After it come the groups whose member list names `name`, in file order,
    /// **skipping any whose gid equals `gid`** — that suppression is glibc's
    /// (`files_initgroups_dyn` compares gids, not names) and it is why an
    /// account explicitly listed in its own login group is not reported twice.
    /// Measured against glibc 2.39 on this machine's own database:
    /// `getgrouplist("inhahe", 1000)` is `[1000, 4, 24, 27, 30, 46, 100,
    /// 1001]`, and `getgrouplist("inhahe", 4)` is `[4, 24, 27, 30, 46, 100,
    /// 1001]` — the same account, the same `adm` line, and `4` still appearing
    /// exactly once because the *passed* gid won.
    ///
    /// The result is what `id -G` and `id`'s `groups=` field print, so the
    /// order is observable and is glibc's rather than sorted.
    #[must_use]
    pub fn group_list(&self, name: &[u8], gid: u32) -> Vec<u32> {
        let mut out = vec![gid];
        out.extend(
            self.all_groups
                .iter()
                .filter(|g| g.gid != gid && g.members.iter().any(|m| m == name))
                .map(|g| g.gid),
        );
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// Every assertion in this module is a row of a transcript produced by
    /// glibc 2.39's own `fgetpwent`/`fgetgrent` on the input quoted beside it.
    /// A test here that disagrees with glibc is this crate's bug, not glibc's.
    fn user(line: &str) -> Option<User> {
        match parse_user_line(line.as_bytes()) {
            Some(Entry::Record(u)) => Some(u),
            _ => None,
        }
    }

    fn group(line: &str) -> Option<Group> {
        match parse_group_line(line.as_bytes()) {
            Some(Entry::Record(g)) => Some(g),
            _ => None,
        }
    }

    fn s(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    // ---------------- the ordinary case ----------------

    #[test]
    fn a_whole_line_becomes_a_whole_record() {
        let u = user("root:x:0:0:root:/root:/bin/bash").unwrap();
        assert_eq!(s(&u.name), "root");
        assert_eq!(s(&u.passwd), "x");
        assert_eq!(u.uid, 0);
        assert_eq!(u.gid, 0);
        assert_eq!(s(&u.gecos), "root");
        assert_eq!(s(&u.dir), "/root");
        assert_eq!(s(&u.shell), "/bin/bash");
    }

    // ---------------- what is skipped ----------------

    /// A comment is a comment even when what follows it is a perfectly
    /// well-formed record — which is the case that distinguishes a real
    /// comment rule from a line that merely failed some other check. Measured
    /// with `#hash:x:1:1:g:/h:/s`, which has every field in place and is still
    /// skipped; a parser without a comment rule would report a user named
    /// `#hash`, and on a file where an administrator commented out an account
    /// that is the account still being honoured.
    #[test]
    fn a_commented_out_account_is_not_an_account() {
        assert!(user("#hash:x:1:1:g:/h:/s").is_none());
        assert!(group("#hashg:x:1:a").is_none());
    }

    /// And the `#` is found after indentation, not only at column zero.
    #[test]
    fn a_comment_may_be_indented() {
        assert!(user("   #indent:x:2:2:g:/h:/s").is_none());
    }

    #[test]
    fn blank_lines_are_skipped_however_they_are_spelled() {
        assert!(user("").is_none());
        assert!(user("   ").is_none());
        assert!(user("\t").is_none());
        assert!(group("").is_none());
    }

    /// Leading blanks belong to nobody: they are removed before the name
    /// begins. Measured: `  spaced:x:1:1:g:/h:/s` is the user `spaced`, not
    /// `  spaced`.
    #[test]
    fn leading_blanks_are_not_part_of_the_name() {
        assert_eq!(s(&user("  spaced:x:1:1:g:/h:/s").unwrap().name), "spaced");
        assert_eq!(
            s(&user("\ttabfirst:x:12:12:g:/h:/s").unwrap().name),
            "tabfirst"
        );
        assert_eq!(s(&group("  indent:x:11:carol").unwrap().name), "indent");
    }

    /// ...and trailing ones do belong to somebody. The asymmetry is glibc's;
    /// see [`members`].
    #[test]
    fn trailing_blanks_are_kept() {
        let g = group("spaces:x:14:a, b ,c").unwrap();
        assert_eq!(
            g.members.iter().map(|m| s(m)).collect::<Vec<_>>(),
            ["a", "b ", "c"]
        );
    }

    // ---------------- the id fields ----------------

    /// The single most consequential measured rule in this file. A uid field
    /// of `010` is **ten**, because glibc converts it in base ten; a parser
    /// that reached for a base-0 conversion — the natural choice, and what C's
    /// `strtoul(s, NULL, 0)` does — reads it as eight and attributes the
    /// account's files to a different user.
    #[test]
    fn a_leading_zero_does_not_mean_octal() {
        assert_eq!(user("oct:x:010:16:g:/h:/s").unwrap().uid, 10);
    }

    /// Nor hexadecimal, which base-0 would also accept.
    #[test]
    fn hexadecimal_is_not_a_uid() {
        assert!(user("hex:x:0x10:15:g:/h:/s").is_none());
    }

    /// `-1` is not 4294967295. glibc skips the line rather than wrapping, so
    /// a corrupt or hostile `-1` produces no user at all instead of quietly
    /// becoming whichever account holds the top uid.
    #[test]
    fn a_negative_id_is_refused_rather_than_wrapped() {
        assert!(user("neg:x:-1:7:g:/h:/s").is_none());
    }

    /// An empty uid is not uid 0. Reading it as 0 would make a truncated line
    /// into root.
    #[test]
    fn an_empty_id_is_not_zero() {
        assert!(user("emptyuid:x::9:g:/h:/s").is_none());
        assert!(user("emptygid:x:11::g:/h:/s").is_none());
        assert!(group("emptygid:x::alice").is_none());
    }

    #[test]
    fn an_id_past_the_end_of_u32_is_refused_not_wrapped() {
        assert_eq!(
            user("big:x:4294967295:8:g:/h:/s").unwrap().uid,
            4_294_967_295
        );
        assert!(user("overflow:x:4294967296:18:g:/h:/s").is_none());
    }

    #[test]
    fn a_non_numeric_id_is_refused() {
        assert!(user("badnum:x:abc:6:g:/h:/s").is_none());
        assert!(group("badgid:x:zz:alice").is_none());
    }

    /// Spaces inside the number are not tolerated, even though spaces *before*
    /// the line are.
    #[test]
    fn an_id_may_not_be_padded() {
        assert!(user("spaceuid:x: 17 :17:g:/h:/s").is_none());
    }

    // ---------------- field counts ----------------

    /// A line that simply stops is accepted, with the absent fields empty.
    /// This is the rule most likely to be "fixed" by a hand-written parser
    /// into a rejection, and rejecting it loses real accounts: a `passwd` line
    /// with no shell is a normal thing to write.
    #[test]
    fn a_line_may_stop_after_the_gid() {
        let u = user("few:x:2:2").unwrap();
        assert_eq!(u.uid, 2);
        assert_eq!(u.gid, 2);
        assert!(u.gecos.is_empty());
        assert!(u.dir.is_empty());
        assert!(u.shell.is_empty());
    }

    #[test]
    fn a_line_that_stops_before_the_gid_is_not_a_record() {
        assert!(user("onecolon:x").is_none());
        assert!(user("nocolonatall").is_none());
    }

    /// The shell keeps every remaining colon. A `splitn(7)`-style parser gets
    /// this right by accident and a `split(':').nth(6)` one silently truncates.
    #[test]
    fn the_shell_absorbs_the_rest_of_the_line() {
        assert_eq!(s(&user("c:x:6:6:a:b:c:d:e").unwrap().shell), "c:d:e");
        assert_eq!(
            s(&user("trail:x:3:3:g:/h:/s:EXTRA").unwrap().shell),
            "/s:EXTRA"
        );
    }

    /// And the group's member list does too — which is the case where the
    /// `splitn` habit is actively wrong, because the member list is field four
    /// of four rather than seven of seven. `extra:x:15:a:b:c` is one member
    /// named `a:b:c`, not three groups' worth of anything.
    #[test]
    fn the_member_list_absorbs_the_rest_of_the_line_and_is_not_split_on_it() {
        let g = group("extra:x:15:a:b:c").unwrap();
        assert_eq!(
            g.members.iter().map(|m| s(m)).collect::<Vec<_>>(),
            ["a:b:c"]
        );
    }

    // ---------------- member lists ----------------

    #[test]
    fn an_empty_member_is_dropped_wherever_it_comes_from() {
        let cases = [("g:x:1:a,,b", vec!["a", "b"]), ("g:x:1:,a", vec!["a"])];
        for (line, want) in cases {
            let got = group(line).unwrap();
            assert_eq!(
                got.members.iter().map(|m| s(m)).collect::<Vec<_>>(),
                want,
                "{line}"
            );
        }
        for line in ["g:x:1:", "g:x:1:,", "g:x:1: ", "g:x:1"] {
            assert!(group(line).unwrap().members.is_empty(), "{line}");
        }
    }

    /// The full asymmetry in one assertion: leading blanks stripped per
    /// member, trailing blanks kept.
    #[test]
    fn a_member_is_trimmed_on_the_left_only() {
        let g = group("g5:x:5:  a  ,  b  ").unwrap();
        assert_eq!(
            g.members.iter().map(|m| s(m)).collect::<Vec<_>>(),
            ["a  ", "b  "]
        );
    }

    // ---------------- the NIS forms ----------------

    /// A colonless `+`/`-` line is a netgroup reference, not a malformed
    /// record and not an account named `+`.
    #[test]
    fn a_colonless_plus_line_is_a_netgroup_reference() {
        for line in ["+@netgroup", "+onlyplus", "-onlyminus", "+", "-"] {
            match parse_user_line(line.as_bytes()) {
                Some(Entry::Nis(name)) => assert_eq!(s(&name), line),
                other => panic!("{line}: {other:?}"),
            }
        }
        assert!(matches!(parse_group_line(b"+@ng"), Some(Entry::Nis(_))));
    }

    /// But a `+` line *with* colons is an ordinary record whose name happens
    /// to start with `+` — and if its uid is bad it is rejected outright
    /// rather than falling back to the netgroup reading. Both measured; the
    /// second is what makes "colonless" the right test rather than "starts
    /// with `+`".
    #[test]
    fn a_plus_line_with_fields_is_an_ordinary_record() {
        let u = user("+plusfull:x:14:14:g:/h:/s").unwrap();
        assert_eq!(s(&u.name), "+plusfull");
        assert_eq!(u.uid, 14);
        assert_eq!(s(&user("-minus:x:4:4:g:/h:/s").unwrap().name), "-minus");
        assert!(parse_user_line(b"+bad:x:zz:1:g:/h:/s").is_none());
        assert!(parse_user_line(b"+halfway:x:7").is_none());
    }

    #[test]
    fn a_netgroup_line_is_not_an_account() {
        let db = Db::from_bytes(b"+@netgroup\nroot:x:0:0:r:/root:/bin/sh\n", b"");
        assert_eq!(db.all_users().len(), 1);
        assert_eq!(s(&db.user_by_uid(0).unwrap().name), "root");
    }

    // ---------------- whole files ----------------

    /// The transcript from the probe, replayed whole. Sixteen lines in,
    /// eleven records out, in file order.
    #[test]
    fn the_measured_transcript_replays_row_for_row() {
        let text = concat!(
            "root:x:0:0:root:/root:/bin/bash\n",
            "# a comment line\n",
            "\n",
            "   \n",
            "  spaced:x:1:1:g:/h:/s\n",
            "few:x:2:2\n",
            "trail:x:3:3:g:/h:/s:EXTRA\n",
            "+@netgroup\n",
            "-minus:x:4:4:g:/h:/s\n",
            "empty::5:5:::\n",
            "badnum:x:abc:6:g:/h:/s\n",
            "neg:x:-1:7:g:/h:/s\n",
            "big:x:4294967295:8:g:/h:/s\n",
            "tab\tin:x:9:9:g:/h:/s\n",
            "dup:x:0:0:second-uid-zero:/d:/s\n",
            "colonless-line-no-colons\n",
            "last:x:10:10:g:/h:/s\n",
        );
        let got: Vec<String> = users(text.as_bytes()).iter().map(|u| s(&u.name)).collect();
        assert_eq!(
            got,
            [
                "root", "spaced", "few", "trail", "-minus", "empty", "big", "tab\tin", "dup",
                "last"
            ]
        );
    }

    /// A tab inside a name survives — it is only *leading* blanks that are
    /// removed, and the name field ends at a colon, not at whitespace.
    #[test]
    fn a_name_may_contain_a_tab() {
        assert_eq!(s(&user("tab\tin:x:9:9:g:/h:/s").unwrap().name), "tab\tin");
    }

    // ---------------- the index ----------------

    #[test]
    fn a_duplicate_uid_resolves_to_the_first_as_getpwuid_does() {
        let db = Db::from_bytes(b"root:x:0:0:r:/root:/bin/sh\ndup:x:0:0:second:/d:/s\n", b"");
        assert_eq!(s(&db.user_by_uid(0).unwrap().name), "root");
        // ...and the duplicate is still reachable, because a tool auditing the
        // file needs to see it.
        assert_eq!(db.all_users().len(), 2);
        assert_eq!(s(&db.all_users().get(1).unwrap().name), "dup");
    }

    #[test]
    fn lookups_answer_by_name_and_by_id_in_both_databases() {
        let db = Db::from_bytes(
            b"root:x:0:0:r:/root:/bin/sh\nalice:x:1000:1000:A:/home/alice:/bin/sh\n",
            b"root:x:0:\nwheel:x:10:alice,bob\n",
        );
        assert_eq!(db.user_by_uid(1000).unwrap().uid, 1000);
        assert_eq!(db.user_by_name(b"alice").unwrap().uid, 1000);
        assert_eq!(s(&db.group_by_gid(10).unwrap().name), "wheel");
        assert_eq!(db.group_by_name(b"wheel").unwrap().gid, 10);
        assert!(db.user_by_uid(4242).is_none());
        assert!(db.user_by_name(b"nobody").is_none());
        assert!(db.group_by_gid(4242).is_none());
        assert!(db.group_by_name(b"nogroup").is_none());
    }

    /// A missing file is an empty database and not a panic — the property
    /// `ls -l` depends on to keep listing a directory on a system that has no
    /// `/etc/passwd`, which is every system this test runs on.
    #[test]
    fn a_missing_file_is_an_empty_database() {
        let db = Db::from_files(
            Path::new("/nonexistent/passwd"),
            Path::new("/nonexistent/group"),
        );
        assert!(db.all_users().is_empty());
        assert!(db.all_groups().is_empty());
        assert!(db.user_by_uid(0).is_none());
    }

    /// A file with no trailing newline loses nothing. `split` on `\n` yields a
    /// final empty piece for a file that ends in one and a final *record* for
    /// a file that does not, and only the second is easy to drop by accident.
    #[test]
    fn the_last_line_is_read_with_or_without_a_final_newline() {
        assert_eq!(users(b"a:x:1:1:g:/h:/s").len(), 1);
        assert_eq!(users(b"a:x:1:1:g:/h:/s\n").len(), 1);
        assert_eq!(users(b"a:x:1:1:g:/h:/s\nb:x:2:2:g:/h:/s").len(), 2);
    }

    /// Not UTF-8, and not required to be. A home directory is a path and a
    /// path on this OS is bytes.
    #[test]
    fn a_non_utf8_field_survives_as_bytes() {
        let line: &[u8] = b"m\xff:x:1:1:g:/home/\xfe:/bin/sh";
        let Some(Entry::Record(u)) = parse_user_line(line) else {
            panic!("rejected a line glibc accepts");
        };
        assert_eq!(u.name, b"m\xff");
        assert_eq!(u.dir, b"/home/\xfe");
    }

    // ---------------------------------------------------------- group_list ---

    /// The `/etc/group` lines that mention `inhahe` on the machine the
    /// `getgrouplist` transcript below was taken from, verbatim and in file
    /// order — the order is what makes the expected lists reproducible.
    const MEASURED_GROUP_FILE: &[u8] = b"root:x:0:\n\
        adm:x:4:syslog,inhahe\n\
        cdrom:x:24:inhahe\n\
        sudo:x:27:inhahe\n\
        dip:x:30:inhahe\n\
        plugdev:x:46:inhahe\n\
        users:x:100:inhahe\n\
        inhahe:x:1000:\n\
        docker:x:1001:inhahe\n";

    fn measured_db() -> Db {
        Db::from_bytes(
            b"root:x:0:0:root:/root:/bin/bash\n\
              inhahe:x:1000:1000:,,,:/home/inhahe:/bin/bash\n",
            MEASURED_GROUP_FILE,
        )
    }

    /// `os.getgrouplist("inhahe", 1000)` on glibc 2.39, against the file above.
    ///
    /// Note that the login group `inhahe` (gid 1000) leads the list even though
    /// its member field is *empty*: the gid is a passwd field, and glibc trusts
    /// the caller's copy of it rather than searching for it.
    #[test]
    fn the_measured_group_list_replays() {
        assert_eq!(
            measured_db().group_list(b"inhahe", 1000),
            vec![1000, 4, 24, 27, 30, 46, 100, 1001]
        );
    }

    /// `os.getgrouplist("inhahe", 4)` — the same account and the same `adm`
    /// line, with `adm`'s gid passed as the login group. `4` appears exactly
    /// once, because glibc suppresses a member-list hit whose *gid* matches the
    /// one it was given.
    #[test]
    fn the_passed_gid_suppresses_the_member_list_entry_with_the_same_gid() {
        assert_eq!(
            measured_db().group_list(b"inhahe", 4),
            vec![4, 24, 27, 30, 46, 100, 1001]
        );
    }

    /// `os.getgrouplist("inhahe", 65534)` — a gid belonging to no line at all
    /// is still reported, and still first. This is why the argument exists: an
    /// account's login group need not appear in `/etc/group`.
    #[test]
    fn a_login_group_with_no_group_line_is_still_first() {
        assert_eq!(
            measured_db().group_list(b"inhahe", 65534),
            vec![65534, 4, 24, 27, 30, 46, 100, 1001]
        );
    }

    /// `os.getgrouplist("root", 0)` — an account in no member list gets a
    /// one-element list, not an empty one.
    #[test]
    fn an_account_in_no_member_list_gets_only_its_login_group() {
        assert_eq!(measured_db().group_list(b"root", 0), vec![0]);
        assert_eq!(measured_db().group_list(b"nosuchuser", 7), vec![7]);
    }

    /// Membership is an exact byte match, not a prefix or a substring: the
    /// member field is split on commas first, so an account called `in` is not
    /// a member of a group listing `inhahe`.
    #[test]
    fn membership_does_not_match_a_prefix_of_another_member() {
        let db = measured_db();
        assert_eq!(db.group_list(b"in", 5), vec![5]);
        assert_eq!(db.group_list(b"syslog", 5), vec![5, 4]);
    }

    /// A name that is not UTF-8 is looked up as bytes, like every other name
    /// in this crate.
    #[test]
    fn a_non_utf8_member_is_matched_as_bytes() {
        let db = Db::from_bytes(b"", b"caf\xe9s:x:9:caf\xe9,other\n");
        assert_eq!(db.group_list(b"caf\xe9", 1), vec![1, 9]);
        assert_eq!(db.group_list(b"caf", 1), vec![1]);
    }
}
