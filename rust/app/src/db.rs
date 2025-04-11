//! Utilities for creating, updating, and querying a database with a user and document table.
//!
//! The tables opt to not cache connections to the database under the assumption that the number of reads and writes
//! to the database will be relatively small, so the cost of creating a new connection will be incurred infrequently.
//!
//! In rudimentary local tests, caching the database connection across 1000 insertions reduced insertion time by 20x
//! relative to repeatedly opening a new connection on each insert (~3ms vs 70ms). However, this total latency is
//! small on an absolute scale, and it is unlikely this number of insertions will occur in a tight loop.

use rusqlite::{Connection, ToSql};
use std::{
    // Rename `Backtrace` type such that the `thiserror::Error` proc macro does not generate
    // `Error::provide` which is a nightly only feature gated by `error_generic_member_access`
    backtrace::Backtrace as Bt,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

use corust_components::{
    network::{Activity, User, UserId},
    server::DocumentState,
};

use crate::sessions::SessionId;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    SqliteError(#[from] rusqlite::Error),
    #[error(transparent)]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Entry not found")]
    EntryNotFound,
    #[error("{source} at {file}:{line}")]
    WithBacktrace {
        source: Box<DbError>,
        file: &'static str,
        line: u32,
        backtrace: Bt,
    },
}

/// Utility macro to add a backtrace to an error
macro_rules! with_backtrace {
    ($error:expr) => {
        DbError::WithBacktrace {
            source: Box::new($error.into()),
            file: file!(),
            line: line!(),
            backtrace: Bt::capture(),
        }
    };
}

struct BaseTable {
    db_path: PathBuf,
}

impl BaseTable {
    fn new(db_path: PathBuf) -> Self {
        BaseTable { db_path }
    }

    fn create_connection(&self) -> Result<Connection, DbError> {
        // Equivalent to opening for reading and writing, create if nonexistent, diables per connection mutex
        let conn = Connection::open(&self.db_path)?;
        // Always open connection with foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON;", [])?;
        Ok(conn)
    }
}

pub struct DocumentTableKey {
    pub session_id: SessionId,
}

pub struct DocumentTable {
    base_table: BaseTable,
}

impl DocumentTable {
    pub fn new(db_path: PathBuf) -> Self {
        let base_table = BaseTable::new(db_path);
        DocumentTable { base_table }
    }

    pub fn create_connection(&self) -> Result<Connection, DbError> {
        self.base_table.create_connection()
    }
}

impl Table for DocumentTable {
    type Item = DocumentState;
    type Error = DbError;
    type Key = DocumentTableKey;

    /// Create the documents table if it does not already exist
    fn create(&self) -> Result<(), DbError> {
        let conn = self.create_connection()?;
        // Ignore return value of number of rows updated
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS documents (
                    session_id TEXT PRIMARY KEY,
                    state_id INTEGER,           -- not strictly needed, could restart from 0 on reload            
                    document TEXT,
                    text_op TEXT,               -- JSON serialized `TextOperation`, not strictly needed (could be insert entire document)
                    cursor_map TEXT,            -- JSON serialized `CursorMap`
                    creation_time TIMESTAMP DEFAULT CURRENT_TIMESTAMP 
                );
                ",
            (),
        ).map_err(|e| with_backtrace!(e))?;
        Ok(())
    }

    /// Insert a [`DocumentState`] into the table, or update it if it already exists (based on the provided `key`)
    fn insert_or_update(
        &self,
        key: DocumentTableKey,
        document_state: DocumentState,
    ) -> Result<(), DbError> {
        let conn = self.create_connection()?;
        let _ = conn
            .execute(
                "INSERT INTO documents (session_id, state_id, document, text_op, cursor_map) 
                    VALUES (:session_id, :state_id, :document, :text_op, :cursor_map)
                    ON CONFLICT(session_id) DO UPDATE SET
                        state_id = excluded.state_id,
                        document = excluded.document,
                        text_op = excluded.text_op,
                        cursor_map = excluded.cursor_map;
                    ",
                &[
                    (":session_id", &key.session_id as &dyn ToSql),
                    (":state_id", &document_state.state_id() as &dyn ToSql),
                    (":document", &document_state.document() as &dyn ToSql),
                    (
                        ":text_op",
                        &serde_json::to_string(document_state.text_op())? as &dyn ToSql,
                    ),
                    (
                        ":cursor_map",
                        &serde_json::to_string(document_state.cursor_map())? as &dyn ToSql,
                    ),
                ],
            )
            .map_err(|e| with_backtrace!(e))?;
        Ok(())
    }

    /// Get all [`DocumentState`] from the table by the [`SessionId`] key.
    /// This will only return at most one [`DocumentState`] for a given [`SessionId`].
    fn get_all(&self, session_id: &str) -> Result<Vec<DocumentState>, DbError> {
        let conn = self.create_connection()?;
        let mut stmt = conn.prepare(
            "SELECT state_id, document, text_op, cursor_map FROM documents WHERE session_id = :session_id",
        ).map_err(|e| with_backtrace!(e))?;
        let document_state = stmt.query_map(&[(":session_id", session_id)], |row| {
            let state_id = row.get(0)?;
            let document = row.get(1)?;
            let text_op: String = row.get(2)?;
            // unwrap: the text_op is always serialized to a string in the database
            let text_op = serde_json::from_str(&text_op).unwrap();
            let cursor_map: String = row.get(3)?;
            // unwrap: the cursor map is always serialized to a string in the database
            let cursor_map = serde_json::from_str(&cursor_map).unwrap();
            Ok(DocumentState::new(state_id, document, text_op, cursor_map))
        })?;
        let document_state = document_state
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| with_backtrace!(e))?;
        debug_assert!(document_state.len() <= 1);
        Ok(document_state)
    }
}

