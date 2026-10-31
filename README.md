# gv_pb

网易 UU远程 PC 被控端 `GameViewerServer.exe` 的 protobuf 协议还原，以及登录签名实现。

目标版本 `4.45.0.4318`，`GameViewerServer.exe` 33,736,696 字节。

## 还原结果

| 项目 | 数量 |
|---|---|
| proto 文件 | 13 |
| message | 197 |
| 字段 | 778 |
| enum | 57 |
| Message 信封分支 | 28 |

## 为什么能拿到

`GameViewerServer.exe` 由 MSVC 构建，未剥离 RTTI。protobuf 的
`FileDescriptorProto` 表以原始 wire format 内嵌在 `.rdata`，含文件名、
package、依赖列表、message/field/enum 全部定义。

不需要 PDB，不需要运行进程，不需要内存 dump，静态扫描即可无损还原。

## 协议结构

应用层是单一信封 `gv_pb.Message`，28 个分支：

```
 1  seq                      15  system_state_change
 2  timestamp                16  query_system_state
 3  simple_action            17  clipboard_change
 4  launch_app               18  codec_negotiation
 5  show_app                 19  capture_config_response
 6  mumu_operate             20  input_event
 7  screens                  21  rpc_request
 8  capture_change           22  rpc_response
 9  capture_config           23  active_window_change
10  rom_message              24  launch_cloud_pc_app
11  send_to_rom              25  capture_setting_sync
12  report_error             26  remote_download_path
13  system_metrics           27  port_mapping_frame
14  report_qos_stats         28  terminal_session_changed
```

`RpcRequest` 29 个分支与 `RpcResponse` 26 个分支严格配对，靠
`RequestHeader.request_id` / `ResponseHeader.request_id` 关联。

`SimpleAction.ActionType` 28 个值：

```
 0  ACTION_TYPE_ECHO_REQUEST         14  ACTION_TYPE_MUTE_SYSTEM
 1  ACTION_TYPE_ECHO_RESPONSE        15  ACTION_TYPE_AUTO_UNLOCK_SYSTEM
 2  ACTION_TYPE_SHOW_DESKTOP         16  ACTION_TYPE_MUTE_RESTORE
 3  ACTION_TYPE_SHOW_WINDOWS         17  ACTION_TYPE_MUTE_PERMANENT
 4  ACTION_TYPE_CAPTURE_DESKTOP      18  ACTION_TYPE_MUTE_TEMPORARY
 5  ACTION_TYPE_CAPTURE_WINDOW       19  ACTION_TYPE_SET_AUTO_LOCK
 6  ACTION_TYPE_CAPTURE_MUMU         20  ACTION_TYPE_STOP_ACQUIRE_UPDATE
 7  ACTION_TYPE_STOP_CAPTURE         21  ACTION_TYPE_QUIT_CAPTURE
 8  ACTION_TYPE_RESTART_CAPTURE      22  ACTION_TYPE_ACQUIRE_SYSTEM_INFO
 9  ACTION_TYPE_REBOOT_SYSTEM        23  ACTION_TYPE_MICROPHONE_OPENED
10  ACTION_TYPE_LOCK_SYSTEM          24  ACTION_TYPE_MICROPHONE_CLOSED
11  ACTION_TYPE_KEY_TOGGLE           25  ACTION_TYPE_SPEAKER_OPENED
12  ACTION_TYPE_SHUTDOWN_SYSTEM      26  ACTION_TYPE_SPEAKER_CLOSED
13  ACTION_TYPE_SHOW_TASKMGR         27  ACTION_TYPE_NEED_RESTART_USERAPP
```

## 官方传输路径

`streamer.dll` 保留了完整 Google libwebrtc 编译单元路径：

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

编解码：

```
H.264   modules\video_coding\codecs\h264\h264_encoder_impl.cc
        CWelsH264SVCEncoder
H.265   modules\rtp_rtcp\source\video_rtp_depacketizer_h265.cc
        modules\rtp_rtcp\source\rtp_format_h265.cc
Opus    libopus 完整源码
FEC     ulpfec / flexfec
```

媒体层是 RFC 3550 / 8831，不是私有协议。信令 `soac` 消息携带
`sdp` / `gzip_sdp` / `candidate` / `ice_id` / `sdp_mid` / `sdp_mline_index`。

## 登录

签名密钥：

```
alWiSzXZTLu3WfFnw13uBru3    X-Param-SIGN
oK5zJ1zJ5cF6kK9eW2jM8oF3    X-Param-Plugin-Ident
```

签名算法：

```
payload    = METHOD + path + params + body
signature  = hex(HMAC-SHA256(key, payload))
```

`params`：所有 `x-param-*` 头小写化，排除 `x-param-sign` 自身，跳过空值，
拼成 `key=value`，按 key 字典序，`&` 连接。`path` 不含 query。

登录接口：

