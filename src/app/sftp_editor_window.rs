use std::sync::{Arc, Mutex, OnceLock};

use gpui::{AnyWindowHandle, App, AppContext as _, Bounds, Entity, WindowOptions, point, px, size};
use gpui_component::Root;
use rust_i18n::t;

use crate::{app::sftp_editor::SftpEditor, sftp::SftpHandle};

#[derive(Clone)]
struct EditorWindowEntry {
    session_id: String,
    window: AnyWindowHandle,
    editor: Entity<SftpEditor>,
}

static EDITOR_WINDOWS: OnceLock<Arc<Mutex<Vec<EditorWindowEntry>>>> = OnceLock::new();

fn registry() -> Arc<Mutex<Vec<EditorWindowEntry>>> {
    EDITOR_WINDOWS
        .get_or_init(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

fn entry_for(session_id: &str) -> Option<EditorWindowEntry> {
    registry()
        .lock()
        .ok()?
        .iter()
        .find(|entry| entry.session_id == session_id)
        .cloned()
}

fn register(entry: EditorWindowEntry) {
    let registry = registry();
    let mut entries = registry.lock().unwrap();
    entries.retain(|existing| existing.session_id != entry.session_id);
    entries.push(entry);
}

fn deregister(session_id: &str, window: AnyWindowHandle) {
    if let Ok(mut entries) = registry().lock() {
        entries.retain(|entry| entry.session_id != session_id || entry.window != window);
    }
}

fn window_options(cx: &App) -> WindowOptions {
    let mut options = WindowOptions::default();

    if let Some(display) = cx.displays().first().cloned() {
        let display_bounds = display.bounds();
        let editor_size = size(
            px(1000.).min(display_bounds.size.width * 0.9),
            px(760.).min(display_bounds.size.height * 0.9),
        );
        let origin = point(
            display_bounds.origin.x + (display_bounds.size.width - editor_size.width) / 2.,
            display_bounds.origin.y + (display_bounds.size.height - editor_size.height) / 2.,
        );
        options.window_bounds = Some(gpui::WindowBounds::Windowed(Bounds::new(
            origin,
            editor_size,
        )));
    }

    #[cfg(not(target_os = "macos"))]
    if let Ok(image) = image::load_from_memory(include_bytes!("../../assets/icons/ashell.png")) {
        options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
    }

    options
}

pub(crate) fn open_or_focus(
    session_id: String,
    remote_path: String,
    content: String,
    sftp: SftpHandle,
    cx: &mut App,
) {
    if let Some(entry) = entry_for(&session_id) {
        let editor = entry.editor.clone();
        let remote_path_for_existing = remote_path.clone();
        let content_for_existing = content.clone();
        if entry
            .window
            .update(cx, move |_, window, cx| {
                window.activate_window();
                editor.update(cx, |editor, cx| {
                    editor.open_file(remote_path_for_existing, content_for_existing, window, cx);
                    editor.focus_active(window, cx);
                });
            })
            .is_ok()
        {
            return;
        }
        deregister(&session_id, entry.window);
    }

    let options = window_options(cx);
    let session_id_for_window = session_id.clone();
    let remote_path_for_title = remote_path.clone();
    let opened = cx.open_window(options, move |window, cx| {
        window.set_window_title(&format!(
            "{} - {}",
            remote_path_for_title,
            t!("editor_window_title")
        ));
        gpui_component::Theme::sync_system_appearance(Some(window), cx);

        let editor = cx.new(|cx| {
            SftpEditor::new(
                session_id_for_window.clone(),
                remote_path,
                content,
                sftp,
                window,
                cx,
            )
        });
        let window_handle = window.window_handle();
        debug_assert_eq!(editor.read(cx).session_id(), session_id_for_window);
        register(EditorWindowEntry {
            session_id: session_id_for_window.clone(),
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

pub(crate) fn notify_connection_lost(session_id: &str, cx: &mut App) {
    let Some(entry) = entry_for(session_id) else {
        return;
    };
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

pub(crate) fn request_session_close(
    session_id: &str,
    tab_id: String,
    owner: Entity<crate::Ashell>,
    cx: &mut App,
) -> bool {
    let Some(entry) = entry_for(session_id) else {
        return true;
    };
    let editor = entry.editor.clone();
    entry
        .window
        .update(cx, move |_, window, cx| {
            window.activate_window();
            editor.update(cx, |editor, cx| {
                editor.request_session_close(tab_id, owner, window, cx)
            })
        })
        .unwrap_or(true)
}

pub(crate) fn focus_path(session_id: &str, remote_path: &str, cx: &mut App) -> bool {
    let Some(entry) = entry_for(session_id) else {
        return false;
    };
    let editor = entry.editor.clone();
    let remote_path = remote_path.to_string();
    entry
        .window
        .update(cx, move |_, window, cx| {
            if editor.update(cx, |editor, cx| editor.focus_path(&remote_path, cx)) {
                window.activate_window();
                editor.update(cx, |editor, cx| editor.focus_active(window, cx));
                true
            } else {
                false
            }
        })
        .unwrap_or(false)
}

pub(crate) fn mark_uploaded(session_id: &str, remote_path: &str, cx: &mut App) {
    let Some(entry) = entry_for(session_id) else {
        return;
    };
    let remote_path = remote_path.to_string();
    entry
        .editor
        .update(cx, |editor, cx| editor.mark_uploaded(&remote_path, cx));
}

pub(crate) fn mark_upload_failed(session_id: &str, remote_path: &str, cx: &mut App) {
    let Some(entry) = entry_for(session_id) else {
        return;
    };
    let remote_path = remote_path.to_string();
    entry
        .editor
        .update(cx, |editor, cx| editor.mark_upload_failed(&remote_path, cx));
}
