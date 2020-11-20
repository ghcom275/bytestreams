use bytestreams::{Read, StdReader, StdWriter, Utf8Reader, Utf8Writer, Write};

fn main() -> anyhow::Result<()> {
    let mut reader = Utf8Reader::new(StdReader::new(std::io::stdin()));
    let mut stdout = Utf8Writer::new(StdWriter::new(std::io::stdout()));
    let mut buf = [0; 8];
    loop {
        let outcome = reader.read_outcome(&mut buf)?;
        stdout.write_all(&buf[..outcome.size])?;
        stdout.flush(outcome.status)?;
        if outcome.status.is_end() {
            return Ok(());
        }
    }
}
