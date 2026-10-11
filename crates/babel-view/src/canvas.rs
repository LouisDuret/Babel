//! Rendu wgpu du canevas : toutes les cellules en un seul appel instancié.
//!
//! - positions par vue et par verset dans un buffer statique ;
//! - valeurs par métrique dans un buffer par métrique ;
//! - une texture LUT de 256 texels par palette ;
//! - caméra, transition, survol et sélection dans des uniformes.
//!
//! Changer de métrique, de palette ou de vue ne renvoie aucune géométrie au
//! GPU : seuls les uniformes et le choix des buffers changent. Les coins
//! arrondis et l'anticrénelage sont calculés dans le fragment shader par
//! fonction de distance signée, sans MSAA.

use crate::palette::{Palette, TEXELS};
use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt as _;
use std::sync::Arc;

const SHADER: &str = r"
struct Uniforms {
    center: vec2<f32>,
    viewport: vec2<f32>,
    scale: f32,
    transition: f32,
    cell_gap: f32,
    corner: f32,
    hovered: i32,
    selected: i32,
    srgb_target: u32,
    _padding: f32,
    missing: vec4<f32>,
    hover_color: vec4<f32>,
    accent: vec4<f32>,
    background: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var lut: texture_2d<f32>;
@group(0) @binding(2) var lut_sampler: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) value: f32,
    @location(2) @interpolate(flat) index: u32,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex: u32,
    @builtin(instance_index) instance: u32,
    @location(0) wall: vec2<f32>,
    @location(1) hilbert: vec2<f32>,
    @location(2) value: f32,
) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex];
    // La transition entre vues est interpolée ici, par cellule.
    let world = mix(wall, hilbert, u.transition) + corner;
    let pixels = (world - u.center) * u.scale;
    var out: VsOut;
    out.position = vec4<f32>(
        pixels.x / (0.5 * u.viewport.x),
        -pixels.y / (0.5 * u.viewport.y),
        0.0,
        1.0,
    );
    if (value < -1.5) {
        // Verset masqué par un filtre : rejeté hors du volume de vue.
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    out.local = corner;
    out.value = value;
    out.index = instance;
    return out;
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let size = u.scale;
    let half = 0.5 * size * (1.0 - u.cell_gap);
    let p = (in.local - vec2<f32>(0.5)) * size;
    let distance = rounded_box(p, vec2<f32>(half), u.corner * 2.0 * half);
    let coverage = clamp(0.5 - distance, 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }

    var color: vec3<f32>;
    if (in.value < 0.0) {
        // Verset sans données : hachures, lisibles dès que la cellule est
        // assez grande, aplat discret sinon.
        let stripe = step(0.5, fract((in.position.x + in.position.y) / 6.0));
        let flat_tint = mix(u.background.rgb, u.missing.rgb, 0.45);
        color = mix(flat_tint, u.missing.rgb, stripe * smoothstep(4.0, 10.0, size));
    } else {
        let coordinate = (in.value * 255.0 + 0.5) / 256.0;
        color = textureSampleLevel(lut, lut_sampler, vec2<f32>(coordinate, 0.5), 0.0).rgb;
    }

    let index = i32(in.index);
    if (index == u.selected) {
        let width = clamp(0.03 * size, 2.0, 5.0);
        let ring = smoothstep(-width - 0.5, -width + 0.5, distance);
        color = mix(color, u.accent.rgb, ring);
    } else if (index == u.hovered) {
        let width = clamp(0.02 * size, 1.5, 3.0);
        let ring = smoothstep(-width - 0.5, -width + 0.5, distance);
        color = mix(color, u.hover_color.rgb, ring * u.hover_color.a);
    }

    if (u.srgb_target == 1u) {
        color = to_linear(color);
    }
    return vec4<f32>(color * coverage, coverage);
}
";

/// Uniformes du shader. La disposition mémoire suit la structure WGSL.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Uniforms {
    /// Point du monde au centre de la vue, en cellules.
    pub center: [f32; 2],
    /// Taille de la vue, en pixels physiques.
    pub viewport: [f32; 2],
    /// Pixels physiques par cellule.
    pub scale: f32,
    /// 0 = vue Mur, 1 = vue Hilbert.
    pub transition: f32,
    pub cell_gap: f32,
    /// Rayon des coins, en part de la taille de cellule.
    pub corner: f32,
    /// Verset survolé, ou −1.
    pub hovered: i32,
    /// Verset sélectionné, ou −1.
    pub selected: i32,
    /// 1 si la cible de rendu est en sRGB (le shader convertit alors).
    pub srgb_target: u32,
    pub padding: f32,
    pub missing: [f32; 4],
    pub hover_color: [f32; 4],
    pub accent: [f32; 4],
    pub background: [f32; 4],
}

