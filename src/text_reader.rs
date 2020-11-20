use crate::{
    no_forbidden_characters::NoForbiddenCharacters,
    rc_char_queue::{RcCharQueue, RcCharQueueIter},
    unicode::{
        is_normalization_form_starter, BOM, DEL, ESC, FF, MAX_UTF8_SIZE, NORMALIZATION_BUFFER_LEN,
        NORMALIZATION_BUFFER_SIZE, REPL,
    },
    Read, ReadOutcome, Status, Utf8Reader,
};
use std::{io, mem, str};
use unicode_normalization::{Recompositions, StreamSafe, UnicodeNormalization};

/// A `Read` implementation which translates from an input `Read` producing
/// an arbitrary byte sequence into a valid plain text stream.
///
/// In addition to the transforms performed by `Utf8Reader`, an input text
/// stream ensures the following properties:
///  - U+FEFF (BOM) scalar values are stripped
///  - A '\n' is appended at the end of the stream if it doesn't already
///    have one.
///  - '\r' followed by '\n' is replaced by '\n'.
///  - U+000C (FF) is replaced by ' '.
///  - All other control codes other than '\n' and '\t' are replaced
///    by U+FFFD (REPLACEMENT CHARACTER).
///  - Text is transformed to Normalization Form C (NFC).
///  - The Stream-Safe Text Process (UAX15-D4) is applied.
///  - Streams never start or resume after a lull with a normalization-form
///    non-starter.
///
/// TODO: use `from_utf8_unchecked` and `as_mut_vec` to optimize this.
///
/// TODO: canonical_combining_class doesn't know about the astral
/// compositions like U+11099 U+110BA => U+1109A. Restrict non-starters
/// of that form too? Or use unicode-segmentation to detect grapheme boundaries.
///
/// TODO: support security restrictions? Or have a mode where they are supported?
///   - [Unicode Restriction Levels](https://www.unicode.org/reports/tr39/#Restriction_Level_Detection)
///   - [unicode-security crate](https://crates.io/crates/unicode-security)
///
/// TODO: Forbidden characters?
///   - [11.4 Forbidden Characters](https://unicode.org/reports/tr15/#Forbidding_Characters)
///
/// TODO: Problem sequences?
///   - [11.5 Problem Sequences](https://unicode.org/reports/tr15/#Corrigendum_5_Sequences)
///
/// TODO: Implement Stablized Strings
///   - [12.1 Stablized Strings](https://unicode.org/reports/tr15/#Normalization_Process_for_Stabilized_Strings)
///
/// TODO: NFC is not closed under concatenation.
pub struct TextReader<Inner: Read> {
    /// The wrapped byte stream.
    inner: Utf8Reader<Inner>,

    /// Temporary storage for reading scalar values from the underlying stream.
    raw_string: String,

    /// A queue of scalar values which have been translated but not written to
    /// the output yet.
    /// TODO: This is awkward; what we really want here is a streaming stream-safe
    /// and NFC translator.
    queue: RcCharQueue,

    /// An iterator over the chars in `self.queue`.
    queue_iter: Option<NoForbiddenCharacters<Recompositions<StreamSafe<RcCharQueueIter>>>>,

    /// When we can't fit all the data from an underlying read in our buffer,
    /// we buffer it up. Remember the status value so we can replay that too.
    pending_status: Status,

    /// At the beginning of a stream or after a lull, expect a
    /// normalization-form starter.
    expect_starter: bool,

    /// Control-code and escape-sequence state machine.
    state: State,
}

impl<Inner: Read> TextReader<Inner> {
    /// Construct a new instance of `TextReader` wrapping `inner`.
    #[inline]
    pub fn new(inner: Inner) -> Self {
        let queue = RcCharQueue::new();
        Self {
            inner: Utf8Reader::new(inner),
            raw_string: String::new(),
            queue,
            queue_iter: None,
            pending_status: Status::ready(),
            expect_starter: true,
            state: State::Ground(true),
        }
    }

