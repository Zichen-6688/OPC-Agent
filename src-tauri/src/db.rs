//! SQLite 数据层:员工、会话、消息、派单日志、设置
//! 所有表使用 WAL 模式,支持多连接并发读写(后台 LLM 任务独立开连接)。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const DB_FILE: &str = "opc-agent.db";

// ---------------- 模型 ----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Employee {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub emoji: String,
    pub color: String,
    pub system_prompt: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
    pub capabilities: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub role: String, // boss | employee | orchestrator
    pub employee_id: Option<i64>,
    pub employee_name: Option<String>,
    pub employee_emoji: Option<String>,
    pub employee_color: Option<String>,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchLog {
    pub id: i64,
    pub conversation_id: i64,
    pub boss_message: String,
    pub assigned_ids: Vec<i64>,
    pub assigned_names: Vec<String>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub orc_base_url: String,
    pub orc_api_key: String,
    pub orc_model: String,
    pub orc_temperature: f64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            orc_base_url: "https://api.deepseek.com".into(),
            orc_api_key: String::new(),
            orc_model: "deepseek-v4-flash".into(),
            orc_temperature: 0.3,
        }
    }
}

// ---------------- 连接与迁移 ----------------

/// 打开(或创建)数据库并迁移表结构。
pub fn open(path: PathBuf) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(&path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS employees (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL,
            role          TEXT NOT NULL,
            emoji         TEXT NOT NULL DEFAULT '🤖',
            color         TEXT NOT NULL DEFAULT '#2563eb',
            system_prompt TEXT NOT NULL DEFAULT '',
            base_url      TEXT NOT NULL,
            api_key       TEXT NOT NULL DEFAULT '',
            model         TEXT NOT NULL,
            temperature   REAL NOT NULL DEFAULT 0.7,
            capabilities  TEXT NOT NULL DEFAULT '[]',
            enabled       INTEGER NOT NULL DEFAULT 1,
            created_at    TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS conversations (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            title      TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS messages (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            role            TEXT NOT NULL,
            employee_id     INTEGER,
            content         TEXT NOT NULL,
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS dispatch_log (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            boss_message    TEXT NOT NULL,
            assigned_ids    TEXT NOT NULL,
            reason          TEXT NOT NULL DEFAULT '',
            created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )
}

// ---------------- 员工 ----------------

pub fn list_employees(conn: &Connection) -> rusqlite::Result<Vec<Employee>> {
    let mut stmt = conn.prepare(
        "SELECT id,name,role,emoji,color,system_prompt,base_url,api_key,model,temperature,capabilities,enabled
         FROM employees ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        let caps: String = r.get(10)?;
        Ok(Employee {
            id: r.get(0)?,
            name: r.get(1)?,
            role: r.get(2)?,
            emoji: r.get(3)?,
            color: r.get(4)?,
            system_prompt: r.get(5)?,
            base_url: r.get(6)?,
            api_key: r.get(7)?,
            model: r.get(8)?,
            temperature: r.get(9)?,
            capabilities: serde_json::from_str(&caps).unwrap_or_default(),
            enabled: r.get::<_, i64>(11)? != 0,
        })
    })?;
    rows.collect()
}

pub fn create_employee(conn: &Connection, e: &Employee) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO employees (name,role,emoji,color,system_prompt,base_url,api_key,model,temperature,capabilities,enabled)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            e.name,
            e.role,
            e.emoji,
            e.color,
            e.system_prompt,
            e.base_url,
            e.api_key,
            e.model,
            e.temperature,
            serde_json::to_string(&e.capabilities).unwrap_or_else(|_| "[]".into()),
            e.enabled as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_employee(conn: &Connection, e: &Employee) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE employees SET name=?1,role=?2,emoji=?3,color=?4,system_prompt=?5,base_url=?6,api_key=?7,model=?8,temperature=?9,capabilities=?10,enabled=?11 WHERE id=?12",
        params![
            e.name,
            e.role,
            e.emoji,
            e.color,
            e.system_prompt,
            e.base_url,
            e.api_key,
            e.model,
            e.temperature,
            serde_json::to_string(&e.capabilities).unwrap_or_else(|_| "[]".into()),
            e.enabled as i64,
            e.id,
        ],
    )?;
    Ok(())
}

pub fn delete_employee(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM employees WHERE id=?1", params![id])?;
    Ok(())
}

// ---------------- 会话与消息 ----------------

pub fn create_conversation(conn: &Connection, title: &str) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO conversations (title) VALUES (?1)", params![title])?;
    Ok(conn.last_insert_rowid())
}

