//! 注目テーマの登録（管理者）と照合。テーマを保存するたびに、有報の「事業の内容」の文（edinet_docs.business_text）と
//! 言葉を突き合わせて stock_themes を作り直す。レコメンドと画面はその結果を読むだけ。

use crate::PortError;
use domain::theme::{validate_theme, ThemeError, ThemeInput};

/// 登録済みのテーマ
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub id: i64,
    pub name: String,
    pub words: Vec<String>,
    /// 照合で当たった銘柄数
    pub count: i64,
    pub updated_at: i64,
}

/// テーマに当たった銘柄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeStock {
    pub code: String,
    pub name: Option<String>,
    pub matched_word: String,
}

pub trait ThemeRepository {
    async fn list(&self) -> Result<Vec<Theme>, PortError>;
    async fn get(&self, id: i64) -> Result<Option<Theme>, PortError>;
    /// 新しく作って id を返す。同じ名前があれば Err
    async fn insert(&self, input: &ThemeInput, now: i64) -> Result<i64, PortError>;
    /// 名前と言葉を書き換える。なければ false
    async fn update(&self, id: i64, input: &ThemeInput, now: i64) -> Result<bool, PortError>;
    async fn delete(&self, id: i64) -> Result<(), PortError>;
    /// 照合をやり直す（stock_themes のそのテーマの行を消して、言葉ごとに事業の内容と突き合わせて入れ直す）
    async fn rematch(&self, id: i64, words: &[String]) -> Result<(), PortError>;
    async fn stocks(&self, id: i64) -> Result<Vec<ThemeStock>, PortError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThemesError {
    Invalid(ThemeError),
    NotFound,
    Port(PortError),
}

impl From<PortError> for ThemesError {
    fn from(e: PortError) -> Self {
        ThemesError::Port(e)
    }
}

pub async fn list<R: ThemeRepository>(repo: &R) -> Result<Vec<Theme>, PortError> {
    repo.list().await
}

/// 登録して照合する。返すのは id
pub async fn create<R: ThemeRepository>(
    repo: &R,
    name: &str,
    words: &[String],
    now: i64,
) -> Result<i64, ThemesError> {
    let input = validate_theme(name, words).map_err(ThemesError::Invalid)?;
    let id = repo.insert(&input, now).await?;
    repo.rematch(id, &input.words).await?;
    Ok(id)
}

/// 書き換えて照合しなおす
pub async fn update<R: ThemeRepository>(
    repo: &R,
    id: i64,
    name: &str,
    words: &[String],
    now: i64,
) -> Result<(), ThemesError> {
    let input = validate_theme(name, words).map_err(ThemesError::Invalid)?;
    if !repo.update(id, &input, now).await? {
        return Err(ThemesError::NotFound);
    }
    repo.rematch(id, &input.words).await?;
    Ok(())
}

pub async fn delete<R: ThemeRepository>(repo: &R, id: i64) -> Result<(), ThemesError> {
    if repo.get(id).await?.is_none() {
        return Err(ThemesError::NotFound);
    }
    repo.delete(id).await?;
    Ok(())
}

/// 1テーマと、当たった銘柄
pub async fn detail<R: ThemeRepository>(
    repo: &R,
    id: i64,
) -> Result<(Theme, Vec<ThemeStock>), ThemesError> {
    let Some(theme) = repo.get(id).await? else {
        return Err(ThemesError::NotFound);
    };
    let stocks = repo.stocks(id).await?;
    Ok((theme, stocks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::block_on;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        themes: RefCell<Vec<Theme>>,
        matched: RefCell<Vec<(i64, Vec<String>)>>,
    }

    impl ThemeRepository for Fake {
        async fn list(&self) -> Result<Vec<Theme>, PortError> {
            Ok(self.themes.borrow().clone())
        }
        async fn get(&self, id: i64) -> Result<Option<Theme>, PortError> {
            Ok(self.themes.borrow().iter().find(|t| t.id == id).cloned())
        }
        async fn insert(&self, input: &ThemeInput, now: i64) -> Result<i64, PortError> {
            let mut ts = self.themes.borrow_mut();
            if ts.iter().any(|t| t.name == input.name) {
                return Err(PortError("UNIQUE constraint failed".into()));
            }
            let id = ts.len() as i64 + 1;
            ts.push(Theme {
                id,
                name: input.name.clone(),
                words: input.words.clone(),
                count: 0,
                updated_at: now,
            });
            Ok(id)
        }
        async fn update(&self, id: i64, input: &ThemeInput, now: i64) -> Result<bool, PortError> {
            let mut ts = self.themes.borrow_mut();
            let Some(t) = ts.iter_mut().find(|t| t.id == id) else {
                return Ok(false);
            };
            t.name = input.name.clone();
            t.words = input.words.clone();
            t.updated_at = now;
            Ok(true)
        }
        async fn delete(&self, id: i64) -> Result<(), PortError> {
            self.themes.borrow_mut().retain(|t| t.id != id);
            Ok(())
        }
        async fn rematch(&self, id: i64, words: &[String]) -> Result<(), PortError> {
            self.matched.borrow_mut().push((id, words.to_vec()));
            Ok(())
        }
        async fn stocks(&self, _id: i64) -> Result<Vec<ThemeStock>, PortError> {
            Ok(vec![])
        }
    }

    #[test]
    fn create_validates_then_matches() {
        let repo = Fake::default();
        let id = block_on(create(
            &repo,
            " 半導体",
            &["半導体".into(), "ＳｉＣ".into(), "".into()],
            1,
        ))
        .unwrap();
        assert_eq!(id, 1);
        assert_eq!(
            repo.matched.borrow()[0],
            (1, vec!["半導体".to_string(), "SiC".to_string()]),
            "整えた言葉で照合する"
        );
        assert!(matches!(
            block_on(create(&repo, "", &["a".into()], 1)),
            Err(ThemesError::Invalid(ThemeError::EmptyName))
        ));
        assert!(
            matches!(
                block_on(create(&repo, "半導体", &["x".into()], 1)),
                Err(ThemesError::Port(_))
            ),
            "同じ名前は入れない"
        );
    }

    #[test]
    fn update_and_delete_need_an_existing_theme() {
        let repo = Fake::default();
        assert_eq!(
            block_on(update(&repo, 9, "防衛", &["防衛".into()], 1)),
            Err(ThemesError::NotFound)
        );
        assert_eq!(block_on(delete(&repo, 9)), Err(ThemesError::NotFound));
        let id = block_on(create(&repo, "防衛", &["防衛".into()], 1)).unwrap();
        block_on(update(
            &repo,
            id,
            "防衛・宇宙",
            &["防衛".into(), "宇宙".into()],
            2,
        ))
        .unwrap();
        let (t, stocks) = block_on(detail(&repo, id)).unwrap();
        assert_eq!(t.name, "防衛・宇宙");
        assert_eq!(t.updated_at, 2);
        assert!(stocks.is_empty());
        assert_eq!(repo.matched.borrow().len(), 2, "書き換えても照合しなおす");
        block_on(delete(&repo, id)).unwrap();
        assert!(block_on(list(&repo)).unwrap().is_empty());
    }
}
