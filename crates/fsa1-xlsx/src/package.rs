// Concern: deflates the emitted parts into a byte-stable .xlsx zip | Non-concern: any part's XML, choosing the dest path | IO: (the parts + a sink) -> a .xlsx

use std::io::{Seek, Write};

use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, DateTime};

pub(crate) struct Part {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl Part {
    pub(crate) fn new(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Part {
            path: path.into(),
            bytes,
        }
    }
}

pub(crate) fn write_package<W: Write + Seek>(
    parts: &[Part],
    out: W,
) -> Result<W, zip::result::ZipError> {
    let mut zip = ZipWriter::new(out);
    // `SimpleFileOptions::default()` reads the wall clock when zip's `time` feature is on.
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::DEFAULT);
    for part in parts {
        zip.start_file(&part.path, options)?;
        zip.write_all(&part.bytes)?;
    }
    zip.finish()
}
