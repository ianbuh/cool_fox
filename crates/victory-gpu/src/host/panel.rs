//! On-screen RivaTuner (RTSS)-style performance overlay: FPS, frametime,
//! 1% low, GPU/VRAM/RAM/draws, and a live rolling frametime graph in the
//! upper-left corner of the swap chain.
//!
//! Enabled by default (`VICTORY_HUD=0` or `VICTORY_PANEL=0` starts hidden);
//! press **F3** while the game window is focused to toggle it on or off.
//! Copied onto the swap chain's back buffer behind the title's own picture
//! so saved frames and the frame recorder see the title's own output.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F3};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::{Host, HostList, transition};

/// How often the HUD text and graph texture are redrawn.
const EVERY: Duration = Duration::from_millis(250);

/// A character's dots, and with the space beside and below it.
const GLYPH: (u32, u32) = (5, 7);
const CELL: (u32, u32) = (6, 9);
/// Characters in a line, text lines, rim, and frametime graph height in dots.
const COLUMNS: u32 = 34;
const LINES: u32 = 6;
const RIM: u32 = 4;
const GRAPH_GAP: u32 = 3;
const GRAPH_H: u32 = 24;
/// Number of frametime samples plotted across the graph (1 dot per frame).
const GRAPH_SAMPLES: usize = (COLUMNS * CELL.0) as usize;
/// Top of the frametime graph in milliseconds (66.7 ms = 15 FPS; 33.3 ms is mid-height).
const GRAPH_MAX_MS: f64 = 66.67;

/// The texts are put into one of these in turn: the card reads the others.
const UPLOADS: usize = 4;

fn default_shown() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let off = |name: &str| std::env::var_os(name).is_some_and(|v| v == "0");
        !off("VICTORY_HUD") && !off("VICTORY_PANEL")
    })
}

/// Whether it is shown now.
fn shown() -> &'static AtomicBool {
    static SHOWN: OnceLock<AtomicBool> = OnceLock::new();
    SHOWN.get_or_init(|| AtomicBool::new(default_shown()))
}

/// F3 was pressed since the last call, with one of our windows in front.
fn pressed() -> bool {
    static DOWN: AtomicBool = AtomicBool::new(false);
    // SAFETY: a plain query.
    let down = unsafe { GetAsyncKeyState(i32::from(VK_F3)) } < 0;
    if DOWN.swap(down, Ordering::Relaxed) || !down {
        return false;
    }
    let mut process = 0;
    // SAFETY: plain queries; room for the answer.
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut process) };
    process == std::process::id()
}

/// Palette indices for RTSS-style rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Tone {
    Bg = 0,
    Border = 1,
    Label = 2,  // Classic RTSS orange
    Value = 3,  // Crisp white
    Cyan = 4,   // Hardware stat cyan
    Good = 5,   // >= 30 FPS / <= 33.3 ms
    Warn = 6,   // 20-29 FPS / <= 50 ms
    Bad = 7,    // < 20 FPS / > 50 ms
    Muted = 8,  // Subtle guide lines / hints
    GraphBg = 9,
}

const PALETTE_RGB: [[u8; 3]; 10] = [
    [14, 16, 22],    // Bg
    [52, 60, 76],    // Border
    [255, 148, 36],  // Label (RTSS orange)
    [244, 246, 250], // Value (white)
    [92, 225, 255],  // Cyan
    [72, 240, 128],  // Good (green)
    [255, 210, 56],  // Warn (yellow)
    [255, 76, 76],   // Bad (red)
    [116, 126, 144], // Muted
    [22, 26, 36],    // GraphBg
];

