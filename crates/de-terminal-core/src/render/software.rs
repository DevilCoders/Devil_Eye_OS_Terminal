//! Deterministic CPU cell renderer used as a headless reference backend.
//!
//! The renderer consumes immutable terminal frames and a caller-provided glyph
//! atlas. Font rasterization is deliberately separate: a font backend builds
//! the atlas, while this module performs bounded cell layout, color/style
//! composition, clipping, selection, cursor, and pixel output.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::screen::{CellContent, Color, GraphemeWidth, StyleFlags};
use crate::TerminalSize;

use super::{
    CellRect, Frame, GlyphMetrics, RenderContractError, Renderer, ScaleFactor, Selection,
};

/// One RGBA pixel with an opaque default alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgba {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

impl Rgba {
    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::new(red, green, blue, 255)
    }

    pub const fn red(self) -> u8 {
        self.red
    }

    pub const fn green(self) -> u8 {
        self.green
    }

    pub const fn blue(self) -> u8 {
        self.blue
    }

    pub const fn alpha(self) -> u8 {
        self.alpha
    }

    fn dimmed(self) -> Self {
        Self {
            red: (u16::from(self.red) * 2 / 3) as u8,
            green: (u16::from(self.green) * 2 / 3) as u8,
            blue: (u16::from(self.blue) * 2 / 3) as u8,
            alpha: self.alpha,
        }
    }
}

/// Theme colors for defaults, selections, and the block cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPalette {
    default_foreground: Rgba,
    default_background: Rgba,
    selection_foreground: Rgba,
    selection_background: Rgba,
    cursor_foreground: Rgba,
    cursor_background: Rgba,
}

impl RenderPalette {
    pub const fn new(
        default_foreground: Rgba,
        default_background: Rgba,
        selection_foreground: Rgba,
        selection_background: Rgba,
        cursor_foreground: Rgba,
        cursor_background: Rgba,
    ) -> Self {
        Self {
            default_foreground,
            default_background,
            selection_foreground,
            selection_background,
            cursor_foreground,
            cursor_background,
        }
    }

    pub const fn default_foreground(self) -> Rgba {
        self.default_foreground
    }

    pub const fn default_background(self) -> Rgba {
        self.default_background
    }

    pub const fn selection_foreground(self) -> Rgba {
        self.selection_foreground
    }

    pub const fn selection_background(self) -> Rgba {
        self.selection_background
    }

    pub const fn cursor_foreground(self) -> Rgba {
        self.cursor_foreground
    }

    pub const fn cursor_background(self) -> Rgba {
        self.cursor_background
    }
}

impl Default for RenderPalette {
    fn default() -> Self {
        Self::new(
            Rgba::rgb(220, 220, 220),
            Rgba::rgb(16, 18, 22),
            Rgba::rgb(255, 255, 255),
            Rgba::rgb(48, 88, 150),
            Rgba::rgb(16, 18, 22),
            Rgba::rgb(220, 220, 220),
        )
    }
}

/// Alpha-only glyph bitmap in logical-pixel coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphBitmap {
    width: u16,
    height: u16,
    alpha: Vec<u8>,
}

impl GlyphBitmap {
    pub fn new(width: u16, height: u16, alpha: Vec<u8>) -> Result<Self, SoftwareRenderError> {
        if width == 0 || height == 0 {
            return Err(SoftwareRenderError::InvalidGlyphBitmap);
        }
        let expected = usize::from(width)
            .checked_mul(usize::from(height))
            .ok_or(SoftwareRenderError::InvalidGlyphBitmap)?;
        if alpha.len() != expected {
            return Err(SoftwareRenderError::InvalidGlyphBitmap);
        }
        Ok(Self {
            width,
            height,
            alpha,
        })
    }

    pub const fn width(&self) -> u16 {
        self.width
    }

    pub const fn height(&self) -> u16 {
        self.height
    }

    pub fn alpha(&self) -> &[u8] {
        &self.alpha
    }
}

/// Glyph atlas keyed by complete grapheme clusters.
///
/// A font/rasterization adapter owns glyph creation. Missing graphemes use an
/// explicit fallback bitmap; they never cause a panic or change cell advance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphAtlas {
    glyphs: HashMap<String, GlyphBitmap>,
    fallback: GlyphBitmap,
}

impl GlyphAtlas {
    pub fn new(fallback: GlyphBitmap) -> Self {
        Self {
            glyphs: HashMap::new(),
            fallback,
        }
    }

    pub fn insert(
        &mut self,
        grapheme: &str,
        bitmap: GlyphBitmap,
    ) -> Result<Option<GlyphBitmap>, SoftwareRenderError> {
        if grapheme.is_empty()
            || grapheme.len() > 64
            || grapheme.chars().any(char::is_control)
        {
            return Err(SoftwareRenderError::InvalidGlyphKey);
        }
        Ok(self.glyphs.insert(grapheme.to_owned(), bitmap))
    }

