  (func (;162;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    call 1
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 2
    local.get 1
    i64.le_u
    local.get 3
    local.get 1
    local.get 2
    i64.sub
    i64.eq
    i32.and
    call 4
  )
  (func (;163;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    call 1
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 2
    local.get 3
    i64.le_u
    call 4
  )
  (func (;164;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    call 1
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 2
    local.get 3
    i64.le_u
    local.get 3
    local.get 2
    i64.sub
    i64.const 1
    i64.le_u
    i32.and
    call 4
  )
  (func (;165;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    call 1
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    i32.const 1
    call 5
  )
  (func (;166;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    call 1
    local.set 1
    call 2
    local.set 4
    local.get 4
    local.get 0
    i32.lt_u
    local.get 4
    i64.extend_i32_u
    local.get 0
    i64.extend_i32_u
    i64.const 16
    i64.add
    i64.ge_u
    i32.or
    call 3
    local.get 4
    i32.load8_u
    local.set 5
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 4
    i32.load8_u
    local.get 5
    i32.eq
    call 4
  )
  (func (;167;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    i64.const 0
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 2
    i64.const 0
    i64.eq
    local.get 3
    i64.const 0
    i64.eq
    i32.and
    call 4
  )
  (func (;168;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    i64.const 7
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 2
    i64.const 3
    i64.eq
    local.get 3
    i64.const 4
    i64.eq
    i32.and
    call 4
  )
  (func (;169;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    call 2
    local.set 0
    local.get 0
    i32.const -16
    i32.le_u
    call 3
    i64.const -1
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    local.get 2
    i64.const 9223372036854775807
    i64.eq
    local.get 3
    i64.const -9223372036854775808
    i64.eq
    i32.and
    call 4
  )
  (func (;170;) (type 26)
    (local i32 i64 i64 i64 i32 i32)
    i32.const -16
    local.set 0
    call 1
    local.set 1
    local.get 0
    local.get 1
    call 72
    local.get 0
    i64.load
    local.set 2
    local.get 0
    i64.load offset=8
    local.set 3
    i32.const 1
    call 5
  )
