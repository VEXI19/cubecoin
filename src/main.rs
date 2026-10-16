use crate::config::load_config;

mod config;

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config()?;
    println!("{config:#?}");

    Ok(())
}
