//! `<nss.h>` — Name Service Switch (NSS) status and database constants.
//!
//! NSS provides a uniform interface for looking up system databases
//! (passwd, group, hosts, etc.) from multiple sources (files, LDAP,
//! NIS). These constants define return statuses and database IDs.

// ---------------------------------------------------------------------------
// NSS status codes (enum nss_status)
// ---------------------------------------------------------------------------

/// Try next source.
pub const NSS_STATUS_TRYAGAIN: i32 = -2;
/// Entry not found in this source.
pub const NSS_STATUS_NOTFOUND: i32 = 0;
/// Entry found successfully.
pub const NSS_STATUS_SUCCESS: i32 = 1;
/// Source unavailable.
pub const NSS_STATUS_UNAVAIL: i32 = -1;
/// Return (stop looking).
pub const NSS_STATUS_RETURN: i32 = -3;

// ---------------------------------------------------------------------------
// NSS database identifiers
// ---------------------------------------------------------------------------

/// Password database.
pub const NSS_DB_PASSWD: u32 = 0;
/// Group database.
pub const NSS_DB_GROUP: u32 = 1;
/// Shadow password database.
pub const NSS_DB_SHADOW: u32 = 2;
/// Hosts database.
pub const NSS_DB_HOSTS: u32 = 3;
/// Networks database.
pub const NSS_DB_NETWORKS: u32 = 4;
/// Protocols database.
pub const NSS_DB_PROTOCOLS: u32 = 5;
/// Services database.
pub const NSS_DB_SERVICES: u32 = 6;
/// Ethers (MAC address) database.
pub const NSS_DB_ETHERS: u32 = 7;
/// RPC database.
pub const NSS_DB_RPC: u32 = 8;
/// Aliases (mail) database.
pub const NSS_DB_ALIASES: u32 = 9;
/// Netgroup database.
pub const NSS_DB_NETGROUP: u32 = 10;
/// gshadow database.
pub const NSS_DB_GSHADOW: u32 = 11;

// ---------------------------------------------------------------------------
// NSS action keywords
// ---------------------------------------------------------------------------

/// Action: return result.
pub const NSS_ACTION_RETURN: u32 = 0;
/// Action: continue to next source.
pub const NSS_ACTION_CONTINUE: u32 = 1;
/// Action: merge results.
pub const NSS_ACTION_MERGE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_codes_distinct() {
        let statuses = [
            NSS_STATUS_TRYAGAIN, NSS_STATUS_UNAVAIL,
            NSS_STATUS_NOTFOUND, NSS_STATUS_SUCCESS,
            NSS_STATUS_RETURN,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn test_success_is_positive() {
        assert!(NSS_STATUS_SUCCESS > 0);
    }

    #[test]
    fn test_notfound_is_zero() {
        assert_eq!(NSS_STATUS_NOTFOUND, 0);
    }

    #[test]
    fn test_databases_distinct() {
        let dbs = [
            NSS_DB_PASSWD, NSS_DB_GROUP, NSS_DB_SHADOW,
            NSS_DB_HOSTS, NSS_DB_NETWORKS, NSS_DB_PROTOCOLS,
            NSS_DB_SERVICES, NSS_DB_ETHERS, NSS_DB_RPC,
            NSS_DB_ALIASES, NSS_DB_NETGROUP, NSS_DB_GSHADOW,
        ];
        for i in 0..dbs.len() {
            for j in (i + 1)..dbs.len() {
                assert_ne!(dbs[i], dbs[j]);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [NSS_ACTION_RETURN, NSS_ACTION_CONTINUE, NSS_ACTION_MERGE];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }
}
