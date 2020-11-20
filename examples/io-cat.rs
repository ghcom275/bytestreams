use bytestreams::{Read, StdReader, StdWriter, Write};

fn main() -> anyhow::Result<()> {
    let mut reader = StdReader::new(std::io::stdin());
    let mut stdout = StdWriter::new(std::io::stdout());
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
