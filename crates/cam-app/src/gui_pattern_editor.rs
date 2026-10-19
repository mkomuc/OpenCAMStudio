//! Transactional editor for setup-level operation patterns.
use super::*;
use iced::widget::column;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PatternTab {
    Sources,
    Placement,
    Preview,
}

impl PatternTab {
    const ALL: [Self; 3] = [Self::Sources, Self::Placement, Self::Preview];
}

impl std::fmt::Display for PatternTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sources => "Sources",
            Self::Placement => "Placement",
            Self::Preview => "Preview",
        })
    }
}

pub(super) fn kind_from_draft(draft: &PatternDraft) -> Result<cam_model::PatternKind, String> {
    let kind = if draft.circular {
        let count = draft.values[0]
            .parse::<u32>()
            .map_err(|_| "Enter an integer instance count")?;
        let cx = draft.values[1]
            .parse::<f64>()
            .map_err(|_| "Enter a finite center X")?;
        let cy = draft.values[2]
            .parse::<f64>()
            .map_err(|_| "Enter a finite center Y")?;
        let sweep = draft.values[3]
            .parse::<f64>()
            .map_err(|_| "Enter a sweep angle")?;
        let distribution = if sweep == 360.0 {
            cam_model::CircularDistribution::Full
        } else {
            cam_model::CircularDistribution::Partial {
                sweep_degrees: sweep,
            }
        };
        cam_model::PatternKind::Circular {
            center_xy: [cx, cy],
            count,
            distribution,
            direction: if draft.clockwise {
                cam_model::PatternDirection::Cw
            } else {
                cam_model::PatternDirection::Ccw
            },
        }
    } else {
        cam_model::PatternKind::Rectangular {
            nx: draft.values[0]
                .parse::<u32>()
                .map_err(|_| "Enter an integer X count")?,
            ny: draft.values[1]
                .parse::<u32>()
                .map_err(|_| "Enter an integer Y count")?,
            dx: draft.values[2]
                .parse::<f64>()
                .map_err(|_| "Enter X pitch")?,
            dy: draft.values[3]
                .parse::<f64>()
                .map_err(|_| "Enter Y pitch")?,
        }
    };
    kind.validate()?;
    Ok(kind)
}

impl App {
    fn pattern_draft_error(&self) -> Option<String> {
        let draft = self.pattern_draft.as_ref()?;
        let setup = self
            .controller
            .setups()
            .iter()
            .find(|s| s.id == draft.setup_id)?;
        if draft.name.trim().is_empty() {
            return Some("Enter a pattern name.".into());
        }
        if draft.members.is_empty() {
            return Some("Choose at least one source operation.".into());
        }
        let mut datum = None;
        for id in &draft.members {
            let Some(op) = setup.operations.iter().find(|op| op.id() == *id) else {
                return Some(format!("Source operation {id} no longer exists."));
            };
            if let Some(other) = setup
                .patterns
                .iter()
                .find(|p| Some(p.id) != draft.pattern_id && p.members.contains(id))
            {
                return Some(format!(
                    "Operation {id} already belongs to pattern '{}'.",
                    other.name
                ));
            }
            if datum.is_some_and(|value| value != op.work_offset()) {
                return Some("Pattern sources must share one work datum.".into());
            }
            datum = Some(op.work_offset());
        }
        kind_from_draft(draft).err()
    }

