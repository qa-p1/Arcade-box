//! yt-dlp's own progress, read line by line from its output: bytes done of the total (or fragments of a stream
//! that comes in pieces), speed and time left, which of the streams it is on (a video and its audio are two), and
//! what it does after downloading (joining video and audio, converting, adding subtitles). Nothing is invented:
//! when yt-dlp doesn't know how far it is, neither does the job, and the bar shows work without a number.

use super::download_name;

/// What yt-dlp prints for each progress update, one line each (`--newline`), at most twice a second.
const TEMPLATE: &str = "download:arcade-progress %(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(progress.fragment_index)s|%(progress.fragment_count)s|%(progress.filename)s";
const MARK: &str = "arcade-progress ";

/// The share of the bar the download itself takes; what yt-dlp does afterwards fills the rest.
const DOWNLOAD_SHARE: f64 = 0.95;

/// The arguments that make yt-dlp print its progress for [`Tracker`].
pub(super) fn args() -> [&'static str; 6] {
    [
        "--newline",
        "--progress",
        "--progress-delta",
        "0.5",
        "--progress-template",
        TEMPLATE,
    ]
}

/// What the job should show after a line.
#[derive(Debug, PartialEq)]
pub(super) enum Update {
    /// How far the whole run is (0–1), and what it is doing.
    Fraction(f64, String),
    /// What it is doing, when there is no number for it.
    Step(String),
}

#[derive(Default)]
pub(super) struct Tracker {
    /// How many streams the chosen format has (a video and its audio are two).
    streams: usize,
    /// Streams finished so far.
    done: usize,
    /// The file of the stream being downloaded.
    current: Option<String>,
    /// The most the bar has shown, so it never goes back.
    shown: f64,
}

impl Tracker {
    pub(super) fn new() -> Self {
        Self {
            streams: 1,
            ..Self::default()
        }
    }

    pub(super) fn line(&mut self, line: &str) -> Option<Update> {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix(MARK) {
            return self.progress(rest);
        }
        if let Some(formats) = line
            .strip_prefix("[info] ")
            .and_then(|l| l.split_once(" format(s): "))
            .map(|(_, f)| f)
        {
            // "137+140" is a video and its audio, downloaded one after the other and joined.
            self.streams = formats
                .split(", ")
                .map(|f| f.matches('+').count() + 1)
                .sum::<usize>()
                .max(1);
            return None;
        }
        let (tag, _) = line.strip_prefix('[')?.split_once(']')?;
        let step = match tag {
            "Merger" => "Joining video and audio",
            "ExtractAudio" => "Converting the audio",
            "VideoConvertor" | "VideoRemuxer" => "Converting the video",
            "EmbedSubtitle" | "SubtitlesConvertor" => "Adding subtitles",
            "EmbedThumbnail" | "ThumbnailsConvertor" => "Adding the thumbnail",
            "Metadata" => "Adding the title and details",
            tag if tag.starts_with("Fixup") => "Finishing the file",
            _ => return None,
        };
        Some(Update::Step(step.to_owned()))
    }

