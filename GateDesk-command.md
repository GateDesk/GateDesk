# 运行desk
## 直接启动
```powershell
gatedesk.exe
```
启动更多界面

## 有界面的启动
```powershell
Start-Process -FilePath "gatedesk\target\release\gatedesk.exe" -WorkingDirectory "gatedesk"; Start-Sleep -Seconds 6; Get-Process gatedesk -ErrorAction SilentlyContinue | Select-Object Id,ProcessN
```


# GateDesk CLI 命令接口

> 整理自 `gatedesk/src/core_main.rs` 源码，实测环境：v1.5.0（Windows release，便携模式）。
> 实测：`--get-id` → `477091630`，`--build-date` → `2026-09-02 16:11`。

## 📋 信息查询（无需权限，实测可用）

```powershell
.\gatedesk.exe --version | Out-String      # 版本号 → 1.5.0
.\gatedesk.exe --build-date | Out-String   # 构建日期 → 2026-09-02 16:11
.\gatedesk.exe --get-id | Out-String       # 本机 ID → 477091630
```

> ⚠️ GUI 子系统程序，直接运行无输出，**必须用管道捕获**（`| Out-String` 或 `> file`）。

## 🔗 连接类（启动 GUI 窗口）

```powershell
.\gatedesk.exe --connect <id> <password>   # 远程控制
.\gatedesk.exe --play <id>                 # 仅观看
.\gatedesk.exe --file-transfer <id>        # 文件传输
.\gatedesk.exe --view-camera <id>          # 查看对方摄像头
.\gatedesk.exe --port-forward <id>         # 端口转发
.\gatedesk.exe --terminal <id>             # 远程终端
.\gatedesk.exe --rdp <id>                  # RDP 连接
# 可选附加参数：
#   --password <pwd>     直接带密码连接
#   --relay              强制走中继
#   --switch_uuid <uuid> 切换会话
```

## ⚙️ 配置管理（需已安装 + 管理员权限）

> 权限不满足时提示：`Installation and administrative privileges required!`

```powershell
.\gatedesk.exe --password <新密码>              # 设置永久密码
.\gatedesk.exe --set-id <新ID>                  # 修改本机 ID
.\gatedesk.exe --set-unlock-pin <PIN>           # 设置解锁 PIN
.\gatedesk.exe --option <key>                   # 读取选项值
.\gatedesk.exe --option <key> <value>           # 写入选项值
.\gatedesk.exe --import-config <config.toml>    # 导入配置文件
.\gatedesk.exe --config <加密串>                # 配置自定义服务器（key/host/api/relay）
.\gatedesk.exe --assign --token <t> --user_name <n> ...   # 设备分配到账号
.\gatedesk.exe --deploy --token <t> [--id <id>]           # API 部署设备
```

常用 option 示例：`custom-rendezvous-server`（ID 服务器）、`relay-server`、`api-server`、`key`。

## 🛠 安装/服务类

```powershell
.\gatedesk.exe --install                       # 安装（带 UI）
.\gatedesk.exe --silent-install [printer=1|0] [debug]   # 静默安装
.\gatedesk.exe --uninstall                     # 卸载
.\gatedesk.exe --install-service               # 安装系统服务
.\gatedesk.exe --uninstall-service             # 卸载系统服务
.\gatedesk.exe --service                       # 以服务方式运行
.\gatedesk.exe --server                        # 启动服务进程（主 IPC）
.\gatedesk.exe --tray                          # 启动托盘
.\gatedesk.exe --update                        # 检查更新
.\gatedesk.exe --noinstall                     # 便携模式（不安装）
```

## 🖥 驱动/外设类

```powershell
.\gatedesk.exe --install-idd                   # 安装虚拟显示器驱动
.\gatedesk.exe --uninstall-amyuni-idd          # 卸载 Amyuni 虚拟显示器
.\gatedesk.exe --install-remote-printer        # 安装远程打印机
.\gatedesk.exe --uninstall-remote-printer      # 卸载远程打印机
.\gatedesk.exe --uninstall-cert                # 卸载驱动证书
```

## 🔧 内部/其他（一般不手动使用）

| 参数 | 说明 |
|------|------|
| `--elevate` | 提权重启 |
| `--run-as-system` | 以 SYSTEM 运行 |
| `--quick_support` | 快速支持模式 |
| `--no-server` | 不启动服务 |
| `--portable-service` | 便携服务 |
| `--cm` | 连接管理器 |
| `--whiteboard` | 白板 |
| `--check-hwcodec-config` | 检查硬件编解码配置 |
| `--remove <file>` | 更新后清理文件 |
| `--after-install` / `--before-uninstall` | 安装/卸载钩子 |

## 📌 URL Scheme（也支持）

```text
gatedesk://connection/new/<id>?password=xxx&relay=true
```

## ⚠️ 注意事项

- 当前为便携运行（未 `--install`），配置管理类命令会因 `is_installed()` 为 `false` 被拒。
- `--get-id` / `--version` 等查询命令不受安装状态限制。
- release 版为 Windows GUI 子系统程序（`#![windows_subsystem = "windows"]`），`println!` 输出必须通过管道或重定向才能看到。






