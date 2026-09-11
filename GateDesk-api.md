# GateDesk 本地 HTTP API 文档

> 版本：1.7（2026-09-10）
> 适用：GateDesk 客户端（Sciter 版，含内嵌 HTTP API 的构建）
> 维护约定：**修改源码 `gatedesk/src/http_api.rs` 后必须同步更新本文档**（新增/变更接口、参数、响应、错误码，并在变更记录表加行）；如变更 `GateDesk2.toml` 的配置约定、路径或键语义，需同步更新「附录 A：GateDesk2.toml 配置文件」。

---

## 1. 概述

GateDesk 客户端进程内嵌一个仅限本机访问的 HTTP 服务，供**本机浏览器网页 / 业务系统**获取设备 ID 或触发远程连接，用于将 GateDesk 设备与业务系统关联。

- 监听地址：`http://127.0.0.1:21120`（**仅绑定 127.0.0.1，绝不监听 0.0.0.0**，局域网不可访问）
- 进程形态：GateDesk 主界面进程 或 `--server` 服务进程（任一先启动者占用端口，后者自动禁用）
- 实现文件：`gatedesk/src/http_api.rs`
- 依赖：`tiny_http`（Cargo.toml `[dependencies]`）

## 2. 安全要求（设计硬约束）

| 要求 | 说明 |
|------|------|
| 本机绑定 | 只监听 `127.0.0.1`，禁止改为 `0.0.0.0` |
| Token 校验 | 所有接口必须携带 token，否则 401 |
| Token 存储 | 配置文件 `%AppData%\GateDesk\config\GateDesk2.toml` 的 `[options]` 段，键名 `api-token` |
| Host 校验 | 仅接受 `Host: localhost / 127.0.0.1`，否则 403 `{"error":"host not allowed"}`（防 DNS Rebinding，v1.7） |
| CORS 收紧 | `Access-Control-Allow-Origin` 仅对受信来源回显：本机来源（localhost / 127.0.0.1，任意端口）或 `[options] api-cors-origin` 列出的来源；非受信来源 403 `{"error":"origin not allowed"}` 且不带 CORS 头（v1.7） |
| 请求体上限 | `Content-Length` 超过 1024 字节 → 413（v1.7） |

> ⚠️ Token 必须写在 **GateDesk2.toml**（不是 GateDesk.toml）。`ui_interface::get_option` 链路只读 CONFIG2 的 `[options]`。
> ⚠️ 修改配置后需**重启 GateDesk** 生效（配置为启动时缓存）。
> ⚠️ GateDesk2.toml 已有 `[options]` 表头时，直接在其中追加一行，**不要新增重复 `[options]` 表头**（会导致 TOML 解析失败）。

## 3. Token 配置方法

### 3.1 手工编辑配置文件

文件：`%AppData%\GateDesk\config\GateDesk2.toml`

```toml
[options]
# ... 已有内容 ...
api-token = '你的随机token'     # 新增这行
```

### 3.2 安装版（可选，需已安装+管理员）

```powershell
.\gatedesk.exe --option api-token <token> | Out-String
```

## 4. 鉴权方式（二选一，所有接口通用）

**方式 A：请求头**

```
Authorization: Bearer <token>
```

**方式 B：URL 查询参数**

```
?token=<token>
```

## 5. 通用约定

- 响应格式：`application/json; charset=utf-8`
- 跨域（v1.7）：不再无条件回显 `Access-Control-Allow-Origin: *`。对携带 `Origin` 头的浏览器请求，仅当来源受信（localhost / 127.0.0.1 任意端口，或 `[options] api-cors-origin` 配置的来源，逗号分隔）时才回显该来源；无 `Origin` 的非浏览器请求（curl 等）照常响应但不带 CORS 头；预检 `OPTIONS` 仅对受信来源返回 204 并声明 `GET, POST, OPTIONS` 与 `Authorization, Content-Type`，非受信来源直接 403。
- 查询参数支持 URL 百分号编码（浏览器 `fetch` 自动编码后服务端正确解码）

## 6. 接口列表