/// Seven rows of five dots, the leftmost the highest bit.
#[rustfmt::skip]
const fn glyph(c: u8) -> [u8; 7] {
    match c {
        b'0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        b'1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        b'3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        b'4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        b'5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        b'6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        b'7' => [0b11111, 0b00001, 0b00010, 0b00010, 0b00100, 0b00100, 0b00100],
        b'8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        b'9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        b'A' => [0b00100, 0b01010, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001],
        b'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        b'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        b'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        b'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        b'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        b'G' => [0b01111, 0b10000, 0b10000, 0b10011, 0b10001, 0b10001, 0b01111],
        b'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        b'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        b'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        b'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        b'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        b'N' => [0b10001, 0b11001, 0b11001, 0b10101, 0b10011, 0b10011, 0b10001],
        b'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        b'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        b'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        b'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        b'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        b'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        b'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        b'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        b'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        b'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        b'.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        b'/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        b'-' => [0, 0, 0, 0b11111, 0, 0, 0],
        b':' => [0, 0b00100, 0b00100, 0, 0b00100, 0b00100, 0],
        b'%' => [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011],
        b' ' => [0; 7],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}

/// One coloured text span inside a HUD line.
#[derive(Clone)]
struct Span {
    text: String,
    tone: Tone,
}

type Line = Vec<Span>;

fn span(text: impl Into<String>, tone: Tone) -> Span {
    Span { text: text.into(), tone }
}

/// The box's size in logical dots (before `scale`).
const fn logical_size() -> (u32, u32) {
    (COLUMNS * CELL.0 + 2 * RIM, LINES * CELL.1 + GRAPH_GAP + GRAPH_H + 2 * RIM)
}

/// The box's size in pixels, a dot being `scale` of them each way.
const fn size(scale: u32) -> (u32, u32) {
    let (w, h) = logical_size();
    (w * scale, h * scale)
}

/// Pixels to a dot for a picture that high.
fn scale(height: u32) -> u32 {
    (height / 540).max(1)
}

/// Encodes an sRGB channel (0..=255) as IEEE-754 binary16 (linear-ish for HUD readability).
fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let frac = bits & 0x007f_ffff;
    if exp <= 0 {
        sign
    } else if exp >= 31 {
        sign | 0x7c00
    } else {
        sign | ((exp as u16) << 10) | ((frac >> 13) as u16)
    }
}

struct Palette {
    bytes: usize,
    pixels: [[u8; 8]; 10],
}

impl Palette {
    fn for_format(format: DXGI_FORMAT) -> Option<Palette> {
        let bytes = match format {
            DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_R10G10B10A2_UNORM => 4,
            DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
            _ => return None,
        };
        let mut pixels = [[0u8; 8]; 10];
        for (slot, [r, g, b]) in pixels.iter_mut().zip(PALETTE_RGB) {
            match format {
                DXGI_FORMAT_R8G8B8A8_UNORM => {
                    slot[0..4].copy_from_slice(&[r, g, b, 0xFF]);
                }
                DXGI_FORMAT_B8G8R8A8_UNORM => {
                    slot[0..4].copy_from_slice(&[b, g, r, 0xFF]);
                }
                DXGI_FORMAT_R10G10B10A2_UNORM => {
                    let r10 = (u32::from(r) * 1023 + 127) / 255;
                    let g10 = (u32::from(g) * 1023 + 127) / 255;
                    let b10 = (u32::from(b) * 1023 + 127) / 255;
                    let packed = r10 | (g10 << 10) | (b10 << 20) | (3 << 30);
                    slot[0..4].copy_from_slice(&packed.to_le_bytes());
                }
                DXGI_FORMAT_R16G16B16A16_FLOAT => {
                    let srgb_to_lin = |c: u8| (f32::from(c) / 255.0).powf(2.2);
                    let r16 = f32_to_f16(srgb_to_lin(r)).to_le_bytes();
                    let g16 = f32_to_f16(srgb_to_lin(g)).to_le_bytes();
                    let b16 = f32_to_f16(srgb_to_lin(b)).to_le_bytes();
                    let a16 = f32_to_f16(1.0).to_le_bytes();
                    slot[0..2].copy_from_slice(&r16);
                    slot[2..4].copy_from_slice(&g16);
                    slot[4..6].copy_from_slice(&b16);
                    slot[6..8].copy_from_slice(&a16);
                }
                _ => unreachable!(),
            }
        }
        Some(Palette { bytes, pixels })
    }

    fn get(&self, tone: Tone) -> &[u8] {
        &self.pixels[tone as usize][..self.bytes]
    }
}

fn frametime_tone(ms: f64) -> Tone {
    if ms <= 34.0 {
        Tone::Good
    } else if ms <= 50.0 {
        Tone::Warn
    } else {
        Tone::Bad
    }
}

fn fps_tone(fps: f64) -> Tone {
    if fps >= 29.5 {
        Tone::Good
    } else if fps >= 20.0 {
        Tone::Warn
    } else {
        Tone::Bad
    }
}

