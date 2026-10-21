//! users と login_events の読み書き（UserRepository の実装）。

use serde::Deserialize;
use usecase::auth::{LoginEvent, StoredUser, UserRepository};
use usecase::PortError;
use worker::wasm_bindgen::JsValue;
use worker::D1Database;

pub struct D1Users {
    db: D1Database,
}

impl D1Users {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

fn err(e: worker::Error) -> PortError {
    PortError(format!("d1: {e}"))
}

fn text(s: &str) -> JsValue {
    JsValue::from_str(s)
}

fn opt_text(s: &Option<String>) -> JsValue {
    s.as_deref().map_or(JsValue::NULL, JsValue::from_str)
}

fn int(i: i64) -> JsValue {
    JsValue::from_f64(i as f64)
}

#[derive(Deserialize)]
struct Row {
    id: String,
    email: String,
    display_name: String,
    allowed: i64,
    is_admin: i64,
    checked_at: i64,
}

impl UserRepository for D1Users {
    async fn find(&self, id: &str) -> Result<Option<StoredUser>, PortError> {
        let row: Option<Row> = self
            .db
            .prepare("SELECT id, email, display_name, allowed, is_admin, checked_at FROM users WHERE id = ?")
            .bind(&[text(id)])
            .map_err(err)?
            .first(None)
            .await
            .map_err(err)?;
        Ok(row.map(|r| StoredUser {
            id: r.id,
            email: r.email,
            display_name: r.display_name,
            allowed: r.allowed != 0,
            is_admin: r.is_admin != 0,
            checked_at: r.checked_at,
        }))
    }

    async fn save(&self, u: &StoredUser) -> Result<(), PortError> {
        self.db
            .prepare(
                "INSERT INTO users (id, email, display_name, allowed, is_admin, checked_at, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET email = excluded.email, display_name = excluded.display_name, \
                 allowed = excluded.allowed, is_admin = excluded.is_admin, checked_at = excluded.checked_at",
            )
            .bind(&[
                text(&u.id),
                text(&u.email),
                text(&u.display_name),
                int(u.allowed as i64),
                int(u.is_admin as i64),
                int(u.checked_at),
                int(u.checked_at),
            ])
            .map_err(err)?
            .run()
            .await
            .map_err(err)?;
        Ok(())
    }

    async fn record_login(&self, e: &LoginEvent) -> Result<(), PortError> {
        self.db
            .prepare(
                "INSERT INTO login_events (user_id, email, occurred_at, ip_address, user_agent, result) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&[
                text(&e.user_id),
                opt_text(&e.email),
                int(e.occurred_at),
                opt_text(&e.ip_address),
                opt_text(&e.user_agent),
                text(e.result),
            ])
            .map_err(err)?
            .run()
            .await
            .map_err(err)?;
        Ok(())
    }
}
