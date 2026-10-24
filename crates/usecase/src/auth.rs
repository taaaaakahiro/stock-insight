//! 本人確認（F1）。トークンを検証し、許可リストで使える人かを判定する。
//! Clerk への問い合わせは1ユーザー1時間に1回までにし、結果は users に保存する。

use crate::{Clock, PortError};
use domain::access::{decide_access, Access, DirectoryUser, Permission, Role};
use std::cell::RefCell;
use std::collections::HashMap;

/// Clerk の確認結果を使い回す時間（秒）
pub const RECHECK_AFTER_SECS: i64 = 3600;

/// 認証済みのユーザー（API の処理に渡す）
#[derive(Debug, Clone, PartialEq)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
}

impl AuthUser {
    pub fn role(&self) -> Role {
        Role::from_admin(self.is_admin)
    }

    pub fn can(&self, permission: Permission) -> bool {
        self.role().allows(permission)
    }
}

/// users テーブルの1行
#[derive(Debug, Clone, PartialEq)]
pub struct StoredUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub allowed: bool,
    pub is_admin: bool,
    pub checked_at: i64,
}

impl StoredUser {
    fn to_auth(&self) -> AuthUser {
        AuthUser {
            id: self.id.clone(),
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            is_admin: self.is_admin,
        }
    }
}

/// ログイン記録（拒否も残す）
#[derive(Debug, Clone, PartialEq)]
pub struct LoginEvent {
    pub user_id: String,
    pub email: Option<String>,
    pub occurred_at: i64,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub result: &'static str,
}

/// リクエストの付帯情報（ログイン記録用）
#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    /// トークンがない・不正・期限切れ（401）
    Unauthenticated(String),
    /// 本人確認はできたが使えない人（403）
    Forbidden(Access),
    /// Clerk や D1 に問い合わせられない（503）
    Unavailable(String),
}

pub trait TokenVerifier {
    /// 署名・発行元・有効期限・発行先を確認し、ユーザーID（sub）を返す
    async fn verify(&self, token: &str, now: i64) -> Result<String, String>;
}

pub trait UserDirectory {
    async fn fetch_user(&self, id: &str) -> Result<DirectoryUser, PortError>;
}

pub trait UserRepository {
    async fn find(&self, id: &str) -> Result<Option<StoredUser>, PortError>;
    async fn save(&self, user: &StoredUser) -> Result<(), PortError>;
    async fn record_login(&self, event: &LoginEvent) -> Result<(), PortError>;
}

#[allow(clippy::too_many_arguments)]
pub async fn authenticate<V, D, R, C>(
    token: Option<&str>,
    meta: &RequestMeta,
    admin_email: Option<&str>,
    verifier: &V,
    directory: &D,
    repo: &R,
    clock: &C,
) -> Result<AuthUser, AuthError>
where
    V: TokenVerifier,
    D: UserDirectory,
    R: UserRepository,
    C: Clock,
{
    let token = token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AuthError::Unauthenticated("missing token".into()))?;
    let now = clock.now_unix();
    let id = verifier
        .verify(token, now)
        .await
        .map_err(AuthError::Unauthenticated)?;

    let stored = repo
        .find(&id)
        .await
        .map_err(|e| AuthError::Unavailable(e.0))?;
    if let Some(s) = &stored {
        if now - s.checked_at < RECHECK_AFTER_SECS {
            return result_of(s);
        }
    }

    let user = match directory.fetch_user(&id).await {
        Ok(u) => u,
        // Clerk に問い合わせられないときは、前回の結果があればそれを使う
        Err(e) => {
            return match &stored {
                Some(s) => result_of(s),
                None => Err(AuthError::Unavailable(e.0)),
            }
        }
    };
    let access = decide_access(&user, admin_email);
    let record = StoredUser {
        id: user.id.clone(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        allowed: access.is_allowed(),
        is_admin: matches!(access, Access::Allowed { is_admin: true }),
        checked_at: now,
    };
    repo.save(&record)
        .await
        .map_err(|e| AuthError::Unavailable(e.0))?;
    // 記録の失敗でログインを止めない
    let _ = repo
        .record_login(&LoginEvent {
            user_id: user.id,
            email: Some(user.email),
            occurred_at: now,
            ip_address: meta.ip_address.clone(),
            user_agent: meta.user_agent.clone(),
            result: access.as_str(),
        })
        .await;
    match access {
        Access::Allowed { .. } => Ok(record.to_auth()),
        denied => Err(AuthError::Forbidden(denied)),
    }
}

