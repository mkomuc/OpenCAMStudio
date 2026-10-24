//! Persistent, selectable Output text with the context of the action that failed.
use super::*;
use iced::widget::text_editor;

impl App {
    fn timestamp() -> String {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            % 86_400;
        format!(
            "{:02}:{:02}:{:02} UTC",
            seconds / 3_600,
            (seconds / 60) % 60,
            seconds % 60
        )
    }
    pub(super) fn output_context(&self) -> String {
        let setup = self.controller.document().setup();
        let operation =
            self.operation_draft
                .as_ref()
                .or_else(|| match self.controller.selection() {
                    Selection::Operation(id) => self.controller.operation(id),
                    _ => None,
                });
        let kind = operation
            .map(|op| crate::cad_operations::operation_type(op).to_owned())
            .or_else(|| self.cad_pending_kind.map(|k| format!("{k:?}")))
            .or_else(|| {
                self.controller
                    .pending_op()
                    .map(|p| format!("{:?}", p.kind))
            });
        let mut parts = vec![format!(
            "Setup '{}' (#{}; Z {:?})",
            setup.name,
            setup.id,
            setup.frame.to_world_vector([0., 0., 1.])
        )];
        if let Some(kind) = kind {
            parts.push(operation.map_or_else(
                || format!("{kind} (new operation)"),
                |op| format!("{kind} operation #{}", op.id()),
            ));
        }
        let mut refs = self.controller.cad_selection().to_vec();
        if let Some(editor) = &self.profile_entry_editor {
            parts.push(format!(
                "geometry entry '{}' ({:?}, {:?})",
                editor.draft.name, editor.draft.kind, editor.draft.chain_mode
            ));
            if let cam_model::ProfileGeometrySource::Cad(selected) = &editor.draft.source {
                refs = selected.clone();
            }
        } else if refs.is_empty() {
            refs = self.operation_geometry.clone().unwrap_or_default();
        }
        if refs.is_empty() {
            if let Some(op) = operation {
                if let Some(binding) = self.controller.geometry_state().bindings.get(&op.id()) {
                    refs = binding.selected.clone();
                }
            }
        }
        if !refs.is_empty() {
            parts.push(crate::cad_operations::describe_entities(
                self.controller.geometry_state(),
                &refs,
            ));
        }
        parts.join(" | ")
    }

    pub(super) fn log_geometry_validation(&mut self) {
        let error = if let Some(editor) = &self.profile_entry_editor {
            match &editor.draft.source {
                cam_model::ProfileGeometrySource::Cad(selected) if !selected.is_empty() => {
                    crate::cad_operations::geometry_for_setup(self.controller.document())
                        .and_then(|g| {
                            cam_geometry3d::profile_entry_contours(&g, &editor.draft).map(|_| ())
                        })
                        .err()
                }
                _ => None,
            }
        } else if let Some(kind) = self.cad_pending_kind {
            if self.controller.cad_selection().is_empty() {
                None
            } else {
                self.controller
                    .preview_cad_operation(kind, self.controller.cad_selection())
                    .err()
            }
        } else {
            None
        };
        let context = self.output_context();
        let error_key = error.as_ref().map(|e| format!("{context} | {e}"));
        if error_key != self.output_last_geometry_error {
            if let Some(error) = &error {
                self.output_history.push_back(format!(
                    "[{}] [{context}] Geometry validation error: {error}",
                    Self::timestamp()
                ));
                self.output_cleared = false;
                while self.output_history.len() > 500 {
                    self.output_history.pop_front();
                }
                self.set_pane_visible(Pane::Output, true);
            }
            self.output_last_geometry_error = error_key;
        }
    }

    pub(super) fn report_error(&mut self, error: String) {
        self.status = format!("Error: {error}");
        self.output_cleared = false;
        self.set_pane_visible(Pane::Output, true);
    }

    pub(super) fn append_output(&mut self, context: &str, message: &str) {
        let line = format!("[{}] [{context}] {message}", Self::timestamp());
        if !self
            .output_history
            .back()
            .is_some_and(|last| last.ends_with(&format!("[{context}] {message}")))
        {
            self.output_history.push_back(line);
            self.output_cleared = false;
            while self.output_history.len() > 500 {
                self.output_history.pop_front();
            }
        }
    }