### 6.1 获取本机 ID

```
GET /id
```

获取当前设备的 GateDesk ID（与 `gatedesk.exe --get-id` 一致）。

**请求示例**

```powershell
curl "http://127.0.0.1:21120/id?token=<token>"
```

**成功响应（200）**

```json
{"id":"477091630"}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| id | string | 本机 GateDesk ID |

### 6.2 触发连接指定设备

```
POST /connect?id=<目标ID>[&password=<密码>][&relay=true]
```

弹出 GateDesk 远程连接窗口连接指定 ID 的设备（等价于命令行 `gatedesk.exe --connect <id> <password>`）。HTTP 层仅触发，实际连接流程由客户端自身执行。

**参数**

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| id | 是 | string | 目标设备 ID（1~128 字符） |
| password | 否 | string | 连接密码；省略则弹出窗口等待手动输入 |
| relay | 否 | bool | `true` 时强制走中继服务器 |

**请求示例**

```powershell
# 仅连接
curl -X POST "http://127.0.0.1:21120/connect?token=<token>&id=555555555"

# 带密码
curl -X POST "http://127.0.0.1:21120/connect?token=<token>&id=555555555&password=mypass"

# 强制中继
curl -X POST "http://127.0.0.1:21120/connect?token=<token>&id=555555555&relay=true"
```

**成功响应（200）**

```json
{"ok":true,"id":"555555555"}
```

**浏览器网页 JS 示例**

```javascript
// 获取本机 ID
const res = await fetch('http://127.0.0.1:21120/id?token=' + TOKEN);
const { id } = await res.json();

// 触发连接
await fetch('http://127.0.0.1:21120/connect?token=' + TOKEN +
  '&id=' + encodeURIComponent('555555555') +
  '&password=' + encodeURIComponent('mypass'), { method: 'POST' });

// 断开由本 API 发起的远程会话（断开远程桌面，不影响 GateDesk 主界面）
await fetch('http://127.0.0.1:21120/disconnect?token=' + TOKEN, { method: 'POST' });
```

### 6.3 断开本 API 发起的远程会话

```
POST /disconnect
```

关闭/断开由 `POST /connect` 发起的远程桌面会话（结束对应连接进程），**仅作用于本 API 记录发起的会话**：

- 不会关闭 GateDesk 主界面进程
- 不会执行任何系统级操作（**绝不关机/注销/重启**）
- 通过其他方式（如命令行 `--connect`）手动打开的会话不受影响

无参数。

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/disconnect?token=<token>"
```

**成功响应（200）**

```json
{"ok":true,"closed":1}
```

`closed` 为实际被断开（结束）的会话进程数；`0` 表示当前无由 API 发起的存活会话。

> 注：跨平台断开已实现——Windows 用 `taskkill /T /F` 结束进程树，macOS/Linux 对该会话进程发 `SIGTERM`。仅结束本 API 记录发起的会话进程，不影响 GateDesk 主进程。

### 6.4 获取客户端状态

```
GET /status
```

返回 GateDesk 是否已上线，以及是否存在由本 API 发起的存活远程会话。网页轮询本接口驱动「等待控制 / 控制中 / 已结束」状态展示。

**成功响应（200）**

```json
{"online":true,"in_session":false,"peer_id":null,"assistable":false}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| online | bool | GateDesk 是否已注册/上线（daemon 连接状态） |
| in_session | bool | 是否存在本 API 发起的存活会话（已关闭的窗口会自动清除） |
| peer_id | string\|null | 该会话的目标设备 ID；无会话时为 `null` |
| assistable | bool | 本机是否已设置连接凭据（永久密码）＝客户已授权「可被协助」（v1.7，对应企业版 §5.1 授权方式 A） |

### 6.5 设置本机连接密码（受控端）

```
POST /password
```

请求体：`{"password":"..."}`（JSON）

将本机 GateDesk 的密码设为指定值，运维端即可凭「本机 ID + 该密码」发起连接。每次会话结束后应换一个新随机值调用本接口以轮换凭据。

**PoC 说明**：原生「临时密码」只能自动轮换、不支持指定值，故本接口写入的是永久密码通道（`set_permanent_password_with_result`），凭据失效由调用方会话后轮换实现（保持 GateDesk 原本密码语义）。

**审计（v1.7）**：调用成功即视为客户对本机执行了授权动作（方式 A——生成连接凭据），写入操作级审计事件 `auth.grant`（失败写 `auth.grant` result=`err`）；连同 `connect.start` / `connect.close` / `voice.on` / `voice.off` 等事件，见 §6.7。

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/password?token=<token>" -H "Content-Type: application/json" -d '{\"password\":\"mypass\"}'
```

