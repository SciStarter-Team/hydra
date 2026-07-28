//! Bedrock Converse client for the concierge, plus a zero-cost stub engine
//! for development and testing.

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Error, anyhow};
use aws_sdk_bedrockruntime::{
    Client,
    config::{Credentials, Region},
    types::{
        ContentBlock, ConversationRole, InferenceConfiguration, Message, StopReason,
        SystemContentBlock, ToolConfiguration, ToolResultBlock, ToolResultContentBlock,
        ToolUseBlock,
    },
};
use aws_smithy_types::{Document, Number};

use crate::concierge::MAX_OUTPUT_TOKENS;
use crate::concierge::convo::{TranscriptContent, TranscriptMsg, TranscriptRole};

/// Wall-clock limit for a single Bedrock round trip.
const CONVERSE_TIMEOUT: Duration = Duration::from_secs(25);

/// Token usage for one Bedrock call.
#[derive(Debug, Default, Clone, Copy)]
pub struct TurnUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl TurnUsage {
    pub fn add(&mut self, other: &TurnUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopKind {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other,
}

/// The result of one model call: the assistant message, why generation
/// stopped, and what it cost.
#[derive(Debug)]
pub struct ConverseOutcome {
    pub message: TranscriptMsg,
    pub stop_reason: StopKind,
    pub usage: TurnUsage,
}

pub enum Engine {
    Bedrock(Client),
    Stub,
}

/// Debug builds use the zero-cost stub engine (mirroring the localhost NATS
/// default in `common::shared`); release builds always talk to Bedrock.
static ENGINE: LazyLock<Engine> = LazyLock::new(|| {
    if cfg!(debug_assertions) {
        Engine::Stub
    } else {
        let region = Region::new(env!("AWS_REGION"));
        let credentials = Credentials::new(
            env!("AWS_ACCESS_KEY_ID"),
            env!("AWS_SECRET_ACCESS_KEY"),
            None,
            None,
            "env",
        );
        let config = aws_sdk_bedrockruntime::Config::builder()
            .region(region)
            .credentials_provider(credentials)
            .build();
        Engine::Bedrock(Client::from_conf(config))
    }
});

pub fn engine() -> &'static Engine {
    &ENGINE
}

impl Engine {
    /// Run one model call over the transcript, returning the assistant's
    /// next message.
    pub async fn converse(
        &self,
        system: &str,
        tools: &ToolConfiguration,
        transcript: &[TranscriptMsg],
    ) -> Result<ConverseOutcome, Error> {
        match self {
            Engine::Stub => Ok(converse_stub(transcript)),
            Engine::Bedrock(client) => tokio::time::timeout(
                CONVERSE_TIMEOUT,
                converse_bedrock(client, system, tools, transcript),
            )
            .await
            .map_err(|_| anyhow!("Bedrock converse call timed out"))?,
        }
    }
}

async fn converse_bedrock(
    client: &Client,
    system: &str,
    tools: &ToolConfiguration,
    transcript: &[TranscriptMsg],
) -> Result<ConverseOutcome, Error> {
    let messages = transcript
        .iter()
        .map(message_to_sdk)
        .collect::<Result<Vec<_>, Error>>()?;

    let mut request = client
        .converse()
        .model_id(env!("BEDROCK_MODEL_ID"))
        .system(SystemContentBlock::Text(system.to_string()))
        .tool_config(tools.clone())
        .inference_config(
            InferenceConfiguration::builder()
                .max_tokens(MAX_OUTPUT_TOKENS)
                .build(),
        );
    for message in messages {
        request = request.messages(message);
    }

    let output = request.send().await?;

    let stop_reason = match output.stop_reason() {
        StopReason::EndTurn => StopKind::EndTurn,
        StopReason::ToolUse => StopKind::ToolUse,
        StopReason::MaxTokens => StopKind::MaxTokens,
        _ => StopKind::Other,
    };

    let usage = output
        .usage()
        .map(|u| TurnUsage {
            input_tokens: u.input_tokens() as i64,
            output_tokens: u.output_tokens() as i64,
        })
        .unwrap_or_default();

    let message = output
        .output()
        .and_then(|o| o.as_message().ok())
        .map(message_from_sdk)
        .transpose()?
        .ok_or_else(|| anyhow!("Bedrock converse response contained no message"))?;

    Ok(ConverseOutcome {
        message,
        stop_reason,
        usage,
    })
}

/// Convert a transcript message into the SDK's message type.
fn message_to_sdk(msg: &TranscriptMsg) -> Result<Message, Error> {
    let role = match msg.role {
        TranscriptRole::User => ConversationRole::User,
        TranscriptRole::Assistant => ConversationRole::Assistant,
    };

    let mut builder = Message::builder().role(role);
    for content in &msg.content {
        let block = match content {
            TranscriptContent::Text(text) => ContentBlock::Text(text.clone()),
            TranscriptContent::ToolUse { id, name, input } => ContentBlock::ToolUse(
                ToolUseBlock::builder()
                    .tool_use_id(id)
                    .name(name)
                    .input(json_to_document(input))
                    .build()?,
            ),
            TranscriptContent::ToolResult { id, json } => ContentBlock::ToolResult(
                ToolResultBlock::builder()
                    .tool_use_id(id)
                    .content(ToolResultContentBlock::Json(json_to_document(json)))
                    .build()?,
            ),
        };
        builder = builder.content(block);
    }
    Ok(builder.build()?)
}

/// Convert an SDK response message back into transcript form.
fn message_from_sdk(msg: &Message) -> Result<TranscriptMsg, Error> {
    let role = match msg.role() {
        ConversationRole::Assistant => TranscriptRole::Assistant,
        _ => TranscriptRole::User,
    };

    let mut content = Vec::new();
    for block in msg.content() {
        match block {
            ContentBlock::Text(text) => content.push(TranscriptContent::Text(text.clone())),
            ContentBlock::ToolUse(tool_use) => content.push(TranscriptContent::ToolUse {
                id: tool_use.tool_use_id().to_string(),
                name: tool_use.name().to_string(),
                input: document_to_json(tool_use.input()),
            }),
            _ => {}
        }
    }

    Ok(TranscriptMsg { role, content })
}

/// Convert a serde_json value into a Smithy document.
pub fn json_to_document(value: &serde_json::Value) -> Document {
    match value {
        serde_json::Value::Null => Document::Null,
        serde_json::Value::Bool(b) => Document::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Document::Number(Number::PosInt(u))
            } else if let Some(i) = n.as_i64() {
                Document::Number(Number::NegInt(i))
            } else {
                Document::Number(Number::Float(n.as_f64().unwrap_or(0.0)))
            }
        }
        serde_json::Value::String(s) => Document::String(s.clone()),
        serde_json::Value::Array(items) => {
            Document::Array(items.iter().map(json_to_document).collect())
        }
        serde_json::Value::Object(map) => Document::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), json_to_document(v)))
                .collect(),
        ),
    }
}

