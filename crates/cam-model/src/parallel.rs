//! Surface finishing controls, in setup coordinates and millimetres.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ParallelParams {
    pub tolerance: f64,
    pub stepover: f64,
    pub angle: f64,
    pub feed: f64,
    pub plunge_feed: f64,
    pub both_ways: bool,
    pub containment: crate::AdaptiveContainment,
    pub additional_offset: f64,
}
impl Default for ParallelParams {
    fn default() -> Self {
        Self {
            tolerance: 0.02,
            stepover: 0.5,
            angle: 0.,
            feed: 500.,
            plunge_feed: 100.,
            both_ways: true,
            containment: crate::AdaptiveContainment::Inside,
            additional_offset: 0.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_defaults_and_project_operation_roundtrip() {
        let params: ParallelParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params, ParallelParams::default());
        let mut operation = crate::Operation::Parallel(crate::ParallelOp {
            id: 3,
            tool: 1,
            spindle_rpm: 4000.,
            work_offset: 2,
            tool_preset: None,
            cooling: Default::default(),
            entry_feeds: Default::default(),
            params: ParallelParams {
                angle: 37.,
                stepover: 0.25,
                both_ways: false,
                ..params
            },
        });
        operation.set_id(5);
        operation.set_tool(4);
        operation.set_work_offset(3);
        let json = serde_json::to_string(&operation).unwrap();
        assert_eq!(
            serde_json::from_str::<crate::Operation>(&json).unwrap(),
            operation
        );
        assert_eq!(operation.tools(), vec![4]);
        assert_eq!(operation.work_offset(), 3);
    }
}
