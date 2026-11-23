//! The smoke run itself: Cocoa windows, bundles in `~/Library` and the
//! malloc zone statistics, so macOS only.
#![allow(unsafe_code, missing_debug_implementations)]

use silkscreen_clap::test_host::{self, Host};
use std::cell::{Cell, RefCell};
use std::time::Duration;

use baseview::{
    Event, EventStatus, HandlerError, TimerHandle, Window, WindowContext, WindowHandler,
    WindowSettings, WindowSize,
};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, PluginGui, Window as ClapWindow};
use clack_host::prelude::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

type Loaded = (PluginEntry, PluginInstance<Host>);

fn load_bundle(name: &str) -> Loaded {
    let home = std::env::var("HOME").expect("HOME");
    let path = format!("{home}/Library/Audio/Plug-Ins/CLAP/{name}.clap");
    let entry = unsafe { PluginEntry::load(&path) }.unwrap_or_else(|e| panic!("{path}: {e}"));
    let plugin = test_host::instantiate(&entry);
    (entry, plugin)
}

#[repr(C)]
#[derive(Default)]
struct MallocStatistics {
    blocks_in_use: u32,
    size_in_use: usize,
    max_size_in_use: usize,
    size_allocated: usize,
}

unsafe extern "C" {
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut MallocStatistics);
}

/// Live heap across all malloc zones: (KiB, blocks). Every dylib in the
/// process, `AppKit` and Core Animation included, allocates through these.
fn heap() -> (u64, u32) {
    let mut stats = MallocStatistics::default();
    // SAFETY: a null zone means "all zones"; `stats` is a valid out pointer.
    unsafe { malloc_zone_statistics(std::ptr::null_mut(), &raw mut stats) };
    ((stats.size_in_use / 1024) as u64, stats.blocks_in_use)
}

/// Ticks between steps (16 ms each).
const STEP: usize = 8;
/// Cycles before the memory baseline, so caches and pools warm up.
const WARM_UP: usize = 10;
/// Allowed memory growth from the baseline to the last cycle. One leaked
/// frame at 2x is about 1 MiB.
const GROWTH_LIMIT_KIB: u64 = 1024;

/// Default mode: open each bundle's editor in turn and leave them all open
/// side by side. `--cycles N`: load each bundle once, then open and close
/// all their editors N times, as a user flipping a DAW's plugin window
/// does, checking memory stays flat. Failures exit nonzero.
struct Smoke {
    ctx: WindowContext,
    bundles: Vec<String>,
    cycles: Option<usize>,
    loaded: RefCell<Vec<(String, Loaded)>>,
    ticks: Cell<usize>,
    failed: Cell<bool>,
    baseline: Cell<(u64, u32)>,
}

