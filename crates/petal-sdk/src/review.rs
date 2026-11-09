//! The review envelope Bloom needs before it signs a message.
//!
//! A payload whose first byte is `0x19`, signed with
//! [`CryptoSuite::Secp256k1Keccak256Recoverable`](crate::CryptoSuite), is an
//! EIP-712 typed-data or EIP-191 personal message. Bloom signs one only with a
//! `bloom.message.review.v1` envelope in the sign request's `action`. Broker
//! rebuilds the preimage from the envelope, refuses the request unless that
//! preimage's SHA-256 and Keccak hashes match the payload, and shows the
//! owner every typed field. A Petal's own record in `action` is refused.
//!
//! [`MessageReview`] builds the envelope, and `sdk::sign_payload` refuses a
//! message without one before it reaches Bloom. The SDK checks the envelope's
//! shape against Broker's limits; it cannot check the hash, because it has no
//! Keccak. Test that the typed data hashes to the payload's digest with the
//! Petal's own EIP-712 library.
//!
//! Bloom signs messages on the exact single-payload path only: it refuses one
//! in a batch, except for Polymarket's own operation classes. Safe
//! confirmations carry Safe's own envelope instead.

use serde_json::{Map, Value};

/// The `schema` of every message review envelope.
pub const MESSAGE_REVIEW_SCHEMA: &str = "bloom.message.review.v1";

/// Bloom requires Safe's own envelope for this class, not a message review.
const SAFE_CONFIRM_OPERATION_CLASS: &str = "safe.transaction.confirm";

const KECCAK_SUITE: &str = "secp256k1-keccak256-recoverable";

// Broker's limits on the typed data, from its message review reader.
const MAX_DEPTH: usize = 12;
const MAX_NODES: usize = 4096;
const MAX_ARRAY_ITEMS: usize = 256;
const MAX_STRING_BYTES: usize = 8192;
const MAX_KEY_BYTES: usize = 128;
const MAX_TYPES: usize = 32;
const MAX_TYPE_FIELDS: usize = 64;
const DOMAIN_FIELDS: &[&str] = &["name", "version", "chainId", "verifyingContract", "salt"];

/// A `bloom.message.review.v1` envelope, ready to be a sign request's
/// `action`:
///
/// ```ignore
/// let mut request = claim.sign_request(account.wallet(), petal::SignSelector::Exact)?;
/// let review = petal::MessageReview::eip712(typed_data).map_err(|e| petal::error(-4, e))?;
/// request.action = Some(review.into_bytes());
/// ```
///
/// Build it from the same immutable fields on every retry. Bloom binds the
/// `action` bytes into the signing operation, and refuses a retry whose
/// `action` changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageReview {
    bytes: Vec<u8>,
}

impl MessageReview {
    /// Typed data as `eth_signTypedData_v4` takes it: an object with exactly
    /// `types`, `primaryType`, `domain` and `message`. `types` may include
    /// `EIP712Domain`.
    ///
    /// Write a `uint256` amount as a decimal string. A JSON number holds at
    /// most a `u64` exactly; anything wider becomes a float, which loses
    /// precision and is refused here, like any fractional number.
    pub fn eip712(typed_data: Value) -> Result<Self, String> {
        check_typed_data(&typed_data)?;
        Self::encode("eip712", "typed_data", typed_data)
    }

    /// An EIP-191 personal message: the exact text after
    /// `"\x19Ethereum Signed Message:\n<length>"`, at most 8192 bytes.
    pub fn personal(message: &str) -> Result<Self, String> {
        if message.len() > MAX_STRING_BYTES {
            return Err(format!(
                "a personal message review holds at most {MAX_STRING_BYTES} bytes"
            ));
        }
        Self::encode("personal", "message", Value::String(message.to_owned()))
    }