**成功响应（200）**

```json
{"ok":true}
```

### 6.6 语音开关

```
POST /voice
```

请求体：`{"enabled":true|false}`（JSON）

启用/关闭语音输入。**PoC 说明**：精确的会话级语音开关需进程内会话句柄，本接口以全局 `audio-input` 配置近似（该配置变更会触发音频服务重启）。

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/voice?token=<token>" -H "Content-Type: application/json" -d '{\"enabled\":true}'
```

**成功响应（200）**

```json
{"ok":true,"enabled":true}
```

### 6.7 操作级审计（桌面端内部机制，非本 API 端点）

自 v1.7 起，桌面端对本 API 引发的关键动作以及会话内高风险操作统一做操作级审计（企业版设计 §8）：

- 统一载荷：`{action, actor, device_id, session_id, ts, result, extra}`（JSON Lines）。
- 本地兜底：始终追加写入日志目录下的 `audit.log`（每行一条 JSON），不阻塞主流程，断网上报也不丢记录。
- 转发上报：若 `[options] audit-server-url` 已配置（如 GateDeskWeb 的 `http://<ip>:3000/api/audit`），事件以异步 POST 转发到该端点；失败静默（本地已兜底）。
- 由本 API 引发的动作：`/password` 成功/失败 → `auth.grant`；`/connect` → `connect.start`（ok/err）；`/disconnect` 实际关闭会话 → `connect.close`；`/voice` → `voice.on` / `voice.off`。
- 会话内操作（控制端会话窗口/受控端执行点）也产生事件：`record.start/stop`、`remote.restart`、`privacy.on/off`、`block_input.on/off`、`voice.on/off`。

> 说明：审计事件由桌面端直接上报，不经本地 HTTP API 转发；本小节仅为集成方说明事件来源与排查 `audit.log` 提供索引。

## 7. 错误码

| HTTP | 触发条件 |
|------|---------|
| 200 | 成功 |
| 204 | OPTIONS 预检成功（仅受信来源） |
| 400 | 参数缺失或非法（如 `id` 为空/超长、`/password` 密码为空/超长、`/voice` 的 `enabled` 非 true/false） |
| 401 | 未携带 token、token 错误、或未配置 `api-token`（响应体区分原因） |
| 403 | Host 头非 localhost/127.0.0.1，或 `Origin` 非受信来源（v1.7） |
| 404 | 未知路径 |
| 405 | 方法不允许 |
| 413 | 请求体 `Content-Length` 超过 1024 字节（v1.7） |
| 500 | 服务端失败（如无法启动连接进程、设置密码失败） |

**401 响应体区分**

```json
{"error":"api-token not configured"}   // 未配置 token
{"error":"unauthorized"}               // token 缺失或不匹配
```

## 8. 业务接入建议流程

```
设备本机打开业务网页
   └─ fetch /id（本机 127.0.0.1:21120）→ 拿到本机 GateDesk ID
   └─ 网页将 { 业务设备编号, GateDesk ID } 上报业务系统 → 建立映射
管理端/业务系统
   └─ 需要远程某设备时 → 目标设备上 POST /connect?id=<ID> → 弹出连接窗口
   └─ 远程结束 → POST /disconnect（仅断开该远程会话，不关主界面/不关机）
```

## 9. 冒烟测试命令

