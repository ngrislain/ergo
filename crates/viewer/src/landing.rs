use gloo_net::http::Request;
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{JsFuture, spawn_local};

#[derive(Clone, Deserialize)]
struct Account {
    id: Option<String>,
    name: String,
    google_enabled: bool,
    signed_in: bool,
    development: bool,
    edit_allowance: Option<serde_json::Value>,
    #[serde(default)]
    common_world: Option<String>,
}

#[component]
pub fn Entry() -> impl IntoView {
    let account = RwSignal::new(None::<Account>);
    let error = RwSignal::new(None::<String>);
    Effect::new(move |_| {
        spawn_local(async move {
            match Request::get("/api/auth/me").send().await {
                Ok(response) if response.ok() => match response.json::<Account>().await {
                    Ok(value) => account.set(Some(value)),
                    Err(e) => error.set(Some(e.to_string())),
                },
                Ok(_) => error.set(Some(
                    "Could not check your session. Reload to try again.".into(),
                )),
                Err(e) => error.set(Some(e.to_string())),
            }
        })
    });
    let pathname = web_sys::window()
        .and_then(|w| w.location().pathname().ok())
        .unwrap_or_default();
    let workspace = pathname == "/app";
    let home = pathname == "/";
    if pathname == "/login" {
        match mcp_callback() {
            Ok(None) => {}
            callback => return view! { <McpLogin account error callback/> }.into_any(),
        }
    }
    view! {
        <Show when=move ||account.get().is_some_and(|a|(workspace&&(a.signed_in||a.development||a.common_world.is_some()))||(home&&(a.common_world.is_some()||(a.signed_in&&!a.development))))
            fallback=move ||view!{<Landing account error/>}>
            <crate::app::App/>
        </Show>
    }.into_any()
}

fn google_login_url() -> String {
    let return_path = web_sys::window()
        .map(|w| {
            format!(
                "{}{}{}",
                w.location().pathname().unwrap_or_default(),
                w.location().search().unwrap_or_default(),
                w.location().hash().unwrap_or_default()
            )
        })
        .unwrap_or_else(|| "/".into());
    format!(
        "/auth/google?return_to={}",
        js_sys::encode_uri_component(&return_path)
            .as_string()
            .unwrap_or_default()
    )
}

#[component]
fn McpLogin(
    account: RwSignal<Option<Account>>,
    error: RwSignal<Option<String>>,
    callback: Result<Option<String>, JsValue>,
) -> impl IntoView {
    let login_url = StoredValue::new(google_login_url());
    let destination = StoredValue::new(callback.ok().flatten());
    let valid = destination.get_value().is_some();
    let busy = RwSignal::new(false);
    let signed_in = move || account.get().is_some_and(|a| a.signed_in || a.development);
    let approve = move |_| {
        if busy.get_untracked() || !signed_in() {
            return;
        }
        let Some(destination) = destination.get_value() else {
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = async {
                let (token, _) = issue_token().await?;
                return_mcp_token(&destination, &token)
                    .map_err(|_| anyhow::anyhow!("Could not send approval. Restart Ergo in your agent and open the new sign-in link."))
            }.await;
            if let Err(e) = result {
                error.set(Some(e.to_string()));
                busy.set(false);
            }
        });
    };
    view! {
        <header class="app-header landing-header">
            <a class="brand" href="/app" aria-label="Ergo home"><img src="/favicon.svg" alt="" width="38" height="38"/>"ergo"</a>
        </header>
        <main class="mcp-login landing-intro" aria-labelledby="mcp-login-title">
            <h1 id="mcp-login-title">"Connect Ergo to your agent"</h1>
            <Show when=move ||valid fallback=||view!{
                <p class="onboarding-error" role="alert">"Invalid MCP callback. Restart Ergo in your agent and open the new sign-in link."</p>
            }>
                <p>"Only approve if you just started Ergo in your agent on this device. Approval lets the agent edit constructions using your account’s permissions."</p>
                <Show when=signed_in fallback=move ||view!{
                    <Show when=move ||account.get().is_some_and(|a|a.google_enabled) fallback=move ||view!{
                        <Show when=move ||error.get().is_none()>
                            <p role="status">{move ||if account.get().is_none(){"Checking your session…"}else{"Google sign-in is not available on this server."}}</p>
                        </Show>
                    }>
                        <a class="button primary" href=login_url.get_value()>"Continue with Google"</a>
                        <p class="landing-note">"You’ll return here to review and approve the connection."</p>
                    </Show>
                }>
                    <p class="signed-in-name">{move ||account.get().map(|a|if a.signed_in {format!("Signed in as {}", a.name)} else {"Development session".into()}).unwrap_or_default()}</p>
                    <button class="button primary" disabled=move ||busy.get() on:click=approve>{move ||if busy.get(){"Sending approval…"}else{"Authorize MCP server"}}</button>
                    <p class="landing-note">"After approval, return to your agent. The token is sent directly to Ergo on this device."</p>
                </Show>
                <Show when=move ||error.get().is_some()><p class="onboarding-error" role="alert">{move ||error.get().unwrap_or_default()}</p></Show>
            </Show>
            <a class="text-button" href="/login#connect">"Use manual token setup instead"</a>
        </main>
    }
}

