//! The CLAP gui and timer extensions: the editor embedded in the host's
//! window ([design/hosts.md]).
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use std::sync::atomic::Ordering;

use clack_extensions::gui::{
    AspectRatioStrategy, GuiApiType, GuiConfiguration, GuiResizeHints, GuiSize, HostGui,
    PluginGuiImpl, Window,
};
use clack_extensions::params::HostParams;
use clack_extensions::timer::{HostTimer, PluginTimerImpl, TimerId};
use clack_plugin::prelude::*;
use silkscreen::core::Size;
use silkscreen_host_native::{ParentWindow, WindowOptions, open_embedded, zoom_for_size};

use crate::{MainThread, SilkscreenPlugin};

/// Host timer period while the editor is open. It drives the editor's tick
/// (host changes reaching the screen), so keep it near a frame.
const TIMER_MS: u32 = 16;

/// The CLAP size of a `base` faceplate at `zoom`. `unit` is physical
/// pixels per logical unit as the host counts them.
fn gui_size(base: Size, zoom: f32, unit: f64) -> GuiSize {
    GuiSize {
        width: (f64::from(base.w * zoom) * unit).round() as u32,
        height: (f64::from(base.h * zoom) * unit).round() as u32,
    }
}

/// The zoom step nearest a host-proposed `size`, and its exact size.
fn snap(base: Size, unit: f64, size: GuiSize) -> (f32, GuiSize) {
    let zoom = zoom_for_size(
        base,
        f64::from(size.width) / unit,
        f64::from(size.height) / unit,
    );
    (zoom, gui_size(base, zoom, unit))
}

impl<P: SilkscreenPlugin> MainThread<'_, P> {
    /// Fold host changes into the model, counting them for diagnostics.
    pub(crate) fn pump(&mut self) {
        if let Ok(mut state) = self.state.try_borrow_mut() {
            self.host_changes += state.pump() as u64;
        }
    }

    /// What the badge is made from: with diagnostics, the counts and
    /// the transport; without, nothing that changes.
    fn badge_from(&self) -> (u64, u64, u64, bool) {
        if P::DIAGNOSTICS {
            (
                self.host_changes,
                self.ticks,
                self.shared.tempo.load(Ordering::Relaxed),
                self.shared.playing.load(Ordering::Relaxed),
            )
        } else {
            (0, 0, 0, false)
        }
    }

    fn badge(&self) -> String {
        let mut text = format!("v{}", P::VERSION);
        if !P::BUILD.is_empty() {
            text.push(' ');
            text.push_str(P::BUILD);
        }
        if P::DIAGNOSTICS {
            use std::fmt::Write as _;
            let _ = write!(
                text,
                " hc{} tk{} {:.1} {}",
                self.host_changes,
                self.ticks,
                f64::from_bits(self.shared.tempo.load(Ordering::Relaxed)),
                if self.shared.playing.load(Ordering::Relaxed) {
                    "play"
                } else {
                    "stop"
                }
            );
        }
        text
    }

    fn window_options(&self) -> WindowOptions {
        let mut options = WindowOptions::new(P::NAME, self.base_size());
        options.zoom = self.zoom;
        options.zoom_keys = false;
        options.in_host = true;
        options
    }

    /// The faceplate's logical size.
    fn base_size(&self) -> Size {
        self.state.borrow().layout().size
    }

    /// Physical pixels per logical unit in CLAP sizes: CLAP sizes are
    /// physical, except on macOS where hosts work in points.
    fn gui_unit(&self) -> f64 {
        if cfg!(target_os = "macos") {
            1.0
        } else {
            self.host_scale
        }
    }

    fn apply_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
        if let Some(gui) = &self.gui {
            gui.set_zoom(zoom);
        }
    }

    /// A zoom from saved state. With the editor open, the host must agree
    /// to the new size first.
    pub(crate) fn restore_zoom(&mut self, zoom: f32) {
        if zoom == self.zoom {
            return;
        }
        if self.gui.is_none() {
            self.zoom = zoom;
            return;
        }
        let size = gui_size(self.base_size(), zoom, self.gui_unit());
        let shared = self.host.shared();
        let accepted = shared
            .get_extension::<HostGui>()
            .is_some_and(|gui| gui.request_resize(&shared, size.width, size.height).is_ok());
        if accepted {
            self.apply_zoom(zoom);
        }
    }

    /// Ask the host to call `flush` if the UI sent something while the
    /// audio thread isn't running to carry it.
    pub(crate) fn request_flush_if_needed(&mut self) {
        let sent = self
            .state
            .try_borrow_mut()
            .is_ok_and(|mut s| s.take_engine_activity());
        if sent && !self.shared.processing.load(Ordering::Acquire) {
            let shared = self.host.shared();
            if let Some(params) = shared.get_extension::<HostParams>() {
                params.request_flush(&shared);
            }
        }
    }
}

