# 一人公司智能体(OPC Agent)· 多 AI 员工协调的智能工作台

> BOSS 用文字或语音下达指令,Agent 自动把任务分派给最合适的 AI 员工,员工并行协作、流式汇报结果。

**社区版(本仓库)**:核心功能全部开源,基于 MIT 协议。
**商业版(闭源)**:品牌定制、员工配置批量导入导出、审计日志导出、专属技术支持等能力,由作者直接提供(详见下文「版本说明」)。

---

## ✨ 功能特性

| 能力 | 说明 |
|---|---|
| 🤖 多 AI 员工 | 自由添加 / 编辑 / 删除员工,自定义姓名、岗位、头像、擅长领域、系统提示词 |
| 🧠 智能派单 | 调度中枢根据 BOSS 指令与员工名册,自动匹配 1~N 名最合适的员工并行执行 |
| 💬 文字对话 | 会话式交互,支持多轮上下文,消息持久化 |
| 🎤 语音输入 | macOS / Windows 原生语音识别;Linux 用 Vosk 离线模型(应用内一键下载) |
| 🔊 语音播报 | 员工回复完成自动朗读(TTS),跨三平台 |
| ⚡ 流式输出 | 员工回复实时逐字呈现,无需等待 |
| 🗃️ SQLite 存储 | 员工、会话、消息、派单日志全部本地存储,数据完全私有 |
| 🌍 多模型兼容 | OpenAI 兼容协议:DeepSeek / OpenAI / Moonshot / 本地 Ollama 均可 |
| 📦 跨平台 | Windows / macOS / Linux 三平台原生桌面应用 |

## 🖥️ 技术栈

- **Rust** — 后端核心(异步、流式、并发)
- **Tauri 2** — 桌面框架,打包体积小(<10MB)、内存占用低
- **SQLite (rusqlite)** — 本地数据持久化,WAL 模式多连接并发
- **Vanilla HTML/CSS/JS** — 无构建步骤,前端零依赖,开箱即改

## 🚀 快速开始

### 环境要求

| 平台 | 依赖 |
|---|---|
| macOS | Xcode Command Line Tools(`xcode-select --install`) |
| Windows | VS Build Tools(含 C++ 桌面开发)+ WebView2(系统自带) |
| Linux | `webkit2gtk-4.1`、`libappindicator3`、`librsvg2-dev`、`build-essential`、`libssl-dev` |

Ubuntu/Debian 示例:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### 运行

```bash
# 1. 安装 Rust(若未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. 开发模式运行
cargo tauri dev          # 或: cd src-tauri && cargo run

# 3. 构建安装包
cargo tauri build
```

> 提示:Linux 上若提示缺少 `cargo-tauri`,先 `cargo install tauri-cli` 或使用 `npm i -g @tauri-apps/cli`。

### 首次使用

1. 启动应用 → 点击「设置」,填入调度中枢的 API Key(默认 DeepSeek,可换任意 OpenAI 兼容服务)
2. 右侧「AI 员工」→「＋ 添加」,创建你的员工团队(可预设多套模型)
3. 在输入框下达指令,如:"帮我写一篇产品发布会的宣传文案"
4. 观察调度卡片 → 员工并行回复 → 完成

### 语音输入说明

- **macOS / Windows**:开箱即用,点击 🎤 直接说(系统原生语音识别,零依赖)
- **Linux**:使用 Vosk 离线识别(可选特性)。安装运行时后构建:

  ```bash
  # 1. 安装 Vosk 运行时(自动从 PyPI 轮子提取原生库)
  sudo bash scripts/install_vosk_runtime.sh

  # 2. 启用 vosk-stt 特性构建
  cd src-tauri && cargo tauri build --features vosk-stt
  ```

  首次使用在「设置 → 语音」中一键下载中文识别模型(约 42MB,本地离线识别)。

## 🏗️ 架构

