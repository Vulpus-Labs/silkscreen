//! [`export_page!`](crate::export_page): a synth's page wrapper for
//! JavaScript.

/// Export a synth's web page to JavaScript: a `#[wasm_bindgen]` newtype
/// over [`PresetPage`](crate::PresetPage), with its methods, and the
/// `start` the page calls (`silkscreen-page.mjs`).
///
/// `wasm_bindgen` can't export a generic type, so each synth's crate
/// names its own: `$name` is the class JavaScript sees, `$ui` the
/// synth's generated `UI`.
///
/// ```ignore
/// silkscreen_host_web::export_page!(TonePage, tone::UI);
/// ```
///
/// The crate needs `silkscreen-host-web` and nothing else from
/// wasm-bindgen: the expansion names it, `js-sys`, `web-sys` and
/// `wasm-bindgen-futures` through this crate.
// rustfmt re-indents the attributes' arguments on every run.
#[rustfmt::skip]
#[macro_export]
macro_rules! export_page {
    ($name:ident, $ui:expr) => {
        // wasm_bindgen's expansion names `wasm_bindgen` and
        // `wasm_bindgen_futures` from the caller's scope in places, whatever
        // its options say, and from the crate root in others.
        #[allow(unused_imports)]
        use $crate::__private::{wasm_bindgen, wasm_bindgen_futures};

        /// The running page: the editor, the buffer to hand the worklet,
        /// and the MIDI ring's writer for the keyboard, over a
        /// [`PresetPage`](silkscreen_host_web::PresetPage).
        #[$crate::__private::wasm_bindgen::prelude::wasm_bindgen(wasm_bindgen = $crate::__private::wasm_bindgen)]
        #[derive(Debug)]
        pub struct $name($crate::PresetPage);

        #[$crate::__private::wasm_bindgen::prelude::wasm_bindgen(wasm_bindgen = $crate::__private::wasm_bindgen)]
        impl $name {
            /// The shared buffer for `startAudio`, or `undefined` if the
            /// page isn't cross-origin isolated.
            #[wasm_bindgen(getter)]
            pub fn sab(
                &self,
            ) -> ::core::option::Option<$crate::__private::js_sys::SharedArrayBuffer> {
                self.0.sab()
            }

            /// Send one MIDI 1.0 channel message to the engine. Returns
            /// `false` if there's no engine link or the ring is full.
            pub fn midi(&mut self, status: u8, data1: u8, data2: u8) -> bool {
                self.0.midi(status, data1, data2)
            }

            /// Whether the editor wants the keys: a panel (a menu, the
            /// matrix) is open. The page's computer-keyboard notes stand
            /// aside while it does.
            #[wasm_bindgen(js_name = editorWantsKeys)]
            pub fn editor_wants_keys(&self) -> bool {
                self.0.editor_wants_keys()
            }

            /// Show the faceplate at `zoom` CSS pixels per logical pixel,
            /// sharp at any zoom; `silkscreen-fit.mjs` fits it to the page
            /// with this.
            #[wasm_bindgen(js_name = setZoom)]
            pub fn set_zoom(&self, zoom: f64) {
                self.0.set_zoom(zoom);
            }

            /// Rects put on the canvas since mount, for diagnostics.
            pub fn puts(&self) -> u32 {
                self.0.puts()
            }

            /// Save the current sound as user preset `name` at the root,
            /// for the console and checks.
            #[wasm_bindgen(js_name = savePreset)]
            pub fn save_preset(
                &mut self,
                name: &str,
            ) -> ::core::result::Result<(), $crate::__private::wasm_bindgen::JsValue> {
                self.0.save_preset(name)
            }

            /// Every user preset's key (`name` or `folder/name`).
            #[wasm_bindgen(js_name = presetKeys)]
            pub fn preset_keys(&self) -> ::std::vec::Vec<::std::string::String> {
                self.0.preset_keys()
            }

            /// Why presets can't be kept, if they can't.
            #[wasm_bindgen(js_name = presetProblem)]
            pub fn preset_problem(&self) -> ::core::option::Option<::std::string::String> {
                self.0.preset_problem()
            }

            /// Set param `index` to plain `value` as the user would, for
            /// the console and checks.
            #[wasm_bindgen(js_name = setParam)]
            pub fn set_param(&self, index: u32, value: f32) {
                self.0.set_param(index, value);
            }

            /// Param `index`'s plain value.
            #[wasm_bindgen(js_name = getParam)]
            pub fn get_param(&self, index: u32) -> f32 {
                self.0.get_param(index)
            }

            /// The current preset's name, if any.
            #[wasm_bindgen(js_name = presetName)]
            pub fn preset_name(&self) -> ::core::option::Option<::std::string::String> {
                self.0.preset_name()
            }

            /// Share the current sound (as the browser's Share does) and
            /// return the link.
            #[cfg(target_arch = "wasm32")]
            #[wasm_bindgen(js_name = shareLink)]
            pub fn share_link(&self) -> ::core::option::Option<::std::string::String> {
                self.0.share_link()
            }

            /// Whether preset changes are still waiting to be written.
            #[wasm_bindgen(js_name = presetsPending)]
            pub fn presets_pending(&self) -> bool {
                self.0.presets_pending()
            }
        }

        /// Open the user's presets, then mount the editor on `canvas` and
        /// start drawing.
        #[$crate::__private::wasm_bindgen::prelude::wasm_bindgen(wasm_bindgen = $crate::__private::wasm_bindgen, wasm_bindgen_futures = $crate::__private::wasm_bindgen_futures)]
        pub async fn start(
            canvas: $crate::__private::web_sys::HtmlCanvasElement,
        ) -> ::core::result::Result<$name, $crate::__private::wasm_bindgen::JsValue> {
            $crate::PresetPage::start(canvas, $ui).await.map($name)
        }
    };
}
