//! Streams of bytes, UTF-8, and plain text.

#![deny(missing_docs)]

#[cfg(feature = "text")]
mod no_forbidden_characters;
#[cfg(feature = "text")]
mod rc_char_queue;
mod read;
mod slice_reader;
mod status;
mod std_reader;
mod std_writer;
#[cfg(feature = "text")]
mod text_reader;
#[cfg(feature = "text")]
mod text_writer;
mod unicode;
mod utf8_reader;
mod utf8_writer;
mod write;

pub use read::{
    default_read_exact, default_read_to_end, default_read_to_string, Read, ReadOutcome,
};
pub use slice_reader::SliceReader;
pub use status::{Readiness, Status};
pub use std_reader::StdReader;
pub use std_writer::StdWriter;
#[cfg(feature = "text")]
pub use text_reader::TextReader;
#[cfg(feature = "text")]
pub use text_writer::TextWriter;
pub use unicode::NORMALIZATION_BUFFER_SIZE;
pub use utf8_reader::Utf8Reader;
pub use utf8_writer::Utf8Writer;
pub use write::{default_write_all, default_write_vectored, Write};
