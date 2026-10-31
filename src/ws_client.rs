use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::time::Duration;

use crate::ws_frame;

#[derive(Debug)]
pub enum WsError {
    Address(String),
    Connect(String),
    Handshake(String),
    Io(String),
    Frame(String),
    Closed,
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::Address(e) => write!(f, "address: {}", e),
            WsError::Connect(e) => write!(f, "connect: {}", e),
            WsError::Handshake(e) => write!(f, "handshake: {}", e),
            WsError::Io(e) => write!(f, "io: {}", e),
            WsError::Frame(e) => write!(f, "frame: {}", e),
            WsError::Closed => write!(f, "closed"),
        }
    }
}

impl std::error::Error for WsError {}

pub struct Target {
    pub host: String,
    pub port: u16,
    pub resource: String,
    pub tls: bool,
}

impl Target {
    pub fn parse(url: &str) -> Result<Self, WsError> {
        let trimmed = url.trim();
        let (tls, rest) = if let Some(rest) = trimmed.strip_prefix("wss://") {
            (true, rest)
        } else if let Some(rest) = trimmed.strip_prefix("ws://") {
            (false, rest)
        } else {
            return Err(WsError::Address(String::from("url must start with ws:// or wss://")));
        };

        let (authority, resource) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };

        let (host, port) = match authority.rfind(':') {
            Some(at) if !authority[at + 1..].is_empty() => {
                let port = authority[at + 1..]
                    .parse::<u16>()
                    .map_err(|_| WsError::Address(String::from("bad port")))?;
                (authority[..at].to_string(), port)
            }
            _ => (
                authority.to_string(),
                if tls { 443 } else { 80 },
            ),
        };

        if host.is_empty() {
            return Err(WsError::Address(String::from("empty host")));
        }

        Ok(Self {
            host,
            port,
            resource: resource.to_string(),
            tls,
        })
    }

    fn authority(&self) -> String {
        if (self.tls && self.port == 443) || (!self.tls && self.port == 80) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

pub fn handshake_request(target: &Target, extra_headers: &[(String, String)], key: &str) -> Vec<u8> {
    let mut request = String::new();
    request.push_str(&format!("GET {} HTTP/1.1\r\n", target.resource));
    request.push_str(&format!("Host: {}\r\n", target.authority()));
    request.push_str("Upgrade: websocket\r\n");
    request.push_str("Connection: Upgrade\r\n");
    request.push_str(&format!("Sec-WebSocket-Key: {}\r\n", key));
    request.push_str("Sec-WebSocket-Version: 13\r\n");
    for (name, value) in extra_headers {
        request.push_str(&format!("{}: {}\r\n", name, value));
    }
    request.push_str("\r\n");
    request.into_bytes()
}

pub fn check_handshake_response(response: &str) -> Result<(), WsError> {
    let mut lines = response.split("\r\n");
    let status = lines.next().unwrap_or_default();
    if !status.contains("101") {
        return Err(WsError::Handshake(format!(
            "expected 101, got {}",
            status
        )));
    }
    let mut upgraded = false;
    let mut has_key = false;
    for line in lines {
        let lowered = line.to_ascii_lowercase();
        if lowered.starts_with("upgrade:") && lowered.contains("websocket") {
            upgraded = true;
        }
        if lowered.starts_with("sec-websocket-accept:") {
            has_key = true;
        }
    }
    if !upgraded {
        return Err(WsError::Handshake(String::from("missing upgrade header")));
    }
    if !has_key {
        return Err(WsError::Handshake(String::from("missing accept header")));
    }
    Ok(())
}

pub fn plain_mask(seed: u64) -> [u8; 4] {
    let mut mixed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    mixed ^= mixed >> 29;
    mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed ^= mixed >> 32;
    mixed.to_le_bytes()
}

pub fn client_key(seed: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut state = seed | 1;
    let mut raw = [0u8; 16];
    for slot in raw.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *slot = (state & 0xFF) as u8;
    }

    let mut out = String::with_capacity(24);
    let mut index = 0;
    while index + 3 <= raw.len() {
        let a = raw[index] as u32;
        let b = raw[index + 1] as u32;
        let c = raw[index + 2] as u32;
        let triple = (a << 16) | (b << 8) | c;
        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        out.push(ALPHABET[(triple & 0x3F) as usize] as char);
        index += 3;
    }
    if raw.len() - index == 1 {
        let a = raw[index] as u32;
        out.push(ALPHABET[((a >> 2) & 0x3F) as usize] as char);
        out.push(ALPHABET[((a << 4) & 0x3F) as usize] as char);
        out.push_str("==");
    }
    out
}

