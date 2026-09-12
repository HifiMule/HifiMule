use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let profile = std::path::PathBuf::from(args.next().expect("profile path"));
    let hold_ms = args
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    match hifimule_lifecycle::OwnerGuard::acquire(&profile) {
        Ok(_owner) => {
            println!("acquired");
            std::thread::sleep(Duration::from_millis(hold_ms));
        }
        Err(error) => {
            eprintln!("{:?}", error.code());
            std::process::exit(2);
        }
    }
}
