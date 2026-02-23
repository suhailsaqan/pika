#!/usr/bin/env node

import { spawn } from "node:child_process";
import process from "node:process";
import readline from "node:readline";

import { Container, Editor, ProcessTerminal, Text, TUI } from "@mariozechner/pi-tui";

function requiredEnv(name) {
  const value = process.env[name];
  if (!value || !value.trim()) {
    throw new Error(`missing required env var ${name}`);
  }
  return value.trim();
}

function parseRelays() {
  const raw = process.env.PIKA_AGENT_RELAYS_JSON ?? "[]";
  try {
    const value = JSON.parse(raw);
    if (!Array.isArray(value)) return [];
    return value.map((x) => String(x)).filter((x) => x.trim().length > 0);
  } catch {
    return [];
  }
}

const BLUE = "\x1b[34m";
const CYAN = "\x1b[36m";
const DIM = "\x1b[2m";
const GREEN = "\x1b[32m";
const MAGENTA = "\x1b[35m";
const RED = "\x1b[31m";
const RESET = "\x1b[0m";
const PI_EVENT_PREFIX = "__PI_EVT__";

const editorTheme = {
  borderColor: (s) => `${DIM}${s}${RESET}`,
  selectList: {
    selectedPrefix: (s) => `${CYAN}${s}${RESET}`,
    selectedText: (s) => `${CYAN}${s}${RESET}`,
    description: (s) => `${DIM}${s}${RESET}`,
    scrollInfo: (s) => `${DIM}${s}${RESET}`,
    noMatch: (s) => `${DIM}${s}${RESET}`
  }
};

const cliBin = requiredEnv("PIKA_AGENT_PIKA_CLI_BIN");
const stateDir = requiredEnv("PIKA_AGENT_STATE_DIR");
const groupId = requiredEnv("PIKA_AGENT_GROUP_ID");
const selfPubkey = (process.env.PIKA_AGENT_SELF_PUBKEY ?? "").trim().toLowerCase();
const botPubkey = (process.env.PIKA_AGENT_BOT_PUBKEY ?? "").trim().toLowerCase();
const machineId = (process.env.PIKA_AGENT_MACHINE_ID ?? "").trim();
const flyAppName = (process.env.PIKA_AGENT_FLY_APP_NAME ?? "").trim();
const relays = parseRelays();

const relayArgs = relays.flatMap((relay) => ["--relay", relay]);
const sharedCliArgs = ["--state-dir", stateDir, ...relayArgs];

const terminal = new ProcessTerminal();
const tui = new TUI(terminal);
const root = new Container();
const header = new Text("");
const transcript = new Text("");
const status = new Text("");
const editor = new Editor(tui, editorTheme);

root.addChild(header);
root.addChild(transcript);
root.addChild(status);
root.addChild(editor);
tui.addChild(root);
tui.setFocus(editor);

const messages = [];
let listening = true;
let sending = false;
let shutdown = false;
let streamingPiText = "";

function renderHeader() {
  const relayList =
    relays.length === 0 ? "(default relays)" : relays.map((r) => `  - ${r}`).join("\n");
  header.setText(
    [
      `${GREEN}Connected to pi agent${RESET}`,
      `${DIM}group:${RESET} ${groupId}`,
      machineId && flyAppName
        ? `${DIM}machine:${RESET} ${machineId}  ${DIM}app:${RESET} ${flyAppName}`
        : "",
      `${DIM}relays:${RESET}`,
      relayList,
      `${DIM}commands:${RESET} /exit`,
      ""
    ]
      .filter((line) => line.length > 0)
      .join("\n")
  );
}

function renderTranscript() {
  if (messages.length === 0) {
    if (!streamingPiText) {
      transcript.setText(`${DIM}No messages yet. Type below and press Enter.${RESET}`);
      return;
    }
  }
  const lines = [];
  for (const msg of messages.slice(-120)) {
    const prefix = msg.role === "you" ? `${BLUE}you${RESET}` : msg.role === "pi" ? `${MAGENTA}pi${RESET}` : `${CYAN}sys${RESET}`;
    lines.push(`${prefix}> ${msg.content}`);
    lines.push("");
  }
  if (streamingPiText) {
    lines.push(`${MAGENTA}pi${RESET}> ${streamingPiText}${DIM}█${RESET}`);
    lines.push("");
  }
  transcript.setText(lines.join("\n").trimEnd());
}

function setStatus(text, level = "info") {
  const color = level === "error" ? RED : level === "ok" ? GREEN : DIM;
  status.setText(`${color}${text}${RESET}`);
  tui.requestRender();
}

function addMessage(role, content) {
  messages.push({ role, content: String(content ?? "").trim() });
  renderTranscript();
  tui.requestRender();
}

function appendStreamingPiText(text) {
  const chunk = String(text ?? "");
  if (!chunk) return;
  streamingPiText += chunk;
  renderTranscript();
  tui.requestRender();
}

function spawnCli(args, stdio = ["ignore", "pipe", "pipe"]) {
  return spawn(cliBin, [...sharedCliArgs, ...args], { stdio });
}

