use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Render, Window,
    WindowOptions, point, px, size,
};
use gpui_component::{
    Root,
    input::{InputEvent, InputState},
};
use rust_i18n::t;

use crate::{TinyShell, session::config::ConfigStore};

#[derive(Clone)]
pub(crate) struct ProxySettingsInputs {
    pub(crate) host: Entity<InputState>,
    pub(crate) port: Entity<InputState>,
    pub(crate) user: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
}

#[derive(Clone)]
pub(crate) struct SyncSettingsInputs {
    pub(crate) endpoint: Entity<InputState>,
    pub(crate) username: Entity<InputState>,
    pub(crate) webdav_password: Entity<InputState>,
    pub(crate) s3_endpoint: Entity<InputState>,
    pub(crate) s3_region: Entity<InputState>,
    pub(crate) s3_bucket: Entity<InputState>,
    pub(crate) s3_object_key: Entity<InputState>,
    pub(crate) s3_access_key: Entity<InputState>,
    pub(crate) s3_secret_key: Entity<InputState>,
    pub(crate) s3_session_token: Entity<InputState>,
    pub(crate) privacy_password: Entity<InputState>,
    pub(crate) interval_hours: Entity<InputState>,
}

#[derive(Clone)]
pub(crate) struct UpdateSettingsInputs {
    pub(crate) interval_hours: Entity<InputState>,
}

#[derive(Clone)]
pub(crate) struct SettingsInputs {
    pub(crate) proxy: ProxySettingsInputs,
    pub(crate) sync: SyncSettingsInputs,
    pub(crate) update: UpdateSettingsInputs,
}

impl SettingsInputs {
    pub(crate) fn from_main(owner: &TinyShell) -> Self {
        Self {
            proxy: ProxySettingsInputs {
                host: owner.global_proxy_host_input.clone(),
                port: owner.global_proxy_port_input.clone(),
                user: owner.global_proxy_user_input.clone(),
                password: owner.global_proxy_password_input.clone(),
            },
            sync: SyncSettingsInputs {
                endpoint: owner.sync_endpoint_input.clone(),
                username: owner.sync_username_input.clone(),
                webdav_password: owner.sync_webdav_password_input.clone(),
                s3_endpoint: owner.sync_s3_endpoint_input.clone(),
                s3_region: owner.sync_s3_region_input.clone(),
                s3_bucket: owner.sync_s3_bucket_input.clone(),
                s3_object_key: owner.sync_s3_object_key_input.clone(),
                s3_access_key: owner.sync_s3_access_key_input.clone(),
                s3_secret_key: owner.sync_s3_secret_key_input.clone(),
                s3_session_token: owner.sync_s3_session_token_input.clone(),
                privacy_password: owner.sync_privacy_password_input.clone(),
                interval_hours: owner.sync_interval_hours_input.clone(),
            },
            update: UpdateSettingsInputs {
                interval_hours: owner.update_interval_hours_input.clone(),
            },
        }
    }

    fn new(config: &ConfigStore, window: &mut Window, cx: &mut Context<SettingsWindow>) -> Self {
        let proxy = ProxySettingsInputs {
            host: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_host").to_string())
                    .default_value(config.global_proxy_host())
            }),
            port: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_port").to_string())
                    .default_value(
                        config
                            .global_proxy_port()
                            .map(|port| port.to_string())
                            .unwrap_or_default(),
                    )
            }),
            user: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_user").to_string())
                    .default_value(config.global_proxy_user())
            }),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_password").to_string())
                    .masked(true)
                    .default_value(config.global_proxy_password())
            }),
        };
        let sync = SyncSettingsInputs {
            endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("https://dav.example.com/tiny-shell/")
                    .default_value(config.sync_endpoint())
            }),
            username: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_username").to_string())
                    .default_value(config.sync_username())
            }),
            webdav_password: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_webdav_password").to_string())
                    .masked(true)
                    .default_value(
                        crate::app::config_sync::open_webdav_password(
                            config.sync_webdav_password_sealed(),
                        )
                        .unwrap_or_default(),
                    )
            }),
            s3_endpoint: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("https://s3.example.com")
                    .default_value(config.sync_s3_endpoint())
            }),
            s3_region: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("us-east-1")
                    .default_value(config.sync_s3_region())
            }),
            s3_bucket: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_bucket").to_string())
                    .default_value(config.sync_s3_bucket())
            }),
            s3_object_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("tiny-shell-sync.json")
                    .default_value(config.sync_s3_object_key())
            }),
            s3_access_key: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("sync_s3_access_key").to_string())
            }),
            s3_secret_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_secret_key").to_string())
                    .masked(true)
            }),
            s3_session_token: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("sync_s3_session_token").to_string())
                    .masked(true)
            }),
            privacy_password: cx.new(|cx| {
                let mut state = InputState::new(window, cx)
                    .placeholder(t!("sync_privacy_password").to_string())
                    .masked(true);
                if !config.sync_secrets_password_sealed().is_empty() {
                    let hw = crate::session::config::hardware_uuid();
                    if let Ok(plaintext) = crate::crypto::open_with_hardware_key(
                        config.sync_secrets_password_sealed(),
                        &hw,
                    ) {
                        state = state.default_value(&plaintext);
                    }
                }
                state
            }),
            interval_hours: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("24")
                    .default_value(config.sync_interval_hours().to_string())
            }),
        };
        let update = UpdateSettingsInputs {
            interval_hours: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("24")
                    .default_value(config.update_interval_hours().to_string())
            }),
        };
        Self {
            proxy,
            sync,
            update,
        }
    }
}

