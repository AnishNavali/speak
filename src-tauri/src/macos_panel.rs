use anyhow::{ensure, Context, Result};
use tauri::{AppHandle, Manager};
use tauri_nspanel::{tauri_panel, ManagerExt, WebviewWindowExt};

tauri_panel! {
    panel!(MicrophonePanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true,
            hides_on_deactivate: false
        }
    })
}

pub fn configure(app: &AppHandle) -> Result<()> {
    // Runs in Tauri setup on the main thread, before the microphone is first shown.
    let window = app
        .get_webview_window("indicator")
        .context("Floating microphone window is missing")?;
    let panel = window.to_panel::<MicrophonePanel>()?;
    panel.add_style_mask(tauri_nspanel::panel::NSWindowStyleMask::NonactivatingPanel)?;
    panel.set_becomes_key_only_if_needed(true);
    panel.set_released_when_closed(false); // Tauri, not AppKit, owns the window's lifetime.
    panel.set_transparent(true);
    panel.set_corner_radius(32.0);
    panel.set_has_shadow(true);
    verify(app)?;
    Ok(())
}

pub fn verify(app: &AppHandle) -> Result<()> {
    let panel = app
        .get_webview_panel("indicator")
        .map_err(|_| anyhow::anyhow!("Floating microphone is not an NSPanel"))?;
    ensure!(
        panel
            .as_panel()
            .styleMask()
            .contains(tauri_nspanel::panel::NSWindowStyleMask::NonactivatingPanel),
        "Floating microphone must be non-activating"
    );
    ensure!(
        !panel.can_become_key_window() && !panel.can_become_main_window(),
        "Floating microphone must not take keyboard focus"
    );
    ensure!(
        !panel.as_panel().isOpaque(),
        "Floating microphone must remain transparent for native glass effects"
    );
    Ok(())
}

pub fn set_visible(app: &AppHandle, visible: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Ok(panel) = handle.get_webview_panel("indicator") {
            // A generic window.show() may activate its app. NSPanel ordering does not.
            if visible {
                panel.show();
            } else {
                panel.hide();
            }
        }
    });
}

pub fn restore(app: &AppHandle) {
    if let Ok(panel) = app.get_webview_panel("indicator") {
        let mask = panel.as_panel().styleMask()
            & !tauri_nspanel::panel::NSWindowStyleMask::NonactivatingPanel;
        let _ = panel.set_style_mask(mask);
        panel.to_window(); // Restore Tao's class before its normal window teardown.
    }
}

/// Real AppKit regression check; creates only a hidden indicator, without a model or microphone.
pub fn check() -> Result<()> {
    let mut context = crate::context();
    context
        .config_mut()
        .app
        .windows
        .retain(|window| window.label == "indicator");
    let app = tauri::Builder::default()
        .plugin(tauri_nspanel::init())
        .setup(|app| {
            configure(app.handle())?;
            verify(app.handle())?;
            println!(
                "Floating microphone: NSPanel, non-activating, cannot become key or main window"
            );
            app.handle().exit(0);
            Ok(())
        })
        .build(context)?;
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            restore(app);
        }
    });
    Ok(())
}
