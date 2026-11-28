//! The three retro looks (one per OS), each in light and dark.

use gpui::{rgb, Hsla};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub os: Os,
    pub mode: Mode,
    /// Window and button surface.
    pub face: Hsla,
    /// Inset areas: lists, fields, the well behind the wheel.
    pub panel: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    /// Outlines.
    pub line: Hsla,
    /// Bevel highlights and shadows. `hi2`/`lo2` are the second bevel layer
    /// (only the Windows look uses two layers).
    pub hi: Hsla,
    pub hi2: Hsla,
    pub lo: Hsla,
    pub lo2: Hsla,
    pub track: Hsla,
    pub title: Hsla,
    pub title_text: Hsla,
    pub sel: Hsla,
    pub warn: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Theme {
    /// The look that belongs to the OS this build runs on.
    pub fn native_os() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    pub fn new(os: Os, mode: Mode) -> Self {
        let dark = mode == Mode::Dark;
        match (os, dark) {
            (Os::Mac, false) => Theme {
                os, mode,
                face: c(0xDDDDDD), panel: c(0xFFFFFF), text: c(0x000000), muted: c(0x444444),
                line: c(0x000000), hi: c(0xFFFFFF), hi2: c(0xFFFFFF), lo: c(0x888888), lo2: c(0x888888),
                track: c(0xBBBBBB), title: c(0xDDDDDD), title_text: c(0x000000),
                sel: c(0x333399), warn: c(0x8A4B00),
            },
            (Os::Mac, true) => Theme {
                os, mode,
                face: c(0x2E2E2E), panel: c(0x161616), text: c(0xF2F2F2), muted: c(0xB8B8B8),
                line: c(0xA8A8A8), hi: c(0x555555), hi2: c(0x555555), lo: c(0x111111), lo2: c(0x111111),
                track: c(0x444444), title: c(0x2E2E2E), title_text: c(0xF2F2F2),
                sel: c(0x8C8CF5), warn: c(0xF5B04C),
            },
            (Os::Windows, false) => Theme {
                os, mode,
                face: c(0xC0C0C0), panel: c(0xFFFFFF), text: c(0x000000), muted: c(0x404040),
                line: c(0x000000), hi: c(0xFFFFFF), hi2: c(0xDFDFDF), lo: c(0x808080), lo2: c(0x0A0A0A),
                track: c(0xA0A0A0), title: c(0x000080), title_text: c(0xFFFFFF),
                sel: c(0x000080), warn: c(0x7A3E00),
            },
            (Os::Windows, true) => Theme {
                os, mode,
                face: c(0x454545), panel: c(0x1C1C1C), text: c(0xF2F2F2), muted: c(0xC8C8C8),
                line: c(0xDDDDDD), hi: c(0x8C8C8C), hi2: c(0x606060), lo: c(0x2A2A2A), lo2: c(0x000000),
                track: c(0x2E2E2E), title: c(0x1B2F9E), title_text: c(0xFFFFFF),
                sel: c(0x7F93FF), warn: c(0xF5B04C),
            },
            (Os::Linux, false) => Theme {
                os, mode,
                face: c(0xAEB2C3), panel: c(0xD4D7E2), text: c(0x000000), muted: c(0x30333D),
                line: c(0x000000), hi: c(0xDCDEE5), hi2: c(0xDCDEE5), lo: c(0x5D6069), lo2: c(0x5D6069),
                track: c(0x8E92A3), title: c(0xB24D7A), title_text: c(0xFFFFFF),
                sel: c(0x8A2F5A), warn: c(0x6B3500),
            },
            (Os::Linux, true) => Theme {
                os, mode,
                face: c(0x4A5368), panel: c(0x333A4A), text: c(0xF4F4F6), muted: c(0xCDD0DA),
                line: c(0xE4E6EC), hi: c(0x7A859E), hi2: c(0x7A859E), lo: c(0x232833), lo2: c(0x232833),
                track: c(0x2B303D), title: c(0x2F7D78), title_text: c(0xFFFFFF),
                sel: c(0x6FD0C9), warn: c(0xF5B04C),
            },
        }
    }

    pub fn toggled(self) -> Self {
        let mode = if self.mode == Mode::Dark { Mode::Light } else { Mode::Dark };
        Theme::new(self.os, mode)
    }
}

/// Slice colours for the wheel, shared by every look.
pub const PALETTE: [u32; 6] = [0x3D7BFF, 0x19B394, 0xF2A33A, 0x8E6CF0, 0xEF5F86, 0x4FB8E8];

/// Mixes a palette colour toward white; deeper rings use a higher `t`.
pub fn tint(hex: u32, t: f32) -> Hsla {
    let ch = |shift: u32| {
        let v = ((hex >> shift) & 0xFF) as f32;
        (v * (1.0 - t) + 255.0 * t).round() as u32
    };
    c((ch(16) << 16) | (ch(8) << 8) | ch(0))
}

/// Dark ink used for labels drawn on top of slice colours.
pub fn ink() -> Hsla {
    c(0x0B1024)
}