/// Computes the 1% low FPS from a slice of recent frametimes in milliseconds.
fn one_percent_low_fps(history: &VecDeque<f64>) -> f64 {
    if history.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = history.iter().copied().filter(|ms| *ms > 0.0).collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.total_cmp(b));
    let idx = ((sorted.len() * 99) / 100).min(sorted.len() - 1);
    1000.0 / sorted[idx].max(0.001)
}

/// Flattens a line of coloured spans into `(ascii_char, Tone)` pairs.
fn flatten_line(line: &Line) -> Vec<(u8, Tone)> {
    let mut out = Vec::with_capacity(COLUMNS as usize);
    for s in line {
        for &b in s.text.as_bytes() {
            if out.len() >= COLUMNS as usize {
                break;
            }
            out.push((b, s.tone));
        }
    }
    out
}

/// Renders the RTSS-style text rows + frametime graph as rows of `pitch` bytes.
fn painted(lines: &[Line], history: &VecDeque<f64>, scale: u32, palette: &Palette, pitch: usize) -> Vec<u8> {
    let (lw, lh) = logical_size();
    let height = lh * scale;
    let bytes = palette.bytes;
    let mut grid = vec![Tone::Bg; (lw * lh) as usize];
    let set = |grid: &mut [Tone], x: u32, y: u32, tone: Tone| {
        if x < lw && y < lh {
            grid[(y * lw + x) as usize] = tone;
        }
    };

    // Subtle 1-dot outer border.
    for x in 0..lw {
        set(&mut grid, x, 0, Tone::Border);
        set(&mut grid, x, lh - 1, Tone::Border);
    }
    for y in 0..lh {
        set(&mut grid, 0, y, Tone::Border);
        set(&mut grid, lw - 1, y, Tone::Border);
    }

    // Text rows.
    for (row, line) in lines.iter().take(LINES as usize).enumerate() {
        let cells = flatten_line(line);
        let base_y = RIM + row as u32 * CELL.1;
        for (col, &(ch, tone)) in cells.iter().enumerate() {
            let bits = glyph(ch.to_ascii_uppercase());
            let base_x = RIM + col as u32 * CELL.0;
            for (gy, &mask) in bits.iter().enumerate() {
                for gx in 0..GLYPH.0 {
                    if (mask >> (GLYPH.0 - 1 - gx)) & 1 != 0 {
                        set(&mut grid, base_x + gx, base_y + gy as u32, tone);
                    }
                }
            }
        }
    }

    // Frametime graph area below the text rows.
    let gx0 = RIM;
    let gy0 = RIM + LINES * CELL.1 + GRAPH_GAP;
    let gw = COLUMNS * CELL.0;
    for y in gy0..gy0 + GRAPH_H {
        for x in gx0..gx0 + gw {
            set(&mut grid, x, y, Tone::GraphBg);
        }
    }
    // Dashed 30 FPS (33.3 ms) and 60 FPS (16.7 ms) reference lines.
    for target_ms in [33.33, 16.67] {
        let row_from_bottom = ((target_ms / GRAPH_MAX_MS) * (GRAPH_H - 1) as f64).round() as u32;
        let gy = gy0 + (GRAPH_H - 1).saturating_sub(row_from_bottom.min(GRAPH_H - 1));
        for x in (gx0..gx0 + gw).step_by(3) {
            set(&mut grid, x, gy, Tone::Border);
        }
    }
    // Plot rolling frametimes right-aligned so newest frames arrive on the right.
    let count = history.len().min(gw as usize);
    let start_x = gx0 + gw - count as u32;
    let skip = history.len().saturating_sub(count);
    for (i, &ms) in history.iter().skip(skip).enumerate() {
        let clamped = ms.clamp(0.0, GRAPH_MAX_MS);
        let h_from_bottom = ((clamped / GRAPH_MAX_MS) * (GRAPH_H - 1) as f64).round() as u32;
        let py = gy0 + (GRAPH_H - 1).saturating_sub(h_from_bottom.min(GRAPH_H - 1));
        let px = start_x + i as u32;
        let tone = frametime_tone(ms);
        set(&mut grid, px, py, tone);
        if py + 1 < gy0 + GRAPH_H {
            set(&mut grid, px, py + 1, tone);
        }
    }

    // Scale logical grid up into the swap-chain pixel buffer.
    let mut out = vec![0u8; pitch * height as usize];
    for y in 0..lh {
        let top = pitch * (y * scale) as usize;
        for x in 0..lw {
            let colour = palette.get(grid[(y * lw + x) as usize]);
            for i in 0..scale {
                let at = top + (x * scale + i) as usize * bytes;
                out[at..at + bytes].copy_from_slice(colour);
            }
        }
        for i in 1..scale as usize {
            out.copy_within(top..top + pitch, top + i * pitch);
        }
    }
    out
}

