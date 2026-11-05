petal::route_file!(
    spec: petal::store_read_spec().caps(&["bloom:store", "bloom:sign.status"]),
    read: |ctx: &petal::Ctx| petal::signing::approval_read(ctx, "fixture", &["state/fixture/attempt"])
);
