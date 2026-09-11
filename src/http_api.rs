use hbb_common::log;
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use tiny_http::{Header, Method, Request, Response, Server};

/// Local HTTP API for web pages running on this machine.
///
/// - Listens on 127.0.0.1 only (never 0.0.0.0).
/// - Requires token: `Authorization: Bearer <token>` header or `?token=<token>` query.
/// - Token is read from config option `api-token` (GateDesk2.toml `[options]`).
/// - Host check: only `localhost` / `127.0.0.1` Host headers are accepted
///   (DNS-rebinding protection).
/// - CORS: `Access-Control-Allow-Origin` is echoed only for trusted origins — local
///   origins (localhost / 127.0.0.1, any port) and the origins listed in the
///   `api-cors-origin` config option (comma separated). Other origins are rejected
///   with 403 and get no CORS header.
const PORT: u16 = 21120;

/// Unified cap for request bodies (bytes). /password and /voice payloads are small.
const MAX_BODY_BYTES: usize = 1024;

thread_local! {
    /// Origin allowed for the current request (empty when the request carried no
    /// Origin header or was rejected); drives the per-response CORS header.
    static CORS_ORIGIN: RefCell<String> = RefCell::new(String::new());
}

/// (target id, pid) of connect-session processes spawned by `POST /connect`.
/// `POST /disconnect` closes only these windows, never the main UI process.
static CONNECT_SESSIONS: OnceLock<Mutex<Vec<(String, u32)>>> = OnceLock::new();

fn connect_sessions() -> &'static Mutex<Vec<(String, u32)>> {
    CONNECT_SESSIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// The config file holds `api-token` in `[options]`; tighten its permissions to
/// owner-only on Unix so other local users cannot read the token.
#[cfg(unix)]
fn tighten_config_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let path = hbb_common::config::Config::file();
    match std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
        Ok(()) => log::info!(
            "config file permissions tightened to 0600: {}",
            path.display()
        ),
        Err(e) => log::debug!(
            "could not tighten config file permissions {}: {}",
            path.display(),
            e
        ),
    }
}

#[cfg(not(unix))]
fn tighten_config_permissions() {}

pub fn start() {
    tighten_config_permissions();
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
    let allowed = CORS_ORIGIN.with(|o| o.borrow().clone());
    if !allowed.is_empty() {
        if let Some(h) = header("Access-Control-Allow-Origin", &allowed) {
            response = response.with_header(h);
        }
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

/// First value of a request header, if present.
fn request_header<'a>(request: &'a Request, key: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(key))
        .map(|h| h.value.as_str())
}

/// The API binds 127.0.0.1 only; reject requests whose Host header is not
/// localhost/127.0.0.1 so a page served from another site cannot re-point its
/// requests at this API (DNS rebinding).
fn host_allowed(request: &Request) -> bool {
    let Some(host) = request_header(request, "Host") else {
        return false;
    };
    let host_port = host.split('/').next().unwrap_or("").trim();
    let host_only = host_port
        .rsplit_once(':')
        .map(|(h, p)| {
            if p.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                host_port
            }
        })
        .unwrap_or(host_port);
    let host_only = host_only.trim_matches(|c| c == '[' || c == ']');
    host_only.eq_ignore_ascii_case("localhost") || host_only.eq_ignore_ascii_case("127.0.0.1")
}

/// Whether a browser Origin header is trusted: a local origin (localhost /
/// 127.0.0.1, any port/scheme) or one explicitly listed in `[options]`
/// api-cors-origin (comma separated, e.g. http://192.168.1.10:3000 for the
/// GateDeskWeb business pages served from another address).
fn origin_allowed(origin: &str) -> bool {
    let lower = origin.to_ascii_lowercase();
    let Some(scheme_rest) = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = scheme_rest
        .split('/')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    let host_only = authority
        .rsplit_once(':')
        .map(|(h, p)| {
            if p.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                authority
            }
        })
        .unwrap_or(authority);
    let host_only = host_only.trim_matches(|c| c == '[' || c == ']');
    if host_only.eq_ignore_ascii_case("localhost") || host_only.eq_ignore_ascii_case("127.0.0.1") {
        return true;
    }
    let cfg = crate::ui_interface::get_option("api-cors-origin");
    cfg.split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .any(|s| s == lower)
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

fn read_body(request: &mut Request, max: usize) -> String {
    use std::io::Read;
    let mut s = String::new();
    let _ = request.as_reader().take(max as u64).read_to_string(&mut s);
    s
}

