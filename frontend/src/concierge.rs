//! Flower, the AI concierge: a chat, shown either as a section of the home
//! page or as a floating panel on every standard page, that helps visitors
//! find citizen-science projects, upcoming events, and public
//! communities matching their interests, and points them at the right site
//! pages for who they are (librarians, educators, Girl Scouts, NASA fans,
//! newcomers looking for a starting place).
//!
//! The conversational layer runs on AWS Bedrock (Claude Haiku via the
//! Converse API); retrieval runs entirely through the existing Finder and
//! community list, via the `search_projects` and `list_communities` tools.
//! Rendered cards come only from real results held server-side, and site
//! links come only from the curated navigation whitelist, so the model
//! cannot present a project, community, or page that does not exist.

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
use poem::session::Session;
use poem::web::{Form, Html, RealIp};
use poem::{Endpoint, EndpointExt, IntoResponse, Response, Route, get, handler, post};
use sailfish::TemplateOnce;
use serde::Deserialize;
use ulid::Ulid;

use crate::component::html_escape;
use crate::concierge::convo::{
    Conversation, ShownTurn, TranscriptContent, TranscriptMsg, TranscriptRole,
};
use crate::concierge::engine::{StopKind, TurnUsage, engine};
use crate::concierge::tools::{ConciergeCard, ToolCallResult};
use crate::home::{ConciergePresentation, HomeSettings};
use crate::middleware::authorization::Authenticated;
use crate::middleware::rate_limit::{RateLimitConfig, RateLimitMiddleware};
use crate::peering::LoadStorePeered;

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

/// Session key holding the id of the visitor's current conversation. Keying
/// the conversation to the session rather than to the page lets it follow the
/// visitor from page to page.
const SESSION_KEY: &str = "concierge_conversation";

/// Whether the concierge may talk to a viewer, and if not, why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// Staff have switched the concierge off.
    SwitchedOff,
    /// Today's token budget is spent.
    BudgetExhausted,
    /// The viewer has opted out of AI features.
    OptedOut,
}

impl Availability {
    /// Determine availability for a viewer. The local checks run first, so
    /// the user service is asked about an opt-out only when it would matter.
    pub async fn for_viewer(settings: &HomeSettings, user: Option<&BasicInfo>) -> Availability {
        if !settings.show_concierge {
            return Availability::SwitchedOff;
        }
        if !budget::available() {
            return Availability::BudgetExhausted;
        }
        if let Some(user) = user {
            if ai_opted_out(user.user_id).await {
                return Availability::OptedOut;
            }
        }
        Availability::Available
    }

    /// Flower's reply to a message that arrives while unavailable, and the
    /// notice left on the form. `None` when available.
    fn explanation(self) -> Option<(&'static str, &'static str)> {
        match self {
            Availability::Available => None,
            Availability::SwitchedOff => Some((
                "The concierge isn't available right now — the project finder \
                 has everything you need.",
                "",
            )),
            Availability::BudgetExhausted => Some((
                "Flower is taking a break for the rest of the day. The project \
                 finder is always open, though!",
                "Come back tomorrow to chat with Flower.",
            )),
            Availability::OptedOut => Some((
                "AI features are switched off for your account. The project \
                 finder has everything you need.",
                "",
            )),
        }
    }
}

/// Whether the user has opted out of AI features. Fails closed: if the
/// lookup errors, the user is treated as opted out.
async fn ai_opted_out(user_id: Ulid) -> bool {
    common::service::request::<bool>("user.profile.ai_opt_out_status", user_id.to_string())
        .await
        .unwrap_or(true)
}

/// The visitor's current conversation: the one their session names, provided
/// it has not expired and belongs to them (or to nobody, so a visitor who
/// signs in mid-conversation keeps it).
async fn session_conversation(session: &Session, user: Option<&BasicInfo>) -> Option<Conversation> {
    let id = session.get::<Ulid>(SESSION_KEY)?;
    let existing = Conversation::load(id).await.ok()?;
    (existing.user_id.is_none() || existing.user_id == user.map(|u| u.user_id)).then_some(existing)
}

/// The concierge section of the home page: the inline presentation.
#[derive(TemplateOnce)]
#[template(path = "component/concierge_section.stpl.html")]
pub struct ConciergeSection {
    heading: String,
    chat: ConciergeChat,
}

/// Build the home-page concierge section, or `None` when the concierge is
/// presented some other way or is unavailable to this viewer.
pub async fn section_for(
    settings: &HomeSettings,
    session: &Session,
    user: Option<&BasicInfo>,
) -> Option<ConciergeSection> {
    if settings.concierge_presentation != ConciergePresentation::Inline
        || Availability::for_viewer(settings, user).await != Availability::Available
    {
        return None;
    }
    Some(ConciergeSection {
        heading: settings.concierge_heading.clone(),
        chat: ConciergeChat::for_session(session, user).await,
    })
}

