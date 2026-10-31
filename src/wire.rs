#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Varint(u64),
    Bytes(Vec<u8>),
    Fixed32([u8; 4]),
    Fixed64([u8; 8]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub number: u32,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    Overflow,
    BadField,
    BadWireType(u8),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::Truncated => write!(f, "truncated"),
            WireError::Overflow => write!(f, "varint overflow"),
            WireError::BadField => write!(f, "bad field number"),
            WireError::BadWireType(t) => write!(f, "bad wire type {}", t),
        }
    }
}

pub fn read_varint(buf: &[u8], at: usize) -> Result<(u64, usize), WireError> {
    let mut value = 0u64;
    let mut index = at;
    let mut shift = 0u32;

    while index < buf.len() {
        let byte = buf[index];
        index += 1;
        if shift == 63 && byte > 1 {
            return Err(WireError::Overflow);
        }
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, index));
        }
        shift += 7;
        if shift > 63 {
            return Err(WireError::Overflow);
        }
    }
    Err(WireError::Truncated)
}

pub fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(((value & 0x7F) as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

pub fn write_tag(out: &mut Vec<u8>, number: u32, wire: u8) {
    write_varint(out, ((number as u64) << 3) | wire as u64);
}

pub fn decode(buf: &[u8]) -> Result<Vec<Field>, WireError> {
    let mut fields = Vec::new();
    let mut at = 0usize;

    while at < buf.len() {
        let (key, next) = read_varint(buf, at)?;
        at = next;

        let number = (key >> 3) as u32;
        let wire = (key & 7) as u8;

        if number == 0 || number > 0x1F_FFFF {
            return Err(WireError::BadField);
        }

        let value = match wire {
            0 => {
                let (raw, next) = read_varint(buf, at)?;
                at = next;
                Value::Varint(raw)
            }
            1 => {
                if at + 8 > buf.len() {
                    return Err(WireError::Truncated);
                }
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&buf[at..at + 8]);
                at += 8;
                Value::Fixed64(raw)
            }
            2 => {
                let (size, next) = read_varint(buf, at)?;
                at = next;
                let size = size as usize;
                if at + size > buf.len() {
                    return Err(WireError::Truncated);
                }
                let raw = buf[at..at + size].to_vec();
                at += size;
                Value::Bytes(raw)
            }
            5 => {
                if at + 4 > buf.len() {
                    return Err(WireError::Truncated);
                }
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&buf[at..at + 4]);
                at += 4;
                Value::Fixed32(raw)
            }
            other => return Err(WireError::BadWireType(other)),
        };

        fields.push(Field { number, value });
    }

    Ok(fields)
}

pub fn first_bytes(fields: &[Field], number: u32) -> Option<&[u8]> {
    fields.iter().find_map(|f| match f {
        Field { number: n, value: Value::Bytes(raw) } if *n == number => Some(raw.as_slice()),
        _ => None,
    })
}

pub fn first_varint(fields: &[Field], number: u32) -> Option<u64> {
    fields.iter().find_map(|f| match f {
        Field { number: n, value: Value::Varint(raw) } if *n == number => Some(*raw),
        _ => None,
    })
}

pub fn all_bytes(fields: &[Field], number: u32) -> Vec<Vec<u8>> {
    fields
        .iter()
        .filter_map(|f| match f {
            Field { number: n, value: Value::Bytes(raw) } if *n == number => Some(raw.clone()),
            _ => None,
        })
        .collect()
}

pub fn has(fields: &[Field], number: u32) -> bool {
    fields.iter().any(|f| f.number == number)
}

pub const MESSAGE_SEQ: u32 = 1;
pub const MESSAGE_TIMESTAMP: u32 = 2;
pub const MESSAGE_SIMPLE_ACTION: u32 = 3;
pub const MESSAGE_CAPTURE_CHANGE: u32 = 8;
pub const MESSAGE_CODEC_NEGOTIATION: u32 = 18;
pub const MESSAGE_CAPTURE_CONFIG_RESPONSE: u32 = 19;
pub const MESSAGE_INPUT_EVENT: u32 = 20;
pub const MESSAGE_RPC_REQUEST: u32 = 21;
pub const MESSAGE_RPC_RESPONSE: u32 = 22;

