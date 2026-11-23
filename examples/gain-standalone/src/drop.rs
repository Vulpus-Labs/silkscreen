//! A binding that reports files dropped on its control, then defers to
//! the binding it wraps: proof that an OS file drop reaches a control.

use silkscreen::Binding;
use silkscreen::core::{ControlEvent, ControlSpec, DropPayload, Layout, WidgetState};
use silkscreen::model::{Edit, Model};
use silkscreen::{ParamIndex, ParamSpec};

/// Wraps `B`, printing the paths of anything dropped on the control.
#[derive(Debug)]
pub struct DropLogger<B>(pub B);

/// What a drop reports: one line per path.
pub fn describe_drop(payload: &DropPayload) -> Vec<String> {
    match payload {
        DropPayload::Paths(paths) => paths
            .iter()
            .map(|p| format!("dropped on the knob: {}", p.display()))
            .collect(),
        DropPayload::Files(handles) => {
            vec![format!("dropped on the knob: {} files", handles.len())]
        }
    }
}

impl<B: Binding> Binding for DropLogger<B> {
    fn cursor(&self) -> Option<silkscreen::core::MouseCursor> {
        self.0.cursor()
    }

    fn on_event(&mut self, event: &ControlEvent, edit: &mut Edit<'_>) {
        if let ControlEvent::Drop { payload } = event {
            for line in describe_drop(payload) {
                println!("gain-standalone: {line}");
            }
        }
        self.0.on_event(event, edit);
    }

    fn display(&self, model: &Model) -> WidgetState {
        self.0.display(model)
    }

    fn params(&self) -> &[ParamIndex] {
        self.0.params()
    }

    fn describe(&self, model: &Model) -> Option<String> {
        self.0.describe(model)
    }

    fn attach(
        &mut self,
        control: &ControlSpec,
        specs: &[ParamSpec],
        layout: &Layout,
    ) -> Result<(), String> {
        self.0.attach(control, specs, layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_dropped_path_is_reported() {
        let payload = DropPayload::Paths(vec!["/a/b.wav".into(), "/c.aif".into()]);
        assert_eq!(
            describe_drop(&payload),
            [
                "dropped on the knob: /a/b.wav",
                "dropped on the knob: /c.aif"
            ]
        );
    }
}
