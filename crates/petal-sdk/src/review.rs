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
//! message without one before it reaches Bloom. The SDK checks the envelope
//! against Broker's limits on its size, nesting, types and the values it
//! shows the owner. It does not check that each value fits its type, or the
//! hash, because it has no EIP-712 encoder or Keccak. Test that the typed data
//! hashes to the payload's digest with the Petal's own EIP-712 library.
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
/// Fields in a type with every struct type it uses expanded.
const MAX_EXPANDED_TYPE: usize = 1024;
/// Values Broker shows the owner: one per leaf, and one per empty array.
const MAX_SHOWN_VALUES: usize = 256;
/// Machine's and Broker's limit on the envelope, `SINGLE_PAYLOAD_MAX_BYTES`.
const MAX_ENVELOPE_BYTES: usize = 256 * 1024;
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
    ///
    /// The envelope carries text, so Bloom signs only a UTF-8 message. A dapp
    /// that passes `personal_sign` a `0x`-prefixed hex string means the bytes
    /// it encodes: decode them, and pass the text if it is UTF-8.
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
        let bytes = serde_json::to_vec(&Value::Object(envelope))
            .map_err(|error| format!("encode message review: {error}"))?;
        if bytes.len() > MAX_ENVELOPE_BYTES {
            return Err(format!(
                "the message review is {} bytes, and Bloom takes at most {MAX_ENVELOPE_BYTES}",
                bytes.len()
            ));
        }
        Ok(Self { bytes })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Refuse a message that Bloom would refuse for want of a review envelope.
/// This is Machine's test for any Petal: the keccak suite, a `0x19` first
/// byte, and an `action` whose `schema` is not [`MESSAGE_REVIEW_SCHEMA`].
/// Machine also requires an envelope, whatever the payload, for some
/// first-party classes (Hyperliquid, Polymarket, gasless relay and Venice
/// x402), which this does not check.
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
    for name in types.keys() {
        expanded_size(types, name, &mut Vec::new())?;
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
    } else {
        count_shown_values(types, primary, &root["message"], "message", &mut 0)?;
    }
    Ok(())
}

