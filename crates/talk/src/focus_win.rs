//! Qual janela do T3 está em foco no Windows: o mesmo `watch` do `focus.rs`
//! (que no Linux pergunta ao COSMIC), aqui olhando a janela da frente a cada
//! 300 ms. O Windows não avisa quando o foco muda sem um gancho global; a
//! consulta é barata.
//!
//! O título da janela do T3 é "Projeto / Conversa", como no Linux. O T3 é
//! reconhecido pelo executável (`T3 Code (Beta).exe`, ou sem o sufixo), não
//! pelo título.
//! "Largado" = 30 s sem teclado nem mouse (`GetLastInputInfo`).

#![allow(unsafe_code)]

use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

pub const IDLE_MS: u32 = 30_000;

pub enum Seen<'a> {
    Focus(Option<&'a str>),
    Idle(bool),
}

pub type OnChange = Box<dyn Fn(Seen) + Send>;

/// O nome do executável do T3 Code, sem diferenciar maiúsculas.
fn is_t3(exe: &str) -> bool {
    let name = Path::new(exe).file_stem().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
    ["t3 code", "t3code", "t3-code"].iter().any(|p| name.starts_with(p))
}

fn exe_of(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: `hwnd` veio do GetForegroundWindow; o pid é só escrito.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }
    // SAFETY: abrir um processo só para consulta; o handle é fechado abaixo.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` tem `len` posições; o Windows escreve no máximo isso.
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    // SAFETY: o handle é nosso e válido.
    unsafe { CloseHandle(process) };
    (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    // SAFETY: `buf` tem 512 posições; o Windows escreve no máximo 511 + o zero.
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// O título da janela do T3 na frente, se for o T3.
fn focused_t3() -> Option<String> {
    // SAFETY: sem argumentos; pode voltar nulo (nenhuma janela na frente).
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    exe_of(hwnd).filter(|e| is_t3(e)).map(|_| title_of(hwnd))
}

fn idle_ms() -> u32 {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` é do tamanho que o `cbSize` diz.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0;
    }
    // SAFETY: sem argumentos.
    unsafe { GetTickCount() }.wrapping_sub(info.dwTime)
}

pub fn watch(on_change: OnChange) -> Result<()> {
    thread::spawn(move || {
        let (mut focused, mut idle): (Option<String>, Option<bool>) = (None, None);
        let mut first = true;
        loop {
            let now = focused_t3();
            if first || now != focused {
                on_change(Seen::Focus(now.as_deref()));
                focused = now;
            }
            let away = idle_ms() >= IDLE_MS;
            if idle != Some(away) {
                on_change(Seen::Idle(away));
                idle = Some(away);
            }
            first = false;
            thread::sleep(Duration::from_millis(300));
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_t3_executable() {
        assert!(is_t3(r"C:\Users\p\AppData\Local\Programs\T3 Code\T3 Code.exe"));
        assert!(is_t3(r"C:\Users\p\AppData\Local\Programs\t3code\T3 Code (Beta).exe"));
        assert!(!is_t3(r"C:\Windows\explorer.exe"));
    }
}
