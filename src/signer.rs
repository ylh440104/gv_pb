use hmac::Hmac;
use hmac::Mac;
use sha2::Sha256;
use std::collections::BTreeMap;

type HmacSha256 = Hmac<Sha256>;

pub const SIGN_KEY: &str = "alWiSzXZTLu3WfFnw13uBru3";
pub const PLUGIN_SIGN_KEY: &str = "oK5zJ1zJ5cF6kK9eW2jM8oF3";
pub const BASE_URL: &str = "https://api.nrd.nie.163.com";
pub const PKG_NAME: &str = "com.netease.uuremote";
pub const CHANNEL: &str = "nochannel";
pub const VERSION_NAME: &str = "4.45.0.4318";
pub const VERSION_CODE: &str = "44504318";

#[derive(Clone, Default)]
pub struct DeviceIdentity {
    pub client_id: String,
    pub device_id: String,
    pub system_id: String,
    pub user_id: String,
    pub guest_id: String,
}

#[derive(Clone, Default)]
pub struct AccessToken {
    pub access_token: String,
    pub csrf: String,
}

#[derive(Clone)]
pub struct RequestSigner {
    pub identity: DeviceIdentity,
    pub token: AccessToken,
    pub language: String,
    pub country: String,
    pub version_name: String,
    pub version_code: String,
    pub key: String,
}

impl Default for RequestSigner {
    fn default() -> Self {
        Self {
            identity: DeviceIdentity::default(),
            token: AccessToken::default(),
            language: String::from("zh_CN"),
            country: String::from("CN"),
            version_name: String::from(VERSION_NAME),
            version_code: String::from(VERSION_CODE),
            key: String::from(SIGN_KEY),
        }
    }
}

