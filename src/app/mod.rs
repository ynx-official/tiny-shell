mod backend_events;
pub(crate) mod config_persistence;
pub(crate) mod config_sync;
pub(crate) mod connection_actions;
pub(crate) mod connection_archive_dialogs;
pub(crate) mod connection_import_window;
pub(crate) mod connection_manager;
pub(crate) mod constants;
pub(crate) mod dialogs;
pub(crate) mod input_focus;
pub(crate) mod keybinding_recorder;
pub(crate) mod managed_keys;
pub(crate) mod monitoring;
pub(crate) mod platform;
pub(crate) mod resizable;
pub(crate) mod runtime_state;
pub(crate) mod search;
pub(crate) mod session_actions;
pub(crate) mod settings;
pub(crate) mod settings_window;
pub(crate) mod sftp_dialogs;
pub(crate) mod sftp_editor;
pub(crate) mod sftp_editor_window;
pub(crate) mod ssh_key_import;
pub(crate) mod startup;
pub(crate) mod sync_dialogs;
pub(crate) mod sync_handlers;
pub(crate) mod tab_drag;
pub(crate) mod terminal_completion;
pub(crate) mod terminal_settings;
pub(crate) mod terminal_workspace;
pub(crate) mod theme;
pub(crate) mod tool_panel;
pub(crate) mod transfer_manager;
pub(crate) mod ui;
pub(crate) mod updater;
pub(crate) mod workspace_presentation;

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    ops::Range,
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::app::{
    backend_events::coalesce_backend_events,
    connection_manager::{actions::ConnectionManagerActions, state::ConnectionManagerState},
    monitoring::{MonitoringState, MonitoringVisibilityContext, metrics_visible, push_bounded},
    resizable::ResizableState,
    runtime_state::{
        AsyncRuntimeState, AuxiliaryWindowsState, ConfigPersistenceState, DialogToken,
        SyncRuntimeState, UpdateRuntimeState,
    },
    settings::form::SettingsInputs,
    ssh_key_import::KeyImportState,
    terminal_workspace::WindowState,
    workspace_presentation::WorkspaceMode,
};
use futures::{FutureExt as _, pin_mut, select_biased};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Pixels, Point,
    SharedString, Size, UniformListScrollHandle, Window, point, px, size,
};
use gpui_component::{
    Theme, ThemeMode, ThemeRegistry, WindowExt,
    dialog::Dialog,
    input::{InputEvent, InputState},
    scroll::ScrollbarHandle,
};
use rust_i18n::t;
use tokio::runtime::Runtime;

use crate::{
    session::{
        config::{AuthMethod, ConfigStore, ManagedKey, QuickCommandCategory, TerminalDisplayStyle},
        quick_commands::{BUILTIN_QUICK_COMMANDS_VERSION, merge_builtin_quick_commands},
        ssh_config::SshConfigEntry,
    },
    system::{SharedSystemSampler, SystemSampler, SystemSnapshot},
    terminal::{
        self, BackendCommand, BackendEvent, BackendEventSender, TabKind, TerminalTab,
        backend_event_channel,
    },
};

/// Returns a process-wide shared tokio runtime.
///
/// Previously each window (`TinyShell`) owned its own `Runtime::new()`, which meant
/// every additional window spawned another full set of worker threads
/// (one per CPU core by default). Sharing a single `Arc<Runtime>` across all
/// windows keeps the thread count flat regardless of how many windows are open.
static SHARED_RUNTIME: OnceLock<Result<Arc<Runtime>, String>> = OnceLock::new();

pub(crate) fn shared_runtime() -> Result<Arc<Runtime>, String> {
    SHARED_RUNTIME
        .get_or_init(|| {
            Runtime::new()
                .map(Arc::new)
                .map_err(|error| format!("failed to create shared tokio runtime: {error}"))
        })
        .clone()
}

/// Process-wide shared system sampler. Avoids each window independently
/// reading `/proc` (and equivalents) — only one sample runs per interval
/// regardless of how many windows are open.
static SHARED_SYSTEM_SAMPLER: OnceLock<Arc<std::sync::Mutex<SharedSystemSampler>>> =
    OnceLock::new();

pub(crate) fn shared_system_sampler() -> Arc<std::sync::Mutex<SharedSystemSampler>> {
    SHARED_SYSTEM_SAMPLER
        .get_or_init(|| Arc::new(std::sync::Mutex::new(SharedSystemSampler::new())))
        .clone()
}

// ─── Cross-window registry ────────────────────────────────────────
// Each open tiny-shell window registers its `WindowHandle` + `Entity<TinyShell>`
// + current screen-space bounds here. This lets a tab being dragged in
// one window find another window to merge into by hit-testing the
// cursor's screen position against every other window's bounds.

pub(crate) struct WindowEntry {
    pub window_handle: AnyWindowHandle,
    pub entity: Entity<TinyShell>,
    pub screen_bounds: Bounds<Pixels>,
    pub activation_seq: u64,
}

#[derive(Clone)]
pub(crate) struct IncomingTabDrag {
    pub(crate) drag_id: u64,
    pub(crate) source_window: AnyWindowHandle,
    pub(crate) source: Entity<TinyShell>,
    pub(crate) group_id: String,
}

static WINDOW_REGISTRY: OnceLock<Arc<Mutex<Vec<WindowEntry>>>> = OnceLock::new();
static AUXILIARY_WINDOW_REGISTRY: OnceLock<Arc<Mutex<Vec<AuxiliaryWindowEntry>>>> = OnceLock::new();
static TAB_DRAG_HOVER: OnceLock<Mutex<Option<(u64, AnyWindowHandle, u64)>>> = OnceLock::new();
static WINDOW_ACTIVATION_SEQ: AtomicU64 = AtomicU64::new(1);
static TAB_DRAG_SEQ: AtomicU64 = AtomicU64::new(1);
static TAB_DRAG_HOVER_SEQ: AtomicU64 = AtomicU64::new(1);
static SESSION_OWNER_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct AuxiliaryWindowEntry {
    owner_id: crate::session::store::WindowOwnerId,
    window: AnyWindowHandle,
}
fn lock_recover<'a, T>(mutex: &'a Mutex<T>, name: &str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("{name} lock was poisoned; recovering its state");
            poisoned.into_inner()
        }
    }
}

pub(crate) fn window_registry() -> Arc<Mutex<Vec<WindowEntry>>> {
    WINDOW_REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

fn auxiliary_window_registry() -> Arc<Mutex<Vec<AuxiliaryWindowEntry>>> {
    AUXILIARY_WINDOW_REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

pub(crate) fn register_auxiliary_window(
    window: AnyWindowHandle,
    owner_id: crate::session::store::WindowOwnerId,
) {
    let registry = auxiliary_window_registry();
    let mut guard = lock_recover(&registry, "auxiliary window registry");
    if !guard.iter().any(|entry| entry.window == window) {
        guard.push(AuxiliaryWindowEntry { owner_id, window });
    }
}

pub(crate) fn deregister_auxiliary_window(window: AnyWindowHandle) {
    let registry = auxiliary_window_registry();
    lock_recover(&registry, "auxiliary window registry").retain(|entry| entry.window != window);
}

/// Close every independent window owned by the application. Handles are
/// removed from the registry before calling into GPUI so close callbacks can
/// safely update their owner without re-entering this registry.
pub(crate) fn close_auxiliary_windows(
    owner_id: crate::session::store::WindowOwnerId,
    cx: &mut App,
) {
    crate::app::sftp_editor_window::force_close_all(owner_id, cx);
    let windows = {
        let registry = auxiliary_window_registry();
        let mut guard = lock_recover(&registry, "auxiliary window registry");
        let mut windows = Vec::new();
        guard.retain(|entry| {
            if entry.owner_id == owner_id {
                windows.push(entry.window);
                false
            } else {
                true
            }
        });
        windows
    };
    for window in windows {
        if let Err(error) = window.update(cx, |_, window, _| window.remove_window()) {
            tracing::debug!("independent window was already closed: {error:?}");
        }
    }
}

fn select_config_repository(
    entries: impl IntoIterator<Item = (bool, u64, Arc<config_persistence::ConfigRepository>)>,
) -> Option<Arc<config_persistence::ConfigRepository>> {
    entries
        .into_iter()
        .filter(|(is_open, _, _)| *is_open)
        .max_by_key(|(_, activation_seq, _)| *activation_seq)
        .map(|(_, _, repository)| repository)
}

/// Reuse the repository owned by an already-open application window.
///
/// The registry is the application-level source of truth for window ownership;
/// keeping this lookup here avoids introducing a second process-global
/// repository singleton solely for the macOS reopen callback.
pub(crate) fn config_repository_for_open_window(
    cx: &App,
) -> Option<Arc<config_persistence::ConfigRepository>> {
    let entries = {
        let registry = window_registry();
        lock_recover(&registry, "window registry")
            .iter()
            .map(|entry| {
                (
                    cx.windows().contains(&entry.window_handle),
                    entry.activation_seq,
                    entry.entity.read(cx).config_repository.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    select_config_repository(entries)
}

pub(crate) fn next_tab_drag_id() -> u64 {
    TAB_DRAG_SEQ.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn set_tab_drag_hover(drag_id: u64, target_window: AnyWindowHandle) -> u64 {
    let generation = TAB_DRAG_HOVER_SEQ.fetch_add(1, Ordering::Relaxed);
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(None));
    *lock_recover(hover, "tab drag hover") = Some((drag_id, target_window, generation));
    generation
}

pub(crate) fn tab_drag_hover_is_current(
    drag_id: u64,
    target_window: AnyWindowHandle,
    generation: u64,
) -> bool {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(None));
    lock_recover(hover, "tab drag hover")
        .as_ref()
        .is_some_and(|current| *current == (drag_id, target_window, generation))
}

pub(crate) fn clear_tab_drag_hover() {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(None));
    *lock_recover(hover, "tab drag hover") = None;
}

/// Register a window when it opens.
pub(crate) fn register_window(window_handle: AnyWindowHandle, entity: Entity<TinyShell>) {
    let registry = window_registry();
    let mut guard = lock_recover(&registry, "window registry");
    if let Some(entry) = guard.iter_mut().find(|e| e.window_handle == window_handle) {
        entry.entity = entity;
    } else {
        guard.push(WindowEntry {
            window_handle,
            entity,
            screen_bounds: Bounds::default(),
            activation_seq: WINDOW_ACTIVATION_SEQ.fetch_add(1, Ordering::Relaxed),
        });
    }
}

/// Deregister a window when it closes and remove stale drag references.
pub(crate) fn deregister_window(window_handle: AnyWindowHandle, cx: &mut App) {
    let remaining = {
        let registry = window_registry();
        let mut guard = lock_recover(&registry, "window registry");
        guard.retain(|entry| entry.window_handle != window_handle);
        guard
            .iter()
            .map(|entry| entry.entity.clone())
            .collect::<Vec<_>>()
    };

    for entity in remaining {
        entity.update(cx, |window, cx| {
            if window
                .incoming_tab_drag
                .as_ref()
                .is_some_and(|drag| drag.source_window == window_handle)
            {
                window.incoming_tab_drag = None;
            }
            cx.notify();
        });
    }
}

pub(crate) fn mark_window_active(window_handle: AnyWindowHandle) {
    let registry = window_registry();
    if let Ok(mut guard) = registry.lock()
        && let Some(entry) = guard
            .iter_mut()
            .find(|entry| entry.window_handle == window_handle)
    {
        entry.activation_seq = WINDOW_ACTIVATION_SEQ.fetch_add(1, Ordering::Relaxed);
    }
}

/// Activate a target window after a cross-window operation and verify that the
/// platform accepted the foreground request. Windows can reject the first
/// request while the source window is still completing its mouse-up event.
pub(crate) fn activate_window_with_retry(
    window_handle: AnyWindowHandle,
    focus_handle: FocusHandle,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        const RETRY_DELAYS_MS: [u64; 4] = [0, 40, 80, 160];

        for delay_ms in RETRY_DELAYS_MS {
            if delay_ms > 0 {
                cx.background_executor()
                    .timer(Duration::from_millis(delay_ms))
                    .await;
            }

            if window_handle
                .update(cx, |_, window, cx| {
                    window.activate_window();
                    window.focus(&focus_handle, cx);
                })
                .is_err()
            {
                return;
            }

            cx.background_executor()
                .timer(Duration::from_millis(30))
                .await;
            match window_handle.update(cx, |_, window, _| window.is_window_active()) {
                Ok(true) => return,
                Ok(false) => {}
                Err(_) => return,
            }
        }

        tracing::warn!("[ui] target window did not become active after retries");
    })
    .detach();
}

/// Update the stored screen bounds for `window_handle`.
pub(crate) fn update_window_bounds(window_handle: AnyWindowHandle, bounds: Bounds<Pixels>) {
    let registry = window_registry();
    if let Ok(mut guard) = registry.lock() {
        if let Some(entry) = guard.iter_mut().find(|e| e.window_handle == window_handle) {
            entry.screen_bounds = bounds;
        }
    }
}

/// Find another window (other than `exclude`) whose screen bounds contain
/// `screen_pos`. Returns the target's entity and a clone of its bounds.
pub(crate) fn find_window_at_screen_pos(
    exclude: &AnyWindowHandle,
    screen_pos: Point<Pixels>,
) -> Option<(AnyWindowHandle, Entity<TinyShell>, Bounds<Pixels>)> {
    let registry = window_registry();
    let guard = lock_recover(&registry, "window registry");
    guard
        .iter()
        .filter(|entry| {
            &entry.window_handle != exclude && entry.screen_bounds.contains(&screen_pos)
        })
        .max_by_key(|entry| entry.activation_seq)
        .map(|entry| {
            (
                entry.window_handle,
                entry.entity.clone(),
                entry.screen_bounds,
            )
        })
}

pub(crate) fn clear_incoming_tab_drag_except(
    drag_id: u64,
    keep_window: Option<AnyWindowHandle>,
    cx: &mut App,
) {
    let targets = {
        let registry = window_registry();
        let guard = lock_recover(&registry, "window registry");
        guard
            .iter()
            .filter(|entry| keep_window != Some(entry.window_handle))
            .map(|entry| entry.entity.clone())
            .collect::<Vec<_>>()
    };

    for target in targets {
        target.update(cx, |target, cx| {
            if target
                .incoming_tab_drag
                .as_ref()
                .is_some_and(|drag| drag.drag_id == drag_id)
            {
                target.incoming_tab_drag = None;
                cx.notify();
            }
        });
    }
}

pub(crate) fn clear_all_incoming_tab_drags(cx: &mut App) {
    clear_tab_drag_hover();
    let targets = {
        let registry = window_registry();
        let guard = lock_recover(&registry, "window registry");
        guard
            .iter()
            .map(|entry| entry.entity.clone())
            .collect::<Vec<_>>()
    };

    for target in targets {
        target.update(cx, |target, cx| {
            if target.incoming_tab_drag.take().is_some() {
                cx.notify();
            }
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug)]
pub(crate) enum PaneLayout {
    Empty,
    Single(String),
    Horizontal(Vec<PaneLayout>, f32), // children, split_ratio (0.0-1.0)
    Vertical(Vec<PaneLayout>, f32),   // children, split_ratio (0.0-1.0)
}

#[derive(Clone)]
pub(crate) struct TabGroup {
    pub(crate) id: String,
    /// Stable identity for drag sessions; it must not change on every render.
    pub(crate) drag_id: u64,
    /// Monotonic per-window label used to distinguish similarly named hosts.
    pub(crate) ordinal: u64,
    pub(crate) title: String,
    pub(crate) pane_root: PaneLayout,
    pub(crate) sftp: Option<crate::terminal::SftpUiState>,
}

#[derive(Clone)]
pub(crate) struct SystemInfoTab {
    pub(crate) id: String,
    pub(crate) source_tab_id: String,
    pub(crate) title: String,
}

impl PaneLayout {
    pub fn tab_ids(&self) -> Vec<&str> {
        match self {
            PaneLayout::Empty => Vec::new(),
            PaneLayout::Single(id) => vec![id.as_str()],
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                children.iter().flat_map(|c| c.tab_ids()).collect()
            }
        }
    }

    pub fn contains(&self, tab_id: &str) -> bool {
        match self {
            PaneLayout::Empty => false,
            PaneLayout::Single(id) => id == tab_id,
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                children.iter().any(|c| c.contains(tab_id))
            }
        }
    }

    pub fn focused_tab_id(&self, path: &[usize]) -> Option<&str> {
        match self {
            PaneLayout::Empty => None,
            PaneLayout::Single(id) if path.is_empty() => Some(id.as_str()),
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                let (&first, rest) = path.split_first()?;
                children.get(first).and_then(|c| c.focused_tab_id(rest))
            }
            _ => None,
        }
    }

    pub fn replace_at(&mut self, path: &[usize], replacement: PaneLayout) {
        match (self, path) {
            (this @ PaneLayout::Empty, []) | (this @ PaneLayout::Single(_), []) => {
                *this = replacement
            }
            (
                PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _),
                [first, rest @ ..],
            ) => {
                if let Some(child) = children.get_mut(*first) {
                    child.replace_at(rest, replacement);
                }
            }
            _ => {}
        }
    }

    pub fn remove_tab(&mut self, tab_id: &str) -> bool {
        match self {
            PaneLayout::Empty => false,
            PaneLayout::Single(id) if id == tab_id => {
                *self = PaneLayout::Empty;
                true
            }
            PaneLayout::Single(_) => false,
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                let mut changed = false;
                for child in children.iter_mut() {
                    changed |= child.remove_tab(tab_id);
                }
                children.retain(|child| !matches!(child, PaneLayout::Empty));
                match children.len() {
                    0 => *self = PaneLayout::Empty,
                    1 => {
                        if let Some(replacement) = children.pop() {
                            *self = replacement;
                        }
                    }
                    _ => {}
                }
                changed
            }
        }
    }

    pub fn total_panes(&self) -> usize {
        match self {
            PaneLayout::Empty => 0,
            PaneLayout::Single(_) => 1,
            PaneLayout::Horizontal(children, _) | PaneLayout::Vertical(children, _) => {
                children.iter().map(|c| c.total_panes()).sum()
            }
        }
    }
}

