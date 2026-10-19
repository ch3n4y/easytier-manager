# Android 版

安卓端不是"给桌面版加个平台目录"，因为桌面版的三根支柱在安卓上都不存在。这份文档说明换了什么、怎么构建、有哪些已知差异。

## 为什么架构不同

| | 桌面（Windows / macOS） | Android |
|---|---|---|
| 核心从哪来 | 从 GitHub release 下载 `easytier-core` 二进制 | **随 APK 链接进进程**（`easytier` crate） |
| 谁托管核心 | launchd / 服务控制管理器 | 应用自己 + `VpnService` 前台服务 + 常驻通知 |
| 怎么拿到 TUN | 核心自己开 `/dev/net/tun` 或 Wintun | 只能由 `VpnService.Builder.establish()` 拿 fd，再交给核心 |

三条原因决定了必须内嵌：

1. Android 没有 launchd/SCM 的等价物，`targetSdk` 29+ 也不能执行应用私有目录里的可执行文件；
2. EasyTier 官方 release **没有** Android 版核心二进制（只有 `app-*-release.apk`）；
3. 非 root 应用拿到 TUN 设备的唯一途径是 `VpnService`，它给的是一个 fd，只有同进程的核心才能用。

这套做法与官方 `easytier-gui` 的安卓端一致：`easytier::instance::factory::attach_mobile_tun_fd` 把 fd 接到运行中的实例上，`tauri-plugin-vpnservice` 负责 `VpnService` 与授权弹窗。

## 代码分布

- `src-tauri/src/mobile_core.rs` — 内嵌核心：配置服务器模式（等价 `easytier-core -w <url>`）与本地 `default.conf` 模式（等价 `-c`）、TUN 计划、fd 挂接。
- `src-tauri/src/platform/android.rs` — 安卓平台层：启停、配置、日志、`tracing` 落盘。
- `src-tauri/src/lib.rs` — 桌面与移动共用的入口；托盘、自更新、窗口尺寸等按 `cfg(desktop)` 编译剔除。
- `src-tauri/src/app.rs` — `android_tun_config` / `android_attach_tun_fd` 两个命令，以及与桌面共用的命令在安卓上的语义。
- `src/api.ts` / `src/App.tsx` — 前端编排：启动 → 取 TUN 计划 → 授权 → 建 VPN → 拿 fd 回传。
- `src/styles.css` 的 "Phone shell" 段 + `src/platform.ts` — 移动布局（底部标签栏、安全区、触控尺寸、底部弹出面板）。

## 构建前置

1. JDK 17+（`JAVA_HOME` 指向它；JDK 8 不行）
2. Android Studio：SDK Platform、Platform-Tools、NDK (side by side)、Build-Tools、Command-line Tools
3. `ANDROID_HOME`、`NDK_HOME` 指向上面两项
4. Rust 安卓目标：
   ```
   rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
   ```
5. 能访问 `github.com`：`easytier` / `easytier-core` / `tauri-plugin-vpnservice` 是 git 依赖（crates.io 上的 `easytier` 停在 2.0.3，没有移动端 TUN API）。国内网络常见做法是给 git 配代理，例如：
   ```
   git config --global http.proxy http://127.0.0.1:7890
   ```
   注意 `git` 的代理或 VPN 是必需的：`gh-proxy.com` 这类下载镜像只代理 HTTP 下载，不代理 git 协议（实测返回 502/403）。

## 首次生成工程

```
pnpm tauri android init
```

它会生成 `src-tauri/gen/android/`（Gradle 工程、`MainActivity`、清单合并配置）。生成后建议补一处：把 `MainActivity` 的返回键改成 `moveTaskToBack(true)`，否则返回键会直接结束应用，而桌面端的心智是"关窗不退出"。

权限由两处提供，不需要手改清单：

