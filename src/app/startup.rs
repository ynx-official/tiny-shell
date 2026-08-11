use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gpui::{App, AppContext as _, Bounds, Entity, WindowOptions, point, px, size};
use gpui_component::{Root, WindowExt as _, button::ButtonVariant, dialog::DialogButtonProps};
use rust_i18n::t;

use crate::TinyShell;
use crate::{
    app::session_actions::GroupTransfer,
    session::{
        config::{ConfigStore, Session},
        store::{SessionStore, WindowOwnerId},
    },
};

static STARTUP_UPDATE_CHECK_STARTED: AtomicBool = AtomicBool::new(false);

impl TinyShell {
    pub(crate) fn request_main_window_close(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.close_finalized || self.close_prompt_open || self.pending_close_window.is_some() {
            return;
        }

        self.close_prompt_open = true;
        self.begin_close_sync(cx);
        let owner = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _dialog_window, _| {
            dialog
                .title(t!("close_window_confirm_title").to_string())
                .description(t!("close_window_confirm_desc").to_string())
                .button_props(
                    DialogButtonProps::default()
                        .cancel_text(t!("cancel").to_string())
                        .show_cancel(true)
                        .ok_text(t!("close_window_confirm").to_string())
                        .ok_variant(ButtonVariant::Danger),
                )
                .on_close({
                    let owner = owner.clone();
                    move |_, _, cx| {
                        owner.update(cx, |this, _| {
                            this.close_prompt_open = false;
                        });
                    }
                })
                .on_cancel({
                    let owner = owner.clone();
                    move |_, _, cx| {
                        owner.update(cx, |this, _| {
                            this.close_prompt_open = false;
                            this.pending_close_window = None;
                        });
                        true
                    }
                })
                .on_ok({
                    let owner = owner.clone();
                    move |_, window, cx| {
                        // Do not call `AnyWindowHandle::update` while the
                        // dialog button is still dispatching on this window.
                        // GPUI rejects that re-entrant update, which used to
                        // leave the confirmation dialog closed but the main
                        // window still open when sync had already completed.
                        let owner_for_deferred = owner.clone();
                        window.defer(cx, move |window, cx| {
                            owner_for_deferred.update(cx, |this, cx| {
                                this.confirm_close_after_sync_in_window(window, cx);
                            });
                        });
                        true
                    }
                })
        });
    }

    pub(crate) fn approve_pending_close(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(window) = self.pending_close_window.take() else {
            return;
        };
        let owner = cx.entity();
        if let Err(error) = window.update(cx, move |_, window, cx| {
            window.defer(cx, move |window, cx| {
                owner.update(cx, |this, cx| {
                    this.finalize_main_window_close(window, cx);
                });
                window.remove_window();
            });
        }) {
            self.close_prompt_open = false;
            tracing::debug!("failed to finish closing the main window: {error:?}");
        }
    }

    /// Complete an already-confirmed close from the window's deferred callback.
    /// This avoids a nested handle update when the sync had finished before the
    /// user pressed the confirmation button.
    pub(crate) fn confirm_close_after_sync_in_window(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.close_prompt_open = false;
        self.pending_close_window = Some(window.window_handle());
        if self.close_sync_completed {
            self.pending_close_window = None;
            self.finalize_main_window_close(window, cx);
            window.remove_window();
        } else {
            self.continue_queued_close_sync(cx);
        }
    }

    pub(crate) fn finalize_main_window_close(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.close_finalized {
            return;
        }
        self.close_finalized = true;
        self.close_prompt_open = false;

        let mut close_report = crate::app::config_persistence::CloseErrorReport::default();
        self.cancel_tab_drag(cx);
        if let Err(error) = self.persist_config_preferences_checked() {
            close_report.record("preferences", error);
        }
        if let Err(error) = self.save_layout_state_checked(window, cx) {
            close_report.record("layout", error);
        }
        self.cleanup_on_window_close();
        crate::app::close_auxiliary_windows(self.session_owner_id, cx);
        if let Some(lease) = self.window_lease.take()
            && let Err(error) = self.config_repository.close_window(lease)
        {
            close_report.record("close_window", error);
        }
        close_report.log();
        crate::app::deregister_window(window.window_handle(), cx);
        cx.notify();
    }
}

pub(crate) fn bind_workspace_keys(cx: &mut gpui::App) {
    let config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
    crate::app::keybinding_recorder::bind_workspace_keys_from_config(cx, &config);
}