    pub fn get(&self, grapheme: &str) -> &GlyphBitmap {
        self.glyphs.get(grapheme).unwrap_or(&self.fallback)
    }

    pub fn fallback(&self) -> &GlyphBitmap {
        &self.fallback
    }
}

/// Opaque row-major RGBA pixel surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelSurface {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl PixelSurface {
    fn new(width: u32, height: u32, max_pixels: usize) -> Result<Self, SoftwareRenderError> {
        let pixel_count = usize::try_from(width)
            .ok()
            .and_then(|w| {
                usize::try_from(height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            })
            .ok_or(SoftwareRenderError::SurfaceTooLarge)?;
        if pixel_count > max_pixels {
            return Err(SoftwareRenderError::SurfaceTooLarge);
        }
        let byte_count = pixel_count
            .checked_mul(4)
            .ok_or(SoftwareRenderError::SurfaceTooLarge)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(byte_count)
            .map_err(|_| SoftwareRenderError::AllocationFailed)?;
        pixels.resize(byte_count, 0);
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub const fn width(&self) -> u32 {
        self.width
    }

    pub const fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = ((y * self.width + x) * 4) as usize;
        Some(Rgba::new(
            self.pixels[index],
            self.pixels[index + 1],
            self.pixels[index + 2],
            self.pixels[index + 3],
        ))
    }

    fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Rgba) {
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        for row in y.min(self.height)..bottom {
            for column in x.min(self.width)..right {
                self.set_pixel(column, row, color);
            }
        }
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: Rgba) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = ((y * self.width + x) * 4) as usize;
        self.pixels[index] = color.red;
        self.pixels[index + 1] = color.green;
        self.pixels[index + 2] = color.blue;
        self.pixels[index + 3] = color.alpha;
    }

    fn blend_pixel(&mut self, x: u32, y: u32, color: Rgba, coverage: u8) {
        if x >= self.width || y >= self.height || coverage == 0 {
            return;
        }
        let index = ((y * self.width + x) * 4) as usize;
        let source_alpha = (u16::from(color.alpha) * u16::from(coverage) / 255) as u32;
        let inverse_alpha = 255 - source_alpha;
        for (offset, source) in [(0usize, color.red), (1, color.green), (2, color.blue)] {
            let destination = u32::from(self.pixels[index + offset]);
            let blended = (u32::from(source) * source_alpha
                + destination * inverse_alpha
                + 127)
                / 255;
            self.pixels[index + offset] = blended as u8;
        }
        self.pixels[index + 3] = 255;
    }
}

/// Failures while creating or updating the CPU renderer surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftwareRenderError {
    InvalidGlyphBitmap,
    InvalidGlyphKey,
    SurfaceTooLarge,
    AllocationFailed,
    FrameSizeMismatch,
    ClipOutOfBounds,
    Contract(RenderContractError),
}

impl fmt::Display for SoftwareRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidGlyphBitmap => "glyph bitmap dimensions or alpha buffer are invalid",
            Self::InvalidGlyphKey => "glyph atlas key must be a valid non-control grapheme",
            Self::SurfaceTooLarge => "software render surface exceeds the configured pixel limit",
            Self::AllocationFailed => "software render surface allocation failed",
            Self::FrameSizeMismatch => "render frame dimensions do not match renderer dimensions",
            Self::ClipOutOfBounds => "render clip exceeds terminal cell bounds",
            Self::Contract(error) => return write!(formatter, "render contract error: {error}"),
        };
        formatter.write_str(message)
    }
}

impl Error for SoftwareRenderError {}

impl From<RenderContractError> for SoftwareRenderError {
    fn from(value: RenderContractError) -> Self {
        Self::Contract(value)
    }
}

/// Deterministic CPU renderer used for headless golden tests and as a reference
/// implementation for future GPU/window backends.
#[derive(Debug, Clone)]
pub struct SoftwareRenderer {
    metrics: GlyphMetrics,
    palette: RenderPalette,
    atlas: GlyphAtlas,
    max_pixels: usize,
    size: TerminalSize,
    scale_factor: ScaleFactor,
    cell_width: u32,
    cell_height: u32,
    surface: PixelSurface,
    cursor_visible: bool,
    blink_visible: bool,
    force_full_redraw: bool,
}

