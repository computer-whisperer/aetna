//! Headless demo: render a fixture that exercises the custom-shader
//! escape hatch.
//!
//! Validates the load-bearing premise from `SHADER_VISION.md` — that a
//! user crate can register its own WGSL shader and bind it on a node,
//! producing visuals stock shaders can't (here: vertical linear gradient
//! with rounded corners). Three buttons in a row use the same custom
//! shader with different uniforms, alongside one stock button for
//! contrast — proving the renderer interleaves stock + custom in paint
//! order.
//!
//! Same paint path as `render_png` (no surface, no event loop), so this
//! also doubles as a CI fixture.
//!
//! Usage: `cargo run -p attempt_4_demo --bin render_custom`
//! Writes: `attempts/attempt_4/out/custom_shader.wgpu.png`

use attempt_4::*;

const GRADIENT_WGSL: &str = include_str!("../../../attempt_4/shaders/gradient.wgsl");

/// Helper: a button-shaped `El` whose surface paint is the registered
/// `gradient` shader instead of `stock::rounded_rect`. This is what a
/// user crate would write to ship its own component variant.
fn gradient_button(label: &str, top: Color, bottom: Color, radius: f32) -> El {
    button(label)
        .text_color(tokens::TEXT_ON_SOLID_DARK)
        .shader(
            ShaderBinding::custom("gradient")
                .color("vec_a", top)
                .color("vec_b", bottom)
                .f32("vec_c", radius),
        )
}

fn fixture() -> El {
    column([
        h1("Custom shader demo"),
        text(
            "Three buttons below paint via a registered custom shader \
             (gradient.wgsl). The right-hand button is a stock rounded_rect \
             for contrast — proving the renderer interleaves both kinds in \
             paint order.",
        )
        .muted(),
        card("gradient.wgsl — vertical linear gradient", [
            row([
                gradient_button(
                    "Sunrise",
                    Color::rgb(255, 200, 90),
                    Color::rgb(245, 95, 110),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Ocean",
                    Color::rgb(120, 200, 255),
                    Color::rgb(40, 90, 200),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Forest",
                    Color::rgb(180, 230, 140),
                    Color::rgb(40, 110, 80),
                    tokens::RADIUS_MD,
                ),
                spacer(),
                button("Stock").secondary(),
            ])
            .gap(tokens::SPACE_MD),
            text("Same shader, three uniform sets. The fourth button \
                  (stock::rounded_rect) is unrelated and demonstrates that \
                  custom and stock pipelines coexist.")
                .muted()
                .small(),
        ]),
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 720;
    let logical_height: u32 = 360;
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

    println!("adapter: {:?} ({:?})", adapter.get_info().name, adapter.get_info().backend);

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("attempt_4_demo::custom::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("attempt_4_demo::custom::target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
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
    let padded_bytes_per_row = ((unpadded_bytes_per_row + align - 1) / align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("attempt_4_demo::custom::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = UiRenderer::new(&device, &queue, format);
    // The whole point — register a shader by name; nodes referring to
    // ShaderHandle::Custom("gradient") now paint through it.
    renderer.register_shader(&device, "gradient", GRADIENT_WGSL);

    let mut tree = fixture();
    renderer.prepare(&device, &queue, &mut tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("attempt_4_demo::custom::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("attempt_4_demo::custom::pass"),
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
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
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

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join("attempt_4").join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("custom_shader.wgpu.png");
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
    let c = attempt_4::tokens::BG_APP;
    wgpu::Color {
        r: srgb_to_linear(c.r as f64 / 255.0),
        g: srgb_to_linear(c.g as f64 / 255.0),
        b: srgb_to_linear(c.b as f64 / 255.0),
        a: c.a as f64 / 255.0,
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
