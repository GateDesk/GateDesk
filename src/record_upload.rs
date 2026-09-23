//! Upload this machine's session recordings to the enterprise server.
//!
//! The console skin records every session on its own (see `REMOTE_SKIN` in `common.rs`
//! and `client/io_loop.rs`), and that recording has to reach the server for the same
//! reason the audit trail does: the operator's machine is not the one that keeps the
//! record. The transport mirrors the one upstream uses for a peer's own recordings
//! (`hbbs_http/record_upload.rs`) - the recorder reports the path when it starts writing
//! and a `WriteTail` when the file is closed, and the bytes go up as parts - with three
//! differences: the target is the audit server rather than the API server, every request
//! is retried, and what happened is written to the audit trail.
//!
//! Two settings drive it:
//!
//! * `audit-server-url` (shared with the audit trail itself) - the `/api/record` endpoint
//!   is derived from its origin, so one setting configures both.
//! * `record-upload-mode` - `chunked` (the default) streams the file while it is still
//!   being written, so a session cut off half way loses at most the last second; `whole`
//!   waits for the file to be closed and sends it in one request.

use crate::hbbs_http::create_http_client_with_url;
use bytes::Bytes;
use hbb_common::{bail, config::Config, log, ResultType};
use reqwest::blocking::Client;
use scrap::record::RecordState;
use serde::Serialize;
use std::{
    io::{prelude::*, SeekFrom},
    path::PathBuf,
    sync::mpsc::Receiver,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// `chunked` (the default) or `whole`; anything else is read as `chunked`.
pub const OPTION_RECORD_UPLOAD_MODE: &str = "record-upload-mode";

/// Attempts per request. After the last one the file is given up on and an audit event
/// says so - the same stance the audit trail takes, which never blocks on the network.
const RETRY: usize = 3;

/// How long to wait before the next attempt (multiplied by the attempt number).
const RETRY_DELAY: Duration = Duration::from_millis(500);

/// In `chunked` mode, send what has been written so far once this much time or this many
/// bytes have accumulated.
const SHOULD_SEND_TIME: Duration = Duration::from_secs(1);
const SHOULD_SEND_SIZE: u64 = 1024 * 1024;

/// Bytes of the file head sent with `type=tail`, so the server can tell what it received.
const MAX_HEADER_LEN: usize = 1024;

/// Written next to a recording once the server has it, holding the upload time. Only files
/// carrying this marker are ever deleted by [`cleanup`] - a recording the operator made by
/// hand, or one that never reached the server, is theirs to keep.
const UPLOADED_SUFFIX: &str = ".uploaded";

/// How long an uploaded recording is kept on this machine before [`cleanup`] removes it.
const KEEP_UPLOADED_DAYS: u64 = 3;

fn is_chunked() -> bool {
    Config::get_option(OPTION_RECORD_UPLOAD_MODE) != "whole"
}

/// The recording endpoint of the audit server: the origin of `audit-server-url` plus
/// `/api/record`. They are always the same server, so one setting configures both.
fn endpoint() -> ResultType<String> {
    let url = Config::get_option(crate::audit::AUDIT_SERVER_OPTION);
    if url.is_empty() {
        bail!("{} is not set", crate::audit::AUDIT_SERVER_OPTION);
    }
    let after_scheme = url.find("://").map(|i| i + 3).unwrap_or(0);
    let rest = &url[after_scheme..];
    let origin_end = after_scheme + rest.find('/').unwrap_or(rest.len());
    Ok(format!("{}/api/record", &url[..origin_end]))
}

/// Start the uploader for one recording.
///
/// `rx` is the channel the recorder reports its state on; `session_id` travels with every
/// request so the server can file the recording under the session it belongs to. The
/// thread ends when the recorder is dropped, i.e. when the session does.
pub fn run(rx: Receiver<RecordState>, session_id: u64) {
    let url = match endpoint() {
        Ok(url) => url,
        Err(e) => {
            log::warn!("record upload disabled: {}", e);
            return;
        }
    };
    std::thread::spawn(move || {
        let client = create_http_client_with_url(&url);
        let mut uploader = Uploader {
            client,
            url,
            session_id,
            filepath: Default::default(),
            filename: Default::default(),
            sent: 0,
            running: false,
            last_send: Instant::now(),
        };
        loop {
            match rx.recv() {
                Ok(state) => match state {
                    RecordState::NewFile(path) => uploader.on_new_file(path),
                    RecordState::NewFrame => uploader.on_frame(false),
                    RecordState::WriteTail => uploader.on_tail(),
                    RecordState::RemoveFile => uploader.on_remove(),
                },
                Err(e) => {
                    log::trace!("record upload thread stop: {}", e);
                    break;
                }
            }
        }
    });
}

struct Uploader {
    client: Client,
    url: String,
    session_id: u64,
    filepath: String,
    filename: String,
    /// Bytes already accepted by the server.
    sent: u64,
    running: bool,
    last_send: Instant,
}

impl Uploader {
    fn query(&self, typ: &str) -> Vec<(&'static str, String)> {
        vec![
            ("type", typ.to_owned()),
            ("file", self.filename.clone()),
            ("device_id", Config::get_id()),
            ("session_id", self.session_id.to_string()),
        ]
    }

    /// One request, retried `RETRY` times. The body is `Bytes` so it survives the retries.
    fn send<Q: Serialize + ?Sized>(&self, query: &Q, body: Bytes) -> ResultType<()> {
        let mut last = String::new();
        for attempt in 1..=RETRY {
            match self
                .client
                .post(self.url.as_str())
                .query(query)
                .body(body.clone())
                .send()
            {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp) => last = format!("http {}", resp.status()),
                Err(e) => last = e.to_string(),
            }
            log::debug!(
                "record upload attempt {}/{} failed: {}",
                attempt,
                RETRY,
                last
            );
            if attempt < RETRY {
                std::thread::sleep(RETRY_DELAY * attempt as u32);
            }
        }
        bail!("{}", last)
    }

    fn on_new_file(&mut self, path: String) {
        let filename = match PathBuf::from(&path)
            .file_name()
            .and_then(|f| f.to_str())
            .map(|f| f.to_owned())
        {
            Some(filename) => filename,
            None => {
                log::error!("record upload: cannot parse file path: {}", path);
                return;
            }
        };
        self.filepath = path;
        self.filename = filename;
        self.sent = 0;
        self.running = true;
        self.last_send = Instant::now();
        // In `whole` mode nothing is sent until the recorder has closed the file.
        if is_chunked() {
            if let Err(e) = self.send(&self.query("new"), Bytes::new()) {
                self.give_up("new", &e.to_string());
            }
        }
    }

    fn on_frame(&mut self, flush: bool) {
        if !self.running || !is_chunked() {
            return;
        }
        if !flush && self.last_send.elapsed() < SHOULD_SEND_TIME {
            return;
        }
        if let Err(e) = self.send_part(flush) {
            self.give_up("part", &e.to_string());
        }
    }

    /// Send everything written since the last part. With `flush` the size threshold is
    /// ignored - used for the last part before the tail.
    fn send_part(&mut self, flush: bool) -> ResultType<()> {
        let mut file = std::fs::File::open(&self.filepath)?;
        let len = file.metadata()?.len();
        if len <= self.sent {
            return Ok(());
        }
        if !flush && len - self.sent < SHOULD_SEND_SIZE {
            return Ok(());
        }
        file.seek(SeekFrom::Start(self.sent))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        let length = buf.len();
        let mut query = self.query("part");
        query.push(("offset", self.sent.to_string()));
        query.push(("length", length.to_string()));
        self.send(&query, Bytes::from(buf))?;
        self.sent = len;
        self.last_send = Instant::now();
        Ok(())
    }

    fn on_tail(&mut self) {
        if !self.running {
            return;
        }
        self.running = false;
        match self.finish() {
            Ok(()) => {
                self.mark_uploaded();
                log::info!("record uploaded: {} ({} bytes)", self.filename, self.sent);
                crate::audit::record(
                    "record.upload.done",
                    "operator",
                    self.session_id,
                    "ok",
                    serde_json::json!({ "file": self.filename, "bytes": self.sent }),
                );
            }
            Err(e) => {
                log::error!("record upload failed: {} ({})", self.filename, e);
                crate::audit::record(
                    "record.upload.fail",
                    "operator",
                    self.session_id,
                    "err",
                    serde_json::json!({
                        "file": self.filename,
                        "bytes": self.sent,
                        "error": e.to_string(),
                    }),
                );
            }
        }
    }

    /// Send whatever is left, then close the recording on the server.
    fn finish(&mut self) -> ResultType<()> {
        if is_chunked() {
            self.send_part(true)?;
        } else {
            self.send(&self.query("new"), Bytes::new())?;
            // One part holding the whole file.
            let mut file = std::fs::File::open(&self.filepath)?;
            let len = file.metadata()?.len();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            let mut query = self.query("part");
            query.push(("offset", "0".to_owned()));
            query.push(("length", buf.len().to_string()));
            self.send(&query, Bytes::from(buf))?;
            self.sent = len;
        }
        // The head goes along with the tail, as upstream does, so the server can tell
        // what format it ended up with. A file shorter than the head is sent whole.
        let mut file = std::fs::File::open(&self.filepath)?;
        let mut head = vec![0u8; MAX_HEADER_LEN];
        let length = file.read(&mut head)?;
        head.truncate(length);
        self.send(&self.query("tail"), Bytes::from(head))
    }

    fn on_remove(&mut self) {
        if !self.running {
            return;
        }
        self.running = false;
        // The recorder drops files that turned out to be too short to keep. Nothing was
        // uploaded worth keeping, so tell the server to forget them too.
        if is_chunked() {
            if let Err(e) = self.send(&self.query("remove"), Bytes::new()) {
                log::debug!("record upload: remove failed: {}", e);
            }
        }
    }

    /// A request failed for good. The file stays on this machine, unmarked.
    fn give_up(&mut self, what: &str, e: &str) {
        self.running = false;
        log::error!("record upload {} failed: {} ({})", what, self.filename, e);
        crate::audit::record(
            "record.upload.fail",
            "operator",
            self.session_id,
            "err",
            serde_json::json!({
                "file": self.filename,
                "bytes": self.sent,
                "step": what,
                "error": e,
            }),
        );
    }

    fn mark_uploaded(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let marker = format!("{}{}", self.filepath, UPLOADED_SUFFIX);
        if let Err(e) = std::fs::write(&marker, now.to_string()) {
            log::warn!("record upload: cannot write {}: {}", marker, e);
        }
    }
}

