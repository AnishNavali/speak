use anyhow::{bail, Result};
use enigo::{Enigo, Keyboard, Settings};
use std::sync::mpsc;

pub fn wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[derive(Clone, Debug)]
// Destination (macOS PID, otherwise window), optional field, optional macOS window.
pub struct Target(pub Option<u64>, pub Option<u64>, pub Option<u64>);

pub fn target() -> Result<Target> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication();
        let pid = app
            .map(|app| app.processIdentifier())
            .ok_or_else(|| anyhow::anyhow!("No target application is focused"))?;
        if pid as u32 == std::process::id() {
            bail!(
                "Speak is the active application. Focus the destination text field and try again."
            );
        }
        focused_target(pid)
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetGUIThreadInfo, GUITHREADINFO,
        };
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetGUIThreadInfo(0, &mut info)?;
        }
        Ok(Target(
            Some(unsafe { GetForegroundWindow() }.0 as usize as u64),
            Some(info.hwndFocus.0 as usize as u64),
            None,
        ))
    }
    #[cfg(target_os = "linux")]
    {
        if wayland() {
            return Ok(Target(None, None, None));
        }
        use x11rb::{
            connection::Connection,
            protocol::xproto::{AtomEnum, ConnectionExt},
        };
        let (connection, screen) = x11rb::connect(None)?;
        let atom = connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;
        let reply = connection
            .get_property(
                false,
                connection.setup().roots[screen].root,
                atom,
                AtomEnum::WINDOW,
                0,
                1,
            )?
            .reply()?;
        let focus = connection.get_input_focus()?.reply()?.focus;
        Ok(Target(
            reply
                .value32()
                .and_then(|mut values| values.next())
                .map(u64::from),
            Some(u64::from(focus)),
            None,
        ))
    }
}

pub fn target_unchanged(before: &Target) -> Result<()> {
    let after = target()?;
    compare_targets(before, &after)
}

fn compare_targets(before: &Target, after: &Target) -> Result<()> {
    if let Some(id) = before.0 {
        if before.1.is_none() && before.2.is_none() {
            bail!("The destination window could not be verified. Your text is retained; copy it or choose Insert result in the intended field.");
        }
        if after.0 != Some(id)
            || (before.1.is_some() && after.1 != before.1)
            || (before.2.is_some() && after.2 != before.2)
        {
            bail!("The target application changed. Text is retained; copy it or insert again in the intended field.");
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn keyboard_permission_granted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    // Read-only preflight: never opens Settings or asks for permission on startup.
    unsafe { AXIsProcessTrusted() }
}

#[cfg(target_os = "macos")]
fn focused_target(pid: i32) -> Result<Target> {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
        fn AXUIElementCopyAttributeValue(
            element: *const c_void,
            attribute: *const c_void,
            value: *mut *const c_void,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: *const c_void, seconds: f32) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            value: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFHash(value: *const c_void) -> usize;
        fn CFRelease(value: *const c_void);
    }
    unsafe fn query(app: *const c_void, attribute: &CStr) -> Result<Option<u64>> {
        let name = CFStringCreateWithCString(std::ptr::null(), attribute.as_ptr(), 0x08000100);
        if name.is_null() {
            bail!("Could not allocate an Accessibility attribute");
        }
        let mut value = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(app, name, &mut value);
        CFRelease(name);
        let token = if value.is_null() {
            None
        } else {
            let token = if status == 0 {
                Some(CFHash(value) as u64)
            } else {
                None
            };
            CFRelease(value);
            token
        };
        optional_ax_value(status, token, &attribute.to_string_lossy())
    }

    unsafe {
        if !keyboard_permission_granted() {
            bail!("Speak does not have Accessibility permission. Enable this build of Speak in System Settings → Privacy & Security → Accessibility, then enable keyboard insertion again.");
        }
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            bail!("Could not inspect the target application");
        }
        // Bound messaging to busy apps. Reading AXRole activates Chromium's lazy native AX tree;
        // it does not turn on VoiceOver or change the user's editor accessibility settings.
        let _ = AXUIElementSetMessagingTimeout(app, 0.2);
        let result = (|| -> Result<Target> {
            let _ = query(app, c"AXRole");
            let window = query(app, c"AXFocusedWindow")?;
            let mut field = query(app, c"AXFocusedUIElement")?;
            if field.is_none() {
                field = query(app, c"AXFocusedUIElement")?;
            }
            Ok(Target(Some(pid as u64), field, window))
        })();
        CFRelease(app);
        result
    }
}

