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

## 尚未完成 / 需要真机验证

- **未在真机上跑过**：本仓库这版改动没有 Android SDK/NDK 环境，`cargo check` 只验证了桌面端与共享代码；Android 专属模块（`mobile_core.rs`、`platform/android.rs`、`app.rs` 的 `cfg(target_os = "android")` 分支）**尚未编译过**。
- 输入法遮挡：未处理 `visualViewport`，键盘弹起时底部输入框可能被盖。
- 横屏布局偏挤（日志面板最小 260px + 嵌套滚动）。
- 安全区（`env(safe-area-inset-*)`）、各家 ROM 的后台查杀策略需实机确认。
