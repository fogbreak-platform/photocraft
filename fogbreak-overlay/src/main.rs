//! Fogbreak Raster Studio browser runner (WebGPU with WebGL2 fallback, `<canvas data-host="page">`).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod host_bridge;
#[cfg(target_arch = "wasm32")]
mod theme;

#[cfg(target_arch = "wasm32")]
mod web {
    use photocraft_doc::Document;
    use photocraft_engine::Session;
    use photocraft_ui_egui::theme::AppTheme;
    use photocraft_ui_egui::{PhotocraftApp, Services};
    use serde_json::{Value, json};

    use crate::host_bridge::{self, CommandQueue, HostCommand, LOADING_ID};
    use crate::theme::apply_palette;

    struct Shell {
        app: Box<PhotocraftApp>,
        queue: CommandQueue,
        dark: bool,
    }

    impl Shell {
        fn process_commands(&mut self) {
            for cmd in self.queue.drain() {
                match cmd {
                    HostCommand::OpenBytes { name, bytes } => {
                        let _ = self.app.open_bytes(&name, &bytes);
                    }
                    HostCommand::Screenshot { tx } => {
                        let png = self
                            .app
                            .session
                            .active()
                            .and_then(|st| photocraft_io::export(&st.doc, "screenshot.png", &Default::default()).ok())
                            .map(|r| r.bytes)
                            .unwrap_or_else(|| vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
                        let _ = tx.send(Ok(png));
                    }
                    HostCommand::Export { format, tx } => {
                        let _ = tx.send(self.export_bytes(&format));
                    }
                    HostCommand::Import { name, bytes, tx } => {
                        let _ = tx.send(self.import_bytes(&name, &bytes));
                    }
                    HostCommand::Request { method, params, tx } => {
                        let out = self.handle_request(&method, &params);
                        let _ = tx.send(out);
                    }
                }
            }
        }

        fn export_bytes(&self, format: &str) -> Result<Vec<u8>, String> {
            let fmt = format.trim().trim_start_matches('.').to_ascii_lowercase();
            let ext = match fmt.as_str() {
                "psd" | "png" | "webp" | "tiff" | "pdf" => fmt.as_str(),
                "tif" => "tiff",
                _ => return Err(format!("unsupported export format `{fmt}`")),
            };
            let st = self.app.session.active().ok_or_else(|| "no active document".to_string())?;
            photocraft_io::export(&st.doc, &format!("export.{ext}"), &Default::default())
                .map(|r| r.bytes)
                .map_err(|e| e.to_string())
        }

        fn import_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<Value, String> {
            let warnings = self.app.open_bytes(name, bytes)?;
            Ok(json!({
                "ok": true,
                "name": name,
                "byteLength": bytes.len(),
                "warnings": warnings,
            }))
        }

        fn describe_command(&self, command: &str) -> Value {
            let spec = photocraft_engine::commands::find(command).or_else(|| {
                photocraft_engine::command_specs()
                    .iter()
                    .find(|c| c.id.eq_ignore_ascii_case(command))
            });
            let Some(spec) = spec else {
                return Value::Null;
            };
            let mut entry = photocraft_engine::commands::list_entry(
                &self.app.session,
                spec,
                (spec.enabled)(&self.app.session).err(),
            );
            if let Some(map) = entry.as_object_mut() {
                map.insert("summary".to_string(), Value::String(spec.label.to_string()));
                map.insert("mutates".to_string(), Value::Bool(spec.journal));
            }
            entry
        }

        fn handle_request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
            match method {
                "__ui_inspect" => Ok(json!({
                    "engineId": host_bridge::ENGINE_ID,
                    "hostMode": "page",
                    "theme": if self.dark { "dark" } else { "light" },
                    "dirty": self.app.has_unsaved_work(),
                })),
                "__describe_command" => {
                    let id = params.get("command").and_then(Value::as_str).unwrap_or("");
                    Ok(self.describe_command(id))
                }
                _ => self.app.run(method, params.clone()),
            }
        }
    }

    impl eframe::App for Shell {
        fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
            photocraft_ui_egui::touch_twins::drop_twins(ctx, raw_input);
            eframe::App::raw_input_hook(&mut *self.app, ctx, raw_input);
        }

        fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
            self.process_commands();
            self.app.logic_frame(ctx, frame);
            apply_palette(ctx, self.dark);
            host_bridge::emit_dirty(self.app.has_unsaved_work());
        }

        fn persist_egui_memory(&self) -> bool {
            false
        }

        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            self.app.ui_frame(ui, frame);
        }
    }

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    fn prefers_dark() -> bool {
        let q = query();
        if q.contains("theme=light") {
            return false;
        }
        if q.contains("theme=dark") {
            return true;
        }
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .is_some_and(|m| m.matches())
    }

    fn services() -> Services {
        Services {
            import: Some(Box::new(|name: &str, bytes: &[u8], opts: &photocraft_io::ImportOptions| {
                photocraft_io::import_with_options(name, bytes, opts)
                    .map(|r| (r.document, r.warnings))
                    .map_err(|e| e.to_string())
            })),
            export: Some(Box::new(|doc: &Document, path: &str, settings: &photocraft_ui_egui::ExportSettings| {
                photocraft_io::export(doc, path, &settings.io_options())
                    .map(|r| (r.bytes, r.warnings))
                    .map_err(|e| e.to_string())
            })),
            write: Some(Box::new(|path: &str, bytes: &[u8]| host_bridge::deliver_save(path, bytes))),
            write_unconfirmed: true,
            ..Default::default()
        }
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        let canvas = match host_bridge::find_page_canvas() {
            Ok(c) => c,
            Err(err) => {
                log::error!("Fogbreak Raster host error: {err}");
                return;
            }
        };
        let dark = prefers_dark();
        let queue = CommandQueue::default();

        wasm_bindgen_futures::spawn_local(async move {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let mut options = eframe::WebOptions::default();
            photocraft_ui_egui::gpu_canvas::use_adapter_limits(&mut options.wgpu_options.wgpu_setup);
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }

            let queue_for_cb = queue.clone();
            let result = eframe::WebRunner::new()
                .start(
                    canvas.clone(),
                    options,
                    Box::new(move |cc| {
                        let base_theme = if dark { AppTheme::studio() } else { AppTheme::studio_light() };
                        PhotocraftApp::setup_context(&cc.egui_ctx, base_theme.clone());
                        host_bridge::install_rpc(queue_for_cb.clone(), cc.egui_ctx.clone());
                        host_bridge::listen_open(&canvas, queue_for_cb.clone(), cc.egui_ctx.clone());

                        let mut app = PhotocraftApp::with_context(cc.egui_ctx.clone(), Session::new(), services());
                        app.set_theme(&cc.egui_ctx, base_theme);
                        apply_palette(&cc.egui_ctx, dark);
                        if !query().contains("cpu")
                            && let Some(rs) = cc.wgpu_render_state.clone()
                        {
                            app.set_wgpu(rs);
                        }
                        let caps: Vec<String> = photocraft_engine::command_specs()
                            .iter()
                            .map(|c| c.id.to_string())
                            .collect();
                        host_bridge::emit_ready(&caps);
                        Ok(Box::new(Shell {
                            app,
                            queue: queue_for_cb,
                            dark,
                        }))
                    }),
                )
                .await;

            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => el.set_inner_html(&format!("<p>Fogbreak Raster Studio failed to start: {e:?}</p>")),
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("fogbreak-raster runs in the browser via trunk build --release");
}
