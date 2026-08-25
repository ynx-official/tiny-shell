use std::{
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use gpui::{AnyWindowHandle, App, Bounds, Entity, FocusHandle, Pixels, Point};

use super::{TinyShell, config_persistence};

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

#[derive(Clone)]
pub(crate) struct IncomingPaneDrag {
    pub(crate) group_id: String,
    pub(crate) tab_id: String,
}

static WINDOW_REGISTRY: OnceLock<Arc<Mutex<Vec<WindowEntry>>>> = OnceLock::new();
static AUXILIARY_WINDOW_REGISTRY: OnceLock<Arc<Mutex<Vec<AuxiliaryWindowEntry>>>> = OnceLock::new();
static TAB_DRAG_HOVER: OnceLock<Mutex<TabDragHoverState<AnyWindowHandle>>> = OnceLock::new();
static WINDOW_ACTIVATION_SEQ: AtomicU64 = AtomicU64::new(1);
static TAB_DRAG_SEQ: AtomicU64 = AtomicU64::new(1);
static TAB_DRAG_HOVER_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct AuxiliaryWindowEntry {
    owner_id: crate::session::store::WindowOwnerId,
    window: AnyWindowHandle,
}

struct TabDragHoverState<WindowHandle> {
    current: Option<(u64, WindowHandle, u64)>,
}

impl<WindowHandle> Default for TabDragHoverState<WindowHandle> {
    fn default() -> Self {
        Self { current: None }
    }
}

impl<WindowHandle: Copy + PartialEq> TabDragHoverState<WindowHandle> {
    fn set(&mut self, drag_id: u64, target: WindowHandle, generation: u64) {
        self.current = Some((drag_id, target, generation));
    }

    fn is_current(&self, drag_id: u64, target: WindowHandle, generation: u64) -> bool {
        self.current == Some((drag_id, target, generation))
    }

    fn targets(&self, drag_id: u64, target: WindowHandle) -> bool {
        self.current
            .is_some_and(|current| current.0 == drag_id && current.1 == target)
    }

    fn exists(&self, drag_id: u64) -> bool {
        self.current.is_some_and(|current| current.0 == drag_id)
    }

    fn clear_drag(&mut self, drag_id: u64) -> bool {
        if self.exists(drag_id) {
            self.current = None;
            true
        } else {
            false
        }
    }

    fn clear_target(&mut self, drag_id: u64, target: WindowHandle) -> bool {
        if self.targets(drag_id, target) {
            self.current = None;
            true
        } else {
            false
        }
    }

    fn clear(&mut self) {
        self.current = None;
    }
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

pub(super) fn select_config_repository(
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
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").set(drag_id, target_window, generation);
    generation
}

pub(crate) fn tab_drag_hover_is_current(
    drag_id: u64,
    target_window: AnyWindowHandle,
    generation: u64,
) -> bool {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").is_current(drag_id, target_window, generation)
}

pub(crate) fn tab_drag_hover_targets(drag_id: u64, target_window: AnyWindowHandle) -> bool {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").targets(drag_id, target_window)
}

pub(crate) fn tab_drag_hover_exists(drag_id: u64) -> bool {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").exists(drag_id)
}

pub(crate) fn clear_tab_drag_hover_for_drag(drag_id: u64) -> bool {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").clear_drag(drag_id)
}

pub(crate) fn clear_tab_drag_hover_for_target(
    drag_id: u64,
    target_window: AnyWindowHandle,
) -> bool {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").clear_target(drag_id, target_window)
}

pub(crate) fn clear_tab_drag_hover() {
    let hover = TAB_DRAG_HOVER.get_or_init(|| Mutex::new(TabDragHoverState::default()));
    lock_recover(hover, "tab drag hover").clear();
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

pub(crate) fn other_main_windows(
    exclude: AnyWindowHandle,
) -> Vec<(AnyWindowHandle, Entity<TinyShell>)> {
    let registry = window_registry();
    let guard = lock_recover(&registry, "window registry");
    let mut entries = guard
        .iter()
        .filter(|entry| entry.window_handle != exclude)
        .map(|entry| {
            (
                entry.activation_seq,
                entry.window_handle,
                entry.entity.clone(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(activation_seq, _, _)| std::cmp::Reverse(*activation_seq));
    entries
        .into_iter()
        .map(|(_, handle, entity)| (handle, entity))
        .collect()
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
                target.incoming_tab_drop_zone = None;
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
                target.incoming_tab_drop_zone = None;
                cx.notify();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::TabDragHoverState;
    use super::{config_persistence, select_config_repository};

    #[test]
    fn selects_latest_open_window_repository_and_ignores_stale_entries() {
        let stale = config_persistence::ConfigRepository::new();
        let older = config_persistence::ConfigRepository::new();
        let newest = config_persistence::ConfigRepository::new();
        let selected = select_config_repository([
            (false, 99, stale.clone()),
            (true, 1, older.clone()),
            (true, 2, newest.clone()),
        ]);
        assert!(
            selected
                .as_ref()
                .is_some_and(|repository| Arc::ptr_eq(repository, &newest))
        );
        assert!(stale.shutdown().is_ok());
        assert!(older.shutdown().is_ok());
        assert!(newest.shutdown().is_ok());
    }

    #[test]
    fn hover_state_requires_exact_generation_for_delayed_work() {
        let mut hover = TabDragHoverState::default();
        hover.set(7, 11_u8, 3);

        assert!(hover.is_current(7, 11, 3));
        assert!(!hover.is_current(7, 11, 2));
        assert!(hover.targets(7, 11));
        assert!(hover.exists(7));
    }

    #[test]
    fn stale_drag_or_target_cannot_clear_the_current_hover() {
        let mut hover = TabDragHoverState::default();
        hover.set(7, 11_u8, 3);

        assert!(!hover.clear_drag(6));
        assert!(!hover.clear_target(7, 10));
        assert!(hover.is_current(7, 11, 3));
        assert!(hover.clear_target(7, 11));
        assert!(!hover.exists(7));
    }
}
