#!/usr/bin/env python3
"""Bridge between marmotd JSONL (stdin/stdout) and Pi.

Modes:
- PTY call mode (default): auto-accept data calls and attach remote Pi TUI over MoQ call data.
- Legacy chat mode (optional): set PI_BRIDGE_ENABLE_CHAT=1 to keep old RPC text reply behavior.
"""

from __future__ import annotations

import base64
import json
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import threading
import time


EVENT_PREFIX = "__PI_EVT__"
SEND_LOCK = threading.Lock()
PTY_LOCK = threading.Lock()

my_pubkey: str | None = None
pi_proc: subprocess.Popen[bytes] | None = None

active_call_id: str | None = None
active_pty_pid: int | None = None
active_pty_fd: int | None = None
pty_reader_thread: threading.Thread | None = None
active_replay_thread: threading.Thread | None = None
active_replay_stop = threading.Event()
active_replay_peer_ready = threading.Event()

legacy_chat_enabled = os.environ.get("PI_BRIDGE_ENABLE_CHAT", "").strip() == "1"
replay_file = os.environ.get("PI_BRIDGE_REPLAY_FILE", "").strip()
replay_speed_raw = os.environ.get("PI_BRIDGE_REPLAY_SPEED", "1").strip()
replay_initial_delay_ms_raw = os.environ.get("PI_BRIDGE_REPLAY_INITIAL_DELAY_MS", "1000").strip()
replay_wait_for_peer_sec_raw = os.environ.get("PI_BRIDGE_REPLAY_WAIT_FOR_PEER_SEC", "3").strip()


def log(msg: str) -> None:
    print(f"[pi-bridge] {msg}", file=sys.stderr, flush=True)


def send_to_marmotd(cmd: dict) -> None:
    line = json.dumps(cmd, separators=(",", ":"))
    with SEND_LOCK:
        print(line, flush=True)


def send_to_pi(proc: subprocess.Popen[bytes], msg: dict) -> None:
    line = json.dumps(msg) + "\n"
    assert proc.stdin is not None
    proc.stdin.write(line.encode())
    proc.stdin.flush()


def emit_pi_event(group_id: str, payload: dict) -> None:
    send_to_marmotd(
        {
            "cmd": "send_message",
            "nostr_group_id": group_id,
            "content": EVENT_PREFIX + json.dumps(payload, separators=(",", ":")),
        }
    )


def start_pi_rpc() -> subprocess.Popen[bytes]:
    env = os.environ.copy()
    cmd = ["pi", "--mode", "rpc", "--no-session", "--provider", "anthropic"]
    model = os.environ.get("PI_MODEL")
    if model:
        cmd.extend(["--model", model])
    log(f"starting legacy pi rpc: {' '.join(cmd)}")
    return subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
        env=env,
        bufsize=0,
    )


def collect_pi_response(proc: subprocess.Popen[bytes], group_id: str) -> str:
    text_parts: list[str] = []
    pending_delta_parts: list[str] = []
    last_delta_emit = time.time()
    assert proc.stdout is not None
    for raw in proc.stdout:
        raw = raw.decode().strip()
        if not raw:
            continue
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            continue

        etype = event.get("type")
        if etype == "message_update":
            delta_event = event.get("assistantMessageEvent", {})
            delta_type = delta_event.get("type")
            if delta_type == "text_delta":
                delta = delta_event.get("delta", "")
                text_parts.append(delta)
                pending_delta_parts.append(delta)
                pending_joined = "".join(pending_delta_parts)
                now = time.time()
                should_emit = (
                    len(pending_joined) >= 120
                    or "\n" in delta
                    or now - last_delta_emit >= 0.5
                )
                if should_emit and pending_joined:
                    emit_pi_event(group_id, {"kind": "text_delta", "text": pending_joined})
                    pending_delta_parts = []
                    last_delta_emit = now
            else:
                payload = {
                    "kind": "assistant_event",
                    "event_type": str(delta_type or "unknown"),
                }
                for key in (
                    "name",
                    "tool_name",
                    "toolName",
                    "toolCallId",
                    "callId",
                    "id",
                ):
                    value = delta_event.get(key)
                    if isinstance(value, str) and value:
                        payload[key] = value
                if isinstance(delta_event.get("arguments"), str):
                    payload["arguments"] = delta_event["arguments"][:300]
                if isinstance(delta_event.get("text"), str):
                    payload["text"] = delta_event["text"][:300]
                emit_pi_event(group_id, payload)
        elif etype == "agent_end":
            break
        elif etype == "response" and not event.get("success"):
            emit_pi_event(
                group_id,
                {"kind": "error", "message": str(event.get("error") or "pi response failed")},
            )
            break

    if pending_delta_parts:
        emit_pi_event(group_id, {"kind": "text_delta", "text": "".join(pending_delta_parts)})
    return "".join(text_parts)


