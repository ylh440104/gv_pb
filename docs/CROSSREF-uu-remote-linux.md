# 与 VallierDev/uu-remote-linux 的交叉验证

参考项目：<https://github.com/VallierDev/uu-remote-linux>
（非官方 UU 远程 Linux 被控端，Rust，GPL-3.0-or-later）

对照版本：`6bf05a6361e58dbeec5846f044d234bed04fcf02`（2026-10-30）

---

## 一句话结论

那个项目解决了"要怎么跟网易服务端和官方客户端说话"，本仓库解决了"那些二进制
内容和数据结构到底叫什么"。

两者叠加起来，一条完整的 Linux 被控端所需的知识就齐了：

| 层 | uu-remote-linux | 本仓库 |
|---|---|---|
| 传输 | ✅ Socket.IO (Engine.IO v4) 信令 | — |
| 媒体 | ✅ WebRTC (webrtc-rs)，H.264 + Opus | — |
| 数据面 | ✅ DataChannel + SCTP，独立 CONTROL JSON 通道 | — |
| 认证 | ✅ `X-Param-SIGN` = HMAC-SHA256，`api.nrd.nie.163.com` | — |
| protobuf 语义 | ⚠️ 仅 184 行通用 wire 解码器 | ✅ 15 proto / 197 msg / 778 字段 / 57 enum |
| 屏幕采集 | ✅ X11 / Wayland / DRM-KMS / Portal | — |
| 编码 | ✅ VAAPI / NVENC / FFmpeg | — |
| 输入注入 | ✅ uinput / Portal / XTest | — |

## 最重要的证据：他们把字段号猜对了，而我们能说出名字

`src/webrtc.rs` 里判断"这是不是采集参数消息"的函数：

```rust
fn is_capture_setting_message(data: &[u8]) -> bool {
    let Ok(fields) = crate::protocol::decode_wire(data) else { return false; };
    fields.iter().find_map(|field| match (field.number, &field.value) {
        (21, WireValue::Bytes(rpc)) => crate::protocol::decode_wire(rpc).ok(),
        _ => None,
    }).is_some_and(|rpc| rpc.iter().any(|field| matches!(field.number, 2 | 3 | 4 | 6)))
}
```

他们只知道：外层 21 号里装的东西，如果内层出现 2/3/4/6 之一，就是采集参数。

用本仓库的 schema 直接读出这四个号是什么：

```
Message      (main.proto)      rpc_request              = 21  → gv_pb.RpcRequest
RpcRequest   (rpc_main.proto)  capture_setting          = 2   → CaptureSettingRequest
                               fps_setting              = 3   → FpsSettingRequest
                               video_quality_setting    = 4   → VideoQualitySettingReqeust
                               resolution_setting       = 6   → ResolutionSettingRequest
```

**四个号一个不差。** 他们的启发式判断完全正确，只是不知道名字。

再看 `SimpleAction`（`general.proto` 里 28 个值），他们注释里写的
`SimpleAction 4 capture_desktop`：

```
enum ActionType {
  ACTION_TYPE_ECHO_REQUEST         = 0;
  ACTION_TYPE_ECHO_RESPONSE        = 1;   ← 他们唯一编码过的那条 "echo-response shape"
  ACTION_TYPE_SHOW_DESKTOP         = 2;
  ACTION_TYPE_SHOW_WINDOWS         = 3;
  ACTION_TYPE_CAPTURE_DESKTOP      = 4;   ← capture_desktop ✅
  ACTION_TYPE_CAPTURE_WINDOW       = 5;
  ...
  ACTION_TYPE_NEED_RESTART_USERAPP = 27;
}
```

他们 `control_protocol.rs` 开头写着：

> Only the verified protobuf echo-response shape is encoded; other protobuf
> messages remain opaque.

`ECHO_REQUEST = 0` / `ECHO_RESPONSE = 1` 正是这个 shape。也完全对上。

## 双方各自撞到的坑

### 他们踩的

`CONTROL` 通道不是 protobuf，是 **JSON**（`kbd_press` / `mouse_move_absolute`
/ `mouse_scroll` / `heartbeat` …）。这条如果不是动态抓包根本看不出来，
静态分析 exe 也看不到——它藏在 `streamer.dll` 之后的 SCTP 载荷里。

他们因此写了 `control_protocol.rs` 单独处理 JSON，并注明"Mac ARM64 客户端"。

### 我们踩的

静态还原只能拿到**结构**，拿不到：
- 哪个帧在什么时候发（状态机）
- `seq` / `timestamp` 的推进规则
- 握手阶段谁先说话、超时多久
- HEVC 是否启用、`disable_h265_codec` 的实际生效条件

这些都只能靠运行时观察。

## 直接可以拿去用的地方

### 1. 给他们补上字段语义

他们的 `decode_wire` 是纯结构性解码。本仓库的 schema 可以让
`is_capture_setting_message` 从启发式变成确定性判断：

