use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};

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

    /// Send a log line. Never blocks — just pushes to the channel.
    pub fn log(&self, prefix: &str, line: &str) {
        let ts = format_timestamp(unix_timestamp());
        let formatted = format!("[{ts}] [{prefix}] {line}\n");
        let _ = self.tx.send(formatted);
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
/// The server accepts two endpoints:
/// - `GET /health` → responds `200 ok`
/// - `POST /log` → parses a JSON batch of `{message, level, timestamp}` and writes to the session log
pub async fn start_log_server(
    log_tx: mpsc::UnboundedSender<String>,
) -> Option<tokio::task::JoinHandle<()>> {
    let listener = TcpListener::bind("127.0.0.1:19556").await.ok()?;

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => continue,
            };

            let tx = log_tx.clone();
            tokio::spawn(async move {
                handle_connection(stream, tx).await;
            });
        }
    });

    Some(handle)
}

/// Handle a single TCP connection with minimal HTTP parsing.
async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    tx: mpsc::UnboundedSender<String>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 65536];
    let n = match stream.read(&mut buf).await {
        Ok(0) | Err(_) => return,
        Ok(n) => n,
    };

    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the HTTP request line
    let first_line = request.lines().next().unwrap_or("");

    if first_line.starts_with("GET /health") {
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
