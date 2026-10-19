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
| 管理器更新 | 不支持应用内自更新（updater 插件只支持桌面）。当前保留按钮并给出提示，后续应改为跳转商店/下载 APK |
| 托盘 | 不存在。常驻靠 `VpnService`/前台服务通知；移动端底栏不渲染「隐藏到托盘」 |
| 进程号 | 无独立进程，`pid` 恒为 0 |
| MTU | 插件默认 1500（核心的 `MobileTunConfig` 不含 MTU） |
| 多地址 | 插件 `start_vpn` 只接受单个地址，因此传第一个地址；全部地址仍以路由形式覆盖 |
| 返回键 | 需按上文改 `MainActivity`，否则会退出应用而不是返回上一屏 |

## 状态：能编译的是哪部分，哪些没验证

必须说清楚：**这次改动里，Android 目标一次都没有被编译过**，因为本机没有可用的安卓工具链。

已在 Windows 桌面目标上真实跑过：

- `cargo check` 通过；`cargo test` 21/21 通过；
- `pnpm build`（tsc + vite）通过；
- 所用到的 `easytier` API 已逐个比对 v2.7.0 源码确认存在（含签名）；
- 前端手机布局用无头 Chrome 在 390×844 / 844×390 跑过；桌面 980×660 删除新增规则后计算样式差异为 0。

## 未验证（阻断级，必须先解决）

1. **Android 目标未编译**：`pnpm tauri android init` 直接被拒（本机无 Android SDK/NDK），且 `JAVA_HOME` 指向 JDK 8（需 17+）。因此 `cfg(target_os = "android")` 的全部代码**只经过人工核对，没经过编译器**。
2. 具体涉及：`src-tauri/src/mobile_core.rs` 全文、`src-tauri/src/platform/android.rs` 全文、`src-tauri/src/app.rs` 的 Android 分支与 `TunPlan`、`src-tauri/src/lib.rs` 的 Android `setup` 段、`paths.rs` / `service.rs` / `window.rs` 的 Android 分支。
   首次编译最可能出问题的地方按概率排序：`run_web_client` 的参数与返回、`MachineIdOptions` 的字段、`TomlConfig::new_from_str` 的错误类型、`attach_mobile_tun_fd` 的 `Option<&[Uuid]>`、`tracing-subscriber` 的 `fmt::layer().with_writer()` 泛型。
3. **没在真机/模拟器上运行过**：VPN 授权弹窗、`vpn_service_start` 是否带回 fd、`attach_mobile_tun_fd` 是否成功、隧道是否真的通流量 —— 全未验证。
4. 安全区（`env(safe-area-inset-*)`）、各家 ROM 的后台查杀策略、真实 IME 行为，均未在硬件上确认。

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

