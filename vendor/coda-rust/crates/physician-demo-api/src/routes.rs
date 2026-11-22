//! Axum HTTP and SSE route handlers for the embeddable physician demo API.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::{ConnectInfo, Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::stream;
use physician_bus::Event as BusEvent;
use physician_demo_db::store::{DEFAULT_DEMO_NOW, next_demo_seq};
use physician_demo_db::{DbError, DraftSaveOutcome};
use physician_wire::briefings::PanelPreferencesWire;
use physician_wire::follow_ups::{
    CreateFollowUpBody, FollowUpActionCommandBody, ResolveCannotRestartBody, SettleFollowUpBody,
    StopFollowUpBody, UpdateFollowUpBody,
};
use physician_wire::matching_reply::{
    ChooseCodaReplyUpdateBody, ConfirmMessageLinkBody, CopyReplyBody, DisconnectedReplyDoneBody,
    MatchingActionCommandBody, MoveSpruceConversationBody, NoReplyNeededBody, ReplyCommandKeyBody,
    RequestReplyProposalRevisionBody, RestoreMyReplyVersionBody, SaveReplyProposalDraftBody,
};
use physician_wire::source_setup::{
    ConnectSourceBody, RequestAssistedSetupBody, SimulateSourceEventBody, SupportedSourceWire,
};
use physician_wire::today_updates::{MarkTodayDoneBody, OpenTodayUpdateItemBody};
use physician_wire::{
    AnswerState, ChatSnapshot, DemoSessionResponse, DictationRequest, ExchangeSteps,
    InviteColleagueRequest, InvitePatientRequest, QueuedMessageWire, ResponseId,
    ReviewControlRequest, SaveFeedback, SavePhysicianMemoryEditRequest, SavedAt, SnapshotFeedback,
    WorkItemId, WorkResumeTarget, WorkStopTarget,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::DemoApiState;
use crate::budget::{DemoBudgetExhausted, demo_budget_exhausted_response};
use crate::harness_product::{
    create_working_exchange, default_demo_timestamp, extract_prior_history, fallback_title,
    run_reply_revision_with_provider,
};
use crate::sessions::VisitorSession;

/// Builds the complete embeddable `axum::Router` for [`DemoApiState`].
pub fn build_router(state: DemoApiState) -> Router {
    let app = Router::new()
        .route("/health", get(health))
        .route("/auth/demo-session", post(start_demo_session))
        .route("/auth/whoami", get(whoami))
        .route("/auth/sign-out", post(sign_out))
        .route(
            "/auth/panels/{physician_id}/{workspace_id}",
            put(assign_panel).delete(remove_panel),
        )
        .route("/auth/invitations", post(invite_patient))
        .route("/auth/invitations/resend", post(resend_patient_invitation))
        .route("/auth/invitations/cancel", post(cancel_patient_invitation))
        .route("/auth/colleagues/invitations", post(invite_colleague))
        .route("/patient-workspaces/events", get(events))
        .route("/today", get(today))
        .route("/today-updates/open", post(open_today_update_item))
        .route("/today-updates/done", post(mark_today_item_done))
        .route("/practice-day", get(practice_day))
        .route("/practice-day/refresh", post(refresh_practice_day))
        .route("/updates", get(updates))
        .route("/recent-conversations", get(recent_conversations))
        .route("/patient-roster", get(patient_roster))
        .route("/patient-workspaces", get(workspaces))
        .route("/patient-workspaces/{workspace_id}", get(workspace))
        .route(
            "/patient-workspaces/{workspace_id}/past-work",
            get(past_work),
        )
        .route(
            "/patient-workspaces/{workspace_id}/chats/{chat_id}/snapshot",
            get(chat_snapshot),
        )
        .route(
            "/patient-workspaces/{workspace_id}/chats/{chat_id}/sources/{source_key}",
            get(cited_source),
        )
        .route(
            "/physician-state/drafts/{workspace_id}/{chat_id}",
            get(read_draft).put(save_draft),
        )
        .route(
            "/physician-state/feedback/{workspace_id}/{chat_id}/{response_id}",
            put(save_feedback),
        )
        .route(
            "/physician-state/reply-drafts/{workspace_id}/{reply_work_id}",
            get(read_reply_draft),
        )
        .route(
            "/physician-state/panels",
            get(panel_preferences).put(save_panel_preferences),
        )
        .route(
            "/patient-workspaces/{workspace_id}/memory",
            get(patient_memory).post(save_patient_memory),
        )
        .route(
            "/patient-workspaces/{workspace_id}/briefings",
            get(workspace_briefings),
        )
        .route(
            "/patient-workspaces/{workspace_id}/briefings/{briefing_id}",
            get(visit_briefing),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups",
            get(patient_follow_ups).post(create_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}",
            put(update_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/review-now",
            post(review_follow_up_now),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/stop",
            post(stop_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/stop-and-discuss",
            post(stop_and_discuss_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/resume",
            post(resume_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/complete",
            post(complete_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/cancel",
            post(cancel_follow_up),
        )
        .route(
            "/patient-workspaces/{workspace_id}/follow-ups/{follow_up_id}/resolve-cannot-restart",
            post(resolve_cannot_restart_follow_up),
        )
        .route("/connections", get(connections))
        .route(
            "/connections/spruce/accept-backlog",
            post(accept_spruce_backlog),
        )
        .route(
            "/connections/spruce/rollback/resolve",
            post(resolve_spruce_rollback),
        )
        .route(
            "/connections/{source}/connect",
            post(connect_source),
        )
        .route(
            "/connections/{source}/assist",
            post(request_assisted_setup),
        )
        .route(
            "/connections/{source}/import",
            post(import_source_records),
        )
        .route(
            "/connections/{source}/refresh",
            post(refresh_source),
        )
        .route(
            "/connections/{source}/reconnect",
            post(reconnect_source),
        )
        .route(
            "/connections/{source}/disconnect",
            post(disconnect_source),
        )
        .route(
            "/connections/{source}/simulate",
            post(simulate_source_event),
        )
        .route("/spruce-matching/messages", get(messages_to_match))
        .route("/spruce-matching/filtered", get(filtered_spruce_messages))
        .route(
            "/spruce-matching/messages/{message_id}/confirm",
            post(confirm_spruce_message_link),
        )
        .route(
            "/spruce-matching/messages/{message_id}/leave-unlinked",
            post(leave_spruce_message_unlinked),
        )
        .route(
            "/spruce-matching/filtered/{message_id}/this-is-a-patient",
            post(mark_spruce_message_as_patient),
        )
        .route(
            "/spruce-matching/unreadable/{message_id}/done",
            post(mark_unreadable_spruce_message_done),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies",
            get(patient_spruce_replies),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}",
            get(patient_spruce_reply),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/draft",
            put(save_reply_proposal_draft),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/revise",
            post(request_reply_proposal_revision),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/restore-my-version",
            post(restore_my_reply_version),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/choose-update",
            post(choose_coda_reply_update),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/copy",
            post(copy_spruce_reply),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/no-reply-needed",
            post(close_reply_no_reply_needed),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/undo-no-reply-needed",
            post(undo_close_no_reply_needed),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/ask-coda",
            post(ask_coda_to_reply),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/move",
            post(move_spruce_conversation),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/undo-move",
            post(undo_spruce_conversation_move),
        )
        .route(
            "/patient-workspaces/{workspace_id}/spruce-replies/{conversation_id}/disconnected-done",
            post(mark_disconnected_reply_done),
        )
        .route(
            "/patient-workspaces/{workspace_id}/reviews",
            get(patient_reviews),
        )
        .route(
            "/patient-workspaces/{workspace_id}/reviews/{review_id}",
            get(patient_review),
        )
        .route(
            "/patient-workspaces/{workspace_id}/reviews/{review_id}/control",
            post(control_patient_review),
        )
        .route("/dictation", post(dictate))
        .route("/commands", post(command))
        .with_state(state.clone());

    if state.allow_cors {
        app.layer(axum::middleware::from_fn(cors_middleware))
    } else {
        app
    }
}

async fn cors_middleware(req: Request<axum::body::Body>, next: Next) -> Response {
    if req.method() == Method::OPTIONS {
        let mut res = StatusCode::NO_CONTENT.into_response();
        apply_cors_headers(res.headers_mut());
        return res;
    }
    let mut res = next.run(req).await;
    apply_cors_headers(res.headers_mut());
    res
}

fn apply_cors_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type, last-event-id, cache-control"),
    );
}

#[derive(Debug)]
enum ApiError {
    Unauthorized(&'static str),
    Forbidden(&'static str),
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Budget(DemoBudgetExhausted),
    ServiceUnavailable(String),
    Internal(String),
}

impl From<DbError> for ApiError {
    fn from(err: DbError) -> Self {
        match err {
            DbError::NotFound { kind, id } => Self::NotFound(format!("{kind} `{id}` not found")),
            DbError::InvalidCommand(msg) => Self::BadRequest(msg),
            DbError::Conflict(msg) => Self::Conflict(msg),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg).into_response(),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg).into_response(),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
            Self::Conflict(msg) => (StatusCode::CONFLICT, msg).into_response(),
            Self::Budget(exhausted) => demo_budget_exhausted_response(exhausted),
            Self::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg).into_response(),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    raw.strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn require_session(state: &DemoApiState, headers: &HeaderMap) -> Result<VisitorSession, ApiError> {
    let token =
        bearer_token(headers).ok_or(ApiError::Unauthorized("missing demo session bearer token"))?;
    state
        .sessions
        .authenticate(token)
        .ok_or(ApiError::Unauthorized("invalid or expired demo session"))
}

fn check_workspace(session: &VisitorSession, workspace_id: &str) -> Result<(), ApiError> {
    if session.allows_workspace(workspace_id) {
        Ok(())
    } else {
        Err(ApiError::Forbidden("workspace not in demo session panel"))
    }
}

fn no_store(mut res: Response) -> Response {
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

async fn health(State(state): State<DemoApiState>) -> Response {
    let mode = state.provider_backend.mode_label();
    let model = state.provider_backend.model_name();
    no_store(
        Json(json!({
            "status": "ok",
            "mode": "demo",
            "provider_mode": mode,
            "model": model,
            "retrying": [],
            "held_by_newer_release": [],
            "capabilities": {
                "connection_accounts": false,
                "practice_sources": true
            }
        }))
        .into_response(),
    )
}

async fn start_demo_session(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    peer_addr: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Result<Response, ApiError> {
    let peer = peer_addr.map(|Extension(ConnectInfo(addr))| addr);
    state
        .budget
        .check_and_record_session_mint(&headers, peer)
        .map_err(ApiError::Budget)?;
    let session = state.sessions.mint()?;
    let body = DemoSessionResponse {
        session: session.token,
        physician_id: session.physician_id,
        practice_id: session.practice_id,
        ends_at: session.ends_at,
    };
    Ok(no_store((StatusCode::OK, Json(body)).into_response()))
}

async fn whoami(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    Ok(no_store(Json(session.whoami()).into_response()))
}

async fn sign_out(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = bearer_token(&headers) {
        state.sessions.revoke(token);
    }
    Ok(no_store(Json(json!({})).into_response()))
}

async fn events(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let _session = require_session(&state, &headers)?;
    let stream = stream::unfold(state.bus.subscribe(), |mut rx| async move {
        let change = rx.recv().await.ok()?;
        let ev = match change {
            BusEvent::WorkspaceChanged { workspace_id }
            | BusEvent::WorkspaceSettled { workspace_id } => SseEvent::default()
                .event("workspace_changed")
                .data(json!({ "patient_workspace_id": workspace_id }).to_string()),
            BusEvent::Resync => SseEvent::default().event("resync").data("{}"),
        };
        Some((Ok(ev), rx))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Deserialize)]
struct TodayQuery {
    patient_workspace_id: Option<String>,
}

async fn today(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Query(query): Query<TodayQuery>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    let resp = match query.patient_workspace_id.as_deref() {
        Some(ws_id) => {
            check_workspace(&session, ws_id)?;
            session.store.today_for(ws_id).await?
        }
        None => session.store.today().await?,
    };
    Ok(no_store(Json(resp).into_response()))
}

async fn open_today_update_item(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<OpenTodayUpdateItemBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &body.patient_workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .open_today_update_item(
            &body.patient_workspace_id,
            &body.item_id,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn mark_today_item_done(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<MarkTodayDoneBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &body.patient_workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .mark_today_item_done(
            &body.patient_workspace_id,
            &body.item_id,
            &body.version_id,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn practice_day(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.practice_day().await?).into_response(),
    ))
}

async fn refresh_practice_day(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.refresh_practice_day().await?).into_response(),
    ))
}

async fn updates(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.updates().await?).into_response(),
    ))
}

async fn recent_conversations(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.recent_conversations().await?).into_response(),
    ))
}