pub(crate) struct SettingsWindow {
    owner: Entity<TinyShell>,
    inputs: SettingsInputs,
    focus_handle: FocusHandle,
    _owner_subscription: gpui::Subscription,
    _input_subscriptions: Vec<gpui::Subscription>,
}

impl SettingsWindow {
    fn new(
        owner: Entity<TinyShell>,
        config: ConfigStore,
        main_window: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inputs = SettingsInputs::new(&config, window, cx);
        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());
        let sync_interval_hours = inputs.sync.interval_hours.clone();
        let owner_for_sync_interval = owner.clone();
        let sync_interval_subscription = cx.subscribe_in(
            &sync_interval_hours,
            window,
            move |_, input, event, window, cx| match event {
                InputEvent::Change => {
                    if let Ok(hours) = input.read(cx).value().trim().parse::<u32>()
                        && (1..=8_760).contains(&hours)
                    {
                        owner_for_sync_interval.update(cx, |this, cx| {
                            this.config.set_sync_interval_hours(hours);
                            this.mark_config_preferences_dirty();
                            this.schedule_automatic_sync(false, cx);
                            cx.notify();
                        });
                    }
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    let hours = owner_for_sync_interval
                        .read(cx)
                        .config
                        .sync_interval_hours()
                        .to_string();
                    input.update(cx, |input, cx| input.set_value(hours, window, cx));
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
                _ => {}
            },
        );
        let update_interval_hours = inputs.update.interval_hours.clone();
        let owner_for_interval = owner.clone();
        let interval_subscription = cx.subscribe_in(
            &update_interval_hours,
            window,
            move |_, input, event, window, cx| match event {
                InputEvent::Change => {
                    if let Ok(hours) = input.read(cx).value().trim().parse::<u32>()
                        && (1..=8_760).contains(&hours)
                    {
                        owner_for_interval.update(cx, |this, cx| {
                            this.config.set_update_interval_hours(hours);
                            this.mark_config_preferences_dirty();
                            this.schedule_automatic_update_checks(main_window, false, cx);
                            cx.notify();
                        });
                    }
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    let hours = owner_for_interval
                        .read(cx)
                        .config
                        .update_interval_hours()
                        .to_string();
                    input.update(cx, |input, cx| input.set_value(hours, window, cx));
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
                _ => {}
            },
        );
        let mut input_subscriptions: Vec<_> = [
            inputs.sync.endpoint.clone(),
            inputs.sync.username.clone(),
            inputs.sync.webdav_password.clone(),
            inputs.sync.s3_endpoint.clone(),
            inputs.sync.s3_region.clone(),
            inputs.sync.s3_bucket.clone(),
            inputs.sync.s3_object_key.clone(),
            inputs.sync.s3_access_key.clone(),
            inputs.sync.s3_secret_key.clone(),
            inputs.sync.s3_session_token.clone(),
            inputs.sync.privacy_password.clone(),
        ]
        .into_iter()
        .map(|input| cx.subscribe_in(&input, window, |_, _, _: &InputEvent, _, cx| cx.notify()))
        .collect();
        input_subscriptions.push(sync_interval_subscription);
        input_subscriptions.push(interval_subscription);
        Self {
            owner,
            inputs,
            focus_handle: cx.focus_handle(),
            _owner_subscription: owner_subscription,
            _input_subscriptions: input_subscriptions,
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let owner = self.owner.clone();
        owner.read(cx).render_settings_content(
            &owner,
            "settings-window",
            &self.focus_handle,
            &self.inputs,
            cx,
        )
    }
}

fn window_options(cx: &App) -> WindowOptions {
    let mut options = WindowOptions {
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(size(px(720.), px(520.))),
        ..Default::default()
    };

    if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let window_size = size(
            px(980.).min(display_bounds.size.width * 0.9),
            px(700.).min(display_bounds.size.height * 0.9),
        );
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - window_size.width) / 2.,
            display_bounds.origin.y + (display_bounds.size.height - window_size.height) / 2.,
        );
        options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            window_size,
        )));
    }

    #[cfg(not(target_os = "macos"))]
    if let Ok(image) = image::load_from_memory(include_bytes!("../../assets/icons/tiny-shell.png"))
    {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }

    options
}

pub(crate) fn open(
    owner: Entity<TinyShell>,
    config: ConfigStore,
    main_window: AnyWindowHandle,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    let options = window_options(cx);
    let original_config = config.clone();
    let owner_for_window = owner.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("settings").as_ref());

        let owner_for_close = owner_for_window.clone();
        let original_config = original_config.clone();
        let settings_window = cx.new(|cx| {
            SettingsWindow::new(owner_for_window.clone(), config, main_window, window, cx)
        });
        window.on_window_should_close(cx, move |_window, cx| {
            owner_for_close.update(cx, |this, cx| {
                crate::app::keybinding_recorder::unbind_workspace_keys_from_config(
                    cx,
                    &original_config,
                );
                crate::app::keybinding_recorder::bind_workspace_keys_from_config(cx, &this.config);
                this.settings_window = None;
                this.settings_window_opening = false;
                this.recording_action = None;
                this.keybind_error = None;
                this.persist_config_preferences_async();
                cx.notify();
            });
            true
        });

        cx.new(|cx| Root::new(settings_window, window, cx))
    });

    match opened {
        Ok(handle) => Some(handle.into()),
        Err(error) => {
            tracing::error!("failed to open settings window: {error:?}");
            None
        }
    }
}
