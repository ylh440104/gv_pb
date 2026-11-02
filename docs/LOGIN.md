# 登录

本文记录的全部结论都来自对 `api.nrd.nie.163.com` 的真实请求，不是推测。仓库里不含任何真实凭据，脚本运行时自己注册设备、自己走扫码登录、自己把令牌写到本地。

## 一、两层鉴权

NRD 的每个请求要同时过两关，缺一个都不行。

**第一关是请求签名。** 签名对方法、路径、全部 `x-param-*` 头、请求体做 HMAC-SHA256，作为 `x-param-sign` 发出。

**第二关是用户令牌。** 登录后拿到 JWT，放在 `authorization: Bearer <jwt>` 里。

两关的报错码不同，这一点非常有用：

```
code 1003  签名无效        → 签名这一关没过
code 1001  请求参数无效    → 签名过了，缺头或缺 body 字段
code 1120  Token 无效或过期 → 签名和头都过了，令牌不对
```

所以只要看到 `1001`，就说明签名算法是对的，剩下的只是补齐字段。

## 二、签名算法

```
payload = METHOD + PATH + "&".join(sorted(k=v)) + BODY
sign    = hex(HMAC_SHA256(KEY, payload))
```

细节，每一条都在真实请求上验过：

- `METHOD` 必须大写。
- `PATH` 只到路径，不含 query，不含域名。
- 参与排序的只有以 `x-param-` 开头的头，全部转小写。
- `x-param-sign` 自己排除在外。
- 值为空字符串的项直接跳过，不是发空值。
- 排序按参数名字典序，`k=v` 用 `&` 连接。
- `BODY` 原样拼接，`{}` 就拼 `{}`，没有 body 就什么都不拼。

密钥：

```
alWiSzXZTLu3WfFnw13uBru3
```

## 三、请求头全集

PC 端（`x-param-plat: 1`）：

```
x-param-chn   nochannel
x-param-pkgn  com.netease.uuremote
x-param-plat  1
x-param-rel   1
x-param-vc    44504318
x-param-vn    4.45.0.4318
x-param-cnt   CN
x-param-lang  zh_CN
```

Android 端（`x-param-plat: 2`）：

```
x-param-chn     gwqd
x-param-pkgn    com.netease.uuremote
x-param-plat    2
x-param-rel     prod
x-param-vc      445000
x-param-vn      4.45.0
x-param-abi     arm64-v8a
x-param-cnt     CN
x-param-lang    zh_CN
x-param-oaid    <可选>
```

身份三项，按需出现：

```
x-param-client-id   客户端实例标识
x-param-device-id   设备标识
x-param-user-id     登录后才有
```

服务端会按端点要求校验它们，缺了就返回 `1001` 并把缺的字段名列在 `data` 数组里。这个字段级报错是最可靠的 schema 来源。

## 四、第一步：注册设备

新设备上什么都没有，必须先把 `client_id` 和 `device_id` 变成服务端认识的设备对。

```
POST /api/v1/device/windows/init
```

body：

```json
{
  "name": "主机名",
  "client_id": "16位hex",
  "system_id": "16位hex",
  "machine_guid": "UUID大写",
  "os": "Windows 10 Pro 10.0.19045",
  "base_board": "主板名",
  "cpu": "CPU 型号",
  "video": ["显卡型号"],
  "mac": "aa:bb:cc:dd:ee:ff",
  "memory": "16384",
  "screen": "1920x1080"
}
```

两个坑：

- `video` 必须是**数组**，传字符串会返回 `list_type` 错误。
- `system_id` 要和头里的 `x-param-device-id` 一致。

成功返回：

```json
{"code":0,"data":{"device_id":"...","platform":1,"alias":"...","status":"DISCONNECTED","controllable":true}}
```

`data.device_id` 就是服务端分配的正式设备 ID，**之后所有请求的 `x-param-device-id` 都要换成它**。

这一步不需要登录，也不需要令牌。

## 五、第二步：取二维码

```
POST /api/v1/qrcode/gen/login
body: {}
```

返回：

```json
{
  "qrcode_jump_url": "https://uuyc.webapp.163.com/open-app?qrcode_id=...&uu_remote_qr_code=1&lang=zh_CN",
  "token": "...",
  "status_query_ticket": "..."
}
```

`qrcode_jump_url` 就是二维码内容，把它渲染成图片给人扫。`uu_remote_qr_code=1` 是给 App 的识别标记。

注意这个接口要求 `client_id` 和 `device_id` 是一组真实存在的设备对。随便编两个 hex 值会拿到 `1001`，必须先做第四步。

## 六、第三步：轮询状态

```
POST /api/v1/qrcode/login/status
```

body 必须两个字段：

```json
{
  "qrcode_jump_url": "完整跳转地址",
  "login_status": 1
}
```

`login_status` 是枚举，只接受 `1 2 3 4`，传 `0` 会返回枚举错误。它表示客户端要登记的状态值，不是"查询哪一项"。

未扫码时会返回：

```
code 1190  二维码已失效
```

这是正常现象。码在没人扫的时候就是这个状态，`gen/login` 出来的码只有被真实设备扫码之后才会往下一步走。

