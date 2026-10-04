use super::{parsing, storage, SnapshotState, ThreadSummary};
use std::{collections::HashMap, path::Path};

// Immutable rollout IDs, rather than the active thread ID, form the history
// graph. Missing snapshots cannot stand in for files the native reader needs.
pub(super) fn apply(threads: &mut [ThreadSummary]) {
    let mut available = HashMap::new();
    for (position, thread) in threads.iter().enumerate() {
        if thread.integrity == "valid"
            && super::same_source_version(thread, Path::new(&thread.path))
        {
            if let Some(id) = parsing::filename_rollout_id(Path::new(&thread.path)) {
                available
                    .entry((thread.source_id.clone(), id))
                    .or_insert(position);
            }
        }
    }
    let parents: Vec<_> = threads
        .iter()
        .map(|thread| {
            thread.history_base.as_ref().and_then(|base| {
                available
                    .get(&(thread.source_id.clone(), base.thread_id.clone()))
                    .copied()
            })
        })
        .collect();
    let mut checked_prefixes = HashMap::new();
    let prefix_valid: Vec<_> = threads
        .iter()
        .zip(&parents)
        .map(|(thread, parent)| {
            let Some(required) = thread.history_base.as_ref() else {
                return true;
            };
            let Some(parent) = parent else {
                return false;
            };
            let key = (
                thread.source_id.clone(),
                required.thread_id.clone(),
                required.end_ordinal_exclusive,
                required.end_byte_offset,
            );
            *checked_prefixes.entry(key).or_insert_with(|| {
                storage::validate_history_prefix(&threads[*parent], required).is_ok()
            })
        })
        .collect();
    // 0: unchecked, 1: this traversal, 2: complete, 3: missing, 4: cycle.
    // Each node has at most one parent; iterative traversal remains bounded
    // even for long revert/fork chains and visits every node at most once.
    let mut state = vec![0u8; threads.len()];
    for start in 0..threads.len() {
        if state[start] != 0 || !matches!(threads[start].integrity.as_str(), "valid" | "missing") {
            continue;
        }
        let mut chain = Vec::new();
        let mut cursor = start;
        let mut outcome = loop {
            match state[cursor] {
                1 => break 4,
                finished @ 2..=4 => break finished,
                _ => {}
            }
            state[cursor] = 1;
            chain.push(cursor);
            if threads[cursor].history_base.is_none() {
                break 2;
            }
            match parents[cursor] {
                Some(parent) => cursor = parent,
                None => break 3,
            }
        };
        for node in chain.into_iter().rev() {
            if outcome == 2 && !prefix_valid[node] {
                outcome = 3;
            }
            state[node] = outcome;
        }
    }
    for (thread, state) in threads.iter_mut().zip(state) {
        if state == 3 || state == 4 {
            thread.integrity = if state == 4 {
                "dependency-cycle"
            } else {
                "dependency-missing"
            }
            .into();
            thread.recoverability = "dependencies-missing".into();
            thread.snapshot = SnapshotState::Failed;
        }
    }
}
