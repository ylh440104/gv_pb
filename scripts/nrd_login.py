import argparse
import gzip
import hashlib
import hmac
import io
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

BASE = "https://api.nrd.nie.163.com"
SIGN_KEY = "alWiSzXZTLu3WfFnw13uBru3"
STATE_DIR = os.path.join(os.path.expanduser("~"), ".nrd")
STATE_FILE = os.path.join(STATE_DIR, "device.json")
QR_FILE = os.path.join(STATE_DIR, "qr.json")

PROFILES = {
    "win": {
        "x-param-chn": "nochannel",
        "x-param-pkgn": "com.netease.uuremote",
        "x-param-plat": "1",
        "x-param-rel": "1",
        "x-param-vc": "44504318",
        "x-param-vn": "4.45.0.4318",
    },
    "android": {
        "x-param-chn": "gwqd",
        "x-param-pkgn": "com.netease.uuremote",
        "x-param-plat": "2",
        "x-param-rel": "prod",
        "x-param-vc": "445000",
        "x-param-vn": "4.45.0",
        "x-param-abi": "arm64-v8a",
    },
}

NOTES = {
    1190: "二维码已失效或尚未被扫描",
    1194: "扫码登录流程异常",
    1120: "Token 无效或已过期",
    1001: "请求参数无效",
    1003: "签名无效",
}


def signature(method, path, params, body):
    parts = []
    for name in sorted(params):
        lowered = name.lower()
        if not lowered.startswith("x-param-"):
            continue
        if lowered == "x-param-sign":
            continue
        if params[name] == "":
            continue
        parts.append("%s=%s" % (lowered, params[name]))
    payload = "%s%s%s%s" % (method.upper(), path, "&".join(parts), body)
    return payload, hmac.new(SIGN_KEY.encode(), payload.encode(), hashlib.sha256).hexdigest()


class SetupError(Exception):
    pass


class Client:
    def __init__(self, client_id, device_id, user_id="", token="", profile="win",
                 lang="zh_CN", cnt="CN", oaid="", csrf=""):
        self.client_id = client_id
        self.device_id = device_id
        self.user_id = user_id
        self.token = token
        self.profile = profile
        self.lang = lang
        self.cnt = cnt
        self.oaid = oaid
        self.csrf = csrf

    def header_map(self, method, path, body):
        params = dict(PROFILES[self.profile])
        params["x-param-client-id"] = self.client_id
        params["x-param-device-id"] = self.device_id
        params["x-param-cnt"] = self.cnt
        params["x-param-lang"] = self.lang
        params["x-param-ts"] = str(int(time.time() * 1000))
        if self.oaid:
            params["x-param-oaid"] = self.oaid
        if self.user_id:
            params["x-param-user-id"] = self.user_id
        payload, digest = signature(method, path, params, body)
        headers = dict(params)
        headers["x-param-sign"] = digest
        headers["content-type"] = "application/json"
        headers["user-agent"] = "nrd-client"
        if self.token:
            headers["authorization"] = "Bearer " + self.token
            if self.csrf:
                headers["cookie"] = "nrd_access_token=%s; nrd_csrf_access_token=%s" % (
                    self.token, self.csrf
                )
        return headers, payload, digest

    def request(self, method, path, body="", query=""):
        headers, payload, digest = self.header_map(method, path, body)
        url = BASE + path + (("?" + query) if query else "")
        request = urllib.request.Request(
            url, data=(body.encode() if body else None), method=method
        )
        for name, value in headers.items():
            request.add_header(name, value)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read()
                if raw[:2] == b"\x1f\x8b":
                    raw = gzip.decompress(raw)
                return response.status, json.loads(raw.decode("utf-8", "replace"))
        except urllib.error.HTTPError as error:
            raw = error.read()
            if raw[:2] == b"\x1f\x8b":
                raw = gzip.decompress(raw)
            try:
                return error.code, json.loads(raw.decode("utf-8", "replace"))
            except Exception:
                return error.code, {"raw": raw.decode("utf-8", "replace")[:400]}
        except Exception as error:
            return 0, {"transport": str(error)}

    def save(self):
        if not os.path.isdir(STATE_DIR):
            os.makedirs(STATE_DIR, mode=0o700)
        doc = {
            "client_id": self.client_id,
            "device_id": self.device_id,
            "user_id": self.user_id,
            "token": self.token,
            "profile": self.profile,
            "lang": self.lang,
            "cnt": self.cnt,
            "oaid": self.oaid,
            "csrf": self.csrf,
        }
        handle = io.open(STATE_FILE, "w", encoding="utf-8")
        handle.write(json.dumps(doc, ensure_ascii=False, indent=2))
        handle.close()
        os.chmod(STATE_FILE, 0o600)

    @staticmethod
    def load():
        if not os.path.exists(STATE_FILE):
            raise SetupError("尚未注册设备，先运行 register")
        doc = json.load(io.open(STATE_FILE, encoding="utf-8"))
        return Client(
            doc["client_id"], doc["device_id"], doc.get("user_id", ""),
            doc.get("token", ""), doc.get("profile", "win"),
            doc.get("lang", "zh_CN"), doc.get("cnt", "CN"), doc.get("oaid", ""),
            doc.get("csrf", ""),
        )


