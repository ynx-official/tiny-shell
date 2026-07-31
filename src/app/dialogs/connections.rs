use super::*;

impl TinyShell {
    #[allow(dead_code)]
    pub(crate) fn confirm_connection_group_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self
            .connection_group_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if name.is_empty() {
            return;
        }
        if let Some(old_name) = self.editing_connection_group.clone() {
            let full_name = self
                .connection_group_parent
                .as_deref()
                .map(|parent| format!("{parent}/{name}"))
                .unwrap_or(name.clone());
            self.config
                .rename_connection_group(&old_name, full_name.clone());
            if self.connection_group_filter.as_deref() == Some(old_name.as_str()) {
                self.connection_group_filter = Some(full_name);
            }
        } else {
            let full_name = self
                .connection_group_parent
                .as_deref()
                .map(|parent| format!("{parent}/{name}"))
                .unwrap_or(name.clone());
            self.config.add_connection_group(full_name.clone());
            self.connection_group_filter = Some(full_name);
        }
        if let Err(err) = crate::app::config_persistence::save_full(&self.config) {
            tracing::warn!("failed to save connection group: {err:#}");
        }
        self.active_dialog = None;
        self.editing_connection_group = None;
        self.connection_group_parent = None;
        window.close_dialog(cx);
        cx.notify();
    }

    #[allow(dead_code)]
    pub(crate) fn show_move_connection_group_dialog(
        &mut self,
        group: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        self.moving_connection_group = Some(group.clone());
        self.active_dialog = Some(crate::app::DialogKind::ConnectionGroupMove);
        let candidates: Vec<String> = self
            .config
            .connection_groups()
            .iter()
            .filter(|candidate| {
                candidate.as_str() != group && !candidate.starts_with(&format!("{group}/"))
            })
            .cloned()
            .collect();
        let view = cx.entity();
        let scroll_handle = self.group_picker_scroll_handle.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("connection_group_move_dialog_title").to_string())
                .w(px(440.))
                .h(px(500.))
                .overlay_closable(true)
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            this.moving_connection_group = None;
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    let scroll_handle = scroll_handle.clone();
                    let group = group.clone();
                    let candidates = candidates.clone();
                    move |content, window, cx| {
                        let group = group.clone();
                        let candidates = candidates.clone();
                        let source_label = group.rsplit('/').next().unwrap_or(&group).to_string();
                        content.child(
                            v_flex()
                                .size_full()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(rems(0.917))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{}: {}", t!("connection_group_move_source"), source_label)),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .child(
                                            v_flex()
                                                .id("connection-group-picker-scroll")
                                                .size_full()
                                                .track_scroll(&scroll_handle)
                                                .overflow_y_scroll()
                                                .p_2()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .id("connection-group-picker-root")
                                                        .w_full()
                                                        .cursor_pointer()
                                                        .rounded_md()
                                                        .hover(|this| this.bg(cx.theme().secondary))
                                                        .on_click(window.listener_for(&view, {
                                                            let group = group.clone();
                                                            move |this, _, window, cx| {
                                                                let mut staged = this.config.clone();
                                                                match crate::session::connection_catalog::move_connection_group(
                                                                    &mut staged,
                                                                    &group,
                                                                    None,
                                                                )
                                                                .and_then(|_| crate::app::config_persistence::save_full(&staged))
                                                                {
                                                                    Ok(()) => {
                                                                        this.config = staged;
                                                                        this.active_dialog = None;
                                                                        this.moving_connection_group = None;
                                                                        window.close_dialog(cx);
                                                                    }
                                                                    Err(error) => {
                                                                        this.status = t!(
                                                                            "connection_manager_action_failed",
                                                                            error = error.to_string()
                                                                        )
                                                                        .to_string()
                                                                        .into();
                                                                    }
                                                                }
                                                                cx.notify();
                                                            }
                                                        }))
                                                        .child(
                                                            h_flex()
                                                                .items_center()
                                                                .gap_2()
                                                                .p_2()
                                                                .child(Icon::new(IconName::Folder).with_size(gpui_component::Size::Small))
                                                                .child(t!("connection_group_move_root")),
                                                        ),
                                                )
                                                .children(candidates.iter().enumerate().map(|(ix, candidate)| {
                                                    let target = candidate.clone();
                                                    let source = group.clone();
                                                    let depth = candidate.matches('/').count();
                                                    let label = candidate.rsplit('/').next().unwrap_or(candidate).to_string();
                                                    div()
                                                        .id(("connection-group-picker", ix))
                                                        .w_full()
                                                        .cursor_pointer()
                                                        .rounded_md()
                                                        .hover(|this| this.bg(cx.theme().secondary))
                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                            let mut staged = this.config.clone();
                                                            match crate::session::connection_catalog::move_connection_group(
                                                                &mut staged,
                                                                &source,
                                                                Some(&target),
                                                            )
                                                                .and_then(|_| crate::app::config_persistence::save_full(&staged))
                                                            {
                                                                Ok(()) => {
                                                                    this.config = staged;
                                                                    this.active_dialog = None;
                                                                    this.moving_connection_group = None;
                                                                    window.close_dialog(cx);
                                                                }
                                                                Err(error) => {
                                                                    this.status = t!(
                                                                        "connection_manager_action_failed",
                                                                        error = error.to_string()
                                                                    )
                                                                    .to_string()
                                                                    .into();
                                                                }
                                                            }
                                                            cx.notify();
                                                        }))
                                                        .child(
                                                            h_flex()
                                                                .items_center()
                                                                .gap_2()
                                                                .p_2()
                                                                .pl(px(8. + depth as f32 * 16.))
                                                                .child(Icon::new(IconName::Folder).with_size(gpui_component::Size::Small))
                                                                .child(label),
                                                        )
                                                })),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .right_0()
                                                .bottom_0()
                                                .w(px(8.))
                                                .child(
                                                    Scrollbar::vertical(&scroll_handle)
                                                        .scrollbar_show(ScrollbarShow::Scrolling),
                                                ),
                                        ),
                                ),
                        )
                    }
                })
        });
    }

    #[allow(dead_code)]
    pub(crate) fn show_move_saved_session_dialog(
        &mut self,
        session_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        let Some(session) = self.config.get(&session_id).cloned() else {
            return;
        };
        self.active_dialog = Some(crate::app::DialogKind::SessionGroupMove);
        let groups = self.config.connection_groups().to_vec();
        let view = cx.entity();
        let scroll_handle = self.group_picker_scroll_handle.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("connection_group_move_to").to_string())
                .w(px(440.))
                .h(px(500.))
                .overlay_closable(true)
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    let scroll_handle = scroll_handle.clone();
                    let groups = groups.clone();
                    let session_id = session_id.clone();
                    let session_name = session.name.clone();
                    move |content, window, cx| {
                        let groups = groups.clone();
                        let session_id = session_id.clone();
                        content.child(
                            v_flex()
                                .size_full()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(rems(0.917))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{}: {}",
                                            t!("connection_group_move_source"),
                                            session_name
                                        )),
                                )
                                .child(
                                    v_flex()
                                        .id("session-group-picker-scroll")
                                        .flex_1()
                                        .min_h(px(0.))
                                        .track_scroll(&scroll_handle)
                                        .overflow_y_scroll()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .p_2()
                                        .gap_1()
                                        .child(
                                            div()
                                                .id("session-group-picker-root")
                                                .w_full()
                                                .cursor_pointer()
                                                .rounded_md()
                                                .hover(|this| this.bg(cx.theme().secondary))
                                                .on_click(window.listener_for(&view, {
                                                    let session_id = session_id.clone();
                                                    move |this, _, window, cx| {
                                                        let mut staged = this.config.clone();
                                                        match crate::session::connection_catalog::move_session(
                                                            &mut staged,
                                                            &session_id,
                                                            None,
                                                        )
                                                                .and_then(|_| crate::app::config_persistence::save_full(&staged))
                                                        {
                                                            Ok(()) => {
                                                                this.config = staged;
                                                                this.active_dialog = None;
                                                                window.close_dialog(cx);
                                                            }
                                                            Err(error) => {
                                                                this.status = t!(
                                                                    "connection_manager_action_failed",
                                                                    error = error.to_string()
                                                                )
                                                                .to_string()
                                                                .into();
                                                            }
                                                        }
                                                        cx.notify();
                                                    }
                                                }))
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .p_2()
                                                        .child(
                                                            Icon::new(IconName::Folder)
                                                                .with_size(gpui_component::Size::Small),
                                                        )
                                                        .child(t!("connection_group_ungrouped")),
                                                ),
                                        )
                                        .children(groups.iter().enumerate().map(|(ix, group)| {
                                            let target = group.clone();
                                            let session_id = session_id.clone();
                                            let depth = group.matches('/').count();
                                            let label = group
                                                .rsplit('/')
                                                .next()
                                                .unwrap_or(group)
                                                .to_string();
                                            div()
                                                .id(("session-group-picker", ix))
                                                .w_full()
                                                .cursor_pointer()
                                                .rounded_md()
                                                .hover(|this| this.bg(cx.theme().secondary))
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, window, cx| {
                                                        let mut staged = this.config.clone();
                                                        match crate::session::connection_catalog::move_session(
                                                            &mut staged,
                                                            &session_id,
                                                            Some(&target),
                                                        )
                                                                .and_then(|_| crate::app::config_persistence::save_full(&staged))
                                                        {
                                                            Ok(()) => {
                                                                this.config = staged;
                                                                this.active_dialog = None;
                                                                window.close_dialog(cx);
                                                            }
                                                            Err(error) => {
                                                                this.status = t!(
                                                                    "connection_manager_action_failed",
                                                                    error = error.to_string()
                                                                )
                                                                .to_string()
                                                                .into();
                                                            }
                                                        }
                                                        cx.notify();
                                                    },
                                                ))
                                                .child(
                                                    h_flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .p_2()
                                                        .pl(px(8. + depth as f32 * 16.))
                                                        .child(
                                                            Icon::new(IconName::Folder)
                                                                .with_size(gpui_component::Size::Small),
                                                        )
                                                        .child(label),
                                                )
                                        })),
                                ),
                        )
                    }
                })
        });
    }

    #[allow(dead_code)]
    pub(crate) fn show_connection_group_dialog(
        &mut self,
        group: Option<String>,
        parent: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        self.editing_connection_group = group.clone();
        self.connection_group_parent = group
            .as_deref()
            .and_then(|path| path.rsplit_once('/').map(|(parent, _)| parent.to_string()))
            .or(parent);
        Self::set_input_value(
            &self.connection_group_input,
            group
                .as_deref()
                .and_then(|path| path.rsplit('/').next())
                .unwrap_or_default(),
            window,
            cx,
        );
        self.active_dialog = Some(crate::app::DialogKind::ConnectionGroup);

        let view = cx.entity();
        let group_input = self.connection_group_input.clone();
        let focus_group_input = group_input.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("connection_group_dialog_title").to_string())
                .w(px(380.))
                .h(px(180.))
                .overlay_closable(true)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.confirm_connection_group_dialog(window, cx);
                        });
                        false
                    }
                })
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            this.editing_connection_group = None;
                            this.connection_group_parent = None;
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    let group_input = group_input.clone();
                    move |content, window, cx| {
                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(rems(0.917))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("connection_group_name")),
                                )
                                .child(Input::new(&group_input).w_full())
                                .child(
                                    h_flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("connection-group-cancel")
                                                .secondary()
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.active_dialog = None;
                                                        this.editing_connection_group = None;
                                                        this.connection_group_parent = None;
                                                        window.close_dialog(cx);
                                                        cx.notify();
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("connection-group-save")
                                                .primary()
                                                .label(t!("save").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.confirm_connection_group_dialog(
                                                            window, cx,
                                                        );
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_group_input, window, cx);
    }
}
