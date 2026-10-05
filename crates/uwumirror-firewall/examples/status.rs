//! What the firewall says about a program, read-only (no administrator):
//!
//!   cargo run -p uwumirror-firewall --example status -- <path to UwUMirror.exe>
//!
//! `--scripts` prints the PowerShell the setup would run elevated, without
//! running it.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = args
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .expect("a program path");
    if args.iter().any(|arg| arg == "--scripts") {
        println!("{}", uwumirror_firewall::set_up_script(&exe));
        println!("{}", uwumirror_firewall::remove_script(&exe));
        return;
    }
    println!("{}", exe.display());
    println!("{:#?}", uwumirror_firewall::status(&exe));
    println!("has rules: {}", uwumirror_firewall::has_rules(&exe));
}