    fn diagnostic_text(&self, diagnostic: &cam_toolpath::Diagnostic) -> String {
        let mut context = Vec::new();
        let document = self.controller.document();
        if let Some(id) = diagnostic.source.setup {
            if let Some(setup) = document.setups.iter().find(|s| s.id == id) {
                context.push(format!("Setup '{}' (#{id})", setup.name));
            }
        }
        if let Some(id) = diagnostic.source.operation.or(diagnostic.op) {
            if let Some(op) = document
                .setups
                .iter()
                .flat_map(|s| &s.operations)
                .find(|op| op.id() == id)
            {
                context.push(format!(
                    "{} operation #{id}",
                    crate::cad_operations::operation_type(op)
                ));
            }
            if let Some(binding) = document.geometry.bindings.get(&id) {
                context.push(crate::cad_operations::describe_entities(
                    &document.geometry,
                    &binding.selected,
                ));
            }
        }
        if let (Some(pattern), Some(instance)) = (
            diagnostic.source.pattern_id,
            diagnostic.source.instance_index,
        ) {
            context.push(format!("Pattern #{pattern}, instance {instance}"));
        }
        format!(
            "{:?} [{:?}{}]: {}",
            diagnostic.severity,
            diagnostic.code,
            if context.is_empty() {
                String::new()
            } else {
                format!(" | {}", context.join(" | "))
            },
            diagnostic.message
        )
    }

    pub(super) fn sync_output(&mut self) {
        if self.output_cleared {
            if !self.output_snapshot.is_empty() || !self.output_content.text().is_empty() {
                self.output_content = text_editor::Content::with_text("");
                self.output_snapshot.clear();
            }
            return;
        }
        let mut lines: Vec<String> = self.output_history.iter().cloned().collect();
        if lines.is_empty() {
            lines.push(self.status.clone());
        }
        if let Some(progress) = self.controller.run_progress() {
            lines.push(format!(
                "{:?}: {} / {}",
                progress.phase, progress.completed, progress.total
            ));
        }
        if let Some(outcome) = self.controller.outcome() {
            lines.push("Toolpath diagnostics:".into());
            if outcome.diagnostics.is_empty() {
                lines.push("No diagnostics.".into());
            }
            lines.extend(outcome.diagnostics.iter().map(|d| self.diagnostic_text(d)));
            if outcome.completion != JobCompletion::Complete {
                lines.push("Simulation incomplete.".into());
            } else if outcome.collisions.is_empty() {
                lines.push("Simulation: no collisions.".into());
            }
            lines.extend(outcome.collisions.iter().map(|c| {
                        let owner = match (c.pattern_id, c.instance_index) {
                    (Some(pattern), Some(instance)) => format!(
                        " (pattern #{pattern}, instance {instance}, operation #{})",
                        c.op_id.unwrap_or(0)
                    ),
                    _ => c
                        .op_id
                        .map_or(String::new(), |op| format!(" (operation #{op})")),
                        };
                        format!("Verification {:?}{}: {}", c.kind, owner, c.message)
            }));
        }
        let output = lines.join("\n");
        if output != self.output_snapshot && self.output_content.selection().is_none() {
            self.output_content = text_editor::Content::with_text(&output);
            self.output_snapshot = output;
        }
    }

