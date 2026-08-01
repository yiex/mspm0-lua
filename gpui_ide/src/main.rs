// GUI app: no console window when double-clicked.
#![windows_subsystem = "windows"]

mod app;
mod bsl;
mod color_wheel;
mod compile;
mod console;
mod editor;
mod fontpack;
mod line_input;
mod metadata;
mod modular;
mod project;
mod scrollbar;
mod serial;
mod settings;
mod snippets;
mod syntax;
mod telemetry;
mod theme;

fn main() {
    // HWND swapchain (no DComp): DComp left half transparent after snap undock.
    // SAFETY: process init before UI/GPU work.
    unsafe {
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1");
    }
    app::run();
}
