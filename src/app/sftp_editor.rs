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
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    Styled, Window, px, relative,
};
use gpui_component::{
    ActiveTheme, Disableable as _, Root, Sizable, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
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
#[derive(Clone)]
pub(crate) struct EditorTab {
    remote_path: String,
    input: Entity<InputState>,
    /// 有未保存的修改。
    dirty: bool,
    /// 正在上传中。
    saving: bool,
    /// 保存发起时的内容，用于判断上传期间是否又发生了编辑。
    text_at_save_start: Option<String>,
}

fn subscribe_input_changes(input: &Entity<InputState>, cx: &mut gpui::Context<SftpEditor>) {
    cx.subscribe(input, |this, emitter, event: &InputEvent, cx| {
        if let InputEvent::Change = event {
            if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.input == emitter) {
                if !tab.dirty {
                    tab.dirty = true;
                    cx.notify();
                }
            }
        }
    })
    .detach();
}

impl EditorTab {
    pub(crate) fn remote_path(&self) -> &str {
        &self.remote_path
    }

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

        subscribe_input_changes(&input, cx);

        Self {
            remote_path,
            input,
            dirty: false,
            saving: false,
            text_at_save_start: None,
        }
    }
}

#[derive(Default)]
struct EditorTabDrag {
    pending_idx: Option<usize>,
    start: Option<Point<Pixels>>,
    dragging_idx: Option<usize>,
    detach_target: bool,
}

impl EditorTabDrag {
    fn begin(&mut self, idx: usize, position: Point<Pixels>) {
        self.pending_idx = Some(idx);
        self.start = Some(position);
        self.dragging_idx = None;
        self.detach_target = false;
    }

    fn update(
        &mut self,
        position: Point<Pixels>,
        viewport: gpui::Size<Pixels>,
        tab_count: usize,
    ) -> bool {
        let mut changed = false;
        if self.dragging_idx.is_none() {
            let (Some(start), Some(idx)) = (self.start, self.pending_idx) else {
                return false;
            };
            let dx: f32 = (position.x - start.x).into();
            let dy: f32 = (position.y - start.y).into();
            if (dx * dx + dy * dy).sqrt() > 5.0 {
                self.dragging_idx = Some(idx);
                self.pending_idx = None;
                changed = true;
            }
        }

        if self.dragging_idx.is_some() {
            let detach_target = tab_count > 1
                && (position.x < px(0.)
                    || position.y < px(0.)
                    || position.x >= viewport.width
                    || position.y >= viewport.height
                    || position.y > px(40.));
            if self.detach_target != detach_target {
                self.detach_target = detach_target;
                changed = true;
            }
        }
        changed
    }

    fn finish(&mut self) -> Option<usize> {
        let result = if self.detach_target {
            self.dragging_idx
        } else {
            None
        };
        self.cancel();
        result
    }

