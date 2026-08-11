use std::sync::{Arc, Mutex, OnceLock};

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Pixels, Point, WindowOptions, point, px,
    size,
};
use gpui_component::Root;
use rust_i18n::t;

use crate::{
    app::sftp_editor::{EditorTab, SftpEditor},
    sftp::{
        SftpHandle,
        text_file::{RemoteFileRevision, RemoteTextFile},
    },
};

#[derive(Clone)]
struct EditorWindowEntry {
    session_id: String,
    owner_id: crate::session::store::WindowOwnerId,
    window: AnyWindowHandle,
    editor: Entity<SftpEditor>,
}

static EDITOR_WINDOWS: OnceLock<Arc<Mutex<Vec<EditorWindowEntry>>>> = OnceLock::new();

fn registry() -> Arc<Mutex<Vec<EditorWindowEntry>>> {
    EDITOR_WINDOWS
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

fn entries_for(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
) -> Vec<EditorWindowEntry> {
    registry()
        .lock()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| entry.session_id == session_id && entry.owner_id == owner_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn register(entry: EditorWindowEntry) {
    match registry().lock() {
        Ok(mut entries) => entries.push(entry),
        Err(poisoned) => {
            tracing::warn!("SFTP editor registry lock was poisoned; recovering its state");
            poisoned.into_inner().push(entry);
        }
    }
}

fn deregister(session_id: &str, window: AnyWindowHandle) {
    if let Ok(mut entries) = registry().lock() {
        entries.retain(|entry| entry.session_id != session_id || entry.window != window);
    }
}

pub(crate) fn deregister_window(window: AnyWindowHandle) {
    if let Ok(mut entries) = registry().lock() {
        entries.retain(|entry| entry.window != window);
    }
    crate::app::deregister_auxiliary_window(window);
}

fn window_options(cx: &App, position_hint: Option<Point<Pixels>>) -> WindowOptions {
    let mut options = WindowOptions::default();

    if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let editor_size = size(
            px(1000.).min(display_bounds.size.width * 0.9),
            px(760.).min(display_bounds.size.height * 0.9),
        );
        let centered = point(
            display_bounds.origin.x + (display_bounds.size.width - editor_size.width) / 2.,
            display_bounds.origin.y + (display_bounds.size.height - editor_size.height) / 2.,
        );
        let origin = position_hint
            .map(|position| {
                let max_x = display_bounds.origin.x + display_bounds.size.width - editor_size.width;
                let max_y =
                    display_bounds.origin.y + display_bounds.size.height - editor_size.height;
                point(
                    (position.x - px(80.)).clamp(display_bounds.origin.x, max_x),
                    (position.y - px(24.)).clamp(display_bounds.origin.y, max_y),
                )
            })
            .unwrap_or(centered);
        options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            editor_size,
        )));
    }

    #[cfg(not(target_os = "macos"))]
    if let Ok(image) = image::load_from_memory(include_bytes!("../../assets/icons/tiny-shell.png"))
    {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }

    options
}

pub(crate) fn open_or_focus(
    session_id: String,
    owner_id: crate::session::store::WindowOwnerId,
    remote_path: String,
    file: RemoteTextFile,
    sftp: SftpHandle,
    cx: &mut App,
) {
    if focus_path(&session_id, owner_id, &remote_path, cx) {
        return;
    }

    for entry in entries_for(&session_id, owner_id) {
        let editor = entry.editor.clone();
        let remote_path_for_existing = remote_path.clone();
        let file_for_existing = file.clone();
        if entry
            .window
            .update(cx, move |_, window, cx| {
                window.activate_window();
                editor.update(cx, |editor, cx| {
                    editor.open_file(remote_path_for_existing, file_for_existing, window, cx);
                    editor.focus_active(window, cx);
                });
            })
            .is_ok()
        {
            return;
        }
        deregister(&session_id, entry.window);
    }

    let options = window_options(cx, None);
    let session_id_for_window = session_id.clone();
    let remote_path_for_title = remote_path.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(&format!(
            "{} - {}",
            remote_path_for_title,
            t!("editor_window_title")
        ));
        let editor = cx.new(|cx| {
            SftpEditor::new(
                session_id_for_window.clone(),
                owner_id,
                remote_path,
                file,
                sftp,
                window,
                cx,
            )
        });
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);
        debug_assert_eq!(editor.read(cx).session_id(), session_id_for_window);
        register(EditorWindowEntry {
            session_id: session_id_for_window.clone(),
            owner_id,
            window: window_handle,
            editor: editor.clone(),
        });

        let session_id_for_close = session_id_for_window.clone();
        let editor_for_close = editor.clone();
        window.on_window_should_close(cx, move |window, cx| {
            let should_close =
                editor_for_close.update(cx, |editor, cx| editor.request_window_close(window, cx));
            if should_close {
                deregister(&session_id_for_close, window.window_handle());
                crate::app::deregister_auxiliary_window(window.window_handle());
            }
            should_close
        });

        window.defer(cx, {
            let editor = editor.clone();
            move |window, cx| {
                window.activate_window();
                editor.update(cx, |editor, cx| editor.focus_active(window, cx));
            }
        });

        cx.new(|cx| Root::new(editor, window, cx))
    });

    if let Err(error) = opened {
        tracing::error!("failed to open SFTP editor window: {error:?}");
    }
}

