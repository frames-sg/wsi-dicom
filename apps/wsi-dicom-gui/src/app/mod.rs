use crate::theme;
mod mapping;
mod presenter;
mod state;
mod view;
mod worker;

pub(crate) use state::WsiDicomGui;

#[cfg(test)]
use std::sync::mpsc;
#[cfg(test)]
use wsi_dicom::AnnotationTarget;

use eframe::egui::{self, Margin, Stroke};

impl eframe::App for WsiDicomGui {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::PAPER.to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();
        if self.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Header bar — same canvas color as the body so the cards float on
        // one continuous neutral surface (modern app shell, not a banded
        // header).
        let header_frame = egui::Frame::new()
            .fill(theme::PAPER)
            .stroke(Stroke::NONE)
            .inner_margin(Margin {
                left: 28,
                right: 28,
                top: 18,
                bottom: 14,
            });
        egui::Panel::top("top_strip")
            .resizable(false)
            .exact_size(88.0)
            .frame(header_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| self.top_strip(ui));

        let body_frame = egui::Frame::new().fill(theme::PAPER).inner_margin(Margin {
            left: 28,
            right: 28,
            top: 2,
            bottom: 18,
        });
        egui::CentralPanel::default()
            .frame(body_frame)
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 14.0;
                self.sources_card(ui);
                self.options_row(ui);
                self.report_card(ui);
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_worker_reaches_visible_terminal_failure() {
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut gui = WsiDicomGui {
            receiver: Some(receiver),
            running: true,
            status: "Export running...".to_string(),
            ..WsiDicomGui::default()
        };

        gui.poll_worker();

        assert!(!gui.running);
        assert!(gui.receiver.is_none());
        assert_eq!(gui.status, "Export failed.");
        assert!(gui.report_json.contains("worker stopped"));
    }

    #[test]
    fn annotation_selection_requires_inputs_and_at_least_one_target() {
        let mut gui = WsiDicomGui {
            convert_annotations: true,
            ..WsiDicomGui::default()
        };
        assert!(gui.selected_annotation_options().is_err());

        gui.annotation_geojson_path = Some("case.geojson".into());
        gui.annotation_mapping_path = Some("mapping.json".into());
        gui.annotation_target_ann = false;
        assert!(gui.selected_annotation_options().is_err());

        gui.annotation_target_seg = true;
        let options = gui.selected_annotation_options().unwrap().unwrap();
        assert_eq!(options.targets, vec![AnnotationTarget::Seg]);
    }
}