#[cfg(any(target_os = "macos", test))]
fn optional_ax_value(status: i32, value: Option<u64>, attribute: &str) -> Result<Option<u64>> {
    match status {
        0 => value.map(Some).ok_or_else(|| anyhow::anyhow!("Accessibility returned an empty {attribute} value")),
        -25212 | -25205 => Ok(None), // NoValue / AttributeUnsupported are capabilities, not permission failures.
        _ => bail!("Cannot inspect {attribute} (macOS Accessibility error {status}). Focus the destination and try again."),
    }
}

pub async fn wait_modifiers() -> Result<()> {
    for _ in 0..100 {
        if !modifiers_pressed()? {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    bail!("Release the shortcut's modifier keys. Your text is retained for insertion.")
}

fn modifiers_pressed() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
        }
        Ok([54, 55, 56, 60, 58, 61, 59, 62]
            .into_iter()
            .any(|key| unsafe { CGEventSourceKeyState(1, key) }))
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        Ok([0x10, 0x11, 0x12, 0x5B, 0x5C]
            .into_iter()
            .any(|key| unsafe { GetAsyncKeyState(key) } < 0))
    }
    #[cfg(target_os = "linux")]
    {
        if wayland() {
            return Ok(false);
        } // Wayland does not expose global physical key state.
        use x11rb::{connection::Connection, protocol::xproto::ConnectionExt};
        let (connection, screen) = x11rb::connect(None)?;
        let state = connection
            .query_pointer(connection.setup().roots[screen].root)?
            .reply()?
            .mask;
        Ok(u16::from(state) & 0x4d != 0)
    }
}

pub enum InputCommand {
    Enable(tokio::sync::oneshot::Sender<Result<(), String>>),
    Text(
        String,
        Target,
        tokio_util::sync::CancellationToken,
        tokio::sync::oneshot::Sender<Result<(), String>>,
    ),
}

pub fn input_worker() -> mpsc::Sender<InputCommand> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut input: Option<Enigo> = None;
        for command in rx {
            let result = if input.is_none() {
                std::panic::catch_unwind(|| Enigo::new(&Settings::default()))
                    .map_err(|_| "The input backend could not initialize. Check Accessibility / Wayland portal permissions, then try Enable keyboard insertion again.".to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()))
                    .map(|value| input = Some(value))
            } else {
                Ok(())
            };
            match command {
                InputCommand::Enable(response) => {
                    let _ = response.send(result);
                }
                InputCommand::Text(text, target, cancel, response) => {
                    let result = result
                        .and_then(|_| {
                            if cancel.is_cancelled() {
                                Err("Insertion cancelled".into())
                            } else {
                                Ok(())
                            }
                        })
                        .and_then(|_| target_unchanged(&target).map_err(|e| e.to_string()))
                        .and_then(|_| {
                            type_chunks(&text, &cancel, |chunk| {
                                input
                                    .as_mut()
                                    .unwrap()
                                    .text(chunk)
                                    .map_err(|error| error.to_string())
                            })
                        });
                    let _ = response.send(result);
                }
            }
        }
    });
    tx
}

