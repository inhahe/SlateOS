//! The service `org.slateos.LoginManager`: its name, its calls, how their
//! arguments and answers are encoded, and a [`Client`] for the login screen.
//!
//! Both ends use this module -- `loginmgr` to answer, lane C's login screen
//! to ask -- so the encoding is written once.
//!
//! # The calls
//!
//! | Call | Arguments | Answer |
//! |---|---|---|
//! | [`LIST_ACCOUNTS`] | none | one field per account ([`encode_account`]) |
//! | [`AUTHENTICATE`] | user, password | code, seconds until another try (`u64`, little-endian), a message for the person |
//! | [`START_SESSION`] | user | nothing, or [`ERR_SESSION_NOT_STARTED`] with a message |
//! | [`START_AUTOMATIC`] | none | nothing, or [`ERR_NO_AUTOMATIC_ACCOUNT`] |
//!
//! `Authenticate`'s answer has the shape of logind's `AuthenticateSession`,
//! and its codes are logind's.
//!
//! Each call is the login screen's: `loginmgr` refuses anyone but the screen
//! it started, and root, with [`ERR_NOT_THE_LOGIN_SCREEN`].

use libservicebus::{BusError, Connection, Outcome, fields};
use loginusers::Account;

/// The service's name.
pub const SERVICE_NAME: &str = "org.slateos.LoginManager";

/// `ListAccounts()`: the accounts the screen offers, with what it shows of
/// each -- the account database's public part, which only root may read.
pub const LIST_ACCOUNTS: &str = "ListAccounts";
/// `Authenticate(user, password)`: whether `password` is `user`'s. An
/// acceptance lets this screen start `user`'s session, for a while.
pub const AUTHENTICATE: &str = "Authenticate";
/// `StartSession(user)`: `user`'s desktop started, `user` having been
/// accepted on this screen. Answered once the desktop is running, or has
/// failed to start, so the screen can say so rather than exit.
pub const START_SESSION: &str = "StartSession";
/// `StartAutomatic()`: the account marked to sign in by itself, signed in --
/// once per start of the machine, never after a log-out, never on a start for
/// repair. The call names no account: only the one `loginmgr` found marked
/// can be started this way.
pub const START_AUTOMATIC: &str = "StartAutomatic";

/// The caller is neither the login screen `loginmgr` started nor root.
pub const ERR_NOT_THE_LOGIN_SCREEN: &str = "org.slateos.LoginManager.Error.NotTheLoginScreen";
/// The arguments are not the call's.
pub const ERR_INVALID_ARGUMENTS: &str = "org.slateos.LoginManager.Error.InvalidArguments";
/// No such call.
pub const ERR_UNKNOWN_METHOD: &str = "org.slateos.LoginManager.Error.UnknownMethod";
/// `StartSession` for an account this screen has not just been accepted for.
pub const ERR_NOT_AUTHENTICATED: &str = "org.slateos.LoginManager.Error.NotAuthenticated";
/// `StartAutomatic` with no account to start: none marked, already used,
/// after a log-out, or a start for repair.
pub const ERR_NO_AUTOMATIC_ACCOUNT: &str = "org.slateos.LoginManager.Error.NoAutomaticAccount";
/// The session could not be started; the one field is a message for the
/// person.
pub const ERR_SESSION_NOT_STARTED: &str = "org.slateos.LoginManager.Error.SessionNotStarted";

/// `Authenticate`'s codes: logind's `AuthenticateSession` codes.
pub mod code {
    /// The password is the account's.
    pub const ACCEPTED: u8 = 0;
    /// It is not, or there is no such account.
    pub const REJECTED: u8 = 1;
    /// An administrator has locked the account.
    pub const LOCKED: u8 = 2;
    /// The stored entry cannot be checked: a broken system, not a wrong
    /// password.
    pub const UNUSABLE: u8 = 4;
    /// Too many wrong passwords: not now, try again after the seconds given.
    pub const TRY_LATER: u8 = 5;
}

/// What `Authenticate` answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The password is the account's: the screen may start its session.
    Accepted,
    /// It is not, or there is no such account (the two are not told apart).
    Rejected,
    /// The account is locked.
    Locked,
    /// The stored entry cannot be checked.
    Unusable,
    /// Not now: too many wrong passwords. Try again after this many seconds.
    TryLater {
        /// Seconds until another try is taken.
        after_secs: u64,
    },
}

impl Verdict {
    /// The code and the seconds until another try, as sent.
    #[must_use]
    pub const fn wire(self) -> (u8, u64) {
        match self {
            Self::Accepted => (code::ACCEPTED, 0),
            Self::Rejected => (code::REJECTED, 0),
            Self::Locked => (code::LOCKED, 0),
            Self::Unusable => (code::UNUSABLE, 0),
            Self::TryLater { after_secs } => (code::TRY_LATER, after_secs),
        }
    }

