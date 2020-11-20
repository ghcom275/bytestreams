use crate::{
    unicode::{is_normalization_form_starter, BOM, MAX_UTF8_SIZE},
    Readiness, Status, Utf8Writer, Write,
};
use std::{io, str};
use unicode_normalization::UnicodeNormalization;

/// A `Write` implementation which translates to an output `Write` producing
/// a valid plain text stream from an arbitrary byte sequence.
///
/// An output text stream enforces the following restrictions:
///  - Data must be valid UTF-8.
///  - U+FEFF (BOM) scalar values must not be present.
///  - A '\n' is required at the end of the stream.
///  - Control codes other than '\n' and '\t' most not be present.
///
/// An output text stream implicitly applies the following transformations:
///  - Text is transformed to Normalization Form C (NFC).
///  - The Stream-Safe Text Process (UAX15-D4) is applied.
///  - Optionally, "\n" is translated to "\r\n".
///
/// `write` is not guaranteed to perform a single operation, because short
/// writes could produce invalid UTF-8, so `write` will retry as needed.
pub struct TextWriter<Inner: Write> {
    /// The wrapped byte stream.
    inner: Utf8Writer<Inner>,

    /// Temporary staging buffer.
    buffer: String,

    /// True if the last byte written was a '\n'.
    nl: NlGuard,

    /// When enabled, "\n" is replaced by "\r\n".
    crlf_compatibility: bool,

    /// At the beginning of a stream or after a lull, expect a
    /// normalization-form starter.
    expect_starter: bool,
}

impl<Inner: Write> TextWriter<Inner> {
    /// Construct a new instance of `TextWriter` wrapping `inner`.
    #[inline]
    pub fn new(inner: Inner) -> Self {
        Self {
            inner: Utf8Writer::new(inner),
            buffer: String::new(),
            nl: NlGuard(false),
            crlf_compatibility: false,
            expect_starter: true,
        }
    }

    /// Like `new`, but writes a U+FEFF (BOM) to the beginning of the output
    /// stream for compatibility with consumers that require that to determine
    /// the text encoding.
    #[inline]
    pub fn with_bom_compatibility(mut inner: Inner) -> io::Result<Self> {
        let mut bom_bytes = [0_u8; MAX_UTF8_SIZE];
        let bom_len = BOM.encode_utf8(&mut bom_bytes).len();
        inner.write(&bom_bytes[..bom_len])?;
        Ok(Self {
            inner: Utf8Writer::new(inner),
            buffer: String::new(),
            nl: NlGuard(false),
            crlf_compatibility: false,
            expect_starter: true,
        })
    }

    /// Like `new`, but enables CRLF output mode, which translates "\n" to
    /// "\r\n" for compatibility with consumers that need that.
    ///
    /// Note: This is not often needed; even on Windows these days most
    /// things are ok with plain '\n' line endings, [including Windows Notepad].
    /// The main notable things that really need them are IETF RFCs, for example
    /// [RFC-5198].
    ///
    /// [including Windows Notepad]: https://devblogs.microsoft.com/commandline/extended-eol-in-notepad/
    /// [RFC-5198]: https://tools.ietf.org/html/rfc5198#appendix-C
    #[inline]
    pub fn with_crlf_compatibility(inner: Inner) -> Self {
        Self {
            inner: Utf8Writer::new(inner),
            buffer: String::new(),
            nl: NlGuard(false),
            crlf_compatibility: true,
            expect_starter: true,
        }
    }

    /// Flush and close the underlying stream and return the underlying
    /// stream object.
    pub fn close_into_inner(mut self) -> io::Result<Inner> {
        self.check_nl(Status::End)?;
        self.inner.close_into_inner()
    }

    /// Discard and close the underlying stream and return the underlying
    /// stream object.
    pub fn abandon_into_inner(mut self) -> io::Result<Inner> {
        self.abandon();
        self.inner.close_into_inner()
    }

    fn normal_write_all_utf8(&mut self, s: &str) -> io::Result<()> {
        self.buffer.extend(s.chars().stream_safe().nfc());

        // Write to the underlying stream.
        self.write_buffer()
    }

    fn crlf_write_all_utf8(&mut self, s: &str) -> io::Result<()> {
        // Translate "\n" into "\r\n".
        let mut first = true;
        for slice in s.split('\n') {
            if first {
                first = false;
            } else {
                self.buffer.push_str("\r\n");
            }
            self.buffer.extend(slice.chars().stream_safe().nfc());
        }

        // Write to the underlying stream.
        self.write_buffer()
    }

