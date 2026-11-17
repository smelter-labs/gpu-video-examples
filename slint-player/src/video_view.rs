use gpu_video::{
    OutputFrame, VideoTexture, WgpuNv12ToRgbaConverter,
    parameters::{ColorRange, ColorSpace, WgpuConverterParameters},
};
use slint::wgpu_30::wgpu;

#[derive(Clone)]
struct OutputTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

pub struct VideoView {
    converter: WgpuNv12ToRgbaConverter,
    texture: Option<OutputTexture>,
}

impl VideoView {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            converter: WgpuNv12ToRgbaConverter::new(
                device,
                WgpuConverterParameters {
                    color_space: ColorSpace::BT709,
                    color_range: ColorRange::Limited,
                },
            )
            .unwrap(),
            texture: None,
        }
    }

    fn output_texture(
        &mut self,
        device: &wgpu::Device,
        frame: &OutputFrame<VideoTexture>,
    ) -> OutputTexture {
        let size = frame.data.size();
        if self
            .texture
            .as_ref()
            .is_none_or(|output| output.texture.size() != size)
        {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.texture = Some(OutputTexture { texture, view });
        }

        self.texture.clone().unwrap()
    }

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &OutputFrame<VideoTexture>,
    ) -> slint::Image {
        let bind_group = self.converter.create_input_bind_group(frame).unwrap();
        let output = self.output_texture(device, frame);
        let mut encoder = device.create_command_encoder(&Default::default());
        self.converter
            .convert(&mut encoder, &bind_group, &output.view);
        queue.submit([encoder.finish()]);

        slint::Image::try_from(output.texture).unwrap()
    }
}
