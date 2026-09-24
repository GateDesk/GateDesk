use crate::ipc::{self, LocalApiAction, LocalApiCall, LocalApiReply};
use hbb_common::log;
use hbb_common::tokio;
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

/// Unified cap for request bodies (bytes). /password payloads are small.
const MAX_BODY_BYTES: usize = 1024;

thread_local! {
    /// Origin allowed for the current request (empty when the request carried no
    /// Origin header or was rejected); drives the per-response CORS header.
    static CORS_ORIGIN: RefCell<String> = RefCell::new(String::new());
}

/// The connect-session processes spawned by `POST /connect`, each with the peer it was
/// opened for and a way to recognise the process again later.
/// `POST /disconnect` closes only these windows, never the main UI process.
static CONNECT_SESSIONS: OnceLock<Mutex<Vec<ConnectSession>>> = OnceLock::new();

fn connect_sessions() -> &'static Mutex<Vec<ConnectSession>> {
    CONNECT_SESSIONS.get_or_init(|| Mutex::new(Vec::new()))
}

/// A session process `POST /connect` started, with what it takes to recognise it again.
#[derive(Clone)]
struct ConnectSession {
    /// The peer this session was opened for, which is what `/status` reports.
    id: String,
    pid: u32,
    /// When the system says the process was created; 0 when that cannot be read.
    ///
    /// A pid is not an identity - Windows reuses them - and `terminate_pid` kills a whole
    /// process tree, so a record whose pid now belongs to somebody else must never be acted
    /// on. Comparing this again before killing is what keeps a stale record from ending a
    /// process nobody asked about.
    created: u64,
}

