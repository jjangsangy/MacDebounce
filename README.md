# MacDebounce

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

## Installation & Compilation

Ensure you have Rust installed (`cargo` and `rustc`):

```bash
cargo build --release
```

The compiled binary will be located at `target/release/macdebounce`.

To install system-wide (optional):

```bash
sudo cp target/release/macdebounce /usr/local/bin/
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
./target/release/macdebounce --status
```

---

## Quick Start & Testing

Test interactively with verbose logging to observe chatter suppression:

```bash
./target/release/macdebounce -v
```

Whenever a chattering bounce click is detected, it logs to terminal:
```text
[DEBOUNCED] Left button: spurious release bounce 14ms after release
```

---

## Configuration

You can configure `macdebounce` via CLI arguments or a TOML configuration file.

### CLI Options

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--debounce-ms <MS>` | `-d` | Debounce lockout window in ms | `50` |
| `--buttons <LIST>` | `-b` | Buttons to debounce (`all`, `left`, `right`, `middle`, `back`, `forward`, `side`, `0,1`) | `all` |
| `--verbose` | `-v` | Log debounced clicks to stdout | `false` |
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
./target/release/macdebounce --buttons left --install-launchd
```

This will:
1. Create `~/Library/LaunchAgents/com.macdebounce.daemon.plist` configured with your binary path and settings.
2. Register and start the service with `launchctl`.
3. Provide the log locations:
   - `~/Library/Logs/macdebounce.log`
   - `~/Library/Logs/macdebounce.err`

To remove and stop the service:
```bash
./target/release/macdebounce --uninstall-launchd
```

### Manual `launchctl` Setup

1. Copy the binary to a permanent location:
   ```bash
   sudo cp target/release/macdebounce /usr/local/bin/
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
