use super::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier, Mutex};

#[derive(Default)]
struct Data {
    store: BTreeMap<String, Vec<u8>>,
    outcomes: BTreeMap<String, Result<SigningOutcome, SigningError>>,
    signed_actions: BTreeMap<String, PreparedAction>,
    sign_calls: usize,
    charges: usize,
    status_calls: usize,
    writes: usize,
}
#[derive(Clone)]
struct Host {
    data: Arc<Mutex<Data>>,
    hash: String,
    write_barrier: Option<Arc<Barrier>>,
    fail_after_write: bool,
    fail_after_sign: bool,
    status_error: Option<SigningError>,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            data: Arc::default(),
            hash: "package-a".into(),
            write_barrier: None,
            fail_after_write: false,
            fail_after_sign: false,
            status_error: None,
        }
    }
}
impl SigningHost for Host {
    fn package_hash(&self) -> &str {
        &self.hash
    }
    fn store_get(&mut self, key: &str) -> Result<Option<Vec<u8>>, SigningError> {
        Ok(self.data.lock().unwrap().store.get(key).cloned())
    }
    fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<bool, SigningError> {
        if let Some(barrier) = self.write_barrier.take() {
            barrier.wait();
        }
        let mut d = self.data.lock().unwrap();
        if d.store.contains_key(key) {
            return Ok(false);
        }
        d.store.insert(key.into(), value.to_vec());
        d.writes += 1;
        if self.fail_after_write {
            return Err(backend("crash after store write"));
        }
        Ok(true)
    }
    fn sign(
        &mut self,
        _: &str,
        action: &PreparedAction,
        key: &str,
    ) -> Result<SigningOutcome, SigningError> {
        let mut d = self.data.lock().unwrap();
        d.sign_calls += 1;
        if let Some(stored) = d.signed_actions.get(key) {
            if stored != action {
                return Err(SigningError {
                    code: ErrorCode::Conflict,
                    message: "different frozen action".into(),
                });
            }
        } else {
            d.signed_actions.insert(key.into(), action.clone());
        }
        let result = d.outcomes.get(key).cloned().unwrap_or_else(|| {
            d.charges += 1;
            let result = Ok(SigningOutcome::Signed {
                signature: vec![7; 65],
            });
            d.outcomes.insert(key.into(), result.clone());
            result
        });
        if self.fail_after_sign {
            return Err(backend("crash after signing"));
        }
        result
    }
    fn status(&mut self, _: &str, key: &str) -> Result<SigningOutcome, SigningError> {
        let mut d = self.data.lock().unwrap();
        d.status_calls += 1;
        if let Some(e) = &self.status_error {
            return Err(e.clone());
        }
        d.outcomes.get(key).cloned().unwrap_or_else(|| {
            Err(SigningError {
                code: ErrorCode::NotFound,
                message: "missing".into(),
            })
        })
    }
    fn now_ms(&mut self) -> Result<u64, SigningError> {
        Ok(123)
    }
}
fn action(value: u8) -> PreparedAction {
    PreparedAction::new(
        "test.message",
        SigningPayload::ed25519_message(vec![value]).unwrap(),
        ReviewArtifact::new("test.message", 1, vec![value]).unwrap(),
    )
    .unwrap()
}
fn open(h: &mut Host) -> Attempt<u8> {
    Attempt::open(h, "scope", || Ok((action(1), 1))).unwrap()
}
fn terminal(h: &mut Host, key: &str, code: ErrorCode) {
    h.data.lock().unwrap().outcomes.insert(
        key.into(),
        Err(SigningError {
            code,
            message: "terminal".into(),
        }),
    );
}

