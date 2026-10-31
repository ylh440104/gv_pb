import argparse
import hashlib
import hmac
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

SIGN_KEY = "alWiSzXZTLu3WfFnw13uBru3"
BASE = "https://api.nrd.nie.163.com"
PKG = "com.netease.uuremote"
CHANNEL = "nochannel"
VERSION_NAME = "4.45.0.4318"
VERSION_CODE = "44504318"


def sign_payload(method, path, params, body):
    parts = []
    for name, value in params.items():
        lowered = name.lower()
        if lowered.startswith("x-param-") and lowered != "x-param-sign" and value != "":
            parts.append("%s=%s" % (lowered, value))
    joined = "&".join(sorted(parts))
    return "%s%s%s%s" % (method.upper(), path, joined, body)


def make_headers(method, path, body, identity, token):
    params = {
        "x-param-plat": "1",
        "x-param-vn": VERSION_NAME,
        "x-param-vc": VERSION_CODE,
        "x-param-pkgn": PKG,
        "x-param-chn": CHANNEL,
        "x-param-lang": "zh_CN",
        "x-param-cnt": "CN",
        "x-param-rel": "1",
        "x-param-ts": str(int(time.time())),
    }
    for key, value in identity.items():
        if value:
            params[key] = value
    payload = sign_payload(method, path, params, body)
    digest = hmac.new(SIGN_KEY.encode(), payload.encode(), hashlib.sha256).hexdigest()

    headers = dict(params)
    headers["x-param-sign"] = digest
    headers["content-type"] = "application/json"
    if token.get("access_token"):
        headers["authorization"] = "Bearer " + token["access_token"]
        headers["cookie"] = "nrd_access_token=%s; nrd_csrf_access_token=%s" % (
            token["access_token"],
            token.get("csrf", ""),
        )
    return headers, payload, digest


def call(method, path, body, identity, token, query=""):
    url = BASE + path
    if query:
        url = url + "?" + query
    headers, payload, digest = make_headers(method, path, body, identity, token)
    data = body.encode() if body else None
    request = urllib.request.Request(url, data=data, method=method.upper())
    for key, value in headers.items():
        request.add_header(key, value)

    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status, response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def show(label, status, text):
    print("%-24s HTTP %s" % (label, status))
    trimmed = text if len(text) <= 600 else text[:600] + "..."
    print("  " + trimmed.replace("\n", "\n  "))


def identity_from(args):
    return {
        "x-param-client-id": args.client_id or "",
        "x-param-device-id": args.device_id or "",
        "x-param-system-id": args.system_id or "",
        "x-param-user-id": args.user_id or "",
    }


def cmd_sign(args):
    identity = identity_from(args)
    token = {"access_token": args.token or "", "csrf": args.csrf or ""}
    headers, payload, digest = make_headers(args.method, args.path, args.body, identity, token)
    print("payload")
    print("  " + payload)
    print("signature")
    print("  " + digest)
    print("headers")
    for key in sorted(headers):
        value = headers[key]
        if key in ("authorization", "cookie", "x-param-sign"):
            print("  %s: %s" % (key, value))
        else:
            print("  %s: %s" % (key, value))
    return 0


def cmd_check(args):
    identity = identity_from(args)
    token = {"access_token": args.token or "", "csrf": ""}
    body = json.dumps({"access_token": args.uu_token}) if args.uu_token else "{}"
    status, text = call("POST", "/api/v1/login/check_uu_token", body, identity, token)
    show("check_uu_token", status, text)
    if status == 200:
        print("\nsignature accepted")
        return 0
    if status in (401, 403, 422):
        print("\nrejected: read the body above to tell signature error from expired token")
        return 1
    print("\nunexpected status")
    return 2


def cmd_qr(args):
    identity = identity_from(args)
    status, text = call("POST", "/api/v1/qrcode/gen/login", "{}", identity, {})
    show("qrcode_gen_login", status, text)
    if status != 200:
        return 1

    try:
        envelope = json.loads(text)
    except json.JSONDecodeError:
        print("response is not json")
        return 1

    data = envelope.get("data") or envelope
    ticket = data.get("qr_id") or data.get("ticket") or data.get("id") or ""
    content = data.get("content") or data.get("url") or data.get("qrcode") or ""
    print()
    print("ticket  " + str(ticket))
    print("content " + str(content))
    if not ticket:
        print("no ticket in response, inspect the json above")
        return 1

    if not args.watch:
        return 0

    print("\npolling, scan with the UU Remote app")
    deadline = time.time() + 300
    while time.time() < deadline:
        query = "qr_id=" + urllib.parse.quote(str(ticket))
        status, text = call("GET", "/api/v1/qrcode/login/status", "", identity, {}, query)
        if status == 200:
            try:
                payload = json.loads(text)
            except json.JSONDecodeError:
                time.sleep(2)
                continue
            body = payload.get("data") or payload
            token = body.get("token") or ""
            if token:
                print("\nlogin succeeded")
                print("token   " + token)
                print("csrf    " + str(body.get("csrf", "")))
                print("user_id " + str(body.get("user_id", "")))
                return 0
        elif status == 404:
            print("\nticket expired")
            return 1
        time.sleep(2)

    print("\ntimed out")
    return 1


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["sign", "check", "qr"])
    parser.add_argument("--path", default="/api/v1/user/info")
    parser.add_argument("--method", default="GET")
    parser.add_argument("--body", default="")
    parser.add_argument("--token", default="")
    parser.add_argument("--csrf", default="")
    parser.add_argument("--uu-token", dest="uu_token", default="")
    parser.add_argument("--client-id", dest="client_id", default="")
    parser.add_argument("--device-id", dest="device_id", default="")
    parser.add_argument("--system-id", dest="system_id", default="")
    parser.add_argument("--user-id", dest="user_id", default="")
    parser.add_argument("--watch", action="store_true")
    args = parser.parse_args()

    if args.command == "sign":
        return cmd_sign(args)
    if args.command == "check":
        return cmd_check(args)
    return cmd_qr(args)


if __name__ == "__main__":
    sys.exit(main())