    fn encode(kind: &str, field: &str, value: Value) -> Result<Self, String> {
        let mut envelope = Map::new();
        envelope.insert("schema".into(), MESSAGE_REVIEW_SCHEMA.into());
        envelope.insert("kind".into(), kind.into());
        envelope.insert(field.into(), value);
        serde_json::to_vec(&Value::Object(envelope))
            .map(|bytes| Self { bytes })
            .map_err(|error| format!("encode message review: {error}"))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Refuse a message that Bloom would refuse for want of a review envelope.
/// This is Machine's own test: the keccak suite, a `0x19` first byte, and an
/// `action` whose `schema` is not [`MESSAGE_REVIEW_SCHEMA`].
pub(crate) fn require_message_review(
    signature_algorithm: &str,
    operation_class: &str,
    preimage: &[u8],
    action: Option<&[u8]>,
) -> Result<(), String> {
    let message = signature_algorithm == KECCAK_SUITE && preimage.first() == Some(&0x19);
    if !message || operation_class == SAFE_CONFIRM_OPERATION_CLASS {
        return Ok(());
    }
    let reviewed = action
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .is_some_and(|value| value["schema"] == MESSAGE_REVIEW_SCHEMA);
    if reviewed {
        Ok(())
    } else {
        Err(
            "this payload is an EIP-712 or EIP-191 message, and Bloom signs one only with a \
             bloom.message.review.v1 envelope as the request's action; build it with \
             petal::MessageReview"
                .into(),
        )
    }
}

fn check_typed_data(typed_data: &Value) -> Result<(), String> {
    let root = typed_data
        .as_object()
        .ok_or("typed data must be a JSON object")?;
    let mut keys: Vec<&str> = root.keys().map(String::as_str).collect();
    keys.sort_unstable();
    if keys != ["domain", "message", "primaryType", "types"] {
        return Err(format!(
            "typed data must have exactly domain, types, primaryType and message, not {keys:?}"
        ));
    }
    bounded(typed_data, 0, &mut 0)?;

    let domain = root["domain"]
        .as_object()
        .ok_or("typed data domain must be an object")?;
    if let Some(key) = domain
        .keys()
        .find(|key| !DOMAIN_FIELDS.contains(&key.as_str()))
    {
        return Err(format!(
            "typed data domain field {key:?} is not one of {DOMAIN_FIELDS:?}"
        ));
    }

    let types = root["types"]
        .as_object()
        .ok_or("typed data types must be an object")?;
    if types.len() > MAX_TYPES {
        return Err(format!("typed data declares more than {MAX_TYPES} types"));
    }
    for (name, fields) in types {
        let fields = fields
            .as_array()
            .ok_or_else(|| format!("type {name:?} must be a list of fields"))?;
        if fields.len() > MAX_TYPE_FIELDS {
            return Err(format!(
                "type {name:?} has more than {MAX_TYPE_FIELDS} fields"
            ));
        }
        let mut seen = Vec::with_capacity(fields.len());
        for field in fields {
            let field = field
                .as_object()
                .filter(|field| field.len() == 2)
                .ok_or_else(|| format!("each field of {name:?} needs only a name and a type"))?;
            let (Some(field_name), Some(_)) = (
                field.get("name").and_then(Value::as_str),
                field.get("type").and_then(Value::as_str),
            ) else {
                return Err(format!(
                    "each field of {name:?} needs a string name and type"
                ));
            };
            let valid = !field_name.is_empty()
                && field_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            if !valid || seen.contains(&field_name) {
                return Err(format!(
                    "field name {field_name:?} in {name:?} is invalid or repeated"
                ));
            }
            seen.push(field_name);
        }
    }

    let primary = root["primaryType"]
        .as_str()
        .ok_or("typed data primaryType must be a string")?;
    let message = root["message"]
        .as_object()
        .ok_or("typed data message must be an object")?;
    if primary == "EIP712Domain" {
        if !message.is_empty() {
            return Err("signing the domain alone takes an empty message".into());
        }
    } else if !types.contains_key(primary) {
        return Err(format!("primaryType {primary:?} is not declared in types"));
    }
    Ok(())
}

/// Broker's structural bounds, plus a refusal of fractional numbers, which no
/// EIP-712 type holds.
fn bounded(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err(format!(
            "typed data is nested deeper than {MAX_DEPTH} levels or has more than {MAX_NODES} values"
        ));
    }
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let valid = !key.is_empty()
                    && key.len() <= MAX_KEY_BYTES
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':'));
                if !valid {
                    return Err(format!(
                        "typed data key {key:?} must be 1-{MAX_KEY_BYTES} ASCII letters, digits, '_' or ':'"
                    ));
                }
                bounded(value, depth + 1, nodes)?;
            }
        }
        Value::Array(values) => {
            if values.len() > MAX_ARRAY_ITEMS {
                return Err(format!(
                    "typed data array holds more than {MAX_ARRAY_ITEMS} items"
                ));
            }
            for value in values {
                bounded(value, depth + 1, nodes)?;
            }
        }
        Value::String(text) if text.len() > MAX_STRING_BYTES => {
            return Err(format!(
                "typed data string is longer than {MAX_STRING_BYTES} bytes"
            ));
        }
        Value::Number(number) if number.is_f64() => {
            return Err(format!(
                "typed data number {number} is not an integer; write wide integers as decimal strings"
            ));
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    type Mutation = fn(&mut Value);

    fn mail() -> Value {
        json!({
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "version", "type": "string"},
                    {"name": "chainId", "type": "uint256"},
                    {"name": "verifyingContract", "type": "address"}
                ],
                "Mail": [
                    {"name": "to", "type": "address"},
                    {"name": "amount", "type": "uint256"}
                ]
            },
            "primaryType": "Mail",
            "domain": {
                "name": "Ether Mail",
                "version": "1",
                "chainId": 1,
                "verifyingContract": "0xcccccccccccccccccccccccccccccccccccccccc"
            },
            "message": {
                "to": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "amount": "115792089237316195423570985008687907853269984665640564039457584007913129639935"
            }
        })
    }

    #[test]
    fn eip712_wraps_the_typed_data_in_the_envelope_broker_reads() {
        let review = MessageReview::eip712(mail()).unwrap();
        let envelope: Value = serde_json::from_slice(review.as_bytes()).unwrap();
        assert_eq!(
            envelope,
            json!({"schema": MESSAGE_REVIEW_SCHEMA, "kind": "eip712", "typed_data": mail()})
        );
        assert_eq!(MessageReview::eip712(mail()).unwrap(), review);
    }

    #[test]
    fn personal_wraps_the_exact_text() {
        let review = MessageReview::personal("Sign in to example.com\nNonce: 7").unwrap();
        let envelope: Value = serde_json::from_slice(&review.into_bytes()).unwrap();
        assert_eq!(
            envelope,
            json!({
                "schema": MESSAGE_REVIEW_SCHEMA,
                "kind": "personal",
                "message": "Sign in to example.com\nNonce: 7"
            })
        );
        assert!(MessageReview::personal(&"x".repeat(MAX_STRING_BYTES)).is_ok());
        assert!(MessageReview::personal(&"x".repeat(MAX_STRING_BYTES + 1)).is_err());
    }

    #[test]
    fn eip712_refuses_typed_data_broker_would_refuse() {
        let cases: Vec<(&str, Mutation)> = vec![
            ("exactly domain", |v: &mut Value| v["extra"] = json!(1)),
            ("exactly domain", |v: &mut Value| {
                v.as_object_mut().unwrap().remove("message");
            }),
            ("domain field", |v: &mut Value| {
                v["domain"]["chain"] = json!(1)
            }),
            ("not declared", |v: &mut Value| {
                v["primaryType"] = json!("Letter")
            }),
            ("only a name and a type", |v: &mut Value| {
                v["types"]["Mail"][0]["indexed"] = json!(true)
            }),
            ("invalid or repeated", |v: &mut Value| {
                v["types"]["Mail"][1]["name"] = json!("to")
            }),
            ("not an integer", |v: &mut Value| {
                v["message"]["amount"] = json!(1.5)
            }),
            ("typed data key", |v: &mut Value| {
                v["message"]["a-b"] = json!(1)
            }),
            ("longer than", |v: &mut Value| {
                v["message"]["to"] = json!("x".repeat(MAX_STRING_BYTES + 1))
            }),
            ("more than 256 items", |v: &mut Value| {
                v["message"]["to"] = json!(vec![0; MAX_ARRAY_ITEMS + 1])
            }),
            ("an empty message", |v: &mut Value| {
                v["primaryType"] = json!("EIP712Domain")
            }),
        ];
        for (expected, mutate) in cases {
            let mut typed = mail();
            mutate(&mut typed);
            let error = MessageReview::eip712(typed).unwrap_err();
            assert!(error.contains(expected), "{expected:?} not in {error:?}");
        }
        assert!(MessageReview::eip712(json!([])).is_err());
    }

    #[test]
    fn eip712_admits_domain_only_signing_and_a_domain_without_a_chain() {
        let mut typed = mail();
        typed["primaryType"] = json!("EIP712Domain");
        typed["message"] = json!({});
        MessageReview::eip712(typed).unwrap();
        let mut typed = mail();
        typed["domain"].as_object_mut().unwrap().remove("chainId");
        MessageReview::eip712(typed).unwrap();
    }

    #[test]
    fn eip712_refuses_nesting_past_broker_depth() {
        let mut nested = json!(1);
        for _ in 0..MAX_DEPTH {
            nested = json!({ "a": nested });
        }
        let mut typed = mail();
        typed["message"]["to"] = nested;
        let error = MessageReview::eip712(typed).unwrap_err();
        assert!(error.contains("nested deeper"), "{error}");
    }

    #[test]
    fn messages_need_a_review_and_everything_else_passes() {
        let review = MessageReview::eip712(mail()).unwrap().into_bytes();
        let typed = [0x19, 0x01, 0xaa];
        let personal = b"\x19Ethereum Signed Message:\n2hi";
        for preimage in [&typed[..], &personal[..]] {
            for action in [
                None,
                Some(&br#"{"schema":"bloom.cowswap.prepared_order.v1"}"#[..]),
            ] {
                let error =
                    require_message_review(KECCAK_SUITE, "cowswap.order.v1", preimage, action)
                        .unwrap_err();
                assert!(error.contains("petal::MessageReview"), "{error}");
            }
            require_message_review(
                KECCAK_SUITE,
                "cowswap.order.v1",
                preimage,
                Some(&review[..]),
            )
            .unwrap();
            // Safe's class carries Safe's own envelope.
            require_message_review(KECCAK_SUITE, SAFE_CONFIRM_OPERATION_CLASS, preimage, None)
                .unwrap();
            // Machine reads a 0x19 first byte as a message only for this suite.
            require_message_review("secp256k1-sha256-recoverable", "x.y", preimage, None).unwrap();
            require_message_review("ed25519-message", "x.y", preimage, None).unwrap();
        }
        // An EVM transaction or any other payload needs no message review.
        require_message_review(KECCAK_SUITE, "x.y", &[0x02, 0xf8], None).unwrap();
        require_message_review(KECCAK_SUITE, "x.y", &[], None).unwrap();
    }
}
