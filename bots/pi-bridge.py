#!/usr/bin/env python3
"""Bridge between pikachat daemon JSONL (stdin/stdout) and pi coding agent RPC mode."""
import json
import os
import subprocess
import sys

my_pubkey = None
pi_proc = None


def log(msg):
    print(f"[pi-bridge] {msg}", file=sys.stderr, flush=True)


def start_pi():
    env = os.environ.copy()
    cmd = ["pi", "--mode", "rpc", "--no-session", "--provider", "anthropic"]
    model = os.environ.get("PI_MODEL")
    if model:
        cmd.extend(["--model", model])
    log(f"starting pi: {' '.join(cmd)}")
    return subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=sys.stderr,
        env=env,
        bufsize=0,
    )


def send_to_pi(pi_proc, msg):
    line = json.dumps(msg) + "\n"
    pi_proc.stdin.write(line.encode())
    pi_proc.stdin.flush()


def collect_pi_response(pi_proc):
    text_parts = []
    for raw in pi_proc.stdout:
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
            if delta_event.get("type") == "text_delta":
                text_parts.append(delta_event["delta"])
        elif etype == "agent_end":
            break
        elif etype == "response" and not event.get("success"):
            break

    return "".join(text_parts)


def send_to_pikachat(cmd):
    print(json.dumps(cmd), flush=True)


def main():
    global my_pubkey, pi_proc

    pi_proc = start_pi()

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
            log(f"pikachat ready, pubkey={my_pubkey}")
            send_to_pikachat({"cmd": "publish_keypackage"})

        elif msg_type == "message_received":
            if msg.get("from_pubkey") == my_pubkey:
                continue
            content = msg.get("content", "")
            group_id = msg.get("nostr_group_id", "")
            send_to_pi(pi_proc, {"type": "prompt", "message": content})
            response = collect_pi_response(pi_proc)
            if response.strip():
                send_to_pikachat({
                    "cmd": "send_message",
                    "nostr_group_id": group_id,
                    "content": response,
                })

    if pi_proc and pi_proc.poll() is None:
        pi_proc.terminate()


if __name__ == "__main__":
    main()
