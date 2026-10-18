//! Transactional editor for setup-level operation patterns.
use super::*;
use iced::widget::column;

pub(super) fn kind_from_draft(draft: &PatternDraft) -> Result<cam_model::PatternKind, String> {
    let kind = if draft.circular {
        let count = draft.values[0].parse::<u32>().map_err(|_| "Enter an integer instance count")?;
        let cx = draft.values[1].parse::<f64>().map_err(|_| "Enter a finite center X")?;
        let cy = draft.values[2].parse::<f64>().map_err(|_| "Enter a finite center Y")?;
        let sweep = draft.values[3].parse::<f64>().map_err(|_| "Enter a sweep angle")?;
        let distribution = if sweep == 360.0 { cam_model::CircularDistribution::Full } else { cam_model::CircularDistribution::Partial { sweep_degrees: sweep } };
        cam_model::PatternKind::Circular { center_xy: [cx, cy], count, distribution, direction: if draft.clockwise { cam_model::PatternDirection::Cw } else { cam_model::PatternDirection::Ccw } }
    } else {
        cam_model::PatternKind::Rectangular {
            nx: draft.values[0].parse::<u32>().map_err(|_| "Enter an integer X count")?,
            ny: draft.values[1].parse::<u32>().map_err(|_| "Enter an integer Y count")?,
            dx: draft.values[2].parse::<f64>().map_err(|_| "Enter X pitch")?,
            dy: draft.values[3].parse::<f64>().map_err(|_| "Enter Y pitch")?,
        }
    };
    kind.validate()?;
    Ok(kind)
}

impl App {
    fn pattern_draft_error(&self) -> Option<String> {
        let draft = self.pattern_draft.as_ref()?;
        let setup = self.controller.setups().iter().find(|s| s.id == draft.setup_id)?;
        if draft.name.trim().is_empty() { return Some("Enter a pattern name.".into()); }
        if draft.members.is_empty() { return Some("Choose at least one source operation.".into()); }
        let mut datum = None;
        for id in &draft.members {
            let Some(op) = setup.operations.iter().find(|op| op.id() == *id) else { return Some(format!("Source operation {id} no longer exists.")); };
            if let Some(other) = setup.patterns.iter().find(|p| Some(p.id) != draft.pattern_id && p.members.contains(id)) { return Some(format!("Operation {id} already belongs to pattern '{}'.", other.name)); }
            if datum.is_some_and(|value| value != op.work_offset()) { return Some("Pattern sources must share one work datum.".into()); }
            datum = Some(op.work_offset());
        }
        kind_from_draft(draft).err()
    }

    pub(super) fn pattern_editor(&self) -> Element<'_, Message> {
        let draft = self.pattern_draft.as_ref().expect("checked above");
        let mut list = column![inspector_heading(if draft.circular { "Circular pattern" } else { "Rectangular pattern" })]
            .spacing(8).padding(12);
        list = list.push(row![text("Name").size(11).width(Length::Fill), text_input("Pattern", &draft.name).on_input(Message::PatternName).width(Length::Fixed(120.0))].align_y(Alignment::Center));
        list = list.push(text("Sources (execution order)").size(12));
        if let Some(setup) = self.controller.setups().iter().find(|s| s.id == draft.setup_id) {
            for op in &setup.operations {
                let id = op.id();
                let used = setup.patterns.iter().any(|p| p.members.contains(&id));
                list = list.push(checkbox(draft.members.contains(&id)).label(format!("{} · operation {id}{}", op_kind(op), if used { " (already grouped)" } else { "" }))
                    .size(12).on_toggle(move |checked| Message::PatternMember(id, checked)));
            }
        }
        let labels: &[&str] = if draft.circular { &["Count", "Center X (mm)", "Center Y (mm)", "Sweep (deg)"] } else { &["X count", "Y count", "X pitch (mm)", "Y pitch (mm)"] };
        for (index, label) in labels.iter().enumerate() {
            list = list.push(row![text(*label).size(11).width(Length::Fill),
                text_input("value", &draft.values[index]).on_input(move |value| Message::PatternValue(index, value)).width(Length::Fixed(92.0))]
                .spacing(6).align_y(Alignment::Center));
        }
        list = list.push(text(if draft.by_operation { "Order: By operation" } else { "Order: By instance" }).size(11));
        list = list.push(button(text("Switch execution order").size(11)).on_press(Message::PatternOrderChanged(!draft.by_operation)));
        if draft.circular {
            list = list.push(text(if draft.clockwise { "Direction: CW viewed from +Z" } else { "Direction: CCW viewed from +Z" }).size(11));
            list = list.push(button(text("Switch direction").size(11)).on_press(Message::PatternDirectionChanged(!draft.clockwise)));
        }
        let instances = if draft.circular {
            draft.values[0].parse::<usize>().ok().unwrap_or(0)
        } else {
            draft.values[0].parse::<usize>().ok().unwrap_or(0).saturating_mul(draft.values[1].parse::<usize>().ok().unwrap_or(0))
        };
        list = list.push(text(format!("{instances} instances including the original · {} source operation(s)", draft.members.len())).size(11).color(palette::GROUP_LABEL));
        let markers = if draft.circular {
            let cx = draft.values[1].parse::<f64>().unwrap_or(0.0); let cy = draft.values[2].parse::<f64>().unwrap_or(0.0);
            let sweep = draft.values[3].parse::<f64>().unwrap_or(360.0);
            (0..instances.min(12)).map(|i| {
                let divisor = if sweep == 360.0 { instances.max(1) as f64 } else { instances.saturating_sub(1).max(1) as f64 };
                let sign = if draft.clockwise { -1.0 } else { 1.0 };
                format!("{i}: center ({cx:.3}, {cy:.3}), {}°", sign * sweep * i as f64 / divisor)
            }).collect::<Vec<_>>().join("\n")
        } else {
            let nx = draft.values[0].parse::<usize>().unwrap_or(0); let ny = draft.values[1].parse::<usize>().unwrap_or(0);
            let dx = draft.values[2].parse::<f64>().unwrap_or(0.0); let dy = draft.values[3].parse::<f64>().unwrap_or(0.0);
            (0..(nx.saturating_mul(ny).min(12))).map(|i| format!("{i}: ({:.3}, {:.3})", (i % nx.max(1)) as f64 * dx, (i / nx.max(1)) as f64 * dy)).collect::<Vec<_>>().join("\n")
        };
        list = list.push(text(format!("Placement preview (setup XY)\n{markers}\nGenerated source paths are overlaid when available; otherwise crosses mark placements.")).size(10).color(palette::GROUP_LABEL));
        if let Some(reason) = self.pattern_draft_error() { list = list.push(text(reason).size(10).color(palette::WARN)); }
        let apply_label = if draft.pattern_id.is_some() { "Apply pattern" } else { "Create pattern" };
        list = list.push(row![button(text(apply_label)).on_press_maybe(self.pattern_draft_error().is_none().then_some(Message::PatternCommit)), button(text("Cancel")).on_press(Message::PatternCancel)].spacing(8));
        scrollable(list).width(Length::Fill).height(Length::Fill).into()
    }
}
