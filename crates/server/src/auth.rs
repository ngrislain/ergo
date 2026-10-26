use crate::{ApiError, AppState};
use anyhow::{Context, Result, ensure};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    env,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::RwLock;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn http_origin(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value)?;
    ensure!(
        ["https", "http"].contains(&url.scheme())
            && url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "Expected an HTTP(S) origin"
    );
    Ok(url)
}

fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[derive(Clone)]
pub struct Auth {
    pub google: bool,
    client_id: String,
    secret: String,
    signing: Vec<u8>,
    public_url: String,
    http: reqwest::Client,
    keys: Arc<RwLock<Option<(u64, JwkSet)>>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub picture: Option<String>,
    pub created_at: u64,
    pub last_login: u64,
}

#[derive(Serialize, Deserialize)]
struct Session {
    user_id: String,
    expires: u64,
}
#[derive(Serialize, Deserialize)]
struct McpDelegation {
    purpose: String,
    actor: String,
    audience: String,
    expires: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationRequest {
    #[serde(default = "delegation_lifetime")]
    expires_in_seconds: u64,
}
fn delegation_lifetime() -> u64 {
    3600
}
#[derive(Serialize, Deserialize)]
struct Login {
    state: String,
    nonce: String,
    verifier: String,
    expires: u64,
    #[serde(default)]
    return_to: Option<String>,
}
#[derive(Deserialize)]
pub struct LoginQuery {
    return_to: Option<String>,
}
#[derive(Clone, Deserialize)]
struct Claims {
    sub: String,
    nonce: String,
    name: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
    picture: Option<String>,
}
#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}
#[derive(Deserialize)]
struct Token {
    id_token: String,
}
#[derive(Clone)]
pub struct Principal {
    pub id: Option<String>,
}
impl Principal {
    pub fn can_read(&self, world: &ergo_world::WorldManifest) -> bool {
        world.owner == "examples" || self.id.as_ref() == Some(&world.owner)
    }
    pub fn required(&self) -> Result<String> {
        self.id
            .clone()
            .context("Sign in with Google to import or save a construction")
    }
}

impl Auth {
    fn login_target(&self, target: Option<&str>) -> Result<String> {
        let target = target.unwrap_or("/");
        ensure!(
            target.len() <= 1024 && target.starts_with('/') && !target.starts_with("//"),
            "Invalid login return path"
        );
        let parsed = url::Url::parse(&self.public_url)?.join(target)?;
        ensure!(
            parsed.origin().ascii_serialization() == self.public_url
                && ["/", "/app", "/login"].contains(&parsed.path()),
            "Login must return to this application's landing page or world viewer"
        );
        Ok(parsed
            .as_str()
            .strip_prefix(&self.public_url)
            .context("Invalid login return origin")?
            .to_owned())
    }
    #[cfg(test)]
    pub(crate) fn test_session(user: &str) -> (Self, HeaderMap) {
        let auth = Self {
            google: true,
            client_id: String::new(),
            secret: String::new(),
            signing: vec![42; 32],
            public_url: "http://localhost:3000".into(),
            http: reqwest::Client::new(),
            keys: Arc::new(RwLock::new(None)),
        };
        let value = auth
            .sign(&Session {
                user_id: user.into(),
                expires: now() + 60,
            })
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("ergo_session={value}").parse().unwrap(),
        );
        headers.insert("X-Ergo-Request", HeaderValue::from_static("1"));
        (auth, headers)
    }