/// Broker's check of a declared type: its fields, then each struct type they
/// use, expanded. It refuses a recursive type, struct types nested more than
/// `MAX_DEPTH` deep, and an expansion past `MAX_EXPANDED_TYPE` fields.
fn expanded_size(
    types: &Map<String, Value>,
    name: &str,
    stack: &mut Vec<String>,
) -> Result<usize, String> {
    let Some(fields) = types.get(name) else {
        return Ok(1);
    };
    if stack.iter().any(|seen| seen == name) {
        return Err(format!(
            "type {name:?} contains itself, and Broker refuses recursive types"
        ));
    }
    if stack.len() >= MAX_DEPTH {
        return Err(format!(
            "type {name:?} is nested more than {MAX_DEPTH} struct types deep"
        ));
    }
    let fields = fields
        .as_array()
        .ok_or_else(|| format!("type {name:?} must be a list of fields"))?;
    if fields.len() > MAX_TYPE_FIELDS {
        return Err(format!(
            "type {name:?} has more than {MAX_TYPE_FIELDS} fields"
        ));
    }
    stack.push(name.to_owned());
    let mut seen = Vec::with_capacity(fields.len());
    let mut size = 1usize;
    for field in fields {
        let field = field
            .as_object()
            .filter(|field| field.len() == 2)
            .ok_or_else(|| format!("each field of {name:?} needs only a name and a type"))?;
        let (Some(field_name), Some(field_type)) = (
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
        let base = field_type.split('[').next().unwrap_or(field_type);
        size = size.saturating_add(expanded_size(types, base, stack)?);
        if size > MAX_EXPANDED_TYPE {
            return Err(format!(
                "type {name:?} expands to more than {MAX_EXPANDED_TYPE} fields"
            ));
        }
    }
    stack.pop();
    Ok(size)
}

/// Walk the message as Broker does to show it to the owner: one value per
/// leaf and per empty array, in field order, refusing the message once
/// `MAX_SHOWN_VALUES` are shown and another is found. Broker also refuses a
/// struct that does not hold exactly its declared fields, and an array type
/// whose value is not an array. Runs only on acyclic types.
fn count_shown_values(
    types: &Map<String, Value>,
    ty: &str,
    value: &Value,
    path: &str,
    shown: &mut usize,
) -> Result<(), String> {
    if *shown >= MAX_SHOWN_VALUES {
        return Err(format!(
            "the message shows the owner more than {MAX_SHOWN_VALUES} values, which Broker refuses"
        ));
    }
    if let Some(open) = ty.strip_suffix(']').and_then(|ty| ty.rfind('[')) {
        let items = value
            .as_array()
            .ok_or_else(|| format!("{path} has type {ty} but is not an array"))?;
        if items.is_empty() {
            *shown += 1;
        }
        for (index, item) in items.iter().enumerate() {
            count_shown_values(types, &ty[..open], item, &format!("{path}[{index}]"), shown)?;
        }
        return Ok(());
    }
    let Some(fields) = types.get(ty).and_then(Value::as_array) else {
        *shown += 1;
        return Ok(());
    };
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path} has type {ty} but is not an object"))?;
    let names: Vec<&str> = fields
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    if object.len() != names.len() || !names.iter().all(|name| object.contains_key(*name)) {
        return Err(format!(
            "{path} must hold exactly the fields {ty} declares, {names:?}, because Broker shows every signed field and nothing else"
        ));
    }
    for field in fields {
        let (Some(name), Some(field_type)) = (field["name"].as_str(), field["type"].as_str())
        else {
            continue;
        };
        let child = format!("{path}.{name}");
        count_shown_values(types, field_type, &object[name], &child, shown)?;
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
            ("exactly the fields Mail declares", |v: &mut Value| {
                v["message"]["memo"] = json!("not signed")
            }),
            ("exactly the fields Mail declares", |v: &mut Value| {
                v["message"].as_object_mut().unwrap().remove("amount");
            }),
            (
                "message.to has type Person but is not an object",
                |v: &mut Value| {
                    v["types"]["Mail"][0]["type"] = json!("Person");
                    v["types"]["Person"] = json!([{"name": "wallet", "type": "address"}]);
                },
            ),
            (
                "message.amount has type uint256[] but is not an array",
                |v: &mut Value| v["types"]["Mail"][1]["type"] = json!("uint256[]"),
            ),
            ("recursive", |v: &mut Value| {
                v["types"]["Mail"][0]["type"] = json!("Mail")
            }),
            ("recursive", |v: &mut Value| {
                v["types"]["Mail"][0]["type"] = json!("Mail[]")
            }),
            ("recursive", |v: &mut Value| {
                v["types"]["Mail"][0]["type"] = json!("Person");
                v["types"]["Person"] = json!([{"name": "inbox", "type": "Mail[2]"}]);
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
    fn eip712_takes_struct_types_nested_as_deep_as_broker_does() {
        let chain = |levels: usize| {
            let mut typed = mail();
            for level in 0..levels {
                let next = if level + 1 == levels {
                    "uint256".to_owned()
                } else {
                    format!("T{}", level + 1)
                };
                typed["types"][format!("T{level}")] = json!([{"name": "next", "type": next}]);
            }
            typed
        };
        MessageReview::eip712(chain(MAX_DEPTH)).unwrap();
        let error = MessageReview::eip712(chain(MAX_DEPTH + 1)).unwrap_err();
        assert!(error.contains("nested more than 12"), "{error}");
    }

    #[test]
    fn eip712_refuses_a_type_that_expands_past_broker_bound() {
        // `Wider` holds 64 `Wide` structs, so it expands to 1 + 64 * (1 + width).
        let wide = |width: usize| {
            let mut typed = mail();
            typed["types"]["Wide"] = (0..width)
                .map(|i| json!({"name": format!("f{i}"), "type": "uint256"}))
                .collect();
            typed["types"]["Wider"] = (0..MAX_TYPE_FIELDS)
                .map(|i| json!({"name": format!("w{i}"), "type": "Wide"}))
                .collect();
            typed
        };
        MessageReview::eip712(wide(14)).unwrap();
        let error = MessageReview::eip712(wide(15)).unwrap_err();
        assert!(error.contains("expands to more than 1024"), "{error}");
    }

    #[test]
    fn eip712_refuses_more_values_than_broker_shows() {
        let bag = |amounts: usize| {
            let mut typed = mail();
            typed["types"]["Mail"][1] = json!({"name": "amounts", "type": "uint256[]"});
            typed["message"]["amounts"] = json!(vec!["1"; amounts]);
            typed["message"].as_object_mut().unwrap().remove("amount");
            typed
        };
        // `to` and 255 amounts are 256 values, the most Broker shows.
        MessageReview::eip712(bag(255)).unwrap();
        let error = MessageReview::eip712(bag(256)).unwrap_err();
        assert!(error.contains("more than 256 values"), "{error}");

        // Broker shows an empty array as one value.
        let typed = bag(0);
        let mut shown = 0;
        count_shown_values(
            typed["types"].as_object().unwrap(),
            "Mail",
            &typed["message"],
            "message",
            &mut shown,
        )
        .unwrap();
        assert_eq!(shown, 2);
    }

    #[test]
    fn eip712_refuses_an_envelope_larger_than_bloom_takes() {
        let notes = |count: usize| {
            let mut typed = mail();
            typed["types"]["Mail"][1] = json!({"name": "notes", "type": "string[]"});
            typed["message"]["notes"] = json!(vec!["x".repeat(MAX_STRING_BYTES); count]);
            typed["message"].as_object_mut().unwrap().remove("amount");
            typed
        };
        let review = MessageReview::eip712(notes(31)).unwrap();
        assert!(review.as_bytes().len() <= MAX_ENVELOPE_BYTES);
        let error = MessageReview::eip712(notes(33)).unwrap_err();
        assert!(error.contains("Bloom takes at most 262144"), "{error}");
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