    /// The verdict a code and a delay stand for, `None` for a code that is
    /// none of these.
    #[must_use]
    pub const fn from_wire(code: u8, after_secs: u64) -> Option<Self> {
        Some(match code {
            code::ACCEPTED => Self::Accepted,
            code::REJECTED => Self::Rejected,
            code::LOCKED => Self::Locked,
            code::UNUSABLE => Self::Unusable,
            code::TRY_LATER => Self::TryLater { after_secs },
            _ => return None,
        })
    }
}

/// `Authenticate`'s answer: `verdict` and `message`, the words to show.
#[must_use]
pub fn encode_verdict(verdict: Verdict, message: &str) -> Vec<u8> {
    let (code, after) = verdict.wire();
    fields::encode(&[&[code], &after.to_le_bytes(), message.as_bytes()])
}

/// `Authenticate`'s answer read back: the verdict and the message.
#[must_use]
pub fn decode_verdict(answer: &[Vec<u8>]) -> Option<(Verdict, String)> {
    let [code, after, message] = answer else {
        return None;
    };
    let [code] = code.as_slice() else {
        return None;
    };
    let after = u64::from_le_bytes(after.as_slice().try_into().ok()?);
    let verdict = Verdict::from_wire(*code, after)?;
    Some((verdict, String::from_utf8(message.clone()).ok()?))
}

/// `"1"` or `"0"`.
fn flag(b: bool) -> &'static [u8] {
    if b { b"1" } else { b"0" }
}

/// One account as `ListAccounts` sends it: its fields, in this order --
/// username, display name, uid (empty for none), whether it has a password,
/// whether it is an administrator's, whether it signs in by itself, when it
/// last signed in (seconds since 1970; 0 for never), its avatar (empty for
/// none), whether it is locked. Flags are `"1"` and `"0"`, numbers decimal.
#[must_use]
pub fn encode_account(a: &Account) -> Vec<u8> {
    let uid = a.uid.map(|u| u.to_string()).unwrap_or_default();
    let last = a.last_login.to_string();
    fields::encode(&[
        a.username.as_bytes(),
        a.display_name.as_bytes(),
        uid.as_bytes(),
        flag(a.has_password),
        flag(a.is_admin),
        flag(a.auto_login),
        last.as_bytes(),
        a.avatar.as_deref().unwrap_or_default().as_bytes(),
        flag(a.is_locked),
    ])
}

/// An account read back from [`encode_account`]'s form.
#[must_use]
pub fn decode_account(bytes: &[u8]) -> Option<Account> {
    let f = fields::decode_exact(bytes, 9)?;
    let text = |i: usize| -> Option<String> { String::from_utf8(f.get(i)?.to_vec()).ok() };
    let truth = |i: usize| -> Option<bool> {
        match *f.get(i)? {
            b"1" => Some(true),
            b"0" => Some(false),
            _ => None,
        }
    };
    let uid = text(2)?;
    let avatar = text(7)?;
    Some(Account {
        username: text(0)?,
        display_name: text(1)?,
        uid: if uid.is_empty() {
            None
        } else {
            Some(uid.parse().ok()?)
        },
        has_password: truth(3)?,
        is_admin: truth(4)?,
        auto_login: truth(5)?,
        last_login: text(6)?.parse().ok()?,
        avatar: if avatar.is_empty() {
            None
        } else {
            Some(avatar)
        },
        is_locked: truth(8)?,
    })
}

/// `ListAccounts`' answer.
#[must_use]
pub fn encode_accounts(accounts: &[Account]) -> Vec<u8> {
    let each: Vec<Vec<u8>> = accounts.iter().map(encode_account).collect();
    let refs: Vec<&[u8]> = each.iter().map(Vec::as_slice).collect();
    fields::encode(&refs)
}

/// Why a call through [`Client`] did not give its answer.
#[derive(Debug)]
pub enum CallError {
    /// The bus failed: no such service, a closed channel, a timeout.
    Bus(BusError),
    /// `loginmgr` refused: the error's name, and its message when it sent
    /// one (only [`ERR_SESSION_NOT_STARTED`] does).
    Refused {
        /// One of the `ERR_` names.
        error: String,
        /// Words for the person, or empty.
        message: String,
    },
    /// An answer that is not this call's.
    Malformed,
}

impl From<BusError> for CallError {
    fn from(e: BusError) -> Self {
        Self::Bus(e)
    }
}

/// How long a call may take before the screen gives up: checking a password
/// is a few milliseconds of hashing, starting a desktop a program's start.
const CALL_TIMEOUT_NS: u64 = 60_000_000_000;

/// The login screen's connection to `loginmgr`.
pub struct Client {
    conn: Connection,
}