    pub fn from_env() -> Result<Self> {
        let client_id = env::var("GOOGLE_CLIENT_ID").unwrap_or_default();
        let secret = env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();
        ensure!(
            client_id.is_empty() == secret.is_empty(),
            "Set both GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET"
        );
        let google = !client_id.is_empty();
        let signing = if google {
            let key = env::var("ERGO_SESSION_SECRET")
                .context("Set ERGO_SESSION_SECRET (at least 32 random characters)")?;
            ensure!(
                key.len() >= 32,
                "Session secret must have at least 32 characters"
            );
            key.into_bytes()
        } else {
            random().into_bytes()
        };
        let public_url = env::var("ERGO_PUBLIC_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:3000".into())
            .trim_end_matches('/')
            .to_owned();
        let parsed =
            http_origin(&public_url).context("ERGO_PUBLIC_URL must be an HTTP(S) origin")?;
        if google {
            ensure!(
                parsed.scheme() == "https"
                    || [Some("localhost"), Some("127.0.0.1")].contains(&parsed.host_str()),
                "Google authentication needs HTTPS outside localhost"
            );
        }
        Ok(Self {
            google,
            client_id,
            secret,
            signing,
            public_url: parsed.origin().ascii_serialization(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            keys: Arc::new(RwLock::new(None)),
        })
    }
    fn sign<T: Serialize>(&self, value: &T) -> Result<String> {
        let text = B64.encode(serde_json::to_vec(value)?);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing)?;
        mac.update(text.as_bytes());
        Ok(format!(
            "{text}.{}",
            B64.encode(mac.finalize().into_bytes())
        ))
    }
    fn verify<T: DeserializeOwned>(&self, value: &str) -> Result<T> {
        ensure!(value.len() < 4096, "Invalid cookie length");
        let (text, signature) = value.split_once('.').context("Invalid session")?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.signing)?;
        mac.update(text.as_bytes());
        mac.verify_slice(&B64.decode(signature)?)?;
        Ok(serde_json::from_slice(&B64.decode(text)?)?)
    }
    pub fn principal(&self, headers: &HeaderMap) -> Principal {
        if !self.google {
            return Principal {
                id: Some("development".into()),
            };
        }
        let session = cookie(headers, "ergo_session")
            .and_then(|v| self.verify::<Session>(v).ok())
            .filter(|s| s.expires > now());
        Principal {
            id: session.map(|s| s.user_id),
        }
    }
    pub fn check_write(&self, headers: &HeaderMap) -> Result<String> {
        self.check_request(headers)?;
        self.principal(headers).required()
    }
    fn check_request(&self, headers: &HeaderMap) -> Result<()> {
        ensure!(
            headers.get("X-Ergo-Request").and_then(|h| h.to_str().ok()) == Some("1"),
            "Missing same-origin request header"
        );
        if let Some(origin) = headers.get(header::ORIGIN) {
            ensure!(
                self.allows_origin(origin.to_str()?, headers),
                "Unexpected request origin. Open {} or set ERGO_PUBLIC_URL to match the browser's origin",
                self.public_url
            );
        }
        Ok(())
    }
    pub fn shared_principal(&self, headers: &HeaderMap) -> Result<Principal> {
        if let Some(value) = headers.get(header::AUTHORIZATION) {
            let token = value
                .to_str()?
                .strip_prefix("Bearer ")
                .context("Expected MCP bearer token")?;
            let grant: McpDelegation = self.verify(token).context("Invalid MCP delegation")?;
            ensure!(
                grant.purpose == "ergo-mcp-edit-v1"
                    && grant.audience == self.public_url
                    && grant.expires > now()
                    && !grant.actor.is_empty(),
                "Invalid or expired MCP delegation"
            );
            return Ok(Principal {
                id: Some(grant.actor),
            });
        }
        Ok(self.principal(headers))
    }
    pub fn check_publish(&self, headers: &HeaderMap) -> Result<String> {
        self.check_request(headers)?;
        self.shared_principal(headers)?.required()
    }
    pub fn check_shared_edit(&self, headers: &HeaderMap) -> Result<String> {
        self.check_request(headers)?;
        let principal = self.shared_principal(headers)?;
        if let Some(value) = headers.get(header::AUTHORIZATION) {
            let token = value
                .to_str()?
                .strip_prefix("Bearer ")
                .context("Expected MCP bearer token")?;
            let grant: McpDelegation = self.verify(token)?;
            ensure!(
                grant.purpose == "ergo-mcp-edit-v1",
                "This older token authorizes publication only; issue a new shared-world edit token"
            );
        }
        principal.required()
    }
    fn delegate(&self, headers: &HeaderMap, lifetime: u64) -> Result<(String, u64)> {
        ensure!(
            !headers.contains_key(header::AUTHORIZATION),
            "Use a browser session to issue an MCP delegation"
        );
        let actor = self.check_write(headers)?;
        ensure!(
            (1..=86400).contains(&lifetime),
            "MCP delegation lifetime must be 1..86400 seconds"
        );
        let mut expires = now() + lifetime;
        if self.google {
            let session: Session =
                self.verify(cookie(headers, "ergo_session").context("Missing session")?)?;
            expires = expires.min(session.expires);
        }
        let token = self.sign(&McpDelegation {
            purpose: "ergo-mcp-edit-v1".into(),
            actor,
            audience: self.public_url.clone(),
            expires,
        })?;
        Ok((token, expires))
    }
    fn allows_origin(&self, value: &str, headers: &HeaderMap) -> bool {
        let Ok(origin) = http_origin(value) else {
            return false;
        };
        if origin.origin().ascii_serialization() == self.public_url {
            return true;
        }
        let Ok(public_url) = http_origin(&self.public_url) else {
            return false;
        };
        if self.google
            || origin.scheme() != public_url.scheme()
            || !is_loopback(&public_url)
            || !is_loopback(&origin)
        {
            return false;
        }
        // Local development may use another loopback name or port, but the
        // browser must still be posting to its own origin. Never trust forwarded
        // host headers or use the request Host to authorize a deployed origin.
        let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
            return false;
        };
        http_origin(&format!("{}://{host}", origin.scheme()))
            .is_ok_and(|request| request.origin() == origin.origin())
    }
    fn cookie(&self, name: &str, value: &str, age: u64) -> String {
        format!(
            "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{}",
            if self.public_url.starts_with("https://") {
                "; Secure"
            } else {
                ""
            }
        )
    }
    async fn verify_google(&self, token: &str, nonce: &str) -> Result<Claims> {
        let header = jsonwebtoken::decode_header(token)?;
        ensure!(
            header.alg == Algorithm::RS256,
            "Unexpected identity-token algorithm"
        );
        let kid = header.kid.context("Identity token has no key ID")?;
        let refresh = {
            let guard = self.keys.read().await;
            guard
                .as_ref()
                .is_none_or(|(expiry, keys)| *expiry < now() || keys.find(&kid).is_none())
        };
        if refresh {
            let keys: JwkSet = self
                .http
                .get("https://www.googleapis.com/oauth2/v3/certs")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            *self.keys.write().await = Some((now() + 3600, keys));
        }
        let guard = self.keys.read().await;
        let jwk = guard
            .as_ref()
            .context("No identity keys")?
            .1
            .find(&kid)
            .context("Unknown Google identity key")?;
        let key = DecodingKey::from_jwk(jwk)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.client_id]);
        validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 30;
        let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)?.claims;
        ensure!(
            claims.nonce == nonce && !claims.sub.is_empty(),
            "Invalid identity nonce or subject"
        );
        ensure!(
            claims.email_verified == Some(true),
            "Google email is not verified"
        );
        Ok(claims)
    }
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k == name).then_some(v)
        })
}

