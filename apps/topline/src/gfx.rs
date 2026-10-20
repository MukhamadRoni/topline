//! Direct3D/Direct2D/DirectWrite: perangkat grafis, font Geist yang dibundel,
//! dan `Painter` untuk menggambar komponen UI dalam satuan DIP.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::*;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

use crate::embedded;
use crate::theme::Rgba;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Font {
    Sans,
    Mono,
    Icons,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Gfx {
    #[allow(dead_code)]
    pub d3d: ID3D11Device,
    pub d2d_factory: ID2D1Factory1,
    pub d2d_device: ID2D1Device,
    /// Konteks untuk membuat resource (bitmap cover) di luar BeginDraw.
    pub res_ctx: ID2D1DeviceContext,
    pub dwrite: IDWriteFactory5,
    fonts: Option<IDWriteFontCollection>,
    pub geist: bool,
    formats: RefCell<HashMap<(u32, u32, Font, Align), IDWriteTextFormat>>,
    wic: IWICImagingFactory,
    round: ID2D1StrokeStyle,
    dashed: ID2D1StrokeStyle,
}

fn create_d3d() -> Result<ID3D11Device> {
    let mut dev: Option<ID3D11Device> = None;
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    let hw = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            None,
        )
    };
    if hw.is_err() {
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                None,
            )?
        };
    }
    dev.ok_or_else(|| Error::from(E_FAIL))
}

fn load_fonts(dwrite: &IDWriteFactory5) -> Result<Option<IDWriteFontCollection>> {
    let datas = [embedded::GEIST_REGULAR, embedded::GEIST_MEDIUM, embedded::GEIST_MONO];
    if datas.iter().all(|d| d.is_empty()) {
        return Ok(None);
    }
    unsafe {
        let loader = dwrite.CreateInMemoryFontFileLoader()?;
        dwrite.RegisterFontFileLoader(&loader)?;
        let f3: IDWriteFactory3 = dwrite.cast()?;
        let builder: IDWriteFontSetBuilder1 = f3.CreateFontSetBuilder()?.cast()?;
        for data in datas {
            if data.is_empty() {
                continue;
            }
            let file = loader.CreateInMemoryFontFileReference(
                dwrite,
                data.as_ptr() as *const c_void,
                data.len() as u32,
                None,
            )?;
            builder.AddFontFile(&file)?;
        }
        let set = builder.CreateFontSet()?;
        let coll = f3.CreateFontCollectionFromFontSet(&set)?;
        Ok(Some(coll.cast()?))
    }
}

impl Gfx {
    pub fn new() -> Result<Self> {
        unsafe {
            let d3d = create_d3d()?;
            let opts = D2D1_FACTORY_OPTIONS::default();
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&opts))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi)?;
            let res_ctx = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let dwrite: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let fonts = load_fonts(&dwrite).unwrap_or(None);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let mut props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let base: ID2D1Factory = d2d_factory.cast()?;
            let round = base.CreateStrokeStyle(&props, None)?;
            props.dashStyle = D2D1_DASH_STYLE_CUSTOM;
            let dashed = base.CreateStrokeStyle(&props, Some(&[2.0, 2.5]))?;
            Ok(Self {
                d3d,
                d2d_factory,
                d2d_device,
                res_ctx,
                geist: fonts.is_some(),
                dwrite,
                fonts,
                formats: RefCell::new(HashMap::new()),
                wic,
                round,
                dashed,
            })
        }
    }

    fn format(&self, size: f32, weight: u32, font: Font, align: Align) -> Result<IDWriteTextFormat> {
        let key = ((size * 100.0) as u32, weight, font, align);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let (family, coll): (HSTRING, Option<&IDWriteFontCollection>) = match font {
            Font::Icons => (h!("Segoe Fluent Icons").clone(), None),
            Font::Mono if self.geist => (h!("Geist Mono").clone(), self.fonts.as_ref()),
            Font::Mono => (h!("Cascadia Mono").clone(), None),
            Font::Sans if self.geist => (h!("Geist").clone(), self.fonts.as_ref()),
            Font::Sans => (h!("Segoe UI Variable Text").clone(), None),
        };
        let f = unsafe {
            let f = self.dwrite.CreateTextFormat(
                &family,
                coll,
                DWRITE_FONT_WEIGHT(weight as i32),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("id-ID"),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            f.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            if font != Font::Icons {
                let sign = self.dwrite.CreateEllipsisTrimmingSign(&f)?;
                let trim = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                f.SetTrimming(&trim, &sign)?;
            }
            f
        };
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// Lebar teks (DIP) untuk ukuran pill dinamis.
    pub fn measure(&self, text: &str, size: f32, weight: u32, font: Font) -> f32 {
        let Ok(fmt) = self.format(size, weight, font, Align::Left) else { return 0.0 };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            match self.dwrite.CreateTextLayout(&wide, &fmt, 4000.0, 100.0) {
                Ok(layout) => {
                    let mut m = DWRITE_TEXT_METRICS::default();
                    if layout.GetMetrics(&mut m).is_ok() {
                        return m.widthIncludingTrailingWhitespace;
                    }
                    0.0
                }
                Err(_) => 0.0,
            }
        }
    }

    /// Decode gambar (PNG/JPEG dari cover album) menjadi bitmap Direct2D.
    pub fn decode_image(&self, bytes: &[u8]) -> Result<ID2D1Bitmap> {
        unsafe {
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let dec = self.wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
            let frame = dec.GetFrame(0)?;
            let conv = self.wic.CreateFormatConverter()?;
            conv.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )?;
            let rt: &ID2D1RenderTarget = &self.res_ctx;
            rt.CreateBitmapFromWicBitmap(&conv, None)
        }
    }

    /// Simpan HBITMAP ikon shell (alpha premultiplied) sebagai PNG.
    pub fn save_png(&self, hbmp: windows::Win32::Graphics::Gdi::HBITMAP, path: &std::path::Path) -> Result<()> {
        unsafe {
            let src = self.wic.CreateBitmapFromHBITMAP(hbmp, windows::Win32::Graphics::Gdi::HPALETTE::default(), WICBitmapUsePremultipliedAlpha)?;
            let conv = self.wic.CreateFormatConverter()?;
            conv.Initialize(&src, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
            let (mut w, mut h) = (0u32, 0u32);
            src.GetSize(&mut w, &mut h)?;
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
            let enc = self.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            enc.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.ok_or_else(|| Error::from(E_FAIL))?;
            frame.Initialize(None)?;
            frame.SetSize(w, h)?;
            let mut fmt = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut fmt)?;
            frame.WriteSource(&conv, std::ptr::null())?;
            frame.Commit()?;
            enc.Commit()
        }
    }
}

