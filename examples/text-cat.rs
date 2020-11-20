use bytestreams::{
    Read, StdReader, StdWriter, TextReader, TextWriter, Write, NORMALIZATION_BUFFER_SIZE,
};

fn main() -> anyhow::Result<()> {
    let mut reader = TextReader::new(StdReader::new(std::io::stdin()));
    let mut stdout = TextWriter::new(StdWriter::new(std::io::stdout()));
    let mut buf = [0; NORMALIZATION_BUFFER_SIZE];
    loop {
        let outcome = reader.read_outcome(&mut buf)?;
        stdout.write_all(&buf[..outcome.size])?;
        stdout.flush(outcome.status)?;
        if outcome.status.is_end() {
            return Ok(());
        }
    }
}
