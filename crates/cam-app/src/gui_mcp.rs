//! Stdio runs on a worker; every desktop command executes on the iced event loop.
use super::*;
use crate::mcp::Command;
use crate::mcp::{self as api, *};
use serde_json::{json, Value};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};

pub(super) struct Request {
    name: String,
    arguments: Value,
    reply: SyncSender<CommandResult>,
}

pub(super) fn start() -> Receiver<Request> {
    let (send, receive) = mpsc::sync_channel(1);
    let tools = api::tools()
        .into_iter()
        .chain(catalog().iter().map(Command::tool))
        .collect();
    std::thread::spawn(move || {
        let result = api::serve(
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            tools,
            |name, arguments| {
                let (reply, result) = mpsc::sync_channel(1);
                send.send(Request {
                    name: name.into(),
                    arguments,
                    reply,
                })
                .map_err(|_| "desktop session closed".to_string())?;
                result
                    .recv()
                    .map_err(|_| "desktop session closed".to_string())?
            },
        );
        if let Err(error) = result {
            eprintln!("OpenCAMStudio MCP: {error}");
        }
    });
    receive
}

impl App {
    pub(super) fn poll_mcp(&mut self) -> iced::Task<Message> {
        let request = match self.mcp.as_ref().map(Receiver::try_recv) {
            Some(Ok(request)) => request,
            Some(Err(TryRecvError::Disconnected)) => return iced::exit(),
            _ => return iced::Task::none(),
        };
        let result = self.mcp_call(&request.name, request.arguments);
        if let Err(error) = &result {
            self.status = format!("MCP {}: {error}", request.name);
        }
        let _ = request.reply.send(result);
        self.sync_output();
        self.sync_inspector_visibility();
        iced::Task::none()
    }

    fn mcp_call(&mut self, name: &str, args: Value) -> CommandResult {
        if let Some(command) = catalog().into_iter().find(|c| c.name == name) {
            return command.invoke(self, args);
        }
        let shop = api::shop_tools().iter().any(|t| t["name"] == name);
        let read_only = if shop {
            api::shop_tools()
                .iter()
                .any(|t| t["name"] == name && t["annotations"]["readOnlyHint"] == true)
        } else {
            api::is_read_only(name)
        };
        if !read_only
            && !matches!(name, "cancel_run" | "cancel_cad_import" | "save_shop")
            && (self.setup_draft_dirty() || self.inspector_dirty() || self.pattern_draft.is_some())
        {
            return Err("Apply or cancel desktop editor changes before issuing an MCP edit. Use desktop_command with cancel_editor to discard them.".into());
        }
        let revision = self.controller.revision();
        let active = self.controller.active_setup_id();
        let selection = self.controller.selection();
        let result = if shop {
            api::call_shop(
                &mut api::Shop {
                    controller: &mut self.controller,
                    library: &mut self.library,
                    machines: &mut self.machines,
                    settings: &mut self.settings,
                },
                name,
                args,
            )
        } else {
            api::call(&mut self.controller, name, args)
        };
        if result.is_ok() {
            if matches!(
                name,
                "new_project" | "open_project" | "commit_cad_preview" | "import_cad" | "open_dxf"
            ) {
                self.reset_model_browser();
                self.view = ViewControls::default();
            }
            if matches!(name, "update_preferences" | "restore_preferences") {
                self.apply_mcp_preferences();
            }
            if shop && !read_only {
                self.reload_tool_edit();
            }
            if self.controller.revision() != revision
                || self.controller.active_setup_id() != active
                || self.controller.selection() != selection
            {
                self.operation_draft = None;
                self.operation_geometry = None;
                self.operation_binding_preview = None;
                self.stock_to_leave_draft = None;
                self.rebase_setup_draft();
                self.height_draft_id = None;
                self.operation_tools.clear();
                if let Selection::Operation(id) = self.controller.selection() {
                    if self.controller.pending_op().is_none() {
                        self.begin_operation_draft(id);
                    } else {
                        self.refresh_fields();
                    }
                } else {
                    self.refresh_fields();
                }
                self.viewport_cache.replace(None);
                self.cad_scene_cache.replace(None);
                self.pending_simulation = None;
                self.pending_toolpath_export = None;
                self.pending_processing_time = None;
                self.processing_time_dialog = None;
            }
            if self
                .simulation
                .as_ref()
                .is_some_and(|player| !player.current(&self.controller))
            {
                self.simulation = None;
            }
            if matches!(
                name,
                "cancel_run" | "start_run" | "start_toolpath_run" | "start_pattern_toolpath_run"
            ) {
                self.pending_simulation = None;
                self.pending_toolpath_export = None;
                self.pending_processing_time = None;
                self.processing_time_dialog = None;
            }
            if matches!(
                name,
                "activate_machine"
                    | "set_machine"
                    | "update_machine"
                    | "open_project"
                    | "update_library_machine"
                    | "delete_machine"
                    | "set_post_kind"
                    | "set_estlcam_options"
            ) {
                self.active_machine = self.controller.machine().name.clone();
                self.machine_name_edit = None;
                self.machine_post_edit = None;
                self.machine_estlcam_edit = None;
                self.refresh_fields();
            }
            self.status = format!("MCP: {name}");
        }
        result
    }

