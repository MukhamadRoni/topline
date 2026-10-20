//! Pohon visual Windows.UI.Composition untuk notch:
//!
//! ```text
//! root
//!  └─ container  (AnchorPoint 0.5,0 · Size dianimasikan · clip rounded-rect)
//!      ├─ backdrop  HostBackdropBrush (blur desktop asli dari DWM)
//!      ├─ tint      rgba(8,12,30,.45)
//!      ├─ content   drawing surface (Direct2D), tetap 640x540 DIP, dipusatkan
//!      ├─ eq        4 batang equalizer, dianimasikan oleh DWM (0% CPU aplikasi)
//!      └─ eyes      mata Personality (lihat `Eyes`): kedip, ekspresi, lirikan, semua oleh DWM
//! ```
//! Satuan visual di DesktopWindowTarget = piksel fisik, jadi semua ukuran DIP
//! dikalikan `scale`.

use windows::core::*;
use windows::Foundation::Numerics::{Vector2, Vector3};
use windows::Foundation::{Size, TimeSpan};
use windows::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::System::WinRT::Composition::*;
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::*;

use topline_core::EyeMood;

use crate::gfx::{Gfx, Painter};
use crate::theme::*;

fn ts(ms: u64) -> TimeSpan {
    TimeSpan { Duration: ms as i64 * 10_000 }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

pub struct Notch {
    compositor: Compositor,
    _target: DesktopWindowTarget,
    _root: ContainerVisual,
    container: ContainerVisual,
    clip_geo: CompositionRoundedRectangleGeometry,
    tint_brush: CompositionColorBrush,
    backdrop: SpriteVisual,
    content: SpriteVisual,
    surface_brush: CompositionSurfaceBrush,
    graphics: CompositionGraphicsDevice,
    surface: CompositionDrawingSurface,
    eq: ContainerVisual,
    eq_bars: Vec<(SpriteVisual, CompositionColorBrush)>,
    eyes: Eyes,
    easing: CubicBezierEasingFunction,
    pub scale: f32,
    pub has_backdrop: bool,
    cur: (f32, f32, f32),
}

impl Notch {
    pub fn new(hwnd: HWND, gfx: &Gfx, scale: f32) -> Result<Self> {
        let compositor = Compositor::new()?;
        let desktop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { desktop.CreateDesktopWindowTarget(hwnd, true)? };
        let root = compositor.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        target.SetRoot(&root)?;

        let container = compositor.CreateContainerVisual()?;
        container.SetAnchorPoint(v2(0.5, 0.0))?;
        root.Children()?.InsertAtTop(&container)?;

        let clip_geo = compositor.CreateRoundedRectangleGeometry()?;
        let clip = compositor.CreateGeometricClipWithGeometry(&clip_geo)?;
        container.SetClip(&clip)?;

        // Kaca: blur desktop di belakang jendela (butuh DWMWA_USE_HOSTBACKDROPBRUSH).
        let backdrop = compositor.CreateSpriteVisual()?;
        backdrop.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        let has_backdrop = match compositor.CreateHostBackdropBrush() {
            Ok(b) => {
                backdrop.SetBrush(&b)?;
                true
            }
            Err(_) => false,
        };
        container.Children()?.InsertAtTop(&backdrop)?;

        let (r, g, b, a) = if has_backdrop { NOTCH_TINT } else { NOTCH_TINT_SOLID };
        let tint_brush = compositor.CreateColorBrushWithColor(Color { A: a, R: r, G: g, B: b })?;
        let tint = compositor.CreateSpriteVisual()?;
        tint.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        tint.SetBrush(&tint_brush)?;
        container.Children()?.InsertAtTop(&tint)?;

        // Konten Direct2D
        let interop: ICompositorInterop = compositor.cast()?;
        let graphics = unsafe { interop.CreateGraphicsDevice(&gfx.d2d_device)? };
        let surface = graphics.CreateDrawingSurface(
            Size { Width: WIN_W * scale, Height: WIN_H * scale },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let surface_brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        surface_brush.SetStretch(CompositionStretch::Fill)?;
        let content = compositor.CreateSpriteVisual()?;
        content.SetBrush(&surface_brush)?;
        content.SetAnchorPoint(v2(0.5, 0.0))?;
        container.Children()?.InsertAtTop(&content)?;
        let expr = compositor.CreateExpressionAnimationWithExpression(h!("Vector3(host.Size.X * 0.5, 0, 0)"))?;
        expr.SetReferenceParameter(h!("host"), &container)?;
        content.StartAnimation(h!("Offset"), &expr)?;

        // Equalizer (hanya tampil saat idle + musik diputar)
        let eq = compositor.CreateContainerVisual()?;
        eq.SetOpacity(0.0)?;
        container.Children()?.InsertAtTop(&eq)?;
        let mut eq_bars = Vec::new();
        for i in 0..4 {
            let brush = compositor.CreateColorBrushWithColor(Color { A: 255, R: 0x40, G: 0xA9, B: 0xFD })?;
            let bar = compositor.CreateSpriteVisual()?;
            bar.SetBrush(&brush)?;
            eq.Children()?.InsertAtTop(&bar)?;
            let anim = compositor.CreateScalarKeyFrameAnimation()?;
            let lin = compositor.CreateLinearEasingFunction()?;
            anim.InsertKeyFrame(0.0, 0.35)?;
            anim.InsertKeyFrameWithEasingFunction(0.5, 1.0, &lin)?;
            anim.InsertKeyFrameWithEasingFunction(1.0, 0.35, &lin)?;
            anim.SetDuration(ts([900, 700, 1100, 800][i]))?;
            anim.SetDelayTime(ts([0, 150, 300, 50][i]))?;
            anim.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
            bar.StartAnimation(h!("Scale.Y"), &anim)?;
            eq_bars.push((bar, brush));
        }

        let easing = compositor.CreateCubicBezierEasingFunction(v2(0.22, 1.0), v2(0.36, 1.0))?;
        let eyes = Eyes::new(&compositor, &container, &easing)?;

        let mut n = Self {
            compositor,
            _target: target,
            _root: root,
            container,
            clip_geo,
            tint_brush,
            backdrop,
            content,
            surface_brush,
            graphics,
            surface,
            eq,
            eq_bars,
            eyes,
            easing,
            scale,
            has_backdrop,
            cur: (0.0, 0.0, 0.0),
        };
        n.layout_static()?;
        n.set_size(IDLE_W_SMALL, IDLE_H, IDLE_R, false)?;
        Ok(n)
    }

    fn layout_static(&mut self) -> Result<()> {
        let s = self.scale;
        self.container.SetOffset(Vector3 { X: WIN_W * s / 2.0, Y: 0.0, Z: 0.0 })?;
        self.content.SetSize(v2(WIN_W * s, WIN_H * s))?;
        // eq: di kanan pill idle lebar (208): pusat + 104 - 14 - 18
        let expr = self
            .compositor
            .CreateExpressionAnimationWithExpression(&HSTRING::from(format!(
                "Vector3(host.Size.X * 0.5 + {}, {}, 0)",
                (IDLE_W_WIDE / 2.0 - 14.0 - 18.0) * s,
                11.0 * s
            )))?;
        expr.SetReferenceParameter(h!("host"), &self.container)?;
        self.eq.StartAnimation(h!("Offset"), &expr)?;
        for (i, (bar, _)) in self.eq_bars.iter().enumerate() {
            bar.SetSize(v2(3.0 * s, 13.0 * s))?;
            bar.SetOffset(Vector3 { X: i as f32 * 5.0 * s, Y: 0.0, Z: 0.0 })?;
            bar.SetCenterPoint(Vector3 { X: 1.5 * s, Y: 13.0 * s, Z: 0.0 })?;
        }
        self.eyes.layout(&self.container, s)
    }

    /// Dipanggil saat DPI berubah.
    pub fn set_scale(&mut self, scale: f32) -> Result<()> {
        self.scale = scale;
        self.surface = self.graphics.CreateDrawingSurface(
            Size { Width: WIN_W * scale, Height: WIN_H * scale },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        self.surface_brush.SetSurface(&self.surface)?;
        self.layout_static()?;
        let (w, h, r) = self.cur;
        self.cur = (0.0, 0.0, 0.0);
        self.set_size(w, h, r, false)
    }

    pub fn current(&self) -> (f32, f32, f32) {
        self.cur
    }

    /// Ubah ukuran notch (DIP). Dengan `animate`, transisi dijalankan oleh DWM.
    pub fn set_size(&mut self, w: f32, h: f32, r: f32, animate: bool) -> Result<()> {
        if (w, h, r) == self.cur {
            return Ok(());
        }
        let s = self.scale;
        let (pw, ph, pr) = (w * s, h * s, r * s);
        // Rounded rect dibuat lebih tinggi dan digeser ke atas agar sudut atas tetap siku.
        let clip_size = v2(pw, ph + pr);
        if animate && self.cur.0 > 0.0 {
            let a = self.compositor.CreateVector2KeyFrameAnimation()?;
            a.InsertKeyFrameWithEasingFunction(1.0, v2(pw, ph), &self.easing)?;
            a.SetDuration(ts(ANIM_MS))?;
            self.container.StartAnimation(h!("Size"), &a)?;

            let c = self.compositor.CreateVector2KeyFrameAnimation()?;
            c.InsertKeyFrameWithEasingFunction(1.0, clip_size, &self.easing)?;
            c.SetDuration(ts(ANIM_MS))?;
            self.clip_geo.StartAnimation(h!("Size"), &c)?;

            let rr = self.compositor.CreateVector2KeyFrameAnimation()?;
            rr.InsertKeyFrameWithEasingFunction(1.0, v2(pr, pr), &self.easing)?;
            rr.SetDuration(ts(ANIM_MS))?;
            self.clip_geo.StartAnimation(h!("CornerRadius"), &rr)?;

            let o = self.compositor.CreateVector2KeyFrameAnimation()?;
            o.InsertKeyFrameWithEasingFunction(1.0, v2(0.0, -pr), &self.easing)?;
            o.SetDuration(ts(ANIM_MS))?;
            self.clip_geo.StartAnimation(h!("Offset"), &o)?;
        } else {
            self.container.StopAnimation(h!("Size"))?;
            self.clip_geo.StopAnimation(h!("Size"))?;
            self.clip_geo.StopAnimation(h!("CornerRadius"))?;
            self.clip_geo.StopAnimation(h!("Offset"))?;
            self.container.SetSize(v2(pw, ph))?;
            self.clip_geo.SetSize(clip_size)?;
            self.clip_geo.SetCornerRadius(v2(pr, pr))?;
            self.clip_geo.SetOffset(v2(0.0, -pr))?;
        }
        self.cur = (w, h, r);
        Ok(())
    }

    /// Konten memudar masuk setelah ganti tampilan.
    pub fn fade_in_content(&self) -> Result<()> {
        let a = self.compositor.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrame(0.0, 0.0)?;
        a.InsertKeyFrameWithEasingFunction(1.0, 1.0, &self.easing)?;
        a.SetDuration(ts(260))?;
        a.SetDelayTime(ts(60))?;
        a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
        self.content.StartAnimation(h!("Opacity"), &a)
    }

    pub fn show_eq(&self, on: bool, accent: Color) -> Result<()> {
        for (_, b) in &self.eq_bars {
            b.SetColor(accent)?;
        }
        self.eq.SetOpacity(if on { 1.0 } else { 0.0 })
    }

    /// Tampilkan mata dengan ekspresi `mood` (None = sembunyikan). `motion` = efek
    /// animasi Windows aktif; jika mati, mata statis tanpa loop.
    pub fn set_eyes(&mut self, mood: Option<EyeMood>, motion: bool) -> Result<()> {
        self.eyes.set(mood, motion, self.scale)
    }

    /// Mata melirik (DIP, sudah dibatasi pemanggil). (0, 0) = kembali ke tengah.
    pub fn eyes_look(&mut self, dx: f32, dy: f32) -> Result<()> {
        self.eyes.look(dx, dy, self.scale)
    }

    pub fn set_solid(&self, solid: bool) -> Result<()> {
        let (r, g, b, a) = if solid || !self.has_backdrop { NOTCH_TINT_SOLID } else { NOTCH_TINT };
        self.backdrop.SetOpacity(if solid { 0.0 } else { 1.0 })?;
        self.tint_brush.SetColor(Color { A: a, R: r, G: g, B: b })
    }

    /// Gambar ulang seluruh konten. `f` menerima Painter dalam satuan DIP.
    pub fn draw(&self, gfx: &Gfx, f: impl FnOnce(&Painter)) -> Result<()> {
        let interop: ICompositionDrawingSurfaceInterop = self.surface.cast()?;
        let mut offset = POINT::default();
        unsafe {
            let ctx: ID2D1DeviceContext = interop.BeginDraw(None, &mut offset)?;
            let dpi = 96.0 * self.scale;
            ctx.SetDpi(dpi, dpi);
            ctx.SetTransform(&windows::Foundation::Numerics::Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: offset.x as f32 / self.scale,
                M32: offset.y as f32 / self.scale,
            });
            ctx.SetTextAntialiasMode(windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            ctx.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let p = Painter { ctx: &ctx, gfx };
            f(&p);
            interop.EndDraw()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- mata

/// Jarak pusat mata ke tengah notch (jarak pusat ke pusat 18 DIP).
const EYE_DX: f32 = 9.0;
/// Pusat vertikal mata = tengah pill idle.
const EYE_CY: f32 = IDLE_H / 2.0;
/// Kotak ShapeVisual per mata; bentuk mata dipusatkan di dalamnya.
const EYE_BOX: f32 = 24.0;
/// Batang "^" untuk Happy: panjang, tebal, sudut.
const ARM_LEN: f32 = 8.0;
const ARM_W: f32 = 2.4;
const ARM_DEG: f32 = 35.0;
const MOOD_MS: u64 = 320;
const SLEEP_MS: u64 = 800;

const C_ICE: Color = Color { A: 255, R: 0xF2, G: 0xF6, B: 0xFF };
const C_SKY: Color = Color { A: 255, R: 0x40, G: 0xA9, B: 0xFD };
const C_AMBER: Color = Color { A: 255, R: 0xF5, G: 0xB5, B: 0x44 };

const ZERO3: Vector3 = Vector3 { X: 0.0, Y: 0.0, Z: 0.0 };
const ONE3: Vector3 = Vector3 { X: 1.0, Y: 1.0, Z: 1.0 };

/// Bentuk satu mata (DIP). `arms`: dua batang membentuk "^".
#[derive(Clone, Copy)]
struct Pose {
    w: f32,
    h: f32,
    r: f32,
    dy: f32,
    arms: bool,
}

fn pose(m: EyeMood) -> Pose {
    match m {
        EyeMood::Happy => Pose { w: ARM_W, h: ARM_LEN, r: ARM_W / 2.0, dy: 0.5, arms: true },
        EyeMood::Sleeping => Pose { w: 9.0, h: 2.0, r: 1.0, dy: 2.0, arms: false },
        // kelopak turun: lebih pendek, sisi bawah tetap di tempat
        EyeMood::LowBattery => Pose { w: 8.0, h: 8.0, r: 3.0, dy: 2.5, arms: false },
        EyeMood::Focus => Pose { w: 9.0, h: 7.0, r: 3.0, dy: 1.0, arms: false },
        EyeMood::Greeting | EyeMood::Music | EyeMood::Normal => Pose { w: 8.0, h: 13.0, r: 4.0, dy: 0.0, arms: false },
    }
}

fn eye_color(m: EyeMood) -> Color {
    match m {
        EyeMood::Focus => C_SKY,
        EyeMood::LowBattery => C_AMBER,
        _ => C_ICE,
    }
}

fn obj<T: Interface>(o: &T) -> Result<CompositionObject> {
    o.cast()
}

type Shape = (CompositionSpriteShape, CompositionRoundedRectangleGeometry);

struct Eye {
    visual: ShapeVisual,
    /// Dua bentuk per mata. Normalnya saling menumpuk (terlihat satu); saat Happy
    /// keduanya diputar ±35° menjadi "^", sehingga transisinya mulus.
    arms: [Shape; 2],
}

/// Mata Personality sebagai visual Composition:
///
/// ```text
/// root (Offset = tengah atas container + EYE_CY, Opacity)
///  ├─ look    lirikan ke arah kursor (Offset)
///  │   └─ sway   ayunan musik, pantulan Happy/Greeting (Offset, Scale)
///  │       ├─ mata kiri  ShapeVisual (Scale.Y = kedip/napas), 2 sprite shape
///  │       └─ mata kanan ShapeVisual
///  └─ z ×3    huruf "z" saat tidur, naik dan memudar
/// ```
/// Semua gerakan berupa animasi keyframe yang dijalankan DWM; aplikasi hanya
/// bekerja saat ekspresi berganti.
struct Eyes {
    c: Compositor,
    easing: CubicBezierEasingFunction,
    root: ContainerVisual,
    look: ContainerVisual,
    sway: ContainerVisual,
    eyes: [Eye; 2],
    brush: CompositionColorBrush,
    zs: Vec<(ShapeVisual, [Shape; 3])>,
    cur: Option<(EyeMood, bool)>,
    looking: (f32, f32),
}

impl Eyes {
    fn new(c: &Compositor, parent: &ContainerVisual, easing: &CubicBezierEasingFunction) -> Result<Self> {
        let root = c.CreateContainerVisual()?;
        root.SetOpacity(0.0)?;
        parent.Children()?.InsertAtTop(&root)?;
        let look = c.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&look)?;
        let sway = c.CreateContainerVisual()?;
        look.Children()?.InsertAtTop(&sway)?;
        let brush = c.CreateColorBrushWithColor(C_ICE)?;

        let shape = |b: &CompositionColorBrush| -> Result<Shape> {
            let geo = c.CreateRoundedRectangleGeometry()?;
            let sp = c.CreateSpriteShapeWithGeometry(&geo)?;
            sp.SetFillBrush(b)?;
            Ok((sp, geo))
        };
        let mut eyes = Vec::new();
        for _ in 0..2 {
            let visual = c.CreateShapeVisual()?;
            let arms = [shape(&brush)?, shape(&brush)?];
            for (sp, _) in &arms {
                visual.Shapes()?.Append(sp)?;
            }
            sway.Children()?.InsertAtTop(&visual)?;
            eyes.push(Eye { visual, arms });
        }
        let Ok(eyes) = <[Eye; 2]>::try_from(eyes) else { unreachable!() };

        let z_brush = c.CreateColorBrushWithColor(Color { A: 220, ..C_ICE })?;
        let mut zs = Vec::new();
        for _ in 0..3 {
            let v = c.CreateShapeVisual()?;
            v.SetOpacity(0.0)?;
            let parts = [shape(&z_brush)?, shape(&z_brush)?, shape(&z_brush)?];
            for (sp, _) in &parts {
                v.Shapes()?.Append(sp)?;
            }
            root.Children()?.InsertAtTop(&v)?;
            zs.push((v, parts));
        }

        Ok(Self {
            c: c.clone(),
            easing: easing.clone(),
            root,
            look,
            sway,
            eyes,
            brush,
            zs,
            cur: None,
            looking: (0.0, 0.0),
        })
    }

    /// Ukuran statis (piksel fisik). Dipanggil saat awal dan saat DPI berubah.
    fn layout(&mut self, host: &ContainerVisual, s: f32) -> Result<()> {
        let expr = self
            .c
            .CreateExpressionAnimationWithExpression(&HSTRING::from(format!("Vector3(host.Size.X * 0.5, {}, 0)", EYE_CY * s)))?;
        expr.SetReferenceParameter(h!("host"), host)?;
        self.root.StartAnimation(h!("Offset"), &expr)?;
        let half = EYE_BOX / 2.0;
        for (i, eye) in self.eyes.iter().enumerate() {
            let dx = if i == 0 { -EYE_DX } else { EYE_DX };
            eye.visual.SetSize(v2(EYE_BOX * s, EYE_BOX * s))?;
            eye.visual.SetOffset(Vector3 { X: (dx - half) * s, Y: -half * s, Z: 0.0 })?;
            eye.visual.SetCenterPoint(Vector3 { X: half * s, Y: half * s, Z: 0.0 })?;
        }
        // "z" 6x6 DIP: garis atas, diagonal, garis bawah.
        let t = 1.3;
        let spec = [(3.0, t / 2.0, 6.0, 0.0), (3.0, 3.0, 7.2, -45.0), (3.0, 6.0 - t / 2.0, 6.0, 0.0)];
        for (v, parts) in &self.zs {
            v.SetSize(v2(8.0 * s, 8.0 * s))?;
            for ((sp, geo), (cx, cy, len, deg)) in parts.iter().zip(spec) {
                geo.SetSize(v2(len * s, t * s))?;
                geo.SetCornerRadius(v2(t / 2.0 * s, t / 2.0 * s))?;
                geo.SetOffset(v2(-len / 2.0 * s, -t / 2.0 * s))?;
                sp.SetOffset(v2((cx + 1.0) * s, (cy + 1.0) * s))?;
                sp.SetRotationAngleInDegrees(deg)?;
            }
        }
        // Terapkan ulang ekspresi saat ini dengan ukuran baru, tanpa transisi.
        if let Some((m, motion)) = self.cur {
            self.apply(m, motion, s, false)?;
        }
        Ok(())
    }

    fn set(&mut self, mood: Option<EyeMood>, motion: bool, s: f32) -> Result<()> {
        let next = mood.map(|m| (m, motion));
        if next == self.cur {
            return Ok(());
        }
        let was_visible = self.cur.is_some();
        self.cur = next;
        let Some(m) = mood else {
            self.root.StopAnimation(h!("Opacity"))?;
            self.root.SetOpacity(0.0)?;
            self.stop_loops(false)?;
            return self.look(0.0, 0.0, s);
        };
        self.apply(m, motion, s, was_visible && motion)?;
        if !was_visible {
            if motion {
                let a = self.c.CreateScalarKeyFrameAnimation()?;
                a.InsertKeyFrame(0.0, 0.0)?;
                a.InsertKeyFrameWithEasingFunction(1.0, 1.0, &self.easing)?;
                a.SetDuration(ts(260))?;
                a.SetDelayTime(ts(60))?;
                a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
                self.root.StartAnimation(h!("Opacity"), &a)?;
            } else {
                self.root.SetOpacity(1.0)?;
            }
        }
        if m == EyeMood::Sleeping || !motion {
            self.look(0.0, 0.0, s)?;
        }
        Ok(())
    }

    /// Lirikan ke arah kursor. Hanya saat mata tampil, bangun, dan animasi aktif.
    fn look(&mut self, dx: f32, dy: f32, s: f32) -> Result<()> {
        let ok = matches!(self.cur, Some((m, true)) if m != EyeMood::Sleeping);
        let (dx, dy) = if ok { (dx, dy) } else { (0.0, 0.0) };
        if (dx, dy) == self.looking {
            return Ok(());
        }
        self.looking = (dx, dy);
        self.to_v3(&self.look, "Offset", Vector3 { X: dx * s, Y: dy * s, Z: 0.0 }, if ok { 140 } else { 1 })
    }

    // ---- animasi

    fn to_v2(&self, o: &impl Interface, name: &str, v: Vector2, ms: u64) -> Result<()> {
        let a = self.c.CreateVector2KeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(1.0, v, &self.easing)?;
        a.SetDuration(ts(ms.max(1)))?;
        obj(o)?.StartAnimation(&HSTRING::from(name), &a)
    }

    fn to_v3(&self, o: &impl Interface, name: &str, v: Vector3, ms: u64) -> Result<()> {
        let a = self.c.CreateVector3KeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(1.0, v, &self.easing)?;
        a.SetDuration(ts(ms.max(1)))?;
        obj(o)?.StartAnimation(&HSTRING::from(name), &a)
    }

    fn to_f(&self, o: &impl Interface, name: &str, v: f32, ms: u64) -> Result<()> {
        let a = self.c.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(1.0, v, &self.easing)?;
        a.SetDuration(ts(ms.max(1)))?;
        obj(o)?.StartAnimation(&HSTRING::from(name), &a)
    }

    fn ease_in_out(&self) -> Result<CubicBezierEasingFunction> {
        self.c.CreateCubicBezierEasingFunction(v2(0.45, 0.0), v2(0.55, 1.0))
    }

    /// Kedip pada Scale.Y: `blinks` = posisi (0..1) di timeline `total` ms,
    /// tiap kedip 1 -> 0,1 -> 1 dalam `blink_ms`.
    fn blink(&self, total: u64, blinks: &[f32], blink_ms: u64, forever: bool) -> Result<ScalarKeyFrameAnimation> {
        let a = self.c.CreateScalarKeyFrameAnimation()?;
        let lin = self.c.CreateLinearEasingFunction()?;
        let f = blink_ms as f32 / total as f32;
        a.InsertKeyFrame(0.0, 1.0)?;
        for &p in blinks {
            a.InsertKeyFrameWithEasingFunction(p, 1.0, &lin)?;
            a.InsertKeyFrameWithEasingFunction(p + f / 2.0, 0.1, &lin)?;
            a.InsertKeyFrameWithEasingFunction(p + f, 1.0, &lin)?;
        }
        a.InsertKeyFrameWithEasingFunction(1.0, 1.0, &lin)?;
        a.SetDuration(ts(total))?;
        if forever {
            a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        }
        Ok(a)
    }

    /// Satu animasi yang sama untuk kedua mata agar serempak.
    fn eyes_scale_y(&self, a: &ScalarKeyFrameAnimation) -> Result<()> {
        for e in &self.eyes {
            e.visual.StartAnimation(h!("Scale.Y"), a)?;
        }
        Ok(())
    }

    /// Pantulan kecil ke atas sekali: naik 2 DIP pada `up`, kembali pada `back`.
    fn bounce(&self, s: f32, total: u64, up: f32, back: f32) -> Result<Vector3KeyFrameAnimation> {
        let a = self.c.CreateVector3KeyFrameAnimation()?;
        a.InsertKeyFrame(0.0, ZERO3)?;
        a.InsertKeyFrameWithEasingFunction(up, Vector3 { X: 0.0, Y: -2.0 * s, Z: 0.0 }, &self.easing)?;
        a.InsertKeyFrameWithEasingFunction(back, ZERO3, &self.easing)?;
        a.SetDuration(ts(total))?;
        Ok(a)
    }

    /// Hentikan semua loop dan kembalikan ke posisi diam.
    fn stop_loops(&self, smooth: bool) -> Result<()> {
        let ms = if smooth { 250 } else { 1 };
        for e in &self.eyes {
            self.to_v3(&e.visual, "Scale", ONE3, ms)?;
        }
        self.to_v3(&self.sway, "Offset", ZERO3, ms)?;
        self.to_v3(&self.sway, "Scale", ONE3, ms)?;
        for (v, _) in &self.zs {
            v.StopAnimation(h!("Opacity"))?;
            v.StopAnimation(h!("Offset"))?;
            v.SetOpacity(0.0)?;
        }
        Ok(())
    }

    /// Terapkan ekspresi. `smooth`: transisi bentuk 320 ms (800 ms ke tidur);
    /// selain itu langsung.
    fn apply(&self, m: EyeMood, motion: bool, s: f32, smooth: bool) -> Result<()> {
        let ms = match (smooth, m) {
            (false, _) => 1,
            (true, EyeMood::Sleeping) => SLEEP_MS,
            (true, _) => MOOD_MS,
        };
        let p = pose(m);
        let half = EYE_BOX / 2.0;
        let sx = ARM_LEN / 2.0 * ARM_DEG.to_radians().sin();
        for eye in &self.eyes {
            for (i, (sp, geo)) in eye.arms.iter().enumerate() {
                let side = if i == 0 { -1.0 } else { 1.0 };
                let (x, deg) = if p.arms { (half + side * sx, -side * ARM_DEG) } else { (half, 0.0) };
                self.to_v2(geo, "Size", v2(p.w * s, p.h * s), ms)?;
                self.to_v2(geo, "CornerRadius", v2(p.r * s, p.r * s), ms)?;
                self.to_v2(geo, "Offset", v2(-p.w / 2.0 * s, -p.h / 2.0 * s), ms)?;
                self.to_v2(sp, "Offset", v2(x * s, (half + p.dy) * s), ms)?;
                self.to_f(sp, "RotationAngleInDegrees", deg, ms)?;
            }
        }
        let ca = self.c.CreateColorKeyFrameAnimation()?;
        ca.InsertKeyFrameWithEasingFunction(1.0, eye_color(m), &self.easing)?;
        ca.SetDuration(ts(ms))?;
        self.brush.StartAnimation(h!("Color"), &ca)?;

        self.stop_loops(smooth)?;
        if !motion {
            return Ok(());
        }
        match m {
            EyeMood::Normal => self.eyes_scale_y(&self.blink(7300, &[0.42, 0.86, 0.882], 140, true)?)?,
            EyeMood::Music => {
                self.eyes_scale_y(&self.blink(7300, &[0.42, 0.86, 0.882], 140, true)?)?;
                let io = self.ease_in_out()?;
                let x = 1.5 * s;
                let a = self.c.CreateVector3KeyFrameAnimation()?;
                a.InsertKeyFrame(0.0, ZERO3)?;
                a.InsertKeyFrameWithEasingFunction(0.25, Vector3 { X: x, Y: 0.0, Z: 0.0 }, &io)?;
                a.InsertKeyFrameWithEasingFunction(0.75, Vector3 { X: -x, Y: 0.0, Z: 0.0 }, &io)?;
                a.InsertKeyFrameWithEasingFunction(1.0, ZERO3, &io)?;
                a.SetDuration(ts(1200))?;
                a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
                self.sway.StartAnimation(h!("Offset"), &a)?;
            }
            EyeMood::Focus => self.eyes_scale_y(&self.blink(12700, &[0.6], 140, true)?)?,
            EyeMood::LowBattery => self.eyes_scale_y(&self.blink(11300, &[0.5], 320, true)?)?,
            EyeMood::Sleeping => {
                // napas pelan pada garis, mulai setelah mata selesai menutup
                let io = self.ease_in_out()?;
                let a = self.c.CreateScalarKeyFrameAnimation()?;
                a.InsertKeyFrame(0.0, 1.0)?;
                a.InsertKeyFrameWithEasingFunction(0.5, 1.4, &io)?;
                a.InsertKeyFrameWithEasingFunction(1.0, 1.0, &io)?;
                a.SetDuration(ts(4000))?;
                a.SetDelayTime(ts(SLEEP_MS))?;
                a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
                self.eyes_scale_y(&a)?;
                let lin = self.c.CreateLinearEasingFunction()?;
                for (i, (v, _)) in self.zs.iter().enumerate() {
                    let o = self.c.CreateVector3KeyFrameAnimation()?;
                    o.InsertKeyFrame(0.0, Vector3 { X: (EYE_DX + 6.0) * s, Y: -8.0 * s, Z: 0.0 })?;
                    o.InsertKeyFrameWithEasingFunction(1.0, Vector3 { X: (EYE_DX + 12.0) * s, Y: -18.0 * s, Z: 0.0 }, &lin)?;
                    let f = self.c.CreateScalarKeyFrameAnimation()?;
                    f.InsertKeyFrame(0.0, 0.0)?;
                    f.InsertKeyFrameWithEasingFunction(0.25, 0.9, &lin)?;
                    f.InsertKeyFrameWithEasingFunction(1.0, 0.0, &lin)?;
                    for ka in [o.cast::<KeyFrameAnimation>()?, f.cast::<KeyFrameAnimation>()?] {
                        ka.SetDuration(ts(3000))?;
                        ka.SetDelayTime(ts(SLEEP_MS + 1000 * i as u64))?;
                        ka.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
                        ka.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
                    }
                    v.StartAnimation(h!("Offset"), &o)?;
                    v.StartAnimation(h!("Opacity"), &f)?;
                }
            }
            EyeMood::Greeting => {
                let io = self.ease_in_out()?;
                let big = Vector3 { X: 1.15, Y: 1.15, Z: 1.0 };
                let sc = self.c.CreateVector3KeyFrameAnimation()?;
                sc.InsertKeyFrame(0.0, ONE3)?;
                sc.InsertKeyFrameWithEasingFunction(0.25, big, &self.easing)?;
                sc.InsertKeyFrameWithEasingFunction(0.7, big, &io)?;
                sc.InsertKeyFrameWithEasingFunction(1.0, ONE3, &io)?;
                sc.SetDuration(ts(1200))?;
                self.sway.StartAnimation(h!("Scale"), &sc)?;
                self.sway.StartAnimation(h!("Offset"), &self.bounce(s, 1200, 0.15, 0.35)?)?;
                self.eyes_scale_y(&self.blink(1200, &[0.6, 0.76], 120, false)?)?;
            }
            EyeMood::Happy => self.sway.StartAnimation(h!("Offset"), &self.bounce(s, 520, 0.4, 1.0)?)?,
        }
        Ok(())
    }
}
