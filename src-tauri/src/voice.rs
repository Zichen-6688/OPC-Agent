//! 跨平台语音播报(TTS):
//! - macOS  : `say`(含中文语音 Tingting)
//! - Windows: PowerShell + System.Speech(SAPI,自动优先中文语音)
//! - Linux  : spd-say → espeak-ng → espeak → festival 链式回退

#[cfg(target_os = "windows")]
use std::io::Write;
#[cfg(target_os = "windows")]
use std::process::Stdio;
use std::process::Command;

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

#[cfg(target_os = "macos")]
fn speak_impl(text: &str, voice: Option<&str>) -> Result<(), String> {
    // 中文优先用 Tingting,否则用指定语音或系统默认
    let chosen = voice
        .map(|v| v.to_string())
        .unwrap_or_else(|| if has_cjk(text) { "Tingting".into() } else { String::new() });

    let mut cmd = Command::new("say");
    if !chosen.is_empty() {
        cmd.arg("-v").arg(&chosen);
    }
    cmd.arg(text);
    let out = cmd.output().map_err(|e| format!("调用 say 失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        // 指定的中文语音不可用时,回退默认语音
        if !chosen.is_empty() {
            return Command::new("say")
                .arg(text)
                .output()
                .map(|o| {
                    if o.status.success() {
                        Ok(())
                    } else {
                        Err(format!("say 输出失败: {}", String::from_utf8_lossy(&o.stderr)))
                    }
                })
                .map_err(|e| format!("调用 say 失败: {e}"))?;
        }
        Err(format!("say 输出失败: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(target_os = "windows")]
fn speak_impl(text: &str, voice: Option<&str>) -> Result<(), String> {
    let script = r#"
Add-Type -AssemblyName System.Speech;
[Console]::InputEncoding = [System.Text.Encoding]::UTF8;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
$zh = $s.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like 'zh*' } | Select-Object -First 1;
if ($zh) { $s.SelectVoice($zh.VoiceInfo.Name) };
$s.Speak([Console]::In.ReadToEnd());
"#;
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 PowerShell 失败: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("无法打开 stdin")?
        .write_all(text.as_bytes())
        .map_err(|e| format!("写入文本失败: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("等待 PowerShell 失败: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("TTS 失败: {}", String::from_utf8_lossy(&out.stderr)))
    }
}

#[cfg(target_os = "linux")]
fn speak_impl(text: &str, voice: Option<&str>) -> Result<(), String> {
    let _ = voice;
    // 依次探测可用 TTS 程序
    let candidates: [(&str, Vec<&str>); 4] = [
        ("spd-say", vec![]),
        ("espeak-ng", vec!["-v", "zh"]),
        ("espeak", vec!["-v", "zh"]),
        ("festival", vec![]),
    ];
    for (bin, args) in candidates {
        if Command::new("which").arg(bin).output().map(|o| o.status.success()).unwrap_or(false) {
            let mut cmd = Command::new(bin);
            cmd.args(&args).arg(text);
            let out = cmd.output().map_err(|e| format!("调用 {bin} 失败: {e}"))?;
            if out.status.success() {
                return Ok(());
            }
        }
    }
    Err("未找到可用的语音引擎,请安装 espeak-ng 或 speech-dispatcher".into())
}

/// 播报文本。voice 为可选语音名称(平台相关)。
pub fn speak(text: &str, voice: Option<&str>) -> Result<(), String> {
    speak_impl(text, voice)
}
