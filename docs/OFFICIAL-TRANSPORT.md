# 官方传输路径：libwebrtc

结论先给：**官方 PC 被控端走的就是标准 WebRTC，实现是 Google libwebrtc（C++），
静态编译进 `streamer.dll`。** 不是自研私有媒体协议。

这意味着不需要逆向媒体栈——直接用任何成熟的 WebRTC 实现对接即可，协议就是
RFC 3550 / RFC 7742 / RFC 8831 那一套。

---

## 证据：`streamer.dll` 里的 libwebrtc 源码路径

`streamer.dll`（18.6 MB）保留了大量 WebRTC 编译单元的 `__FILE__` 字符串：

```
..\..\pc\peer_connection.cc                     ← PeerConnection 本体
..\..\pc\sdp_offer_answer.cc                    ← offer/answer 协商
..\..\pc\jsep_transport.cc                      ← BUNDLE / m-line 映射
..\..\pc\jsep_transport_controller.cc
..\..\pc\dtls_srtp_transport.cc                 ← DTLS-SRTP
..\..\pc\srtp_transport.cc
..\..\pc\srtp_session.cc
..\..\pc\sctp_transport.cc                      ← DataChannel 传输
..\..\pc\sctp_data_channel.cc
..\..\pc\sctp_utils.cc
..\..\pc\data_channel_controller.cc
..\..\pc\ice_server_parsing.cc                  ← STUN/TURN 服务器解析
..\..\pc\rtp_transceiver.cc
..\..\pc\rtp_parameters_conversion.cc
..\..\pc\rtcp_mux_filter.cc
..\..\pc\media_session.cc
..\..\pc\rtc_stats_collector.cc
..\..\call\bitrate_allocator.cc                 ← 码率分配
..\..\call\rtp_transport_controller_send.cc     ← 拥塞控制
..\..\call\rtp_video_sender.cc
..\..\call\rtx_receive_stream.cc                ← RTX 重传
..\..\call\flexfec_receive_stream_impl.cc       ← FEC
..\..\call\rsfec_receive_stream_impl.cc
..\..\rtc_base\async_udp_socket.cc
..\..\rtc_base\async_tcp_socket.cc              ← TURN over TCP
..\..\rtc_base\openssl_stream_adapter.cc        ← DTLS 用 OpenSSL
..\..\rtc_base\physical_socket_server.cc
..\..\p2p\base\stun_port.cc
..\..\p2p\base\turn_port.cc
..\..\p2p\base\dtls_transport.cc
..\..\p2p\base\quic_*.cc
```

这是完整的 libwebrtc 目录结构，一个不缺。

## 编解码器

```
H.264   CWelsH264SVCEncoder（OpenH264 软件编码器）
        modules\video_coding\codecs\h264\h264_encoder_impl.cc
        modules\video_coding\codecs\h264\h264_decoder_impl.cc
        modules\rtp_rtcp\source\video_rtp_depacketizer_h264.cc
        modules\rtp_rtcp\source\rtp_format_h264.cc
        common_video\h264\h264_bitstream_parser.cc

H.265   modules\rtp_rtcp\source\video_rtp_depacketizer_h265.cc
        modules\rtp_rtcp\source\rtp_format_h265.cc
        modules\video_coding\h265_vps_sps_pps_tracker.cc
        （WebRTC 主线没有 H265 打包器，这是网易自己补的）

Opus    libopus 完整源码（72 处符号）

另有 H264/H265 的 DXVA2 硬解：
        third_party\ffmpeg\libavcodec\dxva2_h264.c
        third_party\ffmpeg\libavcodec\dxva2_hevc.c
```

`streamer-config.json` 里那段 `disable_h265_codec` 黑名单（GTX 6xx/7xx/8xx/9xx、
Quadro）现在也解释得通了：H265 打包器是补的，老 N 卡驱动路径上会出问题。

## SDP 在信令里的位置

`GameViewerServer.exe` 的 Socket.IO 消息结构里有：

```json
{
  "type": "soac",
  "sdp": "...",          // 明文 SDP
  "gzip_sdp": "...",     // 压缩后的 SDP（同一份内容）
  "candidate": {...},     // ICE candidate
  "ice_id": "...",
  "sdp_mid": "...",
  "sdp_mline_index": 0,
  "client_id": "..."
}
```

`gzip_sdp` 这个字段名在 `streamer.dll` 里也能找到，和 `CreateOffer` /
`CreateAnswer` / `sdpFmtpLine` / `sdpMLineIndex` 在同一批字符串里。

