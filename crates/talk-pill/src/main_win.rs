//! `talk-pill` no Windows: a mesma pílula do `linux.rs`, nativa.
//!
//! Uma janela "layered" do Win32: transparente por pixel, sempre por cima,
//! fora da barra de tarefas, sem foco (`WS_EX_NOACTIVATE`: o foco tem que
//! ficar no app onde você dita, é lá que o texto vai ser colado) e sem clique
//! (`WS_EX_TRANSPARENT`). Cada quadro é desenhado pelo `paint.rs` e entregue
//! inteiro com `UpdateLayeredWindow`. Sem motor web: aparece na hora.
//!
//! Como no Linux, o serviço a abre no início da gravação; ela ouve os eventos
//! pelo socket e sai sozinha quando a fase volta a pronto.
//!
//! O único lugar do talk com `unsafe`: são as chamadas da API do Windows, que
//! o crate `windows` expõe todas como `unsafe`. Cada bloco só passa ponteiros
//! para estruturas vivas na própria função.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;

use talk_core::paths;
use uds_windows::UnixStream;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG, PostQuitMessage,
    RegisterClassW, SPI_GETWORKAREA, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetTimer, ShowWindow,
    SystemParametersInfoW, ULW_ALPHA, UpdateLayeredWindow, WM_DESTROY, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;

use crate::paint::{self, Frame};

/// Do rodapé da área útil (acima da barra de tarefas) até a pílula: o mesmo
/// respiro de 48 do Linux.
const BOTTOM: f32 = 48.0;

#[derive(Default)]
struct State {
    recording: bool,
    seconds: u64,
    level: f32,
    done: bool,
}

struct Ctx {
    state: Arc<Mutex<State>>,
    scale: f32,
    started: Instant,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

fn lock(s: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

fn listen(state: Arc<Mutex<State>>) {
    std::thread::spawn(move || {
        if let Ok(stream) = UnixStream::connect(paths::socket()) {
            let mut out = &stream;
            let _ = writeln!(out, r#"{{"cmd":"subscribe"}}"#);
            for line in BufReader::new(&stream).lines() {
                let Ok(line) = line else { break };
                let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let mut st = lock(&state);
                match v["event"].as_str() {
                    Some("level") => {
                        st.level = v["level"].as_f64().unwrap_or(0.0) as f32;
                        st.seconds = v["seconds"].as_u64().unwrap_or(st.seconds);
                    }
                    Some("phase") => match v["phase"].as_str() {
                        Some("recording") => st.recording = true,
                        Some("transcribing") => st.recording = false,
                        _ => break,
                    },
                    _ => {}
                }
            }
        }
        lock(&state).done = true;
    });
}

/// Desenha o quadro e o entrega à janela, com o alfa de cada pixel.
fn present(hwnd: HWND, ctx: &Ctx) {
    let frame = {
        let st = lock(&ctx.state);
        Frame {
            recording: st.recording,
            seconds: st.seconds,
            level: st.level,
            pulse: paint::pulse(ctx.started.elapsed().as_secs_f32()),
        }
    };
    let Some(pm) = paint::draw(&frame, ctx.scale) else { return };
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    // SAFETY: DCs e o bitmap são criados, usados e soltos aqui; `bits` aponta
    // para w*h*4 bytes do DIB, que vive até o `DeleteObject`.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // de cima para baixo, como o pixmap
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        if let Ok(dib) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            && !bits.is_null()
        {
            let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
            // O tiny-skia é RGBA pré-multiplicado; o Windows quer BGRA pré-multiplicado.
            for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(pm.data().as_chunks::<4>().0) {
                *d = [s[2], s[1], s[0], s[3]];
            }
            let old = SelectObject(mem, dib.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                None,
                Some(&size),
                Some(mem),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(dib.into());
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            if let Some(ctx) = CTX.get() {
                if lock(&ctx.state).done {
                    // SAFETY: a própria janela, na thread dela.
                    let _ = unsafe { DestroyWindow(hwnd) };
                } else {
                    present(hwnd, ctx);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: encerra o laço de mensagens desta thread.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: o tratamento padrão, com os mesmos argumentos recebidos.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn main() -> anyhow::Result<()> {
    let state = Arc::new(Mutex::new(State { recording: true, ..State::default() }));
    listen(state.clone());

    // SAFETY: chamadas da API do Windows com estruturas locais; a janela e o
    // laço de mensagens vivem nesta thread até o `PostQuitMessage`.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let scale = GetDpiForSystem() as f32 / 96.0;
        let _ = CTX.set(Ctx { state, scale, started: Instant::now() });

        let instance = GetModuleHandleW(None)?;
        let class = w!("talk-pill");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);

        // A área útil exclui a barra de tarefas, onde quer que ela esteja.
        let mut work = RECT::default();
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut work).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        let (w, h) = ((paint::W * scale).round() as i32, (paint::H * scale).round() as i32);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.bottom - h - (BOTTOM * scale).round() as i32;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("talk"),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        if let Some(ctx) = CTX.get() {
            present(hwnd, ctx); // o primeiro quadro antes de aparecer: nada pisca
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), 1, 33, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
