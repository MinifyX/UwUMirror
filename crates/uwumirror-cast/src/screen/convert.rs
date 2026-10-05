//! BGRA to NV12, scaled, on the graphics card.
//!
//! Encoders take NV12 (a full-size brightness plane and a half-size colour
//! plane), the screen comes as BGRA. D3D11's video processor converts and
//! scales in one step, in the same hardware that plays videos, without the
//! picture ever leaving the card. The colours are studio range, BT.709 when
//! the encoder writes that into the stream (the graphics cards' do), and
//! BT.601 when it writes nothing (Windows' own) — what decoders assume then.
//!
//! The picture keeps its proportions: when the screen changes to another
//! shape while sending, it is fitted into the encoder's fixed size with black
//! bars, instead of restarting the encoder.

use std::mem::ManuallyDrop;

use windows::core::{Interface, Result};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIDEO_COLOR, D3D11_VIDEO_COLOR_0,
    D3D11_VIDEO_COLOR_RGBA, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_OPTIMAL_SPEED, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601,
    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};

use super::capture::Gpu;

/// NV12 textures written in turn. An encoder working on its own threads may
/// still hold the last few; this many is far more than any keeps.
const RING: usize = 8;

struct Pipeline {
    /// The views were made for it; it lives as long as they do.
    _enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    input: ID3D11VideoProcessorInputView,
    outputs: Vec<(ID3D11Texture2D, ID3D11VideoProcessorOutputView)>,
}

pub struct Converter {
    video: ID3D11VideoDevice,
    context: ID3D11VideoContext,
    width: u32,
    height: u32,
    fps: u32,
    bt709: bool,
    pipeline: Option<Pipeline>,
    next: usize,
}

/// `inner` fitted into `outer`, centred, proportions kept, on even pixels.
pub fn fit(inner: (u32, u32), outer: (u32, u32)) -> RECT {
    let scale = f64::min(
        outer.0 as f64 / inner.0.max(1) as f64,
        outer.1 as f64 / inner.1.max(1) as f64,
    );
    let width = ((inner.0 as f64 * scale) as i32 & !1).min(outer.0 as i32);
    let height = ((inner.1 as f64 * scale) as i32 & !1).min(outer.1 as i32);
    let left = ((outer.0 as i32 - width) / 2) & !1;
    let top = ((outer.1 as i32 - height) / 2) & !1;
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

impl Converter {
    /// Converts into `width` × `height` NV12 frames, at most `fps` a second,
    /// with BT.709's colours or BT.601's.
    pub fn new(gpu: &Gpu, width: u32, height: u32, fps: u32, bt709: bool) -> Result<Self> {
        Ok(Self {
            video: gpu.device.cast()?,
            context: gpu.context.cast()?,
            width,
            height,
            fps,
            bt709,
            pipeline: None,
            next: 0,
        })
    }

    /// Reads from `source` from now on; again whenever it is a new texture.
    pub fn set_source(&mut self, gpu: &Gpu, source: &ID3D11Texture2D) -> Result<()> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut desc) };
        let rate = DXGI_RATIONAL {
            Numerator: self.fps,
            Denominator: 1,
        };
        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: desc.Width,
            InputHeight: desc.Height,
            OutputFrameRate: rate,
            OutputWidth: self.width,
            OutputHeight: self.height,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        // Everything hangs off the enumerator, which knows the input size:
        // all of it is made anew (rare: the screen changed its resolution).
        self.pipeline = None;
        unsafe {
            let enumerator = self.video.CreateVideoProcessorEnumerator(&content)?;
            let processor = self.video.CreateVideoProcessor(&enumerator, 0)?;
            let mut input = None;
            self.video.CreateVideoProcessorInputView(
                source,
                &enumerator,
                &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    FourCC: 0,
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_VPIV {
                            MipSlice: 0,
                            ArraySlice: 0,
                        },
                    },
                },
                Some(&mut input),
            )?;
            let mut outputs = Vec::with_capacity(RING);
            for _ in 0..RING {
                let texture = nv12_texture(gpu, self.width, self.height)?;
                let mut view = None;
                self.video.CreateVideoProcessorOutputView(
                    &texture,
                    &enumerator,
                    &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                        },
                    },
                    Some(&mut view),
                )?;
                outputs.push((texture, view.expect("an output view")));
            }

            let context = &self.context;
            context.VideoProcessorSetStreamFrameFormat(
                &processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            // No "enhancements" a driver might add to a video: this is text.
            context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
            let source_rect = RECT {
                left: 0,
                top: 0,
                right: desc.Width as i32,
                bottom: desc.Height as i32,
            };
            context.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&source_rect));
            let target = fit((desc.Width, desc.Height), (self.width, self.height));
            context.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&target));
            context.VideoProcessorSetOutputBackgroundColor(
                &processor,
                false,
                &D3D11_VIDEO_COLOR {
                    Anonymous: D3D11_VIDEO_COLOR_0 {
                        RGBA: D3D11_VIDEO_COLOR_RGBA {
                            R: 0.0,
                            G: 0.0,
                            B: 0.0,
                            A: 1.0,
                        },
                    },
                },
            );
            if let Ok(context) = context.cast::<ID3D11VideoContext1>() {
                context.VideoProcessorSetStreamColorSpace1(
                    &processor,
                    0,
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                );
                context.VideoProcessorSetOutputColorSpace1(
                    &processor,
                    if self.bt709 {
                        DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709
                    } else {
                        DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601
                    },
                );
            }
            self.pipeline = Some(Pipeline {
                _enumerator: enumerator,
                processor,
                input: input.expect("an input view"),
                outputs,
            });
        }
        Ok(())
    }

    /// Converts the source as it is now into the next NV12 texture.
    pub fn convert(&mut self) -> Result<ID3D11Texture2D> {
        let pipeline = self
            .pipeline
            .as_ref()
            .expect("set_source comes before convert");
        let (texture, view) = &pipeline.outputs[self.next];
        self.next = (self.next + 1) % RING;
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(pipeline.input.clone())),
            ..Default::default()
        };
        let streams = [stream];
        let result = unsafe {
            self.context
                .VideoProcessorBlt(&pipeline.processor, view, 0, &streams)
        };
        // The stream holds a reference of its own to the input view.
        let [mut stream] = streams;
        unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
        result?;
        Ok(texture.clone())
    }
}

fn nv12_texture(gpu: &Gpu, width: u32, height: u32) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, None, Some(&mut texture))?
    };
    Ok(texture.expect("CreateTexture2D gave a texture"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_the_shape() {
        let r = fit((2560, 1440), (1920, 1080));
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 0, 1920, 1080));
        // A 16:10 screen into 16:9: bars left and right.
        let r = fit((1920, 1200), (1920, 1080));
        assert_eq!((r.top, r.bottom), (0, 1080));
        assert_eq!(r.right - r.left, 1728);
        assert_eq!(r.left, 96);
    }
}
