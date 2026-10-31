pub const OPCODE_CONTINUATION: u8 = 0x0;
pub const OPCODE_TEXT: u8 = 0x1;
pub const OPCODE_BINARY: u8 = 0x2;
pub const OPCODE_CLOSE: u8 = 0x8;
pub const OPCODE_PING: u8 = 0x9;
pub const OPCODE_PONG: u8 = 0xA;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Incomplete,
    Reserved,
    ControlTooLarge,
    BadOpcode(u8),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Incomplete => write!(f, "incomplete frame"),
            FrameError::Reserved => write!(f, "reserved bits set"),
            FrameError::ControlTooLarge => write!(f, "control frame too large"),
            FrameError::BadOpcode(op) => write!(f, "bad opcode {}", op),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub opcode: u8,
    pub payload: Vec<u8>,
    pub consumed: usize,
}

pub fn encode(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode);

    let len = payload.len();
    if len < 126 {
        out.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }

    out.extend_from_slice(&mask);
    for (index, byte) in payload.iter().enumerate() {
        out.push(byte ^ mask[index % 4]);
    }
    out
}

pub fn encode_text(text: &str, mask: [u8; 4]) -> Vec<u8> {
    encode(OPCODE_TEXT, text.as_bytes(), mask)
}

pub fn encode_close(mask: [u8; 4]) -> Vec<u8> {
    encode(OPCODE_CLOSE, &[0x03, 0xE8], mask)
}

pub fn decode(buf: &[u8]) -> Result<Frame, FrameError> {
    if buf.len() < 2 {
        return Err(FrameError::Incomplete);
    }

    let first = buf[0];
    let second = buf[1];

    if first & 0x70 != 0 {
        return Err(FrameError::Reserved);
    }

    let opcode = first & 0x0F;
    match opcode {
        OPCODE_CONTINUATION | OPCODE_TEXT | OPCODE_BINARY | OPCODE_CLOSE | OPCODE_PING
        | OPCODE_PONG => {}
        other => return Err(FrameError::BadOpcode(other)),
    }

    let masked = second & 0x80 != 0;
    let marker = second & 0x7F;

    let (length, mut at) = match marker {
        126 => {
            if buf.len() < 4 {
                return Err(FrameError::Incomplete);
            }
            (u16::from_be_bytes([buf[2], buf[3]]) as usize, 4)
        }
        127 => {
            if buf.len() < 10 {
                return Err(FrameError::Incomplete);
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&buf[2..10]);
            let value = u64::from_be_bytes(bytes);
            if value > usize::MAX as u64 {
                return Err(FrameError::Incomplete);
            }
            (value as usize, 10)
        }
        small => (small as usize, 2),
    };

    if opcode >= OPCODE_CLOSE && length > 125 {
        return Err(FrameError::ControlTooLarge);
    }

    let mask_key = if masked {
        if buf.len() < at + 4 {
            return Err(FrameError::Incomplete);
        }
        let mut key = [0u8; 4];
        key.copy_from_slice(&buf[at..at + 4]);
        at += 4;
        Some(key)
    } else {
        None
    };

    if buf.len() < at + length {
        return Err(FrameError::Incomplete);
    }

    let mut payload = buf[at..at + length].to_vec();
    if let Some(key) = mask_key {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[index % 4];
        }
    }

    Ok(Frame {
        opcode,
        payload,
        consumed: at + length,
    })
}

pub fn text_of(frame: &Frame) -> Option<String> {
    if frame.opcode == OPCODE_TEXT {
        String::from_utf8(frame.payload.clone()).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_roundtrip() {
        let mask = [1, 2, 3, 4];
        let wire = encode_text("hello", mask);
        let frame = decode(&wire).unwrap();
        assert_eq!(frame.opcode, OPCODE_TEXT);
        assert_eq!(frame.payload, b"hello");
        assert_eq!(frame.consumed, wire.len());
    }

    #[test]
    fn medium_length_uses_16_bit_header() {
        let mask = [9, 8, 7, 6];
        let body = "x".repeat(300);
        let wire = encode_text(&body, mask);
        assert_eq!(wire[1] & 0x7F, 126);
        let frame = decode(&wire).unwrap();
        assert_eq!(frame.payload.len(), 300);
    }

    #[test]
    fn large_length_uses_64_bit_header() {
        let mask = [5, 5, 5, 5];
        let body = "y".repeat(70000);
        let wire = encode_text(&body, mask);
        assert_eq!(wire[1] & 0x7F, 127);
        let frame = decode(&wire).unwrap();
        assert_eq!(frame.payload.len(), 70000);
    }

    #[test]
    fn masked_flag_is_set_on_encode() {
        let wire = encode_text("a", [0, 0, 0, 0]);
        assert_eq!(wire[1] & 0x80, 0x80);
    }

    #[test]
    fn unmasked_server_frame_decodes() {
        let wire = vec![0x81, 0x03, b'a', b'b', b'c'];
        let frame = decode(&wire).unwrap();
        assert_eq!(frame.payload, b"abc");
    }

    #[test]
    fn incomplete_header_is_reported() {
        assert_eq!(decode(&[0x81]), Err(FrameError::Incomplete));
    }

    #[test]
    fn incomplete_payload_is_reported() {
        let wire = vec![0x81, 0x05, b'a'];
        assert_eq!(decode(&wire), Err(FrameError::Incomplete));
    }

    #[test]
    fn reserved_bits_rejected() {
        let wire = vec![0xF1, 0x00];
        assert_eq!(decode(&wire), Err(FrameError::Reserved));
    }

    #[test]
    fn oversized_control_frame_rejected() {
        let mut wire = vec![0x88, 0x7F];
        wire.extend_from_slice(&200u64.to_be_bytes());
        assert_eq!(decode(&wire), Err(FrameError::ControlTooLarge));
    }

    #[test]
    fn unknown_opcode_rejected() {
        assert_eq!(decode(&[0x83, 0x00]), Err(FrameError::BadOpcode(3)));
    }

    #[test]
    fn close_frame_encodes_status() {
        let wire = encode_close([1, 1, 1, 1]);
        let frame = decode(&wire).unwrap();
        assert_eq!(frame.opcode, OPCODE_CLOSE);
        assert_eq!(frame.payload, vec![0x03, 0xE8]);
    }

    #[test]
    fn text_of_ignores_binary() {
        let binary = Frame {
            opcode: OPCODE_BINARY,
            payload: b"x".to_vec(),
            consumed: 0,
        };
        assert!(text_of(&binary).is_none());
    }

    #[test]
    fn text_of_rejects_invalid_utf8() {
        let bad = Frame {
            opcode: OPCODE_TEXT,
            payload: vec![0xFF, 0xFE],
            consumed: 0,
        };
        assert!(text_of(&bad).is_none());
    }

    #[test]
    fn two_frames_split_correctly() {
        let mask = [2, 4, 6, 8];
        let first = encode_text("one", mask);
        let second = encode_text("two", mask);
        let mut stream = first.clone();
        stream.extend_from_slice(&second);

        let a = decode(&stream).unwrap();
        assert_eq!(a.payload, b"one");
        let b = decode(&stream[a.consumed..]).unwrap();
        assert_eq!(b.payload, b"two");
    }
}