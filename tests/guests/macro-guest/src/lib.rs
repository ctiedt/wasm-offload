#![cfg(target_arch = "wasm32")]

use offload::{AnCompatible, offload};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, AnCompatible)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[offload]
pub fn add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

#[offload(export = "reverse-values")]
pub fn reverse(mut values: Vec<u32>) -> Vec<u32> {
    values.reverse();
    values
}

#[offload(try)]
pub fn checked_increment(value: i32) -> i32 {
    if value == i32::MAX {
        return i32::MIN;
    }
    value + 1
}

#[offload(deny_floats)]
pub fn translate(point: Point, dx: i32, dy: i32) -> Point {
    Point {
        x: point.x + dx,
        y: point.y + dy,
    }
}

#[offload]
fn inner(value: i32) -> i32 {
    value * 2
}

#[offload]
pub fn nested(value: i32) -> i32 {
    inner(value) + 1
}

#[offload]
pub fn destructured((left, right): (i32, i32)) -> i32 {
    left - right
}