/// Extract a top-level string/bool/number field value from a small JSON body.
/// Handles escaped quotes/backslashes inside string values; good enough for the
/// fixed shapes this API accepts (`{"password":"..."}`, `{"enabled":true}`).
fn json_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let rest = &body[body.find(&needle)? + needle.len()..];
    let val = rest[rest.find(':')? + 1..].trim_start();
    if let Some(rest) = val.strip_prefix('"') {
        let mut out = String::new();
        let mut it = rest.chars();
        while let Some(c) = it.next() {
            match c {
                '"' => return Some(out),
                '\\' => match it.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => return Some(out),
                },
                c => out.push(c),
            }
        }
        Some(out)
    } else {
        let token: String = val
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != ',' && *c != '}')
            .collect();
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    }
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
                connect_sessions().lock().unwrap().push((id.clone(), child.id()));
                crate::audit::record(
                    "connect.start",
                    "operator",
                    0,
                    "ok",
                    serde_json::json!({"target_id": id.clone(), "relay": query_param(query, "relay") == "true"}),
                );
                respond(request, 200, format!("{{\"ok\":true,\"id\":\"{}\"}}", id))
            }
            Err(e) => {
                crate::audit::record(
                    "connect.start",
                    "operator",
                    0,
                    "err",
                    serde_json::json!({"target_id": id.clone(), "error": format!("{}", e)}),
                );
                respond(
                    request,
                    500,
                    format!("{{\"error\":\"failed to launch: {}\"}}", e),
                )
            }
        },
        Err(e) => {
            crate::audit::record(
                "connect.start",
                "operator",
                0,
                "err",
                serde_json::json!({"error": format!("{}", e)}),
            );
            respond(
                request,
                500,
                format!("{{\"error\":\"failed to locate exe: {}\"}}", e),
            )
        }
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

