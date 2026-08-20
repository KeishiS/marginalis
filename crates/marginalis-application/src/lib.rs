//! Marginalisのユースケースと永続化port。
//!
//! HTTP、SQLite、OIDC clientの具体実装を参照せず、業務処理が外側へ要求する境界を定義する。

pub use mcp_authorization_server::{
    AuthenticatedPrincipal as McpAuthenticatedPrincipal,
    AuthorizationClient as McpAuthorizationClient,
    AuthorizationCodeExchange as McpAuthorizationCodeExchange,
    AuthorizationError as McpOAuthUseCaseError, AuthorizationGrant as McpAuthorizationGrant,
    AuthorizationRequest as McpAuthorizationRequest, Client as McpOAuthClient,
    ClientMetadataResolver as McpClientMetadataResolver,
    ClientMetadataResolverError as McpClientMetadataResolverError,
    ClientRegistrationMethod as McpClientRegistrationMethod, Principal as McpPrincipal,
    RefreshTokenRotation as McpRefreshTokenRotation,
    RefreshTokenRotationOutcome as McpRefreshTokenRotationOutcome,
    RegisteredClient as McpRegisteredOAuthClient, Repository as McpOAuthRepository,
    RepositoryError as McpOAuthRepositoryError, ResolvedRedirectUri as McpResolvedRedirectUri,
    ResourcePolicy as McpResourcePolicy, Timestamp as McpTimestamp, TokenPair as McpTokenPair,
    ValidatedAuthorizationRequest as McpValidatedAuthorizationRequest,
};

mod bibliography;
mod bibliography_import;
mod citation;
mod identity;
mod math_macros;
mod mcp_oauth;
mod notes;
mod runtime;
mod snapshot;
mod webhooks;

pub use bibliography::{BibliographyApplication, BibliographyRepository, BibliographyUseCaseError};
pub use bibliography_import::{
    BibliographyImportApplication, BibliographyImportCandidate, BibliographyImportClassification,
    BibliographyImportCommit, BibliographyImportDecision, BibliographyImportDecisionKind,
    BibliographyImportEntry, BibliographyImportInput, BibliographyImportItemMutation,
    BibliographyImportPreview, BibliographyImportRepository, BibliographyImportResult,
    BibliographyImportSourceSelection, BibliographyImportState, BibliographyImportUseCaseError,
    BibliographyImportUseCases,
};
pub use citation::CitationStyle;
pub use identity::{
    AuthenticationUseCaseError, ExternalIdentity, IdentityProvider, IdentityProviderError,
    OidcAuthenticationApplication, OidcAuthenticationUseCases, OidcLoginAttempt,
    OidcLoginAttemptStore, PrincipalDirectory, WebSessionUseCases,
};
pub use math_macros::{
    MAX_MATH_MACRO_ARGUMENTS, MAX_MATH_MACRO_NAME_CHARACTERS,
    MAX_MATH_MACRO_REPLACEMENT_CHARACTERS, MAX_MATH_MACRO_TOTAL_BYTES, MAX_MATH_MACROS, MathMacro,
    MathMacroApplication, MathMacroRepository, MathMacroSettings, MathMacroUseCaseError,
    MathMacroUseCases, validate_math_macros, validate_stored_math_macros,
};
pub use mcp_oauth::{
    McpAuthenticatedActor, McpClientAuthorization, McpClientAuthorizationRecord,
    McpEffectiveScopeCeiling, McpOAuthApplication, McpOAuthUseCases, McpScopeCeilingRepository,
    McpScopeCeilingSetting, McpScopeCeilingUseCaseError, McpStoredClientAuthorization,
    McpStoredScopeCeilings,
};
pub use notes::{
    AccessibleNote, NOTE_SYNC_CURSOR_RETENTION_MS, NOTE_SYNC_DEFAULT_PAGE_SIZE,
    NOTE_SYNC_MAX_PAGE_SIZE, NoteAclChange, NoteAclRepository, NoteAclState,
    NoteAdvisoryDiagnostic, NoteAdvisorySeverity, NoteApplication, NoteApplicationDependencies,
    NoteAttachmentQuery, NoteAttachmentResolution, NoteBibliographyEntry, NoteCitationQuery,
    NoteCitationResolution, NoteCitationSegment, NoteCommandRepository, NoteContent,
    NoteContentError, NoteGraph, NoteGraphCitation, NoteGraphNote, NoteGraphQuery,
    NoteGraphReference, NoteGraphWork, NoteLinkResolver, NoteLinks, NoteListQuery, NoteOutline,
    NoteOutlineSection, NotePatchApplication, NotePatchError, NotePatchOutcome, NotePreview,
    NoteProfile, NoteProfileAdvisoryRule, NoteProfileExample, NoteProfileLimits,
    NoteProfileNormalization, NoteProfileRule, NoteProfileSyntax, NoteQueryRepository,
    NoteReferenceQuery, NoteReferenceResolution, NoteRenderContext, NoteRenderInputs,
    NoteRepository, NoteReviewDetails, NoteReviewRepository, NoteRevisionDiff, NoteRevisionView,
    NoteSourcePosition, NoteSourceSpan, NoteSourceSpanKind, NoteSyncEntry, NoteSyncPage,
    NoteSyncPhase, NoteSyncRemovalReason, NoteSyncRepository, NoteSyncRepositoryError,
    NoteUseCaseError, NoteUseCases, NoteValidationCode, NoteValidationDiagnostic, NoteView,
    NoteViewSnapshot, NoteWritePolicy, RelatedNotes, ValidatedNoteDraft, apply_note_patch,
};
pub use runtime::{Clock, Random, StorageError};
pub use snapshot::{
    InvalidSnapshot, LogicalSnapshot, MathMacroSettingsSnapshot, NoteAclSnapshotEntry, RestorePlan,
};
pub use webhooks::{
    InvalidWebhookDestination, WEBHOOK_BACKOFF_BASE_MS, WEBHOOK_BACKOFF_MAX_MS,
    WEBHOOK_CONTRACT_VERSION, WEBHOOK_DELIVERY_BATCH, WEBHOOK_EVENT_KINDS, WEBHOOK_LEASE_MS,
    WEBHOOK_MAX_ATTEMPTS, WEBHOOK_RETENTION_MS, WebhookDeliveryFailure, WebhookDeliveryRepository,
    WebhookDeliverySender, WebhookDestination, WebhookEvent, WebhookPendingDelivery,
    WebhookSubscriptionApplication, WebhookSubscriptionOverview, WebhookSubscriptionRepository,
    WebhookSubscriptionState, WebhookTickOutcome, WebhookUseCaseError, WebhookUseCases,
    WebhookVerificationOutcome, is_public_webhook_address, validate_webhook_destination,
    webhook_backoff_ms, webhook_delivery_body, webhook_delivery_tick,
};
