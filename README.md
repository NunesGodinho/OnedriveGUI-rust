# OnedriveLinux

A GUI for the [OneDrive Client for Linux](https://github.com/abraunegg/onedrive) with multi-account
support. This is a Rust port of the Python/Qt [OneDriveGUI](https://github.com/bpozdena/OneDriveGUI),
built with [egui/eframe](https://github.com/emilk/egui), the same UI stack and version (0.36) as PdfCraft.
It builds into one self-contained executable with no Python or Qt runtime. Current version: 1.0.0.

It reads and writes the same files as the Python app (`~/.config/onedrive-gui/`), so you can switch
between the two without losing your profiles.

## Features

- Manage several OneDrive, OneDrive for Business and SharePoint library profiles.
- Watch every profile live, each with its own `onedrive --monitor` process: status, account type,
  free space, errors (full text in a tooltip), and a list of recent uploads, downloads and deletions
  with progress bars.
- Setup wizard: create a profile, import an existing client config, or add a SharePoint shared
  library (it lists the sites and libraries for you).
- Profile settings for the client options, grouped as in the original: monitored files, sync options,
  excluded files, selective sync (`sync_list`), webhooks and logging. Options that don't work
  together are disabled, and there are tooltips for most options.
- Auto-sync selected profiles when the GUI starts. Optionally start the GUI on login (XDG autostart).
- A system tray icon that shows the combined state (idle, syncing, stopped, error), with a tooltip
  listing every profile's status and a Show/Hide, Settings and Quit menu.
- Asks before a resync (with a 5-second countdown) and before a big delete.
- Login through the browser: the client's in-browser flow (v2.5.11+), or a response URL you paste
  in for older flows and expired sessions.
- Warns when the OneDrive client is missing, too old (below v2.5.11) or out of date.

## Requirements

- **The OneDrive Client for Linux, v2.5.11 or newer**, installed by following its
  [install instructions](https://github.com/abraunegg/onedrive/blob/master/docs/install.md).
  The GUI runs `onedrive` from your `PATH`; you can set another path in the GUI settings.
- **To build:** Rust 1.95 or newer (`cargo`) and a C compiler (`cc`, for the TLS library).
  No GTK, Qt, D-Bus or X11 development packages are needed: windows open through winit, which
  loads X11/Wayland at runtime, and the tray talks D-Bus in pure Rust.
- **Optional:** a StatusNotifierItem tray host (built into KDE Plasma, XFCE, Cinnamon and others;
  on GNOME install the *AppIndicator and KStatusNotifierItem Support* extension), `xdg-open` to
  open folders, and `notify-send` for the login notification.

## Build and launch

Get the source, then build from its folder:

```sh
git clone https://github.com/NunesGodinho/OnedriveGUI-rust.git
cd OnedriveGUI-rust
```

```sh
# Build and start in one step
cargo run --release

# Or build once, then start the executable
cargo build --release
./target/release/onedrivelinux
```

To detach it from the terminal:

```sh
nohup ./target/release/onedrivelinux >/dev/null 2>&1 &
```

The executable is self-contained, so you can copy `target/release/onedrivelinux` anywhere.

### Install

`packaging/install.sh` builds the release executable and installs it, with a menu entry and an
icon, into `~/.local` (or another `PREFIX`):

```sh
./packaging/install.sh                         # → ~/.local/bin/onedrivelinux + menu entry
onedrivelinux                                    # start it (or pick OnedriveLinux in the app menu)

PREFIX=/usr/local sudo -E ./packaging/install.sh   # system-wide
./packaging/install.sh --uninstall             # remove (your settings are kept)
```

Make sure `~/.local/bin` is on your `PATH`. To start OnedriveLinux when you log in, turn on
**Settings (gear) › Start OnedriveLinux automatically on login**.

### Command line

```
onedrivelinux [--version] [--help]
```

| Environment variable    | Effect |
|-------------------------|--------|
| `RUST_LOG=<level>`      | Log level (`debug`, `info`, `warning`, `error`); overrides the GUI setting. |
| `ONEDRIVELINUX_WAYLAND=1` | Use native Wayland even when there is a tray icon (see below). |

## Usage

1. On first start the setup wizard opens: create a new profile, import an existing OneDrive config
   (e.g. `~/.config/onedrive/config`), or add a SharePoint library (this needs a Business profile
   that has synced once, so its account type is known).
2. Press **Play** to start syncing. If you aren't logged in, the client opens Microsoft's login
   page in your browser (older clients: a login window asks for the response URL).
3. Use the buttons at the bottom to open the sync folder, the **Profiles** window (client options,
   rename, remove, log out) and the **GUI settings**.
4. Closing the window hides it in the tray (or minimizes it when there is no tray). Quit with the
   power button at the top left or from the tray menu; this stops every sync.

## Files

| Path | Contents |
|------|----------|
| `~/.config/onedrive-gui/profiles` | The list of profiles (shared with the Python app). |
| `~/.config/onedrive-gui/gui_settings` | GUI settings (shared with the Python app). |
| `~/.config/onedrive/accounts/<profile>/config` | Client config of profiles created here. Only non-default options are written; the previous file is kept as `config_backup`. |
| `~/.config/onedrive/accounts/<profile>/sync_list` | Selective-sync list. |
| `/tmp/onedrive-gui/onedrive-gui.log` | Log file (path, level and rotation are in the GUI settings). |
| `~/.config/autostart/OnedriveLinux.desktop` | Start-on-login entry, when enabled. |

When you import a config, comments are dropped and multi-line `skip_file`/`skip_dir` options are
combined into one line the next time it is saved. Back up your configs first.

## Differences from the Python version

- **Wayland:** winit can't hide a window on Wayland, which hiding in the tray needs. When there is
  a tray on a Wayland session, the app runs through XWayland. With `ONEDRIVELINUX_WAYLAND=1` it runs
  natively, and closing the window minimizes it instead.
- **Login:** there is no embedded browser (the Qt version's optional QWebEngine login). Login uses
  your default browser.
- **Version warnings** appear on their own line in the header instead of replacing the status text.
  Versions are compared numerically, so a client newer than the latest release isn't reported as
  out of date. There is no check for newer versions of OnedriveLinux itself.
- **GUI settings** apply when you press Save; closing the window without saving discards the edits.
  Logging changes take effect after a restart.
- **Selective sync:** an empty `sync_list` isn't created, and the file is only rewritten when it
  changed, because the client asks for a resync whenever `sync_list` changes.
- Shortened folder names in the transfer list always keep the deepest folder.
- Icons are Lucide line icons (the same set as PdfCraft). The transfer list shows one per file type
  instead of the desktop icon theme's.

## Development

```sh
cargo test             # unit tests: output parser, config files, transfers, versions, …
cargo build            # debug build
```

| Module | Role (Python original) |
|--------|------------------------|
| `src/parser.rs` | Parses the client's `--monitor -v` output (`WorkerThread.read_stdout`). |
| `src/worker.rs`, `src/process.rs` | Runs and stops `onedrive --monitor`, merges its output streams (`WorkerThread`). |
| `src/maintenance.rs` | Login, logout and SharePoint lookups (`MaintenanceWorker`). |
| `src/profiles.rs`, `src/onedrive_config.rs`, `src/ini.rs` | Profiles and client config files (`global_config.py`). |
| `src/gui_settings.rs`, `src/logger.rs`, `src/autostart.rs` | GUI settings, logging, autostart. |
| `src/version.rs` | OneDrive client version check. |
| `src/tray.rs` | System tray (StatusNotifierItem). |
| `src/app.rs` | App state, events, sync control (`MainWindow`). |
| `src/ui/` | Main window, profiles window, GUI settings, setup wizard, login window, dialogs. |

Background threads send `AppEvent`s that the app handles in `eframe::App::logic`, which runs even
while the window is hidden, so the tray stays current.

## Attribution

Icons: [Lucide](https://lucide.dev), the same set as PdfCraft, under the ISC licence
(`assets/lucide/LICENSE-lucide.txt`). The window and tray images are by [Icons8](https://icons8.com),
as in the Python OneDriveGUI.

Fonts: [Inter](https://github.com/rsms/inter) and [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono),
the same as PdfCraft, under the SIL Open Font License 1.1 (`assets/fonts/OFL-*.txt`).

## License

GPL-3.0-only, like the original OneDriveGUI (see `LICENSE`).
