// OIDC Discovery Endpoints

use anyhow::Error;
use poem::{IntoResponse, Response, handler};
use serde_json::json;

/// OpenID Connect Discovery endpoint
/// Returns metadata about the OAuth2/OIDC provider
/// Spec: https://openid.net/specs/openid-connect-discovery-1_0.html
#[handler]
pub async fn openid_configuration() -> Result<Response, Error> {
    let base_url = format!("https://{}", env!("DOMAIN"));

    let config = json!({
        // Required fields
        "issuer": base_url,
        "authorization_endpoint": format!("{}/oauth2/authorize", base_url),
        "token_endpoint": format!("{}/oauth2/token", base_url),
        "jwks_uri": format!("{}/.well-known/jwks.json", base_url),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],

        // Optional but recommended fields
        "userinfo_endpoint": format!("{}/oauth2/userinfo", base_url),
        "scopes_supported": ["openid", "profile", "email", "participation"],
        "claims_supported": [
            "sub",
            "name",
            "email",
            "email_verified",
            "preferred_username",
            "profile",
            "picture"
        ],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["client_secret_post", "client_secret_basic"],
        "code_challenge_methods_supported": ["S256", "plain"],

        // SciStarter-specific
        "service_documentation": "https://scistarter.org/api-docs",
    });

    Ok(poem::web::Json(config).into_response())
}

/// JSON Web Key Set (JWKS) endpoint
/// Returns public keys used to verify JWT signatures
/// Spec: https://tools.ietf.org/html/rfc7517
#[handler]
pub async fn jwks() -> Result<Response, Error> {
    use base64::engine::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::traits::PublicKeyParts;

    // Load RSA private key from environment (same key orrery uses)
    let pem = env!("OIDC_RSA_PRIVATE_KEY");

    // Parse PKCS#1 format (BEGIN RSA PRIVATE KEY)
    let private_key = RsaPrivateKey::from_pkcs1_pem(pem)
        .map_err(|e| anyhow::anyhow!("Failed to parse RSA private key: {}", e))?;

    // Extract public key components for JWK
    let public_key = private_key.to_public_key();
    let n = public_key.n().to_bytes_be();
    let e = public_key.e().to_bytes_be();

    // Encode as base64url
    let n_b64 = URL_SAFE_NO_PAD.encode(&n);
    let e_b64 = URL_SAFE_NO_PAD.encode(&e);

    let jwks = json!({
        "keys": [
            {
                "kty": "RSA",
                "use": "sig",
                "kid": "scistarter-oidc-key",
                "n": n_b64,
                "e": e_b64,
                "alg": "RS256"
            }
        ]
    });

    Ok(poem::web::Json(jwks).into_response())
}
