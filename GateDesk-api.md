# GateDesk 本地 HTTP API 文档

> 版本：1.27（2026-09-24）
> 适用：GateDesk 客户端（Sciter 版，含内嵌 HTTP API 的构建）
> 维护约定：**修改源码 `GateDesk/src/http_api.rs` 后必须同步更新本文档**（新增/变更接口、参数、响应、错误码，并在变更记录表加行）；如变更 `GateDesk2.toml` 的配置约定、路径或键语义，需同步更新「附录 A：GateDesk2.toml 配置文件」。

---

## 1. 概述

GateDesk 客户端进程内嵌一个仅限本机访问的 HTTP 服务，供**本机浏览器网页 / 业务系统**获取设备 ID 或触发远程连接，用于将 GateDesk 设备与业务系统关联。

- 监听地址：`http://127.0.0.1:21120`（**仅绑定 127.0.0.1，绝不监听 0.0.0.0**，局域网不可访问）
- 进程形态：GateDesk 主界面进程 或 `--server` 服务进程（任一先启动者占用端口，后者自动禁用）
- 实现文件：`GateDesk/src/http_api.rs`
- 依赖：`tiny_http`（Cargo.toml `[dependencies]`）

接入前先确认本机在这段业务里是哪一端：

| 类别 | 本机角色 | 端点 | 状态存放在 |
|------|---------|------|-----------|
| 主动连接 | 控制端 | `/id`、`/connect`、`/disconnect`、`/request-permission`、`/status` | 本进程（含本 API 启动的会话窗口；权限请求经进程内通道交给会话进程，§6.9） |
| 本机凭据 | 被控端 | `/password` | 本进程（§6.5）。它设的是本机自己的连接密码，本机是要被连进来的那一端 |
| 会话与批准 | 被控端 | `/sessions`、`/approve`、`/control`、`/permission`、`/terminate`、`/dismiss` | 连接管理器进程（§6.7）；本 API 通过进程内通道与其通信 |
| 出站通知 | 被控端 → 平台 | 事件通知（非端点，§6.10） | 连接层 |

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
- 「本机」指调用本接口的那台机器。控制端的小节里它指发起连接的一方，被控端的小节里指被连进来的一方；同一台机器可能同时是两者。
- 会话接口寻址（v1.8）：`/sessions` 之外的会话类接口以 `id`（整数，取自 `/sessions`）指定目标，不使用设备 ID。同一对端可能同时存在多个会话（远程桌面 + 文件传输），只凭设备 ID 无法区分。

## 6. 接口列表

> `POST /voice`（原 §6.6，批量开关本机所有会话的 `audio` 权限）已在 v1.18 删除，编号留空，其后各节编号不变；按会话开关音频用 §6.7.4。

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
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
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

> 注：跨平台断开已实现。Windows 用 `taskkill /T /F` 结束进程树，macOS/Linux 对该会话进程发 `SIGTERM`。仅结束本 API 记录发起的会话进程，不影响 GateDesk 主进程。

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