pub(crate) struct TerminalScrollbarState {
    line_height: Pixels,
    total_lines: usize,
    viewport_lines: usize,
    display_offset: usize,
}

#[derive(Clone, Default)]
pub(crate) struct TerminalScrollbarHandle {
    state: Rc<RefCell<Option<TerminalScrollbarState>>>,
    pub(crate) future_display_offset: Rc<Cell<Option<usize>>>,
}

impl TerminalScrollbarHandle {
    pub(crate) fn update(&self, snapshot: &terminal::RenderSnapshot, line_height: Pixels) {
        self.state.replace(Some(TerminalScrollbarState {
            line_height,
            total_lines: snapshot.history_size + snapshot.rows,
            viewport_lines: snapshot.rows,
            display_offset: snapshot.display_offset,
        }));
    }
}

impl ScrollbarHandle for TerminalScrollbarHandle {
    fn offset(&self) -> Point<Pixels> {
        let state_ref = self.state.borrow();
        let Some(state) = state_ref.as_ref() else {
            return point(px(0.), px(0.));
        };
        let scroll_offset = state
            .total_lines
            .saturating_sub(state.viewport_lines)
            .saturating_sub(state.display_offset);
        point(px(0.), -(scroll_offset as f32 * state.line_height))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let state_ref = self.state.borrow();
        let Some(state) = state_ref.as_ref() else {
            return;
        };
        let offset_delta = (offset.y / state.line_height).round() as i32;
        let max_offset = state.total_lines.saturating_sub(state.viewport_lines);
        let display_offset = (max_offset as i32 + offset_delta).clamp(0, max_offset as i32);
        self.future_display_offset
            .set(Some(display_offset as usize));
    }

    fn content_size(&self) -> Size<Pixels> {
        let state_ref = self.state.borrow();
        let Some(state) = state_ref.as_ref() else {
            return size(px(0.), px(0.));
        };
        size(
            px(0.),
            state.total_lines.max(state.viewport_lines) as f32 * state.line_height,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyncSecretsPasswordDialogStatus {
    AwaitingInput,
    Verifying,
    PasswordMismatch,
    PasswordRequired,
    RemotePasswordNotConfigured,
    Failed,
}

pub(crate) struct SyncSecretsPasswordDialogState {
    pub(crate) token: DialogToken,
    pub(crate) status: SyncSecretsPasswordDialogStatus,
    pub(crate) message: Option<SharedString>,
    pub(crate) window: AnyWindowHandle,
    pub(crate) settings_password_input: Entity<InputState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogOpenResult {
    Opened,
    Queued,
    Ignored,
}

pub(crate) type DialogBuilder = Box<dyn Fn(Dialog, DialogToken, &mut Window, &mut App) -> Dialog>;

pub(crate) struct PendingDialog {
    pub(crate) kind: DialogKind,
    pub(crate) token: DialogToken,
    pub(crate) builder: DialogBuilder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogKind {
    Updater,
    SessionSelector,
    Transfers,
    NewSsh,
    ManagedKeySelector,
    ManagedKeyImport,
    ConnectionGroup,
    QuickCommandCategory,
    QuickCommand,
    /// 校验隐私密码后才允许启用敏感信息同步。
    VerifySyncSecretsPassword,
    /// 上传预检发现远端敏感字段无法解密。
    SyncUploadSecretsBlocked,
    /// 本地强行重置隐私信息加密密码。
    ResetPrivacyPassword,
    DeleteConfirmation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HomePage {
    #[default]
    Overview,
    Connections,
    Commands,
    KeyManager,
    Settings,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SftpPanelView {
    #[default]
    Files,
    Commands,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SftpPanelState {
    pub(crate) view: SftpPanelView,
    pub(crate) minimized: bool,
    pub(crate) minimize_epoch: u64,
    pub(crate) show_hidden_files: bool,
}

pub(crate) struct SftpWorkspaceState {
    pub(crate) path_input: Entity<InputState>,
    pub(crate) new_folder_input: Entity<InputState>,
    pub(crate) remote_files_scroll_handle: UniformListScrollHandle,
    pub(crate) tree_scroll_handle: gpui::ScrollHandle,
    pub(crate) tree_scroll_target_bounds: Option<(String, Bounds<Pixels>)>,
    pub(crate) file_panels: Entity<ResizableState>,
    pub(crate) delete_scroll_handle: gpui::ScrollHandle,
    pub(crate) pending_path_sync: Option<String>,
    pub(crate) pending_tree_scroll_path: Option<String>,
    pub(crate) context_menu: Option<SftpContextMenuState>,
    pub(crate) creating_folder: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ProcessView {
    #[default]
    Memory,
    Cpu,
    Activity,
}

fn is_legacy_seeded_quick_commands(categories: &[QuickCommandCategory]) -> bool {
    const COMMANDS: [&[&str]; 4] = [
        &["pwd", "ls -lah", "df -h", "uptime"],
        &[
            "free -h",
            "ps aux --sort=-%cpu | head -n 20",
            "ss -lntup",
            "journalctl -n 100 --no-pager",
        ],
        &[
            "docker ps -a",
            "docker images",
            "docker stats --no-stream",
            "docker system df",
        ],
        &[
            "ip address",
            "ip route",
            "cat /etc/resolv.conf",
            "curl -fsSL https://api.ipify.org && echo",
        ],
    ];
    let names_match = categories
        .iter()
        .map(|category| category.name.as_str())
        .eq(["常用", "系统", "Docker", "网络"])
        || categories
            .iter()
            .map(|category| category.name.as_str())
            .eq(["Common", "System", "Docker", "Network"]);
    names_match
        && categories.len() == COMMANDS.len()
        && categories.iter().zip(COMMANDS).all(|(category, expected)| {
            category
                .commands
                .iter()
                .map(|command| command.command.as_str())
                .eq(expected.iter().copied())
        })
}

#[derive(Default)]
pub(crate) struct NetworkHistory {
    pub(crate) receive: VecDeque<f32>,
    pub(crate) transmit: VecDeque<f32>,
}

/// Input fields shared by the SSH/quick-connection forms.
///
/// Grouping these together keeps `TinyShell::new` focused on wiring rather than
/// repetitive `InputState` construction.
pub(crate) struct ConnectionFormInputs {
    pub(crate) host_input: Entity<InputState>,
    pub(crate) session_name_input: Entity<InputState>,
    pub(crate) connection_group_input: Entity<InputState>,
    pub(crate) port_input: Entity<InputState>,
    pub(crate) user_input: Entity<InputState>,
    pub(crate) password_input: Entity<InputState>,
    pub(crate) key_path_input: Entity<InputState>,
    pub(crate) key_inline_input: Entity<InputState>,
    pub(crate) passphrase_input: Entity<InputState>,
    pub(crate) key_import_remark_input: Entity<InputState>,
    pub(crate) key_import_passphrase_input: Entity<InputState>,
    pub(crate) proxy_host_input: Entity<InputState>,
    pub(crate) proxy_port_input: Entity<InputState>,
    pub(crate) proxy_user_input: Entity<InputState>,
    pub(crate) proxy_password_input: Entity<InputState>,
}

impl ConnectionFormInputs {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<TinyShell>) -> Self {
        Self {
            host_input: cx.new(|cx| InputState::new(window, cx).placeholder(t!("host"))),
            session_name_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("session_name_placeholder").to_string())
            }),
            connection_group_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("connection_group_name"))),
            port_input: cx.new(|cx| InputState::new(window, cx).default_value("22")),
            user_input: cx.new(|cx| InputState::new(window, cx).default_value("root")),
            password_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("password"))
                    .masked(true)
            }),
            key_path_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("private_key_path_placeholder").to_string())
            }),
            key_inline_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(5)
                    .placeholder(t!("private_key_data_placeholder").to_string())
            }),
            passphrase_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("ssh_passphrase_placeholder").to_string())
                    .masked(true)
            }),
            key_import_remark_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("key_import_remark_placeholder").to_string())
            }),
            key_import_passphrase_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("key_passphrase").to_string())
                    .masked(true)
            }),
            proxy_host_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("proxy_host").to_string())),
            proxy_port_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("proxy_port").to_string())),
            proxy_user_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("proxy_user").to_string())),
            proxy_password_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(t!("proxy_password").to_string())
                    .masked(true)
            }),
        }
    }

    pub(crate) fn all_inputs(&self) -> [&Entity<InputState>; 15] {
        [
            &self.host_input,
            &self.session_name_input,
            &self.connection_group_input,
            &self.port_input,
            &self.user_input,
            &self.password_input,
            &self.key_path_input,
            &self.key_inline_input,
            &self.passphrase_input,
            &self.key_import_remark_input,
            &self.key_import_passphrase_input,
            &self.proxy_host_input,
            &self.proxy_port_input,
            &self.proxy_user_input,
            &self.proxy_password_input,
        ]
    }
}