fn result_of(s: &StoredUser) -> Result<AuthUser, AuthError> {
    if s.allowed {
        Ok(s.to_auth())
    } else {
        Err(AuthError::Forbidden(Access::DeniedNotListed))
    }
}

/// users の行を Worker の isolate のメモリに持つ入れ物
pub type UserMemo = RefCell<HashMap<String, StoredUser>>;

/// メモリに持つ人数の上限（超えたら一度空にする。使う人は許可リストの数人）
const MEMO_MAX_USERS: usize = 256;

/// users の読み取りをメモリで省く。確認から1時間以内の行があれば D1 を読まない
pub struct CachedUsers<'a, R> {
    inner: &'a R,
    memo: &'a UserMemo,
    now: i64,
}

impl<'a, R> CachedUsers<'a, R> {
    pub fn new(inner: &'a R, memo: &'a UserMemo, now: i64) -> Self {
        Self { inner, memo, now }
    }

    fn remember(&self, user: &StoredUser) {
        let mut m = self.memo.borrow_mut();
        if m.len() >= MEMO_MAX_USERS && !m.contains_key(&user.id) {
            m.clear();
        }
        m.insert(user.id.clone(), user.clone());
    }
}

impl<R: UserRepository> UserRepository for CachedUsers<'_, R> {
    async fn find(&self, id: &str) -> Result<Option<StoredUser>, PortError> {
        let fresh = self
            .memo
            .borrow()
            .get(id)
            .filter(|u| self.now - u.checked_at < RECHECK_AFTER_SECS)
            .cloned();
        if fresh.is_some() {
            return Ok(fresh);
        }
        let found = self.inner.find(id).await?;
        if let Some(u) = &found {
            self.remember(u);
        }
        Ok(found)
    }

    async fn save(&self, user: &StoredUser) -> Result<(), PortError> {
        self.inner.save(user).await?;
        self.remember(user);
        Ok(())
    }

    async fn record_login(&self, event: &LoginEvent) -> Result<(), PortError> {
        self.inner.record_login(event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::block_on;
    use std::cell::Cell;

    struct Verifier;
    impl TokenVerifier for Verifier {
        async fn verify(&self, token: &str, _now: i64) -> Result<String, String> {
            token
                .strip_prefix("ok:")
                .map(str::to_string)
                .ok_or_else(|| "bad signature".into())
        }
    }

    struct Directory {
        flagged: bool,
        google: bool,
        down: bool,
        calls: Cell<u32>,
    }
    impl Directory {
        fn new(flagged: bool, google: bool) -> Self {
            Self {
                flagged,
                google,
                down: false,
                calls: Cell::new(0),
            }
        }
    }
    impl UserDirectory for Directory {
        async fn fetch_user(&self, id: &str) -> Result<DirectoryUser, PortError> {
            self.calls.set(self.calls.get() + 1);
            if self.down {
                return Err(PortError("clerk down".into()));
            }
            Ok(DirectoryUser {
                id: id.into(),
                email: format!("{id}@example.com"),
                display_name: id.into(),
                google_verified: self.google,
                flagged: self.flagged,
            })
        }
    }

    #[derive(Default)]
    struct Repo {
        users: RefCell<Vec<StoredUser>>,
        events: RefCell<Vec<LoginEvent>>,
        finds: Cell<u32>,
    }
    impl UserRepository for Repo {
        async fn find(&self, id: &str) -> Result<Option<StoredUser>, PortError> {
            self.finds.set(self.finds.get() + 1);
            Ok(self.users.borrow().iter().find(|u| u.id == id).cloned())
        }
        async fn save(&self, user: &StoredUser) -> Result<(), PortError> {
            let mut us = self.users.borrow_mut();
            us.retain(|u| u.id != user.id);
            us.push(user.clone());
            Ok(())
        }
        async fn record_login(&self, event: &LoginEvent) -> Result<(), PortError> {
            self.events.borrow_mut().push(event.clone());
            Ok(())
        }
    }

    struct At(i64);
    impl Clock for At {
        fn now_unix(&self) -> i64 {
            self.0
        }
    }

    fn run(
        token: Option<&str>,
        dir: &Directory,
        repo: &Repo,
        now: i64,
    ) -> Result<AuthUser, AuthError> {
        block_on(authenticate(
            token,
            &RequestMeta::default(),
            Some("admin@example.com"),
            &Verifier,
            dir,
            repo,
            &At(now),
        ))
    }

    #[test]
    fn rejects_missing_or_bad_token() {
        let (dir, repo) = (Directory::new(true, true), Repo::default());
        assert!(matches!(
            run(None, &dir, &repo, 0),
            Err(AuthError::Unauthenticated(_))
        ));
        assert!(matches!(
            run(Some("x"), &dir, &repo, 0),
            Err(AuthError::Unauthenticated(_))
        ));
        assert_eq!(dir.calls.get(), 0);
    }

    #[test]
    fn allows_flagged_user_and_caches_for_an_hour() {
        let (dir, repo) = (Directory::new(true, true), Repo::default());
        let u = run(Some("ok:alice"), &dir, &repo, 1000).unwrap();
        assert_eq!(u.email, "alice@example.com");
        assert!(!u.is_admin);
        run(Some("ok:alice"), &dir, &repo, 1000 + RECHECK_AFTER_SECS - 1).unwrap();
        assert_eq!(dir.calls.get(), 1);
        run(Some("ok:alice"), &dir, &repo, 1000 + RECHECK_AFTER_SECS).unwrap();
        assert_eq!(dir.calls.get(), 2);
        assert_eq!(repo.events.borrow().len(), 2);
        assert_eq!(repo.events.borrow()[0].result, "allowed");
    }

    #[test]
    fn admin_does_not_need_flag() {
        let (dir, repo) = (Directory::new(false, true), Repo::default());
        assert!(run(Some("ok:admin"), &dir, &repo, 0).unwrap().is_admin);
    }

    #[test]
    fn denies_unlisted_and_non_google_and_records_it() {
        let repo = Repo::default();
        assert_eq!(
            run(Some("ok:bob"), &Directory::new(false, true), &repo, 0),
            Err(AuthError::Forbidden(Access::DeniedNotListed))
        );
        assert_eq!(
            run(Some("ok:carol"), &Directory::new(true, false), &repo, 0),
            Err(AuthError::Forbidden(Access::DeniedNotGoogle))
        );
        let ev = repo.events.borrow();
        assert_eq!(
            ev.iter().map(|e| e.result).collect::<Vec<_>>(),
            ["denied_not_listed", "denied_not_google"]
        );
    }

    #[test]
    fn falls_back_to_stale_result_when_clerk_is_down() {
        let repo = Repo::default();
        run(Some("ok:alice"), &Directory::new(true, true), &repo, 0).unwrap();
        let mut down = Directory::new(true, true);
        down.down = true;
        assert!(run(Some("ok:alice"), &down, &repo, RECHECK_AFTER_SECS * 5).is_ok());
        assert!(matches!(
            run(Some("ok:new"), &down, &repo, 0),
            Err(AuthError::Unavailable(_))
        ));
    }

    fn run_cached(memo: &UserMemo, dir: &Directory, repo: &Repo, now: i64) -> AuthUser {
        block_on(authenticate(
            Some("ok:alice"),
            &RequestMeta::default(),
            None,
            &Verifier,
            dir,
            &CachedUsers::new(repo, memo, now),
            &At(now),
        ))
        .unwrap()
    }

    #[test]
    fn memo_skips_d1_reads_until_recheck() {
        let (dir, repo, memo) = (
            Directory::new(true, true),
            Repo::default(),
            UserMemo::default(),
        );
        run_cached(&memo, &dir, &repo, 1000);
        run_cached(&memo, &dir, &repo, 1000 + RECHECK_AFTER_SECS - 1);
        assert_eq!(repo.finds.get(), 1, "2回目はメモリから");
        assert_eq!(dir.calls.get(), 1);
        run_cached(&memo, &dir, &repo, 1000 + RECHECK_AFTER_SECS);
        assert_eq!(repo.finds.get(), 2, "1時間たったら D1 を読み直す");
        assert_eq!(dir.calls.get(), 2);
    }

    #[test]
    fn memo_uses_rows_another_isolate_saved() {
        let (dir, repo, memo) = (
            Directory::new(true, true),
            Repo::default(),
            UserMemo::default(),
        );
        run(Some("ok:alice"), &dir, &repo, 1000).unwrap();
        run_cached(&memo, &dir, &repo, 1001);
        assert_eq!(dir.calls.get(), 1, "D1 の新しい行があれば Clerk に聞かない");
        assert!(memo.borrow().contains_key("alice"));
    }
}
