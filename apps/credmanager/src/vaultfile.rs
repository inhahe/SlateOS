//! The vault on disk.
//!
//! Until 2026-09-27 the credential manager kept nothing: every entry was gone
//! when the window closed, and the window said so in red. The vault is now one
//! file, `<config>/credmanager/vault`, encrypted under a key made from the
//! master password with the vetted primitives `rustcrypto/seal` wraps
//! (`design-decisions.md` §539, §1218): Argon2id makes the key, and
//! XChaCha20-Poly1305 encrypts -- and refuses to open anything that has been
//! altered by so much as a bit.
//!
//! **The file:**
//!
//! | bytes | what |
//! |---|---|
//! | 8 | `SLVAULT` and the format number, `1` |
//! | 12 | how the key is made: Argon2id's memory in KiB, passes and lanes, three little-endian `u32`s |
//! | 16 | the salt |
//! | 24 | the nonce, new for every save |
//! | rest | the vault's contents, encrypted, and the 16-byte tag |
//!
//! Everything before the contents is bound to them as associated data, so a
//! header edited to weaker parameters or another salt opens nothing. The
//! master password is never written, nor anything that would check a guess
//! more cheaply than opening the file does. A wrong password and a changed
//! file are one answer -- the key does not open it -- because any way of
//! telling them apart would tell an attacker the same.
//!
//! **The contents** are text, one record a line, fields separated by tabs and
//! escaped with `textfmt::tsv`, so a password may hold any character at all:
//!
//! ```text
//! # SlateOS vault, format 1
//! vault   <name>  <auto-lock minutes>  <next id>
//! folder  <id>  <parent id, or empty>  <name>
//! login   <common>  <site>  <username>  <password>  <url>  <notes>
//! note    <common>  <title>  <content>
//! card    <common>  <name>  <number>  <expiry>  <cardholder>  <notes>
//! identity <common>  <name>  <email>  <phone>  <address>
//! sshkey  <common>  <name>  <fingerprint>  <public key>
//! totp    <entry id>  <secret>
//! tag     <entry id>  <tag>
//! ```
//!
//! `<common>` is `<id> <folder id, or empty> <starred> <compromised>
//! <created> <modified>`, the two flags `0` or `1` and the times in seconds.
//! The contents are read whole or not at all: a record that is not understood
//! refuses the file, because saving what was understood would throw the rest
//! away.

use crate::{
    CreditCardData, Entry, EntryData, Folder, IdentityData, LoginData, SecureNoteData, SshKeyData,
    Vault,
};

/// The file's first seven bytes.
const MAGIC: &[u8; 7] = b"SLVAULT";
/// The one format this program writes and reads.
const FORMAT: u8 = 1;
/// Where the salt starts: after the magic, the format and three `u32`s.
const SALT_AT: usize = 8 + 12;
/// Where the nonce starts.
const NONCE_AT: usize = SALT_AT + seal::SALT_LEN;
/// Bytes before the sealed contents.
pub const HEADER_LEN: usize = NONCE_AT + seal::NONCE_LEN;
/// The first line of the contents.
const CONTENTS_HEADER: &str = "# SlateOS vault, format 1";

/// The most key derivation a file may ask for.
///
/// A vault file names the Argon2id parameters its key is made with, and
/// opening one does that work before anything else can be checked -- so a
/// file that asked for a terabyte, or a million passes, would stop the program
/// at the lock screen. Nothing this program writes comes near these; they are
/// four times RFC 9106's largest recommendation (2 GiB, one pass) in memory,
/// and far past any sane number of passes or lanes.
const MAX_MEMORY_KIB: u32 = 8 * 1024 * 1024;
const MAX_ITERATIONS: u32 = 64;
const MAX_LANES: u32 = 64;

/// What makes the key, and the nonce the contents were sealed with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kdf: seal::KdfParams,
    pub salt: [u8; seal::SALT_LEN],
    pub nonce: seal::Nonce,
}

impl Header {
    /// The header as the file holds it.
    #[must_use]
    pub fn to_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(MAGIC);
        out.push(FORMAT);
        out.extend_from_slice(&self.kdf.memory_kib.to_le_bytes());
        out.extend_from_slice(&self.kdf.iterations.to_le_bytes());
        out.extend_from_slice(&self.kdf.lanes.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out
    }