impl ConnectSession {
    /// Whether the process recorded here is still the same one.
    fn alive(&self) -> bool {
        match process_created(self.pid) {
            // No such process any more.
            None => false,
            // Nothing was recorded to compare against, so all that can be said is that the
            // pid is taken. This is what the check did before there was anything better.
            Some(_) if self.created == 0 => true,
            Some(created) => created == self.created,
        }
    }
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
        // Chrome 私有网络访问(PNA):从局域网来源(如 http://192.168.x.x:3000)访问
        // 回环 127.0.0.1:21120 时,浏览器要求响应显式放行,否则拦截读取导致 fetch 失败。
        if let Some(h) = header("Access-Control-Allow-Private-Network", "true") {
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
                let pid = child.id();
                // Read while the process is certainly still the one just started, so that a
                // later `/disconnect` can tell it from whatever holds the pid after it.
                let created = process_created(pid).unwrap_or(0);
                log::info!("http api connect spawned pid {} args {:?}", pid, args);
                connect_sessions().lock().unwrap().push(ConnectSession {
                    id: id.clone(),
                    pid,
                    created,
                });
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
    use std::os::windows::process::CommandExt;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;
    // `taskkill` is a console program and this one has no console of its own, so without
    // the flag Windows gives the child a fresh console window - a black rectangle flashing
    // up once per pid, which is what a `/disconnect` over a list of stale pids looked like.
    std::process::Command::new("taskkill")
        .args(["/PID", &target_pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW.0)
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

/// When `pid` was created, or `None` when there is no such process.
///
/// This is what tells a recorded pid apart from a different process that has since taken
/// the same number.
#[cfg(windows)]
fn process_created(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let read = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
            .is_ok();
        let _ = CloseHandle(handle);
        if !read {
            // Gone between the open and the query.
            return None;
        }
        Some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }
}

/// When `pid` was created, or `None` when there is no such process.
///
/// The liveness probe is here but not a creation time that every Unix has, so 0 is
/// reported and the comparison falls back to asking whether the pid is taken. The kill
/// that makes the difference between the two worth having - one that takes the whole
/// process tree - is the Windows one.
#[cfg(unix)]
fn process_created(pid: u32) -> Option<u64> {
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    alive.then_some(0)
}

#[cfg(not(any(windows, unix)))]
fn process_created(_pid: u32) -> Option<u64> {
    // No liveness probe at all here: keep the old behaviour, which assumed it was there.
    Some(0)
}

/// Disconnect the remote-session processes spawned by `POST /connect`.
///
/// Safety: terminates ONLY the connect-session processes recorded by this API
/// (process ids captured at spawn time, each checked against the creation time of the
/// process now holding that pid). It never touches the main GateDesk UI process and
/// performs no system-level action (no shutdown / logoff / reboot).
fn disconnect_api_sessions() -> usize {
    let mut sessions = connect_sessions().lock().unwrap();
    let mut closed = 0usize;
    let mut keep: Vec<ConnectSession> = Vec::new();
    for session in sessions.iter() {
        if !session.alive() {
            // The session ended on its own, or this pid has been handed to somebody else
            // since. Either way the record is done with, and nothing is killed: leaving it
            // here is what made every `/disconnect` walk the same dead list again.
            continue;
        }
        if terminate_pid(session.pid) {
            log::info!("http api disconnect terminated pid {}", session.pid);
            closed += 1;
        } else {
            keep.push(session.clone()); // still alive but terminate failed
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

// --- sessions ---------------------------------------------------------------
//
// The endpoints below drive this machine as the CONTROLLED side: letting a peer
// in, answering its request to take the keyboard, switching what it may do. That
// state lives in the connection manager's process - this one may be the main
// client or `--server`, depending on which started first - so they are clients of
// it over the same IPC channel the session panel uses. See
// `crate::ui_cm_interface::local_api_call`.

/// How long the connection manager is given to answer, in milliseconds. It is
/// another process and may be busy drawing, but every action here is a couple of
/// sends; a caller left waiting on a window that is not coming is worse than an
/// error telling it to look at the window.
const CM_REPLY_TIMEOUT_MS: u64 = 2000;

/// Ask the connection manager to act on a session, and wait for its answer.
///
/// The answer is waited for rather than assumed, because a caller with no window
/// has no other way to tell "done" from "no such session".
///
/// The error carries the status to answer with, because the failures are not the
/// same failure: nothing listening means there is no session manager right now,
/// while one that is running and did not answer is a fault worth looking into.
#[tokio::main(flavor = "current_thread")]
async fn cm_call(call: LocalApiCall) -> Result<LocalApiReply, (u16, String)> {
    let mut conn = ipc::connect(1000, "_cm")
        .await
        .map_err(|e| (503, format!("no session manager is listening: {}", e)))?;
    conn.send(&ipc::Data::LocalApi(call))
        .await
        .map_err(|e| (500, format!("cannot reach the session manager: {}", e)))?;
    // One wait, one answer. The session manager replies on this connection and says
    // nothing else on it, so anything else - or nothing - is a fault; looping until
    // something looks like a reply would leave this request without an upper bound,
    // and the request behind it waiting for that.
    match conn.next_timeout2(CM_REPLY_TIMEOUT_MS).await {
        Some(Ok(Some(ipc::Data::LocalApiReply(reply)))) => Ok(reply),
        Some(Ok(Some(_))) => Err((500, "the session manager answered with something else".to_owned())),
        Some(Ok(None)) => Err((500, "the session manager closed the connection".to_owned())),
        Some(Err(e)) => Err((500, format!("session manager error: {}", e))),
        None => Err((504, "the session manager did not answer".to_owned())),
    }
}

/// Turn the connection manager's answer into a response.
///
/// The status codes are the ones a caller can act on: 404 means the session it
/// named is gone, 409 that the action does not apply to the state that session is
/// in - answering a control request nobody made, say - and 503 that there is no
/// session manager to ask at all.
fn respond_reply(request: Request, reply: LocalApiReply, ok_key: &str) {
    match reply {
        LocalApiReply::Ok { data } => {
            let body = if data.is_empty() {
                "{\"ok\":true}".to_owned()
            } else {
                format!("{{\"ok\":true,\"{}\":{}}}", ok_key, data)
            };
            respond(request, 200, body);
        }
        LocalApiReply::BadRequest { reason } => respond(request, 400, error_body(&reason)),
        LocalApiReply::NotFound => respond(request, 404, error_body("no such session")),
        LocalApiReply::Conflict { reason } => respond(request, 409, error_body(&reason)),
        LocalApiReply::Failed { reason } => respond(request, 500, error_body(&reason)),
    }
}

/// Hand one session action to the connection manager.
fn dispatch(request: Request, id: i32, action: LocalApiAction) {
    match cm_call(LocalApiCall { id, action }) {
        Ok(reply) => respond_reply(request, reply, "result"),
        Err((status, reason)) => respond(request, status, error_body(&reason)),
    }
}

fn error_body(reason: &str) -> String {
    format!(
        "{{\"ok\":false,\"error\":{}}}",
        serde_json::Value::String(reason.to_owned())
    )
}

/// `{"id": 3}` out of a request body.
fn id_field(body: &str) -> Option<i32> {
    json_field(body, "id")?.parse().ok()
}

/// `{"accept": true}` / `{"enabled": false}` out of a request body.
fn bool_field(body: &str, key: &str) -> Option<bool> {
    match json_field(body, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// `POST /request-permission` `{"id":"<peer id>","name":"clipboard"}` - ask a peer to
/// open one of its permissions for this session.
///
/// The counterpart of `/permission`: `/permission` is for the machine being controlled and
/// switches one of its permissions here, while this one is for the operator and asks the
/// peer to do it there. The peer's local user answers it in their own window, and apart
/// from the keyboard the answer only shows up as the peer starting to use the channel -
/// there is no result to wait for, which is why the ack only says the session took the
/// request.
///
/// The name is one of the four the controlled side draws a switch for: `keyboard`,
/// `clipboard`, `audio`, `file`.
fn handle_request_permission(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let Some(id) = json_field(&body, "id") else {
        return respond(request, 400, error_body("id is required"));
    };
    if !valid_peer_id(&id) {
        return respond(request, 400, error_body("invalid id"));
    }
    let Some(name) = json_field(&body, "name") else {
        return respond(request, 400, error_body("name is required"));
    };
    // `keyboard` is a name here but not on the wire: the peer is asked for it by clearing
    // `disable_keyboard`, which is the ask `ask_session_for_control` spells as an empty
    // name - the same one the remote window's "Request control" menu item makes. The other
    // three travel as the peer's `request_permission` field, so
    // `Connection::is_requestable_permission` names just those, from the other end.
    //
    // Anything else is refused here rather than sent on: the peer has nothing to act on,
    // and the only thing sending it would buy is a prompt its local user cannot answer.
    let permission = match name.as_str() {
        "keyboard" => "",
        "clipboard" | "audio" | "file" => name.as_str(),
        _ => return respond(request, 400, error_body("unknown permission")),
    };
    match ask_session_for_control(&id, permission) {
        Ok(()) => respond(request, 200, "{\"ok\":true}".to_owned()),
        Err((status, reason)) => respond(request, status, error_body(&reason)),
    }
}

/// Whether a caller-supplied peer id is safe to put into an IPC name.
///
/// On Unix that name is a path and on Windows a backslash starts a new pipe level, so
/// path separators and control characters are kept out; the length cap is the same one
/// the peer id format allows.
fn valid_peer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\' | ':'))
}

/// Hand the request to the session that owns `peer_id`.
///
/// `permission` is empty for the mouse and keyboard request and one of the A-class
/// channels otherwise; the session turns it into its own toggle. Empty is the internal
/// spelling of the `keyboard` name the API takes - see `handle_request_permission`.
///
/// A 503 means no session process is listening for that peer - nothing was opened, or
/// the session has already gone - which the caller may have to tell apart from a peer
/// that simply has not answered its prompt yet. That answer never reaches here: the
/// ack only says the session ran its toggle.
#[tokio::main(flavor = "current_thread")]
async fn ask_session_for_control(peer_id: &str, permission: &str) -> Result<(), (u16, String)> {
    const ACK_TIMEOUT_MS: u64 = 2000;
    let postfix = format!("{}{}", ipc::POSTFIX_CONTROL, peer_id);
    let mut conn = ipc::connect(1000, &postfix)
        .await
        .map_err(|e| (503, format!("no session for {} is listening: {}", peer_id, e)))?;
    conn.send(&ipc::Data::RequestControl {
        peer_id: peer_id.to_owned(),
        permission: permission.to_owned(),
    })
    .await
    .map_err(|e| (500, format!("cannot reach the session: {}", e)))?;
    match conn.next_timeout2(ACK_TIMEOUT_MS).await {
        Some(Ok(Some(ipc::Data::Test))) => Ok(()),
        Some(Ok(Some(_))) => Err((500, "the session answered with something else".to_owned())),
        Some(Ok(None)) => Err((500, "the session closed the connection".to_owned())),
        Some(Err(e)) => Err((500, format!("cannot read the session's answer: {}", e))),
        None => Err((504, "the session did not answer".to_owned())),
    }
}

/// `GET /sessions` - every peer that is here or trying to get in, with the
/// permissions it has, whether it is waiting to be let in, and whether a control
/// request is outstanding. It is the list the session panel draws, and the one a
/// caller reads before deciding anything.
fn handle_sessions(request: Request) {
    let call = LocalApiCall {
        id: 0,
        action: LocalApiAction::Sessions,
    };
    match cm_call(call) {
        Ok(reply) => respond_reply(request, reply, "sessions"),
        // No session manager is a machine with no sessions, and that is the question this
        // endpoint asks. The 503 it used to answer with is the honest one for the other
        // actions - they need the manager itself - but here it leaves a caller unable to
        // tell "there are none" from "nobody is listening", which is the one thing a
        // polling loop has to be able to act on. It is not an edge case either: the manager
        // exits on its own once the last session ends, so this is the ordinary state of a
        // machine nobody is connected to.
        Err((503, _)) => respond(request, 200, "{\"ok\":true,\"sessions\":[]}".to_owned()),
        Err((status, reason)) => respond(request, status, error_body(&reason)),
    }
}

/// `POST /approve` `{"id":3,"accept":true}` - let a peer in, or refuse it: the
/// session panel's Accept / Dismiss. Nothing else is listening for a peer's login
/// when the app runs headless, so this is one of the two ways a session can start.
fn handle_approve(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let (Some(id), Some(accept)) = (id_field(&body), bool_field(&body, "accept")) else {
        return respond(request, 400, error_body("id and accept are required"));
    };
    dispatch(request, id, LocalApiAction::Approve { accept });
}

/// `POST /control` `{"id":3,"name":"keyboard","accept":true}` - answer a peer's
/// request, one of the four the controlled side can be asked for: the session
/// panel's Allow / Deny.
///
/// Every session starts view-only, so answering the keyboard hands it over, and it
/// lasts for that session only. A request nobody answers is denied once it times
/// out, which is why silence is never read as consent.
///
/// `name` names the permission being answered rather than taking it from the request
/// in flight, so that an answer cannot land on a channel the caller did not mean: the
/// two have to be the same, and the caller reads which one is waiting from
/// `GET /sessions` (`pending_permission`). The empty spelling is not accepted here -
/// the keyboard is a name on this side, the same as on `/request-permission`.
fn handle_control(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let (Some(id), Some(name), Some(accept)) = (
        id_field(&body),
        json_field(&body, "name"),
        bool_field(&body, "accept"),
    ) else {
        return respond(
            request,
            400,
            error_body("id, name and accept are required"),
        );
    };
    if !matches!(name.as_str(), "keyboard" | "clipboard" | "audio" | "file") {
        return respond(request, 400, error_body("unknown permission"));
    }
    dispatch(request, id, LocalApiAction::Control { name, accept });
}

/// `POST /permission` `{"id":3,"name":"clipboard","enabled":true}` - switch
/// one of the things a peer may do: the session panel's switches. The names are
/// the ones the panel draws a switch for.
fn handle_permission(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let (Some(id), Some(name), Some(enabled)) = (
        id_field(&body),
        json_field(&body, "name"),
        bool_field(&body, "enabled"),
    ) else {
        return respond(
            request,
            400,
            error_body("id, name and enabled are required"),
        );
    };
    dispatch(request, id, LocalApiAction::Permission { name, enabled });
}

/// `POST /terminate` `{"id":3}` - end a session: the session panel's Disconnect.
/// The peer is told a person ended it, so it is allowed to reconnect.
fn handle_terminate(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let Some(id) = id_field(&body) else {
        return respond(request, 400, error_body("id is required"));
    };
    dispatch(request, id, LocalApiAction::Terminate);
}

/// `POST /dismiss` `{"id":3}` - take a session that has already ended off the
/// list: the session panel's Close. Nothing else would ever remove it.
fn handle_dismiss(mut request: Request) {
    let body = read_body(&mut request, MAX_BODY_BYTES);
    let Some(id) = id_field(&body) else {
        return respond(request, 400, error_body("id is required"));
    };
    dispatch(request, id, LocalApiAction::Dismiss);
}

/// Live session status. `in_session` reflects whether a connect-session spawned
/// by this API is still running (stale pids are pruned), and `peer_id` is that
/// session's target id. `online` reflects whether GateDesk has logged in to the
/// rendezvous server. The sessions this machine is being controlled in are at
/// `GET /sessions` instead - this one is about the connections this API opened.
fn handle_status(request: Request) {
    let online = crate::ui_interface::get_connect_status().status_num != 0;
    let assistable = crate::ui_interface::is_local_permanent_password_set();
    let mut sessions = connect_sessions().lock().unwrap();
    sessions.retain(|session| session.alive());
    let peer_id = sessions.last().map(|session| session.id.clone());
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
            ("Access-Control-Allow-Private-Network", "true"),
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
        (&Method::Post, "/request-permission") => {
            handle_request_permission(request);
        }
        (&Method::Post, "/password") => {
            handle_password(request);
        }
        (&Method::Get, "/sessions") => {
            handle_sessions(request);
        }
        (&Method::Post, "/approve") => {
            handle_approve(request);
        }
        (&Method::Post, "/control") => {
            handle_control(request);
        }
        (&Method::Post, "/permission") => {
            handle_permission(request);
        }
        (&Method::Post, "/terminate") => {
            handle_terminate(request);
        }
        (&Method::Post, "/dismiss") => {
            handle_dismiss(request);
        }
        (&Method::Get, _) | (&Method::Post, _) => {
            respond(request, 404, "{\"error\":\"not found\"}".to_owned());
        }
        _ => {
            respond(request, 405, "{\"error\":\"method not allowed\"}".to_owned());
        }
    }
}



