impl SoftwareRenderer {
    pub fn new(
        size: TerminalSize,
        metrics: GlyphMetrics,
        palette: RenderPalette,
        atlas: GlyphAtlas,
        max_pixels: usize,
    ) -> Result<Self, SoftwareRenderError> {
        let scale_factor = ScaleFactor::new(1.0)?;
        let mut renderer = Self {
            metrics,
            palette,
            atlas,
            max_pixels,
            size,
            scale_factor,
            cell_width: 1,
            cell_height: 1,
            surface: PixelSurface::new(1, 1, max_pixels)?,
            cursor_visible: true,
            blink_visible: true,
            force_full_redraw: true,
        };
        renderer.resize(size, scale_factor)?;
        Ok(renderer)
    }

    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    pub const fn cell_pixel_size(&self) -> (u32, u32) {
        (self.cell_width, self.cell_height)
    }

    pub fn surface(&self) -> &PixelSurface {
        &self.surface
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        if self.cursor_visible != visible {
            self.cursor_visible = visible;
            self.force_full_redraw = true;
        }
    }

    pub fn set_blink_visible(&mut self, visible: bool) {
        if self.blink_visible != visible {
            self.blink_visible = visible;
            self.force_full_redraw = true;
        }
    }

    fn render_region(
        &mut self,
        frame: Frame<'_>,
        region: CellRect,
    ) {
        for row in region.top()..region.bottom_exclusive() as u16 {
            let cells = frame.row_cells(row).expect("validated frame row");
            for column in region.left()..region.right_exclusive() as u16 {
                self.render_cell(frame, row, column, cells[usize::from(column)]);
            }
        }
    }

    fn render_cell(
        &mut self,
        frame: Frame<'_>,
        row: u16,
        column: u16,
        cell: crate::screen::Cell,
    ) {
        let attributes = cell.attributes();
        let styles = attributes.styles();
        let mut foreground = resolve_color(attributes.foreground(), self.palette.default_foreground);
        let mut background = resolve_color(attributes.background(), self.palette.default_background);

        if styles.contains(StyleFlags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }

        if frame
            .selection()
            .is_some_and(|selection| selection.contains(row, column, frame.size()))
        {
            foreground = self.palette.selection_foreground;
            background = self.palette.selection_background;
        }

        let is_cursor = self.cursor_visible
            && frame.cursor().row() == row
            && frame.cursor().column() == column;
        if is_cursor {
            foreground = self.palette.cursor_foreground;
            background = self.palette.cursor_background;
        }

        let x = u32::from(column) * self.cell_width;
        let y = u32::from(row) * self.cell_height;
        self.surface
            .fill_rect(x, y, self.cell_width, self.cell_height, background);

        let CellContent::Grapheme { value, width } = cell.content() else {
            return;
        };
        if styles.contains(StyleFlags::HIDDEN)
            || (styles.contains(StyleFlags::BLINK) && !self.blink_visible)
        {
            return;
        }

        if styles.contains(StyleFlags::DIM) {
            foreground = foreground.dimmed();
        }

        let span = if width == GraphemeWidth::Wide { 2u32 } else { 1u32 };
        let available_width = self.cell_width.saturating_mul(span);
        let bitmap = self.atlas.get(value.as_str()).clone();
        self.draw_glyph(
            &bitmap,
            x,
            y,
            available_width,
            foreground,
            styles.contains(StyleFlags::BOLD),
            styles.contains(StyleFlags::ITALIC),
        );

        if styles.contains(StyleFlags::UNDERLINE) {
            let line_y = y + self.cell_height.saturating_sub(1);
            self.surface
                .fill_rect(x, line_y, available_width, 1, foreground);
        }
        if styles.contains(StyleFlags::STRIKETHROUGH) {
            let line_y = y + self.cell_height / 2;
            self.surface
                .fill_rect(x, line_y, available_width, 1, foreground);
        }
    }

    fn draw_glyph(
        &mut self,
        bitmap: &GlyphBitmap,
        cell_x: u32,
        cell_y: u32,
        available_width: u32,
        foreground: Rgba,
        bold: bool,
        italic: bool,
    ) {
        let scale = self.scale_factor.get();
        let glyph_width = (f32::from(bitmap.width) * scale).ceil() as u32;
        let glyph_height = (f32::from(bitmap.height) * scale).ceil() as u32;
        let x_offset = available_width.saturating_sub(glyph_width) / 2;
        let y_offset = self.cell_height.saturating_sub(glyph_height) / 2;

        for destination_y in 0..glyph_height.min(self.cell_height) {
            let source_y = ((destination_y as f32) / scale).floor() as u32;
            if source_y >= u32::from(bitmap.height) {
                continue;
            }
            let italic_offset = if italic {
                (glyph_height - destination_y) / 4
            } else {
                0
            };
            for destination_x in 0..glyph_width {
                let source_x = ((destination_x as f32) / scale).floor() as u32;
                if source_x >= u32::from(bitmap.width) {
                    continue;
                }
                let alpha_index =
                    (source_y * u32::from(bitmap.width) + source_x) as usize;
                let coverage = bitmap.alpha[alpha_index];
                let x = cell_x + x_offset + destination_x + italic_offset;
                let y = cell_y + y_offset + destination_y;
                self.surface.blend_pixel(x, y, foreground, coverage);
                if bold {
                    self.surface
                        .blend_pixel(x.saturating_add(1), y, foreground, coverage);
                }
            }
        }
    }
}

