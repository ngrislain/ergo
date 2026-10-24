//! Shared sign-out path for the viewer and token/setup page.
use gloo_net::http::Request;

pub(crate) async fn sign_out() -> anyhow::Result<()> {
    let response = Request::post("/api/auth/logout")
        .header("X-Ergo-Request", "1")
        .send()
        .await?;
    anyhow::ensure!(response.ok(), "Could not sign out; please try again");
    // The server expires both auth cookies and clears origin storage/cache.
    // Do not wait on CacheStorage operations before leaving the authenticated UI.
    web_sys::window()
        .unwrap()
        .location()
        .replace("/login")
        .map_err(|_| anyhow::anyhow!("Signed out; open /login to continue"))?;
    Ok(())
}