impl Smoke {
    fn open(&self, name: &str, plugin: &mut PluginInstance<Host>, report: bool) {
        let mut h = plugin.plugin_handle();
        let gui = h.get_extension::<PluginGui>().expect("no gui extension");
        let cfg = GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform().unwrap(),
            is_floating: false,
        };
        let RawWindowHandle::AppKit(parent) = self.ctx.window_handle().unwrap().as_raw() else {
            panic!("gui_smoke supports macOS only");
        };
        let window = ClapWindow::from_cocoa_nsview(parent.ns_view.as_ptr());
        let result = gui
            .create(&mut h, cfg)
            .and_then(|()| unsafe { gui.set_parent(&mut h, window) })
            .and_then(|()| gui.show(&mut h));
        if report || result.is_err() {
            println!("{name}: size {:?}, open {result:?}", gui.get_size(&mut h));
        }
        self.failed.set(self.failed.get() || result.is_err());
    }

    fn close_all(&self) {
        for (_, (_, plugin)) in self.loaded.borrow_mut().iter_mut() {
            let mut h = plugin.plugin_handle();
            h.get_extension::<PluginGui>().unwrap().destroy(&mut h);
        }
    }

    fn finish(&self) {
        self.close_all();
        self.loaded.borrow_mut().clear();
        // Exit directly: baseview's `request_close` on the top-level window
        // is occasionally ignored, leaving the run loop going.
        std::process::exit(i32::from(self.failed.get()));
    }

    /// Default mode, at step `k`.
    fn side_by_side(&self, k: usize) {
        if let Some(name) = self.bundles.get(k) {
            let mut loaded = load_bundle(name);
            self.open(name, &mut loaded.1, true);
            self.loaded.borrow_mut().push((name.clone(), loaded));
        } else if k == self.bundles.len() + 8 {
            self.finish();
        }
    }

    /// `--cycles` mode, at step `k`: even steps open, odd steps close.
    fn cycle(&self, k: usize, cycles: usize) {
        if k == 0 {
            for name in &self.bundles {
                let loaded = load_bundle(name);
                self.loaded.borrow_mut().push((name.clone(), loaded));
            }
        }
        let cycle = k / 2;
        if cycle == cycles {
            let (end, end_blocks) = heap();
            let (base, base_blocks) = self.baseline.get();
            let growth = end.saturating_sub(base);
            println!(
                "{cycles} open/close cycles: heap {base} KiB / {base_blocks} blocks after {WARM_UP}, {end} KiB / {end_blocks} blocks at the end; growth {growth} KiB (limit {GROWTH_LIMIT_KIB})"
            );
            self.failed
                .set(self.failed.get() || growth > GROWTH_LIMIT_KIB);
            self.finish();
            return;
        }
        if k.is_multiple_of(2) {
            let mut loaded = self.loaded.borrow_mut();
            for (name, (_, plugin)) in loaded.iter_mut() {
                self.open(name, plugin, cycle == 0);
            }
        } else {
            self.close_all();
            if cycle + 1 == WARM_UP {
                self.baseline.set(heap());
            }
        }
    }
}

impl WindowHandler for Smoke {
    fn draw(&self) -> Result<(), HandlerError> {
        Ok(())
    }
    fn resized(&self, _size: WindowSize) -> Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, _event: Event) -> EventStatus {
        EventStatus::Ignored
    }
    fn on_timer(&self, _timer: &TimerHandle) {
        let n = self.ticks.get() + 1;
        self.ticks.set(n);
        if !n.is_multiple_of(STEP) {
            return;
        }
        let k = n / STEP - 1;
        match self.cycles {
            Some(cycles) => self.cycle(k, cycles),
            None => self.side_by_side(k),
        }
    }
}

/// Opt out of App Nap, which throttles a background window's timers so
/// far that a run takes minutes. The token must stay alive.
fn keep_awake() -> objc2::rc::Retained<objc2::runtime::AnyObject> {
    use objc2::{class, msg_send};
    // NSActivityUserInitiated | NSActivityLatencyCritical
    let options: u64 = 0x00FF_FFFF | 0xFF_0000_0000;
    // SAFETY: documented Foundation calls with matching argument types.
    unsafe {
        let info: *mut objc2::runtime::AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let reason: *mut objc2::runtime::AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"gui_smoke".as_ptr()];
        msg_send![info, beginActivityWithOptions: options, reason: reason]
    }
}

pub fn main() {
    let _awake = keep_awake();
    let mut args = std::env::args().skip(1).peekable();
    let mut cycles = None;
    let mut bundles = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--cycles" {
            let n: usize = args
                .next()
                .and_then(|n| n.parse().ok())
                .expect("--cycles N");
            cycles = Some(n.max(WARM_UP + 1));
        } else {
            bundles.push(arg);
        }
    }
    if bundles.is_empty() {
        bundles = vec!["SilkscreenGain".into(), "SilkscreenTone".into()];
    }
    let settings = WindowSettings::new()
        .with_title("gui_smoke")
        .with_size(baseview::dpi::LogicalSize::new(800.0, 600.0));
    let window = Window::create(settings, move |ctx| {
        let _ = ctx.create_timer(Duration::from_millis(16));
        Ok(Smoke {
            ctx,
            bundles,
            cycles,
            loaded: RefCell::default(),
            ticks: Cell::default(),
            failed: Cell::default(),
            baseline: Cell::default(),
        })
    })
    .unwrap();
    window.run_until_closed().unwrap();
}