#[derive(Deserialize)]
struct RosterQuery {
    #[serde(default)]
    query: String,
}

async fn patient_roster(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Query(query): Query<RosterQuery>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.search(&query.query).await?).into_response(),
    ))
}

async fn workspaces(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.workspaces().await?).into_response(),
    ))
}

async fn workspace(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.workspace(&workspace_id).await?).into_response(),
    ))
}

async fn past_work(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.past_work(&workspace_id).await?).into_response(),
    ))
}

async fn chat_snapshot(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, chat_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.chat_snapshot(&workspace_id, &chat_id).await?).into_response(),
    ))
}

async fn cited_source(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, chat_id, source_key)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .cited_source(&workspace_id, &chat_id, &source_key)
                .await?,
        )
        .into_response(),
    ))
}

async fn read_draft(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, chat_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.read_draft(&workspace_id, &chat_id).await?).into_response(),
    ))
}

#[derive(Deserialize)]
struct SaveDraftBody {
    expected_revision: i64,
    text: String,
}

async fn save_draft(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, chat_id)): Path<(String, String)>,
    Json(body): Json<SaveDraftBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    match session
        .store
        .save_draft(&workspace_id, &chat_id, body.expected_revision, &body.text)
        .await?
    {
        DraftSaveOutcome::Saved(saved) => Ok(no_store(Json(saved).into_response())),
        DraftSaveOutcome::Conflict(current) => Ok(no_store(
            (
                StatusCode::CONFLICT,
                Json(json!({ "outcome": "conflict", "current": current })),
            )
                .into_response(),
        )),
    }
}

