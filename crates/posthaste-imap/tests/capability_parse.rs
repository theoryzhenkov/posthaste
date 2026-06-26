use imap_client::imap_types::{core::Atom, response::Capability};

#[test]
fn condstore_capability_is_parsed_as_condstore() {
    let atom = Atom::try_from("CONDSTORE").unwrap();
    let cap = Capability::from(atom);
    assert!(matches!(cap, Capability::CondStore), "got {cap}");
    assert_eq!(cap.to_string(), "CONDSTORE");
}

#[test]
fn qresync_capability_is_parsed_as_qresync() {
    let atom = Atom::try_from("QRESYNC").unwrap();
    let cap = Capability::from(atom);
    assert!(matches!(cap, Capability::QResync), "got {cap}");
    assert_eq!(cap.to_string(), "QRESYNC");
}
