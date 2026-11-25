  (func (;67;) (type 16) (param i32 i64)
    (local i64)
    local.get 0
    local.get 1
    i64.const 1
    i64.shr_u
    local.tee 2
    i64.store
    local.get 0
    local.get 1
    local.get 2
    i64.sub
    i64.store offset=8
  )
