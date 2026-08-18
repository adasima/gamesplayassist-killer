//! Win32 のプロセス操作。すべて非昇格・同一ユーザーの権限内で完結する。

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, CreateProcessW, OpenProcess, TerminateProcess, PROCESS_INFORMATION,
    PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ, STARTUPINFOW,
};

/// x64 の PEB におけるプロセスパラメータへのポインタの位置。
const PEB_PROCESS_PARAMETERS: usize = 0x20;
/// x64 の RTL_USER_PROCESS_PARAMETERS における CommandLine (UNICODE_STRING) の位置。
const PARAMETERS_COMMAND_LINE: usize = 0x70;

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        handle: HANDLE,
        class: u32,
        info: *mut c_void,
        len: u32,
        returned: *mut u32,
    ) -> i32;
}

pub struct ProcEntry {
    pub pid: u32,
    pub name: String,
}

/// 閉じ忘れを防ぐためのハンドルラッパ。
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// UTF-8 の文字列を、終端 NUL 付きの UTF-16 バッファに変換する。
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 実行中プロセスを 1 回のスナップショットで列挙する。
pub fn snapshot() -> Vec<ProcEntry> {
    let mut entries = Vec::new();

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return entries;
        }
        let snapshot = OwnedHandle(snapshot);

        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot.0, &mut entry) != 0 {
            loop {
                entries.push(ProcEntry {
                    pid: entry.th32ProcessID,
                    name: from_wide(&entry.szExeFile),
                });
                if Process32NextW(snapshot.0, &mut entry) == 0 {
                    break;
                }
            }
        }
    }

    entries
}

/// 対象プロセスの PEB を辿ってコマンドライン全体を読む。
///
/// 同一ユーザー・同一整合性レベルのプロセスであれば昇格なしで成功する。
/// GamesPlayAssist は DMM GAME PLAYER が非昇格で起動するため、この条件を満たす。
pub fn command_line(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return None;
        }
        let handle = OwnedHandle(handle);

        // PROCESS_BASIC_INFORMATION。x64 では PebBaseAddress がオフセット 8 に来る。
        let mut basic_info = [0u8; 48];
        let mut returned = 0u32;
        let status = NtQueryInformationProcess(
            handle.0,
            0,
            basic_info.as_mut_ptr().cast(),
            basic_info.len() as u32,
            &mut returned,
        );
        if status < 0 {
            return None;
        }

        let peb = usize::from_ne_bytes(basic_info[8..16].try_into().ok()?);
        let parameters = read_usize(handle.0, peb + PEB_PROCESS_PARAMETERS)?;

        // UNICODE_STRING { Length: u16, MaximumLength: u16, Buffer: *mut u16 }
        let mut unicode_string = [0u8; 16];
        read_bytes(
            handle.0,
            parameters + PARAMETERS_COMMAND_LINE,
            &mut unicode_string,
        )?;
        let byte_len = u16::from_ne_bytes(unicode_string[0..2].try_into().ok()?) as usize;
        let buffer = usize::from_ne_bytes(unicode_string[8..16].try_into().ok()?);
        if byte_len == 0 || buffer == 0 {
            return None;
        }

        let mut raw = vec![0u8; byte_len];
        read_bytes(handle.0, buffer, &mut raw)?;
        let utf16: Vec<u16> = raw
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect();
        Some(String::from_utf16_lossy(&utf16))
    }
}

/// プロセスを終了させる。成功したら true。
pub fn terminate(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let handle = OwnedHandle(handle);
        TerminateProcess(handle.0, 1) != 0
    }
}

/// 実行ファイルを起動し、親の終了に巻き込まれないよう切り離す。
///
/// `std::process::Command` ではなく生の CreateProcessW を使うのは、std のプロセス機構を
/// 引き込むとバイナリが倍近くに膨らむため。引数は渡さないので子は常駐モードで動く。
/// 作業ディレクトリを配置先に固定するのは、呼び出し元のフォルダ (多くはダウンロード
/// フォルダ) を掴んだままにして削除できなくするのを避けるため。
pub fn spawn_detached(exe: &str, directory: &str) -> bool {
    unsafe {
        let application = wide(exe);
        let directory = wide(directory);
        let mut startup: STARTUPINFOW = zeroed();
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        let mut info: PROCESS_INFORMATION = zeroed();

        let started = CreateProcessW(
            application.as_ptr(),
            null_mut(),
            null(),
            null(),
            0,
            0,
            null(),
            directory.as_ptr(),
            &startup,
            &mut info,
        );
        if started == 0 {
            return false;
        }
        // 子は独立して動き続ける。こちらはハンドルを手放すだけ。
        CloseHandle(info.hProcess);
        CloseHandle(info.hThread);
        true
    }
}

/// 同名の他インスタンスをすべて終了させる。自分自身は対象にしない。
///
/// `--install` が古い常駐を置き換えるために使う。実行中の exe は上書きできず、
/// 生かしたままだとミューテックスに弾かれて新しい方が起動できない。
pub fn terminate_others(exe_name: &str) {
    let self_pid = std::process::id();
    for entry in snapshot() {
        if entry.pid != self_pid && entry.name.eq_ignore_ascii_case(exe_name) {
            terminate(entry.pid);
        }
    }
}

/// 二重常駐を防ぐ。既に別インスタンスが動いていれば false を返す。
pub fn claim_single_instance() -> bool {
    unsafe {
        let name = wide("gamesplayassist-killer.single-instance");
        let handle = CreateMutexW(null(), 1, name.as_ptr());
        if handle.is_null() {
            return false;
        }
        // ハンドルは意図的に閉じない。プロセス終了時に OS が解放する。
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

unsafe fn read_bytes(handle: HANDLE, address: usize, out: &mut [u8]) -> Option<()> {
    let mut read = 0usize;
    let ok = ReadProcessMemory(
        handle,
        address as *const c_void,
        out.as_mut_ptr().cast(),
        out.len(),
        &mut read,
    );
    (ok != 0 && read == out.len()).then_some(())
}

unsafe fn read_usize(handle: HANDLE, address: usize) -> Option<usize> {
    let mut buf = [0u8; 8];
    read_bytes(handle, address, &mut buf)?;
    Some(usize::from_ne_bytes(buf))
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_contains_the_current_process() {
        let pid = std::process::id();
        assert!(snapshot().iter().any(|entry| entry.pid == pid));
    }

    /// PEB 経由のコマンドライン読み取りが実際に動くことを自プロセスで確かめる。
    /// 他プロセスを読む場合との違いはハンドルの取得元だけ。
    #[test]
    fn reads_a_command_line_through_the_peb() {
        let line =
            command_line(std::process::id()).expect("自プロセスのコマンドラインを読めること");
        assert!(line.to_ascii_lowercase().contains(".exe"), "{line}");
    }
}