- `tauri-plugin-vpnservice` 自带的 `AndroidManifest.xml` 声明 `VpnService`（`BIND_VPN_SERVICE` + `android.net.VpnService` intent-filter）与前台服务类型；
- 本仓库的 `src-tauri/capabilities/mobile.json` 授权前端调用 `prepare_vpn` / `start_vpn` / `stop_vpn`。

## 构建与运行

```
pnpm tauri android dev            # 真机/模拟器调试，需要设备已开启 USB 调试
pnpm tauri android build          # 出 APK/AAB（arm64 主力）
```

签名：`src-tauri/gen/android/keystore.properties` + keystore 文件，不要在仓库里提交私钥。

## 运行时检查清单

首次点「启动」应依次看到：

1. 核心起来（`get_status` 的 `loaded=true`）；
2. 系统 VPN 授权弹窗（`prepare_vpn`）；
3. 常驻通知「EasyTier VPN is running」（`VpnService` 前台服务）；
4. `vpn_service_start` 事件带回 fd，随后核心开始跑流量（`running=true`）。

排查：

- 应用日志（日志面板读的就是它）：`<app_log_dir>/easytier.log`，同时包含核心的 `tracing` 输出；
- 核心 panic / 原生 stderr：`adb logcat -s RustStdoutStderr`；
- VPN 没起来：先看是否授予了通知权限（Android 13+ 需要 `POST_NOTIFICATIONS`），前台服务没通知会被杀。

## 与桌面版的已知差异

| 功能 | Android 上的语义 |
|---|---|
| 安装核心 | 无需安装，核心随 APK；「安装」等价于写入默认配置并启动 |
| 检查/更新核心 | 永远「已是最新」；`update_core` 返回明确提示：核心随应用升级 |
| 管理器更新 | 应用内下载 APK 并调起系统安装器，用户在系统弹窗确认。没有 Play 商店通道 |
| 托盘 | 不存在。常驻靠 `VpnService`/前台服务通知；移动端底栏不渲染「隐藏到托盘」 |
| 进程号 | 无独立进程，`pid` 恒为 0 |
| MTU | 由配置文件的 `mtu` 决定（默认配置写 1380）；配置里为 0 时回退到 1500。插件不做协商，`0` 会直接报 `Bad mtu` |
| 多地址 | 插件 `start_vpn` 只接受单个地址，因此传第一个地址；全部地址仍以路由形式覆盖 |
| 返回键 | 需按上文改 `MainActivity`，否则会退出应用而不是返回上一屏 |

## 状态：已通过编译器，尚未上机

**2026-10-19 更新：Android 目标已成功编译**（`cargo check --target aarch64-linux-android`
退出码 0）。此前"从未被编译器验证"的判断不再成立，但那一轮验证确实抓出了 5 类真实缺陷，
说明当时的担心是对的 —— 见下面的"首次编译修掉的问题"。

已在真机/模拟器之外验证：

- `cargo check --target aarch64-linux-android` 通过（0 error）；
- 桌面目标 `tauri build --no-bundle` 通过，无警告；
- `cargo test --lib` 30/30 通过（重构后测试在 lib target，不再是 bin）；
- `pnpm build`（tsc + vite）通过。

## 首次编译修掉的问题

这些是"人工核对通过、编译器判否"的实际缺陷，记录下来避免重犯：

1. **`attach_mobile_tun_fd` / `get_mobile_tun_config` / `MobileTunConfig` 在 v2.7.0 里根本不存在。**
   此前"已逐个比对源码确认存在"的结论是错的。真实 API 是
   `NativeInstanceManager::attach_tun_fd(instance_id, fd)`（单实例）加上
   `network_info(id)` 读回虚拟 IP 与路由，与官方 `easytier-gui` 的 `set_tun_fd` 一致。
   `mobile_core.rs` 因此重写为自行组装 `TunPlan`。