    fn cancel(&mut self) {
        self.pending_idx = None;
        self.start = None;
        self.dragging_idx = None;
        self.detach_target = false;
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
    tab_drag: EditorTabDrag,
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
            tab_drag: EditorTabDrag::default(),
        }
    }

    pub(crate) fn from_detached(
        session_id: String,
        tab: EditorTab,
        sftp: SftpHandle,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        subscribe_input_changes(&tab.input, cx);
        Self {
            session_id,
            sftp,
            tabs: vec![tab],
            active_idx: 0,
            force_close: false,
            connected: true,
            capacity_notice: String::new(),
            tab_drag: EditorTabDrag::default(),
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

    pub(crate) fn has_dirty_tabs(&self) -> bool {
        self.tabs.iter().any(|tab| tab.dirty)
    }

    pub(crate) fn force_close_window(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.close_window(window, cx);
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
        window.open_alert_dialog(cx, move |dialog, _window, _| {
            let filename = filename.clone();
            dialog
                .title(t!("editor_close_confirm_title").to_string())
                .description(t!("editor_close_confirm_desc", name = filename.as_str()).to_string())
                .width(px(440.))
                .keyboard(false)
                .button_props(
                    DialogButtonProps::default()
                        .cancel_text(t!("editor_close_cancel").to_string())
                        .show_cancel(true)
                        .ok_text(t!("editor_close_discard").to_string())
                        .ok_variant(ButtonVariant::Danger),
                )
                .on_ok({
                    let editor = editor.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        editor.update(cx, |editor, cx| {
                            editor.do_close_tab(idx, window, cx);
                        });
                        false
                    }
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
        window.open_alert_dialog(cx, move |dialog, _window, _| {
            dialog
                .title(t!("editor_close_all_confirm_title").to_string())
                .description(t!("editor_close_all_confirm_desc").to_string())
                .width(px(440.))
                .keyboard(false)
                .button_props(
                    DialogButtonProps::default()
                        .cancel_text(t!("editor_close_cancel").to_string())
                        .show_cancel(true)
                        .ok_text(t!("editor_close_discard").to_string())
                        .ok_variant(ButtonVariant::Danger),
                )
                .on_ok({
                    let editor = editor.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        editor.update(cx, |editor, cx| {
                            editor.close_window(window, cx);
                        });
                        false
                    }
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

        let session_id = self.session_id.clone();
        window.open_alert_dialog(cx, move |dialog, _window, _| {
            let owner = owner.clone();
            let tab_id = tab_id.clone();
            let session_id = session_id.clone();
            dialog
                .title(t!("editor_close_all_confirm_title").to_string())
                .description(t!("editor_close_all_confirm_desc").to_string())
                .width(px(440.))
                .keyboard(false)
                .button_props(
                    DialogButtonProps::default()
                        .cancel_text(t!("editor_close_cancel").to_string())
                        .show_cancel(true)
                        .ok_text(t!("editor_close_discard").to_string())
                        .ok_variant(ButtonVariant::Danger),
                )
                .on_ok(move |_, window, cx| {
                    window.close_dialog(cx);
                    let session_id = session_id.clone();
                    let owner = owner.clone();
                    let tab_id = tab_id.clone();
                    window.defer(cx, move |_window, cx| {
                        crate::app::sftp_editor_window::force_close_session_windows(
                            &session_id,
                            cx,
                        );
                        owner.update(cx, |owner, cx| {
                            owner.handle_tab_close(tab_id);
                            cx.notify();
                        });
                    });
                    false
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

    fn begin_tab_drag(&mut self, idx: usize, event: &MouseDownEvent, cx: &mut gpui::Context<Self>) {
        self.switch_tab(idx, cx);
        self.tab_drag.begin(idx, event.position);
    }

    fn update_tab_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self
            .tab_drag
            .update(event.position, window.viewport_size(), self.tabs.len())
        {
            cx.notify();
        }
    }

    fn finish_tab_drag(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(idx) = self.tab_drag.finish() else {
            cx.notify();
            return;
        };
        if idx >= self.tabs.len() || self.tabs.len() <= 1 {
            cx.notify();
            return;
        }

        let tab = self.tabs[idx].clone();
        let origin = match window.window_bounds() {
            gpui::WindowBounds::Fullscreen(bounds)
            | gpui::WindowBounds::Maximized(bounds)
            | gpui::WindowBounds::Windowed(bounds) => bounds.origin,
        };
        let screen_position = Point::new(origin.x + event.position.x, origin.y + event.position.y);

        if crate::app::sftp_editor_window::open_detached(
            self.session_id.clone(),
            tab,
            self.sftp.clone(),
            screen_position,
            cx,
        ) {
            self.tabs.remove(idx);
            if self.active_idx >= self.tabs.len() {
                self.active_idx = self.tabs.len() - 1;
            } else if idx < self.active_idx {
                self.active_idx -= 1;
            }
            self.focus_active(window, cx);
        }
        cx.notify();
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
        cx.notify();
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
        let cursor_position = active
            .map(|tab| tab.input.read(cx).cursor_position())
            .map(|position| (position.line + 1, position.character + 1))
            .unwrap_or((1, 1));
        let line_ending = active
            .map(|tab| tab.input.read(cx).text().to_string())
            .map(|text| if text.contains("\r\n") { "CRLF" } else { "LF" })
            .unwrap_or("LF");

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
        let dragging_idx = self.tab_drag.dragging_idx;
        let show_detach_hint = self.tab_drag.detach_target;

        // tab 栏数据快照
        let tab_snapshots: Vec<(String, bool, bool)> = self
            .tabs
            .iter()
            .map(|t| (base_name(&t.remote_path).to_string(), t.dirty, t.saving))
            .collect();

        gpui::div()
            .relative()
            .size_full()
            .bg(theme.background)
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_move(cx.listener(Self::update_tab_drag))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::finish_tab_drag))
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
                                                .relative()
                                                .when(dragging_idx == Some(idx), |this| {
                                                    this.opacity(0.62)
                                                })
                                                .border_r_1()
                                                .border_color(theme.border.opacity(0.3))
                                                .bg(if is_active {
                                                    theme.secondary
                                                } else {
                                                    theme.muted.opacity(0.28)
                                                })
                                                .text_color(if is_active {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                })
                                                .hover(|this| {
                                                    this.bg(theme.secondary.opacity(0.72))
                                                        .text_color(theme.foreground)
                                                })
                                                .text_sm()
                                                .when(is_active, |this| {
                                                    this.child(
                                                        gpui::div()
                                                            .absolute()
                                                            .bottom_0()
                                                            .left_0()
                                                            .right_0()
                                                            .h(px(2.))
                                                            .bg(theme.primary),
                                                    )
                                                })
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
                                                                    cx.stop_propagation();
                                                                    this.close_tab(idx, window, cx);
                                                                },
                                                            ),
                                                        ),
                                                )
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    cx.listener(
                                                        move |this, event: &MouseDownEvent, _window, cx| {
                                                            this.begin_tab_drag(idx, event, cx);
                                                        },
                                                    ),
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
                            .child(gpui::div().flex_1())
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
                            .bg(theme.background)
                            .when_some(self.active_tab(), |this, tab| {
                                this.child(
                                    Input::new(&tab.input)
                                        .h_full()
                                        .bordered(false)
                                        .focus_bordered(false)
                                        .font_family("Maple Mono NF CN")
                                        .text_size(px(13.))
                                        .line_height(relative(1.5))
                                        .px_1()
                                        .py_1(),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .h(px(26.))
                            .items_center()
                            .gap_3()
                            .px_3()
                            .border_t_1()
                            .border_color(theme.border.opacity(0.45))
                            .bg(theme.muted.opacity(0.32))
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(lang_label.to_uppercase())
                            .child(t!(
                                "editor_cursor_position",
                                line = cursor_position.0,
                                column = cursor_position.1
                            ).to_string())
                            .child("UTF-8")
                            .child(line_ending)
                            .child(gpui::div().flex_1())
                            .child(
                                gpui::div()
                                    .text_color(if !connected || dirty {
                                        theme.warning
                                    } else if saving {
                                        theme.muted_foreground
                                    } else {
                                        theme.success
                                    })
                                    .child(status_text),
                            ),
                    ),
            )
            .when(show_detach_hint, |this| {
                this.child(
                    gpui::div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_8()
                        .flex()
                        .justify_center()
                        .child(
                            gpui::div()
                                .rounded_lg()
                                .border_1()
                                .border_color(theme.primary.opacity(0.5))
                                .bg(theme.background.opacity(0.94))
                                .px_4()
                                .py_2()
                                .text_sm()
                                .text_color(theme.foreground)
                                .child(t!("editor_drag_detach_hint").to_string()),
                        ),
                )
            })
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size};

    use super::{CloseDisposition, EditorTabDrag, close_disposition};

    #[test]
    fn clean_editor_closes_without_confirmation() {
        assert_eq!(close_disposition(false), CloseDisposition::Close);
    }

    #[test]
    fn dirty_editor_requires_confirmation() {
        assert_eq!(close_disposition(true), CloseDisposition::Confirm);
    }

    #[test]
    fn tab_drag_requires_threshold_and_detach_area() {
        let mut drag = EditorTabDrag::default();
        drag.begin(1, point(px(20.), px(20.)));

        assert!(!drag.update(point(px(23.), px(23.)), size(px(800.), px(600.)), 2));
        assert!(drag.update(point(px(30.), px(22.)), size(px(800.), px(600.)), 2));
        assert_eq!(drag.finish(), None);
    }

    #[test]
    fn tab_drag_detaches_only_when_another_tab_remains() {
        let mut drag = EditorTabDrag::default();
        drag.begin(1, point(px(20.), px(20.)));
        drag.update(point(px(40.), px(100.)), size(px(800.), px(600.)), 2);
        assert_eq!(drag.finish(), Some(1));

        drag.begin(0, point(px(20.), px(20.)));
        drag.update(point(px(40.), px(100.)), size(px(800.), px(600.)), 1);
        assert_eq!(drag.finish(), None);
    }
}