pub async fn login(
    State(s): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Result<Response, ApiError> {
    ensure_auth(s.auth.google)?;
    let login = Login {
        state: random(),
        nonce: random(),
        verifier: random(),
        expires: now() + 600,
        return_to: Some(s.auth.login_target(query.return_to.as_deref())?),
    };
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")?;
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", &s.auth.client_id),
        (
            "redirect_uri",
            &format!("{}/auth/google/callback", s.auth.public_url),
        ),
        ("scope", "openid email profile"),
        ("state", &login.state),
        ("nonce", &login.nonce),
        (
            "code_challenge",
            &B64.encode(Sha256::digest(login.verifier.as_bytes())),
        ),
        ("code_challenge_method", "S256"),
    ]);
    let mut response = Redirect::to(url.as_ref()).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&s.auth.cookie("ergo_login", &s.auth.sign(&login)?, 600))?,
    );
    Ok(response)
}
fn ensure_auth(enabled: bool) -> Result<()> {
    ensure!(
        enabled,
        "Google sign-in is not configured. Set GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET, ERGO_SESSION_SECRET, and ERGO_PUBLIC_URL"
    );
    Ok(())
}

pub async fn callback(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Result<Response, ApiError> {
    ensure_auth(s.auth.google)?;
    let login: Login = s
        .auth
        .verify(cookie(&headers, "ergo_login").context("Login state cookie is missing")?)?;
    if login.expires <= now()
        || query.state.as_deref() != Some(&login.state)
        || query.error.is_some()
    {
        return Err(anyhow::anyhow!("Google sign-in expired or was declined; start again").into());
    }
    let code = query.code.context("Missing authorization code")?;
    let response = s
        .auth
        .http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code.as_str()),
            ("client_id", &s.auth.client_id),
            ("client_secret", &s.auth.secret),
            (
                "redirect_uri",
                &format!("{}/auth/google/callback", s.auth.public_url),
            ),
            ("grant_type", "authorization_code"),
            ("code_verifier", &login.verifier),
        ])
        .send()
        .await?
        .error_for_status()?;
    let token: Token = response.json().await?;
    let claims = s.auth.verify_google(&token.id_token, &login.nonce).await?;
    let id = format!(
        "google-{}",
        hex::encode(Sha256::digest(claims.sub.as_bytes()))
    );
    let previous = s.store.user(&id).await?;
    let user = User {
        id: id.clone(),
        name: claims.name.unwrap_or_else(|| "Brick builder".into()),
        email: claims.email.unwrap_or_default(),
        picture: claims.picture,
        created_at: previous.map(|u| u.created_at).unwrap_or_else(now),
        last_login: now(),
    };
    s.store.put_user(&user).await?;
    let destination = s.auth.login_target(login.return_to.as_deref())?;
    let mut response = Redirect::to(&destination).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&s.auth.cookie(
            "ergo_session",
            &s.auth.sign(&Session {
                user_id: id,
                expires: now() + 7 * 86400,
            })?,
            7 * 86400,
        ))?,
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&s.auth.cookie("ergo_login", "", 0))?,
    );
    Ok(response)
}