    fn apply_mcp_preferences(&mut self) {
        self.show_stock = self.settings.view.show_stock;
        self.show_gizmo = self.settings.view.show_gizmo;
        self.show_origin = self.settings.view.show_origin;
        self.show_envelope = self.settings.view.show_envelope;
        self.tooltips = self.settings.view.tooltips;
        self.gizmo_size = self.settings.view.gizmo_size;
        self.snaps = self.settings.snapping.default_snaps.clone();
        self.project_px = self.settings.panes.project_px;
        self.inspector_px = self.settings.panes.inspector_px;
        self.output_px = self.settings.panes.output_px;
        self.apply_fixed_layout();
        if let Some(player) = &mut self.simulation {
            player.configure(&self.settings.simulation, self.settings.view.stock_color);
        }
    }
}

macro_rules! desktop_catalog {
    ($($name:ident [$read:literal] $description:literal {$($key:literal: $schema:expr),* $(,)?} |$c:ident, $a:ident| $body:block)*) => {
        fn catalog() -> Vec<Command<App>> {
            vec![$(Command {name:stringify!($name),description:$description,read_only:$read,
                properties:json!({$($key:$schema),*}),run:|$c,$a| $body}),*]
        }
    }
}
fn desktop_state(c: &App) -> Value {
    json!({"status":c.status,"ribbon":format!("{:?}",c.active_tab),
        "view":{"yaw":c.view.yaw,"pitch":c.view.pitch,"zoom":c.view.zoom,"pan":c.view.pan},
        "dirty_editor":c.setup_draft_dirty() || c.inspector_dirty() || c.pattern_draft.is_some(),
        "simulation":c.simulation.as_ref().map(|p|json!({"position_seconds":p.position,"playing":p.playing,"speed":p.speed,"options":p.options})),
        "panes": ([Pane::Project,Pane::Library,Pane::Viewport,Pane::Inspector,Pane::Output].iter()
            .map(|pane|json!({"name":pane.name(),"visible":c.pane_handle(*pane).is_some()})).collect::<Vec<_>>())})
}
fn pane(value: &str) -> Result<Pane, String> {
    match value {
        "Project" => Ok(Pane::Project),
        "Library" => Ok(Pane::Library),
        "Viewport" => Ok(Pane::Viewport),
        "Inspector" => Ok(Pane::Inspector),
        "Output" => Ok(Pane::Output),
        _ => Err("unknown pane".into()),
    }
}

