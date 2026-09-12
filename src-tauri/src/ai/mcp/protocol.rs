use serde_json::{json, Value};

use crate::error::{Result, VoloError};

/// 握手采用的 MCP 协议版本。
pub(super) const PROTOCOL_VERSION: &str = "2024-11-05";

/// MCP server 暴露的工具（原始名，未经 sanitize）。
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

/// 从 JSON-RPC 响应消息提取 result / error。
pub(super) fn rpc_outcome(msg: &Value) -> Result<Value> {
    if let Some(error) = msg.get("error") {
        return Err(VoloError::Other(format!(
            "MCP error {}: {}",
            error["code"],
            error["message"].as_str().unwrap_or("未知错误")
        )));
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

/// 解析 tools/list 的 result 为工具信息数组。
pub(super) fn parse_tools_list(result: &Value) -> Vec<McpToolInfo> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    Some(McpToolInfo {
                        name,
                        description: tool
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        input_schema: tool
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 从 tools/call 的 result 提取 text 类型 content 拼接；isError 为 true 时返回 Err。
pub(super) fn extract_tool_text(result: &Value, name: &str) -> Result<Value> {
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(VoloError::Other(format!(
            "MCP 工具 {} 执行失败: {}",
            name, text
        )));
    }
    Ok(Value::String(text))
}

/// 从 SSE 响应体中逐帧解析 data 行，找与请求 id 匹配的 JSON-RPC 响应。
pub(super) fn parse_sse_response(body: &str, id: u64) -> Result<Value> {
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if msg.get("id").and_then(Value::as_u64) == Some(id) {
            return rpc_outcome(&msg);
        }
    }
    Err(VoloError::Other(format!(
        "MCP SSE 响应中没有 id={} 的响应",
        id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_outcome_returns_result_error_or_null() {
        assert_eq!(
            rpc_outcome(&json!({ "id": 1, "result": { "ok": true } })).unwrap(),
            json!({ "ok": true })
        );
        let err = rpc_outcome(&json!({
            "id": 1,
            "error": { "code": -32601, "message": "missing" }
        }))
        .unwrap_err();
        assert!(err.to_string().contains("missing"));
        assert_eq!(rpc_outcome(&json!({ "id": 1 })).unwrap(), Value::Null);
    }

    #[test]
    fn tools_list_defaults_schema_and_skips_invalid_entries() {
        let tools = parse_tools_list(&json!({
            "tools": [
                {
                    "name": "echo",
                    "description": "Echo input",
                    "inputSchema": { "type": "object" }
                },
                { "name": "fallback" },
                { "description": "missing name" }
            ]
        }));
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description.as_deref(), Some("Echo input"));
        assert_eq!(tools[1].input_schema["type"], "object");
    }

    #[test]
    fn tool_text_joins_text_content_and_surfaces_errors() {
        let result = extract_tool_text(
            &json!({
                "content": [
                    { "type": "text", "text": "one" },
                    { "type": "image", "data": "ignored" },
                    { "type": "text", "text": "two" }
                ]
            }),
            "echo",
        )
        .unwrap();
        assert_eq!(result, Value::String("one\ntwo".to_string()));

        let err = extract_tool_text(
            &json!({
                "isError": true,
                "content": [{ "type": "text", "text": "boom" }]
            }),
            "fail",
        )
        .unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn sse_response_selects_matching_id() {
        let body = "event: message\n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":9,\"result\":{\"skip\":true}}\n\
                    \n\
                    data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(
            parse_sse_response(body, 2).unwrap(),
            json!({ "ok": true })
        );
        assert!(parse_sse_response(body, 99).is_err());
    }
}
