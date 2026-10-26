//! Explanations and illustrations for parameter hover help.
use super::*;
use profile_parameters::{Number, Toggle};

pub(super) const PROFILE_ENTRY_SIDES: &str = "Start outside/inside alternates the cutter side for outer and inner boundaries. Always outside/inside uses the same side for every contour in this entry. On follows each contour with the cutter centre. Open left/right follows the selected chain direction.";

pub(super) const HOLDING_TABS: &str = "Leave small bridges of material to hold the part in place. Configure their shape, size and placement on a single closed contour.";
pub(super) const STOCK_TO_LEAVE: &str = "Leave material for a finishing operation. Radial allowance remains on walls; axial allowance remains on floors. Facing uses axial allowance only.";

pub(super) fn height_help(plane: cam_model::HeightPlaneKind) -> &'static str {
    use cam_model::HeightPlaneKind as Plane;
    match plane {
        Plane::Clearance => "Clearance is the safe height for rapid travel between features. Choose a reference and offset along setup Z that clears the workpiece and fixtures.",
        Plane::Retract => "Retract is the return height between passes or plunges. Choose its reference and setup-Z offset above material that must be cleared.",
        Plane::Feed => "Feed height is where the approach changes from rapid travel to the plunge feed. Choose a reference and setup-Z offset, or disable this optional plane to use the normal strategy approach.",
        Plane::Top => "Top is the upper cutting reference. Choose a model, stock, contour or other available reference and apply an offset along setup Z.",
        Plane::Bottom => "Bottom is the lower cutting limit. Choose its reference and setup-Z offset to set the final machining depth.",
    }
}

fn illustration(help: &str) -> Option<&'static [u8]> {
    if help == help::CLIMB {
        Some(include_bytes!("../assets/help/climb-conventional.svg").as_slice())
    } else if [
        help::LEAD_IN,
        help::LEAD_OUT,
        Field::LeadOverlap.help(),
        Field::LeadInSize.help(),
        Field::LeadOutSize.help(),
        Toggle::SameLeadOut.help(),
        Toggle::FeedRetract.help(),
        Number::LeadInSweep.help(),
        Number::LeadOutSweep.help(),
        Number::LeadEndDistance.help(),
    ]
    .contains(&help)
    {
        Some(include_bytes!("../assets/help/leads.svg").as_slice())
    } else if [help::PLUNGE, Field::PlungeA.help(), Field::PlungeB.help()].contains(&help)
        || [
            Plunge::Ramp { angle_deg: 5.0 },
            Plunge::Helix {
                radius: 1.0,
                pitch: 1.0,
            },
            Plunge::ZigZag {
                length: 1.0,
                angle_deg: 5.0,
            },
        ]
        .into_iter()
        .any(|plunge| {
            [true, false]
                .into_iter()
                .any(|first| plunge_param_help(plunge, first) == Some(help))
        })
    {
        Some(include_bytes!("../assets/help/plunge.svg").as_slice())
    } else if [
        HOLDING_TABS,
        Field::TabWidth.help(),
        Field::TabHeight.help(),
        Field::TabSpacing.help(),
        Number::TabCount.help(),
        "Rectangular tabs keep a flat top; triangular tabs rise and fall gradually.",
    ]
    .contains(&help)
    {
        Some(include_bytes!("../assets/help/tabs.svg").as_slice())
    } else if [
        STOCK_TO_LEAVE,
        Field::ProfileOffset.help(),
        Number::FloorLeave.help(),
        Field::StockLeaveRadial.help(),
        Field::StockLeaveAxial.help(),
    ]
    .contains(&help)
    {
        Some(include_bytes!("../assets/help/allowances.svg").as_slice())
    } else if [
        Field::Stepdown.help(),
        Field::Stepover.help(),
        Number::FinishStep.help(),
        Number::FinishDepthStep.help(),
        Number::FinishCount.help(),
        Number::FinishDepths.help(),
        Number::SpringPasses.help(),
        Toggle::Finishing.help(),
        Toggle::FinishOnlyFinal.help(),
        Toggle::Even.help(),
        Toggle::RoughFinal.help(),
    ]
    .contains(&help)
    {
        Some(include_bytes!("../assets/help/passes.svg").as_slice())
    } else if [
        Field::Clearance.help(),
        Field::Retract.help(),
        Field::TopOfStock.help(),
        Field::Depth.help(),
    ]
    .contains(&help)
        || cam_model::HeightPlaneKind::ALL
            .into_iter()
            .any(|plane| height_help(plane) == help)
    {
        Some(include_bytes!("../assets/help/heights.svg").as_slice())
    } else if [help::SIDE, PROFILE_ENTRY_SIDES].contains(&help) {
        Some(include_bytes!("../assets/help/sides.svg").as_slice())
    } else {
        None
    }
}

pub(super) fn content(help: &str) -> Element<'static, Message> {
    let asset = illustration(help);
    let mut body = iced::widget::Column::new()
        .spacing(8)
        .push(text(help.to_owned()).size(12));
    if let Some(bytes) = asset {
        body = body.push(
            iced::widget::svg(iced::widget::svg::Handle::from_memory(bytes)).width(Length::Fill),
        );
    }
    container(body.width(if asset.is_some() { 360 } else { 280 }))
        .padding(10)
        .style(popup_surface)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_specific_help_keeps_its_illustration() {
        let plunge_image = illustration(help::PLUNGE).unwrap();
        for plunge in [
            Plunge::Ramp { angle_deg: 5.0 },
            Plunge::Helix {
                radius: 1.0,
                pitch: 1.0,
            },
            Plunge::ZigZag {
                length: 1.0,
                angle_deg: 5.0,
            },
        ] {
            for first in [true, false] {
                if let Some(help) = plunge_param_help(plunge, first) {
                    assert_eq!(illustration(help), Some(plunge_image));
                }
            }
        }
        let heights_image = illustration(Field::Clearance.help()).unwrap();
        for plane in cam_model::HeightPlaneKind::ALL {
            assert_eq!(illustration(height_help(plane)), Some(heights_image));
        }
    }

    #[test]
    fn illustrated_help_assets_are_valid_and_have_text_descriptions() {
        for (name, bytes) in [
            ("sides", include_str!("../assets/help/sides.svg")),
            ("allowances", include_str!("../assets/help/allowances.svg")),
            ("passes", include_str!("../assets/help/passes.svg")),
            ("leads", include_str!("../assets/help/leads.svg")),
            ("plunge", include_str!("../assets/help/plunge.svg")),
            ("heights", include_str!("../assets/help/heights.svg")),
            ("tabs", include_str!("../assets/help/tabs.svg")),
        ] {
            let doc = roxmltree::Document::parse(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(doc
                .descendants()
                .any(|n| n.has_tag_name("title") && n.text().is_some()));
        }
    }
}