async fn save_feedback(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, chat_id, response_id)): Path<(String, String, String)>,
    Json(body): Json<SaveFeedback>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .save_feedback(&workspace_id, &chat_id, &ResponseId(response_id), &body)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn read_reply_draft(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, reply_work_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .read_reply_draft(&workspace_id, &reply_work_id)
                .await?,
        )
        .into_response(),
    ))
}

async fn panel_preferences(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let prefs = *session
        .panel_preferences
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(no_store(Json(prefs).into_response()))
}

async fn save_panel_preferences(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(prefs): Json<PanelPreferencesWire>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    *session
        .panel_preferences
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = prefs;
    Ok(no_store(Json(prefs).into_response()))
}

async fn patient_memory(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.patient_memory(&workspace_id).await?).into_response(),
    ))
}

async fn save_patient_memory(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<SavePhysicianMemoryEditRequest>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .save_patient_memory_edit(&workspace_id, &body)
                .await?,
        )
        .into_response(),
    ))
}

async fn workspace_briefings(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.workspace_briefings(&workspace_id).await?).into_response(),
    ))
}

async fn visit_briefing(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, briefing_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .visit_briefing(&workspace_id, &briefing_id)
                .await?,
        )
        .into_response(),
    ))
}

async fn patient_follow_ups(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.patient_follow_ups(&workspace_id).await?).into_response(),
    ))
}