#[derive(Debug, Clone)]
pub struct UserTableKey {
    pub session_id: SessionId,
    pub user_id: UserId,
}

pub struct UserTable {
    base_table: BaseTable,
}

impl UserTable {
    pub fn new(db_path: PathBuf) -> Self {
        UserTable {
            base_table: BaseTable::new(db_path),
        }
    }

    fn create_connection(&self) -> Result<Connection, DbError> {
        self.base_table.create_connection()
    }
}

impl Table for UserTable {
    type Item = User;
    type Error = DbError;
    type Key = UserTableKey;

    /// Create the user table if it does not already exist
    fn create(&self) -> Result<(), DbError> {
        let conn = self.create_connection()?;
        // Ignore return value of number of rows updated
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS users (               
                    session_id TEXT,                   
                    username TEXT,
                    user_id INTEGER,    
                    color TEXT,    
                    active BOOLEAN,
                    last_activity INTEGER,                  -- in seconds since UNIX_EPOCH, approximation      
                    creation_time TIMESTAMP DEFAULT CURRENT_TIMESTAMP, 
                    PRIMARY KEY (session_id, user_id),      -- composite primary key
                    FOREIGN KEY (session_id) REFERENCES documents (session_id)
                );
                ",
            (),
        )?;
        Ok(())
    }

    /// Insert a `User` into the table, or update it if it already exists (based on the provided `key`)
    fn insert_or_update(&self, key: UserTableKey, user: User) -> Result<(), DbError> {
        debug_assert!(key.user_id == user.user_id());

        let conn = self.create_connection()?;

        let system_now = SystemTime::now();
        let instant_now = Instant::now();
        // Stores an approximation of the last activity time in seconds since UNIX_EPOCH
        // by converting from `Instant` to `SystemTime`
        // https://github.com/serde-rs/serde/issues/1375#issuecomment-419688068
        let instant_approx = system_now - (instant_now - user.activity.last_activity);
        let last_activity_secs = instant_approx.duration_since(UNIX_EPOCH).unwrap().as_secs();

        conn.execute(
            "INSERT INTO users (session_id, user_id, username, color, active, last_activity) 
            VALUES (:session_id, :user_id, :username, :color, :active, :last_activity)
            ON CONFLICT(session_id, user_id) DO UPDATE SET
                username = excluded.username,
                color = excluded.color,
                active = excluded.active,
                last_activity = excluded.last_activity;",
            &[
                (":session_id", &key.session_id as &dyn ToSql),
                (":user_id", &user.user_id().to_string() as &dyn ToSql),
                (":username", &user.username() as &dyn ToSql),
                (":color", &user.color() as &dyn ToSql),
                (":active", &user.activity.active as &dyn ToSql),
                (
                    ":last_activity",
                    &last_activity_secs.to_string() as &dyn ToSql,
                ),
            ],
        )?;
        Ok(())
    }

    /// Gets all `User`s in a session
    /// Returns an empty vector if no users are found
    fn get_all(&self, session_id: &str) -> Result<Vec<User>, DbError> {
        let conn = self.create_connection()?;
        let mut stmt = conn.prepare(
            "SELECT username, user_id, color, active, last_activity FROM users WHERE session_id = :session_id",
        ).map_err(|e| with_backtrace!(e))?;

        let users = stmt
            .query_map(&[(":session_id", session_id)], |row| {
                let username = row.get(0)?;
                let user_id = row.get(1)?;
                let color = row.get(2)?;
                // Uses this approximation of Instant:
                // https://github.com/serde-rs/serde/issues/1375#issuecomment-419688068
                let last_activity_sec_to_epoch: u64 = row.get(4)?;
                let last_activity_system_time =
                    UNIX_EPOCH + Duration::from_secs(last_activity_sec_to_epoch);
                let last_activity = Instant::now()
                    - (SystemTime::now().duration_since(last_activity_system_time))
                        .unwrap_or(Duration::from_secs(0));
                let last_activity = dbg!(last_activity);
                let activity = Activity {
                    active: row.get(3)?,
                    last_activity,
                };
                Ok(User::new(user_id, username, color, activity))
            })
            .map_err(|e| with_backtrace!(e))?;

        let users = users
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| with_backtrace!(e))?;
        debug_assert!(users.len() <= 1);
        Ok(users)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Compilation {
    /// The user id of the user who compiled the code
    pub user_id: UserId,
}

