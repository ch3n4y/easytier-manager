# EasyTier Manager

A small desktop app for installing, configuring, controlling, and updating the EasyTier core
service without Homebrew and without hand-writing service definitions. Built with Tauri v2 (Rust
backend) and a React + Vite frontend, and shipped for macOS and Windows.

## Development

```sh
pnpm install
pnpm tauri dev
```

On Windows the release build embeds a `requireAdministrator` manifest, so the packaged app raises a
UAC prompt at launch. That is expected: the app registers and drives a system service, so it needs
elevation. The manifest is deliberately attached to release builds only — test harnesses are
separate executables built from the same crate, and an unconditional requirement would make
`cargo test` unlaunchable from an ordinary shell. During development, start `pnpm tauri dev` from an
elevated terminal.

## Build

```sh
pnpm tauri build
```

The bundle is written to `src-tauri/target/release/bundle/` — `dmg/` on macOS, `nsis/` on
Windows. Platform-specific bundling is configured in `src-tauri/tauri.macos.conf.json` and
`src-tauri/tauri.windows.conf.json`, which overlay the shared `tauri.conf.json`.

## Layout

- `src/` — React UI. All backend access goes through the typed `invoke` wrappers in `src/api.ts`.
  `styles.css` carries the whole design system (oklch tokens, the `.pop` card material, components);
  `App.tsx` is the shell and `views/` holds the surfaces.
- `src-tauri/` — Rust backend.
  - `app.rs` exposes the `#[tauri::command]` surface and holds the shared app/admin state.
  - `platform/` owns the privileged operations, split into `macos.rs` (helper-based) and
    `windows.rs` (in-process, elevated).
  - `service.rs` is the cross-platform service dispatcher; `launchd.rs` (macOS) and `winservice.rs`
    (Windows, which also hosts the service process itself) implement the platform specifics.
  - `paths.rs` holds the managed paths and, on macOS, the generated shell/plist templates.
  - `release.rs` handles GitHub release lookup/download and staging the core binaries.
  - `app_update.rs` checks, verifies and installs the manager's own releases.
  - `settings.rs` persists user-level settings, currently the download accelerator.
  - `tray.rs` owns the system tray icon and its menu.
  - `helper.rs` (macOS only) owns the privileged LaunchDaemon helper.

## Managed Paths

### macOS

- App-managed root: `/Library/Application Support/EasyTier Manager`
- LaunchDaemon: `/Library/LaunchDaemons/com.ch3n4y.easytier-manager.plist`
- Logs: `/Library/Logs/EasyTier Manager/easytier.log`
- Helper LaunchDaemon: `/Library/LaunchDaemons/com.ch3n4y.easytier-manager.helper.plist`
- Helper socket: `/var/run/easytier-manager-helper.sock`

The log directory must stay traversable (`0755`) by the invoking user: the GUI tails the service
log directly instead of going through the root helper on every status poll. Both the install script
and `ensure_helper` re-apply that mode, because `mkdir -p` will not correct a directory an older
install left at `0744`.

### Windows

- App-managed root: `%ProgramData%\EasyTier Manager`
- Binaries: `%ProgramData%\EasyTier Manager\bin` (the four EasyTier `.exe` files plus its bundled
  runtime helpers — `wintun.dll`, and `Packet.dll` / `WinDivert64.sys` when the release ships them)
- Config: `%ProgramData%\EasyTier Manager\config\{default.conf, service.env}`
- Logs: `%ProgramData%\EasyTier Manager\logs`
- Service name: `EasyTierManager` (display name `EasyTier Manager`), running as `LocalSystem`

The service process is this same binary re-invoked as `easytier-manager --service-host`. It runs the
core as a child and streams the child's stdout/stderr into `logs\easytier.log` — the same trick the
macOS shell wrapper uses. Registering `easytier-core.exe` with the SCM directly does **not** work for
logging: EasyTier's `--file-log-level` / `--file-log-dir` flags create the file but never write to
it, and a service has no stdout to fall back on. The host resolves the console address at start
time, so changing it only needs a restart.

`wintun.dll` ships inside the EasyTier release zip and must be installed next to the core or the
virtual adapter cannot be created; `Packet.dll` / `WinDivert64.sys` back the packet-capture features
and are installed when the release ships them.

## Window

One shape: the rail + stage workbench, frameless, with the custom chrome doubling as the title bar
and double-click on a drag region toggling maximize. `window.rs` sizes the window against the
monitor it opens on — the configured 980×660 gives way to the work area when the display is small or
scaled, because a window whose edges sit off-screen is worse than a cramped one — and keeps its
center while doing so.

The window is created hidden, and the shell reveals it once it has painted: startup would otherwise
show an empty dark frame and then visibly reshape it. The backend unveils the window anyway after
five seconds, so a webview that fails to load cannot leave the app invisible.

## System Tray

A tray icon (`tray.rs`) is present for as long as the app runs, and its tooltip mirrors the service
state — `EasyTier Manager · 运行中 v2.5.0`, `· 已停止`, or `· 未安装` — so hovering it answers "is it
still running?". The menu offers 显示窗口, 启动服务 / 停止服务 / 重启服务, and 退出.

Closing the window therefore **hides it to the tray** instead of quitting: the app has to stay alive
for the icon to mean anything, and hiding is what makes it possible to keep managing the service
without a window on screen. Use 退出 on the tray menu to actually quit.

## Privileged Operations

### macOS

