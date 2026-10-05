//! The Windows side: reading the rules through the firewall's COM interface,
//! and the one elevated PowerShell that changes them.

use std::path::PathBuf;
use std::time::Duration;

use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HWND, WAIT_OBJECT_0};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, NetFwPolicy2, NET_FW_ACTION_ALLOW, NET_FW_PROFILE2_PRIVATE,
    NET_FW_PROFILE2_PUBLIC, NET_FW_RULE_DIR_IN,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Ole::IEnumVARIANT;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::System::Variant::{VARIANT, VT_DISPATCH};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::{powershell_parameters, Error, Rule};

/// How long the elevated PowerShell may take once the prompt is answered.
/// Removing walks every rule's program filter, which takes a few seconds on a
/// PC with many rules.
const ELEVATED_TIMEOUT: Duration = Duration::from_secs(120);

/// COM for this thread, for as long as it lives. A thread that has COM in
/// another mode already keeps it, and that works just as well.
struct Com(bool);

impl Com {
    fn init() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) struct Read {
    pub rules: Vec<Rule>,
    pub private_on: bool,
    pub public_on: bool,
}

/// Every rule, and which profiles have the firewall on. Reading needs no
/// administrator.
pub(crate) fn read() -> Result<Read, String> {
    let _com = Com::init();
    let fail = |what: &str, e: windows::core::Error| format!("Windows Firewall ({what}): {e}");
    unsafe {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| fail("open", e))?;
        let private_on = policy
            .get_FirewallEnabled(NET_FW_PROFILE2_PRIVATE)
            .map_err(|e| fail("private", e))?
            .as_bool();
        let public_on = policy
            .get_FirewallEnabled(NET_FW_PROFILE2_PUBLIC)
            .map_err(|e| fail("public", e))?
            .as_bool();
        let rules = policy.Rules().map_err(|e| fail("rules", e))?;
        let all: IEnumVARIANT = rules
            ._NewEnum()
            .and_then(|unknown| unknown.cast())
            .map_err(|e| fail("rules", e))?;
        let mut found = Vec::new();
        loop {
            let mut item = [VARIANT::default()];
            let mut fetched = 0u32;
            if all.Next(&mut item, &mut fetched).is_err() || fetched == 0 {
                break;
            }
            let value = &item[0].Anonymous.Anonymous;
            if value.vt != VT_DISPATCH {
                continue;
            }
            let Some(rule) = (*value.Anonymous.pdispVal)
                .as_ref()
                .and_then(|dispatch| dispatch.cast::<INetFwRule>().ok())
            else {
                continue;
            };
            found.push(snapshot(&rule));
        }
        Ok(Read {
            rules: found,
            private_on,
            public_on,
        })
    }
}

fn snapshot(rule: &INetFwRule) -> Rule {
    let text = |value: windows::core::Result<windows::core::BSTR>| {
        value.map(|text| text.to_string()).unwrap_or_default()
    };
    unsafe {
        Rule {
            name: text(rule.Name()),
            group: text(rule.Grouping()),
            program: text(rule.ApplicationName()),
            inbound: rule.Direction().is_ok_and(|d| d == NET_FW_RULE_DIR_IN),
            allow: rule.Action().is_ok_and(|a| a == NET_FW_ACTION_ALLOW),
            enabled: rule.Enabled().is_ok_and(|e| e.as_bool()),
            profiles: rule.Profiles().unwrap_or(0),
            protocol: rule.Protocol().unwrap_or(0),
            local_ports: text(rule.LocalPorts()),
        }
    }
}

/// Windows PowerShell by its full path: never one found in the current folder
/// or on PATH.
fn powershell() -> Result<PathBuf, Error> {
    let mut buffer = [0u16; 260];
    let length = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if length == 0 || length > buffer.len() {
        return Err(Error::Failed("couldn't find Windows' system folder".into()));
    }
    let system = PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
    let path = system.join(r"WindowsPowerShell\v1.0\powershell.exe");
    if path.exists() {
        Ok(path)
    } else {
        Err(Error::Failed(format!("{} is missing", path.display())))
    }
}

/// Runs `script` in an elevated, hidden PowerShell — the UAC prompt — and
/// waits for it. Its exit code says whether it worked.
pub(crate) fn elevated_powershell(script: &str, parent: Option<isize>) -> Result<(), Error> {
    let _com = Com::init();
    let file = HSTRING::from(powershell()?.as_os_str());
    let parameters = HSTRING::from(powershell_parameters(script));
    let verb = HSTRING::from("runas");
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        hwnd: HWND(parent.unwrap_or(0) as *mut _),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    if let Err(error) = unsafe { ShellExecuteExW(&mut info) } {
        return Err(if error.code() == ERROR_CANCELLED.to_hresult() {
            Error::Declined
        } else {
            Error::Failed(format!(
                "couldn't start PowerShell as administrator: {error}"
            ))
        });
    }
    if info.hProcess.is_invalid() {
        return Err(Error::Failed("PowerShell didn't start".into()));
    }
    let process = info.hProcess;
    let waited = unsafe { WaitForSingleObject(process, ELEVATED_TIMEOUT.as_millis() as u32) };
    let mut code = 1u32;
    let read = unsafe { GetExitCodeProcess(process, &mut code) };
    unsafe {
        let _ = CloseHandle(process);
    }
    if waited != WAIT_OBJECT_0 {
        return Err(Error::Failed(
            "PowerShell took too long to change the firewall".into(),
        ));
    }
    match (read, code) {
        (Ok(()), 0) => Ok(()),
        (Ok(()), code) => Err(Error::Failed(format!(
            "PowerShell couldn't change the firewall (exit code {code})"
        ))),
        (Err(error), _) => Err(Error::Failed(error.to_string())),
    }
}
