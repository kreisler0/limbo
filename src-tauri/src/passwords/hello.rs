//! Re-authentication before revealing or exporting passwords: Windows Hello
//! (face, fingerprint, PIN), falling back to the Windows account password.

use windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::Credentials::{
    CRED_PACK_FLAGS, CREDUI_INFOW, CREDUIWIN_ENUMERATE_CURRENT_USER, CredUIPromptForWindowsCredentialsW,
    CredUnPackAuthenticationBufferW,
};
use windows::Win32::Security::{LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
use windows_core::{HSTRING, PCWSTR, PWSTR};
use windows_future::IAsyncOperation;
use zeroize::Zeroize;

/// Blocks until the user verifies (or cancels). Call from a worker thread.
pub fn verify_user(hwnd: isize, message: &str) -> bool {
    let hwnd = HWND(hwnd as *mut _);
    let available = UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|op| op.join())
        .map(|a| a == UserConsentVerifierAvailability::Available)
        .unwrap_or(false);
    if available {
        let result = (|| -> windows_core::Result<UserConsentVerificationResult> {
            let interop = windows_core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()?;
            let op: IAsyncOperation<UserConsentVerificationResult> =
                unsafe { interop.RequestVerificationForWindowAsync(hwnd, &HSTRING::from(message))? };
            op.join()
        })();
        match result {
            Ok(r) => return r == UserConsentVerificationResult::Verified,
            Err(e) => log::warn!("Windows Hello failed, falling back to the account password: {e}"),
        }
    }
    windows_password_prompt(hwnd, message)
}

/// The standard Windows credential dialog, validated with `LogonUserW`.
fn windows_password_prompt(hwnd: HWND, message: &str) -> bool {
    let caption = HSTRING::from("Limbo");
    let text = HSTRING::from(message);
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        hwndParent: hwnd,
        pszMessageText: PCWSTR(text.as_ptr()),
        pszCaptionText: PCWSTR(caption.as_ptr()),
        ..Default::default()
    };
    let mut package = 0u32;
    let mut out_buf: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut out_size = 0u32;
    let mut save = windows_core::BOOL(0);
    let rc = unsafe {
        CredUIPromptForWindowsCredentialsW(
            Some(&info),
            0,
            &mut package,
            None,
            0,
            &mut out_buf,
            &mut out_size,
            Some(&mut save),
            CREDUIWIN_ENUMERATE_CURRENT_USER,
        )
    };
    if rc != 0 || out_buf.is_null() {
        return false;
    }
    let mut user = vec![0u16; 514];
    let mut domain = vec![0u16; 338];
    let mut pass = vec![0u16; 514];
    let (mut ul, mut dl, mut pl) = (user.len() as u32, domain.len() as u32, pass.len() as u32);
    let unpacked = unsafe {
        CredUnPackAuthenticationBufferW(
            CRED_PACK_FLAGS(0),
            out_buf,
            out_size,
            Some(PWSTR(user.as_mut_ptr())),
            &mut ul,
            Some(PWSTR(domain.as_mut_ptr())),
            Some(&mut dl),
            Some(PWSTR(pass.as_mut_ptr())),
            &mut pl,
        )
    };
    unsafe {
        std::ptr::write_bytes(out_buf as *mut u8, 0, out_size as usize);
        CoTaskMemFree(Some(out_buf));
    }
    let mut ok = false;
    if unpacked.is_ok() {
        // "DOMAIN\user" or "user@domain" come back in the user field.
        let mut token = HANDLE::default();
        let domain_ptr = if dl > 1 { PCWSTR(domain.as_ptr()) } else { PCWSTR::null() };
        ok = unsafe {
            LogonUserW(
                PCWSTR(user.as_ptr()),
                domain_ptr,
                PCWSTR(pass.as_ptr()),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
        }
        .is_ok();
        if ok {
            unsafe {
                let _ = CloseHandle(token);
            }
        }
    }
    pass.zeroize();
    user.zeroize();
    ok
}