def encode_call_payload_hex(obj: dict) -> str:
    return json.dumps(obj, separators=(",", ":")).encode("utf-8").hex()


def decode_call_payload(msg: dict) -> dict | None:
    payload_hex = str(msg.get("payload_hex", "")).strip()
    if not payload_hex:
        return None
    try:
        raw = bytes.fromhex(payload_hex)
        payload = json.loads(raw.decode("utf-8"))
        if isinstance(payload, dict):
            return payload
    except Exception:
        return None
    return None


def send_call_payload(call_id: str, payload_obj: dict) -> None:
    send_to_marmotd(
        {
            "cmd": "send_call_data",
            "call_id": call_id,
            "payload_hex": encode_call_payload_hex(payload_obj),
        }
    )


def parse_replay_speed(raw: str) -> float:
    try:
        speed = float(raw)
        if speed > 0:
            return speed
    except Exception:
        pass
    return 1.0


def load_replay_frames(path: str) -> list[tuple[float, bytes]]:
    with open(path, "r", encoding="utf-8") as fh:
        doc = json.load(fh)
    if not isinstance(doc, dict):
        raise ValueError("replay file must contain an object")
    frames_raw = doc.get("frames")
    if not isinstance(frames_raw, list) or not frames_raw:
        raise ValueError("replay file has no frames")
    out: list[tuple[float, bytes]] = []
    for idx, frame in enumerate(frames_raw):
        if not isinstance(frame, dict):
            raise ValueError(f"frame {idx} must be an object")
        data_b64 = str(frame.get("stdout_b64", "")).strip()
        if not data_b64:
            raise ValueError(f"frame {idx} missing stdout_b64")
        delay_ms = float(frame.get("delay_ms", 0) or 0)
        if delay_ms < 0:
            delay_ms = 0
        try:
            payload = base64.b64decode(data_b64)
        except Exception as err:
            raise ValueError(f"frame {idx} invalid stdout_b64: {err}") from err
        out.append((delay_ms / 1000.0, payload))
    return out


REPLAY_SPEED = parse_replay_speed(replay_speed_raw)
REPLAY_INITIAL_DELAY_MS = 1000.0
REPLAY_WAIT_FOR_PEER_SEC = 3.0
try:
    REPLAY_INITIAL_DELAY_MS = max(float(replay_initial_delay_ms_raw), 0.0)
except Exception:
    REPLAY_INITIAL_DELAY_MS = 1000.0
try:
    REPLAY_WAIT_FOR_PEER_SEC = max(float(replay_wait_for_peer_sec_raw), 0.0)
except Exception:
    REPLAY_WAIT_FOR_PEER_SEC = 3.0
REPLAY_FRAMES: list[tuple[float, bytes]] = []
if replay_file:
    try:
        REPLAY_FRAMES = load_replay_frames(replay_file)
        log(
            "replay mode enabled: "
            f"file={replay_file} frames={len(REPLAY_FRAMES)} "
            f"speed={REPLAY_SPEED:.2f}x initial_delay_ms={REPLAY_INITIAL_DELAY_MS:.0f} "
            f"wait_for_peer_sec={REPLAY_WAIT_FOR_PEER_SEC:.1f}"
        )
    except Exception as err:
        log(f"invalid PI_BRIDGE_REPLAY_FILE={replay_file}: {err}")
        sys.exit(2)


def set_pty_size(fd: int, cols: int, rows: int) -> None:
    winsz = struct.pack("HHHH", rows, cols, 0, 0)
    try:
        import fcntl

        fcntl.ioctl(fd, termios.TIOCSWINSZ, winsz)
    except Exception as err:
        log(f"set pty size failed: {err}")


