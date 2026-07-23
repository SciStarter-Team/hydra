//! Flower, the AI concierge: a chat widget on the home page that helps
//! visitors find citizen-science projects matching their interests.
//!
//! The conversational layer runs on AWS Bedrock (Claude Haiku via the
//! Converse API); retrieval runs entirely through the existing Finder, via
//! the `search_projects` tool. Rendered project cards come only from real
//! search results held server-side, so the model cannot present a project
//! that does not exist.

pub mod budget;
pub mod convo;
pub mod engine;
pub mod tools;

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::Error;
use common::message::user::BasicInfo;
use common::timestring::TimeString;
use poem::web::{Form, Html, RealIp};
use poem::{Endpoint, EndpointExt, IntoResponse, Response, Route, handler, post};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::concierge::convo::{Conversation, TranscriptContent, TranscriptMsg, TranscriptRole};
use crate::concierge::engine::{StopKind, TurnUsage, engine};
use crate::concierge::tools::ToolCallResult;
use crate::home::HomeSettings;
use crate::middleware::authorization::Authenticated;
use crate::middleware::rate_limit::{RateLimitConfig, RateLimitMiddleware};
use crate::peering::LoadStorePeered;
use crate::search::FinderResult;

/// Maximum user-visible turns per conversation.
pub const MAX_TURNS: u32 = 10;
/// Maximum Bedrock calls per turn (tool-use round trips).
pub const MAX_TOOL_ITERATIONS: usize = 4;
/// Maximum length of a visitor message, in characters.
pub const MAX_USER_MSG_CHARS: usize = 500;
/// Output token cap per Bedrock call.
pub const MAX_OUTPUT_TOKENS: i32 = 600;
/// Project cards rendered per turn.
pub const CARDS_PER_TURN: usize = 6;
/// Search results included in the digest sent to the model.
pub const TOOL_RESULTS_TO_MODEL: usize = 8;

/// The concierge section of the home page.
#[derive(TemplateOnce)]
#[template(path = "component/concierge_section.stpl.html")]
pub struct ConciergeSection {
    pub heading: String,
    pub form: ConciergeForm,
}

/// The chat input area. Re-rendered out-of-band on every turn so the DOM
/// state (conversation id, disabled state, notices) always comes from the
/// server.
#[derive(TemplateOnce)]
#[template(path = "concierge/form.stpl.html")]
pub struct ConciergeForm {
    pub conversation_id: String,
    pub disabled: bool,
    pub notice: String,
    /// Whether this render replaces the existing form area out-of-band.
    pub oob: bool,
}

impl ConciergeForm {
    fn initial() -> ConciergeForm {
        ConciergeForm {
            conversation_id: String::new(),
            disabled: false,
            notice: String::new(),
            oob: false,
        }
    }

    fn active(conversation_id: Ulid) -> ConciergeForm {
        ConciergeForm {
            conversation_id: conversation_id.to_string(),
            disabled: false,
            notice: String::new(),
            oob: true,
        }
    }

    fn disabled(notice: &str) -> ConciergeForm {
        ConciergeForm {
            conversation_id: String::new(),
            disabled: true,
            notice: notice.to_string(),
            oob: true,
        }
    }
}

/// One turn's worth of chat log content, appended into the log by HTMX with
/// the form area replaced out-of-band.
#[derive(TemplateOnce)]
#[template(path = "concierge/turn.stpl.html")]
struct TurnFragment {
    user_text: String,
    assistant_text: String,
    cards: Vec<FinderResult>,
    finder_url: String,
    form: ConciergeForm,
}

/// Build the home-page concierge section, or `None` when the feature is
/// switched off, the daily budget is exhausted, or the viewer has opted out
/// of AI features.
pub async fn section_for(
    settings: &HomeSettings,
    user: Option<&BasicInfo>,
) -> Option<ConciergeSection> {
    if !settings.show_concierge || !budget::available() {
        return None;
    }
    if let Some(user) = user {
        if ai_opted_out(user.user_id).await {
            return None;
        }
    }
    Some(ConciergeSection {
        heading: settings.concierge_heading.clone(),
        form: ConciergeForm::initial(),
    })
}

