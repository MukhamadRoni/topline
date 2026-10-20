//! Permukaan kaca untuk jendela tambahan (Pengaturan, menu tray), sama seperti notch:
//! DesktopWindowTarget di compositor notch, HostBackdropBrush + tint, dan isi Direct2D di
//! CompositionDrawingSurface berukuran tetap (DIP × scale).

use std::mem::size_of;

use windows::core::*;
use windows::Foundation::Numerics::{Matrix3x2, Vector2, Vector3};
use windows::Foundation::{Size, TimeSpan};
use windows::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::*;

use crate::gfx::{Gfx, Painter};

/// Tint kaca jendela tambahan: lebih pekat dari notch agar teks panjang tetap terbaca.
const TINT_GLASS: Color = Color { A: 196, R: 9, G: 13, B: 34 };
const TINT_SOLID: Color = Color { A: 250, R: 11, G: 22, B: 96 };

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

/// Kelas jendela untuk popup kaca (menu tray, menu konteks); didaftarkan di `app::run`.
pub const POPUP_CLASS: PCWSTR = w!("ToplinePopup");

/// Letak popup terhadap titik jangkar.
#[derive(Clone, Copy)]
pub enum Anchor {
    /// Di atas titik, berpusat horizontal (menu tray di taskbar bawah).
    Above,
    /// Di kanan bawah titik seperti menu konteks; dibalik jika tidak muat.
    Below,
}

/// Buat jendela popup kaca (topmost, tanpa taskbar) berukuran `w` × `h` DIP di dekat `at`,
/// tetap di dalam area kerja monitor. Mengembalikan jendela (belum tampil) dan skala DPI-nya.
pub fn create_popup(at: POINT, w: f32, h: f32, anchor: Anchor) -> Result<(HWND, f32)> {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            POPUP_CLASS,
            w!("Topline"),
            WS_POPUP,
            at.x,
            at.y,
            10,
            10,
            None,
            None,
            hinst,
            None,
        )?;
        prepare_window(hwnd);
        let dpi = GetDpiForWindow(hwnd);
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let (pw, ph) = ((w * scale).round() as i32, (h * scale).round() as i32);
        let mon = MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let r = mi.rcWork;
        let gap = (8.0 * scale) as i32;
        let (x, y) = match anchor {
            Anchor::Above => (
                (at.x - pw / 2).clamp(r.left + gap, (r.right - pw - gap).max(r.left)),
                if at.y - ph - gap >= r.top { at.y - ph - gap } else { (at.y + gap).min(r.bottom - ph) },
            ),
            Anchor::Below => {
                let x = if at.x + pw <= r.right { at.x } else { (at.x - pw).max(r.left) };
                let y = if at.y + 2 + ph <= r.bottom { at.y + 2 } else { (at.y - ph).max(r.top) };
                (x, y)
            }
        };
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, pw, ph, SWP_NOACTIVATE);
        Ok((hwnd, scale))
    }
}

/// Atribut DWM untuk jendela kaca: HostBackdropBrush, mode gelap, sudut membulat, bayangan.
pub fn prepare_window(hwnd: HWND) {
    unsafe {
        let on: BOOL = TRUE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &on as *const _ as *const _, size_of::<BOOL>() as u32);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as *const _, size_of::<BOOL>() as u32);
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as *const _, size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32);
        let m = windows::Win32::UI::Controls::MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 1, cyBottomHeight: 0 };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
    }
}

pub struct Glass {
    compositor: Compositor,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    backdrop: SpriteVisual,
    tint: CompositionColorBrush,
    content: SpriteVisual,
    graphics: CompositionGraphicsDevice,
    surface: CompositionDrawingSurface,
    surface_brush: CompositionSurfaceBrush,
    has_backdrop: bool,
    pub scale: f32,
    /// Ukuran isi (DIP).
    size: (f32, f32),
}

