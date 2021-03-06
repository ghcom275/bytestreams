use crate::{unicode::REPL, Read, ReadOutcome};
use std::{cmp::min, io, str};

/// A `Read` implementation which translates from an input `Read` producing
/// an arbitrary byte sequence into a valid UTF-8 sequence with invalid
/// sequences replaced by U+FFFD (REPLACEMENT CHARACTER) in the manner of
/// `String::from_utf8_lossy`, where scalar value encodings never straddle `read`
/// calls (callers can do `str::from_utf8` and it will always succeed).
pub struct Utf8Reader<Inner: Read> {
    /// The wrapped byte stream.
    inner: Inner,

    /// A queue of bytes which have not been read but which have not been
    /// translated into the output yet.
    overflow: Vec<u8>,
}

impl<Inner: Read> Utf8Reader<Inner> {
    /// Construct a new instance of `Utf8Reader` wrapping `inner`.
    #[inline]
    pub fn new(inner: Inner) -> Self {
        Self {
            inner,
            overflow: Vec::new(),
        }
    }

    /// Like `read` but produces the result in a `str`. Be sure to check
    /// the `size` field of the return value to see how many bytes were written.
    pub fn read_utf8(&mut self, buf: &mut str) -> io::Result<ReadOutcome> {
        let outcome = unsafe { self.read_outcome(buf.as_bytes_mut()) }?;

        debug_assert!(buf.is_char_boundary(outcome.size));

        Ok(outcome)
    }
}

impl<Inner: Read> Read for Utf8Reader<Inner> {
    fn read_outcome(&mut self, buf: &mut [u8]) -> io::Result<ReadOutcome> {
        // To ensure we can always make progress, callers should always use a
        // buffer of at least 4 bytes.
        if buf.len() < 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "buffer for reading from Utf8Reader must be at least 4 bytes long",
            ));
        }

        let mut nread = 0;

        if !self.overflow.is_empty() {
            nread += self
                .process_overflow(&mut buf[nread..], IncompleteHow::Include)
                .unwrap();
            if !self.overflow.is_empty() {
                return Ok(ReadOutcome::ready(nread));
            }
        }

        let outcome = self.inner.read_outcome(&mut buf[nread..])?;
        nread += outcome.size;

        match str::from_utf8(&buf[..nread]) {
            Ok(_) => Ok(ReadOutcome {
                size: nread,
                status: outcome.status,
            }),
            Err(error) => {
                let (valid, after_valid) = buf[..nread].split_at(error.valid_up_to());
                nread = valid.len();

                assert!(self.overflow.is_empty());
                self.overflow.extend_from_slice(after_valid);

                let incomplete_how = if outcome.status.is_end() {
                    IncompleteHow::Replace
                } else {
                    IncompleteHow::Exclude
                };
                nread += self
                    .process_overflow(&mut buf[nread..], incomplete_how)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "invalid UTF-8"))?;
                if self.overflow.is_empty() {
                    Ok(ReadOutcome {
                        size: nread,
                        status: outcome.status,
                    })
                } else {
                    Ok(ReadOutcome::ready(nread))
                }
            }
        }
    }
}