    /// Like `read` but produces the result in a `str`. Be sure to check
    /// the `size` field of the return value to see how many bytes were written.
    pub fn read_utf8(&mut self, buf: &mut str) -> io::Result<ReadOutcome> {
        self.inner.read_utf8(buf)
    }

    fn queue_next(&mut self, sequence_end: bool) -> Option<char> {
        if !sequence_end && self.queue.len() < NORMALIZATION_BUFFER_LEN {
            return None;
        }
        if self.queue_iter.is_none() {
            if self.queue.is_empty() {
                return None;
            }
            self.queue_iter = Some(NoForbiddenCharacters::new(
                self.queue.iter().stream_safe().nfc(),
            ));
        }
        if let Some(c) = self.queue_iter.as_mut().unwrap().next() {
            return Some(c.unwrap_or(REPL));
        }
        self.queue_iter = None;
        None
    }

    fn process_raw_string(&mut self) {
        for c in self.raw_string.chars() {
            loop {
                match (self.state, c) {
                    (State::Ground(_), BOM) => self.state = State::Ground(false),
                    (State::Ground(_), '\n') => {
                        self.queue.push('\n');
                        self.state = State::Ground(true)
                    }
                    (State::Ground(_), '\t') => {
                        self.queue.push('\t');
                        self.state = State::Ground(false)
                    }
                    (State::Ground(_), FF) => {
                        self.queue.push(' ');
                        self.state = State::Ground(false)
                    }
                    (State::Ground(_), '\r') => self.state = State::Cr,
                    (State::Ground(_), ESC) => self.state = State::Esc,
                    (State::Ground(_), c) if c.is_control() => {
                        self.queue.push(REPL);
                        self.state = State::Ground(false);
                    }
                    (State::Ground(_), mut c) => {
                        if self.expect_starter {
                            self.expect_starter = false;
                            if !is_normalization_form_starter(c) {
                                c = REPL;
                            }
                        }
                        self.queue.push(c);
                        self.state = State::Ground(false)
                    }

                    (State::Cr, '\n') => {
                        self.queue.push('\n');
                        self.state = State::Ground(true);
                    }
                    (State::Cr, _) => {
                        self.queue.push(REPL);
                        self.state = State::Ground(false);
                        continue;
                    }

                    (State::Esc, '[') => self.state = State::CsiStart,
                    (State::Esc, ']') => self.state = State::Osc,
                    (State::Esc, c) if ('@'..='~').contains(&c) => {
                        self.state = State::Ground(false)
                    }
                    (State::Esc, _) => {
                        self.state = State::Ground(false);
                        continue;
                    }

                    (State::CsiStart, '[') => self.state = State::Linux,
                    (State::CsiStart, c) | (State::Csi, c) if (' '..='?').contains(&c) => {
                        self.state = State::Csi
                    }
                    (State::CsiStart, c) | (State::Csi, c) if ('@'..='~').contains(&c) => {
                        self.state = State::Ground(false)
                    }
                    (State::CsiStart, _) | (State::Csi, _) => {
                        self.state = State::Ground(false);
                        continue;
                    }

                    (State::Osc, c) if !c.is_control() || c == '\n' || c == '\t' => (),
                    (State::Osc, _) => self.state = State::Ground(false),

                    (State::Linux, c) if ('\0'..=DEL).contains(&c) => {
                        self.state = State::Ground(false)
                    }
                    (State::Linux, _) => {
                        self.state = State::Ground(false);
                        continue;
                    }
                }
                break;
            }
        }
    }
}