pub fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0, g: c.1, b: c.2, a: c.3 }
}

pub fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

/// Menggambar ke `ID2D1DeviceContext` dalam satuan DIP.
pub struct Painter<'a> {
    pub ctx: &'a ID2D1DeviceContext,
    pub gfx: &'a Gfx,
}

impl<'a> Painter<'a> {
    fn brush(&self, c: Rgba) -> Option<ID2D1SolidColorBrush> {
        unsafe { self.ctx.CreateSolidColorBrush(&color(c), None).ok() }
    }

    pub fn fill_rrect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        if let Some(b) = self.brush(c) {
            let rr = D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: r, radiusY: r };
            unsafe { self.ctx.FillRoundedRectangle(&rr, &b) };
        }
    }

    pub fn stroke_rrect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba, width: f32) {
        if let Some(b) = self.brush(c) {
            let rr = D2D1_ROUNDED_RECT {
                rect: rect(x + width / 2.0, y + width / 2.0, w - width, h - width),
                radiusX: r,
                radiusY: r,
            };
            unsafe { self.ctx.DrawRoundedRectangle(&rr, &b, width, None) };
        }
    }

    pub fn stroke_rrect_dashed(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba, width: f32) {
        if let Some(b) = self.brush(c) {
            let rr = D2D1_ROUNDED_RECT {
                rect: rect(x + width / 2.0, y + width / 2.0, w - width, h - width),
                radiusX: r,
                radiusY: r,
            };
            unsafe { self.ctx.DrawRoundedRectangle(&rr, &b, width, &self.gfx.dashed) };
        }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, r: f32, c: Rgba) {
        if let Some(b) = self.brush(c) {
            let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
            unsafe { self.ctx.FillEllipse(&e, &b) };
        }
    }

    pub fn stroke_circle(&self, cx: f32, cy: f32, r: f32, c: Rgba, width: f32, dashed: bool) {
        if let Some(b) = self.brush(c) {
            let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
            let style = if dashed { &self.gfx.dashed } else { &self.gfx.round };
            unsafe { self.ctx.DrawEllipse(&e, &b, width, style) };
        }
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba, width: f32) {
        if let Some(b) = self.brush(c) {
            unsafe { self.ctx.DrawLine(pt(x0, y0), pt(x1, y1), &b, width, &self.gfx.round) };
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(&self, s: &str, x: f32, y: f32, w: f32, h: f32, size: f32, weight: u32, font: Font, align: Align, c: Rgba) {
        if s.is_empty() {
            return;
        }
        let Ok(fmt) = self.gfx.format(size, weight, font, align) else { return };
        let Some(b) = self.brush(c) else { return };
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.ctx.DrawText(
                &wide,
                &fmt,
                &rect(x, y, w, h),
                &b,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    /// Label mono huruf kapital dengan tracking 0.06em (gaya HUD di design.md).
    #[allow(clippy::too_many_arguments)]
    pub fn label(&self, s: &str, x: f32, y: f32, w: f32, h: f32, size: f32, align: Align, c: Rgba) {
        let upper = s.to_uppercase();
        let Ok(fmt) = self.gfx.format(size, 400, Font::Mono, align) else { return };
        let Some(b) = self.brush(c) else { return };
        let wide: Vec<u16> = upper.encode_utf16().collect();
        unsafe {
            if let Ok(layout) = self.gfx.dwrite.CreateTextLayout(&wide, &fmt, w, h) {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let range = DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 };
                    let _ = l1.SetCharacterSpacing(0.0, size * 0.06, 0.0, range);
                }
                self.ctx.DrawTextLayout(pt(x, y), &layout, &b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
            }
        }
    }

    pub fn icon(&self, g: char, cx: f32, cy: f32, size: f32, c: Rgba) {
        let s = g.to_string();
        let box_ = size * 2.0;
        self.text(&s, cx - box_ / 2.0, cy - box_ / 2.0, box_, box_, size, 400, Font::Icons, Align::Center, c);
    }

    /// Busur untuk ring Pomodoro. `frac` 0..1 searah jarum jam dari atas.
    pub fn arc(&self, cx: f32, cy: f32, r: f32, frac: f32, width: f32, c: Rgba) {
        let frac = frac.clamp(0.0, 1.0);
        if frac <= 0.0005 {
            return;
        }
        if frac >= 0.9995 {
            self.stroke_circle(cx, cy, r, c, width, false);
            return;
        }
        let Some(b) = self.brush(c) else { return };
        unsafe {
            let Ok(geo) = self.gfx.d2d_factory.CreatePathGeometry() else { return };
            let Ok(sink) = geo.Open() else { return };
            let a0 = -std::f32::consts::FRAC_PI_2;
            let a1 = a0 + frac * std::f32::consts::TAU;
            sink.BeginFigure(pt(cx + r * a0.cos(), cy + r * a0.sin()), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: pt(cx + r * a1.cos(), cy + r * a1.sin()),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.ctx.DrawGeometry(&geo, &b, width, &self.gfx.round);
        }
    }

    /// Ikon tetes air (path bezier).
    pub fn droplet(&self, cx: f32, cy: f32, size: f32, c: Rgba, filled: bool) {
        let Some(b) = self.brush(c) else { return };
        let s = size;
        unsafe {
            let Ok(geo) = self.gfx.d2d_factory.CreatePathGeometry() else { return };
            let Ok(sink) = geo.Open() else { return };
            let top = pt(cx, cy - s * 0.5);
            sink.BeginFigure(top, if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: pt(cx + s * 0.12, cy - s * 0.28),
                point2: pt(cx + s * 0.36, cy - s * 0.02),
                point3: pt(cx + s * 0.36, cy + s * 0.16),
            });
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: pt(cx + s * 0.36, cy + s * 0.38),
                point2: pt(cx + s * 0.18, cy + s * 0.5),
                point3: pt(cx, cy + s * 0.5),
            });
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: pt(cx - s * 0.18, cy + s * 0.5),
                point2: pt(cx - s * 0.36, cy + s * 0.38),
                point3: pt(cx - s * 0.36, cy + s * 0.16),
            });
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: pt(cx - s * 0.36, cy - s * 0.02),
                point2: pt(cx - s * 0.12, cy - s * 0.28),
                point3: top,
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            if filled {
                self.ctx.FillGeometry(&geo, &b, None);
            } else {
                self.ctx.DrawGeometry(&geo, &b, s * 0.09, &self.gfx.round);
            }
        }
    }

    /// Kotak membulat dengan gradient linier (stops: posisi, warna).
    #[allow(clippy::too_many_arguments)]
    pub fn gradient_rrect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, stops: &[(f32, Rgba)], from: (f32, f32), to: (f32, f32)) {
        let gs: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|(p, c)| D2D1_GRADIENT_STOP { position: *p, color: color(*c) }).collect();
        unsafe {
            let rt: &ID2D1RenderTarget = self.ctx;
            let Ok(coll) = rt.CreateGradientStopCollection(&gs, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: pt(x + from.0 * w, y + from.1 * h),
                endPoint: pt(x + to.0 * w, y + to.1 * h),
            };
            let Ok(b) = rt.CreateLinearGradientBrush(&props, None, &coll) else { return };
            let rr = D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: r, radiusY: r };
            self.ctx.FillRoundedRectangle(&rr, &b);
        }
    }

    /// Gambar bitmap apa adanya ke kotak (ikon launcher), dengan opasitas.
    pub fn bitmap(&self, bmp: &ID2D1Bitmap, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
        unsafe {
            let rt: &ID2D1RenderTarget = self.ctx;
            rt.DrawBitmap(bmp, Some(&rect(x, y, w, h)), opacity, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        }
    }

    /// Gambar bitmap (cover album) di dalam kotak membulat, mode "cover".
    pub fn bitmap_rrect(&self, bmp: &ID2D1Bitmap, x: f32, y: f32, w: f32, h: f32, r: f32) {
        unsafe {
            let size = bmp.GetSize();
            if size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            let scale = (w / size.width).max(h / size.height);
            let dx = x + (w - size.width * scale) / 2.0;
            let dy = y + (h - size.height * scale) / 2.0;
            let rt: &ID2D1RenderTarget = self.ctx;
            let Ok(brush) = rt.CreateBitmapBrush(bmp, None, None) else { return };
            brush.SetInterpolationMode(D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
            brush.SetTransform(&Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: dx, M32: dy });
            let rr = D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: r, radiusY: r };
            self.ctx.FillRoundedRectangle(&rr, &brush);
        }
    }
}
