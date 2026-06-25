use super::*;

#[test]
fn secret_keep_preserves_existing_refs_without_store_instruction() {
    let account_id = AccountId::from("primary");
    let request = secret_request(SecretWriteMode::Keep, None);
    let os_ref = secret_ref(SecretKind::Os, "account:primary");
    let env_ref = secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD");

    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, None, &request),
            "keep should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Preserve,
            store_instruction: SecretStoreInstruction::None,
        }
    );
    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, Some(&os_ref), &request),
            "keep should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(Some(os_ref.clone())),
            store_instruction: SecretStoreInstruction::None,
        }
    );
    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, Some(&env_ref), &request),
            "keep should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(Some(env_ref)),
            store_instruction: SecretStoreInstruction::None,
        }
    );
}

#[test]
fn secret_replace_updates_os_ref_or_saves_new_managed_ref() {
    let account_id = AccountId::from("primary");
    let request = secret_request(SecretWriteMode::Replace, Some("  replacement  "));
    let default_ref = account_secret_ref(&account_id);
    let os_ref = secret_ref(SecretKind::Os, "account:custom");
    let env_ref = secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD");

    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, None, &request),
            "replace should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(Some(default_ref.clone())),
            store_instruction: SecretStoreInstruction::Save {
                secret_ref: default_ref.clone(),
                password: "replacement",
            },
        }
    );
    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, Some(&os_ref), &request),
            "replace should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(Some(os_ref.clone())),
            store_instruction: SecretStoreInstruction::Update {
                secret_ref: os_ref,
                password: "replacement",
            },
        }
    );
    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, Some(&env_ref), &request),
            "replace should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(Some(default_ref.clone())),
            store_instruction: SecretStoreInstruction::Save {
                secret_ref: default_ref,
                password: "replacement",
            },
        }
    );
}

#[test]
fn secret_clear_clears_account_ref_and_deletes_only_os_refs() {
    let account_id = AccountId::from("primary");
    let request = secret_request(SecretWriteMode::Clear, None);
    let os_ref = secret_ref(SecretKind::Os, "account:primary");
    let env_ref = secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD");

    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, None, &request),
            "clear should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(None),
            store_instruction: SecretStoreInstruction::None,
        }
    );
    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, Some(&env_ref), &request),
            "clear should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(None),
            store_instruction: SecretStoreInstruction::None,
        }
    );
    assert_eq!(
        expect_decision(
            decide_secret_instruction(&account_id, Some(&os_ref), &request),
            "clear should be valid"
        ),
        SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(None),
            store_instruction: SecretStoreInstruction::Delete { secret_ref: os_ref },
        }
    );
}

#[test]
fn secret_replace_rejects_missing_or_blank_passwords() {
    for password in [None, Some(""), Some("   ")] {
        let request = secret_request(SecretWriteMode::Replace, password);
        let error = decide_secret_instruction(&AccountId::from("primary"), None, &request)
            .expect_err("replace without a nonblank password should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, ApiErrorCode::InvalidSecret);
        assert_eq!(
            error.body.message,
            "secret.password is required when secret.mode is replace"
        );
    }
}

#[test]
fn apply_secret_instruction_replaces_env_ref_with_managed_os_ref() {
    let test_state = test_app_state();
    let mut account = test_account(Some(secret_ref(SecretKind::Env, "POSTHASTE_PASSWORD")));
    let previous_ref = account.transport.secret_ref.clone();
    let request = secret_request(SecretWriteMode::Replace, Some("  replacement  "));
    let expected_ref = account_secret_ref(&account.id);

    apply_secret_instruction(
        test_state.secret_store.as_ref(),
        &mut account,
        previous_ref.as_ref(),
        &request,
    )
    .unwrap_or_else(|error| {
        panic!(
            "replace should save the managed secret, got {:?}: {}",
            error.body.code, error.body.message
        )
    });

    assert_eq!(account.transport.secret_ref, Some(expected_ref.clone()));
    assert_eq!(
        test_state.secret_store.calls(),
        vec![SecretStoreCall::Save(
            expected_ref,
            "replacement".to_string()
        )]
    );
}
