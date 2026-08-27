use std::ops::Range;

use alacritty_terminal::index::Side;
use alacritty_terminal::selection::SelectionType;
use gpui::{
    ClipboardItem, Context, Focusable as _, KeyDownEvent, KeyUpEvent, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, Window, px,
};

use crate::{
    TerminalBacktabKey, TerminalTabKey, TinyShell,
    terminal::{BackendCommand, RemoteDesktopInput, TabKind, encode_key},
};

thread_local! {
    static LAST_DRAG_SCROLL: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

fn rdp_mouse_button(button: MouseButton) -> Option<u16> {
    match button {
        MouseButton::Left => Some(0x1000),
        MouseButton::Right => Some(0x2000),
        MouseButton::Middle => Some(0x4000),
        MouseButton::Navigate(_) => None,
    }
}

fn rdp_scancode(key: &str) -> Option<(u32, bool)> {
    let key = key.to_ascii_lowercase();
    let code = match key.as_str() {
        "a" => 0x1e,
        "b" => 0x30,
        "c" => 0x2e,
        "d" => 0x20,
        "e" => 0x12,
        "f" => 0x21,
        "g" => 0x22,
        "h" => 0x23,
        "i" => 0x17,
        "j" => 0x24,
        "k" => 0x25,
        "l" => 0x26,
        "m" => 0x32,
        "n" => 0x31,
        "o" => 0x18,
        "p" => 0x19,
        "q" => 0x10,
        "r" => 0x13,
        "s" => 0x1f,
        "t" => 0x14,
        "u" => 0x16,
        "v" => 0x2f,
        "w" => 0x11,
        "x" => 0x2d,
        "y" => 0x15,
        "z" => 0x2c,
        "1" | "digit1" | "!" => 0x02,
        "2" | "digit2" | "@" => 0x03,
        "3" | "digit3" | "#" => 0x04,
        "4" | "digit4" | "$" => 0x05,
        "5" | "digit5" | "%" => 0x06,
        "6" | "digit6" | "^" => 0x07,
        "7" | "digit7" | "&" => 0x08,
        "8" | "digit8" | "*" => 0x09,
        "9" | "digit9" | "(" => 0x0a,
        "0" | "digit0" | ")" => 0x0b,
        "numpad0" => return Some((0x52, false)),
        "numpad1" => return Some((0x4f, false)),
        "numpad2" => return Some((0x50, false)),
        "numpad3" => return Some((0x51, false)),
        "numpad4" => return Some((0x4b, false)),
        "numpad5" => return Some((0x4c, false)),
        "numpad6" => return Some((0x4d, false)),
        "numpad7" => return Some((0x47, false)),
        "numpad8" => return Some((0x48, false)),
        "numpad9" => return Some((0x49, false)),
        "escape" | "esc" => 0x01,
        "backspace" => 0x0e,
        "tab" => 0x0f,
        "enter" | "return" => 0x1c,
        "space" => 0x39,
        "minus" | "-" | "_" => 0x0c,
        "equal" | "=" | "+" => 0x0d,
        "[" | "bracketleft" => 0x1a,
        "]" | "bracketright" => 0x1b,
        "backslash" | "\\" | "|" => 0x2b,
        "semicolon" | ";" | ":" => 0x27,
        "apostrophe" | "'" | "\"" => 0x28,
        "grave" | "`" | "~" => 0x29,
        "comma" | "," | "<" => 0x33,
        "period" | "." | ">" => 0x34,
        "slash" | "/" | "?" => 0x35,
        "f1" => 0x3b,
        "f2" => 0x3c,
        "f3" => 0x3d,
        "f4" => 0x3e,
        "f5" => 0x3f,
        "f6" => 0x40,
        "f7" => 0x41,
        "f8" => 0x42,
        "f9" => 0x43,
        "f10" => 0x44,
        "f11" => 0x57,
        "f12" => 0x58,
        "arrowup" | "up" => return Some((0x48, true)),
        "arrowdown" | "down" => return Some((0x50, true)),
        "arrowleft" | "left" => return Some((0x4b, true)),
        "arrowright" | "right" => return Some((0x4d, true)),
        "home" => return Some((0x47, true)),
        "end" => return Some((0x4f, true)),
        "pageup" => return Some((0x49, true)),
        "pagedown" => return Some((0x51, true)),
        "insert" => return Some((0x52, true)),
        "delete" => return Some((0x53, true)),
        "control" | "ctrl" => return Some((0x1d, false)),
        "alt" | "option" => return Some((0x38, false)),
        "shift" => return Some((0x2a, false)),
        "capslock" => return Some((0x3a, false)),
        "numlock" => return Some((0x45, false)),
        _ => return None,
    };
    Some((code, false))
}

const RDP_MOD_CONTROL: u8 = 1 << 0;
const RDP_MOD_ALT: u8 = 1 << 1;
const RDP_MOD_SHIFT: u8 = 1 << 2;
const RDP_MOD_WINDOWS: u8 = 1 << 3;

fn rdp_modifier_mask(modifiers: gpui::Modifiers) -> u8 {
    let mut mask = 0;
    if modifiers.control || (cfg!(target_os = "macos") && modifiers.platform) {
        mask |= RDP_MOD_CONTROL;
    }
    if modifiers.alt {
        mask |= RDP_MOD_ALT;
    }
    if modifiers.shift {
        mask |= RDP_MOD_SHIFT;
    }
    if modifiers.platform && !cfg!(target_os = "macos") {
        mask |= RDP_MOD_WINDOWS;
    }
    mask
}

fn rdp_modifier_scancode(bit: u8) -> Option<(u32, bool)> {
    match bit {
        RDP_MOD_CONTROL => Some((0x1d, false)),
        RDP_MOD_ALT => Some((0x38, false)),
        RDP_MOD_SHIFT => Some((0x2a, false)),
        RDP_MOD_WINDOWS => Some((0x5b, true)),
        _ => None,
    }
}

fn is_rdp_modifier_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "control" | "ctrl" | "alt" | "option" | "shift" | "command" | "meta" | "super"
    )
}

