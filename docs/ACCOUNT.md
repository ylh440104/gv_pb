# 有账号之后能干什么

先说清楚边界：**账号能拿到 token，但 token 不能绕过协议本身**。有了 token 我能
做的，是把"静态还原 + 猜测"升级成"实测验证 + 补齐时序"。没有 token，协议结构
已经齐了，但所有时序细节都是空白。

---

## 一、需要你提供什么

按"能省多少事"排序。**只给第 1 项就能开始**，后面是逐渐加强。

### 第 1 项（必需）：一份登录后的凭据

四选一，任选一个即可：

| 方式 | 需要什么 | 前提 |
|---|---|---|
| A. 扫码登录 | 你手机上的 UU远程 App | **什么都不用提供**，我这边出二维码，你扫 |
| B. uu_token | 一串 token 字符串 | 装过 UU加速器 |
| C. mumu_token | 一串 token 字符串 | 装过 MuMu 模拟器 |
| D. 手机号验证码 | 手机号 + 短信码 | 什么都没有 |

**推荐 A**。理由：不经过你手复制任何敏感串，且国内手机能正常扫码。

具体怎么走：

```
1. 我调用 POST /api/v1/qrcode/gen/login 出码
2. 你把二维码内容拿手机 UU远程 App 扫一下、点确认
3. 我轮询 GET /api/v1/qrcode/login/status 拿到 token / csrf / user_id
4. 后续所有 API 调用用这个 token 签名
```

二维码内容大概是 `uuremote://login?ticket=...` 这种形式，我会直接打印出来。

### 第 2 项：一次真实抓包

这个比 token 更重要。**token 只证明"我是谁"，抓包才告诉我"话该怎么说"**。

需要抓的东西：

```
WSS:  <server>/socket.io/?transport=websocket&EIO=4
```

用官方的 Windows 客户端连一次你的 Linux 主机（或者反过来，连任意一台机器），
把 WSS 帧全部录下来。

**怎么抓**：

方案一，mitmproxy（推荐，能看明文）：

```bash
# 装好 mitmproxy 后
mitmproxy --mode transparent --showhost
# 或者直接
mitmdump -p 8080 -w uu.flow
```

然后 Windows 客户端设置系统代理指向 mitmproxy，trust mitmproxy 的 CA 证书。

方案二，Wireshark + SSLKEYLOGFILE：

```bash
# 这个方案客户端不用改配置，但要能拿到 TLS 会话密钥
# Chrome/Electron 系支持 SSLKEYLOGFILE，native 客户端一般不支持
# 所以大概率走方案一
```

方案三，如果你能接受改 hosts 把信令域名指到本地，我用自签证书伪装成服务器，
直接录明文。这个最干净，但需要客户端不校验证书（大概率会校验，可能失败）。

**拿到后给我什么**：

- `uu.flow` 文件（mitmproxy 格式），或者
- 导出的 JSON / HAR，或者
- 就是纯文本，每行一个 WSS 文本帧，格式：

```
>> 42["room_info"]
<< 4310["success",{"reconnect_key":"..."}]
<< 42["be-controlled",{...}]
>> 42["soac",{"type":"answer",...}]
```

**脱敏很重要**：token、csrf、sdp 里的 ICE 公网 IP、设备真实 ID 都可以替换成
`X`。我需要的是**字段结构、顺序、时序**，不是值。

### 第 3 项：官方客户端装出来的本地状态

用来交叉验证凭据到底存在哪、长什么样。

在 Windows 上跑（**只读**，不改任何东西）：

```powershell
# 注册表三处
reg query "HKCU\Software\Netease\GameViewer" /s
reg query "HKCU\Software\Netease\GameViewerServer" /s
reg query "HKLM\SOFTWARE\Netease\GameViewerSetup" /s

# 数据目录
dir /s "%APPDATA%\Netease"
dir /s "%LOCALAPPDATA%\Netease"
dir /s "%LOCALAPPDATA%\GameViewer"
dir /s "C:\ProgramData\Netease"
```

**这些路径是从二进制里挖出来的**：

```
Software\Netease\GameViewer          ← 主配置
Software\Netease\GameViewerServer    ← 服务端配置
SOFTWARE\Netease\GameViewerSetup     ← 安装信息
%APPDATA%                            ← 常量出现在 setup 路径解析代码附近
%LOCALAPPDATA%
global_config.ini                    ← 配置文件名字
```

**注意**：这些是我从字符串推断的，没实测过。你跑完把输出贴给我，我就能确认。
如果某个路径不存在，说明凭据在别处，我们再找。

**千万别做的事**：不要贴任何看起来像密码/密钥的值。看到 `keybag` / `shkeybag` /
`password` 字段直接把值打码。

