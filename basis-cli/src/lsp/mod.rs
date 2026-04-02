//! Basis LSP server — publishes governance diagnostics to editors.
//!
//! Runs on stdio. Triggers a full `basis check` on file open/save,
//! then pushes violations as LSP diagnostics to the editor.
//!
//! Features:
//! - Workspace folder awareness (uses InitializeParams to find project roots)
//! - Spec caching (skips re-check when basis.yaml mtime is unchanged)
//! - Progress reporting ($/progress notifications during checks)
//! - Full-project diagnostics (publishes for all files, clears stale squiggles)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::SystemTime;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    InitializeParams, NumberOrString, Position, ProgressParams, ProgressParamsValue,
    PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd,
};

use crate::check;
use crate::language::LangRegistry;
use crate::loader;
use crate::validate;

/// Monotonic counter for progress tokens and request IDs.
static NEXT_ID: AtomicI32 = AtomicI32::new(1);

fn next_id() -> i32 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Cached state for a single workspace (project root with a basis.yaml).
struct WorkspaceCache {
    spec_mtime: Option<SystemTime>,
    violations: Vec<check::output::UnifiedViolation>,
    /// Files we last published diagnostics for (so we can clear them).
    published_files: Vec<PathBuf>,
}

/// Server state across the session.
struct ServerState {
    /// Workspace roots discovered from InitializeParams.
    workspace_roots: Vec<PathBuf>,
    /// Cache keyed by spec file path.
    caches: HashMap<PathBuf, WorkspaceCache>,
}

impl ServerState {
    fn new(workspace_roots: Vec<PathBuf>) -> Self {
        Self {
            workspace_roots,
            caches: HashMap::new(),
        }
    }

    /// Find the spec file for a given file path.
    /// First checks workspace roots, then walks up from the file.
    fn find_spec_for(&self, file_path: &Path) -> Option<PathBuf> {
        // Check workspace roots first — fast path
        for root in &self.workspace_roots {
            if file_path.starts_with(root) {
                let candidate = root.join("basis.yaml");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        // Fallback: walk up from the file
        find_spec_file(file_path)
    }

    /// Check if the spec has changed since last check.
    fn spec_changed(&self, spec_path: &Path) -> bool {
        let current_mtime = std::fs::metadata(spec_path)
            .and_then(|m| m.modified())
            .ok();

        match self.caches.get(spec_path) {
            Some(cache) => cache.spec_mtime != current_mtime,
            None => true, // No cache yet
        }
    }

    /// Get cached violations if spec hasn't changed.
    fn get_cached(&self, spec_path: &Path) -> Option<&Vec<check::output::UnifiedViolation>> {
        if !self.spec_changed(spec_path) {
            self.caches.get(spec_path).map(|c| &c.violations)
        } else {
            None
        }
    }

    /// Store check results in cache.
    fn store(
        &mut self,
        spec_path: PathBuf,
        violations: Vec<check::output::UnifiedViolation>,
        published_files: Vec<PathBuf>,
    ) {
        let mtime = std::fs::metadata(&spec_path)
            .and_then(|m| m.modified())
            .ok();
        self.caches.insert(
            spec_path.clone(),
            WorkspaceCache {
                spec_mtime: mtime,
                violations,
                published_files,
            },
        );
    }
}

/// Run the LSP server on stdio until shutdown.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (connection, io_threads) = Connection::stdio();

    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        ..Default::default()
    };

    let caps_json = serde_json::to_value(capabilities)?;
    let init_params_json = connection.initialize(caps_json)?;

    // Extract workspace roots from InitializeParams
    let workspace_roots = extract_workspace_roots(&init_params_json);

    let mut state = ServerState::new(workspace_roots);

    main_loop(&connection, &mut state)?;

    io_threads.join()?;
    Ok(())
}

/// Extract workspace roots from the initialize params.
fn extract_workspace_roots(params_json: &serde_json::Value) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(params) = serde_json::from_value::<InitializeParams>(params_json.clone()) {
        // Prefer workspace_folders (modern LSP)
        if let Some(folders) = params.workspace_folders {
            for folder in folders {
                if let Some(path) = uri_to_path(&folder.uri) {
                    roots.push(path);
                }
            }
        }

        // Fallback to root_uri
        #[allow(deprecated)]
        if roots.is_empty() {
            if let Some(ref uri) = params.root_uri {
                if let Some(path) = uri_to_path(uri) {
                    roots.push(path);
                }
            }
        }
    }

    roots
}

