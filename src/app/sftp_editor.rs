//! SFTP 内置编辑器(多标签页)。
//!
//! 双击 txt/sh/yaml/json 等文本文件时,下载内容到内存并用 gpui-component 的
//! CodeEditor 模式打开(自带行号 + Tree Sitter 语法高亮)。
//! 多个文件以 tab 形式合并到同一个编辑器窗口,可互相切换。
//! Ctrl+S 保存当前 tab,Ctrl+W 关闭当前 tab,Esc 关闭整个编辑器。
//! 保存后自动上传覆盖远程文件,无需落地临时文件。

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext, Entity, Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, Styled, Window, px,
};
use gpui_component::{
    ActiveTheme, Disableable as _, Root, Sizable, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use rust_i18n::t;

use crate::sftp::SftpHandle;

/// 文件扩展名 → Tree Sitter 语言名映射。
/// 返回 None 时回退到纯多行模式(无高亮)。
fn language_for_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    match ext.as_str() {
        "sh" | "bash" | "zsh" => Some("bash"),
        "py" => Some("python"),
        "rs" => Some("rust"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "ts" => Some("typescript"),
        "json" => Some("json"),
        "yml" | "yaml" => Some("yaml"),
        "md" => Some("markdown"),
        "toml" => Some("toml"),
        "xml" | "html" | "htm" => Some("html"),
        "css" | "scss" => Some("css"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" => Some("cpp"),
        "go" => Some("go"),
        "java" => Some("java"),
        "conf" | "cfg" | "ini" => Some("ini"),
        "sql" => Some("sql"),
        "lua" => Some("lua"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "kt" => Some("kotlin"),
        "swift" => Some("swift"),
        _ => None,
    }
}

/// 从路径提取文件名(最后一段)。
fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 单个文件对应的编辑器状态。
struct EditorTab {
    remote_path: String,
    input: Entity<InputState>,
    /// 有未保存的修改。
    dirty: bool,
    /// 正在上传中。
    saving: bool,
    /// 保存发起时的内容，用于判断上传期间是否又发生了编辑。
    text_at_save_start: Option<String>,
}

impl EditorTab {
    fn new(
        remote_path: String,
        content: String,
        window: &mut Window,
        cx: &mut gpui::Context<SftpEditor>,
    ) -> Self {
        let lang = language_for_path(&remote_path).unwrap_or("text");
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(lang)
                .rows(30)
                .default_value(content)
        });

        // 订阅该 tab 的内容变化 → 标记对应 tab dirty
        cx.subscribe(&input, |this, emitter, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                // 通过 emitter 找到是哪个 tab 触发的
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.input == emitter) {
                    if !tab.dirty {
                        tab.dirty = true;
                        cx.notify();
                    }
                }
            }
        })
        .detach();

        Self {
            remote_path,
            input,
            dirty: false,
            saving: false,
            text_at_save_start: None,
        }
    }
}

pub struct SftpEditor {
    session_id: String,
    sftp: SftpHandle,
    tabs: Vec<EditorTab>,
    /// 当前激活的 tab 索引。
    active_idx: usize,
    /// 已确认关闭，允许系统窗口关闭回调继续执行。
    force_close: bool,
    /// 所属 SSH/SFTP 连接仍可用于保存。
    connected: bool,
    /// tab 数量达到上限时的提示文本(空字符串表示无提示)。
    pub capacity_notice: String,
}

/// 单个编辑器窗口最多同时打开的 tab 数量,防止内存爆炸。
const MAX_TABS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseDisposition {
    Close,
    Confirm,
}

fn close_disposition(has_dirty_tabs: bool) -> CloseDisposition {
    if has_dirty_tabs {
        CloseDisposition::Confirm
    } else {
        CloseDisposition::Close
    }
}

impl SftpEditor {
    /// 创建编辑器并打开第一个文件。
    pub fn new(
        session_id: String,
        remote_path: String,
        content: String,
        sftp: SftpHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let tab = EditorTab::new(remote_path, content, window, cx);
        Self {
            session_id,
            sftp,
            tabs: vec![tab],
            active_idx: 0,
            force_close: false,
            connected: true,
            capacity_notice: String::new(),
        }
    }