impl Client {
    /// Connect to [`SERVICE_NAME`].
    ///
    /// # Errors
    ///
    /// No such service is registered, or the connection failed.
    pub fn connect() -> Result<Self, BusError> {
        Ok(Self {
            conn: Connection::connect(SERVICE_NAME)?,
        })
    }

    /// One call: its answer's fields, or why there are none.
    fn call(&mut self, method: &str, args: &[&[u8]]) -> Result<Vec<Vec<u8>>, CallError> {
        match self.conn.call_fields(method, args, CALL_TIMEOUT_NS)? {
            Outcome::Done(answer) => Ok(answer),
            Outcome::Refused { error, fields } => {
                let message = match fields.first() {
                    None => String::new(),
                    Some(m) => String::from_utf8(m.clone()).map_err(|_| CallError::Malformed)?,
                };
                Err(CallError::Refused { error, message })
            }
        }
    }

    /// [`LIST_ACCOUNTS`].
    ///
    /// # Errors
    ///
    /// As [`CallError`] says.
    pub fn list_accounts(&mut self) -> Result<Vec<Account>, CallError> {
        self.call(LIST_ACCOUNTS, &[])?
            .iter()
            .map(|a| decode_account(a).ok_or(CallError::Malformed))
            .collect()
    }

    /// [`AUTHENTICATE`]: the verdict, and the words to show the person.
    ///
    /// # Errors
    ///
    /// As [`CallError`] says.
    pub fn authenticate(
        &mut self,
        user: &str,
        password: &[u8],
    ) -> Result<(Verdict, String), CallError> {
        let answer = self.call(AUTHENTICATE, &[user.as_bytes(), password])?;
        decode_verdict(&answer).ok_or(CallError::Malformed)
    }

    /// [`START_SESSION`].
    ///
    /// # Errors
    ///
    /// As [`CallError`] says; [`ERR_SESSION_NOT_STARTED`]'s message says why.
    pub fn start_session(&mut self, user: &str) -> Result<(), CallError> {
        self.call(START_SESSION, &[user.as_bytes()]).map(drop)
    }

    /// [`START_AUTOMATIC`].
    ///
    /// # Errors
    ///
    /// As [`CallError`] says.
    pub fn start_automatic(&mut self) -> Result<(), CallError> {
        self.call(START_AUTOMATIC, &[]).map(drop)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            uid: Some(1000),
            username: "ada".to_owned(),
            display_name: "Ada Lovelace".to_owned(),
            has_password: true,
            is_admin: false,
            auto_login: true,
            last_login: 1_700_000_000,
            avatar: Some("ada.png".to_owned()),
            is_locked: false,
        }
    }

    #[test]
    fn an_account_reads_back_as_it_was_sent() {
        let a = account();
        assert_eq!(decode_account(&encode_account(&a)), Some(a.clone()));
        let bare = Account {
            uid: None,
            avatar: None,
            has_password: false,
            is_locked: true,
            ..a
        };
        assert_eq!(decode_account(&encode_account(&bare)), Some(bare));
    }

    #[test]
    fn an_account_with_a_field_out_of_its_form_is_refused() {
        let a = account();
        let bad_flag =
            fields::encode(&[b"ada", b"Ada", b"1000", b"yes", b"0", b"0", b"0", b"", b"0"]);
        assert_eq!(decode_account(&bad_flag), None);
        let bad_uid = fields::encode(&[b"ada", b"Ada", b"x", b"1", b"0", b"0", b"0", b"", b"0"]);
        assert_eq!(decode_account(&bad_uid), None);
        let short = fields::encode(&[b"ada"]);
        assert_eq!(decode_account(&short), None);
        // And the list: every account, in order.
        let list = encode_accounts(&[a.clone(), a.clone()]);
        let each = fields::decode(&list).unwrap();
        assert_eq!(each.len(), 2);
        assert_eq!(decode_account(each[1]), Some(a));
    }

    #[test]
    fn every_verdict_reads_back_with_its_delay_and_message() {
        for v in [
            Verdict::Accepted,
            Verdict::Rejected,
            Verdict::Locked,
            Verdict::Unusable,
            Verdict::TryLater { after_secs: 30 },
        ] {
            let bytes = encode_verdict(v, "words");
            let answer: Vec<Vec<u8>> = fields::decode(&bytes)
                .unwrap()
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect();
            assert_eq!(decode_verdict(&answer), Some((v, "words".to_owned())));
        }
        // logind's codes: a screen that reads one reads the other.
        assert_eq!(Verdict::TryLater { after_secs: 7 }.wire(), (5, 7));
        assert_eq!(Verdict::from_wire(3, 0), None, "NoPassword is not answered");
        assert_eq!(decode_verdict(&[vec![0]]), None);
    }
}
