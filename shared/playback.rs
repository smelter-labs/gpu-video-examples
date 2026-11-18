use std::{
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::Duration,
};

use gpu_video::{
    EncodedInputChunk, OutputFrame, VideoDeviceExt, VideoTexture,
    parameters::{DecoderParameters, InputBytestreamFormatH264},
};

use crate::{demuxer::Mp4Demuxer, wgpu};

type DecodedFrame = OutputFrame<VideoTexture>;

struct PlaybackDropped;

pub struct Playback {
    decoded_frames: Receiver<DecodedFrame>,
    upcoming_frame: Option<DecodedFrame>,
    pub frame_number: u64,
    pub time: Duration,
}

impl Playback {
    const MAX_DECODED_FRAMES: usize = 3;

    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, path: &Path) -> Self {
        let (decoded_frame_sender, decoded_frames) = mpsc::sync_channel(Self::MAX_DECODED_FRAMES);
        let device = device.clone();
        let queue = queue.clone();
        let path = path.to_owned();
        thread::Builder::new()
            .name("decoder".into())
            .spawn(move || {
                let _ = decode_file(&device, &queue, &path, &decoded_frame_sender);
            })
            .unwrap();

        Self {
            decoded_frames,
            upcoming_frame: None,
            frame_number: 0,
            time: Duration::ZERO,
        }
    }

    pub fn next_frame(&mut self, elapsed: Duration) -> Option<DecodedFrame> {
        self.time += elapsed;

        if self.upcoming_frame.is_none() {
            self.upcoming_frame = self.decoded_frames.try_recv().ok();
        }

        let is_due = self
            .upcoming_frame
            .as_ref()
            .is_some_and(|f| Duration::from_micros(f.metadata.pts.unwrap()) <= self.time);
        if !is_due {
            return None;
        }
        self.frame_number += 1;
        self.upcoming_frame.take()
    }
}

fn decode_file(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    path: &Path,
    decoded_frame_sender: &SyncSender<DecodedFrame>,
) -> Result<(), PlaybackDropped> {
    let forward_frame = |frame| {
        decoded_frame_sender
            .send(frame)
            .map_err(|_| PlaybackDropped)
    };
    let mut demuxer = Mp4Demuxer::new(path);
    let (frame_sender, frame_receiver) = mpsc::channel();
    let parameters = DecoderParameters {
        bytestream_format: InputBytestreamFormatH264::Avcc {
            prefix_length: demuxer.prefix_length,
        },
        ..Default::default()
    };
    let mut decoder = device
        .video()
        .unwrap()
        .create_wgpu_textures_decoder_h264(queue, parameters, move |frame| {
            frame_sender.send(frame).unwrap();
        })
        .unwrap();

    while let Some(chunk) = demuxer.next_chunk() {
        decoder
            .decode(EncodedInputChunk {
                data: &chunk.data,
                pts: Some(chunk.pts.as_micros() as u64),
            })
            .unwrap();
        frame_receiver.try_iter().try_for_each(forward_frame)?;
    }

    decoder.flush().unwrap();
    drop(decoder);
    frame_receiver.into_iter().try_for_each(forward_frame)
}
