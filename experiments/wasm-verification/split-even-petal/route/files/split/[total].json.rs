petal::route_file!(
    spec: petal::static_read_spec(),
    read: |ctx: &petal::Ctx| {
        let total = match petal::param(ctx, "total") {
            Ok(total) => total,
            Err(response) => return response,
        };
        // Canonical decimal only: digits, no sign, no leading zero.
        let canonical = !total.is_empty()
            && total.bytes().all(|b| b.is_ascii_digit())
            && (total == "0" || !total.starts_with('0'));
        let total: u64 = match total.parse() {
            Ok(total) if canonical => total,
            _ => return petal::error(-3, "total must be a canonical decimal u64"),
        };
        let (first, second) = split_even_route::split_even(total);
        petal::read_json_value(&serde_json::json!({
            "total": total.to_string(),
            "first": first.to_string(),
            "second": second.to_string(),
        }))
    }
);
