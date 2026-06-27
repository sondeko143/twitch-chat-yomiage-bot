use crate::catalog::{relevant_keys, spec, ParamKind, PARAMS};
use crate::fonts::install_japanese_font;
use crate::opkind::OpKind;
use crate::params::build_routes;
use crate::state::{AppState, PipelineStep};
use eframe::egui;
use std::sync::mpsc::{Receiver, TryRecvError};
use tokio::runtime::Runtime;
use vstc::VstcError;
use vstreamer_protos::Response;

enum SendStatus {
    Idle,
    Sending,
    Success,
    Error(String),
}

pub struct GuiApp {
    state: AppState,
    runtime: Runtime,
    status: SendStatus,
    result_rx: Option<Receiver<Result<Response, VstcError>>>,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: Runtime) -> Self {
        install_japanese_font(&cc.egui_ctx);
        let state = cc
            .storage
            .and_then(|s| eframe::get_value::<AppState>(s, eframe::APP_KEY))
            .unwrap_or_default();
        Self {
            state,
            runtime,
            status: SendStatus::Idle,
            result_rx: None,
        }
    }

    fn start_send(&mut self, ctx: &egui::Context) {
        let routes = match build_routes(&self.state.steps) {
            Ok(routes) => routes,
            Err(errors) => {
                self.status = SendStatus::Error(errors.join("\n"));
                return;
            }
        };
        if routes.is_empty() {
            self.status = SendStatus::Error("パイプラインにステップがありません".to_string());
            return;
        }
        let uri = format!("http://{}:{}", self.state.host.trim(), self.state.port);
        let text = self.state.text.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.result_rx = Some(rx);
        self.status = SendStatus::Sending;
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let result = vstc::process_routes(&uri, routes, text).await;
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn poll_result(&mut self) {
        let Some(rx) = &self.result_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(resp)) => {
                self.status = if resp.result {
                    SendStatus::Success
                } else {
                    SendStatus::Error("サーバーが result=false を返しました".to_string())
                };
                self.result_rx = None;
            }
            Ok(Err(e)) => {
                self.status = SendStatus::Error(e.to_string());
                self.result_rx = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.status = SendStatus::Error("送信タスクが異常終了しました".to_string());
                self.result_rx = None;
            }
        }
    }

    fn ui_endpoint(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("接続先:");
            ui.label("host");
            ui.text_edit_singleline(&mut self.state.host);
            ui.label("port");
            ui.add(egui::DragValue::new(&mut self.state.port).range(1..=u16::MAX));
        });
    }

    fn ui_text(&mut self, ui: &mut egui::Ui) {
        ui.label("送信テキスト");
        ui.add(
            egui::TextEdit::multiline(&mut self.state.text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
    }

    fn ui_pipeline(&mut self, ui: &mut egui::Ui) {
        ui.label("パイプライン");
        let mut delete_idx: Option<usize> = None;
        for (idx, step) in self.state.steps.iter_mut().enumerate() {
            if step_card(ui, idx, step) {
                delete_idx = Some(idx);
            }
        }
        if let Some(idx) = delete_idx {
            self.state.steps.remove(idx);
        }
        if ui.button("＋ ステップ追加").clicked() {
            self.state.steps.push(PipelineStep::default());
        }
    }

    fn ui_send(&mut self, ui: &mut egui::Ui) {
        let sending = matches!(self.status, SendStatus::Sending);
        let clicked = ui
            .add_enabled(!sending, egui::Button::new("送信"))
            .clicked();
        if clicked {
            let ctx = ui.ctx().clone();
            self.start_send(&ctx);
        }
        match &self.status {
            SendStatus::Idle => {
                ui.label("状態: 待機中");
            }
            SendStatus::Sending => {
                ui.label("状態: 送信中…");
            }
            SendStatus::Success => {
                ui.colored_label(egui::Color32::GREEN, "状態: 成功");
            }
            SendStatus::Error(e) => {
                ui.colored_label(egui::Color32::RED, format!("エラー: {e}"));
            }
        }
    }
}

impl eframe::App for GuiApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        // Paint the whole window with the panel fill. eframe 0.35's `ui()` area
        // isn't full-window-filled, so without this the space below the content
        // (visible when the window is enlarged) shows the default clear color
        // (pitch black).
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_result();
        // eframe 0.35's `ui()` hands us a margin-less central area, so reapply the
        // standard central-panel frame to keep the usual content inset (the 0.33
        // path used `CentralPanel`, which provided this margin).
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("vstreamer クライアント");
                self.ui_endpoint(ui);
                ui.separator();
                self.ui_text(ui);
                ui.separator();
                self.ui_pipeline(ui);
                ui.separator();
                self.ui_send(ui);
            });
        });
    }
}

/// Render one parameter input row, bound to `step.params[key]`.
fn param_field(ui: &mut egui::Ui, idx: usize, step: &mut PipelineStep, key: &str) {
    let Some(s) = spec(key) else {
        return;
    };
    let value = step.params.entry(key.to_string()).or_default();
    ui.horizontal(|ui| {
        ui.label(s.label);
        match s.kind {
            ParamKind::Enum(allowed) => {
                let selected = if value.is_empty() {
                    "(未設定)".to_owned()
                } else {
                    value.clone()
                };
                egui::ComboBox::from_id_salt(format!("param-{idx}-{key}"))
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(value, String::new(), "(未設定)");
                        for opt in allowed {
                            ui.selectable_value(value, (*opt).to_string(), *opt);
                        }
                    });
            }
            _ => {
                ui.text_edit_singleline(value);
            }
        }
    });
}

/// Render the relevant + "other" parameter fields for a step.
fn step_params(ui: &mut egui::Ui, idx: usize, step: &mut PipelineStep) {
    let relevant = relevant_keys(step.op.to_proto());
    for key in relevant {
        param_field(ui, idx, step, key);
    }
    let others: Vec<&'static str> = PARAMS
        .iter()
        .map(|p| p.key)
        .filter(|k| !relevant.contains(k))
        .collect();
    if !others.is_empty() {
        // Salt the header ID with the step index: every step uses the same heading
        // text, and a CollapsingHeader derives its ID from that text, so without a
        // per-step salt the second step's expander clashes with the first.
        egui::CollapsingHeader::new("その他のパラメーター")
            .id_salt(format!("other-params-{idx}"))
            .show(ui, |ui| {
                for key in &others {
                    param_field(ui, idx, step, key);
                }
            });
    }
}

/// Render one pipeline step card. Returns true if its delete button was clicked.
fn step_card(ui: &mut egui::Ui, idx: usize, step: &mut PipelineStep) -> bool {
    let mut delete = false;
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut step.enabled, "有効");
            ui.label(format!("ステップ {}", idx + 1));
            egui::ComboBox::from_id_salt(format!("op-{idx}"))
                .selected_text(step.op.label())
                .show_ui(ui, |ui| {
                    for op in OpKind::ALL {
                        ui.selectable_value(&mut step.op, op, op.label());
                    }
                });
            if ui.button("削除").clicked() {
                delete = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("宛先 (remote):");
            ui.text_edit_singleline(&mut step.remote);
        });
        step_params(ui, idx, step);
    });
    delete
}