def shell(command, fallback=""):
    try:
        out = subprocess.run(["sh", "-c", command], capture_output=True,
                             timeout=10, text=True)
        value = out.stdout.strip()
        return value if value else fallback
    except Exception:
        return fallback


def hardware():
    return {
        "board": shell("cat /sys/devices/virtual/dmi/id/board_name 2>/dev/null", "unknown") or "unknown",
        "cpu": "%s (%s cores)" % (
            shell("grep -m1 'model name' /proc/cpuinfo | cut -d: -f2", "unknown").strip() or "unknown",
            shell("nproc", "1"),
        ),
        "memory": shell("awk '/MemTotal/{printf \"%.0f\", $2/1024}' /proc/meminfo", "0") or "0",
        "mac": shell("cat /sys/class/net/*/address 2>/dev/null | head -1", "00:00:00:00:00:00") or "00:00:00:00:00:00",
        "video": shell("lspci 2>/dev/null | grep -im1 vga | cut -d: -f3", "").strip() or "unknown",
    }


def write_private(path, text):
    if not os.path.isdir(STATE_DIR):
        os.makedirs(STATE_DIR, mode=0o700)
    handle = io.open(path, "w", encoding="utf-8")
    handle.write(text)
    handle.close()
    os.chmod(path, 0o600)


def cmd_register(args):
    client_id = args.client_id or uuid.uuid4().hex[:16]
    system_id = args.system_id or uuid.uuid4().hex[:16]
    info = hardware()
    body = {
        "name": args.name or shell("hostname", "nrd-host"),
        "client_id": client_id,
        "system_id": system_id,
        "machine_guid": str(uuid.uuid4()).upper(),
        "os": args.os or "Windows 10 Pro 10.0.19045",
        "base_board": info["board"],
        "cpu": info["cpu"],
        "video": [info["video"]],
        "mac": info["mac"],
        "memory": info["memory"],
        "screen": args.screen or "1920x1080",
    }
    client = Client(client_id, system_id, profile=args.profile,
                    lang=args.lang, cnt=args.cnt)
    status, doc = client.request("POST", "/api/v1/device/windows/init", json.dumps(body))
    print("POST /api/v1/device/windows/init -> %s" % status)
    if status != 200:
        print(json.dumps(doc, ensure_ascii=False, indent=2))
        return 1
    client.device_id = doc["data"]["device_id"]
    client.save()
    print("device_id  %s" % client.device_id)
    print("client_id  %s" % client_id)
    print("system_id  %s" % system_id)
    print("alias      %s" % doc["data"].get("alias"))
    print("状态写入   %s" % STATE_FILE)
    return 0


def cmd_qr(args):
    client = Client.load()
    status, doc = client.request("POST", "/api/v1/qrcode/gen/login", "{}")
    print("POST /api/v1/qrcode/gen/login -> %s" % status)
    if status != 200:
        print(json.dumps(doc, ensure_ascii=False, indent=2))
        return 1
    data = doc["data"]
    write_private(QR_FILE, json.dumps({
        "qrcode_jump_url": data["qrcode_jump_url"],
        "token": data["token"],
        "status_query_ticket": data.get("status_query_ticket", ""),
        "created_at": int(time.time()),
    }, ensure_ascii=False, indent=2))
    parsed = urllib.parse.parse_qs(urllib.parse.urlparse(data["qrcode_jump_url"]).query)
    print()
    print("qrcode_id  %s" % parsed.get("qrcode_id", [""])[0])
    print("跳转地址")
    print("  %s" % data["qrcode_jump_url"])
    print()
    if args.image:
        try:
            import qrcode
            qrcode.make(data["qrcode_jump_url"]).save(args.image)
            print("二维码图片 %s" % args.image)
        except ImportError:
            print("未安装 qrcode 模块，用上面的链接自己生成二维码")
    print("手机 UU远程 App 扫码确认后运行 verify")
    return 0


def cmd_poll(args):
    client = Client.load()
    if not os.path.exists(QR_FILE):
        raise SetupError("没有待登录的二维码，先运行 qr")
    record = json.load(io.open(QR_FILE, encoding="utf-8"))
    deadline = time.time() + args.seconds
    attempt = 0
    while time.time() < deadline:
        attempt += 1
        body = json.dumps({
            "qrcode_jump_url": record["qrcode_jump_url"],
            "login_status": 1,
        })
        status, doc = client.request("POST", "/api/v1/qrcode/login/status", body)
        code = doc.get("code")
        print("  #%02d %s code=%s %s %s" % (
            attempt, status, code, doc.get("msg", ""), NOTES.get(code, "")
        ))
        if status == 200:
            print(json.dumps(doc, ensure_ascii=False, indent=2))
            return 0
        time.sleep(args.interval)
    return 1