pub struct Socket {
    stream: TcpStream,
    mask_seed: u64,
    pending: Vec<u8>,
}

impl Socket {
    pub fn connect(
        url: &str,
        extra_headers: &[(String, String)],
        seed: u64,
        timeout: Duration,
    ) -> Result<Self, WsError> {
        let target = Target::parse(url)?;

        if target.tls {
            return Err(WsError::Connect(String::from(
                "tls requires an external stream adapter; pass a plain ws:// url to this transport",
            )));
        }

        let address = format!("{}:{}", target.host, target.port);
        let resolved = address
            .to_socket_addrs()
            .map_err(|e| WsError::Address(format!("{}: {}", address, e)))?
            .next()
            .ok_or_else(|| WsError::Address(String::from("no address resolved")))?;

        let mut stream =
            TcpStream::connect_timeout(&resolved, timeout).map_err(|e| WsError::Connect(e.to_string()))?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| WsError::Io(e.to_string()))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| WsError::Io(e.to_string()))?;
        stream
            .set_nodelay(true)
            .map_err(|e| WsError::Io(e.to_string()))?;

        let key = client_key(seed);
        let request = handshake_request(&target, extra_headers, &key);
        stream
            .write_all(&request)
            .map_err(|e| WsError::Io(e.to_string()))?;

        let response = read_until_headers_end(&mut stream)?;
        check_handshake_response(&response)?;

        Ok(Self {
            stream,
            mask_seed: seed ^ 0xA5A5_5A5A_1234_5678,
            pending: Vec::new(),
        })
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), WsError> {
        let mask = self.next_mask();
        let frame = ws_frame::encode_text(text, mask);
        self.stream
            .write_all(&frame)
            .map_err(|e| WsError::Io(e.to_string()))?;
        self.stream.flush().map_err(|e| WsError::Io(e.to_string()))
    }

    pub fn send_close(&mut self) -> Result<(), WsError> {
        let mask = self.next_mask();
        let frame = ws_frame::encode_close(mask);
        self.stream
            .write_all(&frame)
            .map_err(|e| WsError::Io(e.to_string()))
    }

    pub fn recv_text(&mut self) -> Result<Option<String>, WsError> {
        loop {
            if !self.pending.is_empty() {
                match ws_frame::decode(&self.pending) {
                    Ok(frame) => {
                        self.pending.drain(..frame.consumed);
                        match frame.opcode {
                            ws_frame::OPCODE_TEXT => {
                                return String::from_utf8(frame.payload)
                                    .map(Some)
                                    .map_err(|e| WsError::Frame(e.to_string()));
                            }
                            ws_frame::OPCODE_CLOSE => return Err(WsError::Closed),
                            ws_frame::OPCODE_PING => {
                                let mask = self.next_mask();
                                let pong = ws_frame::encode(ws_frame::OPCODE_PONG, &frame.payload, mask);
                                self.stream
                                    .write_all(&pong)
                                    .map_err(|e| WsError::Io(e.to_string()))?;
                                continue;
                            }
                            _ => continue,
                        }
                    }
                    Err(ws_frame::FrameError::Incomplete) => {}
                    Err(other) => return Err(WsError::Frame(other.to_string())),
                }
            }

            if !self.fill()? {
                return Ok(None);
            }
        }
    }

    fn fill(&mut self) -> Result<bool, WsError> {
        let mut scratch = [0u8; 8192];
        match self.stream.read(&mut scratch) {
            Ok(0) => Ok(false),
            Ok(count) => {
                self.pending.extend_from_slice(&scratch[..count]);
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(false),
            Err(e) => Err(WsError::Io(e.to_string())),
        }
    }

    fn next_mask(&mut self) -> [u8; 4] {
        self.mask_seed = self.mask_seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        plain_mask(self.mask_seed)
    }
}

