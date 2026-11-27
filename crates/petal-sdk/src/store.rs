//! JSON records in the Petal's store, with failures as route errors.
//!
//! Each function comes in two forms: `get_json(key, …)` calls Bloom, and
//! `get_json_with(&mut host, key, …)` takes the [`Host`] the caller already
//! holds, so the code around it runs in a test.
//!
//! These read and write the state namespace. There is no JSON helper for
//! secrets on purpose: a decode error quotes what it could not parse. Read a
//! secret with [`crate::sdk::store_get_secret`] and decode it where the error
//! text is under the route's control.

use crate::host::{BloomHost, Host};
use crate::{DispatchResponse, backend, host_error, is_safe_segment};

/// The most key bytes [`list_names`] accepts from one listing.
pub const LIST_MAX_BYTES: usize = 256 * 1024;

/// Read and decode the record at `key`. A missing key is `Ok(None)`.
///
/// Errors: [`host_error`] for a failed read, including `-4` for a value over
/// `max_bytes`; `-4` when the stored bytes are not a `T`.
pub fn get_json<T: serde::de::DeserializeOwned>(
    key: &str,
    max_bytes: usize,
) -> Result<Option<T>, DispatchResponse> {
    get_json_with(&mut BloomHost, key, max_bytes)
}

/// [`get_json`] against the caller's host.
pub fn get_json_with<H: Host, T: serde::de::DeserializeOwned>(
    host: &mut H,
    key: &str,
    max_bytes: usize,
) -> Result<Option<T>, DispatchResponse> {
    let Some(bytes) = host.store_get_opt(key, max_bytes).map_err(host_error)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| backend(format!("stored record {key} is unreadable: {err}")))
}

/// Encode `value` and store it at `key`, replacing what was there.
pub fn put_json<T: serde::Serialize + ?Sized>(
    key: &str,
    value: &T,
) -> Result<(), DispatchResponse> {
    put_json_with(&mut BloomHost, key, value)
}

/// [`put_json`] against the caller's host.
pub fn put_json_with<H: Host, T: serde::Serialize + ?Sized>(
    host: &mut H,
    key: &str,
    value: &T,
) -> Result<(), DispatchResponse> {
    let bytes = encode(key, value)?;
    host.store_put(key, &bytes, false).map_err(host_error)
}

/// Encode `value` and store it at `key` only if the key is free.
///
/// Bloom answers a taken key with `denied: store key already exists`, so that
/// is a `-2` here like any other refusal, and Machine shows the caller a bare
/// "permission denied". A route that should say "that id is taken" calls
/// [`Host::store_put_new`] and matches `HostStatus::Denied` itself.
pub fn put_json_new<T: serde::Serialize + ?Sized>(
    key: &str,
    value: &T,
) -> Result<(), DispatchResponse> {
    put_json_new_with(&mut BloomHost, key, value)
}

/// [`put_json_new`] against the caller's host.
pub fn put_json_new_with<H: Host, T: serde::Serialize + ?Sized>(
    host: &mut H,
    key: &str,
    value: &T,
) -> Result<(), DispatchResponse> {
    let bytes = encode(key, value)?;
    host.store_put_new(key, &bytes, false).map_err(host_error)
}

/// The names of the records directly under `prefix`, with `suffix` removed,
/// sorted.
///
/// `list_names("drafts/alice/", ".json")` turns the key
/// `drafts/alice/7.json` into `7`. It leaves out keys that do not end in
/// `suffix`, keys in a deeper directory such as `drafts/alice/7/plan.json`,
/// and any name that is not a safe path segment, so every name can be listed
/// as a directory entry. Pass `""` to keep whole names.
///
/// `prefix` must be empty or end in `/`: without the `/`, `drafts/alice`
/// would also match `drafts/alice2/…`. A prefix under `creds/` lists the
/// secret namespace, as [`crate::sdk::store_list`] does.
///
/// Do not list wallets or accounts this way. Bloom lists a `[wallet]`
/// directory from its own projection; use `account_listing!()`.
pub fn list_names(prefix: &str, suffix: &str) -> Result<Vec<String>, DispatchResponse> {
    list_names_with(&mut BloomHost, prefix, suffix)
}