struct LocalMinutelyRoller {
    dir: std::path::PathBuf,
    prefix: String,
    current_minute: u32,
    file: Option<std::fs::File>,
}

impl LocalMinutelyRoller {
    fn new(dir: std::path::PathBuf, prefix: String) -> Self {
        Self {
            dir,
            prefix,
            current_minute: 60,
            file: None,
        }
    }

    fn rollover(&mut self, now: chrono::DateTime<chrono::Local>) -> std::io::Result<()> {
        use chrono::Timelike;
        let minute = now.minute();
        if self.current_minute != minute || self.file.is_none() {
            let filename = format!("{}-{}.log", self.prefix, now.format("%Y-%m-%d-%H-%M"));
            let path = self.dir.join(filename);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            self.file = Some(file);
            self.current_minute = minute;

            // Cleanup old files keeping last 6
            if let Ok(entries) = std::fs::read_dir(&self.dir) {
                let mut files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&self.prefix))
                    .collect();
                files.sort_by_key(|e| {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                });
                if files.len() > 6 {
                    for file in files.iter().take(files.len() - 6) {
                        let _ = std::fs::remove_file(file.path());
                    }
                }
            }
        }
        Ok(())
    }
}

impl std::io::Write for LocalMinutelyRoller {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let now = chrono::Local::now();
        let _ = self.rollover(now);
        if let Some(f) = &mut self.file {
            f.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(f) = &mut self.file {
            f.flush()
        } else {
            Ok(())
        }
    }
}