**审计（v1.7）**：调用成功即视为客户对本机执行了授权动作（方式 A：生成连接凭据），写入操作级审计事件 `auth.grant`（失败写 `auth.grant` result=`err`）。另有 `connect.start` / `connect.close` 等事件，见 §6.8。

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/password?token=<token>" -H "Content-Type: application/json" -d '{\"password\":\"mypass\"}'
```

**成功响应（200）**

```json
{"ok":true}
```

### 6.7 会话与批准（受控端）

本节接口管的是本机作为被控端的会话。这些状态由连接管理器进程持有，本 API 通过进程内通道（`_cm`）让它执行动作，与人在受控端界面上点按钮是同一批动作。所以接口和界面不会出现两种状态：用接口批准后，界面上对应的提示会同步消失。

| 接口 | 作用 | 前提 | 成功响应 |
|------|--------|------|---------|
| `GET /sessions` | 查看当前有哪些对端、各自能做什么、有没有等到本机应答的申请 | 连接管理器在跑 | `{"ok":true,"sessions":[…]}` |
| `POST /approve` | 放行正在等待接入的对端，或拒绝它 | 对端正在等批准 | `{"ok":true,"result":{"peer_id":…}}` |
| `POST /control` | 应答对端发来的一项申请，键鼠控制或某一项命名权限（`name` 必填，与在途那项一致） | 有 `pending_control` | `{"ok":true,"result":{"peer_id":…,"name":…}}` |
| `POST /permission` | 不经申请，直接开关对端的某一项权限 | 会话已建立 | `{"ok":true,"result":{"peer_id":…}}` |
| `POST /terminate` | 结束会话，与界面上的「断开」一致，对端可重连 | 会话在 | `{"ok":true,"result":{"peer_id":…}}` |
| `POST /dismiss` | 把已结束的会话从列表里清掉，与「关闭」一致 | 会话确已结束 | `{"ok":true,"result":{"peer_id":…}}` |

本节的前提是：接入后默认只读，控制端要操作本机键鼠必须得到本机同意，同意来自界面上的人或来自本节接口。

因此，持有 token 就能代替本机用户批准接入与控制。token 的保管按 §2 执行。

本节的状态变化只能靠轮询发现，本机 API 没有推送通道（`tiny_http` 同步模型）。降低发现延迟的办法见 §6.10 的出站事件通知，需要先在被控机配置 `event-server-url`；没有配置时轮询是唯一方式。

各小节的「什么时候会失败」给的是精确条件，这里只说规律：

- `400`：参数缺失或非法（token 不算在这一类，见下行）。
- `401`：token 缺失、错误，或本机未配置 `api-token`；响应体区分两种原因（§7）。每个接口都要，`GET /sessions` 也不例外。
- `404`：`id` 对应的会话不存在，写错了或已被 `/dismiss` 清掉。
- `409`：会话在，但按当前状态做不了这件事。
- `503`：本机没有连接管理器进程在监听 `_cm`，既没有会话，也没有那个窗口。`GET /sessions` 是例外：对它而言「没有管理器」就是「没有会话」，返回 `{"ok":true,"sessions":[]}`（v1.27）。
- `504`：连接管理器 2 秒没回应。

**超时与重试**：2 秒没回应时，动作可能已经在本机生效。这几个接口都可以安全重试：

| 接口 | 重复调用的结果 |
|------|---------------|
| `/approve` | 对已放行的对端返回 409；对拒绝过的对端仍返回 200，只是再关一次 |
| `/control` | 申请已被应答过则返回 409；`name` 与在途那项不符也返回 409 |
| `/permission` | 把同一个值再写一遍 |
| `/terminate` | 对已结束的会话是空操作，返回 200 |
| `/dismiss` | 条目已清掉则返回 404 |

拿不准就先查 `/sessions`。

对已经结束的会话（`disconnected: true`），只有 `/dismiss` 会因状态不符返回 409；`/approve`、`/control`、`/permission`、`/terminate` 都返回 200，它们的前置条件都不看 `disconnected`。所以 200 不代表会话还活着，判断会话是否存活要看 `/sessions` 的 `disconnected`。

#### 6.7.1 列出当前会话

```
GET /sessions
```

**作用**：返回受控端界面上正显示的内容：每个已接入或正在请求接入的对端、它的权限、是否等待批准、是否有待应答的申请。轮询它就能知道当前该做什么，§8.1 的无人值守流程就是这么做的。

**成功响应（200）**

```json
{
  "ok": true,
  "sessions": [
    {
      "id": 3,
      "authorized": true,
      "disconnected": false,
      "pending_control": false,
      "pending_permission": "",
      "peer_id": "123456789",
      "name": "张三",
      "avatar": "",
      "is_file_transfer": false,
      "is_view_camera": false,
      "is_terminal": false,
      "port_forward": "",
      "keyboard": false,
      "clipboard": false,
      "audio": false,
      "file": false,
      "restart": false,
      "recording": false,
      "block_input": false,
      "privacy_mode": false,
      "from_switch": false,
      "in_voice_call": false,
      "incoming_voice_call": false
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| id | int | 会话标识，其余接口用它寻址 |
| authorized | bool | 是否已放行；`false` 且 `disconnected: false` 即**正在请求接入**（用 §6.7.2 决定） |
| disconnected | bool | 会话是否已结束（仍留在列表里，可用 §6.7.6 清掉） |
| pending_control | bool | 是否有**待应答的申请**（对应界面上的「允许 / 拒绝」提示）。申请有两种，见下一行 |
| pending_permission | string | 待应答的申请要的是哪一项：`clipboard` / `audio` / `file`；**空字符串表示要的是键鼠控制**。`pending_control` 为 `false` 时它无意义 |
| keyboard | bool | 对端当前能不能驱动本机键鼠：权限与会话批准两样都成立才为 `true`，即控制层 `peer_input_enabled()` 的值，与连接管理器界面上键盘那一行是同一个值（v1.14） |
| peer_id | string | 对端设备 ID |
| name | string | 对端名称 |
| other fields | — | 各项权限的当前值，与界面上的开关一一对应 |

**什么时候会失败**：token 缺失或不符 → 401；连接管理器 2 秒没回应 → 504。**连接管理器不在时返回空数组**（v1.27）：本接口问的是「有哪些会话」，而连接管理器在最后一个会话结束时自行退出（`quit_gui`），空闲机器上它本来就不在，此时回 `{"ok":true,"sessions":[]}` 而不是 503——轮询方不必再猜这一轮到底是没会话还是服务没起来。只要它在监听就返回数组：一个会话都没有时是 `[]`，会话刚结束但条目还没清掉时，那些条目仍会出现（`disconnected: true`）。本接口不接受 `id` 参数，因此不会返回 404。

#### 6.7.2 批准或拒绝接入

```
POST /approve
```

**作用**：对正在等待接入（`/sessions` 中 `authorized: false` 且 `disconnected: false`）的对端做决定，`true` 放行，`false` 拒绝并断开。无人值守时这是会话能开始的唯一途径：本机不开 `--ui` 时，那个窗口是唯一在监听对端登录请求的地方，平台不代为应答就只能由现场的人点。

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
| id | 是 | int | 会话标识 |
| accept | 是 | bool | `true` 放行；`false` 拒绝，连接随即结束（与 §6.7.5 是同一个断开动作） |

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/approve?token=<token>" -d "{\"id\":3,\"accept\":true}"
```

**成功响应（200）**

```json
{"ok":true,"result":{"peer_id":"123456789"}}
```

**什么时候会失败**：`id` 或 `accept` 缺失或非法 → 400；会话不存在 → 404；该对端已经放行过 → 409 `the peer is already through the door`。

**审计**：放行 → `login.approve`；拒绝 → `login.deny`（见 §6.8）。

#### 6.7.3 应答控制请求

```
POST /control
```

**作用**：应答对端发来的一项申请，由本机决定。`/sessions` 里 `pending_control: true` 时走这里。两种申请共用这一个入口，`name` 说的就是应答哪一项：

- `name` 为 `keyboard`：对端要的是**键鼠控制**。`accept: true` 交出控制，会话从只读变成可操作；`false` 拒绝，会话保持只读。

  交出控制含两件事：会话控制闸门打开，**以及本机的 `keyboard` 权限一并打开**——与在界面上点键盘开关、或调 §6.7.4 的 `POST /permission {"name":"keyboard"}` 是同一个结果。`GET /sessions` 的 `keyboard` 字段就是这两样的积，只有两样都成立对端才真能敲进键来。v1.25 之前 `accept: true` 只做前一件：若该会话的 `keyboard` 权限此前被关过（人点过开关，或调过 §6.7.4），接受申请后对端仍然一个键也敲不进来，而审计里已经躺着一条 `control.approve`。命名权限（`clipboard` / `audio` / `file`）没有这个问题：那条路在连接管理器侧会替本机用户调一次开关。
- `name` 为 `clipboard` / `audio` / `file`：对端要的是那一项权限。`accept: true` 时本机替本机用户把那项权限打开（与 §6.7.4 是同一批动作），`false` 则什么也不变。

`name` 是**必填**，而且必须与在途那项一致：本接口不再“应答在途的那一项”，因为调用方读的 `/sessions` 可能是旧的，届时一句 `accept: true` 会替本机用户打开一个它没想开的通道。要知道在途的是哪一项，读 `/sessions` 的 `pending_permission`（空字符串=键鼠，对应本接口的 `name: "keyboard"`）——两边名字就照这个对应关系写。

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
| id | 是 | int | 会话标识 |
| name | 是 | string | 应答哪一项：`keyboard` / `clipboard` / `audio` / `file`。与在途申请不符 → 409（v1.24 起必填） |
| accept | 是 | bool | `true` 允许；`false` 拒绝 |

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/control?token=<token>" -d "{\"id\":3,\"name\":\"keyboard\",\"accept\":true}"
```

**成功响应（200）**：`result` 回显应答的那一项，便于调用方核对

```json
{"ok":true,"result":{"peer_id":"123456789","name":"keyboard"}}
```

**超时**：对端的申请有 60 秒时限，无人应答由服务端按拒绝处理，会话保持原样。静默不等于同意。超时记为 `control.timeout`。

**什么时候会失败**：参数缺失或非法 → 400（`name` 缺失或不在四个名字内同样是 400）；会话不存在 → 404；当前没有待应答的申请（`pending_control: false`，可能已被界面点掉或已超时）→ 409 `no control request is pending`；`name` 与在途申请不符 → 409 `the pending request is for clipboard`。

**审计**：允许 → `control.approve`；拒绝 → `control.deny`；超时 → `control.timeout`。`extra` 为 `{peer_id, permission}`，`permission` 空字符串是键鼠，其余为权限名（v1.24 起带这一项；此前只有 `peer_id`，事后分不出那次同意是交了键鼠还是开了剪贴板）。

**本接口不是唯一的决定入口**：键鼠那一项，本机用户在连接管理器界面上点开键盘图标同样是「同意控制」，上游一直就是这个语义，本客户端保持它；关掉图标则是收回控制（§6.7.4）。两条路落到同一处，所以状态和审计不会出现两种说法。

#### 6.7.4 开关权限

```
POST /permission
```

**作用**：不经任何申请，直接开关本机允许对端做的一件事，会话已经建立时用它调整。由对端开口要的那条路在 §6.7.3 与 §6.9。开关的是本机的态度，生效点在连接层，与人在受控端界面上拨同一个开关是同一批动作。

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
| id | 是 | int | 会话标识 |
| name | 是 | string | `keyboard` / `clipboard` / `audio` / `file` |
| enabled | 是 | bool | 目标状态 |

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/permission?token=<token>" -d "{\"id\":3,\"name\":\"clipboard\",\"enabled\":true}"
```

**成功响应（200）**

```json
{"ok":true,"result":{"peer_id":"123456789"}}
```

**什么时候会失败**

| 情况 | HTTP | 响应 |
|------|------|------|
| `id` / `name` / `enabled` 缺失或非法 | 400 | |
| `name` 不在四个名字之内 | 400 | `unknown permission` |
| 会话不存在 | 404 | |
| 权限被运维锁定（`enable-perm-change-in-accept-window = N`）且 `name` 不是 `keyboard` | 409 | `permissions cannot be changed in the accept window` |
| 连接管理器 2 秒没回应 | 504 | |

**边界**

- 只有这四个名字。远程重启、阻止用户输入、隐私模式本客户端不提供；录制会话随会话默认开启，不是可切换项。
- 这里说的是本接口与会话界面之间的边界，不是本机用户本人的边界：`--ui` 打开的原始 CM 窗口仍是上游那八个开关（含远程重启 / 录制 / 阻止输入 / 隐私模式），由坐在机器前的人自己切换，两者不冲突。界面有两套：`--ui` 用原始 CM 窗口（`cm.tis`，与上游一致），非 `--ui` 用定制界面（`cm_sh.tis`，由原始 CM 窗口复制而来，只画这四项并接对端申请提示）。两套是同一个进程、同一个参数（`--cm`），皮肤由 `--ui` 决定。
- 这四项在会话建立时都是关闭的。对端可以开口要，但要不到：申请只是把「有人想开」摆到本机用户眼前（§6.9），给不给始终由本机用户决定。决定的路共有三条：人拨开关、本接口、以及 §6.7.3 应答对端的申请，三条最终落到同一段开关逻辑（本接口与「应答命名权限申请」都调 `switch_permission`），所以状态不会出现两种说法。
- 本接口没有「必须有一个在途申请」这个前提，这点与 §6.7.3 不同：`/control` 只在真有人申请时才能调，否则 409；本接口任何时候都能调。因此它是单方面决定，相当于本机软件代用户授权：谁持有 token，谁就能在电脑前没人的情况下替本机用户把键鼠交出去。
- 运维设置 `enable-perm-change-in-accept-window = N`（锁定权限）时，本接口除 `keyboard` 外一律 409。这道锁也管着 §6.9 里命名权限的应答（`respond_control_request` 自己会检查），但管不住 `keyboard`：`keyboard` 在门口因 `name != "keyboard"` 免于 409，而 §6.7.3 应答键鼠申请走的路径（`resolve_control_request`）根本不经过 `switch_permission`。把 `keyboard` 排除在外是有意的，它是 `--ui` 形态下现场唯一的放行入口，但运维需要知道：锁定权限锁不住键鼠。
- 打开 `keyboard` 就是授权控制：本机用户点开键盘图标，或用本接口打开 `keyboard`，与 §6.7.3 应答一次键鼠申请等价，三者都写同一个闸门；关掉 `keyboard` 则收回控制，会话退回只读。差别只在由谁来点，闸门本身只有一处。
  - 所以 `--ui` 那条路径不需要提示：原始 CM 窗口没有「对端申请控制」的提示条，对端开口只是让服务端记下一个 60 秒的待办，现场的人在窗口上点键盘图标即可放行；没有人点就按超时拒绝处理。
- **`file` 与另外三项不同**：它打开的是**文件通道**（`enable_file_transfer`），不等于「复制粘贴文件」可用。想真的复制粘贴文件，另外还有**两道闸**，两道都成立才通：
  1. **构建闸（被控端）**：被控端必须是带 `unix-file-copy-paste` 编出来的构建。不带该 feature 时，被控端登录时的附加信息里根本没有 `has_file_clipboard` 字段，控制端会话菜单里连「允许复制粘贴文件」这一项都不会出现（`src/ui/header.tis` 要求本端与对端两个 `has_file_clipboard` 同时为真）。唯一不需要该 feature 的组合是**两端同为 Windows**（`is_both_windows`）；构建命令见 `README.md` 的「File copy and paste」一节。
  2. **会话闸（控制端）**：控制端的「允许复制粘贴文件」要处于打开状态。它是控制端**按对端保存的会话选项**（`ClientConfig`），写在 `config/peers/<对端设备 ID>.toml`，键名 `enable-file-copy-paste`，**不是** `GateDesk2.toml` 的 `[options]`（附录 A 与本接口都不涉及它）；菜单里拨一下即落盘。该键不存在时取默认值：`UserDefaultConfig` 对这个键的兜底是 `'Y'`，即**默认开**；要关就写 `config/GateDesk_default.toml` 的 `[options] enable-file-copy-paste = 'N'`（定制客户端的 default/override 设置也在这个位置生效）。本机实例：`peers/419984805.toml` 里为 `true`。打开后控制端才发 `OptionMessage.enable_file_transfer = Yes`，被控端的 `file_transfer_enabled()` 才为真，文件剪贴板服务也才有订阅条件。
  - 文本剪贴板（`clipboard`）不受这两道闸影响；「文件传输」窗口走的是文件通道本身，也不受影响。

**审计**：`permission.change`（见 §6.8），`extra` 为 `{peer_id, name, enabled}`。

`actor` 恒为 `customer`。事件在连接层记录，人拨开关、§6.7.3 应答、本接口调用三条路写出来完全一样，日志里分不出是现场的人做的还是本机软件代做的。平台代授权时需自行留日志，两边靠 `session_id` 和时间对账。

#### 6.7.5 结束会话

```
POST /terminate
```

**作用**：结束一个会话，与受控端界面上的「断开」一致。对端会知道是被本机用户结束的，因此允许重连。

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
| id | 是 | int | 会话标识 |

**请求示例**

```powershell
curl -X POST "http://127.0.0.1:21120/terminate?token=<token>" -d "{\"id\":3}"
```

**成功响应（200）**

```json
{"ok":true,"result":{"peer_id":"123456789"}}
```

**什么时候会失败**：`id` 缺失或非法 → 400；会话不存在 → 404。对一个已经结束的会话再调一次不报错，它是空操作，返回 200（条目不会因此消失，那是 §6.7.6 的事），所以超时后重试是安全的。

**审计**：`session.terminate`。

#### 6.7.6 清理已结束的会话

```
POST /dismiss
```

**作用**：把已经结束（`/sessions` 中 `disconnected: true`）但还留在列表里的条目移除，与受控端界面上的「关闭」一致。它连带做收尾，例如释放该会话占用的剪贴板文件，所以比「从列表里删一行」要多一点。

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
| id | 是 | int | 会话标识 |

**成功响应（200）**

```json
{"ok":true,"result":{"peer_id":"123456789"}}
```

**什么时候会失败**：`id` 缺失或非法 → 400；会话不存在 → 404；**会话还在进行** → 409 `the session is still running`（此时该用 §6.7.5）。

### 6.8 操作级审计（桌面端内部机制，非本 API 端点）

自 v1.7 起，桌面端对本 API 引发的关键动作以及会话内高风险操作统一做操作级审计（企业版设计 §8）：

- 统一载荷：`{action, actor, device_id, session_id, ts, result, extra}`（JSON Lines）。`actor` 有两类值：`operator` 表示本机或平台发起的动作，`customer` 表示受控端会话内由本机用户决定的那批（`login.*` / `control.*` / `session.terminate` / `permission.change`）。
- 本地兜底：每一条都追加写入日志目录下的 `audit.log`（每行一条 JSON），不阻塞主流程；上报断网也不会丢记录。
- 转发上报：若 `[options] audit-server-url` 已配置（如 GateDeskWeb 的 `http://<ip>:3000/api/audit`），事件以异步 POST 转发到该端点；失败静默（本地已兜底）。
- 由本 API 引发的动作：`/password` 成功/失败 → `auth.grant`；`/connect` → `connect.start`（ok/err）；`/disconnect` 实际关闭会话 → `connect.close`。
- 受控端会话动作（v1.8）：放行接入 → `login.approve`；拒绝接入 → `login.deny`；允许控制 → `control.approve`；拒绝控制 → `control.deny`；控制请求超时 → `control.timeout`；结束会话 → `session.terminate`；改权限 → `permission.change`。这些事件在真正执行的连接层记录，所以无论动作来自会话面板还是本 API，日志都一致，并且带会话号和对端 ID。代价是日志里看不出动作是谁发起的，平台侧需自行留日志。
- 三个 `control.*` 的 `extra` 是 `{peer_id, permission}`，`permission` 空字符串表示键鼠，其余为权限名（`permission.change` 的 `extra` 是 `{peer_id, name, enabled}`）。`control.approve` / `control.deny` 自 v1.24 起才带 `permission`，此前只有 `peer_id`。
- 会话内操作（控制端会话窗口/受控端执行点）也产生事件：`record.start/stop`、`remote.restart`、`privacy.on/off`、`block_input.on/off`、`voice.on/off`。

审计事件由桌面端直接上报，不经本地 HTTP API 转发。本小节只说明事件来源，并作为排查 `audit.log` 的索引。

### 6.9 请求对端开启一项权限（控制端）

```
POST /request-permission
```

向一个已经打开的会话的对端要一项权限。本机是控制端，主动开口；对端是受控端，给不给由对端本机用户决定。

| 参数 | 必填 | 类型 | 说明 |
|------|------|------|------|
| token | 是 | string | 本地 API 令牌（`api-token`）。所有接口都要求：随 URL 传 `?token=<token>`，或走请求头 `Authorization: Bearer <token>`（§4）。缺失、错误或本机未配置 → 401 |
| id | 是 | string | 对端设备 ID，必须是本机已由 `/connect` 打开的会话 |
| name | 是 | string | `keyboard` / `clipboard` / `audio` / `file`，即受控端的四项 A 类权限（§6.7.4） |


**请求示例**

```powershell
# 键鼠控制
curl -X POST "http://127.0.0.1:21120/request-permission?token=<token>" -d "{\"id\":\"123456789\",\"name\":\"keyboard\"}"
# 剪贴板（audio / file 同理）
curl -X POST "http://127.0.0.1:21120/request-permission?token=<token>" -d "{\"id\":\"123456789\",\"name\":\"clipboard\"}"
```

**成功响应（200）**

```json
{"ok":true}
```

#### 本接口只负责询问，不负责设置

这个请求自始至终只做一件事：把「有人想开」摆到受控端本机用户眼前。

1. 本机（控制端）调本接口，连接进程按对端设备 ID 把请求投递给该会话。同一台机可以同时开多个会话，各自有自己的监听，不会串；
2. 受控端的 GateDesk 把请求画成一个**同意 / 拒绝**的界面：非 `--ui` 形态是会话面板上的提示条，`--ui` 形态不画提示、靠现场的人在原始连接管理器窗口上点键盘图标（§6.7.3 / §6.7.4）；
3. 只有对端本机用户点了同意，权限才真的打开；拒绝、或 60 秒无人应答，则什么也不变，会话保持原样。

所以 200 只表示会话已受理，不代表对端已同意：本 API 是轮询式的（§8.1），没有回调，也没有结果推送。判断结果不要用这个接口，它的语义就是「问过了」；答没答、答了什么，只有对端那台机器知道。

#### 四个名字的差异

下面把差异写清楚，以免把这几个名字当成同一件事用：

| | `keyboard` | `clipboard` / `audio` / `file` |
|---|---|---|
| 协议落点 | 清掉上游既有的 `OptionMessage.disable_keyboard`，与远程窗口菜单里的「请求控制」是同一动作，无新增字段 | 新加的 `OptionMessage.request_permission` 字段（v1.11）。它故意不是设置项：`disable_*` 说的是「我要关」，它说的是「可以吗」 |
| 受控端闸门 | §6.7.3 的会话控制闸门；同意 = 交出键鼠控制，关掉即收回 | §6.7.4 的权限开关本身；同意 = 受控端本机替用户调一次那个开关，与他手动拨开关是同一批动作 |
| 结果可见性 | 对端同意后发 `Permission::Keyboard`，控制端立刻能看出只读解开 | 没有回执，只能从通道是否真的可用反推（剪贴板能不能用、有没有声音） |
| 审计（记在受控端） | `control.approve` / `control.deny` / `control.timeout`，`extra.permission` 为空表示键鼠 | 允许 → `control.approve` 与 `permission.change` 各一条（后者的 `extra.name` 带权限名、`extra.enabled` 为 `true`；前者的 `extra.permission` 也是那个权限名，两条可对着看）；拒绝 → `control.deny`；超时 → `control.timeout`；请求本身不记事件 |
| 结果怎么判 | 受控端同意后会发一个明确的「键盘开了」信号，控制端据此解开只读 | 只能从通道真的能用反推；受控端那台机器自己的集成可以读 §6.7.1 |

> ⚠️ 表里 `clipboard` / `audio` / `file` 三项是并列写的，但 `file` 多一层：它打开的是文件通道，「复制粘贴文件」能不能用还取决于被控端的构建 feature 与控制端的 `enable-file-copy-paste` 开关（两道闸，见 §6.7.4 边界）。文本剪贴板没有这两个额外条件。

**错误**

| HTTP | 触发条件 |
|------|---------|
| 400 | `id` 缺失、为空、超长（>128），或含 `/` `\` `:`、空白/控制字符（它会进进程内通道名，Unix 是路径、Windows 管道名，必须挡住）；`name` 缺失或不在 `keyboard` / `clipboard` / `audio` / `file` 之内 |
| 503 | 找不到该对端的会话进程（没有开过会话，或会话已退出） |
| 504 | 会话进程在 2 秒内未回应 |

**影响面**：`keyboard` 等于把对端的鼠标键盘交出来，比另外三项重得多。要不要允许平台申请它、要不要加重审计，属于策略层的决定；接口层对四个名字一视同仁，不给 `keyboard` 单独开后门，也不给它单独设卡。

### 6.10 出站事件通知（被控端 → 平台，v1.20）

上报侧在 `GateDesk/src/event.rs`，由连接层四个点调用（对端等批准、对端申请、会话建立、会话结束）。接收侧在 `GateDeskWeb/server.js` 的 `POST /api/event`，收到后经 WebSocket 广播给该设备房间里的页面（`admin.html` / `employee.html` 的 `data.type === 'event'` 分支）。

整条链路由配置键 `event-server-url` 控制，为空时四个点什么也不发，行为与没有这个机制时完全一致。

**为什么要有它**

§6.7 那一组只能靠轮询发现变化，而轮询有两个问题：

1. 发现延迟：一次「有人申请键鼠 / 权限」只有 60 秒窗口（`CONTROL_REQUEST_TIMEOUT`），轮询慢了就永久错过，对端只能重新发起。
2. 代价随设备数增长：每台被控机都要一个定时器加一份 token，`N` 台就是 `N` 份运维对象。

本机制让被控端在状态刚变时主动 POST 一次，平台从定时器改成事件驱动加兜底轮询。

**配置**：`[options] event-server-url`（可空，默认空即关闭，见附录 A.3），为空时行为与现在完全一致。

**投递约定**

| 项 | 约定 |
|----|------|
| 方向 | 被控端 → 平台，出站 POST。与 §6.8 的审计转发同向，但配置和端点各自独立 |
| 方法 / 头 | `POST <event-server-url>`，`Content-Type: application/json` |
| 超时 | 3 秒 |
| 重试 | 不重试，丢了靠兜底轮询补上 |
| 鉴权 | 桌面端不加任何头，与 `audit-server-url` 现状一致；平台侧鉴权由 URL 自带，如 `?key=<secret>` |
| 应答 | 平台返回任意 2xx 即视为收到，其余状态码不影响桌面端 |
| 语义 | 只表示「去看一眼」。状态以 `GET /sessions`（§6.7.1）为准，记录以 §6.8 为准 |

**载荷**

```json
{
  "event": "control.pending",
  "device_id": "123456789",
  "session_id": 3,
  "peer_id": "987654321",
  "ts": 1758412345,
  "extra": { "permission": "" }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| event | string | 事件名，见下表 |
| device_id | string | 通知方（被控机）自己的 GateDesk ID |
| session_id | int | 会话标识，即 `GET /sessions` 返回的 `id`（§5 的寻址约定），可直接拿去调 §6.7 的动作 |
| peer_id | string | 提出申请 / 建立会话的那台设备 ID |
| ts | int | Unix 秒 |
| extra | object | 事件专属字段 |

**事件**

| event | 何时发 | extra | 平台该做什么 |
|-------|--------|-------|-------------|
| `login.pending` | 出现一个在等批准的对端（对应 `/sessions` 里 `authorized:false && disconnected:false`） | `{}` | 按平台策略 `POST /approve`（§6.7.2） |
| `control.pending` | 出现一个待应答的申请（`pending_control` 变成 `true`） | `{"permission":""}` = 键鼠；`{"permission":"clipboard"}` 等 = 命名权限 | 按平台策略 `POST /control`（§6.7.3）；键鼠要慎重，见 §6.9 的影响面说明 |
| `session.open` | 对端被放行、会话真正建立 | `{}` | 可选：记一笔，或按策略调整权限（§6.7.4） |
| `session.close` | 会话结束（对端断开或本机结束） | `{"reason":"…"}`，如 `Peer close` / `Timeout` / `connection manager` | 可选：`POST /dismiss` 清理条目（§6.7.6） |

事件名与 §6.8 的审计 action 名是两套命名空间，并不对应：审计的 action 说的是「发生了什么」（如 `session.terminate`），事件名说的是「要你去看一眼什么」（如 `control.pending`）。`session.open` / `session.close` 在 §6.8 里没有同名事件，不要拿它去对账。

**平台侧要注意的两点**

1. 幂等，并以 `/sessions` 为准：同一次变化可能通知多次，也可能一次都不通知。收到后先查一次 `/sessions` 再动作，不要把通知当状态用：它到达时那个申请可能已经被现场的人点了，或已经超时。
2. 兜底轮询不能省，建议 30 秒一轮。事件通知不是可靠性机制，它只把发现延迟从一个轮询周期降到事件到达即返回。

**和 §6.8 审计的区别**

| | §6.8 审计 | §6.10 事件通知 |
|---|---|---|
| 目的 | 留痕、追责 | 降低发现延迟 |
| 开关 | `audit-server-url` | `event-server-url` |
| 失败处理 | 重试（120 秒预算）+ 本地 `audit.log` 兜底 | 不重试，无兜底 |
| 缺一条的后果 | 审计链断一环 | 那一次变化退回轮询发现 |

两者可以指向同一个平台，但建议用不同路径（如 `/api/audit` 与 `/api/event`），不要把提示和记录混在一个端点上。

### 6.11 会话录像（控制端，v1.22）

控制端录屏，并把定制皮肤录下的文件上传到审计服务端。

通过本 API 的 `POST /connect`（§6.2）发起的会话自动录屏，会话结束后把文件上传到审计服务端。手动录制照常可用，两种录像落在同一个目录，区别只在自动录的这一种会上传。

不自动录屏时，是否开录由本机选项 `allow-auto-record-outgoing` 决定；这样录下的文件只留在本机，不上传。

开录时点在连接的第一个画面线程建立处（`client/io_loop.rs`），与 `record.start` / `record.stop` 审计是同一个去重翻转点。

#### 本机存放位置

录像目录由本机选项 `video-save-directory` 决定。该键在 `GateDesk_local.toml`，不在 `GateDesk2.toml`。取值不做 `~` 展开，也不做环境变量展开。

没配时按下表顺序取第一个可用的（`ui_interface::video_save_directory`）：

| 顺序 | 取法 | Windows | macOS | Linux |
|---|---|---|---|---|
| 1 | 系统视频目录 + `GateDesk` | `C:\Users\<用户>\Videos\GateDesk\` | `~/Movies/GateDesk/` | `~/Videos/GateDesk/`（跟随 XDG） |
| 2 | 系统视频目录 | `C:\Users\<用户>\Videos\` | `~/Movies/` | `~/Videos/` |
| 3 | 桌面 | `C:\Users\<用户>\Desktop\` | `~/Desktop/` | `~/Desktop/` |
| 4 | 主目录 | `C:\Users\<用户>\` | `~/` | `~/` |
| 5 | exe 同级 `videos\` | `<exe 目录>\videos\` | 同左 | 同左 |

Windows 上以服务身份录制（被控端）走的是另一套：`[options] windows-service-video-save-directory`，没配则 `%SystemDrive%\ProgramData\GateDesk\recording\`。控制端不走这条。

#### 文件名与格式

```
outgoing_<对端ID>_<yyyyMMddHHmmss毫秒>_<display|camera><显示序号>_<编码>.<webm|mp4>
```

`outgoing_` 是控制端录的，被控端自己录的是 `incoming_`。编码决定容器：VP8 / VP9 / AV1 用 `.webm`，H264 / H265 用 `.mp4`。例：

```
outgoing_419984805_20260923171149048_display0_av1.webm
```

编码器编在二进制里，不需要外部 ffmpeg。录制不足 1 秒或没有有效帧的文件会被删掉，不留文件。

#### 上传

地址取 `audit-server-url` 的源（scheme + host + port）拼 `/api/record`，与审计是同一台服务器，所以只有这一个键；为空则不上传，录像仍写在本机。

请求体是文件字节，不是 JSON；参数走查询串：

| `type` | 含义 | 查询参数 | 请求体 |
|---|---|---|---|
| `new` | 建文件 | `file` | 空 |
| `part` | 追写一段 | `file`、`offset`、`length` | 这一段字节 |
| `tail` | 收尾（整个文件已传完） | `file` | 文件头（最多 1024 字节） |
| `remove` | 删除（录像过短被丢弃时） | `file` | 空 |

每个请求另带 `device_id`、`session_id`。

传法由 `[options] record-upload-mode` 决定：

| 值 | 行为 | 代价 |
|---|---|---|
| `chunked`（默认） | 边录边传，累计 1 秒或 1 MB 发一段 | 会话中途断掉最多丢最后一秒 |
| `whole` | 文件关闭后一次发完 | 文件大时占用内存 |

每个请求重试 3 次，间隔 0.5s / 1.0s；仍失败就放弃该文件，不重传，写一条审计事件。目标不可达时就是这样：文件留在本机，服务端没有。

新增两个审计 action，接在 §6.8 的表后：

| action | 触发 | `extra` |
|---|---|---|
| `record.upload.done` | 整个文件上传成功 | `{"file":"…","bytes":N}` |
| `record.upload.fail` | 某个请求重试 3 次仍失败 | `{"file":"…","bytes":N,"step":"new/part","error":"…"}` |

#### 服务端存放位置

落在 `服务端应用` 所在目录下：

```
recordings/<device_id>/<session_id>/<录像文件名>
                                   <录像文件名>.head    前 1024 字节
                                   <录像文件名>.json    大小、上传时间
```

与平台无关，就是服务端进程旁边多一个 `recordings/`：

| 服务端跑在 | 路径 |
|---|---|
| Windows | `C:\...\GateDeskWeb\recordings\<设备ID>\<会话号>\` |
| macOS | `~/.../GateDeskWeb/recordings/<设备ID>/<会话号>/` |
| Linux | 同 macOS |

服务端不删录像，保留策略在客户端。

#### 本机保留 3 天

上传成功后在录像旁写一个 `<文件名>.uploaded`，内容是上传时间戳。定制皮肤每次启动扫一遍录像目录，把带标记、且标记时间超过 3 天的录像连同标记一起删掉。没有标记的文件不动，包括从没传成功的和用原版皮肤手动录的。

#### 配置与查看

| 项 | 位置 | 必填 |
|---|---|---|
| 上传地址 | `GateDesk2.toml` `[options] audit-server-url`，与审计共用 | 不填则不上传 |
| 录像目录 | `GateDesk_local.toml` `[options] video-save-directory` | 可选 |
| 传法 | `GateDesk2.toml` `[options] record-upload-mode` | 可选，默认 `chunked` |

配置在启动时缓存，改完需重启 GateDesk。

## 7. 错误码

| HTTP | 触发条件 |
|------|---------|
| 200 | 成功 |
| 204 | OPTIONS 预检成功（仅受信来源） |
| 400 | 参数缺失或非法（如 `id` 为空/超长、`/password` 密码为空/超长） |
| 401 | 未携带 token、token 错误、或未配置 `api-token`（响应体区分原因） |
| 403 | Host 头非 localhost/127.0.0.1，或 `Origin` 非受信来源（v1.7） |
| 404 | 未知路径；或会话接口中 `id` 对应的会话不存在（v1.8） |
| 405 | 方法不允许 |
| 409 | 会话当前状态与该动作不符：应答一个并不存在的申请（`/control`）、`name` 与在途那项申请不符（`/control`，v1.24）、放行一个已经放行的对端（`/approve`）、清理一个还在进行的会话（`/dismiss`）、在权限被运维锁定时改权限（`/permission`，**`keyboard` 例外**） |
| 413 | 请求体 `Content-Length` 超过 1024 字节（v1.7） |
| 500 | 服务端失败（如无法启动连接进程、设置密码失败） |
| 503 | 本机没有连接管理器进程在监听（无会话、也无那个窗口），§6.7 的接口无法执行（v1.8）；或 `/request-permission` 找不到该对端的会话进程（v1.9 / v1.11） |
| 504 | 连接管理器 2 秒没回应（§6.7，v1.8）；或对端的会话进程 2 秒没回应（§6.9，v1.9）。两条各自的 2 秒，互不相干 |

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

### 8.1 被控端无人值守批准（v1.8；v1.20 起支持事件驱动）

设备不开 `--ui` 时，会话面板是唯一能应答对端请求的窗口。若希望由平台统一决策而不是现场有人点，平台要做两件事：发现，然后决策。

```
管理端/业务系统（每个被控设备）
   └─ 发现（二选一或并用，见下表）
         ├─ 收到 §6.10 的通知（login.pending / control.pending）→ 立刻查一次 GET /sessions
         └─ 兜底轮询 GET /sessions（建议 30 秒）
   └─ 决策
         ├─ 有会话 authorized=false         → 请求接入，按平台策略 POST /approve
         ├─ 有会话 pending_control=true     → 按 `/sessions` 的 `pending_permission` 决定应答哪一项，POST /control 带上同名 `name`
         └─ 有会话 disconnected=true        → 已结束，POST /dismiss 清理
   └─ 授权后需要收紧/放开能力 → POST /permission
   └─ 需要主动收尾 → POST /terminate
```

**发现方式怎么选**

| 方式 | 延迟 | 代价 | 适用 |
|------|------|------|------|
| 只轮询 | 不超过一个轮询间隔 | 每台设备一个定时器 | 设备少，或平台已有统一的轮询框架 |
| 事件通知 + 兜底轮询（§6.10） | 约一个网络往返 | 每台设备配一个 `event-server-url` | 设备多，或对 60 秒窗口敏感 |

两种方式都需要兜底轮询：§6.10 的通知不保证送达，也不保证只送一次。

`admin.html` / `employee.html` 的取值是：先按 2 秒兜底轮询（轮询即全部），一旦真收到过一条事件就说明通知链路是通的，兜底随即放到 30 秒。页面无从知道对面配没配，只能这样自适应。纯轮询的集成建议不超过 5 秒一轮。

- 轮询间隔必须显著小于 60 秒，控制请求超时即被拒绝，间隔太大就会错过窗口，控制端只能重新发起。
- 收到通知后先查 `/sessions` 再动作，通知到达时那个申请可能已经被现场的人点了，或已经超时。
- 平台代为批准会在本机 `audit.log` 留下 `login.approve` / `control.approve` 等记录，但不会注明是哪个平台（`actor` 恒为 `customer`，见 §6.7.4）。平台侧需自行留日志，两边靠 `session_id` 和时间对账。
- 一次会话可能同时存在多条请求（远程桌面 + 文件传输），以 `/sessions` 返回的 `id` 寻址。

### 8.2 控制端请求权限（v1.9，v1.15 合并）

控制端要一项它还没有的权限，只有一条路：问。请求落到对端本机用户眼前，由他在自己那台机器上点同意或拒绝，平台代点、程序代点都不存在。

```
控制端
   └─ POST /request-permission { id: <对端设备 ID>, name: keyboard|clipboard|audio|file }
        ├─ 200 → 会话已受理，请求已按对端 ID 投递给该会话进程
        └─ 400 / 503 / 504 → 没投出去，见 §6.9
对端（受控端）
   └─ GateDesk 弹「同意 / 拒绝」
        ├─ 同意 → 权限真的打开，本机远程窗口立刻可用（键鼠还会回来一个 Permission::Keyboard）
        └─ 拒绝 / 60 秒无人应答 → 什么也不变，会话保持原样
控制端远程窗口
   └─ 用它要到的能力：能不能粘、有没有声音、能不能传文件
```

- 不要拿 200 判结果：它说的是「问过了」，不是「给了」。控制端这一侧看不到答案，答案只体现在远程窗口的实际表现上（剪贴板能不能贴、有没有声音、键鼠能不能动）。想直接读状态，只有对端那台机器自己的集成能读到（§6.7.1），本机侧读不到。
- 重试要有节制：对端 60 秒内没答就是拒绝，再发只会再弹一次同样的问题。等这一轮结束（`/sessions` 里 `pending_control` 变回 `false`）再问。
- 键鼠与其余三项的行为差别见 §6.9 的差异表，只有键鼠同意后会回来一个明确信号。

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

# --- 受控端会话接口（v1.8）---
# 列出会话（无会话且面板窗口已退出 → 503；面板窗口还在 → 200 且 sessions 为 []）
curl "http://127.0.0.1:21120/sessions?token=<token>"
# 批准接入（把 <id> 换成 /sessions 返回的 id）→ 200，面板上对应卡片消失
curl -X POST "http://127.0.0.1:21120/approve?token=<token>" -d "{\"id\":<id>,\"accept\":true}"
# 允许控制 → 200，会话面板上的「允许/拒绝」提示同步消失（name 必填且要与在途的那项一致）
curl -X POST "http://127.0.0.1:21120/control?token=<token>" -d "{\"id\":<id>,\"name\":\"keyboard\",\"accept\":true}"
# 应答一项命名权限的申请 → 200，响应回显 name
curl -X POST "http://127.0.0.1:21120/control?token=<token>" -d "{\"id\":<id>,\"name\":\"clipboard\",\"accept\":true}"
# name 与在途那项不符 → 409，错误里写明在途的是哪一项
curl -s -X POST "http://127.0.0.1:21120/control?token=<token>" -d "{\"id\":<id>,\"name\":\"file\",\"accept\":true}"
# name 不在四个名字之内或缺失 → 400
curl -s -o NUL -w "%{http_code}" -X POST "http://127.0.0.1:21120/control?token=<token>" -d "{\"id\":<id>,\"accept\":true}"
# 开关权限 → 200，面板上对应开关同步变化
curl -X POST "http://127.0.0.1:21120/permission?token=<token>" -d "{\"id\":<id>,\"name\":\"clipboard\",\"enabled\":true}"
# 请求对端开启一项权限（v1.15 合并，控制端；<ID> 是本机已 /connect 的对端设备 ID）
# → 200 只表示会话已受理；对端 GateDesk 会弹「同意 / 拒绝」，不点则什么也不变
curl -X POST "http://127.0.0.1:21120/request-permission?token=<token>" -d "{\"id\":\"<ID>\",\"name\":\"clipboard\"}"
# 键鼠是四个名字之一
curl -X POST "http://127.0.0.1:21120/request-permission?token=<token>" -d "{\"id\":\"<ID>\",\"name\":\"keyboard\"}"
# name 不在四个名字之内 → 400
curl -s -o NUL -w "%{http_code}" -X POST "http://127.0.0.1:21120/request-permission?token=<token>" -d "{\"id\":\"<ID>\",\"name\":\"restart\"}"
# 结束会话 → 200，对端断开且允许重连
curl -X POST "http://127.0.0.1:21120/terminate?token=<token>" -d "{\"id\":<id>}"
# 清理已结束的会话 → 200
curl -X POST "http://127.0.0.1:21120/dismiss?token=<token>" -d "{\"id\":<id>}"
# 状态不符 → 409（例：对同一个 id 重复 /approve）
# 无会话管理器 → 503（例：本机当时没有任何会话）

# --- 出站事件通知（v1.20）---
# 1) 在被控机上配 [options] event-server-url 并重启 GateDesk
# 2) 让对端对该机发起一次权限申请（对端调 §6.9 的 /request-permission）
# 3) 平台侧应立刻收到一次 POST，可在 /api/event 回看：
curl "http://127.0.0.1:3000/api/event?limit=10"
#    期望其中一条：
#    {"event":"control.pending","device_id":"<本机ID>","session_id":<id>,"peer_id":"<对端ID>","ts":...}
# 不配该键 → 完全没有这个 POST（与历史版本一致）
# 仅轮询也能拿到同样信息：GET /sessions 里 pending_control=true
```

## 10. 变更记录

| 日期 | 版本 | 变更 |
|------|------|------|
| 2026-09-24 | 1.27 | **`GET /sessions` 在没有连接管理器时返回空数组**（§6.7.1）：此前回 503，而连接管理器在最后一个会话结束时会自行退出（`quit_gui`，界面上关掉最后一个卡片也走同一条路），于是空闲机器上每轮轮询都收到 503，调用方分不清「没有会话」和「服务没起来」。现在只有 `/sessions` 把「没有管理器」读作「没有会话」，回 `{"ok":true,"sessions":[]}`；其余接口维持 503，它们要的是管理器本身 |
| 2026-09-24 | 1.26 | 文档补漏（无接口变更）：**每个接口的参数表加上 `token` 行**，§6.7 的错误码规律补上 `401`。token 此前只在 §2「安全要求」与 §4「鉴权方式」里说明、只出现在各节的 curl 示例里，单独看某一节的参数表（尤其 `GET /sessions` 这种没有业务参数的）会以为不必带；实现里 `handle()` 是**先校验 token 再分派**的，`/id`、`/sessions`、`/approve` 一个都不例外，缺失或不符一律 401 |
| 2026-09-24 | 1.25 | **应答键鼠申请现在真能用了**（§6.7.3，无接口变更）：`accept: true` 此前只打开会话控制闸门（`control_authorized`），不动 `keyboard` 权限，而 `peer_input_enabled()` 是这两样的积 —— 于是当该会话的 `keyboard` 权限被关过时，接受申请后对端依旧一个键也敲不进来，`cm_sh` 上的键盘行也不亮，可审计里已经记了一条 `control.approve`，读日志的人会以为键鼠交出去了。现在 `resolve_control_request` 在同意时一并把 `keyboard` 权限打开，与点界面开关、`/permission {"name":"keyboard"}` 走到同一结果（记录、审计、窗口那一行三处一致）|
| 2026-09-23 | 1.24 | **`POST /control` 补上“应答的是哪一项”**（§6.7.3）：请求新增**必填** `name`（`keyboard` / `clipboard` / `audio` / `file`，与 `/request-permission` 同一套名字），必须与在途申请一致，不符 → 409 `the pending request is for …`；响应回显 `result.name`；审计 `control.approve` / `control.deny` 的 `extra` 从 `{peer_id}` 变为 `{peer_id, permission}`（空字符串=键鼠，与 `control.timeout` 一致）。此前本接口只认“在途的那一项”，调用方若拿着过期的 `/sessions`，一句 `accept: true` 会替本机用户打开一个它没想开的通道，且事后从审计里分不出那次同意是交了键鼠还是开了剪贴板。`employee.html` 同步：申请提示按 `pending_permission` 措辞（键鼠 / 剪贴板 / 声音 / 文件传输），按钮上带 `name`；`/control` 不带 `name` 的老调用方现在收到 400 |
| 2026-09-23 | 1.23 | 修掉权限状态的两种说法（无接口变更）：同一次开关此前只写在其中一处 —— 人在受控端窗口里拨开关时，写的是窗口本地的值，`GET /sessions` 读的那份记录没有动（于是客户页 `/employee` 上的勾选框仍是关的，而通道其实已经通了）；反过来由本接口或应答申请拨动开关时，记录更新了，但窗口那一行不会重画，还是旧位置。现在 `switch_permission`（三条路的共同终点）在发出开关时就把值写进记录，`cm_sh.tis` 的 `addConnection` 对已存在的会话也一并按记录重画这四行 —— 于是窗口、`GET /sessions`、`GET /sessions` 的消费方（`/employee` 勾选框）三处一致。§6.7.4 里「三条落到同一段开关逻辑，所以状态不会出现两种说法」这句此前只是设计意图，现在成立。`--ui` 的原始窗口 `cm.tis` 未动，仍是上游行为 |
| 2026-09-23 | 1.22 | 新增 §6.11 会话录像上传：由本 API `POST /connect` 发起的会话**自动录屏**，会话结束后把录像文件传到审计服务端 —— 上传地址由 `audit-server-url` 的源派生 `/api/record`，不新增地址配置键。协议沿用上游 rustdesk 的 `type=new/part/tail/remove` + raw body 分片；每个请求重试 3 次，失败放弃不重传，成功记 `record.upload.done`、失败记 `record.upload.fail`。本机文件在上传成功后保留 **3 天**，靠 `<文件名>.uploaded` 标记清理，未上传成功的不动。新增配置 `record-upload-mode`（`chunked` 默认 / `whole`）。客户端实现 `GateDesk/src/record_upload.rs`；接收端为 `GateDeskWeb` 的 `POST /api/record`，落在 `recordings/<device_id>/<session_id>/`。§6.11 另写明录像在本机与服务端的**存放位置**（分平台）、文件名与格式、查看命令 |
| 2026-09-23 | 1.21 | 补充「文件复制粘贴」的**两道闸**（仅文档，无接口变更）：§6.7.4 边界新增一条 —— `file` 打开的是文件通道，复制粘贴文件还要求（1）被控端带 `unix-file-copy-paste` 编译，否则登录附加信息里不上报 `has_file_clipboard`，控制端会话菜单里连「允许复制粘贴文件」都不出现（两端同为 Windows 是唯一例外）；（2）控制端的会话选项 `enable-file-copy-paste` 要打开 —— 它按对端存于 `config/peers/<对端ID>.toml`（`ClientConfig`），不是 `GateDesk2.toml` 的 `[options]`，缺省为开（`GateDesk_default.toml` 可关）。§6.9 差异表后加一条指向说明 |
| 2026-09-21 | 1.20 | 落地 §6.10 出站事件通知。上报侧新增 `GateDesk/src/event.rs`，用独立的配置键、队列和线程，不重试、无本地兜底、每次投递 3 秒上限；连接层四个点发出 `login.pending` / `control.pending` / `session.open` / `session.close`。接收侧 `GateDeskWeb` 新增 `POST /api/event` 并广播给页面，两个页面收到后立即拉 `/sessions`，兜底轮询在收到过事件之后由 2 秒放到 30 秒。`session.close` 的 `extra` 增加 `reason`。§6.7 各接口的「干什么」标签统一改为「作用」，§6.7.4 与 §8.1 措辞整理。附带修掉 `employee.html` 里仍在调用已删除的 `POST /voice` 的语音按钮，改为按会话调 `POST /permission {"name":"audio"}` |
| 2026-09-21 | 1.19 | 新增 §6.10 出站事件通知的设计与契约（当时尚未实现），§8.1 增加事件驱动这条发现路径。同时修正四处与实现不符：§1 角色表把 `/password` 归到控制端（它设的是本机密码，属于被控端，改为独立一行）、§1 端点清单还留着已删除的 `/voice`、§6.9 差异表写 `extra.permission` 而实际字段是 `extra.name`、头部维护约定与 §1 的源码路径写成小写 `gatedesk/`。补充 §6.7.4 的三点：`/permission` 没有「必须有在途申请」这个前提、`enable-perm-change-in-accept-window = N` 锁不住 `keyboard`、`permission.change` 的 `actor` 恒为 `customer` |
| 2026-09-21 | 1.18 | **删除 `POST /voice`**（原 §6.6，编号留空）：音频权限用 `POST /permission {"name":"audio"}` 按会话开关就够，批量端点省不掉那次 `/sessions` 查询，却多一套「一次改所有本机会话」的粗放语义和一条独立的审计路径。§6.5、§6.8、§7 里与它相关的说法一并去掉；`audio-input` 的历史污染清理（启动时清掉旧版写入的 `Y`）保留 |
| 2026-09-21 | 1.17 | **删除 `POST /request-control`**：键鼠控制只留 `POST /request-permission {"name":"keyboard"}` 一条路。v1.15 起两者已经等价，留着两个名字只会让人怀疑它们不一样。桌面端已无该路由，继续调用返回 404；用过的集成把 `-d {"id":"<ID>"}` 换成 `-d {"id":"<ID>","name":"keyboard"}` 即可 |
| 2026-09-21 | 1.16 | 整理 §6.6–§6.9：每个接口前面加一句「干什么」，参数/响应/审计之外补一张「什么时候会失败」表，精确到每个状态码的触发条件；§6.7 开头补一张接口一览表与错误码规律。同时修掉与实现不符的四处：`/terminate` 对已结束的会话是空操作返回 200（原写「→ 409」）、§7 的 409 一行漏了 `/permission` 的 `keyboard` 例外、§6.6 漏了「权限被运维锁定时 409」、§6.7.1 漏了 `pending_permission` 字段（`/control` 靠它区分键鼠与命名权限两种申请）。另补：§6.7.3 写明 `accept: true` 应答命名权限申请时会替本机打开那项权限，§6.7.4 的「两个决定入口」改为「三条路」，§6.8 说明 `actor` 的两类取值，§6.9 差异表措辞改为「受控端本机替用户调开关」 |
| 2026-09-21 | 1.15 | **§6.9 与 §6.10 合并为一节**（原 §6.10 取消）：控制端只有一个「请求对端开启一项权限」的端点，`name` 收 `keyboard` / `clipboard` / `audio` / `file` 四个名字；`POST /request-control` 保留为 `name: "keyboard"` 的等价别名。内核本来就是一个表示（一个 `Data::ControlRequest`，`permission` 为空即键鼠），上层分成两个端点是历史顺序造成的。文档同时写明四个名字的两点差异（结果可见性、受控端闸门），并新增 §8.2 描述控制端这一侧的完整流程（发起 → 对端弹「同意 / 拒绝」→ 允许后才真的开）。桌面端同步落地：`/request-permission` 收四个名字，`keyboard` 在实现里对应原来 `disable_keyboard` 那条路（`/request-control` 发的就是它），`/request-control` 转为等价别名 |
| 2026-09-21 | 1.14 | 修正受控端界面的键鼠显示：连接管理器窗口里键盘那一行此前读的是权限（默认开），而真正的输入闸门 `control_authorized` 默认关 —— 界面写「允许」但敲不进一个字。现在该行与 `GET /sessions` 的 `keyboard` 字段都是 `peer_input_enabled()`，即权限与会话批准同时成立才为真（§6.7.1）；会话批准被本机 API、窗口里的提示或 60 秒超时改变时，连接层会把新值回推给窗口 |
| 2026-09-21 | 1.13 | 受控端界面拆成两套：`--ui` 用原始 CM 窗口（`cm.tis`，与上游逐字一致），非 `--ui` 用定制界面（`cm_sh.tis`）。相应更正 §6.7.4：打开 `keyboard` 权限**即**授权控制（与 §6.7.3 写同一个闸门），关掉则收回——连接管理器窗口里点键盘图标一直是这个语义，此前被独立闸门隔开，导致还原成上游界面后现场无人能放行；§6.7.3 补充「另一道门」 | 
| 2026-09-21 | 1.12 | `POST /voice`（§6.6）从「写全局 `audio-input` 配置」改为「切换本机所有活动会话的 `audio` 权限」：原实现把录音设备名当布尔开关写，受控端音频服务随即以 `Failed to get default input device for loopback` 启动失败——权限给对了也没有声音；无活动会话时返回 409，响应体改为 `{"ok":true,"result":{"enabled":…,"sessions":…}}`；启动时自动清除历史遗留的 `audio-input = 'Y'`；附录 A 更正 `audio-input` 的语义（设备名，留空=系统默认）；连接级审计上报接受平台 `{"code":0,…}` 成功信封（此前只认空 body，导致每条记录重试三次后被丢弃）；§6.7.4 补充两条界面路径的权限开关边界 |
| 2026-09-21 | 1.11 | 新增控制端端点 `POST /request-permission`（§6.9，当时编号为 §6.10；v1.15 起并入 §6.9）：向已打开的会话请求对端开启一项 A 类权限（`clipboard` / `audio` / `file`），经进程内通道按对端 ID 投递给会话进程，等价于「请求键鼠控制」；协议新增 `OptionMessage.request_permission` 字段，`disable_*` 说的是「我要关」而它说的是「可以吗」，答案仍由对端本机用户在本地确认界面上给；相应修正 §6.7.4 的措辞——对端可以开口要，但决定入口仍只有本接口与会话面板 |
| 2026-09-21 | 1.10 | `POST /permission`（§6.7.4）的权限名收敛到 `keyboard` / `clipboard` / `audio` / `file` 四项：远程重启、阻止用户输入、隐私模式不再提供，录制会话不再是可切换项；这四项在会话建立时固定为关闭，对端的权限请求不能再代替本机用户打开它们（具体见客户端集成设计方案 §7.1 的三类划分） |
| 2026-09-20 | 1.9 | 新增控制端端点 `POST /request-control`（§6.9）：向已打开的会话请求对端键鼠控制，经进程内通道按对端 ID 投递给会话进程，等价于远程窗口菜单里的「请求控制」；新增 503（无该对端的会话进程）与 504（会话进程未按期回应）说明 |
| 2026-09-17 | 1.8 | 新增受控端会话接口（§6.7）：`GET /sessions` 列出会话与待办请求、`POST /approve` 批准/拒绝接入、`POST /control` 应答控制请求、`POST /permission` 开关权限、`POST /terminate` 结束会话、`POST /dismiss` 清理已结束会话；实现走进程内 `_cm` 通道，与受控端会话面板同一批动作（接口与面板状态互通）；新增审计事件 `login.approve` / `login.deny` / `session.terminate` / `permission.change`（§6.8）；错误码新增 409（会话状态不符）、503（无会话管理器）与 504（管理器未按期回复）；新增 §8.1 被控端无人值守批准流程 |
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

HTTP API 的鉴权 token（`api-token`）与抓声设备（`audio-input`）等配置，均存放在 **GateDesk2.toml**（即 `Config2`）的 `[options]` 表中。本文档重点说明其位置、结构、数据来源、读取优先级、可维护键值以及常见坑。它是运维 / 部署脚本 / 调试时最重要的配置入口之一。

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
| `event-server-url` | 出站事件通知地址（v1.20，可空） | 受控端在「有对端等批准 / 有对端申请 / 会话开始结束」时主动 POST 一次通知（§6.10）；为空则关闭（默认即空）。与 `audit-server-url` **相互独立**：审计是记录（有重试与本地兜底），事件是提示（不重试、无兜底、3 秒硬上限）。鉴权由 URL 自带（桌面端不加请求头）。 |
| `api-cors-origin` | 逗号分隔的业务页面来源（v1.7，可空） | 本地 API 在 CORS 收紧策略下额外放行的来源（如 `http://192.168.1.10:3000`），与默认放行的 localhost/127.0.0.1 互补。 |
| `audio-input` | 录音设备名；留空 = 系统默认 | 抓声通道的**设备选择**，不是开关。留空时 Windows 上抓的是默认**输出**设备的 WASAPI loopback（本机系统声音），非空则按名字找设备、找不到就退回默认录音设备（无声卡输入的机器会因此启动失败）。本地 API 不写入此键；历史上被旧版 `/voice` 写进去的 `Y` 会在启动时自动清除（`common::drop_bogus_audio_input`）。 |
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
# 出站事件通知地址（可空，v1.20；见接口文档 §6.10）
# 为空则不发通知。鉴权由 URL 自带（桌面端不加请求头）
# 例：http://127.0.0.1:3000/api/event?key=replace_with_secret
# ==============================
event-server-url = ''

# ==============================
# 会话录像上传（v1.22；见接口文档 §6.11）
# chunked（默认）= 边录边传；whole = 录完一次发完
# 上传地址由上面的 audit-server-url 派生（源 + /api/record），不单独配置
# ==============================
# record-upload-mode = 'chunked'

# ==============================
# CORS 额外放行的业务页面来源（逗号分隔，可空，v1.7）
# 例：http://192.168.1.10:3000
# ==============================
api-cors-origin = ''

# ==============================
# 抓声设备（设备名，留空 = 系统默认）
# Windows 留空 = 默认输出设备的 loopback（系统声音）
# ==============================
# audio-input = 'Microphone (Realtek(R) Audio)'

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