/// [`list_names`] against the caller's host.
pub fn list_names_with<H: Host>(
    host: &mut H,
    prefix: &str,
    suffix: &str,
) -> Result<Vec<String>, DispatchResponse> {
    if !(prefix.is_empty() || prefix.ends_with('/')) {
        return Err(backend(format!(
            "list_names prefix {prefix:?} must end in '/'"
        )));
    }
    let keys = host
        .store_list(prefix, LIST_MAX_BYTES)
        .map_err(host_error)?;
    let mut names: Vec<String> = keys
        .iter()
        .filter_map(|key| key.strip_prefix(prefix)?.strip_suffix(suffix))
        .filter(|name| is_safe_segment(name) && !name.contains('/'))
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

fn encode<T: serde::Serialize + ?Sized>(key: &str, value: &T) -> Result<Vec<u8>, DispatchResponse> {
    serde_json::to_vec(value).map_err(|err| backend(format!("encode record {key}: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DispatchResponse;
    use crate::host::Host;
    use crate::testing::FakeHost;

    #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
    struct Draft {
        amount: String,
    }

    fn error(response: DispatchResponse) -> (i32, String) {
        match response {
            DispatchResponse::Error { code, message } => (code, message),
            other => panic!("{other:?}"),
        }
    }

    fn draft() -> Draft {
        Draft { amount: "5".into() }
    }

    #[test]
    fn a_record_round_trips_and_a_missing_one_is_none() {
        let mut host = FakeHost::new();
        assert_eq!(
            get_json_with::<_, Draft>(&mut host, "drafts/alice/7.json", 1024).unwrap(),
            None
        );
        put_json_with(&mut host, "drafts/alice/7.json", &draft()).unwrap();
        assert_eq!(
            host.state("drafts/alice/7.json"),
            Some(&br#"{"amount":"5"}"#[..])
        );
        assert_eq!(
            get_json_with::<_, Draft>(&mut host, "drafts/alice/7.json", 1024).unwrap(),
            Some(draft())
        );
        // A second write replaces the first.
        put_json_with(
            &mut host,
            "drafts/alice/7.json",
            &Draft { amount: "6".into() },
        )
        .unwrap();
        assert_eq!(
            get_json_with::<_, Draft>(&mut host, "drafts/alice/7.json", 1024)
                .unwrap()
                .unwrap()
                .amount,
            "6"
        );
    }

    #[test]
    fn a_record_that_cannot_be_read_is_a_backend_error() {
        let mut host = FakeHost::new();
        host.with_state("drafts/7.json", br#"{"amount":5}"#);
        let (code, message) =
            error(get_json_with::<_, Draft>(&mut host, "drafts/7.json", 1024).unwrap_err());
        assert_eq!(code, -4);
        assert!(
            message.starts_with("stored record drafts/7.json is unreadable: "),
            "{message}"
        );
        // Over the limit is not "missing".
        let (code, message) =
            error(get_json_with::<_, Draft>(&mut host, "drafts/7.json", 4).unwrap_err());
        assert_eq!(
            (code, message.as_str()),
            (-4, "buffer too small: needs 12 bytes")
        );
    }

    #[test]
    fn a_failed_host_call_keeps_bloom_s_status() {
        let mut host = FakeHost::new();
        host.fail_next("store_get_opt", "denied: store")
            .fail_next("store_put", "backend: store put: disk full")
            .fail_next("store_list", "invalid: store prefix");
        let (code, message) =
            error(get_json_with::<_, Draft>(&mut host, "drafts/7.json", 1024).unwrap_err());
        assert_eq!(code, -2);
        if cfg!(feature = "host-error-detail") {
            assert_eq!(message, "denied: store");
        } else {
            assert_eq!(message, "denied");
        }
        assert_eq!(
            error(put_json_with(&mut host, "drafts/7.json", &draft()).unwrap_err()),
            (-4, "backend: store put: disk full".into())
        );
        assert_eq!(
            error(list_names_with(&mut host, "drafts/", ".json").unwrap_err()).0,
            -3
        );
    }

    #[test]
    fn put_json_new_keeps_the_first_record() {
        let mut host = FakeHost::new();
        put_json_new_with(&mut host, "drafts/7.json", &draft()).unwrap();
        let (code, message) = error(
            put_json_new_with(&mut host, "drafts/7.json", &Draft { amount: "6".into() })
                .unwrap_err(),
        );
        assert_eq!(code, -2);
        if cfg!(feature = "host-error-detail") {
            assert_eq!(message, "denied: store key already exists");
        }
        assert_eq!(host.state("drafts/7.json"), Some(&br#"{"amount":"5"}"#[..]));
    }

    #[test]
    fn a_value_that_cannot_be_encoded_is_a_backend_error() {
        let mut host = FakeHost::new();
        let unencodable = std::collections::BTreeMap::from([((1, 2), 3)]);
        let (code, message) =
            error(put_json_with(&mut host, "a/b.json", &unencodable).unwrap_err());
        assert_eq!(code, -4);
        assert!(message.starts_with("encode record a/b.json: "), "{message}");
        assert!(host.calls().is_empty());
    }

    #[test]
    fn list_names_lists_the_records_directly_under_the_prefix() {
        let mut host = FakeHost::new();
        for key in [
            "drafts/alice/7.json",
            "drafts/alice/12.json",
            "drafts/alice/7/plan.json",
            "drafts/alice/notes.txt",
            "drafts/alice/.json",
            "drafts/alice/...json",
            "drafts/alice2/9.json",
            "drafts/bob/1.json",
        ] {
            host.with_state(key, b"{}");
        }
        assert_eq!(
            list_names_with(&mut host, "drafts/alice/", ".json").unwrap(),
            ["12", "7"]
        );
        // With no suffix a name is the whole last segment.
        assert_eq!(
            list_names_with(&mut host, "drafts/alice/", "").unwrap(),
            ["...json", ".json", "12.json", "7.json", "notes.txt"]
        );
        assert!(
            list_names_with(&mut host, "drafts/carol/", ".json")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn list_names_needs_a_prefix_that_ends_a_segment() {
        let mut host = FakeHost::new();
        host.with_state("drafts/alice/7.json", b"{}")
            .with_state("top.json", b"{}");
        // Without the slash this would also match `drafts/alice2/…`.
        let (code, message) =
            error(list_names_with(&mut host, "drafts/alice", ".json").unwrap_err());
        assert_eq!(code, -4);
        assert!(message.contains("must end in '/'"), "{message}");
        assert!(host.calls().is_empty());
        assert_eq!(list_names_with(&mut host, "", ".json").unwrap(), ["top"]);
    }

    #[test]
    fn list_names_under_creds_lists_secret_records() {
        let mut host = FakeHost::new();
        host.store_put("creds/keys/alice/treasury.txt", b"k", true)
            .unwrap();
        host.with_state("keys/alice/other.txt", b"x");
        assert_eq!(
            list_names_with(&mut host, "creds/keys/alice/", ".txt").unwrap(),
            ["treasury"]
        );
    }
}