/// Whether the user has opted out of AI features. Fails closed: if the
/// lookup errors, the user is treated as opted out.
async fn ai_opted_out(user_id: Ulid) -> bool {
    common::service::request::<bool>("user.profile.ai_opt_out_status", user_id.to_string())
        .await
        .unwrap_or(true)
}

pub fn routes() -> impl Endpoint {
    let burst = RateLimitConfig::new(6, Duration::from_secs(60))
        .with_error_message("You're sending messages very quickly. Please wait a minute.");
    let hourly = RateLimitConfig::new(40, Duration::from_secs(3600))
        .with_error_message("Hourly conversation limit reached. Please try again later.");

    Route::new().at(
        "/turn",
        post(handle_turn)
            .with(RateLimitMiddleware::new(burst))
            .with(RateLimitMiddleware::new(hourly)),
    )
}

#[derive(Debug, Deserialize)]
struct TurnInput {
    conversation_id: Option<String>,
    message: String,
}

/// Conversations with a turn currently being processed, so a double submit
/// can't run two Bedrock loops for the same conversation.
static IN_FLIGHT: LazyLock<Mutex<HashSet<Ulid>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

struct InFlightGuard(Ulid);

impl InFlightGuard {
    fn acquire(id: Ulid) -> Option<InFlightGuard> {
        IN_FLIGHT
            .lock()
            .expect("concierge in-flight lock")
            .insert(id)
            .then_some(InFlightGuard(id))
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .expect("concierge in-flight lock")
            .remove(&self.0);
    }
}

/// Render a turn fragment. Always a 200 with well-formed HTML: an error
/// status would leave the widget stuck mid-conversation.
fn fragment(fragment: TurnFragment) -> Result<Response, poem::Error> {
    let html = fragment.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok(Html(html).into_response())
}

/// A turn fragment carrying only an assistant-style notice, leaving the
/// conversation unchanged.
fn notice_fragment(
    user_text: &str,
    notice: &str,
    form: ConciergeForm,
) -> Result<Response, poem::Error> {
    fragment(TurnFragment {
        user_text: user_text.to_string(),
        assistant_text: notice.to_string(),
        cards: Vec::new(),
        finder_url: String::new(),
        form,
    })
}

