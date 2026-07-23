use gpui::{
    Anchor, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, Size, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::Dialog,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    progress::Progress,
    radio::{Radio, RadioGroup},
    scroll::{Scrollbar, ScrollbarShow},
    switch::Switch,
    v_flex,
};

#[derive(Clone)]
struct QuickCommandDialogInputs {
    name: Entity<InputState>,
    remark: Entity<InputState>,
    command: Entity<InputState>,
}

struct SftpPermissionsForm {
    remote_path: String,
    is_dir: bool,
    mode: u32,
    recursive: bool,
    apply_to: crate::sftp::PermissionApplyTarget,
    input: Entity<InputState>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl SftpPermissionsForm {
    fn new(
        remote_path: String,
        is_dir: bool,
        mode: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(format!("{:03o}", mode & 0o7777))
                .placeholder("755")
        });
        let subscriptions = vec![cx.subscribe_in(&input, window, Self::on_input_event)];
        Self {
            remote_path,
            is_dir,
            mode: mode & 0o7777,
            recursive: false,
            apply_to: crate::sftp::PermissionApplyTarget::FilesAndDirectories,
            input,
            _subscriptions: subscriptions,
        }
    }

    fn on_input_event(
        &mut self,
        input: &Entity<InputState>,
        _: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = input.read(cx).value().trim().to_string();
        if (3..=4).contains(&value.len())
            && let Ok(mode) = u32::from_str_radix(&value, 8)
            && mode <= 0o7777
        {
            self.mode = mode;
            cx.notify();
        }
    }

    fn permission_checkbox(&self, id: &'static str, bit: u32, cx: &mut Context<Self>) -> Checkbox {
        Checkbox::new(id)
            .checked(self.mode & bit != 0)
            .on_click(cx.listener(move |this, checked, window, cx| {
                if *checked {
                    this.mode |= bit;
                } else {
                    this.mode &= !bit;
                }
                let value = format!("{:03o}", this.mode & 0o7777);
                this.input.update(cx, |input, cx| {
                    input.set_value(value, window, cx);
                });
                cx.notify();
            }))
    }
}

impl Render for SftpPermissionsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .remote_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("/")
            .to_string();
        let parent = TinyShell::sftp_parent_path(&self.remote_path);
        let current_mode = format!("{:03o}", self.mode & 0o7777);
        let recursive = self.recursive;
        let selected_target = match self.apply_to {
            crate::sftp::PermissionApplyTarget::FilesAndDirectories => 0,
            crate::sftp::PermissionApplyTarget::FilesOnly => 1,
            crate::sftp::PermissionApplyTarget::DirectoriesOnly => 2,
        };

        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .size(px(42.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(if self.is_dir {
                                    IconName::Folder
                                } else {
                                    IconName::File
                                })
                                .with_size(Size::Medium),
                            ),
                    )
                    .child(
                        v_flex()
                            .min_w(px(0.))
                            .gap_1()
                            .child(div().font_weight(FontWeight::BOLD).child(name))
                            .child(
                                div()
                                    .text_size(rems(0.833))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(parent),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("sftp_permission_matrix")),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                h_flex()
                                    .pb_1()
                                    .child(div().flex_1())
                                    .child(div().w(px(56.)).child(t!("sftp_permission_read")))
                                    .child(div().w(px(56.)).child(t!("sftp_permission_write")))
                                    .child(div().w(px(56.)).child(t!("sftp_permission_execute"))),
                            )
                            .child(
                                h_flex()
                                    .h(px(36.))
                                    .px_2()
                                    .rounded_md()
                                    .bg(cx.theme().muted.opacity(0.7))
                                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_owner")))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-owner-read", 0o400, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-owner-write", 0o200, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-owner-execute", 0o100, cx))),
                            )
                            .child(
                                h_flex()
                                    .h(px(36.))
                                    .px_2()
                                    .rounded_md()
                                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_group")))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-group-read", 0o040, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-group-write", 0o020, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-group-execute", 0o010, cx))),
                            )
                            .child(
                                h_flex()
                                    .h(px(36.))
                                    .px_2()
                                    .rounded_md()
                                    .bg(cx.theme().muted.opacity(0.7))
                                    .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_other")))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-other-read", 0o004, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-other-write", 0o002, cx)))
                                    .child(div().w(px(56.)).child(self.permission_checkbox("permission-other-execute", 0o001, cx))),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(0.833))
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("sftp_permission_advanced")),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .p_3()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(div().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_octal")))
                                    .child(div().text_size(rems(0.8)).text_color(cx.theme().muted_foreground).child(t!("sftp_permissions_hint"))),
                            )
                            .child(Input::new(&self.input).w(px(100.)))
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_center()
                                    .font_weight(FontWeight::BOLD)
                                    .child(current_mode),
                            ),
                    ),
            )
            .when(self.is_dir, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            h_flex()
                                .items_center()
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_1()
                                        .child(div().font_weight(FontWeight::MEDIUM).child(t!("sftp_permission_recursive")))
                                        .child(div().text_size(rems(0.8)).text_color(cx.theme().muted_foreground).child(t!("sftp_permission_recursive_hint"))),
                                )
                                .child(
                                    Switch::new("permission-recursive")
                                        .checked(recursive)
                                        .on_click(cx.listener(|this, checked, _, cx| {
                                            this.recursive = *checked;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when(recursive, |this| {
                            this.child(
                                RadioGroup::vertical("permission-recursive-target")
                                    .selected_index(Some(selected_target))
                                    .on_click(cx.listener(|this, index, _, cx| {
                                        this.apply_to = match *index {
                                            1 => crate::sftp::PermissionApplyTarget::FilesOnly,
                                            2 => crate::sftp::PermissionApplyTarget::DirectoriesOnly,
                                            _ => crate::sftp::PermissionApplyTarget::FilesAndDirectories,
                                        };
                                        cx.notify();
                                    }))
                                    .child(Radio::new("permission-all").label(t!("sftp_permission_apply_all").to_string()))
                                    .child(Radio::new("permission-files").label(t!("sftp_permission_apply_files").to_string()))
                                    .child(Radio::new("permission-directories").label(t!("sftp_permission_apply_directories").to_string())),
                            )
                        }),
                )
            })
    }
}
use rust_i18n::t;

use crate::{
    TinyShell,
    app::ssh_key_import::KeyImportValidation,
    session::config::{AuthMethod, QuickCommand, QuickCommandCategory},
    system::format_bytes,
};