fn rdp_fitted_point(
    container_width: f32,
    container_height: f32,
    local_x: f32,
    local_y: f32,
    frame_width: u32,
    frame_height: u32,
) -> Option<(u16, u16)> {
    if container_width <= 0.0 || container_height <= 0.0 || frame_width == 0 || frame_height == 0 {
        return None;
    }
    let scale = (container_width / frame_width as f32).min(container_height / frame_height as f32);
    let fitted_width = frame_width as f32 * scale;
    let fitted_height = frame_height as f32 * scale;
    let offset_x = (container_width - fitted_width) * 0.5;
    let offset_y = (container_height - fitted_height) * 0.5;
    let fitted_x = local_x - offset_x;
    let fitted_y = local_y - offset_y;
    if fitted_x < 0.0 || fitted_y < 0.0 || fitted_x >= fitted_width || fitted_y >= fitted_height {
        return None;
    }
    let x = (fitted_x / fitted_width * frame_width as f32)
        .floor()
        .clamp(0.0, frame_width.saturating_sub(1) as f32) as u16;
    let y = (fitted_y / fitted_height * frame_height as f32)
        .floor()
        .clamp(0.0, frame_height.saturating_sub(1) as f32) as u16;
    Some((x, y))
}

fn printable_terminal_input(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes)
        .ok()
        .filter(|text| !text.is_empty() && text.chars().all(|character| !character.is_control()))
}

fn trackable_terminal_paste(text: &str) -> Option<&str> {
    (!text.is_empty() && text.chars().all(|character| !character.is_control())).then_some(text)
}

impl TinyShell {
    pub(crate) fn accept_rdp_certificate(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(request) = self.rdp_certificate_requests.remove(tab_id) {
            request.decision.accept();
            cx.notify();
        }
    }

    pub(crate) fn accept_rdp_certificate_always(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(request) = self.rdp_certificate_requests.remove(tab_id) {
            crate::backend::remote_desktop::remember_certificate(
                &request.host,
                request.port,
                &request.fingerprint,
            );
            request.decision.accept_always();
            cx.notify();
        }
    }

    pub(crate) fn reject_rdp_certificate(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(request) = self.rdp_certificate_requests.remove(tab_id) {
            request.decision.reject();
            cx.notify();
        }
    }

    pub(crate) fn on_remote_desktop_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.active_remote_desktop_tab() else {
            return false;
        };
        if is_rdp_modifier_key(&event.keystroke.key) {
            window.prevent_default();
            cx.stop_propagation();
            return true;
        }
        let Some((scancode, extended)) = rdp_scancode(&event.keystroke.key) else {
            return false;
        };
        self.send_remote_desktop_input(
            &tab_id,
            RemoteDesktopInput::Key {
                scancode,
                down: true,
                extended,
            },
        );
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(crate) fn on_remote_desktop_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.active_remote_desktop_tab() else {
            return false;
        };
        if is_rdp_modifier_key(&event.keystroke.key) {
            window.prevent_default();
            cx.stop_propagation();
            return true;
        }
        let Some((scancode, extended)) = rdp_scancode(&event.keystroke.key) else {
            return false;
        };
        self.send_remote_desktop_input(
            &tab_id,
            RemoteDesktopInput::Key {
                scancode,
                down: false,
                extended,
            },
        );
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(crate) fn on_remote_desktop_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.active_remote_desktop_tab() else {
            return false;
        };
        let next = rdp_modifier_mask(event.modifiers);
        let previous = self
            .rdp_modifier_state
            .insert(tab_id.clone(), next)
            .unwrap_or_default();
        for bit in [RDP_MOD_CONTROL, RDP_MOD_ALT, RDP_MOD_SHIFT, RDP_MOD_WINDOWS] {
            if (previous & bit) == (next & bit) {
                continue;
            }
            let Some((scancode, extended)) = rdp_modifier_scancode(bit) else {
                continue;
            };
            self.send_remote_desktop_input(
                &tab_id,
                RemoteDesktopInput::Key {
                    scancode,
                    down: next & bit != 0,
                    extended,
                },
            );
        }
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    /// Releases modifiers that were synthesized for an embedded RDP tab
    /// before focus moves to another pane. GPUI reports the eventual key-up
    /// against the new active pane, so forwarding it there would leave the
    /// old remote session stuck in Ctrl/Alt/Shift/Win.
    pub(crate) fn release_rdp_modifiers(&mut self, tab_id: &str) {
        let Some(mask) = self.rdp_modifier_state.remove(tab_id) else {
            return;
        };
        for bit in [RDP_MOD_CONTROL, RDP_MOD_ALT, RDP_MOD_SHIFT, RDP_MOD_WINDOWS] {
            if mask & bit == 0 {
                continue;
            }
            let Some((scancode, extended)) = rdp_modifier_scancode(bit) else {
                continue;
            };
            self.send_remote_desktop_input(
                tab_id,
                RemoteDesktopInput::Key {
                    scancode,
                    down: false,
                    extended,
                },
            );
        }
    }

