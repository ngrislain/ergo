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
struct Login {
    state: String,
    nonce: String,
    verifier: String,
    expires: u64,
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
        let parsed = url::Url::parse(&public_url)?;
        ensure!(
            ["https", "http"].contains(&parsed.scheme())
                && parsed.path() == "/"
                && parsed.query().is_none(),
            "ERGO_PUBLIC_URL must be an HTTP(S) origin"
        );
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
            public_url,
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
        ensure!(
            headers.get("X-Ergo-Request").and_then(|h| h.to_str().ok()) == Some("1"),
            "Missing same-origin request header"
        );
        if let Some(origin) = headers.get(header::ORIGIN) {
            ensure!(
                origin.to_str()? == self.public_url,
                "Unexpected request origin"
            );
        }
        self.principal(headers).required()
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

pub async fn login(State(s): State<AppState>) -> Result<Response, ApiError> {
    ensure_auth(s.auth.google)?;
    let login = Login {
        state: random(),
        nonce: random(),
        verifier: random(),
        expires: now() + 600,
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
    let mut response = Redirect::to("/").into_response();
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

pub async fn me(
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let principal = s.auth.principal(&headers);
    let user = if let Some(id) = &principal.id {
        s.store.user(id).await?
    } else {
        None
    };
    Ok(Json(
        serde_json::json!({"id":principal.id,"name":user.as_ref().map(|u|u.name.as_str()).unwrap_or("Guest"),"google_enabled":s.auth.google,"signed_in":user.is_some(),"development":!s.auth.google}),
    ))
}

pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    s.auth.check_write(&headers)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&s.auth.cookie("ergo_session", "", 0))?,
    );
    response.headers_mut().insert(
        "Clear-Site-Data",
        HeaderValue::from_static("\"cache\", \"storage\""),
    );
    Ok(response)
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
