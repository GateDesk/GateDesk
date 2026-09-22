//! Outbound event notification for the GateDesk desktop client (interface doc §6.10).
//!
//! The audit trail (`crate::audit`) answers *what happened*; this answers *go and
//! look*. The controlled side tells the platform the moment a decision is waiting
//! for it, so the platform does not have to discover it by polling
//! `GET /sessions` inside the sixty seconds a control request lives for.
//!
//! Deliberately thinner than the audit forwarder:
//!
//! * its own config key (`[options] event-server-url`), endpoint and queue, so
//!   switching one on does not switch the other on;
//! * **no retry and no local fallback** - a lost notification costs one polling
//!   interval and nothing else, because `GET /sessions` stays the source of
//!   truth and the platform polls it anyway;
//! * a hard three second cap, so a platform that is down cannot pile events up
//!   behind a stalled socket.
//!
//! Nothing here is a record. Do not use it to reconstruct state, and do not
//! expect an event for every change: the payload says "go and look", not "this is
//! so".

use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::OnceLock;
use std::time::Duration;

use hbb_common::config::Config;
use hbb_common::get_time;
use hbb_common::log;
use hbb_common::tokio;
use serde_json::{json, Value};

/// Config option holding the event server URL (e.g. the PoC server.js
/// `/api/event` endpoint). Empty means notifications are off, which is the
/// default: nothing is sent until an operator configures one.
const EVENT_SERVER_OPTION: &str = "event-server-url";

/// Hard cap on one delivery. The shared HTTP helper would otherwise allow
/// `CONNECT_TIMEOUT + READ_TIMEOUT` (18s + 18s), which is not a bound a "go and
/// look" hint is worth.
const EVENT_TIMEOUT: Duration = Duration::from_secs(3);

/// Capacity of the bounded queue. Small on purpose: events are hints, and an
/// hour-old hint is worth nothing, so a stalled platform should lose the old
/// ones rather than have them delivered in a burst later.
const QUEUE_LEN: usize = 64;

/// Tell the platform that something on this machine wants looking at.
///
/// Never blocks and never panics: the caller is a connection task, and a
/// platform that is unreachable must not delay a session. See the module comment
/// for what this is not.
///
/// # Arguments
/// * `event` - one of `login.pending`, `control.pending`, `session.open`,
///   `session.close`.
/// * `session_id` - the session this is about, the id `GET /sessions` hands out.
/// * `peer_id` - the peer that asked or connected.
/// * `extra` - event-specific fields, e.g. `{"permission": "clipboard"}`.
pub fn notify(event: &str, session_id: u64, peer_id: &str, extra: Value) {
    if Config::get_option(EVENT_SERVER_OPTION).is_empty() {
        return;
    }
    let payload = json!({
        "event": event,
        "device_id": Config::get_id(),
        "session_id": session_id,
        "peer_id": peer_id,
        "ts": get_time(),
        "extra": extra,
    });
    match sender().try_send(payload.to_string()) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            log::debug!("event {} dropped: queue full", event);
        }
        Err(TrySendError::Disconnected(_)) => {
            log::debug!("event {} dropped: worker unavailable", event);
        }
    }
}

/// Sender of the single delivery worker.
///
/// One dedicated thread drains the queue and performs the blocking POST, rather
/// than a detached thread per event - the same shape `audit::forward_sender`
/// uses, and the reason it is safe to call `notify` from a tokio task.
fn sender() -> &'static SyncSender<String> {
    static SENDER: OnceLock<SyncSender<String>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<String>(QUEUE_LEN);
        if let Err(e) = std::thread::Builder::new()
            .name("event-forward".to_owned())
            .spawn(move || {
                while let Ok(body) = rx.recv() {
                    // Re-read per event, like the audit forwarder, so a URL added
                    // after start takes effect without a restart.
                    let url = Config::get_option(EVENT_SERVER_OPTION);
                    if url.is_empty() {
                        continue;
                    }
                    if let Err(e) = post(url, body) {
                        log::debug!("event notify failed (not retried): {}", e);
                    }
                }
            })
        {
            log::warn!("event: cannot start forward worker: {}", e);
        }
        tx
    })
}

/// POST one notification with a hard cap, on this worker thread's own runtime.
///
/// Any 2xx counts as delivered and no status code is looked at: the platform's
/// answer carries no instruction, and the body of a failure would not change
/// what we do next (nothing).
#[tokio::main(flavor = "current_thread")]
async fn post(url: String, body: String) -> Result<(), String> {
    let result = tokio::time::timeout(
        EVENT_TIMEOUT,
        crate::common::post_request(url, body, ""),
    )
    .await;
    match result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("timed out after {:?}", EVENT_TIMEOUT)),
    }
}
