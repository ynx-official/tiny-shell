use std::collections::HashMap;

use crate::terminal::{BackendEvent, BackendEventEnvelope};

pub(super) fn coalesce_backend_events(
    events: Vec<BackendEventEnvelope>,
) -> Vec<BackendEventEnvelope> {
    let mut result: Vec<BackendEventEnvelope> = Vec::with_capacity(events.len());
    let mut output_positions: HashMap<String, usize> = HashMap::new();
    let mut latest_positions: HashMap<(u8, String), usize> = HashMap::new();
    for envelope in events {
        let latest_key = match &envelope.event {
            BackendEvent::TransferProgress { id, .. } => Some((0, id.clone())),
            BackendEvent::SftpLatency { tab_id, .. } => Some((1, tab_id.clone())),
            BackendEvent::RemoteSystem { tab_id, .. } => Some((2, tab_id.clone())),
            BackendEvent::RemoteDesktopFrameReady { tab_id, .. } => Some((3, tab_id.clone())),
            _ => None,
        };
        if let Some(key) = latest_key {
            if let Some(position) = latest_positions.get(&key).copied() {
                result[position] = envelope;
            } else {
                latest_positions.insert(key, result.len());
                result.push(envelope);
            }
            continue;
        }
        if let BackendEvent::Output { tab_id, bytes } = &envelope.event {
            if let Some(position) = output_positions.get(tab_id).copied()
                && let Some(existing) = result.get_mut(position)
                && let BackendEvent::Output { bytes: current, .. } = &mut existing.event
            {
                current.extend(bytes);
                existing.sequence = envelope.sequence;
                continue;
            }
            output_positions.insert(tab_id.clone(), result.len());
        } else {
            output_positions.clear();
        }
        result.push(envelope);
    }
    result.sort_by_key(|event| event.sequence);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(event: BackendEvent, sequence: u64) -> BackendEventEnvelope {
        BackendEventEnvelope {
            event,
            generation: 1,
            sequence,
        }
    }

    #[test]
    fn output_before_closed_by_sequence() {
        let result = coalesce_backend_events(vec![
            envelope(
                BackendEvent::Output {
                    tab_id: "a".into(),
                    bytes: b"one".to_vec(),
                },
                1,
            ),
            envelope(
                BackendEvent::Closed {
                    tab_id: "a".into(),
                    reason: "closed".into(),
                },
                2,
            ),
        ]);
        assert!(matches!(result[0].event, BackendEvent::Output { .. }));
        assert!(matches!(result[1].event, BackendEvent::Closed { .. }));
    }
}