pub const RPC_RESPONSE_HEADER: u32 = 1;
pub const RPC_RESPONSE_CAPTURE_SETTING: u32 = 2;
pub const RPC_RESPONSE_PRIVATE_SCREEN: u32 = 3;
pub const RPC_RESPONSE_FILE_TRANSFER: u32 = 4;
pub const RPC_RESPONSE_CLIPBOARD: u32 = 5;
pub const RPC_RESPONSE_TEXT_CHANGE: u32 = 6;
pub const RPC_RESPONSE_MOUSE_SWITCH: u32 = 7;
pub const RPC_RESPONSE_CREATE_DISPLAY: u32 = 8;
pub const RPC_RESPONSE_REMOVE_DISPLAY: u32 = 9;
pub const RPC_RESPONSE_QUIT_SUPER_SCREEN: u32 = 10;
pub const RPC_RESPONSE_SEND_VIDEO_TRACK: u32 = 11;
pub const RPC_RESPONSE_QUERY_PLUGIN: u32 = 12;
pub const RPC_RESPONSE_UPDATE_PLUGIN: u32 = 13;
pub const RPC_RESPONSE_INSTALL_PLUGIN: u32 = 14;
pub const RPC_RESPONSE_ENABLE_PLUGIN: u32 = 15;
pub const RPC_RESPONSE_PLUGIN_NOTIFY: u32 = 16;
pub const RPC_RESPONSE_AUDIO_POLICY: u32 = 17;
pub const RPC_RESPONSE_FLIP_SCREEN: u32 = 18;
pub const RPC_RESPONSE_QUICK_LAUNCH_SCAN: u32 = 19;
pub const RPC_RESPONSE_QUICK_LAUNCH_ICON: u32 = 20;
pub const RPC_RESPONSE_SCREEN_SAVER: u32 = 21;
pub const RPC_RESPONSE_ENTER_SUPER_SCREEN: u32 = 22;
pub const RPC_RESPONSE_DRAW: u32 = 23;
pub const RPC_RESPONSE_CHECK_ENV: u32 = 24;
pub const RPC_RESPONSE_RENAME_SESSION: u32 = 26;

pub const RPC_REQUEST_HEADER: u32 = 1;
pub const RPC_REQUEST_CAPTURE_SETTING: u32 = 2;
pub const RPC_REQUEST_FPS_SETTING: u32 = 3;
pub const RPC_REQUEST_VIDEO_QUALITY: u32 = 4;
pub const RPC_REQUEST_CURSOR: u32 = 5;
pub const RPC_REQUEST_RESOLUTION: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub seq: u64,
    pub timestamp: u64,
    pub arm: u32,
    pub body: Vec<u8>,
}

impl Envelope {
    pub fn parse(buf: &[u8]) -> Result<Self, WireError> {
        let fields = decode(buf)?;
        let seq = first_varint(&fields, MESSAGE_SEQ).unwrap_or(0);
        let timestamp = first_varint(&fields, MESSAGE_TIMESTAMP).unwrap_or(0);

        for field in &fields {
            if field.number == MESSAGE_SEQ || field.number == MESSAGE_TIMESTAMP {
                continue;
            }
            if let Value::Bytes(raw) = &field.value {
                return Ok(Envelope {
                    seq,
                    timestamp,
                    arm: field.number,
                    body: raw.clone(),
                });
            }
        }

        Ok(Envelope {
            seq,
            timestamp,
            arm: 0,
            body: Vec::new(),
        })
    }

    pub fn build(seq: u64, timestamp: u64, arm: u32, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 24);
        write_tag(&mut out, MESSAGE_SEQ, 0);
        write_varint(&mut out, seq);
        write_tag(&mut out, MESSAGE_TIMESTAMP, 0);
        write_varint(&mut out, timestamp);
        write_tag(&mut out, arm, 2);
        write_varint(&mut out, body.len() as u64);
        out.extend_from_slice(body);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcReply {
    pub request_id: i64,
    pub arm: u32,
    pub body: Vec<u8>,
}

impl RpcReply {
    pub fn new(request_id: i64) -> Self {
        Self {
            request_id,
            arm: 0,
            body: Vec::new(),
        }
    }

    pub fn with_error(mut self, arm: u32, code: i32, message: &str) -> Self {
        self.arm = arm;
        let mut inner = Vec::new();
        let mut error = Vec::new();
        write_tag(&mut error, 1, 0);
        write_varint(&mut error, code as u64);
        if !message.is_empty() {
            write_tag(&mut error, 2, 2);
            write_varint(&mut error, message.len() as u64);
            error.extend_from_slice(message.as_bytes());
        }
        write_tag(&mut inner, 1, 2);
        write_varint(&mut inner, error.len() as u64);
        inner.extend_from_slice(&error);
        self.body = inner;
        self
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut header = Vec::new();
        write_tag(&mut header, 1, 0);
        write_varint(&mut header, self.request_id as u64);
        write_tag(&mut out, RPC_RESPONSE_HEADER, 2);
        write_varint(&mut out, header.len() as u64);
        out.extend_from_slice(&header);

        if self.arm != 0 {
            write_tag(&mut out, self.arm, 2);
            write_varint(&mut out, self.body.len() as u64);
            out.extend_from_slice(&self.body);
        }
        out
    }

