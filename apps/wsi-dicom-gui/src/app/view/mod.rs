use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, TextStyle, Vec2};
use wsi_dicom::{
    AnnotationCoordinateSpace, CodecValidation, JpegDirectHtj2kProfile, TransferSyntax,
};

use super::mapping::{
    annotation_coordinate_space_label, codec_validation_label, color_management_label,
    htj2k_profile_label, transfer_syntax_label, GuiColorManagement,
};
use super::WsiDicomGui;
use crate::theme;

impl WsiDicomGui {
    pub(super) fn top_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::brand_mark(ui, 36.0);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(-2.0);
                ui.label(
                    RichText::new("wsi-dicom")
                        .family(theme::display_family())
                        .size(26.0)
                        .color(theme::INK),
                );
                ui.label(
                    RichText::new(&self.status)
                        .family(theme::body_family())
                        .size(13.5)
                        .color(theme::INK_MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_export =
                    !self.running && self.source_path.is_some() && self.output_dir.is_some();
                if theme::primary_button(ui, "Export DICOM", can_export).clicked() {
                    self.start_export();
                }
                ui.add_space(10.0);
                if theme::secondary_button(ui, "Save report", !self.report_json.is_empty())
                    .clicked()
                {
                    self.save_report();
                }
                ui.add_space(12.0);
                if self.running {
                    ui.spinner();
                    ui.add_space(8.0);
                }
                let (color, deep) = self.status_color();
                theme::status_pill(ui, self.status_label(), color, deep);
            });
        });
    }

    pub(super) fn sources_card(&mut self, ui: &mut egui::Ui) {
        theme::card(
            ui,
            theme::SAND,
            theme::SAND_INK,
            "Sources",
            Some("input slide / output directory / metadata"),
            |ui| {
                let running = self.running;
                path_row(
                    ui,
                    PathRow {
                        label: "Source slide",
                        value_text: path_label(self.source_path.as_deref()),
                        placeholder: self.source_path.is_none(),
                        enabled: !running,
                        button_label: "Browse",
                        pick: || rfd::FileDialog::new().pick_file(),
                    },
                    &mut self.source_path,
                );
                ui.add_space(4.0);
                path_row(
                    ui,
                    PathRow {
                        label: "Output directory",
                        value_text: path_label(self.output_dir.as_deref()),
                        placeholder: self.output_dir.is_none(),
                        enabled: !running,
                        button_label: "Choose",
                        pick: || rfd::FileDialog::new().pick_folder(),
                    },
                    &mut self.output_dir,
                );
                ui.add_space(4.0);
                path_row(
                    ui,
                    PathRow {
                        label: "Metadata",
                        value_text: path_label(self.metadata_path.as_deref()),
                        placeholder: self.metadata_path.is_none(),
                        enabled: !running && !self.research_placeholder,
                        button_label: "Load JSON",
                        pick: || {
                            rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .pick_file()
                        },
                    },
                    &mut self.metadata_path,
                );
                ui.add_space(8.0);
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.research_placeholder, "");
                        ui.label(
                            RichText::new("Use research placeholder metadata")
                                .family(theme::body_family())
                                .size(15.0)
                                .color(theme::INK),
                        );
                        ui.label(
                            RichText::new("· skips strict patient / study fields")
                                .family(theme::mono_family())
                                .size(12.5)
                                .color(theme::INK_FAINT),
                        );
                    });
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.add_enabled_ui(!running, |ui| {
                    toggle_row(
                        ui,
                        &mut self.convert_annotations,
                        "Convert QuPath annotations",
                        "profiled GeoJSON → verified DICOM ANN / SEG / SR sidecars",
                    );
                });
                if self.convert_annotations {
                    path_row(
                        ui,
                        PathRow {
                            label: "QuPath GeoJSON",
                            value_text: path_label(self.annotation_geojson_path.as_deref()),
                            placeholder: self.annotation_geojson_path.is_none(),
                            enabled: !running,
                            button_label: "Load JSON",
                            pick: || {
                                rfd::FileDialog::new()
                                    .add_filter("GeoJSON", &["geojson", "json"])
                                    .pick_file()
                            },
                        },
                        &mut self.annotation_geojson_path,
                    );
                    ui.add_space(4.0);
                    path_row(
                        ui,
                        PathRow {
                            label: "DICOM mapping",
                            value_text: path_label(self.annotation_mapping_path.as_deref()),
                            placeholder: self.annotation_mapping_path.is_none(),
                            enabled: !running,
                            button_label: "Load JSON",
                            pick: || {
                                rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .pick_file()
                            },
                        },
                        &mut self.annotation_mapping_path,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.set_min_width(160.0);
                        theme::field_label(ui, "DICOM targets");
                        ui.checkbox(&mut self.annotation_target_ann, "ANN");
                        ui.checkbox(&mut self.annotation_target_seg, "SEG");
                        ui.checkbox(&mut self.annotation_target_sr, "SR");
                        ui.add_space(16.0);
                        theme::field_label(ui, "Coordinates");
                        egui::ComboBox::from_id_salt("annotation_coordinate_space")
                            .selected_text(annotation_coordinate_space_label(
                                self.annotation_coordinate_space,
                            ))
                            .show_ui(ui, |ui| {
                                for candidate in [
                                    AnnotationCoordinateSpace::Level0Pixels,
                                    AnnotationCoordinateSpace::SourcePixels,
                                    AnnotationCoordinateSpace::SlideMillimeters,
                                ] {
                                    ui.selectable_value(
                                        &mut self.annotation_coordinate_space,
                                        candidate,
                                        annotation_coordinate_space_label(candidate),
                                    );
                                }
                            });
                    });
                }
            },
        );
    }

    pub(super) fn options_row(&mut self, ui: &mut egui::Ui) {
        let running = self.running;
        ui.add_enabled_ui(!running, |ui| {
            ui.columns(2, |cols| {
                self.export_options_card(&mut cols[0]);
                self.validation_options_card(&mut cols[1]);
            });
        });
    }

    fn export_options_card(&mut self, ui: &mut egui::Ui) {
        theme::card(
            ui,
            theme::SAGE,
            theme::SAGE_INK,
            "Export",
            Some("transfer syntax · tiling · quality"),
            |ui| {
                ui.columns(2, |inner| {
                    self.export_route_grid(&mut inner[0]);
                    self.export_quality_grid(&mut inner[1]);
                });
            },
        );
    }

    fn export_route_grid(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("export_left")
            .num_columns(2)
            .spacing([10.0, 10.0])
            .min_col_width(100.0)
            .show(ui, |ui| {
                grid_label(ui, "Transfer");
                inline_combo(
                    ui,
                    "transfer_syntax",
                    transfer_syntax_label(self.transfer_syntax),
                    |ui| {
                        for candidate in [
                            TransferSyntax::JpegBaseline8Bit,
                            TransferSyntax::Jpeg2000,
                            TransferSyntax::Jpeg2000Lossless,
                            TransferSyntax::Htj2k,
                            TransferSyntax::Htj2kLossless,
                            TransferSyntax::Htj2kLosslessRpcl,
                        ] {
                            ui.selectable_value(
                                &mut self.transfer_syntax,
                                candidate,
                                transfer_syntax_label(candidate),
                            );
                        }
                    },
                );
                ui.end_row();

                grid_label(ui, "HTJ2K");
                inline_combo(
                    ui,
                    "htj2k_profile",
                    htj2k_profile_label(self.jpeg_direct_htj2k_profile),
                    |ui| {
                        for candidate in [
                            JpegDirectHtj2kProfile::Lossless53,
                            JpegDirectHtj2kProfile::Lossy97Near,
                            JpegDirectHtj2kProfile::Lossy97Balanced,
                            JpegDirectHtj2kProfile::Lossy97Aggressive,
                            JpegDirectHtj2kProfile::Lossy97Preview,
                            JpegDirectHtj2kProfile::Lossy97Thumbnail,
                        ] {
                            ui.selectable_value(
                                &mut self.jpeg_direct_htj2k_profile,
                                candidate,
                                htj2k_profile_label(candidate),
                            );
                        }
                    },
                );
                ui.end_row();

                grid_label(ui, "Tile size");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.tile_size)
                            .range(1..=4096)
                            .speed(8.0),
                    );
                    ui.label(
                        RichText::new("px")
                            .family(theme::mono_family())
                            .size(13.0)
                            .color(theme::INK_FAINT),
                    );
                });
                ui.end_row();
            });
    }

    fn export_quality_grid(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("export_right")
            .num_columns(2)
            .spacing([10.0, 10.0])
            .min_col_width(100.0)
            .show(ui, |ui| {
                grid_label(ui, "Color management");
                inline_combo(
                    ui,
                    "icc_policy",
                    color_management_label(self.color_management),
                    |ui| {
                        for candidate in [
                            GuiColorManagement::SourceOrSrgb,
                            GuiColorManagement::SourceOrDisplayP3,
                            GuiColorManagement::RequireSource,
                        ] {
                            ui.selectable_value(
                                &mut self.color_management,
                                candidate,
                                color_management_label(candidate),
                            );
                        }
                    },
                );
                ui.end_row();

                grid_label(ui, "Codec check");
                inline_combo(
                    ui,
                    "codec_validation",
                    codec_validation_label(self.codec_validation),
                    |ui| {
                        for candidate in [CodecValidation::Disabled, CodecValidation::RoundTrip] {
                            ui.selectable_value(
                                &mut self.codec_validation,
                                candidate,
                                codec_validation_label(candidate),
                            );
                        }
                    },
                );
                ui.end_row();

                grid_label(ui, "Quality");
                ui.horizontal(|ui| {
                    let w = ui.available_width().max(120.0);
                    ui.style_mut().spacing.slider_width = (w - 60.0).max(80.0);
                    ui.add(
                        egui::Slider::new(&mut self.jpeg_quality, 1..=100)
                            .show_value(true)
                            .trailing_fill(true),
                    );
                });
                ui.end_row();

                grid_label(ui, "Overwrite");
                ui.checkbox(&mut self.overwrite, "");
                ui.end_row();
            });
    }

    fn validation_options_card(&mut self, ui: &mut egui::Ui) {
        theme::card(
            ui,
            theme::MAUVE,
            theme::MAUVE_INK,
            "Validation",
            Some("post-export integrity checks"),
            |ui| {
                toggle_row(
                    ui,
                    &mut self.validate_after_export,
                    "Validate after export",
                    "runs DICOM conformance checks on output",
                );
                toggle_row(
                    ui,
                    &mut self.validation_strict,
                    "Strict validation",
                    "errors on warnings and optional fields",
                );
                ui.add_space(8.0);
                egui::Grid::new("validation_grid")
                    .num_columns(2)
                    .spacing([14.0, 10.0])
                    .min_col_width(140.0)
                    .show(ui, |ui| {
                        grid_label(ui, "HTJ2K decoder");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.htj2k_decoder)
                                .desired_width(f32::INFINITY)
                                .hint_text("external decoder (optional)")
                                .font(TextStyle::Monospace),
                        );
                        ui.end_row();
                    });
            },
        );
    }

    pub(super) fn report_card(&mut self, ui: &mut egui::Ui) {
        let subtitle = if self.report_json.is_empty() {
            "awaiting run"
        } else {
            "export & validation manifest"
        };
        // Let the report body grow into whatever vertical space is left in
        // the central panel — keeps the whole UI in one window without
        // window-level scrolling.
        theme::card(
            ui,
            theme::STEEL,
            theme::STEEL_INK,
            "Report",
            Some(subtitle),
            |ui| {
                let body_h = (ui.available_height() - 4.0).max(64.0);
                if self.report_json.is_empty() {
                    let body_h = body_h.max(70.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), body_h),
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                        |ui| {
                            ui.label(
                                RichText::new(
                                    "no report yet  ·  run an export to see the JSON manifest",
                                )
                                .family(theme::body_family())
                                .size(14.0)
                                .color(theme::INK_FAINT),
                            );
                        },
                    );
                } else {
                    // ~18px per line for mono at 14pt — convert available
                    // height into a row count so the textedit fills the card.
                    let rows = ((body_h - 8.0) / 18.0).floor().max(4.0) as usize;
                    egui::ScrollArea::vertical()
                        .max_height(body_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.report_json)
                                    .desired_rows(rows)
                                    .desired_width(f32::INFINITY)
                                    .font(TextStyle::Monospace)
                                    .interactive(false),
                            );
                        });
                }
            },
        );
    }
}
fn path_label(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "not selected".to_string())
}

