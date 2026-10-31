use std::collections::BTreeMap;

fn read_varint(buf: &[u8], at: usize) -> Result<(u64, usize), &'static str> {
    let mut value = 0u64;
    let mut index = at;
    let mut shift = 0u32;
    while index < buf.len() {
        let byte = buf[index];
        index += 1;
        if shift == 63 && byte > 1 {
            return Err("overflow");
        }
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((value, index));
        }
        shift += 7;
        if shift > 63 {
            return Err("overflow");
        }
    }
    Err("truncated")
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(((value & 0x7F) as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn write_tag(out: &mut Vec<u8>, number: u32, wire: u8) {
    write_varint(out, ((number as u64) << 3) | wire as u64);
}

enum Slot {
    Varint(u64),
    Chunk(Vec<u8>),
    Word32([u8; 4]),
    Word64([u8; 8]),
}

fn decode(buf: &[u8]) -> Result<Vec<(u32, Slot)>, &'static str> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < buf.len() {
        let (key, next) = read_varint(buf, at)?;
        at = next;
        let number = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        if number == 0 || number > 0x001F_FFFF {
            return Err("bad field");
        }
        let slot = match wire {
            0 => {
                let (raw, next) = read_varint(buf, at)?;
                at = next;
                Slot::Varint(raw)
            }
            1 => {
                if at + 8 > buf.len() {
                    return Err("truncated");
                }
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&buf[at..at + 8]);
                at += 8;
                Slot::Word64(raw)
            }
            2 => {
                let (size, next) = read_varint(buf, at)?;
                at = next;
                let size = size as usize;
                if at + size > buf.len() {
                    return Err("truncated");
                }
                let raw = buf[at..at + size].to_vec();
                at += size;
                Slot::Chunk(raw)
            }
            5 => {
                if at + 4 > buf.len() {
                    return Err("truncated");
                }
                let mut raw = [0u8; 4];
                raw.copy_from_slice(&buf[at..at + 4]);
                at += 4;
                Slot::Word32(raw)
            }
            _ => return Err("bad wire type"),
        };
        out.push((number, slot));
    }
    Ok(out)
}

fn chunk_of<'a>(fields: &'a [(u32, Slot)], number: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|(n, s)| match s {
        Slot::Chunk(raw) if *n == number => Some(raw.as_slice()),
        _ => None,
    })
}

fn varint_of(fields: &[(u32, Slot)], number: u32) -> Option<u64> {
    fields.iter().find_map(|(n, s)| match s {
        Slot::Varint(v) if *n == number => Some(*v),
        _ => None,
    })
}

fn chunks_of(fields: &[(u32, Slot)], number: u32) -> Vec<Vec<u8>> {
    fields
        .iter()
        .filter_map(|(n, s)| match s {
            Slot::Chunk(raw) if *n == number => Some(raw.clone()),
            _ => None,
        })
        .collect()
}

fn envelope_build(seq: u64, ts: u64, arm: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_tag(&mut out, 1, 0);
    write_varint(&mut out, seq);
    write_tag(&mut out, 2, 0);
    write_varint(&mut out, ts);
    write_tag(&mut out, arm, 2);
    write_varint(&mut out, body.len() as u64);
    out.extend_from_slice(body);
    out
}

struct Env {
    seq: u64,
    timestamp: u64,
    arm: u32,
    body: Vec<u8>,
}

fn envelope_parse(buf: &[u8]) -> Result<Env, &'static str> {
    let fields = decode(buf)?;
    let seq = varint_of(&fields, 1).unwrap_or(0);
    let timestamp = varint_of(&fields, 2).unwrap_or(0);
    for (number, slot) in &fields {
        if *number == 1 || *number == 2 {
            continue;
        }
        if let Slot::Chunk(raw) = slot {
            return Ok(Env {
                seq,
                timestamp,
                arm: *number,
                body: raw.clone(),
            });
        }
    }
    Ok(Env {
        seq,
        timestamp,
        arm: 0,
        body: Vec::new(),
    })
}

fn reply_encode(request_id: i64, arm: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut header = Vec::new();
    write_tag(&mut header, 1, 0);
    write_varint(&mut header, request_id as u64);
    write_tag(&mut out, 1, 2);
    write_varint(&mut out, header.len() as u64);
    out.extend_from_slice(&header);
    if arm != 0 {
        write_tag(&mut out, arm, 2);
        write_varint(&mut out, body.len() as u64);
        out.extend_from_slice(body);
    }
    out
}

fn error_body(code: i32, message: &str) -> Vec<u8> {
    let mut error = Vec::new();
    write_tag(&mut error, 1, 0);
    write_varint(&mut error, code as u64);
    if !message.is_empty() {
        write_tag(&mut error, 2, 2);
        write_varint(&mut error, message.len() as u64);
        error.extend_from_slice(message.as_bytes());
    }
    let mut inner = Vec::new();
    write_tag(&mut inner, 1, 2);
    write_varint(&mut inner, error.len() as u64);
    inner.extend_from_slice(&error);
    inner
}

