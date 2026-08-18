//! ログオン時の自動起動登録。HKEY_CURRENT_USER 配下なので管理者権限は不要。

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ,
};

use crate::proc::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "gamesplayassist-killer";

pub fn install() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return "実行ファイルのパスを取得できませんでした。".to_owned();
    };

    let data = wide(&format!("\"{}\"", exe.display()));
    let name = wide(VALUE_NAME);

    let written = with_run_key(|key| unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast::<u8>(),
            (data.len() * 2) as u32,
        ) == ERROR_SUCCESS
    });

    match written {
        Some(true) => format!(
            "自動起動を登録しました。\n次回ログオンから常駐します。\n\n{}",
            exe.display()
        ),
        _ => "自動起動の登録に失敗しました。".to_owned(),
    }
}

pub fn uninstall() -> String {
    let name = wide(VALUE_NAME);

    let deleted =
        with_run_key(|key| unsafe { RegDeleteValueW(key, name.as_ptr()) == ERROR_SUCCESS });

    match deleted {
        Some(true) => "自動起動の登録を解除しました。".to_owned(),
        _ => "自動起動の登録は見つかりませんでした。".to_owned(),
    }
}

/// Run キーは常に存在するので作成はせず開くだけにする。
fn with_run_key<T>(action: impl FnOnce(HKEY) -> T) -> Option<T> {
    unsafe {
        let mut key: HKEY = null_mut();
        let subkey = wide(RUN_KEY);
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let result = action(key);
        RegCloseKey(key);
        Some(result)
    }
}