    /// Read a file's header, before any key is made.
    ///
    /// # Errors
    ///
    /// Not a vault, a format this program does not know, cut short, or
    /// parameters past what it will spend on one unlock.
    pub fn parse(bytes: &[u8]) -> Result<Self, OpenError> {
        if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err(OpenError::NotAVault);
        }
        match bytes.get(MAGIC.len()) {
            Some(&FORMAT) => {}
            Some(&other) => return Err(OpenError::UnknownFormat(other)),
            None => return Err(OpenError::Truncated),
        }
        let header = bytes.get(..HEADER_LEN).ok_or(OpenError::Truncated)?;
        let word = |at: usize| -> Result<u32, OpenError> {
            header
                .get(at..at.saturating_add(4))
                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                .map(u32::from_le_bytes)
                .ok_or(OpenError::Truncated)
        };
        let kdf = seal::KdfParams {
            memory_kib: word(8)?,
            iterations: word(12)?,
            lanes: word(16)?,
        };
        if kdf.memory_kib > MAX_MEMORY_KIB
            || kdf.iterations > MAX_ITERATIONS
            || kdf.lanes > MAX_LANES
        {
            return Err(OpenError::TooCostly);
        }
        let salt = header
            .get(SALT_AT..NONCE_AT)
            .and_then(|b| <[u8; seal::SALT_LEN]>::try_from(b).ok())
            .ok_or(OpenError::Truncated)?;
        let nonce = header
            .get(NONCE_AT..HEADER_LEN)
            .and_then(|b| <[u8; seal::NONCE_LEN]>::try_from(b).ok())
            .ok_or(OpenError::Truncated)?;
        Ok(Self { kdf, salt, nonce })
    }
}

/// Why a vault file did not open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The file does not start the way a vault does.
    NotAVault,
    /// A vault in a newer format than this program reads.
    UnknownFormat(u8),
    /// The file ends before its contents begin.
    Truncated,
    /// The file asks for more work to make its key than any vault this
    /// program writes; see [`MAX_MEMORY_KIB`].
    TooCostly,
    /// The key made from this password does not open the file: the password
    /// is not the one it was saved with, or the file has been changed since.
    WrongPasswordOrChanged,
    /// Not enough memory to make the key.
    OutOfMemory,
    /// Its header names parameters no key can be made with -- no passes, no
    /// lanes, too little memory: it was not written by this program as it is.
    Damaged,
    /// The file opened, and what is inside is not a vault this program reads.
    Contents(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAVault => f.write_str("it is not a vault file"),
            Self::UnknownFormat(n) => {
                write!(f, "it is a vault in format {n}, newer than this program reads")
            }
            Self::Truncated => f.write_str("it is cut short"),
            Self::TooCostly => {
                f.write_str("it asks for far more work to unlock than any vault this program writes")
            }
            Self::WrongPasswordOrChanged => f.write_str(
                "that is not its master password -- or the file has been changed since it was saved",
            ),
            Self::OutOfMemory => f.write_str("there is not enough memory to unlock it"),
            Self::Damaged => f.write_str("its header is damaged"),
            Self::Contents(why) => write!(f, "what is inside is not a vault: {why}"),
        }
    }
}

/// The key `password` makes under `kdf` and `salt`.
///
/// # Errors
///
/// [`OpenError::OutOfMemory`], or [`OpenError::Damaged`] for parameters
/// Argon2 refuses.
pub fn key_for(
    password: &str,
    kdf: seal::KdfParams,
    salt: &[u8; seal::SALT_LEN],
) -> Result<seal::Key, OpenError> {
    seal::derive_key(password.as_bytes(), salt, kdf).map_err(|e| match e {
        seal::Error::OutOfMemory => OpenError::OutOfMemory,
        _ => OpenError::Damaged,
    })
}

/// The file's bytes: `header`, then `contents` sealed under `key` with the
/// header as associated data.
///
/// # Errors
///
/// The contents are past what one nonce can seal (256 GiB).
pub fn seal_file(key: &seal::Key, header: &Header, contents: &str) -> Result<Vec<u8>, seal::Error> {
    let head = header.to_bytes();
    let sealed = seal::encrypt(key, &header.nonce, &head, contents.as_bytes())?;
    let mut out = head;
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// The contents' text, opened with `key`.
///
/// # Errors
///
/// The header does not read, or the key does not open the file.
pub fn open_file(bytes: &[u8], key: &seal::Key) -> Result<String, OpenError> {
    let header = Header::parse(bytes)?;
    let (head, sealed) = bytes
        .split_at_checked(HEADER_LEN)
        .ok_or(OpenError::Truncated)?;
    let plain = seal::decrypt(key, &header.nonce, head, sealed)
        .map_err(|_| OpenError::WrongPasswordOrChanged)?;
    // Authentic, so this program wrote it: text that is not UTF-8 would be a
    // bug here, not an attack, and is still refused rather than guessed at.
    String::from_utf8(plain).map_err(|_| OpenError::Contents("it is not text".to_string()))
}

// ============================================================================
// The contents
// ============================================================================

/// What a vault's contents hold.
#[derive(Clone, Debug)]
pub struct Contents {
    pub name: String,
    pub auto_lock_minutes: u32,
    pub next_id: u64,
    pub folders: Vec<Folder>,
    pub entries: Vec<Entry>,
}

fn esc(s: &str) -> String {
    textfmt::tsv::escape(s)
}

fn flag(b: bool) -> &'static str {
    if b { "1" } else { "0" }
}