2. `window.rs` 的 `show_main_window` 桌面版缺 `#[cfg(desktop)]`，与安卓版重名（E0428/E0252）。
3. `release.rs` 的 `latest_asset` / `asset_name_for_version` 未限 `desktop`，安卓上找不到
   `platform_asset_tag`（E0425）。安卓内嵌核心、不下载二进制，这两个只应存在于桌面。
4. `platform/android.rs::write_config_server` 把 `server` move 进闭包后又使用（E0382）。
5. 工具链要求：`easytier v2.7.0` 要求 rustc ≥ 1.95，本机原为 1.92；且交叉编译需要
   `protoc` 与 NDK sysroot 的 bindgen 参数（见文末环境变量）。

## 真机验证进展（2026-10-19，Xiaomi 2211133C / Android 16）

首次在真机上安装并启动。已确认：

- APK 构建成功（`tauri android build --apk --target aarch64`，universal 55MB）；
- 安装后启动，**`.so` 加载成功**（`nativeloader: ... libeasytier_manager_lib.so ... ok`）；
- 顺带修掉一个只有真机才会暴露的崩溃，见下。

### 真机首启崩溃及修复

进程在启动后立即 `SIGABRT`：

```
No rustls crypto provider is configured. When using the `rustls-no-provider`
feature you must install a crypto provider before building a Client.
```

`reqwest` 以 `rustls-no-provider` 编译，但没有任何地方安装 provider，所以首个 HTTPS
客户端一构造就 panic。已在 `lib.rs::run()` 开头安装 `ring` provider（桌面与移动共用
该入口），并在 `Cargo.toml` 显式依赖 `rustls`。**编译期完全看不出这个问题**，只有运行时
才暴露 —— 这也印证了"能编译 ≠ 能跑"。

### 隧道打通（2026-10-19 续，Xiaomi 2211133C / Android 16）

**「启动」现在能真正建立隧道**：`running=true`，系统栏出现 VPN 图标，
隧道内 `ping 10.254.27.1` 返回 **0% 丢包、RTT 约 40ms**。

拿到这个结果前，启动流程在四个独立位置各失败一次，每修好一个才暴露下一个。
全部只在真机上可见，单元测试与编译都发现不了：

1. **capability 缺 `windows`**：Tauri 的 ACL 是按 `windows`/`webviews` 列表匹配的，
   列表为空 ≠ 全匹配。所以 `prepare_vpn` 被拒，报错还自相矛盾
   （`allowed on: [URL: local]` 却说 not allowed）。同时缺 `allow-registerListener`。
2. **`TauriVpnService` 未在清单声明**：插件只带了快捷开关磁贴的声明，服务本体要由宿主应用
   声明，否则 `start_vpn` 报 `Unable to start service ... not found`。
   还需要 `FOREGROUND_SERVICE`、`FOREGROUND_SERVICE_SPECIAL_USE`、`POST_NOTIFICATIONS`。
3. **MTU 必须显式传**：`setMtu(0)` 抛 `Bad mtu`。插件的
   `args?.getInt(MTU) ?: 1500` 对缺失的 key 不会回落到 1500（`getInt` 返回 0 而非 null），
   所以调用方一定要给值。现在用核心配置里的 MTU（1380）。
4. **fd 事件走插件通道**：Kotlin 用 `PluginHandle.trigger` 发出，全局 `listen`
   收不到，必须用 `addPluginListener('vpnservice', 'vpn_service_start', ...)`；
   而且要先 await 订阅再 `start_vpn` —— fd 在约 50ms 后就到，订阅没挂好就会丢。

另外：config server 模式冷启动时，核心先起来、网络列表几秒后才下发，
所以 `startTunnelAndroid` 会最多等 12 秒再判定"没有隧道地址"。

### 仍未验证

1. 各家 ROM 的后台查杀策略、真实 IME 行为，尚未在硬件上确认（安全区已在挖孔屏上确认）。
2. 通知文案/图标仍是插件默认（英文、系统图标）。