async fn create_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<CreateFollowUpBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .create_follow_up(
            &workspace_id,
            &body.summary,
            &body.due_date,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn update_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<UpdateFollowUpBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .update_follow_up(&workspace_id, &follow_up_id, &body)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn review_follow_up_now(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<FollowUpActionCommandBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .review_follow_up_now(&workspace_id, &follow_up_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn stop_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<StopFollowUpBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .stop_follow_up(
            &workspace_id,
            &follow_up_id,
            body.reason,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn stop_and_discuss_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<StopFollowUpBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let tz = body.physician_time_zone.as_deref().unwrap_or("UTC");
    let out = session
        .store
        .stop_and_discuss_follow_up(
            &workspace_id,
            &follow_up_id,
            body.reason,
            tz,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn resume_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<FollowUpActionCommandBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .resume_follow_up(&workspace_id, &follow_up_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn complete_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<SettleFollowUpBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .complete_follow_up(
            &workspace_id,
            &follow_up_id,
            &body.note,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn cancel_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<SettleFollowUpBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .cancel_follow_up(
            &workspace_id,
            &follow_up_id,
            &body.note,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn resolve_cannot_restart_follow_up(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, follow_up_id)): Path<(String, String)>,
    Json(body): Json<ResolveCannotRestartBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .resolve_cannot_restart_follow_up(
            &workspace_id,
            &follow_up_id,
            body.resolution,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

fn parse_source(slug: &str) -> Result<SupportedSourceWire, ApiError> {
    SupportedSourceWire::from_slug(slug)
        .ok_or_else(|| ApiError::NotFound(format!("unknown practice source `{slug}`")))
}

async fn connections(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.connections().await?).into_response(),
    ))
}

async fn accept_spruce_backlog(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.accept_spruce_backlog().await?).into_response(),
    ))
}

#[derive(Deserialize)]
struct ResolveRollbackBody {
    adopt_running_release: bool,
}

async fn resolve_spruce_rollback(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<ResolveRollbackBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .resolve_spruce_rollback(body.adopt_running_release)
                .await?,
        )
        .into_response(),
    ))
}

async fn connect_source(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<ConnectSourceBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.connect_source(source, &body).await?).into_response(),
    ))
}

async fn request_assisted_setup(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<RequestAssistedSetupBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.request_assisted_setup(source, &body).await?).into_response(),
    ))
}

async fn import_source_records(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.import_source_records(source).await?).into_response(),
    ))
}

async fn refresh_source(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.refresh_source(source).await?).into_response(),
    ))
}

async fn reconnect_source(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<ConnectSourceBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.reconnect_source(source, &body).await?).into_response(),
    ))
}

async fn disconnect_source(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.disconnect_source(source).await?).into_response(),
    ))
}

async fn simulate_source_event(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(body): Json<SimulateSourceEventBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let source = parse_source(&slug)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.simulate_source_event(source, &body).await?).into_response(),
    ))
}

