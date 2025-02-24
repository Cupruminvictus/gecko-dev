// |jit-test| skip-if: !wasmGcEnabled()
//
// ref.eq is part of the gc feature, not the reftypes feature.

let { exports } = wasmEvalText(`(module
    (func (export "ref_eq") (param $a externref) (param $b externref) (result i32)
        (ref.eq (local.get $a) (local.get $b)))

    (func (export "ref_eq_for_control") (param $a externref) (param $b externref) (result f64)
        (if (result f64) (ref.eq (local.get $a) (local.get $b))
            (f64.const 5.0)
            (f64.const 3.0))))`);

assertEq(exports.ref_eq(null, null), 1);
assertEq(exports.ref_eq(null, {}), 0);
assertEq(exports.ref_eq(this, this), 1);
assertEq(exports.ref_eq_for_control(null, null), 5);
assertEq(exports.ref_eq_for_control(null, {}), 3);
assertEq(exports.ref_eq_for_control(this, this), 5);

