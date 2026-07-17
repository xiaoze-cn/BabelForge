use crate::error::{BfxError, Result};
use chrono::Local;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Deserialize, Serialize)]
pub struct TaskPlan {
    pub pages: String,
    pub language: String,
    pub format: String,
    pub destination: String,
    pub watermark: bool,
    pub engine_model: String,
    pub engine_url: String,
}

pub struct Store {
    path: PathBuf,
}

pub struct Task {
    pub id: String,
    pub input: String,
    pub model: String,
    pub state: String,
    pub plan_text: String,
    pub input_hash: Option<String>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub pair: Option<String>,
    pub pair_hash: Option<String>,
    pub mono: Option<String>,
    pub duration_ms: Option<i64>,
}

pub struct WorkItem {
    pub id: String,
    pub input: PathBuf,
    pub model: String,
    pub plan: TaskPlan,
}

pub struct NewTask {
    pub input: PathBuf,
    pub model: String,
    pub preset: String,
    pub plan: TaskPlan,
    pub priority: bool,
}

pub struct Replacement {
    pub id: i64,
    pub backup: PathBuf,
    pub source_hash: String,
    pub pair_hash: String,
}

impl Store {
    pub fn open(path: PathBuf) -> Result<Self> {
        let store = Self { path };
        let connection = store.connection()?;
        connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                CREATE TABLE IF NOT EXISTS tasks (
                    id TEXT PRIMARY KEY,
                    input_path TEXT NOT NULL,
                    model TEXT NOT NULL,
                    preset TEXT NOT NULL,
                    priority INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    stop_requested INTEGER NOT NULL DEFAULT 0,
                    plan_text TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    started_at TEXT,
                    finished_at TEXT,
                    input_hash TEXT,
                    pair_path TEXT,
                    pair_hash TEXT,
                    mono_path TEXT,
                    mono_hash TEXT,
                    duration_ms INTEGER,
                    error_code TEXT,
                    error_detail TEXT
                );
                CREATE INDEX IF NOT EXISTS tasks_order ON tasks (state, priority DESC, created_at, id);
                CREATE INDEX IF NOT EXISTS tasks_input ON tasks (input_path, created_at DESC);
                CREATE TABLE IF NOT EXISTS replacements (
                    id INTEGER PRIMARY KEY,
                    source_path TEXT NOT NULL,
                    backup_path TEXT NOT NULL,
                    task_id TEXT NOT NULL,
                    source_hash TEXT NOT NULL,
                    pair_hash TEXT NOT NULL,
                    replaced_at TEXT NOT NULL,
                    undone_at TEXT
                );
                CREATE INDEX IF NOT EXISTS replacements_source ON replacements (source_path, replaced_at DESC);
                ",
            )
            .map_err(db_error)?;
        ensure_column(&connection, "stop_requested")?;
        remove_plan_keys(&connection)?;
        Ok(store)
    }

    pub fn enqueue_many(&self, tasks: &[NewTask]) -> Result<Vec<String>> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let mut ids = Vec::with_capacity(tasks.len());
        for task in tasks {
            let id = next_id(&transaction)?;
            let plan = toml::to_string(&task.plan).map_err(|error| {
                BfxError::storage(format!("Cannot save the translation plan ({error})"))
            })?;
            transaction
                .execute(
                    "INSERT INTO tasks (id, input_path, model, preset, priority, state, plan_text, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'QUE', ?6, ?7)",
                    params![id, task.input.display().to_string(), task.model, task.preset, i64::from(task.priority), plan, now()],
                )
                .map_err(db_error)?;
            ids.push(id);
        }
        transaction.commit().map_err(db_error)?;
        Ok(ids)
    }

    pub fn claim(&self) -> Result<Option<WorkItem>> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let row: Option<(String, String, String, String)> = transaction
            .query_row(
                "SELECT id, input_path, model, plan_text FROM tasks WHERE state = 'QUE' ORDER BY priority DESC, created_at, id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(db_error)?;
        let Some((id, input, model, plan_text)) = row else {
            transaction.commit().map_err(db_error)?;
            return Ok(None);
        };
        let plan = toml::from_str(&plan_text).map_err(|error| {
            BfxError::storage(format!("Cannot read the queued translation plan ({error})"))
        })?;
        transaction
            .execute(
                "UPDATE tasks SET state = 'RUN', started_at = ?2 WHERE id = ?1 AND state = 'QUE'",
                params![id, now()],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(Some(WorkItem {
            id,
            input: PathBuf::from(input),
            model,
            plan,
        }))
    }

    pub fn recover(&self) -> Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE tasks SET state = 'STP', finished_at = ?1 WHERE state = 'RUN' AND stop_requested = 1",
                [now()],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE tasks SET state = 'QUE', started_at = NULL WHERE state = 'RUN' AND stop_requested = 0",
                [],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn finish(
        &self,
        id: &str,
        input_hash: &str,
        pair: Option<(&Path, &str)>,
        mono: Option<(&Path, &str)>,
        duration_ms: i64,
    ) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE tasks SET state = 'FIN', finished_at = ?2, input_hash = ?3, pair_path = ?4, pair_hash = ?5, mono_path = ?6, mono_hash = ?7, duration_ms = ?8 WHERE id = ?1 AND state = 'RUN' AND stop_requested = 0",
                params![
                    id,
                    now(),
                    input_hash,
                    pair.map(|value| value.0.display().to_string()),
                    pair.map(|value| value.1),
                    mono.map(|value| value.0.display().to_string()),
                    mono.map(|value| value.1),
                    duration_ms,
                ],
            )
            .map_err(db_error)?;
        if changed != 0 {
            return Ok(());
        }
        let stopped = connection
            .execute(
                "UPDATE tasks SET state = 'STP', finished_at = ?2, duration_ms = ?3 WHERE id = ?1 AND state = 'RUN' AND stop_requested = 1",
                params![id, now(), duration_ms],
            )
            .map_err(db_error)?;
        if stopped != 0 {
            return Ok(());
        }
        Err(BfxError::storage(format!(
            "Cannot complete task \"{id}\" because its state changed"
        )))
    }

    pub fn fail(&self, id: &str, code: &str, detail: &str, duration_ms: i64) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE tasks SET state = 'ERR', finished_at = ?2, error_code = ?3, error_detail = ?4, duration_ms = ?5 WHERE id = ?1 AND state = 'RUN' AND stop_requested = 0",
                params![id, now(), code, detail, duration_ms],
            )
            .map_err(db_error)?;
        if changed != 0 {
            return Ok(());
        }
        let stopped = connection
            .execute(
                "UPDATE tasks SET state = 'STP', finished_at = ?2, duration_ms = ?3 WHERE id = ?1 AND state = 'RUN' AND stop_requested = 1",
                params![id, now(), duration_ms],
            )
            .map_err(db_error)?;
        if stopped != 0 {
            return Ok(());
        }
        Err(BfxError::storage(format!(
            "Cannot record task \"{id}\" because its state changed"
        )))
    }

    pub fn stop(&self, id: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE tasks SET state = 'STP', finished_at = ?2 WHERE id = ?1 AND state = 'QUE'",
                params![id, now()],
            )
            .map_err(db_error)?;
        if changed != 0 {
            return Ok(());
        }
        let changed = connection
            .execute(
                "UPDATE tasks SET stop_requested = 1 WHERE id = ?1 AND state = 'RUN' AND stop_requested = 0",
                [id],
            )
            .map_err(db_error)?;
        if changed != 0 {
            return Ok(());
        }
        let state: Option<String> = connection
            .query_row("SELECT state FROM tasks WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)?;
        match state {
            Some(state) => Err(BfxError::input(format!(
                "Task \"{id}\" cannot be stopped because state is {state}"
            ))),
            None => Err(BfxError::input(format!("Task \"{id}\" is not available"))),
        }
    }

    pub fn stopped(&self, id: &str) -> Result<bool> {
        self.connection()?
            .query_row(
                "SELECT state = 'STP' OR stop_requested = 1 FROM tasks WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)
            .map(|value| value.unwrap_or(false))
    }

    pub fn finish_stop(&self, id: &str, duration_ms: i64) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE tasks SET state = 'STP', finished_at = ?2, duration_ms = ?3 WHERE id = ?1 AND state = 'RUN' AND stop_requested = 1",
                params![id, now(), duration_ms],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn retry(&self, id: &str) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE tasks SET state = 'QUE', stop_requested = 0, started_at = NULL, finished_at = NULL, input_hash = NULL, pair_path = NULL, pair_hash = NULL, mono_path = NULL, mono_hash = NULL, duration_ms = NULL, error_code = NULL, error_detail = NULL WHERE id = ?1 AND state IN ('STP', 'ERR')",
                [id],
            )
            .map_err(db_error)?;
        if changed != 0 {
            return Ok(());
        }
        let state: Option<String> = connection
            .query_row("SELECT state FROM tasks WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)?;
        match state {
            Some(state) => Err(BfxError::input(format!(
                "Task \"{id}\" cannot be retried because state is {state}"
            ))),
            None => Err(BfxError::input(format!("Task \"{id}\" is not available"))),
        }
    }

    pub fn get(&self, id: &str) -> Result<Option<Task>> {
        self.connection()?
            .query_row(
                &format!("SELECT {TASK_FIELDS} FROM tasks WHERE id = ?1"),
                [id],
                read_task,
            )
            .optional()
            .map_err(db_error)
    }

    pub fn list(
        &self,
        states: &[String],
        from: Option<&str>,
        to: Option<&str>,
        limit: u32,
        asc: bool,
    ) -> Result<Vec<Task>> {
        let connection = self.connection()?;
        let order = if asc { "ASC" } else { "DESC" };
        let mut filters = Vec::new();
        let mut values = Vec::new();
        if !states.is_empty() {
            let marks = std::iter::repeat_n("?", states.len())
                .collect::<Vec<_>>()
                .join(", ");
            filters.push(format!("state IN ({marks})"));
            values.extend(states.iter().cloned().map(Value::Text));
        }
        if let Some(from) = from {
            filters.push("id >= ?".to_owned());
            values.push(Value::Text(from.to_owned()));
        }
        if let Some(to) = to {
            filters.push("id <= ?".to_owned());
            values.push(Value::Text(to.to_owned()));
        }
        let condition = if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        };
        let query =
            format!("SELECT {TASK_FIELDS} FROM tasks{condition} ORDER BY id {order} LIMIT ?");
        values.push(Value::Integer(i64::from(limit)));
        let mut statement = connection.prepare(&query).map_err(db_error)?;
        let mut rows = Vec::new();
        let result = statement
            .query_map(params_from_iter(values), read_task)
            .map_err(db_error)?;
        for item in result {
            rows.push(item.map_err(db_error)?);
        }
        Ok(rows)
    }

    pub fn find_pair(&self, input: &Path) -> Result<Option<Task>> {
        self.connection()?
            .query_row(
                &format!("SELECT {TASK_FIELDS} FROM tasks WHERE input_path = ?1 AND state = 'FIN' AND pair_path IS NOT NULL ORDER BY created_at DESC LIMIT 1"),
                [input.display().to_string()],
                read_task,
            )
            .optional()
            .map_err(db_error)
    }

    pub fn save_replace(
        &self,
        source: &Path,
        backup: &Path,
        task_id: &str,
        source_hash: &str,
        pair_hash: &str,
    ) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO replacements (source_path, backup_path, task_id, source_hash, pair_hash, replaced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![source.display().to_string(), backup.display().to_string(), task_id, source_hash, pair_hash, now()],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn replacement(&self, source: &Path) -> Result<Option<Replacement>> {
        self.connection()?
            .query_row(
                "SELECT id, backup_path, source_hash, pair_hash FROM replacements WHERE source_path = ?1 AND undone_at IS NULL ORDER BY replaced_at DESC LIMIT 1",
                [source.display().to_string()],
                |row| {
                    Ok(Replacement {
                        id: row.get(0)?,
                        backup: PathBuf::from(row.get::<_, String>(1)?),
                        source_hash: row.get(2)?,
                        pair_hash: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub fn undo_replace(&self, id: i64) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE replacements SET undone_at = ?2 WHERE id = ?1",
                params![id, now()],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path).map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        Ok(connection)
    }
}

const TASK_FIELDS: &str = "id, input_path, model, state, plan_text, input_hash, error_code, error_detail, pair_path, pair_hash, mono_path, duration_ms";

fn read_task(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        input: row.get(1)?,
        model: row.get(2)?,
        state: row.get(3)?,
        plan_text: row.get(4)?,
        input_hash: row.get(5)?,
        error_code: row.get(6)?,
        error_detail: row.get(7)?,
        pair: row.get(8)?,
        pair_hash: row.get(9)?,
        mono: row.get(10)?,
        duration_ms: row.get(11)?,
    })
}

fn next_id(connection: &rusqlite::Transaction<'_>) -> Result<String> {
    let base = Local::now().format("%Y%m%d-%H%M%S").to_string();
    for number in 1..1_000 {
        let id = if number == 1 {
            base.clone()
        } else {
            format!("{base}-{number:02}")
        };
        let exists: Option<String> = connection
            .query_row("SELECT id FROM tasks WHERE id = ?1", [&id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)?;
        if exists.is_none() {
            return Ok(id);
        }
    }
    Err(BfxError::storage(
        "Cannot allocate a translation identifier",
    ))
}

fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn db_error(error: rusqlite::Error) -> BfxError {
    BfxError::storage(format!("Database operation failed ({error})"))
}

fn ensure_column(connection: &Connection, name: &str) -> Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(tasks)")
        .map_err(db_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_error)?;
    for column in columns {
        if column.map_err(db_error)? == name {
            return Ok(());
        }
    }
    connection
        .execute(
            "ALTER TABLE tasks ADD COLUMN stop_requested INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(db_error)?;
    Ok(())
}

fn remove_plan_keys(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("SELECT id, plan_text FROM tasks WHERE instr(plan_text, 'engine_key') > 0")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut updates = Vec::new();
    for row in rows {
        let (id, text) = row.map_err(db_error)?;
        let mut plan: toml::Table = toml::from_str(&text).map_err(|error| {
            BfxError::storage(format!(
                "Cannot remove a saved API key from task \"{id}\" ({error})"
            ))
        })?;
        if plan.remove("engine_key").is_some() {
            let text = toml::to_string(&plan).map_err(|error| {
                BfxError::storage(format!("Cannot update task \"{id}\" ({error})"))
            })?;
            updates.push((id, text));
        }
    }
    drop(statement);
    for (id, text) in updates {
        connection
            .execute(
                "UPDATE tasks SET plan_text = ?2 WHERE id = ?1",
                params![id, text],
            )
            .map_err(db_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> TaskPlan {
        TaskPlan {
            pages: "All".to_owned(),
            language: "EN->ZH".to_owned(),
            format: "Pair".to_owned(),
            destination: "Same".to_owned(),
            watermark: false,
            engine_model: "test-model".to_owned(),
            engine_url: "https://example.test/v1".to_owned(),
        }
    }

    fn task(path: &str, priority: bool) -> NewTask {
        NewTask {
            input: PathBuf::from(path),
            model: "Model".to_owned(),
            preset: "Default".to_owned(),
            plan: plan(),
            priority,
        }
    }

    #[test]
    fn claims_priority_first() {
        let path = std::env::temp_dir().join(format!(
            "bfx-store-{}.sqlite3",
            Local::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Store::open(path.clone()).unwrap();
        let ids = store
            .enqueue_many(&[task("normal.pdf", false), task("priority.pdf", true)])
            .unwrap();
        let normal = ids[0].clone();
        let priority = ids[1].clone();
        let saved = store.get(&normal).unwrap().unwrap();
        let saved_plan: TaskPlan = toml::from_str(&saved.plan_text).unwrap();
        assert_eq!(saved_plan.language, "EN->ZH");
        assert!(!saved.plan_text.contains("engine_key"));
        let claimed = store.claim().unwrap().unwrap();
        assert_eq!(claimed.id, priority);
        assert_eq!(claimed.model, "Model");
        store.stop(&priority).unwrap();
        assert_eq!(store.get(&priority).unwrap().unwrap().state, "RUN");
        assert!(store.stopped(&priority).unwrap());
        store
            .fail(&priority, "BFX-ENG", "failed after stop", 100)
            .unwrap();
        let stopped = store.get(&priority).unwrap().unwrap();
        assert_eq!(stopped.state, "STP");
        assert_eq!(stopped.duration_ms, Some(100));
        store.retry(&priority).unwrap();
        assert_eq!(store.get(&priority).unwrap().unwrap().state, "QUE");
        let active = store
            .list(&["QUE".to_owned(), "RUN".to_owned()], None, None, 20, true)
            .unwrap();
        assert_eq!(active.len(), 2);
        let range = store
            .list(&[], Some(&normal), Some(&priority), 20, true)
            .unwrap();
        assert_eq!(range.len(), 2);
        store.stop(&normal).unwrap();
        assert_eq!(store.get(&normal).unwrap().unwrap().state, "STP");
        assert!(store.stop(&normal).is_err());
        store.retry(&normal).unwrap();
        assert_eq!(store.get(&normal).unwrap().unwrap().state, "QUE");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn removes_key_from_legacy_task_plan() {
        let path = std::env::temp_dir().join(format!(
            "bfx-store-{}.sqlite3",
            Local::now().timestamp_nanos_opt().unwrap()
        ));
        let store = Store::open(path.clone()).unwrap();
        let id = store
            .enqueue_many(&[task("paper.pdf", false)])
            .unwrap()
            .remove(0);
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE tasks SET plan_text = ?2 WHERE id = ?1",
                params![
                    id,
                    "pages = \"All\"\nlanguage = \"EN->ZH\"\nformat = \"Pair\"\ndestination = \"Same\"\nwatermark = false\nengine_model = \"test-model\"\nengine_url = \"https://example.test/v1\"\nengine_key = \"test-key\"\n"
                ],
            )
            .unwrap();
        let reopened = Store::open(path.clone()).unwrap();
        let task = reopened.get(&id).unwrap().unwrap();
        assert!(!task.plan_text.contains("engine_key"));
        let _ = std::fs::remove_file(path);
    }
}
