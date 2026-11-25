//! A conversa do T3 Code de onde o `talk-rs speak` foi chamado.
//!
//! O T3 põe `T3CODE_THREAD_ID` no ambiente de cada sessão. O título da
//! conversa (o que aparece na janela) está no banco do T3, lido só para leitura.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};

fn database() -> Option<PathBuf> {
    let home = match std::env::var_os("T3CODE_HOME") {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?).join(".t3"),
    };
    Some(home.join("userdata/statev2.sqlite"))
}

/// `None` fora do T3, ou se o banco não responde: aí a fala toca na hora.
pub fn current_thread_title() -> Option<String> {
    let id = std::env::var("T3CODE_THREAD_ID").ok().filter(|i| !i.is_empty())?;
    let db = Connection::open_with_flags(database()?, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    db.query_row(
        "SELECT title FROM orchestration_v2_projection_threads WHERE thread_id = ?1",
        [&id],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .filter(|t| !t.trim().is_empty())
}
