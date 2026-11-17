use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

use gpu_video::{VideoAdapterExt, parameters::VideoDeviceDescriptor};
use slint::{
    ComponentHandle, Timer, TimerMode,
    wgpu_30::{WGPUConfiguration, wgpu},
};

use playback::Playback;
use ui::AppWindow;
use video_view::VideoView;

mod demuxer;
mod playback;
mod video_view;

#[expect(clippy::todo)]
mod ui {
    slint::include_modules!();
}

const PLAYBACK_TICK_INTERVAL: Duration = Duration::from_millis(4);

struct Player {
    device: wgpu::Device,
    queue: wgpu::Queue,
    video_view: VideoView,
    paused: bool,
    playback: Option<Playback>,
    last_tick: Instant,
}

impl Player {
    fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            video_view: VideoView::new(&device),
            device,
            queue,
            paused: false,
            playback: None,
            last_tick: Instant::now(),
        }
    }

    fn open_file(&mut self, path: &Path) {
        self.playback = Some(Playback::new(&self.device, &self.queue, path));
        self.last_tick = Instant::now();
    }

    fn advance_playback(&mut self, window: &AppWindow) {
        let now = Instant::now();
        let elapsed = now - self.last_tick;
        self.last_tick = now;

        if self.paused {
            return;
        }
        let Some(playback) = &mut self.playback else {
            return;
        };
        let Some(frame) = playback.next_frame(elapsed) else {
            return;
        };

        let size = frame.data.size();
        window.set_frame(self.video_view.update(&self.device, &self.queue, &frame));
        window.set_status(
            format!(
                "{:.2}s | frame {} | {}x{}",
                playback.time.as_secs_f64(),
                playback.frame_number,
                size.width,
                size.height,
            )
            .into(),
        );
    }
}

fn main() {
    tracing_subscriber::fmt().init();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::default()))
        .into_iter()
        .find(|a| {
            a.video_adapter_info()
                .is_some_and(|info| info.decode_capabilities.h264.is_some())
        })
        .unwrap();
    let (device, queue) = adapter
        .request_device_with_video_support(&VideoDeviceDescriptor::default())
        .unwrap();

    slint::BackendSelector::new()
        .require_wgpu_30(WGPUConfiguration::Manual {
            instance,
            adapter,
            device: device.clone(),
            queue: queue.clone(),
        })
        .select()
        .unwrap();

    let window = AppWindow::new().unwrap();
    let player = Rc::new(RefCell::new(Player::new(device, queue)));

    window.on_open_file({
        let player = player.clone();
        move || {
            let picked = rfd::FileDialog::new()
                .add_filter("MP4", &["mp4"])
                .pick_file();
            if let Some(path) = picked {
                player.borrow_mut().open_file(&path);
            }
        }
    });

    window.on_toggle_pause({
        let player = player.clone();
        let window = window.as_weak();
        move || {
            let mut player = player.borrow_mut();
            player.paused = !player.paused;
            window.unwrap().set_paused(player.paused);
        }
    });

    let playback_timer = Timer::default();
    playback_timer.start(TimerMode::Repeated, PLAYBACK_TICK_INTERVAL, {
        let window = window.as_weak();
        move || {
            player.borrow_mut().advance_playback(&window.unwrap());
        }
    });

    window.run().unwrap();
}
