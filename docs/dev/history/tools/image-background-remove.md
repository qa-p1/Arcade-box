# Background Remover

**LOCAL** · Uses a compatible system `rembg` command and a cached U²-Net model.

Select a PNG, JPEG, or WebP image. Choose U²-Net-P for a smaller, faster model or U²-Net for the larger model. Output is a new transparent PNG; the source stays unchanged. Optional edge refinement uses rembg alpha matting, and mask cleanup uses its post-processing option.

Arcade Box enables this tool only when both the CLI and the selected model file are already installed. It invokes rembg's custom local-model mode with a validated cached model path. It does not let rembg download weights on first use, call a cloud service, or write output beside the input implicitly. The result is staged privately and saved under a new name without overwriting an existing file.

Model weights have their own license and provenance, separate from rembg's MIT code license. Arcade Box does not bundle or automatically download weights. The current model registry reports cached model paths and flags weights whose license has not been verified; check upstream terms before redistribution.

The `u2netp` and `u2net` model choices use `model`, defaulting to `u2netp`. Other options are `alphaMatting`, `postProcessMask`, and `outputName` (must end in `.png`).
