//! Clerk との接続（wasm32 のみ）。
//! - ClerkVerifier: セッショントークンの署名を公開鍵（JWKS）と Web Crypto で検証する
//! - ClerkDirectory: Backend API からユーザーを取得する

use crate::clerk_user::{is_valid_user_id, parse_user};
use crate::jwt::{check_claims, parse};
use domain::access::DirectoryUser;
use serde::Deserialize;
use std::cell::RefCell;
use usecase::auth::{TokenVerifier, UserDirectory};
use usecase::PortError;
use worker::js_sys::{Array, Object, Reflect, Uint8Array, JSON};
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::wasm_bindgen_futures::JsFuture;
use worker::{Fetch, Headers, Request, RequestInit};

/// 公開鍵を使い回す時間（秒）。知らない kid が来たら期限前でも取り直す
const JWKS_TTL_SECS: i64 = 3600;

#[derive(Debug, Clone, Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

thread_local! {
    // Worker の isolate 内で共有するキャッシュ（取得時刻, 鍵）
    static JWKS: RefCell<Option<(i64, Vec<Jwk>)>> = const { RefCell::new(None) };
}

pub struct ClerkVerifier {
    issuer: String,
    authorized_parties: Vec<String>,
}

impl ClerkVerifier {
    pub fn new(issuer: &str, authorized_parties: Vec<String>) -> Self {
        Self {
            issuer: issuer.trim_end_matches('/').to_string(),
            authorized_parties,
        }
    }

    async fn key(&self, kid: &str, now: i64) -> Result<Jwk, String> {
        let cached = JWKS.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(at, _)| now - at < JWKS_TTL_SECS)
                .and_then(|(_, keys)| keys.iter().find(|k| k.kid == kid).cloned())
        });
        if let Some(k) = cached {
            return Ok(k);
        }
        let url = format!("{}/.well-known/jwks.json", self.issuer);
        let body = get(&url, None).await?;
        let set: JwkSet = serde_json::from_slice(&body).map_err(|e| format!("jwks: {e}"))?;
        let found = set.keys.iter().find(|k| k.kid == kid).cloned();
        JWKS.with(|c| *c.borrow_mut() = Some((now, set.keys)));
        found.ok_or_else(|| "unknown signing key".into())
    }
}

impl TokenVerifier for ClerkVerifier {
    async fn verify(&self, token: &str, now: i64) -> Result<String, String> {
        let p = parse(token)?;
        if p.header.alg != "RS256" {
            return Err("unsupported alg".into());
        }
        let kid = p.header.kid.as_deref().ok_or("token has no kid")?;
        let jwk = self.key(kid, now).await?;
        if jwk.kty != "RSA" {
            return Err("unsupported key type".into());
        }
        if !rs256_verify(&jwk, p.signing_input.as_bytes(), &p.signature).await? {
            return Err("bad signature".into());
        }
        check_claims(&p.claims, &self.issuer, &self.authorized_parties, now)
    }
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| {
            Reflect::get(&e, &"message".into())
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| "crypto error".into())
}

async fn rs256_verify(jwk: &Jwk, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    let crypto: worker::web_sys::Crypto = Reflect::get(&worker::js_sys::global(), &"crypto".into())
        .map_err(js_err)?
        .unchecked_into();
    let subtle = crypto.subtle();
    let jwk_json =
        serde_json::json!({ "kty": "RSA", "n": jwk.n, "e": jwk.e, "alg": "RS256", "ext": true });
    let key_data: Object = JSON::parse(&jwk_json.to_string())
        .map_err(js_err)?
        .unchecked_into();
    let algorithm: Object = JSON::parse(r#"{"name":"RSASSA-PKCS1-v1_5","hash":"SHA-256"}"#)
        .map_err(js_err)?
        .unchecked_into();
    let usages = Array::of1(&"verify".into());
    let key = JsFuture::from(
        subtle
            .import_key_with_object("jwk", &key_data, &algorithm, false, &usages)
            .map_err(js_err)?,
    )
    .await
    .map_err(js_err)?;
    let ok = JsFuture::from(
        subtle
            .verify_with_str_and_js_u8_array_and_js_u8_array(
                "RSASSA-PKCS1-v1_5",
                &key.unchecked_into(),
                &Uint8Array::from(signature),
                &Uint8Array::from(data),
            )
            .map_err(js_err)?,
    )
    .await
    .map_err(js_err)?;
    Ok(ok.as_bool() == Some(true))
}

/// GET してボディを返す。bearer はヘッダーで渡す（URL には入れない）
async fn get(url: &str, bearer: Option<&str>) -> Result<Vec<u8>, String> {
    let headers = Headers::new();
    headers
        .set("Accept", "application/json")
        .map_err(|e| e.to_string())?;
    if let Some(b) = bearer {
        headers
            .set("Authorization", &format!("Bearer {b}"))
            .map_err(|e| e.to_string())?;
    }
    let mut init = RequestInit::new();
    init.with_headers(headers);
    let req = Request::new_with_init(url, &init).map_err(|e| e.to_string())?;
    let mut res = Fetch::Request(req)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status_code();
    if !(200..300).contains(&status) {
        // 応答の本文は出さない（個人情報を含みうるため）
        return Err(format!(
            "HTTP {status} from {}",
            url.split('?').next().unwrap_or(url)
        ));
    }
    res.bytes().await.map_err(|e| e.to_string())
}

pub struct ClerkDirectory {
    api_base: String,
    secret_key: String,
}

impl ClerkDirectory {
    pub fn new(api_base: &str, secret_key: String) -> Self {
        Self {
            api_base: api_base.trim_end_matches('/').to_string(),
            secret_key,
        }
    }
}

impl UserDirectory for ClerkDirectory {
    async fn fetch_user(&self, id: &str) -> Result<DirectoryUser, PortError> {
        if !is_valid_user_id(id) {
            return Err(PortError("invalid user id".into()));
        }
        let body = get(
            &format!("{}/users/{id}", self.api_base),
            Some(&self.secret_key),
        )
        .await
        .map_err(|e| PortError(format!("clerk: {e}")))?;
        parse_user(&body).map_err(PortError)
    }
}