/// Remove recordings that reached the server and have been sitting here long enough.
///
/// Called once when the console skin starts. Only files carrying the marker
/// [`Uploader::mark_uploaded`] writes are touched: a recording that never made it to the
/// server stays put.
pub fn cleanup(dir: &str) {
    let keep = Duration::from_secs(KEEP_UPLOADED_DAYS * 24 * 60 * 60);
    let now = SystemTime::now();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let marker = entry.path();
        if !marker
            .file_name()
            .and_then(|f| f.to_str())
            .map_or(false, |f| f.ends_with(UPLOADED_SUFFIX))
        {
            continue;
        }
        let uploaded_at = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs));
        let expired = match uploaded_at.and_then(|t| now.duration_since(t).ok()) {
            Some(age) => age >= keep,
            None => false,
        };
        if !expired {
            continue;
        }
        let s = marker.to_string_lossy().to_string();
        let recording = PathBuf::from(&s[..s.len() - UPLOADED_SUFFIX.len()]);
        if let Err(e) = std::fs::remove_file(&recording) {
            log::warn!("record upload: cannot remove {}: {}", recording.display(), e);
            continue;
        }
        if let Err(e) = std::fs::remove_file(&marker) {
            log::warn!("record upload: cannot remove {}: {}", marker.display(), e);
        }
        log::info!("record upload: removed expired {}", recording.display());
    }
}
