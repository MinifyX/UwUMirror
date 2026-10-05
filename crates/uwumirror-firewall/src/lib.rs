//! UwUMirror's own Windows Firewall rules.
//!
//! UwUMirror installs per user, without an administrator, so it can't open
//! the firewall by itself. Windows' own prompt on the first start helps only
//! halfway: it lets UwUMirror in on *private* networks, but Miracast's
//! Wi-Fi Direct link counts as a *public* one, and there the picture never
//! arrives (Windows' player gives up with 0xC00D4278).
//!
//! So the setup, and the app on request, run one elevated PowerShell (one UAC
//! prompt) that replaces UwUMirror's rules — the group [`GROUP`] — with two:
//!
//! - [`RULE`]: inbound, allow, the program, *private* networks, every
//!   protocol: AirPlay (TCP 7000, its sound and timing over UDP), UwUCast
//!   (TCP 7100), mDNS (UDP 5353).
//! - [`MIRACAST_RULE`]: inbound, allow, the program, *public* networks, UDP
//!   only, and only from the local subnet: Miracast's video (RTP) over
//!   Wi-Fi Direct. The RTSP control connection needs no rule: the receiver
//!   connects out to the phone (port 7236), not the other way round.
//!
//! Checking needs no administrator: the firewall's COM interface reads every
//! rule, and unlike `netsh`'s output it isn't translated, so a German Windows
//! reads the same as an English one. The check doesn't insist on these two
//! rules: any enabled rule that lets the program in counts, and a block rule
//! for it counts against.
//!
//! Elsewhere than on Windows there is nothing to set up: [`status`] says it
//! isn't needed.

use std::path::Path;

use serde::Serialize;

#[cfg(windows)]
mod win;

/// The group both rules are in; what gets replaced and removed.
pub const GROUP: &str = "UwUMirror";
/// Private networks, every protocol.
pub const RULE: &str = "UwUMirror";
/// Public networks (Wi-Fi Direct), UDP, local subnet only.
pub const MIRACAST_RULE: &str = "UwUMirror (Miracast)";

/// Windows' numbers for profiles and protocols (`NET_FW_PROFILE2_*`,
/// `NET_FW_IP_PROTOCOL_*`), so that the check is the same on every system.
pub const PROFILE_PRIVATE: i32 = 2;
pub const PROFILE_PUBLIC: i32 = 4;
pub const PROTOCOL_TCP: i32 = 6;
pub const PROTOCOL_UDP: i32 = 17;
pub const PROTOCOL_ANY: i32 = 256;

/// One firewall rule, as far as the check cares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rule {
    pub name: String,
    pub group: String,
    /// The program's path; may hold `%VARIABLES%`, and its case varies.
    pub program: String,
    pub inbound: bool,
    pub allow: bool,
    pub enabled: bool,
    /// `PROFILE_*` bits.
    pub profiles: i32,
    pub protocol: i32,
    /// `*` (or empty) for every port.
    pub local_ports: String,
}

/// Whether phones get through the firewall to this UwUMirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// False where there's nothing to set up (not Windows).
    pub needed: bool,
    /// Private networks let the program in (TCP and UDP), or their firewall is off.
    pub private: bool,
    /// Public networks let Miracast's video (UDP) in, or their firewall is off.
    pub miracast: bool,
    /// UwUMirror's own two rules are there, enabled, for this program.
    pub ours: bool,
    /// Both of `private` and `miracast`.
    pub ready: bool,
    /// The firewall couldn't be read.
    pub error: Option<String>,
}