pub(crate) fn init_logging() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let log_dir = directories::BaseDirs::new()
        .map(|dirs| {
            dirs.home_dir()
                .join(".config")
                .join("tiny-shell")
                .join("log")
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    std::fs::create_dir_all(&log_dir).ok();

    let roller = LocalMinutelyRoller::new(log_dir.clone(), "tiny-shell".to_string());

    let (non_blocking, _guard) = tracing_appender::non_blocking(roller);
    // Leak the guard so it lives for the entire duration of the app since GPUI's run might not return
    std::mem::forget(_guard);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let stdout_layer = if cfg!(debug_assertions) {
        Some(
            tracing_subscriber::fmt::layer()
                .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
                .with_target(true),
        )
    } else {
        None
    };

    let file_layer = tracing_subscriber::fmt::layer()
        .with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();
}

#[cfg(target_os = "macos")]
pub(crate) fn sync_macos_launch_environment() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let Ok(output) = std::process::Command::new(&shell)
        .args(["-l", "-c", "env -0"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }

    for entry in output.stdout.split(|b| *b == 0) {
        if entry.is_empty() {
            continue;
        }
        let Some(eq) = entry.iter().position(|b| *b == b'=') else {
            continue;
        };
        let Ok(key) = std::str::from_utf8(&entry[..eq]) else {
            continue;
        };
        let Ok(value) = std::str::from_utf8(&entry[eq + 1..]) else {
            continue;
        };

        let should_import = matches!(
            key,
            "PATH"
                | "MANPATH"
                | "INFOPATH"
                | "LANG"
                | "LC_ALL"
                | "LC_CTYPE"
                | "SHELL"
                | "HOME"
                | "HOMEBREW_PREFIX"
                | "HOMEBREW_CELLAR"
                | "HOMEBREW_REPOSITORY"
                | "HTTP_PROXY"
                | "HTTPS_PROXY"
                | "ALL_PROXY"
                | "http_proxy"
                | "https_proxy"
                | "all_proxy"
        ) || key.starts_with("LC_");

        if should_import {
            unsafe {
                std::env::set_var(key, value);
            }
        }
    }
}

fn read_proxy_from_env() -> Option<(String, String, Option<u16>, String, String)> {
    let vars = [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ];
    for var in vars {
        if let Ok(val) = std::env::var(var) {
            if val.is_empty() {
                continue;
            }
            if let Ok(url) = reqwest::Url::parse(&val) {
                let scheme = url.scheme();
                let proxy_type = match scheme {
                    "socks5" | "socks5h" => "socks5".to_string(),
                    "http" | "https" => "http".to_string(),
                    _ => "socks5".to_string(),
                };
                let host = url.host_str().unwrap_or("").to_string();
                let port = url.port();
                let user = url.username().to_string();
                let password = url.password().unwrap_or("").to_string();
                return Some((proxy_type, host, port, user, password));
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn sync_macos_launch_environment() {}

pub(crate) fn open_main_window(cx: &mut App) {
    if let Err(error) = ConfigStore::initialize_temp_workspace() {
        tracing::warn!("failed to initialize temporary workspace: {error:#}");
    }
    let config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());

    let _ = crate::session::config::ENV_PROXY.get_or_init(|| {
        read_proxy_from_env().map(|(proxy_type, host, port, user, password)| {
            tracing::info!(
                "[proxy] Loaded proxy configuration from environment: type={}, host={}, port={:?}, user={}",
                proxy_type,
                host,
                port,
                user
            );
            crate::session::config::EnvProxy {
                proxy_type,
                host,
                port,
                user,
                pass: password,
            }
        })
    });

    let window_options = build_window_options(&config, cx, None);
    let session_store = cx.new(|_| SessionStore::new());
    let config_repository = crate::app::config_persistence::ConfigRepository::new();
    open_window_with_options(window_options, None, session_store, config_repository, cx);
}

/// Open a new window, optionally auto-connecting to a session.
pub(crate) fn open_new_window(
    session: Option<Session>,
    session_store: Option<Entity<SessionStore>>,
    config_repository: Arc<crate::app::config_persistence::ConfigRepository>,
    cx: &mut App,
) {
    let config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
    // Offset new windows so they don't completely overlap
    let offset = Some((px(40.), px(40.)));
    let window_options = build_window_options(&config, cx, offset);
    let session_store = session_store.unwrap_or_else(|| cx.new(|_| SessionStore::new()));
    open_window_with_options(
        window_options,
        session,
        session_store,
        config_repository,
        cx,
    );
}

fn build_window_options(
    config: &ConfigStore,
    cx: &App,
    offset: Option<(gpui::Pixels, gpui::Pixels)>,
) -> WindowOptions {
    let mut window_options = WindowOptions::default();

    if config.title_bar_style() == crate::session::config::TitleBarStyle::Integrated {
        window_options.titlebar = Some(gpui::TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
        });
    }

    #[cfg(not(target_os = "macos"))]
    if let Ok(img) = image::load_from_memory(include_bytes!("../../assets/icons/tiny-shell.png")) {
        window_options.icon = Some(std::sync::Arc::new(img.into_rgba8()));
    }

    if let Some(bounds) = config.window_bounds() {
        window_options.window_bounds = Some(match bounds {
            crate::session::config::SavedWindowBounds::Fullscreen {
                x,
                y,
                width,
                height,
            } => gpui::WindowBounds::Fullscreen(Bounds::new(
                point(px(*x), px(*y)),
                size(px(*width), px(*height)),
            )),
            crate::session::config::SavedWindowBounds::Maximized {
                x,
                y,
                width,
                height,
            } => gpui::WindowBounds::Maximized(Bounds::new(
                point(px(*x), px(*y)),
                size(px(*width), px(*height)),
            )),
            crate::session::config::SavedWindowBounds::Windowed {
                x,
                y,
                width,
                height,
            } => {
                let (mx, my) = offset.unwrap_or((px(0.), px(0.)));
                gpui::WindowBounds::Windowed(Bounds::new(
                    point(px(*x) + mx, px(*y) + my),
                    size(px(*width), px(*height)),
                ))
            }
        });
    } else if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let width = display_bounds.size.width * 0.8;
        let height = display_bounds.size.height * 0.9;

        let x = display_bounds.origin.x + (display_bounds.size.width - width) / 2.0;

        #[cfg(target_os = "macos")]
        let y = display_bounds.origin.y;
        #[cfg(not(target_os = "macos"))]
        let y = display_bounds.origin.y + (display_bounds.size.height - height) / 2.0;

        let (ox, oy) = offset.unwrap_or((px(0.), px(0.)));
        window_options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            point(x + ox, y + oy),
            size(width, height),
        )));
    }

    window_options
}

fn open_window_with_options(
    window_options: WindowOptions,
    session: Option<Session>,
    session_store: Entity<SessionStore>,
    config_repository: Arc<crate::app::config_persistence::ConfigRepository>,
    cx: &mut App,
) {
    if let Err(error) = open_window_with_initializer(
        window_options,
        session_store,
        config_repository,
        move |view, cx| {
            if let Some(session) = session {
                view.update(cx, |this, cx| this.open_ssh_session(session, cx));
            }
            true
        },
        cx,
    ) {
        tracing::error!(%error, "failed to open window");
    }
}

