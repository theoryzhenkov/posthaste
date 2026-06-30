use super::*;

impl AccountMutationService {
    pub fn patch_app_settings(
        &self,
        request: PatchAppSettingsMutation,
    ) -> Result<posthaste_domain::AppSettings, RuntimeError> {
        let mut settings = self.service.get_app_settings()?;

        // Each entry pairs an audit name with the merge it performs, so the
        // `changed` list reported in the event can never drift from what was
        // actually applied. Order here defines the order of `changed`.
        let patches = [
            AppSettingsFieldPatch {
                name: "defaultAccount",
                present: request.default_account_id.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    self.apply_default_account(settings, &request.default_account_id)
                }),
            },
            AppSettingsFieldPatch {
                name: "automationRules",
                present: request.automation_rules.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(rules) = &request.automation_rules {
                        settings.automation_rules = normalize_automation_rules(rules);
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "automationDrafts",
                present: request.automation_drafts.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(drafts) = &request.automation_drafts {
                        settings.automation_drafts = normalize_automation_rules(drafts);
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "cachePolicy",
                present: request.cache_policy.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(policy) = &request.cache_policy {
                        settings.cache_policy = normalize_cache_policy(policy.clone());
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "appearance",
                present: request.appearance.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(appearance) = &request.appearance {
                        settings.appearance = Some(appearance.clone());
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "notifications",
                present: request.notifications.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(notifications) = &request.notifications {
                        settings.notifications = Some(notifications.clone());
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "mailbox_colors",
                present: request.mailbox_colors.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(mailbox_colors) = &request.mailbox_colors {
                        settings.mailbox_colors.clone_from(mailbox_colors);
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "smartMailboxOrder",
                present: request.smart_mailbox_order.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(order) = &request.smart_mailbox_order {
                        settings.smart_mailbox_order.clone_from(order);
                    }
                    Ok(())
                }),
            },
            AppSettingsFieldPatch {
                name: "accountOrder",
                present: request.account_order.is_some(),
                apply: Box::new(|settings: &mut AppSettings| {
                    if let Some(order) = &request.account_order {
                        settings.account_order.clone_from(order);
                    }
                    Ok(())
                }),
            },
        ];

        let mut changed = Vec::new();
        for patch in patches {
            if patch.present {
                (patch.apply)(&mut settings)?;
                changed.push(patch.name);
            }
        }

        validate_automation_rules(&settings.automation_rules)?;
        validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)?;

        self.service.put_app_settings(&settings)?;
        self.append_and_publish_event(
            &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
            EVENT_TOPIC_SETTINGS_UPDATED,
            config_event_payload(
                vec![ResourceChange::app_settings_updated()],
                json!({
                    "scope": "app",
                    "changed": changed,
                }),
            ),
        )?;
        // On-demand "backfill now" resets the current ruleset's job to pending so
        // the supervisor re-applies it even when nothing in the fingerprint
        // changed (actions are idempotent). Reset creates the job if absent, so
        // it fully supersedes the ensure-on-rules-change path below.
        if request.force_backfill {
            self.service
                .reset_automation_backfills_for_current_rules()?;
        } else if request.automation_rules.is_some() {
            self.service
                .ensure_automation_backfills_for_current_rules()?;
        }
        Ok(settings)
    }

    fn apply_default_account(
        &self,
        settings: &mut AppSettings,
        default_account_id: &Option<Option<String>>,
    ) -> Result<(), RuntimeError> {
        let Some(default_account_id) = default_account_id else {
            return Ok(());
        };
        match default_account_id {
            Some(id) => {
                let account_id = AccountId::from(id.as_str());
                validate_default_account_exists(
                    &account_id,
                    self.service.get_source(&account_id)?.is_some(),
                )?;
                settings.default_account_id = Some(account_id);
            }
            None => settings.default_account_id = None,
        }
        Ok(())
    }
}

/// One patchable field of [`AppSettings`]: the audit name reported in the
/// `settings.updated` event, whether the request carried a value for it, and the
/// merge that applies that value. Bundling the three keeps the audit list in
/// lockstep with the applied changes.
type AppSettingsPatchApply<'a> = Box<dyn FnOnce(&mut AppSettings) -> Result<(), RuntimeError> + 'a>;

struct AppSettingsFieldPatch<'a> {
    name: &'static str,
    present: bool,
    apply: AppSettingsPatchApply<'a>,
}

fn normalize_cache_policy(mut policy: CachePolicy) -> CachePolicy {
    policy.hard_cap_bytes = policy.hard_cap_bytes.max(policy.soft_cap_bytes);
    policy
}