#[handler]
async fn handle_turn(
    RealIp(client_ip): RealIp,
    Authenticated(user_info): Authenticated,
    Form(input): Form<TurnInput>,
) -> Result<Response, poem::Error> {
    let mut message = input.message.trim().to_string();
    if message.is_empty() {
        return notice_fragment("", "Tell me what you're curious about!", ConciergeForm::initial());
    }
    if message.chars().count() > MAX_USER_MSG_CHARS {
        message = message.chars().take(MAX_USER_MSG_CHARS).collect();
    }

    let settings = HomeSettings::current().await;
    let user = user_info.as_deref();

    // The widget may be open in a stale tab: re-check every availability
    // condition the section renderer applies.
    if !settings.show_concierge {
        return notice_fragment(
            &message,
            "The concierge isn't available right now — the project finder \
             has everything you need.",
            ConciergeForm::disabled(""),
        );
    }
    if let Some(user) = user {
        if ai_opted_out(user.user_id).await {
            return notice_fragment(
                &message,
                "AI features are switched off for your account. The project \
                 finder has everything you need.",
                ConciergeForm::disabled(""),
            );
        }
    }
    if !budget::available() {
        return notice_fragment(
            &message,
            "Flower is taking a break for the rest of the day. The project \
             finder is always open, though!",
            ConciergeForm::disabled("Come back tomorrow to chat with Flower."),
        );
    }

    // Load the conversation, or start fresh when the id is missing, expired,
    // or belongs to a different account.
    let conversation_id = input
        .conversation_id
        .as_deref()
        .and_then(|id| id.parse::<Ulid>().ok());
    let mut conversation = match conversation_id {
        Some(id) => match Conversation::load(id).await {
            Ok(existing)
                if existing.user_id == user.map(|u| u.user_id)
                    || existing.user_id.is_none() =>
            {
                existing
            }
            _ => Conversation::new(user.map(|u| u.user_id)),
        },
        None => Conversation::new(user.map(|u| u.user_id)),
    };

    if conversation.turns >= MAX_TURNS {
        return notice_fragment(
            &message,
            "We've covered a lot of ground! For anything more, the project \
             finder is the best next stop.",
            ConciergeForm::disabled("This conversation has ended — refresh the page to start a new one."),
        );
    }

    let Some(_guard) = InFlightGuard::acquire(conversation.conversation_id) else {
        return notice_fragment(
            &message,
            "One thing at a time — I'm still thinking about your last message!",
            ConciergeForm::active(conversation.conversation_id),
        );
    };

    // Per-request context goes in the first user message, keeping the system
    // prompt byte-stable for prompt caching.
    let user_content = if conversation.transcript.is_empty() {
        match visitor_context(client_ip, user).await {
            Some(context) => format!("[{}]\n\n{}", context, message),
            None => message.clone(),
        }
    } else {
        message.clone()
    };

    conversation.transcript.push(TranscriptMsg {
        role: TranscriptRole::User,
        content: vec![TranscriptContent::Text(user_content)],
    });

    let started = std::time::Instant::now();
    match run_turn(&mut conversation).await {
        Ok(outcome) => {
            conversation.turns += 1;
            conversation.tokens_in += outcome.usage.input_tokens;
            conversation.tokens_out += outcome.usage.output_tokens;
            conversation.updated = TimeString::now().to_string();

            if let Err(e) = conversation.store().await {
                tracing::error!("Failed to store concierge conversation: {}", e);
            }
            budget::record(&outcome.usage).await;

            tracing::info!(
                "Concierge turn {}/{}: {} in / {} out tokens, {} tool calls ({} empty), {} ms",
                conversation.conversation_id,
                conversation.turns,
                outcome.usage.input_tokens,
                outcome.usage.output_tokens,
                outcome.tool_calls,
                outcome.empty_searches,
                started.elapsed().as_millis(),
            );

            let extra = serde_json::json!({
                "conversation": conversation.conversation_id.to_string(),
                "turn": conversation.turns,
                "in": outcome.usage.input_tokens,
                "out": outcome.usage.output_tokens,
                "tool_calls": outcome.tool_calls,
                "empty_searches": outcome.empty_searches,
                "ms": started.elapsed().as_millis() as u64,
            });
            let user_id = conversation.user_id;
            tokio::spawn(async move {
                if let Err(e) = common::service::log_activity(
                    None,
                    "concierge_turn",
                    user_id,
                    None,
                    Some(extra.to_string()),
                )
                .await
                {
                    tracing::warn!("Failed to log concierge turn: {}", e);
                }
            });

            let (cards, finder_url) = match outcome.last_result {
                Some(result) => {
                    let mut cards = result.results;
                    cards.truncate(CARDS_PER_TURN);
                    (cards, result.finder_url)
                }
                None => (Vec::new(), String::new()),
            };

            let form = if conversation.turns >= MAX_TURNS {
                ConciergeForm::disabled(
                    "This conversation has ended — refresh the page to start a new one.",
                )
            } else {
                ConciergeForm::active(conversation.conversation_id)
            };

            fragment(TurnFragment {
                user_text: message,
                assistant_text: outcome.assistant_text,
                cards,
                finder_url,
                form,
            })
        }
        Err(e) => {
            tracing::error!("Concierge turn failed: {}", e);
            notice_fragment(
                &message,
                "Oh dear, I ran into a hiccup. Please try again in a moment — \
                 or browse the project finder while I recover.",
                ConciergeForm::active(conversation.conversation_id),
            )
        }
    }
}

/// The outcome of one turn's model/tool loop.
struct TurnOutcome {
    assistant_text: String,
    /// The last search's results — the source for rendered cards.
    last_result: Option<ToolCallResult>,
    usage: TurnUsage,
    tool_calls: u32,
    /// Searches that came back with zero results. Logged so we can measure
    /// whether keyword+facet retrieval leaves a gap worth closing with
    /// semantic search (the deferred phase 2).
    empty_searches: u32,
}

