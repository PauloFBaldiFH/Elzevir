// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded KEI transport and Ed25519 signatures, independent of host effects.
use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use sha2::{Digest as _, Sha256};
use std::{io::Read as _, time::Duration};

pub(crate) fn origin(value: &str) -> Result<String, String> {
    let uri = value
        .parse::<ureq::http::Uri>()
        .map_err(|_| "invalid broker URL")?;
    if uri.scheme_str() != Some("https")
        || uri
            .authority()
            .is_none_or(|host| host.as_str().contains('@'))
        || !matches!(uri.path(), "" | "/")
        || uri.query().is_some()
        || value.contains('#')
        || value.len() > 2048
    {
        return Err(
            "broker requires an HTTPS origin without credentials, path, query or fragment"
                .to_owned(),
        );
    }
    Ok(format!(
        "https://{}",
        uri.authority().ok_or("broker authority required")?
    ))
}

pub(crate) fn capabilities(bytes: &[u8]) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid broker capabilities")?;
    if !value
        .get("services")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|services| services.iter().any(|name| name.as_str() == Some("elzevir")))
        || value
            .pointer("/elzevir/contract")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
        || value
            .pointer("/elzevir/configurable_origin")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
    {
        return Err("B1: broker does not support Elzevir with a configurable origin".to_owned());
    }
    Ok(())
}

pub(crate) struct Key(Ed25519KeyPair);
impl Key {
    pub(crate) fn from_seed(mut seed: [u8; 32]) -> Result<Self, String> {
        use zeroize::Zeroize as _;
        let result = Ed25519KeyPair::from_seed_unchecked(&seed)
            .map(Self)
            .map_err(|_| "invalid address signing key".to_owned());
        seed.zeroize();
        result
    }
    pub(crate) fn public(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self.0.public_key().as_ref())
    }
    pub(crate) fn headers(
        &self,
        library: &str,
        method: &str,
        path: &str,
        body: &[u8],
        at: i64,
        nonce: &str,
    ) -> Result<Vec<(&'static str, String)>, String> {
        if library.is_empty()
            || library.len() > 64
            || !library
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !(16..=64).contains(&nonce.len())
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !(0..=999_999_999_999).contains(&at)
            || !valid_request(method, path)
        {
            return Err("invalid signed address request".to_owned());
        }
        let message = canonical(method, path, body, at, nonce);
        Ok(vec![
            ("X-KEI-Library", library.to_owned()),
            ("X-KEI-Timestamp", at.to_string()),
            ("X-KEI-Nonce", nonce.to_owned()),
            (
                "X-KEI-Signature",
                base64::engine::general_purpose::STANDARD
                    .encode(self.0.sign(message.as_bytes()).as_ref()),
            ),
        ])
    }
}
fn canonical(method: &str, path: &str, body: &[u8], at: i64, nonce: &str) -> String {
    format!(
        "KEI-SIG-v1\n{}\n{path}\n{at}\n{nonce}\n{:x}",
        method.to_ascii_uppercase(),
        Sha256::digest(body)
    )
}
fn valid_request(method: &str, path: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "DELETE"
    ) && path.starts_with("/v1/")
        && path.len() <= 256
        && path
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'#' | b'\\'))
}

pub(crate) struct Response {
    pub status: u16,
    pub body: zeroize::Zeroizing<Vec<u8>>,
}
impl Response {
    pub(crate) fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        if !(200..300).contains(&self.status) {
            return Err(format!(
                "address service refused the request (HTTP {})",
                self.status
            ));
        }
        serde_json::from_slice(&self.body)
            .map_err(|_| "invalid address service response".to_owned())
    }
}
pub(crate) trait Transport {
    fn request(
        &self,
        base: &str,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<Response, String>;
}
pub(crate) struct Http;
impl Transport for Http {
    fn request(
        &self,
        base: &str,
        method: &str,
        path: &str,
        headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<Response, String> {
        if !valid_request(method, path) || body.len() > 16_384 {
            return Err("invalid address request".to_owned());
        }
        let url = format!("{}{path}", origin(base)?);
        let agent = crate::http_client::secure_agent(0, Duration::from_secs(30))?;
        let mut request = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("User-Agent", "Elzevir-address/1")
            .header("Content-Type", "application/json");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let request = request.body(body).map_err(|_| "invalid address request")?;
        let mut response = agent
            .run(request)
            .map_err(|_| "address service unavailable")?;
        let status = response.status().as_u16();
        let mut body = zeroize::Zeroizing::new(Vec::new());
        response
            .body_mut()
            .as_reader()
            .take(65_537)
            .read_to_end(&mut body)
            .map_err(|_| "address response unavailable")?;
        if body.len() > 65_536 {
            return Err("address response exceeds limit".to_owned());
        }
        Ok(Response { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_cannot_silently_publish_elzevir_as_koha() {
        for document in [r#"{"services":["koha"]}"#, r#"{"services":["elzevir"]}"#] {
            assert!(capabilities(document.as_bytes()).is_err());
        }
        capabilities(
            br#"{"services":["elzevir"],"elzevir":{"contract":1,"configurable_origin":true}}"#,
        )
        .unwrap();
    }
    #[test]
    fn signatures_bind_body_path_timestamp_nonce_and_library() {
        use ring::signature::{ED25519, KeyPair as _, UnparsedPublicKey};
        let key = Key::from_seed([7; 32]).unwrap();
        let headers = key
            .headers(
                "fictional-library",
                "put",
                "/v1/listing",
                b"{}",
                42,
                "0123456789abcdef0123456789abcdef",
            )
            .unwrap();
        let encoded = headers
            .iter()
            .find(|(name, _)| *name == "X-KEI-Signature")
            .unwrap()
            .1
            .clone();
        let signature = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let message = canonical(
            "PUT",
            "/v1/listing",
            b"{}",
            42,
            "0123456789abcdef0123456789abcdef",
        );
        UnparsedPublicKey::new(&ED25519, key.0.public_key().as_ref())
            .verify(message.as_bytes(), &signature)
            .unwrap();
        assert!(
            UnparsedPublicKey::new(&ED25519, key.0.public_key().as_ref())
                .verify(
                    canonical(
                        "PUT",
                        "/v1/listing",
                        b"[]",
                        42,
                        "0123456789abcdef0123456789abcdef"
                    )
                    .as_bytes(),
                    &signature
                )
                .is_err()
        );
        assert!(!message.ends_with('\n'));
    }
    #[test]
    fn broker_origins_never_accept_secrets_or_redirect_targets() {
        for value in [
            "http://broker.example.org",
            "https://user:secret@broker.example.org",
            "https://broker.example.org/other",
            "https://broker.example.org?token=x",
            "https://broker.example.org#x",
        ] {
            assert!(origin(value).is_err(), "{value}");
        }
        assert_eq!(
            origin("https://broker.example.org/").unwrap(),
            "https://broker.example.org"
        );
    }
}