```
POST /api/v1/qrcode/gen/login        出码
GET  /api/v1/qrcode/login/status     轮询，确认后返回 token
POST /api/v1/login/by_qrcode
POST /api/v1/login/by_mobile
POST /api/v1/login/by_uu_token
POST /api/v1/login/by_mumu_token
POST /api/v1/login/check_uu_token
POST /api/v1/login/check_mumu_token
GET  /api/v1/user/info
POST /api/v1/user/logout
```

设备与房间：

```
POST /api/v1/device
GET  /api/v1/device/list
GET  /api/v1/device/controllable
POST /api/v1/room/create
POST /api/v1/room/create/refetch
POST /api/v1/room/join/by_device
POST /api/v1/room/clear/by_device/<id>
```

完整链路：

```
扫码取 token
  -> 生成 client_id / device_id / system_id 并持久化
  -> POST /api/v1/device 注册设备
  -> POST /api/v1/room/create 建房
  -> WSS Socket.IO 发 room_info，收 be-controlled / soac
  -> CreateAnswer + ICE，建 DataChannel
  -> protobuf 控制面按 request_id 回 RpcResponse
```

## 代码

| 路径 | 内容 |
|---|---|
| `src/signer.rs` | 签名与请求头 |
| `src/api.rs` | 登录、设备、房间请求 |
| `src/signaling.rs` | Engine.IO 帧解析、事件解码 |
| `src/signal_session.rs` | 心跳状态机、SDP offer 应答 |
| `src/ws_frame.rs` | WebSocket 帧编解码 |
| `src/ws_client.rs` | WS 握手与收发 |
| `src/wire.rs` | protobuf wire、信封、RpcReply |
| `src/control.rs` | 控制面请求分派与回包 |
| `src/bin/login_probe.rs` | 登录命令行 |
| `src/bin/host.rs` | 被控端命令行 |
| `src/roundtrip_test.cc` | C++ wire 往返验证 |
| `scripts/extract_proto.py` | PE 提取 schema |
| `scripts/gen_proto.py` | schema 生成 proto |
| `scripts/extract_rtti.py` | PE 提取 C++ 符号 |
| `scripts/sign_check.rs` | 签名自检，零依赖 |
| `scripts/wire_check.rs` | wire 与帧自检，零依赖 |

```
rustc -O -o sign_check scripts/sign_check.rs && ./sign_check
rustc -O -o wire_check scripts/wire_check.rs && ./wire_check

cargo test
cargo run --bin login_probe -- sign --path /api/v1/login/check_uu_token --ts 1700000000
cargo run --bin login_probe -- qr
cargo run --bin login_probe -- check --token <TOKEN>

python3 scripts/extract_proto.py GameViewerServer.exe -o schema.json
python3 scripts/gen_proto.py schema.json -o proto
python3 scripts/extract_rtti.py GameViewerServer.exe -o rtti.txt

cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build -j
ctest --test-dir build --output-on-failure
```

`scripts/sign_check.rs` 不依赖任何 crate，自带 SHA-256 与 HMAC 实现，用
RFC 4231 向量和 `analysis/sign_vectors.json` 双重校验签名契约。

## 交叉验证

`VallierDev/uu-remote-linux` 是运行时观察驱动的 Linux 被控端，只带 184 行通用
wire 解码器。它盲猜的字段号与本仓库 schema 完全一致：

```
src/webrtc.rs    (21, Bytes(rpc)) 然后 rpc.number in {2,3,4,6}
main.proto       Message.rpc_request              = 21
rpc_main.proto   RpcRequest.capture_setting       = 2
                 RpcRequest.fps_setting           = 3
                 RpcRequest.video_quality_setting = 4
                 RpcRequest.resolution_setting    = 6
```

它的 `SimpleAction 4 capture_desktop` 对应 `ACTION_TYPE_CAPTURE_DESKTOP = 4`。
它唯一实现了回包编码的 echo-response shape 对应 `ECHO_REQUEST = 0` /
`ECHO_RESPONSE = 1`。

三方独立源码指向同一份签名密钥 `alWiSzXZTLu3WfFnw13uBru3`。

## 文件

```
proto/             13 个 proto
analysis/
  schema.json      完整 schema，含字段号类型嵌套
  rtti_symbols.txt 1539 个 C++ 类与命名空间
  pb_messages.txt  177 个 gv_pb 消息类名
docs/
  AUTH.md          登录鉴权细节
  OFFICIAL-TRANSPORT.md  传输层证据
  CROSSREF-uu-remote-linux.md  与参考项目逐条对照
```

## 边界

本仓库不含真实 token、csrf、user_id，不含服务端返回体字段清单，不含手机号
短信签名。

`connect.proto` 与 `general.proto` 在原二进制互相 import，descriptor set 无法
表达 import 环，因此合并为 `session.proto`，其余文件名保持原样。