/// Process messages until shutdown.
fn main_loop(
    connection: &Connection,
    state: &mut ServerState,
) -> Result<(), Box<dyn std::error::Error>> {
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
            }
            Message::Notification(notif) => {
                handle_notification(connection, state, notif);
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// Route notifications to handlers.
fn handle_notification(connection: &Connection, state: &mut ServerState, notif: Notification) {
    match notif.method.as_str() {
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(notif.params) {
                check_and_publish(connection, state, &params.text_document.uri);
            }
        }
        "textDocument/didSave" => {
            if let Ok(params) = serde_json::from_value::<DidSaveTextDocumentParams>(notif.params) {
                // If the saved file IS a basis.yaml, invalidate its cache
                if let Some(path) = uri_to_path(&params.text_document.uri) {
                    if path.file_name().is_some_and(|n| n == "basis.yaml") {
                        state.caches.remove(&path);
                    }
                }
                check_and_publish(connection, state, &params.text_document.uri);
            }
        }
        _ => {}
    }
}

/// Extract a filesystem path from a file:// URI.
fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let s = uri.as_str();
    if !s.starts_with("file:///") {
        return None;
    }
    let path_str = &s["file:///".len()..];
    // Decode percent-encoded characters (minimal: spaces, colons)
    let decoded = path_str
        .replace("%20", " ")
        .replace("%3A", ":")
        .replace("%3a", ":");
    // On Windows, file:///C:/foo → C:/foo. On Unix, file:///home → /home.
    #[cfg(windows)]
    {
        Some(PathBuf::from(decoded))
    }
    #[cfg(not(windows))]
    {
        Some(PathBuf::from(format!("/{decoded}")))
    }
}

/// Convert a PathBuf back to a file:// URI.
fn path_to_uri(path: &Path) -> Option<Uri> {
    let s = path.to_string_lossy();
    #[cfg(windows)]
    let uri_str = format!(
        "file:///{}",
        s.replace('\\', "/").replace(' ', "%20").replace(':', "%3A")
    );
    #[cfg(not(windows))]
    let uri_str = format!("file://{}", s.replace(' ', "%20"));
    uri_str.parse().ok()
}

/// Send a progress begin/end notification pair around a check.
fn send_progress_begin(connection: &Connection, token: NumberOrString) {
    // Create the progress token via a request
    let create_params = WorkDoneProgressCreateParams {
        token: token.clone(),
    };
    let req = Request {
        id: RequestId::from(next_id()),
        method: "window/workDoneProgress/create".to_string(),
        params: serde_json::to_value(create_params).unwrap(),
    };
    let _ = connection.sender.send(Message::Request(req));

    // Send begin notification
    let progress = ProgressParams {
        token,
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: "basis check".to_string(),
            cancellable: Some(false),
            message: Some("Checking governance rules...".to_string()),
            percentage: None,
        })),
    };
    let notif = Notification::new(
        "$/progress".to_string(),
        serde_json::to_value(progress).unwrap(),
    );
    let _ = connection.sender.send(Message::Notification(notif));
}

fn send_progress_end(connection: &Connection, token: NumberOrString, violation_count: usize) {
    let msg = if violation_count == 0 {
        "basis check passed".to_string()
    } else {
        format!("{violation_count} violation(s)")
    };
    let progress = ProgressParams {
        token,
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some(msg),
        })),
    };
    let notif = Notification::new(
        "$/progress".to_string(),
        serde_json::to_value(progress).unwrap(),
    );
    let _ = connection.sender.send(Message::Notification(notif));
}

/// Run basis check for the file's project and publish diagnostics.
fn check_and_publish(connection: &Connection, state: &mut ServerState, uri: &Uri) {
    let Some(file_path) = uri_to_path(uri) else {
        return;
    };

    let Some(spec_path) = state.find_spec_for(&file_path) else {
        return; // No basis.yaml found — nothing to check
    };

    let root = spec_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();

    // Check cache — skip re-check if spec hasn't changed
    if let Some(cached_violations) = state.get_cached(&spec_path) {
        publish_all_diagnostics(connection, cached_violations, &root);
        return;
    }

    // Progress: begin
    let token = NumberOrString::Number(next_id());
    send_progress_begin(connection, token.clone());

    // Load and validate spec
    let spec = match loader::load_spec(&spec_path) {
        Ok(s) => s,
        Err(_) => {
            send_progress_end(connection, token, 0);
            return;
        }
    };

    if !validate::validate(&spec).is_empty() {
        send_progress_end(connection, token, 0);
        return;
    }

    // Run all four checks
    let registry = LangRegistry::new();
    let (violations, _axes) = run_check(&spec, &root, &registry);
    let violation_count = violations.len();

    // Progress: end
    send_progress_end(connection, token, violation_count);

    // Clear diagnostics for files that had violations last time but don't now
    if let Some(old_cache) = state.caches.get(&spec_path) {
        for old_file in &old_cache.published_files {
            let still_has = violations.iter().any(|v| root.join(&v.file) == *old_file);
            if !still_has {
                if let Some(file_uri) = path_to_uri(old_file) {
                    let clear = PublishDiagnosticsParams {
                        uri: file_uri,
                        diagnostics: vec![],
                        version: None,
                    };
                    let notif = Notification::new(
                        "textDocument/publishDiagnostics".to_string(),
                        serde_json::to_value(clear).unwrap(),
                    );
                    let _ = connection.sender.send(Message::Notification(notif));
                }
            }
        }
    }

    // Publish diagnostics for all files with violations
    let published_files = publish_all_diagnostics(connection, &violations, &root);

    // Cache results
    state.store(spec_path, violations, published_files);
}