    pub(super) fn output_action(&mut self, action: text_editor::Action) {
        if !action.is_edit() {
            self.output_content.perform(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_errors_retain_feature_context_after_the_status_changes_and_can_be_copied() {
        let (mut app, _) = App::new();
        app.cad_pending_kind = Some(OpKind::Profile);
        let context = app.output_context();
        app.report_error(
            "Edge 86: Chain edges must lie on one accessible plane normal to setup Z".into(),
        );
        app.append_output(&context, &app.status.clone());
        app.status = "Ready".into();
        app.sync_output();
        let text = app.output_content.text();
        assert!(text.contains("Profile"));
        assert!(text.contains("Edge 86"));
        assert!(text.contains("Setup"));
        app.output_action(text_editor::Action::SelectAll);
        assert_eq!(app.output_content.selection().unwrap(), text);
        app.output_action(text_editor::Action::Edit(text_editor::Edit::Insert('x')));
        assert_eq!(app.output_content.text(), text, "Output is read-only");
        app.append_output("Run", "Another message arrived while copying");
        app.sync_output();
        assert_eq!(
            app.output_content.selection().unwrap(),
            text,
            "new output does not disturb a selection"
        );
        app.output_action(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
        app.sync_output();
        assert!(
            app.output_content
            .text()
                .contains("Another message arrived while copying")
        );
    }

    #[test]
    fn output_clear_stays_empty_until_new_timestamped_output_arrives() {
        let (mut app, _) = App::new();
        app.append_output("Run", "first entry");
        app.append_output("Run", "first entry");
        app.sync_output();
        let before = app.output_content.text();
        assert!(before.contains(" UTC] [Run] first entry"));
        assert!(!before.contains("\n\n"));
        assert_eq!(before.matches("first entry").count(), 1);
        let _ = app.update(Message::OutputClear);
        app.sync_output();
        assert!(app.output_content.text().is_empty());
        app.append_output("Run", "second entry");
        app.sync_output();
        assert!(app.output_content.text().contains("second entry"));
    }
    #[cfg(feature = "occt")]
    #[test]
    #[ignore = "requires OCCT SDK and local testfiles/Tigerente.step"]
    fn rejected_cad_chains_are_logged_immediately_with_model_operation_and_entry_names() {
        let (mut app, _) = App::new();
        app.controller = AppController::new(crate::default_machine());
        app.controller
            .import_cad(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../testfiles/Tigerente.step"),
            )
            .unwrap();
        let instances = app
            .controller
            .geometry_state()
            .instances
            .iter()
            .map(|i| i.id)
            .collect();
        let mut setup = app.controller.document().setup().clone();
        setup.model_instances = instances;
        setup.name = "Mouth setup".into();
        setup.stock = cam_model::Stock::RelativeBox {
            offsets: [[1.; 3]; 2],
        };
        setup.frame = cam_model::SetupFrame::from_xy([1., 0., 0.], [0., 0., 1.]).unwrap();
        app.controller.apply_setup_settings(setup).unwrap();
        app.controller.select(Selection::Setup);
        app.setup_draft = None;
        app.refresh_fields();
        let _ = app.update(Message::BeginOp(OpKind::Profile));
        let asset = &app.controller.geometry_state().assets[0];
        let instance = &app.controller.geometry_state().instances[0];
        let edge = EntityRef {
            asset_id: asset.id,
            revision: asset.revision,
            instance_id: instance.id,
            body_id: instance.body_id,
            kind: EntityKind::Edge,
            id: 85,
        };
        let _ = app.update(Message::CadSelect(edge.clone()));
        let text = app.output_content.text();
        for detail in [
            "Profile",
            "Mouth setup",
            "Tigerente.step",
            "Edge 85",
            "setup Z",
            "Geometry validation error",
        ] {
            assert!(text.contains(detail), "missing {detail}: {text}");
        }
        assert!(app.pane_handle(Pane::Output).is_some());
        let _ = app.update(Message::ProfileEntry(
            profile_entries_view::EntryMessage::Add(cam_model::ProfileGeometryKind::Chain),
        ));
        let _ = app.update(Message::ProfileEntry(
            profile_entries_view::EntryMessage::Name("Whole mouth".into()),
        ));
        app.profile_entry_editor.as_mut().unwrap().draft.source =
            cam_model::ProfileGeometrySource::Cad(vec![edge]);
        let _ = app.update(Message::ProfileEntry(
            profile_entries_view::EntryMessage::Confirm,
        ));
        let text = app.output_content.text();
        assert!(text.contains("geometry entry 'Whole mouth'"), "{text}");
        assert!(text.contains("Edge 85"));
        assert!(text.contains("rejected"));
    }
}
