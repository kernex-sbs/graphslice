use anyhow::{Context, Result, anyhow};
use lsp_types::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use url::Url;

#[derive(Clone)]
pub struct LspClient {
    writer_tx: mpsc::UnboundedSender<String>,
    pending_requests: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value>>>>>,
    next_id: Arc<Mutex<i64>>,
    diagnostics: Arc<Mutex<HashMap<Uri, Vec<Diagnostic>>>>,
}

impl LspClient {
    /// Start rust-analyzer process and initialize
    pub async fn new(workspace_root: PathBuf) -> Result<Self> {
        let mut child = Command::new("rust-analyzer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn rust-analyzer")?;

        let mut stdin = child.stdin.take().context("Failed to open stdin")?;
        let stdout = child.stdout.take().context("Failed to open stdout")?;
        let stderr = child.stderr.take().context("Failed to open stderr")?;

        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<String>();
        let pending_requests: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value>>>>> = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics: Arc<Mutex<HashMap<Uri, Vec<Diagnostic>>>> = Arc::new(Mutex::new(HashMap::new()));

        // Stderr logger
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(_line)) = reader.next_line().await {
                // Keep stderr open but don't spam stdout unless needed
                // eprintln!("LSP Stderr: {}", _line);
            }
        });

        // Writer task
        tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                // eprintln!("--> LSP: {}", msg);
                let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                if stdin.write_all(content.as_bytes()).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Reader task
        let pending_requests_clone = pending_requests.clone();
        let diagnostics_clone = diagnostics.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);

            loop {
                // Read headers
                let mut content_length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return; // EOF
                    }

                    if line == "\r\n" {
                        break; // End of headers
                    }

                    if line.starts_with("Content-Length: ")
                        && let Ok(len) = line.trim_start_matches("Content-Length: ").trim().parse::<usize>() {
                            content_length = len;
                        }
                }

                if content_length > 0 {
                    let mut buffer = vec![0; content_length];
                    if reader.read_exact(&mut buffer).await.is_err() {
                        break;
                    }

                    if let Ok(val) = serde_json::from_slice::<Value>(&buffer) {
                        // eprintln!("<-- LSP: {}", serde_json::to_string(&val).unwrap_or_default());
                        if let Some(id) = val.get("id").and_then(|i| i.as_i64()) {
                            // Response
                            let mut requests = pending_requests_clone.lock().unwrap();
                            if let Some(tx) = requests.remove(&id) {
                                if let Some(error) = val.get("error") {
                                    let _ = tx.send(Err(anyhow!("LSP Error: {}", error)));
                                } else if let Some(result) = val.get("result") {
                                    let _ = tx.send(Ok(result.clone()));
                                } else {
                                    // Some responses might be null result for success
                                    let _ = tx.send(Ok(Value::Null));
                                }
                            }
                        } else {
                            // Notification or Request from server
                            if let Some(method) = val.get("method").and_then(|m| m.as_str())
                                && method == "textDocument/publishDiagnostics"
                                    && let Some(params) = val.get("params")
                                        && let Ok(diag_params) = serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) {
                                            let mut guard = diagnostics_clone.lock().unwrap();
                                            guard.insert(diag_params.uri, diag_params.diagnostics);
                                        }
                        }
                    }
                }
            }
        });

        let client = Self {
            writer_tx,
            pending_requests,
            next_id: Arc::new(Mutex::new(0)),
            diagnostics,
        };

        // Initialize
        let root_url = Url::from_file_path(&workspace_root).map_err(|_| anyhow!("Invalid path: {}", workspace_root.display()))?;
        let root_uri = Uri::from_str(root_url.as_str()).map_err(|e| anyhow!("Failed to create URI: {}", e))?;

        #[allow(deprecated)]
        let init_params = InitializeParams {
            root_uri: Some(root_uri.clone()),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri,
                name: workspace_root.file_name().unwrap_or_default().to_string_lossy().to_string(),
            }]),
            capabilities: ClientCapabilities::default(),
            ..Default::default()
        };

        // Wait for initialize response
        client.request("initialize", init_params).await?;
        client.notify("initialized", serde_json::json!({})).await?;

        Ok(client)
    }

    /// Send LSP request and get response
    async fn request<T: serde::Serialize>(
        &self,
        method: &str,
        params: T,
    ) -> Result<Value> {
        let params_value = serde_json::to_value(params)?;
        let mut attempts = 0;

        loop {
            attempts += 1;
            let id = {
                let mut guard = self.next_id.lock().unwrap();
                *guard += 1;
                *guard
            };

            let (tx, rx) = oneshot::channel();
            self.pending_requests.lock().unwrap().insert(id, tx);

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params_value,
            });

            self.writer_tx.send(serde_json::to_string(&request)?)
                .map_err(|_| anyhow!("LSP writer closed"))?;

            // eprintln!("Sending request (attempt {}): {}", attempts, method);

            match rx.await.context("LSP client dropped or response failed")? {
                Ok(val) => return Ok(val),
                Err(e) => {
                    let err_str = e.to_string();
                    // Check for "content modified" error (-32801)
                    if attempts < 5 && (err_str.contains("content modified") || err_str.contains("-32801")) {
                        // eprintln!("LSP 'content modified' error, retrying in {}ms...", 500 * attempts);
                        tokio::time::sleep(std::time::Duration::from_millis(500 * attempts as u64)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    /// Send LSP notification (no response expected)
    pub async fn notify<T: serde::Serialize>(
        &self,
        method: &str,
        params: T,
    ) -> Result<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        self.writer_tx.send(serde_json::to_string(&notification)?)
            .map_err(|_| anyhow!("LSP writer closed"))?;
        Ok(())
    }

    /// Notify server that a file was opened
    pub async fn did_open(&self, file_path: &PathBuf, text: String) -> Result<()> {
        let url = Url::from_file_path(file_path).map_err(|_| anyhow!("Invalid file path"))?;
        let uri = Uri::from_str(url.as_str()).map_err(|e| anyhow!("Failed to create URI: {}", e))?;

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "rust".to_string(),
                version: 0,
                text,
            },
        };

        self.notify("textDocument/didOpen", params).await
    }

    /// Get all references to symbol at position
    pub async fn get_references(
        &self,
        file_path: &PathBuf,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        let url = Url::from_file_path(file_path).map_err(|_| anyhow!("Invalid file path"))?;
        let uri = Uri::from_str(url.as_str()).map_err(|e| anyhow!("Failed to create URI: {}", e))?;

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        };

        let response = self.request("textDocument/references", params).await?;

        // Parse response into locations
        let locations: Vec<Location> = serde_json::from_value(response)
            .unwrap_or_default();

        Ok(locations)
    }

    /// Get definition of symbol at position
    pub async fn get_definition(
        &self,
        file_path: &PathBuf,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>> {
        let url = Url::from_file_path(file_path).map_err(|_| anyhow!("Invalid file path"))?;
        let uri = Uri::from_str(url.as_str()).map_err(|e| anyhow!("Failed to create URI: {}", e))?;

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = self.request("textDocument/definition", params).await?;

        // Definition response can be Location, []Location, or null
        // rust-analyzer usually returns []Location
        if response.is_null() {
            return Ok(Vec::new());
        }

        if let Ok(location) = serde_json::from_value::<Location>(response.clone()) {
            return Ok(vec![location]);
        }

        let locations: Vec<Location> = serde_json::from_value(response)
            .unwrap_or_default();

        Ok(locations)
    }

    /// Prepare call hierarchy at position
    pub async fn prepare_call_hierarchy(
        &self,
        file_path: &PathBuf,
        line: u32,
        character: u32,
    ) -> Result<Vec<CallHierarchyItem>> {
        let url = Url::from_file_path(file_path).map_err(|_| anyhow!("Invalid file path"))?;
        let uri = Uri::from_str(url.as_str()).map_err(|e| anyhow!("Failed to create URI: {}", e))?;

        let params = CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let response = self.request("textDocument/prepareCallHierarchy", params).await?;

        if response.is_null() {
            return Ok(Vec::new());
        }

        // Result is usually Vec<CallHierarchyItem>
        let items: Vec<CallHierarchyItem> = serde_json::from_value(response)
            .unwrap_or_default();

        Ok(items)
    }

    /// Get outgoing calls for a hierarchy item
    pub async fn get_outgoing_calls(
        &self,
        item: CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>> {
        let params = CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        let response = self.request("callHierarchy/outgoingCalls", params).await?;

        if response.is_null() {
            return Ok(Vec::new());
        }

        let calls: Vec<CallHierarchyOutgoingCall> = serde_json::from_value(response)
            .unwrap_or_default();

        Ok(calls)
    }

    /// Get diagnostics for a file
    pub fn get_diagnostics(&self, file_path: &PathBuf) -> Result<Vec<Diagnostic>> {
        let url = Url::from_file_path(file_path).map_err(|_| anyhow!("Invalid file path"))?;
        let uri = Uri::from_str(url.as_str()).map_err(|e| anyhow!("Failed to create URI: {}", e))?;

        let guard = self.diagnostics.lock().unwrap();
        Ok(guard.get(&uri).cloned().unwrap_or_default())
    }
}
