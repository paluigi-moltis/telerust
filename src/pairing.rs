use crate::config::PairedUser;
use tracing::trace;

pub fn is_paired_user(
    paired: &PairedUser,
    sender_id: Option<i64>,
    sender_username: Option<&str>,
) -> bool {
    if let Some(paired_id) = paired.user_id {
        if let Some(sid) = sender_id {
            return sid == paired_id;
        }
        trace!("Paired user_id configured but sender has no user_id");
        return false;
    }

    if let Some(ref paired_name) = paired.username {
        if let Some(sender_name) = sender_username {
            return sender_name.eq_ignore_ascii_case(paired_name);
        }
        trace!("Paired username configured but sender has no username");
        return false;
    }

    trace!("No paired user configured — rejecting message");
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PairedUser;

    #[test]
    fn test_user_id_match() {
        let paired = PairedUser {
            username: Some("alice".to_string()),
            user_id: Some(12345),
        };
        assert!(is_paired_user(&paired, Some(12345), Some("alice")));
    }

    #[test]
    fn test_user_id_mismatch_ignores_username() {
        let paired = PairedUser {
            username: Some("alice".to_string()),
            user_id: Some(12345),
        };
        assert!(!is_paired_user(&paired, Some(99999), Some("alice")));
    }

    #[test]
    fn test_username_fallback_when_no_user_id_configured() {
        let paired = PairedUser {
            username: Some("alice".to_string()),
            user_id: None,
        };
        assert!(is_paired_user(&paired, Some(99999), Some("alice")));
    }

    #[test]
    fn test_no_match() {
        let paired = PairedUser {
            username: Some("alice".to_string()),
            user_id: Some(12345),
        };
        assert!(!is_paired_user(&paired, Some(99999), Some("bob")));
    }

    #[test]
    fn test_no_paired_user_configured() {
        let paired = PairedUser {
            username: None,
            user_id: None,
        };
        assert!(!is_paired_user(&paired, Some(12345), Some("alice")));
    }
}
