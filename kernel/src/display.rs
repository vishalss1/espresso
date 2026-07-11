//! Display driver module - SSD1306 OLED display driver

pub mod controller {
    pub struct SSD1306;
}

pub mod tile {
    #[derive(Copy, Clone)]
    pub struct Cell {
        pub ch: u8,
        pub inv: bool,
    }

    pub const GRID_COLS: usize = 21;
    pub const GRID_ROWS: usize = 8;
    pub const SCROLL_ROWS: usize = 40;

    pub struct CharGrid {
        pub visible: [[Cell; GRID_COLS]; GRID_ROWS],
        pub scroll_buf: [[Cell; GRID_COLS]; SCROLL_ROWS],
    }

    impl CharGrid {
        pub fn new() -> Self {
            Self {
                visible: [[Cell { ch: b' ', inv: false }; GRID_COLS]; GRID_ROWS],
                scroll_buf: [[Cell { ch: b' ', inv: false }; GRID_COLS]; SCROLL_ROWS],
            }
        }
    }
}

pub const SCREEN_WIDTH: u8 = 128;
pub const SCREEN_HEIGHT: u8 = 64;
pub const CHAR_WIDTH: u8 = 6;
pub const CHAR_HEIGHT: u8 = 8;