    pub(crate) fn on_remote_desktop_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.remote_desktop_tab_at(event.position) else {
            return false;
        };
        let Some((x, y)) = self.remote_desktop_point(&tab_id, event.position) else {
            return false;
        };
        let Some(button) = rdp_mouse_button(event.button) else {
            return false;
        };
        self.send_remote_desktop_input(
            &tab_id,
            RemoteDesktopInput::MouseButton {
                flags: button | 0x8000,
                x,
                y,
            },
        );
        self.focus_pane_with_id(tab_id);
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(crate) fn on_remote_desktop_mouse_up(
        &mut self,
        event: &gpui::MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.remote_desktop_tab_at(event.position) else {
            return false;
        };
        let Some((x, y)) = self.remote_desktop_point(&tab_id, event.position) else {
            return false;
        };
        let Some(button) = rdp_mouse_button(event.button) else {
            return false;
        };
        self.send_remote_desktop_input(
            &tab_id,
            RemoteDesktopInput::MouseButton {
                flags: button,
                x,
                y,
            },
        );
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(crate) fn on_remote_desktop_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.remote_desktop_tab_at(event.position) else {
            return false;
        };
        let Some((x, y)) = self.remote_desktop_point(&tab_id, event.position) else {
            return false;
        };
        self.send_remote_desktop_input(&tab_id, RemoteDesktopInput::MouseMove { x, y });
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    pub(crate) fn on_remote_desktop_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.remote_desktop_tab_at(event.position) else {
            return false;
        };
        let Some((x, y)) = self.remote_desktop_point(&tab_id, event.position) else {
            return false;
        };
        let delta = match event.delta {
            ScrollDelta::Lines(point) => point.y,
            ScrollDelta::Pixels(point) => point.y.as_f32() / 40.0,
        };
        if delta.abs() < f32::EPSILON {
            return false;
        }
        let ticks = (delta.abs().round().max(1.0) as u16)
            .saturating_mul(120)
            .min(0x01ff);
        let flags = 0x0200 | if delta.is_sign_negative() { 0x0100 } else { 0 } | ticks;
        self.send_remote_desktop_input(&tab_id, RemoteDesktopInput::MouseWheel { flags, x, y });
        window.prevent_default();
        cx.stop_propagation();
        true
    }

    fn remote_desktop_tab_at(&self, position: Point<Pixels>) -> Option<String> {
        self.terminal_bounds.iter().find_map(|(tab_id, bounds)| {
            bounds.contains(&position).then(|| {
                self.workspace()
                    .terminal_tab(tab_id)
                    .filter(|tab| tab.kind == TabKind::Rdp)
                    .map(|_| tab_id.clone())
            })?
        })
    }

    fn active_remote_desktop_tab(&self) -> Option<String> {
        let tab_id = self.preferred_terminal_tab_id()?;
        self.terminal_tab(&tab_id)
            .filter(|tab| tab.kind == TabKind::Rdp)
            .map(|_| tab_id)
    }

    fn remote_desktop_point(&self, tab_id: &str, position: Point<Pixels>) -> Option<(u16, u16)> {
        let bounds = self.terminal_bounds.get(tab_id)?;
        let size = self
            .remote_desktop_surfaces
            .get(tab_id)
            .map(|surface| surface.size)
            .unwrap_or(crate::backend::remote_desktop::FrameSize {
                width: bounds.size.width.as_f32().max(1.0) as u32,
                height: bounds.size.height.as_f32().max(1.0) as u32,
                stride: 0,
            });
        rdp_fitted_point(
            bounds.size.width.as_f32(),
            bounds.size.height.as_f32(),
            (position.x - bounds.origin.x).as_f32(),
            (position.y - bounds.origin.y).as_f32(),
            size.width,
            size.height,
        )
    }

    fn send_remote_desktop_input(&mut self, tab_id: &str, input: RemoteDesktopInput) {
        if let Some(tab) = self.terminal_tab_mut(tab_id) {
            tab.send_backend(BackendCommand::RemoteDesktopInput(input));
        }
    }

    pub(crate) fn on_terminal_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cmd_ctrl_pressed = event.keystroke.modifiers.platform;
        // If the search input is focused, skip terminal key processing
        // so the input can handle text entry, paste, etc. normally.
        if self
            .search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
        {
            return;
        }

