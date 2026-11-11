//! BR-8 (docs/v1/audit/box-real.md): `providers` lists every engine a tool uses, found or missing, so a tool that
//! needs one can say so up front. Fixed; this was the BR-8 lock (docs/v1/DECISIONS.md D3).
use std::process::Command;

#[test]
fn providers_lists_every_engine_even_when_missing() {
    let empty = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_arcadebox"))
        .arg("providers")
        .env_clear()
        .env("PATH", empty.path())
        .env("HOME", empty.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    // Barcodes are read by the built-in zxing, so zbarimg (seen on the audit desktop) is no engine of Box.
    let missing: Vec<_> = [
        "ffmpeg",
        "qpdf",
        "vips",
        "tesseract",
        "yt-dlp",
        "ghostscript",
        "imagemagick",
        "curl",
    ]
    .into_iter()
    .filter(|engine| !text.contains(engine))
    .collect();
    assert!(
        missing.is_empty(),
        "BR-8: not listed: {missing:?} in {text:?}"
    );
}