impl TinyShell {
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
        if let Err(err) = self.config.save() {
            tracing::warn!("failed to save connection group: {err:#}");
        }
        self.active_dialog = None;
        self.editing_connection_group = None;
        self.connection_group_parent = None;
        window.close_dialog(cx);
        cx.notify();
    }

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
                                                                this.config.move_connection_group(&group, None);
                                                                let _ = this.config.save();
                                                                this.active_dialog = None;
                                                                this.moving_connection_group = None;
                                                                window.close_dialog(cx);
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
                                                            this.config.move_connection_group(&source, Some(&target));
                                                            let _ = this.config.save();
                                                            this.active_dialog = None;
                                                            this.moving_connection_group = None;
                                                            window.close_dialog(cx);
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

    pub(crate) fn show_quick_command_category_dialog(
        &mut self,
        category_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        let existing = category_id.as_deref().and_then(|category_id| {
            self.config
                .quick_command_categories()
                .and_then(|categories| categories.iter().find(|item| item.id == category_id))
                .cloned()
        });
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).default_value(
                existing
                    .as_ref()
                    .map(|item| item.name.as_str())
                    .unwrap_or_default(),
            )
        });
        self.active_dialog = Some(crate::app::DialogKind::QuickCommandCategory);
        let view = cx.entity();
        let submit_input = input.clone();
        let focus_input = input.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            let submit_input = submit_input.clone();
            let content_input = input.clone();
            let existing = existing.clone();
            dialog
                .title(t!("quick_command_category_dialog_title").to_string())
                .w(px(420.))
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            cx.notify();
                        });
                    }
                })
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        let name = submit_input.read(cx).value().trim().to_string();
                        if name.is_empty() {
                            view.update(cx, |this, cx| {
                                this.status = t!("quick_command_category_name_required").into();
                                cx.notify();
                            });
                            return false;
                        }
                        view.update(cx, |this, cx| {
                            let category =
                                existing.clone().unwrap_or_else(|| QuickCommandCategory {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    name: String::new(),
                                    commands: Vec::new(),
                                });
                            let category = QuickCommandCategory { name, ..category };
                            this.command_category_filter = Some(category.id.clone());
                            this.config.upsert_quick_command_category(category);
                            this.mark_config_preferences_dirty();
                            this.active_dialog = None;
                            cx.notify();
                        });
                        window.close_dialog(cx);
                        true
                    }
                })
                .content(move |content, _, cx| {
                    content.child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("quick_command_category_name")),
                            )
                            .child(Input::new(&content_input).w_full()),
                    )
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn confirm_quick_command_dialog(
        &mut self,
        source_category_id: String,
        existing: Option<QuickCommand>,
        inputs: &QuickCommandDialogInputs,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = inputs.name.read(cx).value().trim().to_string();
        let remark = inputs.remark.read(cx).value().trim().to_string();
        let command_text = inputs.command.read(cx).value().trim().to_string();
        if name.is_empty() || command_text.is_empty() {
            self.status = t!("quick_command_fields_required").into();
            cx.notify();
            return false;
        }
        let Some(category_id) = self.editing_quick_command_category.clone() else {
            self.status = t!("quick_command_category_name_required").into();
            cx.notify();
            return false;
        };
        let is_new_command = existing.is_none();
        let command = existing.unwrap_or_else(|| QuickCommand {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            remark: String::new(),
            command: String::new(),
        });
        let command_id = command.id.clone();
        let edited_command_was_selected = self
            .selected_quick_command
            .as_ref()
            .is_some_and(|(_, selected_command_id)| selected_command_id == &command_id);
        if source_category_id != category_id {
            self.config
                .remove_quick_command(&source_category_id, &command_id);
        }
        self.config.upsert_quick_command(
            &category_id,
            QuickCommand {
                name,
                remark,
                command: command_text,
                ..command
            },
        );
        self.command_category_filter = Some(category_id.clone());
        if is_new_command || edited_command_was_selected {
            self.selected_quick_command = Some((category_id, command_id));
        }
        self.mark_config_preferences_dirty();
        cx.notify();
        true
    }

    pub(crate) fn show_quick_command_dialog(
        &mut self,
        category_id: String,
        command_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        let categories = self
            .config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec();
        let existing = self
            .config
            .quick_command_categories()
            .and_then(|categories| categories.iter().find(|item| item.id == category_id))
            .and_then(|category| {
                command_id.as_deref().and_then(|command_id| {
                    category.commands.iter().find(|item| item.id == command_id)
                })
            })
            .cloned();
        let name_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).default_value(
                existing
                    .as_ref()
                    .map(|item| item.name.as_str())
                    .unwrap_or_default(),
            )
        });
        let remark_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).default_value(
                existing
                    .as_ref()
                    .map(|item| item.remark.as_str())
                    .unwrap_or_default(),
            )
        });
        let command_input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx)
                .multi_line(true)
                .rows(12)
                .default_value(
                    existing
                        .as_ref()
                        .map(|item| item.command.as_str())
                        .unwrap_or_default(),
                )
        });
        let dialog_inputs = QuickCommandDialogInputs {
            name: name_input.clone(),
            remark: remark_input.clone(),
            command: command_input.clone(),
        };
        self.editing_quick_command_category = Some(category_id.clone());
        self.active_dialog = Some(crate::app::DialogKind::QuickCommand);
        let view = cx.entity();
        let source_category_id = category_id.clone();
        let submit_inputs = dialog_inputs.clone();
        let focus_name_input = name_input.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            let submit_inputs = submit_inputs.clone();
            let content_name = name_input.clone();
            let content_remark = remark_input.clone();
            let content_command = command_input.clone();
            let existing = existing.clone();
            let categories = categories.clone();
            let source_category_id = source_category_id.clone();
            dialog
                .title(t!("quick_command_dialog_title").to_string())
                .w(px(760.))
                .h(px(620.))
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            this.editing_quick_command_category = None;
                            cx.notify();
                        });
                    }
                })
                .on_ok({
                    let view = view.clone();
                    let submit_inputs = submit_inputs.clone();
                    let existing = existing.clone();
                    let source_category_id = source_category_id.clone();
                    move |_, _window, cx| {
                        view.update(cx, |this, cx| {
                            this.confirm_quick_command_dialog(
                                source_category_id.clone(),
                                existing.clone(),
                                &submit_inputs,
                                cx,
                            )
                        })
                    }
                })
                .content({
                    let view = view.clone();
                    move |content, window, cx| {
                        let selected_category = view
                            .read(cx)
                            .editing_quick_command_category
                            .clone();
                        let category_label = categories
                            .iter()
                            .find(|category| {
                                Some(category.id.as_str()) == selected_category.as_deref()
                            })
                            .map(|category| category.name.clone())
                            .unwrap_or_else(|| t!("quick_command_category_name").to_string());
                        content.child(
                        v_flex()
                            .size_full()
                            .gap_4()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("quick_command_name")),
                                            )
                                            .child(Input::new(&content_name).w_full()),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("quick_command_category_name")),
                                            )
                                            .child(
                                                Button::new("quick-command-category-picker")
                                                    .secondary()
                                                    .w_full()
                                                    .label(category_label)
                                                    .dropdown_menu_with_anchor(
                                                        Anchor::BottomLeft,
                                                        {
                                                            let view = view.clone();
                                                            let categories = categories.clone();
                                                            move |mut menu, window, cx| {
                                                                let selected = view
                                                                    .read(cx)
                                                                    .editing_quick_command_category
                                                                    .clone();
                                                                for category in &categories {
                                                                    let category_id =
                                                                        category.id.clone();
                                                                    menu = menu.item(
                                                                        PopupMenuItem::new(
                                                                            category.name.clone(),
                                                                        )
                                                                        .checked(
                                                                            selected.as_deref()
                                                                                == Some(
                                                                                    category.id
                                                                                        .as_str(),
                                                                                ),
                                                                        )
                                                                        .on_click(
                                                                            window.listener_for(
                                                                                &view,
                                                                                move |this,
                                                                                      _,
                                                                                      _,
                                                                                      cx| {
                                                                                    this.editing_quick_command_category = Some(category_id.clone());
                                                                                    cx.notify();
                                                                                },
                                                                            ),
                                                                        ),
                                                                    );
                                                                }
                                                                menu
                                                            }
                                                        },
                                                    ),
                                            ),
                                    )
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("quick_command_remark")),
                                    )
                                    .child(Input::new(&content_remark).w_full()),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("quick_command_content")),
                                    )
                                    .child(Input::new(&content_command).size_full()),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!("quick_command_insert_parameter")),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .children((1usize..=5).map(|index| {
                                                let command_input = content_command.clone();
                                                Button::new(("insert-command-parameter", index))
                                                    .small()
                                                    .secondary()
                                                    .label(format!("[p{index}]"))
                                                    .on_click(move |_, window, cx| {
                                                        let current = command_input
                                                            .read(cx)
                                                            .value()
                                                            .to_string();
                                                        command_input.update(cx, |input, cx| {
                                                            input.set_value(
                                                                format!("{current}[p{index}]"),
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    })
                                            }))
                                            .child(
                                                div()
                                                    .text_size(rems(0.75))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t!("quick_command_parameter_hint")),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_none()
                                    .justify_end()
                                    .gap_2()
                                    .pt_2()
                                    .child(
                                        Button::new("quick-command-dialog-cancel")
                                            .secondary()
                                            .label(t!("cancel").to_string())
                                            .on_click(window.listener_for(
                                                &view,
                                                |_this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    window.defer(cx, |window, cx| {
                                                        window.close_dialog(cx);
                                                    });
                                                },
                                            )),
                                    )
                                    .child(
                                        Button::new("quick-command-dialog-save")
                                            .primary()
                                            .label(t!("save").to_string())
                                            .on_click(window.listener_for(&view, {
                                                let source_category_id =
                                                    source_category_id.clone();
                                                let existing = existing.clone();
                                                let inputs = QuickCommandDialogInputs {
                                                    name: content_name.clone(),
                                                    remark: content_remark.clone(),
                                                    command: content_command.clone(),
                                                };
                                                move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    if this.confirm_quick_command_dialog(
                                                        source_category_id.clone(),
                                                        existing.clone(),
                                                        &inputs,
                                                        cx,
                                                    ) {
                                                        window.defer(cx, |window, cx| {
                                                            window.close_dialog(cx);
                                                        });
                                                    }
                                                }
                                            })),
                                    ),
                            ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_name_input, window, cx);
    }

    pub(crate) fn show_ssh_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::NewSsh);

        let view = cx.entity();
        let session_name_input = self.session_name_input.clone();
        let host_input = self.host_input.clone();
        let focus_host_input = host_input.clone();
        let port_input = self.port_input.clone();
        let user_input = self.user_input.clone();
        let password_input = self.password_input.clone();
        let key_path_input = self.key_path_input.clone();
        let key_inline_input = self.key_inline_input.clone();
        let passphrase_input = self.passphrase_input.clone();
        let proxy_host_input = self.proxy_host_input.clone();
        let proxy_port_input = self.proxy_port_input.clone();
        let proxy_user_input = self.proxy_user_input.clone();
        let proxy_password_input = self.proxy_password_input.clone();

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            dialog
                .title(t!("new_ssh_connection"))
                .w(px(520.))
                .overlay_closable(true)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.connect_ssh(window, cx);
                        });
                        false
                    }
                })
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            this.key_import.close();
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    let session_name_input = session_name_input.clone();
                    let host_input = host_input.clone();
                    let port_input = port_input.clone();
                    let user_input = user_input.clone();
                    let password_input = password_input.clone();
                    let key_path_input = key_path_input.clone();
                    let key_inline_input = key_inline_input.clone();
                    let passphrase_input = passphrase_input.clone();
                    let proxy_host_input = proxy_host_input.clone();
                    let proxy_port_input = proxy_port_input.clone();
                    let proxy_user_input = proxy_user_input.clone();
                    let proxy_password_input = proxy_password_input.clone();
                    move |content, window, cx| {
                        let auth_method = view.read(cx).ssh_auth_method;
                        let is_password = auth_method == AuthMethod::Password;
                        let is_key = auth_method == AuthMethod::Key;
                        let is_config = auth_method == AuthMethod::Config;
                        let is_editing = view.read(cx).editing_session_id.is_some();
                        let proxy_type = view.read(cx).ssh_proxy_type.clone();
                        let show_proxy_fields = proxy_type != "none";
                        let key_auth_incomplete = is_key
                            && !view.read(cx).using_custom_key_path
                            && view.read(cx).managed_key_selected.is_none();
                        let connection_groups = view.read(cx).config.connection_groups().to_vec();
                        let selected_group = view.read(cx).session_group_selection.clone();
                        let group_label = selected_group
                            .clone()
                            .unwrap_or_else(|| t!("connection_group_ungrouped").to_string());
                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("ssh-auth-password")
                                                .label(t!("password").to_string())
                                                .when(is_password, |button| button.primary())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_ssh_auth_method(
                                                            AuthMethod::Password,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("ssh-auth-key")
                                                .label(t!("key").to_string())
                                                .when(is_key, |button| button.primary())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_ssh_auth_method(
                                                            AuthMethod::Key,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("ssh-auth-config")
                                                .label(t!("ssh_config").to_string())
                                                .when(is_config, |button| button.primary())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| {
                                                        this.set_ssh_auth_method(
                                                            AuthMethod::Config,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        ),
                                )
                                .when(!is_config, |this| {
                                    this.child(Input::new(&session_name_input).tab_index(0))
                                        .child(Input::new(&host_input).tab_index(1))
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    Input::new(&port_input).w(px(96.)).tab_index(2),
                                                )
                                                .child(
                                                    Input::new(&user_input).flex_1().tab_index(3),
                                        ),
                                        )
                                        .child(
                                            Button::new("ssh-session-group")
                                                .secondary()
                                                .w_full()
                                                .label(format!("{}: {}", t!("connection_group"), group_label))
                                                .dropdown_menu_with_anchor(Anchor::BottomLeft, {
                                                    let view = view.clone();
                                                    move |mut menu, window, cx| {
                                                        let selected = view.read(cx).session_group_selection.clone();
                                                        menu = menu.item(
                                                            PopupMenuItem::new(t!("connection_group_ungrouped").to_string())
                                                                .checked(selected.is_none())
                                                                .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                    this.session_group_selection = None;
                                                                    cx.notify();
                                                                })),
                                                        );
                                                        for group in connection_groups.clone() {
                                                            let group_name = group.clone();
                                                            let checked = selected.as_deref() == Some(group.as_str());
                                                            menu = menu.item(
                                                                PopupMenuItem::new(group)
                                                                    .checked(checked)
                                                                    .on_click(window.listener_for(&view, move |this, _, _, cx| {
                                                                        this.session_group_selection = Some(group_name.clone());
                                                                        cx.notify();
                                                                    })),
                                                            );
                                                        }
                                                        menu
                                                    }
                                                }),
                                        )
                                })
                                .when(is_password, |this| {
                                    this.child(
                                        Input::new(&password_input).mask_toggle().tab_index(4),
                                    )
                                })
                                .when(is_key, |this| {
                                    let managed_keys = view.read(cx).managed_keys.clone();
                                    let managed_key_selected =
                                        view.read(cx).managed_key_selected.clone();
                                    let using_custom_key_path = view.read(cx).using_custom_key_path;
                                    let theme = cx.theme();
                                    let using_managed_key = !using_custom_key_path;

                                    let key_label = if let Some(mk_id) = &managed_key_selected {
                                        managed_keys
                                            .iter()
                                            .find(|k| &k.id == mk_id)
                                            .map(|mk| format!("{} ({})", mk.name, mk.key_type))
                                            .unwrap_or_else(|| t!("select_managed_key").to_string())
                                    } else {
                                        t!("select_managed_key").to_string()
                                    };

                                    let this =
                                        this.child(
                                            h_flex()
                                                .w_full()
                                                .gap_1()
                                                .p(px(3.))
                                                .rounded_md()
                                                .border_1()
                                                .border_color(theme.border)
                                                .bg(theme.muted)
                                                .child(
                                                    Button::new("use-managed-key")
                                                        .small()
                                                        .flex_1()
                                                        .when(using_managed_key, |button| {
                                                            button.primary()
                                                        })
                                                        .label(t!("select_managed_key").to_string())
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, _, cx| {
                                                                this.use_managed_key(cx)
                                                            },
                                                        )),
                                                )
                                                .child(
                                                    Button::new("use-custom-path")
                                                        .small()
                                                        .flex_1()
                                                        .when(using_custom_key_path, |button| {
                                                            button.primary()
                                                        })
                                                        .label(t!("use_custom_path").to_string())
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, _, cx| {
                                                                this.use_custom_key_path(cx)
                                                            },
                                                        )),
                                                ),
                                        );

                                    if using_managed_key {
                                        let this = this.child(
                                            Button::new("managed-key-select")
                                                .small()
                                                .w_full()
                                                .label(key_label)
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.open_managed_key_selector(window, cx);
                                                    },
                                                )),
                                        );
                                        let info = managed_key_selected
                                            .as_ref()
                                            .and_then(|id| {
                                                managed_keys.iter().find(|k| &k.id == id)
                                            })
                                            .map(|mk| {
                                                format!(
                                                    "{}: {}  |  {}: {}",
                                                    t!("key_type"),
                                                    mk.key_type,
                                                    t!("key_fingerprint"),
                                                    mk.fingerprint
                                                )
                                            });

                                        if let Some(info) = info {
                                            this.child(
                                                div()
                                                    .px(px(8.))
                                                    .py(px(6.))
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(theme.border)
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(info),
                                            )
                                        } else {
                                            this.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(
                                                        t!("select_managed_key_hint").to_string(),
                                                    ),
                                            )
                                        }
                                    } else {
                                        this.child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .cursor_pointer()
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            window.listener_for(
                                                                &view,
                                                                |this, _, window, cx| {
                                                                    this.pick_ssh_key_path(
                                                                        window, cx,
                                                                    );
                                                                },
                                                            ),
                                                        )
                                                        .child(
                                                            Input::new(&key_path_input)
                                                                .tab_index(4),
                                                        ),
                                                )
                                                .child(
                                                    Button::new("clear-key-path")
                                                        .ghost()
                                                        .icon(IconName::Close)
                                                        .on_click(window.listener_for(
                                                            &view,
                                                            |this, _, window, cx| {
                                                                Self::set_input_value(
                                                                    &this.key_path_input,
                                                                    "",
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )),
                                                ),
                                        )
                                        .child(
                                            Input::new(&key_inline_input).h(px(128.)).tab_index(5),
                                        )
                                        .child(
                                            Input::new(&passphrase_input)
                                                .mask_toggle()
                                                .tab_index(6),
                                        )
                                    }
                                })
                                .when(is_config, |this| {
                                    let entries = view.read(cx).ssh_config_entries.clone();
                                    let selected = view.read(cx).ssh_config_selected;
                                    let theme = cx.theme();
                                    if entries.is_empty() {
                                        this.child(
                                            div()
                                                .text_sm()
                                                .text_color(theme.muted_foreground)
                                                .child(t!("ssh_config_empty").to_string()),
                                        )
                                    } else {
                                        this.child(
                                            div()
                                                .h(px(192.))
                                                .id("ssh-config-list")
                                                .track_scroll(
                                                    &view.read(cx).connection_scroll_handle,
                                                )
                                                .overflow_y_scroll()
                                                .border_1()
                                                .border_color(theme.border)
                                                .rounded_md()
                                                .children(entries.iter().enumerate().map(
                                                    |(i, entry)| {
                                                        let is_selected = selected == Some(i);
                                                        let label = if entry.user.is_empty() {
                                                            format!(
                                                                "{}:{}",
                                                                entry.hostname, entry.port
                                                            )
                                                        } else {
                                                            format!(
                                                                "{}@{}:{}",
                                                                entry.user,
                                                                entry.hostname,
                                                                entry.port
                                                            )
                                                        };
                                                        let alias_label =
                                                            if entry.host_alias == entry.hostname {
                                                                String::new()
                                                            } else {
                                                                format!(" ({})", entry.host_alias)
                                                            };
                                                        let view_clone = view.clone();
                                                        div()
                                                            .id(("ssh-config-entry", i))
                                                            .px_2()
                                                            .py_1()
                                                            .when(is_selected, |el| {
                                                                el.bg(theme.selection)
                                                            })
                                                            .cursor_pointer()
                                                            .hover(|el| el.bg(theme.selection))
                                                            .text_sm()
                                                            .child(format!("{label}{alias_label}"))
                                                            .on_click(window.listener_for(
                                                                &view_clone,
                                                                move |this, _, window, cx| {
                                                                    this.select_ssh_config_entry(
                                                                        i, window, cx,
                                                                    );
                                                                },
                                                            ))
                                                    },
                                                )),
                                        )
                                    }
                                })
                                .when(!is_config, |this| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .child(t!("proxy").to_string()),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Button::new("proxy-none")
                                                    .label(t!("proxy_none").to_string())
                                                    .when(proxy_type == "none", |button| {
                                                        button.primary()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.set_ssh_proxy_type(
                                                                "none".to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("proxy-socks5")
                                                    .label("SOCKS5")
                                                    .when(proxy_type == "socks5", |button| {
                                                        button.primary()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.set_ssh_proxy_type(
                                                                "socks5".to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("proxy-http")
                                                    .label("HTTP")
                                                    .when(proxy_type == "http", |button| {
                                                        button.primary()
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.set_ssh_proxy_type(
                                                                "http".to_string(),
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            ),
                                    )
                                    .when(
                                        show_proxy_fields,
                                        |this| {
                                            this.child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(Input::new(&proxy_host_input).flex_1())
                                                    .child(
                                                        Input::new(&proxy_port_input).w(px(96.)),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(Input::new(&proxy_user_input).flex_1())
                                                    .child(
                                                        Input::new(&proxy_password_input).flex_1(),
                                                    ),
                                            )
                                        },
                                    )
                                })
                                .child(
                                    h_flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("connect-ssh-cancel")
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.active_dialog = None;
                                                        window.close_dialog(cx);
                                                        cx.notify();
                                                    },
                                                )),
                                        )
                                        .when(!is_config, |this| {
                                            this.child(
                                                Button::new("connect-ssh-confirm")
                                                    .primary()
                                                    .disabled(key_auth_incomplete)
                                                    .label(if is_editing {
                                                        t!("save")
                                                    } else {
                                                        t!("connect")
                                                    })
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.connect_ssh(window, cx)
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_host_input, window, cx);
    }
    pub(crate) fn show_managed_key_selector_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::ManagedKeySelector);

        let view = cx.entity();
        let rename_input = self.key_import_remark_input.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            dialog
                .title(t!("select_private_key").to_string())
                .w(px(760.))
                .close_button(false)
                .overlay_closable(false)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            if this.managed_key_dialog_selection.is_some() {
                                this.confirm_managed_key_selection(window, cx);
                            }
                        });
                        false
                    }
                })
                .content({
                    let view = view.clone();
                    let rename_input = rename_input.clone();
                    move |content, window, cx| {
                        let keys = view.read(cx).managed_keys.clone();
                        let selected = view.read(cx).managed_key_dialog_selection.clone();
                        let is_renaming = view.read(cx).editing_managed_key_id.is_some();
                        let has_selection = selected.is_some();

                        let mut rows = v_flex()
                            .h(px(220.))
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_md()
                            .overflow_hidden()
                            .child(
                                h_flex()
                                    .px_2()
                                    .py_1()
                                    .bg(cx.theme().muted)
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .w(px(220.))
                                            .flex_shrink_0()
                                            .text_sm()
                                            .child(t!("name").to_string()),
                                    )
                                    .child(
                                        div()
                                            .w(px(110.))
                                            .flex_shrink_0()
                                            .text_sm()
                                            .child(t!("key_type").to_string()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_sm()
                                            .child(t!("key_fingerprint").to_string()),
                                    ),
                            );

                        if keys.is_empty() {
                            rows = rows.child(
                                div()
                                    .flex_1()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("no_managed_keys").to_string()),
                            );
                        } else {
                            for (index, key) in keys.into_iter().enumerate() {
                                let key_id = key.id.clone();
                                let is_selected = selected.as_deref() == Some(key.id.as_str());
                                let fingerprint = if key.fingerprint.len() > 24 {
                                    format!("{}…", &key.fingerprint[..24])
                                } else {
                                    key.fingerprint.clone()
                                };
                                rows = rows.child(
                                    h_flex()
                                        .id(("managed-key-choice", index))
                                        .px_2()
                                        .py_2()
                                        .cursor_pointer()
                                        .border_b_1()
                                        .border_color(cx.theme().border)
                                        .when(is_selected, |row| row.bg(cx.theme().selection))
                                        .hover(|row| row.bg(cx.theme().selection))
                                        .child(
                                            div()
                                                .w(px(220.))
                                                .flex_shrink_0()
                                                .min_w(px(0.))
                                                .overflow_hidden()
                                                .text_sm()
                                                .child(key.name),
                                        )
                                        .child(
                                            div()
                                                .w(px(110.))
                                                .flex_shrink_0()
                                                .text_sm()
                                                .child(key.key_type),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .overflow_hidden()
                                                .text_sm()
                                                .child(fingerprint),
                                        )
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this, _, _, cx| {
                                                this.select_managed_key_candidate(
                                                    key_id.clone(),
                                                    cx,
                                                );
                                            },
                                        )),
                                );
                            }
                        }

                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    h_flex().gap_3().child(rows.flex_1()).child(
                                        v_flex()
                                            .w(px(104.))
                                            .gap_2()
                                            .child(
                                                Button::new("selector-import-key")
                                                    .w_full()
                                                    .label(t!("import_key").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.open_key_import(window, cx);
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("selector-edit-key")
                                                    .w_full()
                                                    .disabled(!has_selection)
                                                    .label(t!("edit").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.begin_managed_key_rename(
                                                                window, cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("selector-delete-key")
                                                    .w_full()
                                                    .disabled(!has_selection)
                                                    .label(t!("delete").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.delete_selected_managed_key(
                                                                window, cx,
                                                            );
                                                        },
                                                    )),
                                            ),
                                    ),
                                )
                                .when(is_renaming, |this| {
                                    this.child(
                                        h_flex()
                                            .gap_2()
                                            .child(Input::new(&rename_input).flex_1())
                                            .child(
                                                Button::new("save-key-rename")
                                                    .primary()
                                                    .label(t!("save").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.save_managed_key_rename(cx);
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("cancel-key-rename")
                                                    .label(t!("cancel").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.cancel_managed_key_rename(cx);
                                                        },
                                                    )),
                                            ),
                                    )
                                })
                                .child(
                                    h_flex()
                                        .justify_center()
                                        .gap_2()
                                        .child(
                                            Button::new("confirm-key-selection")
                                                .primary()
                                                .disabled(!has_selection)
                                                .label(t!("confirm").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.confirm_managed_key_selection(
                                                            window, cx,
                                                        );
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("cancel-key-selection")
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.return_to_ssh_dialog(window, cx);
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    }
                })
        });
    }

    pub(crate) fn show_managed_key_import_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::ManagedKeyImport);

        let view = cx.entity();
        let remark_input = self.key_import_remark_input.clone();
        let passphrase_input = self.key_import_passphrase_input.clone();
        let focus_remark_input = remark_input.clone();
        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            dialog
                .title(t!("key_import_dialog_title").to_string())
                .w(px(440.))
                .close_button(false)
                .overlay_closable(false)
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.confirm_managed_key_import(window, cx);
                        });
                        false
                    }
                })
                .content({
                    let view = view.clone();
                    let remark_input = remark_input.clone();
                    let passphrase_input = passphrase_input.clone();
                    move |content, window, cx| {
                        let path = view.read(cx).key_import.path.clone();
                        let validation = view.read(cx).key_import.validation.clone();
                        let can_confirm = validation.can_confirm();
                        let (status, status_color) = match &validation {
                            KeyImportValidation::WaitingForFile => (
                                t!("key_import_select_file_hint").to_string(),
                                cx.theme().muted_foreground,
                            ),
                            KeyImportValidation::Validating => (
                                t!("key_import_validating").to_string(),
                                cx.theme().muted_foreground,
                            ),
                            KeyImportValidation::Invalid(error) => (
                                format!("{}: {error}", t!("key_import_failed")),
                                cx.theme().danger,
                            ),
                            KeyImportValidation::Duplicate => (
                                t!("key_duplicate_fingerprint").to_string(),
                                cx.theme().danger,
                            ),
                            KeyImportValidation::Valid {
                                key_type,
                                fingerprint,
                            } => (
                                format!(
                                    "{} · {}: {}",
                                    key_type,
                                    t!("key_fingerprint"),
                                    fingerprint
                                ),
                                cx.theme().success,
                            ),
                        };

                        content.child(
                            v_flex()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .text_sm()
                                                .child(t!("name").to_string()),
                                        )
                                        .child(Input::new(&remark_input).flex_1()),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .text_sm()
                                                .child(t!("private_key").to_string()),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .px_3()
                                                .py_2()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .text_sm()
                                                .overflow_hidden()
                                                .text_color(if path.is_empty() {
                                                    cx.theme().muted_foreground
                                                } else {
                                                    cx.theme().foreground
                                                })
                                                .child(if path.is_empty() {
                                                    t!("key_import_choose_file").to_string()
                                                } else {
                                                    path
                                                }),
                                        )
                                        .child(
                                            Button::new("browse-key-import")
                                                .label(t!("browse").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.pick_managed_key_import_file(
                                                            window, cx,
                                                        );
                                                    },
                                                )),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .w(px(80.))
                                                .text_sm()
                                                .child(t!("key_passphrase").to_string()),
                                        )
                                        .child(
                                            Input::new(&passphrase_input).flex_1().mask_toggle(),
                                        ),
                                )
                                .child(
                                    div()
                                        .pl(px(80.))
                                        .text_xs()
                                        .text_color(status_color)
                                        .child(status),
                                )
                                .child(
                                    h_flex()
                                        .justify_center()
                                        .gap_2()
                                        .child(
                                            Button::new("confirm-key-import")
                                                .primary()
                                                .disabled(!can_confirm)
                                                .label(t!("confirm").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.confirm_managed_key_import(window, cx);
                                                    },
                                                )),
                                        )
                                        .child(
                                            Button::new("cancel-key-import")
                                                .label(t!("cancel").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.close_key_import(window, cx);
                                                    },
                                                )),
                                        ),
                                ),
                        )
                    }
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_remark_input, window, cx);
    }

    pub(crate) fn show_selector_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::SessionSelector);

        let view = cx.entity();
        let selector_focus_handle = self.selector_focus_handle.clone();
        let deferred_selector_focus_handle = selector_focus_handle.clone();
        let sessions = self.config.sessions().to_vec();
        let active_session_id = self.active_session_id().map(ToOwned::to_owned);
        self.selector_selection = self.default_selector_index();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("open_session").to_string())
                .w(px(520.))
                .on_close({
                    let view = view.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            cx.notify();
                        });
                    }
                })
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.activate_selector_selection(window, cx);
                        });
                        false
                    }
                })
                .content({
                    let view = view.clone();
                    let sessions = sessions.clone();
                    let _active_session_id = active_session_id.clone();
                    let selector_focus_handle = selector_focus_handle.clone();
                    move |content, window, _cx| {
                        let selected_index = view.read(_cx).selector_selection;
                        let scroll_handle = view.read(_cx).selector_scroll_handle.clone();
                        content.child(
                            v_flex()
                                .track_focus(&selector_focus_handle)
                                .on_key_down(window.listener_for(
                                    &view,
                                    |this, event, window, cx| {
                                        this.on_selector_key_down(event, window, cx)
                                    },
                                ))
                                .gap_2()
                                .child(
                                    div()
                                        .w_full()
                                        .p_2()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if selected_index == 0 {
                                            _cx.theme().primary
                                        } else {
                                            _cx.theme().border
                                        })
                                        .bg(if selected_index == 0 {
                                            _cx.theme().tab_active
                                        } else {
                                            _cx.theme().muted
                                        })
                                        .cursor_pointer()
                                        .hover(|this| this.bg(_cx.theme().secondary))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            window.listener_for(&view, |this, _, window, cx| {
                                                this.active_dialog = None;
                                                this.open_local(cx);
                                                window.close_dialog(cx);
                                                cx.notify();
                                            }),
                                        )
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(rems(1.0))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child(t!("local_terminal")),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(rems(0.917))
                                                        .text_color(_cx.theme().muted_foreground)
                                                        .child(t!("open_local_shell_tab")),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .p_2()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if selected_index == 1 {
                                            _cx.theme().primary
                                        } else {
                                            _cx.theme().border
                                        })
                                        .bg(if selected_index == 1 {
                                            _cx.theme().tab_active
                                        } else {
                                            _cx.theme().muted
                                        })
                                        .cursor_pointer()
                                        .hover(|this| this.bg(_cx.theme().secondary))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            window.listener_for(&view, |this, _, window, cx| {
                                                this.active_dialog = None;
                                                window.close_dialog(cx);
                                                this.open_new_ssh_dialog(window, cx);
                                                cx.notify();
                                            }),
                                        )
                                        .child(
                                            v_flex()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(rems(1.0))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child(t!("new_ssh_connection")),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(rems(0.917))
                                                        .text_color(_cx.theme().muted_foreground)
                                                        .child(t!("create_or_edit_ssh_session")),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .max_h(px(320.))
                                        .size_full()
                                        .child(
                                            v_flex()
                                                .size_full()
                                                .id("selector-scroll-view")
                                                .track_scroll(&scroll_handle)
                                                .overflow_y_scroll()
                                                .gap_2()
                                                .children(
                                                    sessions.clone().into_iter().enumerate().map(
                                                        |(ix, session)| {
                                                            let connect_id = session.id.clone();
                                                            let is_selected =
                                                                selected_index == ix + 2;
                                                            let name = session.name.clone();
                                                            let detail = format!(
                                                                "{}@{}:{}",
                                                                session.user,
                                                                session.host,
                                                                session.port
                                                            );
                                                            div()
                                                    .id(("selector-open", ix))
                                                    .w_full()
                                                    .p_2()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(if is_selected {
                                                        _cx.theme().primary
                                                    } else {
                                                        _cx.theme().border
                                                    })
                                                    .bg(if is_selected {
                                                        _cx.theme().tab_active
                                                    } else {
                                                        _cx.theme().muted
                                                    })
                                                    .cursor_pointer()
                                                    .hover(|this| this.bg(_cx.theme().secondary))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        window.listener_for(
                                                            &view,
                                                            move |this, _, window, cx| {
                                                                this.active_dialog = None;
                                                                this.connect_saved_session(
                                                                    connect_id.clone(),
                                                                    cx,
                                                                );
                                                                window.close_dialog(cx);
                                                                cx.notify();
                                                            },
                                                        ),
                                                    )
                                                    .child(
                                                        v_flex()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_size(rems(1.0))
                                                                    .font_weight(
                                                                        FontWeight::SEMIBOLD,
                                                                    )
                                                                    .child(name),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(rems(0.917))
                                                                    .text_color(
                                                                        _cx.theme()
                                                                            .muted_foreground,
                                                                    )
                                                                    .child(detail),
                                                            ),
                                                    )
                                                        },
                                                    ),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .bottom_0()
                                                .left_0()
                                                .right_0()
                                                .child(
                                                gpui_component::scroll::Scrollbar::new(
                                                    &scroll_handle,
                                                )
                                                .id("selector-scrollbar")
                                                .axis(
                                                    gpui_component::scroll::ScrollbarAxis::Vertical,
                                                )
                                                .scrollbar_show(
                                                    gpui_component::scroll::ScrollbarShow::Always,
                                                ),
                                            ),
                                        ),
                                ),
                        )
                    }
                })
        });
        window.defer(cx, move |window, cx| {
            window.focus(&deferred_selector_focus_handle, cx);
        });
    }
    fn quick_connection_row(
        session: crate::session::config::Session,
        index: usize,
        depth: usize,
        view: &gpui::Entity<Self>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> gpui::AnyElement {
        let connect_id = session.id.clone();
        h_flex()
            .id(("quick-connection-row", index))
            .min_h(px(34.))
            .pl(px(14. + depth as f32 * 16.))
            .pr_3()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.45))
            .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(view, move |this, _, window, cx| {
                    this.active_dialog = None;
                    this.connect_saved_session(connect_id.clone(), cx);
                    window.close_dialog(cx);
                    cx.notify();
                }),
            )
            .child(Icon::new(IconName::SquareTerminal).with_size(Size::Small))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(rems(0.74))
                    .font_weight(FontWeight::MEDIUM)
                    .child(session.name),
            )
            .child(
                div()
                    .w(px(190.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(rems(0.7))
                    .child(session.host),
            )
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .text_center()
                    .text_size(rems(0.7))
                    .child(session.port.to_string()),
            )
            .child(
                div()
                    .w(px(100.))
                    .flex_none()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(rems(0.7))
                    .child(session.user),
            )
            .into_any_element()
    }

    pub(crate) fn show_quick_connection_manager_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::QuickConnectionManager);
        self.quick_connection_search_input
            .update(cx, |input, cx| input.set_value("", window, cx));

        let view = cx.entity();
        let search_input = self.quick_connection_search_input.clone();
        let deferred_search_input = search_input.clone();
        let scroll_handle = self.quick_connection_scroll_handle.clone();

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            dialog
                .title(t!("quick_connection_title").to_string())
                .w(px(760.))
                .h(px(540.))
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
                    let search_input = search_input.clone();
                    let scroll_handle = scroll_handle.clone();
                    move |content, window, cx| {
                        let query = search_input.read(cx).value().trim().to_lowercase();
                        let (mut sessions, mut groups) = {
                            let state = view.read(cx);
                            (
                                state.config.sessions().to_vec(),
                                state.config.connection_groups().to_vec(),
                            )
                        };
                        for session in &sessions {
                            if let Some(group) = &session.group
                                && !groups.contains(group)
                            {
                                groups.push(group.clone());
                            }
                        }
                        groups = Self::connection_group_tree_order(groups);

                        let session_matches = |session: &crate::session::config::Session| {
                            query.is_empty()
                                || session.name.to_lowercase().contains(&query)
                                || session.host.to_lowercase().contains(&query)
                                || session.user.to_lowercase().contains(&query)
                                || session
                                    .group
                                    .as_deref()
                                    .is_some_and(|group| group.to_lowercase().contains(&query))
                        };
                        sessions.retain(session_matches);

                        if !query.is_empty() {
                            groups.retain(|group| {
                                group.to_lowercase().contains(&query)
                                    || sessions.iter().any(|session| {
                                        session.group.as_deref().is_some_and(|session_group| {
                                            session_group == group
                                                || session_group.starts_with(&format!("{group}/"))
                                                || group.starts_with(&format!("{session_group}/"))
                                        })
                                    })
                            });
                        }

                        let mut rows = Vec::new();
                        let mut row_index = 0usize;
                        for group in &groups {
                            let depth = group.matches('/').count();
                            let group_name = group.rsplit('/').next().unwrap_or(group).to_string();
                            rows.push(
                                h_flex()
                                    .min_h(px(32.))
                                    .pl(px(10. + depth as f32 * 16.))
                                    .pr_3()
                                    .items_center()
                                    .gap_2()
                                    .bg(cx.theme().muted.opacity(0.38))
                                    .border_t_1()
                                    .border_color(cx.theme().border.opacity(0.45))
                                    .child(Icon::new(IconName::Folder).with_size(Size::Small))
                                    .child(
                                        div()
                                            .text_size(rems(0.72))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(group_name),
                                    )
                                    .into_any_element(),
                            );
                            for session in sessions
                                .iter()
                                .filter(|session| session.group.as_deref() == Some(group.as_str()))
                            {
                                rows.push(Self::quick_connection_row(
                                    session.clone(),
                                    row_index,
                                    depth + 1,
                                    &view,
                                    window,
                                    cx,
                                ));
                                row_index += 1;
                            }
                        }

                        let ungrouped: Vec<_> = sessions
                            .iter()
                            .filter(|session| session.group.as_deref().is_none_or(str::is_empty))
                            .cloned()
                            .collect();
                        if !ungrouped.is_empty() {
                            rows.push(
                                h_flex()
                                    .min_h(px(32.))
                                    .px_3()
                                    .items_center()
                                    .gap_2()
                                    .bg(cx.theme().muted.opacity(0.38))
                                    .border_t_1()
                                    .border_color(cx.theme().border.opacity(0.45))
                                    .child(Icon::new(IconName::Folder).with_size(Size::Small))
                                    .child(
                                        div()
                                            .text_size(rems(0.72))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!("quick_connection_ungrouped").to_string()),
                                    )
                                    .into_any_element(),
                            );
                            for session in ungrouped {
                                rows.push(Self::quick_connection_row(
                                    session,
                                    row_index,
                                    1,
                                    &view,
                                    window,
                                    cx,
                                ));
                                row_index += 1;
                            }
                        }

                        let has_rows = !rows.is_empty();
                        content.child(
                            v_flex()
                                .size_full()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .child(Input::new(&search_input).small()),
                                        )
                                        .child(
                                            Button::new("quick-connection-new")
                                                .primary()
                                                .small()
                                                .icon(IconName::Plus)
                                                .label(t!("overview_new_connection").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, window, cx| {
                                                        this.active_dialog = None;
                                                        window.close_dialog(cx);
                                                        this.open_new_ssh_dialog(window, cx);
                                                    },
                                                )),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .overflow_hidden()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .child(
                                            h_flex()
                                                .flex_none()
                                                .h(px(32.))
                                                .px_3()
                                                .items_center()
                                                .bg(cx.theme().tab_bar)
                                                .text_size(rems(0.68))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(cx.theme().muted_foreground)
                                                .child(div().flex_1().child(t!("name").to_string()))
                                                .child(
                                                    div()
                                                        .w(px(190.))
                                                        .flex_none()
                                                        .child(t!("host").to_string()),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(64.))
                                                        .flex_none()
                                                        .text_center()
                                                        .child(t!("port").to_string()),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(100.))
                                                        .flex_none()
                                                        .child(t!("user").to_string()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .relative()
                                                .flex_1()
                                                .min_h(px(0.))
                                                .child(
                                                    v_flex()
                                                        .id("quick-connection-scroll-view")
                                                        .size_full()
                                                        .track_scroll(&scroll_handle)
                                                        .overflow_y_scroll()
                                                        .children(rows)
                                                        .when(!has_rows, |this| {
                                                            this.items_center()
                                                                .justify_center()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(
                                                                    t!("quick_connection_empty")
                                                                        .to_string(),
                                                                )
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .absolute()
                                                        .top_0()
                                                        .bottom_0()
                                                        .left_0()
                                                        .right_0()
                                                        .child(
                                                            Scrollbar::new(&scroll_handle)
                                                                .id("quick-connection-scrollbar")
                                                                .axis(
                                                                    gpui_component::scroll::ScrollbarAxis::Vertical,
                                                                )
                                                                .scrollbar_show(
                                                                    ScrollbarShow::Scrolling,
                                                                ),
                                                        ),
                                                ),
                                        ),
                                ),
                        )
                    }
                })
        });

        crate::app::input_focus::defer_focus_input_at_end(deferred_search_input, window, cx);
    }

    pub(crate) fn show_transfers_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::Transfers);

        let view = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .w(px(600.))
                .close_button(false)
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
                    move |content, window, cx| {
                        let can_clear = view.read(cx).transfers.iter().any(|t| {
                            !matches!(
                                t.state,
                                crate::terminal::TransferState::Running
                                    | crate::terminal::TransferState::Paused
                            )
                        });

                        let clear_btn = if can_clear {
                            Some(
                                Button::new("clear_transfers_btn")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .label(t!("clear_transfers").to_string())
                                    .on_click(window.listener_for(&view, |this, _, _, cx| {
                                        this.transfers.retain(|t| {
                                            matches!(
                                                t.state,
                                                crate::terminal::TransferState::Running
                                                    | crate::terminal::TransferState::Paused
                                            )
                                        });
                                        this.config.set_transfers(this.transfers.clone());
                                        cx.notify();
                                    })),
                            )
                        } else {
                            None
                        };

                        let header = h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                h_flex()
                                    .items_baseline()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!("transfers").to_string()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .ml_2()
                                            .child(t!("transfers_limit").to_string()),
                                    ),
                            )
                            .child(
                                h_flex().gap_2().children(clear_btn).child(
                                    Button::new("close_dialog")
                                        .small()
                                        .ghost()
                                        .icon(IconName::Close)
                                        .on_click(window.listener_for(
                                            &view,
                                            |this, _, window, cx| {
                                                this.active_dialog = None;
                                                window.close_dialog(cx);
                                                cx.notify();
                                            },
                                        )),
                                ),
                            );

                        let mut transfers = view.read(cx).transfers.clone();
                        transfers.sort_by_key(|t| match t.state {
                            crate::terminal::TransferState::Running
                            | crate::terminal::TransferState::Paused => 0,
                            _ => 1,
                        });

                        if transfers.is_empty() {
                            return content.child(
                                v_flex().gap_2().child(header).child(
                                    div()
                                        .p_4()
                                        .text_center()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("no_transfers_yet").to_string()),
                                ),
                            );
                        }
                        let list = v_flex().gap_2().children(transfers.into_iter().map(|t| {
                            let (icon, _color) = match t.info.kind {
                                crate::terminal::TransferType::Upload => {
                                    (IconName::ArrowUp, cx.theme().primary)
                                }
                                crate::terminal::TransferType::Download => {
                                    (IconName::ArrowDown, cx.theme().success)
                                }
                            };

                            let (status_text, actions) = match t.state {
                                crate::terminal::TransferState::Running => {
                                    let percent = t
                                        .total
                                        .map(|tot| {
                                            (t.transferred as f64 / tot as f64 * 100.0)
                                                .clamp(0.0, 100.0)
                                        })
                                        .unwrap_or(0.0);
                                    let txt = if let Some(tot) = t.total {
                                        format!(
                                            "{:.1}% ({}/{})",
                                            percent,
                                            format_bytes(t.transferred),
                                            format_bytes(tot)
                                        )
                                    } else {
                                        match t.info.kind {
                                            crate::terminal::TransferType::Upload => {
                                                format!("{}...", t!("uploading"))
                                            }
                                            crate::terminal::TransferType::Download => {
                                                format!("{}...", t!("downloading"))
                                            }
                                        }
                                    };
                                    let btn_pause = Button::new(SharedString::from(format!(
                                        "pause-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Pause)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, _| {
                                            if let Some(handle) = this.active_sftp_handle() {
                                                handle.pause_transfer(id.clone());
                                            }
                                        }
                                    }));
                                    let btn_cancel = Button::new(SharedString::from(format!(
                                        "cancel-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, _| {
                                            if let Some(handle) = this.active_sftp_handle() {
                                                handle.cancel_transfer(id.clone());
                                            }
                                        }
                                    }));
                                    (txt, h_flex().gap_1().child(btn_pause).child(btn_cancel))
                                }
                                crate::terminal::TransferState::Paused => {
                                    let txt = t!("paused").to_string();
                                    let btn_resume = Button::new(SharedString::from(format!(
                                        "resume-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Play)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, _| {
                                            if let Some(handle) = this.active_sftp_handle() {
                                                handle.resume_transfer(id.clone());
                                            }
                                        }
                                    }));
                                    let btn_cancel = Button::new(SharedString::from(format!(
                                        "cancel-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, _| {
                                            if let Some(handle) = this.active_sftp_handle() {
                                                handle.cancel_transfer(id.clone());
                                            }
                                        }
                                    }));
                                    (txt, h_flex().gap_1().child(btn_resume).child(btn_cancel))
                                }
                                crate::terminal::TransferState::Interrupted(ref reason) => {
                                    let txt = format!("{}: {}", t!("interrupted"), reason);
                                    let btn_remove = Button::new(SharedString::from(format!(
                                        "remove-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, cx| {
                                            this.remove_transfer(&id, cx);
                                        }
                                    }));
                                    (txt, h_flex().gap_1().child(btn_remove))
                                }
                                crate::terminal::TransferState::Completed => {
                                    let txt = t!("completed").to_string();
                                    let mut actions = h_flex().gap_1();
                                    if matches!(
                                        t.info.kind,
                                        crate::terminal::TransferType::Download
                                    ) {
                                        let btn_folder = Button::new(SharedString::from(format!(
                                            "folder-{}",
                                            t.info.id
                                        )))
                                        .ghost()
                                        .small()
                                        .icon(IconName::Folder)
                                        .on_click({
                                            let target = t.info.target.clone();
                                            move |_, _, _| {
                                                let _ = std::process::Command::new("open")
                                                    .arg(&target)
                                                    .spawn();
                                            }
                                        });
                                        actions = actions.child(btn_folder);
                                    }
                                    let btn_remove = Button::new(SharedString::from(format!(
                                        "remove-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, cx| {
                                            this.remove_transfer(&id, cx);
                                        }
                                    }));
                                    actions = actions.child(btn_remove);
                                    (txt, actions)
                                }
                                crate::terminal::TransferState::Failed(ref err) => {
                                    let txt = format!("{}: {}", t!("failed"), err);
                                    let btn_remove = Button::new(SharedString::from(format!(
                                        "remove-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, cx| {
                                            this.remove_transfer(&id, cx);
                                        }
                                    }));
                                    (txt, h_flex().gap_1().child(btn_remove))
                                }
                                crate::terminal::TransferState::Zombie(ref reason) => {
                                    let txt = format!("{}: {}", t!("zombie"), reason);
                                    let btn_remove = Button::new(SharedString::from(format!(
                                        "remove-{}",
                                        t.info.id
                                    )))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Close)
                                    .on_click(window.listener_for(&view, {
                                        let id = t.info.id.clone();
                                        move |this, _, _, cx| {
                                            this.remove_transfer(&id, cx);
                                        }
                                    }));
                                    (txt, h_flex().gap_1().child(btn_remove))
                                }
                            };

                            let percent = match t.state {
                                crate::terminal::TransferState::Completed => 100.0,
                                _ => t
                                    .total
                                    .map(|tot| t.transferred as f64 / tot as f64 * 100.0)
                                    .unwrap_or(0.0),
                            };

                            v_flex()
                                .gap_1()
                                .p_2()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().muted)
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "icon-{}",
                                                t.info.id
                                            )))
                                            .icon(icon)
                                            .ghost()
                                            .small()
                                            .disabled(true),
                                        )
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .overflow_hidden()
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(cx.theme().foreground)
                                                        .overflow_hidden()
                                                        .child(t.info.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .overflow_hidden()
                                                        .child(format!(
                                                            "{}: {}",
                                                            t!("session"),
                                                            t.tab_title
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(status_text.clone()),
                                                ),
                                        )
                                        .child(actions),
                                )
                                .when(
                                    matches!(
                                        t.state,
                                        crate::terminal::TransferState::Running
                                            | crate::terminal::TransferState::Paused
                                    ),
                                    |this| {
                                        this.child(
                                            Progress::new(format!("progress-{}", t.info.id))
                                                .with_size(px(4.))
                                                .value(percent as f32)
                                                .color(cx.theme().primary)
                                                .w_full(),
                                        )
                                    },
                                )
                        }));

                        let scroll_handle = window
                            .use_keyed_state("transfers-scroll", cx, |_, _| {
                                gpui::ScrollHandle::default()
                            })
                            .read(cx)
                            .clone();

                        content.child(
                            v_flex().gap_2().child(header).child(
                                div()
                                    .w_full()
                                    .relative()
                                    .child(
                                        div()
                                            .w_full()
                                            .max_h(px(400.))
                                            .flex_col()
                                            .id("transfers-scroll-view")
                                            .track_scroll(&scroll_handle)
                                            .overflow_y_scroll()
                                            .pr(px(14.))
                                            .child(list),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .right_0()
                                            .bottom_0()
                                            .w(px(16.))
                                            .child(
                                                Scrollbar::vertical(&scroll_handle)
                                                    .scrollbar_show(ScrollbarShow::Always),
                                            ),
                                    ),
                            ),
                        )
                    }
                })
        });
    }
    pub(crate) fn request_managed_key_deletion(
        &mut self,
        key_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .config
            .sessions()
            .iter()
            .any(|session| session.managed_key_id.as_deref() == Some(key_id.as_str()))
        {
            self.delete_managed_key(key_id, cx);
            return;
        }

        let view = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("confirm_delete").to_string())
                .w(px(420.))
                .content(move |content, _window, _cx| {
                    content.child(div().text_sm().child(t!("key_delete_confirm").to_string()))
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-delete-managed-key")
                                .ghost()
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new(format!("confirm-delete-managed-key-{key_id}"))
                                .danger()
                                .label(t!("delete").to_string())
                                .on_click({
                                    let view = view.clone();
                                    let key_id = key_id.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.delete_managed_key(key_id.clone(), cx);
                                        });
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
        });
    }

    pub(crate) fn show_delete_confirm_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let selected_entries = self
            .active_sftp()
            .map(|s| s.selected_entries.clone())
            .unwrap_or_default();
        if selected_entries.is_empty() {
            return;
        }

        let has_system_path = selected_entries.iter().any(|path| {
            let p = path.as_str();
            p.starts_with("/bin/")
                || p == "/bin"
                || p.starts_with("/etc/")
                || p == "/etc"
                || p.starts_with("/usr/")
                || p == "/usr"
                || p.starts_with("/var/")
                || p == "/var"
                || p.starts_with("/sys/")
                || p == "/sys"
                || p.starts_with("/dev/")
                || p == "/dev"
                || p.starts_with("/boot/")
                || p == "/boot"
                || p.starts_with("/lib/")
                || p == "/lib"
                || p.starts_with("/opt/")
                || p == "/opt"
                || p.starts_with("/run/")
                || p == "/run"
                || p.starts_with("/sbin/")
                || p == "/sbin"
        });

        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("confirm_delete").to_string())
                .w(px(500.))
                .keyboard(false)
                .on_ok({
                    let view = view.clone();
                    let paths_to_delete: Vec<String> =
                        selected_entries.clone().into_iter().collect();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            if let Some(handle) = this.active_sftp_handle() {
                                let _ = handle.commands.send(
                                    crate::sftp::SftpCommand::DeletePaths(paths_to_delete.clone()),
                                );
                            }
                            if let Some(sftp) = this.active_sftp_mut() {
                                sftp.selected_entries.clear();
                            }
                            cx.notify();
                        });
                        window.close_dialog(cx);
                        true
                    }
                })
                .content({
                    let view = view.clone();
                    move |content, _window, cx| {
                        let scroll_handle = view.read(cx).sftp_delete_scroll_handle.clone();
                        let selected_paths: Vec<String> = view
                            .read(cx)
                            .active_sftp()
                            .map(|s| s.selected_entries.clone().into_iter().collect())
                            .unwrap_or_default();

                        let warning_block = if has_system_path {
                            Some(
                                div()
                                    .w_full()
                                    .p_3()
                                    .mb_3()
                                    .rounded_md()
                                    .bg(gpui::rgba(0xff00001a))
                                    .border_1()
                                    .border_color(gpui::rgba(0xff000080))
                                    .child(
                                        div()
                                            .text_color(gpui::rgba(0xff0000ff))
                                            .font_weight(FontWeight::BOLD)
                                            .child(t!("system_path_warning").to_string()),
                                    ),
                            )
                        } else {
                            None
                        };

                        let paths_list = div()
                            .relative()
                            .max_h(px(200.))
                            .w_full()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .rounded_md()
                            .child(
                                v_flex()
                                    .id("delete-scroll-view")
                                    .size_full()
                                    .track_scroll(&scroll_handle)
                                    .overflow_y_scroll()
                                    .p_2()
                                    .gap_1()
                                    .children(selected_paths.into_iter().map(|path| {
                                        div()
                                            .text_size(rems(0.917))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(path)
                                    })),
                            )
                            .child(
                                div().absolute().top_0().bottom_0().right_0().child(
                                    gpui_component::scroll::Scrollbar::vertical(&scroll_handle)
                                        .scrollbar_show(
                                            gpui_component::scroll::ScrollbarShow::Always,
                                        ),
                                ),
                            );

                        content.child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .children(warning_block)
                                .child(
                                    div().text_size(rems(1.0)).mb_2().child(
                                        t!(
                                            "confirm_delete_desc",
                                            count = view
                                                .read(cx)
                                                .active_sftp()
                                                .map(|s| s.selected_entries.len())
                                                .unwrap_or(0)
                                        )
                                        .to_string(),
                                    ),
                                )
                                .child(paths_list),
                        )
                    }
                })
                .footer({
                    let view = view.clone();
                    let paths_to_delete: Vec<String> =
                        selected_entries.clone().into_iter().collect();
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel")
                                .ghost()
                                .label(t!("cancel").to_string())
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            Button::new("confirm")
                                .danger()
                                .label(t!("confirm").to_string())
                                .on_click({
                                    let view = view.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            if let Some(handle) = this.active_sftp_handle() {
                                                let _ = handle.commands.send(
                                                    crate::sftp::SftpCommand::DeletePaths(
                                                        paths_to_delete.clone(),
                                                    ),
                                                );
                                            }
                                            if let Some(sftp) = this.active_sftp_mut() {
                                                sftp.selected_entries.clear();
                                            }
                                            cx.notify();
                                        });
                                        window.close_dialog(cx);
                                    }
                                }),
                        )
                })
        });
    }

    fn apply_sftp_create_input(
        &mut self,
        input: &Entity<InputState>,
        base_path: &str,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            self.status = t!("sftp_invalid_name").into();
            cx.notify();
            return false;
        }
        if let Some(handle) = self.active_sftp_handle() {
            let path = crate::sftp::join_remote(base_path, &name);
            let command = if is_dir {
                crate::sftp::SftpCommand::CreateDir(path)
            } else {
                crate::sftp::SftpCommand::CreateFile(path)
            };
            let _ = handle.commands.send(command);
        }
        cx.notify();
        true
    }

    pub(crate) fn show_sftp_create_dialog(
        &mut self,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let base_path = self
            .active_sftp()
            .map(|sftp| sftp.current_path.clone())
            .unwrap_or_else(|| "/".to_string());
        let input = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).placeholder(if is_dir {
                t!("sftp_new_folder_name").to_string()
            } else {
                t!("sftp_new_file_name").to_string()
            })
        });
        let submit_input = input.clone();
        let focus_input = input.clone();
        window.open_dialog(cx, move |dialog: Dialog, window, _| {
            let submit_input = submit_input.clone();
            let content_input = input.clone();
            let confirm_input = input.clone();
            let confirm_base_path = base_path.clone();
            dialog
                .title(if is_dir {
                    t!("sftp_new_folder").to_string()
                } else {
                    t!("sftp_new_file").to_string()
                })
                .w(px(420.))
                .on_ok({
                    let view = view.clone();
                    let base_path = base_path.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.apply_sftp_create_input(&submit_input, &base_path, is_dir, cx)
                        })
                    }
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-sftp-create")
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm-sftp-create")
                                .primary()
                                .label(t!("confirm").to_string())
                                .on_click(window.listener_for(
                                    &view,
                                    move |this, _, window, cx| {
                                        if this.apply_sftp_create_input(
                                            &confirm_input,
                                            &confirm_base_path,
                                            is_dir,
                                            cx,
                                        ) {
                                            window.close_dialog(cx);
                                        }
                                    },
                                )),
                        ),
                )
                .content(move |content, _, _| content.child(Input::new(&content_input).w_full()))
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn apply_sftp_rename_input(
        &mut self,
        input: &Entity<InputState>,
        remote_path: &str,
        parent: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let name = input.read(cx).value().trim().to_string();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            self.status = t!("sftp_invalid_name").into();
            cx.notify();
            return false;
        }
        if let Some(handle) = self.active_sftp_handle() {
            let _ = handle.commands.send(crate::sftp::SftpCommand::RenamePath {
                old_path: remote_path.to_string(),
                new_path: crate::sftp::join_remote(parent, &name),
            });
        }
        cx.notify();
        true
    }

    pub(crate) fn show_sftp_rename_dialog(
        &mut self,
        remote_path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let old_name = remote_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let parent = crate::sftp::parent_dir(&remote_path).unwrap_or_else(|| "/".to_string());
        let input =
            cx.new(|cx| gpui_component::input::InputState::new(window, cx).default_value(old_name));
        let submit_input = input.clone();
        let focus_input = input.clone();
        window.open_dialog(cx, move |dialog: Dialog, window, _| {
            let submit_input = submit_input.clone();
            let content_input = input.clone();
            let confirm_input = input.clone();
            let confirm_remote_path = remote_path.clone();
            let confirm_parent = parent.clone();
            dialog
                .title(t!("sftp_rename").to_string())
                .w(px(420.))
                .on_ok({
                    let view = view.clone();
                    let remote_path = remote_path.clone();
                    let parent = parent.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.apply_sftp_rename_input(&submit_input, &remote_path, &parent, cx)
                        })
                    }
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-sftp-rename")
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm-sftp-rename")
                                .primary()
                                .label(t!("confirm").to_string())
                                .on_click(window.listener_for(
                                    &view,
                                    move |this, _, window, cx| {
                                        if this.apply_sftp_rename_input(
                                            &confirm_input,
                                            &confirm_remote_path,
                                            &confirm_parent,
                                            cx,
                                        ) {
                                            window.close_dialog(cx);
                                        }
                                    },
                                )),
                        ),
                )
                .content(move |content, _, _| content.child(Input::new(&content_input).w_full()))
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn apply_sftp_permissions_form(
        &mut self,
        form: &Entity<SftpPermissionsForm>,
        remote_path: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let (value, recursive, apply_to) = {
            let form = form.read(cx);
            (
                form.input.read(cx).value().trim().to_string(),
                form.recursive,
                form.apply_to,
            )
        };
        let Ok(mode) = u32::from_str_radix(&value, 8) else {
            self.status = t!("sftp_invalid_permissions").into();
            cx.notify();
            return false;
        };
        if value.len() < 3 || value.len() > 4 || mode > 0o7777 {
            self.status = t!("sftp_invalid_permissions").into();
            cx.notify();
            return false;
        }
        if let Some(handle) = self.active_sftp_handle() {
            let _ = handle
                .commands
                .send(crate::sftp::SftpCommand::SetPermissions {
                    remote_path: remote_path.to_string(),
                    mode,
                    recursive,
                    apply_to,
                });
        }
        cx.notify();
        true
    }

    pub(crate) fn show_sftp_permissions_dialog(
        &mut self,
        remote_path: String,
        is_dir: bool,
        permissions: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let initial_mode = permissions.unwrap_or(if is_dir { 0o755 } else { 0o644 });
        let form = cx.new(|cx| {
            SftpPermissionsForm::new(remote_path.clone(), is_dir, initial_mode, window, cx)
        });
        let focus_input = form.read(cx).input.clone();
        let submit_form = form.clone();
        window.open_dialog(cx, move |dialog: Dialog, window, _| {
            let submit_form = submit_form.clone();
            let content_form = form.clone();
            let confirm_form = form.clone();
            let confirm_path = remote_path.clone();
            let content_max_height = (window.viewport_size().height - px(220.))
                .max(px(180.))
                .min(px(520.));
            dialog
                .title(t!("sftp_file_permissions").to_string())
                .w(px(560.))
                .on_ok({
                    let view = view.clone();
                    let remote_path = remote_path.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.apply_sftp_permissions_form(&submit_form, &remote_path, cx)
                        })
                    }
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-sftp-permissions")
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm-sftp-permissions")
                                .primary()
                                .label(t!("confirm").to_string())
                                .on_click(window.listener_for(
                                    &view,
                                    move |this, _, window, cx| {
                                        if this.apply_sftp_permissions_form(
                                            &confirm_form,
                                            &confirm_path,
                                            cx,
                                        ) {
                                            window.close_dialog(cx);
                                        }
                                    },
                                )),
                        ),
                )
                .content(move |content, _, _| {
                    content.child(
                        div()
                            .id("sftp-permissions-scroll")
                            .max_h(content_max_height)
                            .overflow_y_scroll()
                            .child(content_form.clone()),
                    )
                })
        });
        crate::app::input_focus::defer_focus_input_at_end(focus_input, window, cx);
    }

    fn apply_sftp_delete_paths(&mut self, paths: &[String], quick: bool, cx: &mut Context<Self>) {
        if let Some(handle) = self.active_sftp_handle() {
            let command = if quick {
                crate::sftp::SftpCommand::QuickDeletePaths(paths.to_vec())
            } else {
                crate::sftp::SftpCommand::DeletePaths(paths.to_vec())
            };
            let _ = handle.commands.send(command);
        }
        if let Some(sftp) = self.active_sftp_mut() {
            sftp.selected_entries.clear();
        }
        cx.notify();
    }

    pub(crate) fn show_sftp_delete_paths_confirm_dialog(
        &mut self,
        paths: Vec<String>,
        quick: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let submit_paths = paths.clone();
        window.open_dialog(cx, move |dialog: Dialog, window, _| {
            let confirm_paths = paths.clone();
            let content_max_height = (window.viewport_size().height - px(220.))
                .max(px(160.))
                .min(px(420.));
            dialog
                .title(if quick {
                    t!("sftp_quick_delete_title").to_string()
                } else {
                    t!("confirm_delete").to_string()
                })
                .w(px(520.))
                .on_ok({
                    let view = view.clone();
                    let submit_paths = submit_paths.clone();
                    move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.apply_sftp_delete_paths(&submit_paths, quick, cx);
                        });
                        true
                    }
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new(if quick {
                                "cancel-sftp-quick-delete"
                            } else {
                                "cancel-sftp-delete"
                            })
                            .label(t!("cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new(if quick {
                                "confirm-sftp-quick-delete"
                            } else {
                                "confirm-sftp-delete"
                            })
                            .danger()
                            .label(t!("confirm").to_string())
                            .on_click(window.listener_for(
                                &view,
                                move |this, _, window, cx| {
                                    this.apply_sftp_delete_paths(&confirm_paths, quick, cx);
                                    window.close_dialog(cx);
                                },
                            )),
                        ),
                )
                .content({
                    let paths = paths.clone();
                    move |content, _, cx| {
                        let mut body = v_flex().gap_3().child(
                            div().child(t!("confirm_delete_desc", count = paths.len()).to_string()),
                        );
                        if quick {
                            body = body.child(
                                div()
                                    .w_full()
                                    .p_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(cx.theme().danger)
                                    .bg(cx.theme().danger.opacity(0.12))
                                    .text_color(cx.theme().danger)
                                    .child(t!("sftp_quick_delete_warning").to_string()),
                            );
                        }
                        body = body.child(v_flex().gap_1().children(paths.iter().map(|path| {
                            div()
                                .text_size(rems(0.833))
                                .text_color(cx.theme().muted_foreground)
                                .child(path.clone())
                        })));
                        content.child(
                            div()
                                .id("sftp-delete-confirm-scroll")
                                .max_h(content_max_height)
                                .overflow_y_scroll()
                                .child(body),
                        )
                    }
                })
        });
    }
    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.updater_status,
            Some(crate::app::updater::UpdateStatus::Checking)
                | Some(crate::app::updater::UpdateStatus::Downloading(_, _, _))
        ) {
            return;
        }

        self.updater_status = Some(crate::app::updater::UpdateStatus::Checking);
        cx.notify();
        let view = cx.entity();
        cx.spawn({
            let view = view.clone();
            |_, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let (tx, rx) = futures::channel::oneshot::channel();
                    crate::app::shared_runtime().spawn(async move {
                        let result = crate::app::updater::check_for_update().await;
                        let _ = tx.send(result);
                    });
                    let result = rx
                        .await
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("update check cancelled")));
                    cx.update(|cx| match result {
                        Ok(crate::app::updater::UpdateCheckResult::UpdateAvailable(info)) => {
                            view.update(cx, |this, cx| {
                                this.updater_status =
                                    Some(crate::app::updater::UpdateStatus::UpdateAvailable(info));
                                cx.notify();
                            });
                        }
                        Ok(crate::app::updater::UpdateCheckResult::UpToDate(info)) => {
                            view.update(cx, |this, cx| {
                                this.updater_status =
                                    Some(crate::app::updater::UpdateStatus::UpToDate(info));
                                cx.notify();
                            });
                        }
                        Err(err) => {
                            view.update(cx, |this, cx| {
                                this.updater_status = Some(
                                    crate::app::updater::UpdateStatus::Error(format!("{err:#}")),
                                );
                                cx.notify();
                            });
                        }
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn download_available_update(&mut self, cx: &mut Context<Self>) {
        let info = match self.updater_status.clone() {
            Some(crate::app::updater::UpdateStatus::UpdateAvailable(info)) => info,
            _ => return,
        };
        self.updater_status = Some(crate::app::updater::UpdateStatus::Downloading(
            info.clone(),
            0,
            info.size,
        ));
        cx.notify();

        let view = cx.entity();
        cx.spawn({
            let view = view.clone();
            |_, cx: &mut gpui::AsyncApp| {
                let cx = cx.clone();
                async move {
                    let (result_tx, result_rx) = futures::channel::oneshot::channel();
                    let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded();
                    let update_info = info.clone();
                    crate::app::shared_runtime().spawn(async move {
                        let result =
                            crate::app::updater::download_update(&update_info, |done, total| {
                                let _ = progress_tx.unbounded_send((done, total));
                            })
                            .await;
                        let _ = result_tx.send(result);
                    });
                    use futures::StreamExt as _;
                    while let Some((done, total)) = progress_rx.next().await {
                        cx.update(|cx| {
                            view.update(cx, |this, cx| {
                                this.updater_status =
                                    Some(crate::app::updater::UpdateStatus::Downloading(
                                        info.clone(),
                                        done,
                                        total,
                                    ));
                                cx.notify();
                            });
                        });
                    }
                    let result = result_rx
                        .await
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("update download cancelled")));
                    cx.update(|cx| match result {
                        Ok(path) => {
                            view.update(cx, |this, cx| {
                                this.updater_status =
                                    Some(crate::app::updater::UpdateStatus::ReadyToRestart(
                                        info.clone(),
                                        path,
                                    ));
                                cx.notify();
                            });
                        }
                        Err(err) => {
                            view.update(cx, |this, cx| {
                                this.updater_status = Some(
                                    crate::app::updater::UpdateStatus::Error(format!("{err:#}")),
                                );
                                cx.notify();
                            });
                        }
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn confirm_update_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(crate::app::updater::UpdateStatus::ReadyToRestart(info, path)) =
            self.updater_status.clone()
        else {
            return;
        };

        let view = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            let version = info.version.clone();
            let path = path.clone();
            let view = view.clone();
            dialog
                .title(t!("update_restart_confirm_title").to_string())
                .w(px(440.))
                .content(move |content, _window, _cx| {
                    content.child(div().text_sm().child(
                        t!("update_restart_confirm_desc", version = version.clone()).to_string(),
                    ))
                })
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("cancel-update-restart")
                                .ghost()
                                .label(t!("cancel").to_string())
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("confirm-update-restart")
                                .primary()
                                .label(t!("update_restart_now").to_string())
                                .on_click({
                                    let path = path.clone();
                                    let view = view.clone();
                                    move |_, _window, _cx| {
                                        if let Err(error) =
                                            crate::app::updater::install_and_restart(&path)
                                        {
                                            tracing::error!("failed to install update: {error:#}");
                                            view.update(_cx, |this, cx| {
                                                this.updater_status =
                                                    Some(crate::app::updater::UpdateStatus::Error(
                                                        format!("{error:#}"),
                                                    ));
                                                cx.notify();
                                            });
                                        }
                                    }
                                }),
                        ),
                )
        });
    }

    pub(crate) fn show_update_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            return;
        }
        if self.updater_status.is_none() {
            self.check_for_updates(cx);
        }
        self.active_dialog = Some(crate::app::DialogKind::Updater);

        let view = cx.entity();
        let notes_scroll_handle = gpui::ScrollHandle::new();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("update_dialog_title").to_string())
                .w(px(600.))
                .h(px(520.))
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
                    let notes_scroll_handle = notes_scroll_handle.clone();
                    move |content, window, cx| {
                        let current_version = env!("CARGO_PKG_VERSION");
                        let status = view.read(cx).updater_status.clone();
                        let can_restart = matches!(
                            &status,
                            Some(crate::app::updater::UpdateStatus::ReadyToRestart(_, _))
                        );
                        let (title, detail, notes, has_update, is_busy, is_error) = match status.clone() {
                            Some(crate::app::updater::UpdateStatus::Checking) => (
                                t!("checking_update").to_string(),
                                format!("{} v{current_version}", t!("update_current_version")),
                                String::new(),
                                false,
                                true,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::UpToDate(info)) => (
                                t!("update_no_update").to_string(),
                                format!(
                                    "{} v{current_version}  ·  {} v{}",
                                    t!("update_current_version"),
                                    t!("update_latest_version"),
                                    info.version
                                ),
                                info.notes,
                                false,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::UpdateAvailable(info)) => (
                                t!("update_available", version = info.version.clone()).to_string(),
                                format!(
                                    "{} v{current_version}  ·  {} v{}",
                                    t!("update_current_version"),
                                    t!("update_latest_version"),
                                    info.version
                                ),
                                info.notes,
                                true,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::Downloading(info, done, total)) => (
                                t!("update_downloading").to_string(),
                                if total > 0 {
                                    format!("{} / {}", format_bytes(done), format_bytes(total))
                                } else {
                                    format_bytes(done)
                                },
                                info.notes,
                                false,
                                true,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::ReadyToRestart(info, _)) => (
                                t!("update_install_complete").to_string(),
                                t!("update_restart_hint").to_string(),
                                info.notes,
                                false,
                                false,
                                false,
                            ),
                            Some(crate::app::updater::UpdateStatus::Error(error)) => (
                                t!("update_check_failed").to_string(),
                                error,
                                String::new(),
                                false,
                                false,
                                true,
                            ),
                            None => (
                                t!("update_no_update").to_string(),
                                format!("{} v{current_version}", t!("update_current_version")),
                                String::new(),
                                false,
                                false,
                                false,
                            ),
                        };

                        let note_rows = notes
                            .lines()
                            .filter_map(|line| {
                                let line = line.trim();
                                if line.is_empty() {
                                    return None;
                                }
                                let is_heading = line.starts_with('#');
                                let text = if is_heading {
                                    line.trim_start_matches('#').trim().to_string()
                                } else if let Some(item) = line
                                    .strip_prefix("- ")
                                    .or_else(|| line.strip_prefix("* "))
                                {
                                    format!("• {item}")
                                } else {
                                    line.to_string()
                                };
                                Some(
                                    div()
                                        .w_full()
                                        .text_size(if is_heading { rems(0.92) } else { rems(0.8) })
                                        .when(is_heading, |this| {
                                            this.font_weight(FontWeight::SEMIBOLD)
                                        })
                                        .child(text),
                                )
                            })
                            .collect::<Vec<_>>();

                        content.child(
                            v_flex()
                                .size_full()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap_3()
                                        .p_3()
                                        .rounded_lg()
                                        .bg(cx.theme().muted.opacity(0.45))
                                        .child(
                                            div()
                                                .size(px(10.))
                                                .rounded_full()
                                                .bg(if is_error {
                                                    cx.theme().danger
                                                } else if has_update {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().success
                                                }),
                                        )
                                        .child(
                                            v_flex()
                                                .min_w(px(0.))
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(rems(1.05))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(rems(0.78))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(detail),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(rems(0.85))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("update_release_notes").to_string()),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .bg(cx.theme().background)
                                        .child(
                                            v_flex()
                                                .id("update-notes-scroll")
                                                .size_full()
                                                .track_scroll(&notes_scroll_handle)
                                                .overflow_y_scroll()
                                                .p_3()
                                                .gap_2()
                                                .when(note_rows.is_empty(), |this| {
                                                    this.items_center()
                                                        .justify_center()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(t!("update_no_release_notes").to_string())
                                                })
                                                .children(note_rows),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .right_0()
                                                .bottom_0()
                                                .child(
                                                    Scrollbar::vertical(&notes_scroll_handle)
                                                        .scrollbar_show(ScrollbarShow::Scrolling),
                                                ),
                                        ),
                                )
                                .when(
                                    matches!(
                                        status,
                                        Some(crate::app::updater::UpdateStatus::Downloading(_, _, _))
                                    ),
                                    |this| {
                                        let (done, total) = match status.as_ref() {
                                            Some(crate::app::updater::UpdateStatus::Downloading(_, done, total)) => (*done, *total),
                                            _ => unreachable!(),
                                        };
                                        this.child(
                                            Progress::new("update-download-progress")
                                                .with_size(px(5.))
                                                .value(if total > 0 { done as f32 / total as f32 } else { 0.0 })
                                                .color(cx.theme().primary)
                                                .w_full(),
                                        )
                                    },
                                )
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("update-release-page")
                                                .ghost()
                                                .label(t!("update_release_page").to_string())
                                                .on_click(|_, _, _| {
                                                    let _ = open::that(
                                                        "https://github.com/ynx-official/tiny-shell/releases/latest",
                                                    );
                                                }),
                                        )
                                        .child(
                                            Button::new("update-check-again")
                                                .secondary()
                                                .disabled(is_busy)
                                                .label(t!("check_update").to_string())
                                                .on_click(window.listener_for(
                                                    &view,
                                                    |this, _, _, cx| this.check_for_updates(cx),
                                                )),
                                        )
                                        .when(has_update, |this| {
                                            this.child(
                                                Button::new("update-download")
                                                    .primary()
                                                    .label(t!("update_download").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, _, cx| {
                                                            this.download_available_update(cx)
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(can_restart, |this| {
                                            this.child(
                                                Button::new("update-restart")
                                                    .primary()
                                                    .label(t!("update_restart_now").to_string())
                                                    .on_click(window.listener_for(
                                                        &view,
                                                        |this, _, window, cx| {
                                                            this.confirm_update_restart(window, cx)
                                                        },
                                                    )),
                                            )
                                        }),
                                ),
                        )
                    }
                })
        });
    }

    pub(crate) fn render_settings_content(
        &self,
        view: &gpui::Entity<Self>,
        settings_id: &'static str,
        cx: &mut gpui::App,
    ) -> gpui::AnyElement {
        use gpui::IntoElement;
        use gpui_component::setting::{
            SettingField, SettingGroup, SettingItem, SettingPage, Settings,
        };
        let version = env!("CARGO_PKG_VERSION");
        let view_clone_for_general = view.clone();
        let sync_endpoint_input = self.sync_endpoint_input.clone();
        let sync_username_input = self.sync_username_input.clone();
        let sync_webdav_password_input = self.sync_webdav_password_input.clone();
        let sync_s3_endpoint_input = self.sync_s3_endpoint_input.clone();
        let sync_s3_region_input = self.sync_s3_region_input.clone();
        let sync_s3_bucket_input = self.sync_s3_bucket_input.clone();
        let sync_s3_object_key_input = self.sync_s3_object_key_input.clone();
        let sync_s3_access_key_input = self.sync_s3_access_key_input.clone();
        let sync_s3_secret_key_input = self.sync_s3_secret_key_input.clone();
        let sync_s3_session_token_input = self.sync_s3_session_token_input.clone();
        let sync_encryption_password_input = self.sync_encryption_password_input.clone();

        let focus_handle = self.focus_handle.clone();

        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                            div()
                                .flex()
                                .flex_col()
                                .size_full()
                                .min_w_0()
                                .min_h_0()
                                .track_focus(&focus_handle)
                                .on_key_down({
                                    let view = view.clone();
                                    move |ev: &gpui::KeyDownEvent, window, cx| {
                                        view.update(cx, |this, cx| {
                                            let Some(action) = this.recording_action.clone() else {
                                                return;
                                            };

                                            window.prevent_default();
                                            cx.stop_propagation();

                                            if ev.keystroke.key == "escape" {
                                                this.recording_action = None;
                                                cx.notify();
                                                return;
                                            }

                                            let Some(new_key) = crate::app::keybinding_recorder::normalize_recorded_keystroke(ev) else {
                                                return;
                                            };

                                            // Check for conflicts with other actions
                                            if let Some((_conflict_id, conflict_label)) =
                                                crate::app::keybinding_recorder::find_conflict(
                                                    &this.config,
                                                    &action,
                                                    &new_key,
                                                )
                                            {
                                                let formatted = crate::app::keybinding_recorder::format_keystroke(&new_key);
                                                this.recording_action = None;
                                                this.keybind_error = Some((
                                                    action.clone(),
                                                    t!("keybind_conflict", key = formatted, action = conflict_label).to_string(),
                                                ));
                                                cx.notify();
                                                return;
                                            }

                                            this.recording_action = None;
                                            this.keybind_error = None;
                                            this.config.set_key_binding(&action, &new_key);
                                            this.mark_config_preferences_dirty();
                                            cx.notify();
                                        });
                                    }
                                })
                                .on_mouse_down_out({
                                    let view = view.clone();
                                    move |_, _window, cx| {
                                        view.update(cx, |this, cx| {
                                            if this.recording_action.is_some() {
                                                this.recording_action = None;
                                                cx.notify();
                                            }
                                        });
                                    }
                                })
                                .child(
                                    Settings::new(settings_id)
                                        .sidebar_width(px(180.))
                                        .sidebar_style(div().bg(cx.theme().background).style())
                                .page(
                                    SettingPage::new(t!("settings_general").to_string())
                                        .icon(IconName::Settings)
                                        .default_open(true)
                                        .group(
                                            SettingGroup::new()
                                                .title(t!("settings_group_appearance").to_string())
                                                .item(
                                                    SettingItem::new(
                                                        t!("theme_mode").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                let (follow_system, is_dark_mode) = {
                                                                    let state = view.read(cx);
                                                                    (state.follow_system_theme, state.theme_mode.is_dark())
                                                                };
                                                                Button::new("theme-mode-dropdown")
                                                                    .small()
                                                                    .icon(if follow_system { IconName::Sun } else if is_dark_mode { IconName::Moon } else { IconName::Sun })
                                                                    .label(if follow_system { t!("follow_system").to_string() } else if is_dark_mode { t!("use_dark_mode").to_string() } else { t!("use_light_mode").to_string() })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let (follow_system, is_dark_mode) = {
                                                                                let state = view.read(cx);
                                                                                (state.follow_system_theme, state.theme_mode.is_dark())
                                                                            };
                                                                            menu = menu.min_w(160.)
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("follow_system").to_string())
                                                                                        .checked(follow_system)
                                                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                                            this.set_follow_system_theme(true, window, cx)
                                                                                        }))
                                                                                )
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("use_light_mode").to_string())
                                                                                        .checked(!follow_system && !is_dark_mode)
                                                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                                            this.switch_theme_mode(crate::app::ThemeMode::Light, window, cx)
                                                                                        }))
                                                                                )
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("use_dark_mode").to_string())
                                                                                        .checked(!follow_system && is_dark_mode)
                                                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                                            this.switch_theme_mode(crate::app::ThemeMode::Dark, window, cx)
                                                                                        }))
                                                                                );
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("light_theme").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                let current_theme = view.read(cx).light_theme_name.to_string();
                                                                Button::new("light-theme-dropdown")
                                                                    .small()
                                                                    .icon(IconName::Sun)
                                                                    .label(current_theme.clone())
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let current_theme = view.read(cx).light_theme_name.to_string();
                                                                            let themes = gpui_component::ThemeRegistry::global(cx).sorted_themes();
                                                                            let light_themes: Vec<_> = themes.into_iter().filter(|t| !t.mode.is_dark()).map(|t| t.name.clone()).collect();
                                                                            menu = menu.min_w(160.).max_h(px(320.)).scrollable(true);
                                                                            for theme_name in light_themes {
                                                                                let checked = theme_name == current_theme;
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(theme_name.clone())
                                                                                        .checked(checked)
                                                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                                                            this.apply_theme(theme_name.clone(), window, cx)
                                                                                        }))
                                                                                );
                                                                            }
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("dark_theme").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                let current_theme = view.read(cx).dark_theme_name.to_string();
                                                                Button::new("dark-theme-dropdown")
                                                                    .small()
                                                                    .icon(IconName::Moon)
                                                                    .label(current_theme.clone())
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let current_theme = view.read(cx).dark_theme_name.to_string();
                                                                            let themes = gpui_component::ThemeRegistry::global(cx).sorted_themes();
                                                                            let dark_themes: Vec<_> = themes.into_iter().filter(|t| t.mode.is_dark()).map(|t| t.name.clone()).collect();
                                                                            menu = menu.min_w(160.).max_h(px(320.)).scrollable(true);
                                                                            for theme_name in dark_themes {
                                                                                let checked = theme_name == current_theme;
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(theme_name.clone())
                                                                                        .checked(checked)
                                                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                                                            this.apply_theme(theme_name.clone(), window, cx)
                                                                                        }))
                                                                                );
                                                                            }
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        format!("{}{}", t!("title_bar_style"), t!("restart_hint")),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                let current_style = view.read(cx).config.title_bar_style();
                                                                Button::new("title-bar-style-dropdown")
                                                                    .small()
                                                                    .label(match current_style {
                                                                        crate::session::config::TitleBarStyle::Native => t!("title_bar_native").to_string(),
                                                                        crate::session::config::TitleBarStyle::Integrated => t!("title_bar_integrated").to_string(),
                                                                    })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let current_style = view.read(cx).config.title_bar_style();
                                                                            menu = menu.min_w(160.)
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("title_bar_native").to_string())
                                                                                        .checked(current_style == crate::session::config::TitleBarStyle::Native)
                                                                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                                            this.config.set_title_bar_style(crate::session::config::TitleBarStyle::Native);
                                                                                            this.mark_config_preferences_dirty();
                                                                                            cx.notify();
                                                                                        }))
                                                                                )
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("title_bar_integrated").to_string())
                                                                                        .checked(current_style == crate::session::config::TitleBarStyle::Integrated)
                                                                                        .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                                            this.config.set_title_bar_style(crate::session::config::TitleBarStyle::Integrated);
                                                                                            this.mark_config_preferences_dirty();
                                                                                            cx.notify();
                                                                                        }))
                                                                                );
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                        )
                                        .group(
                                            SettingGroup::new()
                                                .title(t!("settings_group_font").to_string())
                                                .item(
                                                    SettingItem::new(
                                                        t!("ui_font_size").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, window, cx| {
                                                                h_flex()
                                                                    .items_center()
                                                                    .gap_3()
                                                                    .child(Button::new("ui-font-size-down").small().label("-").on_click(window.listener_for(&view, |this, _, _, cx| this.change_ui_font_size(-1.0, cx))))
                                                                    .child(div().min_w(px(64.)).text_center().child(format!("{:.0}px", view.read(cx).ui_font_size)))
                                                                    .child(Button::new("ui-font-size-up").small().label("+").on_click(window.listener_for(&view, |this, _, _, cx| this.change_ui_font_size(1.0, cx))))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("terminal_font_size").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, window, cx| {
                                                                h_flex()
                                                                    .items_center()
                                                                    .gap_3()
                                                                    .child(Button::new("terminal-font-size-down").small().label("-").on_click(window.listener_for(&view, |this, _, _, cx| this.change_terminal_font_size(-1.0, cx))))
                                                                    .child(div().min_w(px(64.)).text_center().child(format!("{:.0}px", view.read(cx).terminal_font_size)))
                                                                    .child(Button::new("terminal-font-size-up").small().label("+").on_click(window.listener_for(&view, |this, _, _, cx| this.change_terminal_font_size(1.0, cx))))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("ui_font_family").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                Button::new("ui-font-dropdown")
                                                                    .small()
                                                                    .icon(IconName::ChevronsUpDown)
                                                                    .label({
                                                                        let current = view.read(cx).ui_font_family.to_string();
                                                                        let names = cx.text_system().all_font_names();
                                                                        let using_system_maple = crate::app::theme::USING_SYSTEM_MAPLE.load(std::sync::atomic::Ordering::Relaxed);
                                                                        if current == *".SystemUIFont" || current.is_empty() || !names.contains(&current) {
                                                                            t!("system_default").to_string()
                                                                        } else if !using_system_maple && current == "Maple Mono NF CN" {
                                                                            format!("Maple Mono NF CN ({})", t!("software_builtin"))
                                                                        } else {
                                                                            current
                                                                        }
                                                                    })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let current = view.read(cx).ui_font_family.to_string();
                                                                            let mut names = cx.text_system().all_font_names();
                                                                            menu = menu.min_w(200.).max_h(px(320.)).scrollable(true);
                                                                            menu = menu.item(
                                                                                PopupMenuItem::new(t!("system_default").to_string())
                                                                                    .checked(current == *".SystemUIFont" || current.is_empty())
                                                                                    .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                                                        this.change_ui_font_family(".SystemUIFont", window, cx);
                                                                                    }))
                                                                            );
                                                                            let maple_font = "Maple Mono NF CN".to_string();
                                                                            let using_system_maple = crate::app::theme::USING_SYSTEM_MAPLE.load(std::sync::atomic::Ordering::Relaxed);
                                                                            if !using_system_maple && names.contains(&maple_font) {
                                                                                names.retain(|n| n != &maple_font);
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(format!("{} ({})", maple_font, t!("software_builtin")))
                                                                                        .checked(current == maple_font)
                                                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                                                            this.change_ui_font_family("Maple Mono NF CN", window, cx);
                                                                                        }))
                                                                                ).separator();
                                                                            }
                                                                            for name in names {
                                                                                let checked = name == current;
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(name.clone())
                                                                                        .checked(checked)
                                                                                        .on_click(window.listener_for(&view, move |this, _, window, cx| {
                                                                                            this.change_ui_font_family(&name, window, cx);
                                                                                        }))
                                                                                );
                                                                            }
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("terminal_font_family").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                Button::new("terminal-font-dropdown")
                                                                    .small()
                                                                    .icon(IconName::ChevronsUpDown)
                                                                    .label({
                                                                        let current = view.read(cx).terminal_font_family.to_string();
                                                                        let using_system_maple = crate::app::theme::USING_SYSTEM_MAPLE.load(std::sync::atomic::Ordering::Relaxed);
                                                                        if !using_system_maple && current == "Maple Mono NF CN" {
                                                                            format!("Maple Mono NF CN ({})", t!("software_builtin"))
                                                                        } else {
                                                                            current
                                                                        }
                                                                    })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let current = view.read(cx).terminal_font_family.to_string();
                                                                            let mut names = cx.text_system().all_font_names();
                                                                            menu = menu.min_w(200.).max_h(px(320.)).scrollable(true);
                                                                            let maple_font = "Maple Mono NF CN".to_string();
                                                                            let using_system_maple = crate::app::theme::USING_SYSTEM_MAPLE.load(std::sync::atomic::Ordering::Relaxed);
                                                                            if !using_system_maple && names.contains(&maple_font) {
                                                                                names.retain(|n| n != &maple_font);
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(format!("{} ({})", maple_font, t!("software_builtin")))
                                                                                        .checked(current == maple_font)
                                                                                        .on_click(window.listener_for(&view, move |this, _, _window, cx| {
                                                                                            this.change_terminal_font_family("Maple Mono NF CN", cx);
                                                                                        }))
                                                                                ).separator();
                                                                            }
                                                                            for name in names {
                                                                                let checked = name == current;
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(name.clone())
                                                                                        .checked(checked)
                                                                                        .on_click(window.listener_for(&view, move |this, _, _window, cx| {
                                                                                            this.change_terminal_font_family(&name, cx);
                                                                                        }))
                                                                                );
                                                                            }
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("cursor_style").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                use crate::session::config::CursorStyle;
                                                                let current = view.read(cx).cursor_style;
                                                                Button::new("cursor-style-dropdown")
                                                                    .small()
                                                                    .icon(IconName::ChevronsUpDown)
                                                                    .label(match current {
                                                                        CursorStyle::Default => t!("cursor_style_default").to_string(),
                                                                        CursorStyle::Blink => t!("cursor_style_blink").to_string(),
                                                                        CursorStyle::Beam => t!("cursor_style_beam").to_string(),
                                                                        CursorStyle::BeamBlink => t!("cursor_style_beam_blink").to_string(),
                                                                    })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            use crate::session::config::CursorStyle;
                                                                            let current = view.read(cx).cursor_style;
                                                                            menu = menu.min_w(160.).max_h(px(320.)).scrollable(true);
                                                                            for style in [
                                                                                CursorStyle::Default,
                                                                                CursorStyle::Blink,
                                                                                CursorStyle::Beam,
                                                                                CursorStyle::BeamBlink,
                                                                            ] {
                                                                                let checked = style == current;
                                                                                let label = match style {
                                                                                    CursorStyle::Default => t!("cursor_style_default").to_string(),
                                                                                    CursorStyle::Blink => t!("cursor_style_blink").to_string(),
                                                                                    CursorStyle::Beam => t!("cursor_style_beam").to_string(),
                                                                                    CursorStyle::BeamBlink => t!("cursor_style_beam_blink").to_string(),
                                                                                };
                                                                                menu = menu.item(
                                                                                    PopupMenuItem::new(label)
                                                                                        .checked(checked)
                                                                                        .on_click(window.listener_for(&view, move |this, _, _window, cx| {
                                                                                            this.change_cursor_style(style, cx);
                                                                                        }))
                                                                                );
                                                                            }
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                        )
                                        .group(
                                            SettingGroup::new()
                                                .title(t!("settings_group_other").to_string())
                                                .item(
                                                    SettingItem::new(
                                                        t!("keyword_highlight").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, window, cx| {
                                                                Switch::new("keyword-highlight")
                                                                    .small()
                                                                    .checked(view.read(cx).config.keyword_highlight())
                                                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                                                        this.config.set_keyword_highlight(*checked);
                                                                        this.mark_config_preferences_dirty();
                                                                        cx.notify();
                                                                    }))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("lock_layout").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, window, cx| {
                                                                Switch::new("lock-layout")
                                                                    .small()
                                                                    .checked(view.read(cx).config.lock_layout())
                                                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                                                        this.config.set_lock_layout(*checked);
                                                                        this.mark_config_preferences_dirty();
                                                                        cx.notify();
                                                                    }))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    ).description(t!("lock_layout_hint").to_string())
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("monitoring_position").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                Button::new("monitoring-position-dropdown")
                                                                    .small()
                                                                    .icon(IconName::PanelLeftOpen)
                                                                    .label({
                                                                        let pos = view.read(cx).config.monitoring_position().to_string();
                                                                        if pos == "Sidebar" {
                                                                            t!("position_sidebar").to_string()
                                                                        } else if pos == "Hidden" {
                                                                            t!("position_hidden").to_string()
                                                                        } else {
                                                                            t!("position_bottom").to_string()
                                                                        }
                                                                    })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let pos = view.read(cx).config.monitoring_position().to_string();
                                                                            menu = menu.min_w(160.)
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("position_bottom").to_string())
                                                                                        .checked(pos == "Bottom")
                                                                                        .on_click(window.listener_for(&view, |this, _, _window, cx| {
                                                                                            this.config.set_monitoring_position("Bottom");
                                                                                            this.mark_config_preferences_dirty();
                                                                                            cx.notify();
                                                                                        }))
                                                                                )
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("position_sidebar").to_string())
                                                                                        .checked(pos == "Sidebar")
                                                                                        .on_click(window.listener_for(&view, |this, _, _window, cx| {
                                                                                            this.config.set_monitoring_position("Sidebar");
                                                                                            this.mark_config_preferences_dirty();
                                                                                            cx.notify();
                                                                                        }))
                                                                                )
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("position_hidden").to_string())
                                                                                        .checked(pos == "Hidden")
                                                                                        .on_click(window.listener_for(&view, |this, _, _window, cx| {
                                                                                            this.config.set_monitoring_position("Hidden");
                                                                                            this.mark_config_preferences_dirty();
                                                                                            cx.notify();
                                                                                        }))
                                                                                );
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("language").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, _window, cx| {
                                                                Button::new("language-dropdown")
                                                                    .small()
                                                                    .icon(IconName::Globe)
                                                                    .label({
                                                                        let current_locale = view.read(cx).config.locale().to_string();
                                                                        if current_locale == "en" {
                                                                            t!("english").to_string()
                                                                        } else if current_locale == "zh-CN" {
                                                                            t!("chinese").to_string()
                                                                        } else {
                                                                            t!("follow_system").to_string()
                                                                        }
                                                                    })
                                                                    .dropdown_menu_with_anchor(Anchor::BottomRight, {
                                                                        let view = view.clone();
                                                                        move |mut menu, window, cx| {
                                                                            let current_locale = view.read(cx).config.locale().to_string();
                                                                            menu = menu.min_w(160.)
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("follow_system").to_string())
                                                                                        .checked(current_locale == "system")
                                                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                                            this.set_display_language("system", window, cx)
                                                                                        }))
                                                                                )
                                                                                .separator()
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("english").to_string())
                                                                                        .checked(current_locale == "en")
                                                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                                            this.set_display_language("en", window, cx)
                                                                                        }))
                                                                                )
                                                                                .item(
                                                                                    PopupMenuItem::new(t!("chinese").to_string())
                                                                                        .checked(current_locale == "zh-CN")
                                                                                        .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                                            this.set_display_language("zh-CN", window, cx)
                                                                                        }))
                                                                                );
                                                                            menu
                                                                        }
                                                                    })
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("reset_layout").to_string(),
                                                        SettingField::render({
                                                            let view = view_clone_for_general.clone();
                                                            move |_, window, _cx| {
                                                                Button::new("reset-layout")
                                                                    .small()
                                                                    .label(t!("reset").to_string())
                                                                    .on_click(window.listener_for(&view, |this, _, window, cx| {
                                                                        this.reset_layout(window, cx);
                                                                    }))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    ).description(t!("reset_layout_hint").to_string())
                                                )
                                        )
                                )
                                .page(
                                    SettingPage::new(t!("settings_sync").to_string())
                                        .icon(IconName::Globe)
                                        .group(
                                            SettingGroup::new()
                                                .title(t!("settings_sync").to_string())
                                                .item(SettingItem::render({
                                                    let view = view.clone();
                                                    let endpoint = sync_endpoint_input.clone();
                                                    let username = sync_username_input.clone();
                                                    let webdav_password = sync_webdav_password_input.clone();
                                                    let s3_endpoint = sync_s3_endpoint_input.clone();
                                                    let s3_region = sync_s3_region_input.clone();
                                                    let s3_bucket = sync_s3_bucket_input.clone();
                                                    let s3_object_key = sync_s3_object_key_input.clone();
                                                    let s3_access_key = sync_s3_access_key_input.clone();
                                                    let s3_secret_key = sync_s3_secret_key_input.clone();
                                                    let s3_session_token = sync_s3_session_token_input.clone();
                                                    let encryption_password = sync_encryption_password_input.clone();
                                                    move |_, window, cx| {
                                                        let in_progress = view.read(cx).sync_in_progress;
                                                        let status = view.read(cx).sync_status.clone();
                                                        let is_s3 = view.read(cx).config.sync_backend() == "s3";
                                                        v_flex()
                                                            .w_full()
                                                            .gap_3()
                                                            .child(
                                                                h_flex()
                                                                    .gap_2()
                                                                    .child(
                                                                        Button::new("sync-backend-webdav")
                                                                            .small()
                                                                            .label("WebDAV")
                                                                            .when(!is_s3, |button| button.primary())
                                                                            .on_click(window.listener_for(&view, |this, _, _, cx| this.set_sync_backend("webdav", cx)))
                                                                    )
                                                                    .child(
                                                                        Button::new("sync-backend-s3")
                                                                            .small()
                                                                            .label("S3")
                                                                            .when(is_s3, |button| button.primary())
                                                                            .on_click(window.listener_for(&view, |this, _, _, cx| this.set_sync_backend("s3", cx)))
                                                                    )
                                                            )
                                                            .when(!is_s3, |this| this
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_endpoint").to_string())).child(Input::new(&endpoint).w_full()))
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_username").to_string())).child(Input::new(&username).w_full()))
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_webdav_password").to_string())).child(Input::new(&webdav_password).w_full())))
                                                            .when(is_s3, |this| this
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_s3_endpoint").to_string())).child(Input::new(&s3_endpoint).w_full()))
                                                                .child(h_flex().gap_2()
                                                                    .child(v_flex().flex_1().gap_1().child(div().text_sm().child(t!("sync_s3_region").to_string())).child(Input::new(&s3_region).w_full()))
                                                                    .child(v_flex().flex_1().gap_1().child(div().text_sm().child(t!("sync_s3_bucket").to_string())).child(Input::new(&s3_bucket).w_full())))
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_s3_object_key").to_string())).child(Input::new(&s3_object_key).w_full()))
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_s3_access_key").to_string())).child(Input::new(&s3_access_key).w_full()))
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_s3_secret_key").to_string())).child(Input::new(&s3_secret_key).w_full()))
                                                                .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_s3_session_token").to_string())).child(Input::new(&s3_session_token).w_full())))
                                                            .child(v_flex().gap_1().child(div().text_sm().child(t!("sync_encryption_password").to_string())).child(Input::new(&encryption_password).w_full()))
                                                            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(t!("sync_security_hint").to_string()))
                                                            .child(
                                                                h_flex()
                                                                    .gap_2()
                                                                    .child(Button::new("sync-download").small().disabled(in_progress).label(t!("sync_download").to_string()).on_click(window.listener_for(&view, |this, _, _, cx| this.download_sync_config(cx))))
                                                                    .child(Button::new("sync-upload").small().disabled(in_progress).label(t!("sync_upload").to_string()).on_click(window.listener_for(&view, |this, _, _, cx| this.upload_sync_config(cx)))),
                                                            )
                                                            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(status))
                                                    }
                                                }))
                                        )
                                )
                                .page(
                                    SettingPage::new(t!("settings_proxy").to_string())
                                        .icon(IconName::Network)
                                        .group(
                                            SettingGroup::new()
                                                .title(t!("settings_proxy").to_string())
                                                .item(
                                                    SettingItem::new(
                                                        t!("enable_proxy").to_string(),
                                                        SettingField::render({
                                                            let view = view.clone();
                                                            move |_, window, cx| {
                                                                Switch::new("use-proxy")
                                                                    .small()
                                                                    .checked(view.read(cx).config.use_proxy())
                                                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                                                        this.config.set_use_proxy(*checked);
                                                                        this.mark_config_preferences_dirty();
                                                                        cx.notify();
                                                                    }))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    )
                                                )
                                                .item(
                                                    SettingItem::new(
                                                        t!("read_env_proxy").to_string(),
                                                        SettingField::render({
                                                            let view = view.clone();
                                                            move |_, window, cx| {
                                                                Switch::new("read-env-proxy")
                                                                    .small()
                                                                    .checked(view.read(cx).config.read_env_proxy())
                                                                    .on_click(window.listener_for(&view, |this, checked, _, cx| {
                                                                        this.config.set_read_env_proxy(*checked);
                                                                        this.mark_config_preferences_dirty();
                                                                        cx.notify();
                                                                    }))
                                                                    .into_any_element()
                                                            }
                                                        })
                                                    ).description(t!("read_env_proxy_desc").to_string())
                                                )
                                                .item(SettingItem::render({
                                                    let view = view.clone();
                                                    let global_proxy_host_input = self.global_proxy_host_input.clone();
                                                    let global_proxy_port_input = self.global_proxy_port_input.clone();
                                                    let global_proxy_user_input = self.global_proxy_user_input.clone();
                                                    let global_proxy_password_input = self.global_proxy_password_input.clone();
                                                    move |_, window, cx| {
                                                        let proxy_type = view.read(cx).global_proxy_type.clone();
                                                        v_flex()
                                                            .w_full()
                                                            .gap_3()
                                                            .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("global_proxy_settings").to_string()))
                                                            .child(
                                                                h_flex()
                                                                    .gap_2()
                                                                    .child(
                                                                        Button::new("global-proxy-type-socks5")
                                                                            .small()
                                                                            .label("SOCKS5")
                                                                            .when(proxy_type == "socks5", |b| b.primary())
                                                                            .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                                this.global_proxy_type = "socks5".to_string();
                                                                                cx.notify();
                                                                            }))
                                                                    )
                                                                    .child(
                                                                        Button::new("global-proxy-type-http")
                                                                            .small()
                                                                            .label("HTTP")
                                                                            .when(proxy_type == "http", |b| b.primary())
                                                                            .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                                this.global_proxy_type = "http".to_string();
                                                                                cx.notify();
                                                                            }))
                                                                    )
                                                            )
                                                            .child(v_flex().gap_1().child(div().text_sm().child(t!("global_proxy_host").to_string())).child(Input::new(&global_proxy_host_input).w_full()))
                                                            .child(v_flex().gap_1().child(div().text_sm().child(t!("global_proxy_port").to_string())).child(Input::new(&global_proxy_port_input).w_full()))
                                                            .child(v_flex().gap_1().child(div().text_sm().child(t!("global_proxy_user").to_string())).child(Input::new(&global_proxy_user_input).w_full()))
                                                            .child(v_flex().gap_1().child(div().text_sm().child(t!("global_proxy_password").to_string())).child(Input::new(&global_proxy_password_input).w_full()))
                                                            .child(
                                                                Button::new("save-global-proxy")
                                                                    .small()
                                                                    .primary()
                                                                    .label(t!("save_proxy").to_string())
                                                                    .on_click(window.listener_for(&view, |this, _, _, cx| {
                                                                        let host = this.global_proxy_host_input.read(cx).value().trim().to_string();
                                                                        let port_str = this.global_proxy_port_input.read(cx).value();
                                                                        let port = port_str.trim().parse::<u16>().ok();
                                                                        let user = this.global_proxy_user_input.read(cx).value().trim().to_string();
                                                                        let password = this.global_proxy_password_input.read(cx).value().to_string();

                                                                        if host.is_empty() || port.is_none() {
                                                                            return;
                                                                        }

                                                                        this.config.set_global_proxy_type(this.global_proxy_type.clone());
                                                                        this.config.set_global_proxy_host(host);
                                                                        this.config.set_global_proxy_port(port);
                                                                        this.config.set_global_proxy_user(user);
                                                                        this.config.set_global_proxy_password(password);
                                                                        this.mark_config_preferences_dirty();
                                                                        cx.notify();
                                                                    }))
                                                            )
                                                    }
                                                }))
                                        )
                                )
                                .page({
                                    let mut page = SettingPage::new(t!("settings_key_bindings").to_string())
                                        .icon(IconName::SquareTerminal)
                                        .default_open(true);
                                    for group in crate::app::keybinding_recorder::KeybindingsPage::render_groups(self, view) {
                                        page = page.group(group);
                                    }
                                    page
                                })
                                .page(
                                    SettingPage::new(t!("settings_help").to_string())
                                        .icon(IconName::BookOpen)
                                )
                                .page(
                                    SettingPage::new(t!("settings_about").to_string())
                                        .icon(IconName::Info)
                                        .group(
                                            SettingGroup::new()
                                                .item(SettingItem::render({
                                                    let view = view.clone();
                                                    move |_, _window, cx| {
                                                        let status_text = {
                                                            let state = view.read(cx);
                                                            match &state.updater_status {
                                                                Some(crate::app::updater::UpdateStatus::Checking) => {
                                                                    Some(t!("checking_update").to_string())
                                                                }
                                                                Some(crate::app::updater::UpdateStatus::UpToDate(_)) => {
                                                                    Some(t!("update_latest").to_string())
                                                                }
                                                                Some(crate::app::updater::UpdateStatus::UpdateAvailable(info)) => {
                                                                    Some(t!("update_available", version = info.version.clone()).to_string())
                                                                }
                                                                Some(crate::app::updater::UpdateStatus::Downloading(_, _, _)) => {
                                                                    Some(t!("update_downloading").to_string())
                                                                }
                                                                Some(crate::app::updater::UpdateStatus::ReadyToRestart(_, _)) => {
                                                                    Some(t!("update_install_complete").to_string())
                                                                }
                                                                Some(crate::app::updater::UpdateStatus::Error(msg)) => {
                                                                    Some(t!("update_error", error = msg.clone()).to_string())
                                                                }
                                                                None => None,
                                                            }
                                                        };
                                                        let has_update = matches!(
                                                            view.read(cx).updater_status,
                                                            Some(crate::app::updater::UpdateStatus::UpdateAvailable(_))
                                                        );

                                                        v_flex()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(div().text_size(rems(1.5)).font_weight(FontWeight::BOLD).child(t!("app_name")))
                                                            .child(div().text_size(rems(0.9)).child(format!("Version {}", version)))
                                                            .child(
                                                                div()
                                                                    .text_size(rems(0.9))
                                                                    .text_color(cx.theme().muted_foreground)
                                                                    .child("A GPUI Component based SSH and local terminal client"),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(rems(0.9))
                                                                    .text_color(cx.theme().muted_foreground)
                                                                    .child(t!("about_feedback_hint")),
                                                            )
                                                            .child(
                                                                Button::new("github-link")
                                                                    .label("https://github.com/ynx-official/tiny-shell")
                                                                    .ghost()
                                                                    .on_click(|_, _window, _cx| {
                                                                        let _ = open::that("https://github.com/ynx-official/tiny-shell");
                                                                    }),
                                                            )
                                                            .child(
                                                                h_flex()
                                                                    .gap_2()
                                                                    .items_center()
                                                                    .child(
                                        Button::new("check-update")
                                            .label(t!("check_update").to_string())
                                            .on_click({
                                                let view = view.clone();
                                                move |_, _window, cx| {
                                                    view.update(cx, |this, cx| this.check_for_updates(cx));
                                                }
                                            }),
                                    )
                                                                    .when(has_update, |this| {
                                                                        this.child(
                                                                            Button::new("download-update")
                                                                                .primary()
                                                                                .label(t!("update_download").to_string())
                                                                                .on_click({
                                                                                    let view = view.clone();
                                                                                    move |_, _window, cx| {
                                                                                        view.update(cx, |this, cx| this.download_available_update(cx));
                                                                                    }
                                                                                }),
                                                                        )
                                                                    })
                                                                    .when_some(status_text, |this, text| {
                                                                        this.child(
                                                                            div()
                                                                                .text_size(rems(0.85))
                                                                                .text_color(cx.theme().muted_foreground)
                                                                                .child(text),
                                                                        )
                                                                    }),
                                                            )
                                                    }
                                                }))
                                        )
                                )
                                )
                        )
        .into_any_element()
    }

    pub(crate) fn show_settings_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_dialog.is_some() {
            return;
        }
        self.active_dialog = Some(crate::app::DialogKind::Settings);

        let view = cx.entity();

        // Unbind all workspace keys so they don't interfere with keybinding recording
        crate::app::keybinding_recorder::unbind_all_workspace_keys(cx, &self.config);
        self.keybinds_suspended = true;

        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("settings").to_string())
                .w(px(840.))
                .h(px(560.))
                .on_close({
                    let view = view.clone();
                    move |_, _window, cx| {
                        // Re-register all workspace keys when closing settings
                        view.update(cx, |this, cx| {
                            this.active_dialog = None;
                            this.keybinds_suspended = false;
                            this.recording_action = None;
                            this.keybind_error = None;
                            this.persist_config_preferences_async();
                            crate::app::keybinding_recorder::bind_workspace_keys_from_config(
                                cx,
                                &this.config,
                            );
                            cx.notify();
                        });
                    }
                })
                .content({
                    let view = view.clone();
                    move |content, _window, cx| {
                        let settings = view.update(cx, |this, cx| {
                            this.render_settings_content(&view, "settings-dialog", cx)
                        });
                        content.child(settings)
                    }
                })
        });
    }
}
