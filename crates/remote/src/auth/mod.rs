mod handoff;
mod jwt;
mod local;
mod middleware;
mod oauth_token_validator;
mod provider;

pub(crate) use handoff::{CallbackResult, HandoffError, OAuthHandoffService};
pub(crate) use jwt::{JwtError, JwtService};
pub(crate) use local::{LocalAuthError, auth_methods_response, is_local_provider, login};
// QRAFT-CUSTOM: add request_context_from_access_token for jira.rs
pub(crate) use middleware::{RequestContext, request_context_from_access_token, require_session};
pub(crate) use oauth_token_validator::{OAuthTokenValidationError, OAuthTokenValidator};
pub(crate) use provider::{
    GitHubOAuthProvider, GoogleOAuthProvider, ProviderRegistry, ProviderTokenDetails,
};
