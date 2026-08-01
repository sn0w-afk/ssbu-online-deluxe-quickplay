use linkle::format::nxo::NxoFile;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: elf2nro <input-elf> <output-nro>");
        std::process::exit(1);
    }
    let mut nxo = NxoFile::from_elf(&args[1]).expect("failed to parse ELF");
    nxo.write_nro(&mut std::fs::File::create(&args[2]).unwrap(), None, None, None)
        .expect("failed to write NRO");
    println!("wrote {}", args[2]);
}
