use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let profile = std::path::PathBuf::from(args.next().expect("profile path"));
    let hold_ms = args
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let mode = args.next();
    if mode.as_deref() == Some("--ui-handoff") {
        let mut launch_request = hifimule_lifecycle::parse_ui_activation_request_args(
            args.map(std::ffi::OsString::from),
        )
        .unwrap_or_else(|error| {
            eprintln!("{:?}", error.code());
            std::process::exit(3);
        });
        let _guard = match hifimule_lifecycle::UiInstanceGuard::acquire(&profile) {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                let request_id = hifimule_lifecycle::request_ui_activation_for_launch(
                    &profile,
                    launch_request.as_deref(),
                )
                .unwrap();
                println!("requested");
                match hifimule_lifecycle::wait_for_ui_handoff(
                    &profile,
                    &request_id,
                    Duration::from_secs(2),
                ) {
                    Ok(Some(guard)) => guard,
                    Ok(None) => {
                        println!("handled");
                        return;
                    }
                    Err(error) => {
                        eprintln!("{:?}", error.code());
                        std::process::exit(3);
                    }
                }
            }
            Err(error) => {
                eprintln!("{:?}", error.code());
                std::process::exit(3);
            }
        };
        println!("acquired");
        // Exercise the same readiness decision as production after takeover.
        for _ in 0..2 {
            let destination =
                hifimule_lifecycle::take_ui_activation_destination(&profile, &mut launch_request)
                    .unwrap();
            println!("ready:{}", destination.unwrap_or("none"));
        }
        return;
    }
    if mode.as_deref() == Some("--ui") {
        match hifimule_lifecycle::UiInstanceGuard::acquire(&profile) {
            Ok(Some(_guard)) => {
                println!("acquired");
                std::thread::sleep(Duration::from_millis(hold_ms));
            }
            Ok(None) => {
                hifimule_lifecycle::request_ui_activation(&profile).unwrap();
                println!("requested");
            }
            Err(error) => {
                eprintln!("{:?}", error.code());
                std::process::exit(3);
            }
        }
        return;
    }
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
