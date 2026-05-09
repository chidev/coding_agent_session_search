use std::fs::File;
use std::io::{BufRead, BufReader};

use anyhow::Result;
use serde_json::Value;

use super::{
    Connector, DetectionResult, DiscoveredSourceFile, NormalizedConversation, NormalizedMessage,
    ScanContext, parse_timestamp, reindex_messages,
};

const MAX_INDEXED_TOOL_OUTPUT_CHARS: usize = 128 * 1024;

pub struct CodexConnector {
    inner: franken_agent_detection::CodexConnector,
}

impl Default for CodexConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexConnector {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: franken_agent_detection::CodexConnector::new(),
        }
    }
}

impl Connector for CodexConnector {
    fn detect(&self) -> DetectionResult {
        self.inner.detect()
    }

    fn scan(&self, ctx: &ScanContext) -> Result<Vec<NormalizedConversation>> {
        let mut conversations = self.inner.scan(ctx)?;
        for conversation in &mut conversations {
            augment_modern_codex_messages(conversation);
        }
        Ok(conversations)
    }

    fn supports_streaming_scan(&self) -> bool {
        self.inner.supports_streaming_scan()
    }

    fn discover_source_files(&self, ctx: &ScanContext) -> Result<Vec<DiscoveredSourceFile>> {
        self.inner.discover_source_files(ctx)
    }

    fn scan_with_callback(
        &self,
        ctx: &ScanContext,
        on_conversation: &mut dyn FnMut(NormalizedConversation) -> Result<()>,
    ) -> Result<()> {
        self.inner.scan_with_callback(ctx, &mut |mut conversation| {
            augment_modern_codex_messages(&mut conversation);
            on_conversation(conversation)
        })
    }
}

fn augment_modern_codex_messages(conversation: &mut NormalizedConversation) {
    if conversation
        .source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("jsonl"))
    {
        return;
    }

    let Ok(file) = File::open(&conversation.source_path) else {
        return;
    };

    let mut added = false;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(message) = modern_codex_message(&raw) else {
            continue;
        };
        if message_already_indexed(conversation, &message) {
            continue;
        }
        conversation.messages.push(message);
        added = true;
    }

    if added {
        conversation.messages.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.idx.cmp(&right.idx))
        });
        reindex_messages(&mut conversation.messages);
    }
}

fn modern_codex_message(raw: &Value) -> Option<NormalizedMessage> {
    let entry_type = raw.get("type").and_then(Value::as_str)?;
    let payload = raw.get("payload")?;
    let created_at = raw.get("timestamp").and_then(parse_timestamp);

    match entry_type {
        "response_item" => response_item_message(payload, created_at, raw),
        "event_msg" => event_message(payload, created_at, raw),
        _ => None,
    }
}

fn response_item_message(
    payload: &Value,
    created_at: Option<i64>,
    raw: &Value,
) -> Option<NormalizedMessage> {
    match payload.get("type").and_then(Value::as_str) {
        Some("message") | None => {
            let content = payload.get("content").and_then(flatten_modern_content)?;
            let role = payload
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("agent")
                .to_string();
            Some(normalized_message(
                role,
                None,
                created_at,
                content,
                raw.clone(),
                payload.get("content").map_or_else(
                    Vec::new,
                    franken_agent_detection::extract_invocations_from_content_blocks,
                ),
            ))
        }
        Some("function_call") => {
            let tool_name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let arguments = payload.get("arguments").cloned();
            let content = tool_call_content(tool_name, arguments.as_ref());
            let call_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(normalized_message(
                "assistant".to_string(),
                None,
                created_at,
                content,
                raw.clone(),
                vec![franken_agent_detection::NormalizedInvocation {
                    kind: "tool".to_string(),
                    name: tool_name.to_string(),
                    raw_name: None,
                    call_id,
                    arguments: arguments.and_then(normalize_invocation_arguments),
                }],
            ))
        }
        Some("function_call_output") => {
            let output = payload.get("output").and_then(Value::as_str)?;
            let call_id = payload.get("call_id").and_then(Value::as_str);
            Some(normalized_message(
                "tool".to_string(),
                None,
                created_at,
                tool_output_content(call_id, output),
                raw.clone(),
                Vec::new(),
            ))
        }
        _ => None,
    }
}

