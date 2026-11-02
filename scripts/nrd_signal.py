import base64
import io
import json
import os
import socket
import ssl
import struct
import sys
import time
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from nrd_login import Client, SetupError


class SignalingError(Exception):
    pass


class Socket:
    def __init__(self, host, path, headers, timeout=20, port=443):
        context = ssl.create_default_context()
        self.raw = socket.create_connection((host, port), timeout=timeout)
        self.raw = context.wrap_socket(self.raw, server_hostname=host)
        lines = [
            "GET %s HTTP/1.1" % path,
            "Host: %s" % host,
            "Upgrade: websocket",
            "Connection: Upgrade",
            "Sec-WebSocket-Key: %s" % base64.b64encode(os.urandom(16)).decode(),
            "Sec-WebSocket-Version: 13",
            "User-Agent: nrd-client",
        ]
        for name, value in headers.items():
            lines.append("%s: %s" % (name, value))
        self.raw.sendall(("\r\n".join(lines) + "\r\n\r\n").encode())
        buffer = b""
        while b"\r\n\r\n" not in buffer:
            chunk = self.raw.recv(4096)
            if not chunk:
                break
            buffer += chunk
        head, _, self.pending = buffer.partition(b"\r\n\r\n")
        self.head = head.decode("latin-1")
        self.status = self.head.split("\r\n")[0] if self.head else ""

    def ok(self):
        return "101" in self.status

    def send(self, payload, opcode=0x1):
        data = payload.encode() if isinstance(payload, str) else payload
        mask = os.urandom(4)
        frame = bytearray([0x80 | opcode])
        length = len(data)
        if length < 126:
            frame.append(0x80 | length)
        elif length < 65536:
            frame.append(0x80 | 126)
            frame += struct.pack(">H", length)
        else:
            frame.append(0x80 | 127)
            frame += struct.pack(">Q", length)
        frame += mask
        frame += bytes(byte ^ mask[index % 4] for index, byte in enumerate(data))
        self.raw.sendall(bytes(frame))

    def recv(self, timeout=15):
        self.raw.settimeout(timeout)
        try:
            head = self.raw.recv(2)
        except socket.timeout:
            return "timeout", None
        if len(head) < 2:
            return "closed", None
        opcode = head[0] & 0x0F
        length = head[1] & 0x7F
        if length == 126:
            length = struct.unpack(">H", self.raw.recv(2))[0]
        elif length == 127:
            length = struct.unpack(">Q", self.raw.recv(8))[0]
        data = b""
        while len(data) < length:
            chunk = self.raw.recv(length - len(data))
            if not chunk:
                break
            data += chunk
        return opcode, data

    def close(self):
        try:
            self.raw.close()
        except Exception:
            pass


def room_token(client):
    status, doc = client.request("POST", "/api/v1/room/create", "{}")
    if status != 200:
        raise SignalingError("room/create 失败 %s %s" % (status, json.dumps(doc, ensure_ascii=False)[:200]))
    return doc["data"]


def connect(room, index=0, timeout=20):
    servers = room.get("signaling_list") or [room.get("signaling_server", "")]
    server = servers[min(index, len(servers) - 1)]
    parsed = urllib.parse.urlparse(server)
    host = parsed.hostname
    path = "/socket.io/?transport=websocket&EIO=4"
    headers = {"X-NRD-AUTH": room["token"]}
    sock = Socket(host, path, headers, timeout=timeout)
    if not sock.ok():
        raise SignalingError("握手失败 %s" % sock.status)
    return sock


def open_namespace(sock, timeout=10):
    opcode, data = sock.recv(timeout)
    if opcode == "timeout":
        raise SignalingError("等待 open 包超时")
    text = data.decode("utf-8", "replace")
    if not text.startswith("0"):
        raise SignalingError("非预期首包 %r" % text[:80])
    info = json.loads(text[1:])
    sock.send("40", 0x1)
    opcode, data = sock.recv(timeout)
    if opcode == "timeout":
        raise SignalingError("等待命名空间确认超时")
    reply = data.decode("utf-8", "replace")
    if reply.startswith("44"):
        raise SignalingError("命名空间被拒 %r" % reply[:120])
    if not reply.startswith("40"):
        raise SignalingError("非预期命名空间回复 %r" % reply[:80])
    session = json.loads(reply[2:]) if len(reply) > 2 else {}
    return info, session


def parse_event(text):
    if not text.startswith("42"):
        return None, None
    body = text[2:]
    if body.startswith("/"):
        slash = body.find(",")
        body = body[slash + 1:]
    try:
        payload = json.loads(body)
    except Exception:
        return None, None
    if isinstance(payload, list) and payload:
        name = payload[0]
        args = payload[1] if len(payload) > 1 else None
        return name, args
    return None, None


def room_info(sock, timeout=10):
    sock.send('42["room_info",{}]', 0x1)
    opcode, data = sock.recv(timeout)
    if opcode == "timeout":
        return None, None
    text = data.decode("utf-8", "replace")
    return parse_event(text)


def drain(sock, seconds, on_event=None):
    deadline = time.time() + seconds
    events = []
    while time.time() < deadline:
        opcode, data = sock.recv(max(0.3, min(5.0, deadline - time.time())))
        if opcode == "timeout":
            continue
        if opcode == "closed":
            break
        if opcode == 0x9:
            sock.send(data, 0xA)
            continue
        text = data.decode("utf-8", "replace")
        name, args = parse_event(text)
        if name:
            events.append((name, args))
            if on_event:
                on_event(name, args)
        elif text == "2":
            sock.send("3", 0x1)
    return events


def main():
    try:
        client = Client.load()
    except SetupError as error:
        print("错误: %s" % error)
        return 2

    if not client.token:
        print("错误: 尚未登录，先运行 nrd_login.py register 再 qr / verify")
        return 2

    room = room_token(client)
    print("signaling_server %s" % room.get("signaling_server"))
    print("token 长度 %d" % len(room.get("token", "")))
    print()

    sock = connect(room)
    print(sock.status)
    info, session = open_namespace(sock)
    print("engine.io  ping=%s timeout=%s maxPayload=%s" % (
        info.get("pingInterval"), info.get("pingTimeout"), info.get("maxPayload")
    ))
    print("namespace  %s" % json.dumps(session, ensure_ascii=False))
    print()

    name, args = room_info(sock)
    print("room_info -> %s %s" % (name, json.dumps(args, ensure_ascii=False)[:600]))
    print()

    print("监听 30 秒")
    def printer(event, payload):
        text = json.dumps(payload, ensure_ascii=False)
        print("  %-16s %s" % (event, text[:300]))

    drain(sock, 30, printer)
    sock.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())