pub(crate) fn open_detached(
    session_id: String,
    owner_id: crate::session::store::WindowOwnerId,
    tab: EditorTab,
    sftp: SftpHandle,
    position: Point<Pixels>,
    cx: &mut App,
) -> bool {
    let options = window_options(cx, Some(position));
    let session_id_for_window = session_id.clone();
    let remote_path_for_title = tab.remote_path().to_string();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(&format!(
            "{} - {}",
            remote_path_for_title,
            t!("editor_window_title")
        ));
        let editor = cx.new(|cx| {
            SftpEditor::from_detached(
                session_id_for_window.clone(),
                owner_id,
                tab,
                sftp,
                window,
                cx,
            )
        });
        let window_handle = window.window_handle();
        crate::app::register_auxiliary_window(window_handle, owner_id);
        register(EditorWindowEntry {
            session_id: session_id_for_window.clone(),
            owner_id,
            window: window_handle,
            editor: editor.clone(),
        });

        let session_id_for_close = session_id_for_window.clone();
        let editor_for_close = editor.clone();
        window.on_window_should_close(cx, move |window, cx| {
            let should_close =
                editor_for_close.update(cx, |editor, cx| editor.request_window_close(window, cx));
            if should_close {
                deregister(&session_id_for_close, window.window_handle());
                crate::app::deregister_auxiliary_window(window.window_handle());
            }
            should_close
        });

        window.defer(cx, {
            let editor = editor.clone();
            move |window, cx| {
                window.activate_window();
                editor.update(cx, |editor, cx| editor.focus_active(window, cx));
            }
        });

        cx.new(|cx| Root::new(editor, window, cx))
    });

    match opened {
        Ok(_) => true,
        Err(error) => {
            tracing::error!("failed to detach SFTP editor tab: {error:?}");
            false
        }
    }
}

pub(crate) fn notify_connection_lost(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    cx: &mut App,
) {
    for entry in entries_for(session_id, owner_id) {
        let editor = entry.editor.clone();
        if entry
            .window
            .update(cx, move |_, window, cx| {
                editor.update(cx, |editor, cx| editor.notify_connection_lost(window, cx));
            })
            .is_err()
        {
            deregister(session_id, entry.window);
        }
    }
}

pub(crate) fn force_close_session_windows(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    cx: &mut App,
) {
    for entry in entries_for(session_id, owner_id) {
        let editor = entry.editor.clone();
        let _ = entry.window.update(cx, move |_, window, cx| {
            editor.update(cx, |editor, cx| editor.force_close_window(window, cx));
        });
        crate::app::deregister_auxiliary_window(entry.window);
        deregister(session_id, entry.window);
    }
}

pub(crate) fn force_close_all(owner_id: crate::session::store::WindowOwnerId, cx: &mut App) {
    let entries = registry()
        .lock()
        .map(|entries| entries.clone())
        .unwrap_or_default();
    for entry in entries
        .into_iter()
        .filter(|entry| entry.owner_id == owner_id)
    {
        let editor = entry.editor.clone();
        let _ = entry.window.update(cx, move |_, window, cx| {
            editor.update(cx, |editor, cx| editor.force_close_window(window, cx));
        });
        crate::app::deregister_auxiliary_window(entry.window);
        deregister(&entry.session_id, entry.window);
    }
}

pub(crate) fn request_session_close(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    tab_id: String,
    owner: Entity<crate::TinyShell>,
    cx: &mut App,
) -> bool {
    let entries = entries_for(session_id, owner_id);
    if entries.is_empty() {
        return true;
    }

    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.editor.read(cx).has_dirty_tabs())
        .cloned()
    {
        let editor = entry.editor.clone();
        return entry
            .window
            .update(cx, move |_, window, cx| {
                window.activate_window();
                editor.update(cx, |editor, cx| {
                    editor.request_session_close(tab_id, owner, window, cx)
                })
            })
            .unwrap_or(true);
    }

    force_close_session_windows(session_id, owner_id, cx);
    true
}

pub(crate) fn focus_path(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    remote_path: &str,
    cx: &mut App,
) -> bool {
    for entry in entries_for(session_id, owner_id) {
        let editor = entry.editor.clone();
        let remote_path = remote_path.to_string();
        match entry.window.update(cx, move |_, window, cx| {
            if editor.update(cx, |editor, cx| editor.focus_path(&remote_path, cx)) {
                window.activate_window();
                editor.update(cx, |editor, cx| editor.focus_active(window, cx));
                true
            } else {
                false
            }
        }) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(_) => deregister(session_id, entry.window),
        }
    }
    false
}

pub(crate) fn mark_uploaded(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    remote_path: &str,
    revision: RemoteFileRevision,
    cx: &mut App,
) {
    for entry in entries_for(session_id, owner_id) {
        let remote_path = remote_path.to_string();
        let revision = revision.clone();
        entry.editor.update(cx, |editor, cx| {
            editor.mark_uploaded(&remote_path, revision, cx)
        });
    }
}

pub(crate) fn mark_conflict(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    remote_path: &str,
    remote_file: RemoteTextFile,
    cx: &mut App,
) {
    for entry in entries_for(session_id, owner_id) {
        let remote_path = remote_path.to_string();
        let remote_file = remote_file.clone();
        entry.editor.update(cx, |editor, cx| {
            editor.mark_conflict(&remote_path, remote_file, cx)
        });
    }
}

pub(crate) fn mark_upload_failed(
    session_id: &str,
    owner_id: crate::session::store::WindowOwnerId,
    remote_path: &str,
    error: String,
    cx: &mut App,
) {
    for entry in entries_for(session_id, owner_id) {
        let remote_path = remote_path.to_string();
        let error = error.clone();
        entry.editor.update(cx, |editor, cx| {
            editor.mark_upload_failed(&remote_path, error, cx)
        });
    }
}
