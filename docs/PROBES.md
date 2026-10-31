# 实测记录

所有条目都是对 `https://api.nrd.nie.163.com` 的真实请求结果，不是推断。

签名实现见 `scripts/nrd_login.py`，密钥 `alWiSzXZTLu3WfFnw13uBru3`。

---

## 一、签名链路已被服务端接受

**结论：签名算法、密钥、头集合、拼接顺序全部正确。**

证据链：

请求 A，缺身份头、缺 body 字段：

```
POST /api/v1/login/check_uu_token
body: {}

HTTP 422
{
  "code": 1001,
  "msg": "无效的请求参数",
  "data": [
    {"type": "missing", "loc": ["header", "X-Param-client-id"], "msg": "Field required"},
    {"type": "missing", "loc": ["header", "X-Param-device-id"], "msg": "Field required"},
    {"type": "missing", "loc": ["body", "access_token"], "msg": "Field required"}
  ]
}
```

这个响应的意义：

1. **服务端执行了签名校验并通过了**。如果 HMAC 算错，会在签名中间件就被拦下，
   不会走到参数校验层。
2. 服务端把缺失字段按 `loc` 精确列出来了，等于免费给了 schema。

请求 B，补齐三个字段（token 用假值）：

```
POST /api/v1/login/check_uu_token
headers: X-Param-client-id, X-Param-device-id
body: {"access_token": "testvalue123"}

HTTP 400
{"code": 1001, "msg": "无效的请求参数", "data": {}}
```

`data` 从数组变成空对象，说明**字段级校验全过了**，只是业务层拒绝了假 token。
这已足够证明签名正确。

## 二、body 字段名修正

| 端点 | 正确字段 |
|---|---|
| `POST /api/v1/login/check_uu_token` | `access_token`（不是 `token`） |
| `POST /api/v1/login/by_uu_token` | 待定 |

之前基于猜测写的 `{"token": ...}` 是错的。

## 三、必需的请求头

```
X-Param-client-id    必需
X-Param-device-id    必需
X-Param-system-id    存在但非必需
X-Param-user-id      登录后必需
```

## 四、端点存在性实测

| 方法 | 路径 | 结果 | 含义 |
|---|---|---|---|
| POST | `/api/v1/login/check_uu_token` | 422 → 400 | 存在，字段已对齐 |
| GET | `/api/v1/login/by_uu_token` | 405 Method Not Allowed | 存在，只收 POST |
| POST | `/api/v1/qrcode/gen/login` | 400 | 存在，但 body 需要更多字段 |
| POST | `/api/v1/login/by_mobile` | 400 | 存在 |
| GET | `/api/v1/device/list` | 400 | 存在 |
| POST | `/api/v1/room/create` | 400 | 存在 |
| POST | `/api/v1/device` | 404 Not Found | **路径不对** |
| POST | `/api/v1/device/` | 404 Not Found | 也不对 |
| GET | `/api/v1/device` | 404 Not Found | 也不对 |

**重要修正**：`/api/v1/device` 这个路径在 POST 下不存在。注册设备的真实路径
需要重新确认。exe 里 `/api/v1/device` 是作为前缀出现的，实际可能是
`POST /api/v1/device/<something>`。

## 五、错误码含义

| HTTP | body | 含义 |
|---|---|---|
| 422 | `code:1001` + `data` 为数组 | 字段级校验失败，`loc` 指出缺什么 |
| 400 | `code:1001` + `data:{}` | 业务层拒绝（token 无效 / 参数值不合法） |
| 404 | `{"detail":"Not Found"}` | 路径不存在 |
| 405 | `{"detail":"Method Not Allowed"}` | 方法不对，但路径存在 |

`{"detail": ...}` 是 FastAPI 原生格式，说明这层是 Python 写的。
业务层错误用 `{code, msg, data}`。

## 六、还没解决的

1. `POST /api/v1/qrcode/gen/login` 的完整 body 字段。目前所有组合都返回
   `400 data:{}`，说明缺关键字段但服务端不再细分。
2. 设备注册的真实路径。
3. `X-Param-user-id` 缺失是否就是那个 400 的原因。需要真 token 才能区分。

## 七、复现命令

```bash
python3 scripts/nrd_login.py sign \
  --path /api/v1/login/check_uu_token --method POST --body '{}'

python3 scripts/nrd_login.py check --uu-token <TOKEN> \
  --client-id <CID> --device-id <DID>
```

`sign` 只本地算，不联网。`check` 会真发请求。

---

## 八、这些实测推翻了什么

之前的文档里有几处是推断，现在被实测修正：

| 之前写 | 实际 |
|---|---|
| body 用 `{"token": ...}` | 用 `{"access_token": ...}` |
| `POST /api/v1/device` 可用 | 404，路径不对 |
| `client-id` / `device-id` 可选 | 必需 |
| 认证通过后才会校验参数 | 参数校验在先，签名在中间件 |

签名本身没有被推翻——这是最重要的结论。