/// Publish diagnostics for all files with violations. Returns the list of files published.
fn publish_all_diagnostics(
    connection: &Connection,
    violations: &[check::output::UnifiedViolation],
    root: &Path,
) -> Vec<PathBuf> {
    // Group violations by file
    let mut by_file: HashMap<PathBuf, Vec<&check::output::UnifiedViolation>> = HashMap::new();
    for v in violations {
        let abs_path = root.join(&v.file);
        by_file.entry(abs_path).or_default().push(v);
    }

    let mut published = Vec::new();

    for (file_path, file_violations) in &by_file {
        let diagnostics: Vec<Diagnostic> = file_violations
            .iter()
            .map(|v| violation_to_diagnostic(v))
            .collect();

        if let Some(file_uri) = path_to_uri(file_path) {
            let publish = PublishDiagnosticsParams {
                uri: file_uri,
                diagnostics,
                version: None,
            };

            let notif = Notification::new(
                "textDocument/publishDiagnostics".to_string(),
                serde_json::to_value(publish).unwrap(),
            );
            let _ = connection.sender.send(Message::Notification(notif));
            published.push(file_path.clone());
        }
    }

    published
}

/// Run all five checks (replicates the logic from main.rs run_check).
fn run_check(
    spec: &crate::spec::BasisSpec,
    root: &Path,
    registry: &LangRegistry,
) -> (Vec<check::output::UnifiedViolation>, Vec<String>) {
    let mut violations = Vec::new();
    let mut axes_checked = Vec::new();

    axes_checked.push("placement".to_string());
    let hits = check::placement::check_placement(spec, root, registry);
    for v in &hits {
        violations.push(check::output::UnifiedViolation::from(v));
    }

    axes_checked.push("values".to_string());
    let hits = check::values::check_values(spec, root, registry);
    for v in &hits {
        violations.push(check::output::UnifiedViolation::from(v));
    }

    axes_checked.push("completeness".to_string());
    let result = check::completeness::check_completeness(spec, root, registry);
    for v in &result.violations {
        violations.push(check::output::UnifiedViolation::from(v));
    }

    axes_checked.push("purity".to_string());
    let hits = check::purity::check_purity(spec, root, registry);
    for v in &hits {
        violations.push(check::output::UnifiedViolation::from(v));
    }

    axes_checked.push("contracts".to_string());
    let hits = check::contracts::check_contracts(spec, root, registry);
    for v in &hits {
        violations.push(check::output::UnifiedViolation::from(v));
    }

    (violations, axes_checked)
}

/// Convert a UnifiedViolation to an LSP Diagnostic.
fn violation_to_diagnostic(v: &check::output::UnifiedViolation) -> Diagnostic {
    let line = if v.line > 0 { v.line - 1 } else { 0 };
    Diagnostic {
        range: Range::new(Position::new(line as u32, 0), Position::new(line as u32, 200)),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(v.code.clone())),
        source: Some("basis".to_string()),
        message: v.message.clone(),
        ..Default::default()
    }
}

