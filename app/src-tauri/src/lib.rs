//! Alt Ledger desktop app.
//!
//! The app is a thin shell around `alt-ledger-server`: it starts the server,
//! sits in the tray, and opens a window that loads the server's own page
//! (http://127.0.0.1:PORT). The page gets no access to Tauri's IPC, so the
//! window has exactly the same powers as a browser tab on this PC.
//!
//! Closing the window destroys the webview (freeing its memory); the server
//! keeps running in the tray so phones and tablets stay connected. Quit from
//! the tray menu to stop everything.

use std::sync::Mutex;

use alt_ledger_server::{Options, Running};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::Color;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;

/// Passed by the "start when I sign in" entry so login doesn't pop a window.
const MINIMIZED_FLAG: &str = "--minimized";
const UPDATE_CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

struct Shared {
    origin: Url,
    server: Mutex<Option<Running>>,
    autostart_item: CheckMenuItem<Wry>,
    update_item: MenuItem<Wry>,
    pending_update: Mutex<Option<Update>>,
}

pub fn run() {
    let app = tauri::Builder::default()
        // Must be first: a second launch just brings the existing window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app, "/")))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_FLAG])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let settings_path = alt_ledger_core::settings::default_dir().join("settings.json");
            let running = tauri::async_runtime::block_on(alt_ledger_server::start(Options {
                settings_path,
                ui_dir: None,
                port: None,
                cache_dir: None,
            }))?;
            let origin: Url = running.local_url().parse()?;

            let handle = app.handle();
            let autostart_on = handle.autolaunch().is_enabled().unwrap_or(false);
            let open = MenuItem::with_id(handle, "open", "Open Alt Ledger", true, None::<&str>)?;
            let connect = MenuItem::with_id(handle, "connect", "Connect a device…", true, None::<&str>)?;
            let autostart_item =
                CheckMenuItem::with_id(handle, "autostart", "Start when I sign in", true, autostart_on, None::<&str>)?;
            let update_item = MenuItem::with_id(handle, "update", "Up to date", false, None::<&str>)?;
            let quit_item = MenuItem::with_id(handle, "quit", "Quit Alt Ledger", true, None::<&str>)?;
            let menu = Menu::with_items(
                handle,
                &[
                    &open,
                    &connect,
                    &PredefinedMenuItem::separator(handle)?,
                    &autostart_item,
                    &update_item,
                    &PredefinedMenuItem::separator(handle)?,
                    &quit_item,
                ],
            )?;

            let mut tray = TrayIconBuilder::with_id("tray")
                .tooltip(format!("Alt Ledger on port {}", running.port))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_window(app, "/"),
                    "connect" => show_window(app, "/?open=connect"),
                    "autostart" => toggle_autostart(app),
                    "update" => install_update(app.clone()),
                    "quit" => quit(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } =
                        event
                    {
                        show_window(tray.app_handle(), "/");
                    }
                });
            if let Some(icon) = handle.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(handle)?;

            app.manage(Shared {
                origin,
                server: Mutex::new(Some(running)),
                autostart_item,
                update_item,
                pending_update: Mutex::new(None),
            });

            if !std::env::args().any(|a| a == MINIMIZED_FLAG) {
                show_window(handle, "/");
            }
            spawn_update_checks(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("couldn't start Alt Ledger");

    app.run(|app, event| match event {
        // Closing the last window is not quitting: keep serving from the tray.
        // An explicit app.exit() carries a code and goes through.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => stop_server(app),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_window(app, "/"),
        _ => {}
    });
}

/// Brings the window forward, creating it if it was closed.
fn show_window(app: &AppHandle, path: &str) {
    let Some(shared) = app.try_state::<Shared>() else { return };
    let Ok(url) = shared.origin.join(path) else { return };

    if let Some(window) = app.get_webview_window("main") {
        if path != "/" {
            let _ = window.navigate(url);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let origin = shared.origin.clone();
    let opener = app.clone();
    let built = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
        .title("Alt Ledger")
        .inner_size(1200.0, 800.0)
        .min_inner_size(380.0, 520.0)
        .background_color(Color(24, 32, 44, 255))
        // The window only ever shows Alt Ledger. Anything else opens in the
        // person's own browser instead of inside the app.
        .on_navigation(move |target| {
            if target.origin() == origin.origin() {
                return true;
            }
            if matches!(target.scheme(), "http" | "https") {
                let _ = opener.opener().open_url(target.as_str(), None::<&str>);
            }
            false
        })
        .build();
    match built {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => eprintln!("couldn't open the window: {e}"),
    }
}

fn toggle_autostart(app: &AppHandle) {
    let launcher = app.autolaunch();
    let on = launcher.is_enabled().unwrap_or(false);
    let result = if on { launcher.disable() } else { launcher.enable() };
    if let Err(e) = result {
        eprintln!("couldn't change start-at-login: {e}");
    }
    let now_on = launcher.is_enabled().unwrap_or(on);
    let _ = app.state::<Shared>().autostart_item.set_checked(now_on);
}

fn stop_server(app: &AppHandle) {
    if let Some(shared) = app.try_state::<Shared>() {
        if let Some(mut running) = shared.server.lock().unwrap().take() {
            running.stop();
        }
    }
}

fn quit(app: &AppHandle) {
    stop_server(app);
    app.exit(0);
}

/// True when this build was given a real update-signing key (see README).
fn updates_configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.is_empty() && !k.starts_with("REPLACE"))
}

fn spawn_update_checks(app: AppHandle) {
    if cfg!(debug_assertions) || !updates_configured(&app) {
        let _ = app.state::<Shared>().update_item.set_text("Updates are off in this build");
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        loop {
            check_for_update(&app).await;
            tokio::time::sleep(UPDATE_CHECK_EVERY).await;
        }
    });
}

async fn check_for_update(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    match found {
        Ok(Some(update)) => {
            let _ = shared.update_item.set_text(format!("Install update {}", update.version));
            let _ = shared.update_item.set_enabled(true);
            *shared.pending_update.lock().unwrap() = Some(update);
        }
        Ok(None) => {
            let _ = shared.update_item.set_text("Up to date");
        }
        // Offline or GitHub unreachable: try again next time, quietly.
        Err(e) => eprintln!("update check failed: {e}"),
    }
}

fn install_update(app: AppHandle) {
    let Some(update) = app.state::<Shared>().pending_update.lock().unwrap().take() else { return };
    tauri::async_runtime::spawn(async move {
        let shared = app.state::<Shared>();
        let _ = shared.update_item.set_enabled(false);
        let _ = shared.update_item.set_text(format!("Downloading {}…", update.version));
        match update.download_and_install(|_, _| {}, || {}).await {
            Ok(()) => {
                stop_server(&app);
                app.restart();
            }
            Err(e) => {
                eprintln!("update failed: {e}");
                let _ = shared.update_item.set_text(format!("Install update {} (retry)", update.version));
                let _ = shared.update_item.set_enabled(true);
                *shared.pending_update.lock().unwrap() = Some(update);
            }
        }
    });
}
