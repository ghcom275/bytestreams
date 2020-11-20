use bytestreams::{Read, StdReader};
use std::fs;

fn main() -> anyhow::Result<()> {
    let f = fs::File::open("Cargo.toml")?;
    let mut f = StdReader::new(f);

    let mut v = String::new();
    f.read_to_string(&mut v)?;

    Ok(())
}
