use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Render, Window,
    WindowOptions, px, size,
};
use gpui_component::{Root, input::InputEvent};
use rust_i18n::t;

use crate::{TinyShell, app::settings::form::SettingsInputs, session::config::ConfigStore};

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
                    if let Some(hours) = crate::app::settings::actions::parse_hour_interval(
                        input.read(cx).value().as_ref(),
                    ) {
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
                    if let Some(hours) = crate::app::settings::actions::parse_hour_interval(
                        input.read(cx).value().as_ref(),
                    ) {
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

fn window_options(cx: &mut App) -> WindowOptions {
    crate::app::platform::auxiliary_window_options(
        cx,
        crate::app::platform::AuxiliaryWindowSpec::new(size(px(980.), px(700.)))
            .with_min_size(size(px(720.), px(520.)))
            .with_max_ratio(0.9, 0.9),
    )
}

pub(crate) fn open(
    owner: Entity<TinyShell>,
    config: ConfigStore,
    main_window: AnyWindowHandle,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    let options = window_options(cx);
    let owner_id = owner.read(cx).session_owner_id;
    let original_config = config.clone();
    let owner_for_window = owner.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(t!("settings").as_ref());
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);

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
                this.auxiliary_windows.settings.handle = None;
                this.auxiliary_windows.settings.opening = false;
                this.recording_action = None;
                this.keybind_error = None;
                this.persist_config_preferences_async(cx);
                cx.notify();
            });
            crate::app::deregister_auxiliary_window(window_handle);
            true
        });

        cx.new(|cx| Root::new(settings_window, window, cx))
    });

    match opened {
        Ok(handle) => Some(handle.into()),
        Err(error) => {
            tracing::error!("failed to open settings window: {error:?}");
            crate::feedback::Feedback::show_for_owner(
                &owner,
                cx,
                crate::feedback::FeedbackKind::Error,
                format!("{} · {}: {error:?}", t!("settings"), t!("failed")),
            );
            None
        }
    }
}