/// Terminate the connect-session process via `kill -TERM`. No new crate needed;
/// the spawned process is our own child so the signal is permitted.
#[cfg(unix)]
fn terminate_pid(target_pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-TERM", &target_pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(any(windows, unix)))]
fn terminate_pid(_target_pid: u32) -> bool {
    false
}

/// Whether a recorded connect-session process is still running.
#[cfg(unix)]
fn pid_alive(target_pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &target_pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn pid_alive(_target_pid: u32) -> bool {
    true // keep old semantics on platforms without a cheap liveness probe
}

/// Disconnect the remote-session processes spawned by `POST /connect`.
///
/// Safety: terminates ONLY the connect-session processes recorded by this API
/// (process ids captured at spawn time). It never touches the main GateDesk UI
/// process and performs no system-level action (no shutdown / logoff / reboot).
fn disconnect_api_sessions() -> usize {
    let mut sessions = connect_sessions().lock().unwrap();
    let mut closed = 0usize;
    let mut keep: Vec<(String, u32)> = Vec::new();
    for (id, pid) in sessions.iter() {
        if !pid_alive(*pid) {
            continue; // window already closed by the user -> session ended
        }
        if terminate_pid(*pid) {
            log::info!("http api disconnect terminated pid {}", pid);
            closed += 1;
        } else {
            keep.push((id.clone(), *pid)); // still alive but terminate failed
        }
    }
    *sessions = keep;
    closed
}

fn handle_disconnect(request: Request) {
    let closed = disconnect_api_sessions();
    if closed > 0 {
        crate::audit::record(
            "connect.close",
            "operator",
            0,
            "ok",
            serde_json::json!({"closed": closed}),
        );
    }
    respond(
        request,
        200,
        format!("{{\"ok\":true,\"closed\":{}}}", closed),
    );
}

/// Set the machine's connection password so the operator can reach it by
/// `id + password`. Uses the permanent-password primitive because the
/// one-time (temporary) password can only be auto-rotated, not set to a
/// caller-chosen value; the page should re-call this endpoint with a fresh
/// random value after each session to rotate the credential.
fn handle_password(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let password = json_field(&body, "password").unwrap_or_default();
    if password.is_empty() || password.len() > 64 {
        respond(
            request,
            400,
            "{\"ok\":false,\"error\":\"missing or invalid password\"}".to_owned(),
        );
        return;
    }
    let ok = crate::ui_interface::set_permanent_password_with_result(password);
    if ok {
        // Granting access: the customer (this machine) explicitly enabled
        // "assistable" by provisioning a connection credential (authorization
        // method A). Keep GateDesk's native permanent-password semantics; the
        // caller rotates the credential after each session.
        crate::audit::record(
            "auth.grant",
            "customer",
            0,
            "ok",
            serde_json::json!({"method": "password"}),
        );
        respond(request, 200, "{\"ok\":true}".to_owned());
    } else {
        crate::audit::record(
            "auth.grant",
            "customer",
            0,
            "err",
            serde_json::json!({"method": "password", "error": "failed to set password"}),
        );
        respond(
            request,
            500,
            "{\"ok\":false,\"error\":\"failed to set password\"}".to_owned(),
        );
    }
}

/// Toggle voice by driving the global `audio-input` option (which restarts the
/// audio service). This is a PoC approximation of per-session voice: the exact
/// session-level toggle needs a live in-process `Session` handle, which the
/// process-spawn connect model does not hold.
fn handle_voice(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    match json_field(&body, "enabled") {
        Some(v) if v == "true" || v == "false" => {
            let on = v == "true";
            crate::ui_interface::set_option(
                "audio-input".to_owned(),
                if on { "Y" } else { "" }.to_owned(),
            );
            crate::audit::record(
                if on { "voice.on" } else { "voice.off" },
                "operator",
                0,
                "ok",
                serde_json::json!({"method": "http-api"}),
            );
            respond(request, 200, format!("{{\"ok\":true,\"enabled\":{}}}", on));
        }
        _ => respond(
            request,
            400,
            "{\"ok\":false,\"error\":\"missing or invalid enabled\"}".to_owned(),
        ),
    }
}

/// Live session status. `in_session` reflects whether a connect-session spawned
/// by this API is still running (stale pids are pruned), and `peer_id` is that
/// session's target id. `online` reflects whether GateDesk has logged in to the
/// rendezvous server. True cross-process per-session state is out of scope for
/// the PoC.
fn handle_status(request: Request) {
    let online = crate::ui_interface::get_connect_status().status_num != 0;
    let assistable = crate::ui_interface::is_local_permanent_password_set();
    let mut sessions = connect_sessions().lock().unwrap();
    sessions.retain(|(_, pid)| pid_alive(*pid));
    let peer_id = sessions.last().map(|(id, _)| id.clone());
    drop(sessions);
    match peer_id {
        Some(id) => respond(
            request,
            200,
            format!(
                "{{\"online\":{},\"in_session\":true,\"peer_id\":\"{}\",\"assistable\":{}}}",
                online, id, assistable
            ),
        ),
        None => respond(
            request,
            200,
            format!(
                "{{\"online\":{},\"in_session\":false,\"peer_id\":null,\"assistable\":{}}}",
                online, assistable
            ),
        ),
    }
}

fn handle(request: Request) {
    // --- local API hardening: Host check, CORS, body-size cap ---------------
    // This thread may have served a previous request; its origin must not leak
    // into a response that is produced before CORS_ORIGIN is assigned below
    // (e.g. the 403 paths).
    CORS_ORIGIN.with(|o| o.borrow_mut().clear());
    // Bound to 127.0.0.1; still refuse foreign Host headers (DNS rebinding).
    if !host_allowed(&request) {
        respond(request, 403, "{\"error\":\"host not allowed\"}".to_owned());
        return;
    }
    let origin = request_header(&request, "Origin").unwrap_or("").trim().to_owned();
    if !origin.is_empty() && !origin_allowed(&origin) {
        respond(request, 403, "{\"error\":\"origin not allowed\"}".to_owned());
        return;
    }
    CORS_ORIGIN.with(|o| *o.borrow_mut() = origin);
    // Unified request-body cap.
    if let Some(len) = request_header(&request, "Content-Length") {
        if let Ok(n) = len.trim().parse::<usize>() {
            if n > MAX_BODY_BYTES {
                respond(request, 413, "{\"error\":\"payload too large\"}".to_owned());
                return;
            }
        }
    }

    // CORS preflight
    if request.method() == &Method::Options {
        let mut response = Response::empty(204);
        let allowed = CORS_ORIGIN.with(|o| o.borrow().clone());
        if allowed.is_empty() {
            let _ = request.respond(response);
            return;
        }
        for (k, v) in [
            ("Access-Control-Allow-Origin", allowed.as_str()),
            ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            ("Access-Control-Allow-Headers", "Authorization, Content-Type"),
            ("Access-Control-Max-Age", "600"),
        ] {
            if let Some(h) = header(k, v) {
                response = response.with_header(h);
            }
        }
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
        (&Method::Get, "/status") => {
            handle_status(request);
        }
        (&Method::Post, "/connect") => {
            handle_connect(request, query);
        }
        (&Method::Post, "/disconnect") => {
            handle_disconnect(request);
        }
        (&Method::Post, "/password") => {
            handle_password(request);
        }
        (&Method::Post, "/voice") => {
            handle_voice(request);
        }
        (&Method::Get, _) | (&Method::Post, _) => {
            respond(request, 404, "{\"error\":\"not found\"}".to_owned());
        }
        _ => {
            respond(request, 405, "{\"error\":\"method not allowed\"}".to_owned());
        }
    }
}


