/// Position d'un verset dans chaque vue.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    wall: [f32; 2],
    hilbert: [f32; 2],
}

/// Ressources GPU du canevas, rangées dans les ressources de rappel
/// d'egui-wgpu.
pub struct CanvasResources {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    /// Un buffer de valeurs par métrique.
    values: Vec<wgpu::Buffer>,
    /// Un groupe de liaison par palette.
    palettes: Vec<wgpu::BindGroup>,
    count: u32,
}

impl CanvasResources {
    /// Crée le pipeline et les buffers.
    ///
    /// `values[m]` donne la valeur initiale de chaque verset pour la
    /// métrique m ; `palettes` les textures LUT.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
        wall: &[[f32; 2]],
        hilbert: &[[f32; 2]],
        values: &[Vec<f32>],
        palettes: &[Palette],
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("babel canvas"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("babel canvas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("babel uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("babel lut"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Les texels sont des demi-flottants en sRGB encodé : la cible
        // d'egui n'est pas en sRGB, le shader écrit donc des valeurs déjà
        // encodées. Huit bits par canal ne distingueraient pas 256 texels
        // dans les teintes sombres.
        let palette_groups = palettes
            .iter()
            .map(|palette| {
                let texture = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some("babel lut"),
                        size: wgpu::Extent3d {
                            width: TEXELS as u32,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba16Float,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::default(),
                    &palette.rgba16f(),
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("babel palette"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniforms.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                })
            })
            .collect();

        let instance_data: Vec<Instance> = wall
            .iter()
            .zip(hilbert)
            .map(|(wall, hilbert)| Instance {
                wall: *wall,
                hilbert: *hilbert,
            })
            .collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("babel positions"),
            contents: bytemuck::cast_slice(&instance_data),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let value_buffers = values
            .iter()
            .map(|v| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("babel values"),
                    contents: bytemuck::cast_slice(v),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                })
            })
            .collect();

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("babel canvas"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let position_attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
        let value_attributes = wgpu::vertex_attr_array![2 => Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("babel canvas"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &position_attributes,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<f32>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &value_attributes,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            uniforms,
            instances,
            values: value_buffers,
            palettes: palette_groups,
            count: instance_data.len() as u32,
        }
    }
}

/// Ce qu'une image du canevas demande au GPU.
pub struct CanvasFrame {
    pub uniforms: Uniforms,
    /// Index du buffer de valeurs à afficher.
    pub metric: usize,
    /// Index de la palette.
    pub palette: usize,
    /// Buffers de valeurs à réécrire avant le rendu : (métrique, valeurs).
    pub uploads: Vec<(usize, Arc<Vec<f32>>)>,
}

impl egui_wgpu::CallbackTrait for CanvasFrame {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(canvas) = resources.get::<CanvasResources>() {
            for (metric, values) in &self.uploads {
                if let Some(buffer) = canvas.values.get(*metric) {
                    queue.write_buffer(buffer, 0, bytemuck::cast_slice(values));
                }
            }
            queue.write_buffer(&canvas.uniforms, 0, bytemuck::bytes_of(&self.uniforms));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(canvas) = resources.get::<CanvasResources>() else {
            return;
        };
        let (Some(values), Some(palette)) = (
            canvas.values.get(self.metric),
            canvas.palettes.get(self.palette),
        ) else {
            return;
        };
        pass.set_pipeline(&canvas.pipeline);
        pass.set_bind_group(0, palette, &[]);
        pass.set_vertex_buffer(0, canvas.instances.slice(..));
        pass.set_vertex_buffer(1, values.slice(..));
        // Deux triangles par cellule, toutes les cellules en un appel.
        pass.draw(0..6, 0..canvas.count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_memoire_des_uniformes() {
        // La structure WGSL fait 112 octets ; les vec4 commencent à 48.
        assert_eq!(std::mem::size_of::<Uniforms>(), 112);
        assert_eq!(std::mem::offset_of!(Uniforms, scale), 16);
        assert_eq!(std::mem::offset_of!(Uniforms, hovered), 32);
        assert_eq!(std::mem::offset_of!(Uniforms, missing), 48);
        assert_eq!(std::mem::offset_of!(Uniforms, background), 96);
        assert_eq!(std::mem::size_of::<Instance>(), 16);
    }
}