desktop_catalog! {
    get_desktop_state [true] "Inspect the visible desktop, editor state, camera, panes and simulation playback." {} |c,_a| {Ok(desktop_state(c))}
    desktop_set_view [false] "Set camera yaw/pitch in degrees, zoom (-4..6) and pan [x,y,z]; omitted fields are retained." {"yaw":optional(number()),"pitch":optional(number()),"zoom":optional(number()),"pan":optional(vector(3))} |c,a| {
        if let Some(value)=a.optional("yaw")? {c.view.yaw=value;}
        if let Some(value)=a.optional("pitch")? {c.view.pitch=value;}
        if let Some(value)=a.optional::<f32>("zoom")? {c.view.zoom=value.clamp(-4.,6.);}
        if let Some(value)=a.optional("pan")? {c.view.pan=value;}
        Ok(desktop_state(c))
    }
    desktop_set_pane [false] "Show or hide a desktop pane." {"pane":choice(&["Project","Library","Viewport","Inspector","Output"]),"visible":boolean()} |c,a| {
        let _=c.update(Message::SetPaneVisible(pane(&a.get::<String>("pane")?)?,a.get("visible")?)); Ok(desktop_state(c))
    }
    desktop_select_ribbon [false] "Select a desktop ribbon tab." {"tab":choice(&["Home","Machining","Tools","Machinery","View"])} |c,a| {
        let tab=match a.get::<String>("tab")?.as_str() {"Home"=>RibbonTab::Home,"Machining"=>RibbonTab::Machining,
            "Tools"=>RibbonTab::Tools,"Machinery"=>RibbonTab::Machinery,_=>RibbonTab::View};
        let _=c.update(Message::SelectRibbonTab(tab)); Ok(desktop_state(c))
    }
    desktop_set_visibility [false] "Show or hide a CAD asset/instance, setup, or operation by stable ID." {"target":choice(&["asset","instance","setup","operation"]),"id":id(),"visible":boolean()} |c,a| {
        let id:u64=a.get("id")?; let visible=a.get("visible")?;
        let message=match a.get::<String>("target")?.as_str() {
            "asset" if c.controller.geometry_state().asset(id).is_some()=>Message::SetAssetVisible(id,visible),
            "instance" if c.controller.geometry_state().instance(id).is_some()=>Message::SetInstanceVisible(id,visible),
            "setup" if c.controller.setups().iter().any(|s|s.id==id)=>Message::SetSetupVisible(id,visible),
            "operation" if u32::try_from(id).ok().is_some_and(|id|c.controller.operation(id).is_some())=>Message::SetOperationVisible(id as u32,visible),
            _=>return Err("target not found".into()),
        };
        let _=c.update(message); Ok(desktop_state(c))
    }
    desktop_isolate_instance [false] "Isolate a CAD instance in the viewport." {"id":id()} |c,a| {
        let id=a.get("id")?; if c.controller.geometry_state().instance(id).is_none(){return Err("instance not found".into());}
        let _=c.update(Message::CadIsolateInstance(id)); Ok(desktop_state(c))
    }
    desktop_open_editor [false] "Open a setup, operation, pattern, or shop tool editor." {"target":choice(&["setup","operation","pattern","library_tool"]),"id":id()} |c,a| {
        let id:u64=a.get("id")?;
        let message=match a.get::<String>("target")?.as_str() {
            "setup" if c.controller.setups().iter().any(|s|s.id==id)=>Message::EditSetup(id),
            "operation" if u32::try_from(id).ok().is_some_and(|id|c.controller.operation(id).is_some())=>Message::Select(Selection::Operation(id as u32)),
            "pattern" if c.controller.document().setup().patterns.iter().any(|p|p.id==id)=>Message::EditPattern(id),
            "library_tool" if (id as usize)<c.library.tools.len()=>Message::SelectLibraryTool(id as usize),
            _=>return Err("editor target not found".into()),
        };
        let _=c.update(message); Ok(desktop_state(c))
    }
    desktop_command [false] "Invoke a desktop presentation or editor command. Most project edits use the shared application commands instead." {"command":choice(&[
        "reset_view","toggle_stock","toggle_transparent_stock","toggle_origin","toggle_envelope","toggle_gizmo","toggle_tooltips",
        "toggle_snapping","toggle_feature_highlights","show_all_models","show_preferences","close_preferences","show_license","close_license",
        "clear_output","apply_editor","cancel_editor"
    ])} |c,a| {
        let message=match a.get::<String>("command")?.as_str() {
            "reset_view"=>Message::ResetView,"toggle_stock"=>Message::ToggleStock,
            "toggle_transparent_stock"=>Message::ToggleTransparentStock,"toggle_origin"=>Message::ToggleShowOrigin,
            "toggle_envelope"=>Message::ToggleEnvelope,"toggle_gizmo"=>Message::ToggleGizmo,"toggle_tooltips"=>Message::ToggleTooltips,
            "toggle_snapping"=>Message::ToggleSnapping,"toggle_feature_highlights"=>Message::ToggleFeatureHighlights,
            "show_all_models"=>Message::CadShowAll,"show_preferences"=>Message::ShowPrefs,"close_preferences"=>Message::ClosePrefs,
            "show_license"=>Message::ShowLicense,"close_license"=>Message::CloseLicense,"clear_output"=>Message::OutputClear,
            "apply_editor"=>if c.pattern_draft.is_some(){Message::PatternCommit}else if c.setup_draft.is_some(){Message::SetupCommit}else{Message::Apply},
            "cancel_editor"=>{
                c.end_setup_draft(); c.pattern_draft=None; c.operation_draft=None;
                c.operation_geometry=None; c.operation_binding_preview=None; c.stock_to_leave_draft=None;
                c.cad_pending_kind=None; c.cad_editing_op=None; c.profile_entry_editor=None;
                c.machine_name_edit=None; c.machine_post_edit=None; c.machine_estlcam_edit=None;
                c.controller.cancel_operation(); c.reload_tool_edit(); c.height_draft_id=None;
                if let Selection::Operation(id)=c.controller.selection(){c.begin_operation_draft(id);}else{c.refresh_fields();}
                c.status="Editor changes cancelled by MCP.".into(); return Ok(desktop_state(c));
            },
            _=>return Err("unknown desktop command".into()),
        };
        let _=c.update(message); Ok(desktop_state(c))
    }
    desktop_simulation [false] "Control stock-removal playback. Open after a completed run; seek uses seconds and speed is 0.25..10." {
        "action":choice(&["open","close","play","pause","start","end","previous_point","next_point","previous_operation","next_operation","seek","speed"]),
        "value":optional(number()),"operation":optional(id())
    } |c,a| {
        let action:String=a.get("action")?;
        if action=="open" {
            c.controller.poll_run();
            let mut player=SimulationPlayer::new_for_operation(&c.controller,a.optional("operation")?)?;
            player.configure(&c.settings.simulation,c.settings.view.stock_color); c.simulation=Some(player);
        } else if action=="close" {c.simulation=None; c.pending_simulation=None;}
        else {
            let player=c.simulation.as_mut().ok_or("open simulation first")?;
            match action.as_str() {
                "play"=>{if !player.playing{player.toggle();}},"pause"=>{if player.playing{player.toggle();}},
                "start"=>player.seek_end(false),"end"=>player.seek_end(true),
                "previous_point"=>player.seek_point(false),"next_point"=>player.seek_point(true),
                "previous_operation"=>player.seek_operation(false),"next_operation"=>player.seek_operation(true),
                "seek"=>player.seek(a.get("value")?),"speed"=>player.speed=a.get::<f64>("value")?.clamp(0.25,10.),
                _=>return Err("unknown simulation action".into()),
            }
        }
        Ok(desktop_state(c))
    }
    get_desktop_output [true] "Read the desktop output/diagnostics console." {} |c,_a| {serialized(&c.output_snapshot)}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_mcp_edits_the_visible_controller_and_preserves_undo() {
        let (mut app, _) = App::new();
        let original = app.controller.document().setup().name.clone();
        app.mcp_call("update_setup", json!({"patch":{"name":"MCP setup"}}))
            .unwrap();
        assert_eq!(app.controller.document().setup().name, "MCP setup");
        app.mcp_call("undo", json!({})).unwrap();
        assert_eq!(app.controller.document().setup().name, original);
        app.mcp_call(
            "desktop_set_view",
            json!({"yaw":45.,"pitch":30.,"zoom":2.,"pan":[1.,2.,3.]}),
        )
        .unwrap();
        assert_eq!(app.view.yaw, 45.);
        assert_eq!(app.view.pan, [1., 2., 3.]);
    }

    #[test]
    fn desktop_mcp_keeps_drafts_until_explicitly_cancelled() {
        let (mut app, _) = App::new();
        app.begin_setup_draft();
        app.setup_draft.as_mut().unwrap().name = "Uncommitted".into();
        let before = app.controller.document().clone();
        assert!(app.mcp_call("add_setup", json!({})).is_err());
        assert_eq!(app.controller.document(), &before);
        app.mcp_call("get_state", json!({})).unwrap();
        app.mcp_call("desktop_command", json!({"command":"cancel_editor"}))
            .unwrap();
        app.mcp_call("add_setup", json!({})).unwrap();
        assert_eq!(app.controller.setups().len(), 2);
        assert!(!app
            .controller
            .setups()
            .iter()
            .any(|s| s.name == "Uncommitted"));
    }

    #[test]
    fn remote_operation_edits_rebase_the_visible_height_and_stock_editors() {
        let (mut app, _) = App::new();
        app.mcp_call("open_dxf", json!({"text":SAMPLE_DXF,"name":"MCP sample"}))
            .unwrap();
        let id = app.controller.document().setup().operations[0].id();
        app.begin_operation_draft(id);
        app.mcp_call(
            "update_operation",
            json!({"id":id,"patch":{"Profile":{"feed":225.}},
            "heights":{"clearance":15.,"retract":5.,"top_of_stock":0.}}),
        )
        .unwrap();
        assert!(!app.inspector_dirty());
        app.mcp_call(
            "update_operation",
            json!({"id":id,"patch":{"Profile":{"feed":250.}}}),
        )
        .unwrap();
        assert!(!app.inspector_dirty());
        app.height_offsets
            .insert(cam_model::HeightPlaneKind::Clearance, "17".into());
        app.mcp_call("desktop_command", json!({"command":"cancel_editor"}))
            .unwrap();
        assert!(!app.inspector_dirty());
    }

    #[test]
    fn desktop_queue_returns_the_result_of_the_same_session_edit() {
        let (mut app, _) = App::new();
        let (send, receive) = mpsc::sync_channel(1);
        let (reply, result) = mpsc::sync_channel(1);
        app.mcp = Some(receive);
        send.send(Request {
            name: "add_setup".into(),
            arguments: json!({}),
            reply,
        })
        .unwrap();
        let _ = app.poll_mcp();
        assert!(result.recv().unwrap().is_ok());
        assert_eq!(app.controller.setups().len(), 2);
    }

    #[test]
    fn desktop_and_shared_tool_names_are_unique() {
        let mut names = std::collections::BTreeSet::new();
        for tool in api::tools()
            .into_iter()
            .chain(catalog().iter().map(Command::tool))
        {
            assert!(names.insert(tool["name"].as_str().unwrap().to_owned()));
        }
    }
}
