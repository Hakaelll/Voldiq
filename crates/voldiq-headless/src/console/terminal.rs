pub(super) struct Terminal {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) accent: &'static str,
    pub(super) strong: &'static str,
    pub(super) quiet: &'static str,
    pub(super) reset: &'static str,
    pub(super) motion: bool,
    #[cfg(windows)]
    _mode: Option<OutputMode>,
}

impl Terminal {
    pub(super) fn open(interactive: bool, no_motion: bool) -> Self {
        let interactive = interactive && !std::env::var("TERM").is_ok_and(|term| term == "dumb");
        #[cfg(windows)]
        let mode = interactive.then(OutputMode::enable).flatten();
        #[cfg(windows)]
        let (ansi, width, height) = mode
            .as_ref()
            .map_or((false, 80, 24), |mode| (true, mode.width, mode.height));
        #[cfg(not(windows))]
        let (ansi, width, height) = (interactive, 80, 24);

        let color = ansi && std::env::var_os("NO_COLOR").is_none();
        let style = |sequence| if color { sequence } else { "" };
        Self {
            width,
            height,
            accent: style("\x1b[1;96m"),
            strong: style("\x1b[1m"),
            quiet: style("\x1b[2m"),
            reset: style("\x1b[0m"),
            motion: ansi
                && !no_motion
                && std::env::var_os("VOLDIQ_REDUCE_MOTION").is_none()
                && system_motion(),
            #[cfg(windows)]
            _mode: mode,
        }
    }
}

#[cfg(windows)]
struct OutputMode {
    handle: windows_sys::Win32::Foundation::HANDLE,
    previous: u32,
    width: usize,
    height: usize,
}

#[cfg(windows)]
impl OutputMode {
    fn enable() -> Option<Self> {
        use windows_sys::Win32::System::Console::*;
        let mut previous = 0;
        // The handles and buffer info belong to this process's attached console.
        unsafe {
            let handle = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut screen = std::mem::zeroed::<CONSOLE_SCREEN_BUFFER_INFO>();
            if GetConsoleMode(handle, &mut previous) == 0
                || GetConsoleScreenBufferInfo(handle, &mut screen) == 0
                || SetConsoleMode(
                    handle,
                    previous | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
                ) == 0
            {
                return None;
            }
            Some(Self {
                handle,
                previous,
                width: (screen.srWindow.Right - screen.srWindow.Left + 1) as usize,
                height: (screen.srWindow.Bottom - screen.srWindow.Top + 1) as usize,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for OutputMode {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleMode(self.handle, self.previous);
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

#[cfg(not(windows))]
fn system_motion() -> bool {
    true
}