impl Glass {
    pub fn new(compositor: &Compositor, gfx: &Gfx, hwnd: HWND, w: f32, h: f32, scale: f32, solid: bool) -> Result<Self> {
        unsafe {
            let desktop: ICompositorDesktopInterop = compositor.cast()?;
            let target = desktop.CreateDesktopWindowTarget(hwnd, false)?;
            let root = compositor.CreateContainerVisual()?;
            root.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            target.SetRoot(&root)?;

            let backdrop = compositor.CreateSpriteVisual()?;
            backdrop.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            let has_backdrop = match compositor.CreateHostBackdropBrush() {
                Ok(b) => {
                    backdrop.SetBrush(&b)?;
                    true
                }
                Err(_) => false,
            };
            root.Children()?.InsertAtTop(&backdrop)?;
            let tint = compositor.CreateColorBrush()?;
            let tint_v = compositor.CreateSpriteVisual()?;
            tint_v.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            tint_v.SetBrush(&tint)?;
            root.Children()?.InsertAtTop(&tint_v)?;

            let interop: ICompositorInterop = compositor.cast()?;
            let graphics = interop.CreateGraphicsDevice(&gfx.d2d_device)?;
            let surface = graphics.CreateDrawingSurface(
                Size { Width: w * scale, Height: h * scale },
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                DirectXAlphaMode::Premultiplied,
            )?;
            let surface_brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
            let content = compositor.CreateSpriteVisual()?;
            content.SetBrush(&surface_brush)?;
            content.SetSize(v2(w * scale, h * scale))?;
            root.Children()?.InsertAtTop(&content)?;

            let g = Self {
                compositor: compositor.clone(),
                _target: target,
                root,
                backdrop,
                tint,
                content,
                graphics,
                surface,
                surface_brush,
                has_backdrop,
                scale,
                size: (w, h),
            };
            g.set_solid(solid)?;
            Ok(g)
        }
    }

    pub fn set_solid(&self, solid: bool) -> Result<()> {
        let glass = self.has_backdrop && !solid;
        self.backdrop.SetOpacity(if glass { 1.0 } else { 0.0 })?;
        self.tint.SetColor(if glass { TINT_GLASS } else { TINT_SOLID })
    }

    /// DPI berubah: buat ulang surface dengan resolusi baru.
    pub fn set_scale(&mut self, scale: f32) -> Result<()> {
        self.scale = scale;
        let (w, h) = self.size;
        self.surface = self.graphics.CreateDrawingSurface(
            Size { Width: w * scale, Height: h * scale },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        self.surface_brush.SetSurface(&self.surface)?;
        self.content.SetSize(v2(w * scale, h * scale))
    }

    /// Muncul: memudar masuk sambil naik 6 DIP (dijalankan DWM).
    pub fn appear(&self) -> Result<()> {
        let ease = self.compositor.CreateCubicBezierEasingFunction(v2(0.22, 1.0), v2(0.36, 1.0))?;
        let ms = TimeSpan { Duration: 180 * 10_000 };
        let o = self.compositor.CreateScalarKeyFrameAnimation()?;
        o.InsertKeyFrame(0.0, 0.0)?;
        o.InsertKeyFrameWithEasingFunction(1.0, 1.0, &ease)?;
        o.SetDuration(ms)?;
        self.root.StartAnimation(h!("Opacity"), &o)?;
        let y = self.compositor.CreateVector3KeyFrameAnimation()?;
        y.InsertKeyFrame(0.0, Vector3 { X: 0.0, Y: 6.0 * self.scale, Z: 0.0 })?;
        y.InsertKeyFrameWithEasingFunction(1.0, Vector3 { X: 0.0, Y: 0.0, Z: 0.0 }, &ease)?;
        y.SetDuration(ms)?;
        self.content.StartAnimation(h!("Offset"), &y)
    }

    /// Gambar ulang seluruh isi. `f` menerima Painter dalam satuan DIP.
    pub fn draw(&self, gfx: &Gfx, f: impl FnOnce(&Painter)) -> Result<()> {
        let interop: ICompositionDrawingSurfaceInterop = self.surface.cast()?;
        let mut offset = POINT::default();
        unsafe {
            let ctx: ID2D1DeviceContext = interop.BeginDraw(None, &mut offset)?;
            let dpi = 96.0 * self.scale;
            ctx.SetDpi(dpi, dpi);
            ctx.SetTransform(&Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: offset.x as f32 / self.scale,
                M32: offset.y as f32 / self.scale,
            });
            ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            ctx.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            f(&Painter { ctx: &ctx, gfx });
            interop.EndDraw()
        }
    }
}
