//! Signing in to Modrinth: the player's browser opens modrinth.com, and the
//! site hands an encrypted session back to a loopback port Calcite listens on.
//! Calcite never sees the password.

use std::time::Duration;

use calcite_core::modrinth_auth::{Reply, Seal, request_url};
use gpui_kit::App;
use theseus::mr_auth::ModrinthAuthFlow;

use crate::backend::Backend;
use crate::state::AppState;

/// How long Calcite waits for the browser before giving up.
const WAIT: Duration = Duration::from_secs(10 * 60);

pub fn sign_in(cx: &mut App) {
    let state = AppState::global(cx);
    if state.read(cx).modrinth_signing_in.is_some() {
        return;
    }
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    state.update(cx, |s, cx| {
        s.modrinth_signing_in = Some(cancel_tx);
        cx.notify();
    });
    let backend = Backend::global(cx).clone();
    let prepare = backend.run(async move {
        let begin = theseus::mr_auth::authenticate_begin_flow(ModrinthAuthFlow::SignIn).await?;
        let seal = Seal::generate();
        let reply = Reply::bind(seal.nonce()).await?;
        let url = request_url(begin, reply.addr()?, &seal)?;
        anyhow::Ok((seal, reply, url))
    });
    cx.spawn(async move |cx| {
        let result: anyhow::Result<bool> = async {
            let (seal, reply, url) = prepare.await??;
            cx.update(|cx| cx.open_url(&url));
            let code = backend
                .run(async move { reply.wait(cancel_rx, WAIT).await })
                .await??;
            let Some(code) = code else {
                return Ok(false);
            };
            let token = seal.open(&code)?;
            backend
                .call(async move { theseus::mr_auth::authenticate_finish_flow(&token).await })
                .await?;
            Ok(true)
        }
        .await;
        state.update(cx, |s, cx| {
            s.modrinth_signing_in = None;
            match result {
                Ok(true) => {
                    s.refresh_modrinth(cx);
                    cx.activate(true);
                }
                Ok(false) => {}
                Err(error) => s.notify_error("Couldn't sign in to Modrinth", format!("{error:#}")),
            }
            cx.notify();
        });
    })
    .detach();
}

/// Stop waiting for the browser.
pub fn cancel(cx: &mut App) {
    AppState::global(cx).update(cx, |s, cx| {
        if let Some(cancel) = s.modrinth_signing_in.take() {
            let _ = cancel.send(());
        }
        cx.notify();
    });
}

pub fn sign_out(cx: &mut App) {
    let state = AppState::global(cx);
    let task = Backend::global(cx).call(async { theseus::mr_auth::logout().await });
    cx.spawn(async move |cx| {
        let result = task.await;
        state.update(cx, |s, cx| {
            match result {
                Ok(()) => {
                    s.modrinth = None;
                    s.friends.clear();
                }
                Err(error) => s.notify_error("Couldn't sign out of Modrinth", format!("{error:#}")),
            }
            cx.notify();
        });
    })
    .detach();
}
