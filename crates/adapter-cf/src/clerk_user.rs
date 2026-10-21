//! Clerk Backend API のユーザー（GET /v1/users/{id}）を、内側の DirectoryUser に変換する。

use domain::access::DirectoryUser;
use serde::Deserialize;

#[derive(Deserialize)]
struct Verification {
    status: Option<String>,
}

#[derive(Deserialize)]
struct EmailAddress {
    id: String,
    email_address: String,
    verification: Option<Verification>,
}

#[derive(Deserialize)]
struct ExternalAccount {
    provider: Option<String>,
    email_address: Option<String>,
    verification: Option<Verification>,
}

#[derive(Deserialize)]
struct User {
    id: String,
    first_name: Option<String>,
    last_name: Option<String>,
    username: Option<String>,
    primary_email_address_id: Option<String>,
    #[serde(default)]
    email_addresses: Vec<EmailAddress>,
    #[serde(default)]
    external_accounts: Vec<ExternalAccount>,
    #[serde(default)]
    public_metadata: serde_json::Value,
}

fn verified(v: &Option<Verification>) -> bool {
    v.as_ref().and_then(|v| v.status.as_deref()) == Some("verified")
}

/// public_metadata に {"stock_insight": true} があれば利用可
pub const FLAG_KEY: &str = "stock_insight";

pub fn parse_user(body: &[u8]) -> Result<DirectoryUser, String> {
    let u: User = serde_json::from_slice(body).map_err(|e| format!("clerk user: {e}"))?;
    let primary = u
        .email_addresses
        .iter()
        .find(|e| Some(&e.id) == u.primary_email_address_id.as_ref())
        .filter(|e| verified(&e.verification))
        .ok_or("clerk user has no verified primary email")?;
    let email = primary.email_address.clone();
    let google_verified = u.external_accounts.iter().any(|a| {
        matches!(a.provider.as_deref(), Some("oauth_google") | Some("google"))
            && verified(&a.verification)
            && a.email_address
                .as_deref()
                .is_some_and(|x| x.eq_ignore_ascii_case(&email))
    });
    let name = [u.first_name.as_deref(), u.last_name.as_deref()]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let display_name = if !name.is_empty() {
        name
    } else {
        u.username
            .clone()
            .unwrap_or_else(|| email.split('@').next().unwrap_or_default().to_string())
    };
    Ok(DirectoryUser {
        id: u.id,
        email,
        display_name,
        google_verified,
        flagged: u.public_metadata[FLAG_KEY].as_bool() == Some(true),
    })
}

/// Clerk のユーザーIDとして安全な文字だけか（URL のパスに入れるため）
pub fn is_valid_user_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = r#"{
      "id": "user_2abc",
      "first_name": "Taro", "last_name": "Yamada", "username": null,
      "primary_email_address_id": "idn_1",
      "email_addresses": [
        {"id": "idn_0", "email_address": "old@example.com", "verification": {"status": "verified"}},
        {"id": "idn_1", "email_address": "taro@example.com", "verification": {"status": "verified"}}
      ],
      "external_accounts": [
        {"provider": "oauth_google", "email_address": "Taro@example.com", "verification": {"status": "verified"}}
      ],
      "public_metadata": {"stock_insight": true}
    }"#;

    #[test]
    fn parses_google_user() {
        let u = parse_user(USER.as_bytes()).unwrap();
        assert_eq!(u.email, "taro@example.com");
        assert_eq!(u.display_name, "Taro Yamada");
        assert!(u.google_verified && u.flagged);
    }

    #[test]
    fn email_only_user_is_not_google() {
        let body = USER
            .replace("oauth_google", "oauth_github")
            .replace("true}", "\"yes\"}");
        let u = parse_user(body.as_bytes()).unwrap();
        assert!(!u.google_verified);
        assert!(!u.flagged, "flag must be the boolean true");
    }

    #[test]
    fn google_account_for_another_address_does_not_count() {
        let body = USER.replace("Taro@example.com", "someone@gmail.com");
        assert!(!parse_user(body.as_bytes()).unwrap().google_verified);
    }

    #[test]
    fn unverified_primary_email_is_rejected() {
        let body = USER.replacen(
            r#""taro@example.com", "verification": {"status": "verified"}"#,
            r#""taro@example.com", "verification": {"status": "unverified"}"#,
            1,
        );
        assert!(parse_user(body.as_bytes()).is_err());
    }

    #[test]
    fn user_id_validation() {
        assert!(is_valid_user_id("user_2abcDEF123"));
        assert!(!is_valid_user_id("../users"));
        assert!(!is_valid_user_id(""));
    }
}
