#![forbid(unsafe_code)]

mod app;
mod theme;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 840.0])
            .with_min_inner_size([960.0, 720.0])
            .with_title("wsi-dicom"),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "wsi-dicom",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::WsiDicomGui::default()))
        }),
    )
}
