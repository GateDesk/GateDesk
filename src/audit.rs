//! Enterprise operation-level audit for the GateDesk desktop client.
//!
//! Records an operation-level audit trail (design doc §8) for actions performed
//! on this machine: authorization grants, connect/disconnect, voice, recording,
//! privacy, input lock, remote restart, password changes, ...
//!
//! Every event is:
//!  1. appended to the local `audit.log` (JSON Lines) under the log directory
//!     (never blocks the main flow, survives audit-server outages), and
//!  2. optionally forwarded to an audit server whose base URL is configured in
//!     GateDesk2.toml `[options] audit-server-url` (empty = local-only).

use std::io::Write;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};

use hbb_common::{config::Config, get_time, log};
use serde_json::{json, Value};

/// File name of the local fallback audit log.
const AUDIT_LOG_FILE: &str = "audit.log";

/// Config option holding the audit-server base URL (e.g. the PoC server.js
/// `/api/audit` endpoint). Empty means local audit.log only.
const AUDIT_SERVER_OPTION: &str = "audit-server-url";

/// Serializes appends to audit.log coming from different threads.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Capacity of the bounded forwarding queue. When it is full the event is
/// dropped from forwarding (it is still in the local audit.log), so a slow or
/// unreachable audit server can never grow the process without bound.
const FORWARD_QUEUE_LEN: usize = 1024;

/// Sender of the single forwarding worker.
///
/// One dedicated thread drains the queue and performs the (blocking) HTTP POST,
/// instead of spawning a detached thread per event.
fn forward_sender() -> &'static SyncSender<Value> {
    static SENDER: OnceLock<SyncSender<Value>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Value>(FORWARD_QUEUE_LEN);
        if let Err(e) = std::thread::Builder::new()
            .name("audit-forward".to_owned())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    // Re-read the option per event so a config change (or a
                    // server URL that was not set yet) is honoured.
                    let url = Config::get_option(AUDIT_SERVER_OPTION);
                    if url.is_empty() {
                        continue;
                    }
                    if let Err(e) = crate::common::post_request_sync(url, event.to_string(), "") {
                        log::debug!("audit forward failed (kept in local audit.log): {}", e);
                    }
                }
            })
        {
            log::debug!("audit: cannot start forward worker: {}", e);
        }
        tx
    })
}

/// Record one operation-level audit event.
///
/// Unified payload: `{action, actor, device_id, session_id, ts, result, extra}`.
/// Never panics and never blocks the caller on network: the optional forward is
/// handed to a dedicated worker thread through a bounded queue.
///
/// # Arguments
/// * `action` - event name from the audit event table (e.g. `auth.grant`,
///   `connect.start`, `voice.on`, `record.start`, `remote.restart`, ...).
/// * `actor`  - who triggered the action (e.g. `local`, `operator`).
/// * `session_id` - id of the affected control session, `0` when not in a session.
/// * `result` - `ok` or a short error description.
/// * `extra`  - action-specific context (target id, method, counts, ...).
pub fn record(action: &str, actor: &str, session_id: u64, result: &str, extra: Value) {
    let event = json!({
        "action": action,
        "actor": actor,
        "device_id": Config::get_id(),
        "session_id": session_id,
        "ts": get_time(),
        "result": result,
        "extra": extra,
    });
    append_local(&event);
    if Config::get_option(AUDIT_SERVER_OPTION).is_empty() {
        return;
    }
    match forward_sender().try_send(event) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            log::debug!("audit forward queue full, event kept in local audit.log only");
        }
        Err(TrySendError::Disconnected(_)) => {
            log::debug!("audit forward worker unavailable, event kept in local audit.log only");
        }
    }
}

/// Append one JSON Lines record to the local audit.log.
fn append_local(event: &Value) {
    let dir = Config::log_path();
    if dir.as_os_str().is_empty() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::debug!("audit: create log dir {} failed: {}", dir.display(), e);
        return;
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut line = event.to_string();
    line.push('\n');
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(AUDIT_LOG_FILE))
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(line.as_bytes()) {
                log::debug!("audit: append to audit.log failed: {}", e);
            }
        }
        Err(e) => {
            log::debug!("audit: open audit.log failed: {}", e);
        }
    }
}