### 第 4 项（可选）：官方客户端版本信息

```powershell
# 确认客户端版本和我分析的一致
(Get-Item "C:\Program Files\Netease\UU远程\GameViewer.exe").VersionInfo.FileVersion
```

我的分析基于 `4.45.0.4318`。如果你装的是别的版本，字段号可能变。

---

## 二、有账号我能干什么

分五层，从"立刻能验证"到"需要你配合"。

### 第 1 层：验证签名算法（5 分钟，只需 token）

调一次 `POST /api/v1/login/check_uu_token`。

```
HTTP 200 → 签名完全正确，头集合正确，密钥正确
HTTP 401/422 → token 过期（算法对，凭据废）
HTTP 403 → 签名算错了，我需要重新看拼接顺序
```

这是最重要的单点验证。目前 `sign_check` 只验证了"我算出来的和 Python 算出来的
一样"，**没验证过服务端认不认**。这一次调用就能定生死。

### 第 2 层：走通设备注册与建房（30 分钟）

```
POST /api/v1/device          → 我拿到设备注册的真实请求体结构
GET  /api/v1/device/list     → 确认设备上线，能看到 device_id
POST /api/v1/room/create     → 拿到 server + token
```

**这里有个大坑我现在没法回答**：`POST /api/v1/device` 的请求体字段清单。
exe 里只有路径，没有 schema（那是 JSON 不是 protobuf）。必须实测。

有了这一步，我就能写出**完整可用的设备注册代码**，而不是现在这样只透传 payload。

### 第 3 层：连上信令（1 小时）

有了 `server` + `token`，我能真连 `wss://`。

**需要你先确认一件事**：`ws_client` 现在只支持明文 `ws://`。要连 `wss://` 我要么

- A. 加 TLS 层（rustls 或 openssl）
- B. 你先用 mitmproxy 做 TLS 终结，我连 mitmproxy 的明文端口

选 B 更快，而且顺便就把抓包做了。

连上之后能验证的：

```
0{...} 里的 pingInterval / pingTimeout 是不是真的 25000/20000
42<id>["room_info"] 的 ack 到底回什么
bmsg_push 什么时候推、推什么
be-controlled 的 app_data 到底是不是 ConnectOptions
reconnect_key 多久刷一次
```

### 第 4 层：收到 offer 并回 answer（需要主控端配合）

**这一步是整个项目最关键的未知数。**

我完全不知道：

- host 是**等** controller 发 offer，还是 host **主动**发 offer
- `soac` 里 `type: "offer"` 出现时，`client_id` / `ice_id` 怎么对应
- answer 的 SDP 里 m-line 顺序要不要和 offer 一致
- `gzip_sdp` 到底是 gzip(sdp) 还是独立的另一份

有了账号 + 抓包，这些全部能实测。

**具体要怎么做**：你在一台 Windows 上装官方客户端，登录同一个账号，然后尝试
连接我的 Linux 主机。我这边会打印收到的每一帧。

第一次大概率失败（SDP 不匹配 / ICE 不通 / 证书不对），但**失败信息本身就是答
案**——我能看到官方客户端期待什么。

### 第 5 层：控制面完整打通（需要反复试）

DataChannel 建起来之后，protobuf 这层我已有完整 schema，理论上能直接对。但：

- `seq` 是每条消息 +1 还是每帧 +1
- `timestamp` 是毫秒还是微秒
- 收到 `rpc_request` 后多久必须回，超时会怎样
- `CaptureSettingRequest` 回 `CaptureSettingResponse` 还是回具体的子响应

这些靠"发一条看对方反应"能快速试出来。有账号的话，一轮 iter 大概 10 分钟。

---

## 三、不能干什么（把边界说清楚）

有账号也做不到的：

1. **不能绕过设备数量限制**。账号能绑几台设备是服务端说了算。
2. **不能伪造别人的设备**。`device_id` 和服务端记录绑定。
3. **不能解密媒体流**。DTLS-SRTP 的密钥是 P2P 协商的，服务端也没有。
4. **不能长期免登录**。token 会过期，过期就得重扫。
5. **不能替代真实客户端做"被控"以外的角色**。官方客户端有完整性校验，我这个
   是自己实现的，做被控端没问题，冒充官方主控端会撞校验。

---

## 四、最省事的路径（按顺序做）

```
第 1 步  你扫个码，我拿到 token          → 5 分钟
第 2 步  我调 check_uu_token 验证签名    → 1 分钟，立刻知道算法对不对
第 3 步  我调 device + room/create       → 拿到 server/token，同时反推请求体结构
第 4 步  你开 mitmproxy，抓一次完整会话   → 拿到全部时序
第 5 步  我按抓包补齐状态机              → 协议层完工
第 6 步  补 TLS，接真 wss                → 能脱离 mitmproxy 跑
```

