# RustDesk Windows 编译说明

## 1、结果

已成功构建并运行。以下用 `<PROJECT_ROOT>` 表示 RustDesk 项目根目录：

```text
<PROJECT_ROOT>\flutter\build\windows\x64\runner\Release\rustdesk.exe
```

## 2、关键版本

| 工具                | 版本                    |
| ------------------- | ----------------------- |
| Windows             | 10，Build 19045.6466    |
| Flutter             | 3.24.5                  |
| Dart                | 3.5.4                   |
| Rust / Cargo        | 1.98.0                  |
| Python              | 3.11.8                  |
| Visual Studio       | 2022 Community 17.14.19 |
| Windows SDK         | 10.0.26100.0            |
| LLVM                | 15.0.6                  |
| flutter_rust_bridge | 1.80.1                  |
| ffigen              | 8.0.2                   |
| extended_text       | 14.0.0                  |

## 3、关键依赖

`<PROJECT_ROOT>/flutter/pubspec.yaml` 中保持：

```yaml
extended_text: 14.0.0
flutter_rust_bridge: "1.80.1"
ffigen: ^8.0.2
```

不要使用 `extended_text 13.0.0`，它与 Flutter 3.24.5 不兼容。

## 4、修改内容

### `build.py`

本次实际修改的是 `<PROJECT_ROOT>/build.py`（当前文件约 1197 行）。在
`ffi_bindgen_function_refactor()` 中增加 Windows bridge 兼容处理，并在
`build_flutter_windows()` 中调用它。

核心修改如下：

```python
generated_bridge = pathlib.Path('flutter/lib/generated_bridge.dart')
if windows and generated_bridge.exists():
    content = generated_bridge.read_text()
    content = content.replace(
        'typedef bool = ffi.NativeFunction<ffi.Int Function(ffi.Pointer<ffi.Int>)>;',
        '')
    content = content.replace('ffi.Pointer<bool>', 'ffi.Bool')
    content = content.replace('ffi.Pointer<NativeBool>', 'ffi.Bool')
    content = content.replace('ffi.Bool', 'bool')
    content = re.sub(
        r'ffi\.NativeFunction<.*?>>',
        lambda match: match.group(0).replace('bool', 'ffi.Bool'),
        content,
        flags=re.DOTALL)
    content = content.replace(
        'void store_dart_post_cobject(\n    int ptr,',
        'void store_dart_post_cobject(\n    DartPostCObject ptr,')
    content = content.replace(
        'ffi.NativeFunction<ffi.Void Function(ffi.Int)>>(\n'
        "          'store_dart_post_cobject');",
        'ffi.NativeFunction<ffi.Void Function(DartPostCObject)>>(\n'
        "          'store_dart_post_cobject');")
    content = content.replace(
        '.asFunction<void Function(int)>();\n\n  Dart_Handle get_dart_object(',
        '.asFunction<void Function(DartPostCObject)>();\n\n'
        '  Object get_dart_object(')
    content = content.replace(
        'int new_dart_opaque(\n    Dart_Handle handle,',
        'int new_dart_opaque(\n    Object handle,')
    content = content.replace(
        'return _new_dart_opaque(\n      handle,\n    );',
        'return _new_dart_opaque(\n      handle as Dart_Handle,\n    );')
    content = content.replace(
        'external ffi.Pointer<ffi.Int> ptr;\n\n  @ffi.Int()\n  external int len;\n}\n\nfinal class wire_int_32_list',
        'external ffi.Pointer<ffi.Uint8> ptr;\n\n  @ffi.Int()\n  external int len;\n}\n\nfinal class wire_int_32_list')
    content = content.replace(
        'final class wire_int_32_list extends ffi.Struct {\n'
        '  external ffi.Pointer<ffi.Int> ptr;',
        'final class wire_int_32_list extends ffi.Struct {\n'
        '  external ffi.Pointer<ffi.Int32> ptr;')
    content = content.replace(
        'ffi.NativeFunction<ffi.Bool Function(DartPort',
        'ffi.NativeFunction<ffi.Uint8 Function(DartPort')
    generated_bridge.write_text(content)
    return
```

Windows 构建入口方法`build_flutter_windows`增加：

```python
ffi_bindgen_function_refactor()
os.chdir('flutter')
system2('flutter build windows --release')
```

脚本现在会在 Windows 构建前自动修正：

- 错误覆盖 Dart 内置 `bool` 的 FFI typedef
- `DartPostCObject` 和 `Dart_Handle` 类型
- `wire_uint_8_list` 和 `wire_int_32_list` 指针类型
- Windows 下 Dart port 回调的返回类型

这样可以避免手动修改生成的 Dart 文件，并保证每次执行 `build.py` 时自动应用修正。

修改前，Windows 构建流程在 `cargo build` 后直接执行：

```python
os.chdir('flutter')
system2('flutter build windows --release')
```

修改后，在两条语句之间增加：

```python
ffi_bindgen_function_refactor()
```

