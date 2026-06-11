use super::events;

#[test]
fn event_constants_are_stable_names() {
    assert_eq!(
        events::HTTP_REQUEST_COMPLETED.name(),
        "http.request.completed"
    );
}
