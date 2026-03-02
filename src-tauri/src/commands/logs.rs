use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Shared state exposed to the Studio plugin via HTTP on port 19556.
/// Updated by start_rojo/stop_rojo to reflect whether "Start Development" is active.
pub struct LauncherStatus {
    inner: Arc<Mutex<LauncherStatusInner>>,
}

pub(crate) struct LauncherStatusInner {
    pub(crate) active: bool,
    pub(crate) project_path: String,
    pub(crate) project_name: String,
    /// Port where rojo serve is running (detected from stdout).
    pub(crate) rojo_port: Option<u16>,
    /// placeId linked to the current project (set by the Studio plugin via POST /link-place)
    pub(crate) linked_place_id: Option<u64>,
    pub(crate) linked_universe_id: Option<u64>,
    pub(crate) linked_place_name: Option<String>,
}

impl Default for LauncherStatus {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(LauncherStatusInner {
                active: false,
                project_path: String::new(),
                project_name: String::new(),
                rojo_port: None,
                linked_place_id: None,
                linked_universe_id: None,
                linked_place_name: None,
            })),
        }
    }
}

impl LauncherStatus {
    /// Mark the launcher as active with the given project info.
    /// Also loads the previously linked placeId from config for the /status endpoint.
    pub async fn set_active(&self, project_path: &str, project_name: &str) {
        let mut guard = self.inner.lock().await;
        guard.active = true;
        guard.project_path = project_path.to_string();
        guard.project_name = project_name.to_string();

        // Load placeId from config so the plugin can verify before connecting
        if let Some(config) = crate::commands::config::load_config().await {
            if let Some(project) = config.projects.iter().find(|p| p.path == project_path) {
                guard.linked_place_id = project.place_id;
                guard.linked_universe_id = project.universe_id;
            }
        }
    }

    /// Mark the launcher as inactive.
    pub async fn set_inactive(&self) {
        let mut guard = self.inner.lock().await;
        guard.active = false;
        guard.rojo_port = None;
    }

    /// Get a clone of the inner Arc for passing to the log server.
    pub fn shared(&self) -> Arc<Mutex<LauncherStatusInner>> {
        self.inner.clone()
    }
}

// ─── MCP Command Queue ───────────────────────────────────────────────────────
// Enables AI tools to execute Luau code in Studio via a polling pattern:
// 1. MCP sends code to launcher (POST /mcp/run-code) — blocks waiting for result
// 2. Studio plugin polls (GET /mcp/pending-command) — picks up the command
// 3. Plugin executes and sends result (POST /mcp/command-result) — unblocks step 1

struct McpCommandResult {
    success: bool,
    result: String,
}

pub struct McpState {
    inner: Arc<Mutex<McpStateInner>>,
}

pub(crate) struct McpStateInner {
    /// Command waiting to be picked up by the Studio plugin.
    pending_command: Option<(String, String)>, // (id, code)
    /// Channel to deliver the result back to the POST /mcp/run-code caller.
    result_sender: Option<oneshot::Sender<McpCommandResult>>,
}

impl Default for McpState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(McpStateInner {
                pending_command: None,
                result_sender: None,
            })),
        }
    }
}

impl McpState {
    /// Get a clone of the inner Arc for passing to the HTTP server.
    pub fn shared(&self) -> Arc<Mutex<McpStateInner>> {
        self.inner.clone()
    }
}

// ─── Logger State ────────────────────────────────────────────────────────────

/// Managed Tauri state holding the current session logger (if any).
pub struct LoggerState {
    pub logger: Arc<Mutex<Option<SessionLogger>>>,
}

impl Default for LoggerState {
    fn default() -> Self {
        Self {
            logger: Arc::new(Mutex::new(None)),
        }
    }
}

/// Async session logger that writes timestamped lines to `.roxlit/logs/latest.log`.
///
/// Uses an mpsc channel so callers never block on disk I/O — `log()` just sends
/// through the channel, and a background task does the actual writing.
pub struct SessionLogger {
    tx: mpsc::UnboundedSender<String>,
}

