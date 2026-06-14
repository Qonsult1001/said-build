//! LSP client — connects to language servers (rust-analyzer, tsserver, pyright).
//!
//! Spawns a language server process, communicates via JSON-RPC over stdio.
//! Results get cached as frames in the .said file — first call hits LSP,
//! subsequent searches find it in the brain instantly.
//!
//! This is what Claude Code's LSPTool does (861 lines of TypeScript).
//! Our implementation does the same + caches results in the .said brain.

#[cfg(feature = "lsp")]
use std::io::{BufRead, BufReader, Write};
#[cfg(feature = "lsp")]
use std::process::{Child, Command, Stdio};
#[cfg(feature = "lsp")]
use std::path::Path;

/// LSP client — talks to a language server via JSON-RPC stdio.
#[cfg(feature = "lsp")]
pub struct LspClient {
    process: Child,
    request_id: i64,
    workspace_root: String,
}

#[cfg(feature = "lsp")]
impl LspClient {
    /// Connect to a language server by command name.
    /// Common servers: "rust-analyzer", "typescript-language-server", "pyright"
    pub fn connect(server_cmd: &str, workspace: &str) -> Result<Self, String> {
        let process = Command::new(server_cmd)
            .current_dir(workspace) // CRITICAL: must run from project root
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // capture stderr for diagnostics
            .spawn()
            .map_err(|e| format!("Failed to start {}: {}", server_cmd, e))?;

        // Clean workspace path (remove \\?\ Windows prefix)
        let clean_workspace = workspace
            .replace("\\\\?\\", "")
            .replace('\\', "/");

        let mut client = Self {
            process,
            request_id: 0,
            workspace_root: clean_workspace,
        };

        // Initialize the LSP connection
        eprintln!("[LSP] Initializing {}...", server_cmd);
        client.initialize()?;

        // Send "initialized" notification (required by LSP protocol)
        client.send_notification("initialized", serde_json::json!({}))?;

        // Wait for server to finish indexing (rust-analyzer indexes the whole project)
        // Keep reading notifications until we see progress/done or timeout
        eprintln!("[LSP] Waiting for server to finish indexing...");
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(120); // 2 minutes max

        loop {
            if start.elapsed() > timeout {
                eprintln!("[LSP] Timeout waiting for indexing ({}s). Proceeding anyway.", timeout.as_secs());
                break;
            }

            // Try reading a notification — if server sends progress, it's still working
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Check if server is still alive
            if let Some(ref mut proc) = Some(&mut client.process) {
                match proc.try_wait() {
                    Ok(Some(status)) => {
                        return Err(format!("Server exited with status: {}", status));
                    }
                    Ok(None) => {} // still running
                    Err(_) => break,
                }
            }

            // Try a workspace symbol query with a common name — if results come back, indexing is done
            if start.elapsed() > std::time::Duration::from_secs(5) {
                match client.workspace_symbol("main") {
                    Ok(ref s) if !s.is_empty() => {
                        eprintln!("[LSP] Server ready after {:.1}s ({} symbols for 'main')",
                            start.elapsed().as_secs_f64(), s.lines().count());
                        break;
                    }
                    Ok(_) => {
                        // Empty result — server responded but hasn't indexed yet
                        eprintln!("[LSP] Still indexing... ({:.0}s)", start.elapsed().as_secs_f64());
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                    Err(_) => continue, // not ready yet
                }
            }
        }

        eprintln!("[LSP] Connected to {}", server_cmd);
        Ok(client)
    }

    /// Initialize the LSP connection (required before any requests).
    fn initialize(&mut self) -> Result<serde_json::Value, String> {
        let params = serde_json::json!({
            "processId": std::process::id(),
            "capabilities": {
                "textDocument": {
                    "definition": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "hover": { "contentFormat": ["plaintext"] },
                    "documentSymbol": { "dynamicRegistration": false },
                    "implementation": { "dynamicRegistration": false }
                },
                "workspace": {
                    "symbol": { "dynamicRegistration": false }
                }
            },
            "rootUri": self.file_uri("."),
            "workspaceFolders": [{
                "uri": self.file_uri("."),
                "name": "workspace"
            }]
        });

        self.send_request("initialize", params)
    }

    /// Go to definition — finds where a symbol is defined.
    pub fn go_to_definition(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let params = self.text_document_position_params(file_path, line, character);
        let result = self.send_request("textDocument/definition", params)?;
        Ok(Self::format_location_result(&result))
    }

    /// Find references — finds all usages of a symbol.
    pub fn find_references(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let mut params = self.text_document_position_params(file_path, line, character);
        params["context"] = serde_json::json!({ "includeDeclaration": true });
        let result = self.send_request("textDocument/references", params)?;
        Ok(Self::format_location_result(&result))
    }

    /// Hover — gets type info and documentation for a symbol.
    pub fn hover(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let params = self.text_document_position_params(file_path, line, character);
        let result = self.send_request("textDocument/hover", params)?;
        // Extract hover contents
        if let Some(contents) = result.get("contents") {
            if let Some(value) = contents.get("value") {
                return Ok(value.as_str().unwrap_or("").to_string());
            }
            if let Some(s) = contents.as_str() {
                return Ok(s.to_string());
            }
        }
        Ok(format!("{}", result))
    }

    /// Document symbols — lists all symbols in a file.
    pub fn document_symbols(&mut self, file_path: &str) -> Result<String, String> {
        let params = serde_json::json!({
            "textDocument": {
                "uri": self.file_uri(file_path)
            }
        });
        let result = self.send_request("textDocument/documentSymbol", params)?;
        Ok(Self::format_symbols(&result))
    }

    /// Workspace symbol — searches for symbols across the entire workspace.
    pub fn workspace_symbol(&mut self, query: &str) -> Result<String, String> {
        let params = serde_json::json!({ "query": query });
        let result = self.send_request("workspace/symbol", params)?;
        Ok(Self::format_symbols(&result))
    }

    /// Go to implementation — finds implementations of an interface.
    pub fn go_to_implementation(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let params = self.text_document_position_params(file_path, line, character);
        let result = self.send_request("textDocument/implementation", params)?;
        Ok(Self::format_location_result(&result))
    }

    /// Incoming calls — who calls this function?
    pub fn incoming_calls(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        // First prepare call hierarchy
        let params = self.text_document_position_params(file_path, line, character);
        let items = self.send_request("textDocument/prepareCallHierarchy", params)?;

        if let Some(item) = items.as_array().and_then(|a| a.first()) {
            let result = self.send_request("callHierarchy/incomingCalls",
                serde_json::json!({ "item": item }))?;
            return Ok(format!("{}", result));
        }
        Ok("No call hierarchy item found".to_string())
    }

    /// Outgoing calls — what does this function call?
    pub fn outgoing_calls(&mut self, file_path: &str, line: u32, character: u32) -> Result<String, String> {
        let params = self.text_document_position_params(file_path, line, character);
        let items = self.send_request("textDocument/prepareCallHierarchy", params)?;

        if let Some(item) = items.as_array().and_then(|a| a.first()) {
            let result = self.send_request("callHierarchy/outgoingCalls",
                serde_json::json!({ "item": item }))?;
            return Ok(format!("{}", result));
        }
        Ok("No call hierarchy item found".to_string())
    }

    // =========================================================================
    // JSON-RPC TRANSPORT
    // =========================================================================

    /// Send a notification (no response expected).
    fn send_notification(&mut self, method: &str, params: serde_json::Value) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body = serde_json::to_string(&notification)
            .map_err(|e| format!("JSON serialize: {}", e))?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let stdin = self.process.stdin.as_mut().ok_or("No stdin")?;
        stdin.write_all(header.as_bytes()).map_err(|e| format!("Write: {}", e))?;
        stdin.write_all(body.as_bytes()).map_err(|e| format!("Write: {}", e))?;
        stdin.flush().map_err(|e| format!("Flush: {}", e))?;
        Ok(())
    }