impl<Inner: Read> Read for TextReader<Inner> {
    fn read_outcome(&mut self, buf: &mut [u8]) -> io::Result<ReadOutcome> {
        if buf.len() < NORMALIZATION_BUFFER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "buffer for text input must be at least NORMALIZATION_BUFFER_SIZE bytes",
            ));
        }

        let mut nread = 0;

        loop {
            match self.queue_next(false) {
                Some(c) => nread += c.encode_utf8(&mut buf[nread..]).len(),
                None => break,
            }
            if buf.len() - nread < MAX_UTF8_SIZE {
                return Ok(ReadOutcome::ready(nread));
            }
        }
        if self.pending_status != Status::ready() {
            self.pending_status = Status::ready();
            self.expect_starter = true;
            return Ok(ReadOutcome {
                size: nread,
                status: self.pending_status,
            });
        }

        let mut raw_bytes = mem::replace(&mut self.raw_string, String::new()).into_bytes();
        raw_bytes.resize(4096, 0_u8);
        let outcome = self.inner.read_outcome(&mut raw_bytes)?;
        raw_bytes.resize(outcome.size, 0);
        self.raw_string = String::from_utf8(raw_bytes).unwrap();

        self.process_raw_string();

        if outcome.status != Status::ready() {
            match self.state {
                State::Ground(_) => {}
                State::Cr => {
                    self.queue.push(REPL);
                    self.state = State::Ground(false);
                }
                State::Esc | State::CsiStart | State::Csi | State::Osc | State::Linux => {
                    self.state = State::Ground(false);
                }
            }

            if outcome.status.is_end() && self.state != State::Ground(true) {
                self.queue.push('\n');
                self.state = State::Ground(true);
            }
        }

        loop {
            match self.queue_next(outcome.status != Status::ready()) {
                Some(c) => nread += c.encode_utf8(&mut buf[nread..]).len(),
                None => break,
            }
            if buf.len() - nread < MAX_UTF8_SIZE {
                break;
            }
        }

        Ok(ReadOutcome {
            size: nread,
            status: if self.queue_iter.is_none() {
                if outcome.status != Status::ready() {
                    self.expect_starter = true;
                }
                outcome.status
            } else {
                self.pending_status = outcome.status;
                Status::ready()
            },
        })
    }
}

impl<Inner: Read> io::Read for TextReader<Inner> {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Read::read(self, buf)
    }

    #[inline]
    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        Read::read_vectored(self, bufs)
    }

    #[cfg(feature = "nightly")]
    #[inline]
    fn is_read_vectored(&self) -> bool {
        Read::is_read_vectored(self)
    }

    #[inline]
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        Read::read_to_end(self, buf)
    }

    #[inline]
    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        Read::read_to_string(self, buf)
    }

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        Read::read_exact(self, buf)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    // Default state. Boolean is true iff we just saw a '\n'.
    Ground(bool),

    // After a '\r'.
    Cr,

    // After a '\x1b'.
    Esc,

    // Immediately after a "\x1b[".
    CsiStart,

    // Within a sequence started by "\x1b[".
    Csi,

    // Within a sequence started by "\x1b]".
    Osc,

    // After a "\x1b[[".
    Linux,
}

#[cfg(test)]
fn translate_via_std_reader(bytes: &[u8]) -> String {
    let mut reader = TextReader::new(crate::StdReader::generic(bytes));
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    s
}

#[cfg(test)]
fn translate_via_slice_reader(bytes: &[u8]) -> String {
    let mut reader = TextReader::new(crate::SliceReader::new(bytes));
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    s
}

#[cfg(test)]
fn translate_with_small_buffer(bytes: &[u8]) -> String {
    let mut reader = TextReader::new(crate::SliceReader::new(bytes));
    let mut v = Vec::new();
    let mut buf = [0; NORMALIZATION_BUFFER_SIZE];
    loop {
        let ReadOutcome { size, status } = reader.read_outcome(&mut buf).unwrap();
        v.extend_from_slice(&buf[..size]);
        if status.is_end() {
            break;
        }
    }
    String::from_utf8(v).unwrap()
}

#[cfg(test)]
fn test(bytes: &[u8], s: &str) {
    assert_eq!(translate_via_std_reader(bytes), s);
    assert_eq!(translate_via_slice_reader(bytes), s);
    assert_eq!(translate_with_small_buffer(bytes), s);
}

