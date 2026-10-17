//! The real [`Accounts`], [`Sessions`] and [`Launcher`]: the account
//! database, logind over the bus, and the kernel.

use std::path::{Path, PathBuf};

use libservicebus::{Connection, Outcome};
use loginusers::Account;

use crate::manager::{Accounts, Launcher, Person, SEAT, Sessions};

/// Say `what` on standard error, which init keeps.
pub fn say(what: &str) {
    eprintln!("loginmgr: {what}");
}

/// The account database at a path: `/etc/users.yaml`, or a test's.
pub struct Database {
    path: PathBuf,
}

impl Database {
    /// The database at `path`.
    #[must_use]
    pub fn at(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }
}

impl Accounts for Database {
    fn offered(&self) -> Vec<Account> {
        loginusers::offered(&self.path)
    }

    fn person(&self, user: &str) -> Option<Person> {
        let db = match userdb::UserDb::load(&self.path) {
            Ok(db) => db,
            Err(e) => {
                say(&format!("cannot read {}: {e}", self.path.display()));
                return None;
            }
        };
        let r = db.find(user)?;
        Some(Person {
            username: user.to_owned(),
            uid: r.uid()?,
            gid: r.gid()?,
            home: r.home().unwrap_or_else(|| "/".to_owned()),
            shell: r.shell().unwrap_or_else(|| "/bin/sh".to_owned()),
        })
    }

    /// Under the database's hold (`userdb::UserDb::lock`), the file is read
    /// as it is now and only this account's login time and count change
    /// before it is saved, so nothing another tool changed since is undone.
    fn signed_in(&self, user: &str, at: u64) {
        let count = || -> std::io::Result<()> {
            let _hold = userdb::UserDb::lock(&self.path)?;
            let mut db = userdb::UserDb::load(&self.path)?;
            let Some(record) = db.find_mut(user) else {
                // Gone since it was checked: nothing to count it against.
                return Ok(());
            };
            record.record_login(at);
            db.save(&self.path)
        };
        if let Err(e) = count() {
            say(&format!("{user}'s sign-in was not recorded: {e}"));
        }
    }

    /// Unreadable, no one: signing someone in by themselves from a database
    /// that cannot be read would be signing in an account it may not say.
    fn automatic(&self) -> Option<String> {
        match userdb::UserDb::load(&self.path) {
            Ok(db) => db.automatic_account().and_then(userdb::Record::username),
            Err(e) => {
                say(&format!("cannot read {}: {e}", self.path.display()));
                None
            }
        }
    }
}

/// logind's service name (`userspace/logind`, `bus::SERVICE_NAME`).
pub const LOGIND: &str = "system.logind";

/// How long a call to logind may take.
const LOGIND_TIMEOUT_NS: u64 = 10_000_000_000;

/// logind, over the bus. A connection per call: `loginmgr` makes three a
/// session, and a connection held between them would be one more thing to
/// find broken when logind restarts.
pub struct Logind;

impl Logind {
    /// One call; its answer, or the words for why there is none.
    fn call(method: &str, args: &[&[u8]]) -> Result<Vec<Vec<u8>>, String> {
        let mut conn = Connection::connect(LOGIND)
            .map_err(|e| format!("cannot reach the session manager: {e:?}"))?;
        match conn.call_fields(method, args, LOGIND_TIMEOUT_NS) {
            Ok(Outcome::Done(answer)) => Ok(answer),
            Ok(Outcome::Refused { error, .. }) => {
                Err(format!("the session manager refused {method}: {error}"))
            }
            Err(e) => Err(format!(
                "the session manager did not answer {method}: {e:?}"
            )),
        }
    }
}

impl Sessions for Logind {
    fn create(&mut self, person: &Person) -> Result<String, String> {
        let uid = person.uid.to_string();
        // uid, user, type, class, seat, vtnr, tty, remote, remote-host,
        // service, desktop, leader pid -- the leader named once the desktop
        // has one (`set_leader`). `other` is logind's `unspecified`: a
        // SlateOS desktop is neither X11 nor Wayland.
        let answer = Self::call(
            "CreateSession",
            &[
                uid.as_bytes(),
                person.username.as_bytes(),
                b"other",
                b"user",
                SEAT.as_bytes(),
                b"0",
                b"",
                b"0",
                b"",
                b"loginmgr",
                b"slate",
                b"0",
            ],
        )?;
        let [id] = answer.as_slice() else {
            return Err("the session manager gave no session id".to_owned());
        };
        String::from_utf8(id.clone())
            .map_err(|_| "the session manager gave a session id that is not text".to_owned())
    }

