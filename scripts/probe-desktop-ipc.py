#!/usr/bin/env python3
"""Read-only local IPC discovery; exit 2 unless a non-TUI loaded list is found."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import stat
import struct
import subprocess
import sys
import time


LIMIT = 4 * 1024 * 1024
TIMEOUT = 3.0


def emit(label, value):
    print(f"{label}={json.dumps(value, ensure_ascii=False)}", flush=True)


def discover_processes():
    processes = {}
    output = subprocess.check_output(
        ["ps", "-axo", "pid=,ppid=,args="], text=True, timeout=TIMEOUT
    )
    for line in output.splitlines():
        parts = line.strip().split(None, 2)
        if len(parts) == 3:
            processes[int(parts[0])] = (int(parts[1]), parts[2])
    embedded = []
    for pid, (ppid, argv) in processes.items():
        if not re.search(r"(?:^|\s)app-server(?:\s|$)", argv):
            continue
        parent = processes.get(ppid, (0, ""))[1]
        desktop = bool(re.search(r"(?:^|/)ChatGPT(?:\s|$)", parent))
        # Parent identity takes precedence over flags shared with TUI servers.
        classification = "desktop-embedded" if desktop else "other-app-server"
        if not desktop and "--listen" in argv:
            classification = "listen-app-server (UA required to identify TUI)"
        emit("APP_SERVER", {
            "pid": pid, "ppid": ppid, "classification": classification,
            "code_mode_host": "features.code_mode_host=true" in argv,
            "has_listen_flag": "--listen" in argv,
        })
        if desktop:
            embedded.append(pid)
    emit("DESKTOP_EMBEDDED_PIDS", embedded)
    emit("DESKTOP_TRANSPORT_KNOWN", "com.openai.codex / ChatGPT: parent stdio/unix-socketpair; no named listener")


def discover_path(label, path):
    target = path.resolve()
    emit(label, {"path": str(path), "symlink": path.is_symlink(), "target": str(target)})
    if not stat.S_ISSOCK(target.stat().st_mode):
        raise ValueError(f"{label}: target is not a Unix socket")
    return target


class WebSocket:
    """Bounded stdlib WebSocket client for local JSON-RPC only."""

    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.buffer = bytearray()
        self.deadline = time.monotonic() + TIMEOUT
        self.stage = "connect"
        try:
            self.sock.settimeout(TIMEOUT)
            self.sock.connect(str(path))
            self.stage = "upgrade"
            key = base64.b64encode(os.urandom(16)).decode()
            self.sock.sendall((
                "GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n"
                "Connection: Upgrade\r\nSec-WebSocket-Version: 13\r\n"
                f"Sec-WebSocket-Key: {key}\r\n\r\n"
            ).encode())
            while b"\r\n\r\n" not in self.buffer:
                if len(self.buffer) > 16384:
                    raise ValueError("no HTTP upgrade within 16 KiB")
                self.receive()
            header, remainder = self.buffer.split(b"\r\n\r\n", 1)
            self.buffer = bytearray(remainder)
            lines = header.decode("latin1").split("\r\n")
            if not re.match(r"HTTP/1\.[01] 101(?: |$)", lines[0]):
                raise ValueError(f"non-101 HTTP response: {lines[0][:120]}")
            headers = {name.lower(): value.strip() for name, value in
                       (line.split(":", 1) for line in lines[1:] if ":" in line)}
            accept = base64.b64encode(hashlib.sha1(
                (key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()
            ).digest()).decode()
            if headers.get("sec-websocket-accept", "").strip() != accept:
                raise ValueError("invalid Sec-WebSocket-Accept")
            if headers.get("upgrade", "").lower() != "websocket" or "upgrade" not in headers.get("connection", "").lower():
                raise ValueError("invalid upgrade headers")
        except Exception as exc:
            self.sock.close()
            raise ValueError(f"{self.stage}: {type(exc).__name__}: {exc}") from exc

    def receive(self):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("deadline exceeded")
        self.sock.settimeout(remaining)
        data = self.sock.recv(65536)
        if not data:
            raise EOFError("peer closed connection")
        self.buffer.extend(data)

    def read(self, size):
        while len(self.buffer) < size:
            self.receive()
        data = bytes(self.buffer[:size])
        del self.buffer[:size]
        return data

    def send(self, payload, opcode=1):
        mask = os.urandom(4)
        size = len(payload)
        header = bytes([0x80 | opcode])
        if size < 126:
            header += bytes([0x80 | size])
        elif size < 65536:
            header += bytes([0xFE]) + struct.pack("!H", size)
        else:
            header += bytes([0xFF]) + struct.pack("!Q", size)
        self.sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def send_json(self, value):
        self.send(json.dumps(value).encode())

    def message(self):
        message = bytearray()
        started = False
        while True:
            first, second = self.read(2)
            opcode = first & 15
            size = second & 127
            if first & 0x70 or second & 0x80:
                raise ValueError("unsupported RSV or masked server frame")
            if size == 126:
                size = struct.unpack("!H", self.read(2))[0]
            elif size == 127:
                size = struct.unpack("!Q", self.read(8))[0]
            if size + len(message) > LIMIT:
                raise ValueError("WebSocket message exceeds 4 MiB")
            payload = self.read(size)
            if opcode == 8:
                raise EOFError("WebSocket close frame")
            if opcode in (9, 10):
                if size > 125 or not first & 0x80:
                    raise ValueError("invalid control frame")
                if opcode == 9:
                    self.send(payload, 10)
                continue
            if opcode != (0 if started else 1):
                raise ValueError("expected text/continuation frame")
            started = True
            message.extend(payload)
            if first & 0x80:
                return json.loads(message)

    def request(self, request_id, method, params):
        self.deadline = time.monotonic() + TIMEOUT
        self.sock.settimeout(TIMEOUT)
        self.send_json({"id": request_id, "method": method, "params": params})
        while True:
            response = self.message()
            if isinstance(response, dict) and response.get("id") == request_id:
                if "error" in response:
                    raise ValueError(f"{method}: {json.dumps(response['error'])}")
                return response["result"]


def loaded_success(user_agent, loaded):
    return (isinstance(user_agent, str) and bool(user_agent.strip())
            and "codex-tui" not in user_agent.lower()
            and isinstance(loaded, dict) and isinstance(loaded.get("data"), list)
            and bool(loaded["data"]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex-home", type=Path, default=Path.home() / ".codex")
    args = parser.parse_args()
    paths_ok = True
    user_agent = None
    loaded = None
    try:
        discover_processes()
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"PROCESS_DISCOVERY_ERROR={exc}", file=sys.stderr)
        paths_ok = False
    for label, path in (
        ("CONTROL_SOCKET", args.codex_home / "app-server-control/app-server-control.sock"),
        ("IPC_SOCKET", args.codex_home / "ipc/ipc.sock"),
    ):
        try:
            target = discover_path(label, path)
        except (OSError, ValueError, RuntimeError) as exc:
            print(f"PATH_ERROR={label}: {exc}", file=sys.stderr)
            paths_ok = False
            continue
        ws = None
        try:
            ws = WebSocket(target)
            emit(f"{label}_WS", "101 upgrade accepted")
            if label == "CONTROL_SOCKET":
                initialized = ws.request(1, "initialize", {
                    "clientInfo": {"name": "desktop-ipc-probe", "version": "1.0"},
                    "capabilities": {"experimentalApi": True},
                })
                user_agent = initialized.get("userAgent")
                emit("CONTROL_USER_AGENT", user_agent)
                emit("CONTROL_CLASSIFICATION", "tui-daemon" if isinstance(user_agent, str) and "codex-tui" in user_agent.lower() else "non-tui-or-unknown (not proof of Desktop ownership)")
                ws.send_json({"method": "initialized"})
                loaded = ws.request(2, "thread/loaded/list", {})
                emit("CONTROL_THREAD_LOADED_LIST", loaded)
        except (OSError, ValueError, EOFError, KeyError, AttributeError) as exc:
            emit(f"{label}_WS_FAILURE", f"{type(exc).__name__}: {exc}")
        finally:
            if ws is not None:
                ws.sock.close()
    success = paths_ok and loaded_success(user_agent, loaded)
    print("DESKTOP_LOADED_LIST=" + ("nonempty-non-tui" if success else "empty-or-unreachable"))
    return 0 if success else 2


if __name__ == "__main__":
    sys.exit(main())
