//! DMM GAME PLAYER の GamesPlayAssist.exe を落とし続ける常駐ツール。
//!
//! GamesPlayAssist はスクリーンショット/オーバーレイ用のショートカットを
//! グローバルホットキーとして登録する。登録されたキーはシステム全体で横取りされ、
//! ゲーム本体には届かなくなる。設定画面ではキーの変更しかできず、無効化できない。
//! 他プロセスが登録したホットキーを外部から解除する API も存在しないため、
//! プロセスを終了させるのが唯一の解決手段になる。

#![cfg_attr(not(test), windows_subsystem = "windows")]

mod autostart;
mod proc;

#[cfg(not(target_pointer_width = "64"))]
compile_error!("64bit 版 Windows 専用です (対象プロセスの PEB を x64 のレイアウトで読むため)");

use std::collections::HashMap;
use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

/// 監視対象。DMM GAME PLAYER がゲーム起動時に生成する常駐プロセス。
const ASSIST_EXE: &str = "GamesPlayAssist.exe";
/// GamesPlayAssist の起動引数。ゲーム本体の実行ファイルパスが入っている。
const GAME_EXE_FLAG: &str = "--game-exe-file-path";
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

const USAGE: &str = "\
gamesplayassist-killer

引数なし      常駐して GamesPlayAssist を監視・終了させる
--install     %LOCALAPPDATA% へ自分をコピーして自動起動を登録し、常駐を開始する
--uninstall   自動起動を解除し、常駐を停止してコピーを削除する
--force       ゲーム本体の起動を待たず、見つけ次第すぐ終了させる
--help        このヘルプを表示する";

fn main() {
    let message = match std::env::args().nth(1).as_deref() {
        None => return watch(false),
        Some("--force") => return watch(true),
        Some("--install") => autostart::install(),
        Some("--uninstall") => autostart::uninstall(),
        Some("--help") | Some("-h") => USAGE.to_owned(),
        Some(other) => format!("不明な引数です: {other}\n\n{USAGE}"),
    };
    show(&message);
}

/// 常駐ループ。
///
/// 既定では GamesPlayAssist のコマンドラインから割り出したゲーム本体が
/// 実際に起動しているときだけ終了させる。GamesPlayAssist がゲームの起動を
/// 仲介している可能性を排除できないため、起動シーケンスを壊さないための保険。
/// ゲーム終了後は GamesPlayAssist が自分で終了するので、こちらは何もしない。
fn watch(force: bool) {
    if !proc::claim_single_instance() {
        return;
    }

    // pid ごとのゲーム本体 exe 名。読み取れなかった場合は None を憶えて再試行しない。
    let mut game_of: HashMap<u32, Option<String>> = HashMap::new();

    loop {
        let processes = proc::snapshot();
        game_of.retain(|pid, _| processes.iter().any(|entry| entry.pid == *pid));

        for assist in processes
            .iter()
            .filter(|entry| same(&entry.name, ASSIST_EXE))
        {
            let game = game_of
                .entry(assist.pid)
                .or_insert_with(|| game_exe_name(assist.pid));

            let game_running = game
                .as_deref()
                .is_some_and(|name| processes.iter().any(|entry| same(&entry.name, name)));

            if force || game_running {
                proc::terminate(assist.pid);
            }
        }

        sleep(POLL_INTERVAL);
    }
}

fn game_exe_name(pid: u32) -> Option<String> {
    game_exe_from_command_line(&proc::command_line(pid)?)
}

/// GamesPlayAssist のコマンドラインからゲーム本体の exe 名を取り出す。
///
/// 例: `... "--game-exe-file-path" "D:\Games\WarThunder\win64\aces.exe" ...` から `aces.exe`。
/// タイトル固有の値をハードコードしないので、DMM のどのゲームでも動く。
fn game_exe_from_command_line(line: &str) -> Option<String> {
    let args = split_args(line);
    let inline_prefix = format!("{GAME_EXE_FLAG}=");

    let path = args.iter().enumerate().find_map(|(index, arg)| {
        if arg == GAME_EXE_FLAG {
            args.get(index + 1).cloned()
        } else {
            arg.strip_prefix(&inline_prefix).map(str::to_owned)
        }
    })?;

    let name = path.rsplit(['\\', '/']).next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// コマンドライン文字列を引数に分解する。
///
/// GamesPlayAssist は全トークンを素直に `"` で囲むだけなので、
/// バックスラッシュのエスケープ規則までは実装しない。
fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;

    for ch in line.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if has_token {
                    args.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            ch => {
                current.push(ch);
                has_token = true;
            }
        }
    }
    if has_token {
        args.push(current);
    }
    args
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// コンソールを持たないので、利用者への通知はダイアログで行う。
fn show(text: &str) {
    let body = proc::wide(text);
    let title = proc::wide("gamesplayassist-killer");
    unsafe {
        MessageBoxW(
            null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実際に観測した GamesPlayAssist の起動引数 (パスは一般化してある)。
    const REAL: &str = r#""C:\Program Files\DMMGamePlayer\resources\GamesPlayAssist.exe" "--window-name" "War Thunder" "--startup-exe-file-path" "D:\Games\WarThunder\launcher.exe" "--product-id" "wt" "--game-title" "War Thunder" "--game-exe-file-path" "D:\Games\WarThunder\win64\aces.exe" "--app-version" "5.5.16""#;

    #[test]
    fn splits_quoted_arguments_containing_spaces() {
        let args = split_args(REAL);
        assert_eq!(
            args[0],
            r"C:\Program Files\DMMGamePlayer\resources\GamesPlayAssist.exe"
        );
        assert_eq!(args[1], "--window-name");
        assert_eq!(args[2], "War Thunder");
    }

    #[test]
    fn finds_game_executable_name() {
        assert_eq!(
            game_exe_from_command_line(REAL).as_deref(),
            Some("aces.exe")
        );
    }

    #[test]
    fn accepts_inline_form() {
        let line = r#""assist.exe" "--game-exe-file-path=D:\game\bin\game.exe""#;
        assert_eq!(
            game_exe_from_command_line(line).as_deref(),
            Some("game.exe")
        );
    }

    #[test]
    fn returns_none_when_flag_is_absent() {
        assert_eq!(
            game_exe_from_command_line(r#""assist.exe" "--product-id" "wt""#),
            None
        );
    }

    #[test]
    fn ignores_startup_exe_path() {
        // --startup-exe-file-path (ランチャー) を誤って拾わないこと。
        assert_ne!(
            game_exe_from_command_line(REAL).as_deref(),
            Some("launcher.exe")
        );
    }
}