pub async fn me(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let principal = s.auth.principal(&headers);
    let user = if let Some(id) = &principal.id {
        s.store.user(id).await?
    } else {
        None
    };
    let edit_allowance = if let Some(id) = &principal.id {
        Some(s.shared.pacing.status(id).await?)
    } else {
        None
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(
        serde_json::json!({"id":principal.id,"name":user.as_ref().map(|u|u.name.as_str()).unwrap_or("Guest"),"google_enabled":s.auth.google,"signed_in":user.is_some(),"development":!s.auth.google,"edit_allowance":edit_allowance,"common_world":s.shared.common_world}),
    )).into_response())
}

pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    Ok(logout_response(&s.auth, &headers)?)
}

fn logout_response(auth: &Auth, headers: &HeaderMap) -> Result<Response> {
    // Clearing an expired/missing session must succeed too. Same-origin checks
    // still apply, but sign-out does not require a valid signed-in identity.
    auth.check_request(headers)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth.cookie("ergo_session", "", 0))?,
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&auth.cookie("ergo_login", "", 0))?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "Clear-Site-Data",
        HeaderValue::from_static("\"cache\", \"storage\""),
    );
    Ok(response)
}

pub async fn mcp_token(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DelegationRequest>,
) -> Result<Response, ApiError> {
    let (token, expires) = s.auth.delegate(&headers, request.expires_in_seconds)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            serde_json::json!({"token":token,"expires_at":expires,"audience":s.auth.public_url,
            "scope":"shared-worlds:read shared-worlds:create assemblies:import geometry:commit"}),
        ),
    )
        .into_response())
}

