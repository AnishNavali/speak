#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod diagnostics;
mod download;
mod engine;
mod inference;
#[cfg(target_os = "macos")]
mod macos_panel;
mod models;
mod platform;
mod runtime;
mod runtime_assets;
mod updates;

use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == "--verify-update-signature")
    {
        let result = match (args.get(2), args.get(3)) {
            (Some(file), Some(signature)) => updates::verify_artifact(
                std::path::Path::new(file),
                std::path::Path::new(signature),
            ),
            _ => Err(anyhow::anyhow!(
                "Usage: speak --verify-update-signature artifact signature"
            )),
        };
        if let Err(error) = result {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        println!("Updater signature matches the embedded public key and app version");
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "--probe-cuda-driver") {
        println!(
            "{}",
            serde_json::to_string(&runtime_assets::probe_cuda_driver()).unwrap()
        );
        return;
    }
    #[cfg(target_os = "macos")]
    if args
        .get(1)
        .is_some_and(|arg| arg == "--check-floating-panel")
    {
        if let Err(error) = macos_panel::check() {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    if args.get(1).is_some_and(|arg| arg == "--smoke-test") {
        let result = if let Some(path) = args.get(2) {
            tokio::runtime::Runtime::new()
                .expect("Could not start async runtime")
                .block_on(diagnostics::smoke_test(std::path::Path::new(path)))
        } else {
            Err(anyhow::anyhow!("Usage: speak --smoke-test recording.wav"))
        };
        if let Err(error) = result {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(core) = app.try_state::<Arc<engine::Core>>() {
                core.show_view(engine::View::Settings);
                return;
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(|app| {
            updates::configure(app.handle())?;
            #[cfg(target_os = "macos")]
            {
                app.handle().plugin(tauri_nspanel::init())?;
                macos_panel::configure(app.handle())?;
            }
            // The desktop shortcut plugin opens X11 on Linux. Pure Wayland uses portals instead.
            if !platform::wayland() {
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(|app, shortcut, event| {
                            let core = app.state::<Arc<engine::Core>>();
                            if shortcut.to_string() == "Escape" {
                                if event.state() == ShortcutState::Pressed {
                                    core.cancel();
                                }
                            } else if event.state() == ShortcutState::Pressed {
                                if let Err(error) = core.begin(false) {
                                    core.notice(error.to_string());
                                }
                            } else {
                                core.release();
                            }
                        })
                        .build(),
                )?;
            }
            let core = engine::Core::new(app.handle().clone())?;
            app.manage(core.clone());
            core.setup_tray()?;
            core.position_indicator();
            #[cfg(target_os = "macos")]
            if platform::keyboard_permission_granted() {
                let input = core.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = input.enable_input().await {
                        input.notice(error.to_string());
                    }
                });
            }
            if let Err(error) = core.register_shortcut() {
                core.notice(format!("Shortcut unavailable: {error:#}"));
            }
            if std::env::args().any(|arg| arg == "--background") {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            }
            if !core.settings.lock().unwrap().needs_model_setup() {
                let task = core.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = task.start_runtime().await {
                        task.runtime_error(error.to_string());
                    }
                });
            }
            let monitor = core.clone();
            tauri::async_runtime::spawn(async move {
                monitor.monitor().await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            engine::snapshot,
            engine::open_test_ui,
            engine::open_settings,
            engine::save_settings,
            engine::start_model,
            engine::recheck_acceleration,
            engine::press,
            engine::release,
            engine::cancel,
            engine::close_indicator,
            engine::retry,
            engine::discard,
            engine::save_audio,
            engine::enable_input,
            engine::insert_result,
            engine::microphones,
            engine::set_autostart,
            engine::get_autostart,
            models::download_model,
            updates::check_update,
            updates::install_update,
        ])
        .build(context())
        .expect("Could not build Speak");
    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            macos_panel::restore(app);
        }
        if let tauri::RunEvent::Exit = event {
            let core = app.state::<Arc<engine::Core>>();
            core.shutdown();
        }
    });
}
