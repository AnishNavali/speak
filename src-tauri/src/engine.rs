use crate::{
    audio,
    config::{self, Settings},
    inference::{self, Inference},
    platform,
    runtime::Runtime,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Settings,
    Test,
}

impl View {
    fn test_target(&self, explicit_test: bool, main_focused: bool) -> Result<bool> {
        if explicit_test || (main_focused && matches!(self, Self::Test)) {
            return Ok(true);
        }
        if main_focused {
            bail!("Open the test notepad, or focus a text field in another application before dictating.");
        }
        Ok(false)
    }
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub view: View,
    pub session_id: Option<u64>,
    pub phase: String,
    pub ready: bool,
    pub model_setup_needed: bool,
    pub input_ready: bool,
    pub model_status: String,
    pub message: String,
    pub notice: String,
    pub raw: String,
    pub result: String,
    pub shortcut: String,
    pub settings: Settings,
    pub data_dir: String,
    pub wayland: bool,
    pub acceleration: String,
    pub updates_enabled: bool,
}

pub struct Chunk {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub text: Option<String>,
}
enum Job {
    Chunk(usize),
    End,
}
pub struct Session {
    pub id: u64,
    pub chunks: Mutex<Vec<Chunk>>,
    sender: mpsc::UnboundedSender<Job>,
    cancel: CancellationToken,
    test: bool,
    target: platform::Target,
    ended: AtomicBool,
    fault: Mutex<Option<String>>,
}

pub struct Core {
    pub app: AppHandle,
    pub settings: Mutex<Settings>,
    pub data_dir: PathBuf,
    state: Mutex<Snapshot>,
    runtime: Mutex<Option<Runtime>>,
    pub(crate) runtime_operation: tokio::sync::Mutex<()>,
    runtime_cancel: Mutex<CancellationToken>,
    cpu_recovery: AtomicBool,
    audio: OnceLock<std::sync::mpsc::Sender<audio::Command>>,
    floating_menu: OnceLock<CheckMenuItem<tauri::Wry>>,
    input: std::sync::mpsc::Sender<platform::InputCommand>,
    session: Mutex<Option<Arc<Session>>>,
    sequence: AtomicU64,
    shortcut_cancel: Mutex<Option<CancellationToken>>,
    actions: Mutex<()>,
    pub download_lock: tokio::sync::Mutex<()>,
}

impl Core {
    pub fn new(app: AppHandle) -> Result<Arc<Self>> {
        let data_dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;
        let settings = config::load(&data_dir)?;
        let core = Arc::new(Self {
            app,
            data_dir: data_dir.clone(),
            settings: Mutex::new(settings.clone()),
            state: Mutex::new(Snapshot {
                view: View::Settings,
                session_id: None,
                phase: "idle".into(),
                ready: false,
                model_setup_needed: settings.needs_model_setup(),
                input_ready: false,
                model_status: "Choose model files to begin".into(),
                message: "Hold the shortcut to dictate".into(),
                notice: String::new(),
                raw: String::new(),
                result: String::new(),
                shortcut: settings.shortcut.clone(),
                settings,
                data_dir: data_dir.to_string_lossy().into_owned(),
                wayland: platform::wayland(),
                acceleration: "Not running".into(),
                updates_enabled: crate::updates::configured(),
            }),
            runtime: Mutex::new(None),
            runtime_operation: tokio::sync::Mutex::new(()),
            runtime_cancel: Mutex::new(CancellationToken::new()),
            cpu_recovery: AtomicBool::new(false),
            audio: OnceLock::new(),
            floating_menu: OnceLock::new(),
            input: platform::input_worker(),
            session: Mutex::new(None),
            sequence: AtomicU64::new(1),
            shortcut_cancel: Mutex::new(None),
            actions: Mutex::new(()),
            download_lock: tokio::sync::Mutex::new(()),
        });
        let _ = core.audio.set(audio::spawn(Arc::downgrade(&core)));
        Ok(core)
    }