        if self.clear_completion_in_alternate_screen() {
            cx.notify();
        }

        if self.handle_terminal_completion_key(event, window, cx) {
            return;
        }

        // Pane navigation: Alt + h/j/k/l
        if event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.shift
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.platform
        {
            match event.keystroke.key.to_ascii_lowercase().as_str() {
                "h" => self.focus_adjacent_pane(crate::app::PaneDirection::Left),
                "j" => self.focus_adjacent_pane(crate::app::PaneDirection::Down),
                "k" => self.focus_adjacent_pane(crate::app::PaneDirection::Up),
                "l" => self.focus_adjacent_pane(crate::app::PaneDirection::Right),
                "q" => {
                    if let Some(active_id) = self.preferred_terminal_tab_id() {
                        self.close_tab(active_id, cx);
                    }
                }
                _ => return,
            }
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
            return;
        }

        // Pane split: Shift+Alt + h/j/k/l
        if event.keystroke.modifiers.shift
            && event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.platform
        {
            let direction = match event.keystroke.key.to_ascii_lowercase().as_str() {
                "h" => Some(crate::app::PaneDirection::Left),
                "j" => Some(crate::app::PaneDirection::Down),
                "k" => Some(crate::app::PaneDirection::Up),
                "l" => Some(crate::app::PaneDirection::Right),
                _ => None,
            };
            if let Some(dir) = direction {
                self.split_current_pane(dir, cx);
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }

        if event.keystroke.modifiers.secondary() && event.keystroke.key == "," {
            self.show_settings_window(window, cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.shift
            && event.keystroke.modifiers.secondary()
            && event.keystroke.key == "o"
        {
            self.show_selector_dialog(window, cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if event.keystroke.modifiers.secondary() && event.keystroke.key.eq_ignore_ascii_case("c") {
            if let Some(text) = self.active_terminal_selection_text() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                window.prevent_default();
                cx.stop_propagation();
                return;
            }
        }
        if event.keystroke.modifiers.secondary() && event.keystroke.key.eq_ignore_ascii_case("v") {
            if let Some(clipboard) = cx.read_from_clipboard() {
                self.paste_clipboard_item(&clipboard, window, cx);
                return;
            }
        }

        // If the active tab is disconnected and user presses Enter, reconnect
        if event.keystroke.key == "enter"
            && !event.keystroke.modifiers.shift
            && !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform
        {
            let active_id = self.preferred_terminal_tab_id();
            if let Some(active_id) = active_id {
                let is_disconnected = self
                    .terminal_tab(&active_id)
                    .is_some_and(|tab| tab.disconnected_reason.is_some());
                if is_disconnected {
                    self.retry_disconnected_tab(&active_id, cx);
                    window.prevent_default();
                    cx.stop_propagation();
                    return;
                }
            }
        }

        if event.prefer_character_input {
            if let Some(text) = event.keystroke.key_char.as_deref() {
                if !text.is_empty()
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.function
                    && !event.keystroke.modifiers.platform
                {
                    self.send_terminal_input(text.as_bytes().to_vec(), window, cx);
                    if !event.keystroke.modifiers.alt {
                        self.track_terminal_completion_text(text);
                        cx.notify();
                    } else {
                        self.clear_active_terminal_completion();
                    }
                }
            }
            return;
        }

        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(&active_id) else {
            return;
        };

        if tab.display_offset() > 0 {
            tab.scroll_to_bottom();
        }
        tab.clear_selection();

        if let Some(bytes) = encode_key(&event.keystroke, tab.app_cursor_mode(), false) {
            let completion_text = printable_terminal_input(&bytes).map(str::to_owned);
            tab.send_backend(BackendCommand::Input(bytes));
            if let Some(text) = completion_text {
                self.track_terminal_completion_text(&text);
            } else {
                self.update_terminal_completion_for_key(event);
            }
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn active_ssh_completion_tab_id(&self) -> Option<String> {
        let active_id_binding = self.preferred_terminal_tab_id();
        let active_id = active_id_binding.as_ref()?;
        self.terminal_tab(active_id)
            .filter(|tab| tab.kind == crate::terminal::TabKind::Ssh && !tab.is_alternate_screen())
            .map(|tab| tab.id.clone())
    }

    fn clear_completion_in_alternate_screen(&mut self) -> bool {
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return false;
        };
        let is_alternate_ssh = self.terminal_tab(&active_id).is_some_and(|tab| {
            tab.kind == crate::terminal::TabKind::Ssh && tab.is_alternate_screen()
        });
        is_alternate_ssh && self.terminal_completions.remove(&active_id).is_some()
    }

    fn quick_command_categories_for_completion(
        &self,
    ) -> Vec<crate::session::config::QuickCommandCategory> {
        self.config
            .quick_command_categories()
            .unwrap_or_default()
            .to_vec()
    }

    fn track_terminal_completion_text(&mut self, text: &str) {
        let Some(tab_id) = self.active_ssh_completion_tab_id() else {
            return;
        };
        let categories = self.quick_command_categories_for_completion();
        let state = self.terminal_completions.entry(tab_id).or_default();
        if text.chars().any(char::is_control) {
            state.clear();
        } else {
            state.push_text(text, &categories);
        }
    }

    fn clear_active_terminal_completion(&mut self) {
        let Some(tab_id) = self.active_ssh_completion_tab_id() else {
            return;
        };
        if let Some(state) = self.terminal_completions.get_mut(&tab_id) {
            state.clear();
        }
    }

    fn update_terminal_completion_for_key(&mut self, event: &KeyDownEvent) {
        let Some(tab_id) = self.active_ssh_completion_tab_id() else {
            return;
        };
        let categories = self.quick_command_categories_for_completion();
        let state = self.terminal_completions.entry(tab_id).or_default();
        let modifiers = event.keystroke.modifiers;
        let has_modifiers = modifiers.shift
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function;

        if !has_modifiers && event.keystroke.key == "backspace" {
            state.backspace(&categories);
        } else {
            state.clear();
        }
    }

    fn handle_terminal_completion_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = event.keystroke.modifiers;
        if modifiers.shift
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return false;
        }
        let Some(tab_id) = self.active_ssh_completion_tab_id() else {
            return false;
        };
        let is_visible = self
            .terminal_completions
            .get(&tab_id)
            .is_some_and(|state| state.is_visible());
        if !is_visible {
            return false;
        }
        match event.keystroke.key.as_str() {
            "tab" => return self.accept_active_terminal_completion(true, window, cx),
            "enter" => return self.accept_active_terminal_completion(false, window, cx),
            _ => {}
        }
        let Some(state) = self.terminal_completions.get_mut(&tab_id) else {
            return false;
        };

        match event.keystroke.key.as_str() {
            "up" => state.move_selection(-1),
            "down" => state.move_selection(1),
            "escape" => state.dismiss(),
            _ => return false,
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
        true
    }

    pub(crate) fn accept_active_terminal_completion(
        &mut self,
        select_first_if_needed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab_id) = self.active_ssh_completion_tab_id() else {
            return false;
        };
        let Some(suffix) = self
            .terminal_completions
            .get_mut(&tab_id)
            .and_then(|state| {
                if select_first_if_needed {
                    state.accept_selected_or_first()
                } else {
                    state.accept_selected()
                }
            })
        else {
            return false;
        };

        if suffix.is_empty() {
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        } else {
            self.send_terminal_input(suffix.into_bytes(), window, cx);
        }
        true
    }

    pub(crate) fn accept_terminal_completion_at(
        &mut self,
        tab_id: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active_ssh_completion_tab_id().as_deref() != Some(tab_id) {
            return false;
        }
        let Some(state) = self.terminal_completions.get_mut(tab_id) else {
            return false;
        };
        state.select(index);
        self.accept_active_terminal_completion(false, window, cx)
    }

    pub(crate) fn on_terminal_tab_action(
        &mut self,
        _: &TerminalTabKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.accept_active_terminal_completion(true, window, cx) {
            self.send_terminal_input(vec![b'\t'], window, cx);
        }
    }

    pub(crate) fn on_terminal_backtab_action(
        &mut self,
        _: &TerminalBacktabKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.send_terminal_input(b"\x1b[Z".to_vec(), window, cx);
    }

    pub(crate) fn send_terminal_input(
        &mut self,
        bytes: Vec<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(&active_id) else {
            return;
        };

        if tab.display_offset() > 0 {
            tab.scroll_to_bottom();
        }

        tab.clear_selection();
        tab.send_backend(BackendCommand::Input(bytes));
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn active_terminal_selection_text(&self) -> Option<String> {
        let active_id_binding = self.preferred_terminal_tab_id();
        let active_id = active_id_binding.as_ref()?;
        self.terminal_tab(active_id)
            .and_then(|tab| tab.selection_text())
    }

    pub(crate) fn paste_into_terminal(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste_text_into_terminal(text, window, cx);
    }

    pub(crate) fn paste_clipboard_item(
        &mut self,
        item: &ClipboardItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(&active_id) else {
            return;
        };

        if tab.kind == TabKind::Rdp {
            let paths = item.entries().iter().find_map(|entry| match entry {
                gpui::ClipboardEntry::ExternalPaths(paths) if !paths.0.is_empty() => {
                    Some(paths.0.to_vec())
                }
                _ => None,
            });
            if let Some(paths) = paths {
                tab.send_backend(BackendCommand::RemoteDesktopClipboardFiles(paths));
            } else if let Some(text) = item.text() {
                tab.send_backend(BackendCommand::RemoteDesktopClipboard(text));
            }
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
            return;
        }

        if let Some(text) = item.text() {
            self.paste_text_into_terminal(&text, window, cx);
        }
    }

    fn paste_text_into_terminal(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let completion_text = trackable_terminal_paste(text).map(str::to_owned);
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        let Some(tab) = self.terminal_tab_mut(&active_id) else {
            return;
        };

        if tab.display_offset() > 0 {
            tab.scroll_to_bottom();
        }
        tab.clear_selection();
        tab.paste_text(text);
        if let Some(text) = completion_text {
            self.track_terminal_completion_text(&text);
        } else {
            self.clear_active_terminal_completion();
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn terminal_accepts_text_input(&self) -> bool {
        self.preferred_terminal_tab_id().is_some()
    }

    pub(crate) fn terminal_marked_text_range(&self) -> Option<Range<usize>> {
        self.terminal_marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    pub(crate) fn set_terminal_marked_text(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal_marked_text = if text.is_empty() { None } else { Some(text) };
        window.invalidate_character_coordinates();
        cx.notify();
    }

    pub(crate) fn clear_terminal_marked_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_marked_text.take().is_some() {
            window.invalidate_character_coordinates();
            cx.notify();
        }
    }

    pub(crate) fn commit_terminal_ime_text(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        {
            let Some(tab) = self.terminal_tab_mut(&active_id) else {
                return;
            };

            if tab.display_offset() > 0 {
                tab.scroll_to_bottom();
            }
            tab.clear_selection();
            tab.send_backend(BackendCommand::Input(text.as_bytes().to_vec()));
        }
        self.terminal_marked_text = None;
        self.track_terminal_completion_text(text);
        window.invalidate_character_coordinates();
        cx.notify();
    }

    pub(crate) fn clear_active_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        if let Some(tab) = self.terminal_tab_mut(&active_id) {
            tab.clear_contents();
            cx.notify();
        }
    }

    pub(crate) fn begin_terminal_selection(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.clear_active_terminal_completion();
        let click_count = event.click_count.max(1);
        let selection_type = match click_count {
            1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            3 => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        let Some((row, col, side)) = self.terminal_grid_point_and_side(event.position) else {
            return;
        };
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        if let Some(tab) = self.terminal_tab_mut(&active_id) {
            tab.begin_selection(row, col, side, selection_type);
            self.terminal_selecting = true;
            cx.notify();
        }
    }

    pub(crate) fn on_terminal_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Skip terminal mouse handling during tab drag-to-split
        if self.tab_drag.is_dragging() || self.tab_drag.is_pending() {
            return;
        }

        // Handle split drag
        if self.dragging_splitter.is_some() {
            if event.pressed_button == Some(MouseButton::Left) {
                self.on_split_drag_move(event, window, cx);
                cx.notify();
            } else {
                self.end_drag_split();
                cx.notify();
            }
            return;
        }

        // Track semantic entities; modifier-click exposes their primary action.
        let mut hovered_entity = None;
        let cmd_ctrl_pressed = event.modifiers.platform;
        if let Some((row, col, _side)) = self.terminal_grid_point_and_side(event.position) {
            if let Some(snapshot) = self.active_snapshot() {
                if let Some(active_id) = self.preferred_terminal_tab_id() {
                    if let Some(entity) = crate::terminal::highlight::find_entity_at_cell(
                        &snapshot.cells,
                        snapshot.rows,
                        row,
                        col,
                    ) {
                        hovered_entity = Some(crate::app::HoveredTerminalEntity {
                            kind: entity.kind,
                            value: entity.value,
                            tab_id: active_id.clone(),
                            cells: entity.cells,
                        });
                    }
                }
            }
        }

        if self.hovered_entity != hovered_entity || self.cmd_ctrl_pressed != cmd_ctrl_pressed {
            self.hovered_entity = hovered_entity;
            self.cmd_ctrl_pressed = cmd_ctrl_pressed;
            cx.notify();
        }

        if !self.terminal_selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((row, col, side)) = self.terminal_grid_point_and_side(event.position) else {
            return;
        };
        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };
        let snapshot = match self.active_snapshot() {
            Some(s) => s,
            None => return,
        };
        let max_row = snapshot.rows.saturating_sub(1);

        let mut scroll_delta = 0i32;
        if max_row >= 6 {
            if row <= 2 || row >= max_row.saturating_sub(2) {
                let now = std::time::Instant::now();
                let should_scroll = LAST_DRAG_SCROLL.with(|last| {
                    if let Some(last_time) = last.get() {
                        if now.duration_since(last_time) >= std::time::Duration::from_millis(80) {
                            last.set(Some(now));
                            true
                        } else {
                            false
                        }
                    } else {
                        last.set(Some(now));
                        true
                    }
                });

                if should_scroll {
                    if row == 0 {
                        scroll_delta = 2;
                    } else if row == 1 || row == 2 {
                        scroll_delta = 1;
                    } else if row == max_row {
                        scroll_delta = -2;
                    } else if row == max_row.saturating_sub(1) || row == max_row.saturating_sub(2) {
                        scroll_delta = -1;
                    }
                }
            } else {
                LAST_DRAG_SCROLL.with(|last| last.set(None));
            }
        }

        if let Some(tab) = self.terminal_tab_mut(&active_id) {
            if scroll_delta != 0 {
                tab.scroll_history(scroll_delta);
            }
            tab.update_selection(row, col, side);
            cx.notify();
        }
    }

    pub(crate) fn on_terminal_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Skip during tab drag — root handler takes care of it
        if self.tab_drag.is_dragging() || self.tab_drag.is_pending() {
            return;
        }

        if self.dragging_splitter.is_some() {
            self.end_drag_split();
        }
        self.terminal_selecting = false;
        cx.notify();
    }

    pub(crate) fn terminal_grid_point_and_side(
        &self,
        position: Point<Pixels>,
    ) -> Option<(usize, usize, Side)> {
        let active_id_binding = self.preferred_terminal_tab_id();
        let active_id = active_id_binding.as_ref()?;
        let bounds = self.terminal_bounds.get(active_id)?;
        if !bounds.contains(&position) {
            // Try other pane bounds
            for b in self.terminal_bounds.values() {
                if b.contains(&position) {
                    // Found a different pane - focus it
                    // (this path is for click-to-focus; handled via focus_terminal)
                    return None;
                }
            }
            return None;
        }
        let local_x = (position.x - bounds.origin.x).max(px(0.));
        let local_y = (position.y - bounds.origin.y).max(px(0.));
        let cell_width = px(self.terminal_cell_width());
        let line_height = px(self.terminal_line_height());
        let snapshot = self.active_snapshot()?;
        let max_col = snapshot.cols.saturating_sub(1);
        let max_row = snapshot.rows.saturating_sub(1);
        let col = ((local_x / cell_width).floor() as usize).min(max_col);
        let row = ((local_y / line_height).floor() as usize).min(max_row);
        let cell_offset_x = px(local_x.as_f32() % cell_width.as_f32());
        let side = if cell_offset_x >= (cell_width / 2.) {
            Side::Right
        } else {
            Side::Left
        };
        Some((row, col, side))
    }

    pub(crate) fn on_terminal_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Platform modifier (Cmd on macOS, Ctrl on Windows/Linux) + scroll → zoom terminal font size
        if event.modifiers.platform {
            let delta = match event.delta {
                ScrollDelta::Lines(point) => point.y * 20.0,
                ScrollDelta::Pixels(point) => point.y.as_f32(),
            };
            self.terminal_zoom_accumulator += delta;
            let step = 20.0;
            if self.terminal_zoom_accumulator.abs() >= step {
                let zoom_steps = (self.terminal_zoom_accumulator / step).trunc();
                self.terminal_zoom_accumulator -= zoom_steps * step;
                self.change_terminal_font_size(zoom_steps * 0.5, cx);
            }
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        let Some(active_id) = self.preferred_terminal_tab_id() else {
            return;
        };

        // Get coordinates before mutably borrowing tabs
        let grid_point = self.terminal_grid_point_and_side(event.position);

        let line_height = self.terminal_line_height();

        if let Some(tab) = self.terminal_tab_mut(&active_id) {
            let delta_lines = match event.delta {
                ScrollDelta::Lines(point) => point.y.round() as i32,
                ScrollDelta::Pixels(point) => {
                    tab.scroll_pixel_y += point.y.as_f32();
                    let lines = (tab.scroll_pixel_y / line_height).trunc() as i32;
                    tab.scroll_pixel_y -= (lines as f32) * line_height;
                    lines
                }
            };

            if delta_lines == 0 {
                return;
            }

            let mode = tab.mode();

            let is_mouse_tracking = mode.intersects(
                alacritty_terminal::term::TermMode::MOUSE_REPORT_CLICK
                    | alacritty_terminal::term::TermMode::MOUSE_MOTION
                    | alacritty_terminal::term::TermMode::MOUSE_DRAG,
            );

            let is_alternate_scroll = mode.contains(
                alacritty_terminal::term::TermMode::ALT_SCREEN
                    | alacritty_terminal::term::TermMode::ALTERNATE_SCROLL,
            );

            if is_mouse_tracking {
                if let Some((row, col, _)) = grid_point {
                    let sgr = mode.contains(alacritty_terminal::term::TermMode::SGR_MOUSE);
                    let button = if delta_lines > 0 { 64 } else { 65 };
                    let times = delta_lines.abs();
                    let mut bytes = Vec::new();
                    for _ in 0..times {
                        if sgr {
                            bytes.extend_from_slice(
                                format!("\x1b[<{};{};{}M", button, col + 1, row + 1).as_bytes(),
                            );
                        } else if col < 223 && row < 223 {
                            bytes.extend_from_slice(b"\x1b[M");
                            bytes.push(button as u8 + 32);
                            bytes.push(col as u8 + 33);
                            bytes.push(row as u8 + 33);
                        }
                    }
                    if !bytes.is_empty() {
                        tab.send_backend(crate::terminal::BackendCommand::Input(bytes));
                    }
                }
                window.prevent_default();
                cx.stop_propagation();
                return;
            } else if is_alternate_scroll {
                let times = delta_lines.abs();
                let code = if delta_lines > 0 { b'A' } else { b'B' };
                let mut bytes = Vec::with_capacity((times * 3) as usize);
                for _ in 0..times {
                    bytes.extend_from_slice(&[b'\x1b', b'O', code]);
                }
                if !bytes.is_empty() {
                    tab.send_backend(crate::terminal::BackendCommand::Input(bytes));
                }
                window.prevent_default();
                cx.stop_propagation();
                return;
            }

            tab.scroll_history(delta_lines);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::MouseButton;

    use super::{
        RDP_MOD_ALT, RDP_MOD_CONTROL, RDP_MOD_SHIFT, RDP_MOD_WINDOWS, printable_terminal_input,
        rdp_fitted_point, rdp_modifier_mask, rdp_modifier_scancode, rdp_mouse_button, rdp_scancode,
        trackable_terminal_paste,
    };

    #[test]
    fn printable_input_tracks_windows_keydown_text() {
        assert_eq!(printable_terminal_input(b"g"), Some("g"));
        assert_eq!(printable_terminal_input("状态".as_bytes()), Some("状态"));
    }

    #[test]
    fn control_sequences_do_not_enter_completion_state() {
        assert_eq!(printable_terminal_input(b"\r"), None);
        assert_eq!(printable_terminal_input(b"\x7f"), None);
        assert_eq!(printable_terminal_input(b"\x1b[A"), None);
    }

    #[test]
    fn single_line_paste_is_trackable_but_multiline_paste_is_not() {
        assert_eq!(trackable_terminal_paste("do"), Some("do"));
        assert_eq!(trackable_terminal_paste("docker ps"), Some("docker ps"));
        assert_eq!(trackable_terminal_paste("docker\nps"), None);
        assert_eq!(trackable_terminal_paste("docker\r"), None);
        assert_eq!(trackable_terminal_paste(""), None);
    }

    #[test]
    fn rdp_scancodes_cover_common_keyboard_navigation() {
        assert_eq!(rdp_scancode("A"), Some((0x1e, false)));
        assert_eq!(rdp_scancode("0"), Some((0x0b, false)));
        assert_eq!(rdp_scancode("1"), Some((0x02, false)));
        assert_eq!(rdp_scancode("9"), Some((0x0a, false)));
        assert_eq!(rdp_scancode("Digit1"), Some((0x02, false)));
        assert_eq!(rdp_scancode("Numpad7"), Some((0x47, false)));
        assert_eq!(rdp_scancode("f12"), Some((0x58, false)));
        assert_eq!(rdp_scancode("ArrowLeft"), Some((0x4b, true)));
        assert_eq!(rdp_scancode("PageDown"), Some((0x51, true)));
        assert_eq!(rdp_scancode("unknown-key"), None);
    }

    #[test]
    fn rdp_modifier_scancodes_match_windows_set_1() {
        assert_eq!(rdp_modifier_scancode(RDP_MOD_CONTROL), Some((0x1d, false)));
        assert_eq!(rdp_modifier_scancode(RDP_MOD_ALT), Some((0x38, false)));
        assert_eq!(rdp_modifier_scancode(RDP_MOD_SHIFT), Some((0x2a, false)));
        assert_eq!(rdp_modifier_scancode(RDP_MOD_WINDOWS), Some((0x5b, true)));
        assert_eq!(rdp_modifier_scancode(0), None);
    }

    #[test]
    fn rdp_modifier_mask_preserves_native_control_alt_shift() {
        let modifiers = gpui::Modifiers {
            control: true,
            alt: true,
            shift: true,
            ..Default::default()
        };
        let mask = rdp_modifier_mask(modifiers);
        assert_eq!(
            mask & (RDP_MOD_CONTROL | RDP_MOD_ALT | RDP_MOD_SHIFT),
            RDP_MOD_CONTROL | RDP_MOD_ALT | RDP_MOD_SHIFT
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_command_is_forwarded_as_remote_control() {
        let mask = rdp_modifier_mask(gpui::Modifiers {
            platform: true,
            ..Default::default()
        });
        assert_ne!(mask & RDP_MOD_CONTROL, 0);
        assert_eq!(mask & RDP_MOD_WINDOWS, 0);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn platform_modifier_remains_remote_windows_key_off_macos() {
        let mask = rdp_modifier_mask(gpui::Modifiers {
            platform: true,
            ..Default::default()
        });
        assert_eq!(mask & RDP_MOD_CONTROL, 0);
        assert_ne!(mask & RDP_MOD_WINDOWS, 0);
    }

    #[test]
    fn rdp_mouse_buttons_use_freerdp_flag_values() {
        assert_eq!(rdp_mouse_button(MouseButton::Left), Some(0x1000));
        assert_eq!(rdp_mouse_button(MouseButton::Right), Some(0x2000));
        assert_eq!(rdp_mouse_button(MouseButton::Middle), Some(0x4000));
    }

    #[test]
    fn rdp_pointer_mapping_respects_contain_letterboxing() {
        assert_eq!(
            rdp_fitted_point(1000.0, 1000.0, 500.0, 500.0, 1920, 1080),
            Some((960, 540))
        );
        assert_eq!(
            rdp_fitted_point(1000.0, 1000.0, 500.0, 100.0, 1920, 1080),
            None
        );
        assert_eq!(
            rdp_fitted_point(1000.0, 500.0, 10.0, 250.0, 1000, 1000),
            None
        );
    }
}