/// Convert a Smithy document into a serde_json value.
pub fn document_to_json(doc: &Document) -> serde_json::Value {
    match doc {
        Document::Null => serde_json::Value::Null,
        Document::Bool(b) => serde_json::Value::Bool(*b),
        Document::Number(n) => match n {
            Number::PosInt(u) => serde_json::Value::from(*u),
            Number::NegInt(i) => serde_json::Value::from(*i),
            Number::Float(f) => serde_json::Value::from(*f),
        },
        Document::String(s) => serde_json::Value::String(s.clone()),
        Document::Array(items) => {
            serde_json::Value::Array(items.iter().map(document_to_json).collect())
        }
        Document::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), document_to_json(v)))
                .collect(),
        ),
    }
}

/// Deterministic canned engine: first call requests a `search_projects` tool
/// use derived from the visitor's message, the follow-up call produces a
/// short text reply. Exercises the full turn loop — including the real
/// search path and card rendering — without any AWS traffic.
fn converse_stub(transcript: &[TranscriptMsg]) -> ConverseOutcome {
    let last = transcript.last();

    let awaiting_tool_result = last.is_some_and(|msg| {
        msg.content
            .iter()
            .any(|c| matches!(c, TranscriptContent::ToolResult { .. }))
    });

    if awaiting_tool_result {
        ConverseOutcome {
            message: TranscriptMsg {
                role: TranscriptRole::Assistant,
                content: vec![TranscriptContent::Text(
                    "Here are some projects that look like a **good match** — take a \
                     look at the cards below. Want me to narrow things down further? \
                     There's always more to sniff out at /finder. [stub engine reply]"
                        .to_string(),
                )],
            },
            stop_reason: StopKind::EndTurn,
            usage: TurnUsage::default(),
        }
    } else {
        let text = last
            .map(|msg| {
                msg.content
                    .iter()
                    .filter_map(|c| match c {
                        TranscriptContent::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        // The first user message carries a bracketed visitor-context prefix
        // on its own lines; a real model composes keywords itself, so the
        // stub approximates that by searching only the message's last line.
        let keywords = text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");

        // A message mentioning communities exercises the list_communities
        // path; one mentioning events exercises the events-only search;
        // anything else exercises the plain search_projects path.
        let lowered = text.to_lowercase();
        let tool_use = if lowered.contains("communit") {
            TranscriptContent::ToolUse {
                id: format!("stub-{}", ulid::Ulid::new()),
                name: "list_communities".to_string(),
                input: serde_json::json!({}),
            }
        } else if lowered.contains("event") {
            // No keywords: browse all upcoming events, so the events-only
            // restriction is exercised deterministically.
            TranscriptContent::ToolUse {
                id: format!("stub-{}", ulid::Ulid::new()),
                name: "search_projects".to_string(),
                input: serde_json::json!({ "what": "events" }),
            }
        } else {
            TranscriptContent::ToolUse {
                id: format!("stub-{}", ulid::Ulid::new()),
                name: "search_projects".to_string(),
                input: serde_json::json!({ "keywords": keywords }),
            }
        };

        ConverseOutcome {
            message: TranscriptMsg {
                role: TranscriptRole::Assistant,
                content: vec![tool_use],
            },
            stop_reason: StopKind::ToolUse,
            usage: TurnUsage::default(),
        }
    }
}
