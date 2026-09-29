# MacDebounce

[![Test](https://github.com/jjangsangy/MacDebounce/actions/workflows/test.yml/badge.svg)](https://github.com/jjangsangy/MacDebounce/actions/workflows/test.yml)
[![Release](https://img.shields.io/github/v/release/jjangsangy/MacDebounce)](https://github.com/jjangsangy/MacDebounce/releases)
[![Platform](https://img.shields.io/badge/platform-macOS-black?logo=apple&logoColor=white)](https://apple.com)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A minimal, ultra-low-overhead macOS mouse key debounce daemon written in pure Rust.

Designed to eliminate physical switch chatter and accidental double-clicks from worn or sensitive mouse microswitches (Logitech, Razer, Apple Magic Mouse, Zowie, etc.) without introducing perceptible input lag.

---

## Highlights

- **Near-Zero CPU Usage (0.0% idle)**: Uses native macOS `CGEventTap` on `CFRunLoop`. Mouse movement and dragging are never tapped, meaning the process sleeps in the kernel and only wakes for microseconds on button presses.
- **Zero External Dependencies**: Compiles directly against macOS system frameworks (`CoreGraphics`, `CoreFoundation`, `ApplicationServices`, `libSystem`).
- **Tiny Footprint**: Compiles in under 1 second to a standalone `<600 KB` binary.
- **Background / `launchctl` Ready**: Full macOS `LaunchAgent` support with automated `--install-launchd` and `--uninstall-launchd` helpers.
- **Configurable**: Debounce all mouse buttons or selectively debounce specific buttons (e.g., Left click only, Right click only, or side buttons).
- **Graceful Termination**: Handles `SIGTERM` and `SIGINT` cleanly for smooth management by `launchctl`.

---

## Installation

### Quick Install (Recommended)

Install the latest universal macOS binary directly into your PATH (`/usr/local/bin`):

```bash
curl -fsSL https://raw.githubusercontent.com/jjangsangy/MacDebounce/main/scripts/install.sh | sh
```

You can customize the destination directory or version if desired:

```bash
# Install to a custom directory (e.g., ~/.local/bin)
curl -fsSL https://raw.githubusercontent.com/jjangsangy/MacDebounce/main/scripts/install.sh | INSTALL_DIR=~/.local/bin sh

# Install a specific release version
curl -fsSL https://raw.githubusercontent.com/jjangsangy/MacDebounce/main/scripts/install.sh | MACDEBOUNCE_VERSION=0.1.0 sh
```

### Building from Source

If you prefer compiling from source, ensure you have Rust installed (`cargo` and `rustc`):

```bash
git clone https://github.com/jjangsangy/MacDebounce.git
cd MacDebounce
cargo build --release
cp target/release/macdebounce /usr/local/bin/
```

---

## Accessibility Permission Setup

macOS requires accessibility permissions for any process that intercepts input events (`CGEventTap`):

1. Open **System Settings** -> **Privacy & Security** -> **Accessibility**.
2. Click the `+` button (or enable the toggle).
3. If testing in a terminal (e.g., Terminal, iTerm, Zed, Ghostty), grant permission to that terminal app.
4. When running as a `launchctl` background service, add the binary path (e.g., `/usr/local/bin/macdebounce` or your project target path) to the Accessibility list.

Check permission status anytime:
```bash
macdebounce --status
```

---

## Quick Start & Testing

Test interactively with verbose logging to observe chatter suppression:

```bash
macdebounce -v
```

Whenever a chattering bounce click is detected, it logs to terminal:
```text
[DEBOUNCED] Left button: spurious release bounce 14ms after release
```

---

## Logging & Debugging

`macdebounce` provides comprehensive macOS logging so you can easily diagnose if mouse buttons are misbehaving, chattering, or failing to register.

### 1. View Logs in Real Time (macOS Unified Logging System)

Logs are automatically published to macOS's native Unified Logging System (`os_log` / `syslog`):

```bash
# Stream daemon logs in real time from the command line:
macdebounce --stream-logs

# Or using macOS's built-in log command directly:
log stream --predicate 'process == "macdebounce"' --info --debug
```

You can also open Apple's **Console.app**, filter by Process `macdebounce`, and view live system log events.

### 2. Inspect Daemon Log Files

To inspect recent background daemon log entries without digging through directories:

```bash
macdebounce --show-logs
```

This reads the tail of:
- `~/Library/Logs/macdebounce.log` (standard daemon output)
- `~/Library/Logs/macdebounce.err` (debounce event logs and errors)

### 3. Diagnose Broken or Unknown Mouse Buttons (`--log-all`)

If a mouse button (e.g. side Back/Forward) isn't behaving properly, run `macdebounce` interactively with `--log-all`:

```bash
macdebounce --log-all
```

Every single mouse event is printed with its hardware button index and nanosecond timestamp:

```text
20:40:12.102 [TRACE] Mouse DOWN: Left (btn 0) at 1450284200ns -> Accepted
20:40:12.180 [TRACE] Mouse UP:   Left (btn 0) at 1450362200ns -> Accepted
20:40:12.195 [DEBUG] [DEBOUNCED] Left button (btn 0): spurious release bounce 15ms after release
20:40:12.195 [TRACE] Mouse DOWN: Left (btn 0) at 1450377200ns -> Dropped (spurious release bounce 15ms after release)
20:40:13.400 [TRACE] Mouse DOWN: Back (btn 3) at 1451582200ns -> Accepted
20:40:14.200 [TRACE] Mouse DOWN: Forward (btn 4) at 1452382200ns -> Accepted
```

---

## Configuration

You can configure `macdebounce` via CLI arguments or a TOML configuration file.

### CLI Options

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--debounce-ms <MS>` | `-d` | Debounce lockout window in ms | `50` |
| `--buttons <LIST>` | `-b` | Buttons to debounce (`all`, `left`, `right`, `middle`, `back`, `forward`, `side`, `0,1`) | `all` |
| `--verbose` | `-v` | Log debounced clicks to stdout & macOS Unified Log | `false` |
| `--log-all` | | Log EVERY mouse event (passed & dropped) for deep debugging | `false` |
| `--show-logs` | | Display recent log file entries from the background service | — |
| `--stream-logs` | | Stream live daemon logs from macOS Unified Logging | — |
| `--log-file <PATH>` | | Custom log file path to write log entries to | — |
| `--no-syslog` | | Disable macOS Unified Logging (`syslog`) | `false` |
| `--config <FILE>` | `-c` | Path to custom TOML config file | — |
| `--status` | | Display accessibility and service status | — |
| `--install-launchd` | | Install & load LaunchAgent for current user | — |
| `--uninstall-launchd` | | Unload & remove LaunchAgent | — |
| `--generate-plist` | | Output LaunchAgent XML plist to stdout | — |

### Examples

**Debounce only the Left mouse button with a 60ms threshold:**
```bash
macdebounce --buttons left --debounce-ms 60
```

**Debounce Left and Right buttons:**
```bash
macdebounce --buttons left,right
```

**Debounce mouse Back and Forward side buttons:**
```bash
macdebounce --buttons side
# or specify individually:
macdebounce --buttons back,forward
```

**Debounce all mouse buttons:**
```bash
macdebounce --buttons all --debounce-ms 50
```

### Config File

By default, `macdebounce` looks for:
1. `~/.config/macdebounce/config.toml`
2. `/Library/Application Support/MacDebounce/config.toml`

Example `~/.config/macdebounce/config.toml`:

```toml
# Debounce duration in milliseconds
debounce_ms = 50

# Buttons to debounce: "all", or array of names/numbers
# Supported button names: "left", "right", "middle", "back", "forward", "side", "button3", etc.
buttons = ["left"]

# Verbose logging (useful for debugging)
verbose = false
```

---

## Running in Background with `launchctl`

### Automatic Setup (Recommended)

Run:
```bash
macdebounce --buttons left --install-launchd
```

This will:
1. Create `~/Library/LaunchAgents/com.macdebounce.daemon.plist` configured with your binary path and settings.
2. Register and start the service with `launchctl`.
3. Provide the log locations:
   - `~/Library/Logs/macdebounce.log`
   - `~/Library/Logs/macdebounce.err`

To remove and stop the service:
```bash
macdebounce --uninstall-launchd
```

### Manual `launchctl` Setup

1. Copy the binary to a permanent location:
   ```bash
   cp target/release/macdebounce /usr/local/bin/
   ```

2. Copy the plist template:
   ```bash
   cp com.macdebounce.daemon.plist ~/Library/LaunchAgents/
   ```

3. Load the service into your user GUI session:
   ```bash
   launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.macdebounce.daemon.plist
   ```
   *(or `launchctl load ~/Library/LaunchAgents/com.macdebounce.daemon.plist` on older macOS versions)*

4. Check or restart the service:
   ```bash
   # Restart
   launchctl kickstart -k gui/$(id -u)/com.macdebounce.daemon

   # Stop and unload
   launchctl bootout gui/$(id -u)/com.macdebounce.daemon
   ```

---

## How It Works

When a mechanical switch wears out, releasing or pressing the switch causes the internal metal spring contact to bounce rapidly, emitting multiple Down/Up state changes within 5–40 milliseconds.

1. **Hardware Timestamps**: `macdebounce` inspects the kernel HID timestamp (`CGEventGetTimestamp`) directly on each event.
2. **Instant Delivery**: Legitimate clicks are delivered with **0 added latency** (no artificial delay on mouse down or mouse up).
3. **Lockout Filtering**:
   - Any secondary `Down` event arriving within `debounce_ms` of a valid `Down` or `Up` is dropped.
   - Any corresponding bounce `Up` event paired with a suppressed `Down` is dropped.
   - When the user deliberately double clicks (> `debounce_ms`), both clicks are delivered normally.
4. **Selective Masking**: If you only configure the Left button, CoreGraphics only taps `kCGEventLeftMouseDown` and `kCGEventLeftMouseUp`. Right clicks and middle clicks bypass the tap entirely.