impl Renderer for SoftwareRenderer {
    type Error = SoftwareRenderError;

    fn resize(
        &mut self,
        size: TerminalSize,
        scale_factor: ScaleFactor,
    ) -> Result<(), Self::Error> {
        let cell_width = (self.metrics.advance_width() * scale_factor.get())
            .ceil()
            .max(1.0) as u32;
        let cell_height = (self.metrics.line_height() * scale_factor.get())
            .ceil()
            .max(1.0) as u32;
        let width = u32::from(size.columns())
            .checked_mul(cell_width)
            .ok_or(SoftwareRenderError::SurfaceTooLarge)?;
        let height = u32::from(size.rows())
            .checked_mul(cell_height)
            .ok_or(SoftwareRenderError::SurfaceTooLarge)?;
        let mut surface = PixelSurface::new(width, height, self.max_pixels)?;
        surface.fill_rect(
            0,
            0,
            width,
            height,
            self.palette.default_background,
        );

        self.size = size;
        self.scale_factor = scale_factor;
        self.cell_width = cell_width;
        self.cell_height = cell_height;
        self.surface = surface;
        self.force_full_redraw = true;
        Ok(())
    }

    fn render(
        &mut self,
        frame: Frame<'_>,
        clip: Option<CellRect>,
    ) -> Result<(), Self::Error> {
        if frame.size() != self.size {
            return Err(SoftwareRenderError::FrameSizeMismatch);
        }
        if clip.is_some_and(|rectangle| !rectangle.is_within(self.size)) {
            return Err(SoftwareRenderError::ClipOutOfBounds);
        }

        let full = self.force_full_redraw || frame.damage().is_full();
        let regions = if full {
            vec![CellRect::new(0, 0, self.size.rows(), self.size.columns())?]
        } else {
            frame.damage().rectangles().to_vec()
        };

        for region in regions {
            let clipped = if let Some(clip) = clip {
                intersect_rect(region, clip)
            } else {
                Some(region)
            };
            if let Some(clipped) = clipped {
                self.render_region(frame, clipped);
            }
        }

        self.force_full_redraw = false;
        Ok(())
    }
}

fn intersect_rect(left: CellRect, right: CellRect) -> Option<CellRect> {
    let top = left.top().max(right.top());
    let column = left.left().max(right.left());
    let bottom = left.bottom_exclusive().min(right.bottom_exclusive());
    let end = left.right_exclusive().min(right.right_exclusive());
    if bottom <= u32::from(top) || end <= u32::from(column) {
        return None;
    }
    CellRect::new(
        top,
        column,
        (bottom - u32::from(top)) as u16,
        (end - u32::from(column)) as u16,
    )
    .ok()
}

fn resolve_color(color: Color, default: Rgba) -> Rgba {
    match color {
        Color::Default => default,
        Color::Rgb { red, green, blue } => Rgba::rgb(red, green, blue),
        Color::Indexed(index) => indexed_color(index),
    }
}

fn indexed_color(index: u8) -> Rgba {
    const BASIC: [Rgba; 16] = [
        Rgba::rgb(0, 0, 0),
        Rgba::rgb(205, 49, 49),
        Rgba::rgb(13, 188, 121),
        Rgba::rgb(229, 229, 16),
        Rgba::rgb(36, 114, 200),
        Rgba::rgb(188, 63, 188),
        Rgba::rgb(17, 168, 205),
        Rgba::rgb(229, 229, 229),
        Rgba::rgb(102, 102, 102),
        Rgba::rgb(241, 76, 76),
        Rgba::rgb(35, 209, 139),
        Rgba::rgb(245, 245, 67),
        Rgba::rgb(59, 142, 234),
        Rgba::rgb(214, 112, 214),
        Rgba::rgb(41, 184, 219),
        Rgba::rgb(255, 255, 255),
    ];

    match index {
        0..=15 => BASIC[usize::from(index)],
        16..=231 => {
            let value = index - 16;
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let red = LEVELS[usize::from(value / 36)];
            let green = LEVELS[usize::from((value % 36) / 6)];
            let blue = LEVELS[usize::from(value % 6)];
            Rgba::rgb(red, green, blue)
        }
        _ => {
            let gray = 8 + 10 * (index - 232);
            Rgba::rgb(gray, gray, gray)
        }
    }
}
