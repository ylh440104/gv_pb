use gvpb_login::control;
use gvpb_login::signaling;
use gvpb_login::wire;
use gvpb_login::ws_frame;
use serde_json::Value;
use std::env;
use std::process::ExitCode;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

fn arg(args: &[String], flag: &str) -> Option<String> {
    let mut cursor = 0;
    while cursor < args.len() {
        if args[cursor] == flag {
            return args.get(cursor + 1).cloned();
        }
        if args[cursor].starts_with(flag) {
            if let Some(rest) = args[cursor].strip_prefix(flag) {
                if let Some(rest) = rest.strip_prefix('=') {
                    return Some(rest.to_string());
                }
            }
        }
        cursor += 1;
    }
    None
}

fn seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let cleaned: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() % 2 != 0 {
        return None;
    }
    let bytes = cleaned.as_bytes();
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let mut at = 0;
    while at < bytes.len() {
        let high = (bytes[at] as char).to_digit(16)?;
        let low = (bytes[at + 1] as char).to_digit(16)?;
        out.push(((high << 4) | low) as u8);
        at += 2;
    }
    Some(out)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

fn print_usage() {
    println!("commands:");
    println!("  decode --hex HEX            parse an application frame and print the command");
    println!("  reply  --hex HEX            build the matching RpcResponse envelope");
    println!("  action --id N               build a SimpleAction envelope");
    println!("  frames --file PATH          decode every engine.io frame in a captured transcript");
    println!("  script                      run the signalling state machine over a fixture transcript");
}

fn run_decode(args: &[String]) -> ExitCode {
    let raw = match arg(args, "--hex").and_then(|t| decode_hex(&t)) {
        Some(bytes) => bytes,
        None => {
            println!("missing or malformed --hex");
            return ExitCode::from(2);
        }
    };

    match control::decode_command(&raw) {
        Ok(command) => {
            println!("command {}", control::to_json(&command));
            match control::reply_for(&command) {
                Some(reply) => {
                    let frame = control::wrap_reply(&reply, 1, seconds());
                    println!("reply-hint {}", to_hex(&frame));
                }
                None => println!("reply-hint none"),
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("decode failed: {}", e);
            ExitCode::FAILURE
        }
    }
}

fn run_reply(args: &[String]) -> ExitCode {
    let raw = match arg(args, "--hex").and_then(|t| decode_hex(&t)) {
        Some(bytes) => bytes,
        None => {
            println!("missing or malformed --hex");
            return ExitCode::from(2);
        }
    };

    let command = match control::decode_command(&raw) {
        Ok(c) => c,
        Err(e) => {
            println!("decode failed: {}", e);
            return ExitCode::FAILURE;
        }
    };

    match control::reply_for(&command) {
        Some(reply) => {
            let frame = control::wrap_reply(&reply, 1, seconds());
            println!("{}", to_hex(&frame));
            ExitCode::SUCCESS
        }
        None => {
            println!("no reply expected for {}", control::to_json(&command));
            ExitCode::SUCCESS
        }
    }
}

fn run_action(args: &[String]) -> ExitCode {
    let id = match arg(args, "--id").and_then(|t| t.parse::<u64>().ok()) {
        Some(v) => v,
        None => {
            println!("missing --id");
            return ExitCode::from(2);
        }
    };
    let frame = control::simple_action_frame(id, 1, seconds());
    println!("{}", to_hex(&frame));
    ExitCode::SUCCESS
}

fn run_frames(args: &[String]) -> ExitCode {
    let path = match arg(args, "--file") {
        Some(p) => p,
        None => {
            println!("missing --file");
            return ExitCode::from(2);
        }
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            println!("read failed: {}", e);
            return ExitCode::FAILURE;
        }
    };

    let mut session = signaling::Decoder::default();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let signals = session.process(trimmed);
        for signal in &signals {
            println!("{}", describe(signal));
        }
    }
    ExitCode::SUCCESS
}

