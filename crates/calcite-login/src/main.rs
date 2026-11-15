//! Calcite's Microsoft sign-in window.
//!
//! Usage: `calcite-login <authorize-url> <redirect-prefix>`
//!
//! Opens the authorize URL in a small native web view. When Microsoft
//! redirects to `<redirect-prefix>…`, the full redirect URL (which carries the
//! one-time code) is printed on stdout and the process exits with status 0.
//! Closing the window exits with status 1. Nothing is stored on disk.

use std::process::ExitCode;

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

#[derive(Debug)]
enum Signal {
    Finished(String),
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(authorize), Some(redirect_prefix)) = (args.next(), args.next()) else {
        eprintln!("usage: calcite-login <authorize-url> <redirect-prefix>");
        return ExitCode::from(2);
    };
    if url::Url::parse(&authorize).is_err() {
        eprintln!("calcite-login: not a valid URL");
        return ExitCode::from(2);
    }

    let event_loop = EventLoopBuilder::<Signal>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = match WindowBuilder::new()
        .with_title("Sign in with Microsoft")
        .with_inner_size(LogicalSize::new(500.0, 640.0))
        .with_min_inner_size(LogicalSize::new(400.0, 520.0))
        .build(&event_loop)
    {
        Ok(window) => window,
        Err(error) => {
            eprintln!("calcite-login: couldn't open a window: {error}");
            return ExitCode::from(3);
        }
    };

    let builder = WebViewBuilder::new()
        .with_url(&authorize)
        .with_navigation_handler(move |next: String| {
            if next.starts_with(&redirect_prefix) {
                let _ = proxy.send_event(Signal::Finished(next));
                return false;
            }
            true
        });

    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window);
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("tao provides a GTK box"))
    };
    let _webview = match webview {
        Ok(webview) => webview,
        Err(error) => {
            eprintln!("calcite-login: no web view available: {error}");
            return ExitCode::from(3);
        }
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Signal::Finished(redirect)) => {
                println!("{redirect}");
                std::process::exit(0);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => std::process::exit(1),
            _ => {}
        }
    })
}
