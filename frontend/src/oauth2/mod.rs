// OAuth2 / OpenID Connect Provider Implementation

pub mod authorize;
pub mod client;
pub mod discovery;
pub mod token;
pub mod types;
pub mod userinfo;

use poem::{Route, get, post};

/// OAuth2 routes (to be nested under /oauth2)
pub fn oauth2_routes() -> Route {
    Route::new()
        .at(
            "/authorize",
            get(authorize::authorize_get).post(authorize::authorize_post),
        )
        .at("/token", post(token::token_endpoint))
        .at("/userinfo", get(userinfo::userinfo_endpoint))
}

/// Well-known routes (to be nested under /.well-known at root level)
pub fn well_known_routes() -> Route {
    Route::new()
        .at(
            "/openid-configuration",
            get(discovery::openid_configuration),
        )
        .at("/jwks.json", get(discovery::jwks))
}
