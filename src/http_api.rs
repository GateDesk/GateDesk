use hbb_common::log;
use std::sync::{Mutex, OnceLock};
use tiny_http::{Header, Method, Request, Response, Server};

/// Local HTTP API for web pages running on this machine.
///
/// - Listens on 127.0.0.1 only (never 0.0.0.0).
/// - Requires token: `Authorization: Bearer <token>` header or `?token=<token>` query.
/// - Token is read from config option `api-token` (GateDesk.toml `[options]`).
/// - CORS: `Access-Control-Allow-Origin: *` so business web pages can fetch it.
const PORT: u16 = 21120;

/// Pids of the connect-session processes spawned by `POST /connect`.
/// `POST /disconnect` closes only these windows, never the main UI process.
static CONNECT_PIDS: OnceLock<Mutex<Vec<u32>>> = OnceLock::new();

fn connect_pids() -> &'static Mutex<Vec<u32>> {
    CONNECT_PIDS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn start() {
    std::thread::spawn(|| {
        let addr = format!("127.0.0.1:{}", PORT);
        let server = match Server::http(&addr) {
            Ok(s) => s,
            Err(e) => {
                // Port already taken by another GateDesk process (e.g. --server);
                // silently disable in this process.
                log::info!("http api disabled, bind {} failed: {}", addr, e);
                return;
            }
        };
        log::info!("http api listening on http://{}", addr);
        for request in server.incoming_requests() {
            handle(request);
        }
    });
}

fn header(k: &str, v: &str) -> Option<Header> {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).ok()
}

fn respond(request: Request, status: u16, body: String) {
    let mut response = Response::from_string(body).with_status_code(status);
    if let Some(h) = header("Access-Control-Allow-Origin", "*") {
        response = response.with_header(h);
    }
    if let Some(h) = header("Content-Type", "application/json; charset=utf-8") {
        response = response.with_header(h);
    }
    let _ = request.respond(response);
}

fn extract_token(request: &Request, query: &str) -> String {
    for h in request.headers() {
        if h.field.equiv("Authorization") {
            let v = h.value.as_str().trim();
            if let Some(t) = v.strip_prefix("Bearer ") {
                return t.trim().to_owned();
            }
        }
    }
    for pair in query.split('&') {
        if let Some(t) = pair.strip_prefix("token=") {
            return t.to_owned();
        }
    }
    "".to_owned()
}

fn query_param(query: &str, key: &str) -> String {
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix(&format!("{}=", key)) {
            return percent_decode(v);
        }
    }
    "".to_owned()
}

fn percent_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn handle_connect(request: Request, query: &str) {
    let id = query_param(query, "id");
    if id.is_empty() || id.len() > 128 {
        respond(
            request,
            400,
            "{\"error\":\"missing or invalid id\"}".to_owned(),
        );
        return;
    }
    // Password always occupies the 3rd positional arg (see ui.rs arg parsing);
    // empty password makes the connect window prompt for it.
    let password = query_param(query, "password");
    let mut args: Vec<String> = vec!["--connect".to_owned(), id.clone(), password];
    if query_param(query, "relay") == "true" {
        args.push("--relay".to_owned());
    }
    match std::env::current_exe() {
        Ok(exe) => match std::process::Command::new(exe)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                log::info!("http api connect spawned pid {} args {:?}", child.id(), args);
                connect_pids().lock().unwrap().push(child.id());
                respond(request, 200, format!("{{\"ok\":true,\"id\":\"{}\"}}", id))
            }
            Err(e) => respond(
                request,
                500,
                format!("{{\"error\":\"failed to launch: {}\"}}", e),
            ),
        },
        Err(e) => respond(
            request,
            500,
            format!("{{\"error\":\"failed to locate exe: {}\"}}", e),
        ),
    }
}

/// Terminate the process with `target_pid` (its whole process tree) via `taskkill`.
/// Used only for the connect-session processes recorded by this API.
#[cfg(windows)]
fn terminate_pid(target_pid: u32) -> bool {
    std::process::Command::new("taskkill")
        .args(["/PID", &target_pid.to_string(), "/T", "/F"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn terminate_pid(_target_pid: u32) -> bool {
    false
}

/// Disconnect the remote-session processes spawned by `POST /connect`.
///
/// Safety: terminates ONLY the connect-session processes recorded by this API
/// (process ids captured at spawn time). It never touches the main GateDesk UI
/// process and performs no system-level action (no shutdown / logoff / reboot).
fn disconnect_api_sessions() -> usize {
    let mut pids = connect_pids().lock().unwrap();
    let mut closed = 0usize;
    let mut still_alive: Vec<u32> = Vec::new();
    for &pid in pids.iter() {
        if terminate_pid(pid) {
            log::info!("http api disconnect terminated pid {}", pid);
            closed += 1;
        } else {
            still_alive.push(pid); // process already gone -> session already ended
        }
    }
    *pids = still_alive;
    closed
}

fn handle_disconnect(request: Request) {
    let closed = disconnect_api_sessions();
    respond(
        request,
        200,
        format!("{{\"ok\":true,\"closed\":{}}}", closed),
    );
}

fn handle(request: Request) {
    // CORS preflight
    if request.method() == &Method::Options {
        let response = Response::empty(204);
        let response = match header("Access-Control-Allow-Origin", "*") {
            Some(h) => response.with_header(h),
            None => response,
        };
        let response = match header("Access-Control-Allow-Methods", "GET, POST, OPTIONS") {
            Some(h) => response.with_header(h),
            None => response,
        };
        let response = match header("Access-Control-Allow-Headers", "Authorization") {
            Some(h) => response.with_header(h),
            None => response,
        };
        let _ = request.respond(response);
        return;
    }

    // token check
    let expected = crate::ui_interface::get_option("api-token");
    if expected.is_empty() {
        respond(
            request,
            401,
            "{\"error\":\"api-token not configured\"}".to_owned(),
        );
        return;
    }
    let url = request.url().to_owned();
    let query = url.split_once('?').map(|x| x.1).unwrap_or("");
    let provided = extract_token(&request, query);
    if provided.is_empty() || provided != expected {
        respond(request, 401, "{\"error\":\"unauthorized\"}".to_owned());
        return;
    }

    let path = url.split('?').next().unwrap_or("");
    match (request.method(), path) {
        (&Method::Get, "/id") => {
            let id = crate::ipc::get_id();
            respond(request, 200, format!("{{\"id\":\"{}\"}}", id));
        }
        (&Method::Post, "/connect") => {
            handle_connect(request, query);
        }
        (&Method::Post, "/disconnect") => {
            handle_disconnect(request);
        }
        (&Method::Get, _) | (&Method::Post, _) => {
            respond(request, 404, "{\"error\":\"not found\"}".to_owned());
        }
        _ => {
            respond(request, 405, "{\"error\":\"method not allowed\"}".to_owned());
        }
    }
}












