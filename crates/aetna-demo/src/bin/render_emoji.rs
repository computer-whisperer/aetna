//! Headless: render a small UI containing color emoji to PNG via wgpu.
//!
//! End-to-end smoke test for the v0.7 unified-RGBA atlas + NotoColorEmoji
//! bundle. The fixture mixes outline text (Roboto) with two color emoji
//! glyphs; the PNG goes to `out/emoji.wgpu.png`. The program also walks
//! the readback buffer and prints whether any pixel in the rendered
//! frame shows non-grayscale RGB beyond the background — this is the
//! programmatic sanity check that color emoji actually came through
//! the GPU as colored pixels rather than alpha silhouettes.
//!
//! Usage: `cargo run -p aetna-demo --bin render_emoji`

use aetna_core::*;
use aetna_wgpu::Runner;

fn fixture() -> El {
    column([
        h1("Hello 👋"),
        paragraph("LLM output reaches for emoji freely. Aetna's bundled NotoColorEmoji renders them in color through the unified RGBA atlas — same shader as outline text."),
        row([
            text("Status").bold(),
            spacer(),
            text("✅ shipped").muted(),
        ]),
        row([
            text("Mood").bold(),
            spacer(),
            text("🚀 ready").muted(),
        ]),
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 560;
    let logical_height: u32 = 320;
    let scale_factor: f32 = 2.0;
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
            label: Some("aetna_demo::emoji::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aetna_demo::emoji::target"),
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
        label: Some("aetna_demo::emoji::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::new(&device, &queue, format);
    renderer.set_animation_mode(aetna_core::AnimationMode::Settled);
    let mut tree = fixture();
    renderer.prepare(&device, &queue, &mut tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("aetna_demo::emoji::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aetna_demo::emoji::pass"),
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

    // Probe: count pixels whose RGB is meaningfully non-grayscale.
    // Outline text (modulated by a single text color) renders monochrome;
    // color emoji shows distinct R/G/B channels. Threshold of 16 ignores
    // tiny sRGB rounding noise.
    let mut color_pixels = 0usize;
    let mut chunks = unpadded.chunks_exact(4);
    for chunk in chunks.by_ref() {
        let (r, g, b) = (chunk[0] as i32, chunk[1] as i32, chunk[2] as i32);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if max - min > 16 {
            color_pixels += 1;
        }
    }
    println!(
        "color pixels: {} / {} ({:.2}%)",
        color_pixels,
        width * height,
        100.0 * color_pixels as f64 / (width as f64 * height as f64),
    );

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("emoji.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    if color_pixels < 100 {
        return Err(format!(
            "expected color emoji glyphs (many colored pixels); got only {color_pixels} — \
             render path probably collapsed RGB"
        )
        .into());
    }
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
