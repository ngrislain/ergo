//! Throttled private position synchronization for authenticated agent placement.
use super::*;
use ergo_world::user_position::PositionUpdate;

pub(super) struct Presence {
    session: String,
    sequence: u64,
    last_attempt: f64,
    pending: bool,
    last_sent: Option<PositionUpdate>,
}
impl Default for Presence {
    fn default() -> Self {
        Self {
            session: format!(
                "{:x}-{:x}",
                js_sys::Date::now() as u64,
                (js_sys::Math::random() * 4_503_599_627_370_496.) as u64
            ),
            sequence: 0,
            last_attempt: 0.,
            pending: false,
            last_sent: None,
        }
    }
}
fn capture(c: &mut Client, force: bool) -> Option<PositionUpdate> {
    if !c.ready || !c.ui.signed_in.get_untracked() || c.ui.signing_out.get_untracked() {
        return None;
    }
    let world_id = c.ui.common_world.get_untracked()?;
    let renderer = c.renderer.as_ref()?;
    let position_world = ergo_world::coordinates::public(renderer.eye()).to_array();
    let view_target_world = ergo_world::coordinates::public(renderer.target).to_array();
    let navigation = renderer.navigation.mode().name().to_owned();
    let now = js_sys::Date::now();
    let state = &mut c.presence;
    if !force && (state.pending || now - state.last_attempt < 3000.) {
        return None;
    }
    if !force
        && state.last_sent.as_ref().is_some_and(|old| {
            old.world_id == world_id
                && old.position_world == position_world
                && old.view_target_world == view_target_world
                && old.navigation == navigation
        })
    {
        return None;
    }
    state.sequence += 1;
    state.last_attempt = now;
    state.pending = true;
    Some(PositionUpdate {
        world_id,
        position_world,
        view_target_world,
        navigation,
        client_session: state.session.clone(),
        sequence: state.sequence,
    })
}
pub(super) fn pump(handle: Handle) {
    let Some(update) = with_client(handle, |c| capture(c, false)) else {
        return;
    };
    spawn_local(async move {
        let result = async {
            Request::put("/api/user-position")
                .header("X-Ergo-Request", "1")
                .json(&update)?
                .send()
                .await
        }
        .await;
        with_client(handle, |c| {
            c.presence.pending = false;
            if result.is_ok_and(|response| response.ok()) {
                c.presence.last_sent = Some(update);
            }
        });
    });
}
pub(super) fn flush(handle: Handle) {
    let Some(update) = with_client(handle, |c| capture(c, true)) else {
        return;
    };
    let Ok(body) = serde_json::to_string(&update) else {
        return;
    };
    let init = web_sys::RequestInit::new();
    init.set_method("PUT");
    let _ = js_sys::Reflect::set(
        init.as_ref(),
        &JsValue::from_str("keepalive"),
        &JsValue::TRUE,
    );
    init.set_body(&JsValue::from_str(&body));
    let Ok(headers) = web_sys::Headers::new() else {
        return;
    };
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Ergo-Request", "1");
    init.set_headers(&headers);
    if let Some(window) = web_sys::window() {
        let _ = window.fetch_with_str_and_init("/api/user-position", &init);
    }
}
pub(super) fn install(handle: Handle) {
    let hidden =
        leptos::leptos_dom::helpers::window_event_listener_untyped("visibilitychange", move |_| {
            if web_sys::window()
                .and_then(|w| w.document())
                .is_some_and(|d| d.hidden())
            {
                flush(handle);
            }
        });
    let leaving =
        leptos::leptos_dom::helpers::window_event_listener_untyped("pagehide", move |_| {
            flush(handle)
        });
    on_cleanup(move || {
        hidden.remove();
        leaving.remove();
    });
}