Privileged work runs through a root LaunchDaemon that re-invokes this same binary as
`easytier-manager --helper <uid>`. It listens on a unix socket owned by the invoking user
(`chown` + mode `0600`), so only that user can submit scripts.

If the daemon cannot be installed, the app falls back to a one-shot macOS AppleScript authorization
dialog (`osascript ... with administrator privileges`). `Status.adminReady` / `Status.adminError`
report which path is currently in use. Each operation performs at most one authorization prompt, and
a cancelled prompt aborts instead of re-asking.

The helper accepts typed EasyTier service operations and runs `service-manager` as root. The app no
longer builds or invokes EasyTier `launchctl` commands itself. Because the service uses `KeepAlive`,
**Stop** persistently unregisters its plist; **Start** regenerates the plist through
`service-manager` and starts it again. The helper daemon itself remains managed by launchd so the app
still has a reusable privilege boundary.

### Windows

Windows has no reusable privilege boundary: the app runs elevated for its whole lifetime (see the
manifest in `src-tauri/windows-app.manifest`), so every operation executes in-process. The
"授权" panel reports administrator status rather than a pending prompt.

The managed service is registered against this binary in `--service-host` mode rather than against
`easytier-core.exe`, so that the core's output can be captured (see above). Because the SCM keeps a
stopped service registered, **Stop** simply stops it — unlike macOS, nothing is uninstalled — and
**Start** starts it again. Installing or updating the core writes into `%ProgramData%`, so the app
must remain elevated for those actions to succeed.

## Downloads

Release assets are tens of megabytes and GitHub is frequently slow or blocked, so requests go
through an accelerator prefix that can be changed under **设置 → 下载加速地址**. It defaults to
`https://gh-proxy.com/`; the direct URL and a built-in mirror are still tried as fallbacks, and each
request has a connect timeout, a stall timeout and an overall timeout so a throttled connection
fails over instead of hanging. Both the release lookup and the asset download stream their progress
to the UI.

## Updates

The manager and the EasyTier core have separate release channels. The core is fetched from the
EasyTier project's own releases as described above; the manager updates itself.

**设置 → 关于** checks the manager's release channel and offers the new version when there is one.
The package is downloaded through the same accelerator chain as the core, so a network that cannot
reach GitHub directly can still update, and it is verified against the minisign public key in
`tauri.conf.json` before anything is installed. The private half of that pair never leaves CI: it is
the `TAURI_SIGNING_PRIVATE_KEY` repository secret, and the release workflow uses it to sign the
update packages — `.app.tar.gz` on macOS, and the NSIS installer itself on Windows, which under the
default `createUpdaterArtifacts` mode *is* the update bundle rather than the `.nsis.zip` that v1
produced. Their signatures end up in the generated `latest.json` manifest. The key is stored without
a password, but the workflow still sets `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to the empty string —
leaving it unset makes the signer stop at an interactive prompt that nothing answers.

On macOS the bundle is replaced in place and the app relaunches itself. On Windows the NSIS
installer takes over and the process exits, coming back once the install finishes.

Both platforms run the core as a service, and on Windows that service *is* this binary, re-invoked
as `--service-host`. The installer kills every process carrying the binary's name before it replaces
the file, so an update would otherwise leave the core stopped with nothing left to notice it had
been up. The restore lives in the installer rather than in the app —
`src-tauri/installer/windows-hooks.nsh`, wired up through `bundle.windows.nsis.installerHooks`: it
stops the core deliberately before the swap and starts it again afterwards, but only if it was
running to begin with. That placement is the point. The build doing the updating is the one being
replaced, so it cannot arrange its successor's behaviour, whereas a *new* installer runs on every
update — including the one that first ships the hook; an app-side fix would only have taken effect
from the update after that. macOS runs the core under its own LaunchDaemon wrapper, which an update
never touches, so it has nothing to restore.

A release has to bump `version` in `tauri.conf.json` to match the tag it is built from — the updater
compares the two, and `latest.json` is stamped with the tag.

## Renamed from EasyTier Desktop

The bundle, LaunchDaemon labels, socket and managed directories all moved from
`EasyTier Desktop` / `com.ch3n4y.easytier-desktop*` to `EasyTier Manager` /
`com.ch3n4y.easytier-manager*`. Because the generated plists embed the install paths, the old
daemons cannot simply be renamed — they have to be booted out and removed by hand:

```sh
sudo launchctl bootout system/com.ch3n4y.easytier-desktop 2>/dev/null || true
sudo launchctl bootout system/com.ch3n4y.easytier-desktop.helper 2>/dev/null || true
sudo rm -f /Library/LaunchDaemons/com.ch3n4y.easytier-desktop.plist \
           /Library/LaunchDaemons/com.ch3n4y.easytier-desktop.helper.plist \
           /var/run/easytier-desktop-helper.sock
```

The service binaries, config and logs can be carried over instead of re-downloading:

```sh
sudo mv "/Library/Application Support/EasyTier Desktop" "/Library/Application Support/EasyTier Manager"
sudo mv "/Library/Logs/EasyTier Desktop" "/Library/Logs/EasyTier Manager"
sudo chmod 755 "/Library/Logs/EasyTier Manager"
```

Then run **安装 EasyTier** once in the app so the service plist and wrapper are regenerated under the
new label.

## Tests

```sh
cd src-tauri && cargo test
```

Tests are platform-aware: the macOS-only helper/launchd cases run on macOS, and the Windows service
argument construction runs on Windows.
