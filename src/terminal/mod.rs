pub(crate) mod custom_blocks;
pub(crate) mod element;
pub(crate) mod highlight;
pub(crate) mod input;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionRange, SelectionType},
    term::{
        Config, LineDamageBounds, Term, TermDamage, TermMode, cell::Cell, point_to_viewport,
        viewport_to_point,
    },
    vte::ansi::{CursorShape, Processor},
};
use gpui::Keystroke;

use crate::session::config::Session;
use crate::sftp::{
    PreviewData, RemoteEntry,
    text_file::{RemoteFileRevision, RemoteTextFile},
};
use crate::system::SystemSnapshot;

type HighlightCache = Option<(u64, Arc<HashMap<(i32, i32), gpui::Hsla>>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabKind {
    Local,
    Ssh,
}

#[derive(Debug)]
pub(crate) enum BackendCommand {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    SampleMetrics,
    Close,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub(crate) enum BackendEvent {
    Output {
        tab_id: String,
        bytes: Vec<u8>,
    },
    Status {
        tab_id: String,
        text: String,
    },
    Connected {
        tab_id: String,
    },
    SftpEntries {
        tab_id: String,
        path: String,
        entries: Vec<RemoteEntry>,
    },
    SftpDirectoryEntries {
        tab_id: String,
        path: String,
        entries: Vec<RemoteEntry>,
    },
    SftpPreview {
        tab_id: String,
        preview: PreviewData,
    },
    SftpStatus {
        tab_id: String,
        text: String,
    },
    SftpLatency {
        tab_id: String,
        latency_ms: Option<u64>,
    },
    /// 文件内容与远程版本信息已下载，供内置编辑器使用。
    SftpFileContent {
        tab_id: String,
        remote_path: String,
        file: RemoteTextFile,
    },
    /// 文件已通过版本校验并原子替换完成。
    SftpContentUploaded {
        tab_id: String,
        remote_path: String,
        revision: RemoteFileRevision,
    },
    /// 保存前检测到远程内容已发生变化。
    SftpContentConflict {
        tab_id: String,
        remote_path: String,
        remote_file: RemoteTextFile,
    },
    /// 内存中的文件内容上传失败。
    SftpContentUploadFailed {
        tab_id: String,
        remote_path: String,
        error: String,
    },
    RemoteSystem {
        tab_id: String,
        snapshot: Box<SystemSnapshot>,
    },
    RemoteSystemUnavailable {
        tab_id: String,
        reason: String,
    },
    SftpHome {
        tab_id: String,
        home: String,
    },
    TransferProgress {
        tab_id: String,
        id: String,
        transferred: u64,
        total: Option<u64>,
        state: TransferState,
    },
    TransferStarted {
        tab_id: String,
        info: TransferInfo,
    },
    Closed {
        tab_id: String,
        reason: String,
    },
    TerminalTitleChanged {
        tab_id: String,
        title: String,
    },
    SyncFinished {
        result: crate::sync::SyncResult,
        task_id: u64,
    },
}

#[derive(Clone)]
pub(crate) struct BackendEventSender {
    events: Sender<BackendEvent>,
    wake_generation: Arc<AtomicU64>,
    wake: tokio::sync::watch::Sender<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BackendEventSendError;

impl BackendEventSender {
    pub fn send(&self, event: BackendEvent) -> Result<(), BackendEventSendError> {
        self.events.send(event).map_err(|_| BackendEventSendError)?;
        let generation = self.wake_generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.wake.send_replace(generation);
        Ok(())
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.wake.subscribe()
    }
}

pub(crate) fn backend_event_channel() -> (BackendEventSender, Receiver<BackendEvent>) {
    let (events, receiver) = mpsc::channel();
    let (wake, _) = tokio::sync::watch::channel(0);
    (
        BackendEventSender {
            events,
            wake_generation: Arc::new(AtomicU64::new(0)),
            wake,
        },
        receiver,
    )
}

#[derive(Clone)]
pub(crate) enum BackendTx {
    Local(Sender<BackendCommand>),
    Ssh(tokio::sync::mpsc::UnboundedSender<BackendCommand>),
}

impl BackendTx {
    pub fn send(&self, command: BackendCommand) {
        let sent = match self {
            Self::Local(tx) => tx.send(command).is_ok(),
            Self::Ssh(tx) => tx.send(command).is_ok(),
        };
        if !sent {
            tracing::debug!("backend command dropped because its receiver is closed");
        }
    }
}

pub(crate) struct TerminalTab {
    pub id: String,
    pub title: String,
    pub dynamic_title: String,
    pub kind: TabKind,
    pub status: String,
    pub connected: bool,
    pub disconnected_reason: Option<String>,
    /// Incremented each time the tab is reconnected. Used to ignore stale
    /// `BackendEvent::Closed` from the previous backend after a retry.
    pub backend_generation: u32,
    /// Set to `true` when the current backend sends its first `Output` or
    /// `Connected` event. Used to skip stale `Closed` events that arrive
    /// before the new backend has started producing output.
    pub backend_initialized: bool,
    pub session: Option<Session>,
    processor: Processor,
    term: Term<TerminalListener>,
    pub cols: u16,
    pub rows: u16,
    pub backend: std::sync::Arc<std::sync::Mutex<BackendTx>>,
    pub scroll_pixel_y: f32,
    pub(crate) highlight_cache: std::cell::RefCell<HighlightCache>,
    render_revision: u64,
    render_cache: std::cell::RefCell<Option<(u64, bool, RenderSnapshot)>>,
    pending_render_damage: std::cell::RefCell<RenderDamage>,
}

#[derive(Debug)]
enum RenderDamage {
    Full,
    Partial(Vec<LineDamageBounds>),
}

impl Default for RenderDamage {
    fn default() -> Self {
        Self::Partial(Vec::new())
    }
}

impl RenderDamage {
    fn merge(&mut self, damage: TermDamage<'_>) {
        match damage {
            TermDamage::Full => *self = Self::Full,
            TermDamage::Partial(lines) => {
                let Self::Partial(pending) = self else {
                    return;
                };
                for line in lines {
                    if let Some(existing) = pending.iter_mut().find(|item| item.line == line.line) {
                        existing.left = existing.left.min(line.left);
                        existing.right = existing.right.max(line.right);
                    } else {
                        pending.push(line);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CursorState {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
}

#[derive(Clone, PartialEq)]
pub(crate) struct RenderCell {
    pub row: i32,
    pub col: i32,
    pub cell: Cell,
}

#[derive(Clone)]
pub(crate) struct RenderSnapshot {
    /// Shared between frames so an idle terminal does not clone its viewport
    /// on every GPUI prepaint pass.
    pub cells: Arc<Vec<RenderCell>>,
    pub cursor: Option<CursorState>,
    pub selection: Option<ViewportSelection>,
    pub display_offset: usize,
    pub history_size: usize,
    pub rows: usize,
    pub cols: usize,
    pub highlights: Arc<HashMap<(i32, i32), gpui::Hsla>>,
}

#[derive(Clone, Copy)]
pub(crate) struct ViewportSelection {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub is_block: bool,
}

#[derive(Clone, Default)]
pub(crate) struct SftpUiState {
    pub current_path: String,
    pub status: String,
    pub entries: Vec<RemoteEntry>,
    pub directory_entries: std::collections::HashMap<String, Vec<RemoteEntry>>,
    pub expanded_directories: std::collections::HashSet<String>,
    pub selected_path: Option<String>,
    pub preview: Option<PreviewData>,
    pub selected_entries: std::collections::HashSet<String>,
    pub home_dir: String,
    pub follow_terminal_cwd: bool,
    pub initial_terminal_cwd_synced: bool,
    pub latency_ms: Option<u64>,
}

impl TerminalTab {
    pub fn new_local(
        id: String,
        title: String,
        backend: BackendTx,
        events: BackendEventSender,
    ) -> Self {
        Self::new(
            id,
            title,
            TabKind::Local,
            "local shell".into(),
            backend,
            events,
        )
    }

    pub fn new_ssh(
        id: String,
        session: &Session,
        backend: BackendTx,
        events: BackendEventSender,
    ) -> Self {
        let mut tab = Self::new(
            id,
            session.name.clone(),
            TabKind::Ssh,
            format!(
                "connecting {}@{}:{}",
                session.user, session.host, session.port
            ),
            backend,
            events,
        );
        tab.session = Some(session.clone());
        tab.connected = false;
        tab
    }

    fn new(
        id: String,
        title: String,
        kind: TabKind,
        status: String,
        backend: BackendTx,
        events: BackendEventSender,
    ) -> Self {
        let shared_backend = std::sync::Arc::new(std::sync::Mutex::new(backend));
        let mut term = new_term(100, 30, shared_backend.clone(), id.clone(), events.clone());
        term.reset_damage();
        Self {
            id: id.clone(),
            title: title.clone(),
            dynamic_title: title,
            kind,
            status,
            connected: matches!(kind, TabKind::Local),
            disconnected_reason: None,
            backend_generation: 0,
            backend_initialized: true,
            session: None,
            processor: Processor::new(),
            term,
            cols: 100,
            rows: 30,
            backend: shared_backend,
            scroll_pixel_y: 0.0,
            highlight_cache: std::cell::RefCell::new(None),
            render_revision: 0,
            render_cache: std::cell::RefCell::new(None),
            pending_render_damage: std::cell::RefCell::new(RenderDamage::Full),
        }
    }

    fn invalidate_render_cache(&mut self) {
        self.render_revision = self.render_revision.wrapping_add(1);
    }

    fn mark_full_render_damage(&mut self) {
        *self.pending_render_damage.get_mut() = RenderDamage::Full;
        self.invalidate_render_cache();
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.invalidate_render_cache();
        self.processor.advance(&mut self.term, bytes);
        {
            let damage = self.term.damage();
            self.pending_render_damage.get_mut().merge(damage);
        }
        self.term.reset_damage();
    }

    pub fn feed_status_line(&mut self, text: &str) {
        let mut line = String::with_capacity(text.len() + 2);
        for character in text.chars() {
            if character == '\t' || !character.is_control() {
                line.push(character);
            } else if matches!(character, '\r' | '\n') && !line.ends_with(' ') {
                line.push(' ');
            }
        }
        line.push_str("\r\n");
        self.feed(line.as_bytes());
    }

    /// Send a command to the backend. Thread-safe via the shared Arc<Mutex>.
    pub fn send_backend(&self, command: BackendCommand) {
        if let Ok(backend) = self.backend.lock() {
            backend.send(command);
        }
    }

    /// Replace the backend with a new one. The `Term`'s internal listener
    /// shares the same `Arc`, so user input is automatically routed to the
    /// new backend. The old backend must be closed by the caller.
    pub fn set_backend(&self, new_backend: BackendTx) {
        if let Ok(mut backend) = self.backend.lock() {
            *backend = new_backend;
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let new_cols = cols.max(1);
        let new_rows = rows.max(1);
        if self.cols != new_cols || self.rows != new_rows {
            self.cols = new_cols;
            self.rows = new_rows;
            self.mark_full_render_damage();
            tracing::info!(
                "[ui] terminal resized to {}x{} (cols x rows)",
                self.cols,
                self.rows
            );
            self.term.resize(TerminalSize::new(self.cols, self.rows));
            self.term.reset_damage();
            self.send_backend(BackendCommand::Resize { cols, rows });
        }
    }

    pub fn cursor_state(&self) -> Option<CursorState> {
        let content = self.term.renderable_content();
        if matches!(content.cursor.shape, CursorShape::Hidden) || content.display_offset > 0 {
            return None;
        }
        let row = content.cursor.point.line.0;
        if row < 0 {
            return None;
        }
        let row = row as usize;
        if row >= self.rows as usize {
            return None;
        }

        Some(CursorState {
            row,
            col: content.cursor.point.column.0,
            shape: content.cursor.shape,
        })
    }

    pub fn app_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn render_snapshot(&self, keyword_highlight: bool) -> RenderSnapshot {
        if let Some((revision, cached_keyword_highlight, snapshot)) =
            self.render_cache.borrow().as_ref()
            && *revision == self.render_revision
            && *cached_keyword_highlight == keyword_highlight
        {
            return snapshot.clone();
        }

        let rows = self.rows;
        let cols = self.cols;
        let content = self.term.renderable_content();
        let display_offset = content.display_offset as i32;
        let previous = self.render_cache.borrow_mut().take();
        let damage = std::mem::take(&mut *self.pending_render_damage.borrow_mut());
        let can_reuse = previous.as_ref().is_some_and(|(_, _, snapshot)| {
            snapshot.rows == rows as usize
                && snapshot.cols == cols as usize
                && snapshot.display_offset == content.display_offset
                && snapshot.cells.len() == rows as usize * cols as usize
        });
        let mut cells = if can_reuse {
            match previous {
                Some((_, _, snapshot)) => {
                    Arc::try_unwrap(snapshot.cells).unwrap_or_else(|cells| cells.as_ref().clone())
                }
                None => Vec::new(),
            }
        } else {
            Vec::with_capacity((rows as usize) * (cols as usize))
        };

        let damaged_lines = match damage {
            RenderDamage::Full => {
                cells.clear();
                Some((0..rows as usize).collect::<Vec<_>>())
            }
            RenderDamage::Partial(lines) if can_reuse => Some(
                lines
                    .into_iter()
                    .filter(|line| line.line < rows as usize)
                    .map(|line| line.line)
                    .collect::<Vec<_>>(),
            ),
            RenderDamage::Partial(_) => {
                cells.clear();
                Some((0..rows as usize).collect::<Vec<_>>())
            }
        };

        if let Some(damaged_lines) = damaged_lines {
            let grid = self.term.grid();
            if cells.is_empty() {
                cells.resize_with(rows as usize * cols as usize, || RenderCell {
                    row: 0,
                    col: 0,
                    cell: Cell::default(),
                });
            }
            for row in damaged_lines {
                let line = Line(row as i32 - display_offset);
                for col in 0..cols as usize {
                    cells[row * cols as usize + col] = RenderCell {
                        row: row as i32,
                        col: col as i32,
                        cell: grid[Point::new(line, Column(col))].clone(),
                    };
                }
            }
        }

        // Get highlights from cache or recompute, only if keyword_highlight is enabled.
        let is_enabled = keyword_highlight;

        let highlights = if is_enabled {
            let mut cache = self.highlight_cache.borrow_mut();
            if let Some((cached_revision, highlights)) = cache.as_ref()
                && *cached_revision == self.render_revision
            {
                highlights.clone()
            } else {
                let computed = Arc::new(self::highlight::highlight_cells(&cells, rows as usize));
                *cache = Some((self.render_revision, computed.clone()));
                computed
            }
        } else {
            Arc::new(HashMap::new())
        };

        let snapshot = RenderSnapshot {
            cells: Arc::new(cells),
            cursor: self.cursor_state(),
            selection: viewport_selection_from_range(
                content.display_offset,
                self.rows as usize,
                self.cols as usize,
                &content.selection,
            ),
            display_offset: content.display_offset,
            history_size: self.term.grid().history_size(),
            rows: self.rows as usize,
            cols: self.cols as usize,
            highlights,
        };
        *self.render_cache.borrow_mut() =
            Some((self.render_revision, keyword_highlight, snapshot.clone()));
        snapshot
    }

    /// Return `(grid_line_base, rows_data)` for the **entire** terminal buffer
    /// including scrollback history. `grid_line_base` is the grid line index of
    /// the first row (typically `-history_size`). Each entry in `rows_data` is
    /// a sorted `Vec<(col, char)>` for that row.
    pub fn full_grid_rows(&self) -> (i32, Vec<Vec<(i32, char)>>) {
        let grid = self.term.grid();
        let history = grid.history_size() as i32;
        let screen = grid.screen_lines() as i32;
        let total = history + screen;
        let cols = self.cols as i32;
        let start_line = -history;

        let mut rows_data: Vec<Vec<(i32, char)>> = Vec::with_capacity(total as usize);
        for line_idx in start_line..(start_line + total) {
            let line = Line(line_idx);
            let mut cells: Vec<(i32, char)> = Vec::new();
            for col_idx in 0..cols {
                let point = Point::new(line, Column(col_idx as usize));
                let c = grid[point].c;
                if c != ' ' && c != '\0' {
                    cells.push((col_idx, c));
                }
            }
            rows_data.push(cells);
        }
        (start_line, rows_data)
    }

    pub fn scroll_history(&mut self, delta: i32) {
        if delta != 0 {
            self.mark_full_render_damage();
            self.term.scroll_display(Scroll::Delta(delta));
            self.term.reset_damage();
        }
    }

    pub fn scroll_up_by(&mut self, lines: usize) {
        if lines != 0 {
            self.mark_full_render_damage();
            self.term.scroll_display(Scroll::Delta(lines as i32));
            self.term.reset_damage();
        }
    }

    pub fn scroll_down_by(&mut self, lines: usize) {
        if lines != 0 {
            self.mark_full_render_damage();
            self.term.scroll_display(Scroll::Delta(-(lines as i32)));
            self.term.reset_damage();
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.mark_full_render_damage();
        self.term.scroll_display(Scroll::Bottom);
        self.term.reset_damage();
    }

    /// 轻量获取当前视口的滚动偏移量（是否在回看历史）。
    ///
    /// 替代此前为判断 `display_offset > 0` 而调用完整 `render_snapshot()`
    /// （含 cell 迭代 + 高亮计算）的重型做法。
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    #[allow(dead_code)]
    pub fn has_selection(&self) -> bool {
        self.term
            .selection_to_string()
            .is_some_and(|text| !text.is_empty())
    }

    pub fn clear_selection(&mut self) {
        self.invalidate_render_cache();
        self.term.selection = None;
    }

    pub fn clear_contents(&mut self) {
        self.processor
            .advance(&mut self.term, b"\x1b[2J\x1b[3J\x1b[H");
        self.mark_full_render_damage();
        self.term.reset_damage();
        self.scroll_pixel_y = 0.0;
        self.clear_selection();
        *self.highlight_cache.borrow_mut() = None;
        self.send_backend(BackendCommand::Input(vec![b'\x0c']));
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    pub fn begin_selection(
        &mut self,
        row: usize,
        col: usize,
        side: Side,
        selection_type: SelectionType,
    ) {
        self.invalidate_render_cache();
        let point = viewport_to_point(
            self.term.grid().display_offset(),
            Point::new(row, Column(col)),
        );
        self.term.selection = Some(Selection::new(selection_type, point, side));
    }

    pub fn update_selection(&mut self, row: usize, col: usize, side: Side) {
        self.invalidate_render_cache();
        let point = viewport_to_point(
            self.term.grid().display_offset(),
            Point::new(row, Column(col)),
        );
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn paste_text(&mut self, text: &str) {
        let bracketed = self.term.mode().contains(TermMode::BRACKETED_PASTE);
        let paste_text = text
            .replace('\x1b', "")
            .replace("\r\n", "\r")
            .replace('\n', "\r");

        let mut bytes = Vec::new();
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(paste_text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }

        self.send_backend(BackendCommand::Input(bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_contents_removes_viewport_and_scrollback() {
        let (backend_tx, backend_rx) = std::sync::mpsc::channel();
        let (event_tx, _event_rx) = backend_event_channel();
        let mut tab = TerminalTab::new_local(
            "test".to_string(),
            "Test".to_string(),
            BackendTx::Local(backend_tx),
            event_tx,
        );

        for line in 0..40 {
            tab.feed(format!("line {line}\r\n").as_bytes());
        }
        tab.scroll_pixel_y = 7.0;
        assert!(tab.render_snapshot(false).history_size > 0);

        tab.clear_contents();

        let snapshot = tab.render_snapshot(false);
        assert_eq!(snapshot.history_size, 0);
        assert_eq!(snapshot.display_offset, 0);
        assert_eq!(
            snapshot.cursor.map(|cursor| (cursor.row, cursor.col)),
            Some((0, 0))
        );
        assert!(
            snapshot
                .cells
                .iter()
                .all(|cell| matches!(cell.cell.c, ' ' | '\0'))
        );
        assert_eq!(tab.scroll_pixel_y, 0.0);
        assert!(matches!(
            backend_rx.try_recv(),
            Ok(BackendCommand::Input(bytes)) if bytes == vec![b'\x0c']
        ));
    }

    #[test]
    fn idle_snapshot_reuses_shared_cells_until_terminal_changes() {
        let (backend_tx, _backend_rx) = std::sync::mpsc::channel();
        let (event_tx, _event_rx) = backend_event_channel();
        let mut tab = TerminalTab::new_local(
            "test".to_string(),
            "Test".to_string(),
            BackendTx::Local(backend_tx),
            event_tx,
        );

        tab.feed(b"hello");
        let first = tab.render_snapshot(false);
        let second = tab.render_snapshot(false);
        assert!(Arc::ptr_eq(&first.cells, &second.cells));

        tab.feed(b" world");
        let changed = tab.render_snapshot(false);
        assert!(!Arc::ptr_eq(&first.cells, &changed.cells));
    }

    #[test]
    fn terminal_damage_tracks_only_touched_rows_for_simple_output() {
        let (backend_tx, _backend_rx) = std::sync::mpsc::channel();
        let (event_tx, _event_rx) = backend_event_channel();
        let mut tab = TerminalTab::new_local(
            "test".to_string(),
            "Test".to_string(),
            BackendTx::Local(backend_tx),
            event_tx,
        );
        let _ = tab.render_snapshot(false);

        tab.feed(b"hello");

        let damage = tab.pending_render_damage.borrow();
        assert!(matches!(
            &*damage,
            RenderDamage::Partial(lines)
                if !lines.is_empty() && lines.iter().all(|line| line.line == 0)
        ));
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn benchmark_incremental_terminal_rendering() {
        let (backend_tx, _backend_rx) = std::sync::mpsc::channel();
        let (event_tx, _event_rx) = backend_event_channel();
        let mut tab = TerminalTab::new_local(
            "benchmark".to_string(),
            "Benchmark".to_string(),
            BackendTx::Local(backend_tx),
            event_tx,
        );
        let _ = tab.render_snapshot(false);
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            tab.feed(b"x");
            std::hint::black_box(tab.render_snapshot(false));
        }
        eprintln!("10k incremental terminal renders: {:?}", started.elapsed());
    }
}

fn viewport_selection_from_range(
    display_offset: usize,
    rows: usize,
    cols: usize,
    selection: &Option<SelectionRange>,
) -> Option<ViewportSelection> {
    let SelectionRange {
        start,
        end,
        is_block,
    } = selection.as_ref().copied()?;

    let top_point = viewport_to_point(display_offset, Point::new(0, Column(0)));
    let bottom_point = viewport_to_point(
        display_offset,
        Point::new(rows.saturating_sub(1), Column(0)),
    );

    let top_line = top_point.line;
    let bottom_line = bottom_point.line;

    let start_vp = if start.line < top_line {
        Point::new(0, Column(0))
    } else if start.line > bottom_line {
        Point::new(rows.saturating_sub(1), Column(cols.saturating_sub(1)))
    } else {
        point_to_viewport(display_offset, start).unwrap_or(Point::new(0, Column(0)))
    };

    let end_vp = if end.line < top_line {
        Point::new(0, Column(0))
    } else if end.line > bottom_line {
        Point::new(rows.saturating_sub(1), Column(cols.saturating_sub(1)))
    } else {
        point_to_viewport(display_offset, end).unwrap_or(Point::new(
            rows.saturating_sub(1),
            Column(cols.saturating_sub(1)),
        ))
    };

    Some(ViewportSelection {
        start_row: start_vp.line,
        start_col: start_vp.column.0,
        end_row: end_vp.line,
        end_col: end_vp.column.0,
        is_block,
    })
}

#[derive(Clone)]
struct TerminalListener {
    tab_id: String,
    backend: std::sync::Arc<std::sync::Mutex<BackendTx>>,
    events: BackendEventSender,
}

impl EventListener for TerminalListener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(output) => {
                if let Ok(backend) = self.backend.lock() {
                    backend.send(BackendCommand::Input(output.into_bytes()));
                }
            }
            Event::TextAreaSizeRequest(format) => {
                let size = alacritty_terminal::event::WindowSize {
                    num_lines: 30,
                    num_cols: 100,
                    cell_width: 8,
                    cell_height: 16,
                };
                if let Ok(backend) = self.backend.lock() {
                    backend.send(BackendCommand::Input(format(size).into_bytes()));
                }
            }
            Event::Title(title) => {
                let _ = self.events.send(BackendEvent::TerminalTitleChanged {
                    tab_id: self.tab_id.clone(),
                    title,
                });
            }
            _ => {}
        }
    }
}

fn new_term(
    cols: u16,
    rows: u16,
    backend: std::sync::Arc<std::sync::Mutex<BackendTx>>,
    tab_id: String,
    events: BackendEventSender,
) -> Term<TerminalListener> {
    Term::new(
        Config {
            scrolling_history: 2000,
            ..Config::default()
        },
        &TerminalSize::new(cols, rows),
        TerminalListener {
            tab_id,
            backend,
            events,
        },
    )
}

struct TerminalSize {
    cols: usize,
    rows: usize,
}

impl TerminalSize {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(1) as usize,
            rows: rows.max(1) as usize,
        }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

pub(crate) fn encode_key(
    keystroke: &Keystroke,
    app_cursor_mode: bool,
    option_as_meta: bool,
) -> Option<Vec<u8>> {
    zed_like_to_esc_str(keystroke, app_cursor_mode, option_as_meta)
        .map(|text| text.into_owned().into_bytes())
}

#[derive(Debug, PartialEq, Eq)]
enum TerminalModifiers {
    None,
    Alt,
    Ctrl,
    Shift,
    CtrlShift,
    Other,
}

impl TerminalModifiers {
    fn new(ks: &Keystroke) -> Self {
        match (
            ks.modifiers.alt,
            ks.modifiers.control,
            ks.modifiers.shift,
            ks.modifiers.platform,
        ) {
            (false, false, false, false) => Self::None,
            (true, false, false, false) => Self::Alt,
            (false, true, false, false) => Self::Ctrl,
            (false, false, true, false) => Self::Shift,
            (false, true, true, false) => Self::CtrlShift,
            _ => Self::Other,
        }
    }

    fn any(&self) -> bool {
        !matches!(self, Self::None)
    }
}

fn zed_like_to_esc_str(
    keystroke: &Keystroke,
    app_cursor_mode: bool,
    option_as_meta: bool,
) -> Option<std::borrow::Cow<'static, str>> {
    let modifiers = TerminalModifiers::new(keystroke);
    let key = keystroke.key.to_ascii_lowercase();

    let manual_esc_str = match (key.as_str(), &modifiers) {
        ("tab", TerminalModifiers::None) => Some("\x09"),
        ("tab", TerminalModifiers::Shift) => Some("\x1b[Z"),
        ("escape", TerminalModifiers::None) => Some("\x1b"),
        ("enter", TerminalModifiers::None) => Some("\x0d"),
        ("enter", TerminalModifiers::Shift) => Some("\x0a"),
        ("enter", TerminalModifiers::Alt) => Some("\x1b\x0d"),
        ("backspace", TerminalModifiers::None) => Some("\x7f"),
        ("backspace", TerminalModifiers::Ctrl) => Some("\x08"),
        ("backspace", TerminalModifiers::Alt) => Some("\x1b\x7f"),
        ("backspace", TerminalModifiers::Shift) => Some("\x7f"),
        ("space", TerminalModifiers::Ctrl) => Some("\x00"),
        ("home", TerminalModifiers::None) if app_cursor_mode => Some("\x1bOH"),
        ("home", TerminalModifiers::None) if !app_cursor_mode => Some("\x1b[H"),
        ("end", TerminalModifiers::None) if app_cursor_mode => Some("\x1bOF"),
        ("end", TerminalModifiers::None) if !app_cursor_mode => Some("\x1b[F"),
        ("up", TerminalModifiers::None) if app_cursor_mode => Some("\x1bOA"),
        ("up", TerminalModifiers::None) if !app_cursor_mode => Some("\x1b[A"),
        ("down", TerminalModifiers::None) if app_cursor_mode => Some("\x1bOB"),
        ("down", TerminalModifiers::None) if !app_cursor_mode => Some("\x1b[B"),
        ("right", TerminalModifiers::None) if app_cursor_mode => Some("\x1bOC"),
        ("right", TerminalModifiers::None) if !app_cursor_mode => Some("\x1b[C"),
        ("left", TerminalModifiers::None) if app_cursor_mode => Some("\x1bOD"),
        ("left", TerminalModifiers::None) if !app_cursor_mode => Some("\x1b[D"),
        ("insert", TerminalModifiers::None) => Some("\x1b[2~"),
        ("delete", TerminalModifiers::None) => Some("\x1b[3~"),
        ("pageup", TerminalModifiers::None) => Some("\x1b[5~"),
        ("pagedown", TerminalModifiers::None) => Some("\x1b[6~"),
        ("a", TerminalModifiers::Ctrl) | ("A", TerminalModifiers::CtrlShift) => Some("\x01"),
        ("b", TerminalModifiers::Ctrl) | ("B", TerminalModifiers::CtrlShift) => Some("\x02"),
        ("c", TerminalModifiers::Ctrl) | ("C", TerminalModifiers::CtrlShift) => Some("\x03"),
        ("d", TerminalModifiers::Ctrl) | ("D", TerminalModifiers::CtrlShift) => Some("\x04"),
        ("e", TerminalModifiers::Ctrl) | ("E", TerminalModifiers::CtrlShift) => Some("\x05"),
        ("f", TerminalModifiers::Ctrl) | ("F", TerminalModifiers::CtrlShift) => Some("\x06"),
        ("g", TerminalModifiers::Ctrl) | ("G", TerminalModifiers::CtrlShift) => Some("\x07"),
        ("h", TerminalModifiers::Ctrl) | ("H", TerminalModifiers::CtrlShift) => Some("\x08"),
        ("i", TerminalModifiers::Ctrl) | ("I", TerminalModifiers::CtrlShift) => Some("\x09"),
        ("j", TerminalModifiers::Ctrl) | ("J", TerminalModifiers::CtrlShift) => Some("\x0a"),
        ("k", TerminalModifiers::Ctrl) | ("K", TerminalModifiers::CtrlShift) => Some("\x0b"),
        ("l", TerminalModifiers::Ctrl) | ("L", TerminalModifiers::CtrlShift) => Some("\x0c"),
        ("m", TerminalModifiers::Ctrl) | ("M", TerminalModifiers::CtrlShift) => Some("\x0d"),
        ("n", TerminalModifiers::Ctrl) | ("N", TerminalModifiers::CtrlShift) => Some("\x0e"),
        ("o", TerminalModifiers::Ctrl) | ("O", TerminalModifiers::CtrlShift) => Some("\x0f"),
        ("p", TerminalModifiers::Ctrl) | ("P", TerminalModifiers::CtrlShift) => Some("\x10"),
        ("q", TerminalModifiers::Ctrl) | ("Q", TerminalModifiers::CtrlShift) => Some("\x11"),
        ("r", TerminalModifiers::Ctrl) | ("R", TerminalModifiers::CtrlShift) => Some("\x12"),
        ("s", TerminalModifiers::Ctrl) | ("S", TerminalModifiers::CtrlShift) => Some("\x13"),
        ("t", TerminalModifiers::Ctrl) | ("T", TerminalModifiers::CtrlShift) => Some("\x14"),
        ("u", TerminalModifiers::Ctrl) | ("U", TerminalModifiers::CtrlShift) => Some("\x15"),
        ("v", TerminalModifiers::Ctrl) | ("V", TerminalModifiers::CtrlShift) => Some("\x16"),
        ("w", TerminalModifiers::Ctrl) | ("W", TerminalModifiers::CtrlShift) => Some("\x17"),
        ("x", TerminalModifiers::Ctrl) | ("X", TerminalModifiers::CtrlShift) => Some("\x18"),
        ("y", TerminalModifiers::Ctrl) | ("Y", TerminalModifiers::CtrlShift) => Some("\x19"),
        ("z", TerminalModifiers::Ctrl) | ("Z", TerminalModifiers::CtrlShift) => Some("\x1a"),
        ("@", TerminalModifiers::Ctrl) => Some("\x00"),
        ("[", TerminalModifiers::Ctrl) => Some("\x1b"),
        ("\\", TerminalModifiers::Ctrl) => Some("\x1c"),
        ("]", TerminalModifiers::Ctrl) => Some("\x1d"),
        ("^", TerminalModifiers::Ctrl) => Some("\x1e"),
        ("_", TerminalModifiers::Ctrl) => Some("\x1f"),
        ("?", TerminalModifiers::Ctrl) => Some("\x7f"),
        _ => None,
    };
    if let Some(esc) = manual_esc_str {
        return Some(esc.into());
    }

    if modifiers.any() {
        let modifier_code = modifier_code(keystroke);
        let modified = match key.as_str() {
            "up" => Some(format!("\x1b[1;{}A", modifier_code)),
            "down" => Some(format!("\x1b[1;{}B", modifier_code)),
            "right" => Some(format!("\x1b[1;{}C", modifier_code)),
            "left" => Some(format!("\x1b[1;{}D", modifier_code)),
            "insert" => Some(format!("\x1b[2;{}~", modifier_code)),
            "pageup" => Some(format!("\x1b[5;{}~", modifier_code)),
            "pagedown" => Some(format!("\x1b[6;{}~", modifier_code)),
            "end" => Some(format!("\x1b[1;{}F", modifier_code)),
            "home" => Some(format!("\x1b[1;{}H", modifier_code)),
            _ => None,
        };
        if let Some(esc) = modified {
            return Some(esc.into());
        }
    }

    if !cfg!(target_os = "macos") || option_as_meta {
        let is_alt_lowercase_ascii =
            modifiers == TerminalModifiers::Alt && keystroke.key.is_ascii();
        let is_alt_uppercase_ascii =
            keystroke.modifiers.alt && keystroke.modifiers.shift && keystroke.key.is_ascii();
        if is_alt_lowercase_ascii || is_alt_uppercase_ascii {
            let key = if is_alt_uppercase_ascii {
                keystroke.key.to_ascii_uppercase()
            } else {
                keystroke.key.clone()
            };
            return Some(format!("\x1b{}", key).into());
        }
    }

    if let Some(text) = &keystroke.key_char {
        return Some(text.clone().into());
    }

    if keystroke.key.len() == 1 {
        return Some(keystroke.key.clone().into());
    }

    None
}

fn modifier_code(keystroke: &Keystroke) -> u32 {
    let mut modifier_code = 0;
    if keystroke.modifiers.shift {
        modifier_code |= 1;
    }
    if keystroke.modifiers.alt {
        modifier_code |= 1 << 1;
    }
    if keystroke.modifiers.control {
        modifier_code |= 1 << 2;
    }
    modifier_code + 1
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TransferType {
    Upload,
    Download,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) enum TransferState {
    Running,
    Paused,
    Completed,
    Failed(String),
    Interrupted(String), // 中断传输：包含原因（例如 "User cancelled", "Network timeout"）
    Zombie(String),      // 程序重启后残留的 Running/Paused 任务
                         // 兼容 v0.3.11 -> v0.4.x：旧配置里曾保存过 `Cancelled`，
                         // 新版本改成了带原因的状态，因此要手动接住旧枚举值。
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
enum TransferStateCompat {
    Running,
    Paused,
    Completed,
    Failed(String),
    Interrupted(String),
    Zombie(String),
    Cancelled,
}

impl From<TransferStateCompat> for TransferState {
    fn from(value: TransferStateCompat) -> Self {
        match value {
            TransferStateCompat::Running => Self::Running,
            TransferStateCompat::Paused => Self::Paused,
            TransferStateCompat::Completed => Self::Completed,
            TransferStateCompat::Failed(reason) => Self::Failed(reason),
            TransferStateCompat::Interrupted(reason) => Self::Interrupted(reason),
            TransferStateCompat::Zombie(reason) => Self::Zombie(reason),
            TransferStateCompat::Cancelled => Self::Interrupted("Cancelled".to_string()),
        }
    }
}

impl<'de> serde::Deserialize<'de> for TransferState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        TransferStateCompat::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TransferInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    pub target: String,
    pub kind: TransferType,
    pub total_bytes: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Transfer {
    pub tab_id: String,
    pub tab_title: String,
    pub info: TransferInfo,
    pub transferred: u64,
    pub total: Option<u64>,
    pub state: TransferState,
}
