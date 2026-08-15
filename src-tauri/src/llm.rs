//! LLM 调用层:OpenAI 兼容协议,支持流式输出(SSE)。
//! 兼容 DeepSeek / OpenAI / Moonshot / 本地 Ollama 等所有 chat/completions 端点。

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use futures_util::StreamExt;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f64,
}

fn build_messages(system: &str, history: &[Value]) -> Vec<Value> {
    let mut msgs = vec![json!({"role": "system", "content": system})];
    msgs.extend(history.iter().cloned());
    msgs
}

fn truncate(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > n {
        let mut t: String = chars[..n].iter().collect();
        t.push_str("…");
        t
    } else {
        s.to_string()
    }
}

/// 非流式补全(用于调度决策,要求模型输出 JSON)。
pub async fn chat_once(
    cfg: &LlmConfig,
    system: &str,
    history: &[Value],
) -> Result<String, String> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;
    let body = json!({
        "model": cfg.model,
        "temperature": cfg.temperature,
        "stream": false,
        "messages": build_messages(system, history),
    });
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", truncate(&text, 400)));
    }
    let v: Value =
        serde_json::from_str(&text).map_err(|e| format!("响应解析失败: {e} → {}", truncate(&text, 200)))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("响应中没有内容: {}", truncate(&text, 200)))
}

/// 流式补全。每个增量通过 `llm-token` 事件推送给前端。
/// 返回完整文本(同时用于落库)。
pub async fn chat_stream(
    app: &AppHandle,
    conversation_id: i64,
    employee_id: i64,
    cfg: &LlmConfig,
    system: &str,
    history: &[Value],
) -> Result<String, String> {
    let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;
    let body = json!({
        "model": cfg.model,
        "temperature": cfg.temperature,
        "stream": true,
        "messages": build_messages(system, history),
    });
    let resp = client
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {}", truncate(&text, 400)));
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut full = String::new();
    'outer: while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("流读取失败: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    break 'outer;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                        if !delta.is_empty() {
                            full.push_str(delta);
                            let _ = app.emit(
                                "llm-token",
                                json!({
                                    "conversationId": conversation_id,
                                    "employeeId": employee_id,
                                    "delta": delta,
                                }),
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(full)
}