## 构建备忘（真机实测踩到的坑）

- `tauri android dev` 的 `beforeDevCommand` 需要 dev server 监听 `0.0.0.0`（已在
  `vite.config.ts` 设置 `server.host`），否则 CLI 一直卡在等待 `http://<LAN-IP>:1420`。
- `android-studio-script` 只能由 Android Studio / CLI 在线时调用；直接跑会因
  `WebSocket ConnectionRefused` 失败。Gradle 的 `rustBuild*` 任务走的就是它。
- **同一时间只能有一个 `tauri android dev`**。多个实例会卡在
  `Blocking waiting for file lock on Android` 且不会自愈，必须按 PID 逐个 `kill -9`。
- `tauri android build` 产出的是 **unsigned** APK，需自行 `zipalign` + `apksigner`。
- 交叉编译环境走仓库根 `.cargo/config.toml`（已 gitignore）：cargo 只从**当前目录向上**
  找 config，而 Gradle 在 `gen/android` 下调用，所以配置放在 `src-tauri/` 里不生效。

## 功能上明确没做的

| # | 缺口 | 影响 | 怎么补 |
|---|---|---|---|
| 1 | 管理器自更新（APK） | 目前点「检查更新」仍会走到后端提示；没有真正的升级路径 | updater 插件不支持移动端。改为跳应用商店或下载 APK 并 `Intent` 安装 |
| 2 | 返回键行为 | 未改 `MainActivity`，返回键会退出应用而不是回到上一屏 | `tauri android init` 之后在 `MainActivity` 里加 `onBackPressedDispatcher` + `moveTaskToBack(true)` |
| 3 | 核心版本升级流程 | `Cargo.toml` 里的 `tag = "v2.7.0"` 要手工 bump，没有"检查新核心版本"的机制 | 加一步：比对 EasyTier release tag 与当前固定 tag，提示用户升级应用 |
| 4 | VPN 真实状态未纳入轮询 | `running` 来自"fd 已挂接"的本地标记，用户从系统设置里断开 VPN 时 UI 不会立刻反映 | 轮询里加入插件 `get_vpn_status`，或在 `vpn_service_stop` 事件里主动置为停止 |
| 5 | 断线重连 / 开机自启 | 都没有 | 需要 `BOOT_COMPLETED` 接收器 + 重启策略 |
| 6 | 多地址 | 只把第一个地址交给插件（其 `StartVpnArgs` 只接受单个地址），其余地址靠路由覆盖 | 若要真多地址，需要改插件或用 `Builder.addAddress` 的多次调用 |
| 7 | MTU | 固定用插件默认 1500，未使用核心的 MTU | 核心的 `MobileTunConfig` 不含 MTU，需上游补充 |
| 8 | 前台服务通知文案/图标 | 沿用插件默认（英文 "EasyTier VPN is running" + 系统图标） | 在 `gen/android` 里替换字符串与图标，或换成自己的 VpnService |
| 9 | 日志时间戳 | 应用日志里只有生命周期行带 epoch 秒，核心的 tracing 行无时间戳 | 给 `tracing-subscriber` 开 `time` 特性并换成 `UtcTime` 定时器 |
| 10 | 输入法遮挡 | 键盘弹起时底部输入框可能被盖 | 接 `visualViewport` 调整底部内边距 |
| 11 | 横屏布局 | 可用但偏挤（日志面板最小 260px + 嵌套滚动） | 单独设计横屏构图 |
| 12 | 权限请求 | `POST_NOTIFICATIONS` 未做运行时请求 | Android 13+ 需在首次启动时申请，否则前台服务通知会被拒 |

## 仓库/CI 侧待补