/// The contents' text for `vault`.
#[must_use]
pub fn contents_text(vault: &Vault) -> String {
    let mut out = String::from(CONTENTS_HEADER);
    out.push('\n');
    out.push_str(&format!(
        "vault\t{}\t{}\t{}\n",
        esc(&vault.name),
        vault.auto_lock_minutes,
        vault.id_gen
    ));
    for folder in &vault.folders {
        out.push_str(&format!(
            "folder\t{}\t{}\t{}\n",
            folder.id,
            folder.parent_id.map(|p| p.to_string()).unwrap_or_default(),
            esc(&folder.name)
        ));
    }
    for entry in &vault.entries {
        let common = format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            entry.id,
            entry.folder_id.map(|f| f.to_string()).unwrap_or_default(),
            flag(entry.starred),
            flag(entry.compromised),
            entry.created_at,
            entry.modified_at
        );
        match &entry.data {
            EntryData::Login(d) => {
                out.push_str(&format!(
                    "login\t{common}\t{}\t{}\t{}\t{}\t{}\n",
                    esc(&d.site),
                    esc(&d.username),
                    esc(&d.password),
                    esc(&d.url),
                    esc(&d.notes)
                ));
                if let Some(secret) = &d.totp_secret {
                    out.push_str(&format!("totp\t{}\t{}\n", entry.id, esc(secret)));
                }
            }
            EntryData::SecureNote(d) => {
                out.push_str(&format!(
                    "note\t{common}\t{}\t{}\n",
                    esc(&d.title),
                    esc(&d.content)
                ));
            }
            EntryData::CreditCard(d) => {
                out.push_str(&format!(
                    "card\t{common}\t{}\t{}\t{}\t{}\t{}\n",
                    esc(&d.name),
                    esc(&d.number_masked),
                    esc(&d.expiry),
                    esc(&d.cardholder),
                    esc(&d.notes)
                ));
            }
            EntryData::Identity(d) => {
                out.push_str(&format!(
                    "identity\t{common}\t{}\t{}\t{}\t{}\n",
                    esc(&d.name),
                    esc(&d.email),
                    esc(&d.phone),
                    esc(&d.address)
                ));
            }
            EntryData::SshKey(d) => {
                out.push_str(&format!(
                    "sshkey\t{common}\t{}\t{}\t{}\n",
                    esc(&d.name),
                    esc(&d.fingerprint),
                    esc(&d.public_key)
                ));
            }
        }
        for tag in &entry.tags {
            out.push_str(&format!("tag\t{}\t{}\n", entry.id, esc(tag)));
        }
    }
    out
}