fn type_chunks(
    text: &str,
    cancel: &tokio_util::sync::CancellationToken,
    mut send: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let (mut start, mut units) = (0, 0);
    for (index, character) in text.char_indices() {
        if units + character.len_utf16() > 20 {
            if cancel.is_cancelled() {
                return Err("Insertion cancelled".into());
            }
            send(&text[start..index])?;
            start = index;
            units = 0;
        }
        units += character.len_utf16();
    }
    if cancel.is_cancelled() {
        return Err("Insertion cancelled".into());
    }
    if start < text.len() {
        send(&text[start..])?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub async fn portal_shortcuts(
    core: std::sync::Arc<crate::engine::Core>,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session(Default::default()).await?;
    let mut pressed = portal.receive_activated().await?;
    let mut released = portal.receive_deactivated().await?;
    let configured = core.settings.lock().unwrap().shortcut.clone();
    let preferred = configured
        .replace("CommandOrControl", "CTRL")
        .replace("Control", "CTRL")
        .replace("Shift", "SHIFT")
        .replace("Alt", "ALT")
        .replace("Super", "LOGO")
        .replace("Space", "space");
    let shortcuts = [NewShortcut::new("dictate", "Hold to dictate with Speak")
        .preferred_trigger(preferred.as_str())];
    let binding = portal
        .bind_shortcuts(&session, &shortcuts, None, Default::default())
        .await?
        .response()?;
    if binding.shortcuts().is_empty() {
        session.close().await?;
        bail!("The desktop did not bind a dictation shortcut");
    }
    core.shortcut_hint(binding.shortcuts()[0].trigger_description());
    loop {
        tokio::select! {
            _ = cancel.cancelled() => { let _ = session.close().await; break; }
            event = pressed.next() => {
                let Some(event) = event else { bail!("Wayland shortcut session ended"); };
                if event.shortcut_id() == "dictate" { if let Err(error) = core.begin(false) { core.notice(error.to_string()); } }
            }
            event = released.next() => {
                let Some(event) = event else { bail!("Wayland shortcut session ended"); };
                if event.shortcut_id() == "dictate" { core.release(); }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    #[test]
    fn recovered_insertion_respects_cancellation_and_preserves_unicode() {
        let text = "नमस्ते 😊❤️ hello 🎉 world".repeat(3);
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut result = String::new();
        type_chunks(&text, &cancel, |chunk| {
            assert!(chunk.encode_utf16().count() <= 20);
            result.push_str(chunk);
            Ok(())
        })
        .unwrap();
        assert_eq!(result, text);
        let mut calls = 0;
        assert!(type_chunks(&text, &cancel, |_| {
            calls += 1;
            cancel.cancel();
            Ok(())
        })
        .is_err());
        assert_eq!(calls, 1);
        assert!(type_chunks("never insert", &cancel, |_| panic!(
            "Cancelled text was inserted"
        ))
        .is_err());
    }

    #[test]
    fn absent_fields_do_not_hide_permission_or_messaging_failures() {
        assert_eq!(
            optional_ax_value(-25212, None, "AXFocusedUIElement").unwrap(),
            None
        );
        assert_eq!(
            optional_ax_value(-25205, None, "AXFocusedUIElement").unwrap(),
            None
        );
        assert_eq!(
            optional_ax_value(0, Some(42), "AXFocusedUIElement").unwrap(),
            Some(42)
        );
        for status in [-25211, -25204, -25202] {
            assert!(optional_ax_value(status, None, "AXFocusedUIElement").is_err());
        }
        assert!(optional_ax_value(0, None, "AXFocusedUIElement").is_err());
    }

    #[test]
    fn missing_field_uses_window_guard_without_weakening_known_field_checks() {
        let absent = optional_ax_value(-25212, None, "AXFocusedUIElement").unwrap();
        let before = Target(Some(100), absent, Some(200));
        assert!(compare_targets(&before, &Target(Some(100), None, Some(200))).is_ok());
        assert!(compare_targets(&before, &Target(Some(100), Some(300), Some(200))).is_ok());
        assert!(compare_targets(&before, &Target(Some(101), None, Some(200))).is_err());
        assert!(compare_targets(&before, &Target(Some(100), None, Some(201))).is_err());
        assert!(compare_targets(&before, &Target(Some(100), None, None)).is_err());
        let known_field = Target(Some(100), Some(300), Some(200));
        assert!(compare_targets(&known_field, &Target(Some(100), Some(300), Some(200))).is_ok());
        assert!(compare_targets(&known_field, &Target(Some(100), Some(301), Some(200))).is_err());
        assert!(compare_targets(&known_field, &before).is_err());
        assert!(compare_targets(&Target(Some(100), None, None), &before).is_err());
        assert!(compare_targets(
            &Target(Some(100), None, None),
            &Target(Some(100), None, None)
        )
        .is_err());
    }
}
