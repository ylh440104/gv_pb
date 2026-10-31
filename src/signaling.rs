use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RoomData {
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub room_id: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl RoomData {
    pub fn websocket_url(&self) -> String {
        let base = self.server.trim_end_matches('/');
        format!("{}/socket.io/?transport=websocket&EIO=4", base)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Soac {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub sdp: String,
    #[serde(default)]
    pub gzip_sdp: String,
    #[serde(default)]
    pub candidate: Value,
    #[serde(default)]
    pub ice_id: Value,
    #[serde(default)]
    pub sdp_mid: String,
    #[serde(default)]
    pub sdp_mline_index: i32,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub app_control_id: Value,
}

impl Soac {
    pub fn is_offer(&self) -> bool {
        self.kind == "offer"
    }

    pub fn is_answer(&self) -> bool {
        self.kind == "answer"
    }

    pub fn is_candidate(&self) -> bool {
        self.kind == "candidate"
    }

    pub fn ice_id_string(&self) -> Option<String> {
        self.ice_id.as_str().map(|s| s.to_string())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConnectRequest {
    #[serde(default)]
    pub capture_type: i64,
    #[serde(default)]
    pub ice_id: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub enum Signal {
    Connected,
    RoomInfo(Value),
    DeviceChanged(Value),
    BeControlled(ConnectRequest),
    Soac(Soac),
    Released(Value),
    Left(Value),
    Disconnected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    WaitingOpen,
    WaitingNamespace,
    Ready,
}

pub struct Decoder {
    state: EngineState,
    next_request: u64,
    open_payload: Option<Value>,
}

impl Default for Decoder {
    fn default() -> Self {
        Self {
            state: EngineState::WaitingOpen,
            next_request: 10,
            open_payload: None,
        }
    }
}

impl Decoder {
    pub fn state(&self) -> EngineState {
        self.state
    }

    pub fn open_payload(&self) -> Option<&Value> {
        self.open_payload.as_ref()
    }

    pub fn ping_interval_ms(&self) -> u64 {
        self.open_payload
            .as_ref()
            .and_then(|v| v.get("pingInterval"))
            .and_then(Value::as_u64)
            .unwrap_or(25_000)
    }

    pub fn ping_timeout_ms(&self) -> u64 {
        self.open_payload
            .as_ref()
            .and_then(|v| v.get("pingTimeout"))
            .and_then(Value::as_u64)
            .unwrap_or(20_000)
    }

    pub fn take_request_id(&mut self) -> u64 {
        let id = self.next_request;
        self.next_request += 1;
        id
    }

    pub fn handshake(&mut self) -> String {
        let id = self.take_request_id();
        format!("42{}[\"room_info\"]", id)
    }

    pub fn emit(&mut self, event: &str, payload: &Value) -> String {
        let id = self.take_request_id();
        format!("42{}{}", id, format_args!("[\"{}\",{}]", event, payload))
    }

    pub fn process(&mut self, text: &str) -> Vec<Signal> {
        let mut out = Vec::new();

        if text == "2" {
            out.push(Signal::Connected);
            return Vec::new();
        }

        if let Some(rest) = text.strip_prefix('0') {
            if let Ok(parsed) = serde_json::from_str::<Value>(rest) {
                self.open_payload = Some(parsed);
            }
            self.state = EngineState::WaitingNamespace;
            return out;
        }

        if text.starts_with("40") {
            self.state = EngineState::Ready;
            out.push(Signal::Connected);
            return out;
        }

        if text.starts_with("43") {
            let start = text.find('[').unwrap_or(text.len());
            if let Ok(parsed) = serde_json::from_str::<Value>(&text[start..]) {
                out.push(Signal::RoomInfo(parsed));
            }
            return out;
        }

        if text.starts_with("41") {
            out.push(Signal::Disconnected(String::from("server closed namespace")));
            return out;
        }

        if text.starts_with("44") {
            out.push(Signal::Disconnected(String::from("engine error")));
            return out;
        }

        if let Some(body) = text.strip_prefix("42") {
            let payload = match body.find('[') {
                Some(at) => &body[at..],
                None => body,
            };
            if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(payload) {
                if items.len() >= 2 {
                    let name = items[0].as_str().unwrap_or_default();
                    let data = &items[1];
                    match name {
                        "bmsg_push" => out.push(Signal::DeviceChanged(data.clone())),
                        "be-controlled" => out.push(Signal::BeControlled(parse_connect(data))),
                        "soac" => out.push(Signal::Soac(parse_soac(data))),
                        "released" => out.push(Signal::Released(data.clone())),
                        "left" | "leave" => out.push(Signal::Left(data.clone())),
                        _ => {}
                    }
                }
            }
            return out;
        }

        out
    }
}

fn parse_connect(data: &Value) -> ConnectRequest {
    let inner = data.get("data").unwrap_or(data);
    let capture_type = inner
        .get("capture_type")
        .or_else(|| inner.get("captureType"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let ice_id = pick_str(&[data, inner], &["ice_id", "iceId"]);
    let client_id = pick_str(&[data], &["client_id", "clientId"]);
    ConnectRequest {
        capture_type,
        ice_id,
        client_id,
        raw: data.clone(),
    }
}

fn parse_soac(data: &Value) -> Soac {
    let target = data.get("data").unwrap_or(data);
    let mut parsed: Soac = serde_json::from_value(target.clone()).unwrap_or_default();
    if parsed.client_id.is_empty() {
        if let Some(id) = pick_str(&[data], &["client_id", "clientId"]) {
            parsed.client_id = id;
        }
    }
    parsed
}

fn pick_str(sources: &[&Value], keys: &[&str]) -> Option<String> {
    for source in sources {
        for key in keys {
            if let Some(text) = source.get(*key).and_then(Value::as_str) {
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_sets_engine_parameters() {
        let mut d = Decoder::default();
        let out = d.process("0{\"sid\":\"abc\",\"pingInterval\":30000,\"pingTimeout\":20000}");
        assert!(out.is_empty());
        assert_eq!(d.state(), EngineState::WaitingNamespace);
        assert_eq!(d.ping_interval_ms(), 30000);
        assert_eq!(d.ping_timeout_ms(), 20000);
    }

    #[test]
    fn namespace_open_reports_connected() {
        let mut d = Decoder::default();
        d.process("0{}");
        let out = d.process("40{\"sid\":\"abc\"}");
        assert!(matches!(out.first(), Some(Signal::Connected)));
        assert_eq!(d.state(), EngineState::Ready);
    }

    #[test]
    fn handshake_uses_event_ack_shape() {
        let mut d = Decoder::default();
        let frame = d.handshake();
        assert!(frame.starts_with("42"));
        assert!(frame.ends_with("[\"room_info\"]"));
        let first = d.take_request_id();
        assert_eq!(first, 12);
    }

    #[test]
    fn ack_becomes_room_info() {
        let mut d = Decoder::default();
        let out = d.process("4310[\"success\",{\"reconnect_key\":\"k\"}]");
        assert_eq!(out.len(), 1);
        match &out[0] {
            Signal::RoomInfo(v) => assert_eq!(v[0], "success"),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn be_controlled_reads_capture_type_and_ice() {
        let mut d = Decoder::default();
        let out = d.process(
            r#"42["be-controlled",{"capture_type":4,"ice_id":"ice-7","client_id":"c-1"}]"#,
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            Signal::BeControlled(req) => {
                assert_eq!(req.capture_type, 4);
                assert_eq!(req.ice_id.as_deref(), Some("ice-7"));
                assert_eq!(req.client_id.as_deref(), Some("c-1"));
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn soac_offer_is_recognised() {
        let mut d = Decoder::default();
        let out = d.process(
            r#"42["soac",{"client_id":"c-1","data":{"type":"offer","sdp":"v=0","ice_id":"ice-7"}}]"#,
        );
        match &out[0] {
            Signal::Soac(s) => {
                assert!(s.is_offer());
                assert_eq!(s.sdp, "v=0");
                assert_eq!(s.client_id, "c-1");
                assert_eq!(s.ice_id_string().as_deref(), Some("ice-7"));
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn soac_candidate_is_recognised() {
        let mut d = Decoder::default();
        let out = d.process(r#"42["soac",{"type":"candidate","candidate":{"candidate":"x"}}]"#);
        match &out[0] {
            Signal::Soac(s) => assert!(s.is_candidate()),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn released_and_left_are_reported() {
        let mut d = Decoder::default();
        assert!(matches!(
            d.process(r#"42["released",{"ice_id":"i"}]"#).first(),
            Some(Signal::Released(_))
        ));
        assert!(matches!(
            d.process(r#"42["left",{"ice_id":"i"}]"#).first(),
            Some(Signal::Left(_))
        ));
        assert!(matches!(
            d.process(r#"42["leave",{"ice_id":"i"}]"#).first(),
            Some(Signal::Left(_))
        ));
    }

    #[test]
    fn namespace_close_reports_disconnection() {
        let mut d = Decoder::default();
        let out = d.process("41");
        assert!(matches!(out.first(), Some(Signal::Disconnected(_))));
    }

    #[test]
    fn unknown_event_is_ignored() {
        let mut d = Decoder::default();
        let out = d.process(r#"42["something_new",{"x":1}]"#);
        assert!(out.is_empty());
    }

    #[test]
    fn malformed_frame_does_not_panic() {
        let mut d = Decoder::default();
        let _ = d.process("42[");
        let _ = d.process("not a frame");
        let _ = d.process("");
    }

    #[test]
    fn websocket_url_appends_engine_path() {
        let mut room = RoomData::default();
        room.server = String::from("wss://sig.example.com/");
        assert_eq!(
            room.websocket_url(),
            "wss://sig.example.com/socket.io/?transport=websocket&EIO=4"
        );
    }
}