1. **没有 Android CI job**：`.github/workflows/release.yml` 目前只有 `macos` 与 `windows`；需要新增带 SDK/NDK 的 job 并配置 keystore secrets。
2. **`src-tauri/gen/android/` 未提交**：按 Tauri 约定它由 `tauri android init` 生成，团队需要决定是像官方 `easytier-gui` 那样提交进库，还是每次生成后再打补丁（返回键、通知图标）。
3. **签名未配置**：`gen/android/keystore.properties` + keystore 需要自行准备，且不应入库。
4. **构建依赖网络**：三个 crate 是 git 依赖（`github.com`），构建机必须能访问，或配代理/VPN。crates.io 镜像与 `gh-proxy` 之类的下载镜像**不能**替代（git 协议不走它们）。

## 本地工具链（本次已装好并通过 android init）

- JDK 21：`scoop install temurin21-jdk`，`JAVA_HOME=D:\scoop\apps\temurin21-jdk\current`
- Android SDK：`D:\asdk`，含 cmdline-tools latest、platform-tools 37.0.1、platforms/android-35、build-tools 35.0.0、NDK 27.2.12479018。
  全部取自腾讯镜像 `https://mirrors.cloud.tencent.com/AndroidSDK/` —— `dl.google.com` 在本机直连与走代理都不通；`maven.google.com`（Gradle 拉 AGP）需走代理。
  包里 `source.properties` 里记录的 `hostOs` 决定要下 windows 变体（`*-win.zip` / `*_windows.zip`），取第一个 archive 会拿到 linux 包。
- SDK 许可：`sdkmanager --licenses` 需要访问 `dl.google.com`，改为手工写入 `D:\asdk\licenses\android-sdk-license`（内容为已知的 license 哈希）。
- 解包必须在**短路径**下进行：在 `C:\Users\...\.pi-desktop\scratch\...` 这类长路径下 `tar` 会以 `Invalid argument` 静默丢文件（`clang.exe`、`aapt2.exe`、`adb.exe` 都会缺失）。
- Rust 目标：`rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android` 已执行。
- `pnpm tauri android init` 已成功，产物 `src-tauri/gen/android/` 已入库（含 Gradle wrapper）。

交叉编译 C 依赖时 cc-rs 不会自动定位 NDK，必须显式指定（APK 的 minSdk 是 24，故用
`android24` 包装脚本）。bindgen（`kcp-sys` 等）还需要 sysroot 的 include 路径，否则报
`stdlib.h not found`。macOS 主机的可用配方：

```
NDK=~/Library/Android/sdk/ndk/27.2.12479018
prebuilt=$NDK/toolchains/llvm/prebuilt/darwin-x86_64
BIN=$prebuilt/bin
SYS=$prebuilt/sysroot

env \
  CC_aarch64_linux_android=$BIN/aarch64-linux-android24-clang \
  CXX_aarch64_linux_android=$BIN/aarch64-linux-android24-clang++ \
  AR_aarch64_linux_android=$BIN/llvm-ar \
  CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$BIN/aarch64-linux-android24-clang \
  PROTOC=$(which protoc) \
  "BINDGEN_EXTRA_CLANG_ARGS_aarch64-linux-android=--sysroot=$SYS -I$SYS/usr/include -I$SYS/usr/include/aarch64-linux-android" \
  cargo check --target aarch64-linux-android
```

注意 `BINDGEN_EXTRA_CLANG_ARGS_aarch64-linux-android` 含连字符，bash 的 `export` 不接受，
必须用 `env` 传入。

其他前置：

- `rustup toolchain install 1.99.1` + `rustup override set 1.99.1`（`easytier v2.7.0` 要求
  rustc ≥ 1.95），并 `rustup target add --toolchain 1.99.1 aarch64-linux-android`；
- `brew install protobuf`（`prost-wkt-types` 构建脚本需要 `protoc`）。

**已达成**：上述配方下 `cargo check --target aarch64-linux-android` 退出码 0，安卓侧代码
现在经过了编译器验证。剩下的未知项只有真机运行（见上文"仍未验证"）。


