#![forbid(unsafe_code)]

mod theme;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Color32, Margin, RichText, Stroke, TextStyle, Vec2};
use wsi_dicom::{
    validate_dicom_path, CodecValidation, EncodeBackendPreference, Export, ExportOptions,
    IccProfilePolicy, JpegDirectHtj2kProfile, MetadataSource, TransferSyntax, ValidationOptions,
    METADATA_JSON_MAX_BYTES,
};

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
            Ok(Box::new(WsiDicomGui::default()))
        }),
    )
}

struct WsiDicomGui {
    source_path: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
    transfer_syntax: TransferSyntax,
    jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    icc_profile_policy: IccProfilePolicy,
    codec_validation: CodecValidation,
    tile_size: u32,
    jpeg_quality: u8,
    overwrite: bool,
    validate_after_export: bool,
    validation_strict: bool,
    htj2k_decoder: String,
    running: bool,
    receiver: Option<Receiver<GuiRunResult>>,
    status: String,
    report_json: String,
}

impl Default for WsiDicomGui {
    fn default() -> Self {
        let options = ExportOptions::default();
        Self {
            source_path: None,
            output_dir: None,
            metadata_path: None,
            research_placeholder: false,
            transfer_syntax: options.transfer_syntax,
            jpeg_direct_htj2k_profile: options.jpeg_direct_htj2k_profile,
            icc_profile_policy: options.icc_profile_policy,
            codec_validation: options.codec_validation,
            tile_size: options.tile_size,
            jpeg_quality: options.jpeg_quality,
            overwrite: options.overwrite,
            validate_after_export: true,
            validation_strict: false,
            htj2k_decoder: String::new(),
            running: false,
            receiver: None,
            status: "Select a source slide and output directory.".to_string(),
            report_json: String::new(),
        }
    }
}

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

impl WsiDicomGui {
    fn status_color(&self) -> (Color32, Color32) {
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

    fn status_label(&self) -> &'static str {
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

    fn top_strip(&mut self, ui: &mut egui::Ui) {
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

    fn sources_card(&mut self, ui: &mut egui::Ui) {
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
            },
        );
    }

    fn options_row(&mut self, ui: &mut egui::Ui) {
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
                grid_label(ui, "ICC policy");
                inline_combo(
                    ui,
                    "icc_policy",
                    icc_policy_label(self.icc_profile_policy),
                    |ui| {
                        for candidate in [
                            IccProfilePolicy::FallbackSrgb,
                            IccProfilePolicy::FallbackDisplayP3,
                            IccProfilePolicy::Strict,
                            IccProfilePolicy::OmitIfMissing,
                        ] {
                            ui.selectable_value(
                                &mut self.icc_profile_policy,
                                candidate,
                                icc_policy_label(candidate),
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

    fn report_card(&mut self, ui: &mut egui::Ui) {
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

impl WsiDicomGui {
    fn start_export(&mut self) {
        let Some(source_path) = self.source_path.clone() else {
            self.status = "Choose a source slide first.".to_string();
            return;
        };
        let Some(output_dir) = self.output_dir.clone() else {
            self.status = "Choose an output directory first.".to_string();
            return;
        };
        let request = GuiRunRequest {
            source_path,
            output_dir,
            metadata_path: self.metadata_path.clone(),
            research_placeholder: self.research_placeholder,
            transfer_syntax: self.transfer_syntax,
            jpeg_direct_htj2k_profile: self.jpeg_direct_htj2k_profile,
            icc_profile_policy: self.icc_profile_policy,
            codec_validation: self.codec_validation,
            tile_size: self.tile_size,
            jpeg_quality: self.jpeg_quality,
            overwrite: self.overwrite,
            validate_after_export: self.validate_after_export,
            validation_strict: self.validation_strict,
            htj2k_decoder: (!self.htj2k_decoder.trim().is_empty())
                .then(|| self.htj2k_decoder.trim().to_string()),
        };
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(run_export(request));
        });
        self.receiver = Some(receiver);
        self.running = true;
        self.status = "Export running...".to_string();
        self.report_json.clear();
    }

    fn poll_worker(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.running = false;
        self.receiver = None;
        match result {
            Ok(report) => {
                self.status = report.summary;
                self.report_json = report.json;
            }
            Err(message) => {
                self.status = "Export failed.".to_string();
                self.report_json = message;
            }
        }
    }

    fn save_report(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("wsi-dicom-report.json")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, self.report_json.as_bytes()) {
            Ok(()) => {
                self.status = format!("Report saved to {}", path.display());
            }
            Err(err) => {
                self.status = format!("Failed to save report: {err}");
            }
        }
    }
}

struct GuiRunRequest {
    source_path: PathBuf,
    output_dir: PathBuf,
    metadata_path: Option<PathBuf>,
    research_placeholder: bool,
    transfer_syntax: TransferSyntax,
    jpeg_direct_htj2k_profile: JpegDirectHtj2kProfile,
    icc_profile_policy: IccProfilePolicy,
    codec_validation: CodecValidation,
    tile_size: u32,
    jpeg_quality: u8,
    overwrite: bool,
    validate_after_export: bool,
    validation_strict: bool,
    htj2k_decoder: Option<String>,
}

struct GuiRunReport {
    summary: String,
    json: String,
}

type GuiRunResult = Result<GuiRunReport, String>;

fn run_export(request: GuiRunRequest) -> GuiRunResult {
    let metadata = load_metadata_source(
        request.metadata_path.as_deref(),
        request.research_placeholder,
    )?;
    let mut options = ExportOptions::default();
    options.tile_size = request.tile_size;
    options.transfer_syntax = request.transfer_syntax;
    options.jpeg_direct_htj2k_profile = request.jpeg_direct_htj2k_profile;
    options.jpeg_quality = request.jpeg_quality;
    options.overwrite = request.overwrite;
    options.icc_profile_policy = request.icc_profile_policy;
    options.codec_validation = request.codec_validation;
    options.encode_backend = EncodeBackendPreference::Auto;
    options.validate().map_err(|err| err.to_string())?;
    if options.transfer_syntax != TransferSyntax::Htj2k {
        options.jpeg_direct_htj2k_profile =
            JpegDirectHtj2kProfile::default_for_transfer_syntax(options.transfer_syntax);
    }
    let export_report = Export::from_slide(&request.source_path)
        .to_directory(&request.output_dir)
        .with_metadata(metadata)
        .with_options(options)
        .run()
        .map_err(|err| err.to_string())?;
    let validation_report = if request.validate_after_export {
        let mut validation = ValidationOptions::default();
        validation.strict = request.validation_strict;
        validation.htj2k_decoder = request.htj2k_decoder;
        Some(validate_dicom_path(&request.output_dir, &validation).map_err(|err| err.to_string())?)
    } else {
        None
    };
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "export": export_report,
        "validation": validation_report,
    }))
    .map_err(|err| err.to_string())?;
    let summary = match &validation_report {
        Some(report) => format!(
            "Exported {} instance(s); validation passed={} failed={} skipped={}.",
            export_report.instances.len(),
            report.passed_checks(),
            report.failed_checks(),
            report.skipped_checks()
        ),
        None => format!("Exported {} instance(s).", export_report.instances.len()),
    };
    Ok(GuiRunReport { summary, json })
}

fn load_metadata_source(
    metadata_path: Option<&Path>,
    research_placeholder: bool,
) -> Result<MetadataSource, String> {
    if metadata_path.is_some() && research_placeholder {
        return Err("Choose metadata JSON or research placeholder metadata, not both.".to_string());
    }
    if research_placeholder {
        return Ok(MetadataSource::ResearchPlaceholder);
    }
    let Some(path) = metadata_path else {
        return Err("Choose metadata JSON or enable research placeholder metadata.".to_string());
    };
    let bytes = read_capped_file(path, METADATA_JSON_MAX_BYTES)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| format!("parse metadata JSON {}: {err}", path.display()))?;
    MetadataSource::from_json_value(value)
        .map_err(|err| format!("parse strict DICOM metadata {}: {err}", path.display()))
}