impl Status {
    pub fn not_needed() -> Self {
        Self {
            needed: false,
            private: true,
            miracast: true,
            ours: false,
            ready: true,
            error: None,
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn unreadable(error: String) -> Self {
        Self {
            needed: true,
            private: false,
            miracast: false,
            ours: false,
            ready: false,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The person said no to the administrator prompt.
    Declined,
    /// Not Windows.
    Unsupported,
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Declined => f.write_str("the administrator prompt was declined"),
            Error::Unsupported => f.write_str("only Windows has a firewall to set up"),
            Error::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

/// Reads the firewall (no administrator needed) for the program at `exe`.
pub fn status(exe: &Path) -> Status {
    #[cfg(windows)]
    {
        match win::read() {
            Ok(read) => evaluate(&read.rules, exe, read.private_on, read.public_on),
            Err(error) => Status::unreadable(error),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Status::not_needed()
    }
}

/// Any rule of UwUMirror's group, or any rule for the program at all: what
/// [`remove`] would take away. Without one, the uninstaller needn't ask.
pub fn has_rules(exe: &Path) -> bool {
    #[cfg(windows)]
    {
        win::read().is_ok_and(|read| {
            read.rules
                .iter()
                .any(|rule| rule.group == GROUP || same_program(&rule.program, exe, env))
        })
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        false
    }
}

/// Replaces UwUMirror's rules with the two for `exe`, asking once for an
/// administrator. Blocks until done. `parent` is the window (HWND) the prompt
/// belongs to.
pub fn set_up(exe: &Path, parent: Option<isize>) -> Result<(), Error> {
    #[cfg(windows)]
    {
        win::elevated_powershell(&set_up_script(exe), parent)
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, parent);
        Err(Error::Unsupported)
    }
}

/// Removes UwUMirror's rules, and the ones Windows made for `exe` when it
/// first asked, asking once for an administrator. For the uninstaller.
pub fn remove(exe: &Path, parent: Option<isize>) -> Result<(), Error> {
    #[cfg(windows)]
    {
        win::elevated_powershell(&remove_script(exe), parent)
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, parent);
        Err(Error::Unsupported)
    }
}

/// Looks up an environment variable; a seam for the tests.
#[cfg_attr(not(windows), allow(dead_code))]
fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// `%NAME%` replaced by its value where there is one, as Windows does for a
/// rule's program path.
fn expand(path: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => match lookup(&after[..end]).filter(|_| end > 0) {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A rule's program is `exe`: Windows' own rules have the path in lower case.
fn same_program(program: &str, exe: &Path, lookup: impl Fn(&str) -> Option<String>) -> bool {
    let program = expand(program.trim(), lookup).replace('/', "\\");
    let exe = exe.to_string_lossy().replace('/', "\\");
    !program.is_empty() && program.to_lowercase() == exe.to_lowercase()
}

/// What the rules mean for the program at `exe`, given which profiles have
/// the firewall on.
pub fn evaluate(rules: &[Rule], exe: &Path, private_on: bool, public_on: bool) -> Status {
    evaluate_with(rules, exe, private_on, public_on, env)
}

fn evaluate_with(
    rules: &[Rule],
    exe: &Path,
    private_on: bool,
    public_on: bool,
    lookup: impl Fn(&str) -> Option<String> + Copy,
) -> Status {
    let mine: Vec<&Rule> = rules
        .iter()
        .filter(|rule| rule.enabled && rule.inbound && same_program(&rule.program, exe, lookup))
        .collect();
    let covers = |rule: &Rule, profile: i32, protocol: i32| {
        rule.profiles & profile != 0 && (rule.protocol == PROTOCOL_ANY || rule.protocol == protocol)
    };
    let all_ports = |rule: &Rule| matches!(rule.local_ports.trim(), "" | "*");
    // A block rule wins over any allow rule.
    let open = |profile: i32, protocol: i32| {
        mine.iter()
            .any(|rule| rule.allow && all_ports(rule) && covers(rule, profile, protocol))
            && !mine
                .iter()
                .any(|rule| !rule.allow && covers(rule, profile, protocol))
    };
    let private =
        !private_on || (open(PROFILE_PRIVATE, PROTOCOL_TCP) && open(PROFILE_PRIVATE, PROTOCOL_UDP));
    let miracast = !public_on || open(PROFILE_PUBLIC, PROTOCOL_UDP);
    let ours = |name: &str, profile: i32, protocol: i32| {
        mine.iter().any(|rule| {
            rule.allow
                && rule.group == GROUP
                && rule.name == name
                && rule.profiles & profile != 0
                && rule.protocol == protocol
        })
    };
    Status {
        needed: true,
        private,
        miracast,
        ours: ours(RULE, PROFILE_PRIVATE, PROTOCOL_ANY)
            && ours(MIRACAST_RULE, PROFILE_PUBLIC, PROTOCOL_UDP),
        ready: private && miracast,
        error: None,
    }
}

/// `text` as a PowerShell string literal: in single quotes nothing is
/// expanded, and the one thing to escape is the quote itself — which
/// PowerShell also accepts in its typographic forms.
pub fn powershell_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Stops at the first error and says so by the exit code: the elevated
/// PowerShell has no output anyone could read.
fn script(body: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         $ProgressPreference = 'SilentlyContinue'\n\
         try {{\n{body}  exit 0\n}} catch {{\n  exit 1\n}}\n"
    )
}

/// Removes the group's rules, then adds the two for `exe`.
pub fn set_up_script(exe: &Path) -> String {
    let exe = powershell_literal(&exe.to_string_lossy());
    let group = powershell_literal(GROUP);
    script(&format!(
        "  $exe = {exe}\n\
         \x20 Get-NetFirewallRule -Group {group} -ErrorAction SilentlyContinue | Remove-NetFirewallRule\n\
         \x20 New-NetFirewallRule -DisplayName {rule} -Group {group} -Description {rule_text} \
         -Direction Inbound -Action Allow -Program $exe -Profile Private | Out-Null\n\
         \x20 New-NetFirewallRule -DisplayName {miracast} -Group {group} -Description {miracast_text} \
         -Direction Inbound -Action Allow -Program $exe -Profile Public -Protocol UDP \
         -RemoteAddress LocalSubnet | Out-Null\n",
        rule = powershell_literal(RULE),
        rule_text = powershell_literal(
            "Lets phones and computers mirror to UwUMirror on private networks: AirPlay, UwUCast, mDNS."
        ),
        miracast = powershell_literal(MIRACAST_RULE),
        miracast_text = powershell_literal(
            "Miracast's picture over Wi-Fi Direct, which Windows counts as a public network. UDP, from the local subnet only."
        ),
    ))
}

/// Removes the group's rules and every rule for `exe` (those Windows made
/// when it first asked, too).
pub fn remove_script(exe: &Path) -> String {
    let exe = powershell_literal(&exe.to_string_lossy());
    let group = powershell_literal(GROUP);
    script(&format!(
        "  $exe = {exe}\n\
         \x20 Get-NetFirewallRule -Group {group} -ErrorAction SilentlyContinue | Remove-NetFirewallRule\n\
         \x20 Get-NetFirewallApplicationFilter | Where-Object {{ $_.Program -eq $exe }} | \
         Get-NetFirewallRule | Remove-NetFirewallRule\n"
    ))
}

/// What `-EncodedCommand` takes: the script as UTF-16LE, in base64. Nothing in
/// it then needs quoting on the command line.
pub fn encoded_command(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The elevated PowerShell's command line.
pub fn powershell_parameters(script: &str) -> String {
    format!(
        "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand {}",
        encoded_command(script)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\Mini\AppData\Local\Programs\UwUMirror\UwUMirror.exe";

    fn lookup(name: &str) -> Option<String> {
        match name {
            "LOCALAPPDATA" => Some(r"C:\Users\Mini\AppData\Local".into()),
            _ => None,
        }
    }

    fn rule(name: &str, profiles: i32, protocol: i32) -> Rule {
        Rule {
            name: name.into(),
            program: EXE.into(),
            inbound: true,
            allow: true,
            enabled: true,
            profiles,
            protocol,
            local_ports: "*".into(),
            ..Rule::default()
        }
    }

    fn ours() -> Vec<Rule> {
        vec![
            Rule {
                group: GROUP.into(),
                ..rule(RULE, PROFILE_PRIVATE, PROTOCOL_ANY)
            },
            Rule {
                group: GROUP.into(),
                ..rule(MIRACAST_RULE, PROFILE_PUBLIC, PROTOCOL_UDP)
            },
        ]
    }

    fn check(rules: &[Rule]) -> Status {
        evaluate_with(rules, Path::new(EXE), true, true, lookup)
    }

    #[test]
    fn our_two_rules_are_enough() {
        let status = check(&ours());
        assert!(status.ready && status.ours && status.private && status.miracast);
    }

    #[test]
    fn what_windows_asks_on_the_first_start_leaves_miracast_out() {
        // What this PC had: the prompt's two rules, private only, the path in
        // lower case.
        let lower = EXE.to_lowercase();
        let rules = [
            Rule {
                program: lower.clone(),
                ..rule("uwumirror.exe", PROFILE_PRIVATE, PROTOCOL_UDP)
            },
            Rule {
                program: lower,
                ..rule("uwumirror.exe", PROFILE_PRIVATE, PROTOCOL_TCP)
            },
        ];
        let status = check(&rules);
        assert!(status.private);
        assert!(!status.miracast);
        assert!(!status.ready && !status.ours);

        // With a public UDP rule made by hand, Miracast works too.
        let mut rules = rules.to_vec();
        rules.push(rule("UwUMirror Miracast", PROFILE_PUBLIC, PROTOCOL_UDP));
        let status = check(&rules);
        assert!(status.ready);
        assert!(!status.ours, "not our rules, but enough");
    }

    #[test]
    fn rules_that_dont_count() {
        let mut disabled = ours();
        disabled[1].enabled = false;
        assert!(!check(&disabled).miracast);

        let mut other_program = ours();
        other_program[1].program = r"C:\Old\UwUMirror.exe".into();
        assert!(!check(&other_program).miracast);

        let mut outbound = ours();
        outbound[1].inbound = false;
        assert!(!check(&outbound).miracast);

        let mut one_port = ours();
        one_port[0].local_ports = "7000".into();
        assert!(!check(&one_port).private);

        let mut tcp_only = ours();
        tcp_only[1].protocol = PROTOCOL_TCP;
        assert!(!check(&tcp_only).miracast);
    }

    #[test]
    fn a_block_rule_wins() {
        let mut rules = ours();
        rules.push(Rule {
            allow: false,
            ..rule("uwumirror.exe", PROFILE_PUBLIC, PROTOCOL_UDP)
        });
        let status = check(&rules);
        assert!(!status.miracast && !status.ready);
        assert!(status.ours, "the rules are there, but blocked");
    }

    #[test]
    fn a_profile_without_firewall_needs_no_rule() {
        let status = evaluate_with(&[], Path::new(EXE), false, false, lookup);
        assert!(status.ready && !status.ours);
        let status = evaluate_with(&[], Path::new(EXE), true, false, lookup);
        assert!(!status.private && status.miracast);
    }

    #[test]
    fn program_paths_compare_like_windows() {
        let exe = Path::new(EXE);
        assert!(same_program(&EXE.to_uppercase(), exe, lookup));
        assert!(same_program(
            r"%LOCALAPPDATA%\Programs\UwUMirror\UwUMirror.exe",
            exe,
            lookup
        ));
        assert!(!same_program(r"%NOPE%\UwUMirror.exe", exe, lookup));
        assert!(!same_program("", exe, lookup));
        assert_eq!(expand("100%-sure%", lookup), "100%-sure%");
        assert_eq!(expand("%%", lookup), "%%");
    }

    #[test]
    fn literals_cant_break_out() {
        assert_eq!(powershell_literal("plain"), "'plain'");
        assert_eq!(powershell_literal("it's"), "'it''s'");
        assert_eq!(powershell_literal("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        // $, backticks, %, ; and double quotes mean nothing in single quotes.
        assert_eq!(
            powershell_literal(r#"C:\$x `y %z%; "q""#),
            r#"'C:\$x `y %z%; "q"'"#
        );
    }

    #[test]
    fn the_set_up_script() {
        let exe = Path::new(r"C:\Users\O'Brien\Apps\UwUMirror\UwUMirror.exe");
        let script = set_up_script(exe);
        assert!(script.contains(r"$exe = 'C:\Users\O''Brien\Apps\UwUMirror\UwUMirror.exe'"));
        assert!(script.contains(
            "Get-NetFirewallRule -Group 'UwUMirror' -ErrorAction SilentlyContinue | Remove-NetFirewallRule"
        ));
        assert!(script.contains(
            "New-NetFirewallRule -DisplayName 'UwUMirror' -Group 'UwUMirror' -Description "
        ));
        assert!(script.contains(
            "-Direction Inbound -Action Allow -Program $exe -Profile Private | Out-Null"
        ));
        assert!(
            script.contains("-DisplayName 'UwUMirror (Miracast)' -Group 'UwUMirror' -Description ")
        );
        assert!(script.contains(
            "-Direction Inbound -Action Allow -Program $exe -Profile Public -Protocol UDP -RemoteAddress LocalSubnet | Out-Null"
        ));
        assert!(script.starts_with("$ErrorActionPreference = 'Stop'\n"));
        assert!(script.ends_with("  exit 0\n} catch {\n  exit 1\n}\n"));
        // The program's path appears once, as the literal.
        assert_eq!(script.matches("O''Brien").count(), 1);
    }

    #[test]
    fn the_remove_script() {
        let script = remove_script(Path::new(EXE));
        assert!(script.contains(&format!("$exe = '{EXE}'")));
        assert!(script.contains("Get-NetFirewallRule -Group 'UwUMirror'"));
        assert!(script.contains(
            "Get-NetFirewallApplicationFilter | Where-Object { $_.Program -eq $exe } | Get-NetFirewallRule | Remove-NetFirewallRule"
        ));
    }

    #[test]
    fn encodes_like_powershell_expects() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes(...))
        assert_eq!(encoded_command("exit 0"), "ZQB4AGkAdAAgADAA");
        assert_eq!(encoded_command("a"), "YQA=");
        assert_eq!(encoded_command("ab"), "YQBiAA==");
        assert_eq!(encoded_command("ä€"), "5ACsIA==");
        let parameters = powershell_parameters("exit 0");
        assert!(parameters.ends_with("-EncodedCommand ZQB4AGkAdAAgADAA"));
        assert!(parameters.contains("-NoProfile"));
    }
}
