use serde_json::Value;
use std::time::Duration;
use std::time::Instant;

use crate::signaling::Decoder;
use crate::signaling::EngineState;
use crate::signaling::RoomData;
use crate::signaling::Signal;

#[derive(Debug)]
pub enum SignalError {
    Handshake(String),
    Transport(String),
    Closed(String),
    Timeout(String),
}

impl std::fmt::Display for SignalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalError::Handshake(e) => write!(f, "handshake: {}", e),
            SignalError::Transport(e) => write!(f, "transport: {}", e),
            SignalError::Closed(e) => write!(f, "closed: {}", e),
            SignalError::Timeout(e) => write!(f, "timeout: {}", e),
        }
    }
}

impl std::error::Error for SignalError {}

pub struct Session {
    pub room: RoomData,
    pub decoder: Decoder,
    last_ping: Instant,
    last_rx: Instant,
    ping_interval: Duration,
    ping_timeout: Duration,
    awaiting_pong: bool,
}

impl Session {
    pub fn new(room: RoomData) -> Self {
        Self {
            room,
            decoder: Decoder::default(),
            last_ping: Instant::now(),
            last_rx: Instant::now(),
            ping_interval: Duration::from_millis(25_000),
            ping_timeout: Duration::from_millis(20_000),
            awaiting_pong: false,
        }
    }

    pub fn url(&self) -> String {
        self.room.websocket_url()
    }

    pub fn auth_header(&self) -> (String, String) {
        (String::from("X-NRD-AUTH"), self.room.token.clone())
    }

    pub fn absorb(&mut self, text: &str) -> Vec<Signal> {
        self.last_rx = Instant::now();

        if text == "2" {
            self.awaiting_pong = false;
            return Vec::new();
        }
        if text == "3" {
            self.awaiting_pong = false;
            return Vec::new();
        }

        let signals = self.decoder.process(text);
        self.ping_interval = Duration::from_millis(self.decoder.ping_interval_ms());
        self.ping_timeout = Duration::from_millis(self.decoder.ping_timeout_ms());
        signals
    }

    pub fn ready(&self) -> bool {
        self.decoder.state() == EngineState::Ready
    }

    pub fn handshake_frame(&mut self) -> String {
        self.decoder.handshake()
    }

    pub fn emit(&mut self, event: &str, payload: &Value) -> String {
        self.decoder.emit(event, payload)
    }

    pub fn pong_frame(&self) -> &'static str {
        "3"
    }

    pub fn tick(&mut self) -> Tick {
        let now = Instant::now();

        if self.awaiting_pong && now.duration_since(self.last_rx) > self.ping_interval + self.ping_timeout {
            return Tick::Dead(String::from("pong deadline exceeded"));
        }

        if now.duration_since(self.last_ping) >= self.ping_interval {
            self.last_ping = now;
            self.awaiting_pong = true;
            return Tick::SendPong;
        }

        Tick::Idle
    }

    pub fn soac_answer(&mut self, sdp: &str, ice_id: Option<&str>, client_id: &str) -> String {
        let mut payload = serde_json::Map::new();
        payload.insert(String::from("type"), Value::String(String::from("answer")));
        payload.insert(String::from("sdp"), Value::String(sdp.to_string()));
        if let Some(id) = ice_id {
            payload.insert(String::from("ice_id"), Value::String(id.to_string()));
        }
        if !client_id.is_empty() {
            payload.insert(String::from("client_id"), Value::String(client_id.to_string()));
        }
        self.emit("soac", &Value::Object(payload))
    }

    pub fn soac_candidate(&mut self, candidate: &Value, ice_id: Option<&str>, client_id: &str) -> String {
        let mut payload = serde_json::Map::new();
        payload.insert(String::from("type"), Value::String(String::from("candidate")));
        payload.insert(String::from("candidate"), candidate.clone());
        if let Some(id) = ice_id {
            payload.insert(String::from("ice_id"), Value::String(id.to_string()));
        }
        if !client_id.is_empty() {
            payload.insert(String::from("client_id"), Value::String(client_id.to_string()));
        }
        self.emit("soac", &Value::Object(payload))
    }
}

