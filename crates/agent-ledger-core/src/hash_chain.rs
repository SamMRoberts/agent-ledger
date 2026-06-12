use serde_json::Value;
use thiserror::Error;

use crate::event::{Event, EventType};

pub fn compute_payload_hash(payload: &Value) -> String {
    let bytes = serde_json::to_vec(payload).expect("serializing JSON value for hashing should not fail");
    hex::encode(blake3::hash(&bytes).as_bytes())
}

pub fn compute_event_hash(
    seq: u64,
    timestamp: &str,
    session_id: &str,
    event_type: &EventType,
    payload_hash: &str,
    prev_hash: &str,
) -> String {
    let material = format!(
        "{}|{}|{}|{}|{}|{}",
        seq,
        timestamp,
        session_id,
        event_type.as_str(),
        payload_hash,
        prev_hash
    );
    hex::encode(blake3::hash(material.as_bytes()).as_bytes())
}

#[derive(Debug, Error)]
pub enum HashChainError {
    #[error("sequence mismatch at event {index}: expected {expected}, found {found}")]
    SequenceMismatch {
        index: usize,
        expected: u64,
        found: u64,
    },
    #[error("previous hash mismatch at event {index}: expected {expected}, found {found}")]
    PrevHashMismatch {
        index: usize,
        expected: String,
        found: String,
    },
    #[error("payload hash mismatch at event {index}: expected {expected}, found {found}")]
    PayloadHashMismatch {
        index: usize,
        expected: String,
        found: String,
    },
    #[error("event hash mismatch at event {index}: expected {expected}, found {found}")]
    EventHashMismatch {
        index: usize,
        expected: String,
        found: String,
    },
}

pub fn verify_chain(events: &[Event]) -> Result<(), HashChainError> {
    let mut expected_prev_hash = "genesis".to_owned();
    for (index, event) in events.iter().enumerate() {
        let expected_seq = index as u64;
        if event.seq != expected_seq {
            return Err(HashChainError::SequenceMismatch {
                index,
                expected: expected_seq,
                found: event.seq,
            });
        }
        if event.prev_hash != expected_prev_hash {
            return Err(HashChainError::PrevHashMismatch {
                index,
                expected: expected_prev_hash,
                found: event.prev_hash.clone(),
            });
        }
        let expected_payload_hash = compute_payload_hash(&event.payload);
        if event.payload_hash != expected_payload_hash {
            return Err(HashChainError::PayloadHashMismatch {
                index,
                expected: expected_payload_hash,
                found: event.payload_hash.clone(),
            });
        }
        let expected_event_hash = compute_event_hash(
            event.seq,
            &event.timestamp,
            &event.session_id,
            &event.event_type,
            &event.payload_hash,
            &event.prev_hash,
        );
        if event.event_hash != expected_event_hash {
            return Err(HashChainError::EventHashMismatch {
                index,
                expected: expected_event_hash,
                found: event.event_hash.clone(),
            });
        }
        expected_prev_hash = event.event_hash.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn make_event(seq: u64, prev_hash: &str, payload: Value) -> Event {
        let timestamp = "2026-06-01T12:00:00Z".to_string();
        let session_id = "session-test".to_string();
        let event_type = EventType::SessionStarted;
        let payload_hash = compute_payload_hash(&payload);
        let event_hash = compute_event_hash(
            seq,
            &timestamp,
            &session_id,
            &event_type,
            &payload_hash,
            prev_hash,
        );
        Event {
            seq,
            timestamp,
            session_id,
            event_type,
            payload,
            payload_hash,
            prev_hash: prev_hash.to_string(),
            event_hash,
        }
    }

    #[test]
    fn valid_chain_verifies_successfully() {
        let first = make_event(0, "genesis", json!({"a": 1}));
        let second = make_event(1, &first.event_hash, json!({"b": 2}));
        verify_chain(&[first, second]).expect("chain should verify");
    }

    #[test]
    fn tampered_event_fails_verification() {
        let first = make_event(0, "genesis", json!({"a": 1}));
        let mut second = make_event(1, &first.event_hash, json!({"b": 2}));
        second.payload = json!({"b": 999});
        let err = verify_chain(&[first, second]).unwrap_err();
        assert!(matches!(err, HashChainError::PayloadHashMismatch { .. }));
    }

    #[test]
    fn reordered_events_fail_verification() {
        let first = make_event(0, "genesis", json!({"a": 1}));
        let second = make_event(1, &first.event_hash, json!({"b": 2}));
        let err = verify_chain(&[second, first]).unwrap_err();
        assert!(matches!(err, HashChainError::SequenceMismatch { .. }));
    }

    #[test]
    fn removed_event_fails_verification() {
        let first = make_event(0, "genesis", json!({"a": 1}));
        let second = make_event(1, &first.event_hash, json!({"b": 2}));
        let third = make_event(2, &second.event_hash, json!({"c": 3}));
        let err = verify_chain(&[first, third]).unwrap_err();
        assert!(matches!(err, HashChainError::SequenceMismatch { .. }));
    }
}
