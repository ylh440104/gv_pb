# 信令与控制面

从 `GameViewerServer.exe` 4.45.0.4318 与 `streamer.dll` 还原。

## 连接链路

```
POST /api/v1/room/create
  -> { code: 0, data: { server, token, room_id, ... } }
  -> wss://<server>/socket.io/?transport=websocket&EIO=4
     header: X-NRD-AUTH: <token>
```

Engine.IO v4 的文本帧前缀：

```
0{...}            engine open，带 sid / pingInterval / pingTimeout
40{...}           socket.io namespace 已连上
2                 ping（服务端发）
3                 pong（客户端回）
42<id>["event", payload]      事件，带 ack id
43<id>[payload]               ack 回调
41                服务端关闭 namespace
44                engine 错误
```

握手：namespace 连上后立刻发

```
42<id>["room_info"]
```

ack 回 `4310["success",{"reconnect_key":"..."}]`。

## 事件表

| 事件 | 方向 | 内容 |
|---|---|---|
| `bmsg_push` | 服务端→host | 设备信息变更 |
| `be-controlled` | 服务端→host | 主控端请求接管，带 `capture_type` / `ice_id` / `client_id` |
| `soac` | 双向 | WebRTC 信令：`type` = offer / answer / candidate |
| `released` | 服务端→host | 主控端释放控制 |
| `left` / `leave` | 服务端→host | 对端离开房间 |

`soac` 载荷：

```json
{
  "type": "offer",
  "sdp": "v=0\r\n...",
  "gzip_sdp": "H4sI...",
  "candidate": {...},
  "ice_id": "ice-1",
  "sdp_mid": "0",
  "sdp_mline_index": 0,
  "client_id": "c-1"
}
```

`be-controlled` 的 `capture_type` 与 `SimpleAction.ActionType` 同值域：

```
4  capture_desktop
5  capture_window
6  capture_mumu
7  stop_capture
8  restart_capture
```

## 心跳

服务端 `pingInterval` / `pingTimeout` 由 `0{...}` 下发，典型 25000 / 20000 毫秒。
超时未回 pong 即判链断，需要重建房间。

## 控制面

WebRTC DataChannel 之上跑 protobuf。信封是 `gv_pb.Message`：

```
1  seq        int64
2  timestamp  int64
20 input_event            <- 移动端触摸
21 rpc_request            <- 桌面端控制参数
22 rpc_response           <- 被控端回包
```

每个 RPC 带 `RequestHeader.request_id`，响应回同一个 id。
`RpcRequest` 的分支号与 `RpcResponse` 的分支号是**不同**的：

```
RpcRequest              RpcResponse
1  request_header       1  response_header
2  capture_setting      2  capture_setting
3  fps_setting          3  private_screen_setting
4  video_quality        4  file_transfer_ftp_response
5  cursor_setting       5  clip_response
6  resolution_setting   6  text_change_response
...
```

`CaptureSettingRequest` 的 18 个字段：

```
 1  fps                      FPS
 2  frame_quality            VideoQuality
 3  cursor_capture           bool
 4  screen_id                int32
 5  resolution_width         int32
 6  resolution_height        int32
 7  chorma_format            ChromaFormat   (原拼写如此)
 8  max_custom_bitrate       int32
 9  dpi_scale                int32
10  resolution_type          ChooseResolutionType
11  enable_hdr               bool
12  auto_frame_quality       VideoQuality
13  codec_type               CodecType
14  max_scale_width          int32
15  max_scale_height         int32
16  resolution_pixel_width   int32
17  resolution_pixel_height  int32
18  fpsCount                 int32
```

响应侧 `CaptureSettingResponse` 只有一个字段：

```
1  repeated Error errors
```

`Error`：

```
1  error_code     int32
2  error_message  string
3  error_detail   string
```

## 已被交叉验证的字段号

`VallierDev/uu-remote-linux` 在 `src/webrtc.rs` 里靠运行时观察猜的判据：

```rust
(21, WireValue::Bytes(rpc)) => ..., rpc.number in {2, 3, 4, 6}
```

对照本仓库 schema：

```
Message.rpc_request              = 21
RpcRequest.capture_setting       = 2
RpcRequest.fps_setting           = 3
RpcRequest.video_quality_setting = 4
RpcRequest.resolution_setting    = 6
```

四个号全中。

## 触摸输入

```
Message.input_event (20)
  TouchEvent touch_event (1)
    TouchEventType event_type (1)
    repeated TouchEventPoint touch_points (2)
      uint32 id (1)
      float  x (2)
      float  y (3)
      float  radius_x (4)
      float  radius_y (5)
      float  angle (6)
      float  pressure (7)
```

注意是 `float` + `wire type 5`（fixed32），不是 varint。

## 桌面端键鼠

桌面客户端**不走**上面的 protobuf 触摸通道，而是在同一条 DataChannel 上另开
一路 JSON：

```json
{"action":"kbd_press","key":65}
{"action":"kbd_release","key":65}
{"action":"kbd_click","key":65}
{"action":"mouse_press","button":1}
{"action":"mouse_release","button":1}
{"action":"mouse_click","button":1}
{"action":"mouse_move_absolute","abs_x":512.0,"abs_y":384.0}
{"action":"mouse_relative","dx":3,"dy":-2}
{"action":"mouse_scroll","dx":0,"dy":-1}
{"action":"text_input","content":"hello"}
{"heartbeat":"kbd_heartbeat"}
```

两条通道并存：移动端 controller 用 protobuf，桌面 controller 用 JSON。

## 代码

| 模块 | 职责 |
|---|---|
| `src/signaling.rs` | Engine.IO 帧解析、事件解码 |
| `src/signal_session.rs` | 心跳状态机、offer 应答构造 |
| `src/ws_frame.rs` | WebSocket 帧编解码（RFC 6455） |
| `src/ws_client.rs` | 握手与 socket 收发 |
| `src/wire.rs` | protobuf wire 编解码、信封、RpcReply |
| `src/control.rs` | 请求分派、回包构造 |

`scripts/wire_check.rs` 是不依赖任何 crate 的自检程序，覆盖 43 项：

```
rustc -O -o wire_check scripts/wire_check.rs && ./wire_check
43 passed, 0 failed
```

## 限制

静态还原拿不到**时序**：

- 谁先发、超时多久
- `seq` / `timestamp` 的推进规则
- HEVC 在什么条件下真正启用
- `reconnect_key` 的刷新时机

这些只能靠运行时观察。本仓库给的是结构与字段号，时序需要抓包补。

TLS 运输层没有内置：`ws_client` 只做明文 `ws://`，接 `wss://` 需要在外面套一层
TLS stream。这是有意为之——避免把 TLS 实现硬编码进协议库，交给调用方选择。
