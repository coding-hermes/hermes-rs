use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "hermes-rs",
    version,
    about = "Hermes RS scaffold command-line interface"
)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
    println!("{}", hermes_rs::scaffold_status());
}