/// The counters read for the HUD. They grow, and go back to 0 with the frame-rate line.
#[derive(Clone, Copy, Default)]
struct Counters([u64; COUNTERS]);

const COUNTERS: usize = 5;

impl Counters {
    fn now() -> Counters {
        use super::CARD_CLOCK_US;
        let get = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
        Counters([
            get(&CARD_CLOCK_US[0]),
            get(&CARD_CLOCK_US[2]),
            crate::draw::drawn(),
            crate::draw::dropped_now(),
            super::worked(),
        ])
    }

    /// What came since `before`, per frame.
    fn since(self, before: Counters, frames: f64) -> [f64; COUNTERS] {
        let mut out = [0.0; COUNTERS];
        for (i, out) in out.iter_mut().enumerate() {
            let (was, is) = (before.0[i], self.0[i]);
            *out = if is >= was { is - was } else { is } as f64 / frames;
        }
        out
    }
}

fn process_memory() -> (u64, u64) {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: our own process and a sized structure.
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (c.WorkingSetSize as u64, c.PagefileUsage as u64)
}

/// Builds the 6 RTSS-style HUD lines.
fn lines_for(c: &[f64; COUNTERS], ms: f64, worst: f64, low1_fps: f64, host: &Host) -> Vec<Line> {
    let gb = |bytes: u64| bytes as f64 / (1u64 << 30) as f64;
    let (working, private) = process_memory();
    let [budget, used, _, shared] = host.video_memory().unwrap_or_default();
    let fps = 1000.0 / ms.max(0.001);
    let gpu_idle = (c[0] / 1e3).min(ms);
    let gpu_busy = (ms - gpu_idle).max(0.0);
    let vram_pct = if budget > 0 { (u128::from(used) * 100 / u128::from(budget)) as u64 } else { 0 };
    let vram_tone = if vram_pct >= 92 {
        Tone::Bad
    } else if vram_pct >= 82 {
        Tone::Warn
    } else {
        Tone::Cyan
    };
    let shared_tone = if shared >= (512 << 20) { Tone::Warn } else { Tone::Muted };

    vec![
        vec![
            span("D3D12  ", Tone::Label),
            span(format!("{fps:5.1} FPS"), fps_tone(fps)),
            span(format!("  {ms:5.1} MS"), Tone::Value),
        ],
        vec![
            span("1% LOW ", Tone::Label),
            span(format!("{low1_fps:5.1} FPS"), fps_tone(low1_fps)),
            span(format!("  MAX {worst:3.0} MS"), Tone::Value),
        ],
        vec![
            span("GPU    ", Tone::Label),
            span(format!("{gpu_busy:5.1} MS "), Tone::Cyan),
            span(format!(" WAIT {:4.1} MS", c[1] / 1e3), Tone::Value),
        ],
        vec![
            span("VRAM   ", Tone::Label),
            span(format!("{:.1}/{:.1} GB", gb(used), gb(budget)), vram_tone),
            span(format!("  SH {:.1}", gb(shared)), shared_tone),
        ],
        vec![
            span("RAM    ", Tone::Label),
            span(format!("{:4.1} GB  ", gb(private)), Tone::Cyan),
            span(format!(" WS {:4.1} GB", gb(working)), Tone::Value),
        ],
        vec![
            span("DRAWS  ", Tone::Label),
            span(format!("{:<6.0}", c[2]), Tone::Cyan),
            span("       F3 HIDE", Tone::Muted),
        ],
    ]
}