#[derive(Debug)]
pub enum Tick {
    Idle,
    SendPong,
    Dead(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room() -> RoomData {
        let mut r = RoomData::default();
        r.server = String::from("wss://sig.test");
        r.token = String::from("room-token");
        r
    }

    #[test]
    fn auth_header_carries_room_token() {
        let s = Session::new(room());
        let (name, value) = s.auth_header();
        assert_eq!(name, "X-NRD-AUTH");
        assert_eq!(value, "room-token");
    }

    #[test]
    fn url_points_at_engine_endpoint() {
        assert_eq!(
            Session::new(room()).url(),
            "wss://sig.test/socket.io/?transport=websocket&EIE=4".replace("EIE", "EIO")
        );
    }

    #[test]
    fn absorb_tracks_ping_parameters() {
        let mut s = Session::new(room());
        s.absorb("0{\"pingInterval\":15000,\"pingTimeout\":8000}");
        assert_eq!(s.ping_interval, Duration::from_millis(15000));
        assert_eq!(s.ping_timeout, Duration::from_millis(8000));
    }

    #[test]
    fn ready_only_after_namespace_open() {
        let mut s = Session::new(room());
        assert!(!s.ready());
        s.absorb("0{}");
        assert!(!s.ready());
        s.absorb("40{}");
        assert!(s.ready());
    }

    #[test]
    fn handshake_frame_has_room_info() {
        let mut s = Session::new(room());
        let frame = s.handshake_frame();
        assert!(frame.contains("\"room_info\""));
    }

    #[test]
    fn pong_clears_pending_flag() {
        let mut s = Session::new(room());
        s.absorb("0{\"pingInterval\":1,\"pingTimeout\":1}");
        s.last_ping = Instant::now() - Duration::from_secs(5);
        match s.tick() {
            Tick::SendPong => {}
            other => panic!("expected SendPong, got {:?}", other),
        }
        s.absorb("3");
        assert!(!s.awaiting_pong);
    }

    #[test]
    fn missing_pong_eventually_dead() {
        let mut s = Session::new(room());
        s.absorb("0{\"pingInterval\":1,\"pingTimeout\":1}");
        s.awaiting_pong = true;
        s.last_ping = Instant::now() - Duration::from_secs(10);
        s.last_rx = Instant::now() - Duration::from_secs(10);
        assert!(matches!(s.tick(), Tick::Dead(_)));
    }

    #[test]
    fn idle_when_nothing_due() {
        let mut s = Session::new(room());
        s.absorb("0{\"pingInterval\":60000,\"pingTimeout\":60000}");
        assert!(matches!(s.tick(), Tick::Idle));
    }

    #[test]
    fn answer_frame_is_well_formed() {
        let mut s = Session::new(room());
        let frame = s.soac_answer("v=0\r\n", Some("ice-1"), "client-9");
        assert!(frame.starts_with("42"));
        let body = &frame[frame.find('[').unwrap()..];
        let parsed: Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed[0], "soac");
        assert_eq!(parsed[1]["type"], "answer");
        assert_eq!(parsed[1]["sdp"], "v=0\r\n");
        assert_eq!(parsed[1]["ice_id"], "ice-1");
        assert_eq!(parsed[1]["client_id"], "client-9");
    }

    #[test]
    fn candidate_frame_is_well_formed() {
        let mut s = Session::new(room());
        let candidate = serde_json::json!({"candidate":"candidate:1 1 udp","sdpMLineIndex":0});
        let frame = s.soac_candidate(&candidate, None, "client-9");
        let body = &frame[frame.find('[').unwrap()..];
        let parsed: Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed[1]["type"], "candidate");
        assert_eq!(parsed[1]["candidate"]["sdpMLineIndex"], 0);
        assert!(parsed[1].get("ice_id").is_none());
    }

    #[test]
    fn absorb_returns_be_controlled_signal() {
        let mut s = Session::new(room());
        let out = s.absorb(r#"42["be-controlled",{"capture_type":4}]"#);
        assert_eq!(out.len(), 1);
    }
}