    fn set_leader(&mut self, id: &str, pid: u32) {
        if let Err(why) = Self::call(
            "SetSessionLeader",
            &[id.as_bytes(), pid.to_string().as_bytes()],
        ) {
            say(&format!("session {id} has no leader: {why}"));
        }
    }

    fn end(&mut self, id: &str) {
        if let Err(why) = Self::call("TerminateSession", &[id.as_bytes()]) {
            say(&format!("session {id} was not ended: {why}"));
        }
    }
}

/// The login screen's program: lane C's `greeter`.
pub const LOGIN_SCREEN: &str = "/bin/greeter";

/// The account the login screen runs as.
pub const LOGIN_SCREEN_ACCOUNT: &str = "greeter";

/// Why no program is started yet: see [`KernelLauncher`].
const CANNOT_START_AS_ANOTHER_USER: &str = "the kernel cannot yet start a program as another user \
     holding only the capabilities it is given (requests/b-a-start-a-program-as-a-user-holding-only-what-it-is-given.md)";

/// Starts the login screen and the desktops through the kernel.
///
/// Each must run as its own user and hold only what it is given: the
/// desktop the display service's key, the login screen nothing of
/// `loginmgr`'s. `fork`, `setuid` and `exec` would start them as their users
/// holding every capability `loginmgr` holds -- `Service/0/w` among them,
/// which registers any service name -- because a child keeps its parent's
/// table and giving up root does not take `Service` rights away. The kernel
/// call that starts a program with exactly the capabilities named
/// (`SYS_PROCESS_SPAWN_EX2`) starts it as the caller. Until it can also name
/// the user (lane A's, requested), this starts nothing and says why, rather
/// than hand `loginmgr`'s capabilities to both.
pub struct KernelLauncher;

impl Launcher for KernelLauncher {
    fn start_login_screen(&mut self, _automatic: Option<&str>) -> Result<u32, String> {
        Err(CANNOT_START_AS_ANOTHER_USER.to_owned())
    }

    fn start_desktop(
        &mut self,
        _person: &Person,
        _env: &[(String, String)],
    ) -> Result<u32, String> {
        Err(CANNOT_START_AS_ANOTHER_USER.to_owned())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_person_is_the_account_as_the_database_holds_it_now() {
        let dir = scratchdir::ScratchDir::new("loginmgr-person");
        let path = dir.path("users.yaml");
        let mut file = userdb::UserDb::new();
        let mut ada = userdb::Record::new();
        ada.set("username", "ada");
        ada.set_uid(1000);
        ada.set_gid(100);
        ada.set_home("/home/ada");
        ada.set("shell", "/bin/zsh");
        file.push(ada);
        file.save(&path).unwrap();
        let db = Database::at(&path);
        assert_eq!(
            db.person("ada"),
            Some(Person {
                username: "ada".to_owned(),
                uid: 1000,
                gid: 100,
                home: "/home/ada".to_owned(),
                shell: "/bin/zsh".to_owned(),
            })
        );
        assert_eq!(db.person("nobody"), None);
        db.signed_in("ada", 1_700_000_000);
        let after = userdb::UserDb::load(&path).unwrap();
        assert_eq!(after.find("ada").unwrap().last_login(), 1_700_000_000);
    }

    #[test]
    fn the_automatic_account_is_the_database_s_answer() {
        let dir = scratchdir::ScratchDir::new("loginmgr-automatic");
        let path = dir.path("users.yaml");
        let mut file = userdb::UserDb::new();
        let mut ada = userdb::Record::new();
        ada.set("username", "ada");
        ada.set_uid(1000);
        file.push(ada);
        file.save(&path).unwrap();
        let db = Database::at(&path);
        assert_eq!(db.automatic(), None);
        file.find_mut("ada").unwrap().set_auto_login(true);
        file.save(&path).unwrap();
        assert_eq!(db.automatic(), Some("ada".to_owned()));
        assert_eq!(Database::at(&dir.path("missing.yaml")).automatic(), None);
    }
}
