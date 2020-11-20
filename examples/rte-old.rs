use std::{fs, io::Read};

fn main() -> anyhow::Result<()> {
    let mut f = fs::File::open("Cargo.toml")?;

    let mut v = Vec::new();
    f.read_to_end(&mut v)?;

    Ok(())
}
