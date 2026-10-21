# Build and run OnedriveLinux

## Requirements

- **OneDrive Client for Linux v2.5.11 or newer**, on your `PATH` (needed to sync).
  Install it by following the [client's install instructions](https://github.com/abraunegg/onedrive/blob/master/docs/install.md).
  If it lives somewhere else, set its path in Settings (gear icon) › "Path to OneDrive client binary".
- **To build:** Rust 1.95 or newer (`cargo`) and a C compiler (`cc`).
  No GTK, Qt, D-Bus or X11 development packages are needed.
- **Optional:**
  - A system tray. KDE Plasma, XFCE and Cinnamon have one built in; GNOME needs the
    *AppIndicator and KStatusNotifierItem Support* extension.
  - `xdg-open`, to open the sync folder.
  - `notify-send`, for the login notification.

## Get the source

```sh
git clone https://github.com/NunesGodinho/OnedriveGUI-rust.git
cd OnedriveGUI-rust
```

All commands below are run from this folder.

## Build and start

```sh
# Build and start in one step
cargo run --release

# Or build once, then start the executable
cargo build --release
./target/release/onedrivelinux
```

The executable `target/release/onedrivelinux` is self-contained: you can copy it anywhere.

To start it without keeping a terminal open:

```sh
nohup ./target/release/onedrivelinux >/dev/null 2>&1 &
```

## Install (menu entry and icon)

```sh
./packaging/install.sh              # build and install into ~/.local
onedrivelinux                         # start it, or pick OnedriveLinux in the application menu
```

`~/.local/bin` must be on your `PATH` to start it by name.

To install for all users instead:

```sh
PREFIX=/usr/local sudo -E ./packaging/install.sh
```

To uninstall (your settings and profiles are kept):

```sh
./packaging/install.sh --uninstall
```

## Start on login

In the app, open Settings (gear icon), turn on
"Start OnedriveLinux automatically on login", then press **Save**.

## Command-line options and environment variables

```
onedrivelinux --version
onedrivelinux --help
```

| Variable                | Effect |
|-------------------------|--------|
| `RUST_LOG=<level>`      | Log level: `debug`, `info`, `warning` or `error`. Overrides the GUI setting. |
| `ONEDRIVELINUX_WAYLAND=1` | Use native Wayland even when there is a tray icon. Closing the window then minimizes it instead of hiding it in the tray. |

## First start

1. The setup wizard opens. Create a new profile, import an existing OneDrive config
   (for example `~/.config/onedrive/config`), or add a SharePoint library.
2. Press **Play** to start syncing. If you aren't logged in yet, the client opens
   Microsoft's login page in your browser.
3. Closing the window hides it in the tray. To quit, use the power button at the top left
   or **Quit** in the tray menu; this stops every sync.

## Good to know

- Settings and profiles are in `~/.config/onedrive-gui/`, shared with the Python OneDriveGUI.
  On startup the app rewrites each profile's client config and keeps the previous one as `config_backup`.
- The log file is `/tmp/onedrive-gui/onedrive-gui.log` by default. You can change it in Settings.
- If the header says "Unable to check for latest OneDrive client version", GitHub's hourly limit
  on anonymous requests was probably reached. Syncing isn't affected.

## Run the tests

```sh
cargo test
```