#[derive(Deserialize)]
struct AssignPanelBody {
    allowed: bool,
}

async fn assign_panel(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((physician_id, workspace_id)): Path<(String, String)>,
    Json(body): Json<AssignPanelBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .assign_panel(&physician_id, &workspace_id, body.allowed)
                .await?,
        )
        .into_response(),
    ))
}

async fn remove_panel(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((physician_id, workspace_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .assign_panel(&physician_id, &workspace_id, false)
                .await?,
        )
        .into_response(),
    ))
}

async fn invite_patient(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<InvitePatientRequest>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.invite_patient(&body).await?).into_response(),
    ))
}

#[derive(Deserialize)]
struct PatientInvitationActionBody {
    patient_workspace_id: String,
}

async fn resend_patient_invitation(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<PatientInvitationActionBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &body.patient_workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .resend_patient_invitation(&body.patient_workspace_id)
                .await?,
        )
        .into_response(),
    ))
}

async fn cancel_patient_invitation(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<PatientInvitationActionBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &body.patient_workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .cancel_patient_invitation(&body.patient_workspace_id)
                .await?,
        )
        .into_response(),
    ))
}

async fn invite_colleague(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(body): Json<InviteColleagueRequest>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.invite_colleague(&body).await?).into_response(),
    ))
}

async fn messages_to_match(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.messages_to_match().await?).into_response(),
    ))
}

async fn filtered_spruce_messages(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.filtered_spruce_messages().await?).into_response(),
    ))
}