pub async fn seed_development_user(store: &crate::store::Store) -> Result<()> {
    if store.user("development").await?.is_none() {
        store
            .put_user(&User {
                id: "development".into(),
                name: "Local development".into(),
                email: String::new(),
                picture: None,
                created_at: now(),
                last_login: now(),
            })
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sso_return_paths_preserve_world_links_without_open_redirects() {
        let (auth, _) = Auth::test_session("alice");
        assert_eq!(
            auth.login_target(Some("/app?shared_world=example&x=1"))
                .unwrap(),
            "/app?shared_world=example&x=1"
        );
        assert_eq!(
            auth.login_target(Some("/login#connect")).unwrap(),
            "/login#connect"
        );
        for target in [
            "//attacker.example/app",
            "https://attacker.example/app",
            "/api/auth/mcp-token",
        ] {
            assert!(auth.login_target(Some(target)).is_err());
        }
    }

    #[test]
    fn mcp_delegations_expire_are_audience_bound_and_cannot_become_sessions() {
        let (mut auth, session) = Auth::test_session("alice");
        let (token, expires) = auth.delegate(&session, 3600).unwrap();
        let session_value: Session = auth
            .verify(cookie(&session, "ergo_session").unwrap())
            .unwrap();
        assert_eq!(expires, session_value.expires);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        assert_eq!(
            auth.shared_principal(&headers).unwrap().id.as_deref(),
            Some("alice")
        );
        assert!(auth.check_publish(&headers).is_err());
        headers.insert("X-Ergo-Request", HeaderValue::from_static("1"));
        assert_eq!(auth.check_publish(&headers).unwrap(), "alice");
        assert!(auth.delegate(&headers, 60).is_err());
        assert!(auth.delegate(&session, 86401).is_err());

        let mut cookie_headers = HeaderMap::new();
        cookie_headers.insert(
            header::COOKIE,
            format!("ergo_session={token}").parse().unwrap(),
        );
        assert!(auth.principal(&cookie_headers).id.is_none());
        let original = auth.public_url.clone();
        auth.public_url = "https://other.example".into();
        assert!(auth.shared_principal(&headers).is_err());
        auth.public_url = original;
        for (purpose, expires) in [("ergo-mcp-publish-v1", 0), ("other-purpose", now() + 60)] {
            let token = auth
                .sign(&McpDelegation {
                    purpose: purpose.into(),
                    actor: "alice".into(),
                    audience: auth.public_url.clone(),
                    expires,
                })
                .unwrap();
            headers.insert(
                header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            assert!(auth.shared_principal(&headers).is_err());
        }
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer x{token}").parse().unwrap(),
        );
        assert!(auth.shared_principal(&headers).is_err());
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", cookie(&session, "ergo_session").unwrap())
                .parse()
                .unwrap(),
        );
        assert!(auth.shared_principal(&headers).is_err());
    }

    #[test]
    fn development_writes_accept_same_origin_loopback_names_and_ports() {
        let (mut auth, mut headers) = Auth::test_session("u");
        auth.google = false;
        auth.public_url = "http://127.0.0.1:3000".into();
        for (origin, host) in [
            ("http://127.0.0.1:3000", "127.0.0.1:3000"),
            ("http://localhost:3000", "localhost:3000"),
            ("http://127.0.0.1:3100", "127.0.0.1:3100"),
            ("http://[::1]:3100", "[::1]:3100"),
        ] {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            headers.insert(header::HOST, host.parse().unwrap());
            assert_eq!(
                auth.check_write(&headers).unwrap(),
                "development",
                "{origin}"
            );
        }
    }

    #[test]
    fn development_writes_reject_cross_origin_and_malformed_requests() {
        let (mut auth, mut headers) = Auth::test_session("u");
        auth.google = false;
        auth.public_url = "http://127.0.0.1:3000".into();
        for (origin, host) in [
            ("http://localhost:3000", "127.0.0.1:3000"),
            ("http://localhost:3100", "localhost:3000"),
            ("https://localhost:3000", "localhost:3000"),
            ("http://attacker.example:3000", "attacker.example:3000"),
            ("http://localhost.attacker.example:3000", "localhost:3000"),
            ("http://192.168.1.10:3000", "192.168.1.10:3000"),
            ("http://localhost:3000/path", "localhost:3000"),
            ("http://localhost:3000", "localhost:3000/path"),
            ("null", "localhost:3000"),
        ] {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            headers.insert(header::HOST, host.parse().unwrap());
            let error = auth.check_write(&headers).unwrap_err().to_string();
            assert!(error.contains("ERGO_PUBLIC_URL"), "{origin}: {error}");
        }
        headers.insert(header::ORIGIN, "http://localhost:3000".parse().unwrap());
        headers.remove(header::HOST);
        headers.insert("X-Forwarded-Host", "localhost:3000".parse().unwrap());
        assert!(auth.check_write(&headers).is_err());
    }

    #[test]
    fn deployed_writes_require_the_configured_origin() {
        let (mut auth, mut headers) = Auth::test_session("u");
        headers.insert(header::HOST, "127.0.0.1:3000".parse().unwrap());
        headers.insert(header::ORIGIN, "http://127.0.0.1:3000".parse().unwrap());
        // Google is enabled, so even loopback aliases must match the public URL.
        assert!(auth.check_write(&headers).is_err());
        headers.insert(header::ORIGIN, "http://localhost:3000".parse().unwrap());
        assert_eq!(auth.check_write(&headers).unwrap(), "u");

        auth.google = false;
        auth.public_url = "https://ergo.example".into();
        for (origin, host) in [
            ("http://localhost:3000", "localhost:3000"),
            ("https://attacker.example", "attacker.example"),
            ("https://ergo.example:444", "ergo.example:444"),
        ] {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            headers.insert(header::HOST, host.parse().unwrap());
            assert!(auth.check_write(&headers).is_err(), "{origin}");
        }
        headers.insert(header::ORIGIN, "https://ERGO.example:443".parse().unwrap());
        assert_eq!(auth.check_write(&headers).unwrap(), "development");
    }

    #[test]
    fn writes_still_require_custom_header_and_identity() {
        let (auth, mut headers) = Auth::test_session("u");
        // Native clients do not need to send Origin.
        assert_eq!(auth.check_write(&headers).unwrap(), "u");
        headers.remove("X-Ergo-Request");
        assert!(auth.check_write(&headers).is_err());
        headers.insert("X-Ergo-Request", HeaderValue::from_static("1"));
        headers.remove(header::COOKIE);
        assert!(auth.check_write(&headers).is_err());
    }

    #[test]
    fn logout_clears_both_cookies_and_is_idempotent_without_a_session() {
        let (mut auth, mut headers) = Auth::test_session("u");
        auth.public_url = "https://ergo.example".into();
        headers.insert(header::ORIGIN, "https://ergo.example".parse().unwrap());
        for cookie in [Some("ergo_session=expired"), None] {
            if let Some(cookie) = cookie {
                headers.insert(header::COOKIE, cookie.parse().unwrap());
            } else {
                headers.remove(header::COOKIE);
            }
            let response = logout_response(&auth, &headers).unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            let cookies = response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .map(|value| value.to_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(cookies.len(), 2);
            for name in ["ergo_session=", "ergo_login="] {
                assert!(cookies.iter().any(|cookie| cookie.starts_with(name)
                    && cookie.contains("Max-Age=0")
                    && cookie.contains("Path=/")
                    && cookie.contains("HttpOnly")
                    && cookie.contains("Secure")));
            }
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
        headers.insert(header::ORIGIN, "https://attacker.example".parse().unwrap());
        assert!(logout_response(&auth, &headers).is_err());
        headers.remove(header::ORIGIN);
        headers.remove("X-Ergo-Request");
        assert!(logout_response(&auth, &headers).is_err());
    }

    #[test]
    fn http_origins_are_normalized_and_reject_non_origin_components() {
        assert_eq!(
            http_origin("https://ERGO.example:443/")
                .unwrap()
                .origin()
                .ascii_serialization(),
            "https://ergo.example"
        );
        for value in [
            "null",
            "file:///tmp/model.ldr",
            "https://user:secret@ergo.example",
            "https://ergo.example/path",
            "https://ergo.example?query",
            "https://ergo.example#fragment",
        ] {
            assert!(http_origin(value).is_err(), "{value}");
        }
    }

    #[test]
    fn signed_session_rejects_tampering_and_bad_expiry() {
        let auth = Auth {
            google: true,
            client_id: String::new(),
            secret: String::new(),
            signing: vec![42; 32],
            public_url: "http://localhost:3000".into(),
            http: reqwest::Client::new(),
            keys: Arc::new(RwLock::new(None)),
        };
        let value = auth
            .sign(&Session {
                user_id: "u".into(),
                expires: now() + 60,
            })
            .unwrap();
        assert!(auth.verify::<Session>(&format!("x{value}")).is_err());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("ergo_session={value}").parse().unwrap(),
        );
        assert_eq!(auth.principal(&headers).id, Some("u".into()));
        let expired = auth
            .sign(&Session {
                user_id: "u".into(),
                expires: 0,
            })
            .unwrap();
        headers.insert(
            header::COOKIE,
            format!("ergo_session={expired}").parse().unwrap(),
        );
        assert!(auth.principal(&headers).id.is_none());
    }
}
