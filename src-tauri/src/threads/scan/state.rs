use super::{files, ThreadSummary};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

pub(super) struct StateIndex {
    readable: bool,
    rows: HashMap<String, Row>,
}

struct Row {
    path: String,
    title: Option<String>,
}

impl StateIndex {
    pub(super) fn unavailable() -> Self {
        Self {
            readable: false,
            rows: HashMap::new(),
        }
    }
    pub(super) fn apply(&self, thread: &mut ThreadSummary) {
        if !self.readable {
            thread.state_index = Some("unavailable".into());
            thread.selected_rollout = None;
            return;
        }
        match self.rows.get(&thread.thread_id) {
            Some(row) => {
                thread.state_index = Some("indexed".into());
                let stored = Path::new(&row.path);
                let actual = Path::new(&thread.path);
                let selected = rollout_identity(stored) == rollout_identity(actual);
                thread.selected_rollout = Some(selected);
                if selected
                    && !thread.index_present
                    && row
                        .title
                        .as_ref()
                        .is_some_and(|title| !title.trim().is_empty())
                {
                    thread.title = row.title.as_ref().map(|title| {
                        super::redaction::text(&title.chars().take(256).collect::<String>())
                    });
                }
            }
            None => {
                thread.state_index = Some("missing".into());
                thread.selected_rollout = None;
            }
        }
    }
}

fn rollout_identity(path: &Path) -> String {
    // SQLite may retain Windows 8.3 aliases while discovery uses long paths.
    // Keep the lexical fallback for records whose source file is now missing.
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    files::normalized_path(&resolved)
        .trim_end_matches(".zst")
        .to_owned()
}

pub(super) fn read(root: &Path, sqlite_home: Option<&Path>) -> StateIndex {
    match read_rows(sqlite_home.unwrap_or(root)) {
        Ok(rows) => StateIndex {
            readable: true,
            rows,
        },
        Err(_) => StateIndex::unavailable(),
    }
}

fn read_rows(root: &Path) -> Result<HashMap<String, Row>, rusqlite::Error> {
    let path = root.join("state_5.sqlite");
    files::validate_path(root, &path).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_millis(100))?;
    connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
    let columns: HashSet<String> = connection
        .prepare("PRAGMA table_info(threads)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    if !columns.contains("id") || !columns.contains("rollout_path") {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let query = if columns.contains("title") {
        "SELECT id, rollout_path, title FROM threads LIMIT 200001"
    } else {
        "SELECT id, rollout_path, NULL FROM threads LIMIT 200001"
    };
    let mut result = HashMap::new();
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            Row {
                path: row.get(1)?,
                title: row.get(2)?,
            },
        ))
    })?;
    for row in rows {
        let (id, row) = row?;
        if result.len() >= 200_000 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if uuid::Uuid::parse_str(&id).is_ok() {
            result.insert(id, row);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_rollout_paths_keep_the_lexical_fallback_and_compression_identity() {
        let directory = tempfile::tempdir().unwrap();
        let plain = directory.path().join("missing").join("rollout.jsonl");
        let compressed = plain.with_extension("jsonl.zst");
        assert!(!plain.exists());
        assert!(!compressed.exists());
        assert_eq!(rollout_identity(&plain), files::normalized_path(&plain));
        assert_eq!(rollout_identity(&plain), rollout_identity(&compressed));
    }
}