fn event_message(
    payload: &Value,
    created_at: Option<i64>,
    raw: &Value,
) -> Option<NormalizedMessage> {
    match payload.get("type").and_then(Value::as_str) {
        Some("agent_message") => {
            let content = payload
                .get("message")
                .or_else(|| payload.get("text"))
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            non_empty_message("assistant".to_string(), None, created_at, content, raw)
        }
        Some("tool_result") => {
            let output = payload
                .get("output")
                .or_else(|| payload.get("result"))
                .and_then(Value::as_str)?;
            let call_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str);
            Some(normalized_message(
                "tool".to_string(),
                None,
                created_at,
                tool_output_content(call_id, output),
                raw.clone(),
                Vec::new(),
            ))
        }
        _ => None,
    }
}

fn normalized_message(
    role: String,
    author: Option<String>,
    created_at: Option<i64>,
    content: String,
    extra: Value,
    invocations: Vec<franken_agent_detection::NormalizedInvocation>,
) -> NormalizedMessage {
    NormalizedMessage {
        idx: 0,
        role,
        author,
        created_at,
        content,
        extra,
        invocations,
        snippets: Vec::new(),
    }
}

fn non_empty_message(
    role: String,
    author: Option<String>,
    created_at: Option<i64>,
    content: String,
    raw: &Value,
) -> Option<NormalizedMessage> {
    (!content.trim().is_empty())
        .then(|| normalized_message(role, author, created_at, content, raw.clone(), Vec::new()))
}

fn flatten_modern_content(content: &Value) -> Option<String> {
    if let Some(text) = content
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(text.to_string());
    }

    let mut parts = Vec::new();
    for item in content.as_array()? {
        let text = if let Some(text) = item.as_str() {
            text
        } else {
            let item_type = item.get("type").and_then(Value::as_str);
            if !matches!(
                item_type,
                None | Some("text") | Some("input_text") | Some("output_text")
            ) {
                continue;
            }
            item.get("text").and_then(Value::as_str).unwrap_or("")
        };

        let text = text.trim();
        if !text.is_empty() {
            parts.push(text.to_string());
        }
    }

    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn tool_call_content(tool_name: &str, arguments: Option<&Value>) -> String {
    let mut content = format!("[Tool: {tool_name}]");
    if let Some(arguments) = arguments.and_then(argument_text) {
        content.push('\n');
        content.push_str(&arguments);
    }
    content
}

fn tool_output_content(call_id: Option<&str>, output: &str) -> String {
    let label = call_id.map_or_else(
        || "[Tool output]".to_string(),
        |id| format!("[Tool output: {id}]"),
    );
    let output = truncate_tool_output(output.trim());
    if output.is_empty() {
        label
    } else {
        format!("{label}\n{output}")
    }
}

fn argument_text(arguments: &Value) -> Option<String> {
    let text = match arguments {
        Value::String(text) => text.trim().to_string(),
        other => serde_json::to_string(other).ok()?,
    };
    (!text.is_empty()).then_some(text)
}

fn normalize_invocation_arguments(arguments: Value) -> Option<Value> {
    match arguments {
        Value::String(text) => serde_json::from_str(&text)
            .ok()
            .or_else(|| (!text.trim().is_empty()).then_some(Value::String(text))),
        Value::Null => None,
        other => Some(other),
    }
}

fn truncate_tool_output(output: &str) -> String {
    let mut truncated = String::new();
    let mut chars = output.chars();
    for _ in 0..MAX_INDEXED_TOOL_OUTPUT_CHARS {
        let Some(ch) = chars.next() else {
            return output.to_string();
        };
        truncated.push(ch);
    }
    let omitted = chars.count();
    truncated.push_str(&format!(
        "\n[truncated {omitted} additional chars from tool output]"
    ));
    truncated
}

fn message_already_indexed(
    conversation: &NormalizedConversation,
    candidate: &NormalizedMessage,
) -> bool {
    conversation.messages.iter().any(|message| {
        message.role == candidate.role
            && message.author == candidate.author
            && message.created_at == candidate.created_at
            && message.content == candidate.content
    }) || candidate
        .invocations
        .iter()
        .filter_map(|invocation| invocation.call_id.as_deref())
        .any(|call_id| {
            conversation.messages.iter().any(|message| {
                message
                    .invocations
                    .iter()
                    .any(|invocation| invocation.call_id.as_deref() == Some(call_id))
            })
        })
}