#[test]
fn test_empty_string() {
    test(b"", "");
}

#[test]
fn test_nl() {
    test(b"\n", "\n");
    test(b"\nhello\nworld\n", "\nhello\nworld\n");
}

#[test]
fn test_bom() {
    test("\u{feff}".as_bytes(), "\n");
    test(
        "\u{feff}hello\u{feff}world\u{feff}".as_bytes(),
        "helloworld\n",
    );
}

#[test]
fn test_crlf() {
    test(b"\r\n", "\n");
    test(b"\r\nhello\r\nworld\r\n", "\nhello\nworld\n");
}

#[test]
fn test_cr_plain() {
    test(b"\r", "\u{fffd}\n");
    test(b"\rhello\rworld\r", "\u{fffd}hello\u{fffd}world\u{fffd}\n");
}

#[test]
fn test_ff() {
    test(b"\x0c", " \n");
    test(b"\x0chello\x0cworld\x0c", " hello world \n");
}

#[test]
fn test_del() {
    test(b"\x7f", "\u{fffd}\n");
    test(
        b"\x7fhello\x7fworld\x7f",
        "\u{fffd}hello\u{fffd}world\u{fffd}\n",
    );
}

#[test]
fn test_non_text_c0() {
    test(
        b"\x00\x01\x02\x03\x04\x05\x06\x07",
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
    test(b"\x08\x0b\x0e\x0f", "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n");
    test(
        b"\x10\x11\x12\x13\x14\x15\x16\x17",
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
    test(
        b"\x18\x19\x1a\x1c\x1d\x1e\x1f",
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
}

#[test]
fn test_c1() {
    test(
        "\u{80}\u{81}\u{82}\u{83}\u{84}\u{85}\u{86}\u{87}".as_bytes(),
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
    test(
        "\u{88}\u{89}\u{8a}\u{8b}\u{8c}\u{8d}\u{8e}\u{8f}".as_bytes(),
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
    test(
        "\u{90}\u{91}\u{92}\u{93}\u{94}\u{95}\u{96}\u{97}".as_bytes(),
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
    test(
        "\u{98}\u{99}\u{9a}\u{9b}\u{9c}\u{9d}\u{9e}\u{9f}".as_bytes(),
        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd}\n",
    );
}

#[test]
fn test_nfc() {
    test("\u{212b}".as_bytes(), "\u{c5}\n");
    test("\u{c5}".as_bytes(), "\u{c5}\n");
    test("\u{41}\u{30a}".as_bytes(), "\u{c5}\n");
}

#[test]
fn test_leading_nonstarters() {
    test("\u{30a}".as_bytes(), "\u{fffd}\n");
}

#[test]
fn test_esc() {
    test(b"\x1b", "\n");
    test(b"\x1b@", "\n");
    test(b"\x1b@hello\x1b@world\x1b@", "helloworld\n");
}

#[test]
fn test_csi() {
    test(b"\x1b[", "\n");
    test(b"\x1b[@hello\x1b[@world\x1b[@", "helloworld\n");
    test(b"\x1b[+@hello\x1b[+@world\x1b[+@", "helloworld\n");
}

#[test]
fn test_osc() {
    test(b"\x1b]", "\n");
    test(b"\x1b]\x07hello\x1b]\x07world\x1b]\x07", "helloworld\n");
    test(
        b"\x1b]message\x07hello\x1b]message\x07world\x1b]message\x07",
        "helloworld\n",
    );
    test(
        b"\x1b]mes\ns\tage\x07hello\x1b]mes\ns\tage\x07world\x1b]mes\ns\tage\x07",
        "helloworld\n",
    );
}

#[test]
fn test_linux() {
    test(b"\x1b[[A", "\n");
    test(b"\x1b[[Ahello\x1b[[Aworld\x1b[[A", "helloworld\n");
}

// TODO: Test Stream-Safe
// TODO: test for nonstarter after lull