```powershell
# 正确 token → 200
curl "http://127.0.0.1:21120/id?token=<token>"
# 无 token → 401
curl -s -o NUL -w "%{http_code}" http://127.0.0.1:21120/id
# 连接 → 200 并弹出窗口（窗口标题=目标ID 即成功）
curl -X POST "http://127.0.0.1:21120/connect?token=<token>&id=<ID>"
# 断开 API 发起的会话 → 200，主进程应存活
curl -X POST "http://127.0.0.1:21120/disconnect?token=<token>"
# 绑定检查（必须只出现 127.0.0.1:21120）
netstat -ano | findstr 21120
# 伪造 Host 头 → 403（DNS rebinding 防护，v1.7）
curl -s -o NUL -w "%{http_code}" -H "Host: evil.example" "http://127.0.0.1:21120/id?token=<token>"
# 伪造跨域 Origin → 403（v1.7）
curl -s -o NUL -w "%{http_code}" -H "Origin: http://evil.example" "http://127.0.0.1:21120/id?token=<token>"
```

## 10. 变更记录

| 日期 | 版本 | 变更 |
|------|------|------|
| 2026-09-10 | 1.7 | 本地接口加固：Host 校验（仅 localhost/127.0.0.1，防 DNS Rebinding）、CORS 收紧（`Access-Control-Allow-Origin` 仅对受信来源回显，支持 `[options] api-cors-origin`）、请求体 1024B 上限（413）；`GET /status` 新增 `assistable`（本机已授权「可被协助」）；操作级审计：`/password→auth.grant`、`/connect→connect.start`、`/disconnect→connect.close`、`/voice→voice.on/off`，事件写本地 `audit.log`（JSON Lines）并可选转发 `[options] audit-server-url`（§6.7）；Unix 下启动时对配置文件 chmod 0600 |
| 2026-09-07 | 1.6 | 收敛附录 A 的实现细节，改为面向二次开发的配置约定说明，保留路径、键定义、使用方式与维护纪律，避免过度暴露内部实现 |
| 2026-09-07 | 1.5 | 细化「附录 A：GateDesk2.toml 配置文件」：补充路径、数据源、缓存、优先级、写入机制、TOML 示例与典型键值说明，便于运维直接维护 |
| 2026-09-07 | 1.4 | 精简文档：移除与 API/配置无关的编译章节（原 9.1 开发期编译、9.2 发布编译），保留冒烟测试；变更记录按版本降序整理 |
| 2026-09-07 | 1.3 | 补充「附录 A：GateDesk2.toml 配置文件」：各平台路径、配置项、数据来源与优先级 |
| 2026-09-03 | 1.2 | 新增 `GET /status`、`POST /password`、`POST /voice`；`/disconnect` 支持 macOS/Linux（SIGTERM）；预检允许 `Content-Type` 请求头 |
| 2026-09-03 | 1.1 | 新增 `POST /disconnect`（仅断开本 API 发起的远程会话，不关主界面/不关机） |
| 2026-09-03 | 1.0 | 初始版本：`GET /id`、`POST /connect` |

---

## 附录 A：GateDesk2.toml 配置文件

HTTP API 的鉴权 token（`api-token`）与语音开关（`audio-input`）等配置，均存放在 **GateDesk2.toml**（即 `Config2`）的 `[options]` 表中。本文档重点说明其位置、结构、数据来源、读取优先级、可维护键值以及常见坑。它是运维 / 部署脚本 / 调试时最重要的配置入口之一。

### A.1 配置文件的作用与版本关系

GateDesk 维护两份配置文件：

| 文件 | 常量 | 作用 |
|------|------|------|
| `GateDesk.toml` | `CONFIG1` | 主配置：设备 ID、密钥对等、核心身份材料 |
| `GateDesk2.toml` | `CONFIG2` | 二级配置：`[options]` 表、socks 凭据、unlock_pin、用户设置与部分运行参数 |


### A.2 配置文件路径（不同平台）