/// Read the contents' text back.
///
/// # Errors
///
/// Which line is not understood. The whole of it is refused.
pub fn parse_contents(text: &str) -> Result<Contents, String> {
    let mut lines = text.lines().enumerate();
    if lines.next().map(|(_, l)| l) != Some(CONTENTS_HEADER) {
        return Err("it does not begin as a vault's contents do".to_string());
    }
    let mut contents: Option<Contents> = None;
    for (n, line) in lines {
        let bad = |why: &str| format!("line {}: {why}", n.saturating_add(1));
        let fields: Vec<&str> = line.split('\t').collect();
        let text = |i: usize| -> Result<String, String> {
            let raw = fields.get(i).ok_or_else(|| bad("a field is missing"))?;
            textfmt::tsv::unescape(raw).ok_or_else(|| bad("a field is not escaped as written"))
        };
        let number = |i: usize| -> Result<u64, String> {
            fields
                .get(i)
                .and_then(|f| f.parse::<u64>().ok())
                .ok_or_else(|| bad("a number is not a number"))
        };
        let optional = |i: usize| -> Result<Option<u64>, String> {
            match fields.get(i) {
                Some(&"") => Ok(None),
                Some(f) => f
                    .parse::<u64>()
                    .map(Some)
                    .map_err(|_| bad("an id is not a number")),
                None => Err(bad("a field is missing")),
            }
        };
        let boolean = |i: usize| -> Result<bool, String> {
            match fields.get(i) {
                Some(&"0") => Ok(false),
                Some(&"1") => Ok(true),
                _ => Err(bad("a flag is not 0 or 1")),
            }
        };
        let expect = |count: usize| -> Result<(), String> {
            if fields.len() == count {
                Ok(())
            } else {
                Err(bad("the record has the wrong number of fields"))
            }
        };
        let kind = fields.first().copied().unwrap_or_default();
        if kind == "vault" {
            expect(4)?;
            if contents.is_some() {
                return Err(bad("a second vault record"));
            }
            contents = Some(Contents {
                name: text(1)?,
                auto_lock_minutes: u32::try_from(number(2)?)
                    .map_err(|_| bad("the auto-lock time is out of range"))?,
                next_id: number(3)?,
                folders: Vec::new(),
                entries: Vec::new(),
            });
            continue;
        }
        let c = contents
            .as_mut()
            .ok_or_else(|| bad("a record before the vault's own"))?;
        let common = |c: &Contents, data: EntryData| -> Result<Entry, String> {
            let id = number(1)?;
            if c.entries.iter().any(|e| e.id == id) {
                return Err(bad("two entries have one id"));
            }
            let folder_id = optional(2)?;
            if let Some(f) = folder_id
                && !c.folders.iter().any(|folder| folder.id == f)
            {
                return Err(bad("an entry is in a folder the vault does not have"));
            }
            Ok(Entry {
                id,
                data,
                folder_id,
                tags: Vec::new(),
                starred: boolean(3)?,
                compromised: boolean(4)?,
                created_at: number(5)?,
                modified_at: number(6)?,
            })
        };
        match kind {
            "folder" => {
                expect(4)?;
                let id = number(1)?;
                if c.folders.iter().any(|f| f.id == id) {
                    return Err(bad("two folders have one id"));
                }
                c.folders.push(Folder {
                    id,
                    parent_id: optional(2)?,
                    name: text(3)?,
                });
            }
            "login" => {
                expect(12)?;
                let data = EntryData::Login(LoginData {
                    site: text(7)?,
                    username: text(8)?,
                    password: text(9)?,
                    url: text(10)?,
                    notes: text(11)?,
                    totp_secret: None,
                });
                let entry = common(c, data)?;
                c.entries.push(entry);
            }
            "note" => {
                expect(9)?;
                let data = EntryData::SecureNote(SecureNoteData {
                    title: text(7)?,
                    content: text(8)?,
                });
                let entry = common(c, data)?;
                c.entries.push(entry);
            }
            "card" => {
                expect(12)?;
                let data = EntryData::CreditCard(CreditCardData {
                    name: text(7)?,
                    number_masked: text(8)?,
                    expiry: text(9)?,
                    cardholder: text(10)?,
                    notes: text(11)?,
                });
                let entry = common(c, data)?;
                c.entries.push(entry);
            }
            "identity" => {
                expect(11)?;
                let data = EntryData::Identity(IdentityData {
                    name: text(7)?,
                    email: text(8)?,
                    phone: text(9)?,
                    address: text(10)?,
                });
                let entry = common(c, data)?;
                c.entries.push(entry);
            }
            "sshkey" => {
                expect(10)?;
                let data = EntryData::SshKey(SshKeyData {
                    name: text(7)?,
                    fingerprint: text(8)?,
                    public_key: text(9)?,
                });
                let entry = common(c, data)?;
                c.entries.push(entry);
            }
            "totp" => {
                expect(3)?;
                let id = number(1)?;
                let secret = text(2)?;
                match c
                    .entries
                    .iter_mut()
                    .find(|e| e.id == id)
                    .map(|e| &mut e.data)
                {
                    Some(EntryData::Login(d)) if d.totp_secret.is_none() => {
                        d.totp_secret = Some(secret);
                    }
                    _ => return Err(bad("a one-time-code secret for no login, or a second one")),
                }
            }
            "tag" => {
                expect(3)?;
                let id = number(1)?;
                let tag = text(2)?;
                let entry = c
                    .entries
                    .iter_mut()
                    .find(|e| e.id == id)
                    .ok_or_else(|| bad("a tag for an entry the vault does not have"))?;
                entry.tags.push(tag);
            }
            _ => return Err(bad("a record this program does not know")),
        }
    }
    let contents = contents.ok_or_else(|| "it has no vault record".to_string())?;
    if contents.folders.iter().any(|f| {
        f.parent_id
            .is_some_and(|p| !contents.folders.iter().any(|g| g.id == p))
    }) {
        return Err("a folder is inside a folder the vault does not have".to_string());
    }
    let highest = contents
        .entries
        .iter()
        .map(|e| e.id)
        .chain(contents.folders.iter().map(|f| f.id))
        .max()
        .unwrap_or(0);
    if contents.next_id <= highest {
        return Err("the next id is one already in use".to_string());
    }
    Ok(contents)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    const KDF: seal::KdfParams = seal::KdfParams {
        memory_kib: 8,
        iterations: 1,
        lanes: 1,
    };

    fn header() -> Header {
        Header {
            kdf: KDF,
            salt: [3; seal::SALT_LEN],
            nonce: [4; seal::NONCE_LEN],
        }
    }

    #[test]
    fn a_header_reads_back_as_it_was_written() {
        let bytes = header().to_bytes();
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(Header::parse(&bytes).unwrap(), header());
    }

    #[test]
    fn a_file_that_is_not_a_vault_says_what_it_is_instead() {
        assert_eq!(
            Header::parse(b"my shopping list"),
            Err(OpenError::NotAVault)
        );
        assert_eq!(Header::parse(b""), Err(OpenError::NotAVault));
        let mut newer = header().to_bytes();
        newer[7] = 2;
        assert_eq!(Header::parse(&newer), Err(OpenError::UnknownFormat(2)));
        let whole = header().to_bytes();
        assert_eq!(
            Header::parse(&whole[..HEADER_LEN - 1]),
            Err(OpenError::Truncated)
        );
        assert_eq!(Header::parse(b"SLVAULT"), Err(OpenError::Truncated));
    }

    #[test]
    fn a_file_asking_too_much_work_is_refused_before_any_is_done() {
        for kdf in [
            seal::KdfParams {
                memory_kib: MAX_MEMORY_KIB + 1,
                ..KDF
            },
            seal::KdfParams {
                iterations: MAX_ITERATIONS + 1,
                ..KDF
            },
            seal::KdfParams {
                lanes: MAX_LANES + 1,
                ..KDF
            },
        ] {
            let bytes = Header { kdf, ..header() }.to_bytes();
            assert_eq!(Header::parse(&bytes), Err(OpenError::TooCostly), "{kdf:?}");
        }
        let most = seal::KdfParams {
            memory_kib: MAX_MEMORY_KIB,
            iterations: MAX_ITERATIONS,
            lanes: MAX_LANES,
        };
        assert!(
            Header::parse(
                &Header {
                    kdf: most,
                    ..header()
                }
                .to_bytes()
            )
            .is_ok()
        );
    }

    #[test]
    fn parameters_no_key_can_be_made_with_are_a_damaged_header() {
        for kdf in [
            seal::KdfParams {
                iterations: 0,
                ..KDF
            },
            seal::KdfParams { lanes: 0, ..KDF },
            seal::KdfParams {
                memory_kib: 7,
                ..KDF
            },
        ] {
            assert_eq!(
                key_for("pw", kdf, &[3; seal::SALT_LEN]),
                Err(OpenError::Damaged),
                "{kdf:?}"
            );
        }
    }

    #[test]
    fn sealed_contents_open_with_their_key_and_nothing_else() {
        let key = key_for("password", KDF, &[3; seal::SALT_LEN]).unwrap();
        let bytes = seal_file(&key, &header(), "the contents").unwrap();
        assert_eq!(open_file(&bytes, &key).unwrap(), "the contents");
        let other = key_for("passworD", KDF, &[3; seal::SALT_LEN]).unwrap();
        assert_eq!(
            open_file(&bytes, &other),
            Err(OpenError::WrongPasswordOrChanged)
        );
        let salted = key_for("password", KDF, &[9; seal::SALT_LEN]).unwrap();
        assert_eq!(
            open_file(&bytes, &salted),
            Err(OpenError::WrongPasswordOrChanged)
        );
    }

    #[test]
    fn a_change_to_any_byte_of_the_file_is_refused() {
        let key = key_for("password", KDF, &[3; seal::SALT_LEN]).unwrap();
        let bytes = seal_file(&key, &header(), "the contents").unwrap();
        for i in 0..bytes.len() {
            let mut bent = bytes.clone();
            bent[i] ^= 0x01;
            assert!(
                open_file(&bent, &key).is_err(),
                "a change at byte {i} opened"
            );
        }
        assert!(open_file(&bytes[..bytes.len() - 1], &key).is_err());
    }

    #[test]
    fn the_contents_are_not_in_the_file_in_the_clear() {
        let key = key_for("password", KDF, &[3; seal::SALT_LEN]).unwrap();
        let bytes = seal_file(&key, &header(), "hunter2 is my password").unwrap();
        assert!(!bytes.windows(7).any(|w| w == b"hunter2"));
    }
}