def pty_reader_loop(call_id: str, pid: int, fd: int) -> None:
    global active_call_id, active_pty_fd, active_pty_pid
    flush_interval_sec = 0.03
    max_chunk_bytes = 32768
    pending = bytearray()
    last_flush = time.monotonic()
    next_seq = 0

    def flush_pending() -> None:
        nonlocal pending, last_flush, next_seq
        if not pending:
            return
        send_call_payload(
            call_id,
            {
                "t": "stdout",
                "s": next_seq,
                "d": base64.b64encode(bytes(pending)).decode("ascii"),
            },
        )
        pending.clear()
        last_flush = time.monotonic()
        next_seq += 1

    try:
        while True:
            with PTY_LOCK:
                if active_call_id != call_id or active_pty_fd != fd:
                    break
            rlist, _, _ = select.select([fd], [], [], 0.02)
            if fd in rlist:
                try:
                    chunk = os.read(fd, 8192)
                except OSError:
                    break
                if not chunk:
                    break
                pending.extend(chunk)
            now = time.monotonic()
            if pending and (
                len(pending) >= max_chunk_bytes or now - last_flush >= flush_interval_sec
            ):
                flush_pending()
    finally:
        flush_pending()
        exit_code = None
        try:
            _, status = os.waitpid(pid, os.WNOHANG)
            if status != 0:
                if os.WIFEXITED(status):
                    exit_code = os.WEXITSTATUS(status)
                elif os.WIFSIGNALED(status):
                    exit_code = 128 + os.WTERMSIG(status)
        except ChildProcessError:
            pass
        send_call_payload(call_id, {"t": "exit", "code": exit_code})
        send_to_marmotd({"cmd": "end_call", "call_id": call_id, "reason": "pty_exit"})
        with PTY_LOCK:
            if active_call_id == call_id:
                active_call_id = None
            if active_pty_fd == fd:
                active_pty_fd = None
            if active_pty_pid == pid:
                active_pty_pid = None


def replay_sleep(seconds: float) -> bool:
    if seconds <= 0:
        return True
    deadline = time.time() + seconds
    while True:
        if active_replay_stop.wait(timeout=0.02):
            return False
        if time.time() >= deadline:
            return True


def replay_loop(call_id: str) -> None:
    global active_call_id, active_replay_thread
    next_seq = 0
    try:
        if REPLAY_WAIT_FOR_PEER_SEC > 0:
            active_replay_peer_ready.wait(timeout=REPLAY_WAIT_FOR_PEER_SEC)
        if REPLAY_INITIAL_DELAY_MS > 0:
            if not replay_sleep((REPLAY_INITIAL_DELAY_MS / 1000.0) / REPLAY_SPEED):
                return
        for delay_sec, chunk in REPLAY_FRAMES:
            with PTY_LOCK:
                if active_call_id != call_id:
                    break
            if not replay_sleep(delay_sec / REPLAY_SPEED):
                break
            send_call_payload(
                call_id,
                {
                    "t": "stdout",
                    "s": next_seq,
                    "d": base64.b64encode(chunk).decode("ascii"),
                },
            )
            next_seq += 1
    finally:
        send_call_payload(call_id, {"t": "exit", "code": 0})
        send_to_marmotd({"cmd": "end_call", "call_id": call_id, "reason": "replay_done"})
        with PTY_LOCK:
            if active_call_id == call_id:
                active_call_id = None
            active_replay_thread = None


def start_pi_pty(call_id: str) -> None:
    global active_call_id, active_pty_pid, active_pty_fd, pty_reader_thread, active_replay_thread
    need_stop = False
    with PTY_LOCK:
        if active_call_id and active_call_id != call_id:
            need_stop = True
        elif active_call_id == call_id and active_pty_fd is not None:
            return
    if need_stop:
        stop_pi_pty("superseded")
    with PTY_LOCK:
        active_call_id = call_id

    if REPLAY_FRAMES:
        active_replay_stop.clear()
        active_replay_peer_ready.clear()
        log(f"starting replay PTY stream for call {call_id}")
        t = threading.Thread(target=replay_loop, args=(call_id,), daemon=True)
        with PTY_LOCK:
            active_replay_thread = t
            active_pty_pid = None
            active_pty_fd = None
        t.start()
        return

    env = os.environ.copy()
    cmd = ["pi", "--no-session", "--provider", "anthropic"]
    model = os.environ.get("PI_MODEL")
    if model:
        cmd.extend(["--model", model])
    log(f"starting pi pty for call {call_id}: {' '.join(cmd)}")
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(cmd[0], cmd, env)
        os._exit(1)

    with PTY_LOCK:
        active_pty_pid = pid
        active_pty_fd = fd
        t = threading.Thread(target=pty_reader_loop, args=(call_id, pid, fd), daemon=True)
        pty_reader_thread = t
        t.start()