impl SessionLogger {
    /// Create a new session logger for the given project.
    ///
    /// - Creates `.roxlit/logs/` if it doesn't exist
    /// - Rotates `latest.log` → `session-{timestamp}.log`
    /// - Cleans up old sessions (keeps max 10)
    /// - Spawns a background writer task
    pub async fn new(project_path: &str) -> Option<Self> {
        let logs_dir = std::path::Path::new(project_path)
            .join(".roxlit")
            .join("logs");

        if tokio::fs::create_dir_all(&logs_dir).await.is_err() {
            return None;
        }

        let latest = logs_dir.join("latest.log");

        // Rotate previous latest.log to session-{timestamp}.log
        if latest.exists() {
            let ts = unix_timestamp();
            let rotated = logs_dir.join(format!("session-{ts}.log"));
            let _ = tokio::fs::rename(&latest, &rotated).await;
        }

        // Clean up old sessions (keep max 10)
        cleanup_old_sessions(&logs_dir).await;

        // Open latest.log for writing
        let file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&latest)
            .await
        {
            Ok(f) => f,
            Err(_) => return None,
        };

        let (tx, rx) = mpsc::unbounded_channel::<String>();

        // Spawn writer task
        tokio::spawn(writer_task(file, rx));

        // Write header
        let header = format!(
            "=== Roxlit Session — {} ===\n\n",
            format_timestamp(unix_timestamp())
        );
        let _ = tx.send(header);

        Some(Self { tx })
    }

    /// Clone the sender so reader tasks can log without holding a lock.
    pub fn sender(&self) -> mpsc::UnboundedSender<String> {
        self.tx.clone()
    }
}

/// Format a pre-formatted log line and send it through a sender.
/// Convenience for reader tasks that already have a cloned sender.
pub fn send_log(tx: &mpsc::UnboundedSender<String>, prefix: &str, line: &str) {
    let ts = format_timestamp(unix_timestamp());
    let formatted = format!("[{ts}] [{prefix}] {line}\n");
    let _ = tx.send(formatted);
}

/// Background task that receives lines from the channel and writes to disk.
async fn writer_task(file: tokio::fs::File, mut rx: mpsc::UnboundedReceiver<String>) {
    use tokio::io::AsyncWriteExt;
    let mut writer = tokio::io::BufWriter::new(file);

    while let Some(line) = rx.recv().await {
        let _ = writer.write_all(line.as_bytes()).await;
        // Flush periodically so logs are readable in real-time
        let _ = writer.flush().await;
    }

    // Channel closed — write footer
    let footer = format!(
        "\n=== Session ended — {} ===\n",
        format_timestamp(unix_timestamp())
    );
    let _ = writer.write_all(footer.as_bytes()).await;
    let _ = writer.flush().await;
}

/// Get current Unix timestamp in seconds.
fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Format a Unix timestamp as ISO 8601 UTC (e.g. "2025-06-15T10:30:00Z").
/// No chrono dependency — pure arithmetic (inverse of update.rs::parse_iso8601_to_unix).
fn format_timestamp(secs: u64) -> String {
    let s = secs as i64;

    let sec = s % 60;
    let min = (s / 60) % 60;
    let hour = (s / 3600) % 24;
    let mut days = s / 86400;

    // Convert days since epoch to year/month/day
    let mut year: i64 = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month: i64 = 1;
    for i in 0..12 {
        let mut d = month_days[i];
        if i == 1 && is_leap(year) {
            d += 1;
        }
        if days < d {
            break;
        }
        days -= d;
        month += 1;
    }
    let day = days + 1;

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z"
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Managed Tauri state for the Studio log HTTP server.
pub struct LogServerState {
    handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl Default for LogServerState {
    fn default() -> Self {
        Self {
            handle: Arc::new(Mutex::new(None)),
        }
    }
}

impl LogServerState {
    /// Store the server task handle.
    pub async fn set_handle(&self, h: tokio::task::JoinHandle<()>) {
        let mut guard = self.handle.lock().await;
        *guard = Some(h);
    }

    /// Abort the server task synchronously (for window close handler).
    pub fn kill_sync(&self) {
        if let Ok(mut guard) = self.handle.try_lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }

    /// Abort the server task asynchronously.
    pub async fn stop(&self) {
        let mut guard = self.handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }
}

/// Start the HTTP log server on 127.0.0.1:19556.
///
/// Returns `Some(JoinHandle)` on success, `None` if the port is busy (non-critical).
/// The server accepts these endpoints:
/// - `GET /health` → responds `200 ok`
/// - `GET /status` → JSON with launcher active state, project info
/// - `POST /log` → parses a JSON batch of `{message, level, timestamp}` and writes to the session log
/// - `POST /link-place` → receives `{placeId, placeName}` from Studio plugin
pub async fn start_log_server(
    log_tx: mpsc::UnboundedSender<String>,
    status: Arc<Mutex<LauncherStatusInner>>,
    mcp: Arc<Mutex<McpStateInner>>,
) -> Option<tokio::task::JoinHandle<()>> {
    let listener = TcpListener::bind("127.0.0.1:19556").await.ok()?;

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => continue,
            };

            let tx = log_tx.clone();
            let status = status.clone();
            let mcp = mcp.clone();
            tokio::spawn(async move {
                handle_connection(stream, tx, status, mcp).await;
            });
        }
    });

    Some(handle)
}