/// The buffers of a box that size and format.
struct Buffers {
    format: DXGI_FORMAT,
    scale: u32,
    pitch: u32,
    uploads: Vec<(ID3D12Resource, *mut u8)>,
    card: ID3D12Resource,
    /// The upload written last.
    turn: usize,
    /// It is not in the card's buffer yet.
    fresh: bool,
}

// SAFETY: free-threaded resources, and memory only written under the presenter's lock.
unsafe impl Send for Buffers {}

impl Buffers {
    fn new(host: &Host, format: DXGI_FORMAT, scale: u32) -> Option<Buffers> {
        let palette = Palette::for_format(format)?;
        let (width, height) = size(scale);
        let pitch = (width * palette.bytes as u32).next_multiple_of(256);
        let bytes = u64::from(pitch) * u64::from(height);
        let uploads = (0..UPLOADS).map(|_| host.upload_buffer(bytes)).collect::<Option<Vec<_>>>()?;
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut card: Option<ID3D12Resource> = None;
        // SAFETY: plain creation of a buffer.
        unsafe {
            host.device
                .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut card)
                .ok()?;
        }
        Some(Buffers { format, scale, pitch, uploads, card: card?, turn: 0, fresh: false })
    }

    fn write(&mut self, lines: &[Line], history: &VecDeque<f64>) {
        let Some(palette) = Palette::for_format(self.format) else { return };
        let picture = painted(lines, history, self.scale, &palette, self.pitch as usize);
        self.turn = (self.turn + 1) % UPLOADS;
        // SAFETY: a mapped upload buffer of the picture's size, which the card read earlier.
        unsafe { std::ptr::copy_nonoverlapping(picture.as_ptr(), self.uploads[self.turn].1, picture.len()) };
        self.fresh = true;
    }
}

/// The HUD panel of a presenter.
pub(super) struct Panel {
    since: Instant,
    frames: u32,
    last: Instant,
    worst: f64,
    history: VecDeque<f64>,
    before: Counters,
    lines: Vec<Line>,
    changed: bool,
    buffers: Option<Buffers>,
    failed: bool,
}

impl Panel {
    pub(super) fn new() -> Panel {
        let now = Instant::now();
        Panel {
            since: now,
            frames: 0,
            last: now,
            worst: 0.0,
            history: VecDeque::with_capacity(GRAPH_SAMPLES),
            before: Counters::default(),
            lines: Vec::new(),
            changed: false,
            buffers: None,
            failed: false,
        }
    }

    /// A frame was presented; `line`: the counters go back to 0 right behind this call.
    pub(super) fn frame(&mut self, line: bool) {
        if pressed() {
            shown().fetch_xor(true, Ordering::Relaxed);
        }
        if !shown().load(Ordering::Relaxed) {
            self.frames = 0;
            return;
        }
        let now = Instant::now();
        if self.frames == 0 && now - self.last > 4 * EVERY {
            (self.since, self.last, self.worst, self.before) = (now, now, 0.0, Counters::now());
            self.frames = 0;
        }
        let dt_ms = (now - self.last).as_secs_f64() * 1e3;
        self.last = now;
        self.frames += 1;
        if dt_ms > 0.0 && dt_ms < 5_000.0 {
            self.worst = self.worst.max(dt_ms);
            if self.history.len() == GRAPH_SAMPLES {
                self.history.pop_front();
            }
            self.history.push_back(dt_ms);
        }
        let elapsed = now - self.since;
        if elapsed < EVERY && !(line && elapsed * 2 >= EVERY) {
            return;
        }
        let Some(host) = super::host() else { return };
        let frames = f64::from(self.frames);
        let counters = Counters::now();
        let c = counters.since(self.before, frames);
        self.before = counters;
        let avg_ms = elapsed.as_secs_f64() * 1e3 / frames;
        let low1 = one_percent_low_fps(&self.history);
        self.lines = lines_for(&c, avg_ms, self.worst, low1, host);
        self.changed = true;
        (self.since, self.frames, self.worst) = (now, 0, 0.0);
    }

