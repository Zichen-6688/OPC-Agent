/* ===== 一人公司智能体 前端逻辑 ===== */
(function () {
  "use strict";

  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  const $ = (id) => document.getElementById(id);
  const PALETTE = ["#2563eb", "#7c3aed", "#0d9488", "#ea580c", "#db2777", "#16a34a", "#dc2626", "#0891b2"];

  const state = {
    currentConvId: null,
    employees: [],
    conversations: [],
    settings: null,
    edition: "community",
    sending: false,
    streaming: new Map(),      // employeeId -> {el, done}
    micMode: null,             // 'sr' | 'vosk' | 'vosk-nomodel' | null
    recording: false,
    sr: null,
    voskCtx: null,
    voskRate: 44100,
    voskChunks: [],
    voskStream: null,
    voskNode: null,
    ttsEnabled: localStorage.getItem("opc_tts") !== "0",
    editingEmpId: null,
    pendingConfirm: null,
  };

  /* ---------------- 工具 ---------------- */

  function escapeHtml(s) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
  }

  function renderMd(text) {
    const esc = escapeHtml(text);
    let out = esc
      .replace(/```(\w*)\n([\s\S]*?)```/g, (m, lang, code) => `<pre class="codeblock"><code>${code.trim()}</code></pre>`)
      .replace(/`([^`\n]+)`/g, (m, c) => `<code class="inline-code">${c}</code>`)
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/^### (.*)$/gm, '<div class="md-h3">$1</div>')
      .replace(/^## (.*)$/gm, '<div class="md-h2">$1</div>')
      .replace(/^# (.*)$/gm, '<div class="md-h1">$1</div>')
      .replace(/^\s*[-*] (.*)$/gm, '<div class="md-li">• $1</div>')
      .replace(/^\s*\d+[.、] (.*)$/gm, '<div class="md-li">$1</div>');
    const paras = out.split(/\n{2,}/);
    return paras.map((p) => `<p>${p.replace(/\n/g, "<br>")}</p>`).join("");
  }

  let toastTimer = null;
  function toast(msg) {
    const el = $("toast");
    el.textContent = msg;
    el.classList.remove("hidden");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => el.classList.add("hidden"), 2600);
  }

  function scrollBottom(smooth) {
    const m = $("chatScroll") || $("messages");
    m.scrollTo({ top: m.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  }

  function fmtTime(iso) {
    if (!iso) return "";
    const t = new Date(iso.replace(" ", "T") + (iso.includes("Z") ? "" : ""));
    return isNaN(t) ? "" : `${String(t.getHours()).padStart(2, "0")}:${String(t.getMinutes()).padStart(2, "0")}`;
  }

  async function call(name, args, silent) {
    try {
      return await invoke(name, args || {});
    } catch (e) {
      if (!silent) toast(typeof e === "string" ? e : String(e));
      throw e;
    }
  }

  /* ---------------- 初始化 ---------------- */

  async function init() {
    bindEvents();
    bindModalEvents();
    try {
      await call("get_edition", null, true);
      $("editionBadge").textContent = "社区版";
    } catch (e) { /* 忽略 */ }

    state.settings = await call("get_settings", null, true).catch(() => null);

    await Promise.all([loadEmployees(), loadConversations()]);
    initVoice();
    applyTtsCheckbox();
  }

  /* ---------------- 员工管理 ---------------- */

  async function loadEmployees() {
    state.employees = await call("list_employees", null, true).catch(() => []);
    renderEmployees();
  }

  function renderEmployees() {
    const list = $("employeeList");
    $("teamCount").textContent = state.employees.length;
    if (!state.employees.length) {
      list.innerHTML = `<div class="emp-empty"><span class="big">👥</span>还没有 AI 员工<br>点击「＋ 添加」创建第一位员工吧</div>`;
      return;
    }
    list.innerHTML = state.employees.map((e) => `
      <div class="emp-card ${e.enabled ? "" : "disabled"}">
        <div class="emp-head">
          <div class="emp-avatar" style="background:linear-gradient(135deg, ${e.color}, ${e.color}cc)">${e.emoji || "🤖"}</div>
          <div class="emp-info">
            <div class="emp-name">${escapeHtml(e.name)} <span class="emp-dot" title="${e.enabled ? "启用中" : "已停用"}"></span></div>
            <div class="emp-role">${escapeHtml(e.role)}</div>
          </div>
          <div class="emp-actions">
            <button class="emp-btn" data-act="edit" data-id="${e.id}" title="编辑">✏️</button>
            <button class="emp-btn" data-act="del" data-id="${e.id}" title="删除">🗑️</button>
          </div>
        </div>
        ${e.capabilities && e.capabilities.length ? `<div class="emp-caps">${e.capabilities.map((c) => `<span class="cap-chip">${escapeHtml(c)}</span>`).join("")}</div>` : ""}
        <div class="emp-model">🧠 ${escapeHtml(e.model)}</div>
      </div>`).join("");
  }

  function openEmpModal(emp) {
    state.editingEmpId = emp ? emp.id : null;
    $("empModalTitle").textContent = emp ? "编辑 AI 员工" : "添加 AI 员工";
    $("f_name").value = emp ? emp.name : "";
    $("f_role").value = emp ? emp.role : "";
    $("f_emoji").value = emp ? emp.emoji : "🤖";
    $("f_capabilities").value = emp && emp.capabilities ? emp.capabilities.join(", ") : "";
    $("f_system_prompt").value = emp ? emp.system_prompt : "";
    $("f_base_url").value = emp ? emp.base_url : (state.settings && state.settings.orcBaseUrl) || "https://api.deepseek.com";
    $("f_api_key").value = emp ? emp.api_key : "";
    $("f_model").value = emp ? emp.model : (state.settings && state.settings.orcModel) || "deepseek-chat";
    const temp = emp ? emp.temperature : 0.7;
    $("f_temperature").value = temp;
    $("f_temp_val").textContent = temp.toFixed(1);
    $("f_enabled").checked = emp ? emp.enabled : true;

    // 颜色选择
    const sw = $("f_color");
    const cur = emp ? emp.color : PALETTE[0];
    sw.innerHTML = PALETTE.map((c) =>
      `<div class="swatch ${c === cur ? "selected" : ""}" data-c="${c}" style="background:${c}"></div>`).join("");
    $("modalEmployee").classList.remove("hidden");
  }

  async function saveEmployee() {
    const payload = {
      id: state.editingEmpId || 0,
      name: $("f_name").value.trim(),
      role: $("f_role").value.trim(),
      emoji: $("f_emoji").value.trim() || "🤖",
      color: document.querySelector("#f_color .swatch.selected")?.dataset.c || PALETTE[0],
      capabilities: $("f_capabilities").value.split(/[,，]/).map((s) => s.trim()).filter(Boolean),
      system_prompt: $("f_system_prompt").value,
      base_url: $("f_base_url").value.trim(),
      api_key: $("f_api_key").value.trim(),
      model: $("f_model").value.trim(),
      temperature: parseFloat($("f_temperature").value),
      enabled: $("f_enabled").checked,
    };
    if (!payload.name || !payload.base_url || !payload.model) {
      toast("姓名、API 地址、模型名称为必填项");
      return;
    }
    if (state.editingEmpId) {
      await call("update_employee", { employee: payload });
      toast("员工已更新");
    } else {
      await call("create_employee", { employee: payload });
      toast("员工已添加");
    }
    $("modalEmployee").classList.add("hidden");
    await loadEmployees();
  }

  function confirmDeleteEmployee(id) {
    const emp = state.employees.find((e) => e.id === id);
    state.pendingConfirm = async () => {
      await call("delete_employee", { id });
      toast(`已删除员工「${emp.name}」`);
      await loadEmployees();
    };
    $("confirmTitle").textContent = "删除员工";
    $("confirmText").textContent = `确定删除员工「${emp.name}」吗?删除后不可恢复。`;
    $("modalConfirm").classList.remove("hidden");
  }

  /* ---------------- 会话 ---------------- */

  async function loadConversations() {
    state.conversations = await call("list_conversations", null, true).catch(() => []);
    renderConversations();
  }

  function renderConversations() {
    const list = $("convList");
    if (!state.conversations.length) {
      list.innerHTML = `<div class="sidebar-label" style="margin-top:0">暂无会话,点击「＋ 新对话」开始</div>`;
      return;
    }
    list.innerHTML = state.conversations.map((c) => `
      <div class="conv-item ${c.id === state.currentConvId ? "active" : ""}" data-id="${c.id}">
        <span class="conv-title">${escapeHtml(c.title)}</span>
        <button class="conv-del" data-del="${c.id}" title="删除会话">✕</button>
      </div>`).join("");
  }

  function newChat() {
    state.currentConvId = null;
    $("chatTitle").textContent = "新对话";
    $("messages").innerHTML = "";
    $("welcome").classList.remove("hidden");
    renderConversations();
  }

  async function selectConversation(id) {
    state.currentConvId = id;
    $("welcome").classList.add("hidden");
    const conv = state.conversations.find((c) => c.id === id);
    $("chatTitle").textContent = conv ? conv.title : "对话";
    renderConversations();
    const [msgs, logs] = await Promise.all([
      call("get_messages", { conversationId: id }, true).catch(() => []),
      call("get_dispatch_logs", { conversationId: id }, true).catch(() => []),
    ]);
    renderHistory(msgs, logs);
  }

  function renderHistory(msgs, logs) {
    const box = $("messages");
    box.innerHTML = "";
    // 把派单日志插到对应 BOSS 消息之后
    const entries = msgs.map((m) => ({ type: "msg", data: m }));
    for (const log of logs) {
      let inserted = false;
      for (let i = 0; i < entries.length; i++) {
        const e = entries[i];
        if (e.type === "msg" && e.data.role === "boss" && e.data.content === log.bossMessage) {
          entries.splice(i + 1, 0, { type: "log", data: log });
          inserted = true;
          break;
        }
      }
      if (!inserted) entries.push({ type: "log", data: log });
    }
    for (const entry of entries) {
      if (entry.type === "msg") renderMessage(entry.data);
      else renderDispatchCard(entry.data.assignedIds || [], entry.data.reason || "", null);
    }
    scrollBottom(false);
  }

  function renderMessage(m) {
    if (m.role === "boss") renderBoss(m.content, m.createdAt);
    else if (m.role === "employee") {
      renderEmployee(m.employeeName || "员工", m.employeeEmoji || "🤖", m.employeeColor || "#2563eb", m.content, m.createdAt, true);
    } else if (m.role === "orchestrator") {
      renderSystemNote(m.content);
    }
  }

  /* ---------------- 消息渲染 ---------------- */

  function renderBoss(text, time) {
    const row = document.createElement("div");
    row.className = "msg-row boss";
    row.innerHTML = `
      <div class="msg-body">
        <div class="msg-meta"><span class="msg-name">BOSS</span>${time ? `<span>${fmtTime(time)}</span>` : ""}</div>
        <div class="bubble boss-bubble">${renderMd(text)}</div>
      </div>`;
    $("messages").appendChild(row);
    scrollBottom(true);
  }

  function renderEmployee(name, emoji, color, text, time, done) {
    const box = $("messages");
    const row = document.createElement("div");
    row.className = "msg-row employee";
    row.innerHTML = `
      <div class="msg-avatar" style="background:linear-gradient(135deg, ${color}, ${color}cc)">${emoji || "🤖"}</div>
      <div class="msg-body">
        <div class="msg-meta"><span class="msg-name">${escapeHtml(name)}</span><span class="msg-time">${time ? fmtTime(time) : ""}</span></div>
        <div class="bubble emp-bubble"><div class="msg-content"></div></div>
      </div>`;
    box.appendChild(row);
    const content = row.querySelector(".msg-content");
    if (text) content.innerHTML = renderMd(text);
    if (!done) {
      content.innerHTML = `<span class="thinking-dots"><span></span><span></span><span></span></span>`;
      const timeEl = row.querySelector(".msg-time");
      timeEl.textContent = "思考中…";
    }
    scrollBottom(true);
    return { row, content };
  }

  function renderDispatchCard(assigned, reason, time) {
    const box = $("messages");
    const card = document.createElement("div");
    card.className = "dispatch-card";
    card.innerHTML = `
      <span class="d-icon">📋</span>
      <span>已分派给</span>
      <div class="dispatch-avatars">${assigned.map((a) =>
        `<div class="d-avatar" style="background:${a.color || "#2563eb"}" title="${escapeHtml(a.name || "")}">${a.emoji || "🤖"}</div>`).join("")}</div>
      <span class="d-reason">${escapeHtml(reason || "")}</span>`;
    box.appendChild(card);
    scrollBottom(true);
  }

  function renderThinking() {
    const box = $("messages");
    const card = document.createElement("div");
    card.className = "thinking-card";
    card.id = "thinkingCard";
    card.innerHTML = `<span>🧠</span><span>调度中枢正在分析任务并匹配员工</span><span class="thinking-dots"><span></span><span></span><span></span></span>`;
    box.appendChild(card);
    scrollBottom(true);
  }

  function hideThinking() {
    const el = $("thinkingCard");
    if (el) el.remove();
  }

  function renderSystemNote(text, isError) {
    const box = $("messages");
    const note = document.createElement("div");
    note.className = "sys-note" + (isError ? " error" : "");
    note.textContent = text;
    box.appendChild(note);
    scrollBottom(true);
  }

  /* ---------------- 发送与流式 ---------------- */

  function startStream(emp) {
    if (emp.conversationId !== state.currentConvId) return;
    if (state.streaming.has(emp.employeeId)) return;
    const { content } = renderEmployee(emp.name, emp.emoji, emp.color, "", false);
    state.streaming.set(emp.employeeId, { el: content, done: false, text: "" });
  }

  function appendToken(p) {
    if (p.conversationId !== state.currentConvId) return;
    const s = state.streaming.get(p.employeeId);
    if (!s || s.done) return;
    s.text += p.delta;
    // 流式渲染:仅更新文本节点,避免频繁整块重绘
    if (!s.textEl) {
      s.el.innerHTML = "";
      s.textEl = document.createElement("span");
      s.el.appendChild(s.textEl);
    }
    s.textEl.textContent = s.text;
    scrollBottom(false);
  }

  function finishStream(p) {
    if (p.conversationId !== state.currentConvId) return;
    const s = state.streaming.get(p.employeeId);
    if (!s) return;
    if (p.error) {
      s.el.innerHTML = `<span style="color:#dc2626">⚠️ ${escapeHtml(p.error)}</span>`;
    } else {
      s.done = true;
      s.el.innerHTML = renderMd(p.content || s.text);
      if (state.ttsEnabled && p.content && p.content.trim()) {
        call("speak", { text: p.content.slice(0, 500) }, true).catch(() => {});
      }
    }
    state.streaming.delete(p.employeeId);
    scrollBottom(true);
  }

  async function sendMessage() {
    const input = $("inputText");
    const text = input.value.trim();
    if (!text || state.sending) return;
    state.sending = true;
    $("btnSend").disabled = true;
    $("welcome").classList.add("hidden");
    renderBoss(text);
    input.value = "";
    autoGrow();
    try {
      const convId = await call("send_boss_message", { conversationId: state.currentConvId, text });
      state.currentConvId = convId;
      $("chatTitle").textContent = text.replace(/\s+/g, "").slice(0, 16) || "新对话";
      await loadConversations();
    } catch (e) { /* toast 已显示 */ }
    state.sending = false;
    $("btnSend").disabled = false;
    input.focus();
  }

  /* ---------------- 语音输入 ---------------- */

  async function initVoice() {
    const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
    if (SR) {
      state.micMode = "sr";
      return;
    }
    try {
      const st = await call("stt_status", null, true);
      state.micMode = st.available ? "vosk" : "vosk-nomodel";
      renderSttInfo(st);
    } catch (e) {
      state.micMode = null;
    }
  }

  function renderSttInfo(st) {
    const info = $("sttInfo");
    if (state.micMode === "sr") {
      info.textContent = "✅ 本机支持系统语音识别(实时语音输入)";
    } else if (state.micMode === "vosk") {
      info.textContent = "✅ 离线语音模型已就绪(实时语音输入)";
    } else if (state.micMode === "vosk-nomodel") {
      info.textContent = "当前平台无系统语音识别,可下载离线模型(Vosk)获得语音输入";
      $("sttDownload").classList.remove("hidden");
    } else {
      info.textContent = "语音输入不可用,请使用文本输入";
    }
  }

  function onMicClick() {
    if (state.micMode === null || state.micMode === "vosk-nomodel") {
      if (state.micMode === "vosk-nomodel") {
        toast("请先在「设置」中下载离线语音模型");
      } else {
        toast("当前平台不支持语音输入");
      }
      return;
    }
    if (state.recording) { stopRecording(); return; }
    if (state.micMode === "sr") startSR();
    else startVosk();
  }

  function startSR() {
    const SR = window.SpeechRecognition || window.webkitSpeechRecognition;
    const sr = new SR();
    sr.lang = "zh-CN";
    sr.interimResults = true;
    sr.continuous = true;
    state.sr = sr;
    let finalText = "";
    sr.onresult = (e) => {
      let interim = "";
      for (let i = e.resultIndex; i < e.results.length; i++) {
        const r = e.results[i];
        if (r.isFinal) finalText += r[0].transcript;
        else interim += r[0].transcript;
      }
      $("inputText").value = finalText + interim;
      autoGrow();
    };
    sr.onerror = (e) => {
      stopSR();
      if (e.error !== "aborted" && e.error !== "no-speech") toast("语音识别错误: " + e.error);
    };
    sr.onend = () => { if (state.recording) { try { sr.start(); } catch (err) {} } };
    state.recording = true;
    $("btnMic").classList.add("recording");
    try { sr.start(); } catch (e) {}
  }

  function stopSR() {
    state.recording = false;
    $("btnMic").classList.remove("recording");
    if (state.sr) { try { state.sr.stop(); } catch (e) {} state.sr = null; }
  }

  async function startVosk() {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const Ctx = window.AudioContext || window.webkitAudioContext;
      const ctx = new Ctx();
      const src = ctx.createMediaStreamSource(stream);
      const node = ctx.createScriptProcessor(4096, 1, 1);
      state.voskChunks = [];
      state.voskStream = stream;
      state.voskCtx = ctx;
      state.voskRate = ctx.sampleRate || 44100;
      state.voskNode = node;
      node.onaudioprocess = (e) => {
        const data = e.inputBuffer.getChannelData(0);
        for (let i = 0; i < data.length; i++) state.voskChunks.push(data[i]);
      };
      src.connect(node);
      node.connect(ctx.destination);
      state.recording = true;
      $("btnMic").classList.add("recording");
      toast("🎤 正在聆听…再次点击结束");
    } catch (e) {
      toast("无法访问麦克风: " + (e.message || e));
    }
  }

  async function stopVosk() {
    state.recording = false;
    $("btnMic").classList.remove("recording");
    try {
      if (state.voskNode) state.voskNode.disconnect();
      if (state.voskCtx) state.voskCtx.close();
      if (state.voskStream) state.voskStream.getTracks().forEach((t) => t.stop());
    } catch (e) {}
    const samples = state.voskChunks;
    state.voskChunks = [];
    if (samples.length < 16000) { toast("语音太短,请再说一次"); return; }
    $("btnMic").classList.add("busy");
    try {
      const text = await call("stt_transcribe_pcm", { samples, sampleRate: state.voskRate }, true);
      if (text) {
        const input = $("inputText");
        input.value = (input.value.trim() ? input.value.trim() + " " : "") + text;
        autoGrow();
        input.focus();
      } else {
        toast("未识别到语音内容");
      }
    } catch (e) {
      toast("识别失败: " + (typeof e === "string" ? e : String(e)));
    } finally {
      $("btnMic").classList.remove("busy");
    }
  }

  function stopRecording() {
    if (state.micMode === "sr") stopSR();
    else stopVosk();
  }

  /* ---------------- 设置 ---------------- */

  function applyTtsCheckbox() {
    $("s_tts").checked = state.ttsEnabled;
  }

  function openSettings() {
    const s = state.settings || {};
    $("s_base_url").value = s.orcBaseUrl || "https://api.deepseek.com";
    $("s_api_key").value = s.orcApiKey || "";
    $("s_model").value = s.orcModel || "deepseek-chat";
    const t = s.orcTemperature != null ? s.orcTemperature : 0.3;
    $("s_temperature").value = t;
    $("s_temp_val").textContent = t.toFixed(1);
    applyTtsCheckbox();
    if (state.micMode === "vosk-nomodel" || state.micMode === "vosk") {
      const st = { available: state.micMode === "vosk" };
      renderSttInfo(st);
    } else {
      renderSttInfo();
    }
    $("modalSettings").classList.remove("hidden");
  }

  async function saveSettings() {
    const payload = {
      orcBaseUrl: $("s_base_url").value.trim(),
      orcApiKey: $("s_api_key").value.trim(),
      orcModel: $("s_model").value.trim(),
      orcTemperature: parseFloat($("s_temperature").value),
    };
    await call("save_settings", { settings: payload });
    state.settings = payload;
    state.ttsEnabled = $("s_tts").checked;
    localStorage.setItem("opc_tts", state.ttsEnabled ? "1" : "0");
    $("modalSettings").classList.add("hidden");
    toast("设置已保存");
  }

  function updateSttProgress(p) {
    if (p.done) {
      $("sttProgress").classList.add("hidden");
      if (p.ok) {
        toast("语音模型下载完成 ✅");
        state.micMode = "vosk";
        renderSttInfo({ available: true });
        $("sttDownload").classList.add("hidden");
      } else {
        toast("下载失败: " + (p.error || "未知错误"));
      }
      return;
    }
    $("sttProgress").classList.remove("hidden");
    const bar = $("sttProgressBar");
    bar.classList.add("active");
    bar.style.setProperty("--pct", (p.percent || 0) + "%");
    $("sttProgressText").textContent = (p.percent || 0) + "%";
  }

  /* ---------------- 事件绑定 ---------------- */

  function bindEvents() {
    $("btnNewChat").addEventListener("click", newChat);
    $("btnSend").addEventListener("click", sendMessage);
    $("btnToggleTeam").addEventListener("click", () => {
      const team = $("team");
      team.classList.toggle("collapsed");
    });
    $("btnSettings").addEventListener("click", openSettings);
    $("btnAddEmployee").addEventListener("click", () => openEmpModal(null));
    $("btnTestVoice").addEventListener("click", () => {
      call("speak", { text: "你好,我是一人公司智能体,随时为你服务。" }, true).catch((e) => toast(e));
    });
    $("btnDownloadModel").addEventListener("click", async () => {
      try {
        await call("stt_download_model");
        toast("开始下载语音模型…");
      } catch (e) { toast(e); }
    });
    $("btnSaveEmployee").addEventListener("click", saveEmployee);
    $("btnSaveSettings").addEventListener("click", saveSettings);
    $("btnConfirmOk").addEventListener("click", async () => {
      $("modalConfirm").classList.add("hidden");
      if (state.pendingConfirm) { const fn = state.pendingConfirm; state.pendingConfirm = null; await fn(); }
    });

    // 会话列表事件委托
    $("convList").addEventListener("click", (e) => {
      const del = e.target.closest(".conv-del");
      if (del) {
        e.stopPropagation();
        const id = parseInt(del.dataset.del);
        state.pendingConfirm = async () => {
          await call("delete_conversation", { id });
          if (state.currentConvId === id) newChat();
          await loadConversations();
        };
        $("confirmTitle").textContent = "删除会话";
        $("confirmText").textContent = "确定删除该会话及其全部消息吗?";
        $("modalConfirm").classList.remove("hidden");
        return;
      }
      const item = e.target.closest(".conv-item");
      if (item) selectConversation(parseInt(item.dataset.id));
    });

    // 员工卡片事件委托
    $("employeeList").addEventListener("click", (e) => {
      const btn = e.target.closest(".emp-btn");
      if (!btn) return;
      const id = parseInt(btn.dataset.id);
      if (btn.dataset.act === "edit") {
        const emp = state.employees.find((x) => x.id === id);
        if (emp) openEmpModal(emp);
      } else if (btn.dataset.act === "del") {
        confirmDeleteEmployee(id);
      }
    });

    // 颜色选择
    $("f_color").addEventListener("click", (e) => {
      const sw = e.target.closest(".swatch");
      if (!sw) return;
      document.querySelectorAll("#f_color .swatch").forEach((x) => x.classList.remove("selected"));
      sw.classList.add("selected");
    });

    // 温度显示
    $("f_temperature").addEventListener("input", (e) => {
      $("f_temp_val").textContent = parseFloat(e.target.value).toFixed(1);
    });
    $("s_temperature").addEventListener("input", (e) => {
      $("s_temp_val").textContent = parseFloat(e.target.value).toFixed(1);
    });

    // 输入框
    const input = $("inputText");
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
        e.preventDefault();
        sendMessage();
      }
    });
    input.addEventListener("input", autoGrow);

    // 欢迎快捷指令
    $("welcomeChips").addEventListener("click", (e) => {
      const chip = e.target.closest(".chip");
      if (chip) {
        input.value = chip.dataset.t;
        autoGrow();
        input.focus();
      }
    });

    // 麦克风
    $("btnMic").addEventListener("click", onMicClick);

    // 关闭弹窗
    document.querySelectorAll("[data-close]").forEach((btn) => {
      btn.addEventListener("click", () => $(btn.dataset.close).classList.add("hidden"));
    });
    document.querySelectorAll(".modal").forEach((m) => {
      m.addEventListener("mousedown", (e) => {
        if (e.target === m && !m.querySelector(".modal-box").contains(e.target)) m.classList.add("hidden");
      });
    });
    // Esc 关闭弹窗
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        document.querySelectorAll(".modal:not(.hidden)").forEach((m) => m.classList.add("hidden"));
      }
    });

    // 后端事件
    listen("dispatch-thinking", () => { renderThinking(); });
    listen("dispatch-decision", (e) => {
      hideThinking();
      const p = e.payload;
      if (p.conversationId !== state.currentConvId && state.currentConvId !== null) return;
      renderDispatchCard(p.assigned || [], p.reason || "", null);
    });
    listen("dispatch-warn", (e) => renderSystemNote(e.payload.message || "", false));
    listen("employee-start", (e) => startStream(e.payload));
    listen("llm-token", (e) => appendToken(e.payload));
    listen("employee-done", (e) => finishStream(e.payload));
    listen("task-error", (e) => {
      hideThinking();
      if (e.payload.conversationId === state.currentConvId) renderSystemNote(e.payload.error, true);
    });
    listen("task-done", () => { hideThinking(); loadConversations(); });
    listen("stt-progress", (e) => updateSttProgress(e.payload));
  }

  function bindModalEvents() {}

  function autoGrow() {
    const input = $("inputText");
    input.style.height = "auto";
    input.style.height = Math.min(input.scrollHeight, 140) + "px";
  }

  window.addEventListener("DOMContentLoaded", init);
})();