fn describe(signal: &signaling::Signal) -> String {
    match signal {
        signaling::Signal::Connected => String::from("connected"),
        signaling::Signal::RoomInfo(payload) => format!("room_info {}", compact(payload)),
        signaling::Signal::DeviceChanged(payload) => format!("device_changed {}", compact(payload)),
        signaling::Signal::BeControlled(request) => format!(
            "be_controlled capture_type={} ice_id={:?} client_id={:?}",
            request.capture_type, request.ice_id, request.client_id
        ),
        signaling::Signal::Soac(soac) => format!(
            "soac type={} sdp_len={} gzip_sdp_len={} ice_id={:?} client_id={}",
            soac.kind,
            soac.sdp.len(),
            soac.gzip_sdp.len(),
            soac.ice_id_string(),
            soac.client_id
        ),
        signaling::Signal::Released(payload) => format!("released {}", compact(payload)),
        signaling::Signal::Left(payload) => format!("left {}", compact(payload)),
        signaling::Signal::Disconnected(reason) => format!("disconnected {}", reason),
    }
}

fn compact(value: &Value) -> String {
    let text = value.to_string();
    if text.len() > 160 {
        format!("{}...", &text[..160])
    } else {
        text
    }
}

fn run_script() -> ExitCode {
    let mut session = gvpb_login::SignalSession::new(signaling::RoomData {
        server: String::from("wss://placeholder.invalid"),
        token: String::from("placeholder-token"),
        ..Default::default()
    });

    let transcript = vec![
        "0{\"sid\":\"s1\",\"pingInterval\":25000,\"pingTimeout\":20000}",
        "40{\"sid\":\"s1\"}",
        "4310[\"success\",{\"reconnect_key\":\"rk\"}]",
        "42[\"be-controlled\",{\"capture_type\":4,\"ice_id\":\"ice-1\",\"client_id\":\"c-1\"}]",
        "42[\"soac\",{\"client_id\":\"c-1\",\"data\":{\"type\":\"offer\",\"sdp\":\"v=0\\r\\n\",\"ice_id\":\"ice-1\"}}]",
        "42[\"soac\",{\"client_id\":\"c-1\",\"data\":{\"type\":\"candidate\",\"candidate\":{\"candidate\":\"candidate:1 1 udp 1 1.2.3.4 9 typ host\"}}}]",
        "42[\"soac\",{\"client_id\":\"c-1\",\"data\":{\"type\":\"answer\",\"sdp\":\"v=0\\r\\n\"}}]",
        "42[\"released\",{\"ice_id\":\"ice-1\"}]",
        "42[\"left\",{\"ice_id\":\"ice-1\"}]",
    ];

    for line in transcript {
        let signals = session.absorb(line);
        for signal in &signals {
            println!("{}", describe(signal));
            if let signaling::Signal::Soac(soac) = signal {
                if soac.is_offer() {
                    let answer = session.soac_answer("v=0\r\n", soac.ice_id_string().as_deref(), &soac.client_id);
                    println!("send {}", answer);
                }
            }
        }
    }

    println!("ready={}", session.ready());

    let control_frame = control::simple_action_frame(4, 7, seconds());
    println!("action4 {}", to_hex(&control_frame));
    if let Ok(command) = control::decode_command(&control_frame) {
        println!("action4 decoded {}", control::to_json(&command));
    }

    let frame_bytes = ws_frame::encode_text("2", [1, 2, 3, 4]);
    println!("ws_frame {}", to_hex(&frame_bytes));
    if let Ok(parsed) = ws_frame::decode(&frame_bytes) {
        println!("ws_decoded opcode={} len={}", parsed.opcode, parsed.payload.len());
    }

    let envelope = wire::Envelope::build(1, seconds(), wire::MESSAGE_INPUT_EVENT, &[]);
    println!("envelope {}", to_hex(&envelope));

    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    match args[0].as_str() {
        "decode" => run_decode(&args[1..]),
        "reply" => run_reply(&args[1..]),
        "action" => run_action(&args[1..]),
        "frames" => run_frames(&args[1..]),
        "script" => run_script(),
        "--help" | "-h" | "help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            println!("unknown command {}", other);
            print_usage();
            ExitCode::from(2)
        }
    }
}