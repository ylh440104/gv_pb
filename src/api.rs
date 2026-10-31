use serde::Deserialize;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::signer::AccessToken;
use crate::signer::DeviceIdentity;
use crate::signer::RequestSigner;
use crate::signer::BASE_URL;

#[derive(Debug)]
pub enum Failure {
    Expired,
    Rejected(String),
    Network(String),
    Status(u16, String),
    Decode(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Expired => write!(f, "token expired"),
            Failure::Rejected(b) => write!(f, "signature rejected: {}", b),
            Failure::Network(e) => write!(f, "network: {}", e),
            Failure::Status(s, b) => write!(f, "http {}: {}", s, b),
            Failure::Decode(e) => write!(f, "decode: {}", e),
        }
    }
}

impl std::error::Error for Failure {}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Session {
    #[serde(default, alias = "nrd_access_token")]
    pub token: String,
    #[serde(default, alias = "nrd_csrf_access_token")]
    pub csrf: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub nickname: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QrTicket {
    #[serde(default)]
    pub qr_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub expire: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QrState {
    #[serde(default)]
    pub status: i64,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub csrf: String,
    #[serde(default)]
    pub user_id: String,
}

pub struct Api {
    http: reqwest::Client,
    signer: RequestSigner,
}

impl Api {
    pub fn new(identity: DeviceIdentity) -> Result<Self, Failure> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Failure::Network(e.to_string()))?;
        let mut signer = RequestSigner::default();
        signer.identity = identity;
        Ok(Self { http, signer })
    }

    pub fn signer(&self) -> &RequestSigner {
        &self.signer
    }

    pub fn set_token(&mut self, access: String, csrf: String) {
        self.signer.token = AccessToken {
            access_token: access,
            csrf,
        };
    }

    pub fn set_user_id(&mut self, id: String) {
        self.signer.identity.user_id = id;
    }

    fn seconds() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    async fn send(&self, method: &str, path: &str, query: &str, body: &str) -> Result<String, Failure> {
        let headers = self.signer.headers(method, path, body, Self::seconds());
        let mut url = format!("{}{}", BASE_URL, path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }

        let mut request = if method.eq_ignore_ascii_case("GET") {
            self.http.get(&url)
        } else {
            self.http.post(&url).body(String::from(body))
        };

        for (name, value) in headers {
            request = request.header(name, value);
        }
        if !body.is_empty() {
            request = request.header("content-type", "application/json");
        }

        let response = request
            .send()
            .await
            .map_err(|e| Failure::Network(e.to_string()))?;
        let status = response.status().as_u16();
        let text = response
            .text()
            .await
            .map_err(|e| Failure::Network(e.to_string()))?;

        if (200..300).contains(&status) {
            return Ok(text);
        }
        if text.contains("device_not_logged_in") || text.contains("Token") {
            return Err(Failure::Expired);
        }
        if status == 403 {
            return Err(Failure::Rejected(text));
        }
        Err(Failure::Status(status, text))
    }

    async fn send_json<T: for<'a> Deserialize<'a>>(
        &self,
        method: &str,
        path: &str,
        query: &str,
        body: &str,
    ) -> Result<T, Failure> {
        let text = self.send(method, path, query, body).await?;
        serde_json::from_str(&text).map_err(|e| Failure::Decode(e.to_string()))
    }

    pub async fn qr_ticket(&self) -> Result<QrTicket, Failure> {
        self.send_json("POST", "/api/v1/qrcode/gen/login", "", "{}").await
    }

    pub async fn qr_state(&self, qr_id: &str) -> Result<QrState, Failure> {
        let query = format!("qr_id={}", escape(qr_id));
        self.send_json("GET", "/api/v1/qrcode/login/status", &query, "")
            .await
    }

    pub async fn wait_qr<F>(&self, notify: F) -> Result<Session, Failure>
    where
        F: FnOnce(&QrTicket),
    {
        let ticket = self.qr_ticket().await?;
        notify(&ticket);

        for _ in 0..150 {
            if let Ok(state) = self.qr_state(&ticket.qr_id).await {
                if !state.token.is_empty() {
                    return Ok(Session {
                        token: state.token,
                        csrf: state.csrf,
                        user_id: state.user_id,
                        nickname: String::new(),
                    });
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Err(Failure::Network(String::from("qr login timed out")))
    }

    pub async fn check_uu_token(&self, uu_token: &str) -> Result<Session, Failure> {
        let body = format!("{{\"token\":{}}}", quote(uu_token));
        self.send_json("POST", "/api/v1/login/check_uu_token", "", &body)
            .await
    }

    pub async fn login_uu_token(&self, uu_token: &str) -> Result<Session, Failure> {
        let body = format!("{{\"token\":{}}}", quote(uu_token));
        self.send_json("POST", "/api/v1/login/by_uu_token", "", &body)
            .await
    }

    pub async fn check_mumu_token(&self, token: &str) -> Result<Session, Failure> {
        let body = format!("{{\"token\":{}}}", quote(token));
        self.send_json("POST", "/api/v1/login/check_mumu_token", "", &body)
            .await
    }

    pub async fn user_info(&self) -> Result<String, Failure> {
        self.send("GET", "/api/v1/user/info", "", "").await
    }

    pub async fn logout(&self) -> Result<String, Failure> {
        self.send("POST", "/api/v1/user/logout", "", "{}").await
    }

    pub async fn device_list(&self) -> Result<String, Failure> {
        self.send("GET", "/api/v1/device/list", "", "").await
    }

    pub async fn device_register(&self, payload: &str) -> Result<String, Failure> {
        self.send("POST", "/api/v1/device", "", payload).await
    }

    pub async fn room_create(&self) -> Result<String, Failure> {
        self.send("POST", "/api/v1/room/create", "", "{}").await
    }
}

fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn quote(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for ch in raw.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_keeps_unreserved() {
        assert_eq!(escape("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn escape_encodes_separators() {
        assert_eq!(escape("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn quote_handles_specials() {
        assert_eq!(quote("ab"), "\"ab\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("a\\b"), "\"a\\\\b\"");
        assert_eq!(quote("a\nb"), "\"a\\nb\"");
    }

    #[test]
    fn quote_escapes_controls() {
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn token_body_is_valid_json() {
        let body = format!("{{\"token\":{}}}", quote("x\"y"));
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["token"], "x\"y");
    }
}