```
┌─────────────────────────────────────────────────────┐
│                   前端 (HTML/CSS/JS)                  │
│   聊天界面 · 员工管理面板 · 语音 · 流式渲染 · 设置      │
└───────────────┬─────────────────────────────────────┘
                │ Tauri IPC (invoke / event)
┌───────────────▼─────────────────────────────────────┐
│                  Rust 后端 (src-tauri/src)            │
│  ┌──────────┐  ┌──────────┐  ┌────────────────────┐  │
│  │ commands │→ │orchestrator│→│ llm (流式调用)      │  │
│  └──────────┘  └──────────┘  └────────────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌────────────────────┐  │
│  │   db     │  │  voice   │  │ stt (vosk, Linux)  │  │
│  └──────────┘  └──────────┘  └────────────────────┘  │
└───────────────┬─────────────────────────────────────┘
                │ rusqlite (WAL)
        ┌───────▼───────┐
        │  opc-agent.db  │  员工 / 会话 / 消息 / 派单日志 / 设置
        └───────────────┘
```

**派单流水线**:

```
BOSS 指令
   │
   ▼
调度中枢 LLM ──▶ 解析员工名册(岗位/擅长)──▶ 输出 JSON 派单决策
   │                                              │
   │          兜底:解析失败 → 派发全部启用员工      │
   ▼                                              ▼
落库派单日志 ◀── 事件 dispatch-decision ──▶ 员工 A 并行执行(流式)──▶ 落库 + 事件
   │                                              │
   ▼                                              ▼
前端展示派单卡片                        员工 B 并行执行(流式)──▶ 落库 + 事件
```

## 📁 项目结构

```
opc-agent/
├── frontend/              # 前端(纯静态,无构建)
│   ├── index.html         # 布局:侧栏 / 聊天 / 员工面板 / 弹窗
│   ├── styles.css         # 明亮简洁主题(蓝白配色)
│   └── app.js             # 交互逻辑、流式渲染、语音、派单展示
├── src-tauri/
│   ├── src/
│   │   ├── main.rs        # 入口
│   │   ├── lib.rs         # Tauri 装配、命令注册
│   │   ├── commands.rs    # IPC 命令 + 派单流水线
│   │   ├── db.rs          # SQLite 数据层(模型/迁移/查询)
│   │   ├── llm.rs         # OpenAI 兼容 LLM 调用(流式 SSE)
│   │   ├── orchestrator.rs# 智能调度(JSON 决策 + 兜底)
│   │   ├── voice.rs       # 跨平台 TTS(macOS/Windows/Linux)
│   │   └── stt.rs         # Vosk 离线语音识别(vosk-stt 特性)
│   ├── Cargo.toml         # 特性开关:vosk-stt(可选)
│   ├── tauri.conf.json    # Tauri 配置
│   └── capabilities/      # 权限声明
├── scripts/make_icons.py  # 图标生成脚本(PIL + sips + iconutil)
└── LICENSE                # MIT
```

> 💡 **版本说明**:一人公司智能体(OPC Agent)采用「社区版开源 + 商业版闭源」模式。本仓库为**社区版**(MIT 协议,全部核心功能开源);商业版为闭源产品,提供品牌定制、员工配置批量导入导出、审计日志导出、专属技术支持等能力,由作者直接提供,不在此仓库公开。商业版获取与定制咨询:**联系作者**。

## 🗄️ 数据存储

| 数据 | 位置 |
|---|---|
| 数据库 | `~/Library/Application Support/com.opc.agent/opc-agent.db`(macOS)<br>`%APPDATA%\com.opc.agent\opc-agent.db`(Windows)<br>`~/.local/share/com.opc.agent/opc-agent.db`(Linux) |
| 语音模型 | 同目录下 `vosk-model-small-cn-0.22/` |

## 🧪 测试

```bash
cd src-tauri
cargo test    # 调度解析、员工管理、历史组装等单元测试
```

## 🤝 参与贡献

社区版欢迎一切贡献:功能建议、Bug 报告、代码 PR。请保持代码风格与现有结构一致,并在提交前运行 `cargo test`。

## 📄 许可证

- 社区版:MIT License(见 [LICENSE](LICENSE))
- 商业版:闭源,由作者直接提供(不在此仓库公开)