    pub fn emit(&self) {
        let _ = self
            .app
            .emit("speak-state", self.state.lock().unwrap().clone());
    }
    pub fn notice(&self, notice: String) {
        self.state.lock().unwrap().notice = notice;
        self.emit();
    }
    pub fn shortcut_hint(&self, text: &str) {
        self.state.lock().unwrap().shortcut = text.into();
        self.emit();
    }
    pub fn runtime_error(&self, text: String) {
        {
            let mut state = self.state.lock().unwrap();
            state.ready = false;
            state.model_status = text;
        }
        self.emit();
    }

    pub fn setup_tray(self: &Arc<Self>) -> Result<()> {
        let show = MenuItem::with_id(&self.app, "show", "Settings…", true, None::<&str>)?;
        let test = MenuItem::with_id(&self.app, "test", "Open test notepad", true, None::<&str>)?;
        let floating = CheckMenuItem::with_id(
            &self.app,
            "floating",
            "Show floating microphone",
            true,
            self.settings.lock().unwrap().floating,
            None::<&str>,
        )?;
        let enable = MenuItem::with_id(
            &self.app,
            "input",
            "Enable keyboard insertion…",
            true,
            None::<&str>,
        )?;
        let cancel =
            MenuItem::with_id(&self.app, "cancel", "Cancel dictation", true, None::<&str>)?;
        let quit = MenuItem::with_id(&self.app, "quit", "Quit Speak", true, None::<&str>)?;
        let menu = Menu::with_items(
            &self.app,
            &[&show, &test, &floating, &enable, &cancel, &quit],
        )?;
        let _ = self.floating_menu.set(floating);
        let core = self.clone();
        // A small RGBA waveform icon, generated in memory without an icon dependency.
        let mut rgba = vec![0u8; 32 * 32 * 4];
        for (x, height) in [(5, 8), (11, 18), (17, 26), (23, 14)] {
            for y in (32 - height) / 2..(32 + height) / 2 {
                for dx in 0..4 {
                    rgba[(y * 32 + x + dx) * 4..(y * 32 + x + dx) * 4 + 4]
                        .copy_from_slice(&[82, 106, 78, 255]);
                }
            }
        }
        TrayIconBuilder::with_id("speak")
            .tooltip("Speak — hold to dictate")
            .icon(tauri::image::Image::new_owned(rgba, 32, 32))
            .menu(&menu)
            .on_menu_event(move |app, event| match event.id.as_ref() {
                "show" => core.show_view(View::Settings),
                "test" => core.show_view(View::Test),
                "floating" => {
                    let visible = !core.settings.lock().unwrap().floating;
                    if let Err(error) = core.set_floating(visible) {
                        core.refresh_indicator();
                        core.notice(format!("Could not change microphone visibility: {error:#}"));
                    }
                }
                "input" => {
                    let core = core.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = core.enable_input().await {
                            core.notice(format!("{error:#}"));
                        }
                    });
                }
                "cancel" => core.cancel(),
                "quit" => {
                    core.shutdown();
                    app.exit(0);
                }
                _ => {}
            })
            .build(&self.app)?;
        Ok(())
    }

    pub fn register_shortcut(self: &Arc<Self>) -> Result<()> {
        if let Some(cancel) = self.shortcut_cancel.lock().unwrap().take() {
            cancel.cancel();
        }
        #[cfg(target_os = "linux")]
        if platform::wayland() {
            let cancel = CancellationToken::new();
            *self.shortcut_cancel.lock().unwrap() = Some(cancel.clone());
            let core = self.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = platform::portal_shortcuts(core.clone(), cancel).await {
                    core.notice(format!("Wayland shortcut: {error:#}"));
                }
            });
            return Ok(());
        }
        self.app.global_shortcut().unregister_all()?;
        let shortcut = self.settings.lock().unwrap().shortcut.clone();
        self.app.global_shortcut().register(shortcut.as_str())?;
        self.shortcut_hint(&shortcut);
        Ok(())
    }

    pub async fn enable_input(&self) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.input.send(platform::InputCommand::Enable(tx))?;
        rx.await?.map_err(|error| anyhow!(error))?;
        self.state.lock().unwrap().input_ready = true;
        self.notice(
            "Keyboard insertion enabled. Focus a text field, then hold the shortcut.".into(),
        );
        Ok(())
    }

    pub async fn start_runtime(self: &Arc<Self>) -> Result<()> {
        let _operation = self.runtime_operation.lock().await;
        self.ensure_setup_idle()?;
        self.runtime_error("Loading Gemma…".into());
        self.runtime.lock().unwrap().take();
        let mut settings = self.settings.lock().unwrap().clone();
        if self.cpu_recovery.load(Ordering::Relaxed) {
            settings.acceleration = "cpu".into();
        }
        let cancel = CancellationToken::new();
        self.runtime_cancel.lock().unwrap().cancel();
        *self.runtime_cancel.lock().unwrap() = cancel.clone();
        let runtime = Runtime::launch(
            &settings,
            &self.data_dir,
            &self.app.path().resource_dir()?,
            &cancel,
            &|text| {
                self.state.lock().unwrap().model_status = text;
                self.emit();
            },
        )
        .await?;
        let acceleration = runtime.acceleration.clone();
        let notes = runtime.notes.join("\n");
        *self.runtime.lock().unwrap() = Some(runtime);
        {
            let mut state = self.state.lock().unwrap();
            state.ready = true;
            state.model_status = "Ready".into();
            state.acceleration = acceleration;
            if !notes.is_empty() {
                state.notice = format!("Acceleration fallback details:\n{notes}");
            }
        }
        self.emit();
        Ok(())
    }

    fn inference(&self) -> Result<Inference> {
        if !self.state.lock().unwrap().ready {
            bail!("Gemma is not ready. Start or restart the model in Settings.");
        }
        let runtime = self.runtime.lock().unwrap();
        let runtime = runtime.as_ref().context("Model runtime is not running")?;
        Inference::new(runtime.url.clone(), runtime.key.clone())
    }

    pub async fn monitor(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let crashed = self
                .runtime
                .lock()
                .unwrap()
                .as_mut()
                .is_some_and(|r| r.exited().unwrap_or(true));
            if crashed {
                if self.state.lock().unwrap().ready {
                    self.runtime_error(
                        "Gemma stopped unexpectedly. Pending audio is retained.".into(),
                    );
                    self.release();
                }
                let idle = matches!(self.state.lock().unwrap().phase.as_str(), "idle" | "error");
                if idle && !self.cpu_recovery.swap(true, Ordering::Relaxed) {
                    match self.start_runtime().await {
                        Ok(()) => {
                            if self.state.lock().unwrap().phase == "error" {
                                if let Err(error) = self.retry_pending().await {
                                    self.notice(error.to_string());
                                }
                            }
                        }
                        Err(error) => self.runtime_error(error.to_string()),
                    }
                }
            }
        }
    }

    fn current(&self, id: u64) -> Option<Arc<Session>> {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| s.id == id && !s.cancel.is_cancelled())
            .cloned()
    }

    pub fn begin(self: &Arc<Self>, test: bool) -> Result<()> {
        let main_focused = self
            .app
            .get_webview_window("main")
            .is_some_and(|window| window.is_focused().unwrap_or(false));
        let test = self
            .state
            .lock()
            .unwrap()
            .view
            .test_target(test, main_focused)?;
        let inference = self.inference()?;
        let target = if test {
            platform::Target(None, None, None)
        } else {
            platform::target()?
        };
        let action = self.actions.lock().unwrap();
        {
            let state = self.state.lock().unwrap();
            if !state.ready {
                bail!("Gemma is not ready");
            }
            if state.phase != "idle" {
                return Ok(()); // Ignore key repeat instead of creating a duplicate session.
            }
            if !test && !state.input_ready {
                drop(state);
                drop(action);
                self.notice(
                    "Enable keyboard insertion in Speak before dictating into other apps.".into(),
                );
                bail!("Keyboard insertion permission is required");
            }
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::unbounded_channel();
        let session = Arc::new(Session {
            id,
            chunks: Mutex::new(Vec::new()),
            sender,
            cancel: CancellationToken::new(),
            test,
            target,
            ended: AtomicBool::new(false),
            fault: Mutex::new(None),
        });
        *self.session.lock().unwrap() = Some(session.clone());
        {
            let mut state = self.state.lock().unwrap();
            state.phase = "starting".into();
            state.session_id = Some(id);
            state.raw.clear();
            state.result.clear();
            state.notice.clear();
            state.message = "Opening microphone…".into();
        }
        let core = self.clone();
        tauri::async_runtime::spawn(async move {
            core.pipeline(session, receiver, inference).await;
        });
        let sent = self.audio.get().unwrap().send(audio::Command::Start(
            id,
            self.settings.lock().unwrap().clone(),
        ));
        drop(action); // Never hold the control lock across calls that wait on the main thread.
        if let Err(error) = sent {
            self.capture_failed(id, error.to_string());
            self.finish_capture(id);
            return Err(error.into());
        }
        self.refresh_indicator();
        self.emit();
        self.refresh_escape_binding();
        Ok(())
    }

    pub fn recording(&self, id: u64) {
        let action = self.actions.lock().unwrap();
        if self.current(id).is_none() {
            return;
        }
        {
            let mut state = self.state.lock().unwrap();
            if state.phase == "starting" {
                state.phase = "recording".into();
                state.message = "Listening · release to finish".into();
            }
        }
        drop(action);
        self.emit();
    }

    pub fn level(&self, id: u64, level: f32, seconds: u64) {
        if self.current(id).is_some() {
            let _ = self.app.emit(
                "speak-meter",
                serde_json::json!({"level":level,"seconds":seconds}),
            );
        }
    }

    pub fn chunk(&self, id: u64, samples: Vec<f32>, rate: u32) {
        let _action = self.actions.lock().unwrap();
        if let Some(session) = self.current(id) {
            let mut chunks = session.chunks.lock().unwrap();
            let index = chunks.len();
            chunks.push(Chunk {
                samples,
                rate,
                text: None,
            });
            let _ = session.sender.send(Job::Chunk(index));
        }
    }

    pub fn release(&self) {
        let action = self.actions.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if !matches!(state.phase.as_str(), "starting" | "recording") {
            return;
        }
        state.phase = "processing".into();
        state.message = "Finishing your recording…".into();
        drop(state);
        let _ = self.audio.get().unwrap().send(audio::Command::Stop);
        drop(action);
        self.refresh_escape_binding();
        self.emit();
    }

    pub fn finish_capture(&self, id: u64) {
        let action = self.actions.lock().unwrap();
        if let Some(session) = self.current(id) {
            session.ended.store(true, Ordering::Relaxed);
            {
                let mut state = self.state.lock().unwrap();
                state.phase = "processing".into();
                state.message = "Finishing transcription…".into();
            }
            let _ = session.sender.send(Job::End);
            drop(action);
            self.refresh_escape_binding();
            self.emit();
        }
    }

    pub fn capture_failed(&self, id: u64, message: String) {
        if let Some(session) = self.current(id) {
            *session.fault.lock().unwrap() = Some(message.clone());
            self.error(id, message);
        }
    }

    fn error(&self, id: u64, message: String) {
        let action = self.actions.lock().unwrap();
        if self.current(id).is_none() {
            return;
        }
        drop(action);
        {
            let mut state = self.state.lock().unwrap();
            state.phase = "error".into();
            state.message = message;
        }
        self.refresh_indicator();
        self.emit();
    }

    fn raw(session: &Session) -> String {
        session
            .chunks
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| c.text.as_deref())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    async fn transcribe_chunk(
        &self,
        session: &Session,
        index: usize,
        inference: &Inference,
    ) -> Result<()> {
        let wav = {
            let chunks = session.chunks.lock().unwrap();
            let chunk = &chunks[index];
            if chunk.text.is_some() {
                return Ok(());
            }
            inference::wav(&chunk.samples, chunk.rate)?
        };
        let text = inference.transcribe(&wav, &session.cancel).await?;
        session.chunks.lock().unwrap()[index].text = Some(text);
        let action = self.actions.lock().unwrap();
        if self.current(session.id).is_some() {
            self.state.lock().unwrap().raw = Self::raw(session);
            drop(action);
            self.emit();
        }
        Ok(())
    }

    async fn pipeline(
        self: Arc<Self>,
        session: Arc<Session>,
        mut receiver: mpsc::UnboundedReceiver<Job>,
        inference: Inference,
    ) {
        loop {
            let job = tokio::select! { _ = session.cancel.cancelled() => return, value = receiver.recv() => value };
            match job {
                Some(Job::Chunk(index)) => {
                    if session.fault.lock().unwrap().is_none() {
                        if let Err(error) = self.transcribe_chunk(&session, index, &inference).await
                        {
                            *session.fault.lock().unwrap() = Some(error.to_string());
                        }
                    }
                }
                Some(Job::End) => {
                    let failure = session.fault.lock().unwrap().clone();
                    if let Some(error) = failure {
                        self.error(session.id, error);
                        return;
                    }
                    if let Err(error) = self.finalize(&session, &inference).await {
                        self.error(session.id, format!("{error:#}"));
                    }
                    return;
                }
                None => return,
            }
        }
    }

    async fn finalize(&self, session: &Session, inference: &Inference) -> Result<()> {
        let raw = Self::raw(session);
        if raw.trim().is_empty() {
            bail!("No speech returned. Your captured recording is retained for retry or export.");
        }
        {
            let _action = self.actions.lock().unwrap();
            if self.current(session.id).is_none() {
                return Ok(());
            }
            let mut state = self.state.lock().unwrap();
            state.raw = raw.clone();
            state.message = "Polishing your words…".into();
        }
        self.emit();
        let result = inference.polish(&raw, &session.cancel).await?;
        {
            let _action = self.actions.lock().unwrap();
            if self.current(session.id).is_none() {
                return Ok(());
            }
            self.state.lock().unwrap().result = result.clone();
        }
        if session.test {
            let _ = self.app.emit(
                "speak-test-result",
                serde_json::json!({"id":session.id,"text":result}),
            );
        } else {
            self.state.lock().unwrap().message =
                "Waiting for shortcut modifiers to be released…".into();
            self.emit();
            platform::wait_modifiers().await?;
            if session.cancel.is_cancelled() {
                return Ok(());
            }
            self.type_text(result, session.target.clone(), session.cancel.clone())
                .await?;
        }
        self.complete_session(session.id);
        Ok(())
    }

    async fn type_text(
        &self,
        text: String,
        target: platform::Target,
        cancel: CancellationToken,
    ) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.input
            .send(platform::InputCommand::Text(text, target, cancel, tx))?;
        rx.await?.map_err(|message| anyhow!(message))
    }

    fn complete_session(&self, id: u64) {
        let action = self.actions.lock().unwrap();
        if self.current(id).is_none() {
            return;
        }
        self.session.lock().unwrap().take();
        {
            let mut state = self.state.lock().unwrap();
            state.phase = "idle".into();
            state.session_id = None;
            state.message = "Done · hold the shortcut for your next dictation".into();
        }
        drop(action);
        self.refresh_escape_binding();
        self.refresh_indicator();
        self.emit();
    }

    pub fn cancel(&self) {
        let action = self.actions.lock().unwrap();
        if self.state.lock().unwrap().phase == "updating" {
            return;
        }
        if let Some(session) = self.session.lock().unwrap().take() {
            session.cancel.cancel();
        }
        let _ = self.audio.get().unwrap().send(audio::Command::Cancel);
        {
            let mut state = self.state.lock().unwrap();
            state.phase = "idle".into();
            state.session_id = None;
            state.message = "Dictation cancelled".into();
        }
        drop(action);
        self.refresh_escape_binding();
        self.refresh_indicator();
        self.emit();
    }

    fn set_floating(&self, visible: bool) -> Result<()> {
        let mut settings = self.settings.lock().unwrap().clone();
        settings.floating = visible;
        config::save(&self.data_dir, &settings)?;
        self.settings.lock().unwrap().floating = visible;
        self.state.lock().unwrap().settings.floating = visible;
        self.refresh_indicator();
        self.emit();
        Ok(())
    }

    fn refresh_indicator(&self) {
        let visible = self.settings.lock().unwrap().floating;
        if let Some(item) = self.floating_menu.get() {
            let _ = item.set_checked(visible);
        }
        #[cfg(target_os = "macos")]
        crate::macos_panel::set_visible(&self.app, visible);
        #[cfg(not(target_os = "macos"))]
        if let Some(window) = self.app.get_webview_window("indicator") {
            if visible {
                let _ = window.show();
            } else {
                let _ = window.hide();
            }
        }
    }

    fn refresh_escape_binding(&self) {
        if !platform::wayland() {
            let app = self.app.clone();
            let _ = self.app.run_on_main_thread(move || {
                let active = matches!(
                    app.state::<Arc<Core>>()
                        .state
                        .lock()
                        .unwrap()
                        .phase
                        .as_str(),
                    "starting" | "recording"
                );
                if active {
                    if !app.global_shortcut().is_registered("Escape") {
                        let _ = app.global_shortcut().register("Escape");
                    }
                } else {
                    let _ = app.global_shortcut().unregister("Escape");
                }
            });
        }
    }

    pub fn position_indicator(&self) {
        // Center only at startup; native dragging owns the position for the rest of the session.
        if let Some(window) = self.app.get_webview_window("indicator") {
            if let Ok(Some(monitor)) = window.current_monitor() {
                let size = monitor.size();
                let position = monitor.position();
                let scale = monitor.scale_factor();
                let _ = window.set_position(tauri::PhysicalPosition::new(
                    position.x + (size.width as i32 - (260. * scale) as i32) / 2,
                    position.y + size.height as i32 - (140. * scale) as i32,
                ));
            }
        }
        self.refresh_indicator();
    }

    pub fn show_view(&self, view: View) {
        let title = match view {
            View::Settings => "Speak — Settings",
            View::Test => "Speak — Test notepad",
        };
        self.state.lock().unwrap().view = view;
        self.emit();
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.set_title(title);
            let _ = window.show();
            let _ = window.set_focus();
        }
    }

    pub fn shutdown(&self) {
        self.runtime_cancel.lock().unwrap().cancel();
        crate::runtime::stop_owned_processes();
        self.cancel();
        if let Some(cancel) = self.shortcut_cancel.lock().unwrap().take() {
            cancel.cancel();
        }
        self.runtime.lock().unwrap().take();
    }

    pub fn begin_update(&self) -> Result<()> {
        let _action = self.actions.lock().unwrap();
        if self.state.lock().unwrap().phase != "idle" || self.session.lock().unwrap().is_some() {
            bail!(
                "Finish or discard dictation and wait for model setup before installing an update"
            );
        }
        self.state.lock().unwrap().phase = "updating".into();
        self.state.lock().unwrap().message = "Downloading and verifying update…".into();
        drop(_action);
        self.emit();
        Ok(())
    }

    pub fn ensure_setup_idle(&self) -> Result<()> {
        if !matches!(self.state.lock().unwrap().phase.as_str(), "idle" | "error") {
            bail!("Finish dictation or updating before changing models");
        }
        Ok(())
    }

    pub fn update_failed(&self, message: String) {
        self.state.lock().unwrap().phase = "idle".into();
        self.state.lock().unwrap().ready = self.runtime.lock().unwrap().is_some();
        self.notice(format!("Update failed: {message}"));
    }

    pub fn stop_runtime_for_update(&self) {
        self.runtime_cancel.lock().unwrap().cancel();
        self.runtime.lock().unwrap().take();
        self.state.lock().unwrap().ready = false;
    }
}

