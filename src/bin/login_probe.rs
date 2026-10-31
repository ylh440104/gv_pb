use gvpb_login::Api;
use gvpb_login::DeviceIdentity;
use std::env;
use std::process::ExitCode;

fn value(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(item) = iter.next() {
        if item == flag {
            return iter.next().cloned();
        }
        if let Some(rest) = item.strip_prefix(flag) {
            if let Some(rest) = rest.strip_prefix('=') {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn usage() {
    println!("usage:");
    println!("  login_probe sign   --path PATH [--method METHOD] [--body BODY] [--ts SECONDS]");
    println!("  login_probe check  --token TOKEN [--device-id ID] [--user-id ID]");
    println!("  login_probe qr     [--device-id ID]");
    println!("  login_probe info   --token TOKEN --csrf CSRF [--user-id ID]");
}

fn identity(args: &[String]) -> DeviceIdentity {
    DeviceIdentity {
        client_id: value(args, "--client-id").unwrap_or_else(|| String::from("probe-client")),
        device_id: value(args, "--device-id").unwrap_or_else(|| String::from("probe-device")),
        system_id: value(args, "--system-id").unwrap_or_else(|| String::from("probe-system")),
        user_id: value(args, "--user-id").unwrap_or_default(),
        guest_id: String::new(),
    }
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn run_sign(args: &[String]) -> ExitCode {
    let path = match value(args, "--path") {
        Some(p) => p,
        None => {
            println!("missing --path");
            return ExitCode::from(2);
        }
    };
    let method = value(args, "--method").unwrap_or_else(|| String::from("POST"));
    let body = value(args, "--body").unwrap_or_else(|| String::from("{}"));
    let ts = value(args, "--ts")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or_else(now_seconds);

    let mut signer = gvpb_login::RequestSigner::default();
    signer.identity = identity(args);

    let params = signer.params(ts);
    println!("payload:");
    println!("  {}", gvpb_login::RequestSigner::payload(&method, &path, &params, &body));
    println!("headers:");
    for (name, value) in signer.headers(&method, &path, &body, ts) {
        println!("  {}: {}", name, value);
    }
    ExitCode::SUCCESS
}

fn run_qr(args: &[String]) -> ExitCode {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            println!("runtime error: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let id = identity(args);
    runtime.block_on(async move {
        let api = match Api::new(id) {
            Ok(a) => a,
            Err(e) => {
                println!("client error: {}", e);
                return ExitCode::FAILURE;
            }
        };
        match api.wait_qr(|ticket| {
            println!("scan this code with the UU Remote app:");
            println!("  content: {}", ticket.content);
            if !ticket.url.is_empty() {
                println!("  url:     {}", ticket.url);
            }
            println!("  qr_id:   {}", ticket.qr_id);
        }).await {
            Ok(session) => {
                println!("token:   {}", session.token);
                println!("csrf:    {}", session.csrf);
                println!("user_id: {}", session.user_id);
                ExitCode::SUCCESS
            }
            Err(e) => {
                println!("login failed: {}", e);
                ExitCode::FAILURE
            }
        }
    })
}

fn run_check(args: &[String]) -> ExitCode {
    let token = match value(args, "--token") {
        Some(t) => t,
        None => {
            println!("missing --token");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            println!("runtime error: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let id = identity(args);
    runtime.block_on(async move {
        let api = match Api::new(id) {
            Ok(a) => a,
            Err(e) => {
                println!("client error: {}", e);
                return ExitCode::FAILURE;
            }
        };
        match api.check_uu_token(&token).await {
            Ok(session) => {
                println!("accepted");
                println!("user_id: {}", session.user_id);
                println!("nickname: {}", session.nickname);
                ExitCode::SUCCESS
            }
            Err(e) => {
                println!("rejected: {}", e);
                ExitCode::FAILURE
            }
        }
    })
}

fn run_info(args: &[String]) -> ExitCode {
    let token = match value(args, "--token") {
        Some(t) => t,
        None => {
            println!("missing --token");
            return ExitCode::from(2);
        }
    };
    let csrf = value(args, "--csrf").unwrap_or_default();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            println!("runtime error: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let id = identity(args);
    runtime.block_on(async move {
        let mut api = match Api::new(id) {
            Ok(a) => a,
            Err(e) => {
                println!("client error: {}", e);
                return ExitCode::FAILURE;
            }
        };
        api.set_token(token, csrf);
        match api.user_info().await {
            Ok(body) => {
                println!("{}", body);
                ExitCode::SUCCESS
            }
            Err(e) => {
                println!("failed: {}", e);
                ExitCode::FAILURE
            }
        }
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        return ExitCode::from(2);
    }
    match args[0].as_str() {
        "sign" => run_sign(&args[1..]),
        "qr" => run_qr(&args[1..]),
        "check" => run_check(&args[1..]),
        "info" => run_info(&args[1..]),
        "--help" | "-h" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            println!("unknown command: {}", other);
            usage();
            ExitCode::from(2)
        }
    }
}