// ---------------------------------------------------------------------------
// UI helpers — small composable widget patterns used by the cards above.
// ---------------------------------------------------------------------------

struct PathRow<F> {
    label: &'static str,
    value_text: String,
    placeholder: bool,
    enabled: bool,
    button_label: &'static str,
    pick: F,
}

fn path_row<F>(ui: &mut egui::Ui, config: PathRow<F>, sink: &mut Option<PathBuf>)
where
    F: FnOnce() -> Option<PathBuf>,
{
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(160.0);
            ui.set_max_width(160.0);
            ui.add_space(8.0);
            theme::field_label(ui, config.label);
        });
        let button_width = 108.0;
        let gap = 14.0;
        let value_width = (ui.available_width() - button_width - gap).max(120.0);
        ui.vertical(|ui| {
            ui.set_min_width(value_width);
            ui.set_max_width(value_width);
            theme::path_value(ui, &config.value_text, config.placeholder);
        });
        ui.add_space(gap);
        ui.vertical(|ui| {
            ui.set_min_width(button_width);
            if theme::secondary_button(ui, config.button_label, config.enabled).clicked() {
                if let Some(picked) = (config.pick)() {
                    *sink = Some(picked);
                }
            }
        });
    });
}

/// Right-aligned grid label — sits to the left of an inline input field.
fn grid_label(ui: &mut egui::Ui, label: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        theme::field_label(ui, label);
    });
}

fn inline_combo<F>(ui: &mut egui::Ui, id: &str, selected_text: &str, show_items: F)
where
    F: FnOnce(&mut egui::Ui),
{
    let width = ui.available_width().max(160.0);
    egui::ComboBox::from_id_salt(id)
        .selected_text(
            RichText::new(selected_text)
                .family(theme::body_family())
                .size(15.0)
                .color(theme::INK),
        )
        .width(width)
        .show_ui(ui, show_items);
}

fn toggle_row(ui: &mut egui::Ui, state: &mut bool, title: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(state, "");
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .family(theme::body_family())
                    .size(15.0)
                    .color(theme::INK),
            );
            ui.label(
                RichText::new(description)
                    .family(theme::mono_family())
                    .size(12.5)
                    .color(theme::INK_FAINT),
            );
        });
    });
    ui.add_space(4.0);
}