因此修正发生在 Flutter 编译之前，而不是编译失败后手动改生成文件。

### `pubspec.yaml`

当前工作区中的声明为：

```yaml
flutter_rust_bridge: "1.80.1"
extended_text: 14.0.0
ffigen: ^8.0.2
```

这些声明目前已经是正确版本；本次未提交的 `pubspec.yaml` 没有再次修改。
其中 `extended_text` 必须使用 `14.0.0`。`13.0.0` 缺少当前 Flutter SDK 使用的
`TextGranularity.paragraph` 和 `SelectionEventType.selectParagraph`，会导致编译失败。

### `pubspec.lock`

本次执行 `flutter pub get` 后，`<PROJECT_ROOT>/flutter/pubspec.lock` 发生了依赖解析更新
（当前差异为约 325 行新增、293 行删除）。它记录的是实际解析结果，不是手工修复文件。

关键对应关系如下：

| `pubspec.yaml` 声明           | `pubspec.lock` 实际版本 | 关系                   |
| ------------------------------- | ------------------------: | ---------------------- |
| `flutter_rust_bridge: 1.80.1` |                `1.80.1` | 直接依赖，版本一致     |
| `extended_text: 14.0.0`       |                `14.0.0` | 直接依赖，版本一致     |
| `ffigen: ^8.0.2`              |                 `8.0.2` | 直接开发依赖，满足约束 |
| `ffi: ^2.1.0`                 |                 `2.1.3` | 直接依赖，满足约束     |
| `freezed: ^2.5.2`             |                 `2.5.2` | 直接开发依赖，版本一致 |

lock 文件中的对应内容示例：

```yaml
extended_text:
  dependency: "direct main"
  version: "14.0.0"
ffi:
  dependency: "direct main"
  version: "2.1.3"
ffigen:
  dependency: "direct dev"
  version: "8.0.2"
flutter_rust_bridge:
  dependency: "direct main"
  version: "1.80.1"
freezed:
  dependency: "direct dev"
  version: "2.5.2"
```

例如 lock 文件第 11 行的：

```yaml
version: "67.0.0"
```

属于传递依赖 `_fe_analyzer_shared`，不是项目版本，也不是
`flutter_rust_bridge` 的版本。对应的分析器依赖为 `analyzer 6.4.1`。

lock 文件中的其他变化主要来自：

- 依赖求解器根据当前 Flutter/Dart SDK 重新选择传递依赖
- `pub.dev` 解析源切换为 `pub.flutter-io.cn`
- 传递依赖的版本、哈希值和下载地址同步更新

因此，`pubspec.yaml` 决定允许哪些版本，`pubspec.lock` 记录本次实际选中的版本。
换环境时应优先使用仓库中的 lock 文件，避免依赖版本漂移。

### `pubspec.lock` 的完整版本变化

下面只列出版本发生变化、新增或删除的包；未列出的包只发生了下载源或哈希变化，
没有发生版本变化：

```text
_fe_analyzer_shared       72.0.0       -> 67.0.0
_macros                    0.3.2        -> 删除
analyzer                   6.7.0        -> 6.4.1
async                      2.13.0       -> 2.11.0
auto_size_text_field       2.2.4        -> 2.3.0
boolean_selector           2.1.2        -> 2.1.1
build_runner               2.4.13       -> 2.4.11
build_runner_core          7.3.2        -> 7.3.1
built_value                8.10.1       -> 8.13.0
code_builder               4.10.1       -> 4.10.0
crypto                     3.0.6        -> 3.0.7
dart_style                 2.3.7        -> 2.3.6
dbus                       0.7.11       -> 0.7.12
equatable                  2.0.7        -> 2.1.0
fake_async                 新增          -> 1.3.1
freezed                    2.5.7        -> 2.5.2
get                        4.7.2        -> 4.7.3
glob                       2.1.3        -> 2.2.0
google_fonts               6.2.1        -> 6.3.0
http                       1.4.0        -> 1.6.0
image_picker_android       0.8.12+21    -> 0.8.12+12
io                         1.0.5        -> 1.1.0
leak_tracker               新增          -> 10.0.5
leak_tracker_flutter_testing 新增       -> 3.0.5
leak_tracker_testing       新增          -> 3.0.1
macros                     0.1.2-main.4  -> 删除
matcher                    0.12.17      -> 0.12.16+1
mime                       2.0.0        -> 1.0.6
path_provider_android      2.2.15       -> 2.2.10
pool                       1.5.1        -> 1.5.3
provider                   6.1.5        -> 6.1.5+1
pub_semver                 2.2.0        -> 2.2.1
source_span                1.10.1       -> 1.10.0
sqflite_common             2.5.4+6      -> 2.5.4
stack_trace                1.12.1       -> 1.11.1
stream_channel             2.1.4        -> 2.1.2
stream_transform           2.1.1        -> 2.1.2
string_scanner             1.4.1        -> 1.2.0
synchronized               3.3.0+3      -> 3.1.0+1
term_glyph                 1.2.2        -> 1.2.1
test_api                   0.7.6        -> 0.7.2
typed_data                 1.4.0        -> 1.3.2
url_launcher_android       6.3.14       -> 6.3.9
video_player_android       2.7.16       -> 2.7.1
vm_service                 新增          -> 14.2.5
wakelock_plus_platform_interface 1.2.3 -> 1.3.0
watcher                   1.1.2        -> 1.2.1
win32                     5.10.1       -> 5.5.4
yaml                       3.1.3        -> 3.1.4
yaml_edit                  2.2.2        -> 2.2.4
```

