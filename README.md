# gv_pb — UU远程 (GameViewer) 协议 schema 还原

网易 UU远程 PC 端被控端（`GameViewerServer.exe` 4.45.0）的 protobuf 协议
schema 逆向还原结果，以及可复现的提取工具链。

本仓库只包含**协议数据结构定义**与**分析工具**，不含任何网易的二进制、
代码、密钥或服务端凭据。

---

## 为什么能做

`GameViewerServer.exe` 是 MSVC 构建的 x64 PE，且**未剥离 RTTI**。它使用
protobuf 做控制面协议，而 MSVC + protobuf 的构建产物会把编译后的
`FileDescriptorProto` 表以**原始 wire format 形式内嵌在 `.rdata`** 里：
文件名、package、dependency 列表、message / field / enum 定义全都是原始
长度前缀编码。

这意味着：

- 不需要 PDB
- 不需要运行进程、不需要内存 dump
- 不需要符号表（虽然它恰好也没剥）
- 直接静态扫描即可**无损**还原 `.proto`

## 还原结果

| 项目 | 数量 |
|---|---|
| `.proto` 文件 | 14 个（`session.proto` 由 `connect` + `general` 合并） |
| 顶层 message | 197 |
| 字段 | 778 |
| enum（含嵌套） | 57 |
| `Message` 信封分支 | 28 |

## 协议结构

应用层是一个单一信封 `gv_pb.Message`，28 个 `oneof` 语义分支：

```protobuf
message Message {
  int64 seq = 1;
  int64 timestamp = 2;
  SimpleAction            simple_action            = 3;
  LaunchApp               launch_app               = 4;
  ShowApp                 show_app                 = 5;
  MumuOperate             mumu_operate             = 6;
  ScreenSources           screens                  = 7;
  CaptureChange           capture_change           = 8;
  CaptureConfig           capture_config           = 9;
  RomMessage              rom_message              = 10;
  SendToRom               send_to_rom              = 11;
  ReportError             report_error             = 12;
  SystemMetrics           system_metrics           = 13;
  ReportQosStats          report_qos_stats         = 14;
  SystemStateChange       system_state_change      = 15;
  QuerySystemState        query_system_state       = 16;
  ClipboardChange         clipboard_change         = 17;
  CodecNegotiation        codec_negotiation        = 18;
  CaptureConfigResponse   capture_config_response   = 19;
  InputEvent              input_event              = 20;
  RpcRequest              rpc_request              = 21;
  RpcResponse             rpc_response             = 22;
  ActiveWindowChange      active_window_change     = 23;
  LaunchCloudPcApp        launch_cloud_pc_app      = 24;
  CaptureSettingSync      capture_setting_sync     = 25;
  RemoteDownloadPath      remote_download_path     = 26;
  PortMappingFrame        port_mapping_frame       = 27;
  TerminalSessionChanged  terminal_session_changed = 28;
}
```

`RpcRequest` / `RpcResponse` 是严格配对的请求-应答表（29 / 26 个分支），
覆盖采集参数、帧率、画质、光标、分辨率、隐私屏、文件传输、剪贴板、
虚拟鼠标、虚拟显示器、插件、虚拟声卡、投屏、超屏、绘制、终端会话改名。

### 控制面

RPC 通过 HTTP 走服务端（`NrdHttpResponse` 在 RTTI 中可见），
`RequestHeader.request_id` / `ResponseHeader.request_id` 做请求关联。

### 会话建立

```protobuf
message ConnectOptions {
  Type                      capture_type                  = 1;
  int32                     type_value                    = 2;
  CaptureParams             capture_params                = 3;
  repeated DecoderCap       decoder_cap_list              = 4;
  bool                      force_virtual_display         = 5;
  repeated VirtualDisplayMode virtual_display_modes       = 6;
  VirtualDisplayInitResolution virtual_display_init_resolution = 7;
  ClientType                client_type                   = 8;
  string                    device_id                     = 9;
  ControlConnectType        control_connect_type          = 10;
  FeatureFlag               feature_flag                  = 11;
  string                    client_version                = 12;
}
message CodecNegotiation {
  repeated EncoderCap encoder_cap_list = 1;
}
```

### 输入注入