    /// 打开一个文件到新 tab。若该路径已存在,则切换到对应 tab 不重复打开。
    /// 达到 MAX_TABS 上限时不打开,而是设置 capacity_notice 提示。
    /// 返回该 tab 的索引(已存在或新建);达到上限返回 None。
    pub fn open_file(
        &mut self,
        remote_path: String,
        content: String,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<usize> {
        // 已存在则切换
        if let Some(idx) = self.tabs.iter().position(|t| t.remote_path == remote_path) {
            self.active_idx = idx;
            self.focus_active(window, cx);
            cx.notify();
            return Some(idx);
        }
        // 达到上限 → 提示,不打开
        if self.tabs.len() >= MAX_TABS {
            self.capacity_notice = t!("editor_capacity_reached", max = MAX_TABS).to_string();
            cx.notify();
            return None;
        }
        let tab = EditorTab::new(remote_path, content, window, cx);
        self.tabs.push(tab);
        self.active_idx = self.tabs.len() - 1;
        self.capacity_notice.clear();
        self.focus_active(window, cx);
        cx.notify();
        Some(self.active_idx)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 切换到指定路径的 tab,成功返回 true。
    pub fn focus_path(&mut self, path: &str, cx: &mut gpui::Context<Self>) -> bool {
        if let Some(idx) = self.tabs.iter().position(|t| t.remote_path == path) {
            if self.active_idx != idx {
                self.active_idx = idx;
                cx.notify();
            }
            true
        } else {
            false
        }
    }

    /// 当前激活 tab。
    fn active_tab(&self) -> Option<&EditorTab> {
        self.tabs.get(self.active_idx)
    }

    /// 将窗口键盘焦点交给当前文件的编辑输入框。
    pub fn focus_active(&self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if let Some(tab) = self.active_tab() {
            window.focus(&tab.input.read(cx).focus_handle(cx), cx);
        }
    }

    /// Ctrl+S:保存当前激活 tab,读取其内容并上传。
    /// 注意:此处不清 dirty,等上传成功事件回来再清(mark_uploaded)。
    /// 失败时由 mark_upload_failed 恢复 dirty。避免上传失败却误标已保存。
    pub fn save_active(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) {
        if !self.connected {
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active_idx) else {
            return;
        };
        if tab.saving {
            return;
        }
        let content = tab.input.read(cx).text().to_string();
        let path = tab.remote_path.clone();
        tab.saving = true;
        tab.text_at_save_start = Some(content.clone());
        self.sftp.upload_file_content(path, content);
        cx.notify();
    }

    /// 收到上传完成事件后,按 remote_path 标记对应 tab 已上传(成功才清 dirty)。
    pub fn mark_uploaded(&mut self, remote_path: &str, cx: &mut gpui::Context<Self>) {
        for tab in &mut self.tabs {
            if tab.remote_path == remote_path {
                let uploaded_text = tab.text_at_save_start.take();
                let current_text = tab.input.read(cx).text().to_string();
                tab.saving = false;
                tab.dirty = uploaded_text.as_deref() != Some(current_text.as_str());
            }
        }
        cx.notify();
    }

    /// 收到上传失败事件后,按 remote_path 恢复 tab 状态:
    /// saving=false,dirty 恢复为保存发起时的值(内容其实未保存)。
    pub fn mark_upload_failed(&mut self, remote_path: &str, cx: &mut gpui::Context<Self>) {
        for tab in &mut self.tabs {
            if tab.remote_path == remote_path {
                tab.saving = false;
                tab.text_at_save_start = None;
                tab.dirty = true;
            }
        }
        cx.notify();
    }

    fn close_active(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.tabs.is_empty() {
            self.close_window(window, cx);
            return;
        }
        self.request_close_tab(self.active_idx, window, cx);
    }

    fn close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if idx < self.tabs.len() {
            self.request_close_tab(idx, window, cx);
        }
    }

    fn request_close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        if !tab.dirty {
            self.do_close_tab(idx, window, cx);
            return;
        }

        let filename = base_name(&tab.remote_path).to_string();
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            let filename = filename.clone();
            dialog
                .title(t!("editor_close_confirm_title").to_string())
                .w(px(440.))
                .keyboard(false)
                .on_ok({
                    let editor = editor.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        editor.update(cx, |editor, cx| {
                            editor.do_close_tab(idx, window, cx);
                        });
                        true
                    }
                })
                .content(move |content, _window, _cx| {
                    content.child(gpui::div().p_4().text_sm().child(
                        t!("editor_close_confirm_desc", name = filename.as_str()).to_string(),
                    ))
                })
        });
    }

    fn do_close_tab(&mut self, idx: usize, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.close_window(window, cx);
            return;
        }
        if self.active_idx >= self.tabs.len() {
            self.active_idx = self.tabs.len() - 1;
        } else if idx < self.active_idx {
            self.active_idx -= 1;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn close_window(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.force_close = true;
        cx.notify();
        window.remove_window();
    }

    fn open_close_all_dialog(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            dialog
                .title(t!("editor_close_all_confirm_title").to_string())
                .w(px(440.))
                .keyboard(false)
                .on_ok({
                    let editor = editor.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        editor.update(cx, |editor, cx| {
                            editor.close_window(window, cx);
                        });
                        true
                    }
                })
                .content(move |content, _window, _cx| {
                    content.child(
                        gpui::div()
                            .p_4()
                            .text_sm()
                            .child(t!("editor_close_all_confirm_desc").to_string()),
                    )
                })
        });
    }

    fn close_all(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.open_close_all_dialog(window, cx);
        } else {
            self.close_window(window, cx);
        }
    }

    pub fn notify_connection_lost(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        if !self.connected {
            return;
        }
        self.connected = false;
        for tab in &mut self.tabs {
            tab.saving = false;
        }
        cx.notify();

        if self.tabs.iter().any(|tab| tab.dirty) {
            self.open_close_all_dialog(window, cx);
        } else {
            self.close_window(window, cx);
        }
    }

    pub fn request_session_close(
        &mut self,
        tab_id: String,
        owner: Entity<crate::Ashell>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self.tabs.iter().any(|tab| tab.dirty) {
            self.close_window(window, cx);
            return true;
        }

        let editor = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, _window, _| {
            let owner = owner.clone();
            let tab_id = tab_id.clone();
            dialog
                .title(t!("editor_close_all_confirm_title").to_string())
                .w(px(440.))
                .keyboard(false)
                .on_ok({
                    let editor = editor.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        editor.update(cx, |editor, cx| editor.close_window(window, cx));
                        owner.update(cx, |owner, cx| {
                            owner.handle_tab_close(tab_id.clone());
                            cx.notify();
                        });
                        true
                    }
                })
                .content(move |content, _window, _cx| {
                    content.child(
                        gpui::div()
                            .p_4()
                            .text_sm()
                            .child(t!("editor_close_all_confirm_desc").to_string()),
                    )
                })
        });
        false
    }

    pub fn request_window_close(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.force_close
            || close_disposition(self.tabs.iter().any(|tab| tab.dirty)) == CloseDisposition::Close
        {
            return true;
        }
        self.open_close_all_dialog(window, cx);
        false
    }

    fn switch_tab(&mut self, idx: usize, cx: &mut gpui::Context<Self>) {
        if idx < self.tabs.len() && idx != self.active_idx {
            self.active_idx = idx;
            cx.notify();
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let ks = &event.keystroke;
        let key_lower = ks.key.to_ascii_lowercase();
        // Ctrl+S / Cmd+S 保存当前 tab
        if (ks.modifiers.control || ks.modifiers.platform) && key_lower == "s" {
            self.save_active(window, cx);
            cx.stop_propagation();
        }
        // Ctrl+W 关闭当前 tab
        if (ks.modifiers.control || ks.modifiers.platform) && key_lower == "w" {
            self.close_active(window, cx);
            cx.stop_propagation();
        }
        // Esc 关闭整个编辑器
        if key_lower == "escape" {
            self.close_all(window, cx);
            cx.stop_propagation();
        }
    }
}

