//! Browser effects are host-reported. Durable dispatch is at most once; it is
//! deliberately not an exactly-once transaction with a native browser API.
use super::*;
use babble_capabilities::invocation::{browser::*, is_browser_invocation, new_context_epoch};

impl<P: JudgmentProvider> LocalNode<P> {
    pub fn prepare_browser_invocation(
        &mut self,
        context: InvocationContext,
        request_key: &str,
        method: &str,
        payload: Value,
        deadline: Timestamp,
    ) -> Result<InvocationRecord> {
        self.reconcile_surface_permissions()?;
        let payload = normalize_browser_payload(method, payload)?;
        let definition = self.browser_definition(method)?;
        let key = invocation_key(&context, request_key)?;
        if let Some(existing) = self.store.invocation(&key)? {
            let intent = existing.intent();
            if intent.context != context || intent.method != method || intent.payload != payload {
                return Err(conflict(
                    "invocation key reused with changed intent or context",
                ));
            }
            return self.refresh_invocation(existing);
        }
        self.check_social_context(&context)?;
        self.check_browser_scope(&context, &definition)?;
        let now = Timestamp::now();
        let deadline = deadline.min(Timestamp(
            now.0
                + std::time::Duration::from_millis(
                    definition
                        .quota
                        .max_call_ms
                        .min(MAX_INVOCATION_TTL_MS as u64),
                ),
        ));
        let intent = InvocationIntent {
            request_key: request_key.into(),
            context,
            method: method.into(),
            method_version: 2,
            capability: definition.id,
            capability_version: definition.version,
            scope: json!({}),
            executor: browser_executor(),
            payload,
            created_at: now,
            deadline,
        };
        intent.validate()?;
        self.store.check_social_invocation_quota(&intent, now)?;
        self.store
            .prepare_invocation(intent, now)
            .map_err(publication_error)
    }