**第 2 步是分水岭**。如果 `check_uu_token` 返回 200，说明前面所有工作都是对的，
后面只是补细节。如果返回 403，说明签名拼接有细节错了（可能是 header 顺序、可能
是 body 处理、可能是 path 要不要带 query），我需要重新对着反汇编看。

---

## 五、怎么找我确认字段

如果你已经装了官方客户端但不确定哪个文件有用，跑这个：

```powershell
# 找出所有最近修改过的、名称像配置的文件
Get-ChildItem -Path "$env:APPDATA","$env:LOCALAPPDATA","C:\ProgramData" `
  -Recurse -Include *.json,*.ini,*.cfg,*.dat,*.db -ErrorAction SilentlyContinue |
  Where-Object { $_.LastWriteTime -gt (Get-Date).AddDays(-7) } |
  Select-Object FullName, Length, LastWriteTime |
  Sort-Object LastWriteTime -Descending |
  Format-Table -AutoSize
```

把**文件列表**（不是内容）发我，我就能判断哪些值得看。

如果要看内容，先这样筛：

```powershell
# 看结构不看值：只打印 JSON 的 key
Get-Content "某个文件.json" -Raw | ConvertFrom-Json | Get-Member -MemberType NoteProperty | Select-Object Name
```

或者对 ini：

```powershell
# 只打印节名和键名，值全部打码
Get-Content "global_config.ini" | ForEach-Object {
  if ($_ -match '^\s*\[(.+)\]') { "[$($Matches[1])]" }
  elseif ($_ -match '^\s*([^=]+)=') { "$($Matches[1].Trim())=<redacted>" }
}
```

**永远不要直接贴原始内容**，先跑上面这两个筛子。

---

## 六、我已经能确认的事实（不用你再验证）

从二进制里已经确定的，可以直接用：

```
签名密钥        alWiSzXZTLu3WfFnw13uBru3
插件签名密钥    oK5zJ1zJ5cF6kK9eW2jM8oF3
签名算法        hex(HMAC-SHA256(key, METHOD + path + params + body))
params          x-param-* 小写、排除 x-param-sign、跳过空值、字典序、& 连接
API 基址        https://api.nrd.nie.163.com
登录端点        /api/v1/login/by_qrcode | by_mobile | by_uu_token | by_mumu_token
                /api/v1/login/check_uu_token | check_mumu_token
二维码          /api/v1/qrcode/gen/login | /api/v1/qrcode/login/status
设备            /api/v1/device | /api/v1/device/list | /api/v1/device/controllable
房间            /api/v1/room/create | /api/v1/room/join/by_device
信令            wss://<server>/socket.io/?transport=websocket&EIO=4
                header X-NRD-AUTH
握手            42<id>["room_info"]，ack 43<id>[...]
事件            bmsg_push | be-controlled | soac | released | left | leave
媒体            Google libwebrtc，H.264/H.265 + Opus，DTLS-SRTP + SCTP
注册表          Software\Netease\GameViewer (推断)
                Software\Netease\GameViewerServer
                SOFTWARE\Netease\GameViewerSetup
数据目录        %APPDATA% / %LOCALAPPDATA% + GameViewer (推断)
配置文件名      global_config.ini (推断)
```

"推断"标记的三项是从字符串位置推出来的，没实测。你跑一下第 3 项就能确认。

---

## 七、最坏情况

如果 `check_uu_token` 返回 403，说明签名有细节错。这时候我需要你配合做一件事：

**在 Windows 上用 Frida 挂进 `GameViewerServer.exe`，hook
`service_util.cpp` 里的 `request_sign`，把 string_to_sign 和最终 sign 都打出来。**

Frida 脚本大概长这样（等我确认了函数偏移再给准确的）：

```javascript
// 找到 request_sign 函数（0x140ba6057 附近）
var base = Module.findBaseAddress("GameViewerServer.exe");
var signFn = base.add(0xba6000);   // 需要精确偏移

Interceptor.attach(signFn, {
  onEnter: function (args) {
    // 三个 string 参数：method / path / body
    console.log("arg0:", args[0]);
    console.log("arg1:", args[1]);
  },
  onLeave: function (retval) {
    console.log("sign:", retval);
  }
});
```

有了这个，签名算法就 100% 确定了，不用再猜。

---

## 八、所以，现在给我什么

**最小可用：扫个码。**

我这边可以直接出二维码，你手机扫一下。或者你嫌弃扫码麻烦，给我一串
`uu_token` 也行。

**如果只能给一样东西，给抓包。** 抓包比 token 值钱——token 会过期，抓包里的
时序信息是永久的。

我这边随时可以开始出码。你说一声就行。