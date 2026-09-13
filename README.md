# EasyTier Manager

A small macOS desktop app for installing, configuring, controlling, and updating the EasyTier core
service without Homebrew. Built with Tauri v2 (Rust backend) and a React + Vite frontend.

## Development

```sh
pnpm install
pnpm tauri dev
```

## Build

```sh
pnpm tauri build
```

The bundle is written to `src-tauri/target/release/bundle/`.

## Layout

- `src/` — React UI. All backend access goes through the typed `invoke` wrappers in `src/api.ts`.
  `styles.css` carries the whole design system (oklch tokens, the `.pop` card material, components);
  `App.tsx` is the shell, `CompactCard.tsx` the small-window form and `views/` the expanded surfaces.
- `src-tauri/` — Rust backend. `app.rs` exposes the `#[tauri::command]` surface, `window.rs` owns the
  compact/expanded form factor, `release.rs` handles GitHub release lookup/download, `helper.rs` owns
  the privileged helper, `paths.rs` holds the managed paths and generated shell templates. EasyTier
  service registration and lifecycle operations use the open-source `service-manager` crate.

## Managed Paths

- App-managed root: `/Library/Application Support/EasyTier Manager`
- LaunchDaemon: `/Library/LaunchDaemons/com.ch3n4y.easytier-manager.plist`
- Logs: `/Library/Logs/EasyTier Manager/easytier.log`
- Helper LaunchDaemon: `/Library/LaunchDaemons/com.ch3n4y.easytier-manager.helper.plist`
- Helper socket: `/var/run/easytier-manager-helper.sock`

The log directory must stay traversable (`0755`) by the invoking user: the GUI tails the service log
directly instead of going through the root helper on every status poll. Both the install script and
`ensure_helper` re-apply that mode, because `mkdir -p` will not correct a directory an older install
left at `0744`.

## Window Modes

The window has two shapes. It opens as a compact at-a-glance card (status medallion, one primary
action) and expands into the rail + stage workbench; the rail's footer collapses it again. The
backend owns the native resize — `window.rs` keeps the window's visual center and clamps it into the
current monitor's work area — and the frontend only stamps `data-window-mode` on `<html>` so the CSS
can key the layout on it. The compact card is fixed-size; the expanded workbench is user-resizable.

## Privileged Operations

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
