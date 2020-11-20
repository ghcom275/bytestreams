<div align="center">
  <h1><code>bytestreams</code></h1>

  <p>
    <strong>Stream types and traits</strong>
  </p>

  <p>
    <a href="https://github.com/sunfishcode/bytestreams/actions?query=workflow%3ACI"><img src="https://github.com/sunfishcode/bytestreams/workflows/CI/badge.svg" alt="Github Actions CI Status" /></a>
    <a href="https://crates.io/crates/bytestreams"><img src="https://img.shields.io/crates/v/bytestreams.svg" alt="crates.io page" /></a>
    <a href="https://docs.rs/bytestreams"><img src="https://docs.rs/bytestreams/badge.svg" alt="docs.rs docs" /></a>
  </p>
</div>

This is an early experiment! The API and feature set are likely to
evolve significantly.

`bytestreams` defines several byte-oriented stream types and traits,
and utilities for working with them.

 - `Read` and `Write` are stream traits similar to [`std::io::Read`]
    and [`std::io::Write`] but have additional status features and
    functions for working with UTF-8 data.

 - `StdReader` and `StdWriter` provide adapters that wrap a `std::io::Read`
   or `std::io::Write` implementor and implement these `Read` or `Write`
   traits.

 - `SliceReader` implements `Read` for array slices.

 - `Utf8Reader` and `Utf8Writer` implement `Read` and `Write` and wrap
   arbitrary `Read` and `Write` streams. `Utf8Reader` translates invalid
   UTF-8 encodings into replacements (U+FFFD), while `Utf8Writer` reports
   errors on invalid UTF-8 encodings. Both ensure that scalar values
   are never split at the end of a buffer.

 - `TextReader` and `TextWriter` are similar to `Utf8Reader` and
   `Utf8Writer` but are for "plain text", which should not contain
   most control codes, escape sequences, other other content which
   may have a special meaning for a consumer.

[`std::io::Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
[`std::io::Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
