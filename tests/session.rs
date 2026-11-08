use objectlift::Session;
#[test]
fn cancelled_session_rejects_late_model_results_and_cannot_export() {
    let mut session = Session::default();
    let old = session.begin();
    session.cancel();
    let current = session.begin();
    assert_ne!(old, current);
    assert!(!session.accepts(old));
    assert!(session.accepts(current));
    session.cancel();
    assert!(!session.accepts(current));
}

#[test]
fn older_selection_revision_cannot_replace_newer_corrections() {
    let mut session = Session::default();
    let id = session.begin();
    let old = session.revise();
    let current = session.revise();
    assert!(!session.accepts_result(id, old));
    assert!(session.accepts_result(id, current));
    session.cancel();
    assert!(!session.accepts_result(id, current));
}