```protobuf
message TouchEventPoint {
  uint32 id = 1;  float x = 2;  float y = 3;
  float radius_x = 4;  float radius_y = 5;
  float angle = 6;  float pressure = 7;
}
message TouchEvent {
  TouchEventType event_type = 1;
  repeated TouchEventPoint touch_points = 2;
}
message InputEvent {
  TouchEvent touch_event = 1;
}
```

## 关键枚举

```
CodecType       : H264=1, H265=2
FPS             : FPS_30=1, FPS_60=2, FPS_90=3, FPS_144=4
VideoQuality    : Fast=1, General=2, HD=3, Bluray=4, Auto=5, Custom=6
CaptureMode     : DESKTOP=1, WINDOW=2, MUMU=3, HOOK=4, GLOBAL=5
ChromaFormat    : 420=1, 422=2, 444=3, 400=4
ClientType      : IOS=1, ANDROID=2, WINDOWS=3, MAC=4
ScreenType      : Physics=0, Virtual=1, Super=2
```

## 被控端各模块（来自 RTTI）

```
video::business::host::VideoModule
audio::business::host::AudioModule
input::business::host::InputModule / InputContext
input::support::IInputInjector / IVhidInputTransport
display::*  (85 个符号)
terminal::business::host  (57 个符号)
clipboard::*  file_transfer::*  port_mapping::*  mumu::*
session_auth::business::host::SessionAuthModule
plugin_runtime::shared  wol::*  system_power::*
host::ControlledSession::foundation
  handleManualLoginByQrCode / BySms / ByUUAccToken / ByMumuToken
  requestUserRoom / requestGuestRoom / createGuest
```

媒体引擎不在 exe 里，在 `streamer.dll`（18.6 MB）。它的 116 个导出全是
NVAPI 套壳（`ctl*`），真正的输入/截图 ABI 在 `nrd_renderer_ipc.dll`：

```c
int  nrd_ipc_create(const char *);
int  nrd_ipc_connect(int);
void nrd_ipc_destroy(int);
int  nrd_input_send_event(int, const char *, int);
int  nrd_input_send_text(int, const char *);
int  nrd_screenshot(int, int, int);
int  nrd_system_operate(int, const char *, const char *);
int  nrd_ipc_send_streamer_data(int, int, void *, int);
void nrd_set_callback(struct nrd_callback);
void nrd_ipc_server_destroy(void);
```

纯 C 符号，无 C++ 修饰，可直接在 Linux 上按同一 ABI 重写。

## 构建

本仓库的核心产物是 `.proto`（与平台无关）。C++ 部分是为验证 schema 正确性：

```bash
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
ctest --test-dir build --output-on-failure
```

`roundtrip_test` 做三件事：

1. 构造真实形状的 `Message`（seq + timestamp + RPC 采集参数），序列化再解析，
   逐字段校验
2. 手工拼 wire format 字节流，用还原的 schema 解出来，验证字段号、float 编码、
   repeated 嵌套 message 全部对得上
3. 断言 `Message` 描述符里 28 个分支齐全且编号正确

CI（`.github/workflows/build.yml`）在 Ubuntu + Windows/MSVC 上构建，并且：

- 从 `analysis/schema.json` 重新生成 `.proto`，与仓库中的文件逐字比对
- `protoc` 编译全部 `.proto`
- 生成 `FileDescriptorSet`（可作为任意语言绑定的输入）
- 用生成的 Python 模块做一遍编解码 roundtrip

## 工具

```bash
# 从 PE 中还原 schema
python3 scripts/extract_proto.py GameViewerServer.exe -o schema.json

# 从 schema 生成可编译的 .proto
python3 scripts/gen_proto.py schema.json -o proto/

# 从 PE 中还原 C++ RTTI 类名（模块地图）
python3 scripts/extract_rtti.py GameViewerServer.exe -o rtti.txt
```

## 文件

```
proto/            还原出的 .proto（可直接 protoc 编译）
analysis/
  schema.json     完整 schema（机器可读，含字段号/类型/嵌套）
  rtti_symbols.txt 1539 个 C++ 类/命名空间
  pb_messages.txt   177 个 gv_pb 消息类名
src/
  roundtrip_test.cc  wire 兼容性验证
scripts/
  extract_proto.py   PE → schema.json
  gen_proto.py       schema.json → .proto
  extract_rtti.py    PE → C++ 符号
```

