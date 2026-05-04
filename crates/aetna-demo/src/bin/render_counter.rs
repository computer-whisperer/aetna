//! Headless render of the counter fixture, exercising the v0.2
//! state-driven build + simulated hover path.
//!
//! Drives the renderer like the windowed runner would: build → prepare
//! (lays out, stores tree) → pointer_moved (hit-tests, sets hovered key)
//! → build again → prepare again (applies hover delta) → draw → PNG.
//!
//! The visible result: counter showing a non-zero value, with the "+"
//! button rendered in its hover state. Confirms that state, hover, and
//! the App trait round-trip end-to-end through wgpu without needing a
//! winit window.
//!
//! Usage: `cargo run -p aetna-demo --bin render_counter`
//! Writes: `crates/aetna-demo/out/counter.wgpu.png`

use aetna_core::*;
use aetna_wgpu::Runner;

struct Counter {
    value: i32,
}

impl App for Counter {
    fn build(&self) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("−").key("dec").secondary(),
                button("Reset").key("reset").ghost(),
                button("+").key("inc").primary(),
            ])
            .gap(tokens::SPACE_MD),
            text(if self.value == 0 {
                "Click + or − to change the count.".to_string()
            } else {
                format!("You have clicked +/− a net {} times.", self.value)
            })
            .center_text()
            .muted(),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
        .align(Align::Center)
    }
}

fn find_rect(node: &El, ui_state: &aetna_core::UiState, key: &str) -> Option<Rect> {
    if node.key.as_deref() == Some(key) {
        return Some(ui_state.rect(&node.computed_id));
    }
    for c in &node.children {
        if let Some(r) = find_rect(c, ui_state, key) {
            return Some(r);
        }
    }
    None
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 480;
    let logical_height: u32 = 280;
    let scale_factor: f32 = 2.0;
    let width = (logical_width as f32 * scale_factor) as u32;
    let height = (logical_height as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, logical_width as f32, logical_height as f32);

    let app = Counter { value: 5 };

    // ---- wgpu boilerplate (same as render_png) ----
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or("no compatible adapter")?;
    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("aetna_demo::counter::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aetna_demo::counter::target"),
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
        label: Some("aetna_demo::counter::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::new(&device, &queue, format);
    // Headless single-frame snapshot: settle every animation each tick
    // so the lightened-on-hover output doesn't depend on integrator
    // timing between the simulated pointer move and prepare.
    renderer.set_animation_mode(aetna_core::AnimationMode::Settled);

    // ---- v0.2 round-trip ----
    //
    // First prepare: lays out the tree and stashes it inside the
    // renderer so subsequent pointer events hit-test against real
    // geometry. No GPU draw yet — we discard this pass's instance data
    // by just not running encode/submit.
    let mut tree1 = app.build();
    renderer.prepare(&device, &queue, &mut tree1, viewport, scale_factor);

    // Find the "+" button's center from the laid-out tree, then move
    // the simulated pointer there. The renderer hit-tests against its
    // stored last_tree and updates its hover key.
    let plus =
        find_rect(&tree1, renderer.ui_state(), "inc").ok_or("missing 'inc' button in tree")?;
    let cx = plus.x + plus.w * 0.5;
    let cy = plus.y + plus.h * 0.5;
    let _ = renderer.pointer_moved(cx, cy);
    let hovered = renderer.ui_state().hovered.as_ref().map(|t| t.key.as_str());
    println!("simulated pointer at +button center; hover = {hovered:?}");

    // Second prepare: fresh tree, library applies the now-set hover
    // key automatically, draw_ops emits the lightened fill.
    let mut tree2 = app.build();
    renderer.prepare(&device, &queue, &mut tree2, viewport, scale_factor);

    // ---- Render to texture ----
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("aetna_demo::counter::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aetna_demo::counter::pass"),
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
    let out = out_dir.join("counter.wgpu.png");
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
