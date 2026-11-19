  (func (;67;) (type 16) (param i32 i64)
    local.get 0
    local.get 1
    i64.const 1
    i64.shr_u
    local.tee 1
    i64.store offset=8
    local.get 0
    local.get 1
    i64.store
  )