## 官方传输路径就是标准 WebRTC

这一点是决定性的：`streamer.dll`（18.6 MB）里保留了完整的 Google libwebrtc
编译单元路径。

```
..\..\pc\peer_connection.cc
..\..\pc\sdp_offer_answer.cc
..\..\pc\dtls_srtp_transport.cc
..\..\pc\sctp_data_channel.cc
..\..\pc\ice_server_parsing.cc
..\..\call\bitrate_allocator.cc
..\..\rtc_base\openssl_stream_adapter.cc
..\..\p2p\base\turn_port.cc
```

编解码：H.264（`h264_encoder_impl.cc` / OpenH264）、H.265（
`video_rtp_depacketizer_h265.cc`，网易自己补的打包器）、Opus、UDP FEC。

也就是说，**媒体层不用逆向，它就是 RFC 3550 / 8831 那一套**，用任何成熟的
WebRTC 库都能对接。信令里的 `soac` 消息携带 `sdp` / `gzip_sdp` / `candidate` /
`ice_id` / `sdp_mid` / `sdp_mline_index`，全是标准字段。

一个 Linux 被控端的正确做法：

| 层 | 做什么 | 用什么 |
|---|---|---|
| 信令 | Socket.IO v4，收 `be-controlled` / `soac` | 任意 Socket.IO 客户端 |
| 媒体 | 标准 WebRTC P2P（+ TURN 兜底） | webrtc-rs / libdatachannel / pion |
| 控制面 | protobuf over DataChannel | **本仓库 schema** |
| 采集 | X11 / Wayland / KMS | x11rb / portal / DRM |
| 编码 | H.264 / Opus | VAAPI / NVENC / OpenH264 / libopus |
| 注入 | uinput / Portal RemoteDesktop | — |

细节与证据见 [`docs/OFFICIAL-TRANSPORT.md`](docs/OFFICIAL-TRANSPORT.md)。

---

## 交叉验证：与 VallierDev/uu-remote-linux 对照

社区已有一个可用的 Linux 被控端实现
[VallierDev/uu-remote-linux](https://github.com/VallierDev/uu-remote-linux)
（Rust，GPL-3.0，Socket.IO 信令 + webrtc-rs + uinput/Portal 注入）。

它是运行时观察驱动的，只带了 184 行通用 protobuf wire 解码器，把消息当作不透明
字节；本仓库是静态还原的完整 schema。两者是互补的：一个知道**怎么通话**，
一个知道**说的是什么话**。

最有说服力的交叉验证在 `src/webrtc.rs`：

```rust
(21, WireValue::Bytes(rpc)) => crate::protocol::decode_wire(rpc).ok(),
...
.is_some_and(|rpc| rpc.iter().any(|field| matches!(field.number, 2 | 3 | 4 | 6)))
```

他们盲猜的 4 个字段号，对照本仓库 schema：

```
Message.rpc_request              = 21
RpcRequest.capture_setting       = 2
RpcRequest.fps_setting           = 3
RpcRequest.video_quality_setting = 4
RpcRequest.resolution_setting    = 6
```

一个不差。同理他们注释里的 `SimpleAction 4 capture_desktop`，对应
`ActionType::ACTION_TYPE_CAPTURE_DESKTOP = 4`；而他们唯一实现了回包编码的
"echo-response shape"，正是 `ACTION_TYPE_ECHO_REQUEST=0` / `ECHO_RESPONSE=1`。

详细对照、双方各自的盲区、以及把 schema 注入那个项目的具体路线，见
[`docs/CROSSREF-uu-remote-linux.md`](docs/CROSSREF-uu-remote-linux.md)。

---

## 说明

`connect.proto` 与 `general.proto` 在原二进制中互相 import。descriptor set
无法表达 import 环，因此合并为 `session.proto`，其余文件名保持原样。

逆向目标为 UU远程 PC 端 4.45.0.4318（`GameViewerServer.exe`，
33,736,696 字节）。该版本 protobuf 相关符号未剥离，RTTI 完整。
