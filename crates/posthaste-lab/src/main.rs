fn main() {
    if let Err(error) = posthaste_lab::run_cli(std::env::args().collect()) {
        let exit_code = error.exit_code();
        eprintln!("posthaste-lab: {error}");
        std::process::exit(exit_code);
    }
}