fn read_capped_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>, String> {
    let file =
        std::fs::File::open(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    let mut limited = file.take(max_bytes.saturating_add(1));
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|err| format!("read {}: {err}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(format!(
            "metadata JSON {} exceeds {} byte limit",
            path.display(),
            max_bytes
        ));
    }
    Ok(bytes)
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

fn transfer_syntax_label(value: TransferSyntax) -> &'static str {
    match value {
        TransferSyntax::JpegBaseline8Bit => "JPEG Baseline 8-bit",
        TransferSyntax::Jpeg2000 => "JPEG 2000",
        TransferSyntax::Jpeg2000Lossless => "JPEG 2000 Lossless",
        TransferSyntax::Htj2k => "HTJ2K",
        TransferSyntax::Htj2kLossless => "HTJ2K Lossless",
        TransferSyntax::Htj2kLosslessRpcl => "HTJ2K Lossless RPCL",
        TransferSyntax::ExplicitVrLittleEndian => "Explicit VR Little Endian",
        _ => "Unknown transfer syntax",
    }
}

fn htj2k_profile_label(value: JpegDirectHtj2kProfile) -> &'static str {
    match value {
        JpegDirectHtj2kProfile::Lossless53 => "5/3 lossless",
        JpegDirectHtj2kProfile::Lossy97 => "9/7 balanced",
        JpegDirectHtj2kProfile::Lossy97Near => "9/7 near-lossless",
        JpegDirectHtj2kProfile::Lossy97Balanced => "9/7 balanced",
        JpegDirectHtj2kProfile::Lossy97Aggressive => "9/7 aggressive",
        JpegDirectHtj2kProfile::Lossy97Preview => "9/7 preview",
        JpegDirectHtj2kProfile::Lossy97Thumbnail => "9/7 thumbnail",
        _ => "Unknown profile",
    }
}

fn icc_policy_label(value: IccProfilePolicy) -> &'static str {
    match value {
        IccProfilePolicy::Strict => "Strict",
        IccProfilePolicy::FallbackSrgb => "Fallback sRGB",
        IccProfilePolicy::FallbackDisplayP3 => "Fallback Display P3",
        IccProfilePolicy::OmitIfMissing => "Omit if missing",
        _ => "Unknown ICC policy",
    }
}

fn codec_validation_label(value: CodecValidation) -> &'static str {
    match value {
        CodecValidation::Disabled => "Disabled",
        CodecValidation::RoundTrip => "Round trip",
        _ => "Unknown validation",
    }
}