    fn write_buffer(&mut self) -> io::Result<()> {
        if self.expect_starter {
            self.expect_starter = false;
            if let Some(c) = self.buffer.chars().next() {
                if !is_normalization_form_starter(c) {
                    self.abandon();
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "write data must begin with a Unicode Normalization Form starter",
                    ));
                }
            }
        }

        if self
            .buffer
            .chars()
            .any(|c| (c.is_control() && c != '\n' && c != '\t') || c == BOM)
        {
            self.abandon();
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "invalid Unicode scalar value written to text stream",
            ));
        }

        match self.inner.write_all_utf8(&self.buffer) {
            Ok(()) => (),
            Err(e) => {
                self.abandon();
                return Err(e);
            }
        }

        if let Some(last) = self.buffer.as_bytes().last() {
            self.nl.0 = *last == b'\n';
        }

        // Reset the temporary buffer.
        self.buffer.clear();

        Ok(())
    }

    fn check_nl(&mut self, status: Status) -> io::Result<()> {
        match status {
            Status::End => {
                if !self.nl.0 {
                    self.abandon();
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "output text stream must end with newline",
                    ));
                }
            }
            Status::Open(Readiness::Lull) => {
                if !self.nl.0 {
                    self.abandon();
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "output text stream lull must be preceded by newline",
                    ));
                }
            }
            Status::Open(Readiness::Ready) => (),
        }
        Ok(())
    }
}

impl<Inner: Write> Write for TextWriter<Inner> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match str::from_utf8(buf) {
            Ok(s) => self.write_all_utf8(s).map(|_| buf.len()),
            Err(error) if error.valid_up_to() != 0 => self
                .write_all(&buf[..error.valid_up_to()])
                .map(|_| buf.len()),
            Err(error) => {
                self.abandon();
                Err(io::Error::new(io::ErrorKind::Other, error))
            }
        }
    }

    fn flush(&mut self, status: Status) -> io::Result<()> {
        if status != Status::ready() {
            self.expect_starter = true;
        }
        self.check_nl(status)?;
        self.inner.flush(status)
    }

    fn abandon(&mut self) {
        self.inner.abandon();

        // Don't enforce a trailing newline.
        self.nl.0 = true;
    }

    fn write_all_utf8(&mut self, s: &str) -> io::Result<()> {
        if self.crlf_compatibility {
            self.crlf_write_all_utf8(s)
        } else {
            self.normal_write_all_utf8(s)
        }
    }
}

struct NlGuard(bool);

impl Drop for NlGuard {
    fn drop(&mut self) {
        if !self.0 {
            panic!("output text stream not ended with newline");
        }
    }
}

#[cfg(test)]
fn translate_via_std_writer(bytes: &[u8]) -> io::Result<String> {
    let mut writer = TextWriter::new(crate::StdWriter::new(Vec::<u8>::new()));
    writer.write_all(bytes)?;
    let inner = writer.close_into_inner()?;
    Ok(String::from_utf8(inner.get_ref().to_vec()).unwrap())
}

#[cfg(test)]
fn test(bytes: &[u8], s: &str) {
    assert_eq!(translate_via_std_writer(bytes).unwrap(), s);
}

#[cfg(test)]
fn test_error(bytes: &[u8]) {
    assert!(translate_via_std_writer(bytes).is_err());
}

#[test]
fn test_empty_string() {
    test_error(b"");
}

#[test]
fn test_nl() {
    test(b"\n", "\n");
    test(b"\nhello\nworld\n", "\nhello\nworld\n");
}

#[test]
fn test_bom() {
    test_error("\u{feff}".as_bytes());
    test_error("\u{feff}hello\u{feff}world\u{feff}".as_bytes());
    test_error("\u{feff}hello world".as_bytes());
    test_error("hello\u{feff}world".as_bytes());
    test_error("hello world\u{feff}".as_bytes());
}

#[test]
fn test_crlf() {
    test_error(b"\r\n");
    test_error(b"\r\nhello\r\nworld\r\n");
    test_error(b"\r\nhello world");
    test_error(b"hello\r\nworld");
    test_error(b"hello world\r\n");
}

#[test]
fn test_cr_plain() {
    test_error(b"\r");
    test_error(b"\rhello\rworld\r");
    test_error(b"\rhello world");
    test_error(b"hello\rworld");
    test_error(b"hello world\r");
}

