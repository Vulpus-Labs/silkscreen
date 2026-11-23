fn main() {
    // Every control kind, drawn in the library's default style: a plugin
    // brings its own skins (design/faceplates.md).
    silkscreen_build::Faceplate::new("widgets.svg").build();
    // The live displays: a scope and a meter, no skins.
    silkscreen_build::Faceplate::new("live.svg").build();
    // A canvas control the plugin paints (design/faceplates.md).
    silkscreen_build::Faceplate::new("canvas.svg").build();
    // A page group: three tabs, each page with its own art and controls.
    silkscreen_build::Faceplate::new("pages.svg").build();
    // The factory presets (design/presets.md), checked and embedded.
    silkscreen_build::FactoryBank::new("presets/factory").build();
}