/// The floating presentation: a launcher at the corner of the page and the
/// chat panel it opens. The panel's chat is fetched from `/-concierge/chat`
/// the first time it opens, so pages whose visitors never open it pay
/// nothing for it.
#[derive(TemplateOnce)]
#[template(path = "concierge/launcher.stpl.html")]
pub struct ConciergeLauncher {}

/// Build the floating launcher for a standard page, or `None` when the
/// concierge is presented some other way or is unavailable to this viewer.
pub async fn launcher_for(user: Option<&BasicInfo>) -> Option<ConciergeLauncher> {
    let settings = HomeSettings::current().await;
    (settings.concierge_presentation == ConciergePresentation::Floating
        && Availability::for_viewer(&settings, user).await == Availability::Available)
        .then_some(ConciergeLauncher {})
}

/// The chat itself — log and form — shared by both presentations.
#[derive(TemplateOnce)]
#[template(path = "concierge/chat.stpl.html")]
pub struct ConciergeChat {
    history: Vec<TurnView>,
    form: ConciergeForm,
}

impl ConciergeChat {
    /// A chat with nothing said yet.
    fn fresh() -> ConciergeChat {
        ConciergeChat {
            history: Vec::new(),
            form: ConciergeForm::open(false, false),
        }
    }

    /// The chat as the visitor left it: their session's conversation
    /// redrawn, or a fresh chat when there is none.
    async fn for_session(session: &Session, user: Option<&BasicInfo>) -> ConciergeChat {
        let Some(conversation) = session_conversation(session, user).await else {
            return ConciergeChat::fresh();
        };
        let urls = tools::linkable_urls().await;
        ConciergeChat {
            history: conversation
                .shown
                .iter()
                .map(|turn| TurnView::shown(turn, &urls))
                .collect(),
            form: ConciergeForm::for_conversation(&conversation, false),
        }
    }

    /// The chat for a viewer the concierge is not available to: Flower's
    /// explanation, and a form that offers the project finder instead.
    fn unavailable(reply: &str, form_notice: &str) -> ConciergeChat {
        ConciergeChat {
            history: vec![TurnView::notice("", reply)],
            form: ConciergeForm::unavailable(form_notice, false),
        }
    }

    /// The chat for a viewer as things stand: redrawn from their session
    /// when available, otherwise the explanation of why not.
    async fn for_viewer(session: &Session, user: Option<&BasicInfo>) -> ConciergeChat {
        let settings = HomeSettings::current().await;
        match Availability::for_viewer(&settings, user)
            .await
            .explanation()
        {
            None => ConciergeChat::for_session(session, user).await,
            Some((reply, form_notice)) => ConciergeChat::unavailable(reply, form_notice),
        }
    }
}

/// What the form offers.
enum FormState {
    /// Accepting messages. `started` once the conversation has a completed
    /// turn, when the form also offers to start over.
    Open { started: bool },
    /// The conversation reached [`MAX_TURNS`]; the form offers to start over.
    Ended,
    /// The concierge is unavailable to this viewer; the form offers the
    /// project finder instead.
    Unavailable { notice: String },
}

/// The chat input area. Re-rendered out-of-band on every turn so the DOM
/// state (open, ended or unavailable, and any notice) always comes from the
/// server.
#[derive(TemplateOnce)]
#[template(path = "concierge/form.stpl.html")]
pub struct ConciergeForm {
    state: FormState,
    /// Whether this render replaces the existing form area out-of-band.
    oob: bool,
}

impl ConciergeForm {
    fn open(started: bool, oob: bool) -> ConciergeForm {
        ConciergeForm {
            state: FormState::Open { started },
            oob,
        }
    }

    fn unavailable(notice: &str, oob: bool) -> ConciergeForm {
        ConciergeForm {
            state: FormState::Unavailable {
                notice: notice.to_string(),
            },
            oob,
        }
    }

    /// The form as it stands after the conversation's latest turn.
    fn for_conversation(conversation: &Conversation, oob: bool) -> ConciergeForm {
        let state = if conversation.turns >= MAX_TURNS {
            FormState::Ended
        } else {
            FormState::Open {
                started: conversation.turns > 0,
            }
        };
        ConciergeForm { state, oob }
    }
}