async function runSend(content) {
  return await new Promise((resolve, reject) => {
    const proc = spawnCli(["send", "--group", groupId, "--content", content]);
    let stderr = "";
    proc.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });
    proc.on("error", (err) => reject(err));
    proc.on("close", (code) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(new Error(stderr.trim() || `pika-cli send exited with status ${code}`));
    });
  });
}

function startListener() {
  const lookbackSec = process.env.PIKA_AGENT_LOOKBACK_SEC?.trim() || "3600";
  const proc = spawnCli(["listen", "--timeout", "0", "--lookback", lookbackSec]);
  const out = readline.createInterface({ input: proc.stdout });
  const err = readline.createInterface({ input: proc.stderr });

  function renderPiEvent(event) {
    const kind = String(event.kind ?? "");
    if (kind === "status") {
      const msg = String(event.message ?? "").trim();
      if (msg === "processing") setStatus("Pi is working...");
      if (msg === "done") setStatus("Pi finished", "ok");
      return;
    }
    if (kind === "text_delta") {
      appendStreamingPiText(String(event.text ?? ""));
      return;
    }
    if (kind === "error") {
      addMessage("sys", `pi error: ${String(event.message ?? "unknown error")}`);
      setStatus("Pi error", "error");
      return;
    }
    if (kind !== "assistant_event") return;
    const eventType = String(event.event_type ?? "event");
    if (!eventType.toLowerCase().includes("tool")) {
      return;
    }
    const name =
      String(event.name ?? event.tool_name ?? event.toolName ?? "tool").trim() || "tool";
    const args = String(event.arguments ?? "").trim();
    const argsSuffix = args ? ` ${DIM}${args}${RESET}` : "";
    addMessage("sys", `[tool:${eventType}] ${name}${argsSuffix}`);
    setStatus(`Pi tool: ${name}`);
  }

  out.on("line", (line) => {
    const trimmed = line.trim();
    if (!trimmed) return;
    let event;
    try {
      event = JSON.parse(trimmed);
    } catch {
      return;
    }
    if (event.type === "welcome" && event.nostr_group_id === groupId) {
      setStatus("Welcome received", "ok");
      return;
    }
    if (event.type !== "message") return;
    if (event.nostr_group_id !== groupId) return;
    const fromPubkey = String(event.from_pubkey ?? "").toLowerCase();
    if (fromPubkey && fromPubkey === selfPubkey) return;
    const fromBot = Boolean(fromPubkey && fromPubkey === botPubkey);
    const content = String(event.content ?? "").trim();
    if (!content) return;
    if (fromBot && content.startsWith(PI_EVENT_PREFIX)) {
      try {
        renderPiEvent(JSON.parse(content.slice(PI_EVENT_PREFIX.length)));
      } catch {
        addMessage("sys", "received malformed pi event");
      }
      return;
    }
    const role = fromBot ? "pi" : "sys";
    if (fromBot) {
      streamingPiText = "";
    }
    addMessage(role, content);
    if (fromBot) {
      setStatus("Pi replied", "ok");
    }
  });

  err.on("line", (line) => {
    if (!line.trim()) return;
    setStatus(`listen: ${line.trim()}`);
  });

  proc.on("error", (error) => {
    listening = false;
    setStatus(`listen failed: ${error.message}`, "error");
  });

  proc.on("close", (code) => {
    listening = false;
    if (!shutdown) {
      setStatus(`listen exited (${code ?? "unknown"})`, "error");
    }
  });

  return proc;
}

async function cleanup(exitCode = 0) {
  if (shutdown) return;
  shutdown = true;
  listening = false;
  if (listener && !listener.killed) {
    listener.kill("SIGTERM");
  }
  try {
    tui.stop();
  } finally {
    process.exit(exitCode);
  }
}

renderHeader();
renderTranscript();
setStatus("Listening for messages...");
tui.start();

editor.onSubmit = async (text) => {
  const input = String(text ?? "").trim();
  if (!input) return;
  if (input === "/exit") {
    await cleanup(0);
    return;
  }
  if (!listening) {
    setStatus("listen process is not running", "error");
    return;
  }
  editor.addToHistory(input);
  editor.setText("");
  streamingPiText = "";
  addMessage("you", input);
  sending = true;
  editor.disableSubmit = true;
  setStatus("Sending...");
  try {
    await runSend(input);
    setStatus("Waiting for pi...");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    addMessage("sys", `send failed: ${message}`);
    setStatus("Send failed", "error");
  } finally {
    sending = false;
    editor.disableSubmit = false;
    tui.requestRender();
  }
};

const listener = startListener();

process.on("SIGINT", async () => {
  await cleanup(0);
});

process.on("SIGTERM", async () => {
  await cleanup(0);
});

process.on("uncaughtException", async (error) => {
  setStatus(`fatal: ${error.message}`, "error");
  await cleanup(1);
});

process.on("unhandledRejection", async (error) => {
  const message = error instanceof Error ? error.message : String(error);
  setStatus(`fatal: ${message}`, "error");
  await cleanup(1);
});