pub(crate) struct TinyShell {
    pub(crate) focus_handle: FocusHandle,
    pub(crate) selector_focus_handle: FocusHandle,
    pub(crate) connection_inputs: ConnectionFormInputs,
    pub(crate) key_import: KeyImportState,
    pub(crate) managed_key_dialog_selection: Option<String>,
    pub(crate) managed_key_dialog_token: Option<DialogToken>,
    pub(crate) selector_dialog_token: Option<DialogToken>,
    pub(crate) connection_group_dialog_token: Option<DialogToken>,
    pub(crate) managed_key_editor_target:
        Option<Entity<connection_manager::ssh_editor_window::SshEditorWindow>>,
    pub(crate) ssh_proxy_type: String,
    pub(crate) global_proxy_type: String,
    pub(crate) settings_inputs: SettingsInputs,
    pub(crate) sync_runtime: SyncRuntimeState,
    pub(crate) ssh_auth_method: AuthMethod,
    pub(crate) ssh_config_entries: Vec<SshConfigEntry>,
    pub(crate) ssh_config_selected: Option<usize>,
    pub(crate) editing_session_id: Option<String>,
    pub(crate) editing_connection_group: Option<String>,
    pub(crate) editing_quick_command_category: Option<String>,
    pub(crate) connection_group_parent: Option<String>,
    pub(crate) session_group_selection: Option<String>,
    /// Managed SSH keys cache (mirrors ConfigStore for UI rendering).
    pub(crate) managed_keys: Vec<ManagedKey>,
    /// Selected managed key id in the SSH connection form.
    pub(crate) managed_key_selected: Option<String>,
    /// Whether the SSH form is using a custom key path (not a managed key).
    pub(crate) using_custom_key_path: bool,
    /// ID of the managed key being renamed in settings (None = not editing).
    pub(crate) editing_managed_key_id: Option<String>,
    pub(crate) follow_system_theme: bool,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) light_theme_name: SharedString,
    pub(crate) dark_theme_name: SharedString,
    pub(crate) ui_font_size: f32,
    pub(crate) terminal_font_size: f32,
    pub(crate) terminal_display_style: TerminalDisplayStyle,
    pub(crate) terminal_zoom_accumulator: f32,
    pub(crate) ui_font_family: SharedString,
    pub(crate) terminal_font_family: SharedString,
    pub(crate) session_store: Entity<crate::session::store::SessionStore>,
    pub(crate) session_owner_id: crate::session::store::WindowOwnerId,
    pub(crate) window_state: WindowState,
    pub(crate) pending_dialog: Option<PendingDialog>,
    pub(crate) home_page_open: bool,
    pub(crate) home_page: HomePage,
    pub(crate) prev_home_page: HomePage,
    pub(crate) home_page_epoch: u64,
    pub(crate) connection_group_filter: Option<String>,
    pub(crate) command_category_filter: Option<String>,
    pub(crate) selected_quick_command: Option<(String, String)>,
    pub(crate) quick_command_parameter_inputs: Vec<Entity<InputState>>,
    pub(crate) terminal_completions: HashMap<String, terminal_completion::TerminalCompletionState>,
    /// Bounds of the visible group rows, used to calculate a drop position.
    pub(crate) connection_group_bounds: HashMap<String, Bounds<Pixels>>,
    pub(crate) pending_connection_group_drag: Option<(String, Point<Pixels>)>,
    pub(crate) dragging_connection_group: Option<String>,
    pub(crate) connection_group_drop_before: Option<String>,
    pub(crate) selector_selection: usize,
    pub(crate) workspace_panels: Entity<ResizableState>,
    pub(crate) body_panels: Entity<ResizableState>,
    pub(crate) is_layout_reset: bool,
    pub(crate) terminal_scrollbars: HashMap<String, TerminalScrollbarHandle>,
    pub(crate) command_manager_scroll_handle: gpui::ScrollHandle,
    pub(crate) disk_scroll_handle: gpui::ScrollHandle,
    pub(crate) tabs_scroll_handle: gpui::ScrollHandle,
    pub(crate) selector_scroll_handle: gpui::ScrollHandle,
    pub(crate) quick_connection_scroll_handle: gpui::ScrollHandle,
    pub(crate) saved_scroll_handle: gpui::ScrollHandle,
    pub(crate) connection_scroll_handle: gpui::ScrollHandle,
    /// Increments each time a context menu is opened, used as an animation
    /// epoch so the menu fade-in restarts on every open.
    pub(crate) context_menu_epoch: u64,
    /// Increments each time a tab becomes disconnected, used as an animation
    /// epoch so the reconnect bar fade-in restarts on every disconnect.
    pub(crate) disconnect_epoch: u64,
    pub(crate) sftp_workspace: SftpWorkspaceState,
    pub(crate) sftp_panel: SftpPanelState,
    pub(crate) transfers: Vec<crate::terminal::Transfer>,
    pub(crate) transfer_manager: transfer_manager::TransferManager,
    pub(crate) terminal_panel_bounds: Option<Bounds<Pixels>>,
    pub(crate) terminal_bounds: HashMap<String, Bounds<Pixels>>,
    pub(crate) tab_bar_bounds: Option<Bounds<Pixels>>,
    pub(crate) tab_group_bounds: HashMap<String, Bounds<Pixels>>,
    pub(crate) terminal_selecting: bool,
    pub(crate) dragging_splitter: Option<(Vec<usize>, usize)>, // (parent_path, child_index)
    pub(crate) drag_split_origin: Option<gpui::Point<Pixels>>,
    // Tab drag state
    pub(crate) tab_drag: tab_drag::TabDragState,
    /// Source drag currently hovering over this window.
    pub(crate) incoming_tab_drag: Option<IncomingTabDrag>,
    pub(crate) terminal_marked_text: Option<String>,
    pub(crate) quick_command_category: usize,
    pub(crate) workspace_mode: WorkspaceMode,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) collapsed_saved_scroll_handle: gpui::ScrollHandle,
    pub(crate) status: SharedString,
    pub(crate) config: ConfigStore,
    pub(crate) config_repository: Arc<config_persistence::ConfigRepository>,
    pub(crate) config_persistence: ConfigPersistenceState,
    pub(crate) active_title_bar_style: crate::session::config::TitleBarStyle,
    pub(crate) cursor_style: crate::session::config::CursorStyle,
    pub(crate) recording_action: Option<String>,
    pub(crate) auxiliary_windows: AuxiliaryWindowsState,
    pub(crate) update_runtime: UpdateRuntimeState,
    /// Error message when a recorded keybinding conflicts with another
    pub(crate) keybind_error: Option<(String, String)>, // (action_id, error_message)
    pub(crate) monitoring: MonitoringState,
    pub(crate) tool_panel: tool_panel::ToolPanelState,
    pub(crate) docker_search_input: Entity<InputState>,
    pub(crate) connection_manager_state: Entity<ConnectionManagerState>,
    pub(crate) connection_manager_actions: ConnectionManagerActions,
    pub(crate) search_input: Entity<InputState>,

    pub(crate) system_tab_id: Option<String>,
    pub(crate) sftp_handles: std::collections::HashMap<String, crate::sftp::SftpHandle>,

    pub(crate) runtime: Arc<Runtime>,
    pub(crate) async_runtime: AsyncRuntimeState,
    pub(crate) pending_close_window: Option<AnyWindowHandle>,
    pub(crate) close_prompt_open: bool,
    pub(crate) close_sync_requested: bool,
    pub(crate) close_sync_running: bool,
    pub(crate) close_sync_completed: bool,
    pub(crate) close_finalized: bool,
    pub(crate) window_lease: Option<config_persistence::WindowLeaseId>,
    pub(crate) last_window_size: Option<gpui::Size<Pixels>>,
    pub(crate) last_registered_window_bounds: Option<Bounds<Pixels>>,
    pub(crate) was_window_active: bool,
    pub(crate) last_sidebar_width: Option<Pixels>,
    pub(crate) should_move_window: bool,
    pub(crate) hovered_url: Option<HoveredUrl>,
    pub(crate) cmd_ctrl_pressed: bool,
    pub(crate) _subscriptions: Vec<gpui::Subscription>,
}

impl TinyShell {
    pub(crate) fn workspace(&self) -> &crate::app::terminal_workspace::TerminalWorkspaceState {
        self.window_state.workspace()
    }

    pub(crate) fn window_state_mut(&mut self) -> &mut WindowState {
        &mut self.window_state
    }

    pub(crate) fn open_dialog<F>(
        &mut self,
        kind: DialogKind,
        window: &mut Window,
        cx: &mut Context<Self>,
        builder: F,
    ) -> DialogOpenResult
    where
        F: Fn(Dialog, DialogToken, &mut Window, &mut App) -> Dialog + 'static,
    {
        if self.window_state.is_same_active_dialog(kind) {
            return DialogOpenResult::Ignored;
        }
        let token = self.window_state.request_dialog(kind);
        self.pending_dialog = Some(PendingDialog {
            kind,
            token,
            builder: Box::new(builder),
        });
        if self.window_state.active_request().is_some() {
            DialogOpenResult::Queued
        } else if self.open_pending_dialog(window, cx) {
            DialogOpenResult::Opened
        } else {
            DialogOpenResult::Queued
        }
    }

    pub(crate) fn replace_dialog<F>(
        &mut self,
        kind: DialogKind,
        window: &mut Window,
        cx: &mut Context<Self>,
        builder: F,
    ) -> DialogOpenResult
    where
        F: Fn(Dialog, DialogToken, &mut Window, &mut App) -> Dialog + 'static,
    {
        let active = self.window_state.active_request();
        let token = self.window_state.request_dialog(kind);
        self.pending_dialog = Some(PendingDialog {
            kind,
            token,
            builder: Box::new(builder),
        });
        if let Some(active) = active {
            self.dialog_closed(active.token);
            window.close_dialog(cx);
            DialogOpenResult::Queued
        } else if self.open_pending_dialog(window, cx) {
            DialogOpenResult::Opened
        } else {
            DialogOpenResult::Queued
        }
    }