```rust
// 现在的启发式
rpc.iter().any(|f| matches!(f.number, 2 | 3 | 4 | 6))

// 变成语义判断后
// f.number == 2 → CaptureSettingRequest{fps, frame_quality, cursor_capture,
//                  screen_id, resolution_width, resolution_height, chorma_format,
//                  max_custom_bitrate, dpi_scale, resolution_type, enable_hdr,
//                  auto_frame_quality, codec_type, max_scale_width, max_scale_height,
//                  resolution_pixel_width, resolution_pixel_height, fpsCount}
```

也可以直接用 `protobuf` crate + 本仓库的 `.proto`，把 `WireValue` 换掉，
让整条 RPC 从 opaque 变成强类型。

### 2. 补齐他们"Sending is not implemented"的部分

他们注释里明确说这是待办：

> This decodes fields only. It intentionally assigns no heartbeat/input semantics
> and sends no acknowledgements.

本仓库的 `RpcResponse` 有完整的 26 个分支，可以对着填：

```
RpcResponse (rpc_main.proto):
  response_header            = 1   ResponseHeader{request_id}
  capture_setting            = 2   CaptureSettingResponse{repeated Error errors}
  private_screen_setting     = 3
  file_transfer_ftp_response = 4
  clip_response              = 5
  ...
  draw_resp                  = 23
  terminal_check_env_resp    = 24
  terminal_rename_session_resp = 26
```

`Sequence`: 请求带 `RequestHeader.request_id`，响应回同一个 id。这就是他们
缺的那条"acknowledgement"规则。

### 3. 输入注入的字段可以直接对上

他们的 JSON 通道：

```
kbd_press   {key:i32}
kbd_release {key:i32}
kbd_click   {key:i32}
mouse_press {button:i32}
mouse_release {button:i32}
mouse_click {button:i32}
mouse_move_absolute {abs_x:f64, abs_y:f64}
mouse_relative {dx:i32, dy:i32}
mouse_scroll {dx:i32, dy:i32}
text_input {content:String}
```

而我们还原的 protobuf 侧是触摸语义（`TouchEventPoint{id,x,y,radius_x,radius_y,angle,pressure}`，
`TouchEvent{event_type, repeated touch_points}`）。两条路是并行的：
桌面客户端走 JSON 键鼠，移动端走 protobuf 触摸。他们代码里也提到了：

> Mobile viewers in touch mode send a finger as a left-button drag at absolute
> positions (observed: Android sends press, a stream of mouse_move_absolute, release)

## 建议的合并路线

不要 fork 重写。他们的胶水层（信令、WebRTC、采集、编码、注入）是几个月
打磨出来的，重写不可行。正确做法是把 schema 当作 **spec 注入**：

1. 用本仓库的 `.proto` 生成 Rust 类型（`protobuf` crate 或 `prost`）
2. 替换 `src/protocol.rs` 的 `decode_wire`（保留它作为 fallback）
3. 把 `is_capture_setting_message`、`is_*` 系列换成 `try_into::<Message>()`
4. 实现 `RpcResponse` 回包，按 `request_id` 配对
5. 用本仓库 `analysis/rtti_symbols.txt` 里的模块名对照他们的模块划分，
   找出他们还缺哪块（比如 `wol`、`port_mapping`、`terminal` 他们都有，
   但 `notification`、`draw`、`mumu` 可能没覆盖）

`analysis/rtti_symbols.txt` 里的 `host::ControlledSession::foundation` 有完整的
登录分支：

```
handleManualLoginByQrCode
handleManualLoginBySms
handleManualLoginByUUAccToken
handleManualLoginByMumuToken
requestUserRoom / requestGuestRoom / createGuest
scheduleAutoLoginRetryIfNeeded / scheduleAutoStartDeviceInitRetry
```

他们 `login.rs` / `auth.rs` 里实现的是 token 直填（`Credentials{user_id, device_id,
client_id, system_id, token, csrf, sign_key}`）。二维码 / 短信 / 游客登录这条
他们没走，但 exe 里全都有。

## 限制

本仓库**没有**、也**不打算**包含：

- `sign_key` / token / csrf（那是账号凭据，从官方客户端登录态里取）
- 服务端 API 的响应语义
- WebRTC 的 SDP 协商细节

那些在 uu-remote-linux 里已经解决得很好，直接参考它。本仓库专注在其
没有的那一层：**数据结构定义**。

## 引用

```
VallierDev/uu-remote-linux @ 6bf05a63
  src/protocol.rs          184 行   通用 protobuf wire 解码
  src/control_protocol.rs  218 行   CONTROL JSON 通道
  src/signaling.rs         602 行   Socket.IO 信令
  src/webrtc.rs           1752 行   PeerConnection / DataChannel
  src/auth.rs              491 行   X-Param-SIGN HMAC 签名
  src/input.rs            1139 行   uinput / Portal / XTest 注入
  src/session_ice.rs       154 行   ICE 服务器与帧长判定
```