pub fn list_conversations(conn: &Connection) -> rusqlite::Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.created_at,
                (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id) AS msg_cnt
         FROM conversations c ORDER BY c.id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            Conversation {
                id: r.get(0)?,
                title: r.get(1)?,
                created_at: r.get(2)?,
            },
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (mut c, cnt) = row?;
        if cnt == 0 {
            // 空会话不展示
            continue;
        }
        if c.title.len() > 30 {
            c.title = c.title.chars().take(30).collect();
        }
        out.push(c);
    }
    Ok(out)
}

pub fn get_messages(conn: &Connection, conversation_id: i64) -> rusqlite::Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.conversation_id, m.role, m.employee_id, m.content, m.created_at,
                e.name, e.emoji, e.color
         FROM messages m LEFT JOIN employees e ON e.id = m.employee_id
         WHERE m.conversation_id = ?1 ORDER BY m.id",
    )?;
    let rows = stmt.query_map(params![conversation_id], |r| {
        Ok(ChatMessage {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            role: r.get(2)?,
            employee_id: r.get(3)?,
            employee_name: r.get(6)?,
            employee_emoji: r.get(7)?,
            employee_color: r.get(8)?,
            content: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn delete_conversation(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM conversations WHERE id=?1", params![id])?;
    Ok(())
}

pub fn insert_message(
    conn: &Connection,
    conversation_id: i64,
    role: &str,
    employee_id: Option<i64>,
    content: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO messages (conversation_id, role, employee_id, content) VALUES (?1,?2,?3,?4)",
        params![conversation_id, role, employee_id, content],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 组装给员工 LLM 的最近上下文(最多 20 条,跳过 orchestrator 内部消息)。
pub fn build_history(conn: &Connection, conversation_id: i64, employee_id: i64) -> rusqlite::Result<Vec<Value>> {
    let msgs = get_messages(conn, conversation_id)?;
    let mut history: Vec<Value> = Vec::new();
    for m in msgs.iter().rev().take(20).collect::<Vec<_>>().iter().rev() {
        match m.role.as_str() {
            "boss" => history.push(serde_json::json!({"role": "user", "content": m.content})),
            "employee" => {
                if m.employee_id == Some(employee_id) {
                    let name = m.employee_name.clone().unwrap_or_else(|| "员工".into());
                    history.push(serde_json::json!({
                        "role": "assistant",
                        "content": format!("【{}】{}", name, m.content),
                    }));
                }
            }
            _ => {}
        }
    }
    Ok(history)
}

// ---------------- 派单日志 ----------------

pub fn insert_dispatch(
    conn: &Connection,
    conversation_id: i64,
    boss_message: &str,
    assigned_ids: &[i64],
    assigned_names: &[String],
    reason: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO dispatch_log (conversation_id, boss_message, assigned_ids, reason) VALUES (?1,?2,?3,?4)",
        params![
            conversation_id,
            boss_message,
            serde_json::to_string(assigned_ids).unwrap_or_else(|_| "[]".into()),
            reason,
        ],
    )?;
    // assigned_names 仅用于展示,存一份便于商业版导出
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params![
            format!("dispatch_names_{}", conn.last_insert_rowid()),
            serde_json::to_string(assigned_names).unwrap_or_else(|_| "[]".into()),
        ],
    )?;
    Ok(())
}

pub fn list_dispatch_logs(conn: &Connection, conversation_id: i64) -> rusqlite::Result<Vec<DispatchLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, boss_message, assigned_ids, reason, created_at
         FROM dispatch_log WHERE conversation_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![conversation_id], |r| {
        let ids: String = r.get(3)?;
        Ok(DispatchLog {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            boss_message: r.get(2)?,
            assigned_ids: serde_json::from_str(&ids).unwrap_or_default(),
            assigned_names: Vec::new(),
            reason: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    rows.collect()
}

// ---------------- 设置 ----------------

pub fn get_settings(conn: &Connection) -> AppSettings {
    let mut s = AppSettings::default();
    let mut stmt = match conn.prepare("SELECT key, value FROM settings") {
        Ok(stmt) => stmt,
        Err(_) => return s,
    };
    let rows = match stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
        Ok(rows) => rows,
        Err(_) => return s,
    };
    for row in rows.flatten() {
        match row.0.as_str() {
            "orc_base_url" => s.orc_base_url = row.1,
            "orc_api_key" => s.orc_api_key = row.1,
            "orc_model" => s.orc_model = row.1,
            "orc_temperature" => {
                if let Ok(v) = row.1.parse() {
                    s.orc_temperature = v;
                }
            }
            _ => {}
        }
    }
    s
}

pub fn save_settings(conn: &Connection, s: &AppSettings) -> rusqlite::Result<()> {
    let pairs = [
        ("orc_base_url", &s.orc_base_url),
        ("orc_api_key", &s.orc_api_key),
        ("orc_model", &s.orc_model),
        ("orc_temperature", &s.orc_temperature.to_string()),
    ];
    for (k, v) in pairs {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![k, v],
        )?;
    }
    Ok(())
}