    fn progress(&mut self, rest: &str) -> Option<Update> {
        let fields: Vec<&str> = rest.splitn(9, '|').collect();
        let [
            status,
            downloaded,
            total,
            estimate,
            speed,
            eta,
            fragment,
            fragments,
            filename,
        ] = fields[..]
        else {
            return None;
        };
        let number = |s: &str| {
            s.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let name = download_name(filename.rsplit(['/', '\\']).next().unwrap_or(filename));
        if self.current.as_deref() != Some(filename) {
            self.current = Some(filename.to_owned());
        }
        if status == "finished" {
            self.done = (self.done + 1).min(self.streams.max(1));
            let fraction = self.overall(0.0);
            return Some(Update::Fraction(fraction, format!("Downloaded {name}")));
        }
        if status != "downloading" {
            return None;
        }
        let downloaded = number(downloaded);
        let size = number(total)
            .or_else(|| number(estimate))
            .filter(|t| *t > 0.0);
        let within = match (downloaded, size) {
            (Some(d), Some(t)) => Some((d / t).min(1.0)),
            _ => match (number(fragment), number(fragments)) {
                (Some(i), Some(n)) if n > 0.0 => Some((i / n).min(1.0)),
                _ => None,
            },
        };
        let mut note = format!("Downloading {name}");
        if self.streams > 1 {
            note.push_str(&format!(
                " (part {} of {})",
                (self.done + 1).min(self.streams),
                self.streams
            ));
        }
        if let Some(within) = within {
            note.push_str(&format!(" · {}%", (within * 100.0).floor() as u32));
        }
        match (downloaded, size) {
            (Some(d), Some(t)) => note.push_str(&format!(" · {} of {}", bytes(d), bytes(t))),
            (Some(d), None) => note.push_str(&format!(" · {}", bytes(d))),
            _ => {}
        }
        if let Some(speed) = number(speed).filter(|s| *s > 0.0) {
            note.push_str(&format!(" · {}/s", bytes(speed)));
        }
        if let Some(eta) = number(eta).filter(|_| within.is_some()) {
            note.push_str(&format!(" · {}", time_left(eta)));
        }
        Some(match within {
            Some(within) => Update::Fraction(self.overall(within), note),
            None => Update::Step(note),
        })
    }

    fn overall(&mut self, within: f64) -> f64 {
        let streams = self.streams.max(1) as f64;
        let fraction =
            (((self.done as f64 + within) / streams) * DOWNLOAD_SHARE).min(DOWNLOAD_SHARE);
        self.shown = self.shown.max(fraction);
        self.shown
    }
}

pub(super) fn bytes(value: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = value;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn time_left(seconds: f64) -> String {
    let seconds = seconds.round() as u64;
    match seconds {
        0 => "almost done".to_owned(),
        1..=59 => format!("{seconds} s left"),
        60..=3599 => format!("{} min left", seconds.div_ceil(60)),
        _ => format!("{} h {} min left", seconds / 3600, (seconds % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fraction(update: Option<Update>) -> f64 {
        match update {
            Some(Update::Fraction(f, _)) => f,
            other => panic!("expected a fraction, got {other:?}"),
        }
    }

    // The lines below are what yt-dlp 2026.11.23 printed for these arguments through a pipe.
    #[test]
    fn one_file_moves_with_its_bytes_and_says_how_much_is_left() {
        let mut t = Tracker::new();
        assert_eq!(t.line("[info] clip: Downloading 1 format(s): mp4"), None);
        let first =
            fraction(t.line("arcade-progress downloading|1024|2892642|NA|NA|NA|NA|NA|clip.mp4"));
        assert!((first - 1024.0 / 2892642.0 * DOWNLOAD_SHARE).abs() < 1e-12);
        let Some(Update::Fraction(f, note)) = t.line(
            "arcade-progress downloading|1047552|2892642|NA|1387840.4353807506|1|NA|NA|clip.mp4",
        ) else {
            panic!()
        };
        assert!((f - 1047552.0 / 2892642.0 * DOWNLOAD_SHARE).abs() < 1e-9);
        assert_eq!(
            note,
            "Downloading clip.mp4 · 36% · 1.0 MB of 2.9 MB · 1.4 MB/s · 1 s left"
        );
        let done = fraction(
            t.line("arcade-progress finished|2892642|2892642|NA|1304255.0|NA|NA|NA|clip.mp4"),
        );
        assert!((done - DOWNLOAD_SHARE).abs() < 1e-9);
    }

    #[test]
    fn video_and_audio_are_two_halves_and_joining_is_a_step() {
        let mut t = Tracker::new();
        t.line("[info] dQw4w9WgXcQ: Downloading 1 format(s): 137+140");
        let half = fraction(
            t.line("arcade-progress downloading|50|100|NA|NA|NA|NA|NA|Song.f137.mp4.part"),
        );
        assert!((half - 0.25 * DOWNLOAD_SHARE).abs() < 1e-9);
        let Some(Update::Fraction(_, note)) =
            t.line("arcade-progress downloading|60|100|NA|NA|NA|NA|NA|Song.f137.mp4.part")
        else {
            panic!()
        };
        assert_eq!(
            note,
            "Downloading Song.mp4 (part 1 of 2) · 60% · 60 B of 100 B"
        );
        t.line("arcade-progress finished|100|100|NA|NA|NA|NA|NA|Song.f137.mp4");
        let audio = fraction(
            t.line("arcade-progress downloading|50|100|NA|NA|NA|NA|NA|Song.f140.m4a.part"),
        );
        assert!((audio - 0.75 * DOWNLOAD_SHARE).abs() < 1e-9);
        assert_eq!(
            t.line("[Merger] Merging formats into \"Song.mp4\""),
            Some(Update::Step("Joining video and audio".into()))
        );
        assert_eq!(
            t.line("[ExtractAudio] Destination: Song.mp3"),
            Some(Update::Step("Converting the audio".into()))
        );
        assert_eq!(t.line("[download] Destination: Song.f137.mp4"), None);
        assert_eq!(t.line("[youtube] dQw4w9WgXcQ: Downloading webpage"), None);
    }

    #[test]
    fn a_stream_in_pieces_counts_its_pieces_and_an_unknown_size_has_no_number() {
        let mut t = Tracker::new();
        let f = fraction(t.line("arcade-progress downloading|5000|NA|NA|NA|NA|3|12|live.mp4.part"));
        assert!((f - 0.25 * DOWNLOAD_SHARE).abs() < 1e-9);
        let mut t = Tracker::new();
        assert_eq!(
            t.line("arcade-progress downloading|5000000|NA|NA|250000|NA|NA|NA|radio.mp3.part"),
            Some(Update::Step(
                "Downloading radio.mp3 · 5.0 MB · 250 KB/s".into()
            ))
        );
    }

    #[test]
    fn the_bar_never_goes_back_and_never_passes_the_download_share() {
        let mut t = Tracker::new();
        let a = fraction(t.line("arcade-progress downloading|80|100|NA|NA|NA|NA|NA|a.mp4.part"));
        // A size estimate that grows makes the plain fraction drop; the bar stays.
        let b = fraction(t.line("arcade-progress downloading|81|200|NA|NA|NA|NA|NA|a.mp4.part"));
        assert_eq!(a, b);
        let c = fraction(t.line("arcade-progress downloading|500|100|NA|NA|NA|NA|NA|a.mp4.part"));
        assert!(c <= DOWNLOAD_SHARE);
        assert_eq!(t.line("arcade-progress downloading|garbage"), None);
    }

    #[test]
    fn sizes_and_times_read_like_people_say_them() {
        assert_eq!(bytes(512.0), "512 B");
        assert_eq!(bytes(1_500_000.0), "1.5 MB");
        assert_eq!(bytes(123_400_000.0), "123 MB");
        assert_eq!(time_left(0.2), "almost done");
        assert_eq!(time_left(42.0), "42 s left");
        assert_eq!(time_left(61.0), "2 min left");
        assert_eq!(time_left(3725.0), "1 h 2 min left");
    }
}
