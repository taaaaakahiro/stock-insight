//! 誰がこのアプリを使えるか（F1）。認証サービスに依存しない形で判定する。

/// 利用を許可されたユーザーの区分。管理者はスーパーユーザー、それ以外は一般。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Superuser,
    General,
}

/// 全ユーザーに影響する管理操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ManageThemes,
    RegenerateRecommendations,
}

impl Role {
    pub fn from_admin(is_admin: bool) -> Self {
        if is_admin {
            Self::Superuser
        } else {
            Self::General
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Superuser => "superuser",
            Self::General => "general",
        }
    }

    pub fn allows(self, permission: Permission) -> bool {
        match permission {
            Permission::ManageThemes | Permission::RegenerateRecommendations => {
                self == Self::Superuser
            }
        }
    }
}

/// 認証サービス（Clerk）から得たユーザーの情報
#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    /// 主メールアドレスと同じアドレスの Google アカウントが連携・確認済みか
    pub google_verified: bool,
    /// 「stock-insight 利用可」の印（Clerk の public_metadata）
    pub flagged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Allowed {
        is_admin: bool,
    },
    /// Google でログインしていない
    DeniedNotGoogle,
    /// 許可リストにない
    DeniedNotListed,
}

impl Access {
    pub fn is_allowed(self) -> bool {
        matches!(self, Access::Allowed { .. })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Access::Allowed { .. } => "allowed",
            Access::DeniedNotGoogle => "denied_not_google",
            Access::DeniedNotListed => "denied_not_listed",
        }
    }
}

/// 管理者（ADMIN_NOTIFY_EMAIL と一致）は印がなくても使える。それ以外は印が必要。
/// どちらも Google で確認済みであることが前提。
pub fn decide_access(user: &DirectoryUser, admin_email: Option<&str>) -> Access {
    if !user.google_verified {
        return Access::DeniedNotGoogle;
    }
    let is_admin = admin_email
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .is_some_and(|a| a.eq_ignore_ascii_case(user.email.trim()));
    if is_admin || user.flagged {
        Access::Allowed { is_admin }
    } else {
        Access::DeniedNotListed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_superusers_can_manage_shared_content() {
        for permission in [
            Permission::ManageThemes,
            Permission::RegenerateRecommendations,
        ] {
            assert!(Role::from_admin(true).allows(permission));
            assert!(!Role::from_admin(false).allows(permission));
        }
        assert_eq!(Role::from_admin(true).as_str(), "superuser");
        assert_eq!(Role::from_admin(false).as_str(), "general");
    }

    fn user(google: bool, flagged: bool) -> DirectoryUser {
        DirectoryUser {
            id: "user_1".into(),
            email: "Someone@Example.com".into(),
            display_name: "someone".into(),
            google_verified: google,
            flagged,
        }
    }

    #[test]
    fn rules() {
        let admin = Some("someone@example.com");
        assert_eq!(
            decide_access(&user(true, false), admin),
            Access::Allowed { is_admin: true }
        );
        assert_eq!(
            decide_access(&user(true, true), Some("other@example.com")),
            Access::Allowed { is_admin: false }
        );
        assert_eq!(
            decide_access(&user(true, false), None),
            Access::DeniedNotListed
        );
        assert_eq!(
            decide_access(&user(true, false), Some(" ")),
            Access::DeniedNotListed
        );
        // 管理者でも Google 以外は拒否
        assert_eq!(
            decide_access(&user(false, true), admin),
            Access::DeniedNotGoogle
        );
    }
}