另外，绝大多数 hosted 包的：

```yaml
url: "https://pub.dev"
```

变为：

```yaml
url: "https://pub.flutter-io.cn"
```

对应的 `sha256` 也随下载源重新记录。这个变化是镜像源变化，不代表每个包的代码都被手工修改。

#### 与本次编译错误的直接关系

只有以下 lock 条目直接对应本次问题：

1. `extended_text 14.0.0`：解决 Flutter 3.24.5 缺少旧版本兼容的问题。
2. `flutter_rust_bridge 1.80.1`：决定 bridge 生成代码所使用的版本。
3. `ffigen 8.0.2` 和 `ffi 2.1.3`：决定 FFI 类型生成和 Dart FFI API。
4. `analyzer 6.4.1`、`_fe_analyzer_shared 67.0.0`、`build_runner 2.4.11`：
   影响 Freezed/build_runner 的代码生成，但不是 Windows bridge 修正逻辑本身。

其余 lock 变化是依赖求解结果或镜像元数据变化，不是修复 `bool`/`NativeFunction`
错误的直接原因。真正修复该错误的是 `<PROJECT_ROOT>/build.py` 中的 Windows bridge
兼容处理。

### 自动生成文件

以下文件由 bridge、Freezed 和 build_runner 生成，不建议手工修改：

```text
<PROJECT_ROOT>\flutter\lib\generated_bridge.dart
<PROJECT_ROOT>\flutter\lib\generated_bridge.freezed.dart
```

## 5、编译步骤

```powershell
cd <PROJECT_ROOT>\flutter
flutter clean
flutter pub get
dart run build_runner build --delete-conflicting-outputs

cd ..
python3 build.py --flutter
```

`<PROJECT_ROOT>/build.py` 不负责创建 `generated_bridge.dart`。它只会在文件已经存在时，
对生成结果执行 Windows FFI 兼容修正。因此，首次构建或文件不存在时，必须先手工运行
`flutter_rust_bridge_codegen`。

生成文件后，后续执行 `build.py` 会自动处理该文件：

```text
已有 generated_bridge.dart
        |
        v
build.py 执行 Windows FFI 兼容修正
        |
        v
flutter build windows --release
```

如果 `flutter/lib/generated_bridge.dart` 不存在，`build.py` 不会自动创建它，
也不会自动调用 `flutter_rust_bridge_codegen`。

首次生成、Rust FFI 接口变化，或 bridge 文件被删除时，执行：

```powershell
cd <PROJECT_ROOT>
flutter_rust_bridge_codegen `
  --rust-input .\src\flutter_ffi.rs `
  --dart-output .\flutter\lib\generated_bridge.dart `
  --llvm-path "<LLVM_ROOT>" `
  --no-build-runner
dart run build_runner build --delete-conflicting-outputs
python3 build.py --flutter --skip-cargo --skip-portable-pack
```

## 6、常见问题

- `extended_text` 报 `TextGranularity.paragraph` 或 `selectParagraph`：确认版本为 `14.0.0`。
- `bool` 与 `NativeFunction` 类型冲突：不要直接执行 `flutter build windows`，使用 `python3 build.py --flutter`。
- `flutter clean` 不会修复错误的 bridge；需要重新生成后再运行 `build.py`。
- `generated_bridge.dart` 和 `generated_bridge.freezed.dart` 被 `.gitignore` 忽略，换环境时需要重新生成。

## Fork 项目注意事项

如果项目是 RustDesk 的 fork，Cargo 包名可能不是 `rustdesk`。例如包名为
`gatedesk` 时，`flutter_rust_bridge_codegen` 会生成：

```dart
GatedeskImpl
```

而 RustDesk 原有 Flutter 代码可能仍引用：

```dart
RustdeskImpl
```

这种情况下需要在 `generated_bridge.dart` 中增加兼容别名：

```dart
typedef RustdeskImpl = GatedeskImpl;
```

本项目已在 `<PROJECT_ROOT>/build.py` 中加入自动注入逻辑；只要生成结果包含
`GatedeskImpl`，Windows 构建前会自动添加该别名。当前生成文件中也应能看到同一行。

另外，`common.dart` 中对可空窗口位置采用显式空值判断后再调用
`LastWindowPosition.loadFromString`，避免将 `String?` 传给要求 `String` 的函数。
