//! MCPで必要になるOAuth Authorization Serverの製品非依存中核。
//!
//! HTTP framework、database、identity provider、利用製品のdomain型には依存しない。

mod core;
mod metadata;
mod policy;
mod protocol;

pub use core::{
    AuthorizationServer, AuthorizationServerConfig, ClientMetadataResolver, Clock, Random,
    Repository, RepositoryError,
};
pub use metadata::{
    AuthorizationServerEndpoints, AuthorizationServerMetadata, ProtectedResourceMetadata,
};
pub use policy::{
    AuthorizationClientError, ResourcePolicy, ResourcePolicyError, canonical_scopes, pkce_s256,
    redirect_uri_matches, valid_client_metadata_document_url, valid_pkce_challenge,
    valid_pkce_verifier, valid_redirect_uri, validate_client_metadata,
    validate_dynamic_client_registration,
};
pub use protocol::{
    ApplicationType, AuthenticatedPrincipal, AuthorizationClient, AuthorizationCodeExchange,
    AuthorizationError, AuthorizationGrant, AuthorizationRequest, Client, ClientRegistrationMethod,
    DynamicClientRegistrationError, DynamicClientRegistrationRequest,
    DynamicClientRegistrationResponse, Principal, RefreshTokenRotation,
    RefreshTokenRotationOutcome, RegisteredClient, ResolvedRedirectUri, Timestamp, TokenPair,
    ValidatedAuthorizationRequest, ValidatedDynamicClientRegistration,
};

#[cfg(test)]
mod tests;
