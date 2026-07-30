use offload::{AnCompatible, offload};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, AnCompatible)]
struct Point {
    x: i32,
    y: i32,
}

#[offload]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[offload(try)]
fn checked_increment(value: i32) -> i32 {
    if value == i32::MAX {
        return i32::MIN;
    }
    value + 1
}

#[offload(export = "custom-point", deny_floats)]
fn echo_point(point: Point) -> Point {
    point
}

#[test]
fn generated_host_signatures_are_preserved() {
    let _: fn(i32, i32) -> i32 = add;
    let _: fn(i32) -> Result<i32, offload::OffloadError> = checked_increment;
    let _: fn(Point) -> Point = echo_point;
}