fn read_until_headers_end(stream: &mut TcpStream) -> Result<String, WsError> {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 1024];

    loop {
        let count = stream
            .read(&mut scratch)
            .map_err(|e| WsError::Handshake(e.to_string()))?;
        if count == 0 {
            return Err(WsError::Handshake(String::from("connection closed mid handshake")));
        }
        buffer.extend_from_slice(&scratch[..count]);
        if let Some(at) = find_double_crlf(&buffer) {
            let head = String::from_utf8_lossy(&buffer[..at + 4]).to_string();
            return Ok(head);
        }
        if buffer.len() > 16 * 1024 {
            return Err(WsError::Handshake(String::from("handshake response too large")));
        }
    }
}

fn find_double_crlf(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wss_url() {
        let t = Target::parse("wss://sig.example.com/socket.io/?transport=websocket&EIO=4").unwrap();
        assert_eq!(t.host, "sig.example.com");
        assert_eq!(t.port, 443);
        assert!(t.tls);
        assert_eq!(t.resource, "/socket.io/?transport=websocket&EIO=4");
    }

    #[test]
    fn parses_ws_url_with_port() {
        let t = Target::parse("ws://127.0.0.1:8080/path").unwrap();
        assert_eq!(t.host, "127.0.0.1");
        assert_eq!(t.port, 8080);
        assert!(!t.tls);
        assert_eq!(t.resource, "/path");
    }

    #[test]
    fn rejects_bad_scheme() {
        assert!(Target::parse("http://example.com").is_err());
    }

    #[test]
    fn authority_omits_default_ports() {
        let t = Target::parse("wss://a.example/p").unwrap();
        assert_eq!(t.authority(), "a.example");
        let t = Target::parse("ws://a.example:81/p").unwrap();
        assert_eq!(t.authority(), "a.example:81");
    }

    #[test]
    fn handshake_request_contains_required_headers() {
        let t = Target::parse("ws://example.com/socket.io/").unwrap();
        let request = String::from_utf8(
            handshake_request(&t, &[(String::from("X-NRD-AUTH"), String::from("tok"))], "KEY123"),
        )
        .unwrap();
        assert!(request.starts_with("GET /socket.io/ HTTP/1.1\r\n"));
        assert!(request.contains("Host: example.com\r\n"));
        assert!(request.contains("Upgrade: websocket\r\n"));
        assert!(request.contains("Connection: Upgrade\r\n"));
        assert!(request.contains("Sec-WebSocket-Key: KEY123\r\n"));
        assert!(request.contains("Sec-WebSocket-Version: 13\r\n"));
        assert!(request.contains("X-NRD-AUTH: tok\r\n"));
        assert!(request.ends_with("\r\n\r\n"));
    }

    #[test]
    fn accepts_101_with_upgrade_headers() {
        let response = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: abc\r\n\r\n";
        assert!(check_handshake_response(response).is_ok());
    }

    #[test]
    fn rejects_non_101() {
        let response = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n";
        assert!(check_handshake_response(response).is_err());
    }

    #[test]
    fn rejects_missing_accept() {
        let response = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n";
        assert!(check_handshake_response(response).is_err());
    }

    #[test]
    fn client_key_has_expected_shape() {
        let key = client_key(42);
        assert_eq!(key.len() % 4, 0);
        assert!(key.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
    }

    #[test]
    fn client_key_is_deterministic_for_seed() {
        assert_eq!(client_key(7), client_key(7));
        assert_ne!(client_key(7), client_key(8));
    }

    #[test]
    fn finds_header_terminator() {
        assert_eq!(find_double_crlf(b"HTTP/1.1 101\r\n\r\nbody"), Some(12));
        assert_eq!(find_double_crlf(b"no terminator"), None);
    }

    #[test]
    fn mask_sequence_is_not_constant() {
        let a = plain_mask(1);
        let b = plain_mask(2);
        assert_ne!(a, b);
    }
}