fn reply_parse(buf: &[u8]) -> Result<(i64, u32, Vec<u8>), &'static str> {
    let fields = decode(buf)?;
    let header = chunk_of(&fields, 1).ok_or("missing header")?;
    let header_fields = decode(header)?;
    let request_id = varint_of(&header_fields, 1).unwrap_or(0) as i64;
    for (number, slot) in &fields {
        if *number == 1 {
            continue;
        }
        if let Slot::Chunk(raw) = slot {
            return Ok((request_id, *number, raw.clone()));
        }
    }
    Ok((request_id, 0, Vec::new()))
}

fn request_id_of(buf: &[u8]) -> Option<i64> {
    let fields = decode(buf).ok()?;
    let header = chunk_of(&fields, 1)?;
    let parsed = decode(header).ok()?;
    varint_of(&parsed, 1).map(|v| v as i64)
}

fn simple_action(action: u64, seq: u64, ts: u64) -> Vec<u8> {
    let mut body = Vec::new();
    write_tag(&mut body, 1, 0);
    write_varint(&mut body, action);
    envelope_build(seq, ts, 3, &body)
}

fn ws_encode(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::new();
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
    for (i, b) in payload.iter().enumerate() {
        out.push(b ^ mask[i % 4]);
    }
    out
}

struct WsFrame {
    opcode: u8,
    payload: Vec<u8>,
    consumed: usize,
}

fn ws_decode(buf: &[u8]) -> Result<WsFrame, &'static str> {
    if buf.len() < 2 {
        return Err("incomplete");
    }
    let first = buf[0];
    let second = buf[1];
    if first & 0x70 != 0 {
        return Err("reserved");
    }
    let opcode = first & 0x0F;
    let masked = second & 0x80 != 0;
    let marker = second & 0x7F;
    let (length, mut at) = match marker {
        126 => {
            if buf.len() < 4 {
                return Err("incomplete");
            }
            (u16::from_be_bytes([buf[2], buf[3]]) as usize, 4)
        }
        127 => {
            if buf.len() < 10 {
                return Err("incomplete");
            }
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&buf[2..10]);
            (u64::from_be_bytes(raw) as usize, 10)
        }
        small => (small as usize, 2),
    };
    if opcode >= 8 && length > 125 {
        return Err("control too large");
    }
    let key = if masked {
        if buf.len() < at + 4 {
            return Err("incomplete");
        }
        let mut k = [0u8; 4];
        k.copy_from_slice(&buf[at..at + 4]);
        at += 4;
        Some(k)
    } else {
        None
    };
    if buf.len() < at + length {
        return Err("incomplete");
    }
    let mut payload = buf[at..at + length].to_vec();
    if let Some(k) = key {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= k[i % 4];
        }
    }
    Ok(WsFrame {
        opcode,
        payload,
        consumed: at + length,
    })
}

fn engine_payload(method: &str, path: &str, params: &BTreeMap<String, String>, body: &str) -> String {
    let mut parts = Vec::new();
    for (name, value) in params.iter() {
        let lowered = name.to_ascii_lowercase();
        if lowered.starts_with("x-param-") && lowered != "x-param-sign" && !value.is_empty() {
            parts.push(format!("{}={}", lowered, value));
        }
    }
    format!("{}{}{}{}", method.to_ascii_uppercase(), path, parts.join("&"), body)
}

fn check(label: &str, ok: bool, pass: &mut u32, fail: &mut u32) {
    println!("  {:<52} {}", label, if ok { "ok" } else { "FAIL" });
    if ok {
        *pass += 1;
    } else {
        *fail += 1;
    }
}

