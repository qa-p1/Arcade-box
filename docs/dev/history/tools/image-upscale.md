# Image Upscale / Enhance

**LOCAL** · Uses an installed Real-ESRGAN NCNN/Vulkan provider and its local x4plus model files.

Select a PNG, JPEG, or WebP image and choose a 2×, 3×, or 4× output scale. The output is a new PNG. Arcade Box bounds output dimensions and pixel count, keeps the source unchanged, and will not overwrite an existing file.

Real-ESRGAN can synthesize fine detail while enlarging an image. Review the output before relying on it. The operation runs locally and supports cancellation; the provider may require a Vulkan-capable GPU and compatible drivers.

Arcade Box only enables this tool when the verified system CLI and its `realesrgan-x4plus.param` / `.bin` files already exist. It does not bundle or automatically download either the provider or its model. Model weights have separate licensing from provider source code; the registry reports the local model path and flags weights whose license has not been verified.

Options are `scale` (2, 3, or 4; default 2) and `outputName` (must end in `.png`).