async fn confirm_spruce_message_link(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
    Json(body): Json<ConfirmMessageLinkBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &body.patient_workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .confirm_spruce_message_link(
            &message_id,
            &body.patient_workspace_id,
            body.sender_policy,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn leave_spruce_message_unlinked(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
    Json(body): Json<MatchingActionCommandBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .leave_spruce_message_unlinked(&message_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn mark_spruce_message_as_patient(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
    Json(body): Json<MatchingActionCommandBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .mark_spruce_message_as_patient(&message_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn mark_unreadable_spruce_message_done(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(message_id): Path<String>,
    Json(body): Json<MatchingActionCommandBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .mark_unreadable_spruce_message_done(&message_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn patient_spruce_replies(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.patient_spruce_replies(&workspace_id).await?).into_response(),
    ))
}

async fn patient_spruce_reply(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .patient_spruce_reply(&workspace_id, &conversation_id)
                .await?,
        )
        .into_response(),
    ))
}

async fn save_reply_proposal_draft(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<SaveReplyProposalDraftBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .save_reply_proposal_draft(
            &workspace_id,
            &conversation_id,
            body.expected_base,
            &body.text,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn request_reply_proposal_revision(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<RequestReplyProposalRevisionBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .request_reply_proposal_revision(&workspace_id, &conversation_id, &body)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn restore_my_reply_version(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<RestoreMyReplyVersionBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .restore_my_reply_version(
            &workspace_id,
            &conversation_id,
            body.expected_draft_revision,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn choose_coda_reply_update(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<ChooseCodaReplyUpdateBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .choose_coda_reply_update(
            &workspace_id,
            &conversation_id,
            body.expected_draft_revision,
            body.expected_proposal_version,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn copy_spruce_reply(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<CopyReplyBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .copy_spruce_reply(
            &workspace_id,
            &conversation_id,
            body.expected_base,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn close_reply_no_reply_needed(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<NoReplyNeededBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .close_reply_no_reply_needed(
            &workspace_id,
            &conversation_id,
            body.expected_proposal_version,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn undo_close_no_reply_needed(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<ReplyCommandKeyBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .undo_close_no_reply_needed(&workspace_id, &conversation_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn ask_coda_to_reply(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<ReplyCommandKeyBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .ask_coda_to_reply(&workspace_id, &conversation_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn move_spruce_conversation(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<MoveSpruceConversationBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .move_spruce_conversation(
            &workspace_id,
            &conversation_id,
            body.destination,
            body.undo_window_seconds,
            &body.idempotency_key,
        )
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn undo_spruce_conversation_move(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<ReplyCommandKeyBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .undo_spruce_conversation_move(&workspace_id, &conversation_id, &body.idempotency_key)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn mark_disconnected_reply_done(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, conversation_id)): Path<(String, String)>,
    Json(body): Json<DisconnectedReplyDoneBody>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    let out = session
        .store
        .mark_disconnected_reply_done(&workspace_id, &conversation_id, &body)
        .await?;
    Ok(no_store(Json(out).into_response()))
}

async fn patient_reviews(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(session.store.patient_reviews(&workspace_id).await?).into_response(),
    ))
}

async fn patient_review(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, review_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .patient_review(&workspace_id, &review_id)
                .await?,
        )
        .into_response(),
    ))
}

async fn control_patient_review(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Path((workspace_id, review_id)): Path<(String, String)>,
    Json(body): Json<ReviewControlRequest>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    check_workspace(&session, &workspace_id)?;
    let _guard = session.store_lock.lock().await;
    Ok(no_store(
        Json(
            session
                .store
                .control_patient_review(&workspace_id, &review_id, &body)
                .await?,
        )
        .into_response(),
    ))
}

async fn dictate(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    Json(_body): Json<DictationRequest>,
) -> Result<Response, ApiError> {
    let _session = require_session(&state, &headers)?;
    Err(ApiError::ServiceUnavailable(
        "Audio dictation is unavailable in the public sandbox.".to_owned(),
    ))
}

fn required_str(obj: &Value, key: &str) -> Result<String, ApiError> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::BadRequest(format!("missing required `{key}`")))
}

async fn command(
    State(state): State<DemoApiState>,
    headers: HeaderMap,
    peer_addr: Option<Extension<ConnectInfo<SocketAddr>>>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let session = require_session(&state, &headers)?;
    let kind = required_str(&body, "type")?;
    let payload = body.get("payload").unwrap_or(&body);
    let peer = peer_addr.map(|Extension(ConnectInfo(addr))| addr);

    match kind.as_str() {
        "message.submit" => {
            handle_message_submit(&state, &session, &headers, peer, &body, payload).await
        }
        "message.queue_send_now" => {
            handle_queue_send_now(&state, &session, &headers, peer, payload).await
        }
        "work.stop" => handle_work_stop(&state, &session, payload).await,
        "work.resume" => handle_work_resume(&state, &session, &headers, peer, payload).await,
        "reply.revise" => handle_reply_revise(&state, &session, &headers, peer, payload).await,
        _ => {
            if let Some(ws_id) = payload
                .get("patient_workspace_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                check_workspace(&session, ws_id)?;
            }
            let _guard = session.store_lock.lock().await;
            let out = session.store.command(&body).await?;
            Ok(no_store(Json(out).into_response()))
        }
    }
}

fn check_turn_budget(
    state: &DemoApiState,
    session: &VisitorSession,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
) -> Result<(), ApiError> {
    state
        .budget
        .check_and_record(&session.physician_id, headers, peer)
        .map_err(ApiError::Budget)
}

async fn handle_message_submit(
    state: &DemoApiState,
    session: &VisitorSession,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    envelope: &Value,
    payload: &Value,
) -> Result<Response, ApiError> {
    let ws_id = required_str(payload, "patient_workspace_id")?;
    check_workspace(session, &ws_id)?;
    let chat_id = required_str(payload, "chat_id")?;
    let question = required_str(payload, "text")?;
    check_turn_budget(state, session, headers, peer)?;

    let msg_id = envelope
        .get("idempotency_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("msg-{}", next_demo_seq()));

    let delivery = payload.get("delivery").and_then(Value::as_str);
    let guard = session.store_lock.lock().await;
    let existing = session.store.load_chat_snapshot_row(&ws_id, &chat_id)?;
    let (title, created_at, mut snap) = existing.unwrap_or_else(|| {
        (
            fallback_title(&question),
            DEFAULT_DEMO_NOW.to_owned(),
            ChatSnapshot {
                sources: Default::default(),
                transcript: Vec::new(),
                feedback: SnapshotFeedback::Loaded,
                queued: Vec::new(),
                archived: false,
            },
        )
    });
    let seq = next_demo_seq();
    let ts = default_demo_timestamp(seq);

    if delivery == Some("send_next") {
        snap.queued.push(QueuedMessageWire {
            id: physician_wire::MessageKey(msg_id),
            text: question,
            saved_at: SavedAt(ts),
        });
        session
            .store
            .save_chat_snapshot_row(&ws_id, &chat_id, &title, &created_at, &snap)?;
        drop(guard);
        let _ = state
            .bus
            .publish(BusEvent::WorkspaceChanged {
                workspace_id: ws_id,
            })
            .await;
        return Ok(no_store(
            Json(json!({ "ok": true, "chat_id": chat_id })).into_response(),
        ));
    }

    let prior_history = extract_prior_history(&snap);
    let work_item_id = format!("wi-live-{seq}");
    let run_id = format!("run-live-{seq}");
    snap.transcript.push(create_working_exchange(
        work_item_id.clone(),
        msg_id,
        question.clone(),
        ts,
    ));
    session
        .store
        .save_chat_snapshot_row(&ws_id, &chat_id, &title, &created_at, &snap)?;
    drop(guard);
    let _ = state
        .bus
        .publish(BusEvent::WorkspaceChanged {
            workspace_id: ws_id.clone(),
        })
        .await;

    state.harness.spawn_exchange(
        session.clone(),
        ws_id,
        chat_id.clone(),
        work_item_id,
        run_id,
        question,
        prior_history,
    );
    Ok(no_store(
        Json(json!({ "ok": true, "chat_id": chat_id })).into_response(),
    ))
}

async fn handle_queue_send_now(
    state: &DemoApiState,
    session: &VisitorSession,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    payload: &Value,
) -> Result<Response, ApiError> {
    let ws_id = required_str(payload, "patient_workspace_id")?;
    check_workspace(session, &ws_id)?;
    let chat_id = required_str(payload, "chat_id")?;
    let msg_id = required_str(payload, "message_id")?;
    check_turn_budget(state, session, headers, peer)?;

    let guard = session.store_lock.lock().await;
    let Some((title, created_at, mut snap)) =
        session.store.load_chat_snapshot_row(&ws_id, &chat_id)?
    else {
        return Ok(no_store(
            Json(json!({
                "outcome": "rejected",
                "message": "Coda couldn’t send that queued message."
            }))
            .into_response(),
        ));
    };
    let Some(pos) = snap.queued.iter().position(|q| q.id.0 == msg_id) else {
        return Ok(no_store(
            Json(json!({
                "outcome": "rejected",
                "message": "Coda couldn’t send that queued message."
            }))
            .into_response(),
        ));
    };
    if let Some(active) = snap
        .transcript
        .iter()
        .find(|ex| matches!(ex.answer, AnswerState::Working { .. }))
    {
        session.cancel_run(&ws_id, &active.work_item_id.0);
    }
    let queued = snap.queued.remove(pos);
    let prior_history = extract_prior_history(&snap);
    let seq = next_demo_seq();
    let work_item_id = format!("wi-live-{seq}");
    let run_id = format!("run-live-{seq}");
    snap.transcript.push(create_working_exchange(
        work_item_id.clone(),
        queued.id.0,
        queued.text.clone(),
        queued.saved_at.0,
    ));
    session
        .store
        .save_chat_snapshot_row(&ws_id, &chat_id, &title, &created_at, &snap)?;
    drop(guard);
    let _ = state
        .bus
        .publish(BusEvent::WorkspaceChanged {
            workspace_id: ws_id.clone(),
        })
        .await;

    state.harness.spawn_exchange(
        session.clone(),
        ws_id.clone(),
        chat_id.clone(),
        work_item_id,
        run_id,
        queued.text,
        prior_history,
    );
    Ok(no_store(
        Json(json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "message_id": msg_id,
        }))
        .into_response(),
    ))
}

async fn handle_work_stop(
    state: &DemoApiState,
    session: &VisitorSession,
    payload: &Value,
) -> Result<Response, ApiError> {
    let ws_id = required_str(payload, "patient_workspace_id")?;
    check_workspace(session, &ws_id)?;
    let chat_id = required_str(payload, "chat_id")?;
    let requested_work = payload
        .get("work_item_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let guard = session.store_lock.lock().await;
    let Some((title, created_at, mut snap)) =
        session.store.load_chat_snapshot_row(&ws_id, &chat_id)?
    else {
        return Ok(no_store(
            Json(json!({ "outcome": "rejected", "message": "Coda couldn’t stop that work." }))
                .into_response(),
        ));
    };
    let idx = match requested_work {
        Some(work_id) => snap
            .transcript
            .iter()
            .position(|ex| ex.work_item_id.0 == work_id),
        None => snap
            .transcript
            .iter()
            .rposition(|ex| matches!(ex.answer, AnswerState::Working { .. })),
    };
    let Some(idx) = idx else {
        return Ok(no_store(
            Json(json!({ "outcome": "rejected", "message": "Coda couldn’t stop that work." }))
                .into_response(),
        ));
    };
    let ex = &mut snap.transcript[idx];
    if !matches!(ex.answer, AnswerState::Working { .. }) {
        return Ok(no_store(
            Json(json!({ "outcome": "rejected", "message": "Coda couldn’t stop that work." }))
                .into_response(),
        ));
    }
    let work_id = ex.work_item_id.0.clone();
    let expected_run_id = session
        .cancel_run(&ws_id, &work_id)
        .unwrap_or_else(|| format!("run-stopped-{}", next_demo_seq()));

    crate::harness_product::close_running_steps(&mut ex.steps, true);
    ex.answer = AnswerState::Stopped {
        resume: Some(WorkResumeTarget {
            work_item_id: WorkItemId(work_id.clone()),
            expected_run_id: expected_run_id.clone(),
        }),
    };
    let returned_messages: Vec<String> = snap.queued.drain(..).map(|q| q.text).collect();
    session
        .store
        .save_chat_snapshot_row(&ws_id, &chat_id, &title, &created_at, &snap)?;
    drop(guard);
    let _ = state
        .bus
        .publish(BusEvent::WorkspaceChanged {
            workspace_id: ws_id.clone(),
        })
        .await;

    Ok(no_store(
        Json(json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "work_item_id": work_id,
            "expected_run_id": expected_run_id,
            "returned_messages": returned_messages,
        }))
        .into_response(),
    ))
}

async fn handle_work_resume(
    state: &DemoApiState,
    session: &VisitorSession,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    payload: &Value,
) -> Result<Response, ApiError> {
    let ws_id = required_str(payload, "patient_workspace_id")?;
    check_workspace(session, &ws_id)?;
    let work_item_id = required_str(payload, "work_item_id")?;
    let expected_run_id = payload.get("expected_run_id").and_then(Value::as_str);
    let requested_chat = payload
        .get("chat_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    check_turn_budget(state, session, headers, peer)?;

    let guard = session.store_lock.lock().await;
    let Some(mut found) =
        session
            .store
            .locate_chat_work_exchange(&ws_id, requested_chat, &work_item_id)?
    else {
        return Ok(no_store(
            Json(json!({ "outcome": "rejected", "message": "Coda couldn’t resume that work." }))
                .into_response(),
        ));
    };
    let target_run = match &found.snapshot.transcript[found.exchange_idx].answer {
        AnswerState::Stopped { resume } => resume.as_ref().map(|r| r.expected_run_id.as_str()),
        AnswerState::CutOff { try_again } | AnswerState::CouldntFinish { try_again } => {
            try_again.as_ref().map(|r| r.expected_run_id.as_str())
        }
        AnswerState::Working { .. } | AnswerState::Finished { .. } => None,
    };
    let Some(actual_run) = target_run else {
        return Ok(no_store(
            Json(json!({ "outcome": "rejected", "message": "Coda couldn’t resume that work." }))
                .into_response(),
        ));
    };
    if let Some(expected) = expected_run_id
        && actual_run != expected
    {
        return Ok(no_store(
            Json(json!({ "outcome": "rejected", "message": "Coda couldn’t resume that work." }))
                .into_response(),
        ));
    }

    let prior_history = {
        let mut prior_snap = found.snapshot.clone();
        prior_snap.transcript.truncate(found.exchange_idx);
        extract_prior_history(&prior_snap)
    };
    let ex = &mut found.snapshot.transcript[found.exchange_idx];
    let question = ex
        .messages
        .iter()
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    ex.steps = ExchangeSteps::Read(Vec::new());
    ex.answer = AnswerState::Working {
        stop: WorkStopTarget {
            work_item_id: WorkItemId(work_item_id.clone()),
        },
        writing: false,
    };
    let new_run_id = format!("run-live-{}", next_demo_seq());
    let chat_id = found.chat_id.clone();
    session.store.save_chat_snapshot_row(
        &ws_id,
        &chat_id,
        &found.title,
        &found.created_at,
        &found.snapshot,
    )?;
    drop(guard);
    let _ = state
        .bus
        .publish(BusEvent::WorkspaceChanged {
            workspace_id: ws_id.clone(),
        })
        .await;

    state.harness.spawn_exchange(
        session.clone(),
        ws_id.clone(),
        chat_id.clone(),
        work_item_id.clone(),
        new_run_id,
        question,
        prior_history,
    );
    Ok(no_store(
        Json(json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "work_item_id": work_item_id,
        }))
        .into_response(),
    ))
}

async fn handle_reply_revise(
    state: &DemoApiState,
    session: &VisitorSession,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    payload: &Value,
) -> Result<Response, ApiError> {
    let ws_id = required_str(payload, "patient_workspace_id")?;
    check_workspace(session, &ws_id)?;
    let reply_work_id = required_str(payload, "reply_work_id")?;
    let instruction = required_str(payload, "instruction")?;
    let expected_rev = payload
        .get("expected_draft_revision")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    check_turn_budget(state, session, headers, peer)?;

    let next_rev = run_reply_revision_with_provider(
        &state.provider_backend,
        session,
        &ws_id,
        &reply_work_id,
        &instruction,
        expected_rev,
    )
    .await
    .map_err(|msg| {
        if msg.contains("revision changed") {
            ApiError::Conflict(msg)
        } else {
            ApiError::Internal(msg)
        }
    })?;
    let _ = state
        .bus
        .publish(BusEvent::WorkspaceChanged {
            workspace_id: ws_id,
        })
        .await;
    Ok(no_store(
        Json(json!({ "ok": true, "draft_revision": next_rev })).into_response(),
    ))
}
