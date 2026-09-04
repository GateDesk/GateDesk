# GateDesk 本地 HTTP API 文档

> 版本：1.2（2026-09-03）
> 适用：GateDesk 客户端（Sciter 版，含内嵌 HTTP API 的构建）
> 维护约定：**修改源码 `gatedesk/src/http_api.rs` 后必须同步更新本文档**（新增/变更接口、参数、响应、错误码，并在变更记录表加行）。

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
- 跨域：所有响应含 `Access-Control-Allow-Origin: *`；预检 `OPTIONS` 返回 204，声明允许 `GET, POST, OPTIONS` 与请求头 `Authorization, Content-Type`（后者用于网页 POST JSON body）
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
{"online":true,"in_session":false,"peer_id":null}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| online | bool | GateDesk 是否已注册/上线（daemon 连接状态） |
| in_session | bool | 是否存在本 API 发起的存活会话（已关闭的窗口会自动清除） |
| peer_id | string\|null | 该会话的目标设备 ID；无会话时为 `null` |

### 6.5 设置本机连接密码（受控端）

```
POST /password
```

请求体：`{"password":"..."}`（JSON）

将本机 GateDesk 的密码设为指定值，运维端即可凭「本机 ID + 该密码」发起连接。每次会话结束后应换一个新随机值调用本接口以轮换凭据。

**PoC 说明**：原生「临时密码」只能自动轮换、不支持指定值，故本接口写入的是永久密码通道（`set_permanent_password_with_result`）。

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

## 7. 错误码

| HTTP | 触发条件 |
|------|---------|
| 200 | 成功 |
| 204 | OPTIONS 预检成功 |
| 400 | 参数缺失或非法（如 `id` 为空/超长、`/password` 密码为空/超长、`/voice` 的 `enabled` 非 true/false） |
| 401 | 未携带 token、token 错误、或未配置 `api-token`（响应体区分原因） |
| 404 | 未知路径 |
| 405 | 方法不允许 |
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

## 9. 开发与测试

### 9.1 开发期编译（快）

```powershell
cd gatedesk
$env:VCPKG_ROOT="C:\Users\deepblue\vcpkg"
$env:LIBCLANG_PATH="C:\Users\deepblue\LLVM\bin"
cargo build --features inline -j 16        # debug，增量约 0.7s
Copy-Item "$env:USERPROFILE\Downloads\rustdesk_build\sciter.dll" target\debug\  # 首次
```

### 9.2 发布编译

```powershell
cargo build --release --features inline -j 16
Copy-Item "$env:USERPROFILE\Downloads\rustdesk_build\sciter.dll" target\release\
```

### 9.3 冒烟测试命令

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
```

## 10. 变更记录

| 日期 | 版本 | 变更 |
|------|------|------|
| 2026-09-03 | 1.0 | 初始版本：`GET /id`、`POST /connect` |
| 2026-09-03 | 1.1 | 新增 `POST /disconnect`（仅断开本 API 发起的远程会话，不关主界面/不关机） |
| 2026-09-03 | 1.2 | 新增 `GET /status`、`POST /password`、`POST /voice`；`/disconnect` 支持 macOS/Linux（SIGTERM）；预检允许 `Content-Type` 请求头 |