#[component]
fn Landing(account: RwSignal<Option<Account>>, error: RwSignal<Option<String>>) -> impl IntoView {
    let signing_out = RwSignal::new(false);
    let logout = move |_| {
        if signing_out.get_untracked() {
            return;
        }
        signing_out.set(true);
        spawn_local(async move {
            if let Err(e) = crate::session::sign_out().await {
                error.set(Some(e.to_string()));
                signing_out.set(false);
            }
        });
    };
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default();
    let login_url = StoredValue::new(google_login_url());
    let mut environment = serde_json::json!({"ERGO_PUBLISH_URL":origin,"ERGO_PUBLISH_TOKEN_FILE":"/absolute/path/ergo-mcp-token"});
    let query = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|q| web_sys::UrlSearchParams::new_with_str(&q).ok());
    if let Some(query) = query {
        let position =
            ["x", "y", "z"].map(|key| query.get(key).and_then(|v| v.parse::<f64>().ok()));
        if let [Some(x), Some(y), Some(z)] = position {
            if [x, y, z]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= ergo_world::MAX_WORLD_COORD)
            {
                environment["ERGO_POSITION_WORLD"] =
                    serde_json::json!(serde_json::to_string(&[x, y, z]).unwrap_or_default());
            }
        }
        if let Some(assembly) = query.get("assembly") {
            if assembly.len() == 64 && assembly.bytes().all(|b| b.is_ascii_hexdigit()) {
                environment["ERGO_SHARED_ASSEMBLY_ID"] = serde_json::json!(assembly);
            }
        }
    }
    let mcp = serde_json::to_string_pretty(&serde_json::json!({"mcpServers":{"ergo":{
        "command":"ergo-mcp","args":["--workspace","/absolute/path/construction.ergo.json"],
        "env":environment
    }}}))
    .unwrap();
    let api = format!(
        r#"curl --fail '{origin}/api/shared-worlds/allowance' \
  -H "Authorization: Bearer $ERGO_TOKEN"

curl --fail '{origin}/api/shared-worlds?quad=&limit=32' \
  -H "Authorization: Bearer $ERGO_TOKEN"

# For POST/PUT requests, also send:
# X-Ergo-Request: 1
# Content-Type: application/json"#
    );
    view! {
        <header class="app-header landing-header">
            <a class="brand" href="https://www.ergosandbox.com/" aria-label="Ergo home"><img src="/favicon.svg" alt="" width="38" height="38"/>"ergo"</a>
            <nav aria-label="Main navigation"><a href="https://www.ergosandbox.com/">"Project guide"</a><a href="#connect">"Agent access"</a><Show when=move ||account.get().is_some_and(|a|a.signed_in||a.development)><a class="button primary" href="/app">"Open the world"</a></Show><Show when=move ||account.get().is_some_and(|a|a.signed_in)><button class="button quiet" disabled=move ||signing_out.get() aria-busy=move ||signing_out.get().to_string() on:click=logout>{move ||if signing_out.get(){"Signing out…"}else{"Sign out"}}</button></Show></nav>
        </header>
        <main class="landing-page">
            <section class="landing-hero login-hero" aria-labelledby="landing-title">
                <div class="landing-intro"><h1 id="landing-title">"Sign in to your sandbox."</h1>
                    <p>"Open the construction viewer or give an agent permission to build. The project guide and quickstart are on www.ergosandbox.com."</p>
                    <Show when=move ||account.get().is_some_and(|a|a.signed_in||a.development) fallback=move ||view!{
                        <Show when=move ||account.get().is_some_and(|a|a.google_enabled) fallback=move ||view!{<p class="subtle">{move ||if account.get().is_none(){"Checking your session…"}else{"Google sign-in is not configured on this server."}}</p>}>
                            <a class="button primary sso-button" href=login_url.get_value()>"Continue with Google"</a><p class="landing-note">"Sign in to edit shared constructions and create an MCP or API token."</p>
                        </Show>
                    }>
                        <p class="signed-in-name">{move ||format!("Welcome, {}.",account.get().map(|a|a.name).unwrap_or_default())}</p>
                        <a class="button primary" href="/app">"Open the world"</a>
                        <a class="button quiet" href="#connect">"Set up MCP / API access"</a>
                    </Show>
                    <Show when=move ||error.get().is_some()><p class="onboarding-error" role="alert">{move ||error.get().unwrap_or_default()}</p></Show>
                </div>
            </section>
            <section id="connect" class="connection-section" aria-labelledby="connect-title">
                <div class="connection-copy"><h2 id="connect-title">"Connect your agent."</h2><p>"Run the MCP bridge beside your agent. Rendering runs on your device; accepted production edits commit to the common world in S3 under your signed-in identity. The local file is a cache."</p>
                    <ol><li>"Install the prebuilt npm package using the project guide."</li><li>"Generate a token below and download it as ergo-mcp-token."</li><li>"Add this MCP configuration to your agent, using your actual local file paths."</li></ol>
                    <a class="text-button" href="https://www.ergosandbox.com/#connect">"Install the MCP server with npm"</a>
                    <p class="landing-note">"The package includes the native bridge and WASM assets. Your MCP client starts ergo-mcp automatically; no Rust build is needed."</p>
                    <p class="landing-note">"Start with standard bricks and colors; non-standard Studio/LDraw parts are resolved by ID. Publishing requires sourced physical measurements."</p>
                </div><pre class="connection-code"><code>{mcp}</code></pre>
            </section>
            <TokenPanel account/>
            <section id="api-access" class="connection-section" aria-labelledby="api-title"><div class="connection-copy"><h2 id="api-title">"Use the API directly."</h2><p>"Use the same token as a Bearer credential. Shared-world reads, creation, imports, and geometry commits use your current permissions and edit allowance."</p><p>"The token cannot change grants, retention, or account plans. The server validates each publication; a local export is not a remote commit."</p><a class="text-button" href="/api/config" target="_blank" rel="noopener">"View this server’s configuration"</a></div><pre class="connection-code"><code>{api}</code></pre></section>
        </main>
        <footer class="landing-footer"><span>"ergo — a world, brick by brick"</span><a href="https://www.ergosandbox.com/privacy/">"Privacy"</a><a href="https://www.ergosandbox.com/terms/">"Terms"</a><a href="/app">"Explore the world"</a></footer>
    }
}

