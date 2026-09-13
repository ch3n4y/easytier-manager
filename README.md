# EasyTier Desktop

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
  `App.tsx` is the rail + views shell.
- `src-tauri/` — Rust backend. `app.rs` exposes the `#[tauri::command]` surface, `release.rs` handles
  GitHub release lookup/download, `helper.rs` owns the privileged helper, `paths.rs` holds the managed
  paths and generated shell templates. EasyTier service registration and lifecycle operations use
  the open-source `service-manager` crate.

## Managed Paths

- App-managed root: `/Library/Application Support/EasyTier Desktop`
- LaunchDaemon: `/Library/LaunchDaemons/com.ch3n4y.easytier-desktop.plist`
- Logs: `/Library/Logs/EasyTier Desktop/easytier.log`
- Helper LaunchDaemon: `/Library/LaunchDaemons/com.ch3n4y.easytier-desktop.helper.plist`
- Helper socket: `/var/run/easytier-desktop-helper.sock`

The log directory must stay traversable (`0755`) by the invoking user: the GUI tails the service log
directly instead of going through the root helper on every status poll. Both the install script and
`ensure_helper` re-apply that mode, because `mkdir -p` will not correct a directory an older install
left at `0744`.

## Privileged Operations

Privileged work runs through a root LaunchDaemon that re-invokes this same binary as
`easytier-desktop --helper <uid>`. It listens on a unix socket owned by the invoking user
(`chown` + mode `0600`), so only that user can submit scripts.

If the daemon cannot be installed, the app falls back to a one-shot macOS AppleScript authorization
dialog (`osascript ... with administrator privileges`). `Status.adminReady` / `Status.adminError`
report which path is currently in use.

The helper accepts typed EasyTier service operations and runs `service-manager` as root. The app no
longer builds or invokes EasyTier `launchctl` commands itself. Because the service uses `KeepAlive`,
**Stop** persistently unregisters its plist; **Start** regenerates the plist through
`service-manager` and starts it again. The helper daemon itself remains managed by launchd so the app
still has a reusable privilege boundary.

## Renamed from EasyTier Manager

The bundle, LaunchDaemon labels, socket and managed directories all moved from `EasyTier Manager` /
`com.ch3n4y.easytier*` to `EasyTier Desktop` / `com.ch3n4y.easytier-desktop*`. Because the generated
plists embed the install paths, the old daemons cannot simply be renamed — they have to be booted out
and reinstalled by the app:

```sh
sudo launchctl bootout system/com.ch3n4y.easytier 2>/dev/null || true
sudo launchctl bootout system/com.ch3n4y.easytier-manager.helper 2>/dev/null || true
sudo rm -f /Library/LaunchDaemons/com.ch3n4y.easytier.plist \
           /Library/LaunchDaemons/com.ch3n4y.easytier-manager.helper.plist \
           /var/run/easytier-manager-helper.sock
```

The service binaries, config and logs can be carried over instead of re-downloading:

```sh
sudo mv "/Library/Application Support/EasyTier Manager" "/Library/Application Support/EasyTier Desktop"
sudo mv "/Library/Logs/EasyTier Manager" "/Library/Logs/EasyTier Desktop"
sudo chmod 755 "/Library/Logs/EasyTier Desktop"
```

Then run **安装 EasyTier** once in the app so the service plist and wrapper are regenerated under the
new label.

## Tests

```sh
cd src-tauri && cargo test
```
