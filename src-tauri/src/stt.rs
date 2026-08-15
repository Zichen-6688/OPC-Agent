//! Linux(及所有平台)离线语音识别:基于 Vosk 的 Rust 实现。
//!
//! macOS / Windows 走 WebView 原生 SpeechRecognition;Linux 的 webkit2gtk
//! 不支持该 API,前端会改用本模块:采集麦克风 PCM → 传到这里转写。
//!
//! 首次使用需在应用内下载模型(约 42MB 中文小模型)。

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

pub const MODEL_URL: &str = "https://alphacephei.com/vosk/models/vosk-model-small-cn-0.22.zip";
pub const MODEL_DIR_NAME: &str = "vosk-model-small-cn-0.22";

pub struct SttState {
    /// 模型根目录(app_data_dir),模型解压到 root/MODEL_DIR_NAME
    pub model_root: PathBuf,
    pub model: Mutex<Option<vosk::Model>>,
}

impl SttState {
    pub fn new(model_root: PathBuf) -> Self {
        Self {
            model_root,
            model: Mutex::new(None),
        }
    }

    pub fn model_path(&self) -> PathBuf {
        self.model_root.join(MODEL_DIR_NAME)
    }

    pub fn model_ready(&self) -> bool {
        self.model_path().join("am").exists() || self.model_path().join("conf").exists()
    }
}

/// 查询语音识别状态。
#[tauri::command]
pub fn stt_status(state: State<'_, SttState>) -> Value {
    json!({
        "available": state.model_ready(),
        "modelUrl": MODEL_URL,
        "modelName": MODEL_DIR_NAME,
    })
}

/// 转写 PCM 音频(前端 AudioContext 采集的 Float32 单声道)。
#[tauri::command]
pub fn stt_transcribe_pcm(
    state: State<'_, SttState>,
    samples: Vec<f32>,
    sample_rate: u32,
) -> Result<String, String> {
    if !state.model_ready() {
        return Err("语音模型未安装,请先点击「下载语音模型」".into());
    }
    let mut rec = {
        let mut guard = state.model.lock().map_err(|_| "模型锁获取失败".to_string())?;
        if guard.is_none() {
            let path = state
                .model_path()
                .to_str()
                .ok_or("模型路径非法")?
                .to_string();
            let model = vosk::Model::new(&path).ok_or_else(|| "模型加载失败".to_string())?;
            *guard = Some(model);
        }
        let model = guard.as_ref().ok_or("模型不可用")?;
        vosk::Recognizer::new(model, sample_rate as f32)
            .ok_or_else(|| "识别器初始化失败".to_string())?
    };
    let pcm: Vec<i16> = samples
        .iter()
        .map(|&f| (f.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    // 分块喂入,避免超大输入一次性处理
    for chunk in pcm.chunks(4000) {
        let _ = rec.accept_waveform(chunk);
    }
    let text = match rec.final_result() {
        vosk::CompleteResult::Single(s) => s.text.to_string(),
        vosk::CompleteResult::Multiple(m) => m
            .alternatives
            .first()
            .map(|a| a.text.to_string())
            .unwrap_or_default(),
    };
    Ok(text)
}

/// 后台下载并解压语音模型,进度通过 `stt-progress` 事件推送。
#[tauri::command]
pub fn stt_download_model(app: AppHandle, state: State<'_, SttState>) -> Result<(), String> {
    let target_dir = state.model_root.clone();
    if state.model_ready() {
        return Err("语音模型已存在".into());
    }
    tauri::async_runtime::spawn(async move {
        let result = download_and_extract(&app, &target_dir).await;
        let _ = app.emit("stt-progress", json!({"done": true, "ok": result.is_ok(), "error": result.err()}));
    });
    Ok(())
}

async fn download_and_extract(app: &AppHandle, target_dir: &PathBuf) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(MODEL_URL)
        .send()
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    let zip_path = target_dir.join("model.zip");
    let mut file = std::fs::File::create(&zip_path).map_err(|e| format!("创建文件失败: {e}"))?;
    use std::io::Write;
    let mut received: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        received += chunk.len() as u64;
        file.write_all(&chunk).map_err(|e| format!("写入失败: {e}"))?;
        if total > 0 {
            let pct = (received as f64 / total as f64 * 100.0) as u32;
            let _ = app.emit(
                "stt-progress",
                json!({"done": false, "percent": pct, "receivedMb": received / 1024 / 1024, "totalMb": total / 1024 / 1024}),
            );
        }
    }
    drop(file);

    // 解压
    let file = std::fs::File::open(&zip_path).map_err(|e| format!("打开压缩包失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("压缩包损坏: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("读取条目失败: {e}"))?;
        let out_path = target_dir.join(entry.name());
        // 防路径穿越
        let normalized = out_path
            .components()
            .filter(|c| !matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir))
            .collect::<PathBuf>();
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&normalized);
            continue;
        }
        if let Some(parent) = normalized.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = std::fs::File::create(&normalized).map_err(|e| format!("写入失败: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败: {e}"))?;
    }
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}
