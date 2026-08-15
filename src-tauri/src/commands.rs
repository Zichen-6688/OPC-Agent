//! Tauri 命令层:前端 IPC 入口 + 派单流水线。

use crate::db::{self, AppSettings, ChatMessage, Conversation, Employee};
use crate::llm::{chat_stream, LlmConfig};
use crate::orchestrator;
use crate::voice;
use serde_json::json;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

pub struct AppState {
    pub db_path: PathBuf,
}

fn open_db(state: &State<'_, AppState>) -> Result<rusqlite::Connection, String> {
    db::open(state.db_path.clone()).map_err(|e| format!("数据库打开失败: {e}"))
}

fn cfg_from(s: &AppSettings) -> LlmConfig {
    LlmConfig {
        base_url: s.orc_base_url.clone(),
        api_key: s.orc_api_key.clone(),
        model: s.orc_model.clone(),
        temperature: s.orc_temperature,
    }
}

fn title_from(text: &str) -> String {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).take(12).collect();
    if t.is_empty() { "新对话".into() } else { t }
}

// ---------------- 员工 ----------------

#[tauri::command]
pub fn list_employees(state: State<'_, AppState>) -> Result<Vec<Employee>, String> {
    let conn = open_db(&state)?;
    db::list_employees(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_employee(state: State<'_, AppState>, employee: Employee) -> Result<Employee, String> {
    let conn = open_db(&state)?;
    let id = db::create_employee(&conn, &employee).map_err(|e| e.to_string())?;
    let mut e = employee;
    e.id = id;
    Ok(e)
}

#[tauri::command]
pub fn update_employee(state: State<'_, AppState>, employee: Employee) -> Result<(), String> {
    let conn = open_db(&state)?;
    db::update_employee(&conn, &employee).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_employee(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = open_db(&state)?;
    db::delete_employee(&conn, id).map_err(|e| e.to_string())
}

// ---------------- 会话 / 消息 ----------------

#[tauri::command]
pub fn list_conversations(state: State<'_, AppState>) -> Result<Vec<Conversation>, String> {
    let conn = open_db(&state)?;
    db::list_conversations(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_messages(state: State<'_, AppState>, conversation_id: i64) -> Result<Vec<ChatMessage>, String> {
    let conn = open_db(&state)?;
    db::get_messages(&conn, conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_conversation(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = open_db(&state)?;
    db::delete_conversation(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_dispatch_logs(state: State<'_, AppState>, conversation_id: i64) -> Result<Vec<db::DispatchLog>, String> {
    let conn = open_db(&state)?;
    db::list_dispatch_logs(&conn, conversation_id).map_err(|e| e.to_string())
}

// ---------------- 设置 ----------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let conn = open_db(&state)?;
    Ok(db::get_settings(&conn))
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
    let conn = open_db(&state)?;
    db::save_settings(&conn, &settings).map_err(|e| e.to_string())
}

// ---------------- 语音 ----------------

#[tauri::command]
pub async fn speak(text: String, voice: Option<String>) -> Result<(), String> {
    // 语音播报是长时同步子进程(say/PowerShell/spd-say),不能占用主线程,
    // 否则朗读期间 UI 冻结(鼠标转圈)。spawn_blocking 放到独立线程池执行。
    tauri::async_runtime::spawn_blocking(move || voice::speak(&text, voice.as_deref()))
        .await
        .map_err(|e| format!("语音线程异常: {e}"))?
}

// ---------------- 版本 / 版本信息 ----------------

#[tauri::command]
pub fn get_edition() -> serde_json::Value {
    json!({
        "edition": "community",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
    })
}

// ---------------- 派单流水线 ----------------

/// 老板发送消息。立即落库并返回会话 id,后台异步执行:
/// 调度决策 → 分派 → 员工并行回复(流式)→ 完成事件。
#[tauri::command]
pub async fn send_boss_message(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: Option<i64>,
    text: String,
) -> Result<i64, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("消息不能为空".into());
    }

    let (conv_id, settings, db_path) = {
        let conn = open_db(&state)?;
        let settings = db::get_settings(&conn);
        let conv_id = match conversation_id {
            Some(id) => id,
            None => db::create_conversation(&conn, &title_from(&text)).map_err(|e| e.to_string())?,
        };
        db::insert_message(&conn, conv_id, "boss", None, &text).map_err(|e| e.to_string())?;
        (conv_id, settings, state.db_path.clone())
    };

    tauri::async_runtime::spawn(async move {
        run_dispatch(app, db_path, conv_id, text, settings).await;
    });

    Ok(conv_id)
}

async fn run_dispatch(
    app: AppHandle,
    db_path: PathBuf,
    conv_id: i64,
    boss_text: String,
    settings: AppSettings,
) {
    let conn = match db::open(db_path.clone()) {
        Ok(c) => c,
        Err(e) => {
            let _ = app.emit("task-error", json!({"conversationId": conv_id, "error": format!("数据库错误: {e}")}));
            return;
        }
    };

    let employees: Vec<Employee> = match db::list_employees(&conn) {
        Ok(list) => list.into_iter().filter(|e| e.enabled).collect(),
        Err(e) => {
            let _ = app.emit("task-error", json!({"conversationId": conv_id, "error": format!("员工读取失败: {e}")}));
            return;
        }
    };

    if employees.is_empty() {
        let _ = app.emit(
            "task-error",
            json!({"conversationId": conv_id, "error": "还没有启用的 AI 员工,请先在右侧面板添加。"}),
        );
        return;
    }

    let _ = app.emit("dispatch-thinking", json!({"conversationId": conv_id}));

    let cfg = cfg_from(&settings);
    let decision = orchestrator::decide(&app, &cfg, &boss_text, &employees).await;

    let assigned: Vec<Employee> = decision
        .assigned
        .iter()
        .filter_map(|id| employees.iter().find(|e| e.id == *id).cloned())
        .collect();

    if assigned.is_empty() {
        let _ = app.emit(
            "task-error",
            json!({"conversationId": conv_id, "error": "调度未能匹配到合适的员工,请检查员工配置。"}),
        );
        return;
    }

    let names: Vec<String> = assigned.iter().map(|e| e.name.clone()).collect();
    let ids: Vec<i64> = assigned.iter().map(|e| e.id).collect();
    let _ = db::insert_dispatch(&conn, conv_id, &boss_text, &ids, &names, &decision.reason);
    drop(conn);

    let _ = app.emit(
        "dispatch-decision",
        json!({
            "conversationId": conv_id,
            "assigned": assigned.iter().map(|e| json!({"id": e.id, "name": e.name, "emoji": e.emoji, "color": e.color, "role": e.role})).collect::<Vec<_>>(),
            "reason": decision.reason,
        }),
    );

    // 员工并行执行
    let mut handles = Vec::new();
    for emp in assigned {
        let app2 = app.clone();
        let db2 = db_path.clone();
        let cfg2 = cfg.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            let _ = app2.emit(
                "employee-start",
                json!({"conversationId": conv_id, "employeeId": emp.id, "name": emp.name, "emoji": emp.emoji, "color": emp.color}),
            );
            let conn = match db::open(db2.clone()) {
                Ok(c) => c,
                Err(e) => {
                    let _ = app2.emit("employee-done", json!({"conversationId": conv_id, "employeeId": emp.id, "error": format!("数据库错误: {e}")}));
                    return;
                }
            };
            let history = match db::build_history(&conn, conv_id, emp.id) {
                Ok(h) => h,
                Err(e) => {
                    let _ = app2.emit("employee-done", json!({"conversationId": conv_id, "employeeId": emp.id, "error": format!("历史读取失败: {e}")}));
                    return;
                }
            };
            match chat_stream(&app2, conv_id, emp.id, &cfg2, &emp.system_prompt, &history).await {
                Ok(content) => {
                    let content = content.trim().to_string();
                    let mid = db::insert_message(&conn, conv_id, "employee", Some(emp.id), &content)
                        .unwrap_or(-1);
                    let _ = app2.emit(
                        "employee-done",
                        json!({"conversationId": conv_id, "employeeId": emp.id, "messageId": mid, "content": content, "error": null}),
                    );
                }
                Err(e) => {
                    let _ = app2.emit(
                        "employee-done",
                        json!({"conversationId": conv_id, "employeeId": emp.id, "error": e}),
                    );
                }
            }
        }));
    }

    let _ = futures_util::future::join_all(handles).await;
    let _ = app.emit("task-done", json!({"conversationId": conv_id}));
}