#[derive(Debug, Clone)]
pub struct CompilationTableKey {
    pub session_id: SessionId,
    pub user_id: UserId,
}

/// Tracks the compilations per user
pub struct CompilationTable {
    base_table: BaseTable,
}

impl CompilationTable {
    pub fn new(db_path: PathBuf) -> Self {
        CompilationTable {
            base_table: BaseTable::new(db_path),
        }
    }

    pub fn create_connection(&self) -> Result<Connection, DbError> {
        self.base_table.create_connection()
    }
}

impl Table for CompilationTable {
    type Item = Compilation;
    type Error = DbError;
    type Key = CompilationTableKey;

    fn create(&self) -> Result<(), DbError> {
        let conn = self.create_connection()?;
        // Ignore return value of number of rows updated
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS compilations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT,
                    user_id INTEGER,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY (session_id) REFERENCES documents (session_id)
                );
                ",
            (),
        )?;
        Ok(())
    }

    fn insert_or_update(
        &self,
        key: CompilationTableKey,
        compilation: Compilation,
    ) -> Result<(), DbError> {
        debug_assert!(key.user_id == compilation.user_id);

        let conn = self.create_connection()?;

        conn.execute(
            "INSERT INTO compilations (session_id, user_id) VALUES (:session_id, :user_id)",
            &[
                (":session_id", &key.session_id as &dyn ToSql),
                (":user_id", &key.user_id as &dyn ToSql),
            ],
        )?;
        Ok(())
    }

    /// Get all the compilations in a session (currently not used, so it is a no-op)
    fn get_all(&self, session_id: &str) -> Result<Vec<Compilation>, DbError> {
        let conn = self.create_connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, created_at FROM compilations WHERE session_id = :session_id",
            )
            .map_err(|e| with_backtrace!(e))?;

        let compilations = stmt
            .query_map(&[(":session_id", session_id)], |row| {
                let user_id = row.get(1)?;

                Ok(Compilation { user_id })
            })
            .map_err(|e| with_backtrace!(e))?;

        compilations
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| with_backtrace!(e))
    }
}

/// Represents a queryable table in a database.
pub trait Table {
    type Item;
    type Error;
    /// Primary key(s) for the table, supports composite keys
    type Key;

    /// Creates the table
    fn create(&self) -> Result<(), Self::Error>;
    /// Insert new entry into the `Table` with a given `session_id`, or update it if it already exists
    /// (based on the provided `key`).
    fn insert_or_update(&self, key: Self::Key, item: Self::Item) -> Result<(), Self::Error>;
    /// Gets a row from the table and converts it to `Item` by `key_map`.
    /// Returns `None` if a row with this `session_id` does not exist.
    fn get_all(&self, session_id: &str) -> Result<Vec<Self::Item>, Self::Error>;
}

#[cfg(test)]
mod test {
    use super::*;
    use tempfile::{TempDir, tempdir};

    // Utility to create a document table and insert a document state
    // Required for the user and compilation tables to have a foreign key reference
    fn create_document_table(tmp_dir: &TempDir, session_id: &str) {
        let db_path = tmp_dir.path().join("test.db");
        let document_table = DocumentTable::new(db_path);
        document_table.create().unwrap();

        let document_state = DocumentState::new(
            10,
            "document".to_string(),
            Default::default(),
            Default::default(),
        );
        let document_table_key = DocumentTableKey {
            session_id: session_id.to_string(),
        };
        document_table
            .insert_or_update(document_table_key, document_state.clone())
            .unwrap();
    }

    mod document_table {
        use super::*;

