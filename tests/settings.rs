use objectlift::settings::{Hotkey, Settings};
#[test]
fn settings_round_trip_without_accepting_unknown_shortcuts() {
    let settings = Settings {
        cpu_only: true,
        quick_copy: false,
        quality_mode: objectlift::settings::QualityMode::Fast,
        hotkey: Hotkey::CtrlAltO,
    };
    let json = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&json).unwrap();
    assert!(loaded.cpu_only);
    assert_eq!(loaded.hotkey, Hotkey::CtrlAltO);
    assert!(serde_json::from_str::<Settings>(r#"{"cpu_only":false,"hotkey":"unknown"}"#).is_err());
}