    pub(crate) fn open_pending_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pending) = self.pending_dialog.as_ref() else {
            return false;
        };
        let token = pending.token;
        if self.window_state.active_request().is_some()
            || self.window_state.pending_token() != Some(token)
        {
            return false;
        }
        let Some(activated) = self.window_state.activate_dialog(token).token() else {
            return false;
        };
        let Some(pending) = self.pending_dialog.take() else {
            return false;
        };
        window.open_dialog(cx, move |dialog, window, cx| {
            (pending.builder)(dialog, activated, window, cx)
        });
        self.record_dialog_token(pending.kind, activated);
        true
    }

    fn record_dialog_token(&mut self, kind: DialogKind, token: DialogToken) {
        match kind {
            DialogKind::ManagedKeySelector | DialogKind::ManagedKeyImport => {
                self.managed_key_dialog_token = Some(token)
            }
            DialogKind::SessionSelector => self.selector_dialog_token = Some(token),
            DialogKind::ConnectionGroup => self.connection_group_dialog_token = Some(token),
            DialogKind::VerifySyncSecretsPassword => {
                if let Some(state) = self.sync_runtime.secrets_password_dialog.as_mut() {
                    state.token = token;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn dialog_kind(&self) -> Option<DialogKind> {
        self.window_state.dialog_kind()
    }

    pub(crate) fn dialog_closed(&mut self, token: DialogToken) -> bool {
        self.window_state.dialog_closed(token)
    }

    pub(crate) fn dismiss_dialog(
        &mut self,
        token: DialogToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.dialog_closed(token) {
            return false;
        }
        window.close_dialog(cx);
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HoveredUrl {
    pub(crate) url: String,
    pub(crate) tab_id: String,
    pub(crate) cells: Vec<(usize, usize)>,
}

#[derive(Clone)]
pub(crate) enum SelectorEntry {
    Local,
    NewSsh,
    Saved(String),
}

#[derive(Clone)]
pub(crate) struct SftpContextMenuState {
    pub(crate) remote_path: Option<String>,
    pub(crate) is_dir: bool,
    pub(crate) permissions: Option<u32>,
    pub(crate) position: Point<Pixels>,
}

impl TinyShell {
    pub(crate) fn terminal_tab(&self, tab_id: &str) -> Option<&TerminalTab> {
        self.workspace().terminal_tab(tab_id)
    }

    pub(crate) fn terminal_tab_mut(&mut self, tab_id: &str) -> Option<&mut TerminalTab> {
        self.window_state
            .workspace_state_mut()
            .terminal_tab_mut(tab_id)
    }

    pub(crate) fn tab_group(&self, group_id: &str) -> Option<&TabGroup> {
        self.workspace().tab_group(group_id)
    }

    pub(crate) fn tab_group_mut(&mut self, group_id: &str) -> Option<&mut TabGroup> {
        self.window_state
            .workspace_state_mut()
            .tab_group_mut(group_id)
    }

    pub(crate) fn preferred_terminal_tab_id(&self) -> Option<String> {
        self.workspace().preferred_terminal_tab_id()
    }

    pub(crate) fn set_active_system_info_tab(&mut self, tab_id: Option<String>) {
        self.window_state
            .workspace_state_mut()
            .set_active_system_info_tab(tab_id);
    }

    pub(crate) fn allocate_tab_group_ordinal(&mut self) -> u64 {
        self.window_state
            .workspace_state_mut()
            .allocate_tab_group_ordinal()
    }

    pub(crate) fn install_terminal_tab(&mut self, tab: TerminalTab, group: TabGroup) {
        self.window_state
            .workspace_state_mut()
            .install_terminal_tab(tab, group);
        self.reset_sftp_tree_for_active_group();
    }

    pub(crate) fn terminal_tab_count(&self) -> usize {
        self.workspace().tab_count()
    }

    pub(crate) fn backend_events_sender(&self, cx: &mut Context<Self>) -> BackendEventSender {
        self.session_store.read(cx).events_sender()
    }

    pub(crate) fn register_backend_route(&self, route_id: String, cx: &mut Context<Self>) {
        let owner_id = self.session_owner_id;
        self.session_store.update(cx, |store, _| {
            store.register_event_route(route_id, owner_id);
        });
    }

    pub(crate) fn unregister_backend_route(&self, route_id: &str, cx: &mut Context<Self>) {
        let owner_id = self.session_owner_id;
        self.session_store.update(cx, |store, _| {
            store.unregister_event_route(route_id, owner_id);
        });
    }

    pub(crate) fn tab_title(&self, tab_id: &str) -> String {
        self.workspace()
            .tabs()
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.title.clone())
            .or_else(|| {
                self.workspace()
                    .tab_groups()
                    .iter()
                    .find(|group| group.id == tab_id)
                    .map(|group| group.title.clone())
            })
            .or_else(|| {
                self.workspace()
                    .tab_groups()
                    .iter()
                    .find(|group| group.pane_root.contains(tab_id))
                    .map(|group| group.title.clone())
            })
            .unwrap_or_else(|| "Unknown".to_string())
    }

    pub(crate) fn new(
        window: &mut Window,
        session_store: Entity<crate::session::store::SessionStore>,
        config_repository: Arc<config_persistence::ConfigRepository>,
        window_lease: config_persistence::WindowLeaseId,
        cx: &mut Context<Self>,
    ) -> Self {
        let connection_inputs = ConnectionFormInputs::new(window, cx);
        let sftp_path_input = cx.new(|cx| InputState::new(window, cx).default_value("/"));
        let sftp_new_folder_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("new_folder").to_string()));
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("search").to_string()));
        let docker_search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("docker_search_placeholder").to_string())
        });
        let connection_manager_state = cx.new(|_| ConnectionManagerState::default());
        let quick_command_parameter_inputs = (1..=5)
            .map(|index| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(t!("quick_command_parameter", index = index).to_string())
                })
            })
            .collect::<Vec<_>>();
        let mut config = ConfigStore::load().unwrap_or_else(|err| {
            tracing::warn!("failed to load config: {err:#}");
            ConfigStore::in_memory()
        });
        let settings_inputs = SettingsInputs::new(&config, window, cx);
        let mut _subscriptions = connection_inputs
            .all_inputs()
            .into_iter()
            .map(|input| cx.subscribe_in(input, window, Self::on_input_event))
            .collect::<Vec<_>>();
        _subscriptions.extend([
            cx.subscribe_in(&sftp_path_input, window, Self::on_input_event),
            cx.subscribe_in(&sftp_new_folder_input, window, Self::on_input_event),
            cx.subscribe_in(&search_input, window, Self::on_input_event),
            cx.subscribe_in(&docker_search_input, window, Self::on_input_event),
        ]);
        _subscriptions.extend(
            settings_inputs
                .all_inputs()
                .map(|input| cx.subscribe_in(&input, window, Self::on_input_event)),
        );

        let (events_tx, events_rx) = backend_event_channel();
        let workspace_panels = cx.new(|_| ResizableState::default());
        let body_panels = cx.new(|_| ResizableState::default());
        let system_sampler = shared_system_sampler();
        let system = match system_sampler.lock() {
            Ok(mut sampler) => sampler.sample().clone(),
            Err(poisoned) => {
                tracing::warn!("system sampler lock was poisoned; recovering its state");
                poisoned.into_inner().sample().clone()
            }
        };
        let default_light_theme_name = ThemeRegistry::global(cx).default_light_theme().name.clone();
        let default_dark_theme_name = ThemeRegistry::global(cx).default_dark_theme().name.clone();
        let follow_system_theme =
            theme::initialize_process_theme_preference(config.follow_system_theme());

        let theme_mode = match config.theme_mode() {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => ThemeMode::Light,
        };
        let light_theme_name = if config.light_theme_name().is_empty() {
            default_light_theme_name
        } else {
            config.light_theme_name().into()
        };
        let dark_theme_name = if config.dark_theme_name().is_empty() {
            default_dark_theme_name
        } else {
            config.dark_theme_name().into()
        };

        let configured_locale = config.locale();
        let mut active_locale = configured_locale.to_string();
        if active_locale == "system" {
            active_locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
            if active_locale.starts_with("zh") {
                active_locale = "zh-CN".to_string();
            } else {
                active_locale = "en".to_string();
            }
        }
        rust_i18n::set_locale(&active_locale);
        gpui_component::set_locale(&active_locale);
        docker_search_input.update(cx, |input, cx| {
            input.set_placeholder(t!("docker_search_placeholder").to_string(), window, cx);
        });
        if config.quick_commands_builtin_version() < BUILTIN_QUICK_COMMANDS_VERSION {
            let mut categories = config
                .quick_command_categories()
                .filter(|categories| !is_legacy_seeded_quick_commands(categories))
                .unwrap_or_default()
                .to_vec();
            merge_builtin_quick_commands(&mut categories, &active_locale);
            config.set_quick_command_categories(categories);
            config.set_quick_commands_builtin_version(BUILTIN_QUICK_COMMANDS_VERSION);
            if let Err(err) = crate::app::config_persistence::save_full(&config_repository, &config)
            {
                tracing::warn!("failed to initialize built-in quick commands: {err:#}");
            }
        }
        let ui_font_family: SharedString = config.ui_font_family().into();
        let terminal_font_family: SharedString = config.terminal_font_family().into();
        let sftp_panel_view = if config.sftp_panel_view() == "commands" {
            SftpPanelView::Commands
        } else {
            SftpPanelView::Files
        };
        let last_sidebar_width = Some(px(config
            .workspace_panels()
            .and_then(|s| s.first().copied())
            .unwrap_or(constants::SIDEBAR_WIDTH)));
        let runtime = shared_runtime().unwrap_or_else(|error| {
            tracing::error!("cannot initialize application runtime: {error}");
            // TinyShell cannot create backend sessions without a Tokio runtime;
            // abort explicitly instead of continuing with a partially usable UI.
            std::process::abort();
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            selector_focus_handle: cx.focus_handle(),
            connection_inputs,
            key_import: KeyImportState::default(),
            managed_key_dialog_selection: None,
            managed_key_dialog_token: None,
            selector_dialog_token: None,
            connection_group_dialog_token: None,
            managed_key_editor_target: None,
            ssh_proxy_type: "none".to_string(),
            global_proxy_type: config.global_proxy_type().to_string(),
            settings_inputs,
            sync_runtime: SyncRuntimeState::new(t!("sync_not_run").into()),
            ssh_auth_method: AuthMethod::Password,
            ssh_config_entries: crate::session::ssh_config::parse_ssh_config().unwrap_or_default(),
            ssh_config_selected: None,
            editing_session_id: None,
            editing_connection_group: None,
            editing_quick_command_category: None,
            connection_group_parent: None,
            session_group_selection: None,
            managed_keys: config.managed_keys().to_vec(),
            managed_key_selected: None,
            using_custom_key_path: false,
            editing_managed_key_id: None,
            follow_system_theme,
            theme_mode,
            light_theme_name,
            dark_theme_name,
            ui_font_size: config.ui_font_size(),
            terminal_font_size: config.terminal_font_size(),
            terminal_display_style: config.terminal_display_style(),
            terminal_zoom_accumulator: 0.0,
            cursor_style: config.cursor_style(),
            ui_font_family,
            terminal_font_family,
            session_store,
            session_owner_id: SESSION_OWNER_SEQ.fetch_add(1, Ordering::Relaxed),
            window_state: WindowState::new(),
            pending_dialog: None,
            home_page_open: true,
            home_page: HomePage::default(),
            prev_home_page: HomePage::default(),
            home_page_epoch: 0,
            connection_group_filter: None,
            command_category_filter: None,
            selected_quick_command: None,
            quick_command_parameter_inputs,
            terminal_completions: HashMap::new(),
            connection_group_bounds: HashMap::new(),
            pending_connection_group_drag: None,
            dragging_connection_group: None,
            connection_group_drop_before: None,
            terminal_panel_bounds: None,
            selector_selection: 0,
            workspace_panels,
            body_panels,
            is_layout_reset: false,
            terminal_scrollbars: HashMap::new(),
            command_manager_scroll_handle: gpui::ScrollHandle::new(),
            disk_scroll_handle: gpui::ScrollHandle::new(),
            tabs_scroll_handle: gpui::ScrollHandle::new(),
            selector_scroll_handle: gpui::ScrollHandle::new(),
            quick_connection_scroll_handle: gpui::ScrollHandle::new(),
            saved_scroll_handle: gpui::ScrollHandle::new(),
            connection_scroll_handle: gpui::ScrollHandle::new(),
            context_menu_epoch: 0,
            disconnect_epoch: 0,
            sftp_workspace: SftpWorkspaceState {
                path_input: sftp_path_input,
                new_folder_input: sftp_new_folder_input,
                remote_files_scroll_handle: UniformListScrollHandle::new(),
                tree_scroll_handle: gpui::ScrollHandle::new(),
                tree_scroll_target_bounds: None,
                file_panels: cx.new(|_| ResizableState::default()),
                delete_scroll_handle: gpui::ScrollHandle::new(),
                pending_path_sync: Some("/".into()),
                pending_tree_scroll_path: None,
                context_menu: None,
                creating_folder: false,
            },
            sftp_panel: SftpPanelState {
                view: sftp_panel_view,
                minimized: config.sftp_panel_minimized(),
                minimize_epoch: 0,
                show_hidden_files: config.show_hidden_files(),
            },
            transfers: {
                let mut transfers = config.transfers();
                for t in transfers.iter_mut() {
                    if matches!(
                        t.state,
                        crate::terminal::TransferState::Running
                            | crate::terminal::TransferState::Paused
                    ) {
                        t.state = if transfer_manager::TransferManager::is_resumable(t) {
                            crate::terminal::TransferState::Recoverable(
                                t!("recoverable_reason").to_string(),
                            )
                        } else {
                            crate::terminal::TransferState::Zombie(t!("zombie_reason").to_string())
                        };
                    }
                }
                transfers
            },
            transfer_manager: transfer_manager::TransferManager::new(),
            terminal_bounds: HashMap::new(),
            tab_bar_bounds: None,
            tab_group_bounds: HashMap::new(),
            terminal_selecting: false,
            terminal_marked_text: None,
            dragging_splitter: None,
            drag_split_origin: None,
            tab_drag: tab_drag::TabDragState::default(),
            incoming_tab_drag: None,
            quick_command_category: 0,
            workspace_mode: WorkspaceMode::default(),
            sidebar_collapsed: config.sidebar_collapsed(),
            collapsed_saved_scroll_handle: gpui::ScrollHandle::new(),
            status: "ready".into(),
            active_title_bar_style: config.title_bar_style(),
            config,
            config_repository,
            config_persistence: ConfigPersistenceState::default(),
            monitoring: MonitoringState {
                system_sampler,
                system: system.clone(),
                animated_cpu_percent: system.cpu_percent,
                animated_mem_percent: system.mem_percent,
                animated_swap_percent: system.swap_percent,
                process_view: ProcessView::default(),
                remote_system_snapshots: HashMap::new(),
                cpu_history: VecDeque::with_capacity(20),
                net_rx_history: VecDeque::with_capacity(20),
                net_tx_history: VecDeque::with_capacity(20),
                selected_network_interface: None,
                network_interface_histories: HashMap::new(),
                last_system_sample: Instant::now(),
                last_sftp_latency_sample: Instant::now(),
                system_status: None,
                remote_sample_in_flight: None,
                prev_monitoring_size: None,
            },
            tool_panel: tool_panel::ToolPanelState::default(),
            docker_search_input,
            recording_action: None,
            auxiliary_windows: AuxiliaryWindowsState::default(),
            update_runtime: UpdateRuntimeState::default(),
            keybind_error: None,

            search_input,
            connection_manager_state,
            connection_manager_actions: ConnectionManagerActions::default(),

            system_tab_id: None,
            sftp_handles: std::collections::HashMap::new(),

            runtime,
            async_runtime: AsyncRuntimeState::new(events_tx, events_rx),
            pending_close_window: None,
            close_prompt_open: false,
            close_sync_requested: false,
            close_sync_running: false,
            close_sync_completed: false,
            close_finalized: false,
            window_lease: Some(window_lease),
            last_window_size: None,
            last_registered_window_bounds: None,
            was_window_active: false,
            last_sidebar_width,
            should_move_window: false,
            hovered_url: None,
            cmd_ctrl_pressed: false,
            _subscriptions,
        };

        this.apply_theme_preferences(window, cx);
        // this.open_local(cx);
        this.start_event_pump(cx);
        this
    }

    pub(crate) fn on_input_event(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if input == &self.connection_inputs.key_import_passphrase_input {
            let passphrase = self
                .connection_inputs
                .key_import_passphrase_input
                .read(cx)
                .value()
                .to_string();
            self.key_import.revalidate(&passphrase, &self.managed_keys);
        } else if input == &self.settings_inputs.update.interval_hours {
            match event {
                InputEvent::Change => {
                    if let Some(hours) =
                        settings::actions::parse_hour_interval(input.read(cx).value().as_ref())
                    {
                        self.config.set_update_interval_hours(hours);
                        self.mark_config_preferences_dirty();
                        self.schedule_automatic_update_checks(window.window_handle(), false, cx);
                    }
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    let hours = self.config.update_interval_hours().to_string();
                    self.settings_inputs
                        .update
                        .interval_hours
                        .update(cx, |input, cx| input.set_value(hours, window, cx));
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
                _ => {}
            }
        } else if input == &self.settings_inputs.sync.interval_hours {
            match event {
                InputEvent::Change => {
                    if let Some(hours) =
                        settings::actions::parse_hour_interval(input.read(cx).value().as_ref())
                    {
                        self.config.set_sync_interval_hours(hours);
                        self.mark_config_preferences_dirty();
                        self.schedule_automatic_sync(false, cx);
                    }
                }
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    let hours = self.config.sync_interval_hours().to_string();
                    self.settings_inputs
                        .sync
                        .interval_hours
                        .update(cx, |input, cx| input.set_value(hours, window, cx));
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }
                _ => {}
            }
        } else if input == &self.sftp_workspace.path_input {
            if let InputEvent::PressEnter { .. } = event {
                let path = self
                    .sftp_workspace
                    .path_input
                    .read(cx)
                    .text()
                    .to_string()
                    .trim()
                    .to_string();
                self.navigate_sftp(if path.is_empty() { "/".into() } else { path }, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
        } else if input == &self.sftp_workspace.new_folder_input {
            match event {
                InputEvent::PressEnter { .. } => {
                    let name = self
                        .sftp_workspace
                        .new_folder_input
                        .read(cx)
                        .text()
                        .to_string();
                    if !name.is_empty() {
                        let base_path = self.sftp_workspace.path_input.read(cx).text().to_string();
                        let path = crate::sftp::join_remote(&base_path, &name);
                        if let Some(handle) = self.active_sftp_handle() {
                            handle.send_command(crate::sftp::SftpCommand::CreateDir(path));
                        }
                    }
                    self.sftp_workspace.creating_folder = false;
                    window.prevent_default();
                    cx.stop_propagation();
                }
                InputEvent::Blur => {
                    self.sftp_workspace.creating_folder = false;
                }
                _ => {}
            }
        } else if input == &self.search_input {
            if let InputEvent::PressEnter { .. } = event {
                if self.window_state.search_query.is_empty()
                    || *self.search_input.read(cx).text() != self.window_state.search_query
                {
                    self.perform_search(window, cx);
                } else {
                    self.search_goto_next(cx);
                }
                window.prevent_default();
                cx.stop_propagation();
            }
        } else if input == &self.connection_inputs.connection_group_input {
            if matches!(event, InputEvent::PressEnter { .. })
                && self.dialog_kind() == Some(DialogKind::ConnectionGroup)
            {
                if let Some(token) = self.connection_group_dialog_token.take() {
                    self.confirm_connection_group_dialog(token, window, cx);
                }
                window.prevent_default();
                cx.stop_propagation();
            }
        } else if input == &self.connection_inputs.key_inline_input {
            if matches!(event, InputEvent::PressEnter { .. })
                && let Some(key_id) = self.editing_managed_key_id.clone()
            {
                let name = self
                    .connection_inputs
                    .key_inline_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if !name.is_empty() {
                    self.rename_managed_key(key_id, name, cx);
                }
                window.prevent_default();
                cx.stop_propagation();
            }
        } else if input == &self.connection_inputs.key_import_remark_input {
            if matches!(event, InputEvent::PressEnter { .. })
                && self.editing_managed_key_id.is_some()
            {
                self.save_managed_key_rename(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
        } else if matches!(event, InputEvent::PressEnter { .. })
            && self.dialog_kind() == Some(DialogKind::NewSsh)
            && (input == &self.connection_inputs.session_name_input
                || input == &self.connection_inputs.host_input
                || input == &self.connection_inputs.port_input
                || input == &self.connection_inputs.user_input
                || input == &self.connection_inputs.password_input
                || input == &self.connection_inputs.key_path_input
                || input == &self.connection_inputs.passphrase_input)
        {
            self.connect_ssh(window, cx);
            window.prevent_default();
            cx.stop_propagation();
        }
        cx.notify();
    }

    pub(crate) fn start_event_pump(&mut self, cx: &mut Context<Self>) {
        let cancellation = self.async_runtime.supervisor.start("event-pump");
        let mut local_wake = self.async_runtime.events_tx.subscribe();
        let mut backend_wake = self.session_store.read(cx).events_sender().subscribe();
        cx.spawn(async move |this, cx| {
            let mut last_blink_time = std::time::Instant::now();
            let mut fallback_interval = Duration::from_millis(250);
            loop {
                if cancellation.is_cancelled() {
                    break;
                }
                let timer = cx.background_executor().timer(fallback_interval).fuse();
                let local_event = local_wake.changed().fuse();
                let backend_event = backend_wake.changed().fuse();
                pin_mut!(timer, local_event, backend_event);
                select_biased! {
                    _ = local_event => {},
                    _ = backend_event => {},
                    _ = timer => {},
                }
                if cancellation.is_cancelled() {
                    break;
                }
                let active = match this.update(cx, |this, cx| {
                    this.drive_config_preferences_save(cx);
                    let changed = this.drain_backend_events(cx);
                    let system_sampled = this.sample_system_if_due();
                    this.sample_sftp_latency_if_due();
                    let metrics_animated = this.animate_resource_metrics();
                    this.sync_theme_if_due(cx);
                    let is_blinking = matches!(
                        this.cursor_style,
                        crate::session::config::CursorStyle::Blink
                            | crate::session::config::CursorStyle::BeamBlink
                    );
                    let now = std::time::Instant::now();
                    let blink_due = is_blinking
                        && now.duration_since(last_blink_time)
                            >= std::time::Duration::from_millis(600);
                    if changed || system_sampled || metrics_animated || blink_due {
                        cx.notify();
                        if blink_due {
                            last_blink_time = now;
                        }
                    }
                    changed || system_sampled || metrics_animated || blink_due
                }) {
                    Ok(active) => active,
                    Err(_) => break,
                };
                fallback_interval = if active {
                    Duration::from_millis(16)
                } else {
                    Duration::from_millis(250)
                };
            }
        })
        .detach();
    }

    fn animate_resource_metrics(&mut self) -> bool {
        if !self.monitoring_metrics_visible() {
            self.monitoring.animated_cpu_percent = self.monitoring.system.cpu_percent;
            self.monitoring.animated_mem_percent = self.monitoring.system.mem_percent;
            self.monitoring.animated_swap_percent = self.monitoring.system.swap_percent;
            return false;
        }

        fn advance(current: &mut f32, target: f32) -> bool {
            let difference = target - *current;
            if difference.abs() < 0.0005 {
                if *current != target {
                    *current = target;
                    return true;
                }
                return false;
            }
            *current += difference * 0.12;
            true
        }

        let mut changed = advance(
            &mut self.monitoring.animated_cpu_percent,
            self.monitoring.system.cpu_percent,
        );
        changed |= advance(
            &mut self.monitoring.animated_mem_percent,
            self.monitoring.system.mem_percent,
        );
        changed |= advance(
            &mut self.monitoring.animated_swap_percent,
            self.monitoring.system.swap_percent,
        );
        changed
    }

    fn monitoring_metrics_visible(&self) -> bool {
        metrics_visible(MonitoringVisibilityContext {
            position: crate::app::settings::MonitoringPosition::from_config(
                self.config.monitoring_position(),
            ),
            sidebar_collapsed: self.sidebar_collapsed,
            system_info_open: self.workspace().active_system_info_tab_id().is_some(),
            active_tab_open: self.workspace().active_tab_id().is_some(),
            active_tab_is_ssh: self.active_kind() == Some(TabKind::Ssh),
            home_page_open: self.home_page_open,
        })
    }

    pub(crate) fn set_home_page(&mut self, page: HomePage, cx: &mut Context<Self>) {
        if self.home_page == page {
            return;
        }
        self.prev_home_page = self.home_page;
        self.home_page = page;
        self.home_page_epoch = self.home_page_epoch.wrapping_add(1);
        cx.notify();
    }

    pub(crate) fn main_view_key(&self) -> u64 {
        let mut hash = self.home_page as u64;
        hash = hash
            .wrapping_mul(31)
            .wrapping_add(self.home_page_open as u64);
        if let Some(id) = self.workspace().active_system_info_tab_id() {
            for byte in id.bytes() {
                hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
            }
        }
        if let Some(id) = self.workspace().active_tab_id() {
            for byte in id.bytes() {
                hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
            }
        }
        hash
    }

    pub(crate) fn drain_backend_events(&mut self, cx: &mut Context<Self>) -> bool {
        const MAX_EVENTS_PER_TICK: usize = 2_048;
        let mut changed = false;
        let mut transfers_changed = false;
        let mut transfers_force_persist = false;
        let mut events = Vec::with_capacity(MAX_EVENTS_PER_TICK);
        while events.len() < MAX_EVENTS_PER_TICK
            && let Ok(event) = self.async_runtime.events_rx.try_recv()
        {
            events.push(event);
        }
        let owner_id = self.session_owner_id;
        let remaining = MAX_EVENTS_PER_TICK.saturating_sub(events.len());
        let routed_events = self.session_store.update(cx, |store, _| {
            let events = store.drain_events_for(owner_id, remaining);
            let stats = store.queue_stats();
            if stats.deferred > 0
                || stats.rejected > 0
                || stats.pending > 0
                || stats.last_drain_micros > 1_000
            {
                tracing::debug!(
                    routed = stats.routed,
                    deferred = stats.deferred,
                    pending = stats.pending,
                    peak_pending = stats.peak_pending,
                    last_routed = stats.last_routed,
                    last_drained = stats.last_drained,
                    last_drain_micros = stats.last_drain_micros,
                    sent = stats.sent,
                    rejected = stats.rejected,
                    "backend event queue is under pressure"
                );
            }
            events
        });
        events.extend(routed_events);
        for envelope in coalesce_backend_events(events) {
            if let Some(tab) = self
                .workspace()
                .tabs()
                .iter()
                .find(|tab| envelope.event.tab_id().is_some_and(|id| id == tab.id))
                && envelope.generation != 0
                && envelope.generation != tab.backend_generation
            {
                continue;
            }
            let event = envelope.event;
            changed = true;
            match event {
                BackendEvent::Output { tab_id, bytes } => {
                    self.handle_terminal_output(tab_id, bytes, cx);
                }
                BackendEvent::Status { tab_id, text } => {
                    self.handle_terminal_status(tab_id, text, cx);
                }
                BackendEvent::Connected { tab_id } => {
                    self.handle_terminal_connected(tab_id, cx);
                }
                BackendEvent::TerminalTitleChanged { tab_id, title } => {
                    self.handle_terminal_title_changed(tab_id, title, cx);
                }
                BackendEvent::Closed { tab_id, reason } => {
                    if self.handle_terminal_closed(tab_id, reason, cx) {
                        continue;
                    }
                }
                BackendEvent::SftpEntries {
                    tab_id,
                    path,
                    entries,
                } => {
                    self.handle_sftp_entries(tab_id, path, entries, cx);
                }
                BackendEvent::SftpDirectoryEntries {
                    tab_id,
                    path,
                    entries,
                } => {
                    self.handle_sftp_directory_entries(tab_id, path, entries, cx);
                }
                BackendEvent::SftpStatus { tab_id, text } => {
                    self.handle_sftp_status(tab_id, text, cx);
                }
                BackendEvent::SftpLatency { tab_id, latency_ms } => {
                    self.handle_sftp_latency(tab_id, latency_ms, cx);
                }
                BackendEvent::SftpHome { tab_id, home } => {
                    self.handle_sftp_home(tab_id, home, cx);
                }
                BackendEvent::SftpFileContent {
                    tab_id,
                    remote_path,
                    file,
                } => {
                    self.handle_sftp_file_content(tab_id, remote_path, *file, cx);
                }
                BackendEvent::SftpContentUploaded {
                    tab_id,
                    remote_path,
                    revision,
                } => {
                    self.handle_sftp_content_uploaded(tab_id, remote_path, revision, cx);
                }
                BackendEvent::SftpContentConflict {
                    tab_id,
                    remote_path,
                    remote_file,
                } => {
                    self.handle_sftp_content_conflict(tab_id, remote_path, *remote_file, cx);
                }
                BackendEvent::SftpContentUploadFailed {
                    tab_id,
                    remote_path,
                    error,
                } => {
                    self.handle_sftp_content_upload_failed(tab_id, remote_path, error, cx);
                }
                BackendEvent::RemoteSystem { tab_id, snapshot } => {
                    self.handle_remote_system(tab_id, *snapshot, cx);
                }
                BackendEvent::RemoteSystemUnavailable { tab_id, reason } => {
                    self.handle_remote_system_unavailable(tab_id, reason, cx);
                }
                BackendEvent::DockerResult { tab_id, response } => {
                    self.handle_docker_response(tab_id, response, cx);
                }
                BackendEvent::TransferStarted { tab_id, info } => {
                    transfers_changed |= self.handle_transfer_started(tab_id, *info, cx);
                }
                BackendEvent::TransferProgress {
                    tab_id,
                    id,
                    transferred,
                    total,
                    state,
                } => {
                    transfers_force_persist |= matches!(
                        state,
                        crate::terminal::TransferState::Paused
                            | crate::terminal::TransferState::Completed
                            | crate::terminal::TransferState::Failed(_)
                            | crate::terminal::TransferState::Interrupted(_)
                            | crate::terminal::TransferState::Recoverable(_)
                            | crate::terminal::TransferState::Zombie(_)
                    );
                    transfers_changed |=
                        self.handle_transfer_progress(tab_id, id, transferred, total, state, cx);
                }
                BackendEvent::SyncFinished { result, task_id } => {
                    self.handle_sync_finished(*result, task_id, cx);
                }
            }
        }
        if transfers_changed
            && self
                .transfer_manager
                .should_persist(transfers_force_persist)
        {
            self.config.set_transfers(self.transfers.clone());
        }
        changed
    }

    fn handle_terminal_output(&mut self, tab_id: String, bytes: Vec<u8>, _cx: &mut Context<Self>) {
        if let Some(tab) = self.terminal_tab_mut(&tab_id) {
            tab.feed(&bytes);
        }
    }

    fn handle_terminal_status(&mut self, tab_id: String, text: String, _cx: &mut Context<Self>) {
        if let Some(tab) = self.terminal_tab_mut(&tab_id) {
            tab.status = text.clone();
        }
        self.status = text.into();
    }

    fn handle_terminal_connected(&mut self, tab_id: String, _cx: &mut Context<Self>) {
        if let Some(tab) = self.terminal_tab_mut(&tab_id) {
            tab.feed_status_line(&t!("connection_succeeded"));
            tab.connected = true;
            tab.disconnected_reason = None;
        }
        self.sync_system_tab_to_active_group();
        self.request_active_system_snapshot();
    }

    fn handle_terminal_title_changed(
        &mut self,
        tab_id: String,
        title: String,
        _cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.terminal_tab_mut(&tab_id) {
            tab.dynamic_title = title;
        }
        if self.workspace().active_tab_id() == Some(tab_id.as_str()) {
            let initially_synced = self.sync_initial_sftp_to_terminal_tab(&tab_id);
            if !initially_synced {
                self.sync_sftp_to_terminal_tab(&tab_id, true);
            }
        }
    }

    fn handle_terminal_closed(
        &mut self,
        tab_id: String,
        reason: String,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.monitoring.remote_sample_in_flight.as_deref() == Some(tab_id.as_str()) {
            self.monitoring.remote_sample_in_flight = None;
        }
        self.terminal_completions.remove(&tab_id);
        let was_manually_disconnected = self
            .workspace()
            .tabs()
            .iter()
            .find(|tab| tab.id == tab_id)
            .is_some_and(|tab| !tab.connected && tab.disconnected_reason.is_some());
        let is_graceful_exit = !was_manually_disconnected
            && (reason == "local shell closed" || reason == "ssh session closed");
        let editor_session = self
            .workspace()
            .tab_groups()
            .iter()
            .find(|group| group.pane_root.contains(&tab_id))
            .filter(|group| self.sftp_handles.contains_key(&group.id))
            .filter(|group| !is_graceful_exit || group.pane_root.total_panes() <= 1)
            .map(|group| group.id.clone());
        if let Some(session_id) = editor_session {
            sftp_editor_window::notify_connection_lost(&session_id, self.session_owner_id, cx);
        }
        if is_graceful_exit {
            self.handle_tab_close(tab_id.clone(), cx);
            self.status = reason.into();
            return true;
        }
        if !was_manually_disconnected && let Some(tab) = self.terminal_tab_mut(&tab_id) {
            let terminal_message = if tab.connected {
                t!("session_disconnected", "reason" = reason.clone()).to_string()
            } else {
                format!("{}: {reason}", t!("connection_failed"))
            };
            tab.feed_status_line(&terminal_message);
            tab.connected = false;
            tab.status = reason.clone();
            tab.disconnected_reason = Some(reason.clone());
            self.disconnect_epoch = self.disconnect_epoch.wrapping_add(1);
        }
        if self.system_tab_id.as_deref() == Some(tab_id.as_str()) {
            self.monitoring.system_status = Some(reason.clone().into());
        }
        self.status = reason.into();
        false
    }

    fn handle_sftp_entries(
        &mut self,
        tab_id: String,
        path: String,
        entries: Vec<crate::sftp::RemoteEntry>,
        _cx: &mut Context<Self>,
    ) {
        if let Some(group) = self.tab_group_mut(&tab_id) {
            if let Some(sftp) = group.sftp.as_mut() {
                sftp.directory_entries.insert(path.clone(), entries.clone());
                if sftp.current_path == path {
                    sftp.entries = entries;
                    if self.workspace().active_group_id() == Some(tab_id.as_str()) {
                        self.sftp_workspace.pending_path_sync = Some(path);
                    }
                }
            }
        }
    }

    fn handle_sftp_directory_entries(
        &mut self,
        tab_id: String,
        path: String,
        entries: Vec<crate::sftp::RemoteEntry>,
        _cx: &mut Context<Self>,
    ) {
        if let Some(group) = self.tab_group_mut(&tab_id) {
            if let Some(sftp) = group.sftp.as_mut() {
                sftp.directory_entries.insert(path, entries);
            }
        }
    }

    fn handle_sftp_status(&mut self, tab_id: String, text: String, _cx: &mut Context<Self>) {
        if let Some(group) = self.tab_group_mut(&tab_id) {
            if let Some(sftp) = group.sftp.as_mut() {
                sftp.status = text.clone();
            }
        }
        if self.workspace().active_group_id() == Some(tab_id.as_str()) {
            self.status = text.into();
        }
    }

    fn handle_sftp_latency(
        &mut self,
        tab_id: String,
        latency_ms: Option<u64>,
        _cx: &mut Context<Self>,
    ) {
        if let Some(group) = self.tab_group_mut(&tab_id)
            && let Some(sftp) = group.sftp.as_mut()
        {
            sftp.latency_ms = latency_ms;
        }
    }

    fn handle_sftp_home(&mut self, tab_id: String, home: String, _cx: &mut Context<Self>) {
        if let Some(group) = self.tab_group_mut(&tab_id) {
            if let Some(sftp) = group.sftp.as_mut() {
                sftp.home_dir = home.clone();
                sftp.current_path = home.clone();
                sftp.entries.clear();
                Self::expand_sftp_tree_to_path(sftp, &home);
                if self.workspace().active_group_id() == Some(tab_id.as_str()) {
                    self.sftp_workspace.pending_path_sync = Some(home.clone());
                    self.sftp_workspace.tree_scroll_target_bounds = None;
                    self.sftp_workspace.pending_tree_scroll_path = Some(home);
                }
            }
        }
        if let Some(terminal_tab_id) =
            self.workspace()
                .active_tab_id()
                .map(str::to_owned)
                .filter(|terminal_tab_id| {
                    self.workspace().tab_groups().iter().any(|group| {
                        group.id == tab_id && group.pane_root.contains(terminal_tab_id)
                    })
                })
        {
            self.sync_initial_sftp_to_terminal_tab(&terminal_tab_id);
        }
    }

    fn handle_sftp_file_content(
        &mut self,
        tab_id: String,
        remote_path: String,
        file: crate::sftp::text_file::RemoteTextFile,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.sftp_handles.get(&tab_id).cloned() {
            sftp_editor_window::open_or_focus(
                tab_id,
                self.session_owner_id,
                remote_path,
                file,
                handle,
                cx,
            );
        }
    }

    fn handle_sftp_content_uploaded(
        &mut self,
        tab_id: String,
        remote_path: String,
        revision: crate::sftp::text_file::RemoteFileRevision,
        cx: &mut Context<Self>,
    ) {
        sftp_editor_window::mark_uploaded(
            &tab_id,
            self.session_owner_id,
            &remote_path,
            revision,
            cx,
        );
    }

    fn handle_sftp_content_conflict(
        &mut self,
        tab_id: String,
        remote_path: String,
        remote_file: crate::sftp::text_file::RemoteTextFile,
        cx: &mut Context<Self>,
    ) {
        sftp_editor_window::mark_conflict(
            &tab_id,
            self.session_owner_id,
            &remote_path,
            remote_file,
            cx,
        );
    }

    fn handle_sftp_content_upload_failed(
        &mut self,
        tab_id: String,
        remote_path: String,
        error: String,
        cx: &mut Context<Self>,
    ) {
        sftp_editor_window::mark_upload_failed(
            &tab_id,
            self.session_owner_id,
            &remote_path,
            error,
            cx,
        );
    }

    fn handle_remote_system(
        &mut self,
        tab_id: String,
        snapshot: SystemSnapshot,
        _cx: &mut Context<Self>,
    ) {
        if self.monitoring.remote_sample_in_flight.as_deref() == Some(tab_id.as_str()) {
            self.monitoring.remote_sample_in_flight = None;
        }
        self.monitoring
            .remote_system_snapshots
            .insert(tab_id.clone(), snapshot.clone());
        if self.system_tab_id.as_deref() == Some(tab_id.as_str()) {
            self.record_network_interface_histories(&snapshot);
            self.monitoring.system_status = None;
            self.monitoring.system = snapshot.clone();
            push_bounded(&mut self.monitoring.cpu_history, snapshot.cpu_percent, 20);
            push_bounded(
                &mut self.monitoring.net_rx_history,
                snapshot.net_rx_rate as f32,
                20,
            );
            push_bounded(
                &mut self.monitoring.net_tx_history,
                snapshot.net_tx_rate as f32,
                20,
            );
        }
    }

    fn handle_remote_system_unavailable(
        &mut self,
        tab_id: String,
        reason: String,
        _cx: &mut Context<Self>,
    ) {
        if self.monitoring.remote_sample_in_flight.as_deref() == Some(tab_id.as_str()) {
            self.monitoring.remote_sample_in_flight = None;
        }
        if self.system_tab_id.as_deref() == Some(tab_id.as_str()) {
            self.monitoring.system_status = Some(reason.clone().into());
            self.status = reason.into();
        }
    }

    fn handle_transfer_started(
        &mut self,
        tab_id: String,
        info: crate::terminal::TransferInfo,
        _cx: &mut Context<Self>,
    ) -> bool {
        let tab_title = self.tab_title(&tab_id);
        self.transfers
            .retain(|transfer| transfer.info.id != info.id);
        self.transfers.insert(
            0,
            crate::terminal::Transfer {
                tab_id,
                tab_title,
                info,
                transferred: 0,
                total: None,
                state: crate::terminal::TransferState::Running,
            },
        );
        if self.transfers.len() > 100 {
            self.transfers.truncate(100);
        }
        true
    }

    fn handle_transfer_progress(
        &mut self,
        _tab_id: String,
        id: String,
        transferred: u64,
        total: Option<u64>,
        state: crate::terminal::TransferState,
        _cx: &mut Context<Self>,
    ) -> bool {
        if let Some(t) = self.transfers.iter_mut().find(|t| t.info.id == id) {
            t.transferred = transferred;
            if let Some(total) = total {
                t.total = Some(total);
            }
            t.state = state;
            true
        } else {
            false
        }
    }

    pub(crate) fn sample_system_if_due(&mut self) -> bool {
        if !self.monitoring_metrics_visible() {
            return false;
        }
        if self.monitoring.last_system_sample.elapsed() >= SystemSampler::interval() {
            self.monitoring.last_system_sample = Instant::now();
            // An SSH workspace must never fall back to sampling the local
            // machine, including while connecting, disconnected, or after a
            // transient remote probe failure.
            if let Some(ref tab_id) = self.system_tab_id.clone() {
                if let Some(tab) = self.terminal_tab(tab_id)
                    && tab.kind == TabKind::Ssh
                {
                    if tab.connected {
                        self.request_active_system_snapshot();
                    }
                    return false;
                }
            }
            let snapshot = match self.monitoring.system_sampler.lock() {
                Ok(mut sampler) => sampler.sample().clone(),
                Err(poisoned) => {
                    tracing::warn!("system sampler lock was poisoned; recovering its state");
                    poisoned.into_inner().sample().clone()
                }
            };
            self.record_network_interface_histories(&snapshot);
            let cpu_usage = snapshot.cpu_percent;
            push_bounded(&mut self.monitoring.cpu_history, cpu_usage, 20);
            push_bounded(
                &mut self.monitoring.net_rx_history,
                snapshot.net_rx_rate as f32,
                20,
            );
            push_bounded(
                &mut self.monitoring.net_tx_history,
                snapshot.net_tx_rate as f32,
                20,
            );
            self.monitoring.system = snapshot;
            return true;
        }
        false
    }

    fn sample_sftp_latency_if_due(&mut self) {
        if self.monitoring.last_sftp_latency_sample.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.monitoring.last_sftp_latency_sample = Instant::now();
        if !self.config.sftp_footer_visibility().latency {
            return;
        }
        if let Some(handle) = self.active_sftp_handle() {
            handle.measure_latency();
        }
    }

    fn record_network_interface_histories(&mut self, snapshot: &SystemSnapshot) {
        if self
            .monitoring
            .selected_network_interface
            .as_ref()
            .is_some_and(|selected| {
                !snapshot
                    .network_interfaces
                    .iter()
                    .any(|interface| &interface.name == selected)
            })
        {
            self.monitoring.selected_network_interface = None;
        }
        for interface in &snapshot.network_interfaces {
            let history = self
                .monitoring
                .network_interface_histories
                .entry(interface.name.clone())
                .or_default();
            push_bounded(&mut history.receive, interface.receive_rate as f32, 30);
            push_bounded(&mut history.transmit, interface.transmit_rate as f32, 30);
        }
    }

    pub(crate) fn sync_theme_if_due(&mut self, cx: &mut Context<Self>) {
        if theme::claim_system_theme_sync(Duration::from_secs(1)) {
            Theme::sync_system_appearance(None, cx);
            cx.refresh_windows();
        }
    }

    pub(crate) fn request_active_system_snapshot(&mut self) {
        let Some(ref tab_id) = self.system_tab_id.clone() else {
            return;
        };
        let Some(backend) = (|| {
            let tab = self.terminal_tab(tab_id)?;
            if !tab.connected {
                return None;
            }
            Some(tab.backend.clone())
        })() else {
            return;
        };
        if self.monitoring.remote_sample_in_flight.is_some() {
            return;
        }
        self.monitoring.remote_sample_in_flight = Some(tab_id.clone());
        if let Ok(backend) = backend.lock() {
            backend.send(crate::terminal::BackendCommand::SampleMetrics);
        }
    }

    pub(crate) fn terminal_ime_bounds_for_range(
        &self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        cell_width: f32,
        line_height: f32,
    ) -> Option<Bounds<Pixels>> {
        let snapshot = self.active_snapshot()?;
        let cursor = snapshot.cursor?;
        let x = element_bounds.origin.x
            + px(cell_width) * cursor.col as f32
            + px(cell_width) * range_utf16.start as f32;
        let y = element_bounds.origin.y + px(line_height) * cursor.row as f32;
        Some(Bounds::new(
            point(x, y),
            size(px(cell_width), px(line_height)),
        ))
    }

    pub(crate) fn sftp_handle_for_transfer(
        &self,
        transfer_id: &str,
    ) -> Option<&crate::sftp::SftpHandle> {
        self.transfers
            .iter()
            .find(|transfer| transfer.info.id == transfer_id)
            .and_then(|transfer| self.transfer_manager.handle_for_transfer(transfer))
    }

    pub(crate) fn remove_transfer(&mut self, transfer_id: &str, cx: &mut Context<Self>) {
        let cleanup = self
            .transfers
            .iter()
            .find(|transfer| transfer.info.id == transfer_id)
            .map(|transfer| {
                (
                    transfer.info.kind.clone(),
                    transfer.info.session_id.clone(),
                    transfer.info.partial_path.clone(),
                )
            });
        self.transfers.retain(|t| t.info.id != transfer_id);
        if let Some((kind, session_id, Some(partial_path))) = cleanup {
            match kind {
                crate::terminal::TransferType::Download => {
                    if let Err(error) = std::fs::remove_file(&partial_path)
                        && error.kind() != std::io::ErrorKind::NotFound
                    {
                        tracing::debug!(path = %partial_path, %error, "failed to clean local transfer partial");
                    }
                }
                crate::terminal::TransferType::Upload => {
                    if let Some(handle) = self.transfer_manager.handle_for_session(&session_id) {
                        handle.send_command(crate::sftp::SftpCommand::CleanupRemotePartial(
                            partial_path,
                        ));
                    }
                }
            }
        }
        self.config.set_transfers(self.transfers.clone());
        cx.notify();
    }

    /// Clean up all SSH sessions and SFTP handles when the window is closing.
    pub(crate) fn cleanup_on_window_close(&mut self) {
        let sync_handles = self.async_runtime.supervisor.cancel_all();
        if !sync_handles.is_empty() {
            self.runtime.spawn(async move {
                for handle in sync_handles {
                    let _ = handle.await;
                }
            });
        }
        tracing::info!(
            "[ui] cleaning up {} tabs and {} sftp handles on window close",
            self.workspace().tab_count(),
            self.sftp_handles.len()
        );

        // Send Close to all terminal backends (SSH channels and local PTY)
        for tab in self.workspace().tabs() {
            tab.send_backend(BackendCommand::Close);
        }

        // Close all SFTP handles
        for (_, handle) in self.sftp_handles.drain() {
            handle.close();
        }

        self.window_state_mut().workspace_state_mut().clear();
    }

    pub(crate) fn toggle_follow_terminal_cwd(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(active_group) = self.workspace().active_group_id().map(str::to_owned) else {
            return;
        };
        let enabled = self
            .tab_group_mut(&active_group)
            .and_then(|group| group.sftp.as_mut())
            .map(|sftp| {
                sftp.follow_terminal_cwd = !sftp.follow_terminal_cwd;
                sftp.follow_terminal_cwd
            });

        if enabled == Some(true)
            && let Some(active_tab) = self.workspace().active_tab_id().map(str::to_owned)
        {
            self.sync_sftp_to_terminal_tab(&active_tab, false);
        }
        cx.notify();
    }

    pub(crate) fn sync_sftp_to_terminal_tab(
        &mut self,
        tab_id: &str,
        require_follow_enabled: bool,
    ) -> bool {
        let Some(tab) = self.terminal_tab(tab_id) else {
            return false;
        };
        if tab.kind != TabKind::Ssh {
            return false;
        }
        let Some(group) = self
            .workspace()
            .tab_groups()
            .iter()
            .find(|group| group.pane_root.contains(tab_id))
        else {
            return false;
        };
        let Some(sftp) = group.sftp.as_ref() else {
            return false;
        };
        if require_follow_enabled && !sftp.follow_terminal_cwd {
            return false;
        }
        let Some(path) = Self::parse_path_from_title(&tab.dynamic_title, &sftp.home_dir) else {
            return false;
        };
        if sftp.current_path == path {
            return false;
        }

        let group_id = group.id.clone();
        self.navigate_sftp_group(&group_id, path)
    }

    pub(crate) fn sync_initial_sftp_to_terminal_tab(&mut self, tab_id: &str) -> bool {
        let Some(tab) = self.terminal_tab(tab_id) else {
            return false;
        };
        if tab.kind != TabKind::Ssh {
            return false;
        }
        let Some(group) = self
            .workspace()
            .tab_groups()
            .iter()
            .find(|group| group.pane_root.contains(tab_id))
        else {
            return false;
        };
        let Some(sftp) = group.sftp.as_ref() else {
            return false;
        };
        if sftp.initial_terminal_cwd_synced || sftp.home_dir.is_empty() {
            return false;
        }
        let Some(path) = Self::parse_path_from_title(&tab.dynamic_title, &sftp.home_dir) else {
            return false;
        };
        let group_id = group.id.clone();
        if !self.navigate_sftp_group(&group_id, path) {
            return false;
        }
        if let Some(sftp) = self
            .tab_group_mut(&group_id)
            .and_then(|group| group.sftp.as_mut())
        {
            sftp.initial_terminal_cwd_synced = true;
        }
        true
    }

    fn parse_path_from_title(title: &str, home_dir: &str) -> Option<String> {
        let title = title.strip_prefix("TINY_SHELL_CWD:").unwrap_or(title);
        let path_part = if let Some(pos) = title.find(':') {
            title[pos + 1..].trim()
        } else {
            title.trim()
        };

        if path_part.starts_with('/') {
            Some(path_part.to_string())
        } else if path_part == "~" {
            Some(home_dir.to_string())
        } else if let Some(rest) = path_part.strip_prefix("~/") {
            let home = home_dir.trim_end_matches('/');
            Some(format!("{}/{}", home, rest))
        } else {
            None
        }
    }

    pub(crate) fn mark_config_preferences_dirty(&mut self) {
        self.config_persistence.mark_dirty(Instant::now());
    }

    pub(crate) fn persist_config_preferences_checked(&mut self) -> anyhow::Result<()> {
        if !self.config_persistence.is_dirty() {
            return Ok(());
        }
        let generation = self.config_persistence.generation();
        config_persistence::persist_sync(&self.config_repository, &self.config)?;
        self.config_persistence.mark_saved(generation);
        Ok(())
    }

    pub(crate) fn persist_config_preferences_async(&mut self, cx: &mut Context<Self>) {
        self.config_persistence.request_immediate_save();
        self.drive_config_preferences_save(cx);
    }

    /// Persist a full configuration transaction off the UI thread and expose
    /// it to the window only after storage confirms the write. Only one full
    /// transaction per window is admitted at a time, which prevents two staged
    /// snapshots from committing out of order.
    pub(crate) fn commit_staged_config_async<Committed, Failed>(
        &mut self,
        staged: ConfigStore,
        on_committed: Committed,
        on_failed: Failed,
        cx: &mut Context<Self>,
    ) where
        Committed: FnOnce(&mut Self, &mut Context<Self>) + 'static,
        Failed: FnOnce(&mut Self, anyhow::Error, &mut Context<Self>) + 'static,
    {
        if !self.config_persistence.begin_full_commit() {
            on_failed(
                self,
                anyhow::anyhow!("another configuration save is still in progress"),
                cx,
            );
            return;
        }

        let preference_generation = self.config_persistence.generation();
        let receipt = match self.config_repository.save_full_async(staged.clone()) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.config_persistence.finish_full_commit();
                on_failed(self, error, cx);
                return;
            }
        };
        let wait_for_save = cx
            .background_executor()
            .spawn(async move { receipt.wait() });
        cx.spawn(async move |this, cx| {
            let result = wait_for_save.await;
            let _ = this.update(cx, move |this, cx| {
                this.config_persistence.finish_full_commit();
                match result {
                    Ok(()) => {
                        let mut committed = staged;
                        // Preferences may have changed while the full snapshot
                        // was being written. Keep those newer UI values in
                        // memory and let the generation driver persist them.
                        committed.merge_interactive_preferences_from(&this.config);
                        this.config = committed;
                        this.config_persistence.mark_saved(preference_generation);
                        this.note_local_config_saved(cx);
                        this.continue_queued_close_sync(cx);
                        on_committed(this, cx);
                    }
                    Err(error) => {
                        this.continue_queued_close_sync(cx);
                        on_failed(this, error, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// Window-aware counterpart of [`Self::commit_staged_config_async`] for
    /// dialogs that must only close after the configuration write succeeds.
    pub(crate) fn commit_staged_config_in_window_async<Committed, Failed>(
        &mut self,
        staged: ConfigStore,
        window: &mut Window,
        on_committed: Committed,
        on_failed: Failed,
        cx: &mut Context<Self>,
    ) where
        Committed: FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        Failed: FnOnce(&mut Self, anyhow::Error, &mut Window, &mut Context<Self>) + 'static,
    {
        if !self.config_persistence.begin_full_commit() {
            on_failed(
                self,
                anyhow::anyhow!("another configuration save is still in progress"),
                window,
                cx,
            );
            return;
        }

        let preference_generation = self.config_persistence.generation();
        let receipt = match self.config_repository.save_full_async(staged.clone()) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.config_persistence.finish_full_commit();
                on_failed(self, error, window, cx);
                return;
            }
        };
        let wait_for_save = cx
            .background_executor()
            .spawn(async move { receipt.wait() });
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = wait_for_save.await;
            let commit_result = this.update(cx, move |this, cx| {
                this.config_persistence.finish_full_commit();
                match result {
                    Ok(()) => {
                        let mut committed = staged;
                        committed.merge_interactive_preferences_from(&this.config);
                        this.config = committed;
                        this.config_persistence.mark_saved(preference_generation);
                        this.note_local_config_saved(cx);
                        this.continue_queued_close_sync(cx);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            });
            let Ok(commit_result) = commit_result else {
                return Ok::<(), anyhow::Error>(());
            };
            let _ = window_handle.update(cx, move |_, window, cx| {
                let _ = this.update(cx, move |this, cx| match commit_result {
                    Ok(()) => on_committed(this, window, cx),
                    Err(error) => on_failed(this, error, window, cx),
                });
            });
            Ok::<(), anyhow::Error>(())
        })
        .detach();
    }

    fn drive_config_preferences_save(&mut self, cx: &mut Context<Self>) {
        const PREFERENCE_SAVE_DEBOUNCE: Duration = Duration::from_millis(350);
        const PREFERENCE_SAVE_RETRY_DELAY: Duration = Duration::from_secs(2);

        let now = Instant::now();
        if let Some((generation, result)) = self.config_persistence.poll_result() {
            match result {
                Ok(()) => {
                    self.config_persistence.mark_saved(generation);
                    self.note_local_config_saved(cx);
                }
                Err(error) => {
                    tracing::warn!(generation, "background preference save failed: {error:#}");
                    self.config_persistence
                        .mark_save_failed(now, PREFERENCE_SAVE_RETRY_DELAY);
                }
            }
        }

        let Some(generation) = self
            .config_persistence
            .ready_generation(now, PREFERENCE_SAVE_DEBOUNCE)
        else {
            return;
        };
        match self.config_repository.persist_async(self.config.clone()) {
            Ok(receipt) => self.config_persistence.set_in_flight(generation, receipt),
            Err(error) => {
                tracing::warn!(generation, "failed to queue preference save: {error:#}");
                self.config_persistence
                    .mark_save_failed(now, PREFERENCE_SAVE_RETRY_DELAY);
            }
        }
    }

    pub(crate) fn save_layout_state_checked(
        &self,
        window: &mut gpui::Window,
        cx: &gpui::App,
    ) -> anyhow::Result<()> {
        if self.is_layout_reset {
            tracing::info!("[ui] layout was reset, skipping save layout state.");
            return Ok(());
        }
        let current_bounds = self
            .tool_panel
            .persisted_window_bounds(window.window_bounds());
        let bounds = match current_bounds {
            gpui::WindowBounds::Fullscreen(b) => b,
            gpui::WindowBounds::Maximized(b) => b,
            gpui::WindowBounds::Windowed(b) => b,
        };
        let size = bounds.size;
        if size.width.as_f32() > 400.0 && size.height.as_f32() > 300.0 {
            tracing::info!("[ui] saving layout state...");
            let mut config = match ConfigStore::load() {
                Ok(config) => config,
                Err(error) => {
                    return Err(error.context("failed to load config before saving layout state"));
                }
            };
            let saved_bounds = match current_bounds {
                gpui::WindowBounds::Fullscreen(b) => {
                    crate::session::config::SavedWindowBounds::Fullscreen {
                        x: b.origin.x.into(),
                        y: b.origin.y.into(),
                        width: b.size.width.into(),
                        height: b.size.height.into(),
                    }
                }
                gpui::WindowBounds::Maximized(b) => {
                    let mut restore_bounds = (
                        b.origin.x.into(),
                        b.origin.y.into(),
                        b.size.width.into(),
                        b.size.height.into(),
                    );
                    if let Some(existing_bounds) = config.window_bounds() {
                        match existing_bounds {
                            crate::session::config::SavedWindowBounds::Windowed {
                                x,
                                y,
                                width,
                                height,
                            } => {
                                restore_bounds = (*x, *y, *width, *height);
                            }
                            crate::session::config::SavedWindowBounds::Maximized {
                                x,
                                y,
                                width,
                                height,
                            } => {
                                restore_bounds = (*x, *y, *width, *height);
                            }
                            _ => {}
                        }
                    }
                    crate::session::config::SavedWindowBounds::Maximized {
                        x: restore_bounds.0,
                        y: restore_bounds.1,
                        width: restore_bounds.2,
                        height: restore_bounds.3,
                    }
                }
                gpui::WindowBounds::Windowed(b) => {
                    crate::session::config::SavedWindowBounds::Windowed {
                        x: b.origin.x.into(),
                        y: b.origin.y.into(),
                        width: b.size.width.into(),
                        height: b.size.height.into(),
                    }
                }
            };
            let workspace_sizes: Vec<f32> = self
                .workspace_panels
                .read(cx)
                .sizes()
                .iter()
                .map(|s| s.into())
                .collect();
            let mut body_sizes: Vec<f32> = self
                .body_panels
                .read(cx)
                .sizes()
                .iter()
                .map(|s| s.into())
                .collect();

            if self.sftp_panel.minimized {
                if let Some(prev) = self.monitoring.prev_monitoring_size {
                    if body_sizes.len() > 1 {
                        body_sizes[1] = prev.into();
                    }
                }
            }

            config.set_layout_state(Some(saved_bounds), Some(workspace_sizes), Some(body_sizes));
            config.set_sidebar_collapsed(self.sidebar_collapsed);
            config.set_sftp_panel_minimized(self.sftp_panel.minimized);
            config.set_show_hidden_files(self.sftp_panel.show_hidden_files);
            crate::app::config_persistence::save_full(&self.config_repository, &config)?;
        } else {
            tracing::warn!(
                "[ui] window size is too small ({:?}), skipping save layout state to prevent corrupting saved bounds.",
                size
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{PaneLayout, TinyShell, config_persistence, select_config_repository};

    #[test]
    fn selects_latest_open_window_repository_and_ignores_stale_entries() {
        let stale = config_persistence::ConfigRepository::new();
        let older = config_persistence::ConfigRepository::new();
        let newest = config_persistence::ConfigRepository::new();
        let selected = select_config_repository([
            (false, 99, stale.clone()),
            (true, 1, older.clone()),
            (true, 2, newest.clone()),
        ])
        .expect("an open repository should be selected");
        assert!(Arc::ptr_eq(&selected, &newest));
        stale.shutdown().unwrap();
        older.shutdown().unwrap();
        newest.shutdown().unwrap();
    }

    #[test]
    fn parses_absolute_terminal_path() {
        assert_eq!(
            TinyShell::parse_path_from_title("user@host:/srv/app", "/home/user"),
            Some("/srv/app".to_string())
        );
    }

    #[test]
    fn expands_home_terminal_path() {
        assert_eq!(
            TinyShell::parse_path_from_title("TINY_SHELL_CWD:~/projects", "/home/user"),
            Some("/home/user/projects".to_string())
        );
    }

    #[test]
    fn rejects_titles_without_a_remote_path() {
        assert_eq!(
            TinyShell::parse_path_from_title("user@host", "/home/user"),
            None
        );
    }

    #[test]
    fn removing_missing_tab_reports_no_change() {
        let mut layout = PaneLayout::Single("a".into());

        assert!(!layout.remove_tab("missing"));
        assert_eq!(layout.tab_ids(), vec!["a"]);
    }

    #[test]
    fn removing_last_tab_uses_empty_layout_without_empty_id() {
        let mut layout = PaneLayout::Single("a".into());

        assert!(layout.remove_tab("a"));
        assert!(matches!(layout, PaneLayout::Empty));
        assert!(layout.tab_ids().is_empty());
        assert_eq!(layout.total_panes(), 0);
    }

    #[test]
    fn removing_nested_tab_collapses_single_child_and_preserves_order() {
        let mut layout = PaneLayout::Horizontal(
            vec![
                PaneLayout::Single("a".into()),
                PaneLayout::Vertical(
                    vec![
                        PaneLayout::Single("b".into()),
                        PaneLayout::Single("c".into()),
                    ],
                    0.5,
                ),
            ],
            0.5,
        );

        assert!(layout.remove_tab("b"));
        assert_eq!(layout.tab_ids(), vec!["a", "c"]);
        assert_eq!(layout.total_panes(), 2);
    }
}
