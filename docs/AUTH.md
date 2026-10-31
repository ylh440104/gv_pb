# 登录 / 鉴权：完整可复现

从 `GameViewerServer.exe` 4.45.0.4318 静态还原，并用
[VallierDev/uu-remote-linux](https://github.com/VallierDev/uu-remote-linux)
的实现做交叉验证（两边独立得到同一个签名密钥）。

---

## 1. 签名密钥（已确认）

`service_util.cpp` 字符串表里两个相邻的 24 字符常量：

```
alWiSzXZTLu3WfFnw13uBru3    ← request_sign  (X-Param-SIGN)
oK5zJ1zJ5cF6kK9eW2jM8oF3    ← plugin_request_sign  (X-Param-Plugin-Ident)
```

第一个与 `uu-remote-linux` 的 `DEFAULT_SIGN_KEY` **逐字节相同**：

```rust
// src/auth.rs:14
pub const DEFAULT_SIGN_KEY: &str = "alWiSzXZTLu3WfFnw13uBru3";
```

两个独立来源、同一个人力不可猜的字符串 —— 这条确定。

它写在 `D:\nrd-windows-client-gitlab\work\1131\391013\source\src\shared\business\business_api\service_util.cpp`
里，函数名 `request_sign`，失败日志 `request_sign result=failed reason=hmac_error`。

## 2. 签名算法（已确认）

```
string_to_sign = METHOD  +  path  +  params_str  +  body_str
signature      = hex( HMAC-SHA256(key = sign_key, message = string_to_sign) )
header         = X-Param-SIGN: <signature>
```

其中 `params_str`：

- 只取名字以 `x-param-` 开头的头（小写比较）
- 排除 `x-param-sign` 自身
- 每个拼成 `key=value`
- 用 `&` 连接
- **按 key 小写字典序排序**（`BTreeMap`）

`path` 只到路径，**不含 query string**（query 已经在 `x-param-*` 头里了）。

参考实现：

```rust
let string_to_sign = format!("{}{}{}{}", method.to_uppercase(), path, params_str, body_str);
let mut mac = HmacSha256::new_from_slice(sign_key.as_bytes())?;
mac.update(string_to_sign.as_bytes());
let sign = hex::encode(mac.finalize().into_bytes());
```

## 3. 请求头全集（已确认）

从 `business_json_utils.cpp` 的字符串表按顺序取出：

| 头 | 值 | 说明 |
|---|---|---|
| `X-Param-PLAT` | `1` | 平台：PC 客户端 |
| `X-Param-VN` | `4.45.0.4318` | 版本号 |
| `X-Param-VC` | `44504318` | 版本 code |
| `X-Param-PKGN` | `com.netease.uuremote` | 包名 |
| `X-Param-CHN` | `nochannel` | 渠道，官方网关包 |
| `X-Param-LANG` | `zh_CN` / `en_US` | 语言 |
| `X-Param-CNT` | `CN` | 国家 |
| `X-Param-REL` | `1` | 正式版标记 |
| `X-Param-OPR` | — | 操作者/来源 |
| `X-Param-TS` | unix 秒 | 时间戳 |
| `X-Param-Nonce` | 随机串 | 防重放 |
| `X-Param-client-id` | — | 客户端实例 id |
| `X-Param-device-id` | — | 设备 id |
| `X-Param-system-id` | — | 系统 id |
| `X-Param-user-id` | — | 登录后才有 |
| `X-Param-Guest-ID` | — | 游客/协同时用 |
| `X-Param-SIGN` | 上面算出的签名 | — |

登录前 `user-id` 不存在，就不发也不参与签名（`build_headers` 里
"absent ones are not sent or signed"）。

另外还有：

```
Authorization: Bearer <token>
Cookie: nrd_access_token=<token>; nrd_csrf_access_token=<csrf>
```

## 4. 登录接口（四条路，全部确认）

```
POST /api/v1/login/by_uu_token      用 UU 加速器账号 token 换登录
POST /api/v1/login/by_mumu_token    用 MuMu 模拟器 token 换登录
POST /api/v1/login/by_mobile        手机号 + 验证码
POST /api/v1/login/by_qrcode        扫码登录
POST /api/v1/login/check_uu_token   校验 token 是否有效
POST /api/v1/login/check_mumu_token
```

配套：

```
POST /api/v1/qrcode/gen/login       生成登录二维码
GET  /api/v1/qrcode/login/status    轮询扫码状态
GET  /api/v1/qrcode/abstract
POST /api/v1/security/mobile/code   发短信验证码
POST /api/v1/security/client_lock/send_code
POST /api/v1/security/client_lock/reset
POST /api/v1/user/logout
GET  /api/v1/user/info              拿 user_id / 昵称等
POST /api/v1/user/ticket/for/plugin
POST /api/v1/plugin/login/by_ticket
```

对应 exe 里 `host::ControlledSession::foundation` 的四个处理函数：

```
handleManualLoginByUUAccToken      → /api/v1/login/by_uu_token
handleManualLoginByMumuToken       → /api/v1/login/by_mumu_token
handleManualLoginBySms             → /api/v1/login/by_mobile
handleManualLoginByQrCode          → /api/v1/login/by_qrcode
```

四个都能用。**扫码登录是自动化最省事的一条**（不需要短信网关）。

## 5. 登录之后的设备注册

被控端登录后必须把自己登记成"可控设备"，否则主控端看不到你。

```
POST /api/v1/device                      注册 / 更新本机设备
GET  /api/v1/device/list                 我名下设备列表
GET  /api/v1/device/controllable         可被控设备
GET  /api/v1/device/groups/of/my
POST /api/v1/device/update/prepare       版本升级前准备
GET  /api/v1/device/wake_on_lan/info     WOL 信息
POST /api/v1/device/set_support_wol
GET  /api/v1/device/router_wol_support
GET  /api/v1/device/uu_router_info
POST /api/v1/device/windows/init         Windows 侧初始化
POST /api/v1/device/wallpaper
POST /api/v1/device/share/...            分享控制权
```

`DeviceInitResult`（`foundation`）和 `DeviceInitializer::initDevice`
（`host`）就是这套流程的状态机，失败会
`scheduleAutoStartDeviceInitRetry` 重试。

## 6. 房间 / 会话

登录和设备注册都过了，才能进房间：

```
POST /api/v1/room/create                 建房（被控端）
POST /api/v1/room/create/refetch
POST /api/v1/room/join/by_device         主控端按 device_id 加入
POST /api/v1/room/join/refetch
POST /api/v1/room/join/share
POST /api/v1/room/clear/by_device/<id>
GET  /api/v1/room/app_flag
POST /api/v1/room/notify/user/<id>
```

房间建好后，Socket.IO 连上去，收发 `room_info` / `be-controlled` / `soac` /
`released`，WebRTC 才开始。

游客/临时控制走另一套：

```
POST /api/v1/guest/create
POST /api/v1/guest/room/create
POST /api/v1/guest/qrcode/abstract
POST /api/v1/guest/room/share/upload/sign
POST /api/v2/room/join/share/by_code
POST /api/v2/room/join/share/by_confirmation
```

## 7. 令牌怎么来

这是唯一没有"协议可逆"的部分：**token 是登录成功后服务端发的**，不是本地算的。

```
POST /api/v1/login/by_qrcode
  ↓
200 OK {
  token:      "...",   ← nrd_access_token
  csrf:       "...",   ← nrd_csrf_access_token
  user_id:    "...",
  ...
}
  ↓
之后的每个请求都带上：
  Authorization: Bearer <token>
  Cookie: nrd_access_token=<token>; nrd_csrf_access_token=<csrf>
  X-Param-user-id: <user_id>
```

`token` 有有效期，过期返回 **HTTP 422 + `code=401 reason=device_not_logged_in`**
或 `Token无效或已过期`（这两个字符串 exe 里都有）。

因此 Linux 被控端有两种落地方式：

### A. 自己在 Linux 上做扫码登录（推荐）

```
1. POST /api/v1/qrcode/gen/login        → 拿到二维码内容/qr id
2. 终端里渲染二维码（qrcode crate）
3. 手机 UU 远程 App 扫码确认
4. 轮询 GET /api/v1/qrcode/login/status → 拿到 token
5. 把 token/csrf/user_id 存到本地配置（0600）
6. 之后每 90 天左右重扫一次，或实现 refresh
```

全自动，不需要密码，不需要短信，不需要手动复制任何东西。

### B. 从官方客户端登录态里取（已有账号时最快）

官方客户端装完后把凭据存在本地。`uu-remote-linux` 走的也是这条路
（`Credentials{user_id, device_id, client_id, system_id, token, csrf, sign_key}`
从配置文件读）。

优点：立刻可用。缺点：token 过期后要重新从客户端取。

## 8. 完整调用链

```
① 扫码/取 token
   POST /api/v1/qrcode/gen/login
   GET  /api/v1/qrcode/login/status        → token, csrf, user_id

② 生成本机身份（首次）
   device_id   = 随机 UUID，持久化
   client_id   = 随机 UUID，持久化
   system_id   = 随机 UUID 或系统指纹，持久化

③ 注册设备
   POST /api/v1/device                      → 设备上线，主控端可见

④ 进房间
   POST /api/v1/room/create                 → room token

⑤ 连信令
   WSS  Socket.IO v4  →  发 room_info
   收 be-controlled (app_data = ConnectOptions)  ★ 用本仓库 proto 解

⑥ 建 WebRTC
   CreateOffer → 发 soac(sdp, gzip_sdp) → 收 answer → 交换 candidate
   CreateDataChannel

⑦ 控制面
   DataChannel 收 protobuf Message
   按 request_id 回 RpcResponse     ★ 用本仓库 proto 编

⑧ 保活
   Token 过期 → HTTP 422 → 重新扫码
   房间断开 → POST /api/v1/room/create/refetch
```

## 9. 可直接跑的验证代码

见 `src/login.rs`：不依赖任何网易代码，用 `reqwest` + HMAC-SHA256 实现
签名和头生成，能打出真实的 `/api/v1/login/check_uu_token` 请求验证签名是否正确。

```bash
cargo run --bin login_probe -- --token <TOKEN> --csrf <CSRF> --user-id <UID>
```

返回 200 说明签名算法、头集合、密钥全部正确。返回 401 说明 token 过期
（算法对但凭据过期）；返回 403 说明签名有误。

## 10. 边界说明

本仓库**不包含**：

- 任何真实 token / csrf / user_id（那是账号凭据）
- 服务端返回体的字段清单（`LoginResponse` 结构只能靠抓包补）
- 手机号登录的短信签名

包含的是：**算法、密钥、头集合、端点表** —— 这四样足以从零实现登录。