    pub(super) fn pattern_editor(&self) -> Element<'_, Message> {
        let draft = self.pattern_draft.as_ref().expect("checked above");
        let tab = self.pattern_tab;
        let header = column![
            inspector_heading(if draft.circular {
                "Circular pattern"
            } else {
                "Rectangular pattern"
            }),
            row![
                text("Name")
                    .size(12)
                    .color(INSPECTOR_TEXT)
                    .width(Length::FillPortion(5)),
                inspector_input("Pattern", &draft.name)
                    .on_input(Message::PatternName)
                    .width(Length::FillPortion(6)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row(PatternTab::ALL.into_iter().map(|value| {
                inspector_button(value.to_string())
                    .padding([7, 4])
                    .width(Length::Fill)
                    .style(move |theme, status| inspector_tab(theme, status, tab == value))
                    .on_press(Message::PatternTabChanged(value))
                    .into()
            }))
            .spacing(3),
        ]
        .spacing(10)
        .padding(14);
        let mut list = column![inspector_section(&tab.to_string())]
            .spacing(10)
            .padding(Padding {
                top: 0.0,
                right: 18.0,
                bottom: 16.0,
                left: 14.0,
            });
        let instances = if draft.circular {
            draft.values[0].parse::<usize>().unwrap_or(0)
        } else {
            draft.values[0]
                .parse::<usize>()
                .unwrap_or(0)
                .saturating_mul(draft.values[1].parse::<usize>().unwrap_or(0))
        };
        match tab {
            PatternTab::Sources => {
                list = list.push(
                    text("Select source operations in their execution order.")
                        .size(11)
                        .color(INSPECTOR_MUTED),
                );
                if let Some(setup) = self
                    .controller
                    .setups()
                    .iter()
                    .find(|s| s.id == draft.setup_id)
                {
                    for op in &setup.operations {
                        let id = op.id();
                        let used = setup
                            .patterns
                            .iter()
                            .any(|p| Some(p.id) != draft.pattern_id && p.members.contains(&id));
                        list = list.push(
                            checkbox(draft.members.contains(&id))
                                .label(format!(
                                    "{} · operation {id}{}",
                                    op_kind(op),
                                    if used { " (already grouped)" } else { "" }
                                ))
                                .size(14)
                                .text_size(12)
                                .spacing(8)
                                .on_toggle(move |checked| Message::PatternMember(id, checked)),
                        );
                    }
                    if setup.operations.is_empty() {
                        list = list.push(
                            text("Create an operation before adding pattern sources.")
                                .size(11)
                                .color(INSPECTOR_MUTED),
                        );
                    }
                }
            }
            PatternTab::Placement => {
                let fields = if draft.circular {
                    [
                        ("Count", ""),
                        ("Center X", "mm"),
                        ("Center Y", "mm"),
                        ("Sweep", "deg"),
                    ]
                } else {
                    [
                        ("X count", ""),
                        ("Y count", ""),
                        ("X pitch", "mm"),
                        ("Y pitch", "mm"),
                    ]
                };
                for (index, (label, unit)) in fields.into_iter().enumerate() {
                    list = list.push(inspector_value(
                        label,
                        &draft.values[index],
                        unit,
                        move |value| Message::PatternValue(index, value),
                    ));
                }
                list = list.push(inspector_section("Execution"));
                list = list.push(profile_picker_w(
                    "Order", "By instance runs every source at one placement before moving to the next. By operation runs each source at every placement before the next source.",
                    self.tooltips,
                    if draft.by_operation { "By operation" } else { "By instance" },
                    &["By instance", "By operation"],
                    |value| Message::PatternOrderChanged(value == "By operation"),
                    INSPECTOR_PICKER_W,
                ));
                if draft.circular {
                    list = list.push(profile_picker_w(
                        "Direction",
                        "Rotation direction viewed from +Z in setup coordinates.",
                        self.tooltips,
                        if draft.clockwise {
                            "Clockwise"
                        } else {
                            "Counterclockwise"
                        },
                        &["Counterclockwise", "Clockwise"],
                        |value| Message::PatternDirectionChanged(value == "Clockwise"),
                        INSPECTOR_PICKER_W,
                    ));
                }
            }
            PatternTab::Preview => {
                let markers = if draft.circular {
                    let cx = draft.values[1].parse::<f64>().unwrap_or(0.0);
                    let cy = draft.values[2].parse::<f64>().unwrap_or(0.0);
                    let sweep = draft.values[3].parse::<f64>().unwrap_or(360.0);
                    (0..instances.min(12))
                        .map(|i| {
                            let divisor = if sweep == 360.0 {
                                instances.max(1) as f64
                            } else {
                                instances.saturating_sub(1).max(1) as f64
                            };
                            let sign = if draft.clockwise { -1.0 } else { 1.0 };
                            format!(
                                "{i}: center ({cx:.3}, {cy:.3}), {}°",
                                sign * sweep * i as f64 / divisor
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    let nx = draft.values[0].parse::<usize>().unwrap_or(0);
                    let ny = draft.values[1].parse::<usize>().unwrap_or(0);
                    let dx = draft.values[2].parse::<f64>().unwrap_or(0.0);
                    let dy = draft.values[3].parse::<f64>().unwrap_or(0.0);
                    (0..(nx.saturating_mul(ny).min(12)))
                        .map(|i| {
                            format!(
                                "{i}: ({:.3}, {:.3})",
                                (i % nx.max(1)) as f64 * dx,
                                (i / nx.max(1)) as f64 * dy
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                list = list.push(
                    text("Placements in setup XY")
                        .size(12)
                        .color(INSPECTOR_TEXT),
                );
                list = list.push(text(markers).size(12).color(INSPECTOR_MUTED));
                list = list.push(text("Generated source paths appear in the viewport. Crosses mark placements when paths are unavailable.")
                    .size(11).color(INSPECTOR_MUTED));
            }
        }
        let error = self.pattern_draft_error();
        let mut feedback = column![text(format!(
            "{instances} instances including the original · {} source operation(s)",
            draft.members.len()
        ))
        .size(11)
        .color(INSPECTOR_MUTED)]
        .spacing(8);
        if let Some(reason) = &error {
            feedback = feedback.push(text(reason.clone()).size(11).color(palette::WARN));
        }
        let footer = inspector_footer(
            feedback.push(
                row![
                    inspector_button("Cancel").on_press(Message::PatternCancel),
                    Space::new().width(Length::Fill),
                    inspector_button(if draft.pattern_id.is_some() {
                        "Apply changes"
                    } else {
                        "Create pattern"
                    })
                    .style(inspector_primary)
                    .on_press_maybe(error.is_none().then_some(Message::PatternCommit)),
                ]
                .spacing(8),
            ),
        );
        column![
            header,
            scrollable(list).width(Length::Fill).height(Length::Fill),
            footer,
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
