use hbb_common::log;
use tiny_http::{Header, Method, Request, Response, Server};

/// Local HTTP API for web pages running on this machine.
///
/// - Listens on 127.0.0.1 only (never 0.0.0.0).
/// - Requires token: `Authorization: Bearer <token>` header or `?token=<token>` query.
/// - Token is read from config option `api-token` (GateDesk.toml `[options]`).
/// - CORS: `Access-Control-Allow-Origin: *` so business web pages can fetch it.
const PORT: u16 = 21120;

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

fn handle(request: Request) {
    // CORS preflight
    if request.method() == &Method::Options {
        let response = Response::empty(204);
        let response = match header("Access-Control-Allow-Origin", "*") {
            Some(h) => response.with_header(h),
            None => response,
        };
        let response = match header("Access-Control-Allow-Methods", "GET, OPTIONS") {
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

    if request.method() != &Method::Get {
        respond(request, 405, "{\"error\":\"method not allowed\"}".to_owned());
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
    match path {
        "/id" => {
            let id = crate::ipc::get_id();
            respond(request, 200, format!("{{\"id\":\"{}\"}}", id));
        }
        _ => {
            respond(request, 404, "{\"error\":\"not found\"}".to_owned());
        }
    }
}
