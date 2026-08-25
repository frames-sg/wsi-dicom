use eframe::egui::Color32;

use crate::theme;

use super::WsiDicomGui;

impl WsiDicomGui {
    pub(super) fn status_color(&self) -> (Color32, Color32) {
        if self.running {
            (theme::SAND, theme::SAND_INK)
        } else if self.status.starts_with("Export failed") || self.status.starts_with("Failed") {
            (theme::MAUVE, theme::MAUVE_INK)
        } else if self.status.starts_with("Exported") || self.status.starts_with("Report saved") {
            (theme::SAGE, theme::SAGE_INK)
        } else {
            (theme::STEEL, theme::STEEL_INK)
        }
    }

    pub(super) fn status_label(&self) -> &'static str {
        if self.running {
            "running"
        } else if self.status.starts_with("Export failed") || self.status.starts_with("Failed") {
            "failed"
        } else if self.status.starts_with("Exported") {
            "completed"
        } else if self.status.starts_with("Report saved") {
            "report saved"
        } else if self.source_path.is_some() && self.output_dir.is_some() {
            "ready"
        } else {
            "awaiting input"
        }
    }

    pub(super) fn save_report(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("wsi-dicom-report.json")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, self.report_json.as_bytes()) {
            Ok(()) => self.status = format!("Report saved to {}", path.display()),
            Err(err) => self.status = format!("Failed to save report: {err}"),
        }
    }
}