pub(crate) fn open_new_window_with_group(
    transfer: GroupTransfer,
    source_owner_id: WindowOwnerId,
    session_store: Entity<SessionStore>,
    config_repository: Arc<crate::app::config_persistence::ConfigRepository>,
    cx: &mut App,
) -> Result<(), (String, Box<GroupTransfer>)> {
    let config = ConfigStore::load().unwrap_or_else(|_| ConfigStore::in_memory());
    let window_options = build_window_options(&config, cx, Some((px(40.), px(40.))));
    let (target_window, target) = match open_window_with_initializer(
        window_options,
        session_store,
        config_repository,
        |_view, _cx| false,
        cx,
    ) {
        Ok(opened) => opened,
        Err(message) => return Err((message, Box::new(transfer))),
    };

    // GPUI performs the first Windows draw inside open_window before returning.
    // Keep that draw free of transferred live terminal state; moving the group
    // after the native window is fully registered avoids a Windows draw-time
    // fail-fast during detach.
    match target.update(cx, |this, cx| {
        this.receive_group_transfer(transfer, source_owner_id, cx)
    }) {
        Ok(()) => {
            let focus_handle = target.read(cx).focus_handle.clone();
            crate::app::activate_window_with_retry(target_window, focus_handle, cx);
            Ok(())
        }
        Err((message, transfer)) => {
            if let Err(error) = target_window.update(cx, |_, window, cx| {
                target.update(cx, |this, cx| {
                    this.finalize_main_window_close(window, cx);
                });
                window.remove_window();
            }) {
                tracing::warn!(
                    "[ui] failed to close empty window after group transfer failure: {error:?}"
                );
            }
            Err((message, transfer))
        }
    }
}

fn open_window_with_initializer(
    window_options: WindowOptions,
    session_store: Entity<SessionStore>,
    config_repository: Arc<crate::app::config_persistence::ConfigRepository>,
    initialize: impl FnOnce(Entity<TinyShell>, &mut App) -> bool + 'static,
    cx: &mut App,
) -> Result<(gpui::AnyWindowHandle, Entity<TinyShell>), String> {
    let lease = config_repository
        .register_window()
        .map_err(|error| format!("failed to register config window: {error:#}"))?;
    let opened_view = Rc::new(RefCell::new(None));
    let opened_view_for_window = opened_view.clone();
    let handle = match cx.open_window(window_options, |window, cx| {
        window.set_window_title(&t!("app_name"));
        let view = cx.new(|cx| {
            TinyShell::new(
                window,
                session_store.clone(),
                config_repository.clone(),
                lease,
                cx,
            )
        });

        crate::app::register_window(window.window_handle(), view.clone());
        let should_activate = initialize(view.clone(), cx);
        if !STARTUP_UPDATE_CHECK_STARTED.swap(true, Ordering::AcqRel) {
            let window_handle = window.window_handle();
            view.update(cx, |this, cx| {
                this.schedule_automatic_update_checks(window_handle, true, cx);
                this.schedule_automatic_sync(false, cx);
            });
        }

        tracing::info!("[ui] application window opened");
        if should_activate {
            let focus_handle = view.read(cx).focus_handle.clone();
            // A newly created native window is already activated by Windows. Calling
            // `activate_window` here makes GPUI synthesize a global Alt key press on
            // Windows to obtain foreground permission, which can wake unrelated apps
            // that own global shortcuts. Only establish GPUI's internal focus here;
            // forced activation remains reserved for merging into an existing window.
            window.focus(&focus_handle, cx);
        }

        let view_clone = view.clone();
        window.on_window_should_close(cx, move |window: &mut gpui::Window, cx: &mut gpui::App| {
            view_clone.update(cx, |this, cx| {
                if this.close_finalized {
                    return true;
                }
                this.request_main_window_close(window, cx);
                // Both native title-bar closing and the custom title-bar button are
                // completed asynchronously after confirmation and WebDAV sync.
                false
            })
        });

        *opened_view_for_window.borrow_mut() = Some(view.clone());
        cx.new(|cx| Root::new(view, window, cx))
    }) {
        Ok(handle) => handle,
        Err(error) => {
            if let Err(cleanup_error) = config_repository.close_window(lease) {
                tracing::warn!("failed to clean up config window lease: {cleanup_error:#}");
            }
            return Err(format!("failed to open window: {error:?}"));
        }
    };
    let view = match opened_view.borrow_mut().take() {
        Some(view) => view,
        None => {
            if let Err(cleanup_error) = config_repository.close_window(lease) {
                tracing::warn!("failed to clean up config window lease: {cleanup_error:#}");
            }
            return Err("new window did not create its main view".to_string());
        }
    };
    Ok((handle.into(), view))
}