| 平台 | GateDesk2.toml 完整路径 |
|------|------------------------|
| Windows | `%APPDATA%\GateDesk\config\GateDesk2.toml`（通常为 `C:\Users\<用户名>\AppData\Roaming\GateDesk\config\`） |
| macOS | `~/Library/Preferences/com.carriez.GateDesk/GateDesk2.toml` |
| Linux | `$XDG_CONFIG_HOME/GateDesk/GateDesk2.toml`，未设置时回退到 `~/.config/GateDesk/GateDesk2.toml` |
| Android / iOS | 应用沙盒目录（APP_DIR）内 |

如果正在通过脚本部署，建议统一把配置文件路径写成一个“可定位”的变量，并在修改后重启 GateDesk 进程。
- 修改`GateDesk2.toml` 后，重启后生效；

### A.3 典型 `[options]` 配置项说明

下面这些是与 API / 本机配置最相关的键，适合运维/脚本直接检查：

| 键 | 值语义 | 说明 |
|----|--------|------|
| `api-token` | 任意字符串（建议高强度随机） | 本地 HTTP API 的认证令牌。`http_api.rs` 会读取 `get_option("api-token")`；空值表示“未配置”，所有接口返回 `401` 且响应体为 `{"error":"api-token not configured"}`。一般由脚本或手工编辑配置文件写入。 |
| `audit-server-url` | 审计服务端上报地址（v1.7，可空） | 操作级审计事件的转发端点（如 GateDeskWeb 的 `http://127.0.0.1:3000/api/audit`）；为空时仅写本地 `audit.log`。 |
| `api-cors-origin` | 逗号分隔的业务页面来源（v1.7，可空） | 本地 API 在 CORS 收紧策略下额外放行的来源（如 `http://192.168.1.10:3000`），与默认放行的 localhost/127.0.0.1 互补。 |
| `audio-input` | `Y` 表示启用，空字符串或删除表示禁用 | 语音输入总开关。`POST /voice` 会写入此键；值为空时会被 `set_option` 从表里删除。该变更会触发音频服务重启。 |
| `custom-rendezvous-server` | 服务端地址字符串 | 自定义 rendezvous 服务器。配置可能影响连接路由和注册流程。 |
| `relay-server` | relay 地址字符串 | 中继服务器配置，通常用于穿透/中继场景。 |
| `api-server` | API 服务器地址 | 与 GateDesk 业务后台或网关通信相关。 |
| `stop-service` | `Y`/空字符串 | 控制后端服务运行状态。 |
| `disable-udp` | `Y`/`N` 或空 | UDP 能力开关。 |
| `whitelist` / `id-whitelist` | 条件字符串 | 访问控制相关配置，按实际应用判断。 |

> 说明：`[options]` 里并非只有这几个键。它几乎包含 GateDesk 的大部分用户设置，因为 `Config2.options` 是统一键值表；如果需要看完整清单，可参考 `config.rs` 中的 `default_options()` 与 `keys.rs`。

### A.6 文件结构示例与 TOML 写法

最常见的写法示例如下：

```toml
[options]
# ==============================
# 本机 HTTP API 鉴权
# ==============================
api-token = 'replace_with_strong_random_token'

# ==============================
# 操作级审计转发地址（可空，v1.7）
# 为空则仅写本地 audit.log；例：http://127.0.0.1:3000/api/audit
# ==============================
audit-server-url = ''

# ==============================
# CORS 额外放行的业务页面来源（逗号分隔，可空，v1.7）
# 例：http://192.168.1.10:3000
# ==============================
api-cors-origin = ''

# ==============================
# 语音输入开关
# 'Y' = 启用, 空字符串 = 禁用
# ==============================
audio-input = 'Y'

# ==============================
# 远程连接 / 中继相关
# ==============================
custom-rendezvous-server = 'rs-ny.rustdesk.com'
relay-server = '127.0.0.1:21117'
api-server = ''

# ==============================
# 服务状态相关
# ==============================
stop-service = ''
disable-udp = ''
allow-always-software-render = ''

# ==============================
# 访问控制 / 安全相关
# ==============================
whitelist = ''
id-whitelist = ''

# ==============================
# 其他常见配置项（按实际需要写）
# ==============================
enable-lan-discovery = ''
allow-insecure-tls-fallback = ''
```




