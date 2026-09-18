# Building Amical on Linux

This guide covers building, configuring, and testing Amical and its Linux native helper (`linux-helper`).

## Rebuild this Linux fork from a fresh clone

After installing the system packages listed below, clone this fork with its
Whisper submodule and build the Debian installer:

```bash
git clone --recurse-submodules https://github.com/lduperval/amical.git
cd amical
corepack enable
pnpm install --frozen-lockfile
pnpm --filter @amical/desktop download-node
pnpm make:linux
```

The installer is written to
`apps/desktop/out/make/deb/x64/amical-desktop_1.12.0~beta.7_amd64.deb`.
`pnpm install` applies the tracked Whisper patch and builds its native addon;
`pnpm make:linux` builds and verifies the `uinput` helper. The patched
`whisper.cpp` submodule may appear modified after installation.

---

## 1. Default Behavior

By default, building Amical without any extra flags produces the standard, universal Linux build:

- **Default Input Method**: `clipboard` (Clipboard + manual <kbd>Ctrl</kbd>+<kbd>V</kbd>)
- **How it works**:
  - The transcript is copied directly to the system clipboard via Wayland data-control protocols (or `wl-clipboard` CLI fallback).
  - On compositors supporting the `zwp_virtual_keyboard_v1` Wayland protocol (e.g., Sway, Hyprland, river, KWin), synthetic key injection is automatically attempted.
  - On compositors that restrict virtual keyboard creation for standard unprivileged clients (such as GNOME Wayland / Mutter), automatic key injection is gracefully skipped.
  - **User Action**: The user manually presses <kbd>Ctrl</kbd>+<kbd>V</kbd> to paste the transcript into the target application.

---

## 2. Prerequisites & System Packages

### Core Build Tools

- **Node.js**: 24.x
- **pnpm**: 10.34.5, pinned in `package.json` (`corepack enable`)
- **Rust / Cargo**: 1.80+ (`rustup default stable`)
- **CMake**: 3.20+
- **C/C++ Compiler**: `gcc` / `g++` or `clang`

### Distribution Packages

#### Debian / Ubuntu / Pop!\_OS

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  cmake \
  pkg-config \
  libasound2-dev \
  libudev-dev \
  wl-clipboard
```

#### Fedora / RHEL

```bash
sudo dnf install -y \
  gcc \
  gcc-c++ \
  cmake \
  pkgconf-pkg-config \
  alsa-lib-devel \
  systemd-devel \
  wl-clipboard
```

#### Arch Linux

```bash
sudo pacman -S --needed \
  base-devel \
  cmake \
  alsa-lib \
  systemd-libs \
  wl-clipboard
```

---

## 3. Configurable Input Methods

To support various Linux flavors and display servers, Amical provides alternative input injection strategies:

| Method                     | Cargo Feature | Description                                                                  | Target Environments                                       |
| -------------------------- | ------------- | ---------------------------------------------------------------------------- | --------------------------------------------------------- |
| **Clipboard (Default)**    | _(None)_      | Copies transcription to clipboard; manual <kbd>Ctrl</kbd>+<kbd>V</kbd> paste | Universal fallback, GNOME Wayland default                 |
| **Option A (`uinput`)**    | `uinput`      | Injects Shift+Insert through a persistent kernel virtual keyboard            | Wayland sessions, including GNOME; requires device access |
| **Option B (`xtest`)**     | `xtest`       | Placeholder; not implemented                                                 | Unavailable                                               |
| **Option C (`gnome_ext`)** | `gnome_ext`   | Placeholder; not implemented                                                 | Unavailable                                               |

The uinput keyboard itself is independent of the display server, but this
helper still uses Wayland for clipboard access. A pure X11 session is not yet
supported by the full paste flow.

The recording shortcut remains configurable in Amical. Before injecting a
paste, the uinput backend checks all physical Ctrl/Alt/Shift/Super keys and
waits up to two seconds for release. It needs read access to keyboard devices
under `/dev/input/event*` as well as write access to `/dev/uinput`.

For paste, Amical supplies both the regular clipboard and primary selection,
then sends Shift+Insert, covering standard text fields and terminals without
having to identify the active application. Applications that disable or remap
this shortcut may not accept the paste. The target field must retain focus.
`preserveClipboard` restores previous text contents after 500 ms if the
clipboard has not changed; images and other non-text formats are not preserved.
Successful injection confirms that the keystroke was sent, not that the target
application inserted the text.

On setup or injection failure, the transcript stays on the regular clipboard,
the helper returns `success: false`, and the desktop shows a notification.
The helper's stderr log records the device or modifier error.

> [!NOTE]
> The features `uinput`, `xtest`, and `gnome_ext` are mutually exclusive. At most one optional input method feature can be compiled into `linux-helper`.

---

## 4. Building with Optional Features

### Method 1: Desktop CLI / Environment Flag

Use the environment variable when building or launching from the repository:

```bash
# Run desktop development with uinput input method
AMICAL_INPUT_METHOD=uinput pnpm dev

# Linux installers always package and verify the uinput helper
pnpm make:linux
```

Or export the environment variable:

```bash
export AMICAL_INPUT_METHOD=uinput
pnpm dev
```

The desktop also accepts `--input-method=uinput` at runtime, but the packaged
helper must already have been compiled with `--features uinput`. A runtime flag
cannot add support to a helper built without the feature. A uinput build uses
uinput by default, so no runtime flag is needed after installing that build.

### Method 2: Direct Helper Build

You can build the native helper directly from `packages/native-helpers/linux-helper`:

```bash
cd packages/native-helpers/linux-helper

# 1. Default build (clipboard path, no features)
pnpm build

# 2. Build with uinput support (Option A)
pnpm build:uinput

# 3. Build with XTest support (Option B)
node scripts/build.mjs xtest

# 4. Build with GNOME extension support (Option C)
node scripts/build.mjs gnome_ext
```

---

## 5. Setting Up `/dev/uinput` Permissions (Option A)

The `uinput` method communicates directly with the kernel input subsystem via `/dev/uinput`. By default on many distributions, `/dev/uinput` requires root permissions unless configured:

### 1. Ensure kernel module is loaded

```bash
sudo modprobe uinput
echo "uinput" | sudo tee /etc/modules-load.d/uinput.conf
```

### 2. Configure udev rule

Create `/etc/udev/rules.d/99-uinput.rules`:

```udev
KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"
```

### 3. Add your user to the `input` group

```bash
sudo usermod -aG input $USER
```

_Note: Log out and log back in for group membership changes to take effect._

---

## 6. Running Tests

Run the unit tests for all feature combinations:

```bash
cd packages/native-helpers/linux-helper

# Test default configuration
cargo test

# Test uinput configuration
cargo test --features uinput

# Test xtest configuration
cargo test --features xtest

# Test gnome_ext configuration
cargo test --features gnome_ext
```

To verify device creation on the host without sending any keystrokes:

```bash
cargo test --features uinput creates_real_virtual_keyboard -- --ignored --nocapture
```

For a manual end-to-end check, focus an editable field in a browser, text editor,
and terminal in turn. Hold the shortcut configured in Amical, dictate a short
phrase, then release it. Verify a single insertion in the same field. Repeat
with a different recording shortcut and release the letter before its modifiers.
With clipboard preservation enabled, check that your previous clipboard text
returns after a successful paste. Temporarily select an unavailable backend to
verify the notification and manual-paste fallback.

Implementation references: [Linux uinput documentation](https://docs.kernel.org/input/uinput.html)
and [evdev virtual device API](https://docs.rs/evdev/0.13.2/evdev/uinput/struct.VirtualDevice.html).