    /// Records the copy of the HUD box onto `back`, a buffer of the swap chain
    /// in `COPY_DEST` state.
    pub(super) fn draw(
        &mut self,
        host: &Host,
        list: &HostList,
        back: &ID3D12Resource,
        (width, height): (u32, u32),
        format: DXGI_FORMAT,
    ) {
        if !shown().load(Ordering::Relaxed) || self.lines.is_empty() || self.failed {
            return;
        }
        // Not the title's: the frame recorder leaves it out.
        #[cfg(feature = "diagnostics")]
        let _ours = crate::replay::record::unrecorded();
        let scale = scale(height);
        if self.buffers.as_ref().is_none_or(|b| (b.format, b.scale) != (format, scale)) {
            std::mem::forget(self.buffers.take());
            self.buffers = Buffers::new(host, format, scale);
            self.failed = self.buffers.is_none();
            self.changed = true;
        }
        let Some(buffers) = &mut self.buffers else { return };
        if std::mem::take(&mut self.changed) {
            buffers.write(&self.lines, &self.history);
        }
        let (w, h) = size(scale);
        let list = &list.list;
        // SAFETY: an open list and live resources; the box lies within both.
        unsafe {
            if std::mem::take(&mut buffers.fresh) {
                list.CopyResource(&buffers.card, &buffers.uploads[buffers.turn].0);
                list.ResourceBarrier(&[transition(
                    &buffers.card,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    D3D12_RESOURCE_STATE_COPY_SOURCE,
                )]);
            }
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(&buffers.card),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: format,
                            Width: w,
                            Height: h,
                            Depth: 1,
                            RowPitch: buffers.pitch,
                        },
                    },
                },
            };
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: std::mem::transmute_copy(back),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            let part = D3D12_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: w.min(width),
                bottom: h.min(height),
                back: 1,
            };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&part));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character the lines use has its own picture, and no two look alike.
    #[test]
    fn characters_differ() {
        let used: Vec<u8> = (b'0'..=b'9').chain(b'A'..=b'Z').chain(*b"./-:% ").collect();
        for (i, a) in used.iter().enumerate() {
            assert_ne!(glyph(*a), glyph(b'#'), "{} has a picture", *a as char);
            assert!(glyph(*a).iter().all(|row| row >> GLYPH.0 == 0));
            for b in &used[i + 1..] {
                assert_ne!(glyph(*a), glyph(*b), "{} {}", *a as char, *b as char);
            }
        }
    }

    #[test]
    fn a_box_is_painted_with_colours_and_graph() {
        let lines = vec![vec![span("I", Tone::Label)], vec![span(" i", Tone::Good)]];
        let mut history = VecDeque::new();
        history.push_back(33.3);
        history.push_back(16.6);
        for scale in [1, 2, 4] {
            let (width, height) = size(scale);
            let palette = Palette::for_format(DXGI_FORMAT_R8G8B8A8_UNORM).unwrap();
            let pitch = (width * palette.bytes as u32).next_multiple_of(256) as usize;
            let picture = painted(&lines, &history, scale, &palette, pitch);
            assert_eq!(picture.len(), pitch * height as usize);
            let at = |x: u32, y: u32| &picture[y as usize * pitch + x as usize * 4..][..4];
            // Outer border at (0,0), orange label glyph at (RIM+2, RIM), green glyph on next row.
            assert_eq!(at(0, 0), palette.get(Tone::Border));
            assert_eq!(at((RIM + 2) * scale, RIM * scale), palette.get(Tone::Label));
            assert_eq!(at((RIM + CELL.0 + 2) * scale, (RIM + CELL.1) * scale), palette.get(Tone::Good));
            assert_eq!(at((RIM + CELL.0 * 2 + 2) * scale, (RIM + CELL.1) * scale), palette.get(Tone::Bg));
        }
    }

    #[test]
    fn one_percent_low_tracks_tail_frametimes() {
        let mut history = VecDeque::new();
        for _ in 0..99 {
            history.push_back(16.0);
        }
        history.push_back(50.0);
        let low1 = one_percent_low_fps(&history);
        assert!((low1 - 20.0).abs() < 0.01, "expected 20.0 fps, got {low1}");
    }

    #[test]
    fn counters_that_went_back_to_nothing_count_from_there() {
        let (mut a, mut b) = (Counters::default(), Counters::default());
        (a.0[0], b.0[0]) = (10, 30);
        (a.0[1], b.0[1]) = (50, 20);
        let c = b.since(a, 10.0);
        assert_eq!((c[0], c[1]), (2.0, 2.0));
    }
}