impl<P: SilkscreenPlugin> PluginGuiImpl for MainThread<'_, P> {
    fn is_api_supported(&mut self, configuration: GuiConfiguration) -> bool {
        Some(configuration.api_type) == GuiApiType::default_for_current_platform()
            && !configuration.is_floating
    }

    fn get_preferred_api(&mut self) -> Option<GuiConfiguration<'_>> {
        Some(GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        })
    }

    fn create(&mut self, configuration: GuiConfiguration) -> Result<(), PluginError> {
        if self.is_api_supported(configuration) {
            Ok(())
        } else {
            Err(PluginError::Message("unsupported GUI configuration"))
        }
    }

    fn destroy(&mut self) {
        if let Some((timer, id)) = self.timer.take() {
            let _ = timer.unregister_timer(&mut self.host, id);
        }
        // Closing the window ends any drag, closing its host gesture; the
        // timer is gone, so flush now if that produced anything.
        self.gui = None;
        self.request_flush_if_needed();
    }

    fn set_scale(&mut self, scale: f64) -> Result<(), PluginError> {
        // macOS hosts don't call this; baseview reads the backing scale.
        self.host_scale = scale;
        Ok(())
    }

    fn get_size(&mut self) -> Option<GuiSize> {
        Some(gui_size(self.base_size(), self.zoom, self.gui_unit()))
    }

    fn can_resize(&mut self) -> bool {
        true
    }

    fn get_resize_hints(&mut self) -> Option<GuiResizeHints> {
        let base = self.base_size();
        Some(GuiResizeHints {
            can_resize_horizontally: true,
            can_resize_vertically: true,
            strategy: AspectRatioStrategy::Preserve {
                width: base.w.round() as u32,
                height: base.h.round() as u32,
            },
        })
    }

    /// Snap to the nearest zoom step (design/rendering.md).
    fn adjust_size(&mut self, size: GuiSize) -> Option<GuiSize> {
        Some(snap(self.base_size(), self.gui_unit(), size).1)
    }

    fn set_size(&mut self, size: GuiSize) -> Result<(), PluginError> {
        let (zoom, _) = snap(self.base_size(), self.gui_unit(), size);
        self.apply_zoom(zoom);
        Ok(())
    }

    fn set_parent(&mut self, window: Window) -> Result<(), PluginError> {
        #[allow(deprecated)] // clack implements only the raw-handle trait for rwh 0.6
        let raw = raw_window_handle::HasRawWindowHandle::raw_window_handle(&window)
            .map_err(|_| PluginError::Message("host window has no usable handle"))?;
        // SAFETY: CLAP guarantees the parent window outlives the editor
        // until `destroy`, which drops the embedded window first.
        #[allow(unsafe_code)]
        let parent = unsafe { ParentWindow::new(raw) };
        let embedded = open_embedded(
            &parent,
            self.window_options(),
            self.state.clone(),
            P::UI.raster(),
        )
        .map_err(|e| {
            eprintln!("{}: could not open the editor: {e}", P::ID);
            PluginError::Message("could not open the editor window")
        })?;
        // A new window shows the badge from scratch, once.
        embedded.set_badge(Some(self.badge()));
        self.badge_from = Some(self.badge_from());
        self.gui = Some(embedded);

        if self.timer.is_none()
            && let Some(timer) = self.host.shared().get_extension::<HostTimer>()
            && let Ok(id) = timer.register_timer(&mut self.host, TIMER_MS)
        {
            self.timer = Some((timer, id));
        }
        Ok(())
    }

    fn set_transient(&mut self, _window: Window) -> Result<(), PluginError> {
        Ok(())
    }

    fn suggest_title(&mut self, _title: &str) {}

    fn show(&mut self) -> Result<(), PluginError> {
        if let Some(gui) = &self.gui {
            gui.show();
        }
        Ok(())
    }

    fn hide(&mut self) -> Result<(), PluginError> {
        if let Some(gui) = &self.gui {
            gui.hide();
        }
        Ok(())
    }
}

impl<P: SilkscreenPlugin> PluginTimerImpl for MainThread<'_, P> {
    fn on_timer(&mut self, _timer_id: TimerId) {
        self.ticks += 1;
        self.pump();
        if let Some(gui) = &self.gui {
            // Rebuilt only when what it shows changes: without
            // diagnostics, never after the window's first.
            let from = self.badge_from();
            if self.badge_from != Some(from) {
                self.badge_from = Some(from);
                gui.set_badge(Some(self.badge()));
            }
            gui.tick();
        }
        self.request_flush_if_needed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Size = Size::new(400.0, 300.0);

    fn size(width: u32, height: u32) -> GuiSize {
        GuiSize { width, height }
    }

    #[test]
    fn adjust_snaps_to_the_nearest_step_in_points() {
        assert_eq!(snap(BASE, 1.0, size(400, 300)), (1.0, size(400, 300)));
        assert_eq!(snap(BASE, 1.0, size(480, 360)), (1.25, size(500, 375)));
        assert_eq!(snap(BASE, 1.0, size(560, 420)), (1.5, size(600, 450)));
        assert_eq!(snap(BASE, 1.0, size(5000, 4000)), (2.0, size(800, 600)));
        assert_eq!(snap(BASE, 1.0, size(1, 1)), (0.75, size(300, 225)));
    }

    #[test]
    fn adjust_works_in_physical_pixels_at_a_display_scale() {
        // 2x display at 125%: 1000x750 physical.
        assert_eq!(snap(BASE, 2.0, size(1000, 750)), (1.25, size(1000, 750)));
        assert_eq!(snap(BASE, 1.5, size(700, 520)), (1.25, size(750, 563)));
    }

    #[test]
    fn a_snapped_size_is_a_fixed_point() {
        for unit in [1.0, 1.25, 1.5, 2.0] {
            for &zoom in &silkscreen_host_native::ZOOM_STEPS {
                let exact = gui_size(BASE, zoom, unit);
                assert_eq!(snap(BASE, unit, exact), (zoom, exact), "{unit} {zoom}");
            }
        }
    }

    #[test]
    fn one_dragged_edge_still_changes_the_step() {
        assert_eq!(snap(BASE, 1.0, size(640, 300)).0, 1.25);
        assert_eq!(snap(BASE, 1.0, size(400, 120)).0, 0.75);
    }
}