/// Handle a single TCP connection with minimal HTTP parsing.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    tx: mpsc::UnboundedSender<String>,
    status: Arc<Mutex<LauncherStatusInner>>,
    mcp: Arc<Mutex<McpStateInner>>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 65536];
    // Read headers (and possibly body) in the first chunk
    let mut total = match stream.read(&mut buf).await {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };

    // If this is a POST with Content-Length, ensure we read the full body.
    // Some clients (e.g. PowerShell) send headers and body in separate TCP packets.
    let header_str = String::from_utf8_lossy(&buf[..total]);
    if let Some(header_end) = header_str.find("\r\n\r\n") {
        // Parse Content-Length from headers
        let content_length: usize = header_str[..header_end]
            .lines()
            .find_map(|line| {
                let lower = line.to_lowercase();
                if lower.starts_with("content-length:") {
                    lower.trim_start_matches("content-length:").trim().parse().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let body_start = header_end + 4;
        let body_received = total.saturating_sub(body_start);

        // Read remaining body bytes if needed
        if body_received < content_length {
            let remaining = content_length - body_received;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut read_so_far = 0;
            while read_so_far < remaining {
                if tokio::time::Instant::now() > deadline {
                    break;
                }
                let end = (total + remaining - read_so_far).min(buf.len());
                match stream.read(&mut buf[total..end]).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        total += n;
                        read_so_far += n;
                    }
                }
            }
        }
    }

    let request = String::from_utf8_lossy(&buf[..total]);

    // Parse the HTTP request line
    let first_line = request.lines().next().unwrap_or("");

    if first_line.starts_with("GET /health") {
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\nok";
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    if first_line.starts_with("GET /status") {
        let guard = status.lock().await;
        let linked_place = match guard.linked_place_id {
            Some(id) => format!("{id}"),
            None => "null".to_string(),
        };
        let rojo_port = match guard.rojo_port {
            Some(p) => format!("{p}"),
            None => "null".to_string(),
        };
        let json = format!(
            r#"{{"active":{},"projectPath":"{}","projectName":"{}","linkedPlaceId":{},"rojoPort":{}}}"#,
            guard.active,
            guard.project_path.replace('\\', "\\\\").replace('"', "\\\""),
            guard.project_name.replace('"', "\\\""),
            linked_place,
            rojo_port,
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
            json.len(),
            json,
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    if first_line.starts_with("POST /link-place") {
        if let Some(body_start) = request.find("\r\n\r\n") {
            let body = &request[body_start + 4..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
                let place_id = val["placeId"].as_u64();
                let universe_id = val["universeId"].as_u64();
                let place_name = val["placeName"].as_str().map(String::from);
                let mut guard = status.lock().await;
                guard.linked_place_id = place_id;
                guard.linked_universe_id = universe_id;
                guard.linked_place_name = place_name;
                if let Some(id) = place_id {
                    send_log(&tx, "roxlit", &format!("Studio linked placeId {id}"));
                }
            }
        }
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\nok";
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    if first_line.starts_with("POST /log") {
        // Find the body (after \r\n\r\n)
        if let Some(body_start) = request.find("\r\n\r\n") {
            let body = &request[body_start + 4..];
            process_log_batch(&tx, body);
        }
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\nok";
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    // ─── MCP endpoints ────────────────────────────────────────────────────

    // POST /mcp/run-code — MCP sends Luau code, blocks until plugin returns result
    if first_line.starts_with("POST /mcp/run-code") {
        if let Some(body_start) = request.find("\r\n\r\n") {
            let body = &request[body_start + 4..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
                let code = val["code"].as_str().unwrap_or("").to_string();
                if code.is_empty() {
                    let json = r#"{"error":"code field is required"}"#;
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                        json.len(), json,
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    return;
                }

                let id = format!("{}", unix_timestamp());
                let (result_tx, result_rx) = oneshot::channel::<McpCommandResult>();

                // Enqueue the command
                {
                    let mut guard = mcp.lock().await;
                    guard.pending_command = Some((id.clone(), code));
                    guard.result_sender = Some(result_tx);
                }

                send_log(&tx, "mcp", &format!("Queued run_code command {id}"));

                // Wait for result with 30s timeout
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    result_rx,
                ).await;

                let (status_code, json) = match result {
                    Ok(Ok(res)) => {
                        let escaped_result = res.result
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"")
                            .replace('\n', "\\n")
                            .replace('\r', "\\r")
                            .replace('\t', "\\t");
                        (
                            "200 OK",
                            format!(r#"{{"success":{},"result":"{}"}}"#, res.success, escaped_result),
                        )
                    }
                    Ok(Err(_)) => {
                        ("500 Internal Server Error", r#"{"error":"result channel dropped"}"#.to_string())
                    }
                    Err(_) => {
                        // Timeout — clean up pending command
                        let mut guard = mcp.lock().await;
                        guard.pending_command = None;
                        guard.result_sender = None;
                        ("504 Gateway Timeout", r#"{"error":"Studio plugin did not respond within 30s"}"#.to_string())
                    }
                };

                let response = format!(
                    "HTTP/1.1 {status_code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                    json.len(), json,
                );
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }
        }
        let response = "HTTP/1.1 400 Bad Request\r\nContent-Length: 12\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\ninvalid json";
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    // GET /mcp/pending-command — Plugin polls for commands to execute
    if first_line.starts_with("GET /mcp/pending-command") {
        let mut guard = mcp.lock().await;
        if let Some((id, code)) = guard.pending_command.take() {
            let escaped_code = code
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            let json = format!(r#"{{"id":"{}","code":"{}"}}"#, id, escaped_code);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n{}",
                json.len(), json,
            );
            let _ = stream.write_all(response.as_bytes()).await;
        } else {
            let response = "HTTP/1.1 204 No Content\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        return;
    }

    // POST /mcp/command-result — Plugin sends execution result
    if first_line.starts_with("POST /mcp/command-result") {
        if let Some(body_start) = request.find("\r\n\r\n") {
            let body = &request[body_start + 4..];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(body) {
                let success = val["success"].as_bool().unwrap_or(false);
                let result = val["result"].as_str().unwrap_or("").to_string();
                let id = val["id"].as_str().unwrap_or("?");

                send_log(&tx, "mcp", &format!("Result for command {id}: success={success}"));

                // Deliver the result to the waiting POST /mcp/run-code caller
                let mut guard = mcp.lock().await;
                if let Some(sender) = guard.result_sender.take() {
                    let _ = sender.send(McpCommandResult { success, result });
                }
            }
        }
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\nok";
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    // Handle CORS preflight (OPTIONS)
    if first_line.starts_with("OPTIONS") {
        let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(response.as_bytes()).await;
        return;
    }

    let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found";
    let _ = stream.write_all(response.as_bytes()).await;
}

/// Parse a JSON array of log entries and write each to the session log.
/// Expected format: `[{"message": "...", "level": "info|warn|error", "timestamp": 0.0}]`
fn process_log_batch(tx: &mpsc::UnboundedSender<String>, body: &str) {
    // Minimal JSON parsing — we use serde_json which is already a dependency
    let entries: Vec<serde_json::Value> = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return,
    };

    for entry in &entries {
        let message = entry["message"].as_str().unwrap_or("");
        let level = entry["level"].as_str().unwrap_or("info");

        let prefix = match level {
            "error" => "studio-err",
            "warn" => "studio-warn",
            _ => "studio",
        };

        send_log(tx, prefix, message);
    }
}

/// Keep only the 10 most recent `session-*.log` files.
async fn cleanup_old_sessions(logs_dir: &std::path::Path) {
    let mut entries = match tokio::fs::read_dir(logs_dir).await {
        Ok(rd) => rd,
        Err(_) => return,
    };

    let mut session_files: Vec<std::path::PathBuf> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("session-") && name_str.ends_with(".log") {
            session_files.push(entry.path());
        }
    }

    // Sort by name (timestamp is embedded, so lexicographic = chronological)
    session_files.sort();

    // Remove oldest files until we have at most 10
    while session_files.len() > 10 {
        if let Some(oldest) = session_files.first() {
            let _ = tokio::fs::remove_file(oldest).await;
        }
        session_files.remove(0);
    }
}
