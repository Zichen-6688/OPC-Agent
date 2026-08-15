//! 智能调度中枢:根据 BOSS 指令与员工名册,决定把任务派给哪些 AI 员工。
//!
//! 策略:
//! 1. 主路径 —— 调度 LLM 输出 JSON(容忍解析,DeepSeek 等提供商不支持 json_schema,
//!    因此不依赖 response_format,靠强提示词 + 宽松解析)。
//! 2. 兜底 —— 解析失败或超时,派给所有启用的员工,保证任务不丢失。

use crate::db::Employee;
use crate::llm::{chat_once, LlmConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchDecision {
    pub assigned: Vec<i64>,
    pub reason: String,
}

fn build_roster(employees: &[Employee]) -> String {
    employees
        .iter()
        .map(|e| {
            format!(
                "- id={}, 姓名={}, 岗位={}, 擅长={}",
                e.id,
                e.name,
                e.role,
                e.capabilities.join("、")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 从模型输出中宽松提取 {"assigned": [...], "reason": "..."}
fn parse_decision(text: &str, employees: &[Employee]) -> Option<DispatchDecision> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    let v: Value = serde_json::from_str(&text[start..=end]).ok()?;
    let reason = v["reason"].as_str().unwrap_or("").to_string();
    let mut assigned: Vec<i64> = Vec::new();
    if let Some(arr) = v["assigned"].as_array() {
        for item in arr {
            if let Some(id) = item.as_i64() {
                if employees.iter().any(|e| e.id == id) && !assigned.contains(&id) {
                    assigned.push(id);
                }
            } else if let Some(name) = item.as_str() {
                if let Some(emp) = employees.iter().find(|e| e.name == name) {
                    if !assigned.contains(&emp.id) {
                        assigned.push(emp.id);
                    }
                }
            }
        }
    }
    if assigned.is_empty() {
        return None;
    }
    Some(DispatchDecision { assigned, reason })
}

fn fallback_all(employees: &[Employee]) -> DispatchDecision {
    DispatchDecision {
        assigned: employees.iter().map(|e| e.id).collect(),
        reason: "自动调度未能解析,已派发给全部启用的员工".into(),
    }
}

/// 调度决策入口。
pub async fn decide(
    app: &AppHandle,
    cfg: &LlmConfig,
    boss_message: &str,
    employees: &[Employee],
) -> DispatchDecision {
    let enabled: Vec<Employee> = employees.iter().filter(|e| e.enabled).cloned().collect();
    if enabled.is_empty() {
        return DispatchDecision {
            assigned: vec![],
            reason: "没有启用的 AI 员工".into(),
        };
    }
    if enabled.len() == 1 {
        // 只有一个员工,无需调用调度 LLM
        return DispatchDecision {
            assigned: vec![enabled[0].id],
            reason: "当前仅一名员工,直接派发".into(),
        };
    }

    let system = "你是 OPC Agent 的智能调度中枢。BOSS 会给出一条指令,你根据 AI 员工的岗位与擅长领域,把任务分派给最合适的 1~3 名员工。\
        \n你必须只输出一个 JSON 对象,不要输出任何其他文字,格式如下:\
        \n{\"assigned\": [员工id列表], \"reason\": \"为什么这样分派(一句话中文说明)\"}\
        \n如果指令过于简单(如问候、闲聊),只选 1 名最通用的员工;如果指令复杂,可多选。";
    let user = json!({
        "role": "user",
        "content": format!("【AI 员工名册】\n{}\n\n【BOSS 指令】\n{}", build_roster(&enabled), boss_message)
    });

    match chat_once(cfg, system, &[user]).await {
        Ok(text) => match parse_decision(&text, &enabled) {
            Some(d) => d,
            None => {
                let _ = app.emit(
                    "dispatch-warn",
                    json!({"message": "调度解析失败,已兜底派发全部员工"}),
                );
                fallback_all(&enabled)
            }
        },
        Err(e) => {
            let _ = app.emit("dispatch-warn", json!({"message": format!("调度模型调用失败: {e},已兜底派发全部员工")}));
            fallback_all(&enabled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_employees() -> Vec<Employee> {
        vec![
            Employee {
                id: 1,
                name: "小码".into(),
                role: "前端工程师".into(),
                emoji: "💻".into(),
                color: "#2563eb".into(),
                system_prompt: String::new(),
                base_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                temperature: 0.7,
                capabilities: vec!["代码".into(), "网页".into()],
                enabled: true,
            },
            Employee {
                id: 2,
                name: "小文".into(),
                role: "文案策划".into(),
                emoji: "✍️".into(),
                color: "#7c3aed".into(),
                system_prompt: String::new(),
                base_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                temperature: 0.9,
                capabilities: vec!["文案".into(), "营销".into()],
                enabled: true,
            },
        ]
    }

    #[test]
    fn parse_valid_json() {
        let text = r#"好的,我来分析。{"assigned": [2], "reason": "这是文案任务,交给小文"}"#;
        let d = parse_decision(text, &sample_employees()).unwrap();
        assert_eq!(d.assigned, vec![2]);
        assert!(d.reason.contains("小文"));
    }

    #[test]
    fn parse_by_name() {
        let text = r#"{"assigned": ["小码"], "reason": "代码任务"}"#;
        let d = parse_decision(text, &sample_employees()).unwrap();
        assert_eq!(d.assigned, vec![1]);
    }

    #[test]
    fn parse_ignores_unknown_ids() {
        let text = r#"{"assigned": [999], "reason": "不存在的员工"}"#;
        assert!(parse_decision(text, &sample_employees()).is_none());
    }

    #[test]
    fn parse_non_json_falls_back_to_none() {
        let text = "抱歉,我不确定该派给谁。";
        assert!(parse_decision(text, &sample_employees()).is_none());
    }

    #[test]
    fn fallback_assigns_all_enabled() {
        let d = fallback_all(&sample_employees());
        assert_eq!(d.assigned, vec![1, 2]);
    }
}
