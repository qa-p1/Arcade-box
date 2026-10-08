# Platform abstraction and support matrix

Tools depend on explicit platform services instead of scattering OS checks through tool code. Contracts include `GlobalShortcutProvider`, `ScreenCaptureProvider`, `ScreenRecordingProvider`, `ClipboardProvider`, `ActiveWindowProvider`, `WindowControlProvider`, `AppDiscoveryProvider`, `ProcessProvider`, `AutostartProvider`, `CredentialStoreProvider`, `FileAssociationProvider`, `NotificationProvider`, `OpenWithProvider`, `PowerStateProvider`, and `ThemeProvider`.

Implement platform adapters behind those interfaces. On Linux, prefer XDG Desktop Portals for global shortcuts, screenshots, screencasting, file selection, and URI/open behavior; use PipeWire for screen/video capture where applicable. Wayland compositor extensions can be optional adapters, not the sole implementation. X11 remains an explicit target. Detect portal/protocol capabilities at runtime and explain unavailable operations.

## Verified scope (2026-10-08)

Linux standalone and ecosystem flows run under isolated Xvfb. Selected Hyprland
flows (including window-pin and paste-plain) were also verified. Windows/macOS
builds, tests and installer creation passed in native CI; interactive desktop
behavior remains unverified. "Build only" below means built/tested in CI, not
an unexecuted workflow. See [current status](STATUS.md) for evidence.

| Adapter | Windows | macOS | Linux evidence / limit |
|---|---|---|---|
| Provider broker | PATH, Program Files + x86, WinGet Packages/Links, scoop shims/current; build only | Homebrew Intel/Apple Silicon, app bundles; build only | Candidate layouts unit-tested; installed engines exercised in Rust/smoke tests |
| Screen selection | Windows Graphics Capture system picker; build only | ScreenCaptureKit content picker, macOS 14+; build only | Lens region selection tested under isolated X11; portal capture depends on area-selection support |
| Screen recording | Graphics Capture → verified FFmpeg MP4 encoder; build only | ScreenCaptureKit recording output, macOS 15+; build only | X11 recording with stop/discard verified under Xvfb; full Wayland portal/PipeWire flow still needs a real desktop run |
| Clipboard text/image and opted-in history | Tauri clipboard plus foreground-process lookup; build only | Tauri clipboard plus frontmost-application lookup; build only | Native Clipboard peer/history handover tested under isolated X11; Wayland frontmost-app exclusions unavailable |
| Plain-text paste | SendInput Ctrl+Shift+V; target app must support it; build only | Paste and Match Style via AppleScript; Accessibility permission may be needed; build only | X11 requires xdotool; Hyprland sends the shortcut to the window itself (verified with a terminal); other Wayland compositors unavailable |
| Pin another app's window | SetWindowPos with permissions constraints; build only | Unsupported; tool hidden from desktop search/dashboard | X11 needs wmctrl/EWMH; Hyprland floats and pins the window (verified), restoring tiling on unpin; other Wayland compositors unavailable |
| Global shortcut / tray / Island | Tauri adapters; build only | Tauri adapters; build only | Normal embedded desktop and Ctrl+Alt+Space exercised under isolated X11 |
| Link delegation / cross-app pipelines | Build only | Build only | Real Lens → Box resize/WebP → Clipboard, approval, cancel/crash, version repair tested under isolated X11 |

Runtime capability checks prevent unsupported capture/recording/paste actions
from running. macOS window pinning is hidden because no implementation exists.
Lens is preferred for OCR on Windows/macOS and for selection whenever available;
Box keeps its existing standalone fallback. Recording remains in Box because
Lens exposes no recorder action over Link. Neither an installed peer nor a successful
Linux candidate-path test is evidence of native Windows/macOS behavior.
