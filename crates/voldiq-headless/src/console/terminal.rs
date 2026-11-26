use super::logo::Layout;
#[cfg(windows)]
use windows_sys::Win32::{Foundation::HANDLE, System::Console::*};

pub(super) struct Terminal {
    pub(super) accent: &'static str,
    pub(super) logo_sector: &'static str,
    pub(super) strong: &'static str,
    pub(super) quiet: &'static str,
    pub(super) reset: &'static str,
    #[cfg(windows)]
    motion: bool,
    #[cfg(windows)]
    mode: Option<OutputMode>,
}

impl Terminal {
    pub(super) fn open(interactive: bool, no_motion: bool) -> Self {
        let interactive = interactive && !std::env::var("TERM").is_ok_and(|term| term == "dumb");
        #[cfg(windows)]
        let mode = interactive.then(OutputMode::enable).flatten();
        #[cfg(windows)]
        let ansi = mode.as_ref().is_some_and(|mode| mode.ansi);
        #[cfg(not(windows))]
        let ansi = interactive;
        #[cfg(not(windows))]
        let _ = no_motion;

        let color = ansi && std::env::var_os("NO_COLOR").is_none();
        let style = |sequence| if color { sequence } else { "" };
        Self {
            accent: style("\x1b[1;96m"),
            logo_sector: style("\x1b[94m"),
            strong: style("\x1b[1m"),
            quiet: style("\x1b[2m"),
            reset: style("\x1b[0m"),
            #[cfg(windows)]
            motion: mode.is_some()
                && !no_motion
                && std::env::var_os("VOLDIQ_REDUCE_MOTION").is_none()
                && system_motion(),
            #[cfg(windows)]
            mode,
        }
    }

    pub(super) fn width(&self) -> usize {
        #[cfg(windows)]
        if let Some(info) = self.mode.as_ref().and_then(|mode| screen(mode.handle)) {
            let window = info.srWindow;
            return (window.Right - window.Left + 1) as usize;
        }
        80
    }

    #[cfg(windows)]
    pub(super) fn spin_idle(&self, layout: &Layout) {
        use std::{
            f64::consts::TAU,
            thread,
            time::{Duration, Instant},
        };

        let Some(mode) = &self.mode else {
            return;
        };
        let Some(offset) = layout.rows_above_prompt else {
            return;
        };
        if !self.motion || layout.columns < 26 {
            return;
        }
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let mut input_mode = 0;
        if unsafe { GetConsoleMode(input, &mut input_mode) } == 0
            || input_mode & ENABLE_LINE_INPUT == 0
        {
            return;
        }
        let Some(start) = screen(mode.handle) else {
            return;
        };
        let window = start.srWindow;
        let prompt = start.dwCursorPosition;
        let top = prompt.Y - offset as i16;
        let size = COORD {
            X: layout.columns as i16,
            Y: layout.rows as i16,
        };
        let region = SMALL_RECT {
            Left: 2,
            Top: top,
            Right: 1 + size.X,
            Bottom: top + size.Y - 1,
        };
        if window.Left != 0
            || window.Right + 1 != layout.terminal_width as i16
            || top < window.Top
            || region.Right > window.Right
            || prompt.Y > window.Bottom
            || prompt.X as usize != layout.menu_column + "Voldiq > ".len()
        {
            return;
        }

        let neutral = start.wAttributes;
        let blue = (neutral & !0x000f) | FOREGROUND_BLUE | FOREGROUND_INTENSITY;
        let mut frame = Vec::with_capacity(layout.columns * layout.rows);
        let started = Instant::now();
        loop {
            thread::sleep(Duration::from_millis(50));
            if !idle(input)
                || !screen(mode.handle).is_some_and(|current| {
                    current.dwCursorPosition.X == prompt.X
                        && current.dwCursorPosition.Y == prompt.Y
                        && current.dwSize.X == start.dwSize.X
                        && current.dwSize.Y == start.dwSize.Y
                        && current.srWindow.Left == window.Left
                        && current.srWindow.Right == window.Right
                        && current.srWindow.Top == window.Top
                        && current.srWindow.Bottom == window.Bottom
                })
            {
                return;
            }
            let yaw = super::logo::REST_YAW + TAU * (started.elapsed().as_secs_f64() / 14.0);
            frame.clear();
            frame.extend(layout.cells(yaw).map(|cell| CHAR_INFO {
                Char: CHAR_INFO_0 {
                    UnicodeChar: u16::from(cell.glyph),
                },
                Attributes: if cell.sector && !self.logo_sector.is_empty() {
                    blue
                } else {
                    neutral
                },
            }));
            // Writing only the logo cells keeps the terminal cursor and the menu in place.
            let mut written = region;
            if unsafe {
                WriteConsoleOutputW(
                    mode.handle,
                    frame.as_ptr(),
                    size,
                    COORD::default(),
                    &mut written,
                )
            } == 0
            {
                return;
            }
        }
    }

    #[cfg(not(windows))]
    pub(super) fn spin_idle(&self, _layout: &Layout) {}
}

#[cfg(windows)]
fn screen(handle: HANDLE) -> Option<CONSOLE_SCREEN_BUFFER_INFO> {
    let mut screen = CONSOLE_SCREEN_BUFFER_INFO::default();
    (unsafe { GetConsoleScreenBufferInfo(handle, &mut screen) } != 0).then_some(screen)
}

#[cfg(windows)]
fn idle(input: HANDLE) -> bool {
    let mut events = [INPUT_RECORD::default(); 64];
    let mut count = 0;
    // Inspect without removing input. The normal line editor receives the first key and all queued events.
    unsafe {
        PeekConsoleInputW(input, events.as_mut_ptr(), events.len() as u32, &mut count) != 0
            && (count as usize) < events.len()
            && !events[..count as usize].iter().any(|event| {
                u32::from(event.EventType) == KEY_EVENT && event.Event.KeyEvent.bKeyDown != 0
            })
    }
}

#[cfg(windows)]
struct OutputMode {
    handle: HANDLE,
    previous: u32,
    ansi: bool,
}

#[cfg(windows)]
impl OutputMode {
    fn enable() -> Option<Self> {
        let mut previous = 0;
        // The handles and buffer info belong to this process's attached console.
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            if GetConsoleMode(handle, &mut previous) == 0 {
                return None;
            }
            screen(handle)?;
            let ansi = SetConsoleMode(
                handle,
                previous | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            ) != 0;
            Some(Self {
                handle,
                previous,
                ansi,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for OutputMode {
    fn drop(&mut self) {
        if self.ansi {
            unsafe {
                SetConsoleMode(self.handle, self.previous);
            }
        }
    }
}

#[cfg(windows)]
fn system_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };
    let mut enabled = 0_i32;
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        )
    };
    result != 0 && enabled != 0
}