impl Render for SftpEditor {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        let active = self.active_tab();
        let filename = active.map(|t| base_name(&t.remote_path)).unwrap_or("");
        let lang_label = active
            .and_then(|t| language_for_path(&t.remote_path))
            .unwrap_or("text");

        let status_text = if !self.connected {
            t!("editor_connection_lost").to_string()
        } else if let Some(t) = active {
            if t.saving {
                t!("editor_saving").to_string()
            } else if t.dirty {
                t!("editor_unsaved").to_string()
            } else {
                t!("editor_saved").to_string()
            }
        } else {
            String::new()
        };

        let dirty = active.map(|t| t.dirty).unwrap_or(false);
        let saving = active.map(|t| t.saving).unwrap_or(false);
        let connected = self.connected;
        let active_idx = self.active_idx;
        let tab_count = self.tabs.len();
        let capacity_notice = self.capacity_notice.clone();

        // tab 栏数据快照
        let tab_snapshots: Vec<(String, bool, bool)> = self
            .tabs
            .iter()
            .map(|t| (base_name(&t.remote_path).to_string(), t.dirty, t.saving))
            .collect();

        gpui::div()
            .size_full()
            .bg(theme.background)
            .on_key_down(cx.listener(Self::handle_key_down))
            .child(
                v_flex()
                    .size_full()
                    .bg(theme.background)
                    .overflow_hidden()
                    // tab 栏(多文件时显示)
                    .when(tab_count > 1, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .overflow_hidden()
                                .bg(theme.muted.opacity(0.3))
                                .border_b_1()
                                .border_color(theme.border.opacity(0.5))
                                .children(
                                    tab_snapshots
                                        .iter()
                                        .enumerate()
                                        .map(|(idx, (name, dirty, _saving))| {
                                            let is_active = idx == active_idx;
                                            h_flex()
                                                .id(("editor-tab", idx))
                                                .items_center()
                                                .gap_1()
                                                .px_3()
                                                .py_2()
                                                .min_w(px(120.))
                                                .max_w(px(220.))
                                                .cursor_pointer()
                                                .border_r_1()
                                                .border_color(theme.border.opacity(0.3))
                                                .bg(if is_active {
                                                    theme.background
                                                } else {
                                                    gpui::transparent_black()
                                                })
                                                .text_color(if is_active {
                                                    theme.foreground
                                                } else {
                                                    theme.muted
                                                })
                                                .text_sm()
                                                .when(*dirty, |this| {
                                                    this.child(
                                                        gpui::div()
                                                            .w(px(6.))
                                                            .h(px(6.))
                                                            .rounded_full()
                                                            .bg(theme.warning),
                                                    )
                                                })
                                                .child(
                                                    gpui::div()
                                                        .flex_1()
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .child(name.clone()),
                                                )
                                                .child(
                                                    gpui::div()
                                                        .id(("tab-close", idx))
                                                        .cursor_pointer()
                                                        .text_color(theme.muted)
                                                        .hover(|this| {
                                                            this.text_color(theme.foreground)
                                                        })
                                                        .child("×")
                                                        .on_mouse_down(
                                                            gpui::MouseButton::Left,
                                                            cx.listener(
                                                                move |this, _ev, window, cx| {
                                                                    this.close_tab(idx, window, cx);
                                                                },
                                                            ),
                                                        ),
                                                )
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(move |this, _ev, _window, cx| {
                                                        this.switch_tab(idx, cx);
                                                    }),
                                                )
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        )
                    })
                    // 标题栏
                    .child(
                        h_flex()
                            .w_full()
                            .h(px(44.))
                            .items_center()
                            .px_4()
                            .gap_3()
                            .bg(theme.muted.opacity(0.5))
                            .border_b_1()
                            .border_color(theme.border.opacity(0.5))
                            .child(
                                gpui::div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(filename.to_string()),
                            )
                            .child(
                                gpui::div()
                                    .text_xs()
                                    .text_color(theme.muted)
                                    .child(format!("({})", lang_label)),
                            )
                            .child(gpui::div().flex_1())
                            .child(
                                gpui::div()
                                    .text_xs()
                                    .text_color(if !connected || dirty {
                                        theme.warning
                                    } else if saving {
                                        theme.muted
                                    } else {
                                        theme.success
                                    })
                                    .child(status_text),
                            )
                            .when(!capacity_notice.is_empty(), |this| {
                                this.child(
                                    gpui::div()
                                        .text_xs()
                                        .text_color(theme.warning)
                                        .child(capacity_notice),
                                )
                            })
                            .child(
                                Button::new("save-btn")
                                    .primary()
                                    .small()
                                    .disabled(saving || !connected)
                                    .label(t!("editor_save"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_active(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("close-btn")
                                    .small()
                                    .label(t!("editor_close"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_all(window, cx);
                                    })),
                            ),
                    )
                    // 编辑器区域(渲染当前激活 tab 的 input)
                    .child(
                        gpui::div()
                            .flex_1()
                            .min_h_0()
                            .when_some(self.active_tab(), |this, t| {
                                this.child(Input::new(&t.input).h_full())
                            }),
                    ),
            )
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::{CloseDisposition, close_disposition};

    #[test]
    fn clean_editor_closes_without_confirmation() {
        assert_eq!(close_disposition(false), CloseDisposition::Close);
    }

    #[test]
    fn dirty_editor_requires_confirmation() {
        assert_eq!(close_disposition(true), CloseDisposition::Confirm);
    }
}
