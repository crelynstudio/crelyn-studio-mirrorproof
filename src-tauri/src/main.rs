#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--remove-schedules") {
        std::process::exit(
            if args.len() == 2 && mirrorproof_lib::remove_schedules().is_ok() {
                0
            } else {
                1
            },
        );
    }
    if args.get(1).map(|s| s.as_str()) == Some("--scheduled") {
        let result = args
            .get(2)
            .filter(|_| args.len() == 3)
            .ok_or_else(|| "invalidSchedule".to_string())
            .and_then(|id| mirrorproof_lib::run_scheduled(id));
        std::process::exit(match result {
            Ok(true) => 0,
            Ok(false) => 2,
            Err(_) => 1,
        });
    }
    mirrorproof_lib::run();
}