def stop_pi_pty(reason: str) -> None:
    global active_call_id, active_pty_fd, active_pty_pid, active_replay_thread
    with PTY_LOCK:
        call_id = active_call_id
        pid = active_pty_pid
        fd = active_pty_fd
        replay_thread = active_replay_thread
        active_call_id = None
        active_pty_fd = None
        active_pty_pid = None
        active_replay_thread = None
    active_replay_stop.set()
    if fd is not None:
        try:
            os.close(fd)
        except Exception:
            pass
    if pid is not None:
        try:
            os.kill(pid, signal.SIGTERM)
        except Exception:
            pass
    if replay_thread is not None and replay_thread.is_alive():
        replay_thread.join(timeout=0.5)
    if call_id:
        log(f"stopped pty call {call_id}: {reason}")


def handle_call_data(msg: dict) -> None:
    call_id = str(msg.get("call_id", ""))
    with PTY_LOCK:
        if call_id == active_call_id:
            active_replay_peer_ready.set()

    payload = decode_call_payload(msg)
    if not payload:
        return
    msg_type = str(payload.get("t", ""))
    if msg_type == "exit":
        send_to_marmotd({"cmd": "end_call", "call_id": call_id, "reason": "peer_exit"})
        return

    with PTY_LOCK:
        if call_id != active_call_id:
            return
        fd = active_pty_fd

    if fd is None:
        return

    if msg_type == "stdin":
        data_b64 = str(payload.get("d", ""))
        if not data_b64:
            return
        try:
            data = base64.b64decode(data_b64)
            os.write(fd, data)
        except Exception as err:
            log(f"stdin payload write failed: {err}")
    elif msg_type == "resize":
        cols = int(payload.get("cols", 0) or 0)
        rows = int(payload.get("rows", 0) or 0)
        if cols > 0 and rows > 0:
            set_pty_size(fd, cols, rows)


def main() -> None:
    global my_pubkey, pi_proc
    if legacy_chat_enabled:
        pi_proc = start_pi_rpc()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        msg_type = msg.get("type")
        if msg_type == "ready":
            my_pubkey = msg.get("pubkey")
            log(f"marmotd ready, pubkey={my_pubkey}")
            send_to_marmotd({"cmd": "publish_keypackage"})
            continue

        if msg_type == "call_invite_received":
            call_id = str(msg.get("call_id", ""))
            with PTY_LOCK:
                busy = active_call_id is not None
            if busy:
                send_to_marmotd(
                    {"cmd": "reject_call", "call_id": call_id, "reason": "busy"}
                )
            else:
                send_to_marmotd({"cmd": "accept_call", "call_id": call_id})
            continue

        if msg_type == "call_session_started":
            call_id = str(msg.get("call_id", ""))
            start_pi_pty(call_id)
            continue

        if msg_type == "call_data":
            handle_call_data(msg)
            continue

        if msg_type == "call_session_ended":
            call_id = str(msg.get("call_id", ""))
            should_stop = False
            with PTY_LOCK:
                should_stop = call_id == active_call_id
            if should_stop:
                stop_pi_pty("remote_end")
            continue

        if msg_type == "message_received" and legacy_chat_enabled and pi_proc is not None:
            if msg.get("from_pubkey") == my_pubkey:
                continue
            content = msg.get("content", "")
            group_id = msg.get("nostr_group_id", "")
            send_to_pi(pi_proc, {"type": "prompt", "message": content})
            emit_pi_event(group_id, {"kind": "status", "message": "processing"})
            response = collect_pi_response(pi_proc, group_id)
            if response.strip():
                send_to_marmotd(
                    {
                        "cmd": "send_message",
                        "nostr_group_id": group_id,
                        "content": response,
                    }
                )
            emit_pi_event(group_id, {"kind": "status", "message": "done"})

    stop_pi_pty("stdin_closed")
    if pi_proc and pi_proc.poll() is None:
        pi_proc.terminate()


if __name__ == "__main__":
    main()