type CommandResult<T> = std::result::Result<T, String>;
fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub fn snapshot(core: State<'_, Arc<Core>>) -> Snapshot {
    core.state.lock().unwrap().clone()
}
#[tauri::command]
pub fn open_test_ui(core: State<'_, Arc<Core>>) {
    core.show_view(View::Test);
}
#[tauri::command]
pub fn open_settings(core: State<'_, Arc<Core>>) {
    core.show_view(View::Settings);
}
#[tauri::command]
pub fn microphones() -> CommandResult<Vec<String>> {
    audio::microphones().map_err(message)
}
#[tauri::command]
pub fn press(core: State<'_, Arc<Core>>, test: bool) -> CommandResult<()> {
    let result = core.inner().begin(test);
    if let Err(ref error) = result {
        core.notice(error.to_string());
    }
    result.map_err(message)
}
#[tauri::command]
pub fn release(core: State<'_, Arc<Core>>) {
    core.release();
}
#[tauri::command]
pub fn cancel(core: State<'_, Arc<Core>>) {
    core.cancel();
}
#[tauri::command]
pub fn close_indicator(core: State<'_, Arc<Core>>) -> CommandResult<()> {
    core.set_floating(false).map_err(message)?;
    core.cancel();
    Ok(())
}
#[tauri::command]
pub fn discard(core: State<'_, Arc<Core>>) {
    core.cancel();
}
#[tauri::command]
pub async fn start_model(core: State<'_, Arc<Core>>) -> CommandResult<()> {
    let result = core.start_runtime().await;
    if let Err(ref error) = result {
        core.runtime_error(error.to_string());
    }
    result.map_err(message)
}
#[tauri::command]
pub async fn recheck_acceleration(core: State<'_, Arc<Core>>) -> CommandResult<()> {
    core.cpu_recovery.store(false, Ordering::Relaxed);
    let result = core.start_runtime().await;
    if let Err(ref error) = result {
        core.runtime_error(error.to_string());
    }
    result.map_err(message)
}
#[tauri::command]
pub async fn enable_input(core: State<'_, Arc<Core>>) -> CommandResult<()> {
    core.enable_input().await.map_err(message)
}
#[tauri::command]
pub fn save_settings(core: State<'_, Arc<Core>>, settings: Settings) -> CommandResult<()> {
    if !matches!(core.state.lock().unwrap().phase.as_str(), "idle" | "error") {
        return Err("Finish dictation before changing settings".into());
    }
    config::validate(&settings).map_err(message)?;
    #[cfg(not(target_os = "linux"))]
    settings
        .shortcut
        .parse::<tauri_plugin_global_shortcut::Shortcut>()
        .map_err(message)?;
    let old = core.settings.lock().unwrap().clone();
    *core.settings.lock().unwrap() = settings.clone();
    if let Err(error) = core.inner().register_shortcut() {
        *core.settings.lock().unwrap() = old;
        let _ = core.inner().register_shortcut();
        return Err(error.to_string());
    }
    if let Err(error) = config::save(&core.data_dir, &settings) {
        return Err(error.to_string());
    }
    {
        let mut state = core.state.lock().unwrap();
        state.model_setup_needed = settings.needs_model_setup();
        state.settings = settings;
    }
    core.refresh_indicator();
    core.emit();
    Ok(())
}
#[tauri::command]
pub async fn retry(core: State<'_, Arc<Core>>) -> CommandResult<()> {
    core.retry_pending().await.map_err(message)
}