impl<Inner: Read> io::Read for Utf8Reader<Inner> {
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

impl<Inner: Read> Utf8Reader<Inner> {
    /// If normal reading encounters invalid bytes, the data is copied into
    /// `self.overflow` as it may need to expand to make room for the U+FFFD's,
    /// and we may need to hold on to some of it until the next `read` call.
    ///
    /// TODO: This code could be significantly optimized.
    #[cold]
    fn process_overflow(&mut self, buf: &mut [u8], incomplete_how: IncompleteHow) -> Option<usize> {
        let mut nread = 0;

        loop {
            let num = min(buf[nread..].len(), self.overflow.len());
            match str::from_utf8(&self.overflow[..num]) {
                Ok(_) => {
                    buf[nread..nread + num].copy_from_slice(&self.overflow[..num]);
                    self.overflow.copy_within(num.., 0);
                    self.overflow.resize(self.overflow.len() - num, 0);
                    nread += num;
                }
                Err(error) => {
                    let (valid, after_valid) = self.overflow[..num].split_at(error.valid_up_to());
                    let valid_len = valid.len();
                    let after_valid_len = after_valid.len();
                    buf[nread..nread + valid_len].copy_from_slice(valid);
                    self.overflow.copy_within(valid_len.., 0);
                    self.overflow.resize(self.overflow.len() - valid_len, 0);
                    nread += valid_len;

                    if let Some(invalid_sequence_length) = error.error_len() {
                        if REPL.len_utf8() <= buf[nread..].len() {
                            nread += REPL.encode_utf8(&mut buf[nread..]).len();
                            self.overflow.copy_within(invalid_sequence_length.., 0);
                            self.overflow
                                .resize(self.overflow.len() - invalid_sequence_length, 0);
                            continue;
                        }
                    } else {
                        match incomplete_how {
                            IncompleteHow::Replace => {
                                if REPL.len_utf8() <= buf[nread..].len() {
                                    nread += REPL.encode_utf8(&mut buf[nread..]).len();
                                    self.overflow.clear();
                                } else if self.overflow.is_empty() {
                                    return None;
                                }
                            }
                            IncompleteHow::Include if after_valid_len == self.overflow.len() => {
                                if !buf[nread..].is_empty() {
                                    let num = min(buf[nread..].len(), after_valid_len);
                                    buf[nread..nread + num].copy_from_slice(&self.overflow[..num]);
                                    nread += num;
                                    self.overflow.copy_within(num.., 0);
                                    self.overflow.resize(self.overflow.len() - num, 0);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            break;
        }

        Some(nread)
    }
}

/// What to do when there is an incomplete UTF-8 sequence at the end of
/// the overflow buffer.
enum IncompleteHow {
    /// Include the incomplete sequence in the output.
    Include,
    /// Leave the incomplete sequence in the overflow buffer.
    Exclude,
    /// Replace the incomplete sequence with U+FFFD.
    Replace,
}

#[cfg(test)]
fn translate_via_std_reader(bytes: &[u8]) -> String {
    let mut reader = Utf8Reader::new(crate::StdReader::generic(bytes));
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    s
}

#[cfg(test)]
fn translate_via_slice_reader(bytes: &[u8]) -> String {
    let mut reader = Utf8Reader::new(crate::SliceReader::new(bytes));
    let mut s = String::new();
    reader.read_to_string(&mut s).unwrap();
    s
}

#[cfg(test)]
fn translate_with_small_buffer(bytes: &[u8]) -> String {
    let mut reader = Utf8Reader::new(crate::SliceReader::new(bytes));
    let mut v = Vec::new();
    let mut buf = [0; crate::unicode::MAX_UTF8_SIZE];
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

    for i in 1..4 {
        let mut v = vec![0u8; i + bytes.len()];
        v[i..i + bytes.len()].copy_from_slice(bytes);
        assert_eq!(
            str::from_utf8(&translate_via_std_reader(&v).as_bytes()[i..]).unwrap(),
            s
        );
        assert_eq!(
            str::from_utf8(&translate_via_slice_reader(&v).as_bytes()[i..]).unwrap(),
            s
        );
        assert_eq!(
            str::from_utf8(&translate_with_small_buffer(&v).as_bytes()[i..]).unwrap(),
            s
        );
    }
}

#[test]
fn test_empty_string() {
    test(b"", "");
}

#[test]
fn test_hello_world() {
    test(b"hello world", "hello world");
}

#[test]
fn test_embedded_invalid_byte() {
    test(b"hello\xffworld", "hello�world");
}

#[test]
fn test_invalid_bytes() {
    test(b"\xff\xff\xff", "���");
}

#[test]
fn test_some_ascii_printable() {
    test(
        b"`1234567890-=qwertyuiop[]\\asdfghjkl;\"zxcvbnm,./",
        "`1234567890-=qwertyuiop[]\\asdfghjkl;\"zxcvbnm,./",
    );
}

// Tests derived from the tests in https://hsivonen.fi/broken-utf-8/

// Non-shortest forms for lowest single-byte (U+0000)
#[test]
fn test_two_byte_sequence_lowest_single_byte() {
    test(b"\xC0\x80", "��");
}
#[test]
fn test_three_byte_sequence_lowest_single_byte() {
    test(b"\xE0\x80\x80", "���");
}
#[test]
fn test_four_byte_sequence_lowest_single_byte() {
    test(b"\xF0\x80\x80\x80", "����");
}
#[test]
fn test_five_byte_sequence_lowest_single_byte() {
    test(b"\xF8\x80\x80\x80\x80", "�����");
}
#[test]
fn test_six_byte_sequence_lowest_single_byte() {
    test(b"\xFC\x80\x80\x80\x80\x80", "������");
}

// Non-shortest forms for highest single-byte (U+007F)
#[test]
fn test_two_byte_sequence_highest_single_byte() {
    test(b"\xC1\xBF", "��");
}
#[test]
fn test_three_byte_sequence_highest_single_byte() {
    test(b"\xE0\x81\xBF", "���");
}
#[test]
fn test_four_byte_sequence_highest_single_byte() {
    test(b"\xF0\x80\x81\xBF", "����");
}
#[test]
fn test_five_byte_sequence_highest_single_byte() {
    test(b"\xF8\x80\x80\x81\xBF", "�����");
}
#[test]
fn test_six_byte_sequence_highest_single_byte() {
    test(b"\xFC\x80\x80\x80\x81\xBF", "������");
}

// Non-shortest forms for lowest two-byte (U+0080)
#[test]
fn test_three_byte_sequence_lowest_two_byte() {
    test(b"\xE0\x82\x80", "���");
}
#[test]
fn test_four_byte_sequence_lowest_two_byte() {
    test(b"\xF0\x80\x82\x80", "����");
}
#[test]
fn test_five_byte_sequence_lowest_two_byte() {
    test(b"\xF8\x80\x80\x82\x80", "�����");
}
#[test]
fn test_six_byte_sequence_lowest_two_byte() {
    test(b"\xFC\x80\x80\x80\x82\x80", "������");
}

// Non-shortest forms for highest two-byte (U+07FF)
#[test]
fn test_three_byte_sequence_highest_two_byte() {
    test(b"\xE0\x9F\xBF", "���");
}
#[test]
fn test_four_byte_sequence_highest_two_byte() {
    test(b"\xF0\x80\x9F\xBF", "����");
}
#[test]
fn test_five_byte_sequence_highest_two_byte() {
    test(b"\xF8\x80\x80\x9F\xBF", "�����");
}
#[test]
fn test_six_byte_sequence_highest_two_byte() {
    test(b"\xFC\x80\x80\x80\x9F\xBF", "������");
}

// Non-shortest forms for lowest three-byte (U+0800)
#[test]
fn test_four_byte_sequence_lowest_three_byte() {
    test(b"\xF0\x80\xA0\x80", "����");
}
#[test]
fn test_five_byte_sequence_lowest_three_byte() {
    test(b"\xF8\x80\x80\xA0\x80", "�����");
}
#[test]
fn test_six_byte_sequence_lowest_three_byte() {
    test(b"\xFC\x80\x80\x80\xA0\x80", "������");
}

// Non-shortest forms for highest three-byte (U+FFFF)
#[test]
fn test_four_byte_sequence_highest_three_byte() {
    test(b"\xF0\x8F\xBF\xBF", "����");
}
#[test]
fn test_five_byte_sequence_highest_three_byte() {
    test(b"\xF8\x80\x8F\xBF\xBF", "�����");
}
#[test]
fn test_six_byte_sequence_highest_three_byte() {
    test(b"\xFC\x80\x80\x8F\xBF\xBF", "������");
}

// Non-shortest forms for lowest four-byte (U+10000)
#[test]
fn test_five_byte_sequence_lowest_four_byte() {
    test(b"\xF8\x80\x90\x80\x80", "�����");
}
#[test]
fn test_six_byte_sequence_lowest_four_byte() {
    test(b"\xFC\x80\x80\x90\x80\x80", "������");
}

// Non-shortest forms for last Unicode (U+10FFFF)
#[test]
fn test_five_byte_sequence() {
    test(b"\xF8\x84\x8F\xBF\xBF", "�����");
}
#[test]
fn test_six_byte_sequence() {
    test(b"\xFC\x80\x84\x8F\xBF\xBF", "������");
}

// Out of range
#[test]
fn test_one_past_unicode() {
    test(b"\xF4\x90\x80\x80", "����");
}
#[test]
fn test_longest_five_byte_sequence() {
    test(b"\xFB\xBF\xBF\xBF\xBF", "�����");
}
#[test]
fn test_longest_six_byte_sequence() {
    test(b"\xFD\xBF\xBF\xBF\xBF\xBF", "������");
}
#[test]
fn test_first_surrogate() {
    test(b"\xED\xA0\x80", "���");
}
#[test]
fn test_last_surrogate() {
    test(b"\xED\xBF\xBF", "���");
}
#[test]
fn test_cesu_8_surrogate_pair() {
    test(b"\xED\xA0\xBD\xED\xB2\xA9", "������");
}

// Out of range and non-shortest
#[test]
fn test_one_past_unicode_as_five_byte_sequence() {
    test(b"\xF8\x84\x90\x80\x80", "�����");
}
#[test]
fn test_one_past_unicode_as_six_byte_sequence() {
    test(b"\xFC\x80\x84\x90\x80\x80", "������");
}
#[test]
fn test_first_surrogate_as_four_byte_sequence() {
    test(b"\xF0\x8D\xA0\x80", "����");
}
#[test]
fn test_last_surrogate_as_four_byte_sequence() {
    test(b"\xF0\x8D\xBF\xBF", "����");
}
#[test]
fn test_cesu_8_surrogate_pair_as_two_four_byte_overlongs() {
    test(b"\xF0\x8D\xA0\xBD\xF0\x8D\xB2\xA9", "��������");
}

// Lone trails
#[test]
fn test_one() {
    test(b"\x80", "�");
}
#[test]
fn test_two() {
    test(b"\x80\x80", "��");
}
#[test]
fn test_three() {
    test(b"\x80\x80\x80", "���");
}
#[test]
fn test_four() {
    test(b"\x80\x80\x80\x80", "����");
}
#[test]
fn test_five() {
    test(b"\x80\x80\x80\x80\x80", "�����");
}
#[test]
fn test_six() {
    test(b"\x80\x80\x80\x80\x80\x80", "������");
}
#[test]
fn test_seven() {
    test(b"\x80\x80\x80\x80\x80\x80\x80", "�������");
}
#[test]
fn test_after_valid_two_byte() {
    test(b"\xC2\xB6\x80", "¶�");
}
#[test]
fn test_after_valid_three_byte() {
    test(b"\xE2\x98\x83\x80", "☃�");
}
#[test]
fn test_after_valid_four_byte() {
    test(b"\xF0\x9F\x92\xA9\x80", "💩�");
}
#[test]
fn test_after_five_byte() {
    test(b"\xFB\xBF\xBF\xBF\xBF\x80", "������");
}
#[test]
fn test_after_six_byte() {
    test(b"\xFD\xBF\xBF\xBF\xBF\xBF\x80", "�������");
}

// Truncated_sequences
#[test]
fn test_two_byte_lead() {
    test(b"\xC2", "�");
}
#[test]
fn test_three_byte_lead() {
    test(b"\xE2", "�");
}
#[test]
fn test_three_byte_lead_and_one_trail() {
    test(b"\xE2\x98", "�");
}
#[test]
fn test_four_byte_lead() {
    test(b"\xF0", "�");
}
#[test]
fn test_four_byte_lead_and_one_trail() {
    test(b"\xF0\x9F", "�");
}
#[test]
fn test_four_byte_lead_and_two_trails() {
    test(b"\xF0\x9F\x92", "�");
}

// Leftovers
#[test]
fn test_fe() {
    test(b"\xFE", "�");
}
#[test]
fn test_fe_and_trail() {
    test(b"\xFE\x80", "��");
}
#[test]
fn test_ff() {
    test(b"\xFF", "�");
}
#[test]
fn test_ff_and_trail() {
    test(b"\xFF\x80", "��");
}