    pub fn status_browser_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        id: &InvocationId,
    ) -> Result<InvocationRecord> {
        self.check_ready()?;
        let record = self
            .store
            .invocation(&invocation_key(context, request_key)?)?
            .ok_or_else(|| Error::NotFound("browser invocation".into()))?;
        require_context(&record, context)?;
        if record.id() != id
            || !is_browser_invocation(record.intent().capability.as_str())
            || record.intent().executor != browser_executor()
        {
            return Err(conflict(
                "browser invocation challenge or executor mismatch",
            ));
        }
        self.refresh_invocation(record)
    }

    pub fn decide_browser_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        id: &InvocationId,
        allow: bool,
    ) -> Result<InvocationRecord> {
        self.reconcile_surface_permissions()?;
        let record = self.status_browser_invocation(context, request_key, id)?;
        if record.state() != &InvocationState::Pending {
            return Ok(record);
        }
        self.check_browser_execution(&record)?;
        self.store
            .transition_invocation(
                &record,
                if allow {
                    InvocationAction::Approve
                } else {
                    InvocationAction::Deny
                },
                context,
                Timestamp::now(),
            )
            .map_err(publication_error)
    }

    pub fn cancel_browser_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        id: &InvocationId,
    ) -> Result<InvocationRecord> {
        let record = self.status_browser_invocation(context, request_key, id)?;
        if !unconsumed(&record) {
            return Ok(record);
        }
        self.store
            .transition_invocation(&record, InvocationAction::Cancel, context, Timestamp::now())
            .map_err(publication_error)
    }

    /// A ticket exists only in the winning commit's return value. Never recover
    /// one from a stored phase, even if the first HTTP response was lost.
    pub fn dispatch_browser_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        id: &InvocationId,
    ) -> Result<(InvocationRecord, Option<BrowserExecutionTicket>)> {
        self.reconcile_surface_permissions()?;
        let record = self.status_browser_invocation(context, request_key, id)?;
        if record.consumed_at().is_some() || record.state().is_terminal() {
            return Ok((record, None));
        }
        if record.state() != &InvocationState::Approved {
            return Err(conflict("browser invocation is not approved"));
        }
        self.check_browser_execution(&record)?;
        let dispatch_id = new_context_epoch()?;
        let next = self
            .store
            .transition_invocation(
                &record,
                InvocationAction::Dispatch {
                    dispatch_id: dispatch_id.clone(),
                },
                context,
                Timestamp::now(),
            )
            .map_err(publication_error)?;
        Ok((
            next,
            Some(BrowserExecutionTicket {
                dispatch_id,
                executor: BROWSER_EXECUTOR.into(),
            }),
        ))
    }

    pub fn acknowledge_browser_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        id: &InvocationId,
        ack: AcknowledgeBrowserInvocationRequest,
    ) -> Result<InvocationRecord> {
        let record = self.status_browser_invocation(context, request_key, id)?;
        ack.result.validate_for(&record.intent().method)?;
        let action = match &ack.result {
            BrowserInvocationResult::Failed { code } => InvocationAction::FailExternal {
                dispatch_id: ack.dispatch_id,
                code: code.as_str().into(),
            },
            _ => InvocationAction::CompleteExternal {
                dispatch_id: ack.dispatch_id,
                result: serde_json::to_value(&ack.result).map_err(encoding)?,
            },
        };
        if record.action() == Some(&action) {
            return Ok(record);
        }
        self.store
            .transition_invocation(&record, action, context, Timestamp::now())
            .map_err(publication_error)
    }

    fn browser_definition(&self, method: &str) -> Result<CapabilityDefinition> {
        let capability = method
            .strip_suffix("")
            .filter(|c| is_browser_invocation(c))
            .ok_or_else(|| conflict("unsupported browser invocation method"))?;
        self.capability_definitions()
            .into_iter()
            .find(|d| {
                d.id.as_str() == capability
                    && d.version == 1
                    && d.permission == PermissionMode::AskEachTime
            })
            .ok_or_else(|| conflict("browser invocation capability unavailable"))
    }

    fn check_browser_scope(
        &self,
        context: &InvocationContext,
        definition: &CapabilityDefinition,
    ) -> Result<()> {
        let object = self.require_object(&context.object_id)?;
        if !object.capabilities.iter().any(|c| {
            c.id == definition.id.as_str()
                && c.version == definition.version
                && c.scope == json!({})
        }) {
            return Err(conflict("browser invocation is outside declared scope"));
        }
        Ok(())
    }

    fn check_browser_execution(&self, record: &InvocationRecord) -> Result<()> {
        self.check_social_context(&record.intent().context)?;
        let definition = self.browser_definition(&record.intent().method)?;
        self.check_browser_scope(&record.intent().context, &definition)?;
        if record.intent().executor != browser_executor()
            || Timestamp::now() >= record.intent().deadline
        {
            return Err(conflict("browser invocation executor or deadline invalid"));
        }
        self.store
            .check_social_invocation_quota(record.intent(), Timestamp::now())
    }
}

fn browser_executor() -> InvocationExecutor {
    InvocationExecutor::External {
        provider: BROWSER_EXECUTOR.into(),
        version: "1".into(),
    }
}

fn normalize_browser_payload(method: &str, value: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Clipboard {
        text: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fullscreen {
        target_hint: Option<String>,
        navigation_ui: Option<String>,
    }
    match method {
        "babble.clipboard.write" => {
            let input: Clipboard = serde_json::from_value(value).map_err(encoding)?;
            Ok(json!({"text":input.text}))
        }
        "babble.fullscreen.enter" => {
            let input: Fullscreen = serde_json::from_value(value).map_err(encoding)?;
            if input
                .target_hint
                .as_ref()
                .is_some_and(|v| v.trim().is_empty() || v.len() > 256)
            {
                return Err(conflict("invalid fullscreen target_hint"));
            }
            let navigation_ui = input.navigation_ui.unwrap_or_else(|| "auto".into());
            if !matches!(navigation_ui.as_str(), "auto" | "hide" | "show") {
                return Err(conflict("invalid fullscreen navigation_ui"));
            }
            Ok(json!({"target_hint":input.target_hint,"navigation_ui":navigation_ui}))
        }
        _ => Err(conflict("unsupported browser invocation method")),
    }
}
