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

#[offload]
pub fn inspect_references(point: &Point, label: &str, values: &[i32]) -> i32 {
    point.x + point.y + label.len() as i32 + values.iter().sum::<i32>()
}

#[offload]
pub fn mutate_references(point: &mut Point, label: &mut str, values: &mut [i32]) -> i32 {
    point.x += 10;
    point.y -= 5;
    label.make_ascii_uppercase();
    for value in values.iter_mut() {
        *value *= 2;
    }
    point.x + point.y + label.len() as i32 + values.iter().sum::<i32>()
}

#[offload(try)]
pub fn try_mutate_reference(point: &mut Point) -> i32 {
    point.x += 1;
    point.y += 2;
    point.x + point.y
}

#[offload(try)]
pub fn mutate_then_panic(point: &mut Point) {
    point.x = i32::MAX;
    panic!("mutation failed")
}
