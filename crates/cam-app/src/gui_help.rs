//! One text/graphic content renderer for hover and persistent inspector help.
use super::*;
use profile_parameters::Number;

pub(super) const PROFILE_ENTRY_SIDES: &str = "Start outside/inside alternates the cutter side for outer and inner boundaries. Always outside/inside uses the same side for every contour in this entry. On follows each contour with the cutter centre. Open left/right follows the selected chain direction.";

pub(super) fn content(help: &str) -> Element<'static, Message> {
    let asset = if help == help::CLIMB {
        Some(include_bytes!("../assets/help/climb-conventional.svg").as_slice())
    } else if [
        help::LEAD_IN,
        help::LEAD_OUT,
        Field::LeadOverlap.help(),
        Number::LeadInSweep.help(),
        Number::LeadOutSweep.help(),
        Number::LeadEndDistance.help(),
    ]
    .contains(&help)
    {
        Some(include_bytes!("../assets/help/leads.svg").as_slice())
    } else if [help::PLUNGE, Field::PlungeA.help(), Field::PlungeB.help()].contains(&help) {
        Some(include_bytes!("../assets/help/plunge.svg").as_slice())
    } else if [
        Field::TabWidth.help(),
        Field::TabHeight.help(),
        Field::TabSpacing.help(),
        Number::TabCount.help(),
        "Rectangular tabs keep a flat top; triangular tabs rise and fall gradually.",
    ]
    .contains(&help)
    {
        Some(include_bytes!("../assets/help/tabs.svg").as_slice())
    } else if [Field::ProfileOffset.help(), Number::FloorLeave.help()].contains(&help) {
        Some(include_bytes!("../assets/help/allowances.svg").as_slice())
    } else if [
        Field::Stepdown.help(),
        Field::Stepover.help(),
        Number::FinishStep.help(),
        Number::FinishDepthStep.help(),
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
    {
        Some(include_bytes!("../assets/help/heights.svg").as_slice())
    } else if [help::SIDE, PROFILE_ENTRY_SIDES].contains(&help) {
        Some(include_bytes!("../assets/help/sides.svg").as_slice())
    } else {
        None
    };
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
