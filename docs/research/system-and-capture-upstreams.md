# System, barcode, and capture implementation notes

Reviewed 2026-09-24 against upstream documentation and package metadata. Recheck upstream before changing versions or adding new native capture APIs.

## System and environment inspection

- `sysinfo` 0.39.6 supplies read-only operating system, CPU, memory, disk, and network-interface data. Arcade Box enables its `system`, `disk`, and `network` features only.
- The environment inspector reads the current process environment without launching a subprocess. It redacts variables with credential-like names and URLs containing user information or common token/key/password query parameters. PATH inspection reports order, missing or non-directory entries, duplicates, and matching executable paths; it does not execute a candidate.
- GPU and display inventory are not exposed by this provider yet. The System Information tool returns this limitation explicitly rather than inferring unsupported details.

## QR and barcodes

- The `zxing-cpp` Rust wrapper 0.5.3 bundles ZXing-C++ 3.1.1 behind its `bundled` feature and exposes image reader/writer bindings behind `image`. The writer build uses the upstream Zint writer implementation; retain its BSD-3-Clause notices in the dependency closure.
- The application selects only the PNG/JPEG/WebP/TIFF/BMP/GIF image decoder features needed by the barcode reader. The `image` crate is MIT OR Apache-2.0; codec notices remain part of the resolved dependency audit.
- Barcode image decoding is bounded by input byte size, image dimensions, pixel count, and decoder allocation limits. Outputs are staged privately and published through the same opaque output-directory grant used by other tools.
- Batch barcode creation accepts lines/CSV, validates bounded rows, and publishes generated images plus a printable PDF label sheet. The catalog marks this backend implemented.
- Decoded values, including URL payloads, are shown as local result data. Only an explicit Open action invokes the host URL opener; that command reparses the URL and allows only HTTP/HTTPS without embedded credentials.

## Screen capture implementation

- Linux still capture includes an X11 selection path and a Wayland XDG Screenshot portal path. With an available Lens peer, screen selection delegates to Lens. At runtime Arcade Box checks whether the portal advertises area selection (Screenshot portal v3+); it presents the interactive area picker only when that target is available. Returned URIs must be local `file:` URIs, and the PNG is checked for a regular file, signature, bounded byte size, and bounded dimensions before it becomes an opaque artifact grant. This path does not use compositor-specific commands.
- Windows still capture uses `windows-capture` 2.0.1 (MIT) with Windows.Graphics.Capture and `GraphicsCapturePicker`. The system picker lets the user choose a display or window; the adapter captures one frame, bounds dimensions, validates the PNG, then publishes it as an opaque artifact grant. The picker may require Windows 10 1903+ APIs; production support should be smoke-tested on Windows 11.
- macOS still capture uses `screencapturekit` 10.0.3 (MIT OR Apache-2.0) with `AsyncSCContentSharingPicker` and `AsyncSCScreenshotManager`, available on macOS 14+. The user chooses a display, window, or application in system UI; macOS Screen Recording permission is required. `NSScreenCaptureUsageDescription` is merged into the application bundle Info.plist. The adapter bounds capture dimensions and stages the PNG privately before publishing a scoped artifact.
- Capture returns a scoped selected-file/artifact reference; screen pixels do not cross frontend IPC. Dedicated screenshot, OCR, QR, color, ruler and pin flows are implemented, including standalone X11 and Lens delegation paths.
- Screen recording is implemented with stop/discard and output publication. X11 was exercised under Xvfb. The Wayland portal/PipeWire path needs a real desktop pass; Windows uses Graphics Capture and macOS recording requires macOS 15+. Those native paths compile in CI but are not interactively verified.
- Windows/macOS adapters are cfg-gated and now compile in native CI. Successful compilation and tests do not replace native interactive capture/recording validation; see [platform evidence](../platform.md).

## Primary references

- [ZXing-C++ repository and writer](https://github.com/zxing-cpp/zxing-cpp)
- [ZXing-C++ Rust wrapper guide](https://github.com/zxing-cpp/zxing-cpp/blob/master/wrappers/rust/README.md)
- [ZXing-C++ license](https://github.com/zxing-cpp/zxing-cpp/blob/master/LICENSE)
- [Zint license](https://github.com/zint/zint/blob/master/LICENSE)
- [sysinfo repository](https://github.com/GuillaumeGomez/sysinfo)
- [image-rs repository](https://github.com/image-rs/image)
- [XDG Screenshot portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Screenshot.html)
- [XDG ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [Microsoft Windows.Graphics.Capture](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture)
- [Apple ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)
- [ScreenCaptureKit Rust bindings](https://docs.rs/screencapturekit/10.0.3/screencapturekit/)
- [Windows Capture Rust bindings](https://docs.rs/windows-capture/2.0.1/windows_capture/)
