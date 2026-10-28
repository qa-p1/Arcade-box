# Speed / Pitch Tool (`arcade.audio.speed-pitch`)

**Status:** implemented. **Input:** one audio file. **Output:** a new audio file. **Privacy:** LOCAL via system FFmpeg.

- **Speed** from 0.25× to 4×. With *Keep the original pitch* on, this uses `atempo`, chained for factors outside 0.5–2.
- **Pitch shift** of −12 to +12 semitones in half-semitone steps. It uses the `rubberband` filter when the installed FFmpeg has it, and otherwise resampling plus `atempo`. Result metadata reports which `engine` was used.
- With *Keep the original pitch* off, the pitch follows the speed, like a tape or record. This is done by resampling.

The editor shows the new length and the resulting pitch change before you run.
