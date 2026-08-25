use crate::cli_sustain::SustainConfig;

#[test]
fn sustain_config_rejects_zero_iterations() {
    let error = SustainConfig::new(0, 10).unwrap_err();
    assert!(error.to_string().contains("iterations > 0"));
}

#[test]
fn sustain_interval_is_not_applied_after_the_final_iteration() {
    let config = SustainConfig::new(3, 10).unwrap();
    assert!(config.should_pause_after(1));
    assert!(config.should_pause_after(2));
    assert!(!config.should_pause_after(3));
}