impl RequestSigner {
    pub fn payload(method: &str, path: &str, params: &BTreeMap<String, String>, body: &str) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(params.len());
        for (name, value) in params.iter() {
            let lowered = name.to_ascii_lowercase();
            if lowered.starts_with("x-param-") && lowered != "x-param-sign" && !value.is_empty() {
                parts.push(format!("{}={}", lowered, value));
            }
        }
        format!("{}{}{}{}", method.to_ascii_uppercase(), path, parts.join("&"), body)
    }

    pub fn params(&self, unix_seconds: u64) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert(String::from("x-param-plat"), String::from("1"));
        map.insert(String::from("x-param-vn"), self.version_name.clone());
        map.insert(String::from("x-param-vc"), self.version_code.clone());
        map.insert(String::from("x-param-pkgn"), String::from(PKG_NAME));
        map.insert(String::from("x-param-chn"), String::from(CHANNEL));
        map.insert(String::from("x-param-lang"), self.language.clone());
        map.insert(String::from("x-param-cnt"), self.country.clone());
        map.insert(String::from("x-param-rel"), String::from("1"));
        map.insert(String::from("x-param-ts"), unix_seconds.to_string());

        let optional = [
            ("x-param-client-id", self.identity.client_id.as_str()),
            ("x-param-device-id", self.identity.device_id.as_str()),
            ("x-param-system-id", self.identity.system_id.as_str()),
            ("x-param-user-id", self.identity.user_id.as_str()),
            ("x-param-guest-id", self.identity.guest_id.as_str()),
        ];
        for (name, value) in optional {
            if !value.is_empty() {
                map.insert(String::from(name), String::from(value));
            }
        }
        map
    }

    pub fn signature(&self, method: &str, path: &str, params: &BTreeMap<String, String>, body: &str) -> String {
        let payload = RequestSigner::payload(method, path, params, body);
        let mut mac = HmacSha256::new_from_slice(self.key.as_bytes()).expect("hmac key");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub fn headers(&self, method: &str, path: &str, body: &str, unix_seconds: u64) -> Vec<(String, String)> {
        let params = self.params(unix_seconds);
        let signature = self.signature(method, path, &params, body);

        let mut out: Vec<(String, String)> = params.into_iter().collect();
        out.push((String::from("x-param-sign"), signature));

        if !self.token.access_token.is_empty() {
            out.push((
                String::from("authorization"),
                format!("Bearer {}", self.token.access_token),
            ));
            out.push((
                String::from("cookie"),
                format!(
                    "nrd_access_token={}; nrd_csrf_access_token={}",
                    self.token.access_token, self.token.csrf
                ),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> RequestSigner {
        let mut s = RequestSigner::default();
        s.identity = DeviceIdentity {
            client_id: String::from("c1"),
            device_id: String::from("d1"),
            system_id: String::from("s1"),
            user_id: String::new(),
            guest_id: String::new(),
        };
        s
    }

    #[test]
    fn payload_order_is_method_path_params_body() {
        let mut params = BTreeMap::new();
        params.insert(String::from("x-param-vn"), String::from("2"));
        params.insert(String::from("x-param-cnt"), String::from("1"));
        let p = RequestSigner::payload("post", "/api/v1/x", &params, "BODY");
        assert_eq!(p, "POST/api/v1/xx-param-cnt=1&x-param-vn=2BODY");
    }

    #[test]
    fn sign_excluded_from_params() {
        let mut params = BTreeMap::new();
        params.insert(String::from("x-param-cnt"), String::from("1"));
        let a = RequestSigner::payload("GET", "/p", &params, "");
        params.insert(String::from("x-param-sign"), String::from("zz"));
        let b = RequestSigner::payload("GET", "/p", &params, "");
        assert_eq!(a, b);
    }

    #[test]
    fn non_param_headers_ignored() {
        let mut params = BTreeMap::new();
        params.insert(String::from("authorization"), String::from("Bearer t"));
        params.insert(String::from("content-type"), String::from("application/json"));
        let p = RequestSigner::payload("GET", "/p", &params, "");
        assert_eq!(p, "GET/p");
    }

    #[test]
    fn empty_values_ignored() {
        let mut params = BTreeMap::new();
        params.insert(String::from("x-param-opr"), String::new());
        params.insert(String::from("x-param-cnt"), String::from("1"));
        let p = RequestSigner::payload("GET", "/p", &params, "");
        assert_eq!(p, "GET/px-param-cnt=1");
    }

    #[test]
    fn name_case_normalized() {
        let mut upper = BTreeMap::new();
        upper.insert(String::from("X-Param-CNT"), String::from("1"));
        let mut lower = BTreeMap::new();
        lower.insert(String::from("x-param-cnt"), String::from("1"));
        assert_eq!(
            RequestSigner::payload("GET", "/p", &upper, ""),
            RequestSigner::payload("GET", "/p", &lower, "")
        );
    }

    #[test]
    fn signature_is_hex_sha256() {
        let s = signer();
        let params = s.params(1700000000);
        let sig = s.signature("POST", "/api/v1/room/create", &params, "{}");
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn signature_matches_manual_hmac() {
        let s = signer();
        let params = s.params(1700000000);
        let expected = s.signature("POST", "/api/v1/room/create", &params, "{}");

        let payload = RequestSigner::payload("POST", "/api/v1/room/create", &params, "{}");
        let mut mac = HmacSha256::new_from_slice(SIGN_KEY.as_bytes()).unwrap();
        mac.update(payload.as_bytes());
        assert_eq!(expected, hex::encode(mac.finalize().into_bytes()));
    }

    #[test]
    fn auth_headers_absent_before_login() {
        let s = signer();
        let h = s.headers("GET", "/api/v1/device/list", "", 1700000000);
        let names: Vec<&str> = h.iter().map(|(k, _)| k.as_str()).collect();
        assert!(!names.contains(&"authorization"));
        assert!(!names.contains(&"cookie"));
        assert!(names.contains(&"x-param-sign"));
    }

    #[test]
    fn auth_headers_present_after_login() {
        let mut s = signer();
        s.token = AccessToken {
            access_token: String::from("tok"),
            csrf: String::from("cs"),
        };
        let h = s.headers("GET", "/api/v1/device/list", "", 1700000000);
        let map: BTreeMap<&str, &str> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(map.get("authorization"), Some(&"Bearer tok"));
        assert_eq!(
            map.get("cookie"),
            Some(&"nrd_access_token=tok; nrd_csrf_access_token=cs")
        );
    }

    #[test]
    fn user_id_only_signed_once_present() {
        let mut s = signer();
        let before = s.signature("GET", "/p", &s.params(1), "");
        s.identity.user_id = String::from("u1");
        let after = s.signature("GET", "/p", &s.params(1), "");
        assert_ne!(before, after);
    }

    #[test]
    fn ts_changes_signature() {
        let s = signer();
        let a = s.signature("GET", "/p", &s.params(1), "");
        let b = s.signature("GET", "/p", &s.params(2), "");
        assert_ne!(a, b);
    }

    #[test]
    fn known_vector_matches_analysis() {
        let s = RequestSigner::default();
        let params = s.params(1700000000);
        assert_eq!(
            RequestSigner::payload("POST", "/api/v1/login/check_uu_token", &params, "{}"),
            "POST/api/v1/login/check_uu_tokenx-param-chn=nochannel&x-param-cnt=CN&x-param-lang=zh_CN&x-param-pkgn=com.netease.uuremote&x-param-plat=1&x-param-rel=1&x-param-ts=1700000000&x-param-vc=44504318&x-param-vn=4.45.0.4318{}"
        );
        assert_eq!(
            s.signature("POST", "/api/v1/login/check_uu_token", &params, "{}"),
            "489630636a59f4ebf53b720b87abd5d6b6cfd4435809b88d14a27bc04b7c6709"
        );
    }
}