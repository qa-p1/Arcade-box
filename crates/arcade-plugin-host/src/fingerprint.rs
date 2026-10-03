use std::{fs::Metadata, io, time::UNIX_EPOCH};

/// Encode modification time as signed decimal nanoseconds since the Unix
/// epoch. Core and the worker share this conversion to avoid timestamp drift.
pub fn modified_time_ns(metadata: &Metadata) -> io::Result<String> {
    let modified = metadata.modified()?;
    let nanos = match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos())
            .map_err(|_| io::Error::other("file timestamp exceeds the protocol range"))?,
        Err(error) => -i128::try_from(error.duration().as_nanos())
            .map_err(|_| io::Error::other("file timestamp exceeds the protocol range"))?,
    };
    Ok(nanos.to_string())
}