def cmd_verify(args):
    client = Client.load()
    if not os.path.exists(QR_FILE):
        raise SetupError("没有待登录的二维码，先运行 qr")
    record = json.load(io.open(QR_FILE, encoding="utf-8"))
    body = json.dumps({
        "qrcode_jump_url": record["qrcode_jump_url"],
        "token": record["token"],
    })
    status, doc = client.request("POST", "/api/v1/login/by_qrcode", body)
    print("POST /api/v1/login/by_qrcode -> %s" % status)
    print(json.dumps(doc, ensure_ascii=False, indent=2)[:2000])
    if status != 200:
        code = doc.get("code")
        if code in NOTES:
            print()
            print("说明: %s" % NOTES[code])
        return 1
    data = doc.get("data", {})
    token = data.get("token") or data.get("access_token") or ""
    csrf = data.get("csrf") or ""
    if token:
        client.token = token
        if csrf:
            client.csrf = csrf
        if data.get("user_id"):
            client.user_id = data["user_id"]
        client.save()
        print()
        print("token 长度 %d 已写入 %s" % (len(token), STATE_FILE))
    return 0


def cmd_whoami(args):
    client = Client.load()
    status, doc = client.request("GET", "/api/v1/user/info")
    print("GET /api/v1/user/info -> %s" % status)
    print(json.dumps(doc, ensure_ascii=False, indent=2)[:1500])
    return 0 if status == 200 else 1


def cmd_devices(args):
    client = Client.load()
    status, doc = client.request("GET", "/api/v1/device/list")
    print("GET /api/v1/device/list -> %s" % status)
    print(json.dumps(doc, ensure_ascii=False, indent=2)[:3000])
    return 0 if status == 200 else 1


def cmd_room(args):
    client = Client.load()
    status, doc = client.request("POST", "/api/v1/room/create", "{}")
    print("POST /api/v1/room/create -> %s" % status)
    if status != 200:
        print(json.dumps(doc, ensure_ascii=False, indent=2))
        return 1
    data = doc["data"]
    print("signaling_server %s" % data.get("signaling_server"))
    for item in data.get("signaling_list", []):
        print("  %s" % item)
    print("token 长度 %d" % len(data.get("token", "")))
    if args.save:
        write_private(args.save, json.dumps(data, ensure_ascii=False, indent=2))
        print("写入 %s" % args.save)
    return 0


def cmd_sign(args):
    client = Client.load()
    headers, payload, digest = client.header_map(args.method, args.path, args.body or "")
    print("payload")
    print("  %s" % payload)
    print("signature")
    print("  %s" % digest)
    print("headers")
    for name in sorted(headers):
        print("  %s: %s" % (name, headers[name]))
    return 0


def build_parser():
    parser = argparse.ArgumentParser(prog="nrd_login")
    subs = parser.add_subparsers(dest="command")
    names = ["register", "qr", "poll", "verify", "whoami", "devices", "room", "sign"]
    for name in names:
        subs.add_parser(name)
    parser.set_defaults(profile="win", lang="zh_CN", cnt="CN")

    register_parser = subs.choices["register"]
    register_parser.add_argument("--name")
    register_parser.add_argument("--os")
    register_parser.add_argument("--screen")
    register_parser.add_argument("--client-id")
    register_parser.add_argument("--system-id")
    register_parser.add_argument("--profile", default="win", choices=["win", "android"])
    register_parser.add_argument("--lang", default="zh_CN")
    register_parser.add_argument("--cnt", default="CN")

    subs.choices["qr"].add_argument("--image")
    subs.choices["poll"].add_argument("--seconds", type=int, default=120)
    subs.choices["poll"].add_argument("--interval", type=float, default=2.0)
    subs.choices["room"].add_argument("--save")
    subs.choices["sign"].add_argument("--method", default="GET")
    subs.choices["sign"].add_argument("--path", required=True)
    subs.choices["sign"].add_argument("--body", default="")
    return parser


HANDLERS = {
    "register": cmd_register,
    "qr": cmd_qr,
    "poll": cmd_poll,
    "verify": cmd_verify,
    "whoami": cmd_whoami,
    "devices": cmd_devices,
    "room": cmd_room,
    "sign": cmd_sign,
}


def main():
    parser = build_parser()
    args = parser.parse_args()
    if not args.command:
        parser.print_help()
        return 2
    try:
        return HANDLERS[args.command](args)
    except SetupError as error:
        print("错误: %s" % error)
        return 2
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())