#[component]
fn TokenPanel(account: RwSignal<Option<Account>>) -> impl IntoView {
    let token = RwSignal::new(None::<String>);
    let expires = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let copied = RwSignal::new(false);
    let generate = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        copied.set(false);
        spawn_local(async move {
            match issue_token().await {
                Ok((value, expires_at)) => {
                    token.set(Some(value));
                    let date = js_sys::Date::new(&JsValue::from_f64(expires_at * 1000.));
                    expires.set(date.to_iso_string().as_string().unwrap_or_default());
                }
                Err(e) => error.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };
    view! {
        <section class="token-panel" aria-labelledby="token-title"><div><h2 id="token-title">"Your MCP and API access token"</h2>
            <p class="account-id">{move ||account.get().and_then(|a|a.id).map(|id|format!("Account: {id}")).unwrap_or_default()}</p>
            <p>"Tokens last 365 days, independently of your browser session. S3 credentials stay on the server."</p>
            <p class="allowance-label">{move ||account.get().and_then(|a|a.edit_allowance).map(allowance_label).unwrap_or_else(||"Sign in to see your edit allowance.".into())}</p>
        </div><div class="token-actions">
            <button class="button primary" disabled=move ||busy.get()||!account.get().is_some_and(|a|a.signed_in||a.development) on:click=generate>{move ||if busy.get(){"Generating…"}else{"Generate access token"}}</button>
            <Show when=move ||token.get().is_some()><label>"Access token"<textarea readonly aria-label="MCP and API token" prop:value=move ||token.get().unwrap_or_default()/></label>
                <p class="subtle">{move ||format!("Expires {}. Sign-out does not revoke already-issued tokens.",expires.get())}</p>
                <div class="token-buttons"><button class="button quiet" on:click=move |_|{
                    if let Some(value)=token.get_untracked(){spawn_local(async move {
                        let promise=web_sys::window().unwrap().navigator().clipboard().write_text(&value);
                        match JsFuture::from(promise).await {Ok(_)=>copied.set(true),Err(_)=>error.set(Some("Clipboard access was denied. Select and copy the token, or download it.".into()))}
                    });}
                }>{move ||if copied.get(){"Copied"}else{"Copy token"}}</button>
                <button class="button quiet" on:click=move |_|{if let Some(value)=token.get_untracked() && let Err(e)=download_token(&value){error.set(Some(format!("Could not download token: {e:?}")));}}>"Download MCP token"</button></div>
            </Show>
            <Show when=move ||error.get().is_some()><p class="onboarding-error" role="alert">{move ||error.get().unwrap_or_default()}</p></Show>
        </div></section>
    }
}

async fn issue_token() -> anyhow::Result<(String, f64)> {
    let response = Request::post("/api/auth/mcp-token")
        .header("X-Ergo-Request", "1")
        .json(&serde_json::json!({}))?
        .send()
        .await?;
    let value: serde_json::Value = response.json().await?;
    anyhow::ensure!(
        response.ok(),
        "{}",
        value["error"]
            .as_str()
            .unwrap_or("Could not issue an access token.")
    );
    let token = value["token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Ergo did not return an access token."))?;
    Ok((
        token.into(),
        value["expires_at"].as_f64().unwrap_or_default(),
    ))
}

fn mcp_callback() -> Result<Option<String>, JsValue> {
    let query = web_sys::window().unwrap().location().search()?;
    let Some(value) = web_sys::UrlSearchParams::new_with_str(&query)?.get("mcp_callback") else {
        return Ok(None);
    };
    let url = web_sys::Url::new(&value)?;
    let path = url.pathname();
    let capability = path.strip_prefix("/callback/").unwrap_or_default();
    if url.protocol() != "http:"
        || url.hostname() != "127.0.0.1"
        || url
            .port()
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .is_none()
        || !url.username().is_empty()
        || !url.password().is_empty()
        || !url.search().is_empty()
        || !url.hash().is_empty()
        || capability.len() != 64
        || !capability.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(JsValue::from_str(
            "Expected a one-time loopback MCP callback",
        ));
    }
    Ok(Some(url.href()))
}

