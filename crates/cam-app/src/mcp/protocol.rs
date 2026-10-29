//! Newline-delimited JSON-RPC, following the MCP stdio transport.
use super::*;
use std::io::{self, BufRead, Write};

const VERSION: &str = "2025-11-25";
// Bound a single message, including malformed input without a newline.
const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// Serve until EOF. Only protocol messages are written to `writer`.
/// The callback may execute locally or marshal commands onto the GUI event loop.
pub fn serve(
    mut reader: impl BufRead,
    mut writer: impl Write,
    tools: Vec<Value>,
    mut call: impl FnMut(&str, Value) -> CommandResult,
) -> io::Result<()> {
    let mut initialized = false;
    let mut ready = false;
    loop {
        let mut line = Vec::new();
        let mut oversized = false;
        loop {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                break;
            }
            let count = buffer
                .iter()
                .position(|b| *b == b'\n')
                .map_or(buffer.len(), |i| i + 1);
            if line.len().saturating_add(count) > MAX_MESSAGE {
                oversized = true;
            }
            if !oversized {
                line.extend_from_slice(&buffer[..count]);
            }
            let finished = buffer[count - 1] == b'\n';
            reader.consume(count);
            if finished {
                break;
            }
        }
        if line.is_empty() && !oversized {
            return Ok(());
        }
        let message: Value = match if oversized {
            Err("message exceeds 16 MiB".to_string())
        } else {
            serde_json::from_slice(&line).map_err(|e| e.to_string())
        } {
            Ok(message) => message,
            Err(error) => {
                send(&mut writer, error_response(Value::Null, -32700, &error))?;
                continue;
            }
        };
        let id = message.get("id").cloned();
        let method = message["method"].as_str();
        if message["jsonrpc"] != "2.0"
            || !message.is_object()
            || method.is_none()
            || id
                .as_ref()
                .is_some_and(|id| !id.is_string() && !id.is_i64() && !id.is_u64())
        {
            // Client responses are not requests and never receive responses.
            if message["jsonrpc"] == "2.0"
                && method.is_none()
                && (message.get("result").is_some() || message.get("error").is_some())
            {
                continue;
            }
            send(
                &mut writer,
                error_response(Value::Null, -32600, "invalid JSON-RPC request"),
            )?;
            continue;
        }
        let method = method.unwrap();
        let Some(id) = id else {
            if method == "notifications/initialized" && initialized {
                ready = true;
            }
            continue; // Notifications, including cancellation, never execute tools.
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "ping" => Ok(json!({})),
            "initialize" if !initialized => {
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"].is_object()
                {
                    Err((
                        -32602,
                        "initialize requires protocolVersion, capabilities and clientInfo"
                            .to_string(),
                    ))
                } else {
                    initialized = true;
                    let requested = params["protocolVersion"].as_str().unwrap();
                    let version = match requested {
                        "2024-11-05" | "2025-03-26" | "2025-06-18" => requested,
                        _ => VERSION,
                    };
                    Ok(
                        json!({"protocolVersion":version, "capabilities":{"tools":{}},
                        "serverInfo":{"name":"OpenCAMStudio", "version":env!("CARGO_PKG_VERSION")},
                        "instructions":"Use get_state to inspect the project and serialized model fields. Commands share application validation and undo history. start_run and start_cad_import are asynchronous; poll their status or cancel explicitly. Units are millimetres, degrees, seconds, RPM and mm/min unless stated otherwise."}),
                    )
                }
            }
            _ if !ready => Err((-32002, "complete MCP initialization first".into())),
            "tools/list" => {
                if !params.is_object() || params.get("cursor").is_some() {
                    Err((
                        -32602,
                        "this catalog does not use pagination cursors".into(),
                    ))
                } else {
                    Ok(json!({"tools":tools}))
                }
            }
            "tools/call" => match params["name"].as_str() {
                None => Err((-32602, "tools/call requires a tool name".into())),
                Some(name) if !tools.iter().any(|tool| tool["name"] == name) => {
                    Err((-32602, format!("unknown tool {name}")))
                }
                Some(name) => {
                    let arguments = params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    match call(name, arguments) {
                        Ok(value) => {
                            Ok(json!({"content":[{"type":"text","text":value.to_string()}],
                                "structuredContent":{"result":value}, "isError":false}))
                        }
                        Err(error) => {
                            Ok(json!({"content":[{"type":"text","text":error}], "isError":true}))
                        }
                    }
                }
            },
            _ => Err((-32601, format!("unknown method {method}"))),
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
            Err((code, error)) => error_response(id, code, &error),
        };
        send(&mut writer, response)?;
    }
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message}})
}
fn send(writer: &mut impl Write, response: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