#[test]
fn test_ff() {
    test_error(b"\x0c");
    test_error(b"\x0chello\x0cworld\x0c");
    test_error(b"\x0chello world");
    test_error(b"hello\x0cworld");
    test_error(b"hello world\x0c");
}

#[test]
fn test_del() {
    test_error(b"\x7f");
    test_error(b"\x7fhello\x7fworld\x7f");
    test_error(b"\x7fhello world");
    test_error(b"hello\x7fworld");
    test_error(b"hello world\x7f");
}

#[test]
fn test_non_text_c0() {
    test_error(b"\x00");
    test_error(b"\x01");
    test_error(b"\x02");
    test_error(b"\x03");
    test_error(b"\x04");
    test_error(b"\x05");
    test_error(b"\x06");
    test_error(b"\x07");
    test_error(b"\x08");
    test_error(b"\x0b");
    test_error(b"\x0e");
    test_error(b"\x0f");
    test_error(b"\x10");
    test_error(b"\x11");
    test_error(b"\x12");
    test_error(b"\x13");
    test_error(b"\x14");
    test_error(b"\x15");
    test_error(b"\x16");
    test_error(b"\x17");
    test_error(b"\x18");
    test_error(b"\x19");
    test_error(b"\x1a");
    test_error(b"\x1b");
    test_error(b"\x1c");
    test_error(b"\x1d");
    test_error(b"\x1e");
    test_error(b"\x1f");
}

#[test]
fn test_c1() {
    test_error("\u{80}".as_bytes());
    test_error("\u{81}".as_bytes());
    test_error("\u{82}".as_bytes());
    test_error("\u{83}".as_bytes());
    test_error("\u{84}".as_bytes());
    test_error("\u{85}".as_bytes());
    test_error("\u{86}".as_bytes());
    test_error("\u{87}".as_bytes());
    test_error("\u{88}".as_bytes());
    test_error("\u{89}".as_bytes());
    test_error("\u{8a}".as_bytes());
    test_error("\u{8b}".as_bytes());
    test_error("\u{8c}".as_bytes());
    test_error("\u{8d}".as_bytes());
    test_error("\u{8e}".as_bytes());
    test_error("\u{8f}".as_bytes());
    test_error("\u{90}".as_bytes());
    test_error("\u{91}".as_bytes());
    test_error("\u{92}".as_bytes());
    test_error("\u{93}".as_bytes());
    test_error("\u{94}".as_bytes());
    test_error("\u{95}".as_bytes());
    test_error("\u{96}".as_bytes());
    test_error("\u{97}".as_bytes());
    test_error("\u{98}".as_bytes());
    test_error("\u{99}".as_bytes());
    test_error("\u{9a}".as_bytes());
    test_error("\u{9b}".as_bytes());
    test_error("\u{9c}".as_bytes());
    test_error("\u{9d}".as_bytes());
    test_error("\u{9e}".as_bytes());
    test_error("\u{9f}".as_bytes());
}

#[test]
fn test_nfc() {
    test("\u{212b}\n".as_bytes(), "\u{c5}\n");
    test("\u{c5}\n".as_bytes(), "\u{c5}\n");
    test("\u{41}\u{30a}\n".as_bytes(), "\u{c5}\n");
}

#[test]
fn test_leading_nonstarters() {
    test_error("\u{30a}".as_bytes());
}

#[test]
fn test_esc() {
    test_error(b"\x1b");
    test_error(b"\x1b@");
    test_error(b"\x1b@hello\x1b@world\x1b@");
}

#[test]
fn test_csi() {
    test_error(b"\x1b[");
    test_error(b"\x1b[@hello\x1b[@world\x1b[@");
    test_error(b"\x1b[+@hello\x1b[+@world\x1b[+@");
}

#[test]
fn test_osc() {
    test_error(b"\x1b]");
    test_error(b"\x1b]\x07hello\x1b]\x07world\x1b]\x07");
    test_error(b"\x1b]message\x07hello\x1b]message\x07world\x1b]message\x07");
    test_error(b"\x1b]mes\ns\tage\x07hello\x1b]mes\ns\tage\x07world\x1b]mes\ns\tage\x07");
}

#[test]
fn test_linux() {
    test_error(b"\x1b[[A");
    test_error(b"\x1b[[Ahello\x1b[[Aworld\x1b[[A");
}

// TODO: Test Stream-Safe
// TODO: test for nonstarter after lull
