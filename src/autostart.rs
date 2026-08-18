//! ログオン時の自動起動登録。HKEY_CURRENT_USER 配下なので管理者権限は不要。
//!
//! `--install` は自分自身を %LOCALAPPDATA% 配下へコピーし、そのコピーを Run に登録する。
//! 実行された場所をそのまま登録すると、ダウンロードフォルダから起動された場合に
//! 後で exe を移動・削除しただけで自動起動が無言で壊れるため。

use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ,
};

use crate::proc::{self, wide};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "gamesplayassist-killer";
/// %LOCALAPPDATA% 配下の配置先。利用者が普段触るパスから独立させるのが目的。
const INSTALL_DIR: &str = "gamesplayassist-killer";
const INSTALL_EXE: &str = "gamesplayassist-killer.exe";
/// 直前に終了させたプロセスのイメージが解放されるまでの待ち。
const FILE_RETRIES: u32 = 10;
const FILE_RETRY_INTERVAL: Duration = Duration::from_millis(100);

pub fn install() -> String {
    let Ok(source) = std::env::current_exe() else {
        return "実行ファイルのパスを取得できませんでした。".to_owned();
    };
    let Some(target) = install_path() else {
        return "%LOCALAPPDATA% の場所を取得できませんでした。".to_owned();
    };

    // 先に旧インスタンスを止める。実行中の exe は上書きできないうえ、
    // 生きたままだとミューテックスに弾かれて新しい方が黙って終了してしまう。
    proc::terminate_others(INSTALL_EXE);

    if !same_file(&source, &target) {
        if let Err(err) = copy_into_place(&source, &target) {
            return format!("配置に失敗しました。\n\n{}\n{err}", target.display());
        }
    }

    if register(&target) != Some(true) {
        return "自動起動の登録に失敗しました。".to_owned();
    }

    if start(&target) {
        format!(
            "自動起動を登録し、常駐を開始しました。\n\
             再サインインは不要です。次回ログオン以降も自動で起動します。\n\n{}",
            target.display()
        )
    } else {
        format!(
            "自動起動を登録しました。次回ログオンから常駐します。\n\
             ただし今すぐの常駐開始には失敗しました。\n\n{}",
            target.display()
        )
    }
}

/// 配置したコピーを常駐モードで起動する。作業ディレクトリは配置先に固定する。
fn start(target: &Path) -> bool {
    let (Some(exe), Some(directory)) = (target.to_str(), target.parent().and_then(Path::to_str))
    else {
        return false;
    };
    proc::spawn_detached(exe, directory)
}

pub fn uninstall() -> String {
    let name = wide(VALUE_NAME);
    let removed =
        with_run_key(|key| unsafe { RegDeleteValueW(key, name.as_ptr()) == ERROR_SUCCESS });

    proc::terminate_others(INSTALL_EXE);

    let mut message = match removed {
        Some(true) => "自動起動の登録を解除し、常駐を停止しました。".to_owned(),
        _ => "自動起動の登録は見つかりませんでした。".to_owned(),
    };

    if let Some(target) = install_path() {
        if target.exists() {
            message.push_str(&cleanup(&target));
        }
    }

    message
}

/// 配置したコピーを片付ける。自分自身だった場合は実行中なので消せない。
fn cleanup(target: &Path) -> String {
    let running_from_target =
        std::env::current_exe().is_ok_and(|current| same_file(&current, target));

    if running_from_target {
        let dir = target.parent().unwrap_or(target);
        return format!(
            "\n\nこの exe 自体は実行中のため削除できません。\n\
             このダイアログを閉じたあと、次のフォルダを削除してください。\n\n{}",
            dir.display()
        );
    }

    match remove_installed(target) {
        Ok(()) => String::new(),
        Err(err) => format!(
            "\n\n配置したファイルの削除に失敗しました。\n\n{}\n{err}",
            target.display()
        ),
    }
}

fn install_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(Path::new(&base).join(INSTALL_DIR).join(INSTALL_EXE))
}

fn register(target: &Path) -> Option<bool> {
    let data = wide(&format!("\"{}\"", target.display()));
    let name = wide(VALUE_NAME);

    with_run_key(|key| unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast::<u8>(),
            (data.len() * 2) as u32,
        ) == ERROR_SUCCESS
    })
}

fn copy_into_place(source: &Path, target: &Path) -> std::io::Result<()> {
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    retry(|| std::fs::copy(source, target).map(drop))
}

fn remove_installed(target: &Path) -> std::io::Result<()> {
    retry(|| std::fs::remove_file(target))?;
    if let Some(dir) = target.parent() {
        // 利用者が何か置いていれば空にならない。その場合はフォルダを残す。
        let _ = std::fs::remove_dir(dir);
    }
    Ok(())
}

/// 終了直後のプロセスがファイルを掴んだままのことがあるので数回試す。
fn retry(mut action: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..FILE_RETRIES {
        match action() {
            Ok(()) => return Ok(()),
            Err(err) => last = Some(err),
        }
        if attempt + 1 < FILE_RETRIES {
            sleep(FILE_RETRY_INTERVAL);
        }
    }
    Err(last.expect("ループは必ず一度は回る"))
}

/// 同じ実体を指しているか。大文字小文字や相対表記の違いを吸収するため正規化して比べる。
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_path_sits_under_local_appdata() {
        let path = install_path().expect("LOCALAPPDATA が設定されていること");
        assert!(path.is_absolute(), "{}", path.display());
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(INSTALL_EXE)
        );
        assert_eq!(
            path.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str()),
            Some(INSTALL_DIR)
        );
    }

    /// 表記が違うだけの同一ファイルを別物と誤判定すると、毎回コピーし直してしまう。
    #[test]
    fn same_file_looks_through_path_spelling() {
        let exe = std::env::current_exe().expect("自プロセスの exe パス");
        let dir = exe.parent().expect("親ディレクトリ");
        let name = exe.file_name().expect("ファイル名");

        assert!(same_file(&exe, &dir.join(".").join(name)));
        assert!(!same_file(&exe, &dir.join("does-not-exist.exe")));
    }
}