impl Core {
    async fn retry_pending(&self) -> Result<()> {
        let core = self;
        let session = {
            let _action = core.actions.lock().unwrap();
            let session = core
                .session
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| anyhow!("No pending recording"))?;
            if core.state.lock().unwrap().phase != "error" || !session.ended.load(Ordering::Relaxed)
            {
                bail!("Wait until recording has stopped");
            }
            core.state.lock().unwrap().phase = "processing".into();
            session
        };
        core.emit();
        let result = async {
            let inference = core.inference()?;
            *session.fault.lock().unwrap() = None;
            let count = session.chunks.lock().unwrap().len();
            for index in 0..count {
                core.transcribe_chunk(&session, index, &inference).await?;
            }
            core.finalize(&session, &inference).await
        }
        .await;
        if let Err(ref error) = result {
            core.error(session.id, error.to_string());
        }
        result
    }
}
#[tauri::command]
pub async fn insert_result(core: State<'_, Arc<Core>>) -> CommandResult<()> {
    let (session, text) = {
        let _action = core.actions.lock().unwrap();
        let session = core
            .session
            .lock()
            .unwrap()
            .clone()
            .ok_or("No pending result")?;
        let text = {
            let mut state = core.state.lock().unwrap();
            if state.phase != "error" || state.result.is_empty() {
                return Err("No pending polished text to insert".into());
            }
            state.phase = "processing".into();
            state.result.clone()
        };
        (session, text)
    };
    core.notice("Focus the intended text field. Insertion begins in three seconds.".into());
    let result = async {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if session.cancel.is_cancelled() {
            bail!("Insertion cancelled");
        }
        let target = platform::target()?;
        platform::wait_modifiers().await?;
        if session.cancel.is_cancelled() {
            bail!("Insertion cancelled");
        }
        core.type_text(text, target, session.cancel.clone()).await?;
        core.complete_session(session.id);
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(ref error) = result {
        core.error(session.id, error.to_string());
    }
    result.map_err(message)
}

impl Core {
    pub fn apply_download(&self, settings: Settings) -> Result<()> {
        if !matches!(self.state.lock().unwrap().phase.as_str(), "idle" | "error") {
            bail!("Model files were downloaded. Finish dictation, then select them in Settings.");
        }
        config::save(&self.data_dir, &settings)?;
        *self.settings.lock().unwrap() = settings.clone();
        {
            let mut state = self.state.lock().unwrap();
            state.model_setup_needed = settings.needs_model_setup();
            state.settings = settings;
        }
        self.emit();
        Ok(())
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    #[test]
    fn settings_never_receive_dictation_as_a_hidden_test_notepad() {
        assert!(View::Settings.test_target(false, true).is_err());
        assert!(View::Test.test_target(false, true).unwrap());
        assert!(!View::Test.test_target(false, false).unwrap());
        assert!(!View::Settings.test_target(false, false).unwrap());
        assert!(View::Settings.test_target(true, true).unwrap());
    }
}
#[tauri::command]
pub fn save_audio(core: State<'_, Arc<Core>>, path: String) -> CommandResult<()> {
    let session = core
        .session
        .lock()
        .unwrap()
        .clone()
        .ok_or("No pending recording")?;
    let chunks = session.chunks.lock().unwrap();
    let rate = chunks.first().ok_or("No audio was captured")?.rate;
    let samples = chunks
        .iter()
        .flat_map(|chunk| chunk.samples.iter().copied())
        .collect::<Vec<_>>();
    let bytes = inference::wav(&samples, rate).map_err(message)?;
    std::fs::write(path, bytes).map_err(message)
}
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> CommandResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(message)
}
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> CommandResult<bool> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(message)
}
