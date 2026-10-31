use serde_json::Value;

use crate::wire;
use crate::wire::Envelope;
use crate::wire::RpcReply;
use crate::wire::WireError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Noop,
    Echo {
        request_id: i64,
    },
    CaptureSetting {
        request_id: i64,
        fps: u64,
        quality: u64,
        codec: u64,
        width: i32,
        height: i32,
        cursor: bool,
    },
    FpsSetting {
        request_id: i64,
        fps: u64,
    },
    VideoQuality {
        request_id: i64,
        quality: u64,
    },
    CursorSetting {
        request_id: i64,
        cursor: bool,
    },
    Resolution {
        request_id: i64,
        width: i32,
        height: i32,
    },
    Input {
        event_type: u64,
        points: Vec<TouchPoint>,
    },
    CodecNegotiation {
        entries: Vec<CodecEntry>,
    },
    CaptureChange {
        kind: u64,
        detail: String,
    },
    Unknown {
        arm: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TouchPoint {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecEntry {
    pub capture_mode: u64,
    pub fps: u64,
    pub codec: u64,
    pub width: u64,
    pub height: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyArm {
    CaptureSetting,
    FpsSetting,
    VideoQuality,
    CursorSetting,
    Resolution,
    Input,
}

impl ReplyArm {
    pub fn wire_number(self) -> u32 {
        match self {
            ReplyArm::CaptureSetting => wire::RPC_RESPONSE_CAPTURE_SETTING,
            ReplyArm::FpsSetting => wire::RPC_RESPONSE_CAPTURE_SETTING,
            ReplyArm::VideoQuality => wire::RPC_RESPONSE_CAPTURE_SETTING,
            ReplyArm::CursorSetting => wire::RPC_RESPONSE_CAPTURE_SETTING,
            ReplyArm::Resolution => wire::RPC_RESPONSE_CAPTURE_SETTING,
            ReplyArm::Input => wire::RPC_RESPONSE_CAPTURE_SETTING,
        }
    }
}

pub fn decode_command(payload: &[u8]) -> Result<Command, WireError> {
    let envelope = Envelope::parse(payload)?;
    let body = envelope.body;

    match envelope.arm {
        wire::MESSAGE_RPC_REQUEST => decode_rpc(&body),
        wire::MESSAGE_INPUT_EVENT => decode_input(&body),
        wire::MESSAGE_CODEC_NEGOTIATION => decode_negotiation(&body),
        wire::MESSAGE_CAPTURE_CHANGE => decode_change(&body),
        0 => Ok(Command::Noop),
        other => Ok(Command::Unknown { arm: other }),
    }
}

fn decode_rpc(body: &[u8]) -> Result<Command, WireError> {
    let fields = wire::decode(body)?;
    let request_id = wire::first_bytes(&fields, wire::RPC_REQUEST_HEADER)
        .and_then(|h| wire::decode(h).ok())
        .and_then(|h| wire::first_varint(&h, 1))
        .map(|v| v as i64)
        .unwrap_or(0);

    if let Some(capture) = wire::first_bytes(&fields, wire::RPC_REQUEST_CAPTURE_SETTING) {
        let parsed = wire::decode(capture)?;
        return Ok(Command::CaptureSetting {
            request_id,
            fps: wire::first_varint(&parsed, 1).unwrap_or(0),
            quality: wire::first_varint(&parsed, 2).unwrap_or(0),
            cursor: wire::first_varint(&parsed, 3).unwrap_or(0) != 0,
            width: wire::first_varint(&parsed, 5).unwrap_or(0) as i32,
            height: wire::first_varint(&parsed, 6).unwrap_or(0) as i32,
            codec: wire::first_varint(&parsed, 13).unwrap_or(0),
        });
    }

    if let Some(fps) = wire::first_bytes(&fields, wire::RPC_REQUEST_FPS_SETTING) {
        let parsed = wire::decode(fps)?;
        return Ok(Command::FpsSetting {
            request_id,
            fps: wire::first_varint(&parsed, 1).unwrap_or(0),
        });
    }

    if let Some(quality) = wire::first_bytes(&fields, wire::RPC_REQUEST_VIDEO_QUALITY) {
        let parsed = wire::decode(quality)?;
        return Ok(Command::VideoQuality {
            request_id,
            quality: wire::first_varint(&parsed, 1).unwrap_or(0),
        });
    }

    if let Some(cursor) = wire::first_bytes(&fields, wire::RPC_REQUEST_CURSOR) {
        let parsed = wire::decode(cursor)?;
        return Ok(Command::CursorSetting {
            request_id,
            cursor: wire::first_varint(&parsed, 1).unwrap_or(0) != 0,
        });
    }

    if let Some(resolution) = wire::first_bytes(&fields, wire::RPC_REQUEST_RESOLUTION) {
        let parsed = wire::decode(resolution)?;
        return Ok(Command::Resolution {
            request_id,
            width: wire::first_varint(&parsed, 1).unwrap_or(0) as i32,
            height: wire::first_varint(&parsed, 2).unwrap_or(0) as i32,
        });
    }

    Ok(Command::Echo { request_id })
}

fn decode_input(body: &[u8]) -> Result<Command, WireError> {
    let fields = wire::decode(body)?;
    let touch = wire::first_bytes(&fields, 1).ok_or(WireError::BadField)?;
    let touch_fields = wire::decode(touch)?;
    let event_type = wire::first_varint(&touch_fields, 1).unwrap_or(0);

    let mut points = Vec::new();
    for raw in wire::all_bytes(&touch_fields, 2) {
        let parsed = wire::decode(&raw)?;
        points.push(TouchPoint {
            id: wire::first_varint(&parsed, 1).unwrap_or(0),
            x: read_float(&parsed, 2),
            y: read_float(&parsed, 3),
            pressure: read_float(&parsed, 7),
        });
    }

    Ok(Command::Input { event_type, points })
}

fn decode_negotiation(body: &[u8]) -> Result<Command, WireError> {
    let fields = wire::decode(body)?;
    let mut entries = Vec::new();
    for raw in wire::all_bytes(&fields, 1) {
        let parsed = wire::decode(&raw)?;
        entries.push(CodecEntry {
            capture_mode: wire::first_varint(&parsed, 1).unwrap_or(0),
            fps: wire::first_varint(&parsed, 2).unwrap_or(0),
            codec: wire::first_varint(&parsed, 3).unwrap_or(0),
            width: wire::first_varint(&parsed, 4).unwrap_or(0),
            height: wire::first_varint(&parsed, 5).unwrap_or(0),
        });
    }
    Ok(Command::CodecNegotiation { entries })
}

fn decode_change(body: &[u8]) -> Result<Command, WireError> {
    let fields = wire::decode(body)?;
    let kind = wire::first_varint(&fields, 1).unwrap_or(0);
    let detail = wire::first_bytes(&fields, 3)
        .and_then(|raw| String::from_utf8(raw.to_vec()).ok())
        .unwrap_or_default();
    Ok(Command::CaptureChange { kind, detail })
}

fn read_float(fields: &[wire::Field], number: u32) -> f32 {
    for field in fields {
        if field.number == number {
            if let wire::Value::Fixed32(raw) = &field.value {
                return f32::from_le_bytes(*raw);
            }
        }
    }
    0.0
}

pub fn reply_for(command: &Command) -> Option<RpcReply> {
    match command {
        Command::Echo { request_id } => Some(RpcReply::new(*request_id)),
        Command::CaptureSetting { request_id, .. } => Some(empty_ok(*request_id)),
        Command::FpsSetting { request_id, .. } => Some(empty_ok(*request_id)),
        Command::VideoQuality { request_id, .. } => Some(empty_ok(*request_id)),
        Command::CursorSetting { request_id, .. } => Some(empty_ok(*request_id)),
        Command::Resolution { request_id, .. } => Some(empty_ok(*request_id)),
        _ => None,
    }
}

fn empty_ok(request_id: i64) -> RpcReply {
    RpcReply::new(request_id).with_error(wire::RPC_RESPONSE_CAPTURE_SETTING, 0, "")
}

pub fn wrap_reply(reply: &RpcReply, seq: u64, timestamp: u64) -> Vec<u8> {
    let body = reply.encode();
    Envelope::build(seq, timestamp, wire::MESSAGE_RPC_RESPONSE, &body)
}

pub fn simple_action_frame(action: u64, seq: u64, timestamp: u64) -> Vec<u8> {
    let mut body = Vec::new();
    wire::write_tag(&mut body, 1, 0);
    wire::write_varint(&mut body, action);
    Envelope::build(seq, timestamp, wire::MESSAGE_SIMPLE_ACTION, &body)
}

pub fn echo_response_frame(request_id: i64, seq: u64, timestamp: u64) -> Vec<u8> {
    let reply = RpcReply::new(request_id);
    wrap_reply(&reply, seq, timestamp)
}

pub fn to_json(command: &Command) -> Value {
    match command {
        Command::Noop => serde_json::json!({"kind": "noop"}),
        Command::Echo { request_id } => serde_json::json!({"kind": "echo", "request_id": request_id}),
        Command::CaptureSetting {
            request_id,
            fps,
            quality,
            codec,
            width,
            height,
            cursor,
        } => serde_json::json!({
            "kind": "capture_setting",
            "request_id": request_id,
            "fps": fps,
            "quality": quality,
            "codec": codec,
            "width": width,
            "height": height,
            "cursor": cursor,
        }),
        Command::FpsSetting { request_id, fps } => {
            serde_json::json!({"kind": "fps_setting", "request_id": request_id, "fps": fps})
        }
        Command::VideoQuality { request_id, quality } => serde_json::json!({
            "kind": "video_quality",
            "request_id": request_id,
            "quality": quality,
        }),
        Command::CursorSetting { request_id, cursor } => serde_json::json!({
            "kind": "cursor_setting",
            "request_id": request_id,
            "cursor": cursor,
        }),
        Command::Resolution {
            request_id,
            width,
            height,
        } => serde_json::json!({
            "kind": "resolution",
            "request_id": request_id,
            "width": width,
            "height": height,
        }),
        Command::Input { event_type, points } => serde_json::json!({
            "kind": "input",
            "event_type": event_type,
            "points": points.len(),
        }),
        Command::CodecNegotiation { entries } => serde_json::json!({
            "kind": "codec_negotiation",
            "entries": entries.len(),
        }),
        Command::CaptureChange { kind, detail } => serde_json::json!({
            "kind": "capture_change",
            "capture_type": kind,
            "detail": detail,
        }),
        Command::Unknown { arm } => serde_json::json!({"kind": "unknown", "arm": arm}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_request(request_id: i64, arm: u32, body: &[u8]) -> Vec<u8> {
        let mut rpc = Vec::new();
        let mut header = Vec::new();
        wire::write_tag(&mut header, 1, 0);
        wire::write_varint(&mut header, request_id as u64);
        wire::write_tag(&mut rpc, wire::RPC_REQUEST_HEADER, 2);
        wire::write_varint(&mut rpc, header.len() as u64);
        rpc.extend_from_slice(&header);
        wire::write_tag(&mut rpc, arm, 2);
        wire::write_varint(&mut rpc, body.len() as u64);
        rpc.extend_from_slice(body);
        Envelope::build(1, 1000, wire::MESSAGE_RPC_REQUEST, &rpc)
    }

    #[test]
    fn capture_setting_request_decodes_all_fields() {
        let mut capture = Vec::new();
        wire::write_tag(&mut capture, 1, 0);
        wire::write_varint(&mut capture, 2);
        wire::write_tag(&mut capture, 2, 0);
        wire::write_varint(&mut capture, 3);
        wire::write_tag(&mut capture, 3, 0);
        wire::write_varint(&mut capture, 1);
        wire::write_tag(&mut capture, 5, 0);
        wire::write_varint(&mut capture, 2560);
        wire::write_tag(&mut capture, 6, 0);
        wire::write_varint(&mut capture, 1440);
        wire::write_tag(&mut capture, 13, 0);
        wire::write_varint(&mut capture, 2);

        let frame = rpc_request(41, wire::RPC_REQUEST_CAPTURE_SETTING, &capture);
        match decode_command(&frame).unwrap() {
            Command::CaptureSetting {
                request_id,
                fps,
                quality,
                codec,
                width,
                height,
                cursor,
            } => {
                assert_eq!(request_id, 41);
                assert_eq!(fps, 2);
                assert_eq!(quality, 3);
                assert_eq!(codec, 2);
                assert_eq!(width, 2560);
                assert_eq!(height, 1440);
                assert!(cursor);
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn resolution_request_decodes() {
        let mut body = Vec::new();
        wire::write_tag(&mut body, 1, 0);
        wire::write_varint(&mut body, 1920);
        wire::write_tag(&mut body, 2, 0);
        wire::write_varint(&mut body, 1080);

        let frame = rpc_request(7, wire::RPC_REQUEST_RESOLUTION, &body);
        match decode_command(&frame).unwrap() {
            Command::Resolution { request_id, width, height } => {
                assert_eq!(request_id, 7);
                assert_eq!(width, 1920);
                assert_eq!(height, 1080);
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn header_only_rpc_becomes_echo() {
        let frame = rpc_request(55, 0, b"");
        match decode_command(&frame).unwrap() {
            Command::Echo { request_id } => assert_eq!(request_id, 55),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn input_event_decodes_points() {
        let mut point = Vec::new();
        wire::write_tag(&mut point, 1, 0);
        wire::write_varint(&mut point, 3);
        wire::write_tag(&mut point, 2, 5);
        point.extend_from_slice(&12.5f32.to_le_bytes());
        wire::write_tag(&mut point, 3, 5);
        point.extend_from_slice(&34.5f32.to_le_bytes());

        let mut touch = Vec::new();
        wire::write_tag(&mut touch, 1, 0);
        wire::write_varint(&mut touch, 2);
        wire::write_tag(&mut touch, 2, 2);
        wire::write_varint(&mut touch, point.len() as u64);
        touch.extend_from_slice(&point);

        let mut event = Vec::new();
        wire::write_tag(&mut event, 1, 2);
        wire::write_varint(&mut event, touch.len() as u64);
        event.extend_from_slice(&touch);

        let frame = Envelope::build(1, 1, wire::MESSAGE_INPUT_EVENT, &event);
        match decode_command(&frame).unwrap() {
            Command::Input { event_type, points } => {
                assert_eq!(event_type, 2);
                assert_eq!(points.len(), 1);
                assert_eq!(points[0].id, 3);
                assert!((points[0].x - 12.5).abs() < 0.001);
                assert!((points[0].y - 34.5).abs() < 0.001);
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn codec_negotiation_decodes_entries() {
        let mut entry = Vec::new();
        wire::write_tag(&mut entry, 1, 0);
        wire::write_varint(&mut entry, 1);
        wire::write_tag(&mut entry, 2, 0);
        wire::write_varint(&mut entry, 60);
        wire::write_tag(&mut entry, 3, 0);
        wire::write_varint(&mut entry, 2);
        wire::write_tag(&mut entry, 4, 0);
        wire::write_varint(&mut entry, 1920);
        wire::write_tag(&mut entry, 5, 0);
        wire::write_varint(&mut entry, 1080);

        let mut body = Vec::new();
        wire::write_tag(&mut body, 1, 2);
        wire::write_varint(&mut body, entry.len() as u64);
        body.extend_from_slice(&entry);

        let frame = Envelope::build(1, 1, wire::MESSAGE_CODEC_NEGOTIATION, &body);
        match decode_command(&frame).unwrap() {
            Command::CodecNegotiation { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].fps, 60);
                assert_eq!(entries[0].codec, 2);
                assert_eq!(entries[0].width, 1920);
            }
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn reply_pairs_request_id() {
        let frame = rpc_request(88, wire::RPC_REQUEST_CURSOR, &[0x08, 0x01]);
        let command = decode_command(&frame).unwrap();
        let reply = reply_for(&command).unwrap();
        assert_eq!(reply.request_id, 88);
    }

    #[test]
    fn input_has_no_reply() {
        let mut event = Vec::new();
        wire::write_tag(&mut event, 1, 2);
        wire::write_varint(&mut event, 0);
        let frame = Envelope::build(1, 1, wire::MESSAGE_INPUT_EVENT, &event);
        let command = decode_command(&frame).unwrap();
        assert!(reply_for(&command).is_none());
    }

    #[test]
    fn wrapped_reply_roundtrips_as_envelope() {
        let reply = RpcReply::new(13).with_error(wire::RPC_RESPONSE_CAPTURE_SETTING, 0, "");
        let wire = wrap_reply(&reply, 9, 1700);
        let envelope = Envelope::parse(&wire).unwrap();
        assert_eq!(envelope.seq, 9);
        assert_eq!(envelope.timestamp, 1700);
        assert_eq!(envelope.arm, wire::MESSAGE_RPC_RESPONSE);

        let back = RpcReply::parse(&envelope.body).unwrap();
        assert_eq!(back.request_id, 13);
    }

    #[test]
    fn action_frame_uses_field_three() {
        let frame = simple_action_frame(4, 1, 1);
        let envelope = Envelope::parse(&frame).unwrap();
        assert_eq!(envelope.arm, wire::MESSAGE_SIMPLE_ACTION);
        let fields = wire::decode(&envelope.body).unwrap();
        assert_eq!(wire::first_varint(&fields, 1), Some(4));
    }

    #[test]
    fn unknown_arm_reported_not_crashing() {
        let frame = Envelope::build(1, 1, 99, b"x");
        assert!(matches!(decode_command(&frame).unwrap(), Command::Unknown { arm: 99 }));
    }

    #[test]
    fn empty_payload_is_noop() {
        let frame = Envelope::build(1, 1, 0, b"");
        assert!(matches!(decode_command(&frame).unwrap(), Command::Noop));
    }
}