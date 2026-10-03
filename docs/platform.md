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
