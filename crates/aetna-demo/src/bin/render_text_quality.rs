//! Headless: render a text-quality matrix to PNG via wgpu.
//!
//! Used as the visual-fidelity bench while we evolve text rendering.
//! The fixture intentionally exercises:
//!
//! - body/UI sizes (10..16) where rasterization quality matters most,
//! - mid sizes (18..24) typical for headings and KPIs,
//! - large sizes (32/48/64) where corner sharpness shows up,
//! - regular / medium / bold weights side-by-side,
//! - light and dark backgrounds (gamma blending tells on each),
//! - a small Unicode + math sample so fallback faces participate.
//!
//! Run with multiple scale factors to A/B multi-display behaviour:
//!
//!     cargo run -p aetna-demo --bin render_text_quality
//!     cargo run -p aetna-demo --bin render_text_quality -- --scale=2
//!     cargo run -p aetna-demo --bin render_text_quality -- --scale=1.5 --tag=before
//!
//! Writes `crates/aetna-demo/out/text_quality{.tag}.{scale}x.png`.

use aetna_core::*;
use aetna_wgpu::Runner;

const SIZES: &[f32] = &[10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 32.0, 48.0, 64.0];
const SAMPLE: &str = "The quick brown fox jumps over the lazy dog 0123456789";
const UNICODE_SAMPLE: &str = "—≡ →↑↓ ✓✕ αβγ ∑∫ ▲◆";

fn size_row(size: f32) -> El {
    let label = format!("{}px", size as u32);
    column([
        row([
            text(&label).font_size(11.0).muted().width(Size::Fixed(56.0)),
            text(SAMPLE).font_size(size).font_weight(FontWeight::Regular),
        ])
        .gap(tokens::SPACE_SM)
        .align(Align::End),
        row([
            text("medium")
                .font_size(11.0)
                .muted()
                .width(Size::Fixed(56.0)),
            text(SAMPLE).font_size(size).font_weight(FontWeight::Medium),
        ])
        .gap(tokens::SPACE_SM)
        .align(Align::End),
        row([
            text("bold").font_size(11.0).muted().width(Size::Fixed(56.0)),
            text(SAMPLE).font_size(size).font_weight(FontWeight::Bold),
        ])
        .gap(tokens::SPACE_SM)
        .align(Align::End),
    ])
    .gap(2.0)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn dark_panel() -> El {
    column([
        text("dark surface").caption(),
        text(SAMPLE).font_size(14.0),
        text(SAMPLE).font_size(14.0).font_weight(FontWeight::Bold),
        text(UNICODE_SAMPLE).font_size(16.0),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_MD)
    .fill(Color::rgb(20, 20, 24))
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn light_panel() -> El {
    column([
        text("light surface")
            .caption()
            .text_color(Color::rgb(80, 80, 80)),
        text(SAMPLE).font_size(14.0).text_color(Color::rgb(20, 20, 20)),
        text(SAMPLE)
            .font_size(14.0)
            .font_weight(FontWeight::Bold)
            .text_color(Color::rgb(20, 20, 20)),
        text(UNICODE_SAMPLE)
            .font_size(16.0)
            .text_color(Color::rgb(20, 20, 20)),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_MD)
    .fill(Color::rgb(245, 245, 248))
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn fixture() -> El {
    column(
        [
            vec![h2("Text quality matrix")],
            SIZES.iter().map(|&s| size_row(s)).collect::<Vec<_>>(),
            vec![dark_panel(), light_panel()],
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
    )
    .gap(tokens::SPACE_MD)
    .padding(tokens::SPACE_LG)
    .fill(tokens::BG_APP)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scale_factor: f32 = 1.0;
    let mut tag: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--scale=") {
            scale_factor = v.parse()?;
        } else if let Some(v) = arg.strip_prefix("--tag=") {
            tag = Some(v.to_string());
        } else {
            return Err(format!("unknown arg: {arg}").into());
        }
    }

    let logical_width: u32 = 980;
    let logical_height: u32 = 1820;
    let width = (logical_width as f32 * scale_factor) as u32;
    let height = (logical_height as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, logical_width as f32, logical_height as f32);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or("no compatible adapter (try installing vulkan / mesa drivers)")?;

    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("aetna_demo::text_quality::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aetna_demo::text_quality::target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aetna_demo::text_quality::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::new(&device, &queue, format);
    renderer.set_animation_mode(aetna_core::AnimationMode::Settled);
    let mut tree = fixture();
    renderer.prepare(&device, &queue, &mut tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("aetna_demo::text_quality::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aetna_demo::text_quality::pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(bg),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        renderer.draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback_buf,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let buffer_slice = readback_buf.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
    buffer_slice.map_async(wgpu::MapMode::Read, move |r| {
        sender.send(r).ok();
    });
    device.poll(wgpu::Maintain::Wait);
    receiver.recv()??;

    let padded = buffer_slice.get_mapped_range();
    let mut unpadded = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + unpadded_bytes_per_row as usize;
        unpadded.extend_from_slice(&padded[start..end]);
    }
    drop(padded);
    readback_buf.unmap();

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let suffix = match tag.as_deref() {
        Some(t) => format!(".{t}"),
        None => String::new(),
    };
    let scale_label = if scale_factor.fract() == 0.0 {
        format!("{}x", scale_factor as u32)
    } else {
        format!("{scale_factor}x")
    };
    let out = out_dir.join(format!("text_quality{suffix}.{scale_label}.png"));
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    Ok(())
}

fn bg_color() -> wgpu::Color {
    let c = aetna_core::tokens::BG_APP;
    wgpu::Color {
        r: srgb_to_linear(c.r as f64 / 255.0),
        g: srgb_to_linear(c.g as f64 / 255.0),
        b: srgb_to_linear(c.b as f64 / 255.0),
        a: c.a as f64 / 255.0,
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