/// One exchange in the chat log: the visitor's message, Flower's reply, and
/// any cards shown with it. Used both for a turn as it happens and for
/// earlier turns redrawn from the conversation.
#[derive(TemplateOnce)]
#[template(path = "concierge/turn.stpl.html")]
struct TurnView {
    /// Empty for a reply that answers no visible message.
    user_text: String,
    /// Sanitized HTML for the assistant reply, produced by
    /// [`tools::render_assistant_html`] (markdown interpreted, raw HTML
    /// escaped, links restricted to the destination whitelist) or by
    /// escaping a plain notice. Rendered raw.
    assistant_html: String,
    cards: Vec<ConciergeCard>,
    more_url: String,
    more_label: String,
}

impl TurnView {
    fn shown(turn: &ShownTurn, urls: &[&'static str]) -> TurnView {
        TurnView {
            user_text: turn.user_text.clone(),
            assistant_html: tools::render_assistant_html(&turn.assistant_text, urls),
            cards: turn.cards.clone(),
            more_url: turn.more_url.clone(),
            more_label: turn.more_label.clone(),
        }
    }

    /// A reply carrying only a notice, which is not part of the
    /// conversation.
    fn notice(user_text: &str, notice: &str) -> TurnView {
        TurnView {
            user_text: user_text.to_string(),
            assistant_html: html_escape(notice),
            cards: Vec::new(),
            more_url: String::new(),
            more_label: String::new(),
        }
    }
}

/// The response to a turn: the exchange, appended to the log by HTMX, and
/// the form area, replaced out-of-band.
#[derive(TemplateOnce)]
#[template(path = "concierge/turn_response.stpl.html")]
struct TurnResponse {
    turn: TurnView,
    form: ConciergeForm,
}

pub fn routes() -> impl Endpoint {
    let burst = RateLimitConfig::new(6, Duration::from_secs(60))
        .with_error_message("You're sending messages very quickly. Please wait a minute.");
    let hourly = RateLimitConfig::new(40, Duration::from_secs(3600))
        .with_error_message("Hourly conversation limit reached. Please try again later.");

    Route::new()
        .at(
            "/turn",
            post(handle_turn)
                .with(RateLimitMiddleware::new(burst))
                .with(RateLimitMiddleware::new(hourly)),
        )
        .at("/chat", get(handle_chat))
        .at("/restart", post(handle_restart))
}

#[derive(Debug, Deserialize)]
struct TurnInput {
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

/// Render a template as an HTML response. Always a 200 with well-formed
/// HTML: an error status would leave the widget stuck mid-conversation.
fn html_response(template: impl TemplateOnce) -> Result<Response, poem::Error> {
    let html = template.render_once().map_err(|e| {
        poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok(Html(html).into_response())
}

/// A turn response carrying only a notice, leaving the conversation
/// unchanged.
fn notice_response(
    user_text: &str,
    notice: &str,
    form: ConciergeForm,
) -> Result<Response, poem::Error> {
    html_response(TurnResponse {
        turn: TurnView::notice(user_text, notice),
        form,
    })
}

/// The chat for the floating panel, fetched when the panel first opens on a
/// page.
#[handler]
async fn handle_chat(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<Response, poem::Error> {
    html_response(ConciergeChat::for_viewer(session, user_info.as_deref()).await)
}

/// Leave the current conversation and start a fresh chat. The abandoned
/// conversation expires with the rest.
#[handler]
async fn handle_restart(
    session: &Session,
    Authenticated(user_info): Authenticated,
) -> Result<Response, poem::Error> {
    session.remove(SESSION_KEY);
    html_response(ConciergeChat::for_viewer(session, user_info.as_deref()).await)
}

#[handler]
async fn handle_turn(
    RealIp(client_ip): RealIp,
    session: &Session,
    Authenticated(user_info): Authenticated,
    Form(input): Form<TurnInput>,
) -> Result<Response, poem::Error> {
    let user = user_info.as_deref();

    let mut message = input.message.trim().to_string();
    if message.is_empty() {
        let form = match session_conversation(session, user).await {
            Some(conversation) => ConciergeForm::for_conversation(&conversation, true),
            None => ConciergeForm::open(false, true),
        };
        return notice_response("", "Tell me what you're curious about!", form);
    }
    if message.chars().count() > MAX_USER_MSG_CHARS {
        message = message.chars().take(MAX_USER_MSG_CHARS).collect();
    }

    // The chat may be open in a stale tab: re-check availability.
    let settings = HomeSettings::current().await;
    if let Some((reply, form_notice)) = Availability::for_viewer(&settings, user)
        .await
        .explanation()
    {
        return notice_response(
            &message,
            reply,
            ConciergeForm::unavailable(form_notice, true),
        );
    }

    // Continue the session's conversation, or start fresh when there is
    // none, it has expired, or it belongs to a different account.
    let mut conversation = match session_conversation(session, user).await {
        Some(existing) => existing,
        None => Conversation::new(user.map(|u| u.user_id)),
    };

    if conversation.turns >= MAX_TURNS {
        return notice_response(
            &message,
            "We've covered a lot of ground! For anything more, the project \
             finder is the best next stop.",
            ConciergeForm::for_conversation(&conversation, true),
        );
    }

    let Some(_guard) = InFlightGuard::acquire(conversation.conversation_id) else {
        return notice_response(
            &message,
            "One thing at a time — I'm still thinking about your last message!",
            ConciergeForm::for_conversation(&conversation, true),
        );
    };

    // Per-request context goes in the first user message, keeping the system
    // prompt free of per-user data.
    let user_content = if conversation.transcript.is_empty() {
        format!(
            "[{}]\n\n{}",
            visitor_context(client_ip, user).await,
            message
        )
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
            let (cards, more_url, more_label) = match outcome.last_result {
                Some(result) => {
                    let mut cards = result.cards;
                    cards.truncate(CARDS_PER_TURN);
                    (cards, result.more_url, result.more_label)
                }
                None => (Vec::new(), String::new(), String::new()),
            };
            let shown = ShownTurn {
                user_text: message,
                assistant_text: outcome.assistant_text,
                cards,
                more_url,
                more_label,
            };

            conversation.turns += 1;
            conversation.tokens_in += outcome.usage.input_tokens;
            conversation.tokens_out += outcome.usage.output_tokens;
            conversation.updated = TimeString::now().to_string();
            conversation.shown.push(shown.clone());

            if let Err(e) = conversation.store().await {
                tracing::error!("Failed to store concierge conversation: {}", e);
            }
            session.set(SESSION_KEY, conversation.conversation_id);
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

            let urls = tools::linkable_urls().await;
            html_response(TurnResponse {
                turn: TurnView::shown(&shown, &urls),
                form: ConciergeForm::for_conversation(&conversation, true),
            })
        }
        Err(e) => {
            tracing::error!("Concierge turn failed: {}", e);
            notice_response(
                &message,
                "Oh dear, I ran into a hiccup. Please try again in a moment — \
                 or browse the project finder while I recover.",
                ConciergeForm::for_conversation(&conversation, true),
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
            let result_json = match name.as_str() {
                "search_projects" => match tools::dispatch_search(&input).await {
                    Ok(result) => {
                        let json = result.json_for_model.clone();
                        if result.cards.is_empty() {
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
                },
                "list_communities" => match tools::dispatch_communities(&input).await {
                    Ok(result) => {
                        let json = result.json_for_model.clone();
                        last_result = Some(result);
                        json
                    }
                    Err(e) => {
                        tracing::warn!("Concierge community dispatch failed: {}", e);
                        serde_json::json!({
                            "error": "community list unavailable; apologize briefly and suggest the communities page"
                        })
                    }
                },
                _ => serde_json::json!({ "error": format!("unknown tool: {}", name) }),
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

/// Assemble the visitor-context prefix for the first user message: account
/// status (including Foundations-training completion, which steers the
/// "where do I start?" guidance), approximate location from IP, and declared
/// interests for logged-in users.
async fn visitor_context(client_ip: Option<std::net::IpAddr>, user: Option<&BasicInfo>) -> String {
    let mut parts = Vec::new();

    match user {
        Some(user) => {
            // fetch_status returns an empty default on error, which reads as
            // not-completed — the safe direction, since it only means
            // Foundations gets suggested.
            let status = crate::training::fetch_status(
                common::message::user::WorkflowType::Foundations,
                &user.user_id.to_string(),
            )
            .await;
            parts.push(format!(
                "logged in; {} the Foundations training",
                if status.badge_granted {
                    "has completed"
                } else {
                    "has not completed"
                }
            ));
        }
        None => parts.push("not logged in".to_string()),
    }

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
            common::service::request::<Vec<String>>("user.tags.get", user.user_id.to_string()).await
        {
            if !tags.is_empty() {
                parts.push(format!("interests: {}", tags.join(", ")));
            }
        }
    }

    format!("Visitor context: {}", parts.join("; "))
}

/// Spawn the concierge's background tasks: peering listeners and the local
/// conversation-expiry loop.
pub fn spawn_background_tasks() {
    tokio::spawn(Conversation::peering_task());
    tokio::spawn(budget::Budget::peering_task());
    tokio::spawn(convo::expire_conversations());
}