被控端因此要实现的就四件事：

1. `CreateOffer()` 出 SDP
2. 收 controller 的 answer，`SetRemoteDescription()`
3. 双向交换 ICE candidate
4. `CreateDataChannel()`

## 传输层的真实形状

```
controller (官方客户端)
        │
        │  Socket.IO / Engine.IO v4 over WSS      ← 信令：room_info / be-controlled / soac / released
        ▼
   nrd 信令服务器 (api.nrd.nie.163.com)
        │
        │  Socket.IO 下发 be-controlled，携带二进制 app_data
        ▼
host (Linux 被控端)
        │
        │  ★ WebRTC 直连或经由 TURN 中转（iceServers 由信令下发）
        ▼
controller
   ├── SRTP 媒体流（H.264/H.265 视频 + Opus 音频）
   └── SCTP DataChannel
        ├── protobuf 控制面（本仓库 gv_pb schema）
        └── CONTROL JSON 通道（kbd_press / mouse_move_absolute / ...）
```

关键点：**媒体不经过网易服务器**。信令服务器只做房间撮合和 SDP/candidate 交换，
音视频走 P2P，打不通才走 TURN（`iceServers` 由信令下发，字段里带
`force_relay` 可强制中转）。

## 所以 Linux 被控端应该怎么做

### 不要走的路

- ❌ 用 `libstreamer` / `streamer.dll` 的私有 ABI（`nrd_renderer_ipc.dll` 那套）
  —— 那是 Windows 内部进程间通信，不是网络协议。
- ❌ 重写一个私有媒体协议 —— 它不存在。
- ❌ 依赖网易的 Windows NVENC 路径 —— Linux 侧用 VAAPI / NVENC 原生 API 即可。

### 应该走的路

**用现成的 WebRTC 库对接标准协议。** 现成可选的：

| 语言 | 库 | 说明 |
|---|---|---|
| Rust | `webrtc-rs` (`webrtc = "0.10"`) | 纯 Rust，DataChannel + SRTP + ICE 全有 |
| C++ | libwebrtc (和官方同一份) | 行为最接近，但构建成本高 |
| 任意 | `libdatachannel` | 轻量，C++/C API，SCTP DataChannel 完备 |
| Go | `pion/webrtc` | 纯 Go，生产可用 |

`webrtc-rs` 已经在
[VallierDev/uu-remote-linux](https://github.com/VallierDev/uu-remote-linux)
里跑通了整条链路（信令 + PeerConnection + DataChannel + H.264/Opus），
可以直接参考它的 `src/webrtc.rs`（1752 行）。

### 被控端要写的代码，按优先级

1. **信令客户端** — Socket.IO v4 发 `room_info`，收 `be-controlled` /
   `soac` / `released`。`be-controlled` 的 `app_data` 就是本仓库
   `ConnectOptions`（protobuf），里面有 `capture_params` / `decoder_cap_list` /
   `force_virtual_display` / `client_type` / `device_id` / `client_version`。

2. **PeerConnection** — `SetRemoteDescription(offer)` →
   `CreateAnswer()` → `SetLocalDescription()` → 收集 candidate 回传。
   DataChannel 用 in-band 协商（SCTP over DTLS）。

3. **采集 + 编码** — 任选一种推 track：
   - X11: `x11rb` + XDamage
   - Wayland: `wlr-screencopy` / `xdg-desktop-portal` ScreenCast
   - KMS: DRM dumb buffer
   编码 H.264（VAAPI / NVENC / OpenH264），Opus 采 PipeWire / PulseAudio
   monitor。

4. **控制面响应** — 用本仓库 `RpcResponse` 回包。请求带
   `RequestHeader.request_id`，响应回同一个 id。这一层是纯 protobuf 逻辑，
   不需要碰媒体。

5. **输入注入** — `/dev/uinput` 虚拟键鼠，或 Portal RemoteDesktop
   （Wayland 下的正路）。

## 与本仓库的关系

本仓库提供的是第 4 步和第 1 步要用的**数据结构定义**：

- `ConnectOptions` — `be-controlled` 里 `app_data` 的完整 schema
- `RpcRequest` / `RpcResponse` — 29 / 26 个分支的严格配对表
- `CodecNegotiation` / `EncoderCap` / `DecoderCap` — 编解码协商
- `InputEvent` / `TouchEvent` — 触摸注入（移动端 controller）
- `CaptureParams` / `CaptureSettingRequest` — 采集参数

媒体层不需要本仓库，媒体层是标准 WebRTC。