fn main() {
    let mut pass = 0u32;
    let mut fail = 0u32;

    println!("protobuf wire");
    check(
        "varint 1 byte",
        {
            let mut v = Vec::new();
            write_varint(&mut v, 1);
            v == vec![0x01]
        },
        &mut pass,
        &mut fail,
    );
    check(
        "varint roundtrip boundary set",
        [127u64, 128, 16383, 16384, u32::MAX as u64, u64::MAX]
            .iter()
            .all(|want| {
                let mut v = Vec::new();
                write_varint(&mut v, *want);
                read_varint(&v, 0).map(|(got, at)| got == *want && at == v.len()) == Ok(true)
            }),
        &mut pass,
        &mut fail,
    );
    check(
        "varint rejects truncation",
        read_varint(&[0x80], 0).is_err(),
        &mut pass,
        &mut fail,
    );

    println!("envelope");
    let body = b"hello";
    let wire = envelope_build(77, 1700000000, 21, body);
    let parsed = envelope_parse(&wire).unwrap();
    check("seq preserved", parsed.seq == 77, &mut pass, &mut fail);
    check("timestamp preserved", parsed.timestamp == 1700000000, &mut pass, &mut fail);
    check("arm is rpc_request 21", parsed.arm == 21, &mut pass, &mut fail);
    check("body preserved", parsed.body == body, &mut pass, &mut fail);

    let arm18 = envelope_parse(&envelope_build(1, 1, 18, b"x")).unwrap();
    check("codec_negotiation arm 18", arm18.arm == 18, &mut pass, &mut fail);
    let arm20 = envelope_parse(&envelope_build(1, 1, 20, b"x")).unwrap();
    check("input_event arm 20", arm20.arm == 20, &mut pass, &mut fail);
    let arm22 = envelope_parse(&envelope_build(1, 1, 22, b"x")).unwrap();
    check("rpc_response arm 22", arm22.arm == 22, &mut pass, &mut fail);

    println!("rpc reply");
    let reply = reply_encode(42, 2, &error_body(0, ""));
    let (rid, arm, _) = reply_parse(&reply).unwrap();
    check("request id echoed", rid == 42, &mut pass, &mut fail);
    check("response arm capture_setting 2", arm == 2, &mut pass, &mut fail);

    let with_err = error_body(17, "no display");
    let inner_fields = decode(&error_body(17, "no display")).unwrap();
    let err_chunk = chunk_of(&inner_fields, 1).unwrap();
    let err_fields = decode(err_chunk).unwrap();
    check("error code 17", varint_of(&err_fields, 1) == Some(17), &mut pass, &mut fail);
    check(
        "error text",
        chunk_of(&err_fields, 2) == Some(&b"no display"[..]),
        &mut pass,
        &mut fail,
    );
    check("error body non empty", !with_err.is_empty(), &mut pass, &mut fail);

    let mut rpc = Vec::new();
    let mut header = Vec::new();
    write_tag(&mut header, 1, 0);
    write_varint(&mut header, 1234);
    write_tag(&mut rpc, 1, 2);
    write_varint(&mut rpc, header.len() as u64);
    rpc.extend_from_slice(&header);
    check(
        "request id extracted",
        request_id_of(&rpc) == Some(1234),
        &mut pass,
        &mut fail,
    );

    println!("simple action");
    let action = simple_action(4, 9, 1700);
    let env = envelope_parse(&action).unwrap();
    check("action frame arm 3", env.arm == 3, &mut pass, &mut fail);
    let fields = decode(&env.body).unwrap();
    check(
        "action id 4 is capture_desktop",
        varint_of(&fields, 1) == Some(4),
        &mut pass,
        &mut fail,
    );
    check("action frame seq", env.seq == 9, &mut pass, &mut fail);

    println!("control plane requests");
    let mut capture = Vec::new();
    write_tag(&mut capture, 1, 0);
    write_varint(&mut capture, 2);
    write_tag(&mut capture, 2, 0);
    write_varint(&mut capture, 3);
    write_tag(&mut capture, 5, 0);
    write_varint(&mut capture, 2560);
    write_tag(&mut capture, 6, 0);
    write_varint(&mut capture, 1440);
    write_tag(&mut capture, 13, 0);
    write_varint(&mut capture, 2);

    let mut rpc_body = Vec::new();
    let mut rpc_header = Vec::new();
    write_tag(&mut rpc_header, 1, 0);
    write_varint(&mut rpc_header, 41);
    write_tag(&mut rpc_body, 1, 2);
    write_varint(&mut rpc_body, rpc_header.len() as u64);
    rpc_body.extend_from_slice(&rpc_header);
    write_tag(&mut rpc_body, 2, 2);
    write_varint(&mut rpc_body, capture.len() as u64);
    rpc_body.extend_from_slice(&capture);

    let frame = envelope_build(1, 1, 21, &rpc_body);
    let env = envelope_parse(&frame).unwrap();
    let rpc_fields = decode(&env.body).unwrap();
    check("rpc arm 21 decoded", env.arm == 21, &mut pass, &mut fail);
    check(
        "request id 41",
        request_id_of(&env.body) == Some(41),
        &mut pass,
        &mut fail,
    );
    let cap = chunk_of(&rpc_fields, 2).unwrap();
    let cap_fields = decode(cap).unwrap();
    check("fps 2 (FPS_60)", varint_of(&cap_fields, 1) == Some(2), &mut pass, &mut fail);
    check("quality 3 (HD)", varint_of(&cap_fields, 2) == Some(3), &mut pass, &mut fail);
    check("width 2560", varint_of(&cap_fields, 5) == Some(2560), &mut pass, &mut fail);
    check("height 1440", varint_of(&cap_fields, 6) == Some(1440), &mut pass, &mut fail);
    check("codec 2 (H265)", varint_of(&cap_fields, 13) == Some(2), &mut pass, &mut fail);

    println!("touch input");
    let mut point = Vec::new();
    write_tag(&mut point, 1, 0);
    write_varint(&mut point, 3);
    write_tag(&mut point, 2, 5);
    point.extend_from_slice(&12.5f32.to_le_bytes());
    write_tag(&mut point, 3, 5);
    point.extend_from_slice(&34.5f32.to_le_bytes());

    let mut touch = Vec::new();
    write_tag(&mut touch, 1, 0);
    write_varint(&mut touch, 2);
    write_tag(&mut touch, 2, 2);
    write_varint(&mut touch, point.len() as u64);
    touch.extend_from_slice(&point);

    let mut event = Vec::new();
    write_tag(&mut event, 1, 2);
    write_varint(&mut event, touch.len() as u64);
    event.extend_from_slice(&touch);

    let input_frame = envelope_build(1, 1, 20, &event);
    let env = envelope_parse(&input_frame).unwrap();
    let input_fields = decode(&env.body).unwrap();
    let touch_chunk = chunk_of(&input_fields, 1).unwrap();
    let touch_fields = decode(touch_chunk).unwrap();
    check("event type 2", varint_of(&touch_fields, 1) == Some(2), &mut pass, &mut fail);
    let points = chunks_of(&touch_fields, 2);
    check("one touch point", points.len() == 1, &mut pass, &mut fail);
    let pf = decode(&points[0]).unwrap();
    check("point id 3", varint_of(&pf, 1) == Some(3), &mut pass, &mut fail);
    let x = pf
        .iter()
        .find_map(|(n, s)| match s {
            Slot::Word32(raw) if *n == 2 => Some(f32::from_le_bytes(*raw)),
            _ => None,
        })
        .unwrap_or(0.0);
    check("point x 12.5", (x - 12.5).abs() < 0.001, &mut pass, &mut fail);

    println!("websocket frames");
    let masked = ws_encode(1, b"42[\"room_info\"]", [1, 2, 3, 4]);
    let decoded = ws_decode(&masked).unwrap();
    check("text opcode", decoded.opcode == 1, &mut pass, &mut fail);
    check(
        "payload roundtrip",
        decoded.payload == b"42[\"room_info\"]",
        &mut pass,
        &mut fail,
    );
    check("consumed length", decoded.consumed == masked.len(), &mut pass, &mut fail);

    let long = "z".repeat(300);
    let big = ws_encode(1, long.as_bytes(), [9, 9, 9, 9]);
    check("16 bit length marker", big[1] & 0x7F == 126, &mut pass, &mut fail);
    check(
        "16 bit payload roundtrip",
        ws_decode(&big).unwrap().payload.len() == 300,
        &mut pass,
        &mut fail,
    );

    let huge = "q".repeat(70000);
    let giant = ws_encode(1, huge.as_bytes(), [5, 5, 5, 5]);
    check("64 bit length marker", giant[1] & 0x7F == 127, &mut pass, &mut fail);
    check(
        "64 bit payload roundtrip",
        ws_decode(&giant).unwrap().payload.len() == 70000,
        &mut pass,
        &mut fail,
    );

    let server_frame = vec![0x81, 0x03, b'a', b'b', b'c'];
    check(
        "unmasked server frame",
        ws_decode(&server_frame).unwrap().payload == b"abc",
        &mut pass,
        &mut fail,
    );
    check("incomplete reported", ws_decode(&[0x81]).is_err(), &mut pass, &mut fail);
    check(
        "reserved bits rejected",
        ws_decode(&[0xF1, 0x00]).is_err(),
        &mut pass,
        &mut fail,
    );

    let mut oversized = vec![0x88, 0x7F];
    oversized.extend_from_slice(&200u64.to_be_bytes());
    check(
        "oversized control rejected",
        ws_decode(&oversized).is_err(),
        &mut pass,
        &mut fail,
    );

    println!("signature payload");
    let mut params = BTreeMap::new();
    params.insert("x-param-vn".to_string(), "2".to_string());
    params.insert("x-param-cnt".to_string(), "1".to_string());
    check(
        "payload ordering",
        engine_payload("post", "/api/v1/x", &params, "BODY")
            == "POST/api/v1/xx-param-cnt=1&x-param-vn=2BODY",
        &mut pass,
        &mut fail,
    );

    let mut blank = BTreeMap::new();
    blank.insert("x-param-opr".to_string(), String::new());
    blank.insert("x-param-cnt".to_string(), "1".to_string());
    check(
        "empty value skipped",
        engine_payload("GET", "/p", &blank, "") == "GET/px-param-cnt=1",
        &mut pass,
        &mut fail,
    );

    println!("\n{} passed, {} failed", pass, fail);
    if fail > 0 {
        std::process::exit(1);
    }
}