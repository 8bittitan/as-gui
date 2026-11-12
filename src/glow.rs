//! The composer's "working" glow while a reply streams, after Palermo's composer
//! (palermo.tryprofound.com/docs/composer#accents): three blurred beams of an accent spectrum
//! tucked behind the card's top edge, each scrolling its gradient at its own pace, so only their
//! soft tops show above the card. The slot (`El::effect`) carries the accent and the background
//! the spectrum mixes into; `Glow` paints the beams with its own shader at that slot.

use rune::compose::{Compositor, DeviceKey, Frame};
use rune::render::{CLIP_WGSL, EffectSite, Instance, KIND_EFFECT};

/// The effect slot's tag, so the compositor paints only its own slots.
pub const MARK: u32 = 0x474c_4f57;

/// How far past the card the beams' blur can reach, in points.
pub const BLEED: f32 = 40.0;

/// Where the beams' centre line sits, below the card's top edge, in points (Palermo's
/// `top: 0.58rem`): about half of each beam is behind the card.
pub const BEAM_CENTER: f32 = 9.28;

/// A colour in Oklch: lightness 0..1, chroma, hue in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

fn srgb_decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

pub fn linear_from_srgb8(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|v| srgb_decode(v as f32 / 255.0))
}

pub fn oklch_from_linear([r, g, b]: [f32; 3]) -> Oklch {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let lightness = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    Oklch {
        l: lightness,
        c: a.hypot(bb),
        h: bb.atan2(a).to_degrees().rem_euclid(360.0),
    }
}