fn return_mcp_token(destination: &str, token: &str) -> Result<(), JsValue> {
    let document = web_sys::window().unwrap().document().unwrap();
    // For an HTTPS-to-HTTP navigation POST Chromium sends Origin: null under
    // the default strict policy, even to loopback. Send only the origin so
    // the bridge can validate it without disclosing the sign-in URL.
    let policy = document.create_element("meta")?;
    policy.set_attribute("name", "referrer")?;
    policy.set_attribute("content", "origin")?;
    document
        .query_selector("head")?
        .unwrap()
        .append_child(&policy)?;
    let form = document
        .create_element("form")?
        .dyn_into::<web_sys::HtmlFormElement>()?;
    form.set_method("post");
    form.set_action(destination);
    let input = document
        .create_element("input")?
        .dyn_into::<web_sys::HtmlInputElement>()?;
    input.set_type("hidden");
    input.set_name("token");
    input.set_value(token);
    form.append_child(&input)?;
    document.body().unwrap().append_child(&form)?;
    // Top-level form POST avoids CORS and never puts credentials in the URL.
    form.submit()?;
    form.remove();
    Ok(())
}

fn download_token(token: &str) -> Result<(), JsValue> {
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(token));
    let blob = web_sys::Blob::new_with_str_sequence(&parts)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let document = web_sys::window().unwrap().document().unwrap();
    let anchor = document
        .create_element("a")?
        .dyn_into::<web_sys::HtmlAnchorElement>()?;
    anchor.set_href(&url);
    anchor.set_download("ergo-mcp-token");
    document.body().unwrap().append_child(&anchor)?;
    anchor.click();
    anchor.remove();
    gloo_timers::callback::Timeout::new(1000, move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    })
    .forget();
    Ok(())
}

fn allowance_label(value: serde_json::Value) -> String {
    if value["unlimited"] == true {
        return "Unlimited edit pacing".into();
    }
    let seconds = value["refill_period_seconds"].as_u64().unwrap_or_default();
    let period = if seconds > 0 && seconds.is_multiple_of(86400) {
        format!("{} day(s)", seconds / 86400)
    } else if seconds > 0 && seconds.is_multiple_of(3600) {
        format!("{} hour(s)", seconds / 3600)
    } else {
        format!("{seconds} seconds")
    };
    format!(
        "{} brick changes available; {} refill per {period}",
        value["remaining_brick_changes"], value["capacity"]
    )
}