/// Run the model/tool loop for one turn.
async fn run_turn(conversation: &mut Conversation) -> Result<TurnOutcome, Error> {
    let system = tools::system_prompt().await?;
    let mut usage = TurnUsage::default();
    let mut tool_calls = 0u32;
    let mut empty_searches = 0u32;
    let mut last_result: Option<ToolCallResult> = None;
    let mut assistant_text = String::new();

    for _ in 0..MAX_TOOL_ITERATIONS {
        let outcome = engine()
            .converse(&system, &tools::TOOL_CONFIG, &conversation.transcript)
            .await?;
        usage.add(&outcome.usage);

        let tool_uses: Vec<(String, String, serde_json::Value)> = outcome
            .message
            .content
            .iter()
            .filter_map(|content| match content {
                TranscriptContent::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();

        let text: String = outcome
            .message
            .content
            .iter()
            .filter_map(|content| match content {
                TranscriptContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        conversation.transcript.push(outcome.message);

        if outcome.stop_reason != StopKind::ToolUse || tool_uses.is_empty() {
            assistant_text = text;
            break;
        }

        let mut results = Vec::new();
        for (id, name, input) in tool_uses {
            tool_calls += 1;
            let result_json = if name == "search_projects" {
                match tools::dispatch_search(&input).await {
                    Ok(result) => {
                        let json = result.json_for_model.clone();
                        if result.results.is_empty() {
                            empty_searches += 1;
                        }
                        last_result = Some(result);
                        json
                    }
                    Err(e) => {
                        tracing::warn!("Concierge search dispatch failed: {}", e);
                        serde_json::json!({
                            "error": "search failed; apologize briefly and suggest the project finder page"
                        })
                    }
                }
            } else {
                serde_json::json!({ "error": format!("unknown tool: {}", name) })
            };
            results.push(TranscriptContent::ToolResult {
                id,
                json: result_json,
            });
        }

        conversation.transcript.push(TranscriptMsg {
            role: TranscriptRole::User,
            content: results,
        });
    }

    // Iteration cap reached with the model still asking for tools: keep the
    // response well-formed by presenting whatever the last search found.
    if assistant_text.is_empty() {
        assistant_text = if last_result.is_some() {
            "Here's what I found — take a look at the projects below!".to_string()
        } else {
            "I wasn't able to finish searching just now. The project finder \
             is a great place to keep exploring!"
                .to_string()
        };
    }

    Ok(TurnOutcome {
        assistant_text,
        last_result,
        usage,
        tool_calls,
        empty_searches,
    })
}

/// Assemble the visitor-context prefix for the first user message:
/// approximate location from IP, and declared interests for logged-in users.
async fn visitor_context(
    client_ip: Option<std::net::IpAddr>,
    user: Option<&BasicInfo>,
) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(addr) = client_ip {
        let location: common::message::location::LocationInfo =
            common::service::request("location.ip", addr.to_string())
                .await
                .unwrap_or_default();
        let display = if !location.city_name.is_empty() && !location.region_name.is_empty() {
            format!("{}, {}", location.city_name, location.region_name)
        } else {
            location.city_name.clone()
        };
        if !display.is_empty() {
            parts.push(format!("approximate location: {}", display));
        }
    }

    if let Some(user) = user {
        if let Ok(tags) =
            common::service::request::<Vec<String>>("user.tags.get", user.user_id.to_string())
                .await
        {
            if !tags.is_empty() {
                parts.push(format!("interests: {}", tags.join(", ")));
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("Visitor context: {}", parts.join("; ")))
    }
}

/// Spawn the concierge's background tasks: peering listeners and the local
/// conversation-expiry loop.
pub fn spawn_background_tasks() {
    tokio::spawn(Conversation::peering_task());
    tokio::spawn(budget::Budget::peering_task());
    tokio::spawn(convo::expire_conversations());
}