pub fn linear_from_oklch(c: Oklch) -> [f32; 3] {
    let (a, b) = (c.c * c.h.to_radians().cos(), c.c * c.h.to_radians().sin());
    let l = (c.l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (c.l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (c.l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// CSS `color-mix(in oklch, accent amount, bg)` with an achromatic background: lightness and
/// chroma mix, and the hue stays the accent's (the background's is powerless).
pub fn mix(accent: Oklch, bg: Oklch, amount: f32) -> Oklch {
    Oklch {
        l: bg.l + (accent.l - bg.l) * amount,
        c: bg.c + (accent.c - bg.c) * amount,
        h: accent.h,
    }
}

/// Palermo's working spectrum: `(position, how much accent)` stops, the rest background.
pub const SPECTRUM: [(f32, f32); 12] = [
    (0.0, 1.0),
    (0.05, 1.0),
    (0.16, 0.72),
    (0.22, 0.28),
    (0.34, 0.90),
    (0.44, 0.18),
    (0.50, 0.0),
    (0.55, 0.40),
    (0.62, 1.0),
    (0.78, 0.55),
    (0.86, 0.12),
    (1.0, 1.0),
];

/// A linear colour as one float of a slot's data: its sRGB bytes, bit for bit.
pub fn pack(rgb: [f32; 3]) -> f32 {
    let [r, g, b] = rgb.map(|v| (srgb_encode(v) * 255.0).round() as u32);
    f32::from_bits(r | g << 8 | b << 16 | 0xff << 24)
}

pub fn unpack(packed: f32) -> [f32; 3] {
    let bits = packed.to_bits();
    linear_from_srgb8([bits as u8, (bits >> 8) as u8, (bits >> 16) as u8])
}

/// What the composer's slot carries: the accent fill and the background it mixes into.
pub fn slot_data(k: &rune_kit::Kit) -> [f32; 4] {
    let (accent, bg) = (k.c.bg_accent, k.c.bg_base);
    [
        pack([accent.r, accent.g, accent.b]),
        pack([bg.r, bg.g, bg.b]),
        0.0,
        0.0,
    ]
}

/// A beam's inset from one side of the card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Inset {
    Points(f32),
    Fraction(f32),
}

impl Inset {
    fn of(self, width: f32) -> f32 {
        match self {
            Inset::Points(p) => p,
            Inset::Fraction(f) => f * width,
        }
    }
}

/// One blurred stripe of the spectrum along the card's top edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beam {
    pub left: Inset,
    pub right: Inset,
    pub height: f32,
    /// Gaussian standard deviation, in points (CSS `blur()`).
    pub blur: f32,
    pub opacity: f32,
    /// Seconds to scroll one whole period of the spectrum.
    pub period: f32,
    /// Where in its loop the beam starts, in seconds (a negative CSS delay).
    pub offset: f32,
}

impl Beam {
    /// The beam's left and right edges, in points from the card's left.
    pub fn span(&self, width: f32) -> (f32, f32) {
        (self.left.of(width), width - self.right.of(width))
    }

    /// How far through its loop the spectrum has scrolled at `time`: 0..1. Reduced motion
    /// holds every beam at the start.
    pub fn phase(&self, time: f64, reduced: bool) -> f32 {
        if reduced {
            return 0.0;
        }
        ((time + self.offset as f64) / self.period as f64).rem_euclid(1.0) as f32
    }
}

/// Palermo's three beams, wide, mid and tight, in their painting order.
pub const BEAMS: [Beam; 3] = [
    Beam {
        left: Inset::Points(4.0),
        right: Inset::Points(4.0),
        height: 11.2,
        blur: 12.0,
        opacity: 0.26,
        period: 6.0,
        offset: 0.0,
    },
    Beam {
        left: Inset::Fraction(0.08),
        right: Inset::Fraction(0.12),
        height: 16.0,
        blur: 10.4,
        opacity: 0.4,
        period: 4.4,
        offset: 1.4,
    },
    Beam {
        left: Inset::Fraction(0.38),
        right: Inset::Fraction(0.07),
        height: 7.2,
        blur: 8.0,
        opacity: 0.36,
        period: 7.0,
        offset: 3.0,
    },
];

const SHADER: &str = r#"
struct Beam { span: vec4f, look: vec4f }
struct U {
    quad: vec4f,
    clip: vec4f,
    fade: vec4<u32>,
    frame: vec4f,
    card: vec4f,
    misc: vec4f,
    beams: array<Beam, 3>,
    stops: array<vec4f, 12>,
}
@group(0) @binding(0) var<uniform> u: U;

struct V { @builtin(position) pos: vec4f }

@vertex fn vs(@builtin(vertex_index) vi: u32) -> V {
    let corner = vec2f(f32(vi & 1u), f32(vi >> 1u));
    let p = (u.quad.xy + corner * u.quad.zw - u.frame.xy) / u.frame.zw;
    return V(vec4f(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0));
}

fn erf(x: f32) -> f32 {
    let a = 0.147;
    let x2 = x * x;
    return sign(x) * sqrt(1.0 - exp(-x2 * (1.2732395 + a * x2) / (1.0 + a * x2)));
}

// Coverage of the box lo..hi blurred by a Gaussian of deviation sigma, at x.
fn blurred_box(x: f32, lo: f32, hi: f32, sigma: f32) -> f32 {
    let k = 0.70710678 / max(sigma, 1e-3);
    return 0.5 * (erf((hi - x) * k) - erf((lo - x) * k));
}

fn decode(c: vec3f) -> vec3f {
    return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

fn encode(c: vec3f) -> vec3f {
    return select(1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3f(0.0031308));
}

// The spectrum at u in 0..1, interpolated between stops in sRGB as CSS gradients are.
fn spectrum(t: f32) -> vec3f {
    var c = u.stops[0].rgb;
    for (var i = 1u; i < 12u; i++) {
        let a = u.stops[i - 1u];
        let b = u.stops[i];
        if (t >= a.w && t <= b.w) {
            c = mix(a.rgb, b.rgb, (t - a.w) / max(b.w - a.w, 1e-5));
        }
    }
    return c;
}

@fragment fn fs(v: V) -> @location(0) vec4f {
    let window = v.pos.xy + u.frame.xy;
    let local = window - u.card.xy;
    let half = u.card.zw * 0.5;
    // Behind an opaque card: nothing shows inside its rounded outline.
    let outside = clamp(sd_rounded(local - half, half, vec4f(u.misc.x)) + 0.5, 0.0, 1.0);
    var out = vec4f(0.0);
    for (var i = 0u; i < 3u; i++) {
        let b = u.beams[i];
        let alpha = b.look.y * blurred_box(local.x, b.span.x, b.span.y, b.look.x)
            * blurred_box(local.y, b.span.z - b.span.w * 0.5, b.span.z + b.span.w * 0.5, b.look.x);
        let period = 2.0 * (b.span.y - b.span.x);
        let along = fract((local.x - b.span.x) / period + b.look.z);
        let rgb = decode(spectrum(along));
        let a = alpha * outside;
        out = vec4f(rgb * a, a) + out * (1.0 - a);
    }
    out *= clip_alpha(window, u.clip, u.fade);
    if (u.misc.z > 0.5) {
        out = vec4f(encode(out.rgb / max(out.a, 1e-5)) * out.a, out.a);
    }
    return out;
}
"#;

fn slot_mark(i: &Instance) -> u32 {
    i.border[0] as u32 | (i.border[1] as u32) << 16
}

/// The uniform block for one slot: its quad, clip, frame, card and beams, and the spectrum's
/// stops in sRGB, all in the pass's device pixels.
fn uniforms(site: &EffectSite<'_>, time: f64, reduced: bool) -> Vec<u8> {
    let i = site.instance;
    let k = site.scale;
    let bleed = i.params[2];
    let [x, y, w, h] = i.rect;
    let radius = i.radii.iter().fold(0.0f32, |a, r| a.max(r.abs()));
    let mut f: Vec<f32> = Vec::with_capacity(96);
    f.extend([x - bleed, y - bleed, w + 2.0 * bleed, h + 2.0 * bleed]);
    f.extend(i.clip);
    f.extend(i.fade.map(f32::from_bits));
    f.extend([
        site.origin[0],
        site.origin[1],
        site.size[0] as f32,
        site.size[1] as f32,
    ]);
    f.extend([x, y, w, h]);
    f.extend([radius, k, if site.srgb_blend { 1.0 } else { 0.0 }, 0.0]);
    for beam in BEAMS {
        let (left, right) = beam.span(w / k);
        f.extend([left * k, right * k, BEAM_CENTER * k, beam.height * k]);
        f.extend([beam.blur * k, beam.opacity, beam.phase(time, reduced), 0.0]);
    }
    let accent = oklch_from_linear(unpack(site.data[0]));
    let bg = oklch_from_linear(unpack(site.data[1]));
    for (at, amount) in SPECTRUM {
        let [r, g, b] = linear_from_oklch(mix(accent, bg, amount)).map(srgb_encode);
        f.extend([r, g, b, at]);
    }
    f.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

struct Gpu {
    device: DeviceKey,
    layout: wgpu::BindGroupLayout,
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
}

impl Gpu {
    fn new(device: &wgpu::Device, key: DeviceKey) -> Gpu {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glow"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        Gpu {
            device: key,
            layout,
            pipelines: Vec::new(),
        }
    }

    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        if let Some(i) = self.pipelines.iter().position(|(f, _)| *f == format) {
            return &self.pipelines[i].1;
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glow"),
            source: wgpu::ShaderSource::Wgsl(format!("{CLIP_WGSL}\n{SHADER}").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glow"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glow"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.push((format, pipeline));
        &self.pipelines.last().expect("just pushed").1
    }
}

/// Paints the composer's beams at its effect slot, and leaves every other frame to Rune.
#[derive(Default)]
pub struct Glow {
    gpu: Option<Gpu>,
    live: bool,
}

impl<S: 'static> Compositor<S> for Glow {
    fn render(&mut self, f: &mut Frame<'_, S>) {
        let ours = |i: &Instance| i.params[1] == KIND_EFFECT && slot_mark(i) == MARK;
        let reduced = f.rt.reduced_motion;
        let present = f.rt.instances.iter().any(ours);
        self.live = present && !reduced;
        if !present {
            return f.draw_ui();
        }
        let (device, queue, size) = (f.device, f.queue, f.size);
        let key = f.device_key();
        if self.gpu.as_ref().is_none_or(|g| g.device != key) {
            self.gpu = Some(Gpu::new(device, key));
        }
        let gpu = self.gpu.as_mut().expect("made above");
        let time = f.rt.t;
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with(
            f.encoder,
            f.target,
            clear,
            &mut |pass: &mut wgpu::RenderPass<'_>, site: &EffectSite<'_>| {
                if !ours(site.instance) {
                    return;
                }
                let bytes = uniforms(site, time, reduced);
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("glow"),
                    size: bytes.len() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&buffer, 0, &bytes);
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glow"),
                    layout: &gpu.layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                });
                pass.set_pipeline(gpu.pipeline(device, site.format));
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..4, 0..1);
            },
        );
    }

    fn animating(&self) -> bool {
        self.live
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Accent, ThemePref};
    use crate::shell::kit_for;

    fn near(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    fn assert_oklch(got: Oklch, want: (f32, f32, f32)) {
        assert!(
            near(got.l, want.0, 2e-4) && near(got.c, want.1, 2e-4) && near(got.h, want.2, 0.05),
            "got {got:?}, want oklch{want:?}"
        );
    }

    #[test]
    fn colours_convert_to_oklch_as_css_does() {
        let bg = linear_from_srgb8([18, 17, 16]);
        let got = oklch_from_linear(bg);
        assert!(
            near(got.l, 0.178406, 2e-4) && near(got.c, 0.00262, 2e-4),
            "{got:?}"
        );
    }

    #[test]
    fn oklch_round_trips_through_linear_srgb() {
        let blue = Oklch {
            l: 0.706992,
            c: 0.164981,
            h: 254.619,
        };
        assert_oklch(
            oklch_from_linear(linear_from_oklch(blue)),
            (0.706992, 0.164981, 254.619),
        );
    }

    #[test]
    fn the_spectrum_mixes_the_accent_into_the_background_as_palermo_does() {
        let blue = Oklch {
            l: 0.706992,
            c: 0.164981,
            h: 254.619,
        };
        let bg = Oklch {
            l: 0.178406,
            c: 0.00261913,
            h: 0.0,
        };
        assert_oklch(mix(blue, bg, 0.72), (0.558988, 0.119519, 254.619));
        assert_oklch(mix(blue, bg, 0.18), (0.273552, 0.0318442, 254.619));
        assert_oklch(mix(blue, bg, 1.0), (0.706992, 0.164981, 254.619));
    }

    #[test]
    fn the_spectrum_runs_through_palermos_stops() {
        let positions: Vec<f32> = SPECTRUM.iter().map(|(at, _)| *at).collect();
        assert_eq!(
            positions,
            [
                0.0, 0.05, 0.16, 0.22, 0.34, 0.44, 0.50, 0.55, 0.62, 0.78, 0.86, 1.0
            ]
        );
        assert_eq!(SPECTRUM[0].1, 1.0);
        assert_eq!(SPECTRUM[6].1, 0.0, "the background itself at the middle");
        assert_eq!(SPECTRUM[11].1, 1.0, "ends where it starts, so it loops");
    }

    #[test]
    fn a_colour_packs_into_a_slot_float_and_back() {
        let colour = [0.5f32, 0.25, 0.75];
        let back = unpack(pack(colour));
        for (a, b) in colour.iter().zip(back) {
            assert!(near(*a, b, 0.004), "{colour:?} vs {back:?}");
        }
    }

    #[test]
    fn the_slot_carries_the_accent_and_the_background() {
        let k = kit_for(ThemePref::Dark, Accent::Pink, true);
        let [accent, bg, ..] = slot_data(&k);
        let want = k.c.bg_accent;
        let got = unpack(accent);
        assert!(near(got[0], want.r, 0.01) && near(got[2], want.b, 0.01));
        let want = k.c.bg_base;
        assert!(near(unpack(bg)[1], want.g, 0.01));
    }

    #[test]
    fn beams_resolve_their_insets_against_the_card_width() {
        let width = 344.0;
        let spans: Vec<(f32, f32)> = BEAMS.iter().map(|b| b.span(width)).collect();
        assert_eq!(spans[0], (4.0, 340.0), "wide: 4px each side");
        assert!(
            near(spans[1].0, 27.52, 0.01) && near(spans[1].1, 302.72, 0.01),
            "mid: 8% and 12%"
        );
        assert!(
            near(spans[2].0, 130.72, 0.01) && near(spans[2].1, 319.92, 0.01),
            "tight: 38% and 7%"
        );
    }

    #[test]
    fn beams_scroll_one_period_per_loop_from_their_offsets() {
        let [wide, mid, tight] = BEAMS;
        assert_eq!(wide.phase(0.0, false), 0.0);
        assert!(near(wide.phase(3.0, false), 0.5, 1e-6));
        assert!(
            near(mid.phase(0.0, false), 1.4 / 4.4, 1e-6),
            "mid starts 1.4s in"
        );
        assert!(
            near(tight.phase(0.0, false), 3.0 / 7.0, 1e-6),
            "tight starts 3s in"
        );
        assert!(near(wide.phase(6.0, false), 0.0, 1e-6), "and loops");
    }

    #[test]
    fn under_reduced_motion_the_beams_hold_still() {
        for beam in BEAMS {
            assert_eq!(beam.phase(2.5, true), 0.0);
        }
    }
}