## 七、第四步：换取令牌

```
POST /api/v1/login/by_qrcode
```

body：

```json
{
  "qrcode_jump_url": "完整跳转地址",
  "token": "gen/login 返回的 token"
}
```

成功时返回用户信息和 JWT。失败常见两种：

```
1120  Token 无效或已过期
1194  扫码登录流程异常，请重试
```

`1194` 基本等于"码没走完扫码确认流程"。

## 八、令牌

JWT，三段，头是：

```json
{"alg":"HS256","kid":"v2","typ":"JWT"}
```

载荷里这些字段有用：

```
sub       用户 ID
device_id 绑定设备
client_id 绑定客户端
csrf      CSRF 令牌
iat/nbf   签发与生效时间
jti       唯一 ID
key_ver   密钥版本 v2
type      access
nonce     一次性随机串
```

`sub` 就是 `x-param-user-id` 要填的值。`csrf` 用来拼 cookie：

```
cookie: nrd_access_token=<jwt>; nrd_csrf_access_token=<csrf>
```

令牌会随登录轮换。同一个账号在别处重新登录，旧令牌立刻失效返回 `1120`。

## 九、其余登录方式

同一套鉴权下的另外三条路径：

```
POST /api/v1/login/by_uu_token
POST /api/v1/login/by_mobile
POST /api/v1/login/check_uu_token
POST /api/v1/login/check_mumu_token
POST /api/v1/login/by_mumu_token
```

`check_*` 用来验证已有令牌是否有效，`by_*` 用来换新令牌。`check_uu_token` 的 body 字段名是 `access_token`，不是 `token`，传错名字会得到 `1120`。

## 十、脚本

`scripts/nrd_login.py` 实现上面全部流程，只用标准库。

```bash
python3 scripts/nrd_login.py register --name my-host
python3 scripts/nrd_login.py qr --image qr.png
python3 scripts/nrd_login.py verify
python3 scripts/nrd_login.py whoami
python3 scripts/nrd_login.py devices
python3 scripts/nrd_login.py room
python3 scripts/nrd_login.py sign --method GET --path /api/v1/user/info
```

状态文件：

```
~/.nrd/device.json   client_id device_id user_id token csrf，权限 600
~/.nrd/qr.json       当前待扫的码，权限 600
```

脚本里没有任何内置令牌。`register` 现场生成设备标识，`verify` 拿到令牌后写盘，之后所有命令都从盘上读。

`sign` 子命令可以给任意路径算签名并打印完整请求头，用来对照调试。

## 十一、信令

拿到令牌后建房：

```
POST /api/v1/room/create
body: {}
```

返回：

```json
{
  "signaling_server": "wss://sig-3303-a.nrd.nie.163.com/",
  "signaling_list": ["wss://sig-3303-a.nrd.nie.163.com/", "wss://42.186.225.118/", "wss://sig-dcdn-3303-a.nrd.nie.163.com/"],
  "sni_required_signaling_list": ["wss://sig-dcdn-3303-a.nrd.nie.163.com/"],
  "token": "<1472 字符 base64，解码 1104 字节密文>",
  "report_token": "<同长度>",
  "report_url": "https://relay-mg-3303-a.nrd.nie.163.com/",
  "report_server_address": "115.236.122.142",
  "ws_connect_timeout_ms": 16000,
  "max_reconnect_delta": 300,
  "streamer_retry_delta_ms": 1000,
  "international_connect": false
}
```

那个 `token` 是不透明的，不需要解密，原样放进 `X-NRD-AUTH` 头即可。

连接：

```
wss://sig-3303-a.nrd.nie.163.com/socket.io/?transport=websocket&EIO=4
header: X-NRD-AUTH: <room token>
```

**关键坑：不要发 `Origin` 头。** 带了 `Origin` 会被 nginx 直接 `400 Bad Request`，返回 `{"code":3,"message":"Bad request"}`。去掉就 `101 Switching Protocols`。

连上后的时序：

```
<- 0{"sid":"...","pingInterval":15000,"pingTimeout":18000,"maxPayload":1000000}
-> 40
<- 40{"sid":"<服务端会话 id>"}
-> 42["room_info",{}]
<- 42["bmsg_push",{"type":"device_info_changed","data":{...}}]
<- 2
-> 3
```

`room_info` 之后服务端会推 `bmsg_push`，里面是当前设备状态。心跳由服务端发 `2`，客户端回 `3`。`pingInterval` 15 秒，`pingTimeout` 18 秒。

`scripts/nrd_signal.py` 实现了握手、`40` 命名空间、`room_info`、事件分派和心跳应答。

## 十二、还没打通的

二维码的 `login_status` 四个枚举值的确切语义，需要真机扫码一次才能确定——必须有一个扫描动作把码从"未扫"推进到"已扫"，之后 `login/status` 才会返回非 `1190`。这一步没有真实扫码动作无法完成，`by_qrcode` 也就无法用未扫的码换到令牌。

拿到一次完整扫码的抓包，就能把 `login_status` 的四个值和时序固定下来。