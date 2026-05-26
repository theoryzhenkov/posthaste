fn main() {
    if let Err(error) = posthaste_lab::run_cli(std::env::args().collect()) {
        eprintln!("posthaste-lab: {error}");
        std::process::exit(2);
    }
}