        #[test]
        fn test_document_table_insert_get() {
            // Tests create, insert, get for DocumentTable for a single document
            let tmp_dir = tempdir().unwrap();
            let db_path = tmp_dir.path().join("test.db");
            let document_table = DocumentTable::new(db_path);
            document_table.create().unwrap();

            let session_ids = ["123456", "a", "b"];
            for session_id in session_ids {
                let document_state = DocumentState::new(
                    10,
                    "document".to_string(),
                    Default::default(),
                    Default::default(),
                );
                let document_table_key = DocumentTableKey {
                    session_id: session_id.to_string(),
                };
                document_table
                    .insert_or_update(document_table_key, document_state.clone())
                    .unwrap();

                let retrieved_document_state = document_table.get_all(session_id).unwrap();
                assert_eq!(retrieved_document_state, vec![document_state]);
            }
        }

        #[test]
        fn test_document_table_update() {
            // Tests create, insert, update, get for DocumentTable for a single document
            let tmp_dir = tempdir().unwrap();
            let db_path = tmp_dir.path().join("test.db");
            let document_table = DocumentTable::new(db_path);
            document_table.create().unwrap();

            let session_id = "abc123";
            let document_state =
                DocumentState::new(10, "".to_string(), Default::default(), Default::default());
            let document_table_key = DocumentTableKey {
                session_id: session_id.to_string(),
            };
            document_table
                .insert_or_update(document_table_key, document_state.clone())
                .unwrap();

            for document in ["fn", "fn main", "fn main {}"] {
                let document_state = DocumentState::new(
                    10,
                    document.to_string(),
                    Default::default(),
                    Default::default(),
                );
                let document_table_key = DocumentTableKey {
                    session_id: session_id.to_string(),
                };
                document_table
                    .insert_or_update(document_table_key, document_state.clone())
                    .unwrap();

                let retrieved_document_state = document_table.get_all(session_id).unwrap();
                assert_eq!(retrieved_document_state, vec![document_state]);
                // A single document should be updated
                assert!(retrieved_document_state.len() == 1);
            }
        }
    }

    mod user_table {
        use super::*;

        #[test]
        fn test_user_table_insert_get() {
            // Tests create, insert, get_all for UserTable
            let tmp_dir = tempdir().unwrap();
            let db_path = tmp_dir.path().join("test.db");
            let user_table = UserTable::new(db_path);
            user_table.create().unwrap();

            let session_ids = ["abc", "123xyz", "hello"];
            for session_id in session_ids {
                create_document_table(&tmp_dir, session_id);

                let user = User::new(
                    100,
                    "corust".to_string(),
                    "red".to_string(),
                    Activity {
                        active: true,
                        last_activity: Instant::now(),
                    },
                );
                let user_key = UserTableKey {
                    session_id: session_id.to_string(),
                    user_id: user.user_id(),
                };
                user_table.insert_or_update(user_key, user.clone()).unwrap();

                let retrieved_users = user_table.get_all(session_id).unwrap();
                assert_eq!(retrieved_users, vec![user]);
            }
        }

        #[test]
        fn test_user_table_update() {
            // Tests create, insert, update, get_all for UserTable
            let tmp_dir = tempdir().unwrap();
            let db_path = tmp_dir.path().join("test.db");
            let user_table = UserTable::new(db_path);
            user_table.create().unwrap();

            let session_id = "abc";
            create_document_table(&tmp_dir, session_id);

            for color in ["red", "green", "blue"] {
                let user = User::new(
                    100,
                    "corust".to_string(),
                    color.to_string(),
                    Activity {
                        active: true,
                        last_activity: Instant::now(),
                    },
                );
                let user_key = UserTableKey {
                    session_id: session_id.to_string(),
                    user_id: user.user_id(),
                };
                user_table.insert_or_update(user_key, user.clone()).unwrap();

                let retrieved_users = user_table.get_all(session_id).unwrap();
                assert_eq!(retrieved_users, vec![user]);
                // A single user should be updated
                assert!(retrieved_users.len() == 1);
            }
        }
    }

    mod compilation_table {
        use super::*;

        #[test]
        fn test_compilation_table_insert_get() {
            // Tests create, insert, get_all for CompilationTable
            let tmp_dir = tempdir().unwrap();
            let session_id = "abc";
            create_document_table(&tmp_dir, session_id);

            let db_path = tmp_dir.path().join("test.db");
            let compilation_table = CompilationTable::new(db_path);
            compilation_table.create().unwrap();

            let compilation = Compilation { user_id: 100 };
            let compilation_key = CompilationTableKey {
                session_id: session_id.to_string(),
                user_id: 100,
            };
            compilation_table
                .insert_or_update(compilation_key.clone(), compilation.clone())
                .unwrap();

            let retrieved_compilations = compilation_table.get_all(session_id).unwrap();
            assert_eq!(retrieved_compilations, vec![compilation]);
        }
    }
}