#[test]
fn open_race_returns_winners_action_and_context() {
    let h = Host {
        write_barrier: Some(Arc::new(Barrier::new(2))),
        ..Host::default()
    };
    let h2 = h.clone();
    let a = std::thread::spawn(move || {
        Attempt::open(&mut h.clone(), "scope", || Ok((action(1), 1u8))).unwrap()
    });
    let b = std::thread::spawn(move || {
        Attempt::open(&mut h2.clone(), "scope", || Ok((action(2), 2u8))).unwrap()
    });
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_eq!(a.action(), b.action());
    assert_eq!(a.context(), b.context());
    assert_eq!(a.operation_key(), "scope#0");
}
#[test]
fn corrupt_record_and_oversized_context_fail_without_replacement() {
    let mut h = Host::default();
    h.data
        .lock()
        .unwrap()
        .store
        .insert("scope.0.json".into(), b"broken".to_vec());
    assert_eq!(
        Attempt::<()>::open(&mut h, "scope", || panic!("must not rebuild"))
            .unwrap_err()
            .code,
        ErrorCode::Backend
    );
    assert_eq!(h.data.lock().unwrap().writes, 0);
    assert!(Attempt::open(&mut h, "large", || Ok((action(1), "x".repeat(MAX_CONTEXT)))).is_err());
    assert!(Attempt::<()>::load(&mut h, &"x".repeat(254)).is_err());
}
#[test]
fn normalization_is_fail_closed_and_read_only() {
    let cases = [
        (
            Ok(SigningOutcome::Signed { signature: vec![] }),
            AttemptState::Signed,
        ),
        (
            Ok(SigningOutcome::ApprovalPending {
                action_id: "a".into(),
                expires_ms: 1,
            }),
            AttemptState::Pending {
                action_id: "a".into(),
                expires_ms: 1,
            },
        ),
        (
            Ok(SigningOutcome::RecoveryRequired {
                status: "approved_retry".into(),
            }),
            AttemptState::Approved,
        ),
        (
            Ok(SigningOutcome::RecoveryRequired {
                status: "eligibility_approved_retry".into(),
            }),
            AttemptState::Approved,
        ),
        (
            Ok(SigningOutcome::RecoveryRequired {
                status: "signing_outcome_ambiguous".into(),
            }),
            AttemptState::RecoveryRequired {
                status: "signing_outcome_ambiguous".into(),
            },
        ),
    ];
    for (r, s) in cases {
        assert_eq!(normalize(r).unwrap(), s);
    }
    for (code, state) in [
        (ErrorCode::NotFound, AttemptState::NotStarted),
        (ErrorCode::Expired, AttemptState::Expired),
        (ErrorCode::Cancelled, AttemptState::Cancelled),
        (ErrorCode::Denied, AttemptState::Denied),
    ] {
        assert_eq!(
            normalize(Err(SigningError {
                code,
                message: String::new()
            }))
            .unwrap(),
            state
        );
    }
    for code in [
        ErrorCode::Invalid,
        ErrorCode::Backend,
        ErrorCode::Conflict,
        ErrorCode::Unsupported,
    ] {
        assert_eq!(
            normalize(Err(SigningError {
                code,
                message: "operation not found".into()
            }))
            .unwrap_err()
            .code,
            code
        );
    }
    let mut h = Host::default();
    let attempt = open(&mut h);
    let before = h.data.lock().unwrap().writes;
    assert_eq!(
        attempt.state(&mut h, "w").unwrap(),
        AttemptState::NotStarted
    );
    assert!(matches!(
        approval_read_with_host(&mut h, "w", &["scope"]),
        DispatchResponse::Error { code: -3, .. }
    ));
    assert_eq!(before, h.data.lock().unwrap().writes);
}
#[test]
fn pending_projection_and_halt_keep_retry_details_without_signature() {
    let mut h = Host::default();
    let attempt = open(&mut h);
    h.data.lock().unwrap().outcomes.insert(
        "scope#0".into(),
        Ok(SigningOutcome::ApprovalPending {
            action_id: "approval-7".into(),
            expires_ms: 100,
        }),
    );
    let halt = sign_or_resume(&mut h, "w", &attempt).unwrap_err();
    assert_eq!(halt.code, -3);
    assert!(halt.message.contains("approval-7"));
    assert!(halt.message.contains("retry_write_body"));
    let DispatchResponse::Read(bytes) = approval_read_with_host(&mut h, "w", &["scope"]) else {
        panic!()
    };
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["expired_locally"], true);
    assert_eq!(json["action_id"], "approval-7");
    assert!(json.get("signature").is_none());
}
#[test]
fn renewal_matrix_and_zero_spend_refresh() {
    let cases = [
        ErrorCode::NotFound,
        ErrorCode::Denied,
        ErrorCode::Expired,
        ErrorCode::Cancelled,
        ErrorCode::Backend,
        ErrorCode::Conflict,
    ];
    for code in cases {
        let mut h = Host::default();
        let attempt = open(&mut h);
        terminal(&mut h, "scope#0", code);
        let result = attempt.renew(&mut h, "w", |_| Ok((action(2), 2)));
        assert_eq!(
            result.is_ok(),
            matches!(code, ErrorCode::Expired | ErrorCode::Cancelled),
            "{code:?}"
        );
    }
    for outcome in [
        SigningOutcome::ApprovalPending {
            action_id: "a".into(),
            expires_ms: 0,
        },
        SigningOutcome::RecoveryRequired {
            status: "approved_retry".into(),
        },
        SigningOutcome::RecoveryRequired {
            status: "operation_in_progress".into(),
        },
        SigningOutcome::Signed { signature: vec![] },
    ] {
        let mut h = Host::default();
        let attempt = open(&mut h);
        h.data
            .lock()
            .unwrap()
            .outcomes
            .insert("scope#0".into(), Ok(outcome));
        assert!(attempt.renew(&mut h, "w", |_| Ok((action(2), 2))).is_err());
    }
    for package_changed in [false, true] {
        let mut h = Host::default();
        let attempt = Attempt::open(&mut h, "scope", || {
            Ok((action(1).with_spend(Spend::None), 1u8))
        })
        .unwrap();
        sign_or_resume(&mut h, "w", &attempt).unwrap();
        if package_changed {
            h.hash = "package-b".into();
        }
        assert!(attempt.renew(&mut h, "w", |_| Ok((action(2), 2))).is_err());
        let attempt = Attempt::<u8>::load(&mut h, "scope").unwrap().unwrap();
        let next = attempt
            .renew(&mut h, "w", |_| Ok((action(2).with_spend(Spend::None), 2)))
            .unwrap();
        assert_eq!(next.generation(), 1);
    }
}
#[test]
fn concurrent_renewals_and_stale_handle_only_sign_winning_generation() {
    let mut h = Host::default();
    let stale = open(&mut h);
    terminal(&mut h, "scope#0", ErrorCode::Expired);
    let a = Attempt::<u8>::load(&mut h, "scope").unwrap().unwrap();
    let b = Attempt::<u8>::load(&mut h, "scope").unwrap().unwrap();
    h.write_barrier = Some(Arc::new(Barrier::new(2)));
    let mut h2 = h.clone();
    let mut h3 = h.clone();
    h.write_barrier = None;
    let a = std::thread::spawn(move || {
        let next = a.renew(&mut h2, "w", |_| Ok((action(2), 2))).unwrap();
        sign_or_resume(&mut h2, "w", &next).unwrap();
        next
    });
    let b = std::thread::spawn(move || {
        let next = b.renew(&mut h3, "w", |_| Ok((action(3), 3))).unwrap();
        sign_or_resume(&mut h3, "w", &next).unwrap();
        next
    });
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_eq!(a.action(), b.action());
    assert_eq!(h.data.lock().unwrap().charges, 1);
    terminal(&mut h, "scope#1", ErrorCode::Cancelled);
    let second = a.renew(&mut h, "w", |_| Ok((action(4), 4))).unwrap();
    assert_eq!(second.generation(), 2);
    let stale = stale
        .renew(&mut h, "w", |_| panic!("stale builder must not run"))
        .unwrap();
    assert_eq!(stale.generation(), 2);
    assert_eq!(h.data.lock().unwrap().writes, 3);
}
#[test]
fn crash_after_store_and_sign_retries_same_operation_and_charge() {
    let mut h = Host {
        fail_after_write: true,
        ..Host::default()
    };
    assert!(Attempt::open(&mut h, "scope", || Ok((action(1), 1u8))).is_err());
    h.fail_after_write = false;
    let attempt = Attempt::open(&mut h, "scope", || panic!("must load frozen record")).unwrap();
    h.fail_after_sign = true;
    assert!(sign_or_resume(&mut h, "w", &attempt).is_err());
    h.fail_after_sign = false;
    assert_eq!(sign_or_resume(&mut h, "w", &attempt).unwrap(), vec![7; 65]);
    assert_eq!(h.data.lock().unwrap().charges, 1);
    assert_eq!(h.data.lock().unwrap().sign_calls, 2);
    terminal(&mut h, "scope#0", ErrorCode::Expired);
    h.fail_after_write = true;
    assert!(attempt.renew(&mut h, "w", |_| Ok((action(2), 2))).is_err());
    h.fail_after_write = false;
    let attempt = Attempt::<u8>::load(&mut h, "scope").unwrap().unwrap();
    assert_eq!(attempt.generation(), 1);
    sign_or_resume(&mut h, "w", &attempt).unwrap();
    assert_eq!(h.data.lock().unwrap().charges, 2);
}
#[test]
fn package_change_and_status_failure_have_separate_signing_rules() {
    let mut h = Host::default();
    let attempt = open(&mut h);
    h.hash = "package-b".into();
    assert_eq!(
        attempt.state(&mut h, "w").unwrap(),
        AttemptState::PackageChanged
    );
    assert_eq!(sign_or_resume(&mut h, "w", &attempt).unwrap_err().code, -3);
    assert_eq!(h.data.lock().unwrap().status_calls, 0);
    assert_eq!(h.data.lock().unwrap().sign_calls, 0);
    assert!(attempt.renew(&mut h, "w", |_| panic!()).is_err());
    h.hash = "package-a".into();
    h.status_error = Some(backend("unavailable"));
    let attempt = Attempt::<u8>::load(&mut h, "scope").unwrap().unwrap();
    assert!(attempt.state(&mut h, "w").is_err());
    assert!(matches!(
        approval_read_with_host(&mut h, "w", &["scope"]),
        DispatchResponse::Error { code: -4, .. }
    ));
    sign_or_resume(&mut h, "w", &attempt).unwrap();
}
#[test]
fn generation_cap_and_error_dispatch_codes() {
    let mut h = Host::default();
    let mut attempt = open(&mut h);
    for n in 1..MAX_GENERATIONS {
        terminal(&mut h, &attempt.operation_key(), ErrorCode::Expired);
        attempt = attempt
            .renew(&mut h, "w", |_| Ok((action(n as u8), n as u8)))
            .unwrap();
    }
    terminal(&mut h, &attempt.operation_key(), ErrorCode::Expired);
    assert!(attempt.renew(&mut h, "w", |_| panic!()).is_err());
    for (code, dispatch) in [
        (ErrorCode::Denied, -2),
        (ErrorCode::Invalid, -3),
        (ErrorCode::Unsupported, -3),
        (ErrorCode::Conflict, -4),
        (ErrorCode::Backend, -4),
    ] {
        let response: DispatchResponse = SigningError {
            code,
            message: "detail".into(),
        }
        .into();
        assert!(matches!(response,DispatchResponse::Error {code:c,..} if c==dispatch));
    }
}

#[test]
fn retained_handle_refuses_missing_history() {
    let mut h = Host::default();
    let first = open(&mut h);
    terminal(&mut h, "scope#0", ErrorCode::Expired);
    let second = first.renew(&mut h, "w", |_| Ok((action(2), 2))).unwrap();
    h.data.lock().unwrap().store.remove("scope.1.json");
    let error = second
        .renew(&mut h, "w", |_| panic!("missing history must not rebuild"))
        .unwrap_err();
    assert_eq!(error.code, -4);
    assert_eq!(error.message, "attempt history lost");
}
