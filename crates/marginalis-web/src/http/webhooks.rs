//! Webhook購読の管理API。
//!
//! secretは登録と再生成の応答でだけ返し、一覧や失敗応答には含めない。

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use marginalis_application::WebhookVerificationOutcome;
use marginalis_contract::{
    WebhookDeliveryFailureReason, WebhookDisabledReason, WebhookEventKind, WebhookSecretResponse,
    WebhookSubscriptionCreatedResponse, WebhookSubscriptionInput, WebhookSubscriptionResponse,
    WebhookSubscriptionState, WebhookVerificationResponse,
};

use super::{
    auth::{authenticated_actor, authenticated_mutation_actor},
    error::{HandlerResult, webhook_error},
    state::ApiState,
};

pub(super) async fn list_webhooks(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> HandlerResult<Json<Vec<WebhookSubscriptionResponse>>> {
    let actor = authenticated_actor(&headers, &state).await?;
    let subscriptions = state
        .webhooks
        .list_subscriptions(&actor)
        .await
        .map_err(webhook_error)?;
    Ok(Json(
        subscriptions
            .into_iter()
            .map(subscription_response)
            .collect(),
    ))
}

pub(super) async fn create_webhook(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<WebhookSubscriptionInput>,
) -> HandlerResult<(StatusCode, Json<WebhookSubscriptionCreatedResponse>)> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let event_kinds = input
        .event_kinds
        .iter()
        .map(|kind| event_kind_value(*kind).to_owned())
        .collect();
    let (subscription, secret) = state
        .webhooks
        .create_subscription(&actor, &input.url, event_kinds)
        .await
        .map_err(webhook_error)?;
    Ok((
        StatusCode::CREATED,
        Json(WebhookSubscriptionCreatedResponse {
            subscription: subscription_response(subscription),
            secret,
        }),
    ))
}

pub(super) async fn verify_webhook(
    State(state): State<ApiState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Json<WebhookVerificationResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let outcome = state
        .webhooks
        .verify_subscription(&actor, &subscription_id)
        .await
        .map_err(webhook_error)?;
    Ok(Json(match outcome {
        WebhookVerificationOutcome::Activated => WebhookVerificationResponse {
            verified: true,
            failure: None,
        },
        WebhookVerificationOutcome::Failed(failure) => WebhookVerificationResponse {
            verified: false,
            failure: Some(failure_reason_response(failure.as_str())),
        },
    }))
}

pub(super) async fn regenerate_webhook_secret(
    State(state): State<ApiState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<Json<WebhookSecretResponse>> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    let secret = state
        .webhooks
        .regenerate_secret(&actor, &subscription_id)
        .await
        .map_err(webhook_error)?;
    Ok(Json(WebhookSecretResponse { secret }))
}

pub(super) async fn delete_webhook(
    State(state): State<ApiState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    state
        .webhooks
        .delete_subscription(&actor, &subscription_id)
        .await
        .map_err(webhook_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn retry_webhook_delivery(
    State(state): State<ApiState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    state
        .webhooks
        .retry_delivery(&actor, &subscription_id)
        .await
        .map_err(webhook_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn discard_webhook_delivery(
    State(state): State<ApiState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
) -> HandlerResult<StatusCode> {
    let actor = authenticated_mutation_actor(&headers, &state).await?;
    state
        .webhooks
        .discard_delivery(&actor, &subscription_id)
        .await
        .map_err(webhook_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn subscription_response(
    subscription: marginalis_application::WebhookSubscriptionOverview,
) -> WebhookSubscriptionResponse {
    WebhookSubscriptionResponse {
        subscription_id: subscription.subscription_id,
        url: subscription.url,
        event_kinds: subscription
            .event_kinds
            .iter()
            .filter_map(|kind| event_kind_response(kind))
            .collect(),
        state: match subscription.state {
            marginalis_application::WebhookSubscriptionState::PendingChallenge => {
                WebhookSubscriptionState::PendingChallenge
            }
            marginalis_application::WebhookSubscriptionState::Active => {
                WebhookSubscriptionState::Active
            }
            marginalis_application::WebhookSubscriptionState::Disabled => {
                WebhookSubscriptionState::Disabled
            }
        },
        disabled_reason: subscription
            .disabled_reason
            .as_deref()
            .and_then(disabled_reason_response),
        created_at_ms: subscription.created_at.get(),
        updated_at_ms: subscription.updated_at.get(),
        revision: subscription.revision,
        last_attempted_at_ms: subscription
            .last_attempted_at
            .map(|timestamp| timestamp.get()),
        last_failure: subscription
            .last_failure
            .as_deref()
            .map(failure_reason_response),
        next_attempt_at_ms: subscription
            .next_attempt_at
            .map(|timestamp| timestamp.get()),
        pending_count: subscription.pending_count,
    }
}

fn event_kind_value(kind: WebhookEventKind) -> &'static str {
    match kind {
        WebhookEventKind::NoteCreated => "note.created",
        WebhookEventKind::NoteUpdated => "note.updated",
        WebhookEventKind::NoteDeleted => "note.deleted",
        WebhookEventKind::NoteRestored => "note.restored",
        WebhookEventKind::BibliographyItemCreated => "bibliography_item.created",
        WebhookEventKind::BibliographyItemUpdated => "bibliography_item.updated",
        WebhookEventKind::BibliographyItemDeleted => "bibliography_item.deleted",
    }
}

fn event_kind_response(kind: &str) -> Option<WebhookEventKind> {
    match kind {
        "note.created" => Some(WebhookEventKind::NoteCreated),
        "note.updated" => Some(WebhookEventKind::NoteUpdated),
        "note.deleted" => Some(WebhookEventKind::NoteDeleted),
        "note.restored" => Some(WebhookEventKind::NoteRestored),
        "bibliography_item.created" => Some(WebhookEventKind::BibliographyItemCreated),
        "bibliography_item.updated" => Some(WebhookEventKind::BibliographyItemUpdated),
        "bibliography_item.deleted" => Some(WebhookEventKind::BibliographyItemDeleted),
        _ => None,
    }
}

fn disabled_reason_response(reason: &str) -> Option<WebhookDisabledReason> {
    match reason {
        "delivery_exhausted" => Some(WebhookDisabledReason::DeliveryExhausted),
        "destination_rejected" => Some(WebhookDisabledReason::DestinationRejected),
        "owner_disabled" => Some(WebhookDisabledReason::OwnerDisabled),
        _ => None,
    }
}

/// 保存済みの失敗分類を公開表現へ写す。想定外の値は接続失敗として表す。
fn failure_reason_response(reason: &str) -> WebhookDeliveryFailureReason {
    match reason {
        "non_success_status" => WebhookDeliveryFailureReason::NonSuccessStatus,
        "timed_out" => WebhookDeliveryFailureReason::TimedOut,
        "destination_rejected" => WebhookDeliveryFailureReason::DestinationRejected,
        _ => WebhookDeliveryFailureReason::ConnectFailed,
    }
}