/// Walk up from a file path looking for basis.yaml.
fn find_spec_file(file_path: &Path) -> Option<PathBuf> {
    let mut dir = file_path.parent()?;
    loop {
        let candidate = dir.join("basis.yaml");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_spec_file_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            dir.path().join("basis.yaml"),
            "governance:\n  version: '1.0'\n",
        )
        .unwrap();

        let file = sub.join("test.py");
        std::fs::write(&file, "").unwrap();

        let found = find_spec_file(&file);
        assert!(found.is_some());
        assert_eq!(found.unwrap().file_name().unwrap(), "basis.yaml");
    }

    #[test]
    fn find_spec_file_returns_none_at_root() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("orphan.py");
        std::fs::write(&file, "").unwrap();

        assert!(find_spec_file(&file).is_none());
    }

    #[test]
    fn violation_to_diagnostic_converts_correctly() {
        let v = check::output::UnifiedViolation {
            axis: "placement".into(),
            code: "B001".into(),
            file: "src/logic/compute.py".into(),
            line: 5,
            message: "import crosses boundary".into(),
            identity: "B001:src/logic/compute.py:requests".into(),
            details: check::output::ViolationDetails::Placement {
                from_layer: "laboratory".into(),
                to_layer: "hands".into(),
                module: "requests".into(),
            },
        };
        let d = violation_to_diagnostic(&v);
        assert_eq!(d.range.start.line, 4); // 0-indexed
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(d.code, Some(NumberOrString::String("B001".into())));
        assert_eq!(d.source, Some("basis".into()));
    }

    #[test]
    fn uri_to_path_extracts_file_path() {
        let uri: Uri = "file:///C%3A/Users/test/project/src/main.rs".parse().unwrap();
        let path = uri_to_path(&uri).unwrap();
        assert!(path.to_string_lossy().contains("Users"));
        assert!(path.to_string_lossy().contains("main.rs"));
    }

    #[test]
    fn uri_to_path_rejects_non_file() {
        let uri: Uri = "https://example.com/spec.yaml".parse().unwrap();
        assert!(uri_to_path(&uri).is_none());
    }

    #[test]
    fn server_state_finds_spec_from_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("basis.yaml");
        std::fs::write(&spec_path, "governance:\n  version: '1.0'\n").unwrap();

        let sub = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        let file = sub.join("test.py");
        std::fs::write(&file, "").unwrap();

        let state = ServerState::new(vec![dir.path().to_path_buf()]);
        let found = state.find_spec_for(&file);
        assert_eq!(found, Some(spec_path));
    }

    #[test]
    fn server_state_cache_detects_unchanged_spec() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("basis.yaml");
        std::fs::write(&spec_path, "governance:\n  version: '1.0'\n").unwrap();

        let mut state = ServerState::new(vec![]);

        // No cache yet — spec_changed returns true
        assert!(state.spec_changed(&spec_path));

        // Store violations
        state.store(spec_path.clone(), vec![], vec![]);

        // Now spec_changed returns false (mtime unchanged)
        assert!(!state.spec_changed(&spec_path));

        // Modify the file — mtime changes
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&spec_path, "governance:\n  version: '2.0'\n").unwrap();
        assert!(state.spec_changed(&spec_path));
    }

    #[test]
    fn server_state_cache_returns_violations() {
        let dir = tempfile::tempdir().unwrap();
        let spec_path = dir.path().join("basis.yaml");
        std::fs::write(&spec_path, "governance:\n  version: '1.0'\n").unwrap();

        let mut state = ServerState::new(vec![]);

        // No cache
        assert!(state.get_cached(&spec_path).is_none());

        // Store some violations
        let v = check::output::UnifiedViolation {
            axis: "placement".into(),
            code: "B001".into(),
            file: "src/test.py".into(),
            line: 1,
            message: "test".into(),
            identity: "B001:src/test.py:x".into(),
            details: check::output::ViolationDetails::Placement {
                from_layer: "a".into(),
                to_layer: "b".into(),
                module: "x".into(),
            },
        };
        state.store(spec_path.clone(), vec![v], vec![]);

        // Cache hit
        let cached = state.get_cached(&spec_path);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }

    #[test]
    fn extract_workspace_roots_from_empty_params() {
        let params = serde_json::json!({});
        let roots = extract_workspace_roots(&params);
        assert!(roots.is_empty());
    }

    #[test]
    fn path_to_uri_roundtrip() {
        // Basic test that path_to_uri produces a parseable URI
        #[cfg(windows)]
        {
            let path = PathBuf::from("C:/Users/test/project");
            let uri = path_to_uri(&path);
            assert!(uri.is_some());
            let uri = uri.unwrap();
            assert!(uri.as_str().starts_with("file:///"));
        }
        #[cfg(not(windows))]
        {
            let path = PathBuf::from("/home/test/project");
            let uri = path_to_uri(&path);
            assert!(uri.is_some());
            let uri = uri.unwrap();
            assert!(uri.as_str().starts_with("file:///"));
        }
    }
}