    fn send_request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        self.request_id += 1;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params,
        });

        let body = serde_json::to_string(&request)
            .map_err(|e| format!("JSON serialize: {}", e))?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        // Write to stdin
        let stdin = self.process.stdin.as_mut()
            .ok_or("No stdin")?;
        stdin.write_all(header.as_bytes()).map_err(|e| format!("Write: {}", e))?;
        stdin.write_all(body.as_bytes()).map_err(|e| format!("Write: {}", e))?;
        stdin.flush().map_err(|e| format!("Flush: {}", e))?;

        // Read response — skip notifications, wait for matching response id
        let stdout = self.process.stdout.as_mut()
            .ok_or("No stdout")?;
        let mut reader = BufReader::new(stdout);
        let expected_id = self.request_id;

        // Loop: read messages until we get our response (skip notifications)
        for _attempt in 0..50 { // max 50 messages before giving up
            let msg = Self::read_one_message(&mut reader)?;

            // Check if this is our response (has matching id)
            if let Some(id) = msg.get("id") {
                if id.as_i64() == Some(expected_id) {
                    if let Some(result) = msg.get("result") {
                        return Ok(result.clone());
                    } else if let Some(error) = msg.get("error") {
                        return Err(format!("LSP error: {}", error));
                    }
                    return Ok(serde_json::Value::Null);
                }
            }
            // No id = notification (progress, log, etc.) — skip and read next
        }

        Err("Timeout: no response after 50 messages".to_string())
    }

    /// Read one JSON-RPC message from the LSP server stdout.
    fn read_one_message(reader: &mut BufReader<&mut std::process::ChildStdout>) -> Result<serde_json::Value, String> {
        // Read headers until we find Content-Length
        let mut content_length: usize = 0;
        loop {
            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).map_err(|e| format!("Read header: {}", e))?;
            if bytes_read == 0 {
                return Err("Server closed connection".to_string());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if content_length > 0 { break; } // blank line after Content-Length
                continue; // skip leading blank lines
            }
            if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                content_length = len_str.trim().parse().unwrap_or(0);
            }
        }

        if content_length == 0 {
            return Err("No Content-Length header".to_string());
        }

        // Read body
        let mut body_buf = vec![0u8; content_length];
        std::io::Read::read_exact(reader, &mut body_buf)
            .map_err(|e| format!("Read body ({} bytes): {}", content_length, e))?;

        serde_json::from_slice(&body_buf)
            .map_err(|e| format!("Parse message ({} bytes): {}", content_length, e))
    }

    fn text_document_position_params(&self, file_path: &str, line: u32, character: u32) -> serde_json::Value {
        serde_json::json!({
            "textDocument": { "uri": self.file_uri(file_path) },
            "position": { "line": line.saturating_sub(1), "character": character.saturating_sub(1) }
        })
    }

    fn file_uri(&self, file_path: &str) -> String {
        let full = if Path::new(file_path).is_absolute() {
            file_path.to_string()
        } else {
            format!("{}/{}", self.workspace_root, file_path)
        };
        // Clean up Windows path: remove \\?\ prefix, normalize slashes
        let clean = full
            .replace("\\\\?\\", "")
            .replace('\\', "/");
        // Ensure proper file:/// URI (3 slashes for absolute path)
        if clean.starts_with('/') {
            format!("file://{}", clean)
        } else {
            format!("file:///{}", clean)
        }
    }

    fn format_location_result(value: &serde_json::Value) -> String {
        let mut results = Vec::new();
        let locations = if value.is_array() { value.as_array().unwrap().clone() } else { vec![value.clone()] };
        for loc in &locations {
            let uri = loc.get("uri").or_else(|| loc.get("targetUri"))
                .and_then(|u| u.as_str()).unwrap_or("");
            let range = loc.get("range").or_else(|| loc.get("targetRange"));
            let line = range.and_then(|r| r.get("start")).and_then(|s| s.get("line"))
                .and_then(|l| l.as_u64()).unwrap_or(0) + 1;
            let file = uri.strip_prefix("file://").unwrap_or(uri);
            results.push(format!("{}:{}", file, line));
        }
        results.join("\n")
    }

    fn format_symbols(value: &serde_json::Value) -> String {
        let mut results = Vec::new();
        if let Some(arr) = value.as_array() {
            for sym in arr {
                let name = sym.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                let kind = sym.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
                let kind_name = match kind {
                    1 => "file", 2 => "module", 3 => "namespace", 5 => "class",
                    6 => "method", 8 => "field", 12 => "function", 13 => "variable",
                    14 => "constant", 22 => "struct", 23 => "enum", 25 => "trait",
                    _ => "symbol",
                };
                results.push(format!("{} [{}]", name, kind_name));
            }
        }
        results.join("\n")
    }
}

#[cfg(feature = "lsp")]
impl Drop for LspClient {
    fn drop(&mut self) {
        // Send shutdown + exit
        let _ = self.send_request("shutdown", serde_json::Value::Null);
        let _ = self.process.kill();
    }
}

/// Integration: cache LSP results in a SaidFile for instant future recall.
#[cfg(feature = "lsp")]
pub fn cache_lsp_result(
    brain: &mut crate::said_file::SaidFile,
    operation: &str,
    file_path: &str,
    line: u32,
    result: &str,
) {
    let doc_id = format!("lsp::{}::{}:{}", operation, file_path, line);
    let title = format!("LSP {} at {}:{}", operation, file_path, line);
    brain.put(&doc_id, result, Some(&title));
}
