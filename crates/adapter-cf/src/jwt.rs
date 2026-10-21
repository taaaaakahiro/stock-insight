//! Clerk のセッショントークン（JWT, RS256）の解析とクレームの確認。
//! 署名の検証そのものは Web Crypto で行う（clerk.rs）。ここは暗号に依存しない部分だけ。

use serde::Deserialize;

/// base64url（パディングなし）をデコードする
pub fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32)
    }
    let bytes = s.trim_end_matches('=').as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c)? << (18 - 6 * i as u32);
        }
        let n = chunk.len() - 1;
        for i in 0..n {
            out.push((acc >> (16 - 8 * i as u32)) as u8);
        }
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Header {
    pub alg: String,
    pub kid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Claims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub azp: Option<String>,
    pub exp: Option<i64>,
    pub nbf: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub header: Header,
    pub claims: Claims,
    /// 署名の対象（"<header>.<payload>"）
    pub signing_input: String,
    pub signature: Vec<u8>,
}

pub fn parse(token: &str) -> Result<Parsed, String> {
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("malformed token".into());
    };
    let decode = |x: &str| base64url_decode(x).ok_or_else(|| "bad base64".to_string());
    let header: Header =
        serde_json::from_slice(&decode(h)?).map_err(|_| "bad header".to_string())?;
    let claims: Claims =
        serde_json::from_slice(&decode(p)?).map_err(|_| "bad payload".to_string())?;
    Ok(Parsed {
        header,
        claims,
        signing_input: format!("{h}.{p}"),
        signature: decode(s)?,
    })
}

/// 時計のずれの許容（秒）
const LEEWAY: i64 = 5;

/// 発行元・発行先・有効期限を確認し、ユーザーID（sub）を返す
pub fn check_claims(
    c: &Claims,
    issuer: &str,
    authorized_parties: &[String],
    now: i64,
) -> Result<String, String> {
    if c.iss.as_deref() != Some(issuer) {
        return Err("unexpected issuer".into());
    }
    match c.exp {
        Some(exp) if now <= exp + LEEWAY => {}
        _ => return Err("token expired".into()),
    }
    if c.nbf.is_some_and(|nbf| now + LEEWAY < nbf) {
        return Err("token not yet valid".into());
    }
    // Clerk は発行先のオリジンを azp に入れる。別サイト向けのトークンは受け付けない
    if !c
        .azp
        .as_ref()
        .is_some_and(|a| authorized_parties.iter().any(|p| p == a))
    {
        return Err("token was issued for another origin".into());
    }
    c.sub
        .clone()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "token has no subject".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(s: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in s.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |acc, (i, &b)| acc | (b as u32) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
        out
    }

    #[test]
    fn base64url_roundtrip() {
        for s in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
            &[0xff, 0xfe, 0x00],
        ] {
            assert_eq!(base64url_decode(&b64(s)).unwrap(), s);
        }
        assert_eq!(base64url_decode("Zm9vYg==").unwrap(), b"foob");
        assert!(base64url_decode("a").is_none());
        assert!(base64url_decode("a+b/").is_none());
    }

    fn token(payload: &str) -> String {
        format!(
            "{}.{}.{}",
            b64(br#"{"alg":"RS256","kid":"ins_1","typ":"JWT"}"#),
            b64(payload.as_bytes()),
            b64(b"sig")
        )
    }

    #[test]
    fn parses_token() {
        let p = parse(&token(
            r#"{"sub":"user_1","iss":"https://clerk.example.com","exp":100}"#,
        ))
        .unwrap();
        assert_eq!(p.header.alg, "RS256");
        assert_eq!(p.header.kid.as_deref(), Some("ins_1"));
        assert_eq!(p.claims.sub.as_deref(), Some("user_1"));
        assert_eq!(p.signature, b"sig");
        assert!(p.signing_input.matches('.').count() == 1);
        assert!(parse("a.b").is_err());
        assert!(parse("a.b.c.d").is_err());
    }

    #[test]
    fn checks_claims() {
        let iss = "https://clerk.example.com";
        let parties = vec!["https://app.example.com".to_string()];
        let ok = Claims {
            sub: Some("user_1".into()),
            iss: Some(iss.into()),
            azp: Some("https://app.example.com".into()),
            exp: Some(1000),
            nbf: Some(900),
        };
        assert_eq!(check_claims(&ok, iss, &parties, 950).unwrap(), "user_1");
        assert!(check_claims(&ok, iss, &parties, 1000 + LEEWAY + 1).is_err());
        assert!(check_claims(&ok, iss, &parties, 800).is_err());
        assert!(check_claims(&ok, "https://evil.example.com", &parties, 950).is_err());
        let other = Claims {
            azp: Some("https://evil.example.com".into()),
            ..ok.clone()
        };
        assert!(check_claims(&other, iss, &parties, 950).is_err());
        let no_exp = Claims {
            exp: None,
            ..ok.clone()
        };
        assert!(check_claims(&no_exp, iss, &parties, 950).is_err());
        let no_sub = Claims { sub: None, ..ok };
        assert!(check_claims(&no_sub, iss, &parties, 950).is_err());
    }
}
