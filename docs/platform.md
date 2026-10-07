# Platform abstraction and support matrix

Tools depend on explicit platform services instead of scattering OS checks through tool code. Contracts include `GlobalShortcutProvider`, `ScreenCaptureProvider`, `ScreenRecordingProvider`, `ClipboardProvider`, `ActiveWindowProvider`, `WindowControlProvider`, `AppDiscoveryProvider`, `ProcessProvider`, `AutostartProvider`, `CredentialStoreProvider`, `FileAssociationProvider`, `NotificationProvider`, `OpenWithProvider`, `PowerStateProvider`, and `ThemeProvider`.

Implement platform adapters behind those interfaces. On Linux, prefer XDG Desktop Portals for global shortcuts, screenshots, screencasting, file selection, and URI/open behavior; use PipeWire for screen/video capture where applicable. Wayland compositor extensions can be optional adapters, not the sole implementation. X11 remains an explicit target. Detect portal/protocol capabilities at runtime and explain unavailable operations.

## Initial feature matrix

All entries in this matrix are targets, not verified support claims. Per-tool rollout state lives in the catalog. Update this table only from real platform runs.

| Capability | Windows | macOS | Wayland | X11 |
|---|---|---|---|---|
| Global shortcut | Planned | Planned | Planned; portal availability varies | Planned |
| Island placement and focus return | Planned | Planned | Planned; compositor constraints vary | Planned |
| Screenshot / region selection | Planned | Planned | Planned through portal where available | Planned |
| Screen recording / system audio | Planned | Planned | Planned; portal/PipeWire and audio support varies | Planned |
| Clipboard text and image | Planned | Planned | Planned; compositor/session APIs vary | Planned |
| Always on top / pin window | Planned | Planned | Conditional compositor support | Planned |
| App discovery / process list | Planned | Planned | Planned | Planned |
| Startup entries / file associations | Planned | Planned | Planned | Planned |
| Notifications / credential store | Planned | Planned | Planned | Planned |

Each platform adapter reports capabilities and limitations. The UI disables or explains unavailable actions rather than failing silently. Keep build CI across Windows, macOS, and Linux, and require actual Wayland and X11 functional runs before claiming support.

## Current implementation evidence

The targets above remain Planned until a real platform run verifies them.
Windows/macOS code below is **build only (CI defined, not run here)**. CI builds
the desktop and tests the shared runtime on all three operating systems; this
Linux environment has no Windows/macOS toolchain or native desktop session.

| Adapter | Windows | macOS | Linux evidence / limit |
|---|---|---|---|
| Provider broker | PATH, Program Files + x86, WinGet Packages/Links, scoop shims/current; build only | Homebrew Intel/Apple Silicon, app bundles; build only | Candidate layouts unit-tested; installed engines exercised in Rust/smoke tests |
| Screen selection | Windows Graphics Capture system picker; build only | ScreenCaptureKit content picker, macOS 14+; build only | Lens region selection tested under isolated X11; portal capture depends on area-selection support |
| Screen recording | Graphics Capture → verified FFmpeg MP4 encoder; build only | ScreenCaptureKit recording output, macOS 15+; build only | Existing X11 FFmpeg / Wayland portal-PipeWire adapters; recording not run in this pass |
| Clipboard text/image and opted-in history | Tauri clipboard plus foreground-process lookup; build only | Tauri clipboard plus frontmost-application lookup; build only | Native Clipboard peer/history handover tested under isolated X11; Wayland frontmost-app exclusions unavailable |
| Plain-text paste | SendInput Ctrl+Shift+V; target app must support it; build only | Paste and Match Style via AppleScript; Accessibility permission may be needed; build only | X11 requires xdotool; generic Wayland input injection unavailable |
| Pin another app's window | SetWindowPos with permissions constraints; build only | Unsupported; tool hidden from desktop search/dashboard | X11 needs wmctrl/EWMH; generic Wayland operation unavailable |
| Global shortcut / tray / Island | Tauri adapters; build only | Tauri adapters; build only | Normal embedded desktop and Ctrl+Alt+Space exercised under isolated X11 |
| Link delegation / cross-app pipelines | Build only | Build only | Real Lens → Box resize/WebP → Clipboard, approval, cancel/crash, version repair tested under isolated X11 |

Runtime capability checks prevent unsupported capture/recording/paste actions
from running. macOS window pinning is hidden because no implementation exists.
Lens is preferred for OCR on Windows/macOS and for selection whenever available;
Box keeps its existing standalone fallback. Recording remains in Box because
Lens has no recorder to delegate to. Neither an installed peer nor a successful
Linux candidate-path test is evidence of native Windows/macOS behavior.
