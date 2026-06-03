//! `<security/pam_appl.h>` — PAM (Pluggable Authentication Modules) constants.
//!
//! PAM provides a modular authentication framework. These constants
//! define return codes, message types, and item types used by PAM
//! applications and modules.

// ---------------------------------------------------------------------------
// PAM return codes
// ---------------------------------------------------------------------------

/// Successful operation.
pub const PAM_SUCCESS: u32 = 0;
/// User not known to authentication service.
pub const PAM_USER_UNKNOWN: u32 = 10;
/// Authentication failure.
pub const PAM_AUTH_ERR: u32 = 7;
/// Insufficient credentials.
pub const PAM_CRED_INSUFFICIENT: u32 = 8;
/// Authentication service cannot retrieve credentials.
pub const PAM_AUTHINFO_UNAVAIL: u32 = 9;
/// Maximum number of retries reached.
pub const PAM_MAXTRIES: u32 = 11;
/// New password is too short/simple/etc.
pub const PAM_AUTHTOK_ERR: u32 = 20;
/// Permission denied.
pub const PAM_PERM_DENIED: u32 = 6;
/// Account has expired.
pub const PAM_ACCT_EXPIRED: u32 = 13;
/// Session error.
pub const PAM_SESSION_ERR: u32 = 14;
/// Credential error.
pub const PAM_CRED_ERR: u32 = 17;
/// System error.
pub const PAM_SYSTEM_ERR: u32 = 4;
/// Conversation failure.
pub const PAM_CONV_ERR: u32 = 19;
/// Buffer error.
pub const PAM_BUF_ERR: u32 = 5;
/// Module is unknown.
pub const PAM_MODULE_UNKNOWN: u32 = 28;
/// Abort.
pub const PAM_ABORT: u32 = 26;

// ---------------------------------------------------------------------------
// PAM message styles (msg_style in pam_message)
// ---------------------------------------------------------------------------

/// Prompt for text with echo.
pub const PAM_PROMPT_ECHO_ON: u32 = 2;
/// Prompt for text without echo (password).
pub const PAM_PROMPT_ECHO_OFF: u32 = 1;
/// Error message (display to user).
pub const PAM_ERROR_MSG: u32 = 3;
/// Informational message (display to user).
pub const PAM_TEXT_INFO: u32 = 4;

// ---------------------------------------------------------------------------
// PAM item types (for pam_get_item/pam_set_item)
// ---------------------------------------------------------------------------

/// Service name.
pub const PAM_SERVICE: u32 = 1;
/// Username.
pub const PAM_USER_ITEM: u32 = 2;
/// TTY name.
pub const PAM_TTY: u32 = 3;
/// Remote host.
pub const PAM_RHOST: u32 = 4;
/// Conversation function.
pub const PAM_CONV_ITEM: u32 = 5;
/// Authentication token (password).
pub const PAM_AUTHTOK: u32 = 6;
/// Old authentication token.
pub const PAM_OLDAUTHTOK: u32 = 7;
/// Remote user.
pub const PAM_RUSER: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_return_codes_distinct() {
        let codes = [
            PAM_SUCCESS,
            PAM_SYSTEM_ERR,
            PAM_BUF_ERR,
            PAM_PERM_DENIED,
            PAM_AUTH_ERR,
            PAM_CRED_INSUFFICIENT,
            PAM_AUTHINFO_UNAVAIL,
            PAM_USER_UNKNOWN,
            PAM_MAXTRIES,
            PAM_ACCT_EXPIRED,
            PAM_SESSION_ERR,
            PAM_CRED_ERR,
            PAM_CONV_ERR,
            PAM_AUTHTOK_ERR,
            PAM_ABORT,
            PAM_MODULE_UNKNOWN,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_success_is_zero() {
        assert_eq!(PAM_SUCCESS, 0);
    }

    #[test]
    fn test_msg_styles_distinct() {
        let styles = [
            PAM_PROMPT_ECHO_OFF,
            PAM_PROMPT_ECHO_ON,
            PAM_ERROR_MSG,
            PAM_TEXT_INFO,
        ];
        for i in 0..styles.len() {
            for j in (i + 1)..styles.len() {
                assert_ne!(styles[i], styles[j]);
            }
        }
    }

    #[test]
    fn test_items_distinct() {
        let items = [
            PAM_SERVICE,
            PAM_USER_ITEM,
            PAM_TTY,
            PAM_RHOST,
            PAM_CONV_ITEM,
            PAM_AUTHTOK,
            PAM_OLDAUTHTOK,
            PAM_RUSER,
        ];
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                assert_ne!(items[i], items[j]);
            }
        }
    }
}
