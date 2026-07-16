petal::route_file!(
    spec: petal::static_read_spec(),
    read: |_ctx: &petal::Ctx| petal::DispatchResponse::Read(b"canonical petal fixture\n".to_vec())
);