    pub fn parse(buf: &[u8]) -> Result<Self, WireError> {
        let fields = decode(buf)?;
        let header = first_bytes(&fields, RPC_RESPONSE_HEADER).ok_or(WireError::BadField)?;
        let header_fields = decode(header)?;
        let request_id = first_varint(&header_fields, 1).unwrap_or(0) as i64;

        for field in &fields {
            if field.number == RPC_RESPONSE_HEADER {
                continue;
            }
            if let Value::Bytes(raw) = &field.value {
                return Ok(RpcReply {
                    request_id,
                    arm: field.number,
                    body: raw.clone(),
                });
            }
        }

        Ok(RpcReply {
            request_id,
            arm: 0,
            body: Vec::new(),
        })
    }
}

pub fn request_id_of(buf: &[u8]) -> Option<i64> {
    let fields = decode(buf).ok()?;
    let header = first_bytes(&fields, RPC_REQUEST_HEADER)?;
    let parsed = decode(header).ok()?;
    first_varint(&parsed, 1).map(|v| v as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_small() {
        let mut out = Vec::new();
        write_varint(&mut out, 1);
        assert_eq!(out, vec![0x01]);
        assert_eq!(read_varint(&out, 0).unwrap(), (1, 1));
    }

    #[test]
    fn varint_roundtrip_large() {
        for value in [127u64, 128, 300, 16_384, u32::MAX as u64, u64::MAX] {
            let mut out = Vec::new();
            write_varint(&mut out, value);
            let (back, at) = read_varint(&out, 0).unwrap();
            assert_eq!(back, value);
            assert_eq!(at, out.len());
        }
    }

    #[test]
    fn varint_rejects_truncation() {
        assert_eq!(read_varint(&[0x80], 0), Err(WireError::Truncated));
    }

    #[test]
    fn varint_rejects_overflow() {
        let raw = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02];
        assert_eq!(read_varint(&raw, 0), Err(WireError::Overflow));
    }

    #[test]
    fn decode_all_wire_types() {
        let mut buf = Vec::new();
        write_tag(&mut buf, 1, 0);
        write_varint(&mut buf, 42);
        write_tag(&mut buf, 2, 2);
        write_varint(&mut buf, 3);
        buf.extend_from_slice(b"abc");
        write_tag(&mut buf, 3, 5);
        buf.extend_from_slice(&[1, 2, 3, 4]);
        write_tag(&mut buf, 4, 1);
        buf.extend_from_slice(&[5, 6, 7, 8, 9, 10, 11, 12]);

        let fields = decode(&buf).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(first_varint(&fields, 1), Some(42));
        assert_eq!(first_bytes(&fields, 2), Some(&b"abc"[..]));
        assert_eq!(fields[2].value, Value::Fixed32([1, 2, 3, 4]));
        assert!(matches!(fields[3].value, Value::Fixed64(_)));
    }

    #[test]
    fn decode_rejects_zero_field() {
        let buf = vec![0x00, 0x00];
        assert_eq!(decode(&buf), Err(WireError::BadField));
    }

    #[test]
    fn decode_rejects_bad_wire_type() {
        let buf = vec![0x0B];
        assert_eq!(decode(&buf), Err(WireError::BadWireType(3)));
    }

    #[test]
    fn envelope_build_parse_roundtrip() {
        let body = b"payload";
        let wire = Envelope::build(77, 1700000000, MESSAGE_RPC_REQUEST, body);
        let parsed = Envelope::parse(&wire).unwrap();
        assert_eq!(parsed.seq, 77);
        assert_eq!(parsed.timestamp, 1700000000);
        assert_eq!(parsed.arm, MESSAGE_RPC_REQUEST);
        assert_eq!(parsed.body, body.to_vec());
    }

    #[test]
    fn envelope_arm_numbers_match_schema() {
        let wire = Envelope::build(1, 1, MESSAGE_INPUT_EVENT, b"x");
        assert_eq!(Envelope::parse(&wire).unwrap().arm, 20);
        let wire = Envelope::build(1, 1, MESSAGE_RPC_RESPONSE, b"x");
        assert_eq!(Envelope::parse(&wire).unwrap().arm, 22);
        let wire = Envelope::build(1, 1, MESSAGE_CODEC_NEGOTIATION, b"x");
        assert_eq!(Envelope::parse(&wire).unwrap().arm, 18);
    }

    #[test]
    fn envelope_without_arm_is_tolerated() {
        let mut wire = Vec::new();
        write_tag(&mut wire, MESSAGE_SEQ, 0);
        write_varint(&mut wire, 5);
        let parsed = Envelope::parse(&wire).unwrap();
        assert_eq!(parsed.seq, 5);
        assert_eq!(parsed.arm, 0);
    }

    #[test]
    fn rpc_reply_roundtrip() {
        let reply = RpcReply::new(99).with_error(RPC_RESPONSE_CAPTURE_SETTING, 0, "");
        let wire = reply.encode();
        let back = RpcReply::parse(&wire).unwrap();
        assert_eq!(back.request_id, 99);
        assert_eq!(back.arm, RPC_RESPONSE_CAPTURE_SETTING);
    }

    #[test]
    fn rpc_reply_header_only() {
        let reply = RpcReply::new(7);
        let wire = reply.encode();
        let back = RpcReply::parse(&wire).unwrap();
        assert_eq!(back.request_id, 7);
        assert_eq!(back.arm, 0);
    }

    #[test]
    fn rpc_reply_error_carries_code() {
        let reply = RpcReply::new(3).with_error(RPC_RESPONSE_CHECK_ENV, 17, "no display");
        let back = RpcReply::parse(&reply.encode()).unwrap();
        let errors = all_bytes(&decode(&back.body).unwrap(), 1);
        assert_eq!(errors.len(), 1);
        let inner = decode(&errors[0]).unwrap();
        assert_eq!(first_varint(&inner, 1), Some(17));
        assert_eq!(first_bytes(&inner, 2), Some(&b"no display"[..]));
    }

    #[test]
    fn request_id_extracted_from_request() {
        let mut header = Vec::new();
        write_tag(&mut header, 1, 0);
        write_varint(&mut header, 1234);

        let mut rpc = Vec::new();
        write_tag(&mut rpc, RPC_REQUEST_HEADER, 2);
        write_varint(&mut rpc, header.len() as u64);
        rpc.extend_from_slice(&header);

        assert_eq!(request_id_of(&rpc), Some(1234));
    }

    #[test]
    fn request_id_absent_returns_none() {
        let rpc = vec![0x08, 0x01];
        assert_eq!(request_id_of(&rpc), None);
    }

    #[test]
    fn response_arm_table_covers_all_recovered_entries() {
        let arms = [
            RPC_RESPONSE_HEADER,
            RPC_RESPONSE_CAPTURE_SETTING,
            RPC_RESPONSE_PRIVATE_SCREEN,
            RPC_RESPONSE_FILE_TRANSFER,
            RPC_RESPONSE_CLIPBOARD,
            RPC_RESPONSE_TEXT_CHANGE,
            RPC_RESPONSE_MOUSE_SWITCH,
            RPC_RESPONSE_CREATE_DISPLAY,
            RPC_RESPONSE_REMOVE_DISPLAY,
            RPC_RESPONSE_QUIT_SUPER_SCREEN,
            RPC_RESPONSE_SEND_VIDEO_TRACK,
            RPC_RESPONSE_QUERY_PLUGIN,
            RPC_RESPONSE_UPDATE_PLUGIN,
            RPC_RESPONSE_INSTALL_PLUGIN,
            RPC_RESPONSE_ENABLE_PLUGIN,
            RPC_RESPONSE_PLUGIN_NOTIFY,
            RPC_RESPONSE_AUDIO_POLICY,
            RPC_RESPONSE_FLIP_SCREEN,
            RPC_RESPONSE_QUICK_LAUNCH_SCAN,
            RPC_RESPONSE_QUICK_LAUNCH_ICON,
            RPC_RESPONSE_SCREEN_SAVER,
            RPC_RESPONSE_ENTER_SUPER_SCREEN,
            RPC_RESPONSE_DRAW,
            RPC_RESPONSE_CHECK_ENV,
            RPC_RESPONSE_RENAME_SESSION,
        ];
        assert_eq!(arms.len(), 25);
        assert!(arms.contains(&2));
        assert!(arms.contains(&26));
    }
}