use std::collections::HashMap;

use crate::terminal::BackendEvent;

/// Collapse high-frequency, replaceable backend updates before they reach the UI.
/// Ordering barriers are kept for terminal output, while progress/metrics retain
/// only their newest queued value.
pub(super) fn coalesce_backend_events(events: Vec<BackendEvent>) -> Vec<BackendEvent> {
    let mut coalesced: Vec<Option<BackendEvent>> = Vec::with_capacity(events.len());
    let mut output_positions: HashMap<String, usize> = HashMap::new();
    let mut progress_positions: HashMap<String, usize> = HashMap::new();
    let mut latency_positions: HashMap<String, usize> = HashMap::new();
    let mut remote_system_positions: HashMap<String, usize> = HashMap::new();

    for event in events {
        let latest_key = match &event {
            BackendEvent::TransferProgress { id, .. } => Some((0, id.clone())),
            BackendEvent::SftpLatency { tab_id, .. } => Some((1, tab_id.clone())),
            BackendEvent::RemoteSystem { tab_id, .. } => Some((2, tab_id.clone())),
            _ => None,
        };
        if let Some((kind, key)) = latest_key {
            let positions = match kind {
                0 => &mut progress_positions,
                1 => &mut latency_positions,
                _ => &mut remote_system_positions,
            };
            replace_latest(&mut coalesced, positions, key, event);
            continue;
        }
        match event {
            BackendEvent::Output { tab_id, bytes } => {
                if let Some(position) = output_positions.get(&tab_id).copied()
                    && let Some(Some(BackendEvent::Output {
                        bytes: existing, ..
                    })) = coalesced.get_mut(position)
                {
                    existing.extend(bytes);
                } else {
                    let position = coalesced.len();
                    output_positions.insert(tab_id.clone(), position);
                    coalesced.push(Some(BackendEvent::Output { tab_id, bytes }));
                }
            }
            event => {
                // Status/connection/control events are ordering barriers for output.
                output_positions.clear();
                coalesced.push(Some(event));
            }
        }
    }

    coalesced.into_iter().flatten().collect()
}

fn replace_latest(
    events: &mut Vec<Option<BackendEvent>>,
    positions: &mut HashMap<String, usize>,
    key: String,
    event: BackendEvent,
) {
    if let Some(position) = positions.get(&key).copied() {
        events[position] = Some(event);
    } else {
        positions.insert(key, events.len());
        events.push(Some(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::TransferState;

    #[test]
    fn coalesces_interleaved_output_until_an_ordering_barrier() {
        let events = vec![
            output("a", b"one"),
            output("b", b"two"),
            output("a", b"three"),
            BackendEvent::Status {
                tab_id: "a".into(),
                text: "ready".into(),
            },
            output("a", b"four"),
        ];

        let result = coalesce_backend_events(events);
        assert_eq!(result.len(), 4);
        assert!(matches!(
            &result[0],
            BackendEvent::Output { tab_id, bytes }
                if tab_id == "a" && bytes == b"onethree"
        ));
        assert!(matches!(
            &result[3],
            BackendEvent::Output { tab_id, bytes }
                if tab_id == "a" && bytes == b"four"
        ));
    }

    #[test]
    fn keeps_only_latest_transfer_progress() {
        let result = coalesce_backend_events(vec![progress(10), progress(80)]);
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            BackendEvent::TransferProgress {
                transferred: 80,
                ..
            }
        ));
    }

    fn output(tab_id: &str, bytes: &[u8]) -> BackendEvent {
        BackendEvent::Output {
            tab_id: tab_id.into(),
            bytes: bytes.to_vec(),
        }
    }

    fn progress(transferred: u64) -> BackendEvent {
        BackendEvent::TransferProgress {
            tab_id: "a".into(),
            id: "transfer".into(),
            transferred,
            total: Some(100),
            state: TransferState::Running,
        }
    }
}
