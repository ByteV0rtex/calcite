//! Microsoft sign-in.
//!
//! The backend builds the authorize URL. A small helper process
//! (`calcite-login`) shows Microsoft's page in a native web view and prints
//! the redirect it lands on; the backend then trades the code for tokens. The
//! web engine only ever lives in that short-lived helper.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context as _, anyhow, bail};
use gpui_kit::App;

use crate::backend::Backend;
use crate::state::AppState;

/// Where Microsoft sends the browser after sign-in (from the backend).
const REDIRECT_PREFIX: &str = "https://login.live.com/oauth20_desktop.srf";

/// The helper ships next to the main executable.
fn helper_path() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("locating Calcite")?;
    let dir = exe.parent().context("locating Calcite's folder")?;
    let name = if cfg!(windows) {
        "calcite-login.exe"
    } else {
        "calcite-login"
    };
    let path = dir.join(name);
    if !path.exists() {
        bail!(
            "The sign-in helper is missing ({}). Reinstall Calcite.",
            path.display()
        );
    }
    Ok(path)
}

/// Pull the one-time `code` out of the redirect URL, or explain why there
/// isn't one (for example, the user cancelled).
pub fn code_from_redirect(redirect: &str) -> anyhow::Result<String> {
    let url = url::Url::parse(redirect.trim()).context("reading Microsoft's reply")?;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "error_description" => error = Some(value.into_owned()),
            "error" if error.is_none() => error = Some(value.into_owned()),
            _ => {}
        }
    }
    match (code, error) {
        (Some(code), _) if !code.is_empty() => Ok(code),
        (_, Some(error)) => bail!("Microsoft said: {error}"),
        _ => bail!("Microsoft didn't return a sign-in code"),
    }
}

/// Run the whole flow and refresh the account list when it's done.
pub fn sign_in(cx: &mut App) {
    let backend = Backend::global(cx).clone();
    let state = AppState::global(cx);
    let flow = backend.call(async {
        let flow = theseus::minecraft_auth::begin_login().await?;
        Ok(flow)
    });
    let backend_for_finish = backend.clone();
    cx.spawn(async move |cx| {
        let result: anyhow::Result<String> = async {
            let flow = flow.await?;
            let helper = helper_path()?;
            let uri = flow.auth_request_uri.clone();
            let output = backend
                .run(async move {
                    tokio::process::Command::new(helper)
                        .arg(uri)
                        .arg(REDIRECT_PREFIX)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .kill_on_drop(true)
                        .output()
                        .await
                })
                .await??;
            match output.status.code() {
                Some(0) => {}
                Some(1) => bail!("cancelled"),
                _ => bail!(
                    "The sign-in window couldn't open: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            }
            let redirect = String::from_utf8_lossy(&output.stdout).to_string();
            let code = code_from_redirect(&redirect)?;
            let credentials = backend_for_finish
                .call(async move { theseus::minecraft_auth::finish_login(&code, flow).await })
                .await?;
            Ok(credentials.offline_profile.name.clone())
        }
        .await;

        state.update(cx, |state, cx| {
            match result {
                Ok(name) => {
                    tracing::info!("signed in as {name}");
                    state.refresh_all(cx);
                }
                Err(error) if error.to_string() == "cancelled" => {}
                Err(error) => state.notify_error("Sign-in didn't finish", anyhow!(error)),
            }
            cx.notify();
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::code_from_redirect;

    #[test]
    fn extracts_the_code() {
        let code = code_from_redirect(
            "https://login.live.com/oauth20_desktop.srf?code=M.C123_abc&lc=1033",
        )
        .unwrap();
        assert_eq!(code, "M.C123_abc");
    }

    #[test]
    fn reports_microsoft_errors() {
        let err = code_from_redirect(
            "https://login.live.com/oauth20_desktop.srf?error=access_denied&error_description=The+user+has+denied+access",
        )
        .unwrap_err();
        assert!(err.to_string().contains("